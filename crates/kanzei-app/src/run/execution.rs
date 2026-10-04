//! Model execution, attachments and memory hints. Delegation is model-directed.
use kanzei_core::{run_once_with_parts, AskFuture, RunEvent};

use super::assembly::{RoundContext, RuntimeDeps};

/// R-253 批7b:执行循环的输入打包(本轮执行输入,不含生命周期分组)。
pub(crate) struct ExecutionInput<'a> {
    pub(crate) stage: &'a (dyn Fn(&str, String) + Sync),
    pub(crate) initial_parts: &'a [kanzei_llm::Part],
    pub(crate) prompt: &'a str,
    pub(crate) autonomous: bool,
    pub(crate) subagent_rt: &'a Option<kanzei_core::SubagentRuntime>,
    pub(crate) prior: &'a [kanzei_llm::Message],
}

pub(crate) async fn run_execution_loop(
    deps: &RuntimeDeps,
    round: &mut RoundContext,
    input: &ExecutionInput<'_>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
    ask: &mut (dyn FnMut(kanzei_core::AskRequest) -> AskFuture + Send),
) -> Result<kanzei_core::RunSummary, anyhow::Error> {
    let stage = input.stage;
    let initial_parts = input.initial_parts;
    let prompt = input.prompt;
    let ctx = &round.ctx;
    let autonomous = input.autonomous;
    let config = &deps.config;
    let subagent_rt = input.subagent_rt;
    let client = &deps.client;
    let route = &deps.route;
    let snapshot = &deps.snapshot;
    let agent = &deps.agent;
    let runner_config = &deps.runner_config;
    let prior = input.prior;
    if !initial_parts.is_empty() {
        let image_count = initial_parts
            .iter()
            .filter(|part| matches!(part, kanzei_llm::Part::Image { .. }))
            .count();
        let document_count = initial_parts
            .iter()
            .filter(|part| {
                matches!(
                    part,
                    kanzei_llm::Part::Document { .. } | kanzei_llm::Part::Text { .. }
                )
            })
            .count();
        stage(
            "附件",
            format!(
                "已接收 {} 个附件，转换为 {} 个图片、{} 个文档输入，准备发送给 agent",
                initial_parts.len(),
                image_count,
                document_count
            ),
        );
    }

    // 开跑预检索(R-106):prompt 命中既有记忆时前置索引提示块;历史存用户原文。
    // D-185:提示块不再拼进 run_prompt,改由 run_once 作为本轮 system 一次性注入——
    // 拼进去会随 User message 进 messages → 落 conversations → 下轮回灌累积。
    // 自主轮以同一调度器的当前选择检索，保持与执行顺序一致。
    let memory_query = if autonomous && ctx.project_workflow {
        kanzei_tools::work::resolve_work_selection(&ctx.cwd, &ctx.project_root, ctx.work_priority)
            .ok()
            .and_then(|state| state.selected)
            .map(|item| item.title)
            .unwrap_or_default()
    } else {
        prompt.to_string()
    };
    let memory_hints = kanzei_tools::memory::prompt_hints_for_run(
        &ctx.project_root,
        &memory_query,
        false,
        // R-233:配置了 [embeddings] 就带 embedder 走 hybrid,否则纯 BM25。
        kanzei_tools::embed::embedder_from_config(config)
            .ok()
            .flatten(),
        ctx.run_id.as_deref(),
    );
    let run_prompt = prompt.to_string();
    if let Some(item_id) = round.work_item_id.as_deref() {
        stage("领取", format!("按用户选择领取 {item_id}…"));
        super::work_start::claim_requested_work(ctx, item_id, on_event).await?;
    }
    stage("请求", "已取得工作树写入槽，正在等待模型首响应…".into());
    let run_result = run_once_with_parts(
        client,
        route,
        snapshot,
        agent,
        runner_config,
        ctx,
        &run_prompt,
        memory_hints.as_deref(),
        prior,
        (!initial_parts.is_empty()).then_some(initial_parts),
        subagent_rt.as_ref(),
        // R-246:桌面侧 LineRuntime 由 run/mod.rs 持有并注入(本批先传 None,
        // 批5 桌面接线 persistent adopt 时连入;行为与引入前一致)。
        None,
        on_event,
        ask,
    )
    .await;
    run_result
}
