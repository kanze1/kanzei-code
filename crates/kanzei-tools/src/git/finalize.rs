//! Git 交付事务域(R-257 B4):fmt/clippy → 定向测试 → test_record → stage → CAS commit。
//! 事务顺序与门禁实现仍复用 git.rs 的 dev 当前实现，避免回退到旧分支语义。

use kanzei_harness::{ToolCtx, ToolOutput};

use super::{
    aggregate_gate_errors, clippy_gate, commit_with_gate_state, fmt_gate, source_crates,
    source_endorsement_fingerprint, stage,
};

/// D-334:finalize 事务化——把「fmt → 相关测试 → test_record → stage → CAS commit」
/// 收敛为一次机械调用,Agent 不再手动驾驶 Harness 状态机。
pub(crate) async fn finalize(
    ctx: &ToolCtx,
    files: Vec<String>,
    message: Option<String>,
) -> ToolOutput {
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

    if !sources.is_empty() {
        let (fmt_result, clippy_result) = tokio::join!(fmt_gate(cwd), clippy_gate(cwd));
        if let Err(error) = aggregate_gate_errors(fmt_result, clippy_result, "finalize") {
            return ToolOutput::error(error);
        }
    }

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
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                return ToolOutput::error(format!(
                    "[finalize] failed to run `{test_command}`: {error}"
                ))
            }
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

    let fingerprint = source_endorsement_fingerprint(cwd).unwrap_or_default();
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

    let committed =
        commit_with_gate_state(ctx, Some(message), Some(staged_hash.clone()), true).await;
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
