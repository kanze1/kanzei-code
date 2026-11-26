//! 结构化 Git 工具：把高频 status/diff/stage/commit 从任意 shell 文本收口为可校验契约。
//! stage 只接受逐文件路径；commit 必须携带上一轮 stage 返回的暂存区指纹，避免夹带旧改动。

use std::path::Path;

use kanzei_harness::{ToolCtx, ToolOutput};

mod commands;
pub mod observe;
use commands::append_diagnostics;
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

mod index;
use index::{
    load_stage_request, new_stage_token, remove_stage_request, stage_manifest_path,
    staged_paths_with_diagnostics, staged_state, union_paths, write_stage_request, StageRequest,
};
#[cfg(test)]
use index::{parse_staged_paths, staged_hash, staged_paths};

fn with_git_diagnostics(mut output: ToolOutput, diagnostics: &[String]) -> ToolOutput {
    append_diagnostics(&mut output.content, diagnostics);
    output
}

/// D-332 验收④:暂存**源码**的指纹——test_record 收尾时用它背书「测试跑的是
/// 这份源码」,提交门禁优先比指纹而不是纯 mtime。要点:
/// - 只对源码路径(`is_source_path`)求指纹:tests.md/tracker 等托管文档的写入不改变
///   源码指纹,不会再让「test_record 自己改 tests.md」触发源码重测。
/// - v2 形态是**按路径的 staged blob 清单**(`v2 path@sha12,…`,按路径排序),不再 hash
async fn stage(
    project_root: &Path,
    cwd: &Path,
    raw_files: &[String],
    requested_token: Option<&str>,
) -> ToolOutput {
    let mut diagnostics = Vec::new();
    let output = stage_body(
        project_root,
        cwd,
        raw_files,
        requested_token,
        &mut diagnostics,
    )
    .await;
    with_git_diagnostics(output, &diagnostics)
}

