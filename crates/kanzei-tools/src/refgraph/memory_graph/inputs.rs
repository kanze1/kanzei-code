//! Source collection and freshness fingerprints for memory graph projections.
//!
//! Filesystem reads, tracker source resolution and input cache stamps live here;
//! the parent module builds the graph from the resulting immutable GraphInputs.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use kanzei_harness::areas::AreaRegistry;

use super::{GraphInputs, MemoryInput, TrackerInput};
use crate::refgraph::mentions::ids_in;
use kanzei_memory::memory::MemoryStore;

fn display_path(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) => relative.display().to_string().replace('\\', "/"),
        Err(_) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let archived = path
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|n| n == "archive");
            format!(
                "~/.kanzei/memory/{}{name}",
                if archived { "archive/" } else { "" }
            )
        }
    }
}

/// 项目库 + 全局库(桌面端与 CLI 的默认口径)。
pub fn memory_stores(root: &Path) -> Vec<MemoryStore> {
    let mut stores = vec![MemoryStore::project(root)];
    stores.extend(MemoryStore::global());
    stores
}

/// 全部 IO:读两级记忆库(含 archive/)、tracker 三类文档(含归档)、设计文档名、区域注册表。
pub fn collect_inputs(root: &Path) -> GraphInputs {
    collect_inputs_from(root, &memory_stores(root))
}

/// 同 [`collect_inputs`],记忆库由调用方给(契约测试只给项目库,不读本机全局库)。
pub fn collect_inputs_from(root: &Path, stores: &[MemoryStore]) -> GraphInputs {
    let registry = AreaRegistry::scan(root);
    let mut memories = Vec::new();
    for store in stores {
        let scope = store.scope.label().to_string();
        let hits = store.hit_profile();
        for (archived, list) in [(false, store.load_all()), (true, store.load_archived())] {
            for (path, entry) in list {
                memories.push(MemoryInput {
                    hits: hits.get(&entry.id).map(|h| h.0).unwrap_or(0),
                    raw: std::fs::read_to_string(&path).ok(),
                    path: display_path(root, &path),
                    scope: scope.clone(),
                    archived,
                    entry,
                });
            }
        }
    }
    let (tracker, collection_warnings) = match linked_tracker_inputs(root, &memories) {
        Ok(tracker) => (tracker, Vec::new()),
        Err(error) => (Vec::new(), vec![format!("关联项目条目读取失败：{error:#}")]),
    };
    let mut design_docs: Vec<String> = std::fs::read_dir(root.join("docs/design"))
        .map(|dir| {
            dir.flatten()
                .filter_map(|item| {
                    let name = item.file_name().to_string_lossy().to_string();
                    name.ends_with(".md").then_some(name)
                })
                .collect()
        })
        .unwrap_or_default();
    design_docs.sort();
    GraphInputs {
        memories,
        tracker,
        design_docs,
        registry,
        collection_warnings,
    }
}

fn linked_tracker_inputs(
    root: &Path,
    memories: &[MemoryInput],
) -> anyhow::Result<Vec<TrackerInput>> {
    use kanzei_memory::docstore::{DocStore, DECISIONS, DEFECTS, REQUIREMENTS};
    let wanted: BTreeSet<String> = memories
        .iter()
        .flat_map(|memory| {
            let raw = memory
                .raw
                .clone()
                .unwrap_or_else(|| kanzei_memory::memory::render_entry(&memory.entry));
            ids_in(&raw, "RDA")
                .into_iter()
                .map(|mention| mention.id)
                .collect::<Vec<_>>()
        })
        .collect();
    let mut tracker = Vec::new();
    for (kind, doc) in [
        ("requirement", &REQUIREMENTS),
        ("defect", &DEFECTS),
        ("decision", &DECISIONS),
    ] {
        if !wanted.iter().any(|id| match kind {
            "requirement" => id.starts_with("R-"),
            "defect" => id.starts_with("D-"),
            _ => id.starts_with("A-"),
        }) {
            continue;
        }
        let store = DocStore::open(root, doc);
        for archived in [false, true] {
            // Only unresolved references justify reading the archive for this kind.
            let pending = wanted.iter().any(|id| {
                let same_kind = match kind {
                    "requirement" => id.starts_with("R-"),
                    "defect" => id.starts_with("D-"),
                    _ => id.starts_with("A-"),
                };
                same_kind && !tracker.iter().any(|t: &TrackerInput| &t.id == id)
            });
            if !pending {
                break;
            }
            let entries = if archived {
                store.load_archive()?
            } else {
                store.load()?
            };
            for entry in entries {
                if !wanted.contains(&entry.id)
                    || tracker.iter().any(|t: &TrackerInput| t.id == entry.id)
                {
                    continue;
                }
                let text = entry
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                tracker.push(TrackerInput {
                    id: entry.id,
                    kind,
                    title: entry.title,
                    status: entry.status,
                    archived,
                    text,
                });
            }
        }
    }
    Ok(tracker)
}

