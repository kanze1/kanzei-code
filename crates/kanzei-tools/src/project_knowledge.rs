//! 项目知识的统一只读投影：结构来自当前代码树，经验来自主项目 Markdown。
//! 不创建第二份记忆库；层级包含与依赖影响分开表达，推断关联保留依据。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use kanzei_harness::areas::AreaRegistry;
use serde::{Deserialize, Serialize};

mod context;

use crate::files::{annotation_target, load_annotations, valid_directory_notes, AnnotationStore};
use crate::memory::{MemoryEntry, MemoryStore};
use crate::refgraph::memory_graph::{build_graph, collect_inputs_from, GraphInputs};

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeLink {
    pub area: String,
    pub provenance: String,
    pub strength: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeMemory {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub status: String,
    pub source: String,
    pub path: String,
    pub updated: String,
    pub revision: String,
    pub links: Vec<KnowledgeLink>,
    pub refs: Vec<String>,
    /// 显式区域已不存在；保留记忆，进入复核而非静默归入父区域。
    pub missing_areas: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgePurpose {
    pub path: String,
    pub text: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeArea {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub parent: Option<String>,
    pub tree_depth: usize,
    pub dependency_depth: u32,
    pub dependencies: Vec<String>,
    pub dependents: Vec<String>,
    pub memory_ids: Vec<String>,
    pub purposes: Vec<KnowledgePurpose>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectKnowledge {
    pub version: u32,
    pub enabled: bool,
    pub project_root: String,
    pub code_root: String,
    pub areas: Vec<KnowledgeArea>,
    pub memories: Vec<KnowledgeMemory>,
    pub unassigned: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct KnowledgeSettings {
    enabled: bool,
}

/// 项目独立启用；缺配置的轻量项目不扫描、不加载层级上下文。
pub fn enabled(project_root: &Path) -> bool {
    std::fs::read(project_root.join(".kanzei/project-knowledge.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<KnowledgeSettings>(&raw).ok())
        .is_some_and(|settings| settings.enabled)
}

pub fn set_enabled(project_root: &Path, enabled: bool) -> std::io::Result<()> {
    let path = project_root.join(".kanzei/project-knowledge.json");
    std::fs::create_dir_all(path.parent().unwrap())?;
    let _guard = kanzei_base::atomic_file::lock_exclusive(&path)?;
    kanzei_base::atomic_file::write_atomic(
        &path,
        &serde_json::to_string_pretty(&KnowledgeSettings { enabled })?,
    )
}

fn memory_kind(entry: &MemoryEntry) -> &'static str {
    let subject = entry.field("subject").unwrap_or("");
    if subject.starts_with("responsibility:") {
        return "responsibility";
    }
    if subject.starts_with("contract:") {
        return "contract";
    }
    match entry.category.as_str() {
        "preference" => "constraint",
        "habit" => "environment",
        "sop" => "procedure",
        _ if entry.fingerprint().is_some() => "pitfall",
        _ => "fact",
    }
}

fn tree_depth(registry: &AreaRegistry, id: &str) -> usize {
    let mut seen = BTreeSet::new();
    let mut current = registry.get(id).and_then(|area| area.parent.as_deref());
    while let Some(parent) = current {
        if !seen.insert(parent) {
            break;
        }
        current = registry.get(parent).and_then(|area| area.parent.as_deref());
    }
    seen.len()
}

/// 路径缺失、正文指纹改变或目录聚合来源改变的用途说明不进入当前知识。
fn purpose_map(
    code_root: &Path,
    annotations: &AnnotationStore,
    registry: &AreaRegistry,
) -> BTreeMap<String, Vec<KnowledgePurpose>> {
    let mut purposes: BTreeMap<String, Vec<KnowledgePurpose>> = BTreeMap::new();
    for (path, note) in valid_directory_notes(code_root, annotations) {
        if let Some(area) = registry.resolve_token(&path) {
            purposes.entry(area).or_default().push(KnowledgePurpose {
                path,
                text: note,
                provenance: "directory_summary".into(),
            });
        }
    }
    for (path, annotation) in &annotations.files {
        let Some(target) = annotation_target(code_root, path) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(target) else {
            continue;
        };
        if crate::files::content_hash(&bytes) != annotation.hash {
            continue;
        }
        if let Some(area) = registry.resolve_token(path) {
            purposes.entry(area).or_default().push(KnowledgePurpose {
                path: path.clone(),
                text: annotation.note.clone(),
                provenance: "file_summary".into(),
            });
        }
    }
    purposes
}

pub fn snapshot(project_root: &Path, code_root: &Path) -> ProjectKnowledge {
    if !enabled(project_root) {
        return ProjectKnowledge {
            version: 1,
            enabled: false,
            project_root: project_root.display().to_string(),
            code_root: code_root.display().to_string(),
            areas: Vec::new(),
            memories: Vec::new(),
            unassigned: Vec::new(),
            warnings: Vec::new(),
        };
    }
    let mut inputs = collect_inputs_from(project_root, &[MemoryStore::project(project_root)]);
    // `.kanzei` 是主项目资产；结构和用途指纹必须针对实际工作的代码树。
    inputs.registry = AreaRegistry::scan(code_root);
    inputs.memories.retain(|memory| !memory.archived);
    let purposes = purpose_map(code_root, &load_annotations(project_root), &inputs.registry);
    project(&inputs, purposes, project_root, code_root)
}

fn project(
    inputs: &GraphInputs,
    mut purposes: BTreeMap<String, Vec<KnowledgePurpose>>,
    project_root: &Path,
    code_root: &Path,
) -> ProjectKnowledge {
    let graph = build_graph(inputs);
    let mut memories = Vec::new();
    for input in &inputs.memories {
        let entry = &input.entry;
        if input.archived || !matches!(entry.status.as_str(), "active" | "candidate" | "shadow") {
            continue;
        }
        let links = graph
            .edges
            .iter()
            .filter(|edge| edge.source == entry.id && edge.rel == "about")
            .map(|edge| KnowledgeLink {
                area: edge.target.trim_start_matches("area:").into(),
                provenance: edge.provenance.unwrap_or("unknown").into(),
                strength: edge.strength.into(),
            })
            .collect();
        let missing_areas = entry
            .areas()
            .into_iter()
            .filter(|area| inputs.registry.resolve_token(area).is_none())
            .collect();
        memories.push(KnowledgeMemory {
            id: entry.id.clone(),
            title: entry.title.clone(),
            description: entry.description.clone(),
            kind: memory_kind(entry).into(),
            status: entry.status.clone(),
            source: entry.source.clone(),
            path: input.path.clone(),
            updated: entry.updated.clone(),
            links,
            revision: crate::content_hash(input.raw.as_deref().unwrap_or("").as_bytes()),
            refs: entry.refs(),
            missing_areas,
        });
    }
    memories.sort_by(|a, b| a.id.cmp(&b.id));
    let deps = inputs.registry.crate_deps();
    let areas = inputs
        .registry
        .all()
        .iter()
        .map(|area| KnowledgeArea {
            id: area.id.clone(),
            label: area.label.clone(),
            kind: area.kind.as_str().into(),
            parent: area.parent.clone(),
            tree_depth: tree_depth(&inputs.registry, &area.id),
            dependency_depth: area.depth,
            dependencies: deps
                .iter()
                .filter(|(source, _)| source == &area.id)
                .map(|(_, target)| target.clone())
                .collect(),
            dependents: deps
                .iter()
                .filter(|(_, target)| target == &area.id)
                .map(|(source, _)| source.clone())
                .collect(),
            memory_ids: memories
                .iter()
                .filter(|memory| memory.links.iter().any(|link| link.area == area.id))
                .map(|memory| memory.id.clone())
                .collect(),
            purposes: purposes.remove(&area.id).unwrap_or_default(),
        })
        .collect();
    ProjectKnowledge {
        version: 1,
        enabled: true,
        project_root: project_root.display().to_string(),
        code_root: code_root.display().to_string(),
        unassigned: memories
            .iter()
            .filter(|memory| memory.links.is_empty())
            .map(|memory| memory.id.clone())
            .collect(),
        areas,
        memories,
        warnings: graph.warnings,
    }
}

impl ProjectKnowledge {
    /// 邻域只展开边界知识；依赖某个 crate 不等于需要它每个内部模块的所有经验。
    fn boundary_memory(memory: &KnowledgeMemory) -> bool {
        matches!(
            memory.kind.as_str(),
            "responsibility" | "contract" | "constraint"
        )
    }

    fn relevant_link(
        &self,
        link: &KnowledgeLink,
        selected: &BTreeSet<String>,
        target: &str,
        boundary: bool,
    ) -> bool {
        selected.contains(&link.area)
            || (boundary
                && selected.iter().any(|area| {
                    // 祖先的其他子模块不是影响邻居；不能据此把同 crate 的所有约束装进来。
                    area != target
                        && !target.starts_with(&format!("{area}/"))
                        && link.area.starts_with(&format!("{area}/"))
                }))
    }

    pub fn resolve_area(&self, token: &str) -> Option<String> {
        AreaRegistry::scan(Path::new(&self.code_root)).resolve_token(token)
    }

    /// 层级祖先 + 目标子树 + 一跳上下游。依赖不充当父子关系，也不递归加载整仓。
    pub fn neighborhood(&self, id: &str) -> BTreeSet<String> {
        let mut selected = BTreeSet::from([id.to_string()]);
        let mut cursor = self
            .areas
            .iter()
            .find(|area| area.id == id)
            .and_then(|area| area.parent.as_deref());
        while let Some(parent) = cursor {
            if !selected.insert(parent.into()) {
                break;
            }
            cursor = self
                .areas
                .iter()
                .find(|area| area.id == parent)
                .and_then(|area| area.parent.as_deref());
        }
        for area in &self.areas {
            if area.id.starts_with(&format!("{id}/")) {
                selected.insert(area.id.clone());
            }
        }
        let base = selected.clone();
        for area in self.areas.iter().filter(|area| base.contains(&area.id)) {
            selected.extend(area.dependencies.iter().chain(&area.dependents).cloned());
        }
        selected
    }

    /// Baseline 只注入分层目录与计数；正文通过 architecture context 按需展开。
    pub fn overview(&self, budget: usize) -> String {
        if !self.enabled {
            return String::new();
        }
        let mut out = String::from(
            "<project-knowledge>\n项目结构与依赖由当前代码树生成。经验正文按需加载。\n",
        );
        let mut folded = 0;
        for area in &self.areas {
            // 概览只列项目的第一层；模块在请求上下文时展开。
            if area.parent.is_some() {
                continue;
            }
            let count = self
                .memories
                .iter()
                .filter(|memory| {
                    memory.status == "active"
                        && memory.links.iter().any(|link| {
                            link.area == area.id || link.area.starts_with(&format!("{}/", area.id))
                        })
                })
                .count();
            let line = format!(
                "{} · {} 条有效经验 · 依赖: {}\n",
                area.id,
                count,
                area.dependencies.join(", ")
            );
            if out.chars().count() + line.chars().count() > budget {
                folded += 1;
                continue;
            }
            out.push_str(&line);
        }
        out.push_str(&format!(
            "未归类经验: {}；未列出区域: {folded}。\n",
            self.unassigned.len()
        ));
        out.push_str("修改前用 architecture action=context, area=模块或文件路径，加载职责、约束、上下游与踩坑。推断关联仅供探索；候选和失效经验不作为既定约束。新经验用 memory_note 附 area 与 refs；任务进度留在需求/缺陷。\n</project-knowledge>");
        out
    }

    pub fn context(
        &self,
        area: Option<&str>,
        query: Option<&str>,
        budget: usize,
    ) -> Result<String, String> {
        context::render(self, area, query, budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "kz-knowledge-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).unwrap();
            set_enabled(&root, true).unwrap();
            let fixture = Self(root);
            fixture.write(
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/base\", \"crates/app\", \"crates/other\"]\n",
            );
            fixture.write(
                "crates/base/Cargo.toml",
                "[package]\nname = \"base\"\nversion = \"0.1.0\"\n",
            );
            fixture.write("crates/app/Cargo.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nbase = { path = \"../base\" }\n");
            fixture.write(
                "crates/other/Cargo.toml",
                "[package]\nname = \"other\"\nversion = \"0.1.0\"\n",
            );
            fixture.write("crates/base/src/network/decode.rs", "pub fn decode() {}\n");
            fixture.write("crates/base/src/sibling.rs", "pub fn sibling() {}\n");
            fixture.write("crates/app/src/lib.rs", "// app\n");
            fixture.write("crates/other/src/lib.rs", "// unrelated\n");
            fixture.memory(
                "M-001",
                "active",
                "base/network/decode",
                "[fp:read|bad] decode pitfall",
            );
            fixture.memory(
                "M-002",
                "candidate",
                "base/network/decode",
                "candidate must not load",
            );
            fixture.memory("M-003", "active", "base/sibling", "sibling must not load");
            fixture.memory("M-004", "active", "other", "other must not load");
            fixture.memory("M-005", "active", "base/missing", "orphan must not load");
            fixture
        }
        fn write(&self, path: &str, text: &str) {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        fn memory(&self, id: &str, status: &str, area: &str, body: &str) {
            self.write(&format!(".kanzei/memory/{id}-entry.md"), &format!(
                "---\nid: {id}\nscope: project\ncategory: fact\ntitle: entry {id}\ndescription: when decoding\nstatus: {status}\ncreated: 2026-10-06\nupdated: 2026-10-06\nsource: user\narea: {area}\n---\n{body}\n"));
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn project_switch_defaults_off_and_preserves_memories() {
        let fixture = Fixture::new();
        let path = fixture.0.join(".kanzei/project-knowledge.json");
        std::fs::remove_file(&path).unwrap();
        let off = snapshot(&fixture.0, &fixture.0);
        assert!(!off.enabled);
        assert!(off.areas.is_empty() && off.memories.is_empty());
        assert!(off.overview(1000).is_empty());
        assert!(off.context(None, None, 1000).is_err());
        set_enabled(&fixture.0, true).unwrap();
        let on = snapshot(&fixture.0, &fixture.0);
        assert!(on.enabled && !on.areas.is_empty() && !on.memories.is_empty());
        set_enabled(&fixture.0, false).unwrap();
        assert!(!snapshot(&fixture.0, &fixture.0).enabled);
        set_enabled(&fixture.0, true).unwrap();
        assert_eq!(
            on.memories.len(),
            snapshot(&fixture.0, &fixture.0).memories.len()
        );
        fixture.write(".kanzei/project-knowledge.json", "broken settings");
        assert!(!enabled(&fixture.0));
    }

    #[test]
    fn hierarchy_is_distinct_from_dependencies_and_context_is_local() {
        let fixture = Fixture::new();
        let knowledge = snapshot(&fixture.0, &fixture.0);
        let area = knowledge
            .areas
            .iter()
            .find(|area| area.id == "base/network/decode")
            .unwrap();
        assert_eq!(area.parent.as_deref(), Some("base/network"));
        assert_eq!(area.tree_depth, 2);
        assert_eq!(area.dependency_depth, 0);
        let neighbor = knowledge.neighborhood(&area.id);
        assert!(neighbor.contains("app"), "下游影响必须可见");
        assert!(!neighbor.contains("other"));
        assert!(
            !neighbor.contains("base/sibling"),
            "父区域不应展开所有兄弟模块"
        );
        let context = knowledge
            .context(Some("crates/base/src/network/decode.rs"), None, 6000)
            .unwrap();
        assert!(context.contains("decode pitfall"));
        for text in [
            "candidate must not load",
            "sibling must not load",
            "other must not load",
            "orphan must not load",
        ] {
            assert!(!context.contains(text), "{context}");
        }
        assert!(
            knowledge
                .memories
                .iter()
                .find(|memory| memory.id == "M-005")
                .unwrap()
                .missing_areas
                .len()
                == 1
        );
        assert!(
            !knowledge.overview(1000).contains("decode pitfall"),
            "正文不能常驻"
        );
    }

    #[test]
    fn changed_memory_is_not_injected_from_an_old_snapshot() {
        let fixture = Fixture::new();
        let knowledge = snapshot(&fixture.0, &fixture.0);
        fixture.memory("M-001", "invalid", "base/network/decode", "retired body");
        let context = knowledge
            .context(Some("base/network/decode"), None, 6000)
            .unwrap();
        assert!(!context.contains("retired body"));
        assert!(!context.contains("decode pitfall"));
    }

    #[test]
    fn dependency_interfaces_and_project_boundaries_load_without_neighbor_internal_facts() {
        let fixture = Fixture::new();
        fixture.write("crates/app/src/api/contract.rs", "// public interface\n");
        for (id, area, subject, body) in [
            (
                "M-006",
                "app/api/contract",
                "contract:app/api",
                "neighbor interface must load",
            ),
            (
                "M-007",
                "app/api/contract",
                "fact:app/api",
                "neighbor internal fact must not load",
            ),
            (
                "M-008",
                "base/sibling",
                "contract:base/sibling",
                "sibling contract must not load",
            ),
            (
                "M-009",
                "",
                "responsibility:project",
                "project responsibility must load",
            ),
        ] {
            fixture.memory(id, "active", area, body);
            let path = format!(".kanzei/memory/{id}-entry.md");
            let raw = std::fs::read_to_string(fixture.0.join(&path)).unwrap();
            fixture.write(
                &path,
                &raw.replace(
                    "source: user\n",
                    &format!("source: user\nsubject: {subject}\n"),
                ),
            );
        }
        let knowledge = snapshot(&fixture.0, &fixture.0);
        let context = knowledge
            .context(Some("base/network/decode"), None, 6000)
            .unwrap();
        assert!(
            context.contains("neighbor interface must load"),
            "{context}"
        );
        assert!(
            context.contains("app/api/contract:field:strong"),
            "必须保留邻域关联来源"
        );
        assert!(
            context.contains("project responsibility must load"),
            "{context}"
        );
        assert!(!context.contains("neighbor internal fact must not load"));
        assert!(!context.contains("sibling contract must not load"));
    }

    #[test]
    fn worktree_structure_uses_project_memories_and_validates_its_own_code() {
        let project = Fixture::new();
        let worktree = Fixture::new();
        let code_path = "crates/base/src/network/decode.rs";
        let mut annotations = AnnotationStore::default();
        annotations.files.insert(
            code_path.into(),
            crate::files::Annotation {
                hash: crate::files::content_hash(b"pub fn decode() {}\n"),
                note: "old decoder purpose".into(),
            },
        );
        crate::files::save_annotations(&project.0, &annotations).unwrap();
        worktree.write(code_path, "pub fn decode_v2() {}\n");
        worktree.write(
            "crates/app/Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        );
        let knowledge = snapshot(&project.0, &worktree.0);
        assert!(knowledge
            .areas
            .iter()
            .find(|area| area.id == "app")
            .unwrap()
            .dependencies
            .is_empty());
        assert!(!knowledge
            .areas
            .iter()
            .flat_map(|area| &area.purposes)
            .any(|purpose| purpose.text == "old decoder purpose"));
        assert!(knowledge
            .context(Some("base/network/decode"), None, 6000)
            .unwrap()
            .contains("decode pitfall"));
    }

    #[test]
    fn directory_summaries_require_matching_input_revision() {
        let fixture = Fixture::new();
        let dir = "crates/base/src/network";
        let mut annotations = AnnotationStore::default();
        annotations
            .dirs
            .insert(dir.into(), "decoder directory".into());
        assert!(
            valid_directory_notes(&fixture.0, &annotations).is_empty(),
            "旧无指纹摘要必须待复核"
        );
        annotations.dir_hashes.insert(
            dir.into(),
            crate::files::directory_fingerprint(&fixture.0, dir).unwrap(),
        );
        assert_eq!(valid_directory_notes(&fixture.0, &annotations).len(), 1);
        fixture.write("crates/base/src/network/decode.rs", "// changed\n");
        assert!(valid_directory_notes(&fixture.0, &annotations).is_empty());
        assert_eq!(annotations.dirs.len(), 1, "过期历史仍保留");
    }

    #[test]
    fn contracts_keep_body_budget_ahead_of_incidents_and_purpose_summaries() {
        let fixture = Fixture::new();
        fixture.memory(
            "M-001",
            "active",
            "base/network/decode",
            &"old incident ".repeat(300),
        );
        fixture.memory(
            "M-020",
            "active",
            "base/network/decode",
            "critical contract: verify all consumers",
        );
        let path = ".kanzei/memory/M-020-entry.md";
        let raw = std::fs::read_to_string(fixture.0.join(path)).unwrap();
        fixture.write(
            path,
            &raw.replace(
                "source: user\n",
                "source: user\nsubject: contract:decode\nrefs: crates/base/src/network/decode.rs\n",
            ),
        );
        let code_path = "crates/base/src/network/decode.rs";
        let mut annotations = AnnotationStore::default();
        annotations.files.insert(
            code_path.into(),
            crate::files::Annotation {
                hash: crate::files::content_hash(b"pub fn decode() {}\n"),
                note: "large purpose summary ".repeat(200),
            },
        );
        crate::files::save_annotations(&fixture.0, &annotations).unwrap();
        let context = snapshot(&fixture.0, &fixture.0)
            .context(Some(code_path), None, 1800)
            .unwrap();
        assert!(
            context.contains("critical contract: verify all consumers"),
            "{context}"
        );
        assert!(context.contains("refs: crates/base/src/network/decode.rs"));
        assert!(context.find("critical contract").unwrap() < context.find("M-001 entry").unwrap());
        assert!(!context.contains("old incident"));
        assert!(
            !context.contains("created:"),
            "重复 frontmatter 不应占用正文预算"
        );
        assert!(context.contains("正文未加载"));
        assert!(context.chars().count() <= 1800);
        assert!(context.ends_with("</project-context>"));
    }

    #[test]
    fn task_query_ranks_related_contracts_without_expanding_the_neighborhood() {
        let fixture = Fixture::new();
        for (id, body) in [
            ("M-020", "alpha_wire interface"),
            ("M-021", "beta_wire interface"),
        ] {
            fixture.memory(id, "active", "base/network/decode", body);
            let path = format!(".kanzei/memory/{id}-entry.md");
            let raw = std::fs::read_to_string(fixture.0.join(&path)).unwrap();
            fixture.write(
                &path,
                &raw.replace("source: user\n", "source: user\nsubject: contract:decode\n"),
            );
        }
        fixture.memory("M-004", "active", "other", "beta_wire unrelated area");
        let knowledge = snapshot(&fixture.0, &fixture.0);
        let context = knowledge
            .context(Some("base/network/decode"), Some("beta_wire"), 6000)
            .unwrap();
        assert!(
            context.find("beta_wire interface").unwrap()
                < context.find("alpha_wire interface").unwrap()
        );
        assert!(!context.contains("beta_wire unrelated area"));
        assert!(knowledge
            .context(Some("base/network/decode"), None, 0)
            .is_err());
    }

    #[test]
    fn oversized_memory_is_folded_with_a_readable_pointer() {
        let fixture = Fixture::new();
        fixture.memory(
            "M-001",
            "active",
            "base/network/decode",
            &"long memory ".repeat(2000),
        );
        let knowledge = snapshot(&fixture.0, &fixture.0);
        let context = knowledge
            .context(Some("base/network/decode"), None, 1100)
            .unwrap();
        assert!(
            context.chars().count() <= 1100,
            "{}",
            context.chars().count()
        );
        assert!(context.contains("正文未加载"));
        assert!(!context.contains("long memory"));
    }
}
