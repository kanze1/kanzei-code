//! 项目状态事实(UI2-0926 #13,docs/design/project_workspace.md §2)。
//!
//! 「MD文件保存」现场:用户新建了一个空目录让 agent 做 Flutter 应用,agent 拿到的环境信息
//! 只有「系统、目录、shell」一行,于是派子代理「勘察现有仓库」、发现是空的、又发现没装
//! Flutter,最后反过来问用户「实际仓库在哪」。缺的是几条引擎能直接探测出来的事实:
//! 这是不是空项目、有没有 Git(没有的话哪些功能不可用)、用什么技术栈、工具链在不在。
//!
//! 一处计算、两处消费:[`render`] 进 agent 上下文(`core/project-state`,所有档位),
//! [`ProjectFacts`] 序列化后给桌面端(横幅、「无 Git」芯片、并行线入口)。
//!
//! 约束:
//! - **不 spawn 任何进程**(git 状态读 `.git` 目录,工具链按 PATH 找文件),遍历有上限;
//! - 结果按根缓存 30 秒,bash 每跑完一条命令就让缓存作废(它可能装了工具、建了文件);
//! - 渲染只依赖事实本身(排序、无时间戳),事实不变文本就不变——不打断 prompt 缓存;
//! - 渲染不超过 [`RENDER_BUDGET`] 个字符。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

mod toolchains;

/// 渲染上限(字符)。
pub const RENDER_BUDGET: usize = 600;
const CACHE_TTL: Duration = Duration::from_secs(30);
/// 布局判定的遍历上限:深度与条目数。
const LAYOUT_MAX_DEPTH: usize = 3;
const LAYOUT_MAX_ENTRIES: usize = 2000;
/// 树指纹的遍历上限(进展签名用,比布局判定深)。
const FINGERPRINT_MAX_DEPTH: usize = 8;
const FINGERPRINT_MAX_ENTRIES: usize = 5000;

