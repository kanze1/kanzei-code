//! 工具执行域(R-155 B6):wave 构建与并行执行(PreparedToolCall/
//! build_tool_execution_waves_with/execute_prepared_tools)、权限拒绝占位
//! (append_declined_tool_results)、Gate 门禁与并发上限常量。
//! PreparedToolCall 六字段提 pub(super)(测试按字面量构造)。

use futures::StreamExt;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx};
use kanzei_llm::Part;
use sha2::Digest;
use std::sync::Arc;

use super::tool_failure_telemetry::record_tool_failure;
use crate::runner::event::{preview, RunEvent};

/// 同一无冲突 wave 的普通工具并发上限；超过时按原调用顺序切 wave。
/// 测试锚点:生产调用方(execute_prepared_tools 参数)由 drive 层传值。
#[allow(dead_code)]
pub const MAX_PARALLEL_TOOLS_PER_WAVE: usize = 8;
pub(crate) fn append_declined_tool_results(
    results: &mut Vec<Part>,
    calls: &[(String, String, serde_json::Value, String)],
    declined_index: usize,
) {
    for (index, (id, _, _, _)) in calls.iter().enumerate().skip(declined_index) {
        let content = if index == declined_index {
            "permission request declined by user"
        } else {
            "tool call cancelled because a previous permission request was declined"
        };
        results.push(Part::ToolResult {
            call_id: id.clone(),
            content: content.into(),
            is_error: true,
        });
    }
}

pub(crate) struct PreparedToolCall {
    pub(super) index: usize,
    pub(super) id: String,
    pub(super) name: String,
    pub(super) input: serde_json::Value,
    pub(super) tool: Arc<dyn Tool>,
    pub(super) concurrency: ToolConcurrency,
}

pub(crate) fn build_tool_execution_waves_with(
    max_parallel: usize,
    calls: Vec<PreparedToolCall>,
) -> Vec<Vec<PreparedToolCall>> {
    let mut waves = Vec::new();
    let mut current: Vec<PreparedToolCall> = Vec::new();
    for call in calls {
        let conflicts = current
            .iter()
            .any(|other| call.concurrency.conflicts_with(&other.concurrency));
        if !current.is_empty() && (conflicts || current.len() >= max_parallel) {
            waves.push(std::mem::take(&mut current));
        }
        current.push(call);
    }
    if !current.is_empty() {
        waves.push(current);
    }
    waves
}

/// R-249:把 ToolOutput 的图片转成 llm Part;provider 不支持时降级为文本说明。
///
/// 返回 `(图片 Part, 需要追加到工具结果文本的说明)`。
///
/// 并行与串行两条执行路径都要做这层转换,必须共用一份——media_type 口径分叉的话,
/// 同一个工具在两条路径下会给 provider 发出不同的请求体,而这种差异只在其中一条
/// 路径上复现,极难定位。
///
/// **为什么降级要发生在这里,而不是靠 client.rs 那道硬拒绝**:那条会让整个请求
/// 失败,而图片一旦进了 messages 就跟着历史每轮重发——等于一次 read 图片就把这条
/// 对话在该 provider 上永久打死。所以图片必须在进历史**之前**被拦下,并如实告诉
/// 模型它没拿到图,好让它改走别的手段。静默丢弃是最坏的一种:模型会以为自己看过了。
pub(crate) fn tool_images_to_parts(
    output: &kanzei_harness::ToolOutput,
    images_supported: bool,
) -> (Vec<Part>, Option<String>) {
    if output.images.is_empty() {
        return (Vec::new(), None);
    }
    if !images_supported {
        return (
            Vec::new(),
            Some(format!(
                "\n[image not delivered: the active provider does not accept image input; \
                 {} image(s) were dropped. You did NOT see them — do not describe their contents.]",
                output.images.len()
            )),
        );
    }
    let parts = output
        .images
        .iter()
        .map(|image| Part::Image {
            media_type: image.media_type.clone(),
            data: image.data.clone(),
        })
        .collect();
    (parts, None)
}

const TOOL_RESULT_SPILL_THRESHOLD: usize = 1024 * 1024;
const TOOL_RESULT_SHADOW_THRESHOLD: usize = 32 * 1024;

