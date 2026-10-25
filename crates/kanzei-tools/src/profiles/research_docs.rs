//! Bounded navigation over topic documents and the main project's backlog.
//! Source versions and entries come from one DocStore snapshot per document.

use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use kanzei_harness::{refreshing_fallible_source, ContextRequirement, HarnessDraft, ResolveCtx};
use kanzei_memory::docstore::{DocStore, Entry, DEFECTS, FINDINGS, REQUIREMENTS, SOURCES};
use kanzei_memory::memory::MEMORY_CONTEXT_BUDGET;

struct Document {
    label: &'static str,
    scope: String,
    path: PathBuf,
    revision: String,
    entries: Vec<Entry>,
}

impl Document {
    fn heading(&self) -> String {
        format!(
            "{} [{}]\nfile: {}\nrevision: {}\ncount: {}\n",
            self.label,
            self.scope,
            self.path.display(),
            self.revision,
            self.entries.len()
        )
    }
}

pub(super) fn register(draft: &mut HarnessDraft) {
    draft.context.insert(
        "research/docs",
        refreshing_fallible_source(
            "research/docs",
            ContextRequirement::Optional,
            |ctx: &ResolveCtx| {
                render(
                    &ctx.project_root,
                    &collect(&ctx.project_root)?,
                    MEMORY_CONTEXT_BUDGET,
                )
            },
        ),
    );
}

fn topics(root: &Path) -> anyhow::Result<Vec<String>> {
    let directory = root.join(".kanzei/research");
    let read = match std::fs::read_dir(&directory) {
        Ok(read) => read,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("读取研究目录 {}", directory.display()))
        }
    };
    let mut topics = Vec::new();
    for entry in read {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(topic) = entry
            .file_name()
            .to_str()
            .filter(|topic| DocStore::validate_topic(topic).is_ok())
        {
            topics.push(topic.to_string());
        }
    }
    topics.sort();
    Ok(topics)
}

fn document(
    store: DocStore,
    label: &'static str,
    scope: String,
) -> anyhow::Result<Option<Document>> {
    let Some(snapshot) = store
        .load_snapshot()
        .with_context(|| format!("读取研究导航 {}", store.path.display()))?
    else {
        return Ok(None);
    };
    let entries: Vec<_> = snapshot
        .entries
        .into_iter()
        .filter(|entry| !store.kind.terminal.contains(&entry.status.as_str()))
        .collect();
    if entries.is_empty() {
        return Ok(None);
    }
    Ok(Some(Document {
        label,
        scope,
        path: store.path,
        revision: snapshot.revision,
        entries,
    }))
}

fn collect(root: &Path) -> anyhow::Result<Vec<Document>> {
    let mut documents = Vec::new();
    for topic in topics(root)? {
        for (kind, label) in [(&SOURCES, "Sources"), (&FINDINGS, "Findings")] {
            if let Some(document) = document(
                DocStore::open_topic(root, kind, &topic)?,
                label,
                topic.clone(),
            )? {
                documents.push(document);
            }
        }
    }
    for (kind, label) in [(&REQUIREMENTS, "Requirements"), (&DEFECTS, "Defects")] {
        if let Some(document) = document(DocStore::open(root, kind), label, "project".into())? {
            documents.push(document);
        }
    }
    Ok(documents)
}

fn ending(documents: usize, entries: usize) -> String {
    if entries == 0 {
        "</research-docs>".into()
    } else {
        format!("未列出 {entries} 条，{documents} 份文档有省略；详情按需读取。\n</research-docs>")
    }
}

