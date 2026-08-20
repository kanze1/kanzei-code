use std::path::Path;
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
    draft.tools.insert(
        "websearch",
        Arc::new(crate::websearch::ResearchWebSearchTool),
    );
    draft
        .tools
        .insert("webfetch", Arc::new(crate::webfetch::ResearchWebFetchTool));
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

fn research_topics(project_root: &Path) -> Vec<String> {
    let research_root = project_root.join(".kanzei/research");
    let mut topics = std::fs::read_dir(research_root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() {
                return None;
            }
            let topic = entry.file_name().into_string().ok()?;
            DocStore::validate_topic(&topic).ok()?;
            Some(topic)
        })
        .collect::<Vec<_>>();
    topics.sort();
    topics
}

/// Source/Finding 已按 topic 分目录写入，研究上下文必须把 topic 真源与迁移前的
/// flat 遗留一起投影。scope 写进每一行，避免不同 topic 的 S-/F- 局部编号碰撞后
/// 失去归属；500 条预算在全部 scope 间共享，不会随 topic 数量无界膨胀。
fn research_index_of(
    ctx: &ResolveCtx,
    kind: &'static crate::docstore::DocKind,
    label: &str,
) -> Option<String> {
    const INDEX_LIMIT: usize = 500;
    let mut stores = vec![(
        "legacy-flat".to_string(),
        DocStore::open(&ctx.project_root, kind),
    )];
    for topic in research_topics(&ctx.project_root) {
        if let Ok(store) = DocStore::open_topic(&ctx.project_root, kind, &topic) {
            stores.push((topic, store));
        }
    }

    let mut open = Vec::new();
    let mut closed = 0usize;
    for (scope, store) in stores {
        let Ok(entries) = store.load() else {
            continue;
        };
        closed += entries
            .iter()
            .filter(|entry| kind.terminal.contains(&entry.status.as_str()))
            .count();
        closed += store.load_archive().map_or(0, |archive| archive.len());
        open.extend(
            entries
                .into_iter()
                .filter(|entry| !kind.terminal.contains(&entry.status.as_str()))
                .map(|entry| (scope.clone(), entry)),
        );
    }
    if open.is_empty() && closed == 0 {
        return None;
    }

    let mut lines = open
        .iter()
        .take(INDEX_LIMIT)
        .map(|(scope, entry)| format!("[{scope}] {} [{}] {}", entry.id, entry.status, entry.title))
        .collect::<Vec<_>>();
    if open.len() > INDEX_LIMIT {
        let folded = open
            .iter()
            .skip(INDEX_LIMIT)
            .map(|(scope, entry)| format!("{scope}/{}", entry.id))
            .collect::<Vec<_>>();
        lines.push(format!(
            "… +{} more open ({}); use the source/finding tracker with the shown topic scope",
            open.len() - INDEX_LIMIT,
            folded.join(", ")
        ));
    }
    Some(format!(
        "{label} ({} open, {closed} closed; topic + legacy-flat):\n{}",
        open.len(),
        lines.join("\n")
    ))
}

/// 文档索引:非终态条目一行一个,预算封顶。
pub(crate) fn index_of(
    ctx: &ResolveCtx,
    kind: &'static crate::docstore::DocKind,
    label: &str,
) -> Option<String> {
    if matches!(kind.prefix, "S" | "F") {
        return research_index_of(ctx, kind, label);
    }
    const INDEX_LIMIT: usize = 500;
    let store = DocStore::open(&ctx.project_root, kind);
    let entries = store.load().ok()?;
    if entries.is_empty() {
        return None;
    }
    let open: Vec<&crate::docstore::Entry> = entries
        .iter()
        .filter(|e| !kind.terminal.contains(&e.status.as_str()))
        .collect();
    // 已完成的会被移入归档文件,closed 计数要把两处都算上。
    let closed = entries.len() - open.len() + store.load_archive().map_or(0, |a| a.len());
    let mut lines: Vec<String> = open
        .iter()
        .take(INDEX_LIMIT)
        .map(|e| {
            let sev = e
                .severity
                .as_ref()
                .map(|s| format!("/{s}"))
                .unwrap_or_default();
            format!("{} [{}{sev}] {}", e.id, e.status, e.title)
        })
        .collect();
    if open.len() > INDEX_LIMIT {
        let folded: Vec<&str> = open
            .iter()
            .skip(INDEX_LIMIT)
            .map(|e| e.id.as_str())
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