/// 不算「项目内容」的目录与文件。
const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".kanzei",
    ".idea",
    ".vscode",
    ".dart_tool",
    "node_modules",
    "target",
    "build",
    "dist",
    "__pycache__",
    ".venv",
];
const IGNORED_FILES: &[&str] = &["desktop.ini", "thumbs.db", ".ds_store"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// `.kanzei` 之外没有任何文件:新项目,就在这里搭工程。
    Greenfield,
    /// 只有零星几个文件(≤3)且没有清单文件。
    Sparse,
    /// 已有工程。
    Existing,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum GitState {
    /// 根上与上级都没有 Git。
    None,
    /// 根自己是仓库(`.git` 目录,或工作树的 `.git` 文件)。
    Repo {
        branch: Option<String>,
        has_commits: bool,
    },
    /// 根不是仓库,但某个上级目录是:git 命令会落到那个上级仓库上。
    Parent { toplevel: String },
}

impl GitState {
    pub fn is_repo(&self) -> bool {
        matches!(self, GitState::Repo { .. })
    }
    /// 并行线(git worktree)需要一个有提交的独立仓库。
    pub fn supports_worktrees(&self) -> bool {
        matches!(
            self,
            GitState::Repo {
                has_commits: true,
                ..
            }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlannedStack {
    pub stack: String,
    /// 从哪个 tracker 条目推断出来的(如 `R-001`)。
    pub from: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Toolchain {
    pub name: String,
    /// 文件定位结果，不等于已执行版本验证；None 不代表未安装。
    pub found: Option<String>,
    pub source: Discovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Discovery {
    Path,
    Project,
    SdkEnvironment,
    SdkSibling,
    UserInstall,
    NotFound,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectFacts {
    pub root: String,
    pub layout: Layout,
    /// `.kanzei` 与忽略目录之外的文件数(遍历有上限,是下界)。
    pub files: usize,
    pub git: GitState,
    /// 按清单文件识别出的技术栈(排序、去重)。
    pub stacks: Vec<String>,
    /// 空项目时,从活动 tracker 条目的文字里推断出的计划技术栈。
    pub planned: Vec<PlannedStack>,
    pub toolchains: Vec<Toolchain>,
    /// 有工具链缺失时,本机可用的包管理器(winget/choco/scoop)。
    pub installers: Vec<String>,
}

impl ProjectFacts {
    pub fn missing(&self) -> Vec<&str> {
        self.toolchains
            .iter()
            .filter(|tool| tool.found.is_none())
            .map(|tool| tool.name.as_str())
            .collect()
    }
    pub fn has_stack(&self, stack: &str) -> bool {
        self.stacks.iter().any(|s| s == stack)
    }
}

// ---------- 缓存 ----------

static GENERATION: AtomicU64 = AtomicU64::new(0);

type CacheEntry = (Instant, u64, ProjectFacts);

fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 让全部缓存作废。bash 每跑完一条命令、git init 之后调用。
pub fn invalidate() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// 带缓存的探测(30 秒或 [`invalidate`] 之前复用)。
pub fn probe_cached(root: &Path) -> ProjectFacts {
    let key = crate::path_form::simplify(root).display().to_string();
    let generation = GENERATION.load(Ordering::Relaxed);
    if let Ok(cache) = cache().lock() {
        if let Some((at, cached_generation, facts)) = cache.get(&key) {
            if *cached_generation == generation && at.elapsed() < CACHE_TTL {
                return facts.clone();
            }
        }
    }
    let facts = probe(root);
    if let Ok(mut cache) = cache().lock() {
        cache.insert(key, (Instant::now(), generation, facts.clone()));
    }
    facts
}

/// 只看 Git 三态(读 `.git`/HEAD/refs,不遍历目录、不找工具链、不 spawn git)。
/// `git_status`、并行线入口这类只关心仓库状态的调用方用它,不必跑整套探测。
pub fn git_state_of(root: &Path) -> GitState {
    git_state(&crate::path_form::simplify(root))
}

/// 不带缓存的探测。
pub fn probe(root: &Path) -> ProjectFacts {
    probe_with_locations(
        root,
        &crate::shell::fresh_path(),
        &toolchains::Locations::from_environment(),
    )
}

/// 探测,工具链按给定 PATH 查找(测试用伪 PATH)。
pub fn probe_with_path(root: &Path, path: &std::ffi::OsStr) -> ProjectFacts {
    probe_with_locations(root, path, &toolchains::Locations::default())
}

fn probe_with_locations(
    root: &Path,
    path: &std::ffi::OsStr,
    locations: &toolchains::Locations,
) -> ProjectFacts {
    let root = crate::path_form::simplify(root);
    let files = count_content_files(&root);
    let stacks = detect_stacks(&root);
    let layout = if files == 0 {
        Layout::Greenfield
    } else if files <= 3 && stacks.is_empty() {
        Layout::Sparse
    } else {
        Layout::Existing
    };
    let git = git_state(&root);
    let planned = if stacks.is_empty() {
        planned_stacks(&root)
    } else {
        Vec::new()
    };
    let mut wanted: Vec<&str> = vec!["git"];
    for stack in stacks
        .iter()
        .map(String::as_str)
        .chain(planned.iter().map(|p| p.stack.as_str()))
    {
        for tool in toolchains_for(stack, &root) {
            if !wanted.contains(tool) {
                wanted.push(tool);
            }
        }
    }
    let toolchains: Vec<Toolchain> = wanted
        .iter()
        .map(|name| toolchains::discover(&root, name, path, locations))
        .collect();
    let installers = if toolchains.iter().any(|tool| tool.found.is_none()) {
        ["winget", "choco", "scoop"]
            .iter()
            .filter(|name| crate::shell::find_executable(name, path).is_some())
            .map(|name| (*name).to_string())
            .collect()
    } else {
        Vec::new()
    };
    ProjectFacts {
        root: root.display().to_string(),
        layout,
        files,
        git,
        stacks,
        planned,
        toolchains,
        installers,
    }
}

// ---------- 布局 ----------

fn is_ignored_dir(name: &str) -> bool {
    IGNORED_DIRS
        .iter()
        .any(|ignored| name.eq_ignore_ascii_case(ignored))
}

fn is_ignored_file(name: &str) -> bool {
    IGNORED_FILES
        .iter()
        .any(|ignored| name.eq_ignore_ascii_case(ignored))
}

/// 遍历 `.kanzei` 与忽略目录之外的文件,回调 (相对路径, 元数据)。返回是否因上限截断。
fn walk(
    root: &Path,
    max_depth: usize,
    max_entries: usize,
    visit: &mut dyn FnMut(&Path, &std::fs::Metadata),
) -> bool {
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut seen = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            seen += 1;
            if seen > max_entries {
                return true;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let path = entry.path();
            if meta.is_dir() {
                if is_ignored_dir(&name) || depth + 1 > max_depth {
                    continue;
                }
                stack.push((path, depth + 1));
            } else if !is_ignored_file(&name) {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                visit(&rel, &meta);
            }
        }
    }
    false
}

fn count_content_files(root: &Path) -> usize {
    let mut count = 0usize;
    walk(root, LAYOUT_MAX_DEPTH, LAYOUT_MAX_ENTRIES, &mut |_, _| {
        count += 1
    });
    count
}

/// 树指纹:`.kanzei` 与忽略目录之外文件的 (相对路径, 大小, 修改时间) 哈希。非 Git 项目的
/// 真实进展签名用它代替 HEAD/工作树哈希——否则只写代码不动 tracker 的轮次签名恒定,
/// 连续三轮就被零产出熔断误停(docs/design/project_workspace.md §5)。
pub fn tree_fingerprint(root: &Path) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    let truncated = walk(
        root,
        FINGERPRINT_MAX_DEPTH,
        FINGERPRINT_MAX_ENTRIES,
        &mut |rel, meta| {
            feed(rel.to_string_lossy().as_bytes());
            feed(&meta.len().to_le_bytes());
            let modified = meta
                .modified()
                .ok()
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or_default();
            feed(&modified.to_le_bytes());
        },
    );
    format!("tree-{hash:016x}{}", if truncated { "+" } else { "" })
}

// ---------- Git ----------

/// 解析 `.git`(目录或工作树的 `gitdir:` 文件)得到 git 目录。
fn git_dir_of(dir: &Path) -> Option<PathBuf> {
    let dot_git = dir.join(".git");
    let meta = std::fs::metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(dot_git);
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    let target = PathBuf::from(target);
    Some(if target.is_absolute() {
        target
    } else {
        dir.join(target)
    })
}

fn git_state(root: &Path) -> GitState {
    if let Some(git_dir) = git_dir_of(root) {
        let (branch, has_commits) = head_state(&git_dir);
        return GitState::Repo {
            branch,
            has_commits,
        };
    }
    let mut ancestor = root.parent();
    while let Some(dir) = ancestor {
        if dir.join(".git").exists() {
            return GitState::Parent {
                toplevel: crate::path_form::simplify(dir).display().to_string(),
            };
        }
        ancestor = dir.parent();
    }
    GitState::None
}

/// (当前分支, 是否已有提交)。只读 HEAD / refs / packed-refs,不 spawn git。
fn head_state(git_dir: &Path) -> (Option<String>, bool) {
    let head = std::fs::read_to_string(git_dir.join("HEAD")).unwrap_or_default();
    let head = head.trim();
    let Some(reference) = head.strip_prefix("ref:").map(str::trim) else {
        // 分离 HEAD:内容就是提交哈希。
        return (None, !head.is_empty());
    };
    let branch = reference
        .strip_prefix("refs/heads/")
        .map(str::to_string)
        .or_else(|| Some(reference.to_string()));
    // 工作树的 git 目录在 commondir 里共享 refs。
    let common = std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|text| {
            let path = PathBuf::from(text.trim());
            if path.is_absolute() {
                path
            } else {
                git_dir.join(path)
            }
        });
    let dirs: Vec<&Path> = std::iter::once(git_dir).chain(common.as_deref()).collect();
    let has_commits = dirs.iter().any(|dir| {
        dir.join(reference).is_file()
            || std::fs::read_to_string(dir.join("packed-refs"))
                .map(|packed| {
                    packed
                        .lines()
                        .any(|line| line.split_whitespace().nth(1) == Some(reference))
                })
                .unwrap_or(false)
    });
    (branch, has_commits)
}

// ---------- 技术栈 ----------

fn detect_stacks(root: &Path) -> Vec<String> {
    let mut stacks: Vec<String> = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    // 清单文件在根或下一层(`app/pubspec.yaml`、`frontend/package.json` 很常见)。
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut subdirs: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter(|entry| !is_ignored_dir(&entry.file_name().to_string_lossy()))
            .map(|entry| entry.path())
            .collect();
        subdirs.sort();
        dirs.extend(subdirs.into_iter().take(40));
    }
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let stack = match name.as_str() {
                "cargo.toml" => Some("rust"),
                "package.json" => Some("node"),
                "pubspec.yaml" => Some("flutter"),
                "pyproject.toml" | "requirements.txt" | "setup.py" => Some("python"),
                "go.mod" => Some("go"),
                "pom.xml" | "build.gradle" | "build.gradle.kts" => Some("java"),
                _ if name.ends_with(".sln") || name.ends_with(".csproj") => Some("dotnet"),
                _ => None,
            };
            if let Some(stack) = stack {
                if !stacks.iter().any(|s| s == stack) {
                    stacks.push(stack.to_string());
                }
            }
        }
    }
    stacks.sort();
    stacks
}

/// 空项目时从活动需求的文字里推断计划技术栈(关键词表,忽略大小写)。
fn planned_stacks(root: &Path) -> Vec<PlannedStack> {
    use crate::docstore::{DocStore, REQUIREMENTS};
    const KEYWORDS: &[(&str, &[&str])] = &[
        ("flutter", &["flutter", "dart"]),
        (
            "node",
            &[
                "react",
                "vue",
                "next.js",
                "nextjs",
                "vite",
                "typescript",
                "node.js",
                "nodejs",
                "electron",
                "svelte",
            ],
        ),
        ("rust", &["rust", "cargo", "tauri"]),
        ("python", &["python", "django", "fastapi", "flask"]),
        ("go", &["golang", "go 语言"]),
        ("java", &["java", "kotlin", "spring"]),
        ("dotnet", &[".net", "c#", "dotnet", "wpf", "maui"]),
    ];
    let Ok(entries) = DocStore::open(root, &REQUIREMENTS).load() else {
        return Vec::new();
    };
    let mut out: Vec<PlannedStack> = Vec::new();
    for entry in entries {
        let mut text = entry.title.to_lowercase();
        for (_, value) in &entry.fields {
            text.push(' ');
            text.push_str(&value.to_lowercase());
        }
        for (stack, words) in KEYWORDS {
            if out.iter().any(|p| p.stack == *stack) {
                continue;
            }
            if words.iter().any(|word| contains_word(&text, word)) {
                out.push(PlannedStack {
                    stack: (*stack).to_string(),
                    from: entry.id.clone(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.stack.cmp(&b.stack));
    out
}

/// 词边界匹配(ASCII 字母数字为词内字符):「rust」不命中「trusted」。
fn contains_word(text: &str, word: &str) -> bool {
    let bytes = text.as_bytes();
    let mut start = 0;
    while let Some(offset) = text[start..].find(word) {
        let at = start + offset;
        let end = at + word.len();
        let before = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let after = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before && after {
            return true;
        }
        start = at + word.len().max(1);
        if start >= text.len() {
            break;
        }
    }
    false
}

fn toolchains_for(stack: &str, root: &Path) -> &'static [&'static str] {
    match stack {
        "rust" => &["cargo", "rustc"],
        "node" => {
            if root.join("pnpm-lock.yaml").exists() {
                &["node", "pnpm"]
            } else if root.join("yarn.lock").exists() {
                &["node", "yarn"]
            } else {
                &["node", "npm"]
            }
        }
        "flutter" => &["flutter", "dart"],
        "python" => {
            if cfg!(windows) {
                &["python"]
            } else {
                &["python3"]
            }
        }
        "go" => &["go"],
        "java" => &["java"],
        "dotnet" => &["dotnet"],
        _ => &[],
    }
}

// ---------- Git 初始化(新建项目、横幅「初始化 Git」、git 工具 action=init 共用) ----------

/// `.kanzei/.gitignore` 的内容:运行时文件(可重建的派生物、本机现场)不进版本库;
/// 项目资产(`project/*.md` tracker、`memory/*.md`、`kanzei.toml`)照常入库。
pub const KANZEI_GITIGNORE: &str = "\
# kanzei 运行时文件:可重建的派生物与本机现场,不进版本库。
# 项目资产照常入库:project/*.md(需求/缺陷/决策/测试记录)、memory/*.md、kanzei.toml。
state.db
state.db-*
state.db.v*.bak
*.lock
*.tmp
.write-log/
artifacts/
summaries/
quarantine/
file-annotations.json
memory/index.db
memory/index.db-*
memory/inbox.md
memory/inbox.checkpoint.json
project/auto-run-alerts.jsonl
";

/// 缺失时写 `.kanzei/.gitignore`(已存在绝不覆盖——那可能是用户改过的)。返回是否新写。
pub fn ensure_kanzei_gitignore(root: &Path) -> std::io::Result<bool> {
    let dir = root.join(".kanzei");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(".gitignore");
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(&path, KANZEI_GITIGNORE)?;
    Ok(true)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GitInitOutcome {
    /// 本次新建了仓库(已经是仓库时为 false)。
    pub created: bool,
    pub branch: String,
    /// 做了首次提交。
    pub committed: bool,
    /// 想做首次提交但本机没配 git user.name / user.email。
    pub identity_missing: bool,
    /// 本次新写了 `.kanzei/.gitignore`。
    pub gitignore_written: bool,
}

fn git_sync(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new("git");
    command
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null());
    crate::hide_console(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("找不到 git(不在 PATH 上?): {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "git {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// 在 `root` 建一个独立仓库(默认分支 main),补 `.kanzei/.gitignore`;`initial_commit` 时若本机
/// 配了 git 身份就做一次首提交(并行线/工作树需要 HEAD)。`root` 自己已经是仓库时只补忽略规则。
/// 位于上级仓库内时照样在 `root` 建嵌套仓库——调用方负责先征得同意(git 工具的 init 是 Ask)。
pub fn git_init(root: &Path, initial_commit: bool) -> Result<GitInitOutcome, String> {
    let root = crate::path_form::simplify(root);
    let mut outcome = GitInitOutcome {
        gitignore_written: ensure_kanzei_gitignore(&root)
            .map_err(|error| format!("写 .kanzei/.gitignore 失败: {error}"))?,
        ..GitInitOutcome::default()
    };
    if let GitState::Repo { branch, .. } = git_state(&root) {
        outcome.branch = branch.unwrap_or_default();
        invalidate();
        return Ok(outcome);
    }
    if git_sync(&root, &["init", "-b", "main"]).is_err() {
        // git < 2.28 没有 -b:先 init,再把 HEAD 指到 main。
        git_sync(&root, &["init"])?;
        git_sync(&root, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    }
    outcome.created = true;
    outcome.branch = "main".into();
    if initial_commit {
        let name = git_sync(&root, &["config", "user.name"]).unwrap_or_default();
        let email = git_sync(&root, &["config", "user.email"]).unwrap_or_default();
        if name.is_empty() || email.is_empty() {
            outcome.identity_missing = true;
        } else {
            git_sync(&root, &["add", "-A"])?;
            git_sync(
                &root,
                &["commit", "--allow-empty", "-q", "-m", "初始化项目(kanzei)"],
            )?;
            outcome.committed = true;
        }
    }
    invalidate();
    Ok(outcome)
}

// ---------- 渲染 ----------

/// agent 上下文里的项目状态块(≤ [`RENDER_BUDGET`] 字符)。
pub fn render(facts: &ProjectFacts) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("root: {}", facts.root));
    lines.push(match facts.layout {
        Layout::Greenfield => "layout: 空项目(除 .kanzei 外没有文件)——就在这个目录搭工程,不要向用户索要「实际仓库路径」".to_string(),
        // 文件数只进 ProjectFacts(给界面),不进文本:bash 每跑完一条命令就作废缓存,agent 每建/删
        // 一个文件都会改写系统提示,整段对话的 prompt 缓存随之失效(复核 major)。
        Layout::Sparse => "layout: 几乎为空(无工程清单)——在这里搭工程".to_string(),
        Layout::Existing => "layout: 已有工程".to_string(),
    });
    lines.push(match &facts.git {
        GitState::None => "git: 无——并行线/工作树、提交与差异不可用;需要时用 git 工具 action=init 建库".to_string(),
        GitState::Parent { toplevel } => format!(
            "git: 本目录不是仓库,位于上级仓库 {toplevel} 内——git 工具拒绝操作上级仓库;需要版本管理先 git action=init"
        ),
        GitState::Repo { branch, has_commits } => {
            let branch = branch.as_deref().unwrap_or("(分离 HEAD)");
            if *has_commits {
                format!("git: 仓库,分支 {branch}")
            } else {
                format!("git: 仓库,分支 {branch},还没有提交(并行线要先有一次提交)")
            }
        }
    });
    let detected = if facts.stacks.is_empty() {
        "未检测到".to_string()
    } else {
        facts.stacks.join(", ")
    };
    let planned = if facts.planned.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = facts
            .planned
            .iter()
            .map(|p| format!("{}({})", p.stack, p.from))
            .collect();
        format!(";计划: {}", list.join(", "))
    };
    lines.push(format!("stack: {detected}{planned}"));
    let tools: Vec<String> = facts
        .toolchains
        .iter()
        .map(|tool| match (&tool.found, tool.source) {
            (Some(_), Discovery::Path) => format!("{}: PATH 已定位", tool.name),
            (Some(path), _) => format!("{}: {path}", tool.name),
            (None, _) => format!("{}: 未定位", tool.name),
        })
        .collect();
    lines.push(format!(
        "toolchains: {} (仅定位文件，未验证版本)",
        tools.join(" · ")
    ));
    let mut out = format!("<project-state>\n{}\n", lines.join("\n"));
    if !facts.missing().is_empty() {
        out.push_str("未定位不等于未安装：先核查项目配置、已有 SDK 路径并运行 --version；确认缺失后再决定安装。\n");
    }
    out.push_str("</project-state>");
    if out.chars().count() > RENDER_BUDGET {
        // 极端情况(超长路径):整体截断,保留收尾标签。
        let keep = RENDER_BUDGET - "…\n</project-state>".chars().count();
        let head: String = out.chars().take(keep).collect();
        out = format!("{head}…\n</project-state>");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-project-state-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei").join("project")).unwrap();
        dir
    }

    fn empty_path() -> std::ffi::OsString {
        std::ffi::OsString::new()
    }

    #[test]
    fn 只有_kanzei_与系统文件是空项目() {
        let root = temp_root("greenfield");
        std::fs::write(root.join("desktop.ini"), "x").unwrap();
        std::fs::write(root.join(".kanzei").join("state.db"), "x").unwrap();
        let facts = probe_with_path(&root, &empty_path());
        assert_eq!(facts.layout, Layout::Greenfield);
        assert_eq!(facts.files, 0);
        // 临时目录的上级可能恰好是仓库(比如 CI 的工作目录),只断言不是「自己是仓库」。
        assert!(!facts.git.is_repo(), "{:?}", facts.git);
        let text = render(&facts);
        assert!(text.contains("就在这个目录搭工程"), "{text}");
        assert!(text.contains("不要向用户索要"), "{text}");
        assert!(!text.contains(r"\\?\"), "{text}");
        assert!(text.chars().count() <= RENDER_BUDGET);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 清单文件识别技术栈且不再是空项目() {
        let root = temp_root("stack");
        std::fs::write(root.join("pubspec.yaml"), "name: app").unwrap();
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(root.join("web").join("package.json"), "{}").unwrap();
        std::fs::write(root.join("web").join("pnpm-lock.yaml"), "").unwrap();
        let facts = probe_with_path(&root, &empty_path());
        assert_ne!(facts.layout, Layout::Greenfield);
        assert_eq!(facts.stacks, vec!["flutter".to_string(), "node".into()]);
        let names: Vec<&str> = facts.toolchains.iter().map(|t| t.name.as_str()).collect();
        assert!(names.starts_with(&["git"]), "{names:?}");
        assert!(
            names.contains(&"flutter") && names.contains(&"dart"),
            "{names:?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn git_三态_仓库_无提交_上级仓库() {
        let root = temp_root("git");
        let git = root.join(".git");
        std::fs::create_dir_all(git.join("refs").join("heads")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(
            git_state(&root),
            GitState::Repo {
                branch: Some("main".into()),
                has_commits: false
            }
        );
        std::fs::write(git.join("refs").join("heads").join("main"), "abc\n").unwrap();
        assert!(git_state(&root).supports_worktrees());
        // packed-refs 里的分支同样算有提交。
        std::fs::remove_file(git.join("refs").join("heads").join("main")).unwrap();
        std::fs::write(git.join("packed-refs"), "# pack\nabc refs/heads/main\n").unwrap();
        assert!(git_state(&root).supports_worktrees());
        // 子目录自己没有 .git → 上级仓库。
        let child = root.join("sub");
        std::fs::create_dir_all(&child).unwrap();
        match git_state(&child) {
            GitState::Parent { toplevel } => {
                assert_eq!(PathBuf::from(toplevel), crate::path_form::simplify(&root))
            }
            other => panic!("应判为上级仓库: {other:?}"),
        }
        let text = render(&probe_with_path(&child, &empty_path()));
        assert!(text.contains("git 工具拒绝操作上级仓库"), "{text}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 空项目从活动需求推断计划栈() {
        let root = temp_root("planned");
        std::fs::write(
            root.join(".kanzei").join("project").join("requirements.md"),
            "# Requirements\n\n## R-001 移动端 Markdown 阅读器 [doing]\n- 优先级: P1\n- 技术栈: Flutter + Drift\n",
        )
        .unwrap();
        let facts = probe_with_path(&root, &empty_path());
        assert_eq!(
            facts.planned,
            vec![PlannedStack {
                stack: "flutter".into(),
                from: "R-001".into()
            }]
        );
        let text = render(&facts);
        assert!(text.contains("flutter(R-001)"), "{text}");
        assert!(text.contains("flutter: 未定位"), "{text}");
        assert!(text.contains("未定位不等于未安装"), "{text}");
        assert!(
            text.chars().count() <= RENDER_BUDGET,
            "{}",
            text.chars().count()
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 工具链按_pathext_找到_bat() {
        let root = temp_root("toolchain");
        let bin = root.join("fakebin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(root.join("pubspec.yaml"), "name: app").unwrap();
        #[cfg(windows)]
        std::fs::write(bin.join("flutter.bat"), "@echo off").unwrap();
        #[cfg(not(windows))]
        std::fs::write(bin.join("flutter"), "#!/bin/sh").unwrap();
        let path = std::env::join_paths([bin.clone()]).unwrap();
        let facts = probe_with_path(&root, &path);
        let flutter = facts
            .toolchains
            .iter()
            .find(|tool| tool.name == "flutter")
            .unwrap();
        assert!(flutter.found.is_some(), "{facts:?}");
        let dart = facts
            .toolchains
            .iter()
            .find(|tool| tool.name == "dart")
            .unwrap();
        assert!(dart.found.is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 渲染稳定_事实不变文本不变_且有上限() {
        let facts = ProjectFacts {
            root: format!(r"C:\{}", "长".repeat(400)),
            layout: Layout::Existing,
            files: 12,
            git: GitState::None,
            stacks: vec!["node".into()],
            planned: Vec::new(),
            toolchains: vec![Toolchain {
                name: "node".into(),
                found: None,
                source: Discovery::NotFound,
            }],
            installers: vec!["winget".into()],
        };
        let first = render(&facts);
        assert_eq!(first, render(&facts));
        assert!(
            first.chars().count() <= RENDER_BUDGET,
            "{}",
            first.chars().count()
        );
        assert!(first.ends_with("</project-state>"));
    }

    /// 复核 major:文件数进了文本,agent 每建一个文件系统提示就变,整段对话的 prompt 缓存失效。
    /// 已有工程多一个源文件、几乎为空的目录多一个零星文件,渲染都必须逐字节不变。
    #[test]
    fn 渲染不随文件数变化_加一个文件文本逐字节不变() {
        let root = temp_root("render-files");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for index in 0..5 {
            std::fs::write(root.join("src").join(format!("m{index}.rs")), "").unwrap();
        }
        let before = probe_with_path(&root, &empty_path());
        assert_eq!(before.layout, Layout::Existing);
        std::fs::write(root.join("src").join("new_module.rs"), "pub fn f() {}").unwrap();
        let after = probe_with_path(&root, &empty_path());
        assert_ne!(
            before.files, after.files,
            "文件数照常进 ProjectFacts(界面用)"
        );
        assert_eq!(
            render(&before),
            render(&after),
            "已有工程多一个文件不得改写文本"
        );

        let sparse = temp_root("render-sparse");
        std::fs::write(sparse.join("notes.txt"), "x").unwrap();
        let one = probe_with_path(&sparse, &empty_path());
        std::fs::write(sparse.join("todo.txt"), "y").unwrap();
        let two = probe_with_path(&sparse, &empty_path());
        assert_eq!((one.layout, two.layout), (Layout::Sparse, Layout::Sparse));
        assert_eq!(
            render(&one),
            render(&two),
            "几乎为空的目录多一个文件不得改写文本"
        );
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&sparse).ok();
    }

    #[test]
    fn 树指纹随文件改动变化() {
        let root = temp_root("fingerprint");
        let before = tree_fingerprint(&root);
        std::fs::create_dir_all(root.join("lib").join("src")).unwrap();
        std::fs::write(
            root.join("lib").join("src").join("main.dart"),
            "void main(){}",
        )
        .unwrap();
        let after = tree_fingerprint(&root);
        assert_ne!(before, after);
        // .kanzei 里的变化不算(tracker 由进展签名另算)。
        std::fs::write(root.join(".kanzei").join("state.db-wal"), "x").unwrap();
        assert_eq!(after, tree_fingerprint(&root));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 缓存在_invalidate_后重新探测() {
        let root = temp_root("cache");
        let first = probe_cached(&root);
        assert_eq!(first.layout, Layout::Greenfield);
        std::fs::write(root.join("main.py"), "print(1)").unwrap();
        // (「30 秒内复用」不在这里断言:同进程并行跑的 bash 测试随时会 invalidate。)
        invalidate();
        assert_ne!(probe_cached(&root).layout, Layout::Greenfield);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn git_init_建库_写忽略规则_不覆盖已有文件() {
        if crate::shell::find_executable("git", &crate::shell::fresh_path()).is_none() {
            return;
        }
        let root = temp_root("init");
        let outcome = git_init(&root, false).expect("git init");
        assert!(outcome.created && outcome.gitignore_written);
        assert_eq!(outcome.branch, "main");
        assert!(root.join(".git").is_dir());
        let ignore = std::fs::read_to_string(root.join(".kanzei").join(".gitignore")).unwrap();
        for rule in ["state.db-*", "*.lock", "artifacts/", ".write-log/"] {
            assert!(
                ignore.lines().any(|line| line == rule),
                "缺 {rule}:\n{ignore}"
            );
        }
        assert!(
            !ignore
                .lines()
                .any(|line| line.starts_with("project") && !line.contains("auto-run-alerts")),
            "tracker 必须入库:\n{ignore}"
        );
        assert_eq!(
            git_state(&root),
            GitState::Repo {
                branch: Some("main".into()),
                has_commits: false
            }
        );
        // 已是仓库:不重建、不覆盖用户改过的忽略规则。
        std::fs::write(root.join(".kanzei").join(".gitignore"), "custom\n").unwrap();
        let again = git_init(&root, false).unwrap();
        assert!(!again.created && !again.gitignore_written);
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei").join(".gitignore")).unwrap(),
            "custom\n"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn git_init_首提交取决于本机身份() {
        if crate::shell::find_executable("git", &crate::shell::fresh_path()).is_none() {
            return;
        }
        let root = temp_root("init-commit");
        std::fs::write(root.join(".kanzei").join("state.db-wal"), "x").unwrap();
        std::fs::write(
            root.join(".kanzei").join("project").join("requirements.md"),
            "# R\n",
        )
        .unwrap();
        let outcome = git_init(&root, true).expect("git init");
        if outcome.identity_missing {
            assert!(!outcome.committed);
            assert!(!git_state(&root).supports_worktrees());
        } else {
            assert!(outcome.committed, "{outcome:?}");
            assert!(
                git_state(&root).supports_worktrees(),
                "首提交后应能开并行线"
            );
            let tracked = git_sync(&root, &["ls-files"]).unwrap();
            assert!(tracked.contains(".kanzei/.gitignore"), "{tracked}");
            assert!(
                tracked.contains(".kanzei/project/requirements.md"),
                "{tracked}"
            );
            assert!(!tracked.contains("state.db"), "{tracked}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 词边界() {
        assert!(contains_word("use rust here", "rust"));
        assert!(!contains_word("trusted", "rust"));
        assert!(contains_word("flutter+drift", "flutter"));
        assert!(contains_word("技术栈:flutter", "flutter"));
    }
}
