//! 代码区域注册表(AreaRegistry):把「架构」落成可解析、可分层的区域集合。
//!
//! 记忆图谱(docs/design/memory_knowledge_graph.md §4)按架构渲染记忆:每条记忆挂到它
//! 所说的代码区域上,区域再按依赖深度排成层带。区域有两级:
//!
//! - **crate 级**:Cargo workspace 成员(`kanzei-tools`)、成员内含 ≥3 个前端代码文件的
//!   子目录(`kanzei-app/ui`、`kanzei-app/mobile-pwa`),以及项目根下含 ≥3 个代码文件的
//!   顶层目录(`scripts`)。非 Cargo 项目退化为「顶层目录 = crate 级,其子目录 = 模块」。
//! - **模块级**:`src/*.rs` 与 `src/*/`(跳过 lib/main/mod/*_tests),前端子目录里的
//!   脚本去掉数字前缀(`13-memory.js` → `kanzei-app/ui/memory`)。
//!
//! `depth` = 沿内部依赖的最长路径(带环保护),`band` = round(depth·3/maxDepth),共 4 条带:
//! 0 基础层 … 3 入口层。`resolve_token` 把用户/模型写的各种形态(`area:` 前缀、区域 id、
//! 仓内路径、`kanzei_tools::tracker` 这类 Rust 路径、裸 `13-memory.js`)归一成规范 id。
//!
//! 纯文件系统扫描、只用字符串操作(不依赖 regex);`build_workspace_graph`(kanzei-app
//! docs.rs)委托 [`AreaRegistry::crate_deps`],仓里不再养第二份 Cargo.toml 解析。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// 层带数:入口 / 工具 / 引擎 / 基础。
pub const BANDS: u32 = 4;
/// crate 内前端子目录与顶层代码目录成为区域的最少代码文件数。
const MIN_CODE_FILES: usize = 3;
const FRONTEND_EXTS: &[&str] = &["js", "mjs", "ts", "css"];
const CODE_EXTS: &[&str] = &[
    "rs", "js", "mjs", "cjs", "ts", "tsx", "jsx", "py", "ps1", "sh", "go", "java", "kt", "c", "cc",
    "cpp", "h", "cs", "rb", "swift", "vue", "svelte",
];
/// 永不成为区域的顶层目录(构建产物、文档、元数据)。
const SKIP_TOP: &[&str] = &[
    "crates",
    "docs",
    "target",
    "node_modules",
    "dist",
    "output",
    "build",
    "out",
    "vendor",
];
/// crate 内永不成为前端子区域的目录。
const SKIP_CRATE_DIRS: &[&str] = &[
    "src",
    "target",
    "tests",
    "benches",
    "examples",
    "gen",
    "icons",
    "capabilities",
    "assets",
    "vendor",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AreaKind {
    /// crate 级(Cargo 成员、crate 内前端子目录、顶层代码目录)。
    Crate,
    Module,
}

impl AreaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AreaKind::Crate => "crate",
            AreaKind::Module => "module",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    /// 规范 id,不带 `area:` 前缀:`kanzei-tools`、`kanzei-tools/tracker`、`kanzei-app/ui/memory`、`scripts`。
    pub id: String,
    pub kind: AreaKind,
    /// 画布标签:crate 去掉 `kanzei-` 前缀,模块取末段。
    pub label: String,
    /// 模块与子目录区域的上级区域 id。
    pub parent: Option<String>,
    pub depth: u32,
    pub band: u32,
}

#[derive(Debug, Clone, Default)]
pub struct AreaRegistry {
    areas: Vec<Area>,
    index: HashMap<String, usize>,
    /// crate 目录名 → 成员相对路径(`kanzei-tools` → `crates/kanzei-tools`)。
    member_paths: BTreeMap<String, String>,
    /// Cargo 包名 / Rust 路径名(`kanzei_tools`)→ crate 目录名。
    aliases: HashMap<String, String>,
    deps: Vec<(String, String)>,
    cargo: bool,
}

