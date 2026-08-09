use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use crate::{ensure_default_process, process_session_id, runtime_for, stop_runtime_and_finalize, with_session_id, AppState, PendingAsk, PromptAttachment, SessionRuntime};

pub(crate) use crate::run_task_impl as run_task;


#[tauri::command]
pub(crate) fn stop_run(
    window: Window,
    state: State<'_, AppState>,
    project_dir: Option<String>,
    process_id: Option<String>,
) {
    let target_project = project_dir.as_ref().map(PathBuf::from).map(|cwd| crate::normalized_project_root(&cwd));
    let target_session = target_project.as_ref().map(|root| process_session_id(root, process_id.as_deref()));
    let runtimes: Vec<Arc<SessionRuntime>> = state.runtimes.lock().unwrap().iter().filter(|(session_id, runtime)| target_session.as_ref().map_or(true, |target| target == *session_id) && runtime.running.load(Ordering::SeqCst)).map(|(_, runtime)| runtime.clone()).collect();
    if runtimes.is_empty() {
        let _ = window.emit("kz:error", with_session_id(json!({ "message": "目标项目当前没有可停止的运行" }), target_session.as_deref().unwrap_or("")));
        return;
    }
    let mut cancelled = None;
    for runtime in runtimes {
        let result = target_project.clone().map(|root| {
            let session_id = target_session.clone().unwrap_or_else(|| kanzei_core::project_session_id(&root));
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).and_then(|store| stop_runtime_and_finalize(&runtime, &store, &session_id))
        });
        cancelled = result;
    }
    match cancelled.transpose() {
        Ok(Some(count)) | Ok(None) => { let _ = window.emit("kz:stopped", with_session_id(json!({ "cancelled_queue": count.unwrap_or(0) }), target_session.as_deref().unwrap_or(""))); }
        Err(error) => { let _ = window.emit("kz:error", with_session_id(json!({ "message": format!("停止时清理排队输入失败: {error}") }), target_session.as_deref().unwrap_or(""))); let _ = window.emit("kz:stopped", with_session_id(json!({ "cancelled_queue": 0 }), target_session.as_deref().unwrap_or(""))); }
    }
    if let Some(root) = target_project {
        let window = window.clone();
        let session = target_session.clone().unwrap_or_default();
        tauri::async_runtime::spawn(async move {
            let killed = kanzei_tools::kill_background_processes(&root).await;
            if killed > 0 { let _ = window.emit("kz:status", with_session_id(json!({ "stage": "停止", "detail": format!("已回收 {killed} 个后台进程") }), &session)); }
        });
    }
}

fn parse_delivery(value: Option<&str>) -> anyhow::Result<kanzei_core::Delivery> {
    match value.unwrap_or("queue") { "steer" => Ok(kanzei_core::Delivery::Steer), "queue" => Ok(kanzei_core::Delivery::Queue), other => Err(anyhow::anyhow!("未知输入交付模式: {other}")) }
}

fn admit_input(project_dir: &str, session_id: &str, prompt: &str, delivery: kanzei_core::Delivery) -> anyhow::Result<kanzei_core::AdmittedInput> {
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project_root))?;
    store.create_session(session_id, &project_root.display().to_string(), None)?;
    let input_id = format!("input_{}", SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos());
    let input = store.admit_input(session_id, &input_id, prompt, delivery)?;
    store.append_event(session_id, "prompt.admitted", &json!({ "input_id": input_id, "delivery": if matches!(delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }))?;
    Ok(input)
}

fn promote_next_input(project_dir: &str, session_id: &str) -> anyhow::Result<Option<kanzei_core::AdmittedInput>> {
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project_root))?;
    let Some(input) = store.promote_next_input(session_id)? else { return Ok(None); };
    store.append_event(session_id, "prompt.promoted", &json!({ "input_id": input.input_id, "delivery": if matches!(input.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }))?;
    Ok(Some(input))
}

pub(crate) fn report_persistence_failure(
    window: &Window,
    session_id: &str,
    operation: &str,
    error: impl std::fmt::Display,
) {
    let message = format!("运行结果已保留，但{operation}失败: {error}");
    tracing::warn!("{message}");
    let _ = window.emit(
        "kz:error",
        with_session_id(json!({ "message": message }), session_id),
    );
}

pub(crate) fn append_run_notification(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    status: &str,
    summary: impl Into<String>,
    requires_action: bool,
) -> anyhow::Result<()> {
    store.append_notification_atomic(
        session_id,
        status,
        &summary.into(),
        requires_action,
    )?;
    Ok(())
}

