//! Git 命令执行域(R-257 B4):异步 git 执行器与统一超时/控制台隐藏策略。
//! 该块从 git.rs 迁出，保留 D-349 的完整 stdout/stderr 语义。

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

const GIT_TIMEOUT: Duration = Duration::from_secs(60);

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
    // D-349:保留完整 stdout/stderr；统一消费出口会在事件提交前物化超限结果。
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
