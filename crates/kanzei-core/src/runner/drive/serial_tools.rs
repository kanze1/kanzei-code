//! 串行普通工具执行段。
//!
//! 权限门禁、停止检查点、ToolEnd 事件和 calls/results 对齐都保留在这里；
//! `drive::execute_tool_calls` 只负责预检、并行 wave 与结果分派。

use super::super::tool_failure_telemetry::{record_permission_denied, record_tool_failure};
use super::permissions::{resolve_permission_gate, PermissionGateRequest};
use super::question::execute_question;
use super::*;

pub(super) struct SerialToolRequest<'a> {
    pub(super) config: &'a RunnerConfig,
    pub(super) ctx: &'a ToolCtx,
    pub(super) snapshot: &'a HarnessSnapshot,
    pub(super) tools: &'a [Arc<dyn Tool>],
    pub(super) calls: &'a [(String, String, serde_json::Value, String)],
    pub(super) subagent: Option<&'a SubagentRuntime>,
    pub(super) task_results: &'a mut std::collections::HashMap<String, kanzei_harness::ToolOutput>,
    pub(super) images_supported: bool,
    pub(super) halt: Option<&'a CancellationToken>,
    pub(super) on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    pub(super) ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
    pub(super) session_approved: &'a mut std::collections::HashSet<(String, String)>,
    pub(super) session_rules: &'a mut Vec<(String, String)>,
    pub(super) messages: &'a mut Vec<Message>,
    pub(super) step: u32,
    pub(super) pending_images: &'a mut Vec<Part>,
}

