//! 结构化 Git 工具：把高频 status/diff/stage/commit 从任意 shell 文本收口为可校验契约。
//! stage 只接受逐文件路径；commit 必须携带上一轮 stage 返回的暂存区指纹，避免夹带旧改动。

use std::collections::{hash_map::DefaultHasher, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

const GIT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT: usize = 1024 * 1024;

#[derive(Deserialize, JsonSchema)]
struct GitInput {
    /// status | diff | log | stage | commit | merge_ff
    action: String,
    /// diff/log 按路径过滤;stage 的逐文件相对路径(禁止目录和通配符)。
    #[serde(default)]
    files: Vec<String>,
    /// diff=true 查看暂存区，否则查看工作树。
    #[serde(default)]
    staged: bool,
    /// log 返回的条数(默认 20,封顶 200)。
    #[serde(default)]
    count: Option<u32>,
    /// commit 必填。
    #[serde(default)]
    message: Option<String>,
    /// commit 必填：最近一次 stage 返回的 staged_hash。
    #[serde(default)]
    expected_hash: Option<String>,
    /// merge_ff 必填:要合入的来源分支/引用(如 `dev`)。
    #[serde(default)]
    from: Option<String>,
    /// merge_ff 的目标分支(如 `main`)。目标检出在其它工作树时会去那棵树里快进;
    /// 未检出时直接快进引用。省略 = 合入当前分支。
    #[serde(default)]
    into: Option<String>,
}

pub struct GitTool;

#[async_trait]
impl Tool for GitTool {
    fn name(&self) -> &'static str {
        "git"
    }

    fn description(&self) -> String {
        "Safe Git status/diff/log/stage/commit/merge_ff. log shows recent commits (count, optional path filter). stage requires explicit files and returns staged_hash; commit requires that exact hash, so reviewed staged content cannot silently change. merge_ff fast-forwards branch `into` from ref `from` (finds the worktree where `into` is checked out; refuses non-fast-forward). Do not use bash for git add/commit/merge.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(GitInput)).unwrap();
        if let Some(action) = schema
            .pointer_mut("/properties/action")
            .and_then(|v| v.as_object_mut())
        {
            action.insert(
                "enum".into(),
                serde_json::json!([
                    "status", "diff", "log", "stage", "commit", "merge_ff", "finalize"
                ]),
            );
        }
        schema
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["action"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        match input["action"].as_str() {
            Some("status" | "diff" | "log") => ToolConcurrency::shared_worktree(ctx),
            _ => ToolConcurrency::write_worktree(ctx),
        }
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        // R-244 批5:git 工具走统一 pipeline(guards/策略/观察者现阶段空,
        // 权限判定在 drive 层;body = 原 execute 逻辑)。
        let input2 = input.clone();
        let ctx2 = ctx.clone();
        kanzei_harness::tool_pipeline::run_tool_pipeline(
            "git",
            input,
            ctx,
            &[],
            async move { git_body(self, &input2, &ctx2).await },
            &[],
            &[],
        )
        .await
    }
}

/// R-244 批5:git 工具本体(原 execute 主体),供 pipeline body 调用。
async fn git_body(tool: &dyn Tool, input: &serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
    let input: GitInput = match crate::parse_input(tool, input.clone()) {
        Ok(value) => value,
        Err(output) => return output,
    };
    if let Err(error) = ensure_repository(&ctx.cwd).await {
        return ToolOutput::error(error);
    }
    match input.action.as_str() {
        "status" => match run_git(&ctx.cwd, &["status", "--short", "--branch"]).await {
            Ok(text) => ToolOutput::ok(if text.trim().is_empty() {
                "(clean worktree)".into()
            } else {
                text
            }),
            Err(error) => ToolOutput::error(error),
        },
        "diff" => {
            let files = match normalize_files(&ctx.cwd, &input.files, false) {
                Ok(files) => files,
                Err(error) => return ToolOutput::error(error),
            };
            let mut args = vec![
                "diff".to_string(),
                "--no-ext-diff".into(),
                "--no-color".into(),
            ];
            if input.staged {
                args.push("--cached".into());
            }
            if !files.is_empty() {
                args.push("--".into());
                args.extend(files);
            }
            match run_git_owned(&ctx.cwd, &args).await {
                Ok(text) => ToolOutput::ok(if text.trim().is_empty() {
                    "(no diff)".into()
                } else {
                    text
                }),
                Err(error) => ToolOutput::error(error),
            }
        }
        // 只读查询,模型排查"最近改了什么/某文件何时动过"的高频入口——
        // 没有它模型只能转投 bash(每次 ask)或干脆瞎猜(D-208 实测被拒)。
        "log" => {
            let files = match normalize_files(&ctx.cwd, &input.files, false) {
                Ok(files) => files,
                Err(error) => return ToolOutput::error(error),
            };
            let count = input.count.unwrap_or(20).clamp(1, 200);
            let mut args = vec![
                "log".to_string(),
                format!("--format=%h %ad %an | %s"),
                "--date=format:%m-%d %H:%M".into(),
                format!("-{count}"),
            ];
            if !files.is_empty() {
                args.push("--".into());
                args.extend(files);
            }
            match run_git_owned(&ctx.cwd, &args).await {
                Ok(text) => ToolOutput::ok(if text.trim().is_empty() {
                    "(no commits)".into()
                } else {
                    text
                }),
                Err(error) => ToolOutput::error(error),
            }
        }
        "stage" => stage(&ctx.cwd, &input.files).await,
        "commit" => commit(ctx, input.message, input.expected_hash).await,
        "merge_ff" => merge_ff(&ctx.cwd, input.from, input.into).await,
        // D-334:finalize 事务化——Agent 一次调用,Harness 机械完成
        // fmt → 相关测试 → test_record → stage → CAS commit,不再手动驾驶。
        "finalize" => finalize(ctx, input.files, input.message).await,
        other => ToolOutput::error(format!(
            "unknown action `{other}`; valid: status | diff | log | stage | commit | merge_ff"
        )),
    }
}

async fn ensure_repository(cwd: &Path) -> Result<(), String> {
    run_git(cwd, &["rev-parse", "--show-toplevel"])
        .await
        .map(|_| ())
}

fn normalize_files(
    cwd: &Path,
    files: &[String],
    require_non_empty: bool,
) -> Result<Vec<String>, String> {
    if require_non_empty && files.is_empty() {
        return Err("`files` must list every path explicitly; directories, `.` and wildcards are not accepted".into());
    }
    let mut seen = BTreeSet::new();
    for raw in files {
        let raw = raw.trim();
        if raw.is_empty() || Path::new(raw).is_absolute() || raw.contains('*') || raw.contains('?')
        {
            return Err(format!(
                "invalid Git path `{raw}`; use an explicit repository-relative file path"
            ));
        }
        // 安全校验：normalize_resource 折叠 ..、清理 .，Windows 上还会小写化整个路径——
        // 只用于逃逸与目录判定，不用于传给 git 的路径。
        let normalized = kanzei_harness::permission::normalize_resource(raw);
        if normalized == "."
            || normalized == ".."
            || normalized.starts_with("../")
            || normalized.contains(':')
        {
            return Err(format!(
                "Git path `{raw}` escapes or names the whole worktree"
            ));
        }
        if cwd.join(&normalized).is_dir() {
            return Err(format!(
                "Git path `{raw}` is a directory; list its files individually"
            ));
        }
        // 传给 git 的路径必须保留原始大小写：normalize_resource 在 Windows 上 to_lowercase
        // 整个路径，而 git pathspec 大小写敏感，小写化会让含大写字母的文件（INDEX.md、
        // Cargo.lock 等）匹配不到，stage 静默失败（D-178）。
        seen.insert(preserve_case_path(raw));
    }
    Ok(seen.into_iter().collect())
}

/// 轻量路径清理：统一分隔符、折叠 `.`/`..`，但**不**小写化。
/// 与 `kanzei_harness::permission::normalize_resource` 的区别正在于此——后者在
/// Windows 上会破坏大小写敏感路径（D-178 根因）。
fn preserve_case_path(raw: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    // 先绑定再借用:`raw.replace(..)` 是临时值,直接在 for 头里 split 会在
    // 迭代开始前被丢弃(E0716)。
    let unified = raw.replace('\\', "/");
    for segment in unified.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(segments.last(), Some(&last) if last != "..") {
                    segments.pop();
                } else {
                    segments.push("..");
                }
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

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

/// D-332 验收④:暂存**源码**的指纹 hash(fnv)——test_record 收尾时用它背书「测试跑的是
/// 这份源码」,提交门禁优先比指纹而不是纯 mtime。要点:
/// - 只对源码路径(`is_source_path`)的 staged diff 求 hash:tests.md/tracker 等托管文档
///   的写入不改变源码指纹,不会再让「test_record 自己改 tests.md」触发源码重测。
/// - 与 staged_state 的全体 hash 不同:commit 门禁的 CAS 用全体 hash(防任何内容漂移),
///   测试背书用源码 hash(fmt 后源码 diff 变 → 指纹变 → 要求重测,保守正确)。
/// - 同步实现(内部 std::process::Command 跑 git):test_record 工具(async)与
///   source_test_gate(同步)都要用,拆两个 async 版本会让门禁被迫改签名。
pub fn staged_source_fingerprint(cwd: &Path) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args([
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-color",
        ])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("cannot run git diff: {e}"))?;
    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    let paths = staged_paths_sync(cwd)?;
    let source_paths: Vec<String> = paths
        .iter()
        .filter(|p| is_source_path(p))
        .cloned()
        .collect();
    if source_paths.is_empty() {
        // 本次暂存全是非源码(测试记录/文档):指纹为空,门禁按旧逻辑(mtime)走。
        return Ok(String::new());
    }
    let mut hasher = DefaultHasher::new();
    // 只对源码路径的 diff 段求 hash:按 `diff --git a/<path> b/<path>` 头切块,
    // 命中源码路径的块进 hash,其余(测试记录/文档)排除。
    let mut in_source = false;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            let path = rest
                .split_once(" b/")
                .map(|(p, _)| p.to_string())
                .unwrap_or_default();
            in_source = source_paths.contains(&path);
        }
        if in_source {
            line.hash(&mut hasher);
        }
    }
    Ok(format!("{:016x}", hasher.finish()))
}

