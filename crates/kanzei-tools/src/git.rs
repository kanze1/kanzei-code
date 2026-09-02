//! 结构化 Git 工具：把高频 status/diff/stage/commit 从任意 shell 文本收口为可校验契约。
//! stage 只接受逐文件路径；commit 必须携带上一轮 stage 返回的暂存区指纹，避免夹带旧改动。

use std::collections::{hash_map::DefaultHasher, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::Path;

use kanzei_harness::{ToolCtx, ToolOutput};
use serde::{Deserialize, Serialize};

mod commands;
pub(crate) use commands::{run_git, run_git_owned};
mod finalize;
mod plan;
pub(crate) use plan::build_commit_plan;
mod tool;
pub(crate) use tool::normalize_files;
pub use tool::GitTool;
mod worktree;
pub(crate) use worktree::worktree_for_branch;
pub use worktree::{parse_worktree_list, WorktreeEntry};

async fn staged_paths(cwd: &Path) -> Result<Vec<String>, String> {
    // D-347:必须 -c core.quotepath=false——git 默认对非 ASCII 路径输出带引号的
    // 八进制转义("docs/\347\233\256\345\275\225.md"),与请求的真实 UTF-8 路径
    // 比较必不相等,含中文文件名的暂存区会让后续 stage 全部误判 foreign。
    let text = run_git(
        cwd,
        &[
            "-c",
            "core.quotepath=false",
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
        ],
    )
    .await?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StageRequest {
    token: String,
    paths: Vec<String>,
}

fn stage_manifest_path(cwd: &Path) -> Result<std::path::PathBuf, String> {
    let marker = cwd.join(".git");
    let git_dir = if marker.is_dir() {
        marker
    } else {
        let text = std::fs::read_to_string(&marker)
            .map_err(|e| format!("cannot read .git worktree pointer: {e}"))?;
        let raw = text
            .strip_prefix("gitdir:")
            .map(str::trim)
            .ok_or("invalid .git worktree pointer")?;
        let path = std::path::PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    let key_path = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut hasher = DefaultHasher::new();
    key_path.to_string_lossy().hash(&mut hasher);
    Ok(git_dir.join(format!(
        "kanzei-stage-request-{:016x}.json",
        hasher.finish()
    )))
}

fn load_stage_request(cwd: &Path) -> Result<Option<(std::path::PathBuf, StageRequest)>, String> {
    let path = stage_manifest_path(cwd)?;
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read stage request manifest: {e}"))?;
    let request = serde_json::from_str(&text)
        .map_err(|e| format!("invalid stage request manifest {}: {e}", path.display()))?;
    Ok(Some((path, request)))
}

fn write_stage_request(path: &Path, request: &StageRequest) -> Result<(), String> {
    let text = serde_json::to_string_pretty(request)
        .map_err(|e| format!("serialize stage request: {e}"))?;
    crate::atomic_file::write_atomic(path, &text).map_err(|e| e.to_string())
}

fn remove_stage_request(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove stage request manifest: {error}")),
    }
}

fn new_stage_token(cwd: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    cwd.to_string_lossy().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
        .hash(&mut hasher);
    format!("stage-{:016x}", hasher.finish())
}

fn union_paths(existing: &[String], requested: &[String]) -> Vec<String> {
    let mut paths: BTreeSet<String> = existing.iter().cloned().collect();
    paths.extend(requested.iter().cloned());
    paths.into_iter().collect()
}

async fn staged_state(cwd: &Path) -> Result<(String, String, Vec<String>), String> {
    let diff = run_git(
        cwd,
        &[
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-color",
        ],
    )
    .await?;
    let paths = staged_paths(cwd).await?;
    let mut hasher = DefaultHasher::new();
    diff.hash(&mut hasher);
    let hash = format!("{:016x}", hasher.finish());
    Ok((hash, diff, paths))
}

/// D-332 验收④:暂存**源码**的指纹——test_record 收尾时用它背书「测试跑的是
/// 这份源码」,提交门禁优先比指纹而不是纯 mtime。要点:
/// - 只对源码路径(`is_source_path`)求指纹:tests.md/tracker 等托管文档的写入不改变
///   源码指纹,不会再让「test_record 自己改 tests.md」触发源码重测。
/// - v2 形态是**按路径的 staged blob 清单**(`v2 path@sha12,…`,按路径排序),不再 hash
///   diff 文本。旧形态 hash 的 diff 里带 `index <old>..<new>` 行,old 侧随 HEAD 走——
///   同一轮里分批提交时,第一批一落地 HEAD 就动了,剩余批次源码没变指纹也变,证据
///   凭空作废逼出全量复跑。blob 清单只看暂存内容,对 HEAD 移动免疫;门禁还能按路径
///   做子集背书(见 `fingerprint_endorses`),部分提交不再要求重测。
/// - 与 staged_state 的全体 hash 不同:commit 门禁的 CAS 用全体 hash(防任何内容漂移),
///   测试背书用源码清单(测后改动源码 → 对应路径 blob 变 → 要求重测,保守正确)。
/// - 同步实现(内部 std::process::Command 跑 git):test_record 工具(async)与
///   source_test_gate(同步)都要用,拆两个 async 版本会让门禁被迫改签名。
pub fn staged_source_fingerprint(cwd: &Path) -> Result<String, String> {
    let mut command = std::process::Command::new("git");
    // D-369:kzapp 是 GUI 进程无控制台,子进程跑 git 不隐藏会被 Windows 新建控制台
    // 窗口——提交门禁每次调用都弹黑窗闪现。与 D-238 的 async 路径同纪律。
    crate::hide_console(&mut command);
    let output = command
        .args([
            "diff",
            "--cached",
            "--raw",
            "--no-ext-diff",
            "--no-color",
            "--no-renames",
            "--abbrev=40",
            "-z",
        ])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("cannot run git diff: {e}"))?;
    if !output.status.success() {
        // 无 HEAD 等罕见形态:退化为空指纹,门禁走 mtime 路径,不硬报错。
        return Ok(String::new());
    }
    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    // -z 格式:`:<old_mode> <new_mode> <old_sha> <new_sha> <status>\0<path>\0` 重复。
    // --no-renames 保证每条恰好一个路径。
    let mut entries: Vec<String> = Vec::new();
    let mut parts = raw.split('\0');
    while let Some(meta) = parts.next() {
        if meta.is_empty() {
            continue;
        }
        let Some(path) = parts.next() else { break };
        let fields: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
        if fields.len() < 5 || !is_source_path(path) {
            continue;
        }
        // 删除的新 sha 是全零,天然与修改/新增区分,不需要单独带 status。
        let new_sha: String = fields[3].chars().take(12).collect();
        entries.push(format!("{}@{new_sha}", path.replace('\\', "/")));
    }
    if entries.is_empty() {
        // 本次暂存全是非源码(测试记录/文档):指纹为空,门禁按旧逻辑(mtime)走。
        return Ok(String::new());
    }
    entries.sort();
    Ok(format!("v2 {}", entries.join(",")))
}

/// 测试背书面指纹:**工作区**里全部已变更源码的内容清单(暂存与否无关)。
///
/// test_record 收尾时用它——测试跑的是工作区文件,不是 index;此前录的是暂存清单,
/// 于是「跑测试 → test_record → git stage → commit」这个自然顺序必被拦(录制那一刻
/// 目标文件还没暂存,背书清单里没有它),agent 只能白跑一轮重测。改成工作区内容后
/// 录制/暂存先后无关:commit 门禁的 current 侧仍取暂存清单(那才是要提交的内容),
/// 子集判定不变——暂存内容与测试时工作区一致即放行,测后又改过的文件照样点名拦截。
pub fn source_endorsement_fingerprint(cwd: &Path) -> Result<String, String> {
    let mut command = std::process::Command::new("git");
    // D-369:GUI 进程跑 git 必须隐藏控制台窗口。
    crate::hide_console(&mut command);
    let output = command
        .args([
            "-c",
            "core.quotepath=false", // D-347:非 ASCII 路径以真实 UTF-8 返回
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
        ])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("cannot run git status: {e}"))?;
    if !output.status.success() {
        // 非 git 目录等形态:退化为空指纹,门禁走 mtime 路径。
        return Ok(String::new());
    }
    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    // porcelain v1 -z:`XY <path>\0` 重复(--no-renames 下无第二路径段)。
    let mut present: Vec<String> = Vec::new();
    let mut entries: Vec<String> = Vec::new();
    for item in raw.split('\0') {
        if item.len() < 4 {
            continue;
        }
        let path = item[3..].trim();
        if path.is_empty() || !is_source_path(path) {
            continue;
        }
        if cwd.join(path).is_file() {
            present.push(path.to_string());
        } else {
            // 工作区已删除:与暂存删除的全零 sha 对齐。
            entries.push(format!("{}@000000000000", path.replace('\\', "/")));
        }
    }
    if !present.is_empty() {
        // 一次子进程批量算 blob hash:--stdin-paths 按行读路径,与工作区文件内容
        // 一一对应,输出行序与输入一致。
        let mut hash_command = std::process::Command::new("git");
        crate::hide_console(&mut hash_command);
        let mut child = hash_command
            .args(["hash-object", "--stdin-paths"])
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot run git hash-object: {e}"))?;
        {
            use std::io::Write;
            let stdin = child
                .stdin
                .as_mut()
                .ok_or("hash-object stdin unavailable")?;
            for path in &present {
                writeln!(stdin, "{path}").map_err(|e| format!("hash-object write: {e}"))?;
            }
        }
        let hashed = child
            .wait_with_output()
            .map_err(|e| format!("hash-object failed: {e}"))?;
        if !hashed.status.success() {
            return Ok(String::new());
        }
        let shas = String::from_utf8_lossy(&hashed.stdout).to_string();
        for (path, sha) in present.iter().zip(shas.lines()) {
            let short: String = sha.trim().chars().take(12).collect();
            if short.is_empty() {
                continue;
            }
            entries.push(format!("{}@{short}", path.replace('\\', "/")));
        }
    }
    if entries.is_empty() {
        return Ok(String::new());
    }
    entries.sort();
    Ok(format!("v2 {}", entries.join(",")))
}

/// 按拟提交文件计算工作区源码指纹。提交前计划不能直接使用全工作区指纹：
/// 工作区可能同时存在本轮不提交的源码改动，计划必须只评估 safe stage set 的候选文件。
pub(crate) fn source_endorsement_fingerprint_for_paths(
    cwd: &Path,
    paths: &[String],
) -> Result<String, String> {
    let mut present = Vec::new();
    let mut entries = Vec::new();
    for path in paths.iter().filter(|path| is_source_path(path)) {
        if cwd.join(path).is_file() {
            present.push(path.clone());
        } else {
            entries.push(format!("{}@000000000000", path.replace('\\', "/")));
        }
    }
    if !present.is_empty() {
        let mut hash_command = std::process::Command::new("git");
        crate::hide_console(&mut hash_command);
        let mut child = hash_command
            .args(["hash-object", "--stdin-paths"])
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot run git hash-object: {e}"))?;
        {
            use std::io::Write;
            let stdin = child
                .stdin
                .as_mut()
                .ok_or("hash-object stdin unavailable")?;
            for path in &present {
                writeln!(stdin, "{path}").map_err(|e| format!("hash-object write: {e}"))?;
            }
        }
        let hashed = child
            .wait_with_output()
            .map_err(|e| format!("hash-object failed: {e}"))?;
        if !hashed.status.success() {
            return Ok(String::new());
        }
        let shas = String::from_utf8_lossy(&hashed.stdout).to_string();
        for (path, sha) in present.iter().zip(shas.lines()) {
            let short: String = sha.trim().chars().take(12).collect();
            if short.is_empty() {
                continue;
            }
            entries.push(format!("{}@{short}", path.replace('\\', "/")));
        }
    }
    if entries.is_empty() {
        return Ok(String::new());
    }
    entries.sort();
    Ok(format!("v2 {}", entries.join(",")))
}

/// v2 指纹解析:`v2 path@sha12,…` → 路径到 blob 前缀的映射;非 v2 形态返回 None。
fn parse_fingerprint_entries(
    fingerprint: &str,
) -> Option<std::collections::BTreeMap<String, String>> {
    let list = fingerprint.strip_prefix("v2 ")?;
    let mut map = std::collections::BTreeMap::new();
    for entry in list.split(',') {
        let (path, sha) = entry.rsplit_once('@')?;
        map.insert(path.to_string(), sha.to_string());
    }
    Some(map)
}

