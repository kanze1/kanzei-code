//! Research experiment runner(R-344 B3):专用 local/SSH 执行通道。
//!
//! 研究档禁止 bash；本工具是唯一能启动实验的受控入口。它把 callback 与普通
//! stdout 分开落库/落日志，实验目录与 state.db 都由同一条事实链更新。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use kanzei_core::{
    parse_callback_line, project_state_path, CallbackStats, ResearchRunRecord, SessionStore,
};
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::docstore::DocStore;
use crate::research_environment::{load_environment, ResearchEnvironment};

const DEFAULT_MAX_DURATION_MS: u64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Deserialize)]
struct RunnerInput {
    action: String,
    topic: String,
    environment_id: Option<String>,
    exploration_id: Option<String>,
    result_id: Option<String>,
    execution: Option<ExecutionSpec>,
    params_text: Option<String>,
    code_ref: Option<Value>,
    policy: Option<String>,
    lease_id: Option<String>,
    max_duration_ms: Option<u64>,
    cleanup: Option<String>,
    heartbeat_timeout_ms: Option<u64>,
    confirmed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecutionSpec {
    kind: String,
    command: String,
    host: Option<String>,
    user: Option<String>,
    workdir: Option<String>,
}

pub struct ResearchRunnerTool;

#[async_trait]
impl Tool for ResearchRunnerTool {
    fn name(&self) -> &'static str {
        "research_runner"
    }

