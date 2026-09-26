use super::*;
use std::path::PathBuf;

fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kz-arch-diagram-{tag}-{}-{}",
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

/// 夹具工作区:五个成员,覆盖 workspace 继承、package 改名、target 依赖、dev/build 依赖、
/// 缺描述/分组、[lib] path 入口、描述里的引号/反引号与超长截断。
///
/// 普通依赖:app→core(改名 engine)、app→base(workspace)、app→util(target cfg)、
/// core→base、core→util、util→base。约简后留 app→core、core→util、util→base,隐藏 3 条。
fn fixture_workspace(tag: &str) -> PathBuf {
    let root = temp_root(tag);
    write(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\", \"extra\"]\n\n[workspace.dependencies]\nfx-base = { path = \"crates/base\" }\n",
    );
    write(
        &root,
        "crates/base/Cargo.toml",
        "[package]\nname = \"fx-base\"\ndescription = \"基础原语,含 \\\"引号\\\" 与 `反引号`\"\n\n[package.metadata.kanzei]\ngroup = \"基础\"\n",
    );
    write(&root, "crates/base/src/lib.rs", "");
    write(
        &root,
        "crates/util/Cargo.toml",
        "[package]\nname = \"fx-util\"\ndescription = \"工具函数集合,这一句描述故意写得很长很长很长很长很长很长很长很长很长很长以触发截断\"\n\n[package.metadata.kanzei]\ngroup = \"基础\"\n\n[dependencies]\nfx-base.workspace = true\n",
    );
    write(&root, "crates/util/src/lib.rs", "");
    write(
        &root,
        "crates/core/Cargo.toml",
        "[package]\nname = \"fx-core\"\ndescription = \"运行时主循环\"\n\n[package.metadata.kanzei]\ngroup = \"运行时\"\n\n[dependencies]\nfx-base.workspace = true\nfx-util = { path = \"../util\" }\nserde = \"1\"\n\n[dev-dependencies]\nfx-extra = { path = \"../../extra\" }\n\n[build-dependencies]\nfx-util = { path = \"../util\" }\n",
    );
    write(&root, "crates/core/src/lib.rs", "");
    write(
        &root,
        "crates/app/Cargo.toml",
        "[package]\nname = \"fx-app\"\ndescription = \"桌面应用\"\n\n[package.metadata.kanzei]\ngroup = \"入口\"\n\n[dependencies]\nengine = { package = \"fx-core\", path = \"../core\" }\nfx-base.workspace = true\n\n[target.'cfg(windows)'.dependencies]\nfx-util = { path = \"../util\" }\n",
    );
    write(&root, "crates/app/src/main.rs", "");
    write(
        &root,
        "extra/Cargo.toml",
        "[package]\nname = \"fx-extra\"\n\n[lib]\npath = \"lib/extra.rs\"\n",
    );
    write(&root, "extra/lib/extra.rs", "");
    root
}