fn render(root: &Path, documents: &[Document], budget: usize) -> anyhow::Result<Option<String>> {
    if documents.is_empty() {
        return Ok(None);
    }
    let mut queues = BTreeMap::<&str, VecDeque<(usize, usize)>>::new();
    let mut total = 0;
    for (index, document) in documents.iter().enumerate() {
        total += document.entries.len();
        queues
            .entry(document.label)
            .or_default()
            .extend((0..document.entries.len()).map(|entry| (index, entry)));
    }
    let counts = queues
        .iter()
        .map(|(label, entries)| format!("{label}: {}", entries.len()))
        .collect::<Vec<_>>()
        .join("；");
    let mut text = format!("<research-docs>\n研究资料与项目待办的非终态记录导航；状态不代表证据已验证、当前任务或执行授权，其他记录与详情按需读取。\n研究目录: {}\n项目记录目录: {}\n{counts}\n",
        root.join(".kanzei/research").display(), root.join(".kanzei/project").display());
    let mut used = text.chars().count() + ending(documents.len(), total).chars().count();
    anyhow::ensure!(used <= budget, "研究导航预算不足以容纳目录与省略说明");
    let headings: Vec<_> = documents.iter().map(Document::heading).collect();
    let mut selected = vec![Vec::new(); documents.len()];
    // Alternate document categories so a large source library cannot consume
    // the entire navigation budget before findings and project records appear.
    loop {
        let mut advanced = false;
        for queue in queues.values_mut() {
            let Some((index, entry)) = queue.pop_front() else {
                continue;
            };
            advanced = true;
            let document = &documents[index];
            let entry = &document.entries[entry];
            let severity = entry
                .severity
                .as_ref()
                .map(|value| format!("/{value}"))
                .unwrap_or_default();
            let row = format!(
                "[{}] {} [{}{severity}] {}\n",
                document.scope, entry.id, entry.status, entry.title
            );
            let size = row.chars().count()
                + if selected[index].is_empty() {
                    headings[index].chars().count()
                } else {
                    0
                };
            if used + size <= budget {
                selected[index].push(row);
                used += size;
            }
        }
        if !advanced {
            break;
        }
    }
    let mut folded_documents = 0;
    let mut folded_entries = 0;
    for ((document, heading), selected) in documents.iter().zip(headings).zip(selected) {
        if selected.len() < document.entries.len() {
            folded_documents += 1;
            folded_entries += document.entries.len() - selected.len();
        }
        if !selected.is_empty() {
            text.push_str(&heading);
            text.extend(selected);
        }
    }
    text.push_str(&ending(folded_documents, folded_entries));
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::{Component, ContextSourceStatus, Harness, KanzeiConfig, ProfileKind};
    use std::sync::Arc;

    fn entry(id: &str, title: &str, status: &str) -> Entry {
        Entry {
            id: id.into(),
            title: title.into(),
            status: status.into(),
            severity: None,
            fields: vec![],
        }
    }

    struct Navigation;
    impl Component for Navigation {
        fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
            register(draft);
            Ok(())
        }
    }

    fn snapshot(root: &Path, cwd: &Path) -> Arc<kanzei_harness::HarnessSnapshot> {
        let mut harness = Harness::default();
        harness.add(Navigation);
        harness
            .resolve(&ResolveCtx {
                profile: ProfileKind::Research,
                cwd: cwd.into(),
                project_root: root.into(),
                config: Arc::new(KanzeiConfig::default()),
            })
            .unwrap()
    }

    #[test]
    fn empty_terminal_and_flat_documents_do_not_create_a_navigation_block() {
        let directory = kanzei_base::test_support::TestDir::new("kz-research-empty-");
        let root = directory.as_path();
        assert!(render(root, &collect(root).unwrap(), MEMORY_CONTEXT_BUDGET)
            .unwrap()
            .is_none());
        DocStore::open(root, &SOURCES)
            .save(&[entry("S-901", "FLAT_SOURCE_CANARY", "active")])
            .unwrap();
        DocStore::open_topic(root, &SOURCES, "topic-one")
            .unwrap()
            .save(&[entry("S-001", "Archived source", "archived")])
            .unwrap();
        DocStore::open(root, &REQUIREMENTS)
            .save(&[entry("R-001", "Completed request", "done")])
            .unwrap();
        std::fs::write(root.join(".kanzei/research/memory.md"), "OLD_MEMORY_CANARY").unwrap();
        let (text, sources) = snapshot(root, root)
            .refreshable_system_baseline_with_report()
            .unwrap();
        assert!(text.is_empty());
        assert_eq!(sources[0].status, ContextSourceStatus::Empty);
    }

    #[test]
    fn refresh_preserves_topic_identity_versions_and_main_project_ownership() {
        let directory = kanzei_base::test_support::TestDir::new("kz-research-refresh-");
        let root = directory.join("main");
        let tree = directory.join("worktree");
        let source = DocStore::open_topic(&root, &SOURCES, "topic-one").unwrap();
        source
            .save(&[entry("S-001", "INITIAL_SOURCE", "active")])
            .unwrap();
        DocStore::open_topic(&root, &SOURCES, "topic-two")
            .unwrap()
            .save(&[entry("S-001", "SECOND_TOPIC_SOURCE", "active")])
            .unwrap();
        DocStore::open_topic(&root, &FINDINGS, "topic-one")
            .unwrap()
            .save(&[entry("F-001", "Draft finding", "draft")])
            .unwrap();
        let requirements = DocStore::open(&root, &REQUIREMENTS);
        requirements
            .save(&[entry("R-001", "Project request", "todo")])
            .unwrap();
        DocStore::open(&root, &DEFECTS)
            .save(&[entry("D-001", "Project defect", "open")])
            .unwrap();
        DocStore::open_topic(&tree, &SOURCES, "topic-one")
            .unwrap()
            .save(&[entry("S-001", "WRONG_WORKTREE_SOURCE", "active")])
            .unwrap();
        let original_request = std::fs::read(&requirements.path).unwrap();
        let original_source = std::fs::read(&source.path).unwrap();
        let snapshot = snapshot(&root, &tree);
        let (before, _) = snapshot.refreshable_system_baseline_with_report().unwrap();
        assert!(before.contains("[topic-one] S-001 [active] INITIAL_SOURCE"));
        assert!(before.contains("[topic-two] S-001 [active] SECOND_TOPIC_SOURCE"));
        assert!(!before.contains("WRONG_WORKTREE_SOURCE"));
        assert!(before.contains(&source.path.display().to_string()));
        assert!(before.contains(&kanzei_base::content_hash(&original_source)));
        assert!(before.contains("R-001") && before.contains("D-001") && before.contains("F-001"));
        assert!(before.chars().count() <= MEMORY_CONTEXT_BUDGET);
        source
            .save(&[entry("S-001", "UPDATED_SOURCE", "active")])
            .unwrap();
        let updated = std::fs::read(&source.path).unwrap();
        let (after, _) = snapshot.refreshable_system_baseline_with_report().unwrap();
        assert!(after.contains("UPDATED_SOURCE") && !after.contains("INITIAL_SOURCE"));
        assert!(after.contains(&kanzei_base::content_hash(&updated)));
        assert!(!after.contains(&kanzei_base::content_hash(&original_source)));
        assert_eq!(std::fs::read(requirements.path).unwrap(), original_request);
        assert!(snapshot
            .stable_system_baseline_with_report()
            .unwrap()
            .0
            .is_empty());
    }

    #[test]
    fn shared_budget_keeps_whole_rows_and_reaches_each_document_category() {
        let mut sources = vec![entry(
            "S-001",
            &"OVERSIZED_TITLE_CANARY".repeat(1000),
            "active",
        )];
        sources.extend((2..=80).map(|id| entry(&format!("S-{id:03}"), "多字节来源标题", "active")));
        let document = |label, scope: &str, file: &str, entries| Document {
            label,
            scope: scope.into(),
            path: file.into(),
            revision: "source-version".into(),
            entries,
        };
        let documents = vec![
            document(
                "Sources",
                "topic-one",
                "p/.kanzei/research/topic-one/sources.md",
                sources,
            ),
            document(
                "Findings",
                "topic-one",
                "p/.kanzei/research/topic-one/findings.md",
                vec![entry("F-001", "Finding", "draft")],
            ),
            document(
                "Requirements",
                "project",
                "p/.kanzei/project/requirements.md",
                vec![entry("R-001", "Request", "todo")],
            ),
            document(
                "Defects",
                "project",
                "p/.kanzei/project/defects.md",
                vec![entry("D-001", "Defect", "open")],
            ),
        ];
        let budget = 1400;
        let text = render(Path::new("p"), &documents, budget).unwrap().unwrap();
        assert!(text.chars().count() <= budget && text.ends_with("</research-docs>"));
        assert!(!text.contains("OVERSIZED_TITLE_CANARY"));
        for id in ["S-002", "F-001", "R-001", "D-001"] {
            assert!(text.contains(id), "{id}: {text}");
        }
        assert!(text.contains("Sources: 80"));
        let listed = text.lines().filter(|line| line.starts_with('[')).count();
        assert!(text.contains(&format!("未列出 {} 条", 83 - listed)));
        assert!(render(Path::new("p"), &documents, 0).is_err());
    }

    #[test]
    fn unreadable_topic_source_is_a_failed_read_instead_of_an_empty_index() {
        let directory = kanzei_base::test_support::TestDir::new("kz-research-error-");
        let source = DocStore::open_topic(&directory, &SOURCES, "topic-one").unwrap();
        std::fs::create_dir_all(source.path.parent().unwrap()).unwrap();
        std::fs::write(&source.path, [0xff, 0xfe]).unwrap();
        let error = collect(&directory).err().expect("invalid UTF-8 must fail");
        assert!(error
            .to_string()
            .contains(&source.path.display().to_string()));
        let (text, reads) = snapshot(&directory, &directory)
            .refreshable_system_baseline_with_report()
            .unwrap();
        assert!(text.is_empty());
        assert_eq!(reads[0].status, ContextSourceStatus::Failed);
        assert!(reads[0]
            .error
            .as_ref()
            .unwrap()
            .contains(&source.path.display().to_string()));
    }
}
