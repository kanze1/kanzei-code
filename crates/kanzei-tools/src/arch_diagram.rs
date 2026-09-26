//! 架构图(UI2-0926 #7,docs/design/architecture_diagrams.md):crate 依赖图生成 +
//! `docs/architecture/*.md` 手写图扫描与 mermaid 子集 lint。
//!
//! 真源分两类:
//! - **crate 依赖图不落盘**:每次从 Cargo 清单生成(`workspace_crates` → `transitive_reduction`
//!   → `crates_mermaid`),永不漂移。分组与一句话描述来自各 crate 的
//!   `[package] description` 与 `[package.metadata.kanzei] group`。
//! - **手写图**是 `docs/architecture/NN_slug.md`:第一行 `# 标题`,一段说明,第一个 ```` ```mermaid ````
//!   围栏为主图。agent 用普通 write/edit 改,改完调 `architecture {action:"diagrams"}` 自查。
//!
//! lint 只对「确定写错」的形态报 error(门禁必须可满足,见 gates-must-be-satisfiable);完整语法以
//! 真实渲染为准:应用内渲染失败显示带行号的错误卡,本仓 verify 的 `ui_diagram` 步用无头 Edge 渲一遍。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// 手写图目录(相对项目根)。
pub const DIAGRAM_DIR: &str = "docs/architecture";
/// 渲染器按当前主题注入的五个语义类;源码里只准写这几个类名,不准写颜色。
pub const SEMANTIC_CLASSES: [&str; 5] = ["entry", "ext", "store", "focus", "muted"];
/// 超过就提示拆图(警告,不挡)。
pub const MAX_NODES: usize = 40;
pub const MAX_EDGES: usize = 60;
/// crate 图节点描述的截断长度(字符)。
const DESC_MAX_CHARS: usize = 40;
/// 不能当节点 id 的词(mermaid 关键字;`end` 会提前关掉 subgraph)。
const RESERVED_IDS: [&str; 10] = [
    "end",
    "graph",
    "flowchart",
    "subgraph",
    "click",
    "style",
    "class",
    "classdef",
    "linkstyle",
    "direction",
];