/// Context projection uses one code-tree scan and live project source files.
/// It does not open hit telemetry, global memory or archived memory at all.
pub fn collect_context_inputs(
    project_root: &Path,
    code_root: &Path,
) -> anyhow::Result<GraphInputs> {
    let mut inputs = GraphInputs {
        registry: AreaRegistry::scan(code_root),
        ..Default::default()
    };
    let directory = kanzei_memory::memory::project_memory_root(project_root);
    let files = match std::fs::read_dir(directory) {
        Ok(files) => Some(files),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(files) = files {
        for file in files {
            let path = file?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let raw = std::fs::read_to_string(&path)?;
            let entry = kanzei_memory::memory::parse_entry(&raw);
            if entry.id.is_empty()
                || !matches!(entry.status.as_str(), "active" | "candidate" | "shadow")
            {
                continue;
            }
            inputs.memories.push(MemoryInput {
                entry,
                scope: "project".into(),
                archived: false,
                hits: 0,
                path: display_path(project_root, &path),
                raw: Some(raw),
            });
        }
    }
    inputs.tracker = linked_tracker_inputs(project_root, &inputs.memories)?;
    // 模型的项目知识投影不猜测仓库文档目录；设计导航由项目索引文件拥有。
    Ok(inputs)
}

fn stat_into(hasher: &mut std::collections::hash_map::DefaultHasher, path: &Path) {
    path.hash(hasher);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(hasher);
        if let Ok(modified) = meta.modified() {
            modified.hash(hasher);
        }
    } else {
        0u8.hash(hasher);
    }
}

/// 目录里的 Markdown 文件(名 + 长度 + mtime)。只看 .md:index.db / 锁文件 / WAL 是派生物,
/// 读图谱本身(命中数查询)就可能创建或改动它们,算进来缓存就永远不命中。
fn md_dir_into(hasher: &mut std::collections::hash_map::DefaultHasher, dir: &Path) {
    dir.hash(hasher);
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = read
        .flatten()
        .map(|item| item.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    paths.sort();
    for path in paths {
        stat_into(hasher, &path);
    }
}

/// 注册表相关目录:只看条目名(区域只取决于有哪些文件/子目录),Cargo.toml 另算内容戳(依赖边)。
/// 不看其余文件的 mtime:改代码不该让图谱缓存失效。
fn names_into(hasher: &mut std::collections::hash_map::DefaultHasher, dir: &Path) {
    dir.hash(hasher);
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<(String, bool)> = read
        .flatten()
        .map(|item| {
            (
                item.file_name().to_string_lossy().to_string(),
                item.file_type().map(|t| t.is_dir()).unwrap_or(false),
            )
        })
        .collect();
    names.sort();
    for (name, is_dir) in names {
        name.hash(hasher);
        is_dir.hash(hasher);
        if name.eq_ignore_ascii_case("cargo.toml") {
            stat_into(hasher, &dir.join(&name));
        }
    }
}

/// 输入指纹:任何一个输入文件增删改(长度或 mtime 变),值就变。桌面端 stat 缓存的键。
/// 不含 index.db:命中数由调用方每次现取覆盖,不让它拖垮缓存命中率。
pub fn input_fingerprint(root: &Path) -> u64 {
    input_fingerprint_from(root, &memory_stores(root))
}

/// Cache only the stores used to build this projection.
pub fn input_fingerprint_from(root: &Path, stores: &[MemoryStore]) -> u64 {
    use kanzei_memory::docstore::{DocStore, DECISIONS, DEFECTS, REQUIREMENTS};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for store in stores {
        store.root.hash(&mut hasher);
        md_dir_into(&mut hasher, &store.root);
        md_dir_into(&mut hasher, &store.root.join("archive"));
    }
    for doc in [&REQUIREMENTS, &DEFECTS, &DECISIONS] {
        let store = DocStore::open(root, doc);
        stat_into(&mut hasher, &root.join(doc.rel_path));
        stat_into(&mut hasher, &store.archive_file());
    }
    md_dir_into(&mut hasher, &root.join("docs/design"));
    for dir in AreaRegistry::scan(root).watch_dirs(root) {
        names_into(&mut hasher, &dir);
    }
    hasher.finish()
}