/// 记录指纹是否背书当前暂存源码。相等直接成立;v2 形态额外允许**子集背书**:
/// 当前暂存的每个源码路径,其 blob 都出现在记录清单里即可——同一轮测试背书的
/// 一批改动分多笔提交时,后续批次源码未动就不再要求重测。
pub(crate) fn fingerprint_endorses(record: &str, current: &str) -> bool {
    if record == current {
        return true;
    }
    let (Some(endorsed), Some(staged)) = (
        parse_fingerprint_entries(record),
        parse_fingerprint_entries(current),
    ) else {
        return false;
    };
    staged
        .iter()
        .all(|(path, sha)| endorsed.get(path) == Some(sha))
}

/// 当前暂存里未被记录背书的源码路径(用于把拦截消息说到文件粒度)。
fn unendorsed_paths(record: &str, current: &str) -> Vec<String> {
    let endorsed = parse_fingerprint_entries(record).unwrap_or_default();
    parse_fingerprint_entries(current)
        .unwrap_or_default()
        .into_iter()
        .filter(|(path, sha)| endorsed.get(path) != Some(sha))
        .map(|(path, _)| path)
        .collect()
}

// 旧同步版 staged_paths_sync 已随 diff 文本指纹一起退役:v2 指纹从
// `diff --cached --raw -z` 一次拿到路径与 blob,不再需要单独列路径。

async fn stage(
    project_root: &Path,
    cwd: &Path,
    raw_files: &[String],
    requested_token: Option<&str>,
) -> ToolOutput {
    let files = match normalize_files(cwd, raw_files, true) {
        Ok(files) => files,
        Err(error) => return ToolOutput::error(error),
    };
    let plan = match build_commit_plan(project_root, cwd, &files).await {
        Ok(plan) => plan,
        Err(error) => return ToolOutput::error(format!("[stage] commit_plan failed: {error}")),
    };
    if !plan.unsafe_files.is_empty() || !plan.missing_evidence.is_empty() {
        return plan.blocker("stage");
    }

    let existing = match staged_paths(cwd).await {
        Ok(paths) => paths,
        Err(error) => return ToolOutput::error(error),
    };
    let loaded = match load_stage_request(cwd) {
        Ok(request) => request,
        Err(error) => return ToolOutput::error(error),
    };
    let (manifest_path, token, prior_scope) = match loaded {
        Some((path, request)) => {
            if let Some(requested) = requested_token.filter(|value| !value.trim().is_empty()) {
                if requested != request.token {
                    return ToolOutput::error(format!(
                        "REFUSING to stage: request token `{requested}` does not match active request `{}`.",
                        request.token
                    ));
                }
            }
            let foreign: Vec<String> = existing
                .iter()
                .filter(|path| !request.paths.contains(path))
                .cloned()
                .collect();
            if !foreign.is_empty() {
                return ToolOutput::error(format!(
                    "REFUSING to stage: the index contains paths outside active request `{}`: {}. Commit or unstage them deliberately first.",
                    request.token,
                    foreign.join(", ")
                ));
            }
            (path, request.token, request.paths)
        }
        None => {
            if !existing.is_empty() {
                return ToolOutput::error(format!(
                    "REFUSING to stage: the index already contains paths outside this request: {}. Commit or unstage them deliberately first.",
                    existing.join(", ")
                ));
            }
            let path = match stage_manifest_path(cwd) {
                Ok(path) => path,
                Err(error) => return ToolOutput::error(error),
            };
            let token = requested_token
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| new_stage_token(cwd));
            (path, token, Vec::new())
        }
    };

    let scope = union_paths(&prior_scope, &files);
    let pending = StageRequest {
        token: token.clone(),
        paths: scope.clone(),
    };
    if let Err(error) = write_stage_request(&manifest_path, &pending) {
        return ToolOutput::error(format!("[stage] cannot persist request manifest: {error}"));
    }

    let mut args = vec!["add".to_string(), "--".into()];
    args.extend(files.clone());
    if let Err(error) = run_git_owned(cwd, &args).await {
        return ToolOutput::error(error);
    }
    let (hash, diff, paths) = match staged_state(cwd).await {
        Ok(state) => state,
        Err(error) => return ToolOutput::error(error),
    };
    if paths.is_empty() {
        return ToolOutput::error("nothing is staged after this request".to_string());
    }
    let unexpected: Vec<String> = paths
        .iter()
        .filter(|path| !scope.contains(path))
        .cloned()
        .collect();
    if !unexpected.is_empty() {
        return ToolOutput::error(format!(
            "REFUSING to stage: the index changed during this request and contains out-of-scope paths: {}.",
            unexpected.join(", ")
        ));
    }
    if let Err(error) = write_stage_request(
        &manifest_path,
        &StageRequest {
            token: token.clone(),
            paths: paths.clone(),
        },
    ) {
        return ToolOutput::error(format!("[stage] cannot finalize request manifest: {error}"));
    }
    // D-263:暂存成功后对照工作区,把「本次请求之外的未暂存改动」点名写进返回。
    let unstaged = unstaged_changes(cwd).await.unwrap_or_default();
    let mut base = format!(
        "stage_request: {token}\nstaged {} file(s): {}\nstaged_hash: {hash}\nReview with `git diff` using staged=true, then commit with this exact expected_hash.",
        paths.len(),
        paths.join(", ")
    );
    if !unstaged.is_empty() {
        base.push_str(&format!(
            "\nNote: the working tree also contains {} change(s) NOT staged by this request (left untouched): {}",
            unstaged.len(),
            unstaged.join(", ")
        ));
    }
    ToolOutput::ok(base).with_display(serde_json::json!({
        "kind": "terminal",
        "command": "git stage (structured)",
        "output": diff.chars().take(4000).collect::<String>(),
    }))
}

/// 工作区里**未暂存**的改动(修改/删除/未跟踪),供 stage 后对照点名(D-263)。
/// `git status --porcelain` 输出形如 ` M path`(修改)、` D path`(删除)、
/// `?? path`(未跟踪);未跟踪目录会折叠成 `?? dir/` 一行,这里原样保留目录名。
async fn unstaged_changes(cwd: &Path) -> Result<Vec<String>, String> {
    let text = run_git(
        cwd,
        &["status", "--porcelain", "--untracked-files=all", "-z"],
    )
    .await?;
    Ok(text
        .split('\0')
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            // -z 格式:XY<space>path(XY 两字符状态码)。
            let bytes = line.as_bytes();
            if bytes.len() < 4 || bytes[2] != b' ' {
                return None;
            }
            let x = bytes[0] as char;
            let y = bytes[1] as char;
            // 已暂存的改动(X 位非空)不是"未纳入本次请求"的对象;只报未暂存部分。
            let path_part = &line[3..];
            let staged = x != ' ' && x != '?';
            if staged && y == ' ' {
                None
            } else {
                Some(path_part.to_string())
            }
        })
        .collect())
}

/// 提交里算「源码」的路径。改这两棵树就要有测试背书;`.kanzei/` 下的文档不算。
///
/// R-261:kanzei-app/ui/ 下的纯前端资源(js/css/html)不算 Rust source——它们由前端
/// 冒烟集(ui-runtime/lint/i18n/a11y/markdown,语法面由 ESLint 覆盖;R-228 强制前端标签条目
/// 关闭前有 passed 冒烟)背书,要求 cargo test -p kanzei-app 跑全套 Rust 测试对它们
/// 零信息量(实测 R-260 改 10 行 js 被迫重跑 163 个 Rust 测试)。staged 同时含 Rust
/// 源码与前端资源时,Rust 部分仍按原规则要求测试背书,不受影响。
pub(crate) fn is_source_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let frontend_resource = path.starts_with("crates/kanzei-app/ui/");
    (path.starts_with("crates/") || path.starts_with("scripts/"))
        && !path.contains("/.kanzei/")
        && !frontend_resource
}

/// 提交里算「tracker 文档」的路径:需求/缺陷/测试记录及其归档。
/// R-227 门禁只扫这些文件——占位符测试 ID 只可能出现在关闭证据/进展叙述里。
pub(crate) fn is_tracker_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let file = path.rsplit('/').next().unwrap_or(&path);
    matches!(
        file,
        "requirements.md"
            | "requirements-archive.md"
            | "defects.md"
            | "defects-archive.md"
            | "tests.md"
            | "tests-archive.md"
    )
}

/// R-227:占位符测试 ID 门禁。tracker 文件 diff 里出现 `T-\d+xxx` 形态的占位符
/// (真实测试 ID 是 `T-<10位时间戳>` 如 `T-1786565253`;占位符是数字后直接跟 xxx)
/// 即拒——把「全量跑过但没记 test_record」写成占位符,等于隔时凭记忆写证据,
/// R-198/R-199 的关闭证据就是这么漏出 D-320 的。只扫 tracker 文件的 diff 块,
/// 新增行(以 `+` 开头)与删除行(以 `-` 开头)都查:占位符不该出现在任何一侧。
fn placeholder_id_gate(diff: &str, paths: &[String]) -> Result<(), String> {
    let tracker: Vec<String> = paths
        .iter()
        .filter(|p| is_tracker_path(p))
        .cloned()
        .collect();
    if tracker.is_empty() {
        return Ok(());
    }
    // 占位符模式:`T-` + 至少一位数字 + `xxx`(数字不能为空,避免误伤 `T-xxx` 说明文字;
    // 也不能是完整 10 位时间戳——那是真 ID)。真实 ID 之后绝不该跟 `xxx`。
    // 手写扫描替代 regex:找 `T-` 后数连续数字,数字后紧跟 `xxx` 即命中。
    let is_placeholder = |line: &str| {
        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i + 6 <= bytes.len() {
            if &bytes[i..i + 2] == b"T-" {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 2 && line[j..].starts_with("xxx") {
                    return true;
                }
            }
            i += 1;
        }
        false
    };
    // D-357:只扫**新增行**。删除行里的占位符正是这次提交要清掉的东西,连它一起拒,
    // 等于门禁把自己配套的清理通道(archive_fill 回填)堵死——回填之后的 diff 必然
    // 带着 8 行 `-...T-1786565xxx...`,提交一次拒一次。人还能在 shell 里绕过去,
    // 自举 agent 只能走结构化 git 工具,没有退路,于是「按门禁要求回填」这件事
    // 在 agent 手里永远做不完。`+++ b/path` 是文件头不是内容,先剔掉。
    let mut hits: Vec<String> = Vec::new();
    for line in diff.lines() {
        let Some(added) = line.strip_prefix('+') else {
            continue;
        };
        if added.starts_with("++") {
            continue;
        }
        if is_placeholder(added) {
            hits.push(added.trim().to_string());
        }
    }
    if hits.is_empty() {
        return Ok(());
    }
    let truncated: Vec<String> = hits
        .iter()
        .map(|l| {
            if l.chars().count() > 160 {
                format!("{}…", l.chars().take(160).collect::<String>())
            } else {
                l.clone()
            }
        })
        .collect();
    Err(format!(
        "tracker 文件 diff 出现 {} 处占位符测试 ID(`T-<数字>xxx` 形态):\n{}\n\
         占位符 = 把「测试跑过但没记 test_record」隔时凭记忆写进证据,正是 D-320 根因链。\
         先 test_record 记真实 ID 再引用;存量占位符用 `archive_fill` 回填真值。",
        truncated.len(),
        truncated.join("\n")
    ))
}

