//! bash(shell)工具。设计红线 6:按实际检测到的 shell 动态生成描述,
//! 让模型知道自己面对的是 pwsh/cmd 还是 POSIX sh;超时返回结构化结果而非报错。

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
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
            "pwsh" | "powershell" => "PowerShell syntax (NOT POSIX: use ; or && (pwsh7), $env:VAR, Get-ChildItem)",
            "cmd" => "cmd.exe syntax (NOT POSIX: use %VAR%, dir, &&)",
            _ => "POSIX sh syntax",
        };
        format!("Run a shell command via {} — {syntax}. Params: command; optional timeout_ms, workdir.", shell.name)
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(BashInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["command"].as_str().unwrap_or("*").to_string()]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: BashInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let timeout = Duration::from_millis(input.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).min(MAX_TIMEOUT_MS));
        let workdir = match &input.workdir {
            Some(dir) => ctx.cwd.join(dir),
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

        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("failed to spawn {}: {e}", shell.name)),
        };
        let pid = child.id();

        let mut stdout = child.stdout.take().expect("stdout piped");
        let mut stderr = child.stderr.take().expect("stderr piped");
        let capture = async {
            // 有界读取:两条流各自最多 MAX_CAPTURE_BYTES,超出丢弃(内存红线)。
            let (mut out_buf, mut err_buf) = (Vec::new(), Vec::new());
            let (a, b) = tokio::join!(
                read_capped(&mut stdout, &mut out_buf),
                read_capped(&mut stderr, &mut err_buf)
            );
            let status = child.wait().await;
            (status, out_buf, a, err_buf, b)
        };

        match tokio::time::timeout(timeout, capture).await {
            Ok((status, out_buf, out_capped, err_buf, err_capped)) => {
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
                let text = if text.trim().is_empty() { "(no output)".to_string() } else { text };
                let rendered = format!("exit code: {}\n{text}", code.map_or("unknown".into(), |c| c.to_string()));
                let display = serde_json::json!({
                    "kind": "terminal",
                    "command": input.command,
                    "exitCode": code,
                    "output": text.chars().take(4000).collect::<String>(),
                });
                let output = if ok { ToolOutput::ok(rendered) } else { ToolOutput::error(rendered) };
                output.with_display(display)
            }
            Err(_) => {
                if let Some(pid) = pid {
                    kill_tree(pid).await;
                }
                // 超时是可预期结果:结构化告知,让模型加大 timeout 重试。
                ToolOutput::ok(format!(
                    "timeout: true — command did not finish within {} ms and was killed. Retry with a larger timeout_ms if needed.",
                    timeout.as_millis()
                ))
            }
        }
    }
}

async fn read_capped(reader: &mut (impl tokio::io::AsyncRead + Unpin), buffer: &mut Vec<u8>) -> bool {
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