fn record_tool_result_shadow_telemetry(
    ctx: &ToolCtx,
    tool_name: &str,
    bytes: usize,
    sha256: Option<&str>,
    actual_spilled: bool,
) {
    let safe_tool_name: String = tool_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    let run_id = ctx
        .run_id
        .as_deref()
        .unwrap_or("unknown-run")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let dir = ctx
        .project_root
        .join(".kanzei/artifacts/tool-results/shadow");
    let path = dir.join(format!(
        "{safe_tool_name}-{run_id}-{}-{stamp}.json",
        std::process::id()
    ));
    let payload = serde_json::json!({
        "kind": "tool_result_shadow",
        "tool_name": tool_name,
        "bytes": bytes,
        "shadow_threshold": TOOL_RESULT_SHADOW_THRESHOLD,
        "would_spill": bytes > TOOL_RESULT_SHADOW_THRESHOLD,
        "actual_spilled": actual_spilled,
        "sha256": sha256,
    });
    if let Ok(encoded) = serde_json::to_string(&payload) {
        let _ = kanzei_base::atomic_file::write_atomic(&path, &encoded);
    }
}

/// 把超限工具结果先写入 durable artifact，再把紧凑引用回喂给模型/UI。
///
/// 该函数位于所有工具的统一消费出口：工具权限、错误码和小结果路径不变；只有
/// 大结果在事件提交前被替换为 artifact 引用。写失败时不生成 artifact 引用，并把
/// 结果转成明确失败，避免事件看起来像一次成功的外置结果。
pub(crate) fn materialize_tool_output(
    output: &mut kanzei_harness::ToolOutput,
    ctx: &ToolCtx,
    tool_name: &str,
) {
    if output.content.len() <= TOOL_RESULT_SPILL_THRESHOLD {
        record_tool_result_shadow_telemetry(ctx, tool_name, output.content.len(), None, false);
        return;
    }

    let original = std::mem::take(&mut output.content);
    let mut digest = sha2::Sha256::new();
    sha2::Digest::update(&mut digest, original.as_bytes());
    let sha256 = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let safe_tool_name: String = tool_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    let artifact_id = format!("tool-{safe_tool_name}-{sha256}");
    let relative_path = format!(".kanzei/artifacts/tool-results/{artifact_id}.txt");
    let path = ctx.project_root.join(&relative_path);

    if let Err(error) = kanzei_base::atomic_file::write_atomic(&path, &original) {
        output.content = format!(
            "[tool_result_spill_failed bytes={} sha256={}]: {error}",
            original.len(),
            sha256
        );
        output.is_error = true;
        output.outcome = kanzei_harness::ToolOutcome::Failed;
        output.code = Some("TOOL_RESULT_SPILL_FAILED");
        output.artifact = None;
        record_tool_result_shadow_telemetry(ctx, tool_name, original.len(), Some(&sha256), false);
        return;
    }

    record_tool_result_shadow_telemetry(ctx, tool_name, original.len(), Some(&sha256), true);

    let artifact = kanzei_harness::ToolArtifact {
        artifact_id: artifact_id.clone(),
        relative_path: relative_path.clone(),
        bytes: original.len() as u64,
        sha256: sha256.clone(),
        retrieval_hint: format!("read path={relative_path}"),
    };
    output.content = format!(
        "[tool_result_externalized artifact_id={artifact_id} bytes={} sha256={sha256}]\nPreview: {}\n完整原文已外置；请按 retrieval_hint 回读。",
        original.len(),
        preview(&original),
    );
    output.display = Some(serde_json::json!({
        "kind": "artifact",
        "artifact_id": artifact.artifact_id,
        "bytes": artifact.bytes,
        "sha256": artifact.sha256,
        "retrieval_hint": artifact.retrieval_hint,
    }));
    output.artifact = Some(artifact);
}

