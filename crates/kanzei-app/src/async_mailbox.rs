//! Durable callbacks enter the same serialized scheduler as user input.
use crate::{
    commands::run::{schedule_run, Submission},
    runtime_for, AppState,
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
    let mut current = runtime.async_mailbox.lock().unwrap();
    if let Some(mailbox) = current.as_ref().filter(|m| !m.is_closed()) {
        return mailbox.clone();
    }
    let generation = runtime.async_generation.load(Ordering::SeqCst);
    let owner = owner.to_owned();
    let root = root.to_path_buf();
    let window = window.clone();
    let mailbox = AsyncMailbox::new(move |notice: AsyncNotice| {
        // Admission and explicit stop share a lock. A late callback cannot revive
        // a stopped actor even if it was already enqueued on the executor.
        let app = window.app_handle();
        let state = app.state::<AppState>();
        let runtime = runtime_for(&state, &owner);
        let input_id = format!("async:{owner}:{}", notice.id);
        {
            let _lifecycle = runtime.lifecycle.lock().unwrap();
            if runtime.async_generation.load(Ordering::SeqCst) != generation {
                return Err("原任务已停止".into());
            }
            let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
                .map_err(|e| e.to_string())?;
            store
                .create_session(&owner, &root.display().to_string(), None)
                .map_err(|e| e.to_string())?;
            store
                .admit_input(
                    &owner,
                    &input_id,
                    &notice.text,
                    kanzei_core::Delivery::Steer,
                )
                .map_err(|e| e.to_string())?;
        }
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
    });
    *current = Some(mailbox.clone());
    mailbox
}
