//! 工具名 / 关键词 → 代码区域的受控词表(记忆图谱区域推断的第 3、5 档信号)。
//!
//! 记忆大多是「某个工具怎么用 / 怎么失败」的经验,失败指纹 `[fp:edit|…]`、标题首词
//! `edit 报 …`、反引号里的工具名都直接指向实现它的模块。新加工具忘了在这里配区域,
//! `every_registered_tool_has_area_or_exemption` 会红——要么配区域,要么进豁免表并写理由。

/// 工具名 → 区域 id(不带 `area:` 前缀)。
pub const TOOL_AREAS: &[(&str, &str)] = &[
    ("tool_search", "kanzei-harness/tool_search"),
    ("edit", "kanzei-tools/edit"),
    ("insert", "kanzei-tools/edit"),
    ("bash", "kanzei-tools/bash"),
    ("process", "kanzei-tools/process"),
    ("git", "kanzei-tools/git"),
    ("grep", "kanzei-tools/grep"),
    ("glob", "kanzei-tools/glob"),
    ("read", "kanzei-tools/read"),
    ("write", "kanzei-tools/write"),
    ("files", "kanzei-tools/files"),
    ("symbols", "kanzei-tools/symbols"),
    ("question", "kanzei-tools/question"),
    ("webfetch", "kanzei-tools/webfetch"),
    ("websearch", "kanzei-tools/websearch"),
    ("browser", "kanzei-tools/browser_tool"),
    ("latex", "kanzei-tools/latex_tool"),
    ("plot", "kanzei-tools/plot_tool"),
    ("prior_art", "kanzei-tools/prior_art"),
    ("incident", "kanzei-tools/incident"),
    ("conventions", "kanzei-tools/conventions"),
    ("architecture", "kanzei-tools/architecture"),
    ("test_record", "kanzei-tools/test_record"),
    ("work", "kanzei-tools/work"),
    ("req", "kanzei-tools/tracker"),
    ("defect", "kanzei-tools/tracker"),
    ("idea", "kanzei-tools/tracker"),
    ("decision", "kanzei-tools/tracker"),
    ("goal", "kanzei-tools/tracker"),
    ("source", "kanzei-tools/tracker"),
    ("finding", "kanzei-tools/tracker"),
    ("task", "kanzei-tools/subagent"),
    ("frontend_check", "kanzei-tools/frontend"),
    ("frontend_locate", "kanzei-tools/frontend"),
    ("research_index", "kanzei-tools/research_index"),
    ("research_loop", "kanzei-tools/research_loop"),
    ("research_plan", "kanzei-tools/research_plan"),
    ("research_runner", "kanzei-tools/research_runner"),
    ("research_verify", "kanzei-tools/research_verify"),
    ("research_workflow", "kanzei-tools/research_workflow"),
    ("research_write", "kanzei-tools/research_write"),
    ("memory_note", "kanzei-memory/memory"),
    ("memory_search", "kanzei-memory/memory"),
    ("memory_add", "kanzei-memory/memory"),
    ("memory_update", "kanzei-memory/memory"),
    ("memory_merge", "kanzei-memory/memory"),
    ("memory_stale", "kanzei-memory/memory"),
    ("memory_promote", "kanzei-memory/memory"),
    ("memory_stats", "kanzei-memory/memory"),
    ("memory_get", "kanzei-memory/memory"),
    ("memory_read", "kanzei-memory/memory"),
    ("memory_archive", "kanzei-memory/memory"),
    ("memory_inbox_clear", "kanzei-memory/memory"),
    ("memory_inbox_discard", "kanzei-memory/memory"),
    ("ui_console", "kanzei-app"),
    ("ui_dom", "kanzei-app"),
    ("ui_style", "kanzei-app"),
];

/// 注册了但不映射区域的工具(理由写在旁边;新增须同样写明)。
pub const TOOL_AREAS_EXEMPT: &[&str] = &[
    // 子代理交付回执:协议动作,不是某块代码的经验来源。
    "deliver",
];

/// 关键词 → 区域(兜底信号,weak)。ASCII 关键词按词边界、不分大小写匹配;中文按子串。
pub const KEYWORD_AREAS: &[(&[&str], &str)] = &[
    (
        &["前端", "界面", "i18n", "HTML", "CSS", "冒烟", "smoke", "UI"],
        "kanzei-app/ui",
    ),
    (
        &["runner", "上下文压缩", "compaction", "首次请求"],
        "kanzei-core/runner",
    ),
    (
        &["SSE", "限流", "provider", "context overflow"],
        "kanzei-llm",
    ),
    (
        &["权限", "permission", "ruleset"],
        "kanzei-harness/permission",
    ),
    (
        &["记忆", "INDEX.md", "refresh_derived", "inbox"],
        "kanzei-memory/memory",
    ),
    (&["发版", "verify.ps1", "package.ps1", "安装包"], "scripts"),
    (
        &["tracker", "需求", "缺陷", "批次", "停车", "取活", "backlog"],
        "kanzei-tools/tracker",
    ),
    (&["worktree", "工作树"], "kanzei-tools/worktree"),
];