fn read_manifest(dir: &Path) -> Option<String> {
    ["Cargo.toml", "cargo.toml"]
        .iter()
        .find_map(|name| std::fs::read_to_string(dir.join(name)).ok())
}

/// `[workspace] members = [ "a", "b" ]`(单行或多行)里的引号字符串。
fn workspace_members(toml: &str) -> Vec<String> {
    let Some(at) = toml.find("[workspace]") else {
        return Vec::new();
    };
    let rest = &toml[at..];
    let Some(members_at) = rest.find("members") else {
        return Vec::new();
    };
    let rest = &rest[members_at..];
    let (Some(open), Some(close)) = (rest.find('['), rest.find(']')) else {
        return Vec::new();
    };
    if close < open {
        return Vec::new();
    }
    rest[open + 1..close]
        .lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .flat_map(|line| line.split(','))
        .map(|item| item.trim().trim_matches('"').trim_matches('\'').trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.replace('\\', "/").trim_end_matches('/').to_string())
        .collect()
}

fn package_name(toml: &str) -> Option<String> {
    let at = toml.find("[package]")?;
    toml[at..]
        .lines()
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "name").then(|| value.trim().trim_matches('"').to_string())
        })
}

/// 依赖表里的键(`kanzei-a.workspace = true` / `kanzei-a = { path = … }` / `kanzei-a = "1"`)。
fn dependency_keys(toml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            let section = trimmed.trim_matches(['[', ']']);
            in_deps = section.ends_with("dependencies");
            continue;
        }
        if !in_deps || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(key) = trimmed.split(['.', '=', ' ', '\t']).next() {
            if !key.is_empty() {
                out.push(key.trim_matches('"').to_string());
            }
        }
    }
    out
}

fn ext_of(name: &str) -> Option<&str> {
    name.rsplit_once('.').map(|(_, ext)| ext)
}

fn list_dir(dir: &Path) -> Vec<(String, bool)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, bool)> = read
        .flatten()
        .filter_map(|item| {
            let name = item.file_name().to_string_lossy().to_string();
            let is_dir = item.file_type().ok()?.is_dir();
            Some((name, is_dir))
        })
        .collect();
    out.sort();
    out
}

fn count_files_with(dir: &Path, exts: &[&str]) -> usize {
    list_dir(dir)
        .iter()
        .filter(|(name, is_dir)| {
            !is_dir
                && ext_of(name).is_some_and(|ext| exts.contains(&ext.to_ascii_lowercase().as_str()))
        })
        .count()
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// `13-memory.js` → `memory`;`app.js` → `app`。
pub fn script_stem(file_name: &str) -> String {
    let stem = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name);
    let digits = stem.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && stem.as_bytes().get(digits) == Some(&b'-') {
        stem[digits + 1..].to_string()
    } else {
        stem.to_string()
    }
}

fn crate_label(id: &str) -> String {
    let last = id.rsplit('/').next().unwrap_or(id);
    if id.contains('/') {
        return last.to_string();
    }
    match last.strip_prefix("kanzei-") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => last.to_string(),
    }
}