    fn description(&self) -> String {
        "研究实验专用执行器：run 通过 local 或系统 ssh 客户端执行人工准备好的命令，逐行解析 @@kanzei callback，普通 stdout/stderr 原样写入终端日志；事件、callback_stats、终态、环境快照、参数和产物引用写入 state.db 与 explorations/<E-id>/<result-id>/；get 从持久事实回读。research 档 bash 仍不可用。".into()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["run", "get", "cancel"] },
                "topic": { "type": "string", "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$" },
                "environment_id": { "type": "string", "pattern": "^ENV-.+" },
                "exploration_id": { "type": "string" },
                "result_id": { "type": "string" },
                "execution": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["local", "ssh"] },
                        "command": { "type": "string" },
                        "host": { "type": "string" },
                        "user": { "type": "string" },
                        "workdir": { "type": "string" }
                    },
                    "required": ["kind", "command"]
                },
                "params_text": { "type": "string" },
                "code_ref": { "type": "object" },
                "policy": { "type": "string" },
                "lease_id": { "type": "string" },
                "max_duration_ms": { "type": "integer", "minimum": 1 },
                "heartbeat_timeout_ms": { "type": "integer", "minimum": 1 },
                "cleanup": { "type": "string" },
                "confirmed": { "type": "boolean" }
            },
            "required": ["action", "topic"]
        })
    }

    fn resources(&self, input: &Value) -> Vec<String> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        vec![format!(
            "{}:{action}",
            if action == "get" { "read" } else { "write" }
        )]
    }

    fn concurrency(&self, input: &Value, ctx: &ToolCtx) -> ToolConcurrency {
        if input.get("action").and_then(Value::as_str) == Some("cancel") {
            return ToolConcurrency::Shared(format!(
                "research-cancel:{}",
                input
                    .get("result_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ));
        }
        ToolConcurrency::WorktreeWrite(ctx.worktree_concurrency_key())
    }

    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput {
        let parsed: RunnerInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(error) => {
                return ToolOutput::needs_correction("INVALID_RUNNER_INPUT", error.to_string())
            }
        };
        if let Err(error) = DocStore::validate_topic(&parsed.topic) {
            return ToolOutput::needs_correction("INVALID_TOPIC", error.to_string());
        }
        match parsed.action.as_str() {
            "get" => get_run(&parsed, ctx),
            "run" => run_experiment(parsed, ctx).await,
            "cancel" => cancel_run(&parsed, ctx).await,
            other => ToolOutput::needs_correction(
                "UNKNOWN_ACTION",
                format!("research_runner 不支持 action `{other}`，可用 run/get/cancel"),
            ),
        }
    }
}

fn get_run(input: &RunnerInput, ctx: &ToolCtx) -> ToolOutput {
    let Some(result_id) = input.result_id.as_deref() else {
        return ToolOutput::needs_correction("MISSING_RESULT_ID", "get 必须提供 result_id");
    };
    let store = match SessionStore::open(&project_state_path(&ctx.project_root)) {
        Ok(store) => store,
        Err(error) => return ToolOutput::error(format!("打开 state.db 失败: {error}")),
    };
    match store.get_research_run(result_id) {
        Ok(Some(run)) => {
            let events = store
                .list_research_run_events(result_id, 0)
                .unwrap_or_default();
            ToolOutput::ok(json!({ "run": run, "events": events }).to_string())
        }
        Ok(None) => ToolOutput::error(format!("未找到 research result `{result_id}`")),
        Err(error) => ToolOutput::error(format!("读取 research run 失败: {error}")),
    }
}

async fn cancel_run(input: &RunnerInput, ctx: &ToolCtx) -> ToolOutput {
    let Some(result_id) = input.result_id.as_deref() else {
        return ToolOutput::needs_correction("MISSING_RESULT_ID", "cancel 必须提供 result_id");
    };
    let store = match SessionStore::open(&project_state_path(&ctx.project_root)) {
        Ok(store) => store,
        Err(error) => return ToolOutput::error(format!("打开 state.db 失败: {error}")),
    };
    let Some(mut run) = (match store.get_research_run(result_id) {
        Ok(run) => run,
        Err(error) => return ToolOutput::error(format!("读取 research run 失败: {error}")),
    }) else {
        return ToolOutput::needs_correction(
            "UNKNOWN_RESULT_ID",
            format!("未找到 research result `{result_id}`"),
        );
    };
    if run.status != "running" {
        return ToolOutput::noop(
            "RUN_ALREADY_TERMINAL",
            format!("research result `{result_id}` 已是终态 `{}`", run.status),
        );
    }
    let pid = serde_json::from_str::<Value>(&run.execution_json)
        .ok()
        .and_then(|value| value.get("pid").and_then(Value::as_u64))
        .map(|value| value as u32);
    let Some(pid) = pid else {
        run.status = "cancelled".into();
        run.finished_at = Some(unix_ms());
        run.cancel_reason = Some("user_requested_before_spawn".into());
        let _ = append_event(
            &store,
            result_id,
            "run_cancelled",
            json!({ "reason": "user_requested_before_spawn" }),
        );
        if let Err(error) = store.upsert_research_run(&run) {
            return ToolOutput::error(format!("写入取消终态失败: {error}"));
        }
        let _ = store.release_research_environment_lease_for_result(result_id);
        return ToolOutput::ok(json!({ "result_id": result_id, "status": run.status }).to_string());
    };
    let killed = crate::shell::kill_tree(pid).await;
    if !killed && crate::shell::process_alive(pid) {
        return ToolOutput::failed(
            "RUN_CANCEL_FAILED",
            format!("无法终止 research result `{result_id}` 的进程树 pid={pid}"),
        );
    }
    run.status = "cancelled".into();
    run.finished_at = Some(unix_ms());
    run.cancel_reason = Some("user_requested".into());
    let _ = append_event(
        &store,
        result_id,
        "run_cancelled",
        json!({ "pid": pid, "reason": "user_requested" }),
    );
    if let Err(error) = store.upsert_research_run(&run) {
        return ToolOutput::error(format!("写入取消终态失败: {error}"));
    }
    let _ = store.release_research_environment_lease_for_result(result_id);
    ToolOutput::ok(json!({ "result_id": result_id, "status": run.status, "pid": pid }).to_string())
}

async fn run_experiment(input: RunnerInput, ctx: &ToolCtx) -> ToolOutput {
    let Some(mut execution) = input.execution.clone() else {
        return ToolOutput::needs_correction(
            "MISSING_EXECUTION",
            "run 必须提供 execution.kind 与 execution.command",
        );
    };
    let declared_environment = match input.environment_id.as_deref() {
        Some(environment_id) => match load_environment(&ctx.project_root, environment_id) {
            Ok(environment) if environment.status == "active" => Some(environment),
            Ok(environment) => {
                return ToolOutput::needs_correction(
                    "ENVIRONMENT_INACTIVE",
                    format!(
                        "环境 `{}` 当前状态为 `{}`，只能运行 active 环境",
                        environment.id, environment.status
                    ),
                )
            }
            Err(error) => return ToolOutput::needs_correction("INVALID_ENVIRONMENT", error),
        },
        None => None,
    };
    if execution.command.trim().is_empty() {
        return ToolOutput::needs_correction("EMPTY_COMMAND", "execution.command 不能为空");
    }
    if execution.kind != "local" && execution.kind != "ssh" {
        return ToolOutput::needs_correction(
            "INVALID_EXECUTION_KIND",
            "execution.kind 只能是 local 或 ssh",
        );
    }
    if execution.kind == "ssh"
        && execution.host.as_deref().unwrap_or("").trim().is_empty()
        && declared_environment.is_none()
    {
        return ToolOutput::needs_correction("MISSING_SSH_HOST", "ssh 执行必须提供 execution.host");
    }
    if let Some(environment) = declared_environment.as_ref() {
        if execution.kind != environment.kind {
            return ToolOutput::needs_correction(
                "ENVIRONMENT_KIND_MISMATCH",
                format!(
                    "execution.kind `{}` 与登记环境 `{}` 的 kind `{}` 不一致",
                    execution.kind, environment.id, environment.kind
                ),
            );
        }
        if execution.host.is_none() && !environment.host.is_empty() {
            execution.host = Some(environment.host.clone());
        }
        if execution.workdir.is_none() {
            execution.workdir = Some(environment.workdir.clone());
        }
    }
    let effective_policy = declared_environment
        .as_ref()
        .map(|environment| environment.policy.as_str())
        .or(input.policy.as_deref())
        .unwrap_or("relaxed");
    if !matches!(
        effective_policy,
        "relaxed" | "managed" | "approval" | "strict"
    ) {
        return ToolOutput::needs_correction(
            "INVALID_EXECUTION_POLICY",
            format!("执行策略 `{effective_policy}` 只能是 relaxed/managed/approval/strict"),
        );
    }

    let exploration_id = input
        .exploration_id
        .clone()
        .unwrap_or_else(|| "E-unknown".into());
    let result_id = input
        .result_id
        .clone()
        .unwrap_or_else(|| format!("run-{}", unix_ms()));
    if matches!(effective_policy, "approval" | "strict") && input.confirmed != Some(true) {
        return ToolOutput::needs_confirmation(
            "RESEARCH_RUN_CONFIRMATION_REQUIRED",
            json!({
                "result_id": result_id,
                "policy": effective_policy,
                "estimated_duration_ms": input.max_duration_ms.unwrap_or(DEFAULT_MAX_DURATION_MS),
                "billing": declared_environment
                    .as_ref()
                    .map(|environment| environment.billing.as_str())
                    .unwrap_or("unspecified"),
                "message": "该 research 环境需要显式确认后才会启动"
            })
            .to_string(),
        );
    }
    let output_dir = ctx
        .project_root
        .join(".kanzei/research")
        .join(&input.topic)
        .join("explorations")
        .join(&exploration_id)
        .join(&result_id);
    if let Err(error) = std::fs::create_dir_all(&output_dir) {
        return ToolOutput::error(format!("创建实验产物目录失败: {error}"));
    }
    let terminal_log_path = output_dir.join("terminal.log");
    let environment_path = output_dir.join("environment.json");
    let mut execution_json = serde_json::to_value(&execution).unwrap_or_else(|_| json!({}));
    if let Some(environment_id) = input.environment_id.as_deref() {
        execution_json["environment_id"] = json!(environment_id);
    }
    let mut run = ResearchRunRecord {
        result_id: result_id.clone(),
        exploration_id,
        topic: input.topic.clone(),
        status: "running".into(),
        execution_json: execution_json.to_string(),
        policy: effective_policy.to_owned(),
        lease_id: input.lease_id.unwrap_or_default(),
        max_duration_ms: input.max_duration_ms.unwrap_or(DEFAULT_MAX_DURATION_MS) as i64,
        cleanup: input.cleanup.unwrap_or_else(|| "retain".into()),
        started_at: unix_ms(),
        finished_at: None,
        exit_code: None,
        cancel_reason: None,
        params_text: input.params_text.unwrap_or_default(),
        code_ref_json: serde_json::to_string(&input.code_ref.unwrap_or_else(|| json!({})))
            .unwrap_or_default(),
        environment_snapshot_ref: relative_ref(&ctx.project_root, &environment_path),
        artifacts_json: "[]".into(),
        metrics_last_json: "{}".into(),
        callback_stats_json: serde_json::to_string(&CallbackStats::default()).unwrap_or_default(),
        heartbeat_at: None,
        terminal_log_path: relative_ref(&ctx.project_root, &terminal_log_path),
    };
    let store = match SessionStore::open(&project_state_path(&ctx.project_root)) {
        Ok(store) => store,
        Err(error) => return ToolOutput::error(format!("打开 state.db 失败: {error}")),
    };
    if matches!(effective_policy, "managed" | "approval") {
        let Some(environment) = declared_environment.as_ref() else {
            return ToolOutput::needs_correction(
                "MISSING_MANAGED_ENVIRONMENT",
                "managed/approval 策略必须通过 environment_id 指定登记环境",
            );
        };
        if let Err(error) = store.acquire_research_environment_lease(
            &environment.id,
            &result_id,
            effective_policy,
            run.max_duration_ms,
        ) {
            return ToolOutput::failed("ENVIRONMENT_LEASE_CONFLICT", error.to_string());
        }
    }
    if let Err(error) = store.upsert_research_run(&run) {
        let _ = store.release_research_environment_lease_for_result(&result_id);
        return ToolOutput::error(format!("写入 run_started 事实失败: {error}"));
    }
    let _ = append_event(
        &store,
        &result_id,
        "run_started",
        json!({ "execution": execution }),
    );
    let environment = capture_environment(&execution, ctx, declared_environment.as_ref()).await;
    if let Err(error) = write_json(&environment_path, &environment) {
        return finish_failed(
            store,
            run,
            format!("写入 environment.json 失败: {error}"),
            "run_failed",
        );
    }
    let _ = append_event(
        &store,
        &result_id,
        "environment_captured",
        environment.clone(),
    );

    if let Ok(Some(current)) = store.get_research_run(&result_id) {
        if current.status == "cancelled" {
            let _ = store.release_research_environment_lease_for_result(&result_id);
            return ToolOutput::ok(
                json!({ "result_id": result_id, "status": "cancelled" }).to_string(),
            );
        }
    }

    let mut command = match command_for(&execution, ctx) {
        Ok(command) => command,
        Err(error) => return finish_failed(store, run, error, "run_failed"),
    };
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return finish_failed(
                store,
                run,
                format!("启动实验进程失败: {error}"),
                "run_failed",
            )
        }
    };
    let mut execution_json =
        serde_json::from_str::<Value>(&run.execution_json).unwrap_or_else(|_| json!({}));
    if let Some(pid) = child.id() {
        execution_json["pid"] = json!(pid);
        run.execution_json = execution_json.to_string();
        let _ = store.upsert_research_run(&run);
    }
    let stdout = child.stdout.take().map(BufReader::new);
    let stderr = child.stderr.take().map(BufReader::new);
    let mut stdout = stdout.map(|reader| reader.lines());
    let mut stderr = stderr.map(|reader| reader.lines());
    let mut terminal = match std::fs::File::create(&terminal_log_path) {
        Ok(file) => file,
        Err(error) => {
            return finish_failed(
                store,
                run,
                format!("创建 terminal.log 失败: {error}"),
                "run_failed",
            )
        }
    };
    let max_duration = std::time::Duration::from_millis(run.max_duration_ms.max(1) as u64);
    let heartbeat_duration = std::time::Duration::from_millis(
        input.heartbeat_timeout_ms.unwrap_or(10 * 60 * 1000).max(1),
    );
    let timeout = tokio::time::sleep(max_duration);
    let heartbeat_timeout = tokio::time::sleep(heartbeat_duration);
    tokio::pin!(timeout);
    tokio::pin!(heartbeat_timeout);
    let mut stdout_done = stdout.is_none();
    let mut stderr_done = stderr.is_none();
    let mut exit_code = None;
    let mut timed_out = false;
    let mut timeout_reason: Option<&'static str> = None;
    while !(stdout_done && stderr_done) {
        tokio::select! {
            line = async {
                match stdout.as_mut() {
                    Some(lines) => lines.next_line().await,
                    None => Ok(None),
                }
            }, if !stdout_done => {
                match line {
                    Ok(Some(line)) => {
                        if handle_line(&store, &mut run, &mut terminal, &result_id, &line, false, &output_dir) {
                            heartbeat_timeout.as_mut().reset(tokio::time::Instant::now() + heartbeat_duration);
                        }
                    }
                    _ => stdout_done = true,
                }
            }
            line = async {
                match stderr.as_mut() {
                    Some(lines) => lines.next_line().await,
                    None => Ok(None),
                }
            }, if !stderr_done => {
                match line {
                    Ok(Some(line)) => {
                        if handle_line(&store, &mut run, &mut terminal, &result_id, &line, true, &output_dir) {
                            heartbeat_timeout.as_mut().reset(tokio::time::Instant::now() + heartbeat_duration);
                        }
                    }
                    _ => stderr_done = true,
                }
            }
            status = child.wait() => {
                exit_code = status.ok().and_then(|status| status.code()).map(i64::from);
                break;
            }
            _ = &mut timeout => {
                timed_out = true;
                timeout_reason = Some("max_duration");
                let _ = child.kill().await;
                exit_code = child.wait().await.ok().and_then(|status| status.code()).map(i64::from);
                break;
            }
            _ = &mut heartbeat_timeout => {
                timed_out = true;
                timeout_reason = Some("heartbeat_timeout");
                let _ = child.kill().await;
                exit_code = child.wait().await.ok().and_then(|status| status.code()).map(i64::from);
                break;
            }
        }
    }
    // wait 先完成时仍排空两个 pipe，避免快速实验退出前已写出的 callback 丢失。
    if exit_code.is_some() {
        if let Some(lines) = stdout.as_mut() {
            while let Ok(Some(line)) = lines.next_line().await {
                handle_line(
                    &store,
                    &mut run,
                    &mut terminal,
                    &result_id,
                    &line,
                    false,
                    &output_dir,
                );
            }
        }
        if let Some(lines) = stderr.as_mut() {
            while let Ok(Some(line)) = lines.next_line().await {
                handle_line(
                    &store,
                    &mut run,
                    &mut terminal,
                    &result_id,
                    &line,
                    true,
                    &output_dir,
                );
            }
        }
    }
    if exit_code.is_none() {
        exit_code = child
            .wait()
            .await
            .ok()
            .and_then(|status| status.code())
            .map(i64::from);
    }
    let finished_at = unix_ms();
    let cancelled = store
        .get_research_run(&result_id)
        .ok()
        .flatten()
        .is_some_and(|current| current.status == "cancelled");
    run.finished_at = Some(finished_at);
    run.exit_code = exit_code;
    run.status = if cancelled {
        run.cancel_reason = Some("user_requested".into());
        "cancelled"
    } else if timed_out {
        run.cancel_reason = timeout_reason.map(str::to_string);
        "stuck"
    } else if exit_code == Some(0) {
        "succeeded"
    } else {
        "failed"
    }
    .into();
    let event_type = if cancelled {
        "run_cancelled"
    } else if timed_out {
        "run_failed"
    } else if run.status == "succeeded" {
        "run_finished"
    } else {
        "run_failed"
    };
    if !cancelled {
        let _ = append_event(
            &store,
            &result_id,
            event_type,
            json!({ "exit_code": exit_code, "status": run.status, "timed_out": timed_out, "reason": run.cancel_reason }),
        );
    }
    let _ = store.release_research_environment_lease_for_result(&result_id);
    let _ = store.upsert_research_run(&run);
    let result_table = append_result_table_row(&ctx.project_root, &run);
    let summary = json!({ "result_id": result_id, "status": run.status, "exit_code": exit_code, "terminal_log": run.terminal_log_path, "environment": run.environment_snapshot_ref, "callback_stats": run.callback_stats_json, "result_table": result_table });
    if run.status == "succeeded" && result_table.is_ok() {
        ToolOutput::ok(summary.to_string())
    } else {
        ToolOutput::error(summary.to_string())
    }
}

