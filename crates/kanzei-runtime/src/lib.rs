//! Shared composition root for CLI, desktop and scheduled execution.
//! Capability crates do not construct runners or depend on this crate.
#[cfg(test)]
mod memory_tests;
pub mod schedules;

use std::sync::Arc;

use kanzei_core::{
    BackgroundEventSink, RunnerConfig, SubagentRuntime, SubagentTranscriptProvider,
    TaskCancellations,
};
use kanzei_harness::config::{KanzeiConfig, ResolvedModel};
use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use kanzei_harness::{ConfigComponent, Harness, MarkdownComponent, ResolveCtx};
use kanzei_llm::{ProxyConfig, Route};

use kanzei_tools::{BaseComponent, DevelopmentProfile, ResearchProfile};

/// Shared capability composition. Built-in Agent templates precede custom
/// Markdown definitions; config contributes permission rules. The final
/// projectless projection removes project capabilities without rewriting Agents.
pub fn build_harness(
    profile: kanzei_harness::ProfileKind,
    middle: impl FnOnce(&mut Harness),
    tail: impl FnOnce(&mut Harness),
) -> Harness {
    let mut harness = Harness::default();
    if profile == kanzei_harness::ProfileKind::Isolated {
        harness
            .add(kanzei_tools::IsolatedProfile)
            .add(ConfigComponent);
        return harness;
    }
    harness
        .add(BaseComponent)
        .add(DevelopmentProfile)
        .add(ResearchProfile)
        .add(kanzei_tools::ReadonlyProfile)
        .add(kanzei_tools::GeneralChatProfile);
    middle(&mut harness);
    harness.add(MarkdownComponent).add(ConfigComponent);
    tail(&mut harness);
    harness.add(kanzei_tools::GeneralChatScope);
    harness
}

/// CLI、桌面与定时任务共用运行配置；隔离档位不安装项目记忆召回。
pub fn build_runner_config(
    resolved: &ResolvedModel,
    config: &KanzeiConfig,
    reasoning_override: Option<&str>,
    project_root: &std::path::Path,
    profile: kanzei_harness::ProfileKind,
    ask_policy: kanzei_core::AskPolicy,
    halt: Option<kanzei_core::CancellationToken>,
) -> RunnerConfig {
    RunnerConfig {
        hosted_tools: Vec::new(),
        digest_model: None,
        intensity: kanzei_harness::HarnessIntensity::Autonomous,
        model: resolved.model.clone(),
        max_tokens: config.limits.max_tokens(),
        reasoning: resolve_reasoning_override(
            reasoning_override,
            config.models.reasoning.as_deref(),
        ),
        service_tier: config.service_tier_for(resolved),
        context_limit: resolved.provider.context_limit,
        limits: config.limits.clone(),
        recall: (profile != kanzei_harness::ProfileKind::Isolated).then(|| {
            Box::new(kanzei_memory::memory::FailureRecallPolicy::new(
                project_root,
            )) as Box<dyn kanzei_core::RecallPolicy>
        }),
        execution_policy: if profile == kanzei_harness::ProfileKind::Readonly {
            kanzei_harness::ExecutionPolicy::ReadOnly
        } else {
            kanzei_harness::ExecutionPolicy::Default
        },
        ask_policy,
        halt,
    }
}

/// 任务提示只检索调用方提供的任务；显式嵌入配置错误不伪装成未配置。
pub fn task_memory_hints(
    config: &KanzeiConfig,
    profile: kanzei_harness::ProfileKind,
    project_root: &std::path::Path,
    query: &str,
    run_id: Option<&str>,
) -> anyhow::Result<Option<String>> {
    if profile == kanzei_harness::ProfileKind::Isolated {
        return Ok(None);
    }
    Ok(kanzei_memory::memory::prompt_hints_for_run(
        project_root,
        query,
        kanzei_memory::embed::embedder_from_config(config)?,
        run_id,
    ))
}

