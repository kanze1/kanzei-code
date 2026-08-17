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
use kanzei_harness::{ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};

/// CLI 与桌面端共用同一份有界、可 checkpoint 的整理服务。
pub(crate) async fn consolidate_memory_inbox(
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
    client: &LlmClient,
    rctx: &ResolveCtx,
    ctx: &ToolCtx,
    current_episode_id: Option<i64>,
) -> kanzei_tools::memory_consolidation::ConsolidationReport {
    kanzei_tools::memory_consolidation::consolidate_memory_inbox(
        config,
        proxy,
        client,
        rctx,
        ctx,
        current_episode_id,
    )
    .await
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