async fn run_environment_probe(mut command: Command) -> Value {
    match command.output().await {
        Ok(output) => json!({
            "ok": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "exit_code": output.status.code(),
        }),
        Err(error) => json!({ "ok": false, "stdout": "", "stderr": error.to_string() }),
    }
}

async fn capture_environment(
    execution: &ExecutionSpec,
    ctx: &ToolCtx,
    declared: Option<&ResearchEnvironment>,
) -> Value {
    let observed = if execution.kind == "ssh" {
        let mut command = Command::new("ssh");
        let target = match execution.user.as_deref() {
            Some(user) if !user.trim().is_empty() => {
                format!("{user}@{}", execution.host.as_deref().unwrap_or_default())
            }
            _ => execution.host.clone().unwrap_or_default(),
        };
        command.args([
            "-o",
            "BatchMode=yes",
            &target,
            "nvidia-smi; python --version; git rev-parse HEAD; git status --porcelain",
        ]);
        run_environment_probe(command).await
    } else {
        let workdir = execution
            .workdir
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());
        let mut probes = serde_json::Map::new();
        for (name, program, args) in [
            ("nvidia_smi", "nvidia-smi", Vec::<&str>::new()),
            ("python_version", "python", vec!["--version"]),
            ("git_commit", "git", vec!["rev-parse", "HEAD"]),
            ("git_status", "git", vec!["status", "--porcelain"]),
        ] {
            let mut command = Command::new(program);
            command.args(args).current_dir(&workdir);
            probes.insert(name.into(), run_environment_probe(command).await);
        }
        Value::Object(probes)
    };
    let mut drift = Vec::new();
    if let Some(declared) = declared {
        if declared.kind != execution.kind {
            drift.push("kind");
        }
        if declared.host != execution.host.clone().unwrap_or_default() && declared.kind == "ssh" {
            drift.push("host");
        }
        if declared.workdir != execution.workdir.clone().unwrap_or_default() {
            drift.push("workdir");
        }
    }
    let degraded = if execution.kind == "ssh" {
        observed.get("ok") != Some(&Value::Bool(true))
    } else {
        observed.as_object().is_some_and(|probes| {
            probes
                .values()
                .any(|probe| probe.get("ok") != Some(&Value::Bool(true)))
        })
    };
    json!({
        "captured_at": unix_ms(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "local_workdir": ctx.cwd,
        "execution_kind": execution.kind,
        "remote_host": execution.host,
        "remote_workdir": execution.workdir,
        "declared": declared,
        "observed": observed,
        "degraded": degraded,
        "drift": drift,
    })
}

