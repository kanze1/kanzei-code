//! 记忆图谱投影的规则测试(docs/design/memory_knowledge_graph.md §3)。

use std::path::{Path, PathBuf};

use kanzei_harness::areas::AreaRegistry;

use super::memory_graph::*;
use crate::memory::MemoryEntry;

fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kz-refgraph-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// 最小 workspace:tools(edit/tracker/bash 模块)、memory(memory 模块)、app(ui 前端目录)+ scripts。
fn registry() -> (PathBuf, AreaRegistry) {
    let root = temp_root("reg");
    write(&root, "Cargo.toml", "[workspace]\nmembers = [\"crates/kanzei-tools\", \"crates/kanzei-memory\", \"crates/kanzei-app\"]\n");
    write(
        &root,
        "crates/kanzei-memory/Cargo.toml",
        "[package]\nname = \"kanzei-memory\"\n",
    );
    write(&root, "crates/kanzei-memory/src/memory/mod.rs", "");
    write(
        &root,
        "crates/kanzei-tools/Cargo.toml",
        "[package]\nname = \"kanzei-tools\"\n[dependencies]\nkanzei-memory.workspace = true\n",
    );
    for module in ["edit", "tracker", "bash", "worktree", "git"] {
        write(&root, &format!("crates/kanzei-tools/src/{module}.rs"), "");
    }
    write(
        &root,
        "crates/kanzei-app/Cargo.toml",
        "[package]\nname = \"kanzei-app\"\n[dependencies]\nkanzei-tools.workspace = true\n",
    );
    write(&root, "crates/kanzei-app/src/main.rs", "");
    for file in ["01-core.js", "13-memory.js", "style.css"] {
        write(&root, &format!("crates/kanzei-app/ui/{file}"), "");
    }
    for file in ["verify.ps1", "a.mjs", "b.mjs"] {
        write(&root, &format!("scripts/{file}"), "");
    }
    let registry = AreaRegistry::scan(&root);
    (root, registry)
}

fn memory(id: &str, title: &str, body: &str, extras: &[(&str, &str)]) -> MemoryInput {
    MemoryInput {
        entry: MemoryEntry {
            id: id.into(),
            scope: "project".into(),
            category: "fact".into(),
            title: title.into(),
            description: format!("{title} 的钩子"),
            status: "active".into(),
            created: "2026-09-01".into(),
            updated: "2026-09-01".into(),
            source: "user".into(),
            extras: extras
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            body: body.into(),
        },
        scope: "project".into(),
        archived: false,
        hits: 0,
        path: format!(".kanzei/memory/{id}.md"),
        raw: None,
    }
}

fn tracker(id: &str, text: &str) -> TrackerInput {
    TrackerInput {
        id: id.into(),
        kind: match id.as_bytes()[0] {
            b'R' => "requirement",
            b'D' => "defect",
            _ => "decision",
        },
        title: format!("{id} 标题"),
        status: "open".into(),
        archived: false,
        text: text.into(),
    }
}

fn node<'a>(graph: &'a MemoryGraph, id: &str) -> &'a GraphNode {
    graph
        .nodes
        .iter()
        .find(|n| n.id == id)
        .unwrap_or_else(|| panic!("缺节点 {id}"))
}

fn edges<'a>(graph: &'a MemoryGraph, source: &str, rel: &str) -> Vec<&'a GraphEdge> {
    graph
        .edges
        .iter()
        .filter(|e| e.source == source && e.rel == rel)
        .collect()
}