pub(super) async fn execute_serial_tool_calls(
    request: SerialToolRequest<'_>,
) -> anyhow::Result<super::ToolRunOutcome> {
    let SerialToolRequest {
        config,
        ctx,
        snapshot,
        tools,
        calls,
        subagent,
        task_results,
        images_supported,
        halt,
        on_event,
        ask,
        session_approved,
        session_rules,
        messages,
        step,
        pending_images,
    } = request;
    let halted = || halt.is_some_and(|token| token.is_cancelled());
    let mut results = Vec::new();
    // 串行路径:按 calls 的原始顺序逐个执行并 push,results 与 calls 下标对齐
    // (R-155 设计要点 3)。calls.len() == results.len() 由 note_step 的 debug_assert 兜底。
    for (call_index, (id, name, input, raw_input)) in calls.iter().cloned().enumerate() {
        // D-342 工具间检查点:上一个工具执行期间收到停止,剩余调用全部
        // 取消占位配对后 halted 收尾——已完成的结果原样保留在历史里。
        if halted() {
            append_halted_tool_results(&mut results, calls, call_index);
            commit_tool_results(
                messages,
                results,
                std::mem::take(pending_images),
                step,
                on_event,
            );
            return Ok(super::ToolRunOutcome::Stopped);
        }
        // task 不过权限门禁:子代理快照在代码层面只含只读工具(硬门禁在构造,不在评估)。
        // ToolEnd 已在并行阶段按完成顺序上报过,这里只归位结果。
        if name == "task" && subagent.is_some() {
            let output = task_results.remove(&id).unwrap_or_else(|| {
                kanzei_harness::ToolOutput::error("internal: task result missing")
            });
            results.push(tool_result_part(id, output));
            continue;
        }
        let Some(tool) = tools.iter().find(|t| t.name() == name) else {
            results.push(Part::ToolResult {
                call_id: id,
                content: format!(
                    "unknown tool `{name}`; available: {}",
                    tools
                        .iter()
                        .map(|t| t.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                is_error: true,
            });
            continue;
        };
        on_event(RunEvent::ToolStart {
            id: id.clone(),
            name: name.clone(),
            summary: summarize_input(&input, &raw_input),
            input: input.clone(),
        });

        // question 是交互工具，不再叠加权限询问；答案作为工具结果回喂模型。
        if name == "question" {
            let output = execute_question(config, &input, ask).await;
            on_event(RunEvent::ToolEnd {
                id: id.clone(),
                name: name.clone(),
                ok: !output.is_error,
                outcome: output.outcome.as_str().into(),
                code: output.code.map(str::to_owned),
                preview: preview(&output.content),
                display: output.display.clone(),
                artifact: output.artifact.clone(),
            });
            results.push(tool_result_part(id, output));
            continue;
        }

        // ---- 硬门禁:权限 Ruleset(deny 回喂模型;ask 问用户,拒绝停整轮)----
        let action = tool.action();
        let gate_result = {
            let mut permission_request = PermissionGateRequest {
                config,
                snapshot,
                tool: tool.as_ref(),
                input: &input,
                id: &id,
                ctx,
                on_event,
                ask,
                session_approved,
                session_rules,
            };
            resolve_permission_gate(&mut permission_request).await
        };
        let output = match gate_result {
            // D-173:拒绝理由必须由实际注册的托管族推导,不能固定说
            // "use the dedicated tool"——那个工具可能根本不存在。
            Gate::Deny(resource) => kanzei_harness::ToolOutput::error(format!(
                "permission denied by ruleset: {action} on `{resource}`.\n{}",
                snapshot.denial_hint(action, &resource),
            )),
            Gate::NonInteractive(message) => kanzei_harness::ToolOutput::error(message),
            Gate::UserDeclined => {
                record_permission_denied(ctx, &id, &name);
                on_event(RunEvent::ToolEnd {
                    id: id.clone(),
                    name: name.clone(),
                    ok: false,
                    outcome: "failed".into(),
                    code: Some("USER_DECLINED".into()),
                    preview: "(user declined)".into(),
                    display: None,
                    artifact: None,
                });
                append_declined_tool_results(&mut results, calls, call_index);
                commit_tool_results(
                    messages,
                    results,
                    std::mem::take(pending_images),
                    step,
                    on_event,
                );
                return Ok(super::ToolRunOutcome::Stopped);
            }
            Gate::Pass => {
                if input.is_null() {
                    repair_hint(tool.as_ref(), &raw_input, "tool input was not valid JSON")
                } else {
                    // 串行路径同样接进度旁路:bash 常因权限询问走到这里,
                    // 长命令(装依赖/发版)的增量输出边执行边转发给 UI。
                    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<
                        kanzei_harness::progress::ProgressChunk,
                    >();
                    // D-174:串行路径同样开合法写入窗口。writer 阶段
                    // 走的就是这条,漏掉它等于让专用工具的写入没有窗口
                    // 可归因,后台守卫会把它当成越界回滚掉。
                    // R-259:执行包装(wrap_execute:progress 注入)收编——
                    // 串行/并行共用同一 wrapper;halted 前置拦截也在 wrapper。
                    let exec = kanzei_harness::managed_fence::tool_scope(
                        &name,
                        kanzei_harness::tool_pipeline::wrap_execute(
                            id.clone(),
                            Some(progress_tx),
                            Some(&halted),
                            tool.execute(input, ctx),
                        ),
                    );
                    tokio::pin!(exec);
                    let mut output = loop {
                        tokio::select! {
                            biased;
                            Some((pid, chunk)) = progress_rx.recv() => {
                                on_event(RunEvent::ToolProgress { id: pid, chunk });
                            }
                            output = &mut exec => break output,
                            // D-342:执行中的工具对停止敏感——drop future
                            // 即中断执行(bash 子进程随之回收),以取消
                            // 错误配对;下一轮 for 循环的检查点负责收尾。
                            _ = halt_signalled(halt) => {
                                break kanzei_harness::ToolOutput::error(
                                    "cancelled: run stopped by user during execution",
                                );
                            }
                        }
                    };
                    while let Ok((pid, chunk)) = progress_rx.try_recv() {
                        on_event(RunEvent::ToolProgress { id: pid, chunk });
                    }
                    materialize_tool_output(&mut output, ctx, &name);
                    output
                }
            }
        };
        record_tool_failure(ctx, &id, &name, &output);
        on_event(RunEvent::ToolEnd {
            id: id.clone(),
            name: name.clone(),
            ok: !output.is_error,
            outcome: output.outcome.as_str().into(),
            code: output.code.map(str::to_owned),
            preview: preview(&output.content),
            display: output.display.clone(),
            artifact: output.artifact.clone(),
        });
        let (result, images) = tool_result_part_with_images(id, output, images_supported);
        pending_images.extend(images);
        results.push(result);
    }
    Ok(super::ToolRunOutcome::Results {
        results,
        pending_images: std::mem::take(pending_images),
    })
}