// ---------------------------------------------------------------------------
// crate 依赖图
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CrateInfo {
    /// mermaid 节点 id(包名里的非字母数字换成 `_`,撞保留字时加后缀)。
    pub id: String,
    /// Cargo 包名。
    pub name: String,
    pub description: String,
    pub group: String,
    /// 入口文件(项目根相对,正斜杠):src/lib.rs > src/main.rs > [lib]/[[bin]] path > Cargo.toml。
    pub entry: String,
    /// crate 目录(项目根相对)。
    pub dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CrateEdge {
    /// 依赖方包名。
    pub from: String,
    /// 被依赖方包名。
    pub to: String,
    pub kind: DepKind,
    /// 普通依赖里可由其它路径传递得到(约简图里隐藏)。dev/build 恒为 false。
    pub transitive: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Workspace {
    pub members: Vec<CrateInfo>,
    pub edges: Vec<CrateEdge>,
}

impl Workspace {
    /// 被传递约简隐藏的普通依赖条数。
    pub fn hidden_transitive(&self) -> usize {
        self.edges.iter().filter(|e| e.transitive).count()
    }
}

fn read_toml(path: &Path) -> Option<toml::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    text.parse::<toml::Value>().ok()
}

/// crate 清单:仓里有两个成员的清单是小写 `cargo.toml`(Windows 不分大小写,Linux 分),两个都认。
fn crate_manifest_path(dir: &Path) -> Option<PathBuf> {
    ["Cargo.toml", "cargo.toml"]
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
}

fn rel_slash(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// `[workspace] members`(支持末段 `*` 通配)→ 成员目录。
fn member_dirs(root: &Path, manifest: &toml::Value) -> Vec<PathBuf> {
    let Some(members) = manifest
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for member in members.iter().filter_map(|m| m.as_str()) {
        let member = member.replace('\\', "/");
        if let Some(prefix) = member.strip_suffix("/*") {
            let Ok(entries) = std::fs::read_dir(root.join(prefix)) else {
                continue;
            };
            let mut dirs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| crate_manifest_path(p).is_some())
                .collect();
            dirs.sort();
            out.extend(dirs);
        } else {
            out.push(root.join(member.trim_end_matches('/')));
        }
    }
    out
}

fn mermaid_id(name: &str) -> String {
    let mut id: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if id.is_empty() || id.starts_with(|c: char| c.is_ascii_digit()) {
        id.insert(0, 'c');
    }
    if RESERVED_IDS.contains(&id.to_ascii_lowercase().as_str()) {
        id.push_str("_crate");
    }
    id
}

fn entry_file(root: &Path, dir: &Path, manifest: &toml::Value, manifest_path: &Path) -> String {
    for candidate in ["src/lib.rs", "src/main.rs"] {
        if dir.join(candidate).is_file() {
            return rel_slash(root, &dir.join(candidate));
        }
    }
    let declared = manifest
        .get("lib")
        .and_then(|l| l.get("path"))
        .and_then(|p| p.as_str())
        .or_else(|| {
            manifest
                .get("bin")
                .and_then(|b| b.as_array())
                .and_then(|bins| bins.first())
                .and_then(|b| b.get("path"))
                .and_then(|p| p.as_str())
        });
    if let Some(path) = declared {
        if dir.join(path).is_file() {
            return rel_slash(root, &dir.join(path));
        }
    }
    rel_slash(root, manifest_path)
}

/// 依赖表里的真实包名:`foo = { package = "bar" }` 取 bar;`foo.workspace = true` 再查
/// `[workspace.dependencies].foo.package`。
fn dependency_names(table: &toml::Value, ws_deps: Option<&toml::Value>) -> Vec<String> {
    let Some(table) = table.as_table() else {
        return Vec::new();
    };
    table
        .iter()
        .map(|(key, spec)| {
            if let Some(package) = spec.get("package").and_then(|p| p.as_str()) {
                return package.to_string();
            }
            let inherits = spec
                .get("workspace")
                .and_then(|w| w.as_bool())
                .unwrap_or(false);
            if inherits {
                if let Some(package) = ws_deps
                    .and_then(|d| d.get(key))
                    .and_then(|d| d.get("package"))
                    .and_then(|p| p.as_str())
                {
                    return package.to_string();
                }
            }
            key.clone()
        })
        .collect()
}

fn collect_deps(manifest: &toml::Value, ws_deps: Option<&toml::Value>) -> Vec<(String, DepKind)> {
    let mut out = Vec::new();
    let sections = [
        ("dependencies", DepKind::Normal),
        ("dev-dependencies", DepKind::Dev),
        ("build-dependencies", DepKind::Build),
    ];
    let mut push_from = |holder: &toml::Value| {
        for (key, kind) in sections {
            if let Some(table) = holder.get(key) {
                for name in dependency_names(table, ws_deps) {
                    out.push((name, kind));
                }
            }
        }
    };
    push_from(manifest);
    if let Some(targets) = manifest.get("target").and_then(|t| t.as_table()) {
        for spec in targets.values() {
            push_from(spec);
        }
    }
    out
}

/// 解析工作区:成员(包名/描述/分组/入口)与成员之间的依赖边(normal/dev/build 分开)。
/// 不是 Cargo 工作区(根清单缺 `[workspace]`)时返回 None。
pub fn workspace_crates(root: &Path) -> Option<Workspace> {
    let manifest = read_toml(&root.join("Cargo.toml"))?;
    manifest.get("workspace")?;
    let ws_deps = manifest
        .get("workspace")
        .and_then(|w| w.get("dependencies"));
    let mut members = Vec::new();
    let mut raw_deps: Vec<(String, Vec<(String, DepKind)>)> = Vec::new();
    for dir in member_dirs(root, &manifest) {
        let Some(manifest_path) = crate_manifest_path(&dir) else {
            continue;
        };
        let Some(crate_manifest) = read_toml(&manifest_path) else {
            continue;
        };
        let Some(package) = crate_manifest.get("package") else {
            continue;
        };
        let Some(name) = package.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let description = package
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let group = package
            .get("metadata")
            .and_then(|m| m.get("kanzei"))
            .and_then(|k| k.get("group"))
            .and_then(|g| g.as_str())
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .unwrap_or("未分组")
            .to_string();
        members.push(CrateInfo {
            id: mermaid_id(name),
            name: name.to_string(),
            description,
            group,
            entry: entry_file(root, &dir, &crate_manifest, &manifest_path),
            dir: rel_slash(root, &dir),
        });
        raw_deps.push((name.to_string(), collect_deps(&crate_manifest, ws_deps)));
    }
    // 包名相同的 id 冲突(罕见):后来者加序号。
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();
    for member in &mut members {
        let base = member.id.clone();
        let mut n = 2;
        while !seen_ids.insert(member.id.clone()) {
            member.id = format!("{base}_{n}");
            n += 1;
        }
    }
    let names: BTreeSet<&str> = members.iter().map(|m| m.name.as_str()).collect();
    let mut edges: BTreeSet<(String, String, DepKind)> = BTreeSet::new();
    for (from, deps) in &raw_deps {
        for (to, kind) in deps {
            if to != from && names.contains(to.as_str()) {
                edges.insert((from.clone(), to.clone(), *kind));
            }
        }
    }
    // 同一对既是 normal 又是 dev 时只留 normal(dev 那条不带信息)。
    let normal: BTreeSet<(String, String)> = edges
        .iter()
        .filter(|(_, _, k)| *k == DepKind::Normal)
        .map(|(f, t, _)| (f.clone(), t.clone()))
        .collect();
    let normal_list: Vec<(String, String)> = normal.iter().cloned().collect();
    let (_, hidden) = transitive_reduction(&normal_list);
    let hidden: BTreeSet<(String, String)> = hidden.into_iter().collect();
    let edges = edges
        .into_iter()
        .filter(|(f, t, k)| *k == DepKind::Normal || !normal.contains(&(f.clone(), t.clone())))
        .map(|(from, to, kind)| {
            let transitive =
                kind == DepKind::Normal && hidden.contains(&(from.clone(), to.clone()));
            CrateEdge {
                from,
                to,
                kind,
                transitive,
            }
        })
        .collect();
    Some(Workspace { members, edges })
}

/// 有向边表 (from, to)。
pub type EdgeList = Vec<(String, String)>;

/// 传递约简:a→c 若还能经别的路径(长度 ≥ 2)从 a 到 c,则 a→c 可推出、隐藏。
/// 返回 (保留, 隐藏),各自保持输入顺序。有环时照常工作(环上的边都保留)。
pub fn transitive_reduction(edges: &[(String, String)]) -> (EdgeList, EdgeList) {
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to) in edges {
        adjacency
            .entry(from.as_str())
            .or_default()
            .push(to.as_str());
    }
    let reachable_without = |from: &str, to: &str| -> bool {
        // 从 from 出发、不走 from→to 这条直接边,能否到达 to。
        let mut stack: Vec<&str> = adjacency
            .get(from)
            .map(|next| next.iter().copied().filter(|n| *n != to).collect())
            .unwrap_or_default();
        let mut visited: BTreeSet<&str> = BTreeSet::new();
        while let Some(node) = stack.pop() {
            if node == to {
                return true;
            }
            if node == from || !visited.insert(node) {
                continue;
            }
            if let Some(next) = adjacency.get(node) {
                stack.extend(next.iter().copied());
            }
        }
        false
    };
    let mut kept = Vec::new();
    let mut hidden = Vec::new();
    for edge in edges {
        if reachable_without(&edge.0, &edge.1) {
            hidden.push(edge.clone());
        } else {
            kept.push(edge.clone());
        }
    }
    (kept, hidden)
}

