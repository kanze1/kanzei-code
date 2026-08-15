//! `kz run` 轮末记忆整理与直通放行(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:轮末记忆整理(`consolidate_memory_inbox` 消化 inbox 草稿)是写读分离
//! 的写端,`persist_always_allow` 是交互式「总是允许」规则落盘——两者都是 run 的
//! 轮末/应答辅助,与命令分发正交(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):manager 每轮至多跑一次、prompt 仅数 KB,用主模型换蒸馏质量
//! (M-003 教训);R-213 当轮 episode_id 代填给 manager,否则 provenance 校验拦下
//! 一切晋升。

use kanzei_harness::config::KanzeiConfig;
use kanzei_harness::{Harness, ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};

/// memory-manager 迷你 run:消化 inbox 草稿(写读分离的写端)。
/// fast 失败升级 primary;成功判据只看箱——清空才算消化完成。
pub(crate) async fn consolidate_memory_inbox(
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
    client: &LlmClient,
    rctx: &ResolveCtx,
    ctx: &ToolCtx,
    current_episode_id: Option<i64>,
) {
    let store = kanzei_tools::memory::MemoryStore::project(&ctx.project_root);
    if store.pending_notes() == 0 {
        return;
    }
    let inbox = store.read_inbox();
    let mut harness = Harness::default();
    harness.add(kanzei_tools::memory::MemoryManagerComponent);
    let Ok(snapshot) = harness.resolve(rctx) else {
        return;
    };
    let agent = kanzei_tools::memory::manager_agent();
    // R-213:引擎轮末代填当轮 episode_id——manager 在轮内自报不出真实 id(episode 轮末
    // 才落库、list_episodes 不含 id),不注入的话 memory_promote 的 provenance 校验会
    // 拦下一切晋升,候选记忆永远升不了 active。
    let prompt = kanzei_tools::memory::consolidation_prompt(&inbox, current_episode_id);
    // primary 优先(fast 兜底):记忆会注入之后每一轮的上下文,写错一条就长期误导。
    // 实测 fast(qwen3.5:4b)把失败**次数**误读成事实内容,生成了"需要约 7 次重试才能成功"
    // 这种编造结论(M-003 已人工校正)。manager 每轮至多跑一次、prompt 仅数 KB,
    // 用主模型换蒸馏质量是划算的。
    for role in ["primary", "fast"] {
        let Ok(resolved) = config.resolve_model(role) else {
            continue;
        };
        let Ok(route) = kanzei_core::build_route(&resolved, proxy).await else {
            continue;
        };
        let runner_config = kanzei_core::RunnerConfig {
            model: resolved.model.clone(),
            max_tokens: 4096,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
            ask_policy: kanzei_core::AskPolicy::NonInteractive,
            halt: None,
        };
        let mut on_event = |_event: kanzei_core::RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
            Box::pin(async move {
                match request {
                    // 快照里只有 memory_* 工具,放行安全;问题一律取消(无人应答)。
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        let _ = kanzei_core::run_once(
            client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            ctx,
            &prompt,
            None,
            &[],
            None,
            &mut on_event,
            &mut ask,
        )
        .await;
        if store.pending_notes() == 0 {
            eprintln!("\x1b[90m(memory: inbox 已整理入库)\x1b[0m");
            return;
        }
    }
    eprintln!("\x1b[90m(memory: inbox 未消化,留待下轮)\x1b[0m");
}

pub(crate) fn persist_always_allow(
    project_root: &std::path::Path,
    action: &str,
    resource: &str,
) -> anyhow::Result<kanzei_core::AskReply> {
    let pattern = kanzei_harness::config::generalize_resource(action, resource);
    kanzei_harness::config::append_allow_rule(project_root, action, &pattern)?;
    Ok(kanzei_core::AskReply::AlwaysAllow)
}
