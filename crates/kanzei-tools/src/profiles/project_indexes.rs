//! 项目文档的有界背景索引。生命周期来自文档，读取不推进工作或新增授权。

use anyhow::Context as _;
use kanzei_harness::{refreshing_fallible_source, ContextRequirement, HarnessDraft, ResolveCtx};
use kanzei_memory::docstore::{DocKind, DocStore, DocumentSnapshot, DECISIONS, IDEAS};
use kanzei_memory::memory::MEMORY_CONTEXT_BUDGET;
use std::path::Path;

struct Index {
    key: &'static str,
    kind: &'static DocKind,
    status: &'static str,
    tag: &'static str,
    label: &'static str,
    note: &'static str,
}

const DECISION_INDEX: Index = Index {
    key: "development/decisions", kind: &DECISIONS, status: "accepted",
    tag: "accepted-decisions", label: "已接受决策",
    note: "已接受决策的索引，详情按需读取。适用性结合当前用户指令与后续修订判断；状态不证明实现或验收。",
};

const IDEA_INDEX: Index = Index {
    key: "development/ideas",
    kind: &IDEAS,
    status: "inbox",
    tag: "idea-inbox",
    label: "收件箱想法",
    note: "想法收件箱的背景索引，详情按需读取；收录不代表当前任务或执行授权。",
};

pub(super) fn register(draft: &mut HarnessDraft) {
    for index in [&IDEA_INDEX, &DECISION_INDEX] {
        draft.context.insert(
            index.key,
            refreshing_fallible_source(
                index.key,
                ContextRequirement::Optional,
                move |ctx: &ResolveCtx| {
                    let store = DocStore::open(&ctx.project_root, index.kind);
                    let Some(snapshot) = store
                        .load_snapshot()
                        .with_context(|| format!("读取项目索引 {}", store.path.display()))?
                    else {
                        return Ok(None);
                    };
                    render(index, &store.path, &snapshot, MEMORY_CONTEXT_BUDGET)
                },
            ),
        );
    }
}