/// 返回 (下标, ToolResult, 该结果附带的图片 Part)。
///
/// 图片**不能**混进 results 向量:那里 `results[i] ↔ calls[i]` 是硬约定
/// (note_step 的 debug_assert 锁着)。它们由调用方单独收集,统一追加到
/// tool_results 消息尾部——Anthropic 要求 tool_result 块排在 user 消息最前,
/// 所以只能后缀不能前插。
pub(crate) async fn execute_prepared_tools(
    calls: Vec<PreparedToolCall>,
    ctx: &ToolCtx,
    max_parallel: usize,
    images_supported: bool,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> Vec<(usize, Part, Vec<Part>)> {
    let mut results = Vec::new();
    // 进度旁路:每个调用带自己 id 的 ProgressHandle,共用一条通道;
    // 收集循环里边等完成边转发增量输出,UI 才能在长任务执行中看到"跑到哪了"。
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<kanzei_harness::progress::ProgressChunk>();
    for wave in build_tool_execution_waves_with(max_parallel, calls) {
        let mut jobs: futures::stream::FuturesUnordered<_> = wave
            .into_iter()
            .map(|call| {
                let progress_tx = progress_tx.clone();
                async move {
                    let PreparedToolCall {
                        index,
                        id,
                        name,
                        input,
                        tool,
                        concurrency: _,
                    } = call;
                    // D-174:专用文档工具的执行区间就是它的合法写入窗口,后台守卫
                    // 据此把"专用工具改的"和"后台进程偷改的"分开。非写工具零开销。
                    // R-259:执行包装(wrap_execute:progress 注入)收编——串行/并行
                    // 共用同一 wrapper,工具 body 不再各自实现 progress 注入。
                    let output = kanzei_harness::managed_fence::tool_scope(
                        &name,
                        kanzei_harness::tool_pipeline::wrap_execute(
                            id.clone(),
                            Some(progress_tx.clone()),
                            None,
                            tool.execute(input, ctx),
                        ),
                    )
                    .await;
                    (index, id, name, output)
                }
            })
            .collect();
        loop {
            tokio::select! {
                // 先清进度再收终态:同一调用的增量输出必须排在它的 ToolEnd 之前。
                biased;
                Some((id, chunk)) = progress_rx.recv() => {
                    on_event(RunEvent::ToolProgress { id, chunk });
                }
                job = jobs.next() => {
                    let Some((index, id, name, mut output)) = job else { break };
                    materialize_tool_output(&mut output, ctx, &name);
                    record_tool_failure(ctx, &id, &name, &output);
                    while let Ok((pid, chunk)) = progress_rx.try_recv() {
                        on_event(RunEvent::ToolProgress { id: pid, chunk });
                    }
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name,
                        ok: !output.is_error,
                        outcome: output.outcome.as_str().into(),
                        code: output.code.map(str::to_owned),
                        preview: preview(&output.content),
                        display: output.display.clone(),
                        artifact: output.artifact.clone(),
                    });
                    let mut model_content = output.model_content();
                    let (images, dropped_note) =
                        tool_images_to_parts(&output, images_supported);
                    if let Some(note) = dropped_note {
                        model_content.push_str(&note);
                    }
                    results.push((
                        index,
                        Part::ToolResult {
                            call_id: id,
                            content: model_content,
                            is_error: output.is_error,
                        },
                        images,
                    ));
                }
            }
        }
    }
    results.sort_by_key(|(index, _, _)| *index);
    results
}

pub(crate) enum Gate {
    Pass,
    Deny(String),
    NonInteractive(String),
    UserDeclined,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
    use kanzei_llm::Part;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct ProbeTool {
        name: &'static str,
        concurrency: ToolConcurrency,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
    }

    #[async_trait]

    impl Tool for ProbeTool {
        fn name(&self) -> &'static str {
            self.name
        }

