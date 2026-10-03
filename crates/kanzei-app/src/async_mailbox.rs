//! Durable callbacks enter the same serialized scheduler as user input.
use crate::{
    commands::run::{schedule_run, Submission},
    runtime_for, AppState, MutexPoisonExt, SessionRuntime,
};
use kanzei_harness::{AsyncMailbox, AsyncNotice};
use std::{path::Path, sync::atomic::Ordering};
use tauri::{Emitter, Manager, Window};

pub(crate) fn for_session(
    window: &Window,
    root: &Path,
    owner: &str,
    process_id: Option<String>,
) -> AsyncMailbox {
    let app = window.app_handle();
    let state = app.state::<AppState>();
    let runtime = runtime_for(&state, owner);
    let _lifecycle = runtime.lifecycle.lock_or_recover();
    for_session_locked(window, root, owner, process_id, &runtime)
}

/// Caller holds this runtime's lifecycle; creation and retirement use the same order.
pub(crate) fn for_session_locked(
    window: &Window,
    root: &Path,
    owner: &str,
    process_id: Option<String>,
    runtime: &SessionRuntime,
) -> AsyncMailbox {
    mailbox_locked(runtime, |generation| {
        build(window, root, owner, process_id, generation)
    })
}

fn mailbox_locked(
    runtime: &SessionRuntime,
    create: impl FnOnce(u64) -> AsyncMailbox,
) -> AsyncMailbox {
    let mut current = runtime.async_mailbox.lock_or_recover();
    if let Some(mailbox) = current.as_ref().filter(|m| !m.is_closed()) {
        return mailbox.clone();
    }
    let generation = runtime.async_generation.load(Ordering::SeqCst);
    let mailbox = create(generation);
    *current = Some(mailbox.clone());
    mailbox
}

fn build(
    window: &Window,
    root: &Path,
    owner: &str,
    process_id: Option<String>,
    generation: u64,
) -> AsyncMailbox {
    let owner = owner.to_owned();
    let root = root.to_path_buf();
    let window = window.clone();
    AsyncMailbox::new(move |notice: AsyncNotice| {
        // Admission and explicit stop share a lock. A late callback cannot revive
        // a stopped actor even if it was already enqueued on the executor.
        let app = window.app_handle();
        let state = app.state::<AppState>();
        let runtime = runtime_for(&state, &owner);
        let input_id = admit_notice(&runtime, generation, &root, &owner, &notice)?;
        let window = window.clone();
        let root = root.clone();
        let owner = owner.clone();
        let process_id = process_id.clone();
        tauri::async_runtime::spawn(async move {
            let app = window.app_handle();
            let state = app.state::<AppState>();
            let options = runtime_for(&state, &owner)
                .callback_options
                .lock()
                .unwrap()
                .clone();
            let result = schedule_run(
                window.clone(),
                &state,
                root.display().to_string(),
                process_id,
                Submission::Notice {
                    session_id: owner.clone(),
                    input_id,
                    generation,
                },
                options,
            );
            if let Err(error) = result {
                let _ = window.emit("kz:error", serde_json::json!({"sessionId":owner,"terminal":false,"message":format!("异步结果已保存，恢复执行失败：{error}")}));
            }
        });
        Ok(())
    })
}

fn admit_notice(
    runtime: &SessionRuntime,
    generation: u64,
    root: &Path,
    owner: &str,
    notice: &AsyncNotice,
) -> Result<String, String> {
    let _lifecycle = runtime.lifecycle.lock_or_recover();
    if runtime.async_generation.load(Ordering::SeqCst) != generation {
        return Err("原任务已停止".into());
    }
    let input_id = format!("async:{owner}:{}", notice.id);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
        .map_err(|e| e.to_string())?;
    store
        .create_session(owner, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    store
        .admit_input(owner, &input_id, &notice.text, kanzei_core::Delivery::Steer)
        .map_err(|e| e.to_string())?;
    Ok(input_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc, Mutex};

    #[test]
    fn stop_rejects_a_callback_already_inside_publish_and_current_actor_still_accepts() {
        let root = std::env::temp_dir().join(format!(
            "kz-mailbox-stop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let runtime = Arc::new(SessionRuntime::default());
        let generation = runtime.async_generation.load(Ordering::SeqCst);
        let (entered, entering) = mpsc::channel();
        let (resume, resuming) = mpsc::channel();
        let resuming = Mutex::new(resuming);
        let callback_runtime = runtime.clone();
        let callback_root = root.clone();
        let mailbox = {
            let _lifecycle = runtime.lifecycle.lock_or_recover();
            mailbox_locked(&runtime, |_| {
                AsyncMailbox::new(move |notice| {
                    entered.send(()).unwrap();
                    resuming.lock().unwrap().recv().unwrap();
                    admit_notice(
                        &callback_runtime,
                        generation,
                        &callback_root,
                        "owner",
                        &notice,
                    )
                    .map(|_| ())
                })
            })
        };
        let publishing = std::thread::spawn(move || {
            mailbox.publish(AsyncNotice {
                id: "old".into(),
                text: "old actor reply".into(),
            })
        });
        entering.recv().unwrap();
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store
            .create_session("owner", &root.display().to_string(), None)
            .unwrap();
        crate::stop_runtime_and_finalize(
            &runtime,
            &store,
            &kanzei_core::project_state_path(&root),
            "owner",
        )
        .unwrap();
        resume.send(()).unwrap();
        assert!(publishing.join().unwrap().is_err());
        assert!(store.list_pending_inputs("owner").unwrap().is_empty());
        let current = runtime.async_generation.load(Ordering::SeqCst);
        let notice = AsyncNotice {
            id: "current".into(),
            text: "explicit new actor".into(),
        };
        admit_notice(&runtime, current, &root, "owner", &notice).unwrap();
        admit_notice(&runtime, current, &root, "owner", &notice).unwrap();
        assert_eq!(store.list_pending_inputs("owner").unwrap().len(), 1);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
