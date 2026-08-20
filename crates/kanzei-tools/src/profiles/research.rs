use std::sync::Arc;

use kanzei_harness::{rule, Effect, HarnessDraft, ResolveCtx};

use crate::docstore::{DocStore, DEFECTS, FINDINGS, REQUIREMENTS, SOURCES};
use crate::tracker::{ResearchTrackerTool, TrackerTool};

/// Research 档位的工具面与回流工具注册。
///
/// 保持原 `ResearchProfile::contribute` 的注册顺序与资源配置；调用方仍是
/// `ResearchProfile` 的真实 Harness 装配路径。
pub(crate) fn register_tools(draft: &mut HarnessDraft) {
    draft.tools.insert(
        "source",
        Arc::new(TrackerTool {
            tool_name: "source",
            noun: "source",
            kind: &SOURCES,
            requires_refs: None,
        }),
    );
    draft
        .tools
        .insert("websearch", Arc::new(crate::websearch::WebSearchTool));
    draft.tools.insert(
        "research_plan",
        Arc::new(crate::research_plan::ResearchPlanTool),
    );
    draft.tools.insert(
        "research_loop",
        Arc::new(crate::research_loop::ResearchLoopTool),
    );
    draft.tools.insert(
        "research_write",
        Arc::new(crate::research_write::ResearchWriteTool),
    );
    draft.tools.insert(
        "research_verify",
        Arc::new(crate::research_verify::ResearchVerifyTool),
    );
    draft.tools.insert(
        "research_index",
        Arc::new(crate::research_index::ResearchIndexTool),
    );
    draft.tools.insert(
        "finding",
        Arc::new(TrackerTool {
            tool_name: "finding",
            noun: "finding",
            kind: &FINDINGS,
            requires_refs: Some(&SOURCES),
        }),
    );

    draft
        .tools
        .insert("memory_search", Arc::new(crate::memory::MemorySearchTool));
    draft
        .tools
        .insert("memory_note", Arc::new(crate::memory::MemoryNoteTool));

    draft.tools.insert(
        "req",
        Arc::new(ResearchTrackerTool::new(
            "req",
            "requirement",
            &REQUIREMENTS,
            None,
        )),
    );
    draft.tools.insert(
        "defect",
        Arc::new(ResearchTrackerTool::new("defect", "defect", &DEFECTS, None)),
    );
}