async fn stage_body(
    project_root: &Path,
    cwd: &Path,
    raw_files: &[String],
    requested_token: Option<&str>,
    diagnostics: &mut Vec<String>,
) -> ToolOutput {
    let files = match normalize_files(cwd, raw_files, true) {
        Ok(files) => files,
        Err(error) => return ToolOutput::error(error),
    };
    let plan = match build_commit_plan(project_root, cwd, &files).await {
        Ok(plan) => plan,
        Err(error) => return ToolOutput::error(format!("[stage] commit_plan failed: {error}")),
    };
    if !plan.unsafe_files.is_empty() {
        return plan.blocker("stage");
    }
    diagnostics.extend(plan.diagnostics);

    let existing = match staged_paths_with_diagnostics(cwd, diagnostics).await {
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
    match run_git_owned(cwd, &args).await {
        Ok(output) => {
            output.take_stdout(diagnostics);
        }
        Err(error) => return ToolOutput::error(error),
    }
    let (hash, diff, paths) = match staged_state(cwd, diagnostics).await {
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
    let unstaged = unstaged_changes(cwd, diagnostics).await.unwrap_or_default();
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
async fn unstaged_changes(
    cwd: &Path,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    let text = run_git(
        cwd,
        &["status", "--porcelain", "--untracked-files=all", "-z"],
    )
    .await?
    .take_stdout(diagnostics);
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
/// 2026-08-12 第三次复发),第三次复发才确认必须代码强制。参数与 verification registry 对齐；
/// git 与本机 verify 的参数及清单关系由 gate_checklists_align_across_git_and_local_verify 守护。
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
/// 省 25.7s。提交门禁(此处)与 verify.ps1 走轻量 lint，其参数由
/// gate_checklists_align_across_git_and_local_verify 守护。
/// 本地轻量 lint 不检查测试代码；改测试代码时仍须自跑
/// `cargo clippy -p <crate> --all-targets -- -D warnings`。
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
    let mut diagnostics = Vec::new();
    let output = commit_body(
        ctx,
        message,
        expected_hash,
        gates_already_run,
        &mut diagnostics,
    )
    .await;
    with_git_diagnostics(output, &diagnostics)
}

async fn commit_body(
    ctx: &ToolCtx,
    message: Option<String>,
    expected_hash: Option<String>,
    gates_already_run: bool,
    diagnostics: &mut Vec<String>,
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
    let (current_hash, _staged_diff, paths) = match staged_state(cwd, diagnostics).await {
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
    if !plan.unsafe_files.is_empty() {
        return plan.blocker("commit");
    }
    diagnostics.extend(plan.diagnostics);
    if ctx.project_workflow && !gates_already_run && paths.iter().any(|p| is_source_path(p)) {
        let (fmt_result, clippy_result) = tokio::join!(fmt_gate(cwd), clippy_gate(cwd));
        if let Err(error) = aggregate_gate_errors(fmt_result, clippy_result, "commit") {
            return ToolOutput::error(error);
        }
    }
    match run_git_owned(cwd, &["commit".into(), "-m".into(), message]).await {
        Ok(output) => {
            output.take_stdout(diagnostics);
        }
        Err(error) => return ToolOutput::error(error),
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
            "committed verified staged set ({current_hash}){cleanup_note}\n{stat}",
            stat = stat.take_stdout(diagnostics).trim_end()
        )),
        Err(error) => {
            ToolOutput::error(format!("commit succeeded but verification failed: {error}"))
        }
    }
}

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
async fn merge_ff(ctx: &ToolCtx, from: Option<String>, into: Option<String>) -> ToolOutput {
    let mut diagnostics = Vec::new();
    let output = merge_ff_body(ctx, from, into, &mut diagnostics).await;
    with_git_diagnostics(output, &diagnostics)
}

async fn merge_ff_body(
    ctx: &ToolCtx,
    from: Option<String>,
    into: Option<String>,
    diagnostics: &mut Vec<String>,
) -> ToolOutput {
    let cwd = &ctx.cwd;
    let from = match from.as_deref().map(validate_ref) {
        Some(Ok(name)) => name,
        Some(Err(error)) => return ToolOutput::error(error),
        None => return ToolOutput::error("`from` is required for merge_ff (例如 dev)"),
    };
    // 来源必须能解析成提交,报错要在改任何东西之前。
    let source_commit = match run_git(
        cwd,
        &["rev-parse", "--verify", &format!("{from}^{{commit}}")],
    )
    .await
    {
        Ok(output) => output.take_stdout(diagnostics).trim().to_string(),
        Err(error) => return ToolOutput::error(format!("无法解析来源 `{from}`:{error}")),
    };
    let (target_label, merge_dir, ref_update) = match into.as_deref().map(validate_ref) {
        Some(Err(error)) => return ToolOutput::error(error),
        Some(Ok(into)) => {
            let porcelain = match run_git(cwd, &["worktree", "list", "--porcelain"]).await {
                Ok(output) => output.take_stdout(diagnostics),
                Err(error) => return ToolOutput::error(error),
            };
            match worktree_for_branch(&porcelain, &into) {
                // 分支检出在某棵工作树(可能就是当前树):去那棵树里做真正的
                // merge --ff-only,工作区文件与 HEAD 一起前进。
                Some(path) => (into, Some(path), None),
                // 谁都没检出:快进纯属引用更新,`git fetch . from:into` 天然拒绝非快进。
                None => (into.clone(), None, Some(format!("{source_commit}:{into}"))),
            }
        }
        None => (String::from("HEAD"), Some(cwd.to_path_buf()), None),
    };
    // A run already owns cwd. Never wait for B while retaining A's lease:
    // two simultaneous cross-tree merges would otherwise deadlock.
    let cross_tree = match merge_dir.as_deref() {
        Some(dir) => match (std::fs::canonicalize(dir), std::fs::canonicalize(cwd)) {
            (Ok(target), Ok(current)) => target != current,
            _ => return ToolOutput::error("merge_ff cannot resolve worktree identities"),
        },
        None => false,
    };
    let _target_lease = match merge_dir.as_deref() {
        Some(dir) if cross_tree => {
            let Some(coordinator) = ctx.execution_coordinator.as_ref() else {
                return ToolOutput::error("merge_ff cannot acquire the target worktree writer lease; run this operation from the target worktree");
            };
            use futures::FutureExt;
            use kanzei_harness::orchestration::WriterLeaseRequest;
            static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let request_id = format!(
                "git-merge-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            let lease = coordinator
                .acquire_writer_lease(WriterLeaseRequest {
                    write_scope: dir.to_path_buf(),
                    run_id: request_id.clone(),
                    process_id: ctx.process_id.clone().unwrap_or_else(|| "git".into()),
                    reason: "merge_ff target worktree".into(),
                })
                .now_or_never();
            match lease {
                Some(Ok(lease)) => Some(lease),
                Some(Err(error)) => return ToolOutput::error(error),
                None => {
                    coordinator.cancel_waiter(&request_id);
                    return ToolOutput::needs_correction(
                        "GIT_TARGET_BUSY",
                        "merge_ff target worktree has an active writer; retry after it finishes",
                    );
                }
            }
        }
        _ => None,
    };
    // The target branch can move during lease acquisition. Refuse if the
    // worktree was rebound, rather than merging into its new branch.
    if let (Some(dir), Some(expected)) = (merge_dir.as_deref(), into.as_deref()) {
        match run_git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]).await {
            Ok(output) if output.stdout.trim() == expected.trim() => {}
            _ => {
                return ToolOutput::error(
                    "merge_ff target worktree changed branch; retry with the current target",
                )
            }
        }
    }
    let before = run_git(
        merge_dir.as_deref().unwrap_or(cwd),
        &["rev-parse", "--short", &target_label],
    )
    .await
    .map(|output| output.take_stdout(diagnostics).trim().to_string())
    .unwrap_or_else(|_| "?".into());
    let result = match (&merge_dir, &ref_update) {
        (Some(dir), _) => run_git(dir, &["merge", "--ff-only", &source_commit]).await,
        (None, Some(spec)) => run_git(cwd, &["fetch", ".", spec]).await,
        (None, None) => unreachable!("merge_ff target must be a worktree or a ref update"),
    };
    match result {
        Ok(output) => {
            output.take_stdout(diagnostics);
        }
        Err(error) => {
            return ToolOutput::error(format!(
                "merge_ff 失败(只允许快进;历史分叉时先在 dev 侧收敛):{error}"
            ))
        }
    }
    let after = run_git(
        merge_dir.as_deref().unwrap_or(cwd),
        &["rev-parse", "--short", &target_label],
    )
    .await
    .map(|output| output.take_stdout(diagnostics).trim().to_string())
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
    use kanzei_harness::Tool;

    #[tokio::test]
    async fn readonly_observations_do_not_execute_repository_programs() {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        run_git(root.path(), &["init", "-q"]).await.unwrap();
        std::fs::write(root.path().join("visible.txt"), "before\n").unwrap();
        run_git(root.path(), &["add", "visible.txt"]).await.unwrap();
        run_git(
            root.path(),
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "owned initial",
            ],
        )
        .await
        .unwrap();
        let marker = state.path().join("repository-program-ran");
        let quoted = marker
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''");
        let hook = root.path().join("owned-hook.sh");
        std::fs::write(
            &hook,
            format!("#!/bin/sh\nprintf RAN > '{quoted}'\nprintf 'token\\0'\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        run_git(
            root.path(),
            &[
                "config",
                "core.fsmonitor",
                &hook.to_string_lossy().replace('\\', "/"),
            ],
        )
        .await
        .unwrap();
        run_git(root.path(), &["status", "--short"]).await.unwrap();
        assert!(
            marker.exists(),
            "owned raw Git positive control must execute"
        );
        std::fs::remove_file(&marker).unwrap();
        std::fs::write(
            root.path().join(".gitattributes"),
            "visible.txt filter=owned diff=owned\n",
        )
        .unwrap();
        let program = format!("printf RAN > '{quoted}'; cat");
        run_git(root.path(), &["config", "filter.owned.clean", &program])
            .await
            .unwrap();
        run_git(root.path(), &["config", "diff.owned.textconv", &program])
            .await
            .unwrap();
        std::fs::write(root.path().join("visible.txt"), "after longer\n").unwrap();
        let ctx = ToolCtx::new(root.path().into(), root.path().into());
        for action in ["status", "diff", "log"] {
            let output = GitTool
                .execute(serde_json::json!({"action":action}), &ctx)
                .await;
            assert!(!output.is_error, "{action}: {}", output.content);
            assert!(
                !marker.exists(),
                "read-only {action} executed repository code"
            );
        }
    }

    #[tokio::test]
    async fn readonly_diff_requires_precise_consent_for_tracked_credentials() {
        let root = temp_repo("observe-credentials");
        std::fs::write(root.join(".env"), "OLD_CREDENTIAL_CANARY\n").unwrap();
        run_git(&root, &["add", "-f", ".env"]).await.unwrap();
        std::fs::write(root.join(".env"), "NEW_CREDENTIAL_CANARY\n").unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        for staged in [false, true] {
            let output = GitTool
                .execute(
                    serde_json::json!({"action":"diff","files":[".env"],"staged":staged}),
                    &ctx,
                )
                .await;
            assert_eq!(
                output.code.as_deref(),
                Some("SENSITIVE_FILE_APPROVAL_REQUIRED")
            );
            assert!(!output.content.contains("CREDENTIAL_CANARY"));
        }
        let rules = kanzei_harness::permission::Ruleset::new(vec![kanzei_harness::rule(
            "read",
            ".env",
            kanzei_harness::Effect::Allow,
        )]);
        let output = kanzei_harness::permission::scope(
            rules,
            GitTool.execute(serde_json::json!({"action":"diff","files":[".env"]}), &ctx),
        )
        .await;
        assert!(!output.is_error, "{}", output.content);
        assert!(output.content.contains("NEW_CREDENTIAL_CANARY"));
        std::fs::remove_dir_all(root).unwrap();
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

    async fn git_with_trace(
        root: &Path,
        args: &[&str],
        trace: &str,
    ) -> Result<commands::GitCommandOutput, String> {
        let args = args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>();
        commands::run_git_owned_with_env(root, &args, &[("GIT_TRACE", trace)]).await
    }

    #[tokio::test]
    async fn successful_git_stderr_is_preserved_without_becoming_staged_paths() {
        let root = temp_repo("stderr-paths");
        let args = [
            "-c",
            "core.quotepath=false",
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
        ];
        let empty = git_with_trace(&root, &args, "1").await.unwrap();
        assert!(empty.stdout.is_empty(), "a fresh index is empty");
        assert!(empty.stderr.contains("trace:"), "{empty:?}");
        let mut diagnostics = Vec::new();
        assert!(parse_staged_paths(&empty.take_stdout(&mut diagnostics)).is_empty());

        std::fs::write(root.join("证据.txt"), "actual staged content\n").unwrap();
        run_git(&root, &["add", "--", "证据.txt"]).await.unwrap();
        let output = git_with_trace(&root, &args, "1").await.unwrap();
        let stdout = output.take_stdout(&mut diagnostics);
        assert_eq!(parse_staged_paths(&stdout), vec!["证据.txt"]);
        let shown = with_git_diagnostics(ToolOutput::ok("staged 1 file: 证据.txt"), &diagnostics);
        assert!(shown.content.contains("\n[stderr]\n"));
        assert!(shown.content.contains("trace:"));
        let shown = git_with_trace(&root, &["status", "--short"], "1")
            .await
            .unwrap()
            .render("(clean worktree)");
        assert!(shown.contains("\n[stderr]\n"), "{shown}");
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn staged_fingerprint_uses_git_stdout_even_when_stderr_changes() {
        let root = temp_repo("stderr-hash");
        std::fs::write(root.join("a.txt"), "one staged revision\n").unwrap();
        run_git(&root, &["add", "--", "a.txt"]).await.unwrap();
        let args = [
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-color",
        ];
        let quiet = git_with_trace(&root, &args, "0").await.unwrap();
        let traced = git_with_trace(&root, &args, "1").await.unwrap();
        assert_ne!(quiet.stderr, traced.stderr);
        assert_eq!(quiet.stdout, traced.stdout);
        assert_eq!(staged_hash(&quiet.stdout), staged_hash(&traced.stdout));
        let mut diagnostics = Vec::new();
        let (hash, diff, paths) = staged_state(&root, &mut diagnostics).await.unwrap();
        assert_eq!(hash, staged_hash(&quiet.stdout));
        assert_eq!(diff, quiet.stdout);
        assert_eq!(paths, vec!["a.txt"]);
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn failed_git_preserves_stdout_and_stderr_with_exit_status() {
        let root = temp_repo("stderr-failure");
        let error = git_with_trace(
            &root,
            &["-c", "alias.stream-failure=!echo git-stdout-evidence; echo git-stderr-evidence >&2; exit 7", "stream-failure"],
            "1",
        ).await.unwrap_err();
        assert!(error.contains("exit Some(7)"), "{error}");
        let (stdout, stderr) = error
            .split_once("\n[stderr]\n")
            .expect("separate diagnostic section");
        assert!(
            stdout
                .lines()
                .any(|line| line.trim() == "git-stdout-evidence"),
            "{error}"
        );
        assert!(
            stderr
                .lines()
                .any(|line| line.trim() == "git-stderr-evidence"),
            "{error}"
        );
        assert!(stderr.contains("trace:"), "{error}");
        std::fs::remove_dir_all(root).ok();
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
            execution_coordinator: Some(kanzei_harness::orchestration::ExecutionCoordinator::new(
                std::sync::Arc::new(kanzei_core::orchestration::MemoryCoordinator::new()),
            )),
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
        assert_eq!(dev.stdout, rel.stdout);
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
    async fn merge_ff_refuses_busy_target_and_releases_target_after_merge() {
        use kanzei_harness::orchestration::{ProjectExecutionCoordinator, WriterLeaseRequest};
        let root = temp_repo("ff-target-owner");
        commit_file(&root, "a.txt", "v1\n", "initial");
        git_in(&root, &["branch", "target"]);
        let target = root.join("target-tree");
        git_in(
            &root,
            &["worktree", "add", "-q", target.to_str().unwrap(), "target"],
        );
        commit_file(&root, "a.txt", "v2\n", "advance");
        let coordinator = std::sync::Arc::new(kanzei_core::orchestration::MemoryCoordinator::new());
        let request = |scope: &Path, id: &str| WriterLeaseRequest {
            write_scope: scope.into(),
            run_id: id.into(),
            process_id: id.into(),
            reason: "test".into(),
        };
        let _source = coordinator
            .acquire_writer_lease(request(&root, "source-run"))
            .await
            .unwrap();
        let target_writer = coordinator
            .acquire_writer_lease(request(&target, "target-run"))
            .await
            .unwrap();
        let mut ctx = ToolCtx::new(root.clone(), root.clone());
        ctx.execution_coordinator = Some(kanzei_harness::orchestration::ExecutionCoordinator::new(
            coordinator.clone(),
        ));
        let input = serde_json::json!({"action":"merge_ff", "from":"HEAD", "into":"target"});
        assert_eq!(
            GitTool.concurrency(&input, &ctx),
            kanzei_harness::ToolConcurrency::Exclusive
        );
        let denied = GitTool.execute(input.clone(), &ctx).await;
        assert_eq!(
            denied.code.as_deref(),
            Some("GIT_TARGET_BUSY"),
            "{}",
            denied.content
        );
        assert_eq!(
            std::fs::read_to_string(target.join("a.txt"))
                .unwrap()
                .trim(),
            "v1"
        );
        assert!(coordinator.snapshot(&target).waiting_writers.is_empty());
        drop(target_writer);
        let merged = GitTool.execute(input.clone(), &ctx).await;
        assert!(!merged.is_error, "{}", merged.content);
        assert_eq!(
            std::fs::read_to_string(target.join("a.txt"))
                .unwrap()
                .trim(),
            "v2"
        );
        assert!(coordinator.snapshot(&target).writer_run_id.is_none());
        assert_eq!(
            coordinator.snapshot(&root).writer_run_id.as_deref(),
            Some("source-run")
        );
        commit_file(&root, "a.txt", "v3\n", "source diverges");
        commit_file(&target, "b.txt", "target\n", "target diverges");
        let rejected = GitTool.execute(input, &ctx).await;
        assert!(rejected.is_error);
        assert!(coordinator.snapshot(&target).writer_run_id.is_none());
        git_in(
            &root,
            &["worktree", "remove", "--force", target.to_str().unwrap()],
        );
        std::fs::remove_dir_all(root).unwrap();
    }

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
        assert_eq!(dev.stdout, archive.stdout);
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
        std::fs::write(root.join("notes.md"), "# Notes\n").unwrap();
        let rejected = GitTool
            .execute(
                serde_json::json!({
                    "action":"stage",
                    "files":["notes.md"]
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

    /// registry 检查键唯一；本机 verify 委派给 runner，git 的核心门禁参数与 registry 对齐。
    #[test]
    fn gate_checklists_align_across_git_and_local_verify() {
        // 仓库根:git.rs 在 crates/kanzei-tools/src/,CARGO_MANIFEST_DIR 是
        // crates/kanzei-tools,上溯两级即仓库根。
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let verify = std::fs::read_to_string(repo_root.join("scripts/verify.ps1")).unwrap();

        let registry: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(repo_root.join("scripts/verification-checks.json")).unwrap(),
        )
        .unwrap();
        let checks = registry.as_array().unwrap();
        let keys: std::collections::BTreeSet<_> =
            checks.iter().map(|c| c["key"].as_str().unwrap()).collect();
        assert_eq!(keys.len(), checks.len(), "check keys must be unique");
        assert!(verify.contains("verify-policy.mjs classify"));
        assert!(verify.contains("node scripts/verify-runner.mjs"));
        let runner = std::fs::read_to_string(repo_root.join("scripts/verify-runner.mjs")).unwrap();
        assert!(runner.contains("child.stderr.on(\"data\", write)"));
        let this = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/git.rs"),
        )
        .unwrap();
        assert!(this.contains("cargo fmt --all -- --check"));
        assert!(this.contains("cargo check --workspace --all-targets"));
        assert!(this.contains("cargo clippy --workspace -- -D warnings"));
        let clippy = checks
            .iter()
            .find(|check| check["key"] == "clippy")
            .unwrap();
        assert_eq!(
            clippy["args"],
            serde_json::json!(["clippy", "--workspace", "--", "-D", "warnings"])
        );

        // Repository documentation describes the same local gate contract.
        let contributing = include_str!("../../../CONTRIBUTING.md");
        for command in [
            "cargo check --workspace --all-targets",
            "cargo fmt --all -- --check",
            "cargo clippy --workspace -- -D warnings",
            "cargo clippy -p <crate> --all-targets -- -D warnings",
        ] {
            assert!(contributing.contains(command), "{command}");
        }
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
        let status = run_git(&dir, &["status", "--porcelain"])
            .await
            .unwrap()
            .stdout;
        assert!(
            status.contains("??") || status.contains(" M"),
            "fmt 拦截后不得 stage/commit: {status}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-334 成功路径:干净的最小工程,finalize 一次完成门禁→测试→stage→commit。
    #[tokio::test]
    async fn finalize_runs_tests_stages_and_commits() {
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
        let log = run_git(&dir, &["log", "--oneline", "-1"])
            .await
            .unwrap()
            .stdout;
        assert!(
            log.contains("finalize success test"),
            "finalize 提交应出现在 log: {log}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-261:纯前端资源(kanzei-app/ui/ 下 js/css/html)不算 Rust source。
    #[test]
    fn 纯前端ui资源不算rust源码() {
        assert!(!is_source_path("crates/kanzei-app/ui/01-core.js"));
        assert!(!is_source_path("crates/kanzei-app/ui/style.css"));
        assert!(!is_source_path("crates/kanzei-app/ui/index.html"));
        assert!(
            is_source_path("crates/kanzei-tools/src/lib.rs"),
            "Rust 源码仍算 source"
        );
        assert!(is_source_path("scripts/verify.ps1"), "scripts 仍算 source");
    }

    // ── 分区:工作目录管理(UI2-0926 #13)──

    /// 项目目录只是落在上级仓库里:status 是一条事实(点名上级仓库),stage/commit 拒绝,
    /// 绝不对上级仓库动手。
    #[tokio::test]
    async fn 上级仓库内的项目_只读给事实_写操作拒绝且点名上级仓库() {
        let parent = temp_repo("parent-repo");
        commit_file(&parent, "outer.txt", "x\n", "上级仓库的提交");
        let project = parent.join("child-project");
        std::fs::create_dir_all(project.join(".kanzei")).unwrap();
        std::fs::write(project.join("a.txt"), "a\n").unwrap();
        let ctx = ToolCtx {
            cwd: project.clone(),
            project_root: project.clone(),
            ..Default::default()
        };
        let status = GitTool
            .execute(serde_json::json!({"action": "status"}), &ctx)
            .await;
        assert!(
            !status.is_error,
            "status 应是事实而非失败行: {}",
            status.content
        );
        assert!(status.content.contains("上级仓库"), "{}", status.content);
        assert!(status.content.contains("action=init"), "{}", status.content);
        let staged = GitTool
            .execute(
                serde_json::json!({"action": "stage", "files": ["a.txt"]}),
                &ctx,
            )
            .await;
        assert!(staged.is_error, "{}", staged.content);
        assert!(staged.content.contains("已拒绝操作"), "{}", staged.content);
        let parent_status = run_git(&parent, &["status", "--porcelain"])
            .await
            .unwrap()
            .stdout;
        assert!(
            !parent_status.lines().any(|line| line.starts_with('A')),
            "上级仓库的暂存区不得被动: {parent_status}"
        );
        std::fs::remove_dir_all(&parent).ok();
    }

    #[tokio::test]
    async fn 非仓库_status_是事实_init_建独立仓库() {
        let root = std::env::temp_dir().join(format!(
            "kz-git-norepo-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let status = GitTool
            .execute(serde_json::json!({"action": "status"}), &ctx)
            .await;
        // 临时目录的上级恰好是仓库时(某些 CI)会走上级仓库分支,两种事实都不是失败行。
        assert!(!status.is_error, "{}", status.content);
        assert!(status.content.starts_with("not a"), "{}", status.content);
        let commit = GitTool
            .execute(
                serde_json::json!({"action": "commit", "message": "x", "expected_hash": "h"}),
                &ctx,
            )
            .await;
        assert!(commit.is_error, "{}", commit.content);
        let init = GitTool
            .execute(serde_json::json!({"action": "init"}), &ctx)
            .await;
        assert!(!init.is_error, "{}", init.content);
        assert!(root.join(".git").is_dir());
        assert!(root.join(".kanzei").join(".gitignore").is_file());
        let status = GitTool
            .execute(serde_json::json!({"action": "status"}), &ctx)
            .await;
        assert!(!status.is_error, "{}", status.content);
        assert!(!status.content.starts_with("not a"), "{}", status.content);
        let again = GitTool
            .execute(serde_json::json!({"action": "init"}), &ctx)
            .await;
        assert!(again.content.contains("already"), "{}", again.content);
        std::fs::remove_dir_all(&root).ok();
    }
}