        fn description(&self) -> String {
            "test probe".into()
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        fn concurrency(&self, _input: &serde_json::Value, _ctx: &ToolCtx) -> ToolConcurrency {
            self.concurrency.clone()
        }

        async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> ToolOutput {
            let active = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(
                input["delay_ms"].as_u64().unwrap_or(10),
            ))
            .await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            let label = input["label"].as_str().unwrap_or("probe");
            if input["fail"].as_bool().unwrap_or(false) {
                ToolOutput::error(format!("{label} failed"))
            } else {
                ToolOutput::ok(format!("{label} ok"))
            }
        }
    }

    fn probe_call(
        index: usize,
        id: &str,
        input: serde_json::Value,
        tool: Arc<ProbeTool>,
    ) -> PreparedToolCall {
        PreparedToolCall {
            index,
            id: id.into(),
            name: tool.name().into(),
            concurrency: tool.concurrency(
                &input,
                &ToolCtx::new(std::env::temp_dir(), std::env::temp_dir()),
            ),
            input,
            tool,
        }
    }

    #[tokio::test]
    async fn 普通只读工具真实并发_失败隔离且结果按调用顺序归位() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let tool = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "call_slow",
                serde_json::json!({"label": "slow", "delay_ms": 60}),
                tool.clone(),
            ),
            probe_call(
                1,
                "call_fast_fail",
                serde_json::json!({"label": "fast", "delay_ms": 5, "fail": true}),
                tool,
            ),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir(), std::env::temp_dir());
        let mut completed = Vec::new();
        let mut on_event = |event| {
            if let RunEvent::ToolEnd { id, .. } = event {
                completed.push(id);
            }
        };
        let results = execute_prepared_tools(
            calls,
            &ctx,
            super::MAX_PARALLEL_TOOLS_PER_WAVE,
            true,
            &mut on_event,
        )
        .await;

        assert!(
            max_in_flight.load(Ordering::SeqCst) >= 2,
            "只读调用没有重叠执行"
        );
        assert_eq!(completed, vec!["call_fast_fail", "call_slow"]);
        assert!(matches!(
            &results[0].1,
            Part::ToolResult { call_id, is_error: false, content } if call_id == "call_slow" && content.contains("slow ok")
        ));
        assert!(matches!(
            &results[1].1,
            Part::ToolResult { call_id, is_error: true, content } if call_id == "call_fast_fail" && content.contains("fast failed")
        ));
    }

    #[tokio::test]
    async fn 同一工作树读写与写写冲突严格串行() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let writer = Arc::new(ProbeTool {
            name: "probe_write",
            concurrency: ToolConcurrency::WorktreeWrite("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let reader = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight,
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "write_1",
                serde_json::json!({"delay_ms": 15}),
                writer.clone(),
            ),
            probe_call(1, "read_1", serde_json::json!({"delay_ms": 15}), reader),
            probe_call(2, "write_2", serde_json::json!({"delay_ms": 15}), writer),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir(), std::env::temp_dir());
        let mut on_event = |_event| {};
        let results = execute_prepared_tools(
            calls,
            &ctx,
            super::MAX_PARALLEL_TOOLS_PER_WAVE,
            true,
            &mut on_event,
        )
        .await;

        assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
        assert_eq!(
            results
                .iter()
                .map(|(_, part, _)| match part {
                    Part::ToolResult { call_id, .. } => call_id.as_str(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>(),
            vec!["write_1", "read_1", "write_2"]
        );
    }

    #[test]
    fn shadow_telemetry_records_32k_without_changing_model_input() {
        let root = std::env::temp_dir().join(format!(
            "kz-d245-shadow-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let ctx = ToolCtx::new(root.clone(), root.clone()).with_identity(
            "worktree-shadow".into(),
            "project-shadow".into(),
            "run-shadow".into(),
            "process-shadow".into(),
        );
        let original = "x".repeat(super::TOOL_RESULT_SHADOW_THRESHOLD + 1);
        let mut output = ToolOutput::ok(original.clone());

        super::materialize_tool_output(&mut output, &ctx, "bash");

        assert_eq!(output.content, original);
        assert!(output.artifact.is_none());
        let records = std::fs::read_dir(root.join(".kanzei/artifacts/tool-results/shadow"))
            .unwrap()
            .map(|entry| {
                serde_json::from_slice::<serde_json::Value>(
                    &std::fs::read(entry.unwrap().path()).unwrap(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["tool_name"], "bash");
        assert_eq!(records[0]["bytes"], original.len());
        assert_eq!(records[0]["would_spill"], true);
        assert_eq!(records[0]["actual_spilled"], false);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_tool_output_is_externalized_with_recoverable_bytes() {
        let root = std::env::temp_dir().join(format!(
            "kz-d349-spill-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let original = "x".repeat(super::TOOL_RESULT_SPILL_THRESHOLD + 17);
        let mut output = ToolOutput::ok(original.clone()).with_display(serde_json::json!({
            "kind": "terminal",
            "full": "must not remain in the event",
        }));

        super::materialize_tool_output(&mut output, &ctx, "git");

        let artifact = output
            .artifact
            .as_ref()
            .expect("大结果必须有 artifact 引用");
        assert_eq!(artifact.bytes, original.len() as u64);
        assert_eq!(artifact.sha256.len(), 64);
        assert!(output.content.contains("tool_result_externalized"));
        drop(ctx);
        let restarted_ctx = ToolCtx::new(root.clone(), root.clone());
        assert_eq!(
            std::fs::read(restarted_ctx.project_root.join(&artifact.relative_path)).unwrap(),
            original.as_bytes()
        );
        assert!(artifact.retrieval_hint.contains(&artifact.relative_path));
        assert_eq!(output.display.as_ref().unwrap()["kind"], "artifact");
        assert!(output.display.as_ref().unwrap().get("full").is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn artifact_write_failure_is_visible_without_success_reference() {
        let root = std::env::temp_dir().join(format!(
            "kz-d349-spill-failure-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".kanzei/artifacts")).unwrap();
        std::fs::write(
            root.join(".kanzei/artifacts/tool-results"),
            "not a directory",
        )
        .unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut output = ToolOutput::ok("x".repeat(super::TOOL_RESULT_SPILL_THRESHOLD + 1));

        super::materialize_tool_output(&mut output, &ctx, "bash");

        assert!(output.is_error);
        assert_eq!(output.code, Some("TOOL_RESULT_SPILL_FAILED"));
        assert!(output.artifact.is_none());
        assert!(output.content.contains("tool_result_spill_failed"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn declined_tool_batch_keeps_real_and_placeholder_results_paired() {
        let calls = vec![
            (
                "call_done".into(),
                "write".into(),
                serde_json::json!({}),
                "{}".into(),
            ),
            (
                "call_declined".into(),
                "edit".into(),
                serde_json::json!({}),
                "{}".into(),
            ),
            (
                "call_pending".into(),
                "bash".into(),
                serde_json::json!({}),
                "{}".into(),
            ),
        ];
        let mut results = vec![Part::ToolResult {
            call_id: "call_done".into(),
            content: "真实写入结果".into(),
            is_error: false,
        }];
        append_declined_tool_results(&mut results, &calls, 1);

        assert_eq!(results.len(), 3);
        assert!(matches!(
            &results[0],
            Part::ToolResult { call_id, content, is_error: false }
                if call_id == "call_done" && content == "真实写入结果"
        ));
        assert!(matches!(
            &results[1],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_declined" && content.contains("declined")
        ));
        assert!(matches!(
            &results[2],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_pending" && content.contains("cancelled")
        ));
    }

    /// R-171 批2:writer 阶段 max in-flight=1(wave 上限 1 时逐条执行且不重叠)。
    /// drive 在 ReadParallelWriteSerial 下直接走串行路径不调 wave;此测试锚定
    /// wave 路径若被复用(如防御性回退)同样满足「任意时刻最多一个工具执行」。
    #[tokio::test]
    async fn max_parallel_1_强制串行_结果按下标归位() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let tool = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "call_1",
                serde_json::json!({"label": "first", "delay_ms": 20}),
                tool.clone(),
            ),
            probe_call(
                1,
                "call_2",
                serde_json::json!({"label": "second", "delay_ms": 20}),
                tool.clone(),
            ),
            probe_call(
                2,
                "call_3",
                serde_json::json!({"label": "third", "delay_ms": 20}),
                tool,
            ),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir(), std::env::temp_dir());
        let mut on_event = |_event| {};
        let results = execute_prepared_tools(calls, &ctx, 1, true, &mut on_event).await;

        assert_eq!(
            max_in_flight.load(Ordering::SeqCst),
            1,
            "writer 阶段任意时刻最多一个工具执行"
        );
        assert_eq!(results.len(), 3);
        // 结果按下标与调用顺序对齐。
        for (idx, (call_index, part, images)) in results.iter().enumerate() {
            assert!(images.is_empty(), "纯文本工具不得产生图片 Part");
            assert_eq!(*call_index, idx);
            let expect_id = format!("call_{}", idx + 1);
            assert!(matches!(
                part,
                Part::ToolResult { call_id, is_error: false, .. }
                    if *call_id == expect_id
            ));
        }
    }

    // ---- R-249:图片投递与降级 ----

    fn output_with_images(n: usize) -> ToolOutput {
        ToolOutput::ok("done").with_images(
            (0..n)
                .map(|i| kanzei_harness::ToolImage {
                    media_type: "image/png".into(),
                    data: format!("payload{i}"),
                })
                .collect(),
        )
    }

    #[test]
    fn images_pass_through_when_provider_supports_them() {
        let (parts, note) = tool_images_to_parts(&output_with_images(2), true);
        assert_eq!(parts.len(), 2);
        assert!(note.is_none(), "支持图片时不应产生降级说明");
        assert!(matches!(
            &parts[0],
            Part::Image { media_type, data } if media_type == "image/png" && data == "payload0"
        ));
    }

    #[test]
    fn images_degrade_to_explicit_note_when_unsupported() {
        // 关键不变式:不支持时**一个 Image part 都不能进历史**。进了历史就会跟着
        // 每一轮重发,client.rs 的硬拒绝会让这条对话在该 provider 上永久不可用。
        let (parts, note) = tool_images_to_parts(&output_with_images(3), false);
        assert!(parts.is_empty(), "不支持图片时不得放行任何 Image part");
        let note = note.expect("必须给出显式降级说明,不能静默丢弃");
        assert!(note.contains('3'), "说明里要写清丢了几张: {note}");
        assert!(
            note.contains("did NOT see"),
            "必须明确告诉模型它没看到图,否则它会照着文本编内容: {note}"
        );
    }

    #[test]
    fn no_images_means_no_note_on_either_path() {
        // 回归:纯文本工具的返回在两种能力下都必须逐字节不变。
        for supported in [true, false] {
            let (parts, note) = tool_images_to_parts(&ToolOutput::ok("plain"), supported);
            assert!(parts.is_empty());
            assert!(note.is_none());
        }
    }
}