#[test]
fn area_priority_field_path_tool_via_keyword() {
    let (root, registry) = registry();
    let inputs = GraphInputs {
        memories: vec![memory(
            "M-001",
            "edit 报 old_string not found",
            "[fp:edit|old_string not found] 改 crates/kanzei-tools/src/bash.rs 时;worktree 场景同理",
            &[("area", "kanzei-tools/tracker"), ("refs", "D-010")],
        )],
        tracker: vec![tracker("D-010", "实现: crates/kanzei-tools/src/git.rs")],
        design_docs: vec![],
        registry,
    };
    let graph = build_graph(&inputs);
    let m = node(&graph, "M-001");
    assert_eq!(
        m.primary_area.as_deref(),
        Some("area:kanzei-tools/tracker"),
        "field 优先"
    );
    assert_eq!(m.area_provenance.as_deref(), Some("field"));
    assert_eq!(
        m.areas.len(),
        MAX_AREAS_PER_MEMORY,
        "最多 4 个区域: {:?}",
        m.areas
    );
    assert_eq!(m.areas[1], "area:kanzei-tools/bash", "第二档是正文路径");
    let about: Vec<(&str, &str, Option<&str>, Option<&str>)> = edges(&graph, "M-001", "about")
        .iter()
        .map(|e| {
            (
                e.target.as_str(),
                e.strength,
                e.provenance,
                e.via.as_deref(),
            )
        })
        .collect();
    assert!(
        about.contains(&("area:kanzei-tools/tracker", "strong", Some("field"), None)),
        "{about:?}"
    );
    assert!(
        about.contains(&("area:kanzei-tools/bash", "strong", Some("path"), None)),
        "{about:?}"
    );
    assert!(
        about.contains(&("area:kanzei-tools/edit", "weak", Some("tool"), None)),
        "{about:?}"
    );
    assert!(
        about.contains(&("area:kanzei-tools/git", "weak", Some("via"), Some("D-010"))),
        "{about:?}"
    );
    assert!(
        !about.iter().any(|a| a.0 == "area:kanzei-tools/worktree"),
        "第 5 档关键词被 4 个上限截掉: {about:?}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn scripts_demoted_and_via_only_from_structured_refs() {
    let (root, registry) = registry();
    let inputs = GraphInputs {
        memories: vec![memory(
            "M-002",
            "发版前先跑门禁",
            "跑 scripts/verify.ps1;见 D-011 的说明。edit 相关见 `edit`",
            &[],
        )],
        tracker: vec![tracker("D-011", "改 crates/kanzei-tools/src/git.rs")],
        design_docs: vec![],
        registry,
    };
    let graph = build_graph(&inputs);
    let m = node(&graph, "M-002");
    assert_eq!(
        m.primary_area.as_deref(),
        Some("area:kanzei-tools/edit"),
        "scripts 路径不当 primary: {:?}",
        m.areas
    );
    assert!(
        m.areas.contains(&"area:scripts".to_string()),
        "scripts 仍作为次要区域: {:?}",
        m.areas
    );
    assert!(
        !m.areas.contains(&"area:kanzei-tools/git".to_string()),
        "正文提及的 D-011 不产生 via: {:?}",
        m.areas
    );
    assert_eq!(
        edges(&graph, "M-002", "mentions").len(),
        1,
        "D-011 记为提及弱边"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn supersedes_dedup_both_directions() {
    let (root, registry) = registry();
    let inputs = GraphInputs {
        memories: vec![
            memory("M-010", "新版", "取代旧版", &[("supersedes", "M-011")]),
            memory("M-011", "旧版", "被取代", &[("superseded_by", "M-010")]),
        ],
        tracker: vec![],
        design_docs: vec![],
        registry,
    };
    let graph = build_graph(&inputs);
    let sup: Vec<&GraphEdge> = graph
        .edges
        .iter()
        .filter(|e| e.rel == "supersedes")
        .collect();
    assert_eq!(sup.len(), 1, "{sup:?}");
    assert_eq!(
        (sup[0].source.as_str(), sup[0].target.as_str()),
        ("M-010", "M-011")
    );
    assert!(
        graph.edges.iter().all(|e| e.rel != "mentions"),
        "结构化 supersedes 不再重复记提及"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn shared_concepts_kept_singletons_dropped() {
    let (root, registry) = registry();
    let inputs = GraphInputs {
        memories: vec![
            memory("M-020", "a", "[fp:bash|error[E0433]]", &[]),
            memory(
                "M-021",
                "b",
                "[fp:bash|error[E0433]] [fp:git|push rejected]",
                &[("subject", "安装通道")],
            ),
            memory("M-022", "c", "x", &[("subject", "安装通道")]),
        ],
        tracker: vec![],
        design_docs: vec![],
        registry,
    };
    let graph = build_graph(&inputs);
    let concepts: Vec<(&str, &str)> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == "fingerprint" || n.kind == "subject")
        .map(|n| (n.id.as_str(), n.kind))
        .collect();
    assert_eq!(concepts.len(), 2, "{concepts:?}");
    assert!(concepts
        .iter()
        .any(|(id, kind)| id.starts_with("fp:bash|error[E0433") && *kind == "fingerprint"));
    assert!(concepts.contains(&("subject:安装通道", "subject")));
    assert!(
        !concepts.iter().any(|(id, _)| id.contains("push rejected")),
        "单例指纹丢弃"
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|e| e.rel == "has_fingerprint")
            .count(),
        2
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|e| e.rel == "has_subject")
            .count(),
        2
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn active_wins_over_archived_same_id_and_archived_flag_set() {
    let (root, registry) = registry();
    let mut archived_twin = memory("M-030", "归档版", "旧", &[]);
    archived_twin.archived = true;
    let mut archived_only = memory("M-031", "只在归档", "旧", &[]);
    archived_only.archived = true;
    archived_only.entry.status = "deprecated".into();
    let inputs = GraphInputs {
        memories: vec![
            archived_twin,
            memory("M-030", "活动版", "新", &[]),
            archived_only,
        ],
        tracker: vec![],
        design_docs: vec![],
        registry,
    };
    let graph = build_graph(&inputs);
    assert_eq!(node(&graph, "M-030").title, "活动版");
    assert!(!node(&graph, "M-030").archived);
    assert!(node(&graph, "M-031").archived);
    assert_eq!(
        (graph.stats.memories, graph.stats.live, graph.stats.archived),
        (2, 1, 1)
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn dangling_refs_go_to_warnings_not_edges() {
    let (root, registry) = registry();
    let inputs = GraphInputs {
        memories: vec![memory(
            "M-040",
            "悬空",
            "见 [[不存在的标题]]",
            &[(
                "refs",
                "R-999 M-998 docs/design/nope.md D-012 依据:docs/design/ok.md",
            )],
        )],
        tracker: vec![tracker("D-012", "")],
        design_docs: vec!["ok.md".into()],
        registry,
    };
    let graph = build_graph(&inputs);
    let out: Vec<(&str, &str)> = edges(&graph, "M-040", "refs")
        .iter()
        .chain(edges(&graph, "M-040", "basis").iter())
        .map(|e| (e.target.as_str(), e.rel))
        .collect();
    assert_eq!(out, vec![("D-012", "refs"), ("doc:ok.md", "basis")]);
    for needle in ["R-999", "M-998", "nope.md", "[[不存在的标题]]"] {
        assert!(
            graph.warnings.iter().any(|w| w.contains(needle)),
            "警告缺 {needle}: {:?}",
            graph.warnings
        );
    }
    assert!(graph.edges.iter().all(|e| e.target != "R-999"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn unused_modules_not_emitted_but_listed_in_areas() {
    let (root, registry) = registry();
    let inputs = GraphInputs {
        memories: vec![memory("M-050", "edit 报错", "", &[])],
        tracker: vec![],
        design_docs: vec![],
        registry,
    };
    let graph = build_graph(&inputs);
    let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains(&"area:kanzei-tools/edit"));
    assert!(
        !ids.contains(&"area:kanzei-tools/git"),
        "没有记忆的模块不出节点: {ids:?}"
    );
    for crate_id in [
        "area:kanzei-tools",
        "area:kanzei-memory",
        "area:kanzei-app",
        "area:kanzei-app/ui",
        "area:scripts",
    ] {
        assert!(
            ids.contains(&crate_id),
            "crate 级区域总是出节点:缺 {crate_id}"
        );
    }
    assert!(
        graph.areas.iter().any(|a| a.id == "area:kanzei-tools/git"),
        "areas 仍给全量(设为区域下拉用)"
    );
    let tools_area = graph
        .areas
        .iter()
        .find(|a| a.id == "area:kanzei-tools")
        .unwrap();
    assert_eq!(tools_area.memories, 1);
    assert!(graph.edges.iter().any(|e| e.rel == "contains"
        && e.source == "area:kanzei-tools"
        && e.target == "area:kanzei-tools/edit"));
    assert!(graph.edges.iter().any(|e| e.rel == "contains"
        && e.source == "area:kanzei-app"
        && e.target == "area:kanzei-app/ui"));
    assert!(graph.edges.iter().any(|e| e.rel == "depends_on"
        && e.source == "area:kanzei-tools"
        && e.target == "area:kanzei-memory"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn node_shape_uniform() {
    let (root, registry) = registry();
    let inputs = GraphInputs {
        memories: vec![
            memory(
                "M-060",
                "edit 报错",
                "[fp:edit|x] 见 R-060",
                &[("refs", "docs/design/a.md")],
            ),
            memory("M-061", "edit 另一条", "[fp:edit|x]", &[]),
        ],
        tracker: vec![tracker("R-060", "")],
        design_docs: vec!["a.md".into()],
        registry,
    };
    let graph = build_graph(&inputs);
    let value = serde_json::to_value(&graph).unwrap();
    let key_sets: std::collections::BTreeSet<Vec<String>> = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_object().unwrap().keys().cloned().collect())
        .collect();
    assert_eq!(
        key_sets.len(),
        1,
        "所有节点序列化后键集合必须相同: {key_sets:?}"
    );
    let kinds: std::collections::BTreeSet<&str> = graph.nodes.iter().map(|n| n.kind).collect();
    for kind in [
        "memory",
        "requirement",
        "doc",
        "crate",
        "module",
        "fingerprint",
    ] {
        assert!(kinds.contains(kind), "取样缺 {kind}: {kinds:?}");
    }
    let edge_keys: std::collections::BTreeSet<Vec<String>> = value["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_object().unwrap().keys().cloned().collect())
        .collect();
    assert_eq!(edge_keys.len(), 1);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn collect_inputs_reads_live_archive_tracker_and_docs() {
    let (root, _) = registry();
    write(&root, ".kanzei/memory/M-070-live.md", "---\nid: M-070\nscope: project\ncategory: sop\ntitle: 活动\ndescription: d\nstatus: active\ncreated: x\nupdated: x\nsource: user\nrefs: R-070\n---\n\n正文 crates/kanzei-tools/src/edit.rs\n");
    write(&root, ".kanzei/memory/archive/M-071-old.md", "---\nid: M-071\nscope: project\ncategory: fact\ntitle: 归档\ndescription: d\nstatus: deprecated\ncreated: x\nupdated: x\nsource: user\n---\n\n旧\n");
    write(
        &root,
        ".kanzei/project/requirements.md",
        "# Requirements\n\n## R-070 一条需求 [todo]\n- 优先级: P1\n",
    );
    write(&root, "docs/design/x.md", "# x\n");
    let before = input_fingerprint(&root);
    let inputs = collect_inputs(&root);
    assert!(inputs
        .memories
        .iter()
        .any(|m| m.entry.id == "M-070" && !m.archived && m.path == ".kanzei/memory/M-070-live.md"));
    assert!(inputs
        .memories
        .iter()
        .any(|m| m.entry.id == "M-071" && m.archived));
    assert!(inputs
        .tracker
        .iter()
        .any(|t| t.id == "R-070" && t.kind == "requirement"));
    assert_eq!(inputs.design_docs, vec!["x.md".to_string()]);
    let graph = build_graph(&inputs);
    let refs = edges(&graph, "M-070", "refs");
    assert_eq!(refs.len(), 1);
    assert_eq!(
        refs[0].anchor.as_deref(),
        Some(".kanzei/memory/M-070-live.md:11"),
        "锚点按原文行号"
    );
    assert_eq!(input_fingerprint(&root), before, "未改输入时指纹稳定");
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(
        &root,
        ".kanzei/memory/M-070-live.md",
        "---\nid: M-070\ntitle: 改了\n---\n\n改了正文,长度也变了\n",
    );
    assert_ne!(input_fingerprint(&root), before, "记忆文件变了指纹必须变");
    std::fs::remove_dir_all(root).ok();
}

/// 取样钩子(默认跳过):`KZ_MEMORY_GRAPH_DUMP=<输出.json> cargo test -p kanzei-tools refgraph::tests::dump_repo_graph`
/// 用本仓真实记忆跑一遍投影并写出 JSON,供预览夹具(scripts/ui-preview/fixtures.mjs 的 memory_graph)取子集。
#[test]
fn dump_repo_graph() {
    let Ok(out) = std::env::var("KZ_MEMORY_GRAPH_DUMP") else {
        return;
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let started = std::time::Instant::now();
    let inputs = collect_inputs(&root);
    let collected = started.elapsed();
    let graph = build_graph(&inputs);
    let built = started.elapsed() - collected;
    let mut kinds = std::collections::BTreeMap::new();
    for node in &graph.nodes {
        *kinds.entry(node.kind).or_insert(0) += 1;
    }
    let mut rels = std::collections::BTreeMap::new();
    for edge in &graph.edges {
        *rels
            .entry(format!("{}/{}", edge.rel, edge.strength))
            .or_insert(0) += 1;
    }
    eprintln!(
        "collect {collected:?} build {built:?} nodes {} edges {} kinds {kinds:?} rels {rels:?} stats {:?} warnings {}",
        graph.nodes.len(),
        graph.edges.len(),
        graph.stats,
        graph.warnings.len()
    );
    std::fs::write(out, serde_json::to_string(&graph).unwrap()).unwrap();
}