/// 文字进带引号的标签:双引号、反引号、尖括号换成 mermaid 实体码(尖括号不转会被当成 HTML 标签),
/// 换行压成空格,超长截断。
fn label_text(text: &str) -> String {
    let clean: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let mut out: String = clean.chars().take(DESC_MAX_CHARS).collect();
    if clean.chars().count() > DESC_MAX_CHARS {
        out.push('…');
    }
    out.replace('"', "#quot;")
        .replace('`', "#96;")
        .replace('<', "#lt;")
        .replace('>', "#gt;")
}

fn tip_text(text: &str) -> String {
    text.replace('"', "'").replace(['\n', '\r'], " ")
}

/// 节点深度 = 从「没有被依赖」的入口出发的最长路径(普通依赖);用于分组与组内排序。
fn depths(ws: &Workspace) -> BTreeMap<String, usize> {
    let mut depth: BTreeMap<String, usize> =
        ws.members.iter().map(|m| (m.name.clone(), 0)).collect();
    let normal: Vec<&CrateEdge> = ws
        .edges
        .iter()
        .filter(|e| e.kind == DepKind::Normal)
        .collect();
    // 最多迭代 N 轮(有环时封顶,不死循环)。
    for _ in 0..ws.members.len() {
        let mut changed = false;
        for edge in &normal {
            let next = depth.get(&edge.from).copied().unwrap_or(0) + 1;
            if let Some(slot) = depth.get_mut(&edge.to) {
                if next > *slot && next <= ws.members.len() {
                    *slot = next;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    depth
}

/// 生成 crate 依赖图 mermaid 源码。`full=false` 只画约简后的直接依赖;`full=true` 另把被约简
/// 的依赖画成虚线 `-.->`。每个 group 一个 subgraph(按成员最小深度排序),没被任何成员依赖的
/// crate 标 `:::entry`,每个节点带 `click <id> "<入口文件>" "<包名 · 描述>"`。
pub fn crates_mermaid(ws: &Workspace, full: bool) -> String {
    let depth = depths(ws);
    let has_dependents: BTreeSet<&str> = ws
        .edges
        .iter()
        .filter(|e| e.kind == DepKind::Normal)
        .map(|e| e.to.as_str())
        .collect();
    let mut groups: Vec<(String, usize, usize)> = Vec::new(); // (名, 最小深度, 首次出现)
    for (index, member) in ws.members.iter().enumerate() {
        let d = depth.get(&member.name).copied().unwrap_or(0);
        match groups.iter_mut().find(|(g, _, _)| *g == member.group) {
            Some(slot) => slot.1 = slot.1.min(d),
            None => groups.push((member.group.clone(), d, index)),
        }
    }
    groups.sort_by_key(|(_, d, first)| (*d, *first));
    let id_of: HashMap<&str, &str> = ws
        .members
        .iter()
        .map(|m| (m.name.as_str(), m.id.as_str()))
        .collect();

    let mut out = String::from("flowchart LR\n");
    out.push_str(if full {
        "  %% 由 Cargo 清单生成(不落盘):实线 = 直接依赖,虚线 = 可由传递得到的依赖;dev/build 依赖不画\n"
    } else {
        "  %% 由 Cargo 清单生成(不落盘):只画直接依赖,可由传递得到的依赖已隐藏;dev/build 依赖不画\n"
    });
    for (index, (group, _, _)) in groups.iter().enumerate() {
        out.push_str(&format!(
            "  subgraph grp_{}[\"{}\"]\n",
            index + 1,
            label_text(group)
        ));
        let mut members: Vec<&CrateInfo> =
            ws.members.iter().filter(|m| &m.group == group).collect();
        members.sort_by_key(|m| (depth.get(&m.name).copied().unwrap_or(0), m.name.clone()));
        for member in members {
            let class = if has_dependents.contains(member.name.as_str()) {
                ""
            } else {
                ":::entry"
            };
            // 两行卡片:第一行包名(渲染器加粗),<br/> 之后是描述(渲染器降为次要色)。
            let label = if member.description.is_empty() {
                label_text(&member.name)
            } else {
                format!(
                    "{}<br/>{}",
                    label_text(&member.name),
                    label_text(&member.description)
                )
            };
            out.push_str(&format!("    {}[\"{label}\"]{class}\n", member.id));
        }
        out.push_str("  end\n");
    }
    for edge in ws.edges.iter().filter(|e| e.kind == DepKind::Normal) {
        let (Some(from), Some(to)) = (id_of.get(edge.from.as_str()), id_of.get(edge.to.as_str()))
        else {
            continue;
        };
        if !edge.transitive {
            out.push_str(&format!("  {from} --> {to}\n"));
        } else if full {
            out.push_str(&format!("  {from} -.-> {to}\n"));
        }
    }
    for member in &ws.members {
        let tip = if member.description.is_empty() {
            member.name.clone()
        } else {
            format!("{} · {}", member.name, member.description)
        };
        out.push_str(&format!(
            "  click {} \"{}\" \"{}\"\n",
            member.id,
            member.entry,
            tip_text(&tip)
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// 手写图扫描与 lint
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LintIssue {
    /// 文件内行号(1 起)。
    pub line: usize,
    pub severity: Severity,
    /// D1–D9。
    pub code: &'static str,
    pub message: String,
    /// 一句修法,弱模型照着改就行。
    pub hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagramDoc {
    /// 文件名去掉 .md。
    pub id: String,
    /// 项目根相对路径。
    pub path: String,
    pub title: String,
    pub summary: String,
    /// 第一个 mermaid 围栏里的源码(LF)。
    pub source: String,
    /// 源码第一行在文件里的行号(1 起;没有围栏时为 0)。
    pub source_line: usize,
    pub issues: Vec<LintIssue>,
}

impl DiagramDoc {
    pub fn errors(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }
}

fn issue(
    line: usize,
    severity: Severity,
    code: &'static str,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> LintIssue {
    LintIssue {
        line,
        severity,
        code,
        message: message.into(),
        hint: hint.into(),
    }
}

/// `NN_snake_case.md`:两位数字前缀决定标签页顺序。
fn is_diagram_file_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    let bytes = stem.as_bytes();
    bytes.len() > 3
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2] == b'_'
        && stem[3..]
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !stem.ends_with('_')
}

/// 扫描 `docs/architecture/*.md`(按文件名排序),逐个取标题/说明/首个 mermaid 围栏并 lint。
pub fn scan_diagrams(root: &Path) -> Vec<DiagramDoc> {
    let dir = root.join(DIAGRAM_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".md") && !n.eq_ignore_ascii_case("README.md"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let text = std::fs::read_to_string(dir.join(&name)).unwrap_or_default();
            parse_diagram_doc(root, &format!("{DIAGRAM_DIR}/{name}"), &text)
        })
        .collect()
}

/// 解析一份图文档(CRLF 兼容)并 lint。
pub fn parse_diagram_doc(root: &Path, path: &str, text: &str) -> DiagramDoc {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let id = file_name
        .strip_suffix(".md")
        .unwrap_or(file_name)
        .to_string();
    let mut issues = Vec::new();
    if !is_diagram_file_name(file_name) {
        issues.push(issue(
            1,
            Severity::Error,
            "D2",
            format!("文件名 `{file_name}` 不合约定"),
            "改名为「两位数字_小写蛇形.md」,如 03_memory_flow.md(数字决定标签页顺序)",
        ));
    }
    let first_content = lines.iter().position(|l| !l.trim().is_empty());
    let title = match first_content {
        Some(index) if lines[index].starts_with("# ") => lines[index][2..].trim().to_string(),
        _ => {
            issues.push(issue(
                first_content.map_or(1, |i| i + 1),
                Severity::Error,
                "D1",
                "第一行必须是 `# 标题`",
                "在文件第一行写 `# 这张图叫什么`",
            ));
            String::new()
        }
    };
    // 说明:标题之后第一段(遇到空行、标题或围栏为止)。
    let mut summary_lines = Vec::new();
    let start = first_content.map_or(0, |i| i + 1);
    for line in lines.iter().skip(start) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if summary_lines.is_empty() {
                continue;
            }
            break;
        }
        if trimmed.starts_with('#') || trimmed.starts_with("```") {
            break;
        }
        summary_lines.push(trimmed);
    }
    let summary = summary_lines.join(" ");

    let fence_at = lines.iter().position(|l| {
        l.trim_start().starts_with("```") && l.trim().trim_start_matches('`').trim() == "mermaid"
    });
    let (source, source_line) = match fence_at {
        None => {
            issues.push(issue(
                lines.len().max(1),
                Severity::Error,
                "D1",
                "没有 ```mermaid 围栏",
                "在说明之后加一个 ```mermaid … ``` 代码块,里面写 flowchart",
            ));
            (String::new(), 0)
        }
        Some(open) => {
            let close = lines
                .iter()
                .enumerate()
                .skip(open + 1)
                .find(|(_, l)| l.trim() == "```")
                .map(|(i, _)| i);
            match close {
                None => {
                    issues.push(issue(
                        open + 1,
                        Severity::Error,
                        "D1",
                        "mermaid 围栏没有闭合",
                        "在图源码最后一行之后补一行 ```",
                    ));
                    (lines[open + 1..].join("\n"), open + 2)
                }
                Some(close) => (lines[open + 1..close].join("\n"), open + 2),
            }
        }
    };
    if source_line > 0 {
        issues.extend(lint_mermaid(root, &source, source_line));
    }
    issues.sort_by_key(|i| (i.line, i.code));
    DiagramDoc {
        id,
        path: path.to_string(),
        title,
        summary,
        source,
        source_line,
        issues,
    }
}

/// 形状开/闭定界符(长的在前,贪心匹配)。
const SHAPES: [(&str, &str); 12] = [
    ("(((", ")))"),
    ("([", "])"),
    ("[[", "]]"),
    ("[(", ")]"),
    ("((", "))"),
    ("{{", "}}"),
    ("[/", "/]"),
    ("[\\", "\\]"),
    ("(", ")"),
    ("[", "]"),
    ("{", "}"),
    (">", "]"),
];

#[derive(Debug, Default)]
struct NodeRef {
    id: String,
    /// 带形状/标签(即「声明」);裸 id 为 false。
    labelled: bool,
    classes: Vec<String>,
}

/// 语句行里的节点引用(近似解析:认不出的片段直接跳过,绝不因此报 error)。
/// 返回 (节点引用, 边数, 未加引号且含特殊字符的标签)。
fn scan_statement(line: &str) -> (Vec<NodeRef>, usize, Vec<String>) {
    let chars: Vec<char> = line.chars().collect();
    let mut nodes = Vec::new();
    let mut edges = 0usize;
    let mut bad_labels = Vec::new();
    let mut i = 0usize;
    let is_id_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.';
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() || c == '&' || c == ';' {
            i += 1;
            continue;
        }
        // 边:连续的 - = . < > o x ~ 组合,可带 |文字|。
        if matches!(c, '-' | '=' | '.' | '<' | '~') {
            let start = i;
            while i < chars.len()
                && matches!(chars[i], '-' | '=' | '.' | '<' | '>' | '~' | 'o' | 'x')
            {
                // o/x 只在箭头尾部算(`--o`),否则是下一个 id 的开头。
                if matches!(chars[i], 'o' | 'x')
                    && (i + 1 < chars.len() && is_id_char(chars[i + 1]))
                {
                    break;
                }
                i += 1;
            }
            let op: String = chars[start..i].iter().collect();
            if matches!(op.as_str(), "--" | "==" | "-.") {
                // `A -- 文字 --> B` 形态:开头只有 `--`/`==`/`-.`,跳到收尾箭头为止。
                let rest: String = chars[i..].iter().collect();
                let closer = ["-->", "---", "==>", "===", ".->", "-.-", "--x", "--o"]
                    .iter()
                    .filter_map(|c| rest.find(c).map(|pos| (pos, c.chars().count())))
                    .min();
                let Some((pos, len)) = closer else {
                    return (nodes, edges, bad_labels);
                };
                i += rest[..pos].chars().count() + len;
                edges += 1;
            } else if op.contains("--")
                || op.contains("==")
                || op.contains("-.")
                || op.contains("~~")
            {
                edges += 1;
            }
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '|' {
                if let Some(end) = (i + 1..chars.len()).find(|&j| chars[j] == '|') {
                    i = end + 1;
                } else {
                    return (nodes, edges, bad_labels);
                }
            }
            continue;
        }
        if !is_id_char(c) {
            // 看不懂:放弃这一行剩余部分(宁可漏报,不误报)。
            break;
        }
        let start = i;
        while i < chars.len() && is_id_char(chars[i]) {
            // `a-->b`:id 在第一个 `--` / `-.` / `==` 前结束。
            if chars[i] == '-' && i + 1 < chars.len() && matches!(chars[i + 1], '-' | '.' | '>') {
                break;
            }
            if chars[i] == '.' && i + 1 < chars.len() && chars[i + 1] == '-' {
                break;
            }
            i += 1;
        }
        let id: String = chars[start..i].iter().collect();
        let mut node = NodeRef {
            id,
            ..Default::default()
        };
        let rest: String = chars[i..].iter().collect();
        if let Some((open, close)) = SHAPES.iter().find(|(open, _)| rest.starts_with(open)) {
            node.labelled = true;
            let after_open = i + open.chars().count();
            let body: String = chars[after_open..].iter().collect();
            let trimmed = body.trim_start();
            if trimmed.starts_with('"') {
                // 带引号:找配对引号,再找闭合定界符。
                let lead = body.chars().count() - trimmed.chars().count();
                let q0 = after_open + lead;
                let Some(q1) = (q0 + 1..chars.len()).find(|&j| chars[j] == '"') else {
                    // 引号标签跨行(markdown 字符串):节点已声明,续行由调用方跳过。
                    nodes.push(node);
                    return (nodes, edges, bad_labels);
                };
                let tail: String = chars[q1 + 1..].iter().collect();
                let Some(pos) = tail.find(close) else {
                    return (nodes, edges, bad_labels);
                };
                i = q1 + 1 + tail[..pos].chars().count() + close.chars().count();
            } else {
                let Some(pos) = body.find(close) else {
                    return (nodes, edges, bad_labels);
                };
                let label = &body[..pos];
                if label.contains(['(', ')', '[', ']', '{', '}', '|', '"']) {
                    bad_labels.push(label.to_string());
                }
                i = after_open + label.chars().count() + close.chars().count();
            }
        }
        while i + 2 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' && chars[i + 2] == ':' {
            let s = i + 3;
            let mut e = s;
            while e < chars.len()
                && (chars[e].is_alphanumeric() || chars[e] == '_' || chars[e] == '-')
            {
                e += 1;
            }
            node.classes.push(chars[s..e].iter().collect());
            i = e;
        }
        nodes.push(node);
    }
    (nodes, edges, bad_labels)
}

fn click_target_problem(root: &Path, target: &str) -> Option<String> {
    let is_ref = target.len() > 2
        && matches!(target.as_bytes()[0], b'R' | b'D' | b'I' | b'S' | b'T')
        && target.as_bytes()[1] == b'-'
        && target[2..].chars().all(|c| c.is_ascii_digit());
    if is_ref {
        return None;
    }
    // path[:行[-行]]
    let mut path = target;
    if let Some((head, tail)) = target.rsplit_once(':') {
        let line_part = tail.split('-').next().unwrap_or("");
        if !line_part.is_empty() && line_part.chars().all(|c| c.is_ascii_digit()) {
            path = head;
        }
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.split('/').any(|seg| seg == "..")
    {
        return Some(format!("目标 `{target}` 不是项目内的相对路径"));
    }
    if normalized.is_empty() || !root.join(&normalized).exists() {
        return Some(format!("目标文件不存在:`{normalized}`"));
    }
    None
}

/// mermaid 子集 lint。`source_line` 是源码第一行在文件里的行号,输出行号都是文件行号。
pub fn lint_mermaid(root: &Path, source: &str, source_line: usize) -> Vec<LintIssue> {
    let normalized = source.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let file_line = |index: usize| source_line + index;
    let mut issues = Vec::new();
    if lines.iter().all(|l| l.trim().is_empty()) {
        issues.push(issue(
            source_line,
            Severity::Error,
            "D1",
            "mermaid 围栏是空的",
            "第一行写 `flowchart LR`(或 TB),下面写节点与连线",
        ));
        return issues;
    }
    // frontmatter(`---` … `---`)里不准有 config。
    let mut body_start = 0usize;
    if lines.first().map(|l| l.trim()) == Some("---") {
        if let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---") {
            for (offset, line) in lines[1..=end].iter().enumerate() {
                if line.trim_start().starts_with("config:") {
                    issues.push(issue(
                        file_line(offset + 1),
                        Severity::Error,
                        "D3",
                        "frontmatter 里写了 config(主题/布局由 kanzei 统一注入)",
                        "删掉 config 段;要标题就只留 `title:`",
                    ));
                }
            }
            body_start = end + 2;
        }
    }
    let mut declared_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut labelled: BTreeSet<String> = BTreeSet::new();
    let mut first_seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut edge_count = 0usize;
    let mut clicks: Vec<(usize, String)> = Vec::new();
    let mut subgraph_ids: BTreeSet<String> = BTreeSet::new();
    let mut other_text = String::new();
    // 跨行的引号标签(markdown 字符串 "`**名**\n说明`" 的续行)不是语句,整段跳过。
    let mut in_string = false;
    let odd_quotes = |line: &str| line.matches('"').count() % 2 == 1;

    for (index, raw) in lines.iter().enumerate().skip(body_start) {
        let line = raw.trim();
        let at = file_line(index);
        if in_string {
            other_text.push_str(line);
            other_text.push('\n');
            if !odd_quotes(line) {
                continue;
            }
            in_string = false;
            // 收尾行:第一个引号闭合标签,之后是形状收尾、可选的 :::类名,再往后照常是语句。
            let tail = line.split_once('"').map_or("", |(_, rest)| rest);
            let mut tail = tail.trim_start_matches([')', ']', '}', '>', '/', '\\']);
            while let Some(rest) = tail.strip_prefix(":::") {
                let class: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                if !SEMANTIC_CLASSES.contains(&class.as_str()) {
                    issues.push(issue(
                        at,
                        Severity::Error,
                        "D4",
                        format!("未知的类名 `:::{class}`"),
                        format!("只能用 :::{}", SEMANTIC_CLASSES.join(" / :::")),
                    ));
                }
                tail = &rest[class.len()..];
            }
            if odd_quotes(tail) {
                in_string = true;
            }
            let (nodes, edges, _) = scan_statement(tail);
            edge_count += edges;
            for node in nodes {
                *declared_counts.entry(node.id.clone()).or_default() += 1;
                first_seen.entry(node.id.clone()).or_insert(at);
                if node.labelled {
                    labelled.insert(node.id.clone());
                }
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if !line.starts_with("%%") && odd_quotes(line) {
            in_string = true;
        }
        if line.starts_with("%%{") {
            issues.push(issue(
                at,
                Severity::Error,
                "D3",
                "出现了 %%{init} 指令(主题/布局由 kanzei 统一注入)",
                "删掉这一行;要强调某个节点就用 :::focus 这类语义类",
            ));
            continue;
        }
        if line.starts_with("%%") {
            continue;
        }
        if line.contains("\\n") {
            issues.push(issue(
                at,
                Severity::Warn,
                "D9",
                "标签里写了字面量 \\n(mermaid 不认,会原样显示)",
                "换行改用 <br/>",
            ));
        }
        let keyword = line.split_whitespace().next().unwrap_or("");
        match keyword {
            "flowchart" | "graph" | "direction" | "end" => continue,
            "style" | "classDef" | "linkStyle" => {
                issues.push(issue(
                    at,
                    Severity::Error,
                    "D4",
                    format!("出现了 {keyword}(图里不写颜色,配色由 kanzei 按主题注入)"),
                    format!(
                        "删掉这一行;需要区分节点就用语义类 :::{}",
                        SEMANTIC_CLASSES.join(" / :::")
                    ),
                ));
                continue;
            }
            "class" => {
                // class a,b focus
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(name) = parts.get(2) {
                    if !SEMANTIC_CLASSES.contains(name) {
                        issues.push(issue(
                            at,
                            Severity::Error,
                            "D4",
                            format!("未知的类名 `{name}`"),
                            format!("只能用 {}", SEMANTIC_CLASSES.join(" / ")),
                        ));
                    }
                }
                other_text.push_str(line);
                other_text.push('\n');
                continue;
            }
            "click" => {
                clicks.push((at, line.to_string()));
                continue;
            }
            "subgraph" => {
                let rest = line["subgraph".len()..].trim();
                let id: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                if !id.is_empty() {
                    if RESERVED_IDS.contains(&id.to_ascii_lowercase().as_str()) {
                        issues.push(issue(
                            at,
                            Severity::Error,
                            "D6",
                            format!("子图 id `{id}` 是保留字"),
                            format!("换个 id,如 `{id}_grp`"),
                        ));
                    }
                    subgraph_ids.insert(id);
                }
                other_text.push_str(line);
                other_text.push('\n');
                continue;
            }
            _ => {}
        }
        other_text.push_str(line);
        other_text.push('\n');
        let (nodes, edges, bad_labels) = scan_statement(line);
        edge_count += edges;
        for label in bad_labels {
            issues.push(issue(
                at,
                Severity::Error,
                "D6",
                format!("标签 `{label}` 含括号/竖线/引号却没加引号(mermaid 解析不了)"),
                format!("写成 id[\"{}\"](整段标签加双引号)", label.replace('"', "'")),
            ));
        }
        for node in nodes {
            if node.id.is_empty() {
                continue;
            }
            if RESERVED_IDS.contains(&node.id.to_ascii_lowercase().as_str()) {
                issues.push(issue(
                    at,
                    Severity::Error,
                    "D6",
                    format!("节点 id `{}` 是保留字", node.id),
                    format!("换个 id,如 `{}_node`,标签照旧写在方括号里", node.id),
                ));
            }
            for class in &node.classes {
                if !SEMANTIC_CLASSES.contains(&class.as_str()) {
                    issues.push(issue(
                        at,
                        Severity::Error,
                        "D4",
                        format!("未知的类名 `:::{class}`"),
                        format!("只能用 :::{}", SEMANTIC_CLASSES.join(" / :::")),
                    ));
                }
            }
            *declared_counts.entry(node.id.clone()).or_default() += 1;
            first_seen.entry(node.id.clone()).or_insert(at);
            if node.labelled {
                labelled.insert(node.id.clone());
            }
        }
    }

    // click 行:语法、id 已声明、目标存在且不越出项目根。
    let word_in = |text: &str, id: &str| {
        text.match_indices(id).any(|(pos, _)| {
            let before = text[..pos].chars().next_back();
            let after = text[pos + id.len()..].chars().next();
            let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            boundary(before) && boundary(after)
        })
    };
    for (at, line) in &clicks {
        let rest = line["click".len()..].trim();
        let id: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
        let after = rest[id.len()..].trim();
        let quoted: Vec<&str> = after.split('"').collect();
        // after 应为 `"target"` 或 `"target" "tip"`:split('"') 得 ["", target, "", ...]。
        let well_formed = !id.is_empty()
            && after.starts_with('"')
            && (quoted.len() == 3 || (quoted.len() == 5 && quoted[2].trim().is_empty()))
            && quoted.last().is_some_and(|tail| tail.trim().is_empty());
        if !well_formed {
            issues.push(issue(
                *at,
                Severity::Error,
                "D5",
                "click 语法不对",
                "写成 click 节点id \"项目相对路径[:行号]\" \"悬停提示\"(或目标写条目号 R-123)",
            ));
            continue;
        }
        if !declared_counts.contains_key(&id)
            && !subgraph_ids.contains(&id)
            && !word_in(&other_text, &id)
        {
            issues.push(issue(
                *at,
                Severity::Error,
                "D5",
                format!("click 的节点 `{id}` 在图里不存在"),
                "先声明这个节点,或把 id 改成已有节点",
            ));
        }
        if let Some(problem) = click_target_problem(root, quoted[1]) {
            issues.push(issue(
                *at,
                Severity::Error,
                "D5",
                problem,
                "目标写项目根相对路径(可带 :行号),先确认文件真的存在",
            ));
        }
    }

    // D7:规模(警告)。
    let node_count = declared_counts.len();
    if node_count > MAX_NODES || edge_count > MAX_EDGES {
        issues.push(issue(
            source_line,
            Severity::Warn,
            "D7",
            format!(
                "图太大:{node_count} 个节点、{edge_count} 条边(上限 {MAX_NODES} / {MAX_EDGES})"
            ),
            "按子系统拆成几张图,每张一个文件",
        ));
    }
    // D8:裸 id 全图只出现一次——多半是拼错了已有节点的 id。
    for (id, count) in &declared_counts {
        if *count == 1 && !labelled.contains(id) && !subgraph_ids.contains(id) {
            let at = first_seen.get(id).copied().unwrap_or(source_line);
            issues.push(issue(
                at,
                Severity::Warn,
                "D8",
                format!("节点 `{id}` 全图只出现一次且没有标签,疑似拼错"),
                "核对 id 拼写;确是新节点就给它写上标签 id[\"说明\"]",
            ));
        }
    }
    issues
}

/// 规范化成展示用的一行:`第 N 行 [D5 error] 消息 → 修法`。
pub fn render_issue(path: &str, issue: &LintIssue) -> String {
    let severity = match issue.severity {
        Severity::Error => "error",
        Severity::Warn => "warn",
    };
    format!(
        "{path}:{} [{} {severity}] {} → 修法:{}",
        issue.line, issue.code, issue.message, issue.hint
    )
}

#[cfg(test)]
#[path = "arch_diagram_tests.rs"]
mod tests;
