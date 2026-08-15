//! 运行执行域(R-253 批4,纯搬迁自 run/mod.rs)。
//!
//! 独立理由:「执行」是运行主链路里与事件归约/装配/落库正交的一段——`build_subagent_
//! runtime` 装配 task 子代理运行时,`run_execution_loop` 驱动「恢复→附件→记忆预检索→
//! 勘察→run_once→复核修正」的隐式流水线,`run_review_and_fixup` 是复核/修正复合阶段。
//! 三者回答「这一轮怎么跑」,与「需要什么」(assembly)、「跑完怎么落」(persistence)
//! 分开后,改执行策略不必读懂装配与落库(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):③`prior` 的恢复必须留在 run_task 里——`SessionStore` 非 `Sync`,
//! 跨 `await` 持引用会破坏 future 的 `Send` 约束;这里只消费 `&[Message]`。④`on_event`/
//! `ask` 是双 `&mut dyn FnMut` 跨 `await` 重借用(run_execution_loop 里 run_once 与
//! run_review_and_fixup 共用)——只整体搬迁,任何「顺手抽个函数」的动作都不做。⑨`stage`
//! 闭包签名保持 `&(dyn Fn(&str, String) + Sync)`。

use std::sync::Arc;

use kanzei_core::{run_once_with_parts, AskFuture, RunEvent};
use kanzei_harness::{ConfigComponent, Harness, KanzeiConfig, ResolveCtx, ToolCtx};

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

/// R-253 批7b:复核/修正段的模型调用参数包——run_once 复核段真正消费的参数收成一包:
/// `client/route/snapshot/agent/runner_config/ctx` 是 RuntimeDeps 与 RoundContext 中
/// 模型调用层的子集,`prompt/subagent_rt/stage` 来自本轮执行输入。
/// 生命周期 = 执行层单次模型调用;不与装配(RuntimeDeps)或协调(RoundContext)整体
/// 绑定——复核段不背整棵树,测试也不必伪造未消费的字段(resolved/provider 等)。
pub(crate) struct ReviewExec<'a> {
    pub(crate) client: &'a kanzei_llm::LlmClient,
    pub(crate) route: &'a kanzei_llm::Route,
    pub(crate) snapshot: &'a Arc<kanzei_harness::HarnessSnapshot>,
    pub(crate) agent: &'a kanzei_harness::AgentDef,
    pub(crate) runner_config: &'a kanzei_core::RunnerConfig,
    pub(crate) ctx: &'a ToolCtx,
    pub(crate) prompt: &'a str,
    pub(crate) subagent_rt: Option<&'a kanzei_core::SubagentRuntime>,
    pub(crate) stage: &'a (dyn Fn(&str, String) + Sync),
}

/// 装配 task 子代理运行时(原 run.rs build_subagent_runtime)。
pub(crate) async fn build_subagent_runtime(
    rctx: &ResolveCtx,
    config: &KanzeiConfig,
    proxy: &kanzei_llm::ProxyConfig,
    resolved: &kanzei_harness::config::ResolvedModel,
    route: &kanzei_llm::Route,
    coordinator: &Arc<kanzei_core::orchestration::MemoryCoordinator>,
    task_cancellations: Arc<kanzei_core::TaskCancellations>,
) -> anyhow::Result<Option<kanzei_core::SubagentRuntime>> {
    let mut sub_harness = Harness::default();
    sub_harness
        .add(kanzei_tools::SubagentBase)
        .add(ConfigComponent);
    let sub_snapshot = sub_harness.resolve(rctx)?;
    let fast = match config.resolve_model("fast") {
        Ok(r) => (kanzei_core::build_route(&r, proxy).await)
            .ok()
            .map(|fr| (fr, r.model.clone(), config.service_tier_for(&r))),
        Err(_) => None,
    };
    let primary_tier = config.service_tier_for(resolved);
    let fast_tier = fast
        .as_ref()
        .map(|(_, _, tier)| tier.clone())
        .unwrap_or_else(|| primary_tier.clone());
    // R-236 B3:压缩纪要模型——[models].compact 显式配置才建独立路由;
    // 缺省传 None,运行时回落主模型(digest_model),少建一条重复路由。
    let compact = match config
        .models
        .compact
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        Some(_) => match config.resolve_model("compact") {
            Ok(r) => (kanzei_core::build_route(&r, proxy).await)
                .ok()
                .map(|cr| (cr, r.model.clone(), config.service_tier_for(&r))),
            Err(_) => None,
        },
        None => None,
    };
    Ok(Some(kanzei_core::SubagentRuntime {
        snapshot: sub_snapshot,
        agent: kanzei_tools::explore_agent(),
        fast: fast
            .map(|(r, m, _)| (r, m))
            .unwrap_or_else(|| (route.clone(), resolved.model.clone())),
        primary: (route.clone(), resolved.model.clone()),
        fast_service_tier: fast_tier,
        primary_service_tier: primary_tier,
        compact,
        max_tokens: config.limits.subagent_max_tokens(),
        // 纯兜底(用户定调:不设短限),防子代理失控挂死整轮。
        timeout_secs: config.limits.subagent_timeout_secs(),
        limits: config.limits.clone(),
        // R-171 批6:task 子代理登记读槽(并行查身份可见,结束自动释放)。
        coordinator: Some(Arc::clone(coordinator)
            as Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>),
        // R-176 B2:主对话的 task 子代理是只读勘察/复核——不启用可写档位。
        writable: false,
        ask_router: None,
        change_log: None,
        // R-174:主对话 run 持单条停止注册表,stop_task 命令按 id 命中取消。
        cancellations: Some(task_cancellations),
        // R-175:桌面端主对话本轮保持等齐语义;后台模式由 phase_pipeline
        // (编排角色)开启——run.rs 是用户直连对话,不后台化。
        background: false,
        // R-175 B1b:主对话无后台结果暂存。
        background_results: None,
        // R-175 B2:主对话无后台生命周期事件落库(仅编排角色后台化时开)。
        background_events: None,
        // R-175 B3:主对话不后台化,不启用 transcript 续跑。
        transcripts: None,
        // R-175 B4:主对话不后台化,不发后台通知。
        background_notifications: None,
    }))
}

