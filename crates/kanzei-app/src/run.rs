use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;
use tauri::{Emitter, State, Window};

use crate::{admit_input, ensure_default_process, parse_delivery, process_session_id, promote_next_input, runtime_for, with_session_id, AppState, AskFuture, PendingAsk, PromptAttachment};

#[tauri::command]
pub(crate) async fn run_prompt(
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
            let result = crate::run_task(&window, asks.clone(), ask_seq.clone(), next_prompt, next_attachments.take(), project_dir.clone(), session_id.clone(), subagent_enabled, profile.clone(), agent.clone(), model.clone(), work_priority.clone(), reasoning.clone(), conversation.clone(), live_run.clone(), delivery, next_input.take()).await;
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
pub(crate) fn run_metrics(project_dir: String, limit: Option<usize>) -> Result<serde_json::Value, String> {
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
