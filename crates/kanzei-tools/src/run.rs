//! 运行装配公共层(R-256 批2):桌面端与 CLI 共用的运行期装配构建块。
//!
//! 独立理由:桌面端 `run/assembly.rs` / `run/execution.rs` 与 CLI `main.rs` 各写了一套
//! harness/模型/RunnerConfig/子代理装配,每加一个运行期能力要改两处且只有一端被验证。
//! 本模块把**两端逐字节重复的装配步骤**收成单一实现:RunnerConfig 构造与子代理运行时
//! 构造;两端差异(桌面登记读槽/取消注册表,CLI 单运行不参与)收敛为参数注入。
//!
//! 危险点(搬迁纪律):漂移项先对齐再合并,不在合并动作里顺手改行为;本模块只做
//! 「同一实现、参数化差异」,不改变任何装配顺序与字段值(对照表见
//! docs/design/monolith_decomposition.md 章节 F 的 #12/#16)。

use std::sync::Arc;

use kanzei_core::{RunnerConfig, SubagentRuntime, TaskCancellations};
use kanzei_harness::config::{KanzeiConfig, ResolvedModel};
use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use kanzei_harness::{ConfigComponent, Harness, ResolveCtx};
use kanzei_llm::{ProxyConfig, Route};

/// R-256:两端共用的 RunnerConfig 构造(对照表 #12——CLI 内联与桌面 build_runner_config
/// 字段逐项重复)。与桌面原 `run/assembly.rs::build_runner_config` 行为逐字节一致。
pub fn build_runner_config(
    resolved: &ResolvedModel,
    config: &KanzeiConfig,
    reasoning_override: Option<&str>,
    project_root: &std::path::Path,
    ask_policy: kanzei_core::AskPolicy,
    halt: Option<kanzei_core::CancellationToken>,
) -> RunnerConfig {
    RunnerConfig {
        model: resolved.model.clone(),
        max_tokens: config.limits.max_tokens(),
        reasoning: resolve_reasoning_override(
            reasoning_override,
            config.models.reasoning.as_deref(),
        ),
        service_tier: config.service_tier_for(resolved),
        context_limit: resolved.provider.context_limit,
        limits: config.limits.clone(),
        // R-162 事件触发召回:工具失败瞬间注入相关记忆 Packet(验收⑤ 桌面端/CLI 侧)。
        recall: Some(Box::new(crate::memory::FailureRecallPolicy::new(
            project_root,
        ))),
        // R-171:CLI 单运行实例用默认策略;桌面端多进程场景才启用串行写。
        execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
        ask_policy,
        halt,
    }
}

/// 推理档位覆盖:override 优先,其次配置默认,最后 Off(桌面 assembly.rs 同款私有函数)。
fn resolve_reasoning_override(
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
pub async fn build_subagent_runtime(
    rctx: &ResolveCtx,
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
    resolved: &ResolvedModel,
    route: &Route,
    coordinator: Option<Arc<dyn ProjectExecutionCoordinator>>,
    cancellations: Option<Arc<TaskCancellations>>,
) -> anyhow::Result<Option<SubagentRuntime>> {
    let mut sub_harness = Harness::default();
    sub_harness.add(crate::SubagentBase).add(ConfigComponent);
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
    Ok(Some(SubagentRuntime {
        snapshot: sub_snapshot,
        agent: crate::explore_agent(),
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
    }))
}
