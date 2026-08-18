//! 并行普通工具执行段。
//!
//! 预检、PreparedToolCall 构建、停止敏感 wave 和取消占位集中在这里；
//! `drive::execute_tool_calls` 继续负责串行分派与 ToolRunOutcome 收口。

use super::*;

pub(super) struct ParallelToolRequest<'a> {
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
}

pub(super) async fn execute_parallel_tool_calls(
    request: ParallelToolRequest<'_>,
) -> (Vec<Part>, Vec<Part>) {
    let ParallelToolRequest {
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
    } = request;
    let mut pending_images: Vec<Part> = Vec::new();
    let mut slots: Vec<Option<Part>> = std::iter::repeat_with(|| None).take(calls.len()).collect();
    let mut prepared = Vec::new();
    for (index, (id, name, input, raw_input)) in calls.iter().cloned().enumerate() {
        if name == "task" && subagent.is_some() {
            let output = task_results.remove(&id).unwrap_or_else(|| {
                kanzei_harness::ToolOutput::error("internal: task result missing")
            });
            slots[index] = Some(tool_result_part(id, output));
            continue;
        }
        let tool = tools
            .iter()
            .find(|tool| tool.name() == name)
            .expect("parallel batch was preflighted")
            .clone();
        on_event(RunEvent::ToolStart {
            id: id.clone(),
            name: name.clone(),
            summary: summarize_input(&input, &raw_input),
            input: input.clone(),
        });
        let action = tool.action();
        let denied = tool
            .resources_with_ctx(&input, ctx)
            .into_iter()
            .map(|resource| {
                kanzei_harness::permission::normalize_resource_for_action(action, &resource)
            })
            .find(|resource| snapshot.evaluate(action, resource) == Effect::Deny);
        if let Some(resource) = denied {
            let rule = snapshot
                .evaluate_with_rule(action, &resource)
                .1
                .map(describe_rule);
            on_event(RunEvent::PermissionResolved {
                tool_call_id: id.clone(),
                action: action.to_string(),
                resource: resource.clone(),
                decision: "deny",
                source: "ruleset",
                rule,
            });
            let output = kanzei_harness::ToolOutput::error(format!(
                "permission denied by ruleset: {action} on `{resource}`.\n{}",
                snapshot.denial_hint(action, &resource),
            ));
            on_event(RunEvent::ToolEnd {
                id: id.clone(),
                name,
                ok: false,
                outcome: output.outcome.as_str().into(),
                code: output.code.map(str::to_owned),
                preview: preview(&output.content),
                display: None,
                artifact: None,
            });
            slots[index] = Some(tool_result_part(id, output));
            continue;
        }
        let concurrency = tool.concurrency(&input, ctx);
        prepared.push(PreparedToolCall {
            index,
            id,
            name,
            input,
            tool,
            concurrency,
        });
    }
    let wave_results = {
        let wave = execute_prepared_tools(
            prepared,
            ctx,
            config.limits.max_parallel_tools(),
            images_supported,
            on_event,
        );
        tokio::pin!(wave);
        tokio::select! {
            results = &mut wave => Some(results),
            _ = halt_signalled(halt) => None,
        }
    };
    match wave_results {
        Some(list) => {
            for (index, result, images) in list {
                slots[index] = Some(result);
                pending_images.extend(images);
            }
        }
        None => {
            for (index, (id, _, _, _)) in calls.iter().enumerate() {
                if slots[index].is_none() {
                    slots[index] = Some(Part::ToolResult {
                        call_id: id.clone(),
                        content: "cancelled: run stopped by user during execution".into(),
                        is_error: true,
                    });
                }
            }
        }
    }
    let results = slots
        .into_iter()
        .map(|result| result.expect("every preflighted tool call must produce a result"))
        .collect();
    (results, pending_images)
}
