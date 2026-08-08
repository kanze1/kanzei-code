//! bash(shell)工具。设计红线 6:按实际检测到的 shell 动态生成描述,
//! 让模型知道自己面对的是 pwsh/cmd 还是 POSIX sh;超时返回结构化结果而非报错。

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::io::AsyncReadExt;

use crate::shell::{detected_shell, kill_tree};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

#[derive(Deserialize, JsonSchema)]
struct BashInput {
    /// 要执行的命令
    #[serde(alias = "cmd", alias = "script")]
    command: String,
    /// 超时毫秒(默认 120000,上限 600000)
    #[serde(default)]
    timeout_ms: Option<u64>,
    /// 工作目录(默认 cwd)
    #[serde(default)]
    workdir: Option<String>,
    /// 后台运行:立刻返回进程句柄,用 process 工具查输出/停止(长驻服务、watch 用)
    #[serde(default)]
    background: bool,
}

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> String {
        let shell = detected_shell();
        let syntax = match shell.name {
            "pwsh" | "powershell" => {
                "PowerShell syntax (NOT POSIX: use ; or && (pwsh7), $env:VAR, Get-ChildItem)"
            }
            "cmd" => "cmd.exe syntax (NOT POSIX: use %VAR%, dir, &&)",
            _ => "POSIX sh syntax",
        };
        format!(
            "Run a shell command via {} — {syntax}. Params: command; optional timeout_ms, workdir, \
             background. stdin is closed: interactive prompts get EOF instead of hanging. \
             Set background=true for long-running processes (dev server, watch): it returns a \
             process id immediately; use the `process` tool to read output, check liveness or stop it.",
            shell.name
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(BashInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        let command = input["command"].as_str().unwrap_or("*");
        let Some(workdir) = input["workdir"].as_str().filter(|dir| !dir.is_empty()) else {
            return vec![command.to_string()];
        };
        vec![serde_json::json!({
            "command": command,
            "workdir": kanzei_harness::permission::normalize_resource(workdir),
        })
        .to_string()]
    }

    fn resources_with_ctx(
        &self,
        input: &serde_json::Value,
        ctx: &ToolCtx,
    ) -> Vec<String> {
        let command = input["command"].as_str().unwrap_or("*");
        let workdir = input["workdir"].as_str().filter(|dir| !dir.is_empty());
        let effective_workdir = ctx.cwd.join(
            workdir
                .map(kanzei_harness::permission::normalize_resource)
                .unwrap_or_else(|| ".".into()),
        );
        vec![serde_json::json!({
            "command": command,
            "workdir": kanzei_harness::permission::normalize_resource(
                &effective_workdir.display().to_string(),
            ),
        })
        .to_string()]
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        // Shell 命令可产生任意副作用，不能靠解析命令文本猜测“只读”。
        ToolConcurrency::write_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: BashInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        // D-113 硬门禁:整文件覆写 cmdlet 绕过 edit/write 的语法校验与 diff 展示,一律拦截。
        if let Some(cmdlet) = full_file_write_cmdlet(&input.command) {
            return ToolOutput::error(format!(
                "`{cmdlet}` is blocked: whole-file rewrites via shell bypass the edit/write \
                 tools' syntax validation and diff display. Use `edit` for targeted changes \
                 (it tolerates line-ending differences and, after two misses, shows you the \
                 file's actual content) or `write` to create/replace a file deliberately."
            ));
        }
        let timeout = Duration::from_millis(
            input
                .timeout_ms
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .min(MAX_TIMEOUT_MS),
        );
        let workdir = match &input.workdir {
            Some(dir) => ctx
                .cwd
                .join(kanzei_harness::permission::normalize_resource(dir)),
            None => ctx.cwd.clone(),
        };
        if !workdir.is_dir() {
            return ToolOutput::error(format!("workdir does not exist: {}", workdir.display()));
        }

        let shell = detected_shell();
        let mut command = tokio::process::Command::new(&shell.program);
        command
            .args(&shell.args)
            .arg(&input.command)
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        hide_console_window(&mut command);

        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("failed to spawn {}: {e}", shell.name)),
        };

        // 后台模式:交给注册表托管,立刻返回句柄,不等它结束也不受 timeout 约束。
        if input.background {
            let process = crate::background::register(
                child,
                input.command.clone(),
                &ctx.project_root,
                &workdir,
            );
            let rendered = format!(
                "background: true\nprocess_id: {}\npid: {}\ncommand: {}\n\
                 Use the `process` tool: {{\"action\":\"output\",\"id\":\"{}\"}} to read output, \
                 {{\"action\":\"stop\",\"id\":\"{}\"}} to terminate.",
                process.id,
                process.pid().map_or("unknown".into(), |p| p.to_string()),
                input.command,
                process.id,
                process.id,
            );
            return ToolOutput::ok(rendered).with_display(serde_json::json!({
                "kind": "terminal",
                "command": input.command,
                "background": true,
                "processId": process.id,
                "output": "(后台运行中,用 process 工具查看输出)",
            }));
        }

        let pid = child.id();

        let mut stdout = child.stdout.take().expect("stdout piped");
        let mut stderr = child.stderr.take().expect("stderr piped");
        // 缓冲放在 future 外:超时把整个 future drop 掉时,已经读到的输出必须还在,
        // 否则模型对"卡在哪一步"一无所知,只能盲目加大 timeout 重跑并重复副作用(D-062)。
        let (mut out_buf, mut err_buf) = (Vec::new(), Vec::new());
        let capture = {
            let out_buf = &mut out_buf;
            let err_buf = &mut err_buf;
            async move {
                // 有界读取:两条流各自最多 MAX_CAPTURE_BYTES,超出丢弃(内存红线)。
                let (a, b) = tokio::join!(
                    read_capped(&mut stdout, out_buf),
                    read_capped(&mut stderr, err_buf)
                );
                let status = child.wait().await;
                (status, a, b)
            }
        };

        let outcome = tokio::time::timeout(timeout, capture).await;
        match outcome {
            Ok((status, out_capped, err_capped)) => {
                let mut text = String::from_utf8_lossy(&out_buf).into_owned();
                if out_capped {
                    text.push_str("\n[stdout truncated at 1 MiB]");
                }
                if !err_buf.is_empty() {
                    text.push_str("\n[stderr]\n");
                    text.push_str(&String::from_utf8_lossy(&err_buf));
                    if err_capped {
                        text.push_str("\n[stderr truncated at 1 MiB]");
                    }
                }
                let code = status.as_ref().ok().and_then(|s| s.code());
                let ok = code == Some(0);
                let text = if text.trim().is_empty() {
                    "(no output)".to_string()
                } else {
                    text
                };
                let mut rendered = format!(
                    "exit code: {}\n{text}",
                    code.map_or("unknown".into(), |c| c.to_string())
                );
                // D-112 门禁:提交成功后附带实际提交的文件清单——模型必须核对
                // 它与计划一致(尤其 tracker 归档文件必须与活动文档同行)。
                if ok && looks_like_git_commit(&input.command) {
                    if let Some(stat) = git_head_stat(&workdir).await {
                        rendered.push_str(
                            "\n[committed files — VERIFY this matches what you planned to commit]\n",
                        );
                        rendered.push_str(&stat);
                    }
                }
                let display = serde_json::json!({
                    "kind": "terminal",
                    "command": input.command,
                    "exitCode": code,
                    "output": text.chars().take(4000).collect::<String>(),
                });
                let output = if ok {
                    ToolOutput::ok(rendered)
                } else {
                    ToolOutput::error(rendered)
                };
                output.with_display(display)
            }
            Err(_) => {
                if let Some(pid) = pid {
                    kill_tree(pid).await;
                }
                // 超时是可预期结果:结构化告知,并回传已捕获的输出(卡在哪一步全靠它)。
                let mut text = format!(
                    "timeout: true — command did not finish within {} ms and was killed. Retry with a larger timeout_ms if needed.",
                    timeout.as_millis()
                );
                let partial_out = String::from_utf8_lossy(&out_buf).into_owned();
                let partial_err = String::from_utf8_lossy(&err_buf).into_owned();
                if partial_out.trim().is_empty() && partial_err.trim().is_empty() {
                    text.push_str("\n[no output captured before timeout]");
                } else {
                    if !partial_out.trim().is_empty() {
                        text.push_str("\n[partial stdout before timeout]\n");
                        text.push_str(&partial_out);
                    }
                    if !partial_err.trim().is_empty() {
                        text.push_str("\n[partial stderr before timeout]\n");
                        text.push_str(&partial_err);
                    }
                }
                let display = serde_json::json!({
                    "kind": "terminal",
                    "command": input.command,
                    "exitCode": serde_json::Value::Null,
                    "timeout": true,
                    "output": text.chars().take(4000).collect::<String>(),
                });
                // 超时不是成功:按错误返回,上层不再把它计入正常完成。
                ToolOutput::error(text).with_display(display)
            }
        }
    }
}