fn normal_edges(ws: &Workspace, transitive: bool) -> Vec<(String, String)> {
    ws.edges
        .iter()
        .filter(|e| e.kind == DepKind::Normal && e.transitive == transitive)
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn workspace_crates_separates_kinds_resolves_renames_and_falls_back() {
    let root = fixture_workspace("ws");
    let ws = workspace_crates(&root).expect("fixture 是工作区");
    let names: Vec<&str> = ws.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        ["fx-app", "fx-base", "fx-core", "fx-util", "fx-extra"]
    );
    // 普通依赖:含 target cfg 与 package 改名;dev/build 单列 kind、不进普通边。
    let mut all_normal: Vec<(String, String)> = ws
        .edges
        .iter()
        .filter(|e| e.kind == DepKind::Normal)
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect();
    all_normal.sort();
    assert_eq!(
        all_normal,
        pairs(&[
            ("fx-app", "fx-base"),
            ("fx-app", "fx-core"),
            ("fx-app", "fx-util"),
            ("fx-core", "fx-base"),
            ("fx-core", "fx-util"),
            ("fx-util", "fx-base"),
        ])
    );
    let dev: Vec<&CrateEdge> = ws.edges.iter().filter(|e| e.kind == DepKind::Dev).collect();
    assert_eq!(dev.len(), 1, "{:?}", ws.edges);
    assert_eq!(
        (dev[0].from.as_str(), dev[0].to.as_str()),
        ("fx-core", "fx-extra")
    );
    assert!(!dev[0].transitive);
    // core 的 build 依赖 util 与普通依赖重合:只留 normal 那条。
    assert!(
        ws.edges.iter().all(|e| e.kind != DepKind::Build),
        "{:?}",
        ws.edges
    );
    // 入口文件与兜底。
    let by_name = |n: &str| ws.members.iter().find(|m| m.name == n).unwrap();
    assert_eq!(by_name("fx-app").entry, "crates/app/src/main.rs");
    assert_eq!(by_name("fx-base").entry, "crates/base/src/lib.rs");
    assert_eq!(by_name("fx-extra").entry, "extra/lib/extra.rs");
    assert_eq!(by_name("fx-extra").group, "未分组");
    assert_eq!(by_name("fx-extra").description, "");
    assert_eq!(by_name("fx-app").id, "fx_app");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn transitive_reduction_hides_derivable_edges() {
    let edges = pairs(&[("a", "b"), ("b", "c"), ("a", "c")]);
    let (kept, hidden) = transitive_reduction(&edges);
    assert_eq!(kept, pairs(&[("a", "b"), ("b", "c")]));
    assert_eq!(hidden, pairs(&[("a", "c")]));
    // 有环也不死循环,环上的边都留着。
    let cyclic = pairs(&[("x", "y"), ("y", "x")]);
    let (kept, hidden) = transitive_reduction(&cyclic);
    assert_eq!(kept.len(), 2);
    assert!(hidden.is_empty());

    let root = fixture_workspace("reduce");
    let ws = workspace_crates(&root).unwrap();
    let mut kept = normal_edges(&ws, false);
    kept.sort();
    assert_eq!(
        kept,
        pairs(&[
            ("fx-app", "fx-core"),
            ("fx-core", "fx-util"),
            ("fx-util", "fx-base")
        ])
    );
    assert_eq!(ws.hidden_transitive(), 3);
    std::fs::remove_dir_all(&root).ok();
}

/// 本仓真实工作区:22 条普通依赖约简成 8 条直接边(设计文档 §5 的数字),8 个成员都有分组与描述。
#[test]
fn real_workspace_reduces_to_eight_direct_edges() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let ws = workspace_crates(&root).expect("仓库根是 Cargo 工作区");
    assert_eq!(ws.members.len(), 8, "{:?}", ws.members);
    for member in &ws.members {
        assert!(
            !member.description.is_empty(),
            "{} 缺 [package] description",
            member.name
        );
        assert_ne!(
            member.group, "未分组",
            "{} 缺 [package.metadata.kanzei] group",
            member.name
        );
        assert!(
            root.join(&member.entry).is_file(),
            "{} 入口不存在",
            member.entry
        );
    }
    let mut kept = normal_edges(&ws, false);
    kept.sort();
    assert_eq!(
        kept,
        pairs(&[
            ("kanzei", "kanzei-tools"),
            ("kanzei-app", "kanzei-tools"),
            ("kanzei-core", "kanzei-harness"),
            ("kanzei-core", "kanzei-llm"),
            ("kanzei-harness", "kanzei-base"),
            ("kanzei-llm", "kanzei-base"),
            ("kanzei-memory", "kanzei-core"),
            ("kanzei-tools", "kanzei-memory"),
        ])
    );
    assert_eq!(ws.hidden_transitive(), 14);
}

