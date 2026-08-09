//! 工具执行域(R-155 B6):wave 构建与并行执行(PreparedToolCall/
//! build_tool_execution_waves_with/execute_prepared_tools)、权限拒绝占位
//! (append_declined_tool_results)、Gate 门禁与并发上限常量。
//! PreparedToolCall 六字段提 pub(super)(测试按字面量构造)。

use futures::StreamExt;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx};
use kanzei_llm::Part;
use std::sync::Arc;

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
        if !current.is_empty()
            && (conflicts || current.len() >= max_parallel)
        {
            waves.push(std::mem::take(&mut current));
        }
        current.push(call);
    }
    if !current.is_empty() {
        waves.push(current);
    }
    waves
}

pub(crate) async fn execute_prepared_tools(
    calls: Vec<PreparedToolCall>,
    ctx: &ToolCtx,
    max_parallel: usize,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> Vec<(usize, Part)> {
    let mut results = Vec::new();
    for wave in build_tool_execution_waves_with(max_parallel, calls) {
        let mut jobs: futures::stream::FuturesUnordered<_> = wave
            .into_iter()
            .map(|call| async move {
                let PreparedToolCall {
                    index,
                    id,
                    name,
                    input,
                    tool,
                    concurrency: _,
                } = call;
                let output = tool.execute(input, ctx).await;
                (index, id, name, output)
            })
            .collect();
        while let Some((index, id, name, output)) = jobs.next().await {
            on_event(RunEvent::ToolEnd {
                id: id.clone(),
                name,
                ok: !output.is_error,
                preview: preview(&output.content),
                display: output.display.clone(),
            });
            results.push((
                index,
                Part::ToolResult {
                    call_id: id,
                    content: output.content,
                    is_error: output.is_error,
                },
            ));
        }
    }
    results.sort_by_key(|(index, _)| *index);
    results
}

pub(crate) enum Gate {
    Pass,
    Deny(String),
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
            concurrency: tool.concurrency(&input, &ToolCtx::new(std::env::temp_dir())),
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
        let ctx = ToolCtx::new(std::env::temp_dir());
        let mut completed = Vec::new();
        let mut on_event = |event| {
            if let RunEvent::ToolEnd { id, .. } = event {
                completed.push(id);
            }
        };
        let results = execute_prepared_tools(calls, &ctx, super::MAX_PARALLEL_TOOLS_PER_WAVE, &mut on_event).await;

        assert!(max_in_flight.load(Ordering::SeqCst) >= 2, "只读调用没有重叠执行");
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
            probe_call(0, "write_1", serde_json::json!({"delay_ms": 15}), writer.clone()),
            probe_call(1, "read_1", serde_json::json!({"delay_ms": 15}), reader),
            probe_call(2, "write_2", serde_json::json!({"delay_ms": 15}), writer),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir());
        let mut on_event = |_event| {};
        let results = execute_prepared_tools(calls, &ctx, super::MAX_PARALLEL_TOOLS_PER_WAVE, &mut on_event).await;

        assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
        assert_eq!(
            results
                .iter()
                .map(|(_, part)| match part {
                    Part::ToolResult { call_id, .. } => call_id.as_str(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>(),
            vec!["write_1", "read_1", "write_2"]
        );
    }

    #[test]
    fn declined_tool_batch_keeps_real_and_placeholder_results_paired() {
        let calls = vec![
            ("call_done".into(), "write".into(), serde_json::json!({}), "{}".into()),
            ("call_declined".into(), "edit".into(), serde_json::json!({}), "{}".into()),
            ("call_pending".into(), "bash".into(), serde_json::json!({}), "{}".into()),
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
}