fn append_result_table_row(root: &Path, run: &ResearchRunRecord) -> Result<String, String> {
    if !run
        .result_id
        .starts_with(&format!("{}-", run.exploration_id))
    {
        return Err(format!(
            "result_id `{}` 不匹配 exploration_id `{}`",
            run.result_id, run.exploration_id
        ));
    }
    let path = root
        .join(".kanzei/research")
        .join(&run.topic)
        .join("explorations")
        .join(format!("{}.md", run.exploration_id));
    let mut text = std::fs::read_to_string(&path)
        .map_err(|error| format!("读取探索 Markdown {} 失败: {error}", path.display()))?;
    let section_start = text
        .find("## 实验结果")
        .ok_or_else(|| "探索 Markdown 缺少 `## 实验结果` 段".to_string())?;
    let section_end = text[section_start..]
        .find("\n## ")
        .map(|offset| section_start + offset)
        .unwrap_or(text.len());
    let params = run.params_text.replace('|', "\\|").replace('\n', " ");
    let metrics = run.metrics_last_json.replace('|', "\\|").replace('\n', " ");
    let artifacts: Vec<String> = serde_json::from_str(&run.artifacts_json).unwrap_or_default();
    let artifact_text = artifacts.join(", ").replace('|', "\\|");
    let row = format!(
        "| {} | {} | {} | {} | {} | {} |\n",
        run.result_id,
        params,
        run.status,
        metrics,
        artifact_text,
        run.cancel_reason.clone().unwrap_or_default()
    );
    let insert_at = section_end;
    if !text[..insert_at].ends_with('\n') {
        text.insert(insert_at, '\n');
    }
    let insert_at = if text[..insert_at].ends_with('\n') {
        insert_at
    } else {
        insert_at + 1
    };
    text.insert_str(insert_at, &row);
    std::fs::write(&path, text).map_err(|error| format!("写回探索结果表失败: {error}"))?;
    Ok(path.to_string_lossy().to_string())
}