fn render(
    index: &Index,
    path: &Path,
    snapshot: &DocumentSnapshot,
    budget: usize,
) -> anyhow::Result<Option<String>> {
    let selected: Vec<_> = snapshot
        .entries
        .iter()
        .filter(|entry| entry.status == index.status)
        .collect();
    if selected.is_empty() {
        return Ok(None);
    }
    let mut out = format!(
        "<{}>\n{}\nfile: {}\nrevision: {}\ncount: {}\n",
        index.tag,
        index.note,
        path.display(),
        snapshot.revision,
        selected.len(),
    );
    let ending = |folded| {
        if folded == 0 {
            format!("</{}>", index.tag)
        } else {
            format!(
                "预算未列出 {folded} 条{}，可按需读取源文件。\n</{}>",
                index.label, index.tag
            )
        }
    };
    let mut used = out.chars().count() + ending(selected.len()).chars().count();
    anyhow::ensure!(used <= budget, "项目文档索引预算不足以容纳来源与省略说明");
    let mut folded = 0;
    for entry in selected {
        let line = format!("{} [{}] {}\n", entry.id, entry.status, entry.title);
        let size = line.chars().count();
        if used + size > budget {
            folded += 1;
        } else {
            out.push_str(&line);
            used += size;
        }
    }
    out.push_str(&ending(folded));
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::{Component, KanzeiConfig, ProfileKind};
    use kanzei_memory::docstore::Entry;
    use std::sync::Arc;

    #[test]
    fn budget_includes_source_and_receipt_and_does_not_cut_a_decision_title() {
        let entry = |id: &str, title: &str, status: &str| Entry {
            id: id.into(),
            title: title.into(),
            status: status.into(),
            severity: None,
            fields: vec![],
        };
        let snapshot = DocumentSnapshot {
            revision: "source-version".into(),
            entries: vec![
                entry("A-001", &"不可截断的标题".repeat(1000), "accepted"),
                entry("A-002", "后面的短决策", "accepted"),
                entry("A-003", "草案不能进入接受索引", "draft"),
            ],
        };
        let text = render(
            &DECISION_INDEX,
            Path::new("project/decisions.md"),
            &snapshot,
            500,
        )
        .unwrap()
        .unwrap();
        assert!(text.chars().count() <= 500 && text.ends_with("</accepted-decisions>"));
        assert!(text.contains("A-002 [accepted] 后面的短决策"));
        assert!(!text.contains("不可截断的标题") && !text.contains("A-003"));
        assert!(text.contains("预算未列出 1 条已接受决策"));
        assert!(render(
            &DECISION_INDEX,
            Path::new("project/decisions.md"),
            &snapshot,
            0
        )
        .is_err());
    }

    #[test]
    fn inbox_budget_counts_all_matches_but_lists_whole_titles_only() {
        let snapshot = DocumentSnapshot {
            revision: "source-version".into(),
            entries: vec![
                Entry {
                    id: "I-001".into(),
                    title: "过长标题不得截断".repeat(1000),
                    status: "inbox".into(),
                    severity: None,
                    fields: vec![],
                },
                Entry {
                    id: "I-002".into(),
                    title: "短想法".into(),
                    status: "inbox".into(),
                    severity: None,
                    fields: vec![],
                },
                Entry {
                    id: "I-003".into(),
                    title: "已拆解想法".into(),
                    status: "split".into(),
                    severity: None,
                    fields: vec![],
                },
            ],
        };
        let text = render(&IDEA_INDEX, Path::new("project/ideas.md"), &snapshot, 500)
            .unwrap()
            .unwrap();
        assert!(text.chars().count() <= 500 && text.ends_with("</idea-inbox>"));
        assert!(text.contains("count: 2\n") && text.contains("I-002 [inbox] 短想法"));
        assert!(!text.contains("过长标题不得截断") && !text.contains("I-003"));
        assert!(text.contains("预算未列出 1 条收件箱想法"));
    }

    #[test]
    fn inbox_context_refreshes_project_source_without_turning_background_into_work() {
        let directory = kanzei_base::test_support::TestDir::new("idea-inbox-context");
        for profile in [ProfileKind::DevAuto, ProfileKind::DevPair] {
            let project = directory.join(profile.as_str()).join("project");
            let code = directory.join(profile.as_str()).join("worktree");
            std::fs::create_dir_all(code.join(".kanzei/project")).unwrap();
            std::fs::write(
                code.join(IDEAS.rel_path),
                "# Ideas\n\n## I-999 WRONG_WORKTREE_SOURCE [inbox]\n",
            )
            .unwrap();
            let ctx = ResolveCtx {
                profile,
                cwd: code,
                project_root: project.clone(),
                config: Arc::new(KanzeiConfig::default()),
            };
            let mut draft = HarnessDraft::default();
            crate::profiles::DevelopmentProfile
                .contribute(&mut draft, &ctx)
                .unwrap();
            let source = draft.context.get("development/ideas").unwrap();
            assert!(source.refresh_each_step());
            assert!(
                source.baseline(&ctx).unwrap().is_none(),
                "missing main-project source cannot use a worktree copy"
            );
            let mut original = "# Ideas\n\n".to_string();
            for number in 1..=30 {
                original.push_str(&format!("## I-{number:03} 想法 {number} [inbox]\n- 原文: PRIVATE_IDEA_BODY_{number}\n\n"));
            }
            original.push_str("## I-031 已拆解背景 [split]\n\n## I-032 已放弃背景 [dropped]\n");
            let path = project.join(IDEAS.rel_path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &original).unwrap();
            let text = source.baseline(&ctx).unwrap().unwrap();
            assert!(text.contains("count: 30\n") && text.contains("I-030 [inbox] 想法 30"));
            assert!(text.contains(&format!("file: {}\n", path.display())));
            assert!(text.contains(&crate::content_hash(original.as_bytes())));
            for excluded in [
                "PRIVATE_IDEA_BODY",
                "WRONG_WORKTREE_SOURCE",
                "I-031",
                "I-032",
                "待拆解",
                "录入不过模型",
            ] {
                assert!(!text.contains(excluded), "{excluded}");
            }
            assert!(text.chars().count() <= MEMORY_CONTEXT_BUDGET);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            assert!(!project
                .join(kanzei_memory::docstore::REQUIREMENTS.rel_path)
                .exists());
            assert!(!project
                .join(kanzei_memory::docstore::DEFECTS.rel_path)
                .exists());
            let revised = "# Ideas\n\n## I-001 想法 1 [split]\n\n## I-033 新想法 [inbox]\n- 原文: NEW_PRIVATE_IDEA_BODY\n";
            std::fs::write(&path, revised).unwrap();
            let refreshed = source.baseline(&ctx).unwrap().unwrap();
            assert!(refreshed.contains("count: 1\n") && refreshed.contains("I-033 [inbox] 新想法"));
            assert!(refreshed.contains(&crate::content_hash(revised.as_bytes())));
            assert!(!refreshed.contains("I-001") && !refreshed.contains("NEW_PRIVATE_IDEA_BODY"));
            std::fs::write(&path, "# Ideas\n\n## I-033 新想法 [dropped]\n").unwrap();
            assert!(source.baseline(&ctx).unwrap().is_none());
            std::fs::write(&path, [0xff, 0xfe]).unwrap();
            assert!(source.baseline(&ctx).is_err());
        }
    }
}