(
    window: Window,
    state: State<'_, AppState>,
    prompt: String,
    project_dir: String,
    profile: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    work_priority: Option<String>,
    delivery: Option<String>,
    attachments: Option<Vec<PromptAttachment>>,
    process_id: Option<String>,
) -> Result<(), String> {
    let delivery = parse_delivery(delivery.as_deref()).map_err(|e| e.to_string())?;
    let project_root = crate::normalized_project_root(Path::new(&project_dir));
    let process = if let Some(process_id) = process_id.as_deref() {
        let process = state.processes.lock().unwrap().get(process_id).cloned().ok_or_else(|| format!("进程不存在: {process_id}"))?;
        if process.project_dir != project_root.display().to_string() { return Err("进程不属于当前项目".into()); }
        process
    } else { ensure_default_process(&state, &project_root) };
    let session_id = process_session_id(&project_root, Some(&process.id));
    let profile = profile.or_else(|| process.profile.lock().unwrap().clone());
    let model = model.or_else(|| process.model.lock().unwrap().clone());
    let reasoning = process.reasoning.lock().unwrap().clone();
    let subagent_enabled = process.subagent_enabled.load(Ordering::SeqCst);
    let runtime = runtime_for(&state, &session_id);
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    {
        if runtime.running.load(Ordering::SeqCst) {
            if attachments.as_ref().is_some_and(|items| !items.is_empty()) { return Err("当前任务运行中不能排队附件，请等待本轮完成后再发送".into()); }
            let queued = admit_input(&project_dir, &session_id, &prompt, delivery).map_err(|e| e.to_string())?;
            let _ = window.emit("kz:status", with_session_id(json!({ "stage": "排队", "detail": format!("已排队，前方输入将依次执行（{}）", queued.input_id) }), &session_id));
            return Ok(());
        }
        runtime.running.store(true, Ordering::SeqCst);
    }
    let asks = runtime.asks.clone();
    let ask_seq = state.ask_seq.clone();
    let running = runtime.running.clone();
    let lifecycle = runtime.lifecycle.clone();
    let conversation = runtime.conversation.clone();
    let live_run = runtime.live.clone();
    let runtime_for_task = runtime.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut next_input = None;
        let mut next_prompt = prompt;
        let mut next_attachments = attachments;
        let mut idle_reason = "completed";
        loop {
            let result = run_task(&window, asks.clone(), ask_seq.clone(), next_prompt, next_attachments.take(), project_dir.clone(), session_id.clone(), subagent_enabled, profile.clone(), agent.clone(), model.clone(), work_priority.clone(), reasoning.clone(), conversation.clone(), live_run.clone(), delivery, next_input.take()).await;
            if let Err(e) = &result {
                let message = e.to_string();
                let lower = message.to_lowercase();
                let hint = if ["timed out", "timeout", "connect", "dns", "connection"].iter().any(|k| lower.contains(k)) { "\n提示:疑似网络不通。若需代理,在设置页把代理设为「指定地址」(如 http://127.0.0.1:12000)后重试;本地模型(ollama)不受代理影响。" } else { "" };
                let _ = window.emit("kz:error", with_session_id(json!({ "message": format!("{message}{hint}") }), &session_id));
            }
            if result.is_err() { let _lifecycle = lifecycle.lock().unwrap(); running.store(false, Ordering::SeqCst); idle_reason = "failed"; break; }
            next_input = { let _lifecycle = lifecycle.lock().unwrap(); match promote_next_input(&project_dir, &session_id) { Ok(input) => { if input.is_none() { running.store(false, Ordering::SeqCst); } input }, Err(error) => { let _ = window.emit("kz:error", with_session_id(json!({ "message": error.to_string() }), &session_id)); running.store(false, Ordering::SeqCst); idle_reason = "failed"; None } } };
            let Some(input) = next_input.clone() else { break; };
            next_prompt = input.prompt.clone();
            let _ = window.emit("kz:status", with_session_id(json!({ "stage": "排队", "detail": format!("开始执行排队输入（{}）", input.input_id) }), &session_id));
        }
        let _ = window.emit("kz:idle", with_session_id(json!({ "reason": idle_reason }), &session_id));
        runtime_for_task.current_run.lock().unwrap().take();
    });
    *runtime.current_run.lock().unwrap() = Some(handle);
    if !runtime.running.load(Ordering::SeqCst) { runtime.current_run.lock().unwrap().take(); }
    Ok(())
}

#[tauri::command]
pub(crate) async fn run_promptpub(crate) fn run_metrics(project_dir: String, limit: Option<usize>) -> Result<serde_json::Value, String> {
    let root = std::path::PathBuf::from(&project_dir);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).map_err(|e| e.to_string())?;
    let session_id = kanzei_core::project_session_id(&root);
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let rows = store.recent_episodes(&session_id, limit).map_err(|e| e.to_string())?;
    let rounds: Vec<serde_json::Value> = rows.into_iter().map(|(at, prompt, outcome, steps, input, output, tools, context, metrics)| {
        let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap_or(serde_json::json!({}));
        serde_json::json!({ "at": at, "prompt": prompt, "outcome": outcome, "steps": steps, "inputTokens": input, "outputTokens": output, "tools": parse(&tools), "context": parse(&context), "metrics": parse(&metrics), "measured": metrics.trim() != "{}" && !metrics.trim().is_empty() })
    }).collect();
    Ok(serde_json::json!({ "rounds": rounds }))
}
