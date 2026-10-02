//! Git 命令执行域(R-257 B4):异步 git 执行器与统一超时/控制台隐藏策略。
//! stdout 是 Git 数据；stderr 是诊断，两路完整保留但不混入事实解析。

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

const GIT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub(crate) struct GitCommandOutput {
    pub stdout: String,
    pub stderr: String,
}

impl GitCommandOutput {
    pub(crate) fn take_stdout(self, diagnostics: &mut Vec<String>) -> String {
        if !self.stderr.is_empty() {
            diagnostics.push(self.stderr);
        }
        self.stdout
    }

    pub(crate) fn render(self, empty: &str) -> String {
        let mut text = if self.stdout.trim().is_empty() {
            empty.to_string()
        } else {
            self.stdout.trim_end().to_string()
        };
        append_diagnostics(&mut text, &[self.stderr]);
        text
    }
}

pub(crate) fn append_diagnostics(text: &mut String, diagnostics: &[String]) {
    for stderr in diagnostics.iter().filter(|text| !text.is_empty()) {
        text.push_str("\n[stderr]\n");
        text.push_str(stderr);
    }
}

pub(crate) async fn run_git(cwd: &Path, args: &[&str]) -> Result<GitCommandOutput, String> {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    run_git_owned(cwd, &owned).await
}

pub(crate) async fn run_git_owned(cwd: &Path, args: &[String]) -> Result<GitCommandOutput, String> {
    run_git_owned_with_env(cwd, args, &[]).await
}

pub(super) async fn run_git_owned_with_env(
    cwd: &Path,
    args: &[String],
    env: &[(&str, &str)],
) -> Result<GitCommandOutput, String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.envs(env.iter().copied());
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
    let text = GitCommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    // D-349:保留完整 stdout/stderr；统一消费出口会在事件提交前物化超限结果。
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "git {} failed (exit {:?}):\n{}",
            args.join(" "),
            output.status.code(),
            text.render("")
        ))
    }
}

#[cfg(windows)]
fn hide_console_window(command: &mut tokio::process::Command) {
    crate::hide_console_async(command);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut tokio::process::Command) {}