fn command_for(execution: &ExecutionSpec, ctx: &ToolCtx) -> Result<Command, String> {
    if execution.kind == "ssh" {
        let target = match execution.user.as_deref() {
            Some(user) if !user.trim().is_empty() => {
                format!("{user}@{}", execution.host.as_deref().unwrap_or_default())
            }
            _ => execution.host.clone().unwrap_or_default(),
        };
        let mut command = Command::new("ssh");
        command.args(["-o", "BatchMode=yes", &target, &execution.command]);
        return Ok(command);
    }
    let workdir = execution
        .workdir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.cwd.clone());
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/D", "/C", &execution.command]);
        command
    } else {
        let mut command = Command::new("sh");
        command.args(["-c", &execution.command]);
        command
    };
    command.current_dir(workdir);
    Ok(command)
}

fn handle_line(
    store: &SessionStore,
    run: &mut ResearchRunRecord,
    terminal: &mut std::fs::File,
    result_id: &str,
    line: &str,
    stderr: bool,
    output_dir: &Path,
) -> bool {
    use std::io::Write;
    let parsed = parse_callback_line(line);
    let mut stats: CallbackStats =
        serde_json::from_str(&run.callback_stats_json).unwrap_or_default();
    parsed.apply_stats(&mut stats);
    run.callback_stats_json = serde_json::to_string(&stats).unwrap_or_default();
    if let Some(log) = parsed.terminal_log.as_deref() {
        let _ = writeln!(terminal, "{}{}", if stderr { "[stderr] " } else { "" }, log);
    }
    let mut heartbeat = false;
    if let Some(event) = parsed.event {
        let _ = append_event(store, result_id, &event.event_type, event.payload.clone());
        if event.event_type == "heartbeat" {
            heartbeat = true;
            run.heartbeat_at = Some(unix_ms());
        }
        if event.event_type == "metric" {
            run.metrics_last_json = event.payload.to_string();
        }
        if event.event_type == "artifact" {
            if let Some(path) = copy_artifact(output_dir, &event.payload) {
                let mut artifacts: Vec<String> =
                    serde_json::from_str(&run.artifacts_json).unwrap_or_default();
                artifacts.push(path);
                run.artifacts_json = serde_json::to_string(&artifacts).unwrap_or_default();
            }
        }
    }
    let _ = store.upsert_research_run(run);
    heartbeat
}

fn copy_artifact(output_dir: &Path, payload: &Value) -> Option<String> {
    let path = payload.get("path").and_then(Value::as_str)?;
    let source = Path::new(path);
    let name = source.file_name()?;
    let target = output_dir.join("artifacts").join(name);
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::copy(source, &target).ok()?;
    Some(format!("artifacts/{}", name.to_string_lossy()))
}