impl AreaRegistry {
    /// 扫描项目根。读不到任何东西时返回空注册表(调用方降级为「未归类」)。
    pub fn scan(root: &Path) -> Self {
        let mut registry = AreaRegistry::default();
        let workspace = read_manifest(root).unwrap_or_default();
        let mut members = workspace_members(&workspace);
        // `crates/*` 形态的通配成员:展开为含 Cargo.toml 的子目录。
        members = members
            .into_iter()
            .flat_map(|member| match member.strip_suffix("/*") {
                Some(parent) => list_dir(&root.join(parent))
                    .into_iter()
                    .filter(|(name, is_dir)| {
                        *is_dir && read_manifest(&root.join(parent).join(name)).is_some()
                    })
                    .map(|(name, _)| format!("{parent}/{name}"))
                    .collect::<Vec<_>>(),
                None => vec![member],
            })
            .collect();
        if members.is_empty() && package_name(&workspace).is_some() {
            members.push(".".into());
        }
        registry.cargo = !members.is_empty();
        let mut package_to_dir: HashMap<String, String> = HashMap::new();
        let mut manifests: Vec<(String, String)> = Vec::new();
        for member in &members {
            let dir = root.join(member);
            let manifest = read_manifest(&dir).unwrap_or_default();
            let id = if member == "." {
                package_name(&manifest).unwrap_or_else(|| "root".into())
            } else {
                member.rsplit('/').next().unwrap_or(member).to_string()
            };
            if let Some(name) = package_name(&manifest) {
                package_to_dir.insert(name.clone(), id.clone());
                registry.aliases.insert(name.replace('-', "_"), id.clone());
                registry.aliases.insert(name, id.clone());
            }
            registry.aliases.insert(id.replace('-', "_"), id.clone());
            registry.member_paths.insert(id.clone(), member.clone());
            manifests.push((id.clone(), manifest));
            registry.push(&id, AreaKind::Crate, None);
            registry.scan_crate(&id, &dir);
        }
        // 内部依赖边(crate 目录名 → crate 目录名)。
        let mut deps = BTreeSet::new();
        for (id, manifest) in &manifests {
            for key in dependency_keys(manifest) {
                let target = package_to_dir
                    .get(&key)
                    .or_else(|| registry.member_paths.contains_key(&key).then_some(&key));
                if let Some(target) = target {
                    if target != id {
                        deps.insert((id.clone(), target.clone()));
                    }
                }
            }
        }
        registry.deps = deps.into_iter().collect();
        registry.scan_top_level(root);
        registry.compute_depths();
        registry.areas.sort_by(|a, b| a.id.cmp(&b.id));
        registry.index = registry
            .areas
            .iter()
            .enumerate()
            .map(|(i, a)| (a.id.clone(), i))
            .collect();
        registry
    }

    fn push(&mut self, id: &str, kind: AreaKind, parent: Option<&str>) {
        if self.areas.iter().any(|a| a.id == id) {
            return;
        }
        let label = match kind {
            AreaKind::Crate => crate_label(id),
            AreaKind::Module => id.rsplit('/').next().unwrap_or(id).to_string(),
        };
        self.areas.push(Area {
            id: id.to_string(),
            kind,
            label,
            parent: parent.map(str::to_string),
            depth: 0,
            band: 0,
        });
    }

    fn scan_crate(&mut self, id: &str, dir: &Path) {
        for (name, is_dir) in list_dir(&dir.join("src")) {
            let stem = if is_dir {
                name.clone()
            } else if name.ends_with(".rs") {
                name.trim_end_matches(".rs").to_string()
            } else {
                continue;
            };
            if matches!(stem.as_str(), "lib" | "main" | "mod" | "bin")
                || stem.ends_with("_tests")
                || stem.ends_with("_test")
                || stem == "tests"
            {
                continue;
            }
            self.push(&format!("{id}/{stem}"), AreaKind::Module, Some(id));
        }
        for (name, is_dir) in list_dir(dir) {
            if !is_dir || is_hidden(&name) || SKIP_CRATE_DIRS.contains(&name.as_str()) {
                continue;
            }
            let sub = dir.join(&name);
            if count_files_with(&sub, FRONTEND_EXTS) < MIN_CODE_FILES {
                continue;
            }
            let sub_id = format!("{id}/{name}");
            self.push(&sub_id, AreaKind::Crate, Some(id));
            for (file, file_is_dir) in list_dir(&sub) {
                let script = !file_is_dir
                    && ext_of(&file).is_some_and(|ext| matches!(ext, "js" | "mjs" | "ts"));
                if script {
                    let stem = script_stem(&file);
                    self.push(&format!("{sub_id}/{stem}"), AreaKind::Module, Some(&sub_id));
                }
            }
        }
    }

