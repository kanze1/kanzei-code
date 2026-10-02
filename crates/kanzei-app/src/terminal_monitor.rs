use crate::{AppState, MutexPoisonExt};
use serde_json::{json, Value};
use std::path::Path;
use tauri::{State, Window};
#[tauri::command]
pub(crate) fn terminal_monitor(
    window: Window,
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
    action: String,
    id: Option<String>,
    pattern: Option<String>,
) -> Result<Value, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    let owner = crate::process_session_id(&root, process_id.as_deref());
    let process = process_id
        .clone()
        .unwrap_or_else(|| crate::state::default_process_id(&root));
    crate::processes::registry::restore_processes_from_store_once(&state, &root)?;
    let _ = crate::ensure_default_process(&state, &root);
    if !state
        .processes
        .lock_or_recover()
        .get(&process)
        .is_some_and(|p| p.origin_project.0 == root)
    {
        return Err("对话不属于当前项目".into());
    }
    match action.as_str() {
        "list" => {
            let subscriptions = kanzei_tools::background::monitor::subscriptions(&root, &process);
            Ok(json!(kanzei_tools::background::list(&root).iter().map(|p|json!({"id":p.id,"command":p.command,"running":p.is_running(),"exit":p.exit_code(),"owner":p.owner.process_id,
                "subscribed":subscriptions.iter().any(|s|s["id"]==p.id),"output":p.output().chars().rev().take(6000).collect::<String>().chars().rev().collect::<String>()})).collect::<Vec<_>>()))
        }
        "watch" => {
            let _ = crate::ensure_default_process(&state, &root);
            kanzei_tools::background::monitor::subscribe(
                &root,
                &process,
                id.as_deref().ok_or("需要终端 id")?,
                crate::async_mailbox::for_session(&window, &root, &owner, process_id),
                pattern.as_deref(),
                None,
            )
        }
        "unwatch" => Ok(
            json!({"cancelled":kanzei_tools::background::monitor::unsubscribe(&root,&process,id.as_deref().ok_or("需要终端 id")?)}),
        ),
        _ => Err("未知日志操作".into()),
    }
}