fn golden(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/arch_diagram")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读 golden {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// golden:夹具工作区生成的 crate 图源码。改生成器后用
/// `KZ_UPDATE_GOLDEN=1 cargo test -p kanzei-tools arch_diagram` 重写,再人工看 diff;
/// scripts/ui-diagram-smoke.mjs 会把同两份 golden 放进无头 Edge 真渲染。
#[test]
fn crates_mermaid_matches_golden() {
    let root = fixture_workspace("golden");
    let ws = workspace_crates(&root).unwrap();
    let reduced = crates_mermaid(&ws, false);
    let full = crates_mermaid(&ws, true);
    if std::env::var("KZ_UPDATE_GOLDEN").is_ok() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/arch_diagram");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("crates_reduced.mmd"), &reduced).unwrap();
        std::fs::write(dir.join("crates_full.mmd"), &full).unwrap();
    }
    assert_eq!(reduced, golden("crates_reduced.mmd"));
    assert_eq!(full, golden("crates_full.mmd"));
    // 结构断言(golden 之外再钉几条不变量,免得有人顺手重写 golden 时丢掉)。
    assert!(reduced.starts_with("flowchart LR\n"));
    assert_eq!(reduced.matches("  subgraph grp_").count(), 4, "{reduced}");
    assert!(
        reduced.contains("#quot;引号#quot;") && reduced.contains("#96;反引号#96;"),
        "{reduced}"
    );
    assert!(reduced.contains("…\"]"), "超长描述要截断: {reduced}");
    assert!(
        reduced.contains("    fx_app[\"fx-app<br/>桌面应用\"]:::entry\n"),
        "{reduced}"
    );
    assert!(
        reduced.contains("    fx_extra[\"fx-extra\"]:::entry\n"),
        "缺描述时只有包名: {reduced}"
    );
    assert!(reduced.contains("  click fx_app \"crates/app/src/main.rs\" \"fx-app · 桌面应用\"\n"));
    assert!(!reduced.contains("-.->"));
    assert_eq!(full.matches("-.->").count(), 3, "{full}");
    assert_eq!(reduced.matches(" --> ").count(), 3, "{reduced}");
    std::fs::remove_dir_all(&root).ok();
}

fn lint_codes(root: &Path, source: &str) -> Vec<(usize, &'static str, Severity)> {
    lint_mermaid(root, source, 10)
        .into_iter()
        .map(|i| (i.line, i.code, i.severity))
        .collect()
}

fn lint_root() -> PathBuf {
    let root = temp_root("lint");
    write(&root, "crates/x/src/lib.rs", "");
    write(&root, "docs/design/a.md", "# a");
    root
}

