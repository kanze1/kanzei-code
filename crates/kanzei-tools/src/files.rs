//! 文件导览(R-148):一份扫描器,两个消费者。
//!
//! - `files` 工具:agent 的架构地图——文本树 + 度量 + AI 用途标注,弱模型不必
//!   逐个 read 就知道每个文件是干嘛的。
//! - 桌面端「文件」页:同一扫描结果的 JSON 形态(经 Tauri command)。
//!
//! 度量是**中性**的(用户定调):行数多不必然要拆,工具不预设"该拆"立场;
//! 拆分判断结合用途标注由人/agent 做。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 超过这个大小只 stat 不读内容:二进制/日志文件不该拖慢整树扫描。
const MAX_MEASURE_BYTES: u64 = 2 * 1024 * 1024;
/// 行数统计的代码类扩展名。md 单独按字符数计。
const CODE_EXTS: &[&str] = &[
    "rs", "js", "ts", "tsx", "jsx", "mjs", "cjs", "css", "html", "vue", "py", "go", "java", "c",
    "h", "cpp", "hpp", "toml", "json", "yml", "yaml", "sql", "ps1", "sh", "bat", "xml", "svelte",
];

/// AI 用途标注缓存(.kanzei/file-annotations.json)。
/// 键=仓库相对路径(正斜杠);hash 是内容指纹,文件变了标注即失效。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AnnotationStore {
    #[serde(default)]
    pub files: BTreeMap<String, Annotation>,
    /// 目录用途(键=目录相对路径,"" 代表仓库根)。目录内容变化不使其失效——
    /// 目录职责比单文件稳定,过期由重新标注覆盖。
    #[serde(default)]
    pub dirs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub hash: String,
    pub note: String,
}

pub fn annotations_path(project_root: &Path) -> PathBuf {
    project_root.join(".kanzei").join("file-annotations.json")
}