pub fn tool_area(tool: &str) -> Option<&'static str> {
    TOOL_AREAS
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, area)| *area)
}

fn ascii_word_hit(haystack: &str, needle: &str) -> bool {
    let lower = haystack.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(&needle) {
        let start = from + pos;
        let end = start + needle.len();
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
        while from < lower.len() && !lower.is_char_boundary(from) {
            from += 1;
        }
    }
    false
}

/// 命中的关键词区域(按表序,去重)。
pub fn keyword_areas(text: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for (words, area) in KEYWORD_AREAS {
        let hit = words.iter().any(|word| {
            if word.is_ascii() {
                ascii_word_hit(text, word)
            } else {
                text.contains(word)
            }
        });
        if hit && !out.contains(area) {
            out.push(*area);
        }
    }
    out
}

/// 标题首词里的工具名:`edit 报 …`、`[bash] …`、`git: …`、`grep：…`。
pub fn title_lead_tool(title: &str) -> Option<&str> {
    let trimmed = title.trim_start().trim_start_matches('[');
    let end = trimmed
        .find(|c: char| !(c.is_ascii_lowercase() || c == '_'))
        .unwrap_or(trimmed.len());
    if end == 0 {
        return None;
    }
    let next = trimmed[end..].chars().next();
    matches!(next, Some(' ' | ':' | '：' | ']')).then(|| &trimmed[..end])
}

/// 反引号里的工具名(只认词表里有的)。
pub fn backticked_tools(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('`') else { break };
        let word = &tail[..end];
        if tool_area(word).is_some() && !out.contains(&word) {
            out.push(word);
        }
        rest = &tail[end + 1..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn every_registered_tool_has_area_or_exemption() {
        use kanzei_harness::{Component, HarnessDraft, KanzeiConfig, ProfileKind, ResolveCtx};
        let root = PathBuf::from("C:/kanzei-tool-areas-test");
        let mut names = std::collections::BTreeSet::new();
        for profile in [ProfileKind::Dev, ProfileKind::Research] {
            let ctx = ResolveCtx {
                profile,
                cwd: root.clone(),
                project_root: root.clone(),
                config: Arc::new(KanzeiConfig::default()),
            };
            let mut draft = HarnessDraft::default();
            crate::BaseComponent.contribute(&mut draft, &ctx).unwrap();
            crate::DevProfile.contribute(&mut draft, &ctx).unwrap();
            crate::ResearchProfile.contribute(&mut draft, &ctx).unwrap();
            crate::memory::MemoryManagerComponent
                .contribute(&mut draft, &ctx)
                .unwrap();
            names.extend(draft.tools.names().map(str::to_string));
        }
        assert!(names.len() > 15, "工具清单取样异常:{names:?}");
        let missing: Vec<&String> = names
            .iter()
            .filter(|name| tool_area(name).is_none() && !TOOL_AREAS_EXEMPT.contains(&name.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "这些工具没有配置代码区域(记忆图谱按工具名把失败经验挂到实现模块上):{missing:?}。\
             改法:在 refgraph/tool_areas.rs 的 TOOL_AREAS 里加 (工具名, 区域 id);确实不对应代码区域的,\
             加进 TOOL_AREAS_EXEMPT 并写明理由"
        );
    }

    #[test]
    fn every_tool_area_resolves_in_repo_registry() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let registry = kanzei_harness::areas::AreaRegistry::scan(&root);
        let targets = TOOL_AREAS
            .iter()
            .map(|(_, a)| *a)
            .chain(KEYWORD_AREAS.iter().map(|(_, a)| *a));
        let broken: Vec<&str> = targets.filter(|a| registry.get(a).is_none()).collect();
        assert!(
            broken.is_empty(),
            "词表里的区域在仓库注册表里不存在(模块改名或删了?):{broken:?}"
        );
    }

    #[test]
    fn keyword_and_title_signals() {
        assert_eq!(keyword_areas("provider 返回 SSE 断流"), vec!["kanzei-llm"]);
        assert!(
            keyword_areas("ASSERTION providers").is_empty(),
            "ASCII 关键词按词边界"
        );
        assert_eq!(keyword_areas("前端 i18n 冒烟"), vec!["kanzei-app/ui"]);
        assert_eq!(
            title_lead_tool("edit 报 old_string not found"),
            Some("edit")
        );
        assert_eq!(title_lead_tool("[bash] 超时"), Some("bash"));
        assert_eq!(title_lead_tool("git: 分支"), Some("git"));
        assert_eq!(title_lead_tool("Git tests 跨轮"), None);
        assert_eq!(
            backticked_tools("先 `read` 再 `edit`,`foo` 不算"),
            vec!["read", "edit"]
        );
    }
}