    fn scan_top_level(&mut self, root: &Path) {
        for (name, is_dir) in list_dir(root) {
            if !is_dir || is_hidden(&name) || SKIP_TOP.contains(&name.as_str()) {
                continue;
            }
            if self
                .member_paths
                .values()
                .any(|p| p == &name || p.starts_with(&format!("{name}/")))
            {
                continue;
            }
            let dir = root.join(&name);
            if self.cargo {
                if count_files_with(&dir, CODE_EXTS) >= MIN_CODE_FILES {
                    self.push(&name, AreaKind::Crate, None);
                }
                continue;
            }
            // 非 Cargo 项目:顶层目录(自身或下一层有代码)= crate 级,子目录 = 模块。
            let subdirs: Vec<String> = list_dir(&dir)
                .into_iter()
                .filter(|(n, d)| *d && !is_hidden(n))
                .map(|(n, _)| n)
                .collect();
            let has_code = count_files_with(&dir, CODE_EXTS) > 0
                || subdirs
                    .iter()
                    .any(|sub| count_files_with(&dir.join(sub), CODE_EXTS) > 0);
            if !has_code {
                continue;
            }
            self.push(&name, AreaKind::Crate, None);
            for sub in subdirs {
                if SKIP_TOP.contains(&sub.as_str()) {
                    continue;
                }
                self.push(&format!("{name}/{sub}"), AreaKind::Module, Some(&name));
            }
        }
    }