/// Research 档位的权限与专用工具资源注册。
pub(crate) fn configure_permissions(draft: &mut HarnessDraft) {
    draft
        .permissions
        .push(rule("memory_search", "*", Effect::Allow));
    draft
        .permissions
        .push(rule("memory_note", "*", Effect::Allow));

    // research 回流只允许读取既有单条目和新增草稿；禁止 list/update/close 等既有状态变更。
    for tool_name in ["source", "finding", "req", "defect"] {
        draft
            .permissions
            .push(rule(tool_name, "read:get", Effect::Allow));
        draft
            .permissions
            .push(rule(tool_name, "write:add", Effect::Allow));
        for action in [
            "update",
            "close",
            "archive",
            "reorder",
            "reopen",
            "fix_terminal",
            "archive_fill",
            "raw_delete",
            "repair_reused_id",
            "repair_missing_id",
            "void_id",
            "normalize",
        ] {
            let resource = format!("write:{action}");
            draft.permissions.push_managed_hard_deny(
                rule(tool_name, &resource, Effect::Deny),
                Some(tool_name),
                Some("research 档只允许 tracker get/add；不可修改既有条目状态"),
            );
        }
    }

    // 写权限收窄:仅 .kanzei/research/** 可写(report.md 等自由写作);其余 deny。
    for action in ["write", "edit", "insert"] {
        draft.permissions.push(rule(action, "*", Effect::Deny));
        draft
            .permissions
            .push(rule(action, "*.kanzei/research/*", Effect::Allow));
    }
    // 研究档位的命令面是硬拒绝:专用工具是唯一真实替代通道,避免模型从 bash 绕过
    // LaTeX/绘图工具的权限边界。硬拒绝不可被用户规则重新放开。
    draft.permissions.push_managed_hard_deny(
        rule("bash", "*", Effect::Deny),
        None,
        Some("research 档禁止 bash；请使用已注册的 `latex` 或 `plot` 专用工具，或使用 read/glob/grep/files/git status|diff|log 读取事实"),
    );
    // research 允许观察 Git，但禁止任何会改写仓库或发布的结构化动作。
    for subcommand in ["stage", "commit", "merge_ff", "finalize"] {
        draft.permissions.push_managed_hard_deny(
            rule("git", subcommand, Effect::Deny),
            None,
            Some("research 档只允许 git status|diff|log 观察；不可提交或修改仓库"),
        );
    }
    draft.permissions.push(rule("webfetch", "*", Effect::Allow));
    draft
        .permissions
        .push(rule("websearch", "*", Effect::Allow));
    for resource in [
        "read:get",
        "write:create",
        "write:clarify",
        "write:request_approval",
    ] {
        draft
            .permissions
            .push(rule("research_plan", resource, Effect::Allow));
    }
    for resource in [
        "read:resume",
        "write:start",
        "write:begin_search",
        "write:add_evidence",
        "write:reflect",
        "write:add_finding",
    ] {
        draft
            .permissions
            .push(rule("research_loop", resource, Effect::Allow));
    }
    for resource in [
        "write:write_outline",
        "write:write_section",
        "write:assemble_paper",
        "write:compile_paper",
        "write:repair_paper",
    ] {
        draft
            .permissions
            .push(rule("research_write", resource, Effect::Allow));
    }
    for resource in [
        "write:capture_source",
        "write:verify_claims",
        "write:budget_get",
        "write:budget_set",
    ] {
        draft
            .permissions
            .push(rule("research_verify", resource, Effect::Allow));
    }
    for resource in ["write:index_build", "write:index_resume"] {
        draft
            .permissions
            .push(rule("research_index", resource, Effect::Allow));
    }
    for resource in ["read:search", "read:symbols"] {
        draft
            .permissions
            .push(rule("research_index", resource, Effect::Allow));
    }
}

/// 读取索引来源。S-/F- 在 B2 前可能留在 flat 文件,之后写入 topic 目录；
/// 每次渲染都重新扫描两种落点,因此同一会话下一轮不会继续使用旧 flat 快照。
type IndexedEntry = (Option<String>, crate::docstore::Entry);

fn load_index_entries(
    ctx: &ResolveCtx,
    kind: &'static crate::docstore::DocKind,
) -> Option<(Vec<IndexedEntry>, usize)> {
    let store = DocStore::open(&ctx.project_root, kind);
    let entries = store.load().ok()?;
    let mut indexed = entries
        .into_iter()
        .map(|entry| (None, entry))
        .collect::<Vec<_>>();
    let mut closed = indexed
        .iter()
        .filter(|(_, entry)| kind.terminal.contains(&entry.status.as_str()))
        .count();
    closed += store.load_archive().map_or(0, |archive| archive.len());

    if !matches!(kind.prefix, "S" | "F") {
        return Some((indexed, closed));
    }

    let research_root = ctx.project_root.join(".kanzei/research");
    let mut topics = std::fs::read_dir(research_root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let item = item.ok()?;
            if !item.file_type().ok()?.is_dir() {
                return None;
            }
            let topic = item.file_name().into_string().ok()?;
            crate::docstore::DocStore::validate_topic(&topic).ok()?;
            Some(topic)
        })
        .collect::<Vec<_>>();
    topics.sort();

    for topic in topics {
        let Ok(topic_store) = DocStore::open_topic(&ctx.project_root, kind, &topic) else {
            continue;
        };
        let Ok(topic_entries) = topic_store.load() else {
            continue;
        };
        closed += topic_entries
            .iter()
            .filter(|entry| kind.terminal.contains(&entry.status.as_str()))
            .count();
        closed += topic_store
            .load_archive()
            .map_or(0, |archive| archive.len());
        indexed.extend(
            topic_entries
                .into_iter()
                .map(|entry| (Some(topic.clone()), entry)),
        );
    }

    Some((indexed, closed))
}

