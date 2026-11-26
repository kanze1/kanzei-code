//! 生命周期 Hooks 执行器（零 Token 确定性自动化护栏）。
//!
//! 通过 `tokio::task_local!` 作用域在一次 runner 执行中注入当前配置生效的钩子列表，
//! 串行与并行工具管道共用同一执行入口，不污染 `ToolCtx` 结构体。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::config::{HookEvent, HookRule};

tokio::task_local! {
    static ACTIVE_HOOKS: Arc<Vec<HookRule>>;
}

/// 在给定异步作用域内安装生效的生命周期钩子集合。
pub async fn scope<F, T>(rules: Arc<Vec<HookRule>>, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    ACTIVE_HOOKS.scope(rules, fut).await
}

/// 读取当前异步作用域内的钩子集合；未安装作用域时返回空。
pub fn current_rules() -> Arc<Vec<HookRule>> {
    ACTIVE_HOOKS
        .try_with(Arc::clone)
        .unwrap_or_else(|_| Arc::new(Vec::new()))
}

#[derive(Debug, Clone)]
pub struct HookExecution {
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl HookExecution {
    pub fn ok(&self) -> bool {
        !self.timed_out && self.exit_code == 0
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.stdout.trim().is_empty() {
            parts.push(self.stdout.trim().to_string());
        }
        if !self.stderr.trim().is_empty() {
            parts.push(self.stderr.trim().to_string());
        }
        let body = parts.join("\n");
        if self.timed_out {
            format!(
                "hook `{}` 超时终止{}",
                self.command,
                if body.is_empty() {
                    String::new()
                } else {
                    format!(": {body}")
                }
            )
        } else if body.is_empty() {
            format!("hook `{}` 返回退出码 {}", self.command, self.exit_code)
        } else {
            format!(
                "hook `{}` (exit {}): {}",
                self.command, self.exit_code, body
            )
        }
    }
}

pub struct HookContext<'a> {
    pub event: HookEvent,
    pub cwd: &'a Path,
    pub project_root: &'a Path,
    pub tool_name: Option<&'a str>,
    pub tool_input: Option<&'a str>,
    pub tool_outcome: Option<&'a str>,
}

const MAX_HOOK_OUTPUT_BYTES: usize = 8 * 1024;

fn truncate_utf8(bytes: &[u8], max_bytes: usize) -> String {
    if bytes.len() <= max_bytes {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !std::str::from_utf8(&bytes[..end]).is_ok() {
        end -= 1;
    }
    format!(
        "{}\n...[hook 输出已截断]",
        String::from_utf8_lossy(&bytes[..end])
    )
}

async fn run_single_hook(rule: &HookRule, ctx: &HookContext<'_>) -> HookExecution {
    let command_text = rule.command.trim();
    let timeout = Duration::from_secs(rule.effective_timeout_secs());
    let workdir = if ctx.cwd.is_dir() {
        ctx.cwd
    } else if ctx.project_root.is_dir() {
        ctx.project_root
    } else {
        Path::new(".")
    };

    #[cfg(windows)]
    let mut cmd = {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = tokio::process::Command::new("pwsh");
        c.args(["-NoProfile", "-NonInteractive", "-Command", command_text]);
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };

    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command_text]);
        c
    };

    cmd.current_dir(workdir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("KANZEI_HOOK_EVENT", ctx.event.as_str())
        .env(
            "KANZEI_PROJECT_ROOT",
            ctx.project_root.display().to_string(),
        )
        .env("KANZEI_CWD", workdir.display().to_string())
        .kill_on_drop(true);

    if let Some(name) = ctx.tool_name {
        cmd.env("KANZEI_TOOL_NAME", name);
    }
    if let Some(input) = ctx.tool_input {
        let clipped: String = input.chars().take(4096).collect();
        cmd.env("KANZEI_TOOL_INPUT", clipped);
    }
    if let Some(outcome) = ctx.tool_outcome {
        cmd.env("KANZEI_TOOL_OUTCOME", outcome);
    }

    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            return HookExecution {
                command: command_text.to_string(),
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("无法启动钩子进程: {error}"),
                timed_out: false,
            };
        }
    };

    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => HookExecution {
            command: command_text.to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: truncate_utf8(&output.stdout, MAX_HOOK_OUTPUT_BYTES),
            stderr: truncate_utf8(&output.stderr, MAX_HOOK_OUTPUT_BYTES),
            timed_out: false,
        },
        Ok(Err(error)) => HookExecution {
            command: command_text.to_string(),
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("读取钩子输出失败: {error}"),
            timed_out: false,
        },
        Err(_) => HookExecution {
            command: command_text.to_string(),
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("钩子执行超过 {}s 超时", rule.effective_timeout_secs()),
            timed_out: true,
        },
    }
}

/// 顺序运行匹配当前事件与工具的全部已启用钩子。
/// 对 `PreTool` 事件，遇到第一条非 0 退出码/超时的钩子即刻短路返回（阻断执行）。
pub async fn run_hooks(rules: &[HookRule], ctx: &HookContext<'_>) -> Vec<HookExecution> {
    let mut results = Vec::new();
    for rule in rules {
        if !rule.enabled || rule.event != ctx.event || rule.command.trim().is_empty() {
            continue;
        }
        if let Some(tool_name) = ctx.tool_name {
            if !rule.matches_tool(tool_name) {
                continue;
            }
        }
        let exec = run_single_hook(rule, ctx).await;
        let failed = !exec.ok();
        results.push(exec);
        if ctx.event == HookEvent::PreTool && failed {
            break;
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_matcher_supports_exact_csv_and_wildcards() {
        let rule = HookRule {
            event: HookEvent::PreTool,
            tool_matcher: Some("edit, write, memory_*".into()),
            command: "echo ok".into(),
            timeout_secs: Some(5),
            enabled: true,
        };
        assert!(rule.matches_tool("edit"));
        assert!(rule.matches_tool("write"));
        assert!(rule.matches_tool("memory_note"));
        assert!(!rule.matches_tool("bash"));
    }

    #[tokio::test]
    async fn pre_tool_hook_blocks_on_nonzero_exit() {
        let tmp = std::env::temp_dir();
        let rules = vec![HookRule {
            event: HookEvent::PreTool,
            tool_matcher: Some("bash".into()),
            command: "Write-Error 'blocked by policy'; exit 2".into(),
            timeout_secs: Some(5),
            enabled: true,
        }];
        let res = run_hooks(
            &rules,
            &HookContext {
                event: HookEvent::PreTool,
                cwd: &tmp,
                project_root: &tmp,
                tool_name: Some("bash"),
                tool_input: Some("{\"command\":\"rm -rf\"}"),
                tool_outcome: None,
            },
        )
        .await;
        assert_eq!(res.len(), 1);
        assert!(!res[0].ok());
        assert_eq!(res[0].exit_code, 2);
    }
}