fn staged_paths_sync(cwd: &Path) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "core.quotepath=false", // D-347:非 ASCII 路径以真实 UTF-8 返回,与请求路径可比
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
        ])
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("cannot run git diff --name-only: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff --cached failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

async fn stage(cwd: &Path, raw_files: &[String]) -> ToolOutput {
    let files = match normalize_files(cwd, raw_files, true) {
        Ok(files) => files,
        Err(error) => return ToolOutput::error(error),
    };
    let requested: BTreeSet<&str> = files.iter().map(String::as_str).collect();
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

/// 提交里算「源码」的路径。改这两棵树就要有测试背书;`.kanzei/` 下的文档不算。
fn is_source_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    (path.starts_with("crates/") || path.starts_with("scripts/")) && !path.contains("/.kanzei/")
}

/// 提交里算「tracker 文档」的路径:需求/缺陷/测试记录及其归档。
/// R-227 门禁只扫这些文件——占位符测试 ID 只可能出现在关闭证据/进展叙述里。
fn is_tracker_path(path: &str) -> bool {
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

/// 编译诊断回退(R-210):正常提交路径不再串行跑 cargo check——clippy_gate 的
/// `cargo clippy --workspace --all-targets` 走同一编译管线(全 workspace 编译 +
/// 类型检查),编译错误会被 clippy 拦截并带 `-->` 位置,双份全仓分析是纯冗余。
/// 本函数只在 clippy 失败输出**缺位置信息**时作为诊断回退调用。
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
        .current_dir(cwd);
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
        .current_dir(cwd);
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

/// clippy 门禁:提交源码前由工具**亲自**跑 `cargo clippy --workspace --all-targets -- -D warnings`(D-264)。
///
/// 2026-08-11 实例:新增集成测试落在 crates/kanzei/tests/,而自举只跑了「改动最多的
/// crate」的定向测试,`-p kanzei` 从未被 clippy 覆盖——6 条 lint 红灯随提交进库。
/// 教训:lint 可能只在**别的 crate**(新增测试所在的 crate、被改动 crate 的依赖方)
/// 暴露,定向 `-p <改动 crate>` 不够;全 workspace 才能兜住。命令与 CI/verify.ps1
/// 完全同参数(守护测试比对)。
async fn clippy_gate(cwd: &Path) -> Result<(), String> {
    if !cwd.join("Cargo.toml").is_file() {
        return Ok(());
    }
    let mut command = tokio::process::Command::new("cargo");
    command
        .args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])
        .current_dir(cwd);
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
            let mut message = format!(
                "提交被拦下:`cargo clippy --workspace --all-targets -- -D warnings` 不过。\n{}",
                head.join("\n")
            );
            // R-210:clippy 全 workspace 编译覆盖 check,正常路径不再跑 check;
            // 仅当 clippy 失败输出缺位置信息(编译错误应带 `-->`,缺了说明诊断
            // 没透出来)时,降级跑 cargo check 补位置诊断。
            if !stderr.contains("-->") {
                if let Err(diag) = compile_gate(cwd).await {
                    message.push_str(&format!("\n--- cargo check 诊断回退 ---\n{diag}"));
                }
            }
            Err(message)
        }
        Err(error) => Err(format!(
            "提交被拦下:无法执行 cargo clippy({error})。装好 cargo/clippy 或在非 Rust 仓库里提交。"
        )),
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
    match crate::test_record::last_passed(project_root) {
        None => Err(format!("提交被拦下:没有任何 passed 的测试记录。\n{remedy}")),
        Some((passed_at, coverage, command_text, fingerprint)) => {
            // D-332 验收④:优先比源码指纹——测试记录背书的是「收尾那一刻的暂存源码」。
            // 指纹非空且与当前暂存源码一致 = 背书成立(即使 mtime 因 test_record 自己写
            // tests.md 而变新,源码没变就不要求重测)。指纹为空(旧记录/非 git)时退回 mtime。
            let current_fingerprint = staged_source_fingerprint(cwd).unwrap_or_default();
            if !fingerprint.is_empty() && !current_fingerprint.is_empty() {
                if fingerprint != current_fingerprint {
                    return Err(format!(
                        "提交被拦下:最近一条 passed 测试记录背书的源码指纹与当前暂存源码不一致\
                         (记录: {fingerprint}, 当前: {current_fingerprint})——fmt/改动后源码变了,\
                         这条记录背书的不是要提交的这份代码。\n{remedy}"
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
fn source_crates(paths: &[String]) -> std::collections::BTreeSet<String> {
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
        // ——规则层写过但自举漏了三次,必须代码强制。R-210:不再串行跑 cargo check
        // (clippy 全 workspace 编译覆盖 check,双份全仓分析是纯冗余;clippy 输出
        // 缺位置信息时才降级跑 check 补诊断)。
        if let Err(error) = fmt_gate(cwd).await {
            return ToolOutput::error(error);
        }
        if let Err(error) = clippy_gate(cwd).await {
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
async fn finalize(ctx: &ToolCtx, files: Vec<String>, message: Option<String>) -> ToolOutput {
    let cwd = &ctx.cwd;
    let message = message.unwrap_or_default();
    if message.trim().is_empty() {
        return ToolOutput::error("`message` is required for finalize");
    }
    if files.is_empty() {
        return ToolOutput::error(
            "`files` is required for finalize: explicitly list the files to commit",
        );
    }
    let sources: Vec<String> = files
        .iter()
        .filter(|p| p.ends_with(".rs") || p.ends_with("Cargo.toml"))
        .cloned()
        .collect();

    // 1. fmt gate(先于测试——D-334 核心:别再「测完了才发现 fmt 没过」)。
    if !sources.is_empty() {
        if let Err(error) = fmt_gate(cwd).await {
            return ToolOutput::error(format!(
                "[finalize] fmt gate failed (before tests):\n{error}"
            ));
        }
        if let Err(error) = clippy_gate(cwd).await {
            return ToolOutput::error(format!(
                "[finalize] clippy gate failed (before tests):\n{error}"
            ));
        }
    }

    // 2. 相关测试命令:暂存源码所属 crate 集合;无 crate 改动时退化为 workspace。
    let staged_crates = source_crates(&sources);
    let test_command = if staged_crates.is_empty() {
        "cargo test --workspace".to_string()
    } else {
        staged_crates
            .iter()
            .map(|c| format!("cargo test -p {c}"))
            .collect::<Vec<_>>()
            .join(" && ")
    };

    // 3. 跑测试(超时 10 分钟;失败返回测试输出,不 stage 不 commit)。
    let started = std::time::Instant::now();
    let mut command = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(&test_command);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(&test_command);
        c
    };
    command.current_dir(cwd);
    crate::hide_console_async(&mut command);
    let output =
        match tokio::time::timeout(std::time::Duration::from_secs(600), command.output()).await {
            Ok(out) => match out {
                Ok(output) => output,
                Err(error) => {
                    return ToolOutput::error(format!(
                        "[finalize] failed to run `{test_command}`: {error}"
                    ))
                }
            },
            Err(_) => {
                return ToolOutput::error(format!(
                    "[finalize] tests timed out after 600s: `{test_command}`"
                ))
            }
        };
    let duration_secs = started.elapsed().as_secs_f64();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr
            .lines()
            .filter(|l| l.contains("test result") || l.starts_with("error"))
            .take(12)
            .collect();
        return ToolOutput::error(format!(
            "[finalize] tests failed: `{test_command}`\n{}",
            tail.join("\n")
        ));
    }
    let passed_summary = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| l.contains("test result: ok"))
        .map(str::to_string)
        .collect::<Vec<_>>()
        .join("; ");

    // 4. test_record:记 passed,带 source_fingerprint(与暂存源码一致)与时长。
    let fingerprint = staged_source_fingerprint(cwd).unwrap_or_default();
    let summary = if passed_summary.is_empty() {
        "finalize 测试通过(无 test result 行,或纯非 Rust 改动)".to_string()
    } else {
        passed_summary
    };
    if let Err(error) = crate::test_record::record_test_run_with_duration(
        &ctx.project_root,
        None,
        &format!("git finalize (auto): {test_command}"),
        "passed",
        Some(&test_command),
        Some(&summary),
        None,
        Some(duration_secs),
        Some(&fingerprint),
    ) {
        return ToolOutput::error(format!("[finalize] test_record failed: {error}"));
    }

    // 5. stage(显式文件,与既有 stage 同语义)。
    let staged = stage(cwd, &files).await;
    let ToolOutput {
        content,
        is_error,
        display,
        ..
    } = staged;
    if is_error {
        return ToolOutput::error(format!("[finalize] stage failed:\n{content}"));
    }
    // stage 返回里解析 staged_hash(格式固定:含 `staged_hash: <hash>` 行)。
    let Some(hash_line) = content.lines().find(|l| l.contains("staged_hash:")) else {
        return ToolOutput::error(format!(
            "[finalize] stage succeeded but staged_hash not found in output:\n{content}"
        ));
    };
    let staged_hash = hash_line
        .split("staged_hash:")
        .nth(1)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if staged_hash.is_empty() {
        return ToolOutput::error("[finalize] staged_hash empty after stage");
    }

    // 6. CAS commit(消费 staged_hash)。
    let committed = commit(ctx, Some(message), Some(staged_hash.clone())).await;
    if committed.is_error {
        return ToolOutput::error(format!(
            "[finalize] commit failed after successful stage+test (staged_hash {staged_hash}):\n{}",
            committed.content
        ));
    }
    ToolOutput::ok(format!(
        "[finalize] complete: {test_command} passed in {duration_secs:.1}s → staged {staged_hash} → committed\n{content}\n{}",
        committed.content
    ))
    .with_display(display.unwrap_or_else(|| serde_json::json!({ "kind": "terminal" })))
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

/// `git worktree list --porcelain` 的一条记录。
///
/// R-177 内容③:线清单的**真源**是 git,不是前端 localStorage。手工
/// `git worktree add` 出来的树、以及 kzapp 换了一台机器/清了缓存之后的树,
/// 都必须能被发现——localStorage 清单三者一个都做不到。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: std::path::PathBuf,
    /// 检出的分支短名(`refs/heads/` 已剥)。detached / bare 时为 None。
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
}

/// 解析 `git worktree list --porcelain`。
///
/// porcelain 的形状:每条记录以 `worktree <path>` 开头,后面跟若干属性行,
/// 记录之间用空行分隔。`branch` / `bare` / `detached` / `locked` / `prunable`
/// 都可能出现,`locked` 与 `prunable` 还可能带一个原因串(`locked <reason>`)。
pub fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeEntry> {
    let mut out: Vec<WorktreeEntry> = Vec::new();
    for line in porcelain.lines() {
        let line = line.trim_end();
        if let Some(path) = line.strip_prefix("worktree ") {
            out.push(WorktreeEntry {
                path: std::path::PathBuf::from(path.trim()),
                branch: None,
                bare: false,
                detached: false,
                locked: false,
                prunable: false,
            });
            continue;
        }
        let Some(current) = out.last_mut() else {
            continue; // 属性行出现在任何 `worktree` 之前:不是合法输出,跳过。
        };
        if let Some(head) = line.strip_prefix("branch ") {
            current.branch = Some(
                head.trim()
                    .strip_prefix("refs/heads/")
                    .unwrap_or(head.trim())
                    .to_string(),
            );
        } else if line == "bare" {
            current.bare = true;
        } else if line == "detached" {
            current.detached = true;
        } else if line == "locked" || line.starts_with("locked ") {
            current.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            current.prunable = true;
        }
    }
    out
}

/// 找到检出了指定分支的工作树路径。
fn worktree_for_branch(porcelain: &str, branch: &str) -> Option<std::path::PathBuf> {
    parse_worktree_list(porcelain)
        .into_iter()
        .find(|entry| entry.branch.as_deref() == Some(branch))
        .map(|entry| entry.path)
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

async fn run_git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    run_git_owned(cwd, &owned).await
}

async fn run_git_owned(cwd: &Path, args: &[String]) -> Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
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

    /// D-264 验收② + R-209:git.rs 提交门禁、发版门禁(verify.ps1)与 CI(ci.yml)
    /// 的**完整检查项集合**机械同步——任一侧增删一步即红(不再只比对 fmt/clippy 两项)。
    ///
    /// 口径:verify.ps1 的 `Step-With-Timing "<key>"` 键集合必须等于固定清单
    /// {fmt, clippy, test, ui_syntax, ui_runtime, ui_lint, parallel_lines_regression,
    /// ui_a11y, ui_i18n, ui_markdown};每个键在 ci.yml 里有对应标记(命令文本或
    /// smoke 脚本名);smoke 脚本与 npm ci 在两侧同现同隐。
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
            "ui_syntax",
            "ui_runtime",
            "ui_lint",
            "parallel_lines_regression",
            "ui_a11y",
            "ui_i18n",
            "ui_markdown",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let actual = verify_check_keys(&verify);
        assert_eq!(
            actual, expected,
            "verify.ps1 检查键集合必须等于固定清单——新增/删除门禁时 ci.yml 与 git.rs 也要同步"
        );

        // ② 每个键在 ci.yml 有对应标记(命令文本或 smoke 脚本名)。
        let markers: [(&str, &str); 10] = [
            ("fmt", "cargo fmt --all -- --check"),
            ("clippy", "cargo clippy --workspace --all-targets"),
            ("test", "cargo test --workspace"),
            ("ui_syntax", "node --check"),
            ("ui_runtime", "ui-runtime-smoke.mjs"),
            ("ui_lint", "ui-lint-smoke.mjs"),
            ("parallel_lines_regression", "parallel-lines-regression.mjs"),
            ("ui_a11y", "ui-a11y-smoke.mjs"),
            ("ui_i18n", "ui-i18n-smoke.mjs"),
            ("ui_markdown", "ui-markdown-smoke.mjs"),
        ];
        for (key, marker) in markers {
            assert!(ci.contains(marker), "ci.yml 缺检查 {key}(标记 {marker})");
        }

        // ③ 反向:smoke 脚本在两侧同现同隐;npm ci 必须存在(ui-lint 依赖 eslint)。
        for script in [
            "ui-runtime-smoke.mjs",
            "ui-lint-smoke.mjs",
            "parallel-lines-regression.mjs",
            "ui-a11y-smoke.mjs",
            "ui-i18n-smoke.mjs",
            "ui-markdown-smoke.mjs",
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
        assert!(
            this.contains("cargo clippy --workspace --all-targets -- -D warnings"),
            "clippy_gate 命令与 CI 不一致"
        );
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

    /// R-210 验收①:删掉串行 cargo check 后,编译错误仍被 clippy_gate 拦下,
    /// 且报错含 `-->` 位置——clippy 全 workspace 编译覆盖 check 的实证。
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
}