pub fn load_annotations(project_root: &Path) -> AnnotationStore {
    std::fs::read_to_string(annotations_path(project_root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_annotations(project_root: &Path, store: &AnnotationStore) -> std::io::Result<()> {
    let path = annotations_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 序列化失败必须报错——旧实现 unwrap_or_default 会把空字符串写进缓存,
    // 下次 load 解析失败回落 Default,整库标注静默清零(D-213 一类)。
    let text = serde_json::to_string_pretty(store)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    crate::atomic_file::write_atomic(&path, &text)
}

/// 内容指纹。凡是标注会覆盖的文件(代码/md)扫描时本来就读了全文,顺手 FNV-1a——
/// **真内容 hash**,mtime 免疫:git checkout/触碰时间戳不会让标注大面积假失效
/// (D-213 教训:mtime 版指纹太脆)。只有 oversized 文件退化为大小+mtime。
pub fn content_stamp(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", meta.len(), mtime)
}

fn mtime_ns(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// D-233 批2:vendor/gen 等永不标注的路径段——只 stat 不读内容,树里仍显示
/// 但 measurable 集合缩到项目自有源码。Monaco vendor 85 个文件 1.1MB 被逐个
/// 读+哈希纯属浪费:它们永远不会被标注,读了只拖慢整树扫描。
fn is_vendor_rel(rel: &str) -> bool {
    rel.split('/').any(|seg| {
        matches!(
            seg,
            "vendor" | "node_modules" | "dist" | "target" | "gen" | "third_party"
        )
    })
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn content_hash(bytes: &[u8]) -> String {
    format!("fnv-{:016x}", fnv1a(bytes))
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    /// 仓库相对路径,正斜杠。
    pub path: String,
    pub size: u64,
    /// 代码行数(代码类扩展名)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u64>,
    /// md 字数(字符数)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chars: Option<u64>,
    /// 超过 MAX_MEASURE_BYTES,未读内容。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub oversized: bool,
    /// 内容指纹(标注失效判据)。
    pub stamp: String,
    /// 修改时间(ns)——增量重扫的粗判依据(D-233 批2),不下发前端。
    #[serde(skip)]
    pub(crate) mtime_ns: u128,
}

/// 扫描仓库:git ls-files 拿清单(尊重 .gitignore,含未跟踪),非 git 目录退化为
/// 过滤遍历(跳 .git/target/node_modules/.kanzei 的库文件)。全量扫描入口。
pub fn scan(project_root: &Path) -> Vec<FileEntry> {
    scan_incremental(project_root, None).0
}

/// D-233 批2:增量扫描——按 size+mtime 粗判未变的文件复用上次的行数/哈希,
/// 只重读变了的(全文 FNV 只在标注流程里保持 D-213 的 mtime 免疫语义)。
/// 返回 (entries, reused_count):reused_count 供调用方做缓存命中证据。
pub fn scan_incremental(
    project_root: &Path,
    previous: Option<&[FileEntry]>,
) -> (Vec<FileEntry>, usize) {
    let list = git_file_list(project_root).unwrap_or_else(|| walk_fallback(project_root));
    let prev: std::collections::HashMap<&str, &FileEntry> = previous
        .unwrap_or_default()
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let mut reused = 0usize;
    let mut out = Vec::with_capacity(list.len());
    for rel in list {
        let abs = project_root.join(&rel);
        let Ok(meta) = std::fs::metadata(&abs) else {
            continue; // ls-files 里已删除但未暂存的文件
        };
        if !meta.is_file() {
            continue;
        }
        let rel_slash = rel.replace('\\', "/");
        // 增量命中:同路径、同大小、同 mtime —— 内容几乎必然没变,直接复用
        // 上次的行数/哈希,不碰磁盘读。vendor/二进制等本就不读内容的也走这。
        if let Some(p) = prev.get(rel_slash.as_str()) {
            if p.size == meta.len() && p.mtime_ns == mtime_ns(&meta) {
                out.push((*p).clone());
                reused += 1;
                continue;
            }
        }
        let ext = Path::new(&rel_slash)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let oversized = meta.len() > MAX_MEASURE_BYTES;
        // D-233 批2:vendor/gen 路径只 stat 不读内容——它们永远不会被标注,
        // 读全文纯属浪费(Monaco vendor 85 文件 1.1MB 实测拖慢整树扫描)。
        let measurable = !oversized
            && !is_vendor_rel(&rel_slash)
            && (ext == "md" || CODE_EXTS.contains(&ext.as_str()));
        let bytes = if measurable {
            std::fs::read(&abs).ok()
        } else {
            None
        };
        let (lines, chars) = match (&bytes, ext.as_str()) {
            (Some(bytes), "md") => (
                None,
                Some(String::from_utf8_lossy(bytes).chars().count() as u64),
            ),
            (Some(bytes), _) => (
                Some(bytes.iter().filter(|b| **b == b'\n').count() as u64 + 1),
                None,
            ),
            (None, _) => (None, None),
        };
        // 读到内容的用真 hash(mtime 免疫);其余(二进制/过大/vendor)退化为大小+mtime——
        // 它们本来也不会被标注,指纹只用于展示层的粗判。
        let stamp = bytes
            .as_deref()
            .map(content_hash)
            .unwrap_or_else(|| content_stamp(&meta));
        out.push(FileEntry {
            path: rel_slash,
            size: meta.len(),
            lines,
            chars,
            oversized,
            stamp,
            mtime_ns: mtime_ns(&meta),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    (out, reused)
}

fn git_file_list(root: &Path) -> Option<Vec<String>> {
    let mut command = std::process::Command::new("git");
    command
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(root);
    crate::hide_console(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_string)
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// 非 git 目录的退化遍历。深度与数量都有界:导览不是备份工具。
fn walk_fallback(root: &Path) -> Vec<String> {
    const SKIP: &[&str] = &[".git", "target", "node_modules", "dist", ".kanzei"];
    const MAX_FILES: usize = 5000;
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel_dir) = stack.pop() {
        if out.len() >= MAX_FILES {
            break;
        }
        let Ok(read) = std::fs::read_dir(root.join(&rel_dir)) else {
            continue;
        };
        for entry in read.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().to_string();
            let rel = if rel_dir.as_os_str().is_empty() {
                PathBuf::from(&name)
            } else {
                rel_dir.join(&name)
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !SKIP.contains(&name.as_str()) {
                    stack.push(rel);
                }
            } else if kind.is_file() {
                out.push(rel.to_string_lossy().to_string());
            }
        }
    }
    out
}

/// 目录聚合:文件数/总大小/总行数(md 字数不进目录行数——两种度量不同维)。
#[derive(Debug, Default, Clone, Serialize)]
pub struct DirStat {
    pub files: usize,
    pub size: u64,
    pub lines: u64,
}

pub fn aggregate_dirs(entries: &[FileEntry]) -> BTreeMap<String, DirStat> {
    let mut dirs: BTreeMap<String, DirStat> = BTreeMap::new();
    for entry in entries {
        let mut dir = entry.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        loop {
            let stat = dirs.entry(dir.to_string()).or_default();
            stat.files += 1;
            stat.size += entry.size;
            stat.lines += entry.lines.unwrap_or(0);
            match dir.rsplit_once('/') {
                Some((parent, _)) => dir = parent,
                None if !dir.is_empty() => dir = "",
                None => break,
            }
        }
    }
    dirs
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{bytes}B")
    }
}

fn measure_label(entry: &FileEntry) -> String {
    if entry.oversized {
        return format!("{} 过大未计", human_size(entry.size));
    }
    match (entry.lines, entry.chars) {
        (Some(lines), _) => format!("{} {lines} 行", human_size(entry.size)),
        (_, Some(chars)) => format!("{} {chars} 字", human_size(entry.size)),
        _ => human_size(entry.size),
    }
}

/// 文本树渲染(agent 消费)。目录行带聚合,文件行带度量与标注。
pub fn render_tree(
    entries: &[FileEntry],
    annotations: &AnnotationStore,
    prefix: Option<&str>,
) -> String {
    let filtered: Vec<&FileEntry> = entries
        .iter()
        .filter(|e| prefix.is_none_or(|p| e.path.starts_with(p)))
        .collect();
    if filtered.is_empty() {
        return "(no files)".into();
    }
    let dirs = aggregate_dirs(entries);
    let mut out = String::new();
    let mut last_dir: Option<String> = None;
    for entry in &filtered {
        let dir = entry.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if last_dir.as_deref() != Some(dir) {
            last_dir = Some(dir.to_string());
            if !dir.is_empty() {
                let stat = dirs.get(dir).cloned().unwrap_or_default();
                let note = annotations
                    .dirs
                    .get(dir)
                    .map(|n| format!("  · {n}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "{dir}/  ({} files, {}, {} lines){note}\n",
                    stat.files,
                    human_size(stat.size),
                    stat.lines
                ));
            }
        }
        let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
        let note = annotations
            .files
            .get(&entry.path)
            .filter(|a| a.hash == entry.stamp)
            .map(|a| format!("  · {}", a.note))
            .unwrap_or_default();
        let indent = if dir.is_empty() { "" } else { "  " };
        out.push_str(&format!("{indent}{name}  {}{note}\n", measure_label(entry)));
    }
    out
}

/// top-N 重文件(按行数;无行数的按大小排在其后)。中性数据,不带"该拆"判断。
pub fn render_top(entries: &[FileEntry], annotations: &AnnotationStore, top: usize) -> String {
    let mut sorted: Vec<&FileEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        b.lines
            .unwrap_or(0)
            .cmp(&a.lines.unwrap_or(0))
            .then(b.size.cmp(&a.size))
    });
    let mut out = String::from("files by line count (largest first):\n");
    for entry in sorted.iter().take(top) {
        let note = annotations
            .files
            .get(&entry.path)
            .filter(|a| a.hash == entry.stamp)
            .map(|a| format!("  · {}", a.note))
            .unwrap_or_default();
        out.push_str(&format!("{}  {}{note}\n", entry.path, measure_label(entry)));
    }
    out
}

#[derive(Deserialize, JsonSchema)]
struct FilesInput {
    /// 子树前缀(仓库相对路径,如 "crates/kanzei-core");省略 = 整个仓库。
    #[serde(default)]
    path: Option<String>,
    /// 按行数降序取前 N(定位重文件);省略 = 树形输出。
    #[serde(default)]
    top: Option<usize>,
}

/// 文件导览工具:项目的架构地图。
pub struct FilesTool;

#[async_trait]
impl Tool for FilesTool {
    fn name(&self) -> &'static str {
        "files"
    }

    fn description(&self) -> String {
        "Project file map: directory tree with per-file size, code line counts (md char counts), \
         directory aggregates, and per-file purpose annotations when available. Use `path` for a \
         subtree, `top` for the N largest files by lines. Read-only; prefer this over shell \
         `ls`/`find`/`wc` for understanding project structure."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(FilesInput)).unwrap()
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: FilesInput = match crate::parse_input(self, input) {
            Ok(value) => value,
            Err(output) => return output,
        };
        // R-177 内容②:扫的是**代码树**。判据统一为「凡 `.kanzei/**` 资产走
        // project_root,凡仓库源码走 cwd」——read/glob/grep/write/edit 早就走 cwd,
        // 这里若还取主根,同一个 agent 会在两棵树之间读写:files 列主树的文件,
        // edit 按 cwd 落 worktree。
        let mut entries = scan(&ctx.cwd);
        if entries.is_empty() {
            return ToolOutput::ok("(no files)".to_string());
        }
        // 批注是 `.kanzei/file-annotations.json`,主根一份的资产,取 project_root 不变。
        let annotations = load_annotations(&ctx.project_root);
        let prefix = input
            .path
            .as_deref()
            .map(|p| p.trim_matches('/').replace('\\', "/"));
        let text = match input.top {
            Some(top) => {
                // D-327:top 视图曾丢掉 path 作用域,静默返回全仓重文件——拿到错
                // 作用域的数据不报错,比报错更危险。裁剪口径与 render_tree 一致。
                if let Some(prefix) = prefix.as_deref() {
                    entries.retain(|e| e.path.starts_with(prefix));
                }
                render_top(&entries, &annotations, top.clamp(1, 100))
            }
            None => render_tree(&entries, &annotations, prefix.as_deref()),
        };
        ToolOutput::ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-files-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
        std::fs::write(root.join("src/big.rs"), "x\n".repeat(500)).unwrap();
        std::fs::write(root.join("docs/note.md"), "中文字数按字符计,一共二十个字。").unwrap();
        std::fs::write(root.join("data.bin"), vec![0u8; 128]).unwrap();
        root
    }

    /// R-177 内容②:线上运行时 files 工具读的是**本线的树**。
    /// 主树同名文件内容不同时,不得读到主树那份——否则 agent 会照主树的形态
    /// 构造 edit,而 edit 按 cwd 落 worktree。
    #[tokio::test]
    async fn files读的是本线的树_不是主树() {
        let main_root = fixture("main");
        let worktree = fixture("worktree");
        // 两棵树同名文件内容不同,且各自有一个对方没有的文件。
        std::fs::write(worktree.join("src/lib.rs"), "fn only_in_worktree() {}\n").unwrap();
        std::fs::write(worktree.join("src/线内新增.rs"), "fn 线内() {}\n").unwrap();
        std::fs::write(main_root.join("src/只在主树.rs"), "fn 主树() {}\n").unwrap();
        let ctx = ToolCtx {
            cwd: worktree.clone(),
            project_root: main_root.clone(),
            ..Default::default()
        };
        let out = FilesTool.execute(serde_json::json!({}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("线内新增.rs"),
            "应列出本线树里的文件: {}",
            out.content
        );
        assert!(
            !out.content.contains("只在主树.rs"),
            "不得列出主树独有的文件: {}",
            out.content
        );
        std::fs::remove_dir_all(&worktree).ok();
        std::fs::remove_dir_all(&main_root).ok();
    }

    /// D-327:top 视图必须尊重 path 作用域——曾静默返回全仓重文件。
    #[tokio::test]
    async fn top视图尊重path作用域() {
        let root = fixture("top-scope");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let out = FilesTool
            .execute(serde_json::json!({"path": "docs", "top": 5}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("docs/note.md"), "{}", out.content);
        assert!(
            !out.content.contains("src/big.rs"),
            "top 不得越出 path 作用域: {}",
            out.content
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 扫描区分代码行数与md字数且目录聚合正确() {
        let root = fixture("scan");
        let entries = scan(&root);
        let by_path = |p: &str| entries.iter().find(|e| e.path == p).unwrap();
        assert_eq!(by_path("src/lib.rs").lines, Some(4)); // 3 行内容 + 末尾换行计法
        assert_eq!(by_path("src/big.rs").lines, Some(501));
        let md = by_path("docs/note.md");
        assert!(md.chars.is_some() && md.lines.is_none(), "{md:?}");
        assert!(by_path("data.bin").lines.is_none() && by_path("data.bin").chars.is_none());

        let dirs = aggregate_dirs(&entries);
        assert_eq!(dirs.get("src").unwrap().files, 2);
        assert_eq!(dirs.get("src").unwrap().lines, 505);
        assert_eq!(dirs.get("").unwrap().files, entries.len());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 树输出带度量_top按行数降序_标注随树() {
        let root = fixture("tree");
        let entries = scan(&root);
        let mut ann = AnnotationStore::default();
        let big = entries.iter().find(|e| e.path == "src/big.rs").unwrap();
        ann.files.insert(
            "src/big.rs".into(),
            Annotation {
                hash: big.stamp.clone(),
                note: "大文件样例".into(),
            },
        );
        // 过期标注(hash 不匹配)不得出现。
        ann.files.insert(
            "src/lib.rs".into(),
            Annotation {
                hash: "stale".into(),
                note: "过期标注".into(),
            },
        );
        ann.dirs.insert("src".into(), "源码目录".into());

        let tree = render_tree(&entries, &ann, None);
        assert!(
            tree.contains("src/") && tree.contains("505 lines"),
            "{tree}"
        );
        assert!(tree.contains("big.rs") && tree.contains("501 行"), "{tree}");
        assert!(
            tree.contains("大文件样例") && tree.contains("源码目录"),
            "{tree}"
        );
        assert!(
            !tree.contains("过期标注"),
            "hash 不匹配的标注必须被过滤:\n{tree}"
        );
        assert!(tree.contains("字"), "md 字数缺失:\n{tree}");

        let top = render_top(&entries, &ann, 2);
        let big_pos = top.find("src/big.rs").unwrap();
        let lib_pos = top.find("src/lib.rs").unwrap_or(usize::MAX);
        assert!(big_pos < lib_pos, "top 必须按行数降序:\n{top}");

        // 子树过滤。
        let sub = render_tree(&entries, &ann, Some("docs"));
        assert!(sub.contains("note.md") && !sub.contains("big.rs"), "{sub}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 标注缓存写读回环且指纹随内容变化() {
        let root = fixture("cache");
        let entries = scan(&root);
        let lib = entries.iter().find(|e| e.path == "src/lib.rs").unwrap();
        let mut store = AnnotationStore::default();
        store.files.insert(
            "src/lib.rs".into(),
            Annotation {
                hash: lib.stamp.clone(),
                note: "工具函数集合".into(),
            },
        );
        save_annotations(&root, &store).unwrap();
        let loaded = load_annotations(&root);
        assert_eq!(loaded.files.get("src/lib.rs").unwrap().note, "工具函数集合");

        // mtime 免疫(D-213):重写**相同内容**,mtime 变而指纹不变——git checkout/
        // 触碰时间戳不得让标注大面积假失效。
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(root.join("src/lib.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
        let same = scan(&root);
        let lib_same = same.iter().find(|e| e.path == "src/lib.rs").unwrap();
        assert_eq!(lib.stamp, lib_same.stamp, "内容没变,指纹不得因 mtime 漂移");

        // 内容变化 → stamp 变 → 旧标注失效。
        std::fs::write(root.join("src/lib.rs"), "fn a() {}\nfn changed() {}\n").unwrap();
        let rescanned = scan(&root);
        let lib2 = rescanned.iter().find(|e| e.path == "src/lib.rs").unwrap();
        assert_ne!(lib.stamp, lib2.stamp, "内容变了指纹必须变");
        let tree = render_tree(&rescanned, &loaded, None);
        assert!(!tree.contains("工具函数集合"), "过期标注不得注入:\n{tree}");
        std::fs::remove_dir_all(root).ok();
    }

    // D-233 批2(验收③④):增量扫描——未变文件按 size+mtime 复用不上读;
    // vendor/gen 路径只 stat 不读内容,measurable 集合缩到项目自有源码。
    #[test]
    fn 增量扫描复用未变文件_vendor路径不读内容() {
        let root = fixture("incr");
        std::fs::create_dir_all(root.join("ui/vendor")).unwrap();
        std::fs::write(
            root.join("ui/vendor/editor.js"),
            "// vendor 大文件\n".repeat(100),
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn a() {}\nfn b() {}\n").unwrap();

        // 首次扫描:vendor 文件在树里可见(大小有值),但 lines/chars 为空、
        // 不进 annotatable——只 stat 不读内容。
        let (first, reused_first) = scan_incremental(&root, None);
        assert_eq!(reused_first, 0, "首次扫描没有可复用的");
        let vendor = first
            .iter()
            .find(|e| e.path == "ui/vendor/editor.js")
            .unwrap();
        assert!(
            vendor.lines.is_none() && vendor.chars.is_none(),
            "vendor 不得读内容:\n{vendor:?}"
        );
        assert!(vendor.size > 0, "vendor 仍应显示大小(树里可见)");
        assert!(
            !first
                .iter()
                .any(|e| e.lines.is_some() && e.path.starts_with("ui/vendor")),
            "vendor 不得进入可标注集合"
        );

        // 第二次扫描(内容未变):全部复用,reused 计数等于文件数——缓存命中证据。
        let (second, reused_second) = scan_incremental(&root, Some(&first));
        assert_eq!(
            reused_second,
            first.len(),
            "未变文件应全部复用: {reused_second}/{}",
            first.len()
        );
        assert_eq!(second.len(), first.len(), "条目数不变");
        let lib = first.iter().find(|e| e.path == "src/lib.rs").unwrap();
        let lib2 = second.iter().find(|e| e.path == "src/lib.rs").unwrap();
        assert_eq!(lib.stamp, lib2.stamp, "复用后指纹一致");

        // 内容变化 → 该文件重新读,stamp 变,reused 减少一个。
        std::fs::write(root.join("src/lib.rs"), "fn a() {}\nfn changed() {}\n").unwrap();
        let (third, reused_third) = scan_incremental(&root, Some(&second));
        assert_eq!(reused_third, first.len() - 1, "只有改过的文件重扫");
        let lib3 = third.iter().find(|e| e.path == "src/lib.rs").unwrap();
        assert_ne!(lib2.stamp, lib3.stamp, "内容变了必须重读出新指纹");

        std::fs::remove_dir_all(root).ok();
    }
}