/// R-202 批2:run_task 的事件循环段(原 run_task :1043-1188)——会话恢复 → 附件提示 →
/// 记忆预检索 → 勘察(scout)→ 主循环(run_once_with_parts)→ 复核修正(run_review_and_fixup)。
/// 行为零变更:先恢复 prior,再注入记忆提示,再按流水线状态机 scout/begin_implementation,
/// 最后 run_once + 复核。返回 (run_result, prior):prior 供轮末 typed shadow 报告与
/// 本轮切片使用。
/// R-253 批7b:按层收参(deps+round+execution input+on_event/ask),消 too_many。
/// round 收 `&mut RoundContext`:执行循环要就地驱动 `round.pipeline` 状态机,
/// 与 `round.ctx` 是**不相交字段借用**(ctx 只读、pipeline 独占),可同时成立;
/// 协调器在调用期间不再触碰 round,返回后仍可整体取回字段。
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
    let pipeline = &mut round.pipeline;
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
            .filter(|part| matches!(part, kanzei_llm::Part::Document { .. }))
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
    let memory_hints = kanzei_tools::memory::prompt_hints(
        &ctx.project_root,
        prompt,
        autonomous,
        // R-233:配置了 [embeddings] 就带 embedder 走 hybrid,否则纯 BM25。
        kanzei_tools::embed::embedder_from_config(config)
            .ok()
            .flatten(),
    );
    let run_prompt = prompt.to_string();
    let mut scout_brief: Option<String> = None;
    // R-173 批6 · 勘察阶段:按角色表并行派发只读代理 → 汇总屏障 → 取写租约。
    // 顺序不是靠这里写对,是靠状态机——`begin_implementation` 只能从 synthesis 进,
    // 而 synthesis 的唯一入边是 `scout` 里的汇总屏障(不变量 2)。
    if let Some(pipeline) = pipeline.as_mut() {
        match subagent_rt.as_ref() {
            Some(template) => {
                stage(
                    "勘察",
                    format!(
                        "并行只读勘察中(最多 {} 个角色)…",
                        config.limits.max_tasks_per_turn()
                    ),
                );
                match pipeline
                    .scout(client, template, ctx, prompt, on_event)
                    .await
                {
                    Ok(brief) => {
                        stage("屏障", "勘察全部进入终态,开始申请写租约".into());
                        // 简报只进本轮 system(同 D-185 的 memory_hints),不拼进
                        // prompt。拼进去就随 User message 落 conversations,下一轮
                        // 作为 prior 回灌——而这里每轮都会重新勘察,回灌的旧简报
                        // 永远不是最新可用信息。实测代价:agent 得先推理「这看起来
                        // 是上个会话的残留」再决定忽略,分辨成本与 token 都照付。
                        scout_brief = Some(brief);
                    }
                    Err(error) => {
                        // 勘察失败不该让这一轮跑不成:按无勘察继续,但**不静默**——
                        // 阶段面板上有这一行,轨迹里有 barrier 事件可查。
                        stage("勘察", format!("勘察阶段失败,本轮无勘察简报:{error}"));
                    }
                }
            }
            None => {
                // 子代理关闭时没有勘察能力:空屏障照样走一遍,轨迹里留下
                // agent_count=0 的 barrier,而不是让阶段序列缺一截。
                let _ = pipeline.scout_skipped().await;
            }
        }
        pipeline
            .begin_implementation()
            .await
            .map_err(|e| anyhow::anyhow!("无法进入实现阶段: {e}"))?;
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
        scout_brief.as_deref(),
        prior,
        (!initial_parts.is_empty()).then_some(initial_parts),
        subagent_rt.as_ref(),
        on_event,
        ask,
    )
    .await;
    // R-173 批6 · 集成 → 复核屏障 → 复核 → 修正。
    //
    // 复核屏障(`review` 内的第一句)会**交出写租约**,所以复核代理审的是稳定快照
    // (不变量 9)。只有复核真有发现时才会有第二段 run_once;无发现时本轮的
    // run_once 次数与引入前一样是 1 次。
    let run_result = match (pipeline.as_mut(), run_result) {
        (Some(pipeline), Ok(summary)) => {
            let review_exec = ReviewExec {
                client,
                route,
                snapshot,
                agent,
                runner_config,
                ctx,
                prompt,
                subagent_rt: subagent_rt.as_ref(),
                stage,
            };
            let merged = run_review_and_fixup(&review_exec, pipeline, summary, on_event, ask).await;
            pipeline.finish();
            merged
        }
        (Some(pipeline), Err(error)) => {
            // 运行失败:不变量 7——任意结束路径都要交出租约并给确定终态。
            pipeline.abort("run failed");
            Err(error)
        }
        (None, result) => result,
    };
    run_result
}

