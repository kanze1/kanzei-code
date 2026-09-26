//! Execute and record a test in the same tool call; preserve the pre-run source identity.
use std::time::Instant;

use kanzei_harness::{ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
pub(crate) struct TestExecution {
    /// Short test name; omitted uses the command.
    #[serde(default)]
    title: Option<String>,
    /// Requirement/defect IDs covered by this test, e.g. ["R-001"].
    #[serde(default)]
    refs: Vec<String>,
}

pub(crate) struct TestExecutionGuard {
    ctx: ToolCtx,
    id: String,
    title: String,
    command: String,
    workdir: std::path::PathBuf,
    recorded_command: String,
    refs: Vec<String>,
    fingerprint: String,
    started: Instant,
    finished: bool,
}

impl TestExecutionGuard {
    pub(crate) fn start(
        ctx: &ToolCtx,
        command: &str,
        workdir: Option<&str>,
        input: &TestExecution,
    ) -> Result<Self, String> {
        let title = input.title.as_deref().unwrap_or(command).trim();
        if title.is_empty() || input.refs.iter().any(|r| r.contains(['\r', '\n'])) {
            return Err("test.title must be nonempty and test.refs must be single-line IDs".into());
        }
        let title = title
            .lines()
            .next()
            .unwrap_or("test")
            .chars()
            .take(160)
            .collect::<String>();
        // Keep the Markdown ledger one line; the artifact retains the exact command.
        let recorded_command = command.replace('\r', "\\r").replace('\n', "\\n");
        let fingerprint = crate::git::source_endorsement_fingerprint(&ctx.cwd).unwrap_or_default();
        let snapshot = super::record_test_run_with_duration(
            &ctx.project_root,
            None,
            &title,
            "running",
            Some(&recorded_command),
            Some("由 bash 执行并自动记录，尚未取得结果"),
            Some(&input.refs),
            None,
            None,
        )?;
        let id = snapshot["recorded_id"]
            .as_str()
            .ok_or("test record id missing")?
            .to_string();
        let guard = Self {
            ctx: ctx.clone(),
            id,
            title,
            command: command.into(),
            recorded_command,
            workdir: workdir
                .map(|dir| {
                    ctx.cwd
                        .join(kanzei_harness::permission::normalize_resource(dir))
                })
                .unwrap_or_else(|| ctx.cwd.clone()),
            refs: input.refs.clone(),
            fingerprint,
            started: Instant::now(),
            finished: false,
        };
        guard.note_writes();
        Ok(guard)
    }

    fn note_writes(&self) {
        for path in super::TEST_RUNS_GOVERNANCE_PATHS {
            crate::record_write_log(&self.ctx, path, &self.ctx.project_root.join(path));
        }
    }

    fn record_terminal(&self, status: &str, summary: &str) -> Result<(), String> {
        super::record_test_run_with_duration(
            &self.ctx.project_root,
            Some(&self.id),
            &self.title,
            status,
            Some(&self.recorded_command),
            Some(summary),
            Some(&self.refs),
            Some(self.started.elapsed().as_secs_f64()),
            Some(&self.fingerprint),
        )?;
        self.note_writes();
        Ok(())
    }

    pub(crate) fn finish(mut self, output: &mut ToolOutput) -> Result<(), String> {
        let exit_code = output.display.as_ref().and_then(|d| d["exitCode"].as_i64());
        let status = if !output.is_error && exit_code == Some(0) {
            "passed"
        } else {
            "failed"
        };
        let relative = format!(".kanzei/artifacts/test-runs/{}.json", self.id);
        let path = self.ctx.project_root.join(&relative);
        std::fs::create_dir_all(path.parent().ok_or("test artifact parent missing")?)
            .map_err(|error| error.to_string())?;
        let artifact = json!({
            "id": self.id, "command": self.command, "source_cwd": self.ctx.cwd,
            "workdir": self.workdir,
            "status": status, "exit_code": exit_code,
            "duration_secs": self.started.elapsed().as_secs_f64(),
            "source_fingerprint_before": self.fingerprint,
            "source_binding": if self.fingerprint.is_empty() {
                "no_source_fingerprint"
            } else {
                "existing_source_endorsement_rules"
            },
            "run_id": self.ctx.run_id, "refs": self.refs,
            "content": output.content, "display": output.display,
        });
        crate::atomic_file::write_atomic(&path, &artifact.to_string())
            .map_err(|error| error.to_string())?;
        self.record_terminal(
            status,
            &format!(
            "自动执行结果: {status}; exit={exit_code:?}; 日志: {relative}; 仅背书该命令的验证范围"
        ),
        )?;
        self.finished = true;
        output.content.push_str(&format!(
            "\n自动测试记录: {} [{status}]，日志: {relative}。无需再次调用 test_record。",
            self.id
        ));
        Ok(())
    }
}

impl Drop for TestExecutionGuard {
    fn drop(&mut self) {
        if !self.finished {
            // Future cancellation also closes the running record; it cannot become passed.
            if let Err(error) = self.record_terminal("failed", "执行或记录中断，未取得可用成功证据")
            {
                tracing::warn!(test_id = %self.id, %error, "cannot close interrupted test record");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::Tool;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn fixture() -> ToolCtx {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "kanzei-auto-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        ToolCtx::new(root.clone(), root)
    }

    #[tokio::test]
    async fn command_exit_records_pass_or_fail_without_manual_calls() {
        let ctx = fixture();
        for (code, status) in [(0, "passed"), (7, "failed")] {
            let output = crate::bash::BashTool
                .execute(
                    json!({
                        "command": format!("exit {code}"),
                        "test": {"title": format!("exit-{code}"), "refs": ["R-001"]}
                    }),
                    &ctx,
                )
                .await;
            assert_eq!(output.is_error, code != 0, "{}", output.content);
            assert!(
                output.content.contains(&format!("[{status}]")),
                "{}",
                output.content
            );
        }
        let records = super::super::read_test_records(
            &ctx.project_root.join(super::super::TEST_RUNS_ARCHIVE_REL),
        );
        assert_eq!(records.len(), 2);
        assert!(super::super::read_test_records(
            &ctx.project_root.join(super::super::TEST_RUNS_REL)
        )
        .is_empty());
        std::fs::remove_dir_all(ctx.project_root).unwrap();
    }

    #[tokio::test]
    async fn rejected_background_test_never_starts_or_records_success() {
        let ctx = fixture();
        let output = crate::bash::BashTool
            .execute(
                json!({
                    "command": "exit 0", "background": true, "test": {"title": "background"}
                }),
                &ctx,
            )
            .await;
        assert!(output.is_error);
        assert!(!ctx.project_root.join(super::super::TEST_RUNS_REL).exists());
        std::fs::remove_dir_all(ctx.project_root).unwrap();
    }

    #[test]
    fn interrupted_execution_closes_running_record_as_failed() {
        let ctx = fixture();
        let input: TestExecution = serde_json::from_value(json!({"title": "cancelled"})).unwrap();
        let guard = TestExecutionGuard::start(&ctx, "exit 0", None, &input).unwrap();
        let id = guard.id.clone();
        drop(guard);
        let records = super::super::read_test_records(
            &ctx.project_root.join(super::super::TEST_RUNS_ARCHIVE_REL),
        );
        assert_eq!(records[0].1["id"], id);
        assert_eq!(records[0].1["status"], "failed");
        std::fs::remove_dir_all(ctx.project_root).unwrap();
    }

    #[tokio::test]
    async fn timeout_closes_running_record_as_failed() {
        let ctx = fixture();
        let command = match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 30",
            "cmd" => "ping -n 31 127.0.0.1 > NUL",
            _ => "sleep 30",
        };
        let output = crate::bash::BashTool
            .execute(
                json!({
                    "command": command, "timeout_ms": 200, "test": {"title": "timeout"}
                }),
                &ctx,
            )
            .await;
        assert!(output.is_error, "{}", output.content);
        assert!(output.content.contains("[failed]"), "{}", output.content);
        assert!(super::super::read_test_records(
            &ctx.project_root.join(super::super::TEST_RUNS_REL)
        )
        .is_empty());
        std::fs::remove_dir_all(ctx.project_root).unwrap();
    }

    #[test]
    fn test_evidence_keeps_pre_execution_fingerprint_when_source_changes() {
        let ctx = fixture();
        let mut git = std::process::Command::new("git");
        crate::hide_console(&mut git);
        assert!(git
            .args(["init", "-q"])
            .current_dir(&ctx.cwd)
            .status()
            .unwrap()
            .success());
        std::fs::create_dir_all(ctx.cwd.join("crates")).unwrap();
        std::fs::write(ctx.cwd.join("crates/sample.rs"), "fn before() {}").unwrap();
        let before = crate::git::source_endorsement_fingerprint(&ctx.cwd).unwrap();
        assert!(!before.is_empty());
        let input: TestExecution =
            serde_json::from_value(json!({"title": "source identity"})).unwrap();
        let guard = TestExecutionGuard::start(&ctx, "exit 0", None, &input).unwrap();
        let id = guard.id.clone();
        std::fs::write(ctx.cwd.join("crates/sample.rs"), "fn after() {}").unwrap();
        let after = crate::git::source_endorsement_fingerprint(&ctx.cwd).unwrap();
        assert_ne!(before, after);
        let mut result = ToolOutput::ok("exit code: 0").with_display(json!({"exitCode": 0}));
        guard.finish(&mut result).unwrap();
        let artifact: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                ctx.cwd
                    .join(format!(".kanzei/artifacts/test-runs/{id}.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(artifact["source_fingerprint_before"], before);
        let archive =
            std::fs::read_to_string(ctx.cwd.join(super::super::TEST_RUNS_ARCHIVE_REL)).unwrap();
        assert!(archive.contains(&before));
        assert!(!archive.contains(&after));
        std::fs::remove_dir_all(ctx.project_root).unwrap();
    }
}