fn modified_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// 编译底线门禁。R-210 曾把它降级成诊断回退(理由:clippy 的 `--all-targets` 走同一
/// 编译管线,双份全仓分析是冗余)。现已复位成**主门禁**:实测 `clippy --all-targets`
/// 37.9s 而 `check --all-targets` 只要 7.2s——同样的编译覆盖,四分之一的价钱。lint 那半
/// 由 clippy_gate 用不含测试目标的轻量形态接手。R-210 的判断没错,只是方向反了:
/// 该让便宜的那个覆盖编译,而不是让贵的那个顺带覆盖。
///
/// 为什么不能只看测试记录:记录是 agent 自己写的。实测 2026-08-09 夜里,run.rs 里连续
/// 混入四处「插入却把签名吃掉」的破损(`async fn fast_summarize -> ...` 少了参数、
/// `pub(crate) async fn run_promptpub(crate) fn run_metrics(` 两个签名黏在一起),
/// 而每个提交都配着一条 passed 记录——记录的时间戳比改动新,时序门禁完全满意,
/// 但 kanzei-app 根本编译不过。时序判据防的是「改完没重跑」,防不住「没跑却说跑了」。
/// 编译这条底线必须由工具亲自验。
async fn compile_gate(cwd: &Path) -> Result<(), String> {
    // 非 Rust 仓库不做这件事:门禁要么真的能验,要么不装样子。
    if !cwd.join("Cargo.toml").is_file() {
        return Ok(());
    }
    let mut command = tokio::process::Command::new("cargo");
    command
        .args(["check", "--workspace", "--all-targets", "--quiet"])
        .current_dir(cwd)
        // GitHub Actions 默认 CARGO_TERM_COLOR=always。捕获后的 ANSI 前缀会让
        // 下方按 `error` / `-->` 行筛选的诊断变空，子进程必须固定成机器可解析文本。
        .env("CARGO_TERM_COLOR", "never");
    crate::hide_console_async(&mut command);
    let output = command.output().await;
    match output {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let head: Vec<&str> = stderr
                .lines()
                .filter(|l| l.starts_with("error") || l.trim_start().starts_with("-->"))
                .take(12)
                .collect();
            Err(format!(
                "提交被拦下:`cargo check --workspace --all-targets` 不过,这份代码编译不了。\n{}",
                head.join("\n")
            ))
        }
        // cargo 跑不起来就说清楚,不要静默放行——放行等于门禁在说谎。
        Err(error) => Err(format!(
            "提交被拦下:无法执行 cargo check({error})。装好 cargo 或在非 Rust 仓库里提交。"
        )),
    }
}

/// fmt 门禁:提交源码前由工具**亲自**跑 `cargo fmt --all -- --check`(D-264)。
///
/// 与 compile_gate 同理由:测试记录是自报证据,挡不住「没跑却说跑了」。规则层
/// (conventions §1.4)写「提交前跑 fmt/clippy」已被自举漏掉三次(D-264 复现 +
/// 2026-08-12 第三次复发),第三次复发才确认必须代码强制。命令与 CI(ci.yml)
/// 和发版门禁(scripts/verify.ps1)完全同参数,任何一处增删门禁都要同步——
/// 守护测试 stage_fmt_clippy_gates_align_with_ci 比对三处清单。
async fn fmt_gate(cwd: &Path) -> Result<(), String> {
    if !cwd.join("Cargo.toml").is_file() {
        return Ok(());
    }
    let mut command = tokio::process::Command::new("cargo");
    command
        .args(["fmt", "--all", "--", "--check"])
        .current_dir(cwd)
        .env("CARGO_TERM_COLOR", "never");
    crate::hide_console_async(&mut command);
    let output = command.output().await;
    match output {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            // rustfmt 的 diff 清单走 stdout(Windows 上尤其),stderr 可能只有
            // "Diff in ..." 的行首;两路都读,避免漏掉违规文件清单。
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            // rustfmt --check 的违规清单长这样:`Diff in crates/foo/src/lib.rs at line 4:`
            let files: Vec<String> = combined
                .lines()
                .filter(|l| l.starts_with("Diff in "))
                .map(|l| {
                    l.strip_prefix("Diff in ")
                        .and_then(|s| s.split(" at line ").next())
                        .unwrap_or(l)
                        .to_string()
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let target = if files.is_empty() {
                "cargo fmt --all -- --check".to_string()
            } else {
                files.join(", ")
            };
            Err(format!(
                "提交被拦下:`cargo fmt --all -- --check` 不过,以下文件格式未归一。\
                 \n{target}\n先跑 `cargo fmt --all` 再提交(D-264)。"
            ))
        }
        Err(error) => Err(format!(
            "提交被拦下:无法执行 cargo fmt({error})。装好 cargo/rustfmt 或在非 Rust 仓库里提交。"
        )),
    }
}

/// 提交前的编译 + lint 硬门禁(D-264)。两条命令串起来跑,比原先单条
/// `cargo clippy --workspace --all-targets -- -D warnings` 便宜得多,而底线一分没丢:
///
///   1. `cargo check --workspace --all-targets --quiet`(compile_gate)——**编译底线**,
///      含测试代码。这条不可省:见 compile_gate 注释里 2026-08-09 的事故(四处破损
///      签名配着自写 passed 记录进库),编译必须由工具亲自验。
///   2. `cargo clippy --workspace -- -D warnings`——lint,**不含**测试目标。
///
/// 实测(碰 kanzei-harness/src/lib.rs 后,rust-lld 链接):
///   原 `clippy --all-targets` 37.9s  vs  `check --all-targets` 7.2s + `clippy` 5.0s = 12.2s
/// 省 25.7s。丢掉的只有**测试代码的 lint**,那份覆盖由 CI 每次 push 跑的
/// `cargo clippy --workspace --all-targets` 兜住(ci.yml)。
///
/// 这是刻意的三处分工——提交门禁(此处)与 verify.ps1 走轻量 lint,CI 走全量——
/// 由守护测试 gate_checklists_align_across_git_verify_and_ci 显式断言,不是漂移。
/// 代价明写:测试代码的 lint 违规会本地绿、push 后 CI 红。
///
/// 2026-08-11 实例(为什么 lint 仍必须全 workspace,不能退成 `-p <改动 crate>`):
/// 新增集成测试落在 crates/kanzei/tests/,自举只跑了「改动最多的 crate」的定向测试,
/// 6 条 lint 红灯随提交进库。所以这里保持 `--workspace`,只是不再 `--all-targets`。
async fn clippy_gate(cwd: &Path) -> Result<(), String> {
    if !cwd.join("Cargo.toml").is_file() {
        return Ok(());
    }
    // 编译底线先跑:编译不过时,报带 `-->` 的编译错误远比报 lint 有用,
    // 也省下在编译不了的代码上再跑一遍 lint 分析的时间。
    compile_gate(cwd).await?;

    let mut command = tokio::process::Command::new("cargo");
    command
        .args(["clippy", "--workspace", "--", "-D", "warnings"])
        .current_dir(cwd)
        .env("CARGO_TERM_COLOR", "never");
    crate::hide_console_async(&mut command);
    let output = command.output().await;
    match output {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let head: Vec<&str> = stderr
                .lines()
                .filter(|l| l.starts_with("error") || l.trim_start().starts_with("-->"))
                .take(16)
                .collect();
            Err(format!(
                "提交被拦下:`cargo clippy --workspace -- -D warnings` 不过。\n{}",
                head.join("\n")
            ))
        }
        Err(error) => Err(format!(
            "提交被拦下:无法执行 cargo clippy({error})。装好 cargo/clippy 或在非 Rust 仓库里提交。"
        )),
    }
}

/// 并行门禁失败聚合：fmt 与 clippy 都执行后一次性返回全部失败，避免第二个错误被首个错误遮蔽。
fn aggregate_gate_errors(
    fmt_result: Result<(), String>,
    clippy_result: Result<(), String>,
    context: &str,
) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(error) = fmt_result {
        errors.push(format!("[{context}] fmt gate failed:\n{error}"));
    }
    if let Err(error) = clippy_result {
        errors.push(format!("[{context}] clippy gate failed:\n{error}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n\n"))
    }
}

