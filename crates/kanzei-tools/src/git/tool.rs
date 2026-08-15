//! Git 工具适配层(R-257 B4):GitTool 结构、输入契约(GitInput)、execute 分发
//! (git_body + pipeline)与路径处理(normalize_files/preserve_case_path)。自
//! git.rs 原样迁出,零行为变更。

use std::collections::BTreeSet;
use std::path::Path;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use super::commands::{commit, merge_ff, run_git, run_git_owned, stage};
use super::finalize::finalize;

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

pub(crate) fn normalize_files(
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