/// R-173 批6:集成 → 复核屏障 → 复核 → 修正。
///
/// 只在阶段流水线开启(自主推进轮)时被调用。返回**合并后**的 RunSummary:
/// - 无复核发现:原样返回实现段的 summary,本轮 run_once 次数 = 1(与引入前一致);
/// - 有复核发现:跑一段修正 run_once,`prior` 接实现段的完整 `messages`,
///   所以返回的 `messages` 是「实现段 + 修正段」的连续历史,
///   `prior.len()` 之后的切片仍然正好是本轮全部内容(轮末统计口径不变)。
///
/// R-253 批7b:按层收参(`ReviewExec`/pipeline/summary/on_event/ask),消 too_many。
/// `ReviewExec` 见上:模型调用参数链(run_once 复核段真正消费的子集),生命周期 =
/// 执行层单次模型调用,不与 RuntimeDeps/RoundContext 整体绑定。
pub(crate) async fn run_review_and_fixup(
    exec: &ReviewExec<'_>,
    pipeline: &mut crate::phase_pipeline::PhasePipeline,
    summary: kanzei_core::RunSummary,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
    ask: &mut (dyn FnMut(kanzei_core::AskRequest) -> AskFuture + Send),
) -> anyhow::Result<kanzei_core::RunSummary> {
    let client = exec.client;
    let route = exec.route;
    let snapshot = exec.snapshot;
    let agent = exec.agent;
    let runner_config = exec.runner_config;
    let ctx = exec.ctx;
    let prompt = exec.prompt;
    let subagent_rt = exec.subagent_rt;
    let stage = exec.stage;
    if let Err(error) = pipeline.begin_integration() {
        tracing::warn!(%error, "进入集成阶段失败,跳过复核");
        return Ok(summary);
    }
    let Some(template) = subagent_rt else {
        // 没有子代理能力:仍然走复核屏障(交出写租约),只是没有角色可派。
        if let Err(error) = pipeline.review_skipped().await {
            tracing::warn!(%error, "空复核屏障失败");
        }
        return Ok(summary);
    };
    stage("复核", "写租约已交出,并行只读复核中…".into());
    let findings = match pipeline
        .review(client, template, ctx, prompt, &summary.text, on_event)
        .await
    {
        Ok(findings) => findings,
        Err(error) => {
            stage("复核", format!("复核阶段失败,跳过修正:{error}"));
            return Ok(summary);
        }
    };
    let Some(findings) = findings else {
        stage("复核", "复核无发现,本轮收工".into());
        return Ok(summary);
    };
    stage("修正", "复核有发现,重新获取写租约执行修正…".into());
    if let Err(error) = pipeline.begin_fixup().await {
        stage("修正", format!("无法进入修正阶段:{error}"));
        return Ok(summary);
    }
    let fixup = run_once_with_parts(
        client,
        route,
        snapshot,
        agent,
        runner_config,
        ctx,
        &crate::phase_pipeline::fixup_prompt(&findings),
        // 修正段不注入新记忆提示:同一轮内的第二段,提示已含于实现段的 system。
        None,
        // 修正段同理不再注入勘察简报:实现段的 system 里已经有了。
        None,
        // 历史接续:修正段的 prior 就是实现段跑完的完整 messages。
        &summary.messages,
        None,
        subagent_rt,
        on_event,
        ask,
    )
    .await;
    match fixup {
        Ok(mut merged) => {
            // 合并口径:token/步数是两段之和(用户看到的是"这一条消息花了多少"),
            // messages/context_report 取修正段的——它已经含实现段全历史。
            merged.usage.input += summary.usage.input;
            merged.usage.output += summary.usage.output;
            merged.usage.reasoning += summary.usage.reasoning;
            merged.usage.cache_read += summary.usage.cache_read;
            merged.usage.cache_write += summary.usage.cache_write;
            merged.steps += summary.steps;
            merged.halted_by_user |= summary.halted_by_user;
            let mut traces = summary.overflow_traces;
            traces.extend(merged.overflow_traces);
            merged.overflow_traces = traces;
            Ok(merged)
        }
        Err(error) => {
            // 修正段失败不推翻实现段的成果:实现段已经落盘的改动仍然有效,
            // 本轮按实现段的结果收尾,失败在阶段面板上可见。
            stage("修正", format!("修正段失败,按实现段结果收尾:{error}"));
            Ok(summary)
        }
    }
}