/// 源码提交的硬门禁:必须存在**改完之后**才收尾的 passed 测试记录。
///
/// 这条纪律此前只写在提示词里,实测一天里被绕过三次(R-158 顶掉 reasoning effort、
/// 批4/5 让 HEAD 编译不过、批6 漏 use Path),每次都是"跑了 cargo check 就提交"。
/// 判据放在工具层,提示词说什么都绕不过去。
fn source_test_gate(project_root: &Path, cwd: &Path, paths: &[String]) -> Result<(), String> {
    let sources: Vec<&String> = paths.iter().filter(|p| is_source_path(p)).collect();
    if sources.is_empty() {
        return Ok(());
    }
    // 删除的文件取不到 mtime,跳过;全是删除时没有可比的时间点,放行。
    let Some(newest_change) = sources
        .iter()
        .filter_map(|p| modified_secs(&cwd.join(p)))
        .max()
    else {
        return Ok(());
    };
    let listed = sources
        .iter()
        .take(5)
        .map(|p| format!("  - {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    let remedy = format!(
        "本次暂存的源码:\n{listed}{}\n\
         做法:跑 `cargo test --workspace`(或本次改动的定向 `cargo test -p <crate>`),\
         再用 test_record 记一条 status=passed(带上命令与摘要),然后重新 commit。\
         cargo check 不算——它编译不了测试目标,R-158 那处被顶掉的 reasoning effort 就是这么漏过去的。",
        if sources.len() > 5 { format!("\n  - …还有 {} 个文件", sources.len() - 5) } else { String::new() }
    );
    let current_fingerprint = staged_source_fingerprint(cwd).unwrap_or_default();
    let latest = if current_fingerprint.is_empty() {
        crate::test_record::last_passed(project_root)
    } else {
        crate::test_record::last_passed_for_fingerprint(project_root, &current_fingerprint)
            .or_else(|| crate::test_record::last_passed(project_root))
    };
    match latest {
        None => Err(format!("提交被拦下:没有任何 passed 的测试记录。\n{remedy}")),
        Some((passed_at, coverage, command_text, fingerprint)) => {
            // D-332 验收④:优先比源码指纹——测试记录背书的是「收尾那一刻的暂存源码」。
            // v2 清单按路径做子集背书(分批提交不再作废证据);旧格式(16 位 hex)与
            // v2 没有可比性,按「无指纹」降级走 mtime,避免升级后第一笔提交被假性拦下。
            // 指纹为空(旧记录/非 git)时同样退回 mtime。
            let comparable =
                fingerprint.starts_with("v2 ") && current_fingerprint.starts_with("v2 ");
            if comparable {
                if !fingerprint_endorses(&fingerprint, &current_fingerprint) {
                    let stale = unendorsed_paths(&fingerprint, &current_fingerprint);
                    return Err(format!(
                        "提交被拦下:最近一条 passed 测试记录背书的源码指纹与当前暂存源码不一致\
                         ——这些路径在测试之后又改过(或从未被背书):\n{}\n\
                         这条记录背书的不是要提交的这份代码。\n{remedy}",
                        stale
                            .iter()
                            .take(8)
                            .map(|p| format!("  - {p}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ));
                }
            } else if passed_at < newest_change {
                return Err(format!(
                    "提交被拦下:最近一条 passed 测试记录收尾于 {} 秒前,而暂存的源码在那之后又改过\
                     ({} 秒前)——这条记录背书的不是要提交的这份代码。\n{remedy}",
                    now_secs().saturating_sub(passed_at),
                    now_secs().saturating_sub(newest_change)
                ));
            }
            // R-212:相关性——暂存源码所属 crate 必须被最近 passed 记录覆盖。
            // 只按时间戳背书(改完没重跑)已经防不住「跑了 A 测试以为覆盖了 B」
            // 的诚实失误:前端冒烟记录的时间戳比 Rust 改动新,却背不了这份源码。
            let staged = source_crates(paths);
            let missing: Vec<String> = staged
                .iter()
                .filter(|c| !coverage.covers(c))
                .cloned()
                .collect();
            if missing.is_empty() {
                return Ok(());
            }
            let run_hint = missing
                .iter()
                .map(|c| format!("cargo test -p {c}"))
                .collect::<Vec<_>>()
                .join("、");
            Err(format!(
                "提交被拦下:最近一条 passed 测试记录(命令: {command_text})的覆盖面是 {}——\
                 不覆盖本次暂存源码所属 crate:{}。\n\
                 做法:跑 `{run_hint}`(或 `cargo test --workspace`),再用 test_record 记一条 \
                 status=passed(带上命令与摘要),然后重新 commit。cargo check 不算——它编译不了\
                 测试目标,R-158 那处被顶掉的 reasoning effort 就是这么漏过去的。",
                coverage.describe(),
                missing.join(", "),
            ))
        }
    }
}

/// 暂存源码所属 crate 集合(路径 `crates/<name>/...` → <name>;scripts/ 等不在 crate 内)。
pub(crate) fn source_crates(paths: &[String]) -> std::collections::BTreeSet<String> {
    paths
        .iter()
        .map(|p| p.replace('\\', "/"))
        .filter(|p| p.starts_with("crates/"))
        .filter_map(|p| {
            let mut parts = p.split('/');
            let _ = parts.next(); // "crates"
            parts.next().map(|name| name.to_string())
        })
        .collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn commit(
    ctx: &ToolCtx,
    message: Option<String>,
    expected_hash: Option<String>,
) -> ToolOutput {
    commit_with_gate_state(ctx, message, expected_hash, false).await
}

/// `gates_already_run`:finalize 在第 1 步已对同一工作树跑过 fmt_gate+clippy_gate,
/// 且步骤 1→6 之间只有 test_record(tracker md)与 stage(索引)两类非源码变更——
/// 同一门禁在一次调用里跑两遍是纯浪费(实测每次 12-15s,2026-08-20 门禁审计 P0-1)。
/// 直接调 commit 的路径照旧全量门禁,底线不丢。
async fn commit_with_gate_state(
    ctx: &ToolCtx,
    message: Option<String>,
    expected_hash: Option<String>,
    gates_already_run: bool,
) -> ToolOutput {
    let cwd = &ctx.cwd;
    let message = message.unwrap_or_default();
    if message.trim().is_empty() {
        return ToolOutput::error("`message` is required for commit");
    }
    let Some(expected_hash) = expected_hash else {
        return ToolOutput::error(
            "`expected_hash` is required; call `stage` and review its staged diff first",
        );
    };
    let (current_hash, staged_diff, paths) = match staged_state(cwd).await {
        Ok(state) => state,
        Err(error) => return ToolOutput::error(error),
    };
    if paths.is_empty() {
        return ToolOutput::error("nothing is staged; call `stage` with explicit files first");
    }
    if current_hash != expected_hash {
        return ToolOutput::error(format!(
            "staged content changed: expected `{expected_hash}`, current `{current_hash}`. Re-run stage/diff and review the new index before committing."
        ));
    }
    let plan = match build_commit_plan(&ctx.project_root, cwd, &paths).await {
        Ok(plan) => plan,
        Err(error) => return ToolOutput::error(format!("[commit] commit_plan failed: {error}")),
    };
    if !plan.unsafe_files.is_empty() || !plan.missing_evidence.is_empty() {
        return plan.blocker("commit");
    }
    // R-227:占位符测试 ID 门禁——tracker 文件 diff 里出现 `T-\d+xxx` 形态的占位符
    // (真实测试 ID 是 `T-<10位时间戳>`,占位符是数字后接 xxx)即拒绝提交。存量 8 处
    // (R-198/R-199/D-219/D-266/D-279/D-281/D-282/D-316 关闭证据)曾把「全量跑过但
    // 没记 test_record」写成占位符,隔时凭记忆写证据导致 R-198/R-199 的关闭证据含
    // 占位符仍无人核对(D-320 根因链)。门禁只扫 tracker 文件,源码 diff 不受影响。
    if let Err(error) = placeholder_id_gate(&staged_diff, &paths) {
        return ToolOutput::error(error);
    }
    // R-321 B2:重复/跨轮/阻塞的 execution incident 必须先与正式 D-ID 互链，
    // 防止提交把应晋升的过程失手继续伪装成瞬时 incident。
    if let Err(error) = crate::incident::commit_promotion_gate(&ctx.project_root) {
        return ToolOutput::error(format!("[commit] {error}"));
    }
    if !gates_already_run && paths.iter().any(|p| is_source_path(p)) {
        // 顺序有讲究:先验门禁(机械真值),再看测试记录(自报证据)。编译不过时
        // 报编译错误比报"没有测试背书"有用得多。D-264:fmt/clippy 为提交前硬门禁
        // ——规则层写过但自举漏了三次,必须代码强制。clippy_gate 内部先跑
        // compile_gate(check --all-targets,含测试代码的编译底线)再跑轻量 clippy。
        // R-261:fmt 与 clippy 互不依赖,并行执行——fmt --check 只读不写 target,
        // 与 clippy 的增量编译无资源竞争,串行只会让门禁多等一份时间。
        let (fmt_result, clippy_result) = tokio::join!(fmt_gate(cwd), clippy_gate(cwd));
        if let Err(error) = aggregate_gate_errors(fmt_result, clippy_result, "commit") {
            return ToolOutput::error(error);
        }
    }
    if let Err(error) = source_test_gate(&ctx.project_root, cwd, &paths) {
        return ToolOutput::error(error);
    }
    if let Err(error) = run_git_owned(cwd, &["commit".into(), "-m".into(), message]).await {
        return ToolOutput::error(error);
    }
    let cleanup_note = match load_stage_request(cwd) {
        Ok(Some((path, _))) => match remove_stage_request(&path) {
            Ok(()) => String::new(),
            Err(error) => {
                format!("\nWarning: committed but request manifest cleanup failed: {error}")
            }
        },
        Ok(None) => String::new(),
        Err(error) => {
            format!("\nWarning: committed but request manifest could not be read: {error}")
        }
    };
    match run_git(
        cwd,
        &["show", "--stat", "--no-color", "--format=%h %s", "HEAD"],
    )
    .await
    {
        Ok(stat) => ToolOutput::ok(format!(
            "committed verified staged set ({current_hash}){cleanup_note}\n{stat}"
        )),
        Err(error) => {
            ToolOutput::error(format!("commit succeeded but verification failed: {error}"))
        }
    }
}

/// D-334:finalize 事务化——把「fmt → 相关测试 → test_record → stage → CAS commit」
/// 收敛为一次机械调用,Agent 不再手动驾驶 Harness 状态机。
///
/// 内部顺序(任一失败立即返回该阶段,不留半状态):
///   1. `fmt_gate`(提交前代码门禁,D-264):fmt 不过先拦,省得 Agent 先 test 再被拦;
///   2. `clippy_gate`:与 commit 一致的全仓 clippy 硬门禁;
///   3. 按暂存源码推导相关 crate,构造定向测试命令(无 crate 改动时退化为
///      `cargo test --workspace` 由调用方显式指定 —— 见参数说明);
///   4. 跑测试(超时 10 分钟防挂死),失败返回测试输出;
///   5. `test_record::record_test_run_with_duration` 记 passed(带 source_fingerprint,
///      与 staged_source_fingerprint 一致,背书「这份源码测过」);
///   6. `stage`(显式文件,与既有 stage 同语义);
///   7. CAS commit(消费 stage 的 staged_hash,与既有 commit 同语义)。
///
/// 与手工「test→record→stage→commit」的差异:fmt/clippy 在测试**之前**拦,且全部
/// 在一个调用内完成——Agent 只发一次 finalize,不手动编排每一步。
/// 引用名校验:只放行分支/标签的常规形态。拒绝 `-` 开头(选项注入)、区间语法
/// (`..`)、修订运算符(`~`/`^`/`:`)与空白——merge_ff 只该拿到一个干净的名字。
fn validate_ref(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("引用名不能为空".into());
    }
    if name.starts_with('-')
        || name.contains("..")
        || name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '~' | '^' | ':' | '\\' | '*' | '?' | '['))
    {
        return Err(format!(
            "非法引用名 `{name}`:merge_ff 只接受干净的分支/标签名"
        ));
    }
    Ok(name.to_string())
}

/// 快进合并:分支级变更中唯一"要么无冲突成功、要么干净失败"的形态,所以可以
/// 开放给模型——不产生合并提交、不动索引内容、非快进直接拒绝(D-173 的边界不破)。
/// 发版流程(kanzei-release 树 ff dev→main)此前卡在"bash 拦 merge、工具没 merge"
/// 的空档里,只能让用户手跑,就是这个动作的由来。
async fn merge_ff(cwd: &Path, from: Option<String>, into: Option<String>) -> ToolOutput {
    let from = match from.as_deref().map(validate_ref) {
        Some(Ok(name)) => name,
        Some(Err(error)) => return ToolOutput::error(error),
        None => return ToolOutput::error("`from` is required for merge_ff (例如 dev)"),
    };
    // 来源必须能解析成提交,报错要在改任何东西之前。
    if let Err(error) = run_git(
        cwd,
        &["rev-parse", "--verify", &format!("{from}^{{commit}}")],
    )
    .await
    {
        return ToolOutput::error(format!("无法解析来源 `{from}`:{error}"));
    }
    let (target_label, merge_dir, ref_update) = match into.as_deref().map(validate_ref) {
        Some(Err(error)) => return ToolOutput::error(error),
        Some(Ok(into)) => {
            let porcelain = match run_git(cwd, &["worktree", "list", "--porcelain"]).await {
                Ok(text) => text,
                Err(error) => return ToolOutput::error(error),
            };
            match worktree_for_branch(&porcelain, &into) {
                // 分支检出在某棵工作树(可能就是当前树):去那棵树里做真正的
                // merge --ff-only,工作区文件与 HEAD 一起前进。
                Some(path) => (into, Some(path), None),
                // 谁都没检出:快进纯属引用更新,`git fetch . from:into` 天然拒绝非快进。
                None => (into.clone(), None, Some(format!("{from}:{into}"))),
            }
        }
        None => (String::from("HEAD"), Some(cwd.to_path_buf()), None),
    };
    let before = run_git(
        merge_dir.as_deref().unwrap_or(cwd),
        &["rev-parse", "--short", &target_label],
    )
    .await
    .unwrap_or_else(|_| "?".into());
    let result = match (&merge_dir, &ref_update) {
        (Some(dir), _) => run_git(dir, &["merge", "--ff-only", &from]).await,
        (None, Some(spec)) => run_git(cwd, &["fetch", ".", spec]).await,
        (None, None) => unreachable!("merge_ff target must be a worktree or a ref update"),
    };
    if let Err(error) = result {
        return ToolOutput::error(format!(
            "merge_ff 失败(只允许快进;历史分叉时先在 dev 侧收敛):{error}"
        ));
    }
    let after = run_git(
        merge_dir.as_deref().unwrap_or(cwd),
        &["rev-parse", "--short", &target_label],
    )
    .await
    .unwrap_or_else(|_| "?".into());
    let where_note = match &merge_dir {
        Some(dir) => format!("worktree {}", dir.display()),
        None => "ref-only update (branch not checked out anywhere)".into(),
    };
    ToolOutput::ok(format!(
        "fast-forwarded {target_label}: {before} -> {after} ({where_note})\nsource: {from}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_record::TEST_RUNS_REL;
    use kanzei_harness::Tool;

    /// R-227 验收①:tracker diff 出现 `T-<数字>xxx` 占位符即拒;真实 10 位 ID 放行;
    /// 非 tracker 文件不受影响;无占位符放行。
    #[test]
    fn placeholder_id_gate_拒绝占位符_放行真实_id与非tracker() {
        // 新增行带占位符 → 拒。
        let diff = "\
diff --git a/.kanzei/project/requirements-archive.md b/.kanzei/project/requirements-archive.md
index 0000000..1111111 100644
--- a/.kanzei/project/requirements-archive.md
+++ b/.kanzei/project/requirements-archive.md
@@ -1,1 +1,1 @@
+- 全量 cargo test --workspace 全绿(T-1786565xxx,harness 118)";
        let paths = vec![".kanzei/project/requirements-archive.md".into()];
        let err = placeholder_id_gate(diff, &paths).unwrap_err();
        assert!(err.contains("占位符"), "{err}");
        assert!(err.contains("T-1786565xxx"), "{err}");

        // 真实 10 位 ID → 放行。
        let real = "\
diff --git a/.kanzei/project/requirements-archive.md b/.kanzei/project/requirements-archive.md
+ 全量 cargo test --workspace 全绿(T-1786565346,harness 118)";
        assert!(
            placeholder_id_gate(real, &paths).is_ok(),
            "真实 ID 必须放行"
        );

        // 无占位符 → 放行。
        let clean = "\
diff --git a/.kanzei/project/requirements.md b/.kanzei/project/requirements.md
+ - 进展: 实现已落地,验证通过";
        assert!(placeholder_id_gate(clean, &paths).is_ok());

        // 非 tracker 文件即使含 `T-123xxx` 也不拦(源码/文档不在门禁范围)。
        let source = "\
diff --git a/crates/kanzei-tools/src/tracker.rs b/crates/kanzei-tools/src/tracker.rs
+ // T-123xxx 这里不是占位符,是代码示例";
        let source_paths = vec!["crates/kanzei-tools/src/tracker.rs".into()];
        assert!(placeholder_id_gate(source, &source_paths).is_ok());

        // 无 tracker 路径 → 直接放行(不扫)。
        assert!(placeholder_id_gate(diff, &source_paths).is_ok());

        // D-357 验收①:只删占位符的 diff 必须放行。archive_fill 回填后的清理提交
        // 就是这个形态——删掉带占位符的旧行、写回带真值的新行。连它一起拒,门禁
        // 就把自己配套的清理通道堵死了。
        let cleanup = "\
diff --git a/.kanzei/project/requirements-archive.md b/.kanzei/project/requirements-archive.md
--- a/.kanzei/project/requirements-archive.md
+++ b/.kanzei/project/requirements-archive.md
@@ -1,1 +1,1 @@
-- 全量 cargo test --workspace 全绿(T-1786565xxx,harness 118)
+- 全量 cargo test --workspace 全绿(T-1786565346,harness 118)";
        assert!(
            placeholder_id_gate(cleanup, &paths).is_ok(),
            "回填清理提交(删占位符、加真值)必须放行,否则 archive_fill 的成果提交不出去"
        );

        // D-357 验收③:diff 文件头不参与判定。`+++ b/xxx` 以 `+` 开头,但它是头不是内容。
        let header_only = "\
diff --git a/.kanzei/project/T-1786565xxx.md b/.kanzei/project/T-1786565xxx.md
--- a/.kanzei/project/T-1786565xxx.md
+++ b/.kanzei/project/T-1786565xxx.md
@@ -1,1 +1,1 @@
+- 进展: 一切正常";
        assert!(
            placeholder_id_gate(header_only, &paths).is_ok(),
            "占位符只出现在 diff 文件头里时不该拦"
        );

        // D-357 验收④:同一 diff 既删旧占位符又加新占位符 → 仍拒(新增的那个才是罪)。
        let mixed = "\
diff --git a/.kanzei/project/requirements-archive.md b/.kanzei/project/requirements-archive.md
--- a/.kanzei/project/requirements-archive.md
+++ b/.kanzei/project/requirements-archive.md
@@ -1,2 +1,2 @@
-- 旧证据(T-1786565xxx)
+- 新证据(T-1786566xxx)";
        let mixed_err = placeholder_id_gate(mixed, &paths).unwrap_err();
        assert!(
            mixed_err.contains("T-1786566xxx") && !mixed_err.contains("T-1786565xxx"),
            "只该点名新增的那个占位符,不该把被删掉的也算进去:{mixed_err}"
        );
    }

    /// R-177 内容③:解析器抽出来即补单测——它此前零直接覆盖,只被 merge_ff 间接用到。
    /// 表驱动覆盖 `--porcelain` 的全部行形态。
    #[test]
    fn parse_worktree_list识别分支_bare_detached_locked_prunable() {
        let porcelain = "\
worktree C:/proj/kanzei
HEAD 1111111111111111111111111111111111111111
branch refs/heads/dev

worktree C:/proj/.kanzei-worktree-kanzei.f9
HEAD 2222222222222222222222222222222222222222
branch refs/heads/kanzei/thread-f9

worktree C:/proj/detached-tree
HEAD 3333333333333333333333333333333333333333
detached

worktree C:/proj/bare-tree
bare

worktree C:/proj/locked-tree
HEAD 4444444444444444444444444444444444444444
detached
locked 手工锁住的原因

worktree C:/proj/gone-tree
HEAD 5555555555555555555555555555555555555555
branch refs/heads/gone
prunable gitdir file points to non-existent location
";
        let entries = parse_worktree_list(porcelain);
        assert_eq!(entries.len(), 6, "{entries:?}");

        assert_eq!(entries[0].path, std::path::PathBuf::from("C:/proj/kanzei"));
        assert_eq!(
            entries[0].branch.as_deref(),
            Some("dev"),
            "分支短名要剥前缀"
        );
        assert!(!entries[0].bare && !entries[0].detached);

        assert_eq!(
            entries[1].branch.as_deref(),
            Some("kanzei/thread-f9"),
            "含 / 的分支名不能被截断"
        );

        assert!(entries[2].detached && entries[2].branch.is_none());
        assert!(entries[3].bare && entries[3].branch.is_none());
        assert!(entries[4].locked, "locked 带原因串时也要认出来");
        assert!(entries[5].prunable, "prunable 带原因串时也要认出来");

        // 空输入与孤儿属性行都不能 panic,也不能凭空造出记录。
        assert!(parse_worktree_list("").is_empty());
        assert!(parse_worktree_list("branch refs/heads/x\nbare\n").is_empty());
    }

    fn temp_repo(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-git-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.invalid"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Kanzei Test"])
            .current_dir(&root)
            .status()
            .unwrap();
        root
    }

    #[test]
    fn paths_must_be_explicit_files() {
        let root = temp_repo("paths");
        std::fs::create_dir_all(root.join("src")).unwrap();
        assert!(normalize_files(&root, &[".".into()], true).is_err());
        assert!(normalize_files(&root, &["src".into()], true).is_err());
        assert!(normalize_files(&root, &["../x".into()], true).is_err());
        assert_eq!(
            normalize_files(&root, &["src/main.rs".into()], true).unwrap(),
            vec!["src/main.rs"]
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-178:normalize_resource 在 Windows 上把整条路径 to_lowercase,而 git pathspec
    /// 大小写敏感——小写化会让含大写字母的文件(INDEX.md、Cargo.lock)匹配不到,
    /// stage 静默失败。传给 git 的路径必须保留原始大小写,安全校验照常生效。
    #[test]
    fn paths_keep_original_case_while_still_escaping_check() {
        let root = temp_repo("case");
        std::fs::create_dir_all(root.join("src")).unwrap();
        // 大小写必须原样保留。
        assert_eq!(
            normalize_files(&root, &["INDEX.md".into()], true).unwrap(),
            vec!["INDEX.md"]
        );
        assert_eq!(
            normalize_files(&root, &["src/MyFile.rs".into()], true).unwrap(),
            vec!["src/MyFile.rs"]
        );
        // `..` 折叠等安全语义不受影响。
        assert_eq!(
            normalize_files(&root, &["./src/../INDEX.md".into()], true).unwrap(),
            vec!["INDEX.md"]
        );
        assert!(normalize_files(&root, &["../escape.txt".into()], true).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn stage_uppercase_paths_and_verify_index() {
        let root = temp_repo("case");
        std::fs::write(root.join("INDEX.md"), "# index\n").unwrap();
        std::fs::write(root.join("Cargo.lock"), "lock\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let staged = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["INDEX.md","Cargo.lock"]}),
                &ctx,
            )
            .await;
        assert!(!staged.is_error, "{}", staged.content);
        assert!(
            staged.content.contains("staged_hash: "),
            "{}",
            staged.content
        );
        // 暂存区里必须是原始大小写的路径,而不是被小写化的 index.md。
        let index = staged_paths(&root).await.unwrap();
        assert!(index.contains(&"INDEX.md".to_string()), "{index:?}");
        assert!(index.contains(&"Cargo.lock".to_string()), "{index:?}");
        assert!(!index.contains(&"index.md".to_string()), "{index:?}");
        std::fs::remove_dir_all(root).ok();
    }

    /// D-347:含非 ASCII(中文)文件名的暂存区,后续 stage 必须能被正常追加/覆盖。
    /// 根因是 staged_paths 读 index 路径时 git 默认 core.quotepath=true 输出带引号的
    /// 八进制转义,与请求的真实 UTF-8 路径比较必不相等——即使请求已显式包含该中文
    /// 路径,也会被误判为"index 里存在请求外路径"而拒绝(D-263 的覆盖检查是字面
    /// 集合比较,不能因表示形式不同而误判)。
    #[tokio::test]
    async fn stage_after_non_ascii_path_is_not_foreign() {
        let root = temp_repo("cn");
        std::fs::write(root.join("目录.md"), "# 手册\n").unwrap();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let first = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["目录.md"]}),
                &ctx,
            )
            .await;
        assert!(
            !first.is_error,
            "首次 stage 中文路径失败: {}",
            first.content
        );
        // 修复前:existing 里是转义串,请求已显式包含"目录.md"仍被拒(foreign 误报)。
        // D-263 覆盖检查要求请求列出全部既有路径,这里完整列出 = 正常追加语义。
        let second = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["目录.md","a.txt"]}),
                &ctx,
            )
            .await;
        assert!(
            !second.is_error,
            "请求已包含中文路径仍被误判 foreign: {}",
            second.content
        );
        // 暂存区路径必须以真实 UTF-8 呈现,而不是转义形式。
        let paths = staged_paths(&root).await.unwrap();
        assert!(paths.contains(&"目录.md".to_string()), "{paths:?}");
        assert!(paths.contains(&"a.txt".to_string()), "{paths:?}");
        assert!(
            !paths.iter().any(|p| p.contains("\\347")),
            "路径仍是 quotepath 转义形式: {paths:?}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-208:log 是只读查询,模型排查"最近改了什么"的高频入口——此前没有这个
    /// action,实测模型直接调 `{"action":"log"}` 被拒,只能转投 bash(每次 ask)。
    #[tokio::test]
    async fn log_returns_recent_commits_and_honors_path_filter() {
        let root = temp_repo("log");
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-q", "-m", "第一条:加入 a.txt"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "b.txt"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-q", "-m", "第二条:加入 b.txt"])
            .current_dir(&root)
            .status()
            .unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let all = GitTool
            .execute(serde_json::json!({"action": "log"}), &ctx)
            .await;
        assert!(!all.is_error, "{}", all.content);
        assert!(
            all.content.contains("第一条") && all.content.contains("第二条"),
            "{}",
            all.content
        );
        // count 生效:只要最近 1 条。
        let one = GitTool
            .execute(serde_json::json!({"action": "log", "count": 1}), &ctx)
            .await;
        assert!(
            one.content.contains("第二条") && !one.content.contains("第一条"),
            "{}",
            one.content
        );
        // 路径过滤:只看 a.txt 的历史。
        let filtered = GitTool
            .execute(
                serde_json::json!({"action": "log", "files": ["a.txt"]}),
                &ctx,
            )
            .await;
        assert!(
            filtered.content.contains("第一条") && !filtered.content.contains("第二条"),
            "{}",
            filtered.content
        );
        std::fs::remove_dir_all(root).ok();
    }

    fn git_in(dir: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed in {}", dir.display());
    }

    fn commit_file(dir: &std::path::Path, name: &str, content: &str, message: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        git_in(dir, &["add", name]);
        git_in(dir, &["commit", "-q", "-m", message]);
    }

    /// 发版形态:main 检出在另一棵工作树,merge_ff 要找到那棵树并在里面快进,
    /// 让分支引用与工作区文件一起前进——这是 bash 拦 merge 后发版流程的唯一通道。
    #[tokio::test]
    async fn merge_ff_fast_forwards_branch_checked_out_in_linked_worktree() {
        let root = temp_repo("ffwt");
        commit_file(&root, "a.txt", "v1\n", "初始提交");
        git_in(&root, &["branch", "rel"]);
        git_in(&root, &["switch", "-q", "-c", "dev"]);
        let release = root.join("release-tree");
        git_in(
            &root,
            &["worktree", "add", "-q", release.to_str().unwrap(), "rel"],
        );
        commit_file(&root, "a.txt", "v2\n", "dev 前进一步");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let out = GitTool
            .execute(
                serde_json::json!({"action":"merge_ff","from":"dev","into":"rel"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("fast-forwarded rel"),
            "{}",
            out.content
        );
        // 引用和工作区文件都要真的前进。
        let dev = run_git(&root, &["rev-parse", "dev"]).await.unwrap();
        let rel = run_git(&root, &["rev-parse", "rel"]).await.unwrap();
        assert_eq!(dev, rel);
        // autocrlf 环境下检出内容可能是 CRLF,断言前归一。
        let checked_out = std::fs::read_to_string(release.join("a.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(checked_out, "v2\n");
        git_in(
            &root,
            &["worktree", "remove", "--force", release.to_str().unwrap()],
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// 分支没检出在任何工作树时退化为纯引用快进;历史分叉必须干净失败。
    #[tokio::test]
    async fn merge_ff_updates_unchecked_branch_and_refuses_divergence() {
        let root = temp_repo("ffref");
        commit_file(&root, "a.txt", "v1\n", "初始提交");
        git_in(&root, &["branch", "archive"]);
        git_in(&root, &["switch", "-q", "-c", "dev"]);
        commit_file(&root, "a.txt", "v2\n", "dev 前进");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let out = GitTool
            .execute(
                serde_json::json!({"action":"merge_ff","from":"dev","into":"archive"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("ref-only"), "{}", out.content);
        let dev = run_git(&root, &["rev-parse", "dev"]).await.unwrap();
        let archive = run_git(&root, &["rev-parse", "archive"]).await.unwrap();
        assert_eq!(dev, archive);
        // 制造分叉:archive 上单独长一个提交,dev 再前进,快进必须被拒。
        git_in(&root, &["switch", "-q", "archive"]);
        commit_file(&root, "b.txt", "x\n", "archive 单独前进");
        git_in(&root, &["switch", "-q", "dev"]);
        commit_file(&root, "a.txt", "v3\n", "dev 再前进");
        let rejected = GitTool
            .execute(
                serde_json::json!({"action":"merge_ff","from":"archive","into":"dev"}),
                &ctx,
            )
            .await;
        assert!(rejected.is_error, "{}", rejected.content);
        assert!(rejected.content.contains("快进"), "{}", rejected.content);
        std::fs::remove_dir_all(root).ok();
    }

    /// 选项注入与区间语法要在碰 git 之前被拒掉。
    #[tokio::test]
    async fn merge_ff_rejects_malformed_refs() {
        let root = temp_repo("ffbad");
        commit_file(&root, "a.txt", "v1\n", "初始提交");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        for bad in ["--exec=evil", "a..b", "a b", "HEAD~1", ""] {
            let out = GitTool
                .execute(serde_json::json!({"action":"merge_ff","from": bad}), &ctx)
                .await;
            assert!(out.is_error, "`{bad}` 应被拒绝:{}", out.content);
        }
        let out = GitTool
            .execute(
                serde_json::json!({"action":"merge_ff","from":"HEAD","into":"-evil"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn stage_hash_is_required_and_detects_index_change() {
        let root = temp_repo("cas");
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let staged = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["a.txt"]}),
                &ctx,
            )
            .await;
        assert!(!staged.is_error, "{}", staged.content);
        let hash = staged
            .content
            .lines()
            .find_map(|line| line.strip_prefix("staged_hash: "))
            .unwrap()
            .to_string();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        run_git(&root, &["add", "--", "b.txt"]).await.unwrap();
        let rejected = GitTool
            .execute(
                serde_json::json!({"action":"commit","message":"x","expected_hash":hash}),
                &ctx,
            )
            .await;
        assert!(rejected.is_error);
        assert!(
            rejected.content.contains("staged content changed"),
            "{}",
            rejected.content
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-263:自举 stage 只暂存本次显式列出的文件,工作区里他人的改动
    /// 留在原地,并且被点名可见(不静默吞掉也不静默跳过)。
    #[tokio::test]
    async fn stage_leaves_foreign_changes_unstaged_and_names_them() {
        let root = temp_repo("d263");
        commit_file(&root, "base.txt", "base\n", "初始提交");
        // 本轮要提交的文件。
        std::fs::write(root.join("mine.txt"), "mine\n").unwrap();
        // 并发线/他人改的文件(未跟踪 + 已跟踪被改)。
        std::fs::write(root.join("theirs-new.txt"), "theirs\n").unwrap();
        std::fs::write(root.join("base.txt"), "base changed\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let staged = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["mine.txt"]}),
                &ctx,
            )
            .await;
        assert!(!staged.is_error, "{}", staged.content);
        // 只暂存了 mine.txt。
        let staged_paths = staged_paths(&root).await.unwrap();
        assert_eq!(staged_paths, vec!["mine.txt"], "清单外改动不得入暂存区");
        // 他人的改动仍在工作区,且被点名。
        assert_eq!(
            std::fs::read_to_string(root.join("theirs-new.txt")).unwrap(),
            "theirs\n",
            "未跟踪的他人文件不能被动过"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("base.txt")).unwrap(),
            "base changed\n",
            "已跟踪的他人改动不能被动过"
        );
        assert!(
            staged.content.contains("NOT staged by this request"),
            "应点名未纳入的改动: {}",
            staged.content
        );
        assert!(
            staged.content.contains("theirs-new.txt") && staged.content.contains("base.txt"),
            "点名的文件清单应覆盖他人改动: {}",
            staged.content
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-618 ①②④:同一 request 的 test_record 治理元数据可以增量纳入,不必整批 restage。
    #[tokio::test]
    async fn stage_request_adds_test_record_metadata_without_restage() {
        let root = temp_repo("d618-incremental");
        commit_file(&root, "base.txt", "base\n", "初始提交");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(root.join("src.txt"), "source\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let first = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["src.txt"]}),
                &ctx,
            )
            .await;
        assert!(!first.is_error, "{}", first.content);
        let first_hash = first
            .content
            .lines()
            .find_map(|line| line.strip_prefix("staged_hash: "))
            .unwrap()
            .to_string();
        let token = first
            .content
            .lines()
            .find_map(|line| line.strip_prefix("stage_request: "))
            .unwrap()
            .to_string();

        crate::test_record::record_test_run(
            &root,
            None,
            "D-618 metadata",
            "passed",
            Some("cargo test -p kanzei-tools"),
            Some("request metadata"),
            None,
        )
        .unwrap();
        let second = GitTool
            .execute(
                serde_json::json!({
                    "action":"stage",
                    "files":[".kanzei/project/tests.md",".kanzei/project/tests-archive.md"],
                    "stage_request": token
                }),
                &ctx,
            )
            .await;
        assert!(!second.is_error, "{}", second.content);
        let second_hash = second
            .content
            .lines()
            .find_map(|line| line.strip_prefix("staged_hash: "))
            .unwrap()
            .to_string();
        assert_ne!(first_hash, second_hash, "治理元数据必须进入 CAS hash");
        let staged = staged_paths(&root).await.unwrap();
        assert!(staged.contains(&"src.txt".to_string()));
        assert!(staged.contains(&TEST_RUNS_REL.to_string()));
        assert!(staged.contains(&crate::test_record::TEST_RUNS_ARCHIVE_REL.to_string()));

        let committed = GitTool
            .execute(
                serde_json::json!({
                    "action":"commit",
                    "message":"D-618 metadata",
                    "expected_hash": second_hash
                }),
                &ctx,
            )
            .await;
        assert!(!committed.is_error, "{}", committed.content);
        assert!(!stage_manifest_path(&root).unwrap().exists());
        std::fs::remove_dir_all(root).ok();
    }

    /// D-618 ②:request 外的预暂存文件即使已有 request manifest 也必须拒绝。
    #[tokio::test]
    async fn stage_request_rejects_foreign_pre_staged_path() {
        let root = temp_repo("d618-foreign");
        commit_file(&root, "base.txt", "base\n", "初始提交");
        std::fs::write(root.join("mine.txt"), "mine\n").unwrap();
        std::fs::write(root.join("foreign.txt"), "foreign\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let first = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["mine.txt"]}),
                &ctx,
            )
            .await;
        assert!(!first.is_error, "{}", first.content);
        run_git_owned(&root, &["add".into(), "--".into(), "foreign.txt".into()])
            .await
            .unwrap();
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(root.join(TEST_RUNS_REL), "# Test Runs\n").unwrap();
        std::fs::write(
            root.join(crate::test_record::TEST_RUNS_ARCHIVE_REL),
            "# Archive\n",
        )
        .unwrap();
        let rejected = GitTool
            .execute(
                serde_json::json!({
                    "action":"stage",
                    "files":[TEST_RUNS_REL, crate::test_record::TEST_RUNS_ARCHIVE_REL]
                }),
                &ctx,
            )
            .await;
        assert!(rejected.is_error, "foreign 暂存必须拒绝");
        assert!(
            rejected.content.contains("foreign.txt"),
            "{}",
            rejected.content
        );
        assert!(staged_paths(&root)
            .await
            .unwrap()
            .contains(&"foreign.txt".to_string()));
        std::fs::remove_dir_all(root).ok();
    }

    /// D-618 ③:写入 pending manifest 后中断,下一次 stage 可恢复同一 token,并在 commit
    /// 成功后删除 manifest,避免恢复状态污染下一次 request。
    #[tokio::test]
    async fn stage_request_recovers_pending_manifest_and_cleans_after_commit() {
        let root = temp_repo("d618-recovery");
        commit_file(&root, "base.txt", "base\n", "初始提交");
        std::fs::write(root.join("recover.txt"), "recover\n").unwrap();
        let manifest_path = stage_manifest_path(&root).unwrap();
        write_stage_request(
            &manifest_path,
            &StageRequest {
                token: "stage-recovery-token".into(),
                paths: vec!["recover.txt".into()],
            },
        )
        .unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let recovered = GitTool
            .execute(
                serde_json::json!({
                    "action":"stage",
                    "files":["recover.txt"],
                    "stage_request":"stage-recovery-token"
                }),
                &ctx,
            )
            .await;
        assert!(!recovered.is_error, "{}", recovered.content);
        assert!(recovered
            .content
            .contains("stage_request: stage-recovery-token"));
        assert!(manifest_path.exists());
        std::fs::remove_dir_all(root).ok();
    }

    /// D-264 验收② + R-209:git.rs 提交门禁、发版门禁(verify.ps1)与 CI(ci.yml)
    /// 的**完整检查项集合**机械同步——任一侧增删一步即红(不再只比对 fmt/clippy 两项)。
    ///
    /// 口径:verify.ps1 的 `Step-With-Timing "<key>"` 键集合必须等于固定清单
    /// {fmt, clippy, test, ui_runtime, ui_lint, ipc_event_contract,
    /// parallel_lines_regression, ui_a11y, ui_i18n, ui_markdown, ui_connectivity,
    /// metrics_build, crate_sync, ps1_bom};每个键在 ci.yml 里有对应标记(命令文本或
    /// smoke 脚本名);smoke 脚本与 npm ci 在两侧同现同隐。
    /// ui_syntax 已删(P0-2):ESLint 解析错误覆盖 node --check 的全部检查面。
    #[test]
    fn gate_checklists_align_across_git_verify_and_ci() {
        // 仓库根:git.rs 在 crates/kanzei-tools/src/,CARGO_MANIFEST_DIR 是
        // crates/kanzei-tools,上溯两级即仓库根。
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let ci = std::fs::read_to_string(repo_root.join(".github/workflows/ci.yml")).unwrap();
        let verify = std::fs::read_to_string(repo_root.join("scripts/verify.ps1")).unwrap();

        // ① verify.ps1 检查键集合(Step-With-Timing "<key>")必须等于固定清单。
        fn verify_check_keys(text: &str) -> std::collections::BTreeSet<String> {
            let mut keys = std::collections::BTreeSet::new();
            let needle = "Step-With-Timing \"";
            let mut start = 0;
            while let Some(pos) = text[start..].find(needle) {
                let after = start + pos + needle.len();
                let key_end = text[after..]
                    .find('"')
                    .map(|e| after + e)
                    .unwrap_or(text.len());
                keys.insert(text[after..key_end].to_string());
                start = key_end;
            }
            keys
        }
        let expected: std::collections::BTreeSet<String> = [
            "fmt",
            "clippy",
            "test",
            "ui_runtime",
            "ui_lint",
            "ipc_event_contract",
            "parallel_lines_regression",
            "ui_a11y",
            "ui_i18n",
            "ui_markdown",
            "ui_connectivity",
            "metrics_build",
            "crate_sync",
            "ps1_bom",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let actual = verify_check_keys(&verify);
        assert!(
            verify.contains("$global:LASTEXITCODE = 0"),
            "Step-With-Timing 必须清理上一步外部进程的 LASTEXITCODE"
        );
        assert_eq!(
            actual, expected,
            "verify.ps1 检查键集合必须等于固定清单——新增/删除门禁时 ci.yml 与 git.rs 也要同步"
        );

        // ② 每个键在 ci.yml 有对应标记(命令文本或 smoke 脚本名)。
        let markers: [(&str, &str); 14] = [
            ("metrics_build", "cargo build -q -p kanzei"),
            ("fmt", "cargo fmt --all -- --check"),
            ("clippy", "cargo clippy --workspace --all-targets"),
            ("test", "cargo test --workspace"),
            ("ui_runtime", "ui-runtime-smoke.mjs"),
            ("ui_lint", "ui-lint-smoke.mjs"),
            ("ipc_event_contract", "ipc-event-smoke.mjs"),
            ("parallel_lines_regression", "parallel-lines-regression.mjs"),
            ("ui_a11y", "ui-a11y-smoke.mjs"),
            ("ui_i18n", "ui-i18n-smoke.mjs"),
            ("ui_markdown", "ui-markdown-smoke.mjs"),
            ("ui_connectivity", "ui-connectivity.mjs"),
            ("crate_sync", "check-readme-crates.mjs"),
            ("ps1_bom", "check-ps1-bom.mjs"),
        ];
        for (key, marker) in markers {
            assert!(ci.contains(marker), "ci.yml 缺检查 {key}(标记 {marker})");
        }

        // ③ 反向:smoke 脚本在两侧同现同隐;npm ci 必须存在(ui-lint 依赖 eslint)。
        for script in [
            "ui-runtime-smoke.mjs",
            "ui-lint-smoke.mjs",
            "ipc-event-smoke.mjs",
            "parallel-lines-regression.mjs",
            "ui-a11y-smoke.mjs",
            "ui-i18n-smoke.mjs",
            "ui-markdown-smoke.mjs",
            "ui-connectivity.mjs",
        ] {
            assert_eq!(
                ci.contains(script),
                verify.contains(script),
                "smoke 脚本 {script} 必须在 verify.ps1 与 ci.yml 两侧同现同隐"
            );
        }
        assert!(
            ci.contains("npm ci"),
            "ci.yml 必须 npm ci(ui-lint 依赖 eslint)"
        );

        // ④ 门禁实现(git.rs 本文件)也含同一命令文本。
        let this = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/git.rs"),
        )
        .unwrap();
        assert!(
            this.contains("cargo fmt --all -- --check"),
            "fmt_gate 命令与 CI 不一致"
        );

        // ⑤ clippy 的三处分工是**刻意**的,不是漂移——所以逐处正向断言,而不是
        //    断言三处相同。省下的是 25.7s(37.9s → 12.2s)编译时间,代价是测试代码
        //    的 lint 违规会本地绿、push 后 CI 红。任何一处改动都必须回到这里改。
        //
        //    提交门禁(git.rs):check --all-targets 保编译底线 + 轻量 clippy 做 lint
        assert!(
            this.contains("cargo check --workspace --all-targets"),
            "compile_gate 必须保留 --all-targets:它是测试代码的编译底线,\
             clippy 变轻之后没有别的东西覆盖测试代码能不能编译"
        );
        assert!(
            this.contains("cargo clippy --workspace -- -D warnings"),
            "clippy_gate 应为不含 --all-targets 的轻量形态"
        );
        //    verify.ps1:轻量 clippy(编译覆盖由紧随其后的 test 步骤提供)
        assert!(
            verify.contains("cargo clippy --workspace --manifest-path"),
            "verify.ps1 的 clippy 应为轻量形态(不带 --all-targets)"
        );
        //    ci.yml:全量 clippy——测试代码的 lint 覆盖只剩这一处,丢了就真没人管了
        assert!(
            ci.contains("cargo clippy --workspace --all-targets -- -D warnings"),
            "ci.yml 必须保留 --all-targets 全量 clippy:本地两处都已转轻量,\
             测试代码的 lint 覆盖只由 CI 承担"
        );
    }

    #[test]
    fn gate_failures_are_aggregated_in_one_report() {
        let error = aggregate_gate_errors(
            Err("fmt failure".into()),
            Err("clippy failure".into()),
            "commit",
        )
        .unwrap_err();
        assert!(error.contains("[commit] fmt gate failed"), "{error}");
        assert!(error.contains("fmt failure"), "{error}");
        assert!(error.contains("[commit] clippy gate failed"), "{error}");
        assert!(error.contains("clippy failure"), "{error}");
    }

    /// D-264 验收①:构造「新增文件带 fmt 违规」场景,提交前被拦并明说违规位置。
    /// 在临时最小 cargo 工程上直接调 fmt_gate——门禁只读不写,跑完删目录。
    #[tokio::test]
    async fn fmt_gate_rejects_unformatted_source_and_names_file() {
        let dir = std::env::temp_dir().join(format!(
            "kz-fmtgate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"fmt-gate-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        // 故意不格式化:rustfmt 会要求改成 `pub fn x() -> i32 { 1 }`。
        std::fs::write(dir.join("src/lib.rs"), "pub fn  x( ) -> i32 { 1 }\n").unwrap();

        let err = fmt_gate(&dir).await.unwrap_err();
        assert!(err.contains("提交被拦下"), "应点名门禁: {err}");
        // Windows 上 rustfmt/clippy 输出 `src\lib.rs`(反斜杠),Unix 是正斜杠;
        // 断言文件名片段兼容两种分隔符。
        assert!(err.contains("lib.rs"), "应点名违规文件: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-264 验收①(clippy 侧):构造「新增文件带 clippy 违规」场景,提交前被拦。
    /// 最小工程一条 unused variable 即可让 `-D warnings` 红。
    #[tokio::test]
    async fn clippy_gate_rejects_lint_violation_and_names_file() {
        let dir = std::env::temp_dir().join(format!(
            "kz-clippygate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"clippy-gate-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        )
        .unwrap();
        // unused_variables 是默认 warn,-D warnings 下必红。
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn probe(flag: bool) -> i32 { let unused = 1; if flag { 1 } else { 0 } }\n",
        )
        .unwrap();

        let err = clippy_gate(&dir).await.unwrap_err();
        assert!(err.contains("提交被拦下"), "应点名门禁: {err}");
        // 同上:Windows 输出反斜杠路径,断言文件名片段兼容两种分隔符。
        assert!(err.contains("lib.rs"), "应点名违规文件: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 编译错误必须被 clippy_gate 拦下,且报错含 `-->` 位置。
    /// (原为 R-210 验收①「clippy 覆盖 check」的实证;clippy 转轻量后,这条覆盖
    /// 改由 clippy_gate 内先跑的 compile_gate 提供,断言不变、机制换了。)
    #[tokio::test]
    async fn clippy_gate_rejects_compile_error_with_position() {
        let dir = std::env::temp_dir().join(format!(
            "kz-clippycomp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"clippy-compile-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        )
        .unwrap();
        // 未定义符号:编译错误,不是 lint。check 删掉后必须仍被 clippy 拦截。
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn probe() -> i32 { undefined_symbol_here }\n",
        )
        .unwrap();

        let err = clippy_gate(&dir).await.unwrap_err();
        assert!(err.contains("提交被拦下"), "编译错误必须拦下提交: {err}");
        assert!(
            err.contains("-->"),
            "报错必须含 --> 位置(clippy 编译覆盖 check 的实证): {err}"
        );
        assert!(err.contains("lib.rs"), "应点名出错文件: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// clippy 转轻量之后的护栏:**测试代码**的编译错误必须仍被拦下。
    ///
    /// clippy 去掉 `--all-targets` 后就不再看测试目标了,测试代码能不能编译完全
    /// 靠 clippy_gate 内先跑的 compile_gate(`check --all-targets`)。这条测试盯的
    /// 就是那条底线:它一旦红,说明有人把 compile_gate 也改轻了——而那正是
    /// 2026-08-09 事故的形态(破损代码配着自写的 passed 记录进库)。
    #[tokio::test]
    async fn clippy_gate_rejects_compile_error_in_test_code() {
        let dir = std::env::temp_dir().join(format!(
            "kz-clippytestcomp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"clippy-testcomp-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        )
        .unwrap();
        // 库代码干净——轻量 clippy 看不出任何问题。
        std::fs::write(dir.join("src/lib.rs"), "pub fn probe() -> i32 { 1 }\n").unwrap();
        // 破损只在集成测试里:未定义符号。只有 --all-targets 的编译才会碰到它。
        std::fs::write(
            dir.join("tests/broken.rs"),
            "#[test]\nfn t() { let _: i32 = undefined_symbol_in_test_code(); }\n",
        )
        .unwrap();

        let err = clippy_gate(&dir).await.unwrap_err();
        assert!(
            err.contains("提交被拦下"),
            "测试代码编译不过必须拦下提交: {err}"
        );
        assert!(
            err.contains("broken.rs"),
            "应点名出错的测试文件(证明 --all-targets 编译覆盖仍在): {err}"
        );
        assert!(err.contains("-->"), "报错必须含 --> 位置: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-334 验收②:finalize 在测试**之前**先拦 fmt——构造 fmt 不过的源码,
    /// finalize 报 fmt gate 阶段,而不是先跑测试再在 commit 才拦。
    #[tokio::test]
    async fn finalize_rejects_fmt_before_tests() {
        let dir = temp_repo("finalize-fmt");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"finalize-fmt-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        // 故意不格式化:fmt gate 应拦截。
        std::fs::write(dir.join("src/lib.rs"), "pub fn  x( ) -> i32 { 1 }\n").unwrap();
        // test_record 需要项目骨架。
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();

        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };
        let out = GitTool
            .execute(
                serde_json::json!({
                    "action": "finalize",
                    "files": ["src/lib.rs", "Cargo.toml"],
                    "message": "finalize fmt gate test",
                }),
                &ctx,
            )
            .await;
        assert!(out.is_error, "fmt 不过必须拦下 finalize: {}", out.content);
        assert!(
            out.content.contains("fmt gate failed"),
            "应点名 fmt gate 阶段: {}",
            out.content
        );
        // 未被 stage、未提交——不留半状态。
        let status = run_git(&dir, &["status", "--porcelain"]).await.unwrap();
        assert!(
            status.contains("??") || status.contains(" M"),
            "fmt 拦截后不得 stage/commit: {status}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-334 成功路径:干净的最小工程,finalize 一次完成测试→record→stage→commit。
    /// 断言:返回 complete、commit 出现在 git log、test_record 有 passed 记录。
    #[tokio::test]
    async fn finalize_runs_tests_records_stages_and_commits() {
        let dir = temp_repo("finalize-ok");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"finalize-ok-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        // 干净且带一个会过的测试的源码(rustfmt 规范格式,过 fmt gate)。
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn x() -> i32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {\n        assert_eq!(crate::x(), 1);\n    }\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-001 finalize probe [todo]\n",
        )
        .unwrap();
        // 初始提交(empty repo 无法 stage 后看 log;先提交 README 占位)。
        commit_file(&dir, "README.md", "finalize probe\n", "init");

        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };
        let out = GitTool
            .execute(
                serde_json::json!({
                    "action": "finalize",
                    "files": ["src/lib.rs", "Cargo.toml"],
                    "message": "finalize success test",
                }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "finalize 应成功: {}", out.content);
        assert!(out.content.contains("complete"), "{}", out.content);

        // commit 落地。
        let log = run_git(&dir, &["log", "--oneline", "-1"]).await.unwrap();
        assert!(
            log.contains("finalize success test"),
            "finalize 提交应出现在 log: {log}"
        );
        // test_record 有 passed 记录。
        let records = crate::test_record::test_runs_snapshot(&dir).unwrap();
        let text = serde_json::to_string(&records).unwrap_or_default();
        assert!(
            text.contains("git finalize (auto)") && text.contains("\"passed\""),
            "finalize 应写 passed test_record: {text}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-212 验收①:前端冒烟记录(非 Rust)不能背书 Rust 源码提交——时间戳比
    /// 改动新也不行,覆盖面不匹配即拦。
    #[test]
    fn source_test_gate_frontend_smoke_cannot_back_rust_change() {
        let root = temp_repo("gate-frontend");
        let project = root.join(".kanzei").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let src = root.join("crates/kanzei-tools/src/lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "pub fn x() {}\n").unwrap();
        // 最近 passed 记录是前端冒烟,收尾 = 现在(时序满足,只测相关性)。
        let now = now_secs();
        std::fs::write(
            project.join("tests.md"),
            format!(
                "# Test Runs\n\n## T-{now} 前端冒烟 [passed]\n- 命令: node scripts/ui-runtime-smoke.mjs\n- 收尾: {now}\n"
            ),
        )
        .unwrap();
        let err = source_test_gate(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        )
        .unwrap_err();
        assert!(err.contains("kanzei-tools"), "缺口应点名 crate: {err}");
        assert!(
            err.contains("前端冒烟") || err.contains("非 Rust"),
            "应指明记录覆盖面类型: {err}"
        );
        assert!(
            err.contains("cargo test -p kanzei-tools"),
            "应指明该跑什么: {err}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// R-212 验收②③:覆盖面与暂存 crate 求交——定向记录背书对应 crate、workspace
    /// 记录背书任意 crate、不匹配时拦截并点名缺口;非 crate 源码(scripts/)不受
    /// crate 相关性约束。
    #[test]
    fn source_test_gate_coverage_intersects_with_staged_crates() {
        let root = temp_repo("gate-coverage");
        let project = root.join(".kanzei").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let tools_src = root.join("crates/kanzei-tools/src/lib.rs");
        std::fs::create_dir_all(tools_src.parent().unwrap()).unwrap();
        std::fs::write(&tools_src, "pub fn x() {}\n").unwrap();
        let core_src = root.join("crates/kanzei-core/src/lib.rs");
        std::fs::create_dir_all(core_src.parent().unwrap()).unwrap();
        std::fs::write(&core_src, "pub fn y() {}\n").unwrap();
        // 收尾时间用写入时刻(实时时钟)而非测试开头固定 now:ps1 在测试末尾写入时,
        // 收尾时间必须晚于/等于其 mtime,否则 mtime 分支会确定性拦截(跨秒竞态)。
        let write_record = |command: &str| {
            let t = now_secs();
            std::fs::write(
                project.join("tests.md"),
                format!("# Test Runs\n\n## T-{t} 记录 [passed]\n- 命令: {command}\n- 收尾: {t}\n"),
            )
            .unwrap();
        };
        // 定向记录背书对应 crate。
        write_record("cargo test -p kanzei-tools");
        assert!(
            source_test_gate(
                &root,
                &root,
                &["crates/kanzei-tools/src/lib.rs".to_string()]
            )
            .is_ok(),
            "定向记录必须背书对应 crate"
        );
        // workspace 记录背书任意 crate。
        write_record("cargo test --workspace");
        assert!(
            source_test_gate(&root, &root, &["crates/kanzei-core/src/lib.rs".to_string()]).is_ok(),
            "workspace 记录必须背书任意 crate"
        );
        // 不匹配:kanzei-core 记录背书不了 kanzei-tools 改动,拦截文案点名缺口。
        write_record("cargo test -p kanzei-core");
        let err = source_test_gate(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        )
        .unwrap_err();
        assert!(err.contains("kanzei-tools"), "应点名暂存 crate: {err}");
        assert!(
            err.contains("kanzei-core"),
            "应指出记录覆盖了别的 crate: {err}"
        );
        assert!(
            err.contains("cargo test -p kanzei-tools"),
            "应指明该跑什么: {err}"
        );
        // 非 crate 源码(scripts/)不受 crate 相关性约束,前端记录可背书。
        let ps1 = root.join("scripts/hello.ps1");
        std::fs::create_dir_all(ps1.parent().unwrap()).unwrap();
        std::fs::write(&ps1, "Write-Host hi\n").unwrap();
        // 注意:write_record 用测试开头的固定 now 作收尾时间;ps1 写入必须与 now
        // 同秒才能让 mtime 分支放行——测试在秒内完成,既有设计,勿加 sleep。
        write_record("node scripts/ui-runtime-smoke.mjs");
        match source_test_gate(&root, &root, &["scripts/hello.ps1".to_string()]) {
            Ok(()) => {}
            Err(err) => panic!("非 crate 源码不应被 crate 相关性拦截: {err}"),
        }
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn source_test_gate_prefers_matching_fingerprint_over_newer_legacy_record() {
        let root = temp_repo("gate-fingerprint-legacy");
        let project = root.join(".kanzei").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let src = root.join("crates/kanzei-tools/src/lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "pub fn x() { let a = 1; }\n").unwrap();
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);
        git_in(&root, &["commit", "-m", "init"]);
        std::fs::write(&src, "pub fn x() { let b = 2; }\n").unwrap();
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);
        let fp = staged_source_fingerprint(&root).unwrap();
        let now = now_secs();
        std::fs::write(
            project.join("tests.md"),
            format!(
                "# Test Runs\n\n## T-1786922726036000 legacy frontend [passed]\n- 命令: node scripts/ui-runtime-smoke.mjs\n\n## T-{now} current Rust [passed]\n- 命令: cargo test -p kanzei-tools\n- 收尾: {}\n- 源码指纹: {fp}\n",
                now - 1
            ),
        )
        .unwrap();

        match source_test_gate(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        ) {
            Ok(()) => {}
            Err(error) => {
                panic!("匹配当前 staged 指纹的 Rust 记录应覆盖旧的无指纹前端记录: {error}")
            }
        }
        std::fs::remove_dir_all(root).ok();
    }

    /// D-332 验收④:test_record 收尾记录暂存源码指纹;source_test_gate 优先比指纹——
    /// 指纹一致即背书成立(不再被 test_record 自己写 tests.md 的 mtime 误伤);
    /// 源码改动(fmt/手改)后指纹不一致则拦截。
    #[test]
    fn source_test_gate_prefers_fingerprint_over_mtime() {
        let root = temp_repo("gate-fingerprint");
        let project = root.join(".kanzei").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let src = root.join("crates/kanzei-tools/src/lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "pub fn x() {}\n").unwrap();
        // 提交初始版本,再改源码并 stage —— 模拟「改完代码准备提交」。
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);
        git_in(&root, &["commit", "-m", "init"]);
        std::fs::write(&src, "pub fn x() { let a = 1; }\n").unwrap();
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);

        // 收尾时记录的指纹(与当前 staged 源码一致)。
        let fp = staged_source_fingerprint(&root).unwrap();
        assert!(!fp.is_empty(), "有暂存源码就必须有指纹");
        let now = now_secs();
        // 记录收尾时间设为「过去」(源码 mtime 更新),但指纹一致 → 应放行。
        std::fs::write(
            project.join("tests.md"),
            format!(
                "# Test Runs\n\n## T-{now} 记录 [passed]\n- 命令: cargo test -p kanzei-tools\n- 收尾: {}\n- 源码指纹: {fp}\n",
                now - 99999
            ),
        )
        .unwrap();
        assert!(
            source_test_gate(
                &root,
                &root,
                &["crates/kanzei-tools/src/lib.rs".to_string()]
            )
            .is_ok(),
            "指纹一致时,即使收尾时间早于源码 mtime 也应放行(test_record 写 tests.md 不误伤)"
        );

        // 源码再改(未重测)→ 指纹不一致 → 拦截。
        std::fs::write(&src, "pub fn x() { let b = 2; }\n").unwrap();
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);
        let err = source_test_gate(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        )
        .unwrap_err();
        assert!(
            err.contains("源码指纹") && err.contains("不一致"),
            "指纹不一致应拦截并点名: {err}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// v2 指纹的核心收益:同一轮测试背书的一批改动分多笔提交,第一笔落地后 HEAD
    /// 移动、剩余批次源码未变——记录按路径子集继续背书,不再逼全量复跑;测后再改
    /// 的路径则被点名拦截。
    #[test]
    fn source_test_gate_endorses_partial_commit_subset() {
        let root = temp_repo("gate-fp-subset");
        let project = root.join(".kanzei").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let a = root.join("crates/kanzei-tools/src/lib.rs");
        let b = root.join("scripts/hello.ps1");
        std::fs::create_dir_all(a.parent().unwrap()).unwrap();
        std::fs::create_dir_all(b.parent().unwrap()).unwrap();
        std::fs::write(&a, "pub fn x() {}\n").unwrap();
        std::fs::write(&b, "echo hi\n").unwrap();
        git_in(&root, &["add", "."]);
        git_in(&root, &["commit", "-m", "init"]);
        // 一轮改动两个文件,全部 stage 后测试收尾记指纹。
        std::fs::write(&a, "pub fn x() { let a = 1; }\n").unwrap();
        std::fs::write(&b, "echo hi2\n").unwrap();
        git_in(&root, &["add", "."]);
        let fp = staged_source_fingerprint(&root).unwrap();
        assert!(fp.starts_with("v2 "), "新指纹必须是 v2 清单形态: {fp}");
        let now = now_secs();
        std::fs::write(
            project.join("tests.md"),
            format!(
                "# Test Runs\n\n## T-{now} 批测 [passed]\n- 命令: cargo test -p kanzei-tools\n- 收尾: {}\n- 源码指纹: {fp}\n",
                now - 1
            ),
        )
        .unwrap();
        // 第一笔只提交 lib.rs → HEAD 移动;剩余暂存(hello.ps1)内容未变。
        git_in(
            &root,
            &[
                "commit",
                "-m",
                "batch1",
                "--",
                "crates/kanzei-tools/src/lib.rs",
            ],
        );
        match source_test_gate(&root, &root, &["scripts/hello.ps1".to_string()]) {
            Ok(()) => {}
            Err(err) => panic!("分批提交的剩余批次源码未变,记录应继续背书: {err}"),
        }
        // 剩余文件测后再改 → 该路径 blob 变 → 点名拦截。
        std::fs::write(&b, "echo hi3\n").unwrap();
        git_in(&root, &["add", "scripts/hello.ps1"]);
        let err = source_test_gate(&root, &root, &["scripts/hello.ps1".to_string()]).unwrap_err();
        assert!(
            err.contains("源码指纹") && err.contains("不一致") && err.contains("scripts/hello.ps1"),
            "测后改动要点名路径: {err}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// 背书面 = 测试那一刻的**工作区**内容:「跑测试 → test_record → git stage →
    /// commit」这个自然顺序不再被拦(此前录的是暂存清单,录制时目标文件还没暂存,
    /// 背书清单里没有它,必拦)。
    #[test]
    fn record_before_stage_still_endorses_commit() {
        let root = temp_repo("gate-fp-worktree");
        let project = root.join(".kanzei").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let src = root.join("crates/kanzei-tools/src/lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "pub fn x() {}\n").unwrap();
        git_in(&root, &["add", "."]);
        git_in(&root, &["commit", "-m", "init"]);
        // 改源码但**不暂存** → 录背书(工作区内容)。
        std::fs::write(&src, "pub fn x() { let a = 1; }\n").unwrap();
        let fp = source_endorsement_fingerprint(&root).unwrap();
        assert!(fp.starts_with("v2 "), "工作区背书指纹应为 v2 清单: {fp}");
        let now = now_secs();
        std::fs::write(
            project.join("tests.md"),
            format!(
                "# Test Runs\n\n## T-{now} 顺序 [passed]\n- 命令: cargo test -p kanzei-tools\n- 收尾: {}\n- 源码指纹: {fp}\n",
                now - 1
            ),
        )
        .unwrap();
        // 之后才 stage → staged blob 与录制时工作区一致 → 放行。
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);
        match source_test_gate(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        ) {
            Ok(()) => {}
            Err(err) => panic!("先 record 后 stage 的顺序不该被拦: {err}"),
        }
        // 录完再改工作区并暂存新内容 → 暂存 blob 与背书不一致 → 仍要点名拦截。
        std::fs::write(&src, "pub fn x() { let b = 2; }\n").unwrap();
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);
        let err = source_test_gate(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        )
        .unwrap_err();
        assert!(
            err.contains("源码指纹") && err.contains("不一致"),
            "测后改动仍须拦截: {err}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-332:staged_source_fingerprint 只对源码路径求 hash——只 stage tests.md(非源码)
    /// 时指纹为空,门禁退回 mtime 逻辑,不产生「空指纹 vs 有指纹」的误判。
    #[test]
    fn staged_source_fingerprint_ignores_non_source_paths() {
        let root = temp_repo("gate-fp-nonsource");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(root.join(".kanzei/project/tests.md"), "# Test Runs\n").unwrap();
        git_in(&root, &["add", ".kanzei/project/tests.md"]);
        assert_eq!(
            staged_source_fingerprint(&root).unwrap(),
            "",
            "只有非源码暂存时指纹应为空(门禁走 mtime)"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// R-261:纯前端资源(kanzei-app/ui/ 下 js/css/html)不算 Rust source——
    /// 它们由前端冒烟集背书,不要求 cargo test -p kanzei-app 跑全套 Rust 测试。
    /// staged 同时含 Rust 源码时,Rust 部分仍按原规则要求测试背书。
    #[test]
    fn 纯前端ui资源不算rust源码_门禁放行而rust源码规则不变() {
        assert!(!is_source_path("crates/kanzei-app/ui/01-core.js"));
        assert!(!is_source_path("crates/kanzei-app/ui/style.css"));
        assert!(!is_source_path("crates/kanzei-app/ui/index.html"));
        assert!(
            is_source_path("crates/kanzei-tools/src/lib.rs"),
            "Rust 源码仍算 source"
        );
        assert!(is_source_path("scripts/verify.ps1"), "scripts 仍算 source");

        // 纯前端 staged:source_test_gate 放行(无 Rust 源码 → 不需要测试背书)。
        let root = temp_repo("gate-frontend-only");
        std::fs::create_dir_all(root.join("crates/kanzei-app/ui")).unwrap();
        std::fs::write(
            root.join("crates/kanzei-app/ui/01-core.js"),
            "console.log('x');\n",
        )
        .unwrap();
        git_in(&root, &["add", "crates/kanzei-app/ui/01-core.js"]);
        assert_eq!(
            staged_source_fingerprint(&root).unwrap(),
            "",
            "纯前端暂存指纹应为空(不算 Rust source)"
        );
        assert!(
            source_test_gate(
                &root,
                &root,
                &["crates/kanzei-app/ui/01-core.js".to_string()],
            )
            .is_ok(),
            "纯前端改动不得被 source_test_gate 拦"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
