//! Saved preferences are scoped source records, separate from task recall and Agent rules.

use anyhow::Context;
use kanzei_harness::{refreshing_fallible_source, ContextRequirement, HarnessDraft, ResolveCtx};
use kanzei_memory::memory::{MemoryEntry, MemoryScope, MemoryStore, MEMORY_CONTEXT_BUDGET};
use std::path::PathBuf;

pub(crate) fn register(draft: &mut HarnessDraft) {
    draft.context.insert(
        "core/preferences",
        refreshing_fallible_source(
            "core/preferences",
            ContextRequirement::Required,
            |ctx: &ResolveCtx| {
                let mut stores = Vec::new();
                if !kanzei_harness::is_general_conversation_root(&ctx.project_root) {
                    stores.push(MemoryStore::project(&ctx.project_root));
                }
                if let Some(global) = MemoryStore::global() {
                    stores.push(global);
                }
                render(&stores)
            },
        ),
    );
}

struct Preference {
    scope: MemoryScope,
    path: PathBuf,
    entry: MemoryEntry,
}

fn render(stores: &[MemoryStore]) -> anyhow::Result<Option<String>> {
    let mut preferences = Vec::new();
    for store in stores {
        for (path, entry) in store
            .load_all_checked()
            .with_context(|| format!("读取已保存偏好 {}", store.root.display()))?
        {
            if entry.status == "active"
                && entry.category == "preference"
                && entry.areas().is_empty()
                && (store.scope == MemoryScope::Project || entry.source == "user")
            {
                preferences.push(Preference {
                    scope: store.scope,
                    path,
                    entry,
                });
            }
        }
    }
    if preferences.is_empty() {
        return Ok(None);
    }
    let roots = stores
        .iter()
        .filter(|store| {
            preferences
                .iter()
                .any(|preference| preference.scope == store.scope)
        })
        .map(|store| format!("{}: {}", store.scope.label(), store.root.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = format!(
        "<saved-preferences>\n已保存偏好，按适用情境使用；当前用户指令优先。\n来源目录：\n{roots}\n"
    );
    // Reserve the complete omission receipt and closing tag before admitting whole entries.
    let receipt = |project, global| {
        if project == 0 && global == 0 {
            "</saved-preferences>".to_string()
        } else {
            format!(
                "未载入全文：project={project}，global={global}；可按需读取来源目录中的 preference 条目。\n</saved-preferences>"
            )
        }
    };
    let total = preferences.len();
    let reserved = receipt(total, total).chars().count();
    let mut used = out.chars().count() + reserved;
    anyhow::ensure!(
        used <= MEMORY_CONTEXT_BUDGET,
        "偏好来源目录超出上下文字符预算"
    );
    let (mut project_omitted, mut global_omitted) = (0, 0);
    for preference in preferences {
        let entry = preference.entry;
        let text = format!(
            "\n{} [{}] {}\nsource={}; updated={}; file={}\n适用线索：{}\n{}\n",
            entry.id,
            preference.scope.label(),
            entry.title,
            entry.source,
            entry.updated,
            preference.path.display(),
            entry.description,
            entry.body.trim()
        );
        let size = text.chars().count();
        if used + size <= MEMORY_CONTEXT_BUDGET {
            out.push_str(&text);
            used += size;
        } else {
            match preference.scope {
                MemoryScope::Project | MemoryScope::QqFriend | MemoryScope::QqGroup => {
                    project_omitted += 1
                }
                MemoryScope::Global => global_omitted += 1,
            }
        }
    }
    out.push_str(&receipt(project_omitted, global_omitted));
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_memory::memory::render_entry;

    fn save(store: &MemoryStore, id: &str, source: &str, body: &str) -> MemoryEntry {
        std::fs::create_dir_all(&store.root).unwrap();
        let entry = MemoryEntry {
            id: id.into(),
            scope: store.scope.label().into(),
            category: "preference".into(),
            title: format!("偏好 {id}"),
            description: "适用情境".into(),
            status: "active".into(),
            created: "2026-10-24".into(),
            updated: "2026-10-24".into(),
            source: source.into(),
            extras: vec![],
            body: body.into(),
        };
        write(store, &entry);
        entry
    }

    fn write(store: &MemoryStore, entry: &MemoryEntry) {
        std::fs::write(
            store.root.join(format!("{}.md", entry.id)),
            render_entry(entry),
        )
        .unwrap();
    }

    #[test]
    fn scope_and_source_remain_distinct_and_updates_are_read_from_files() {
        let directory = kanzei_base::test_support::TestDir::new("saved-preferences");
        let project = MemoryStore::open(MemoryScope::Project, directory.join("project"));
        let global = MemoryStore::open(MemoryScope::Global, directory.join("global"));
        let mut entry = save(&project, "M-001", "run:fixture/1", "项目原偏好");
        save(&global, "M-001", "user", "用户保存的全局偏好");
        save(&global, "M-002", "run:fixture/1", "全局自动总结不得常驻");
        let mut scoped = save(&project, "M-002", "user", "模块偏好不得常驻");
        scoped.extras.push(("area".into(), "module".into()));
        write(&project, &scoped);
        let mut candidate = save(&project, "M-003", "user", "候选不得常驻");
        candidate.status = "candidate".into();
        write(&project, &candidate);
        let stores = [project, global];
        let text = render(&stores).unwrap().unwrap();
        assert!(text.contains("M-001 [project]") && text.contains("M-001 [global]"));
        assert!(text.contains("source=run:fixture/1"));
        assert!(text.contains("用户保存的全局偏好"));
        for excluded in ["全局自动总结不得常驻", "模块偏好不得常驻", "候选不得常驻"]
        {
            assert!(!text.contains(excluded));
        }
        entry.body = "项目新偏好".into();
        write(&stores[0], &entry);
        let refreshed = render(&stores).unwrap().unwrap();
        assert!(refreshed.contains("项目新偏好") && !refreshed.contains("项目原偏好"));
        entry.status = "deprecated".into();
        write(&stores[0], &entry);
        assert!(!render(&stores)
            .unwrap()
            .unwrap()
            .contains("M-001 [project]"));
    }

    #[test]
    fn budget_preserves_whole_entries_and_reports_omissions_by_scope() {
        let directory = kanzei_base::test_support::TestDir::new("preference-budget");
        let project = MemoryStore::open(MemoryScope::Project, directory.join("project"));
        save(
            &project,
            "M-001",
            "user",
            &"不可截断的规则".repeat(MEMORY_CONTEXT_BUDGET),
        );
        save(&project, "M-002", "user", "后面的短偏好仍可加载");
        let text = render(&[project]).unwrap().unwrap();
        assert!(text.chars().count() <= MEMORY_CONTEXT_BUDGET);
        assert!(!text.contains("不可截断的规则"));
        assert!(text.contains("后面的短偏好仍可加载"));
        assert!(text.contains("未载入全文：project=1，global=0"));
        assert!(text.ends_with("</saved-preferences>"));
    }

    #[test]
    fn absent_preferences_are_empty_and_read_errors_are_not_empty() {
        let directory = kanzei_base::test_support::TestDir::new("preference-errors");
        let project = MemoryStore::open(MemoryScope::Project, directory.join("project"));
        assert!(render(std::slice::from_ref(&project)).unwrap().is_none());
        std::fs::create_dir_all(&project.root).unwrap();
        std::fs::write(project.root.join("M-001.md"), [0xff, 0xfe]).unwrap();
        assert!(render(&[project]).is_err());
    }
}