fn append_event(
    store: &SessionStore,
    result_id: &str,
    event_type: &str,
    payload: Value,
) -> Result<(), String> {
    store
        .append_research_run_event(result_id, event_type, &payload.to_string())
        .map(|_| ())
        .map_err(|error| error.to_string())
}
fn finish_failed(
    store: SessionStore,
    mut run: ResearchRunRecord,
    error: String,
    event_type: &str,
) -> ToolOutput {
    run.status = "failed".into();
    run.finished_at = Some(unix_ms());
    let _ = append_event(
        &store,
        &run.result_id,
        event_type,
        json!({ "error": error }),
    );
    let _ = store.release_research_environment_lease_for_result(&run.result_id);
    let _ = store.upsert_research_run(&run);
    ToolOutput::error(error)
}
fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn relative_ref(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::ToolOutcome;

    #[test]
    fn local_command_uses_explicit_workdir_and_ssh_uses_batch_mode() {
        let ctx = ToolCtx::new(PathBuf::from("C:/work"), PathBuf::from("C:/project"));
        let local = command_for(
            &ExecutionSpec {
                kind: "local".into(),
                command: "echo ok".into(),
                host: None,
                user: None,
                workdir: None,
            },
            &ctx,
        )
        .unwrap();
        assert_eq!(local.as_std().get_current_dir(), Some(Path::new("C:/work")));
        let remote = command_for(
            &ExecutionSpec {
                kind: "ssh".into(),
                command: "python train.py".into(),
                host: Some("gpu.example".into()),
                user: Some("alice".into()),
                workdir: None,
            },
            &ctx,
        )
        .unwrap();
        assert_eq!(remote.as_std().get_program(), "ssh");
        assert!(remote.as_std().get_args().any(|arg| arg == "BatchMode=yes"));
    }

    #[tokio::test]
    async fn local_run_persists_terminal_fact_and_environment_snapshot() {
        let root = std::env::temp_dir().join(format!("kz-research-runner-{}", unix_ms()));
        std::fs::create_dir_all(root.join(".kanzei/research/nas-search/explorations")).unwrap();
        std::fs::write(
            root.join(".kanzei/research/nas-search/explorations/E-001.md"),
            "---\nkind: exploration\nid: E-001\ntopic: nas-search\ntitle: test\nstatus: running\nhypothesis: test\ndepends_on:\nsupersedes:\nentry_refs:\nenvironment: ENV-test\nbudget: 1\ncreated_at: 1\nupdated_at: 1\n---\n\n## 假设\ntest\n\n## 实验结果\n| 实验 | 参数 | 状态 | 关键指标 | 产物 | 结论 |\n| --- | --- | --- | --- | --- | --- |\n\n## 结论\n待定\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".kanzei/research/environments.md"),
            format!(
                "## ENV-test [active]\n- kind: local\n- host: \n- 归属: test\n- 执行策略: relaxed\n- gpu: unknown\n- workdir: {}\n- 运行时限: 10m\n- 计费: none\n- 凭据引用: \n- 准备步骤: local test environment is already prepared\n",
                root.display()
            ),
        )
        .unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let output = run_experiment(
            RunnerInput {
                action: "run".into(),
                topic: "nas-search".into(),
                environment_id: Some("ENV-test".into()),
                exploration_id: Some("E-001".into()),
                result_id: Some("E-001-01".into()),
                execution: Some(ExecutionSpec {
                    kind: "local".into(),
                    command: "echo ordinary".into(),
                    host: None,
                    user: None,
                    workdir: None,
                }),
                params_text: Some("seed=3".into()),
                code_ref: None,
                policy: Some("relaxed".into()),
                lease_id: None,
                max_duration_ms: Some(10_000),
                cleanup: Some("retain".into()),
                heartbeat_timeout_ms: Some(10_000),
                confirmed: None,
            },
            &ctx,
        )
        .await;
        assert!(!output.is_error, "{}", output.content);
        assert!(output.content.contains("succeeded"), "{}", output.content);
        let store = SessionStore::open(&project_state_path(&root)).unwrap();
        let run = store.get_research_run("E-001-01").unwrap().unwrap();
        assert_eq!(run.status, "succeeded");
        assert_eq!(run.params_text, "seed=3");
        assert!(store.list_research_run_events("E-001-01", 0).unwrap().len() >= 2);
        assert!(root.join(&run.terminal_log_path).is_file());
        assert!(root.join(&run.environment_snapshot_ref).is_file());
        let snapshot: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(&run.environment_snapshot_ref)).unwrap(),
        )
        .unwrap();
        assert_eq!(snapshot["declared"]["id"], "ENV-test");
        assert!(snapshot.get("observed").is_some());
        assert!(snapshot.get("degraded").is_some());
        assert!(snapshot.get("drift").is_some());
        let execution: Value = serde_json::from_str(&run.execution_json).unwrap();
        assert_eq!(execution["environment_id"], "ENV-test");
        assert_eq!(run.policy, "relaxed");
        assert!(std::fs::read_to_string(root.join(&run.terminal_log_path))
            .unwrap()
            .contains("ordinary"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn approval_environment_requires_confirmation_before_spawn() {
        let root = std::env::temp_dir().join(format!("kz-research-approval-{}", unix_ms()));
        std::fs::create_dir_all(root.join(".kanzei/research")).unwrap();
        std::fs::write(
            root.join(".kanzei/research/environments.md"),
            format!(
                "## ENV-approval [active]\n- kind: local\n- host: \n- 归属: test\n- 执行策略: approval\n- gpu: unknown\n- workdir: {}\n- 运行时限: 10m\n- 计费: approval-budget\n- 凭据引用: secret://ref\n- 准备步骤: pre-provisioned\n",
                root.display()
            ),
        )
        .unwrap();
        let output = run_experiment(
            RunnerInput {
                action: "run".into(),
                topic: "approval-test".into(),
                environment_id: Some("ENV-approval".into()),
                exploration_id: Some("E-approval".into()),
                result_id: Some("E-approval-01".into()),
                execution: Some(ExecutionSpec {
                    kind: "local".into(),
                    command: "echo must-not-spawn".into(),
                    host: None,
                    user: None,
                    workdir: None,
                }),
                params_text: None,
                code_ref: None,
                policy: None,
                lease_id: None,
                max_duration_ms: Some(12_000),
                cleanup: None,
                heartbeat_timeout_ms: None,
                confirmed: None,
            },
            &ToolCtx::new(root.clone(), root.clone()),
        )
        .await;
        assert_eq!(output.outcome, ToolOutcome::NeedsConfirmation);
        assert_eq!(output.code, Some("RESEARCH_RUN_CONFIRMATION_REQUIRED"));
        assert!(output.content.contains("approval-budget"));
        assert!(!root.join(".kanzei/state.db").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn managed_environment_conflict_rejects_and_completed_run_releases() {
        let root = std::env::temp_dir().join(format!("kz-research-managed-{}", unix_ms()));
        let explorations = root.join(".kanzei/research/managed-test/explorations");
        std::fs::create_dir_all(&explorations).unwrap();
        std::fs::write(
            explorations.join("E-managed.md"),
            "---\nkind: exploration\nid: E-managed\ntopic: managed-test\ntitle: test\nstatus: running\nhypothesis: test\ndepends_on:\nsupersedes:\nentry_refs:\nenvironment: ENV-managed\nbudget: 1\ncreated_at: 1\nupdated_at: 1\n---\n\n## 假设\ntest\n\n## 实验结果\n| 实验 | 参数 | 状态 | 关键指标 | 产物 | 结论 |\n| --- | --- | --- | --- | --- | --- |\n\n## 结论\n待定\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".kanzei/research/environments.md"),
            format!(
                "## ENV-managed [active]\n- kind: local\n- host: \n- 归属: test\n- 执行策略: managed\n- gpu: unknown\n- workdir: {}\n- 运行时限: 10m\n- 计费: none\n- 凭据引用: secret://ref\n- 准备步骤: pre-provisioned\n",
                root.display()
            ),
        )
        .unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let store = SessionStore::open(&project_state_path(&root)).unwrap();
        store
            .acquire_research_environment_lease("ENV-managed", "held", "managed", 10_000)
            .unwrap();
        let conflict = run_experiment(managed_input("E-managed-01", "E-managed", None), &ctx).await;
        assert_eq!(conflict.code, Some("ENVIRONMENT_LEASE_CONFLICT"));
        assert!(conflict.is_error);
        store
            .release_research_environment_lease("ENV-managed", "held")
            .unwrap();
        let output = run_experiment(managed_input("E-managed-02", "E-managed", None), &ctx).await;
        assert!(!output.is_error, "{}", output.content);
        let reopened = SessionStore::open(&project_state_path(&root)).unwrap();
        reopened
            .acquire_research_environment_lease("ENV-managed", "after", "managed", 10_000)
            .unwrap();
        reopened
            .release_research_environment_lease("ENV-managed", "after")
            .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    fn managed_input(
        result_id: &str,
        exploration_id: &str,
        confirmed: Option<bool>,
    ) -> RunnerInput {
        RunnerInput {
            action: "run".into(),
            topic: "managed-test".into(),
            environment_id: Some("ENV-managed".into()),
            exploration_id: Some(exploration_id.into()),
            result_id: Some(result_id.into()),
            execution: Some(ExecutionSpec {
                kind: "local".into(),
                command: "echo managed".into(),
                host: None,
                user: None,
                workdir: None,
            }),
            params_text: None,
            code_ref: None,
            policy: None,
            lease_id: None,
            max_duration_ms: Some(10_000),
            cleanup: None,
            heartbeat_timeout_ms: None,
            confirmed,
        }
    }

    #[tokio::test]
    async fn heartbeat_timeout_marks_run_stuck_and_writes_result_row() {
        let root = std::env::temp_dir().join(format!("kz-research-timeout-{}", unix_ms()));
        let explorations = root.join(".kanzei/research/nas-search/explorations");
        std::fs::create_dir_all(&explorations).unwrap();
        let markdown = "---\nkind: exploration\nid: E-001\ntopic: nas-search\ntitle: test\nstatus: running\nhypothesis: test\ndepends_on:\nsupersedes:\nentry_refs:\nenvironment: ENV-test\nbudget: 1\ncreated_at: 1\nupdated_at: 1\n---\n\n## 假设\ntest\n\n## 实验结果\n| 实验 | 参数 | 状态 | 关键指标 | 产物 | 结论 |\n| --- | --- | --- | --- | --- | --- |\n\n## 结论\n待定\n";
        std::fs::write(explorations.join("E-001.md"), markdown).unwrap();
        let command = if cfg!(windows) {
            "ping -n 10 127.0.0.1 > NUL"
        } else {
            "sleep 10"
        };
        let output = run_experiment(
            RunnerInput {
                action: "run".into(),
                topic: "nas-search".into(),
                environment_id: None,
                exploration_id: Some("E-001".into()),
                result_id: Some("E-001-02".into()),
                execution: Some(ExecutionSpec {
                    kind: "local".into(),
                    command: command.into(),
                    host: None,
                    user: None,
                    workdir: None,
                }),
                params_text: Some("timeout=true".into()),
                code_ref: None,
                policy: None,
                lease_id: None,
                max_duration_ms: Some(10_000),
                cleanup: None,
                heartbeat_timeout_ms: Some(50),
                confirmed: None,
            },
            &ToolCtx::new(root.clone(), root.clone()),
        )
        .await;
        assert!(output.is_error);
        let store = SessionStore::open(&project_state_path(&root)).unwrap();
        let run = store.get_research_run("E-001-02").unwrap().unwrap();
        assert_eq!(run.status, "stuck");
        assert_eq!(run.cancel_reason.as_deref(), Some("heartbeat_timeout"));
        let text = std::fs::read_to_string(explorations.join("E-001.md")).unwrap();
        assert!(text.contains("| E-001-02 |"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cancel_kills_local_run_and_persists_cancelled_event() {
        let root = std::env::temp_dir().join(format!("kz-research-cancel-{}", unix_ms()));
        let explorations = root.join(".kanzei/research/nas-search/explorations");
        std::fs::create_dir_all(&explorations).unwrap();
        std::fs::write(explorations.join("E-001.md"), "---\nkind: exploration\nid: E-001\ntopic: nas-search\ntitle: test\nstatus: running\nhypothesis: test\ndepends_on:\nsupersedes:\nentry_refs:\nenvironment: ENV-test\nbudget: 1\ncreated_at: 1\nupdated_at: 1\n---\n\n## 假设\ntest\n\n## 实验结果\n| 实验 | 参数 | 状态 | 关键指标 | 产物 | 结论 |\n| --- | --- | --- | --- | --- | --- |\n\n## 结论\n待定\n").unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let ctx_for_run = ctx.clone();
        let run_task = tokio::spawn(async move {
            run_experiment(
                RunnerInput {
                    action: "run".into(),
                    topic: "nas-search".into(),
                    environment_id: None,
                    exploration_id: Some("E-001".into()),
                    result_id: Some("E-001-03".into()),
                    execution: Some(ExecutionSpec {
                        kind: "local".into(),
                        command: if cfg!(windows) {
                            "ping -n 10 127.0.0.1 > NUL".into()
                        } else {
                            "sleep 10".into()
                        },
                        host: None,
                        user: None,
                        workdir: None,
                    }),
                    params_text: None,
                    code_ref: None,
                    policy: None,
                    lease_id: None,
                    max_duration_ms: Some(10_000),
                    cleanup: None,
                    heartbeat_timeout_ms: Some(10_000),
                    confirmed: None,
                },
                &ctx_for_run,
            )
            .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let cancel = cancel_run(
            &RunnerInput {
                action: "cancel".into(),
                topic: "nas-search".into(),
                environment_id: None,
                exploration_id: None,
                result_id: Some("E-001-03".into()),
                execution: None,
                params_text: None,
                code_ref: None,
                policy: None,
                lease_id: None,
                max_duration_ms: None,
                cleanup: None,
                confirmed: None,
                heartbeat_timeout_ms: None,
            },
            &ctx,
        )
        .await;
        assert!(!cancel.is_error, "{}", cancel.content);
        let _ = run_task.await.unwrap();
        let store = SessionStore::open(&project_state_path(&root)).unwrap();
        let run = store.get_research_run("E-001-03").unwrap().unwrap();
        assert_eq!(run.status, "cancelled");
        assert!(store
            .list_research_run_events("E-001-03", 0)
            .unwrap()
            .iter()
            .any(|event| event.event_type == "run_cancelled"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn artifact_callback_is_copied_and_added_to_run_facts() {
        let root = std::env::temp_dir().join(format!("kz-research-artifact-{}", unix_ms()));
        let output_dir = root.join("result");
        std::fs::create_dir_all(&output_dir).unwrap();
        let source = root.join("checkpoint.bin");
        std::fs::write(&source, b"artifact").unwrap();
        let store = SessionStore::open_in_memory().unwrap();
        let mut run = ResearchRunRecord {
            result_id: "R-artifact".into(),
            exploration_id: "E-001".into(),
            topic: "nas-search".into(),
            status: "running".into(),
            execution_json: "{}".into(),
            policy: "relaxed".into(),
            lease_id: "".into(),
            max_duration_ms: 10_000,
            cleanup: "retain".into(),
            started_at: unix_ms(),
            finished_at: None,
            exit_code: None,
            cancel_reason: None,
            params_text: "".into(),
            code_ref_json: "{}".into(),
            environment_snapshot_ref: "environment.json".into(),
            artifacts_json: "[]".into(),
            metrics_last_json: "{}".into(),
            callback_stats_json: serde_json::to_string(&CallbackStats::default()).unwrap(),
            heartbeat_at: None,
            terminal_log_path: "terminal.log".into(),
        };
        store.upsert_research_run(&run).unwrap();
        let mut terminal = std::fs::File::create(root.join("terminal.log")).unwrap();
        handle_line(
            &store,
            &mut run,
            &mut terminal,
            "R-artifact",
            &format!(
                "@@kanzei {{\"t\":\"artifact\",\"kind\":\"checkpoint\",\"path\":\"{}\"}}",
                source.to_string_lossy().replace('\\', "/")
            ),
            false,
            &output_dir,
        );
        assert_eq!(run.artifacts_json, r#"["artifacts/checkpoint.bin"]"#);
        assert!(output_dir.join("artifacts/checkpoint.bin").is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