    fn compute_depths(&mut self) {
        let mut outs: HashMap<&str, Vec<&str>> = HashMap::new();
        for (from, to) in &self.deps {
            outs.entry(from.as_str()).or_default().push(to.as_str());
        }
        fn longest<'a>(
            id: &'a str,
            outs: &HashMap<&'a str, Vec<&'a str>>,
            memo: &mut HashMap<&'a str, u32>,
            stack: &mut Vec<&'a str>,
        ) -> u32 {
            if let Some(depth) = memo.get(id) {
                return *depth;
            }
            if stack.contains(&id) {
                return 0; // 环:截断在这里,深度有限
            }
            stack.push(id);
            let depth = outs
                .get(id)
                .map(|targets| {
                    targets
                        .iter()
                        .map(|t| 1 + longest(t, outs, memo, stack))
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            stack.pop();
            memo.insert(id, depth);
            depth
        }
        let mut memo = HashMap::new();
        let crates: Vec<String> = self.member_paths.keys().cloned().collect();
        let mut depth_of: HashMap<String, u32> = HashMap::new();
        for id in &crates {
            let depth = longest(id.as_str(), &outs, &mut memo, &mut Vec::new());
            depth_of.insert(id.clone(), depth);
        }
        // crate 内子目录区域 = 父 crate + 1。
        for area in &self.areas {
            if area.kind == AreaKind::Crate && !depth_of.contains_key(&area.id) {
                if let Some(parent) = area.parent.as_ref().and_then(|p| depth_of.get(p)) {
                    depth_of.insert(area.id.clone(), parent + 1);
                }
            }
        }
        let max_depth = depth_of.values().copied().max().unwrap_or(0).max(1);
        // 顶层代码目录(scripts 之类)放在最上层带;非 Cargo 项目全部 0。
        for area in &self.areas {
            if area.kind == AreaKind::Crate && !depth_of.contains_key(&area.id) {
                depth_of.insert(area.id.clone(), if self.cargo { max_depth } else { 0 });
            }
        }
        for area in &mut self.areas {
            let crate_id = match area.kind {
                AreaKind::Crate => area.id.clone(),
                AreaKind::Module => area.parent.clone().unwrap_or_default(),
            };
            area.depth = depth_of.get(&crate_id).copied().unwrap_or(0);
            area.band =
                ((area.depth as f64) * f64::from(BANDS - 1) / f64::from(max_depth)).round() as u32;
            area.band = area.band.min(BANDS - 1);
        }
    }

    pub fn all(&self) -> &[Area] {
        &self.areas
    }

    pub fn get(&self, id: &str) -> Option<&Area> {
        self.index.get(id).map(|i| &self.areas[*i])
    }

    pub fn is_empty(&self) -> bool {
        self.areas.is_empty()
    }

    /// crate 级祖先(模块 → 其 crate 级父区域;crate 级区域返回自身)。
    pub fn crate_of(&self, id: &str) -> Option<&str> {
        let area = self.get(id)?;
        match area.kind {
            AreaKind::Crate => Some(area.id.as_str()),
            AreaKind::Module => area.parent.as_deref(),
        }
    }

    /// 决定注册表内容的目录(非递归):项目根、各成员目录与 src/、crate 级子目录、顶层代码目录。
    /// 调用方对它们的列表做 stat 指纹,任何一个增删文件注册表就可能变。
    pub fn watch_dirs(&self, root: &Path) -> Vec<std::path::PathBuf> {
        let mut dirs = vec![root.to_path_buf()];
        for member in self.member_paths.values() {
            let dir = root.join(member);
            dirs.push(dir.join("src"));
            dirs.push(dir);
        }
        for area in &self.areas {
            if area.kind != AreaKind::Crate || self.member_paths.contains_key(&area.id) {
                continue;
            }
            let path = match area.parent.as_ref().and_then(|p| self.member_paths.get(p)) {
                Some(member) => root
                    .join(member)
                    .join(area.id.rsplit('/').next().unwrap_or(&area.id)),
                None => root.join(&area.id),
            };
            dirs.push(path);
        }
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Cargo 成员之间的依赖边(crate 目录名,排序去重)。R-188 架构图数据源。
    pub fn crate_deps(&self) -> Vec<(String, String)> {
        self.deps.clone()
    }

    fn module_or_crate(&self, crate_id: &str, module: &str) -> Option<String> {
        let module_id = format!("{crate_id}/{module}");
        if self.index.contains_key(&module_id) {
            return Some(module_id);
        }
        self.index
            .contains_key(crate_id)
            .then(|| crate_id.to_string())
    }

    /// 仓内相对路径 → 区域。`crates/<c>/src/<m>…`、`crates/<c>/<sub>/NN-x.js`、`scripts/x`。
    fn resolve_path(&self, path: &str) -> Option<String> {
        let path = path.trim_start_matches("./");
        // 最长成员前缀优先(成员路径可以嵌套)。
        let member = self
            .member_paths
            .iter()
            .filter(|(_, p)| {
                p.as_str() != "." && (path == p.as_str() || path.starts_with(&format!("{p}/")))
            })
            .max_by_key(|(_, p)| p.len())
            .map(|(id, p)| (id.clone(), p.clone()))
            .or_else(|| {
                self.member_paths
                    .iter()
                    .find(|(_, p)| p.as_str() == ".")
                    .map(|(id, p)| (id.clone(), p.clone()))
            });
        if let Some((crate_id, member_path)) = member {
            let rest = if member_path == "." {
                path
            } else {
                path[member_path.len()..].trim_start_matches('/')
            };
            let mut parts = rest.split('/').filter(|p| !p.is_empty());
            let first = parts.next();
            let second = parts.next();
            match (first, second) {
                (Some("src"), Some(module)) => {
                    let stem = module.trim_end_matches(".rs");
                    return self.module_or_crate(&crate_id, stem);
                }
                (Some(sub), Some(file)) => {
                    let sub_id = format!("{crate_id}/{sub}");
                    if self.index.contains_key(&sub_id) {
                        return self.module_or_crate(&sub_id, &script_stem(file));
                    }
                    return Some(crate_id);
                }
                (Some(sub), None) => {
                    let sub_id = format!("{crate_id}/{sub}");
                    return Some(if self.index.contains_key(&sub_id) {
                        sub_id
                    } else {
                        crate_id
                    });
                }
                _ => return Some(crate_id),
            }
        }
        let mut parts = path.split('/').filter(|p| !p.is_empty());
        let top = parts.next()?;
        // 只认顶层代码目录区域(scripts);`kanzei-tools/不存在的模块` 这类写错的区域不能退化成 crate。
        if !self.index.contains_key(top) || path == top || self.member_paths.contains_key(top) {
            return None;
        }
        match parts.next() {
            // Cargo 项目的顶层代码目录(scripts)没有模块层。
            Some(next) if !self.cargo => self.module_or_crate(top, next),
            _ => Some(top.to_string()),
        }
    }

    /// 把一个 token 归一成规范区域 id;认不出返回 None。
    ///
    /// 接受:`area:` 前缀、区域 id(`kanzei-tools/tracker`)、包名形态(`kanzei_tools/tracker`)、
    /// 仓内路径(`crates/kanzei-tools/src/tracker.rs:859`、`crates/kanzei-app/ui/13-memory.js`、
    /// `scripts/verify.ps1`)、Rust 路径(`kanzei_tools::tracker::actions`)、裸前端脚本名(`13-memory.js`,
    /// 仅当恰好一个前端区域有它)。
    pub fn resolve_token(&self, token: &str) -> Option<String> {
        let mut token = token
            .trim()
            .trim_matches(|c: char| {
                matches!(
                    c,
                    '`' | '"' | '\'' | '(' | ')' | '[' | ']' | ',' | '，' | '。' | ';' | '；'
                )
            })
            .trim_start_matches("area:")
            .replace('\\', "/");
        // 行号锚点 `path:123` / `path:12-34`。
        if let Some((head, tail)) = token.rsplit_once(':') {
            if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit() || c == '-') {
                token = head.to_string();
            }
        }
        let token = token.trim_end_matches('/');
        if token.is_empty() {
            return None;
        }
        if self.index.contains_key(token) {
            return Some(token.to_string());
        }
        if token.contains("::") {
            let mut parts = token.split("::");
            let krate = self.aliases.get(parts.next()?)?;
            return match parts.next() {
                Some(module) => self.module_or_crate(krate, module),
                None => Some(krate.clone()),
            };
        }
        if let Some((head, tail)) = token.split_once('/') {
            if let Some(krate) = self.aliases.get(head) {
                let candidate = format!("{krate}/{}", tail.trim_end_matches(".rs"));
                return self.index.contains_key(&candidate).then_some(candidate);
            }
            return self.resolve_path(token);
        }
        if let Some(krate) = self.aliases.get(token) {
            return Some(krate.clone());
        }
        // 裸前端脚本名:只在恰好一个前端子区域有同名模块时认。
        if ext_of(token).is_some_and(|ext| matches!(ext, "js" | "mjs" | "ts")) {
            let stem = script_stem(token);
            let hits: Vec<&Area> = self
                .areas
                .iter()
                .filter(|a| {
                    a.kind == AreaKind::Module
                        && a.id.ends_with(&format!("/{stem}"))
                        && a.parent.as_deref().is_some_and(|p| p.contains('/'))
                })
                .collect();
            if hits.len() == 1 {
                return Some(hits[0].id.clone());
            }
        }
        None
    }

    /// 解析失败时的候选(报错写全判据用):前缀或子串最接近的至多 `limit` 个区域 id。
    pub fn nearest(&self, token: &str, limit: usize) -> Vec<String> {
        let needle = token
            .trim()
            .trim_start_matches("area:")
            .replace('\\', "/")
            .to_ascii_lowercase();
        let tail = needle
            .rsplit(['/', ':'])
            .next()
            .unwrap_or(&needle)
            .trim_end_matches(".rs")
            .to_string();
        let tail = script_stem(&tail);
        let mut scored: Vec<(usize, &str)> = self
            .areas
            .iter()
            .filter_map(|area| {
                let id = area.id.to_ascii_lowercase();
                let common = id
                    .chars()
                    .zip(needle.chars())
                    .take_while(|(a, b)| a == b)
                    .count();
                let score = if !tail.is_empty() && id.ends_with(&format!("/{tail}")) {
                    1000 + common
                } else if !tail.is_empty() && id.contains(&tail) {
                    500 + common
                } else {
                    common
                };
                (score >= 3).then_some((score, area.id.as_str()))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, id)| id.to_string())
            .collect()
    }
}

/// 在 `root` 下按工作区 crate 依赖图返回 (crate, 依赖) 边;kanzei-app 架构图的数据源。
pub fn workspace_crate_deps(root: &Path) -> Vec<(String, String)> {
    AreaRegistry::scan(root).crate_deps()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-areas-{tag}-{}-{}",
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

    /// 两个 crate(一个带 ui/ 前端目录)+ 顶层 scripts/。
    fn fixture_workspace(tag: &str) -> PathBuf {
        let root = temp_root(tag);
        write(&root, "Cargo.toml", "[workspace]\nmembers = [\n    \"crates/kanzei-a\", # 注释\n    \"crates/kanzei-b\",\n]\n");
        write(
            &root,
            "crates/kanzei-a/Cargo.toml",
            "[package]\nname = \"kanzei-a\"\n",
        );
        write(&root, "crates/kanzei-a/src/lib.rs", "");
        write(&root, "crates/kanzei-a/src/tracker.rs", "");
        write(&root, "crates/kanzei-a/src/tracker_tests.rs", "");
        write(&root, "crates/kanzei-a/src/memory/mod.rs", "");
        write(&root, "crates/kanzei-b/Cargo.toml", "[package]\nname = \"kanzei-b\"\n[dependencies]\nkanzei-a.workspace = true\nserde = \"1\"\n");
        write(&root, "crates/kanzei-b/src/main.rs", "");
        write(&root, "crates/kanzei-b/src/run.rs", "");
        for file in ["01-core.js", "13-memory.js", "style.css"] {
            write(&root, &format!("crates/kanzei-b/ui/{file}"), "");
        }
        write(&root, "crates/kanzei-b/icons/a.js", "");
        for file in ["verify.ps1", "a.mjs", "b.mjs"] {
            write(&root, &format!("scripts/{file}"), "");
        }
        write(&root, "docs/design/x.md", "");
        root
    }

    #[test]
    fn scan_workspace_emits_crates_modules_and_code_dirs() {
        let root = fixture_workspace("scan");
        let registry = AreaRegistry::scan(&root);
        let ids: Vec<&str> = registry.all().iter().map(|a| a.id.as_str()).collect();
        for expected in [
            "kanzei-a",
            "kanzei-a/tracker",
            "kanzei-a/memory",
            "kanzei-b",
            "kanzei-b/run",
            "kanzei-b/ui",
            "kanzei-b/ui/core",
            "kanzei-b/ui/memory",
            "scripts",
        ] {
            assert!(ids.contains(&expected), "缺区域 {expected}: {ids:?}");
        }
        for absent in [
            "kanzei-a/lib",
            "kanzei-a/tracker_tests",
            "kanzei-b/main",
            "kanzei-b/icons",
            "docs",
        ] {
            assert!(!ids.contains(&absent), "不该有区域 {absent}: {ids:?}");
        }
        assert_eq!(registry.get("kanzei-b/ui").unwrap().kind, AreaKind::Crate);
        assert_eq!(registry.get("kanzei-b/ui").unwrap().label, "ui");
        assert_eq!(registry.get("kanzei-a").unwrap().label, "a");
        assert_eq!(
            registry
                .get("kanzei-b/ui/memory")
                .unwrap()
                .parent
                .as_deref(),
            Some("kanzei-b/ui")
        );
        assert_eq!(registry.crate_of("kanzei-b/ui/memory"), Some("kanzei-b/ui"));
        assert_eq!(
            registry.crate_deps(),
            vec![("kanzei-b".to_string(), "kanzei-a".to_string())]
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn depth_band_follow_internal_deps_and_survive_cycles() {
        let root = temp_root("depth");
        write(
            &root,
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/a\", \"crates/b\", \"crates/c\"]\n",
        );
        write(
            &root,
            "crates/a/Cargo.toml",
            "[package]\nname = \"a\"\n[dependencies]\nb = { path = \"../b\" }\n",
        );
        write(
            &root,
            "crates/b/Cargo.toml",
            "[package]\nname = \"b\"\n[dependencies]\nc.workspace = true\n",
        );
        write(&root, "crates/c/Cargo.toml", "[package]\nname = \"c\"\n");
        let registry = AreaRegistry::scan(&root);
        let depth = |id: &str| registry.get(id).unwrap().depth;
        let band = |id: &str| registry.get(id).unwrap().band;
        assert_eq!((depth("a"), depth("b"), depth("c")), (2, 1, 0));
        assert_eq!((band("a"), band("b"), band("c")), (3, 2, 0));

        // a ↔ b 成环:不栈溢出,深度有限、band 在 0..BANDS。
        write(
            &root,
            "crates/c/Cargo.toml",
            "[package]\nname = \"c\"\n[dependencies]\na = { path = \"../a\" }\n",
        );
        let cyclic = AreaRegistry::scan(&root);
        for area in cyclic.all() {
            assert!(area.depth <= 3 && area.band < BANDS, "{area:?}");
        }
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn non_cargo_project_falls_back_to_top_level_dirs() {
        let root = temp_root("plain");
        write(&root, "src/app/main.py", "");
        write(&root, "src/util/io.py", "");
        write(&root, "web/index.js", "");
        write(&root, "notes/readme.md", "");
        write(&root, "node_modules/x/index.js", "");
        let registry = AreaRegistry::scan(&root);
        let ids: Vec<&str> = registry.all().iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["src", "src/app", "src/util", "web"], "{ids:?}");
        assert!(registry.crate_deps().is_empty());
        assert_eq!(
            registry.resolve_token("src/app/main.py").as_deref(),
            Some("src/app")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn resolve_token_accepts_id_path_rustpath_and_rejects_unknown() {
        let root = fixture_workspace("resolve");
        let registry = AreaRegistry::scan(&root);
        let cases = [
            ("area:kanzei-a/tracker", Some("kanzei-a/tracker")),
            ("kanzei-a/tracker", Some("kanzei-a/tracker")),
            ("kanzei_a/tracker", Some("kanzei-a/tracker")),
            ("crates/kanzei-a/src/tracker.rs", Some("kanzei-a/tracker")),
            (
                "crates/kanzei-a/src/tracker.rs:859-914",
                Some("kanzei-a/tracker"),
            ),
            (
                "crates\\kanzei-a\\src\\memory\\store.rs",
                Some("kanzei-a/memory"),
            ),
            ("crates/kanzei-a/src/nothing.rs", Some("kanzei-a")),
            (
                "crates/kanzei-b/ui/13-memory.js",
                Some("kanzei-b/ui/memory"),
            ),
            ("13-memory.js", Some("kanzei-b/ui/memory")),
            ("`kanzei_a::tracker::actions`", Some("kanzei-a/tracker")),
            ("kanzei_b::nope", Some("kanzei-b")),
            ("kanzei-a/trackr", None),
            ("kanzei-a/tracker.rs", Some("kanzei-a/tracker")),
            ("scripts/verify.ps1", Some("scripts")),
            ("kanzei-b", Some("kanzei-b")),
            ("nope", None),
            ("xyz/abc", None),
            ("", None),
        ];
        for (token, expected) in cases {
            assert_eq!(
                registry.resolve_token(token).as_deref(),
                expected,
                "token {token}"
            );
        }
        let near = registry.nearest("kanzei-a/trackr", 5);
        assert!(
            near.first().is_some_and(|id| id.starts_with("kanzei-a")),
            "{near:?}"
        );
        let near = registry.nearest("crates/x/src/memory.rs", 5);
        assert!(
            near.contains(&"kanzei-a/memory".to_string())
                || near.contains(&"kanzei-b/ui/memory".to_string()),
            "{near:?}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn real_repo_bands_match_architecture_layers() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        if !root.join("crates/kanzei-app").is_dir() {
            return;
        }
        let registry = AreaRegistry::scan(&root);
        let band = |id: &str| registry.get(id).map(|a| a.band);
        assert_eq!(band("kanzei-base"), Some(0));
        assert_eq!(band("kanzei-app"), Some(3));
        assert_eq!(band("kanzei-app/ui"), Some(3));
        assert_eq!(band("scripts"), Some(3));
        assert!(band("kanzei-tools").is_some_and(|b| b == 2));
        assert!(registry.get("kanzei-tools/tracker").is_some());
        assert!(registry.get("kanzei-app/ui/memory").is_some());
    }
}
