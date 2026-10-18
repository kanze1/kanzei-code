//! Disposable execution: explicit file copies, immutable runtime, no network or host writes.
use std::{path::Path, process::Stdio};

use async_trait::async_trait;
use kanzei_harness::{
    permission::Resource, ExternalSourceKind, Tool, ToolConcurrency, ToolCtx, ToolOutput,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

const HELPER: &str = include_str!("../../../scripts/sandbox-helper.py");
const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INPUT_FILES: usize = 128;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    /// POSIX shell command. Runs in an ephemeral Linux workspace, never on the host shell.
    command: String,
    /// Explicit regular project file contents to copy. Relative paths; directories are not inputs.
    #[serde(default)]
    files: Vec<String>,
    /// Execution deadline in milliseconds, 1..=120000. Includes namespace setup.
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

fn default_timeout() -> u64 {
    30_000
}

pub struct SandboxTool;

#[async_trait]
impl Tool for SandboxTool {
    fn name(&self) -> &'static str {
        "sandbox"
    }
    fn description(&self) -> String {
        "Run untrusted POSIX code in bubblewrap (Windows: default WSL2 distribution). Only explicit files are copied; /usr is read-only, networking is denied, writes use a bounded disposable tmpfs, and all descendants end with the command. Returns output, never publishes files to the project. Requires Python 3 and bubblewrap 0.9+; fails closed if unavailable. Use this for untrusted execution; bash is the host shell and is not isolated.".into()
    }
    fn input_schema(&self) -> Value {
        serde_json::to_value(schemars::schema_for!(Input)).unwrap()
    }
    fn permission_resources(&self, input: &Value, _: &ToolCtx) -> Vec<Resource> {
        let mut resources = vec![Resource::Literal(
            json!({"command":input.get("command"), "network":"denied", "host_writes":"none"})
                .to_string(),
        )];
        if let Some(files) = input.get("files").and_then(Value::as_array) {
            resources.extend(
                files
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|path| Resource::File(path.into())),
            );
        }
        resources
    }
    fn concurrency(&self, _: &Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::shared_worktree(ctx)
    }
    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput {
        self.execute_cancellable(input, ctx, None).await
    }
    async fn execute_cancellable(
        &self,
        value: Value,
        ctx: &ToolCtx,
        halt: Option<&CancellationToken>,
    ) -> ToolOutput {
        let input: Input = match crate::parse_input(self, value) {
            Ok(input) => input,
            Err(output) => return output,
        };
        if input.command.is_empty()
            || input.command.len() > 30_000
            || !(1..=120_000).contains(&input.timeout_ms)
            || input.files.len() > MAX_INPUT_FILES
        {
            return ToolOutput::needs_correction(
                "SANDBOX_INPUT_INVALID",
                "command: 1..30000 bytes; timeout_ms: 1..120000; at most 128 regular input files",
            );
        }
        match run(input, ctx, halt).await {
            Ok(result) => {
                let success = result.get("isolation_verified").and_then(Value::as_bool)
                    == Some(true)
                    && result.get("status").and_then(Value::as_str) == Some("completed")
                    && result.get("exit_code").and_then(Value::as_i64) == Some(0);
                let content = result.to_string();
                let output = if success {
                    ToolOutput::ok(content)
                } else {
                    ToolOutput::error(content)
                };
                output.with_external_source(ExternalSourceKind::Process, "sandbox:bubblewrap")
            }
            Err(output) => *output,
        }
    }
}

fn refused(error: impl std::fmt::Display) -> Box<ToolOutput> {
    Box::new(ToolOutput::error(format!(
        "SANDBOX_UNAVAILABLE: {error}. No host command was executed."
    )))
}

fn relative_file(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['\\', ':'])
        && Path::new(value)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

