//! Git 交付事务域(R-257 B4):fmt/clippy → 定向测试 → test_record → stage → CAS commit。
//! 事务顺序与门禁实现仍复用 git.rs 的 dev 当前实现，避免回退到旧分支语义。

use kanzei_harness::{ToolCtx, ToolOutput};

use super::{
    aggregate_gate_errors, build_commit_plan, clippy_gate, commit_with_gate_state, fmt_gate,
    run_git, source_endorsement_fingerprint, stage,
};

/// D-334:finalize 事务化——把「fmt → 相关测试 → test_record → stage → CAS commit」
/// 收敛为一次机械调用,Agent 不再手动驾驶 Harness 状态机。
pub(crate) async fn finalize(
    ctx: &ToolCtx,
    files: Vec<String>,
    message: Option<String>,
    requirement_id: Option<String>,
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
    let plan = match build_commit_plan(&ctx.project_root, cwd, &files).await {
        Ok(plan) => plan,
        Err(error) => return ToolOutput::error(format!("[finalize] commit_plan failed: {error}")),
    };
    if !plan.unsafe_files.is_empty() {
        return plan.blocker("finalize");
    }

    let has_rust_gate_inputs = files
        .iter()
        .any(|path| path.ends_with(".rs") || path.ends_with("Cargo.toml"));
    if has_rust_gate_inputs {
        let (fmt_result, clippy_result) = tokio::join!(fmt_gate(cwd), clippy_gate(cwd));
        if let Err(error) = aggregate_gate_errors(fmt_result, clippy_result, "finalize") {
            return ToolOutput::error(error);
        }
    }

    let test_command = plan
        .test_command
        .unwrap_or_else(|| "cargo test --workspace".to_string());

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
    let test_record = match crate::test_record::record_test_run_with_duration(
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
        Ok(snapshot) => snapshot,
        Err(error) => return ToolOutput::error(format!("[finalize] test_record failed: {error}")),
    };
    let test_record_ids = test_record
        .get("recorded_id")
        .and_then(serde_json::Value::as_str)
        .map(|id| vec![id.to_string()])
        .unwrap_or_default();

    let staged = stage(&ctx.project_root, cwd, &files, None).await;
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
    let deliver_note = match run_git(cwd, &["rev-parse", "HEAD"]).await {
        Ok(commit) => {
            let paths = run_git(
                cwd,
                &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
            )
            .await
            .unwrap_or_default()
            .lines()
            .filter(|path| !path.trim().is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
            let entry_id = requirement_id
                .or_else(|| crate::work::log::latest_claim_id(&ctx.project_root, ctx));
            if let Some(entry_id) = entry_id {
                crate::work::log::append(
                    &ctx.project_root,
                    "deliver",
                    Some(&entry_id),
                    ctx,
                    serde_json::json!({
                        "commit": commit.trim(),
                        "paths": paths,
                        "test_record_ids": test_record_ids,
                        "source": "engine",
                    }),
                );
                format!("\ndeliver recorded: {entry_id} @ {}", commit.trim())
            } else {
                "\nWarning: no bound claim found; commit remains legacy without a deliver record"
                    .to_string()
            }
        }
        Err(error) => format!("\nWarning: committed but deliver lookup failed: {error}"),
    };
    ToolOutput::ok(format!(
        "[finalize] complete: {test_command} passed in {duration_secs:.1}s → staged {staged_hash} → committed\n{content}\n{}{}",
        committed.content, deliver_note
    ))
    .with_display(display.unwrap_or_else(|| serde_json::json!({ "kind": "terminal" })))
}
