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
    /// status | diff | stage | commit
    action: String,
    /// diff/stage 用的逐文件相对路径；stage 禁止目录和通配符。
    #[serde(default)]
    files: Vec<String>,
    /// diff=true 查看暂存区，否则查看工作树。
    #[serde(default)]
    staged: bool,
    /// commit 必填。
    #[serde(default)]
    message: Option<String>,
    /// commit 必填：最近一次 stage 返回的 staged_hash。
    #[serde(default)]
    expected_hash: Option<String>,
}

pub struct GitTool;

#[async_trait]
impl Tool for GitTool {
    fn name(&self) -> &'static str {
        "git"
    }

    fn description(&self) -> String {
        "Safe Git status/diff/stage/commit. stage requires explicit files and returns staged_hash; commit requires that exact hash, so reviewed staged content cannot silently change. Do not use bash for git add/commit.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(GitInput)).unwrap();
        if let Some(action) = schema
            .pointer_mut("/properties/action")
            .and_then(|v| v.as_object_mut())
        {
            action.insert(
                "enum".into(),
                serde_json::json!(["status", "diff", "stage", "commit"]),
            );
        }
        schema
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["action"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        match input["action"].as_str() {
            Some("status" | "diff") => ToolConcurrency::shared_worktree(ctx),
            _ => ToolConcurrency::write_worktree(ctx),
        }
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: GitInput = match crate::parse_input(self, input) {
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
            "stage" => stage(&ctx.cwd, &input.files).await,
            "commit" => commit(&ctx.cwd, input.message, input.expected_hash).await,
            other => ToolOutput::error(format!(
                "unknown action `{other}`; valid: status | diff | stage | commit"
            )),
        }
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
        seen.insert(normalized);
    }
    Ok(seen.into_iter().collect())
}

async fn staged_paths(cwd: &Path) -> Result<Vec<String>, String> {
    let text = run_git(cwd, &["diff", "--cached", "--name-only", "--no-renames"]).await?;
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
    ToolOutput::ok(format!(
        "staged {} file(s): {}\nstaged_hash: {hash}\nReview with `git diff` using staged=true, then commit with this exact expected_hash.",
        paths.len(), paths.join(", ")
    )).with_display(serde_json::json!({
        "kind": "terminal",
        "command": "git stage (structured)",
        "output": diff.chars().take(4000).collect::<String>(),
    }))
}

async fn commit(cwd: &Path, message: Option<String>, expected_hash: Option<String>) -> ToolOutput {
    let message = message.unwrap_or_default();
    if message.trim().is_empty() {
        return ToolOutput::error("`message` is required for commit");
    }
    let Some(expected_hash) = expected_hash else {
        return ToolOutput::error(
            "`expected_hash` is required; call `stage` and review its staged diff first",
        );
    };
    let (current_hash, _, paths) = match staged_state(cwd).await {
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
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut tokio::process::Command) {}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::Tool;

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

    #[tokio::test]
    async fn stage_hash_is_required_and_detects_index_change() {
        let root = temp_repo("cas");
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
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
}
