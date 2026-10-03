use crate::{AppState, MutexPoisonExt, PendingAsk, SessionRuntime};
use kanzei_harness::pending_question as store;
use kanzei_harness::AsyncMailbox;
use serde_json::{json, Value};
use std::{path::Path, sync::atomic::Ordering};
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
        .ok_or("问题缺少原对话")?
        .to_owned();
    let callback = value["callback_id"]
        .as_str()
        .ok_or("问题缺少回调标识")?
        .to_owned();
    let id = value["payload"]["id"].as_u64().ok_or("问题缺少标识")?;
    let runtime = crate::runtime_for(state, &owner);
    let prepared = prepare_reply(root, id, &owner, &callback, &runtime, |record| {
        crate::async_mailbox::for_session_locked(
            window,
            root,
            &owner,
            record["process_id"].as_str().map(str::to_owned),
            &runtime,
        )
    })?;
    let value = prepared.record;
    let receipt = json!({"id":id,"sessionId":owner,"projectDir":root,"requestId":value["request_id"],"status":"delivered"});
    if value["state"] == "delivered" {
        return Ok(receipt);
    }
    let process = value["process_id"].as_str();
    crate::processes::registry::restore_processes_from_store_once(state, root)?;
    if process.is_some_and(|id| {
        id != crate::state::default_process_id(root)
            && !state.processes.lock_or_recover().contains_key(id)
    }) {
        return Err("原对话已关闭，回复已保存".into());
    }
    let reply = value["reply"].as_str().ok_or("回复记录损坏")?;
    let question = value["payload"]["question"].as_str().unwrap_or("");
    let text = if value["user_cancelled"] == true {
        format!("用户取消了异步问题 {callback}：{question}。没有提供回答，不得视为同意。")
    } else {
        format!("用户回答异步问题 {callback}\n原问题：{question}\n回答：{reply}")
    };
    if let Some(child) = value["payload"]["agentId"].as_str() {
        crate::agent_team::reply_to_question(
            window,
            state,
            &root.display().to_string(),
            process,
            crate::agent_team::QuestionReply {
                owner,
                child: child.into(),
                question_id: id,
                generation: prepared.generation,
                mailbox: prepared.mailbox.unwrap(),
                notice: kanzei_harness::AsyncNotice {
                    id: callback.clone(),
                    text,
                },
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    } else {
        prepared
            .mailbox
            .unwrap()
            .publish(kanzei_harness::AsyncNotice {
                id: callback.clone(),
                text,
            })?;
    }
    store::settle(root, &callback, "delivered")?;
    Ok(receipt)
}

struct PreparedReply {
    record: Value,
    generation: u64,
    mailbox: Option<AsyncMailbox>,
}

fn prepare_reply(
    root: &Path,
    id: u64,
    owner: &str,
    callback: &str,
    runtime: &SessionRuntime,
    mailbox: impl FnOnce(&Value) -> AsyncMailbox,
) -> Result<PreparedReply, String> {
    // Never trust an answered snapshot held across stop/recovery. Bind the mailbox
    // while holding the same actor lock used by stop, then publish after release.
    let _lifecycle = runtime.lifecycle.lock_or_recover();
    let record = store::get(root, id)?;
    if record["payload"]["sessionId"] != owner || record["callback_id"] != callback {
        return Err("问题归属不匹配".into());
    }
    match record["state"].as_str() {
        Some("delivered") => {}
        Some("answered") => {}
        Some("cancelled") => return Err("问题已取消，旧回复不能重新启动任务".into()),
        _ => return Err("问题还没有回答".into()),
    }
    if record["state"] == "answered" && runtime.run_generation.load(Ordering::SeqCst) == 0 {
        if let Ok(options) = serde_json::from_value(record["options"].clone()) {
            *runtime.callback_options.lock_or_recover() = options;
        }
    }
    Ok(PreparedReply {
        generation: runtime.async_generation.load(Ordering::SeqCst),
        mailbox: (record["state"] == "answered").then(|| mailbox(&record)),
        record,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicBool, Arc};

    fn project() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-durable-reply-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        store::save(&root, &json!({"payload":{"id":1,"sessionId":"owner","question":"choose"},"callback_id":"callback","state":"pending"})).unwrap();
        root
    }

    #[test]
    fn answered_snapshot_after_stop_cannot_create_a_new_mailbox() {
        let root = project();
        let snapshot = store::answer(&root, 1, "yes", "reply").unwrap();
        let runtime = SessionRuntime::default();
        store::cancel_owner(&root, "owner", None).unwrap();
        {
            let _lifecycle = runtime.lifecycle.lock_or_recover();
            runtime.retire_async();
        }
        let created = AtomicBool::new(false);
        let result = prepare_reply(
            &root,
            snapshot["payload"]["id"].as_u64().unwrap(),
            "owner",
            "callback",
            &runtime,
            |_| {
                created.store(true, Ordering::SeqCst);
                AsyncMailbox::new(|_| Ok(()))
            },
        );
        assert!(result.is_err());
        assert!(!created.load(Ordering::SeqCst));
        assert!(runtime.async_mailbox.lock_or_recover().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reply_bound_before_stop_keeps_the_retired_mailbox_and_delivered_retry_is_a_receipt() {
        let root = project();
        store::answer(&root, 1, "yes", "reply").unwrap();
        let runtime = SessionRuntime::default();
        let published = Arc::new(AtomicBool::new(false));
        let delivered = published.clone();
        let prepared = prepare_reply(&root, 1, "owner", "callback", &runtime, |_| {
            let mailbox = AsyncMailbox::new(move |_| {
                delivered.store(true, Ordering::SeqCst);
                Ok(())
            });
            *runtime.async_mailbox.lock_or_recover() = Some(mailbox.clone());
            mailbox
        })
        .unwrap();
        store::cancel_owner(&root, "owner", None).unwrap();
        {
            let _lifecycle = runtime.lifecycle.lock_or_recover();
            runtime.retire_async();
        }
        assert!(prepared
            .mailbox
            .unwrap()
            .publish(kanzei_harness::AsyncNotice {
                id: "callback".into(),
                text: "yes".into()
            })
            .is_err());
        assert!(!published.load(Ordering::SeqCst));
        std::fs::remove_dir_all(&root).unwrap();

        let root = project();
        store::answer(&root, 1, "yes", "reply").unwrap();
        let legal = prepare_reply(&root, 1, "owner", "callback", &runtime, |_| {
            AsyncMailbox::new(|_| Ok(()))
        })
        .unwrap();
        assert_eq!(legal.record["reply"], "yes");
        store::settle(&root, "callback", "delivered").unwrap();
        let retry = prepare_reply(&root, 1, "owner", "callback", &runtime, |_| {
            panic!("receipt must not create a mailbox")
        })
        .unwrap();
        assert!(retry.mailbox.is_none());
        assert_eq!(retry.record["state"], "delivered");
        std::fs::remove_dir_all(root).unwrap();
    }
}
