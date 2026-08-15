//! Git 命令执行域(R-257 B4):stage/commit/merge_ff 具体命令与 run_git 执行器、
//! staged 状态快照(staged_paths/staged_state)。自 git.rs 原样迁出,零行为变更。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use kanzei_harness::{ToolCtx, ToolOutput};

use super::finalize::{
    clippy_gate, fmt_gate, is_source_path, placeholder_id_gate, source_test_gate, validate_ref,
};
use super::tool::normalize_files;
use super::worktree::worktree_for_branch;

const GIT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT: usize = 1024 * 1024;

pub(crate) async fn staged_paths(cwd: &Path) -> Result<Vec<String>, String> {
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

pub(crate) async fn stage(cwd: &Path, raw_files: &[String]) -> ToolOutput {
    let files = match normalize_files(cwd, raw_files, true) {
        Ok(files) => files,
        Err(error) => return ToolOutput::error(error),
    };
    let requested: std::collections::BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let existing = match staged_paths(cwd).await {
        Ok(paths) => paths,
        Err(error) => return ToolOutput::error(error),
    };
    let foreign: Vec<String> = existing
        .into_iter()
        .filter(|path| !requested.contains(path.as_str()))
        .collect();
    if !foreign.is_empty() {
        return ToolOutput::error(format!(
            "REFUSING to stage: the index already contains paths outside this request: {}. Commit or unstage them deliberately first.",
            foreign.join(", ")
        ));
    }
    let mut args = vec!["add".to_string(), "--".into()];
    args.extend(files);
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
    // D-263:暂存成功后对照工作区,把「本次请求之外的未暂存改动」点名写进返回。
    // 自举提交只该包含本轮显式列出的文件;工作区里若还有别的改动(他人/并发线/
    // 未纳入本次提交的存量),不静默吞掉也不静默跳过,而是明确可见,由调用方决定
    // 是否后续处理。这是对「git add -A 式整区暂存」的机械防线的一部分。
    let unstaged = unstaged_changes(cwd).await.unwrap_or_default();
    let mut base = format!(
        "staged {} file(s): {}\nstaged_hash: {hash}\nReview with `git diff` using staged=true, then commit with this exact expected_hash.",
        paths.len(), paths.join(", ")
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

pub(crate) async fn commit(
    ctx: &ToolCtx,
    message: Option<String>,
    expected_hash: Option<String>,
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
    // R-227:占位符测试 ID 门禁——tracker 文件 diff 里出现 `T-\d+xxx` 形态的占位符
    // (真实测试 ID 是 `T-<10位时间戳>`,占位符是数字后接 xxx)即拒绝提交。存量 8 处
    // (R-198/R-199/D-219/D-266/D-279/D-281/D-282/D-316 关闭证据)曾把「全量跑过但
    // 没记 test_record」写成占位符,隔时凭记忆写证据导致 R-198/R-199 的关闭证据含
    // 占位符仍无人核对(D-320 根因链)。门禁只扫 tracker 文件,源码 diff 不受影响。
    if let Err(error) = placeholder_id_gate(&staged_diff, &paths) {
        return ToolOutput::error(error);
    }
    if paths.iter().any(|p| is_source_path(p)) {
        // 顺序有讲究:先验门禁(机械真值),再看测试记录(自报证据)。编译不过时
        // 报编译错误比报"没有测试背书"有用得多。D-264:fmt/clippy 为提交前硬门禁
        // ——规则层写过但自举漏了三次,必须代码强制。clippy_gate 内部先跑
        // compile_gate(check --all-targets,含测试代码的编译底线)再跑轻量 clippy。
        // R-261:fmt 与 clippy 互不依赖,并行执行——fmt --check 只读不写 target,
        // 与 clippy 的增量编译无资源竞争,串行只会让门禁多等一份时间。
        let (fmt_result, clippy_result) = tokio::join!(fmt_gate(cwd), clippy_gate(cwd));
        if let Err(error) = fmt_result {
            return ToolOutput::error(error);
        }
        if let Err(error) = clippy_result {
            return ToolOutput::error(error);
        }
    }
    if let Err(error) = source_test_gate(&ctx.project_root, cwd, &paths) {
        return ToolOutput::error(error);
    }
    if let Err(error) = run_git_owned(cwd, &["commit".into(), "-m".into(), message]).await {
        return ToolOutput::error(error);
    }
    match run_git(
        cwd,
        &["show", "--stat", "--no-color", "--format=%h %s", "HEAD"],
    )
    .await
    {
        Ok(stat) => ToolOutput::ok(format!(
            "committed verified staged set ({current_hash})\n{stat}"
        )),
        Err(error) => {
            ToolOutput::error(format!("commit succeeded but verification failed: {error}"))
        }
    }
}

/// 快进合并:分支级变更中唯一"要么无冲突成功、要么干净失败"的形态,所以可以
/// 开放给模型——不产生合并提交、不动索引内容、非快进直接拒绝(D-173 的边界不破)。
/// 发版流程(kanzei-release 树 ff dev→main)此前卡在"bash 拦 merge、工具没 merge"
/// 的空档里,只能让用户手跑,就是这个动作的由来。
pub(crate) async fn merge_ff(cwd: &Path, from: Option<String>, into: Option<String>) -> ToolOutput {
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

pub(crate) async fn run_git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    run_git_owned(cwd, &owned).await
}

pub(crate) async fn run_git_owned(cwd: &Path, args: &[String]) -> Result<String, String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    hide_console_window(&mut command);
    let output = tokio::time::timeout(GIT_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            format!(
                "git {} timed out after {}s",
                args.join(" "),
                GIT_TIMEOUT.as_secs()
            )
        })?
        .map_err(|error| format!("cannot run git: {error}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    if text.len() > MAX_OUTPUT {
        text.truncate(MAX_OUTPUT);
        text.push_str("\n(output truncated at 1 MiB)");
    }
    if output.status.success() {
        Ok(text.trim_end().to_string())
    } else {
        Err(format!(
            "git {} failed (exit {:?}):\n{}",
            args.join(" "),
            output.status.code(),
            text.trim_end()
        ))
    }
}

#[cfg(windows)]
fn hide_console_window(command: &mut tokio::process::Command) {
    crate::hide_console_async(command);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut tokio::process::Command) {}
