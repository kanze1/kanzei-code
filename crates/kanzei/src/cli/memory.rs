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
use std::collections::BTreeSet;

use kanzei_tools::memory::{MemoryEntry, MemoryStore};

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

fn normalized_theme(title: &str) -> String {
    title
        .chars()
        .filter(|ch| !ch.is_whitespace() && ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn active_theme_count(entries: &[(std::path::PathBuf, MemoryEntry)]) -> (usize, usize) {
    let entries = entries
        .iter()
        .filter(|(_, entry)| matches!(entry.status.as_str(), "active" | "candidate"));
    let mut themes = BTreeSet::new();
    let mut count = 0;
    for (_, entry) in entries {
        count += 1;
        let key = normalized_theme(&entry.title);
        if !key.is_empty() {
            themes.insert(key);
        }
    }
    (count, themes.len())
}

/// 显式执行一次真实项目/global memory review，不调用模型、不进入生产 global 检索。
pub async fn memory_cli(args: &[String]) -> anyhow::Result<()> {
    if args.len() != 1 {
        anyhow::bail!("usage: kz memory repair-index | review-global");
    }
    let cwd = std::env::current_dir()?;
    let project_root = super::main_project_root(None, &cwd)?;
    match args[0].as_str() {
        "repair-index" => {
            let store = MemoryStore::project(&project_root);
            let entries = store.load_all().len();
            store.repair_derived()?;
            println!(
                "memory repair-index: project entries={entries} INDEX/FTS rebuilt from Markdown"
            );
        }
        "review-global" => {
            let report = kanzei_tools::memory::reconcile_candidates(&project_root, None, 365)?;
            let project = MemoryStore::project(&project_root).load_all();
            let global_store = MemoryStore::global();
            let global = global_store.as_ref().map(|store| store.load_all());
            let persisted_global_recall_rows = global_store
                .as_ref()
                .map(|store| {
                    store
                        .recalls(100_000)
                        .iter()
                        .filter(|round| round.prompt_head == "global-candidate-review-v1")
                        .count()
                })
                .unwrap_or(0);
            let (project_entries, project_themes) = active_theme_count(&project);
            let (global_entries, global_themes) =
                global.as_deref().map(active_theme_count).unwrap_or((0, 0));
            println!(
                "memory review-global: project entries={} themes={} merged={} candidate={}→{}; global entries={} themes={} reviewed={} deprecated={} recall_rows={}",
                project_entries,
                project_themes,
                report.merged.len(),
                report.candidate_files_before,
                report.candidate_files_after,
                global_entries,
                global_themes,
                report.global_reviewed,
                report.global_deprecated,
                report.global_recall_rows.max(persisted_global_recall_rows),
            );
        }
        other => anyhow::bail!(
            "unknown memory command `{other}`; usage: kz memory repair-index | review-global"
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalized_theme;

    #[test]
    fn normalized_theme_collapses_spacing_and_punctuation() {
        assert_eq!(normalized_theme("Same Merge Title!"), "samemergetitle");
        assert_eq!(normalized_theme("same merge title"), "samemergetitle");
    }
}
