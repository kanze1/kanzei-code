use crate::{AppState, MutexPoisonExt, PendingAsk};
use kanzei_harness::pending_question as store;
use serde_json::{json, Value};
use std::path::Path;
use tauri::{Manager, Window};

pub(crate) fn persist(window: &Window, id: u64, pending: &PendingAsk) -> Result<(), String> {
    let kanzei_core::AskRequest::Question {
        callback_id: Some(callback),
        background: true,
        ..
    } = &pending.request
    else {
        return Ok(());
    };
    let state = window.state::<AppState>();
    let runtime = crate::runtime_for(&state, &pending.session_id);
    let mut payload = crate::pending_ask_payload(id, pending);
    payload["projectDir"] = json!(pending.project_root);
    let process_id = state
        .processes
        .lock_or_recover()
        .values()
        .find(|p| crate::process_session_id(&p.origin_project.0, Some(&p.id)) == pending.session_id)
        .map(|p| p.id.clone());
    let options = runtime.callback_options.lock_or_recover().clone();
    store::save(
        &pending.project_root,
        &json!({"payload":payload,"callback_id":callback,"process_id":process_id,"options":options,"state":"pending"}),
    )
}
pub(crate) fn projection(mut payload: Value) -> Value {
    let revision = kanzei_core::store::stable_json_hash(&payload);
    payload["revision"] = json!(revision);
    payload
}
pub(crate) fn pending(root: &Path, owner: Option<&str>) -> Result<Vec<Value>, String> {
    Ok(store::list(root)?
        .into_iter()
        .filter(|v| owner.is_none_or(|owner| v["payload"]["sessionId"] == owner))
        .map(|v| projection(v["payload"].clone()))
        .collect())
}
pub(crate) fn remember_answer(
    root: &Path,
    id: u64,
    reply: &str,
    request: &str,
) -> Result<Option<Value>, String> {
    remember_response(root, id, reply, request, false)
}
pub(crate) fn remember_response(
    root: &Path,
    id: u64,
    reply: &str,
    request: &str,
    cancel: bool,
) -> Result<Option<Value>, String> {
    if !root
        .join(".kanzei/runtime/questions")
        .join(format!("{id}.json"))
        .exists()
    {
        return Ok(None);
    }
    store::respond(root, id, reply, request, cancel).map(Some)
}
pub(crate) async fn deliver(
    window: &Window,
    state: &AppState,
    root: &Path,
    value: Value,
) -> Result<Value, String> {
    let owner = value["payload"]["sessionId"]
        .as_str()
        .ok_or("问题缺少原对话")?;
    let callback = value["callback_id"].as_str().ok_or("问题缺少回调标识")?;
    let id = value["payload"]["id"].as_u64().ok_or("问题缺少标识")?;
    let receipt = json!({"id":id,"sessionId":owner,"projectDir":root,"requestId":value["request_id"],"status":"delivered"});
    if value["state"] == "delivered" {
        return Ok(receipt);
    }
    if value["state"] != "answered" {
        return Err("问题还没有回答".into());
    }
    let process = value["process_id"].as_str();
    crate::processes::registry::restore_processes_from_store_once(state, root)?;
    if process.is_some_and(|id| {
        id != crate::state::default_process_id(root)
            && !state.processes.lock_or_recover().contains_key(id)
    }) {
        return Err("原对话已关闭，回复已保存".into());
    }
    let runtime = crate::runtime_for(state, owner);
    if runtime
        .run_generation
        .load(std::sync::atomic::Ordering::SeqCst)
        == 0
    {
        if let Ok(options) = serde_json::from_value(value["options"].clone()) {
            *runtime.callback_options.lock_or_recover() = options;
        }
    }
    let reply = value["reply"].as_str().ok_or("回复记录损坏")?;
    let question = value["payload"]["question"].as_str().unwrap_or("");
    let text = if value["user_cancelled"] == true {
        format!("用户取消了异步问题 {callback}：{question}。没有提供回答，不得视为同意。")
    } else {
        format!("用户回答异步问题 {callback}\n原问题：{question}\n回答：{reply}")
    };
    if let Some(child) = value["payload"]["agentId"].as_str() {
        let store =
            kanzei_tools::team::store::TeamStore::open(root, owner).map_err(|e| e.to_string())?;
        if matches!(
            store.get(child).map_err(|e| e.to_string())?.state.as_str(),
            "stopped" | "stopping"
        ) {
            return Err("子任务已停止，旧问题不能重新启动它；请明确续做或重新派发".into());
        }
        crate::agent_team::execute(
            window,
            state,
            &root.display().to_string(),
            process,
            json!({"action":"message","id":child,"prompt":text,"message_id":callback}),
        )
        .await
        .map_err(|e| e.to_string())?;
    } else {
        crate::async_mailbox::for_session(window, root, owner, process.map(str::to_owned))
            .publish(kanzei_harness::AsyncNotice {
                id: callback.into(),
                text,
            })?;
    }
    store::settle(root, callback, "delivered")?;
    Ok(receipt)
}
pub(crate) fn roots() -> Vec<std::path::PathBuf> {
    crate::prefs::load_prefs()
        .projects
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect()
}
pub(crate) fn find(id: u64) -> Option<(std::path::PathBuf, Value)> {
    roots()
        .into_iter()
        .find_map(|root| store::get(&root, id).ok().map(|v| (root, v)))
}
pub(crate) fn recover_outbox(window: Window) {
    tauri::async_runtime::spawn(async move {
        for root in roots() {
            let Ok(records) = store::list(&root) else {
                continue;
            };
            for record in records.into_iter().filter(|v| v["state"] == "answered") {
                if let Err(error) =
                    deliver(&window, &window.state::<AppState>(), &root, record).await
                {
                    tracing::warn!(%error,"question reply remains in outbox");
                }
            }
        }
    });
}