#[test]
fn lint_accepts_a_clean_diagram_with_markdown_labels() {
    let root = lint_root();
    let source = "flowchart LR\n  subgraph grp[\"分组\"]\n    a[\"`**甲**\n说明`\"]:::entry\n    b[(state.db)]:::store\n  end\n  c{{判定}} -->|是| a\n  a -- 读 --> b\n  a -.-> d([外部]):::ext\n  class c focus\n  click a \"crates/x/src/lib.rs:12\" \"甲的实现\"\n  click b \"docs/design/a.md\"\n  click d \"R-335\"\n";
    let issues = lint_mermaid(&root, source, 1);
    assert!(issues.is_empty(), "{issues:?}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn lint_rules_fire_with_file_line_numbers() {
    let root = lint_root();
    // D3 init 指令 / frontmatter config
    assert!(lint_codes(
        &root,
        "%%{init: {'theme':'dark'}}%%\nflowchart LR\n a[\"x\"] --> b[\"y\"]"
    )
    .contains(&(10, "D3", Severity::Error)));
    assert!(lint_codes(
        &root,
        "---\nconfig:\n  theme: dark\n---\nflowchart LR\n a[\"x\"] --> b[\"y\"]"
    )
    .contains(&(11, "D3", Severity::Error)));
    assert!(lint_codes(
        &root,
        "---\ntitle: 好\n---\nflowchart LR\n a[\"x\"] --> b[\"y\"]"
    )
    .is_empty());
    // D4 style/classDef/linkStyle 与未知类名
    for bad in [
        "style a fill:#f00",
        "classDef hot fill:#f00",
        "linkStyle 0 stroke:#f00",
    ] {
        let codes = lint_codes(
            &root,
            &format!("flowchart LR\n a[\"x\"] --> b[\"y\"]\n {bad}"),
        );
        assert!(
            codes.contains(&(12, "D4", Severity::Error)),
            "{bad}: {codes:?}"
        );
    }
    assert!(
        lint_codes(&root, "flowchart LR\n a[\"x\"]:::hot --> b[\"y\"]").contains(&(
            11,
            "D4",
            Severity::Error
        ))
    );
    assert!(
        lint_codes(&root, "flowchart LR\n a[\"x\"] --> b[\"y\"]\n class a hot").contains(&(
            12,
            "D4",
            Severity::Error
        ))
    );
    assert!(
        lint_codes(&root, "flowchart LR\n a[\"`**x**\ny`\"]:::hot --> b[\"y\"]").contains(&(
            12,
            "D4",
            Severity::Error
        ))
    );
    // D5 click:语法、未声明、目标不存在、越出项目根
    let base = "flowchart LR\n a[\"x\"] --> b[\"y\"]\n";
    assert!(
        lint_codes(&root, &format!("{base} click a crates/x/src/lib.rs")).contains(&(
            12,
            "D5",
            Severity::Error
        ))
    );
    assert!(
        lint_codes(&root, &format!("{base} click zz \"crates/x/src/lib.rs\"")).contains(&(
            12,
            "D5",
            Severity::Error
        ))
    );
    assert!(
        lint_codes(&root, &format!("{base} click a \"crates/x/src/gone.rs:3\"")).contains(&(
            12,
            "D5",
            Severity::Error
        ))
    );
    assert!(
        lint_codes(&root, &format!("{base} click a \"../outside.md\"")).contains(&(
            12,
            "D5",
            Severity::Error
        ))
    );
    assert!(
        lint_codes(&root, &format!("{base} click a \"C:/abs.rs\"")).contains(&(
            12,
            "D5",
            Severity::Error
        ))
    );
    assert!(lint_codes(
        &root,
        &format!("{base} click a \"crates/x/src/lib.rs:3-9\" \"提示\"")
    )
    .is_empty());
    // D6 保留字 id 与未加引号的特殊字符标签
    assert!(
        lint_codes(&root, "flowchart LR\n end[\"x\"] --> b[\"y\"]").contains(&(
            11,
            "D6",
            Severity::Error
        ))
    );
    assert!(
        lint_codes(&root, "flowchart LR\n a[run(x)] --> b[\"y\"]").contains(&(
            11,
            "D6",
            Severity::Error
        ))
    );
    assert!(
        lint_codes(&root, "flowchart LR\n a[x|y] --> b[\"y\"]").contains(&(
            11,
            "D6",
            Severity::Error
        ))
    );
    assert!(lint_codes(&root, "flowchart LR\n a[\"run(x)\"] --> b[\"y\"]").is_empty());
    // mermaid 12 实测不加引号也能解析的字符(冒号、分号、尖括号、#)不报,免得门禁误拦。
    assert!(
        lint_codes(&root, "flowchart LR\n a[foo: bar; x < y #3] --> b[\"y\"]").is_empty(),
        "只报确定解析失败的字符"
    );
    assert!(
        lint_codes(&root, "flowchart LR\n subgraph end[\"x\"]\n a[\"x\"]\n end").contains(&(
            11,
            "D6",
            Severity::Error
        ))
    );
    // D7 规模(警告)
    let big: String = (0..45)
        .map(|i| format!(" n{i}[\"节点{i}\"] --> n{}[\"节点\"]\n", i + 1))
        .collect();
    let codes = lint_codes(&root, &format!("flowchart LR\n{big}"));
    assert!(codes.contains(&(10, "D7", Severity::Warn)), "{codes:?}");
    assert!(codes.iter().all(|(_, _, s)| *s == Severity::Warn));
    // D8 裸 id 只出现一次(疑似拼错)与 D9 字面量 \n:只警告
    let codes = lint_codes(&root, "flowchart LR\n a[\"x\\ny\"] --> b[\"y\"]\n a --> bb");
    assert!(codes.contains(&(11, "D9", Severity::Warn)), "{codes:?}");
    assert!(codes.contains(&(12, "D8", Severity::Warn)), "{codes:?}");
    assert!(
        codes.iter().all(|(_, _, s)| *s == Severity::Warn),
        "{codes:?}"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn parse_diagram_doc_handles_crlf_titles_and_fences() {
    let root = lint_root();
    let text = "# 运行时主循环\r\n\r\n一次运行从输入到落库。\r\n第二行说明。\r\n\r\n```mermaid\r\nflowchart LR\r\n  a[\"x\"] --> b[\"y\"]\r\n  style a fill:#f00\r\n```\r\n";
    let doc = parse_diagram_doc(&root, "docs/architecture/01_runtime_loop.md", text);
    assert_eq!(doc.id, "01_runtime_loop");
    assert_eq!(doc.title, "运行时主循环");
    assert_eq!(doc.summary, "一次运行从输入到落库。 第二行说明。");
    assert_eq!(doc.source_line, 7);
    assert_eq!(
        doc.source,
        "flowchart LR\n  a[\"x\"] --> b[\"y\"]\n  style a fill:#f00"
    );
    let codes: Vec<(usize, &str)> = doc.issues.iter().map(|i| (i.line, i.code)).collect();
    assert_eq!(codes, vec![(9, "D4")], "CRLF 下行号必须是文件行号");
    assert_eq!(doc.errors(), 1);

    // D1:缺标题 / 缺围栏 / 围栏未闭合;D2:文件名不合约定
    let doc = parse_diagram_doc(&root, "docs/architecture/runtime-loop.md", "说明\n");
    let codes: Vec<&str> = doc.issues.iter().map(|i| i.code).collect();
    assert!(codes.contains(&"D1") && codes.contains(&"D2"), "{codes:?}");
    assert_eq!(doc.source_line, 0);
    let doc = parse_diagram_doc(
        &root,
        "docs/architecture/03_x.md",
        "# 标题\n\n```mermaid\nflowchart LR\n a[\"x\"]\n",
    );
    assert!(
        doc.issues.iter().any(|i| i.code == "D1" && i.line == 3),
        "{:?}",
        doc.issues
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn scan_diagrams_reads_sorted_files_and_skips_readme() {
    let root = lint_root();
    write(
        &root,
        "docs/architecture/02_b.md",
        "# 乙\n\n```mermaid\nflowchart LR\n  a[\"x\"] --> b[\"y\"]\n```\n",
    );
    write(
        &root,
        "docs/architecture/01_a.md",
        "# 甲\n\n```mermaid\nflowchart LR\n  a[\"x\"] --> b[\"y\"]\n```\n",
    );
    write(&root, "docs/architecture/README.md", "# 说明\n");
    let docs = scan_diagrams(&root);
    let ids: Vec<&str> = docs.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["01_a", "02_b"]);
    assert!(docs.iter().all(|d| d.issues.is_empty()), "{docs:?}");
    assert!(scan_diagrams(&temp_root("empty")).is_empty());
    std::fs::remove_dir_all(&root).ok();
}

/// 仓里的两张默认图必须 lint 零 error(点击目标都真实存在)。
#[test]
fn repo_default_diagrams_have_no_lint_errors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let docs = scan_diagrams(&root);
    assert!(docs.len() >= 2, "docs/architecture 下至少两张默认图");
    for doc in &docs {
        let errors: Vec<String> = doc
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| render_issue(&doc.path, i))
            .collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
        assert!(doc.source.contains("click "), "{} 应带节点点击", doc.path);
    }
}
