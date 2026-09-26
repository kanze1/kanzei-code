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
//! 手写图的扫描与 lint 在子模块 `arch_diagram_lint.rs`(本文件只管 crate 图),对外接口经这里 re-export。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Serialize;

#[path = "arch_diagram_lint.rs"]
mod lint;
pub use lint::{
    lint_mermaid, parse_diagram_doc, render_issue, scan_diagrams, DiagramDoc, LintIssue, Severity,
};

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
pub(crate) const RESERVED_IDS: [&str; 10] = [
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

/// 生成 crate 依赖图 mermaid 源码。`full=false` 只画约简后的直接依赖,每个 group 一个 subgraph
/// (按成员最小深度排序);`full=true` 另把被约简的依赖画成虚线 `-.->`,且**不分组**:跨组的传递边
/// 会被 ELK 并到分组框的边上(mermaid 写死了 mergeHierarchyEdges),画出来像一圈圈虚线框,看不出是哪几条。
/// 节点按分组、深度排序(ELK 按模型顺序摆放,同组的仍挨在一起)。没被任何成员依赖的 crate 标
/// `:::entry`,每个节点带 `click <id> "<入口文件>" "<包名 · 描述>"`。
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
        "  %% 由 Cargo 清单生成(不落盘):实线 = 直接依赖,虚线 = 可由传递得到的依赖(不分组);dev/build 依赖不画\n"
    } else {
        "  %% 由 Cargo 清单生成(不落盘):只画直接依赖,可由传递得到的依赖已隐藏;dev/build 依赖不画\n"
    });
    let indent = if full { "  " } else { "    " };
    for (index, (group, _, _)) in groups.iter().enumerate() {
        if !full {
            out.push_str(&format!(
                "  subgraph grp_{}[\"{}\"]\n",
                index + 1,
                label_text(group)
            ));
        }
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
            out.push_str(&format!("{indent}{}[\"{label}\"]{class}\n", member.id));
        }
        if !full {
            out.push_str("  end\n");
        }
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

#[cfg(test)]
#[path = "arch_diagram_tests.rs"]
mod tests;