async fn run(
    input: Input,
    ctx: &ToolCtx,
    halt: Option<&CancellationToken>,
) -> Result<Value, Box<ToolOutput>> {
    let staging = tempfile::tempdir().map_err(refused)?;
    let mut bytes = 0;
    for requested in &input.files {
        if !relative_file(requested) {
            return Err(Box::new(ToolOutput::needs_correction(
                "SANDBOX_INPUT_PATH",
                "sandbox input paths must be normal relative files without parent components",
            )));
        }
        let scoped = crate::file_access::resolve(ctx, Path::new(requested), "sandbox")?;
        let mut file = scoped.open().map_err(refused)?;
        if !file.metadata().map_err(refused)?.is_file() {
            return Err(Box::new(ToolOutput::needs_correction(
                "SANDBOX_INPUT_PATH",
                "sandbox inputs must be regular files",
            )));
        }
        let mut content = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(&mut file, MAX_INPUT_BYTES - bytes + 1),
            &mut content,
        )
        .map_err(refused)?;
        bytes += content.len() as u64;
        if bytes > MAX_INPUT_BYTES {
            return Err(Box::new(ToolOutput::needs_correction(
                "SANDBOX_INPUT_SIZE",
                "sandbox input files exceed 16 MiB",
            )));
        }
        let target = kanzei_harness::permission::ScopedFile::resolve(
            staging.path(),
            staging.path(),
            Path::new(requested),
        )
        .map_err(refused)?;
        target.create_parent().map_err(refused)?;
        target.write_atomic(&content, || Ok(())).map_err(refused)?;
    }
    let inputs = linux_path(staging.path()).await?;
    let mut command = helper_command();
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(refused)?;
    let mut stdin = child.stdin.take().expect("helper stdin");
    let stdout = child.stdout.take().expect("helper stdout");
    let mut stderr = child.stderr.take().expect("helper stderr");
    let errors = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    stdin
        .write_all(
            format!(
                "{}\n",
                json!({"inputs":inputs,"command":input.command,"timeout_ms":input.timeout_ms})
            )
            .as_bytes(),
        )
        .await
        .map_err(refused)?;
    stdin.flush().await.map_err(refused)?;
    let mut lines = BufReader::new(stdout).lines();
    let mut cancelled = false;
    let mut result = None;
    loop {
        let line = match halt {
            Some(token) if !cancelled => tokio::select! {
                biased;
                _ = token.cancelled() => {
                    // Await the helper's result after cancellation, so namespace cleanup precedes completion.
                    stdin.write_all(b"cancel\n").await.map_err(refused)?;
                    stdin.flush().await.map_err(refused)?;
                    cancelled = true;
                    continue;
                }
                line = lines.next_line() => line,
            },
            _ => lines.next_line().await,
        }
        .map_err(refused)?;
        let Some(line) = line else { break };
        let event: Value = serde_json::from_str(&line).map_err(refused)?;
        match event.get("event").and_then(Value::as_str) {
            Some("started") if result.is_none() => {}
            Some("result") if result.is_none() => result = event.get("result").cloned(),
            _ => return Err(refused("invalid or duplicate sandbox event")),
        }
    }
    drop(stdin);
    let status = child.wait().await.map_err(refused)?;
    let stderr = errors.await.map_err(refused)?.map_err(refused)?;
    if !status.success() {
        return Err(refused(String::from_utf8_lossy(&stderr)));
    }
    result
        .filter(|result| result.get("schema").and_then(Value::as_u64) == Some(1))
        .ok_or_else(|| refused("missing sandbox execution receipt"))
}

fn helper_command() -> tokio::process::Command {
    #[cfg(windows)]
    let mut command = {
        let mut command = tokio::process::Command::new("wsl.exe");
        command.args(["--exec", "/usr/bin/python3"]);
        command.env_remove("WSLENV");
        command.creation_flags(0x08000000);
        command
    };
    #[cfg(not(windows))]
    let mut command = tokio::process::Command::new("/usr/bin/python3");
    command.args(["-I", "-u", "-c", HELPER]);
    command
}

async fn linux_path(path: &Path) -> Result<String, Box<ToolOutput>> {
    #[cfg(windows)]
    {
        let mut command = tokio::process::Command::new("wsl.exe");
        command.args(["--exec", "/usr/bin/wslpath", "-a"]);
        command.arg(path).creation_flags(0x08000000);
        let output = command.output().await.map_err(refused)?;
        if !output.status.success() {
            return Err(refused(String::from_utf8_lossy(&output.stderr)));
        }
        let path = String::from_utf8(output.stdout).map_err(refused)?;
        Ok(path.trim_end().to_owned())
    }
    #[cfg(not(windows))]
    {
        Ok(path.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sandbox_input_paths_cannot_escape_the_copy() {
        for path in ["", "../secret", "/etc/passwd", "C:/secret", "folder\\file"] {
            assert!(!relative_file(path), "{path}");
        }
        assert!(relative_file("src/main.py"));
    }
    #[tokio::test]
    async fn sandbox_cannot_copy_a_sensitive_file_without_exact_consent() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".env"), "SANDBOX_SECRET_CANARY").unwrap();
        let output = SandboxTool
            .execute(
                json!({"command":"cat .env","files":[".env"]}),
                &ToolCtx::new(root.path().into(), root.path().into()),
            )
            .await;
        assert_eq!(
            output.code.as_deref(),
            Some("SENSITIVE_FILE_APPROVAL_REQUIRED")
        );
        assert!(!output.content.contains("SANDBOX_SECRET_CANARY"));
    }
}