/// 命令中出现整文件覆写 cmdlet 时返回其名称(词边界匹配,Get-Content 不误伤)。
fn full_file_write_cmdlet(command: &str) -> Option<&'static str> {
    let lower = command.to_ascii_lowercase();
    for (needle, name) in [("set-content", "Set-Content"), ("out-file", "Out-File")] {
        let mut search_from = 0;
        while let Some(pos) = lower[search_from..].find(needle) {
            let absolute = search_from + pos;
            let bounded_left = absolute == 0
                || !matches!(lower.as_bytes()[absolute - 1], b'a'..=b'z' | b'0'..=b'9' | b'-');
            let after = absolute + needle.len();
            let bounded_right = after >= lower.len()
                || !matches!(lower.as_bytes()[after], b'a'..=b'z' | b'0'..=b'9' | b'-');
            if bounded_left && bounded_right {
                return Some(name);
            }
            search_from = after;
        }
    }
    None
}

fn looks_like_git_commit(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    lower.contains("git") && lower.contains("commit")
}

/// 提交后的实际文件清单(git show --stat HEAD);失败静默返回 None,不影响主结果。
async fn git_head_stat(workdir: &std::path::Path) -> Option<String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args(["show", "--stat", "--no-color", "--format=%h %s", "HEAD"])
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    hide_console_window(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(2000).collect())
}