/// 文档索引:非终态条目一行一个,预算封顶。
pub(crate) fn index_of(
    ctx: &ResolveCtx,
    kind: &'static crate::docstore::DocKind,
    label: &str,
) -> Option<String> {
    const INDEX_LIMIT: usize = 500;
    let (entries, closed) = load_index_entries(ctx, kind)?;
    if entries.is_empty() {
        return None;
    }
    let open: Vec<&IndexedEntry> = entries
        .iter()
        .filter(|(_, entry)| !kind.terminal.contains(&entry.status.as_str()))
        .collect();
    let mut lines: Vec<String> = open
        .iter()
        .take(INDEX_LIMIT)
        .map(|(topic, entry)| {
            let sev = entry
                .severity
                .as_ref()
                .map(|s| format!("/{s}"))
                .unwrap_or_default();
            let topic = topic
                .as_deref()
                .map(|topic| format!(" (topic: {topic})"))
                .unwrap_or_default();
            format!(
                "{} [{}{sev}] {}{topic}",
                entry.id, entry.status, entry.title
            )
        })
        .collect();
    if open.len() > INDEX_LIMIT {
        let folded: Vec<&str> = open
            .iter()
            .skip(INDEX_LIMIT)
            .map(|(_, entry)| entry.id.as_str())
            .collect();
        lines.push(format!(
            "… +{} more open ({}), read `{}` or use the tracker list tool for the full queue",
            open.len() - INDEX_LIMIT,
            folded.join(", "),
            kind.rel_path
        ));
    }
    Some(format!(
        "{label} ({} open, {closed} closed):\n{}",
        open.len(),
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::index_of;
    use crate::docstore::{DocStore, Entry, FINDINGS, SOURCES};
    use kanzei_harness::{KanzeiConfig, ProfileKind, ResolveCtx};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "kz-research-index-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn entry(id: &str, title: &str, status: &str) -> Entry {
        Entry {
            id: id.into(),
            title: title.into(),
            status: status.into(),
            severity: None,
            fields: vec![],
        }
    }

    fn ctx(root: PathBuf) -> ResolveCtx {
        ResolveCtx {
            profile: ProfileKind::Research,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        }
    }

    #[test]
    fn research_index_聚合flat遗留和topic并能回读同会话新条目() {
        let root = test_root("sources");
        let flat = DocStore::open(&root, &SOURCES);
        flat.save(&[entry("S-001", "flat legacy source", "active")])
            .unwrap();
        let topic = DocStore::open_topic(&root, &SOURCES, "r221-chain").unwrap();
        topic
            .save(&[entry("S-001", "topic source", "active")])
            .unwrap();

        let first = index_of(&ctx(root.clone()), &SOURCES, "Sources").unwrap();
        assert!(first.contains("flat legacy source"));
        assert!(first.contains("topic source (topic: r221-chain)"));

        topic
            .save(&[
                entry("S-001", "topic source", "active"),
                entry("S-002", "new same-session source", "active"),
            ])
            .unwrap();
        let second = index_of(&ctx(root.clone()), &SOURCES, "Sources").unwrap();
        assert!(second.contains("new same-session source (topic: r221-chain)"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn research_index_同时覆盖topic_findings() {
        let root = test_root("findings");
        let topic = DocStore::open_topic(&root, &FINDINGS, "r221-chain").unwrap();
        topic
            .save(&[entry("F-001", "topic finding", "draft")])
            .unwrap();
        let index = index_of(&ctx(root.clone()), &FINDINGS, "Findings").unwrap();
        assert!(index.contains("topic finding (topic: r221-chain)"));
        std::fs::remove_dir_all(root).ok();
    }
}