pub async fn build_digest_model(
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
    resolved: &ResolvedModel,
    route: &Route,
) -> kanzei_core::DigestModel {
    let fallback = kanzei_core::DigestModel {
        route: route.clone(),
        model: resolved.model.clone(),
        service_tier: config.service_tier_for(resolved),
        focus: None,
        archive_root: None,
    };
    if config
        .models
        .compact
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        if let Ok(compact) = config.resolve_model("compact") {
            if let Ok(route) = kanzei_core::build_route(&compact, proxy).await {
                return kanzei_core::DigestModel {
                    route,
                    model: compact.model.clone(),
                    service_tier: config.service_tier_for(&compact),
                    focus: None,
                    archive_root: None,
                };
            }
        }
        tracing::warn!("configured compact model unavailable; using primary model for compaction");
    }
    fallback
}

/// 推理档位覆盖:override 优先,其次配置默认,最后 Off。
///
/// UI-0926 #3:这是「这一轮用哪一档思考」的**唯一判据**——运行路径(build_runner_config)
/// 与桌面端开跑前的展示查询(kanzei-app model_config::model_effective)都调它,
/// 输入框上方的思考芯片因此与真正发出去的档位同源。不要在别处就地抄 `or().map(parse)`。
pub fn resolve_reasoning_override(
    override_value: Option<&str>,
    configured_value: Option<&str>,
) -> kanzei_llm::ReasoningEffort {
    override_value
        .or(configured_value)
        .map(kanzei_llm::ReasoningEffort::parse)
        .unwrap_or_default()
}

/// R-256:两端共用的 task 子代理运行时构造(对照表 #16——CLI 内联与桌面
/// `run/execution.rs::build_subagent_runtime` 除 coordinator/cancellations 外逐字节相同)。
/// 桌面端传 `Some(coordinator)`/`Some(cancellations)`(登记读槽 R-171 批6、单条停止
/// 注册表 R-174);CLI 单运行传 `None`/`None`(不参与共享仲裁、无前端停止按钮)。
#[allow(clippy::too_many_arguments)] // 对外装配函数,参数对应装配点差异(coordinator/cancellations/transcript 钩子),收拢成对象会扰动全部调用方(R-256/R-279)。
pub async fn build_subagent_runtime(
    rctx: &ResolveCtx,
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
    resolved: &ResolvedModel,
    route: &Route,
    coordinator: Option<Arc<dyn ProjectExecutionCoordinator>>,
    cancellations: Option<Arc<TaskCancellations>>,
    // R-279:子代理 transcript 事件落库/恢复回调。桌面端传(写主会话事件 +
    // gate 判断读事件);CLI 单轮 task 传 None(续跑与跨进程恢复非 CLI 场景)。
    transcript_sink: Option<BackgroundEventSink>,
    transcript_provider: Option<SubagentTranscriptProvider>,
) -> anyhow::Result<Option<SubagentRuntime>> {
    if rctx.profile == kanzei_harness::ProfileKind::Isolated {
        return Ok(None);
    }
    let mut sub_harness = Harness::default();
    sub_harness
        .add(kanzei_tools::SubagentBase)
        .add(MarkdownComponent)
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
    let roster = sub_snapshot
        .agents()
        .iter()
        .map(|(_, agent)| agent)
        .filter(|agent| {
            agent.mode == kanzei_harness::AgentMode::Subagent
                && agent.profile.includes(rctx.profile)
        })
        .cloned()
        .collect();
    Ok(Some(SubagentRuntime {
        options: kanzei_core::SubagentOptions {
            mode: kanzei_harness::SubagentMode::Auto,
            reasoning: resolve_reasoning_override(None, config.models.reasoning.as_deref()),
            primary_context_limit: resolved.provider.context_limit,
            fast_context_limit: config
                .resolve_model("fast")
                .ok()
                .and_then(|m| m.provider.context_limit)
                .or(resolved.provider.context_limit),
            host: None,
            ask_policy: None,
        },
        // 基础名册提供只读角色；桌面宿主的 task 定义按档位额外提供可写角色。
        roster,
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
        // R-171 批6:桌面登记读槽(并行查身份可见,结束自动释放);CLI 单运行不参与。
        coordinator,
        // R-176 B2:主对话/CLI 单运行的 task 子代理只读勘察,不启用可写档位。
        writable: false,
        ask_router: None,
        change_log: None,
        // R-174:桌面挂单条停止注册表;CLI 无前端停止按钮,不挂。
        cancellations,
        // R-175:两端主对话本轮保持等齐语义,不开后台模式。
        background: false,
        background_results: None,
        background_events: None,
        transcripts: None,
        background_notifications: None,
        transcript_sink,
        transcript_provider,
    }))
}