#[cfg(windows)]
fn hide_console_window(command: &mut tokio::process::Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut tokio::process::Command) {}

async fn read_capped(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    buffer: &mut Vec<u8>,
) -> bool {
    let mut chunk = [0u8; 8192];
    let mut capped = false;
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                if buffer.len() < MAX_CAPTURE_BYTES {
                    let take = n.min(MAX_CAPTURE_BYTES - buffer.len());
                    buffer.extend_from_slice(&chunk[..take]);
                    if take < n {
                        capped = true;
                    }
                } else {
                    capped = true;
                }
            }
            Err(_) => break,
        }
    }
    capped
}

#[cfg(test)]
mod tests {
    use super::{full_file_write_cmdlet, looks_like_git_commit, BashTool};
    use kanzei_harness::{Tool, ToolCtx};
    use std::path::PathBuf;

    #[test]
    fn whole_file_write_cmdlets_are_detected_with_word_boundaries() {
        // D-113:拦截整文件覆写,但不误伤 Get-Content 等读取。
        assert_eq!(
            full_file_write_cmdlet("Set-Content -Path main.rs -Value $code"),
            Some("Set-Content")
        );
        assert_eq!(
            full_file_write_cmdlet("$lines | out-file -Encoding utf8 x.txt"),
            Some("Out-File")
        );
        assert_eq!(full_file_write_cmdlet("Get-Content main.rs"), None);
        assert_eq!(full_file_write_cmdlet("cargo test --workspace"), None);
        assert_eq!(full_file_write_cmdlet("echo reset-contentious"), None);
    }

    #[tokio::test]
    async fn set_content_command_is_blocked_before_spawn() {
        let out = BashTool
            .execute(
                serde_json::json!({"command": "Set-Content -Path x.rs -Value 'fn main(){}'"}),
                &ToolCtx {
                    cwd: std::env::temp_dir(),
                    project_root: std::env::temp_dir(),
                },
            )
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("edit"), "{}", out.content);
    }

    #[tokio::test]
    async fn timeout_kills_command_and_returns_explicit_error() {
        let command = match super::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 5",
            "cmd" => "ping 127.0.0.1 -n 6 > nul",
            _ => "sleep 5",
        };
        let started = std::time::Instant::now();
        let out = BashTool
            .execute(
                serde_json::json!({"command": command, "timeout_ms": 50}),
                &ToolCtx {
                    cwd: std::env::temp_dir(),
                    project_root: std::env::temp_dir(),
                },
            )
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("timeout: true"), "{}", out.content);
        assert!(started.elapsed() < std::time::Duration::from_secs(4));
    }

    #[test]
    fn description_explicitly_says_interactive_stdin_is_unavailable() {
        let description = BashTool.description();
        assert!(description.contains("stdin is closed"));
        assert!(description.contains("EOF"));
    }

    #[test]
    fn git_commit_detection() {
        assert!(looks_like_git_commit("git commit -m 'x'"));
        assert!(looks_like_git_commit("git add a.rs; git commit -m fix"));
        assert!(!looks_like_git_commit("git status"));
        assert!(!looks_like_git_commit("cargo test"));
    }

    #[test]
    fn resources_keep_the_complete_command_without_prefix_generalization() {
        let input = serde_json::json!({
            "command": "git status > .kanzei/project/requirements.md",
            "workdir": "subdir"
        });
        let resources = BashTool.resources(&input);
        assert_eq!(resources.len(), 1);
        let resource: serde_json::Value = serde_json::from_str(&resources[0]).unwrap();
        assert_eq!(resource["command"], "git status > .kanzei/project/requirements.md");
        assert_eq!(resource["workdir"], "subdir");
        let resource: serde_json::Value = serde_json::from_str(
            &BashTool.resources_with_ctx(
                &serde_json::json!({"command": "git status", "workdir": "subdir"}),
                &ToolCtx {
                    cwd: PathBuf::from("C:/project"),
                    project_root: PathBuf::from("C:/project"),
                },
            )[0],
        )
        .unwrap();
        assert_eq!(
            resource["workdir"],
            kanzei_harness::permission::normalize_resource("C:/project/subdir")
        );
        let resource: serde_json::Value = serde_json::from_str(
            &BashTool.resources_with_ctx(
                &serde_json::json!({"command": "git status"}),
                &ToolCtx {
                    cwd: PathBuf::from("C:/project"),
                    project_root: PathBuf::from("C:/project"),
                },
            )[0],
        )
        .unwrap();
        assert_eq!(
            resource["workdir"],
            kanzei_harness::permission::normalize_resource("C:/project")
        );
    }
}
