//! A successful shutdown receipt means all execution owners have settled.
use crate::{AppState, MutexPoisonExt};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tauri::Manager;

pub(super) async fn quiesce(app: &tauri::AppHandle) -> Result<Value, String> {
    super::compatibility::begin_shutdown()?;
    finish_shutdown(app).await
}

pub(super) async fn finish_shutdown(app: &tauri::AppHandle) -> Result<Value, String> {
    let result = tokio::time::timeout(super::SHUTDOWN_DEADLINE, settle(app))
        .await
        .map_err(|_| "后台尚有未静止的运行或命令，关闭未完成，未退出进程".to_string())
        .and_then(|result| result);
    if result.is_err() {
        super::compatibility::cancel_shutdown();
    }
    result.map(|()| json!({"stopped":true,"state":"quiescent"}))
}

async fn settle(app: &tauri::AppHandle) -> Result<(), String> {
    // Complete every already-admitted dispatch before taking the owner snapshot;
    // otherwise one could create a runtime just after it was enumerated.
    let mut poll = tokio::time::interval(std::time::Duration::from_millis(50));
    while super::compatibility::in_flight() > 0 {
        poll.tick().await;
    }
    let state = app.state::<AppState>();
    let mobile_active = state.mobile_service.lock_or_recover().is_some();
    if mobile_active {
        // HTTP writes do not enter the desktop RPC admission counter.
        // Its owned connections must settle before snapshotting database owners.
        crate::mobile::stop_service(&state)?;
    }
    let processes = state
        .processes
        .lock_or_recover()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let runtimes = state.runtimes.lock_or_recover().clone();
    let mut roots = crate::durable_questions::roots();
    for (root, team) in kanzei_tools::team::registered_teams() {
        roots.push(root);
        team.stop_all();
    }
    for process in &processes {
        let root = &process.origin_project.0;
        roots.push(root.clone());
        roots.push(process.project_dir.0.clone());
        if let Some(path) = &process.worktree_path {
            roots.push(path.0.clone());
        }
        let session = crate::process_session_id(root, Some(&process.id));
        if let Some(runtime) = runtimes.get(&session) {
            retire_process_runtime(root, &session, runtime)?;
        }
    }
    // An unregistered execution owner must never be silently omitted from shutdown.
    for runtime in runtimes.values() {
        let _lifecycle = runtime.lifecycle.lock_or_recover();
        runtime.retire_async();
        runtime.task_cancellations.cancel_all();
        if let Some(token) = runtime.halt.lock_or_recover().as_ref() {
            token.cancel();
        }
        runtime.asks.lock_or_recover().clear();
    }
    roots.sort();
    roots.dedup();
    stop_terminals(&roots).await?;
    loop {
        let idle = super::compatibility::in_flight() == 0
            && runtimes.values().all(|runtime| owners_quiescent(runtime))
            && kanzei_tools::team::registered_teams()
                .iter()
                .all(|(_, team)| team.is_quiescent());
        if idle {
            break;
        }
        poll.tick().await;
    }
    // Idle is published before the scheduler future drops its final lease. Join it.
    for runtime in runtimes.values() {
        let handle = runtime.current_run.lock_or_recover().take();
        if let Some(handle) = handle {
            match handle.await {
                Ok(()) => {}
                Err(tauri::Error::JoinError(error)) if error.is_cancelled() => {}
                Err(error) => return Err(format!("运行清理任务未正常结束: {error}")),
            }
        }
    }
    // A tool already in flight at cancellation may have registered a terminal
    // after the first pass. With every owner settled this is the final registry.
    stop_terminals(&roots).await
}

fn retire_process_runtime(
    root: &std::path::Path,
    session: &str,
    runtime: &crate::SessionRuntime,
) -> Result<(), String> {
    runtime.task_cancellations.cancel_all();
    kanzei_harness::pending_question::cancel_owner(root, session, None)?;
    let path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&path).map_err(|error| error.to_string())?;
    // Shutdown owns the completion boundary itself. Do not schedule a
    // detached stop watchdog or publish idle before the task has ended.
    let _lifecycle = runtime.lifecycle.lock_or_recover();
    runtime.retire_async();
    if let Some(token) = runtime.halt.lock_or_recover().as_ref() {
        token.cancel();
    }
    runtime.asks.lock_or_recover().clear();
    store
        .finalize_interrupt_if_present(session)
        .map_err(|error| format!("停止会话 {session} 失败 ({}): {error}", root.display()))?;
    Ok(())
}

pub(super) fn owners_quiescent(runtime: &crate::SessionRuntime) -> bool {
    !runtime.running.load(Ordering::SeqCst)
        && !runtime.compacting.load(Ordering::SeqCst)
        && runtime.task_cancellations.is_quiescent()
        && runtime
            .current_run
            .lock_or_recover()
            .as_ref()
            .is_none_or(|handle| handle.inner().is_finished())
}

async fn stop_terminals(roots: &[std::path::PathBuf]) -> Result<(), String> {
    for root in roots {
        for terminal in kanzei_tools::background::list(root) {
            kanzei_tools::background::stop_result(&terminal.id).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shutdown_retires_opened_empty_conversations_without_creating_history() {
        let root = std::env::temp_dir().join(format!(
            "kz-shutdown-empty-{}-{}",
            std::process::id(),
            crate::run::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = crate::normalized_project_root(&root);
        let state = AppState::default();
        let process = crate::processes::registry::ensure_conversation(&state, &root).unwrap();
        let session = crate::process_session_id(&root, Some(&process.id));
        // Opening a conversation hydrates pending asks before the first send.
        let runtime = crate::runtime_for(&state, &session);
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        assert!(store.get_session(&session).unwrap().is_none());
        retire_process_runtime(&root, &session, &runtime).unwrap();
        assert!(owners_quiescent(&runtime));
        assert!(store.get_session(&session).unwrap().is_none());
        assert!(store.list_events(&session, 0).unwrap().is_empty());
        // The same boundary still finalizes real unfinished work atomically.
        store
            .create_session(&session, &root.display().to_string(), None)
            .unwrap();
        store.set_status(&session, "running").unwrap();
        store
            .admit_input(&session, "queued", "hello", kanzei_core::Delivery::Queue)
            .unwrap();
        let halt = kanzei_core::CancellationToken::new();
        *runtime.halt.lock_or_recover() = Some(halt.clone());
        retire_process_runtime(&root, &session, &runtime).unwrap();
        assert!(halt.is_cancelled());
        assert_eq!(store.get_session(&session).unwrap().unwrap().status, "idle");
        assert_eq!(
            store.input_status("queued").unwrap().as_deref(),
            Some("cancelled")
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn idle_and_cancellation_do_not_release_a_live_owner() {
        let runtime = crate::SessionRuntime::default();
        let child = runtime.task_cancellations.register("fixture-child");
        let (started, running) = tokio::sync::oneshot::channel();
        let (release, finish) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            let _child = child;
            started.send(()).unwrap();
            let _ = finish.await;
        });
        *runtime.current_run.lock_or_recover() =
            Some(tauri::async_runtime::JoinHandle::Tokio(handle));
        running.await.unwrap();
        runtime.task_cancellations.cancel_all();
        assert!(!runtime.running.load(Ordering::SeqCst));
        assert!(
            !owners_quiescent(&runtime),
            "idle UI and cancellation are not owner completion"
        );
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !owners_quiescent(&runtime) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let handle = runtime.current_run.lock_or_recover().take().unwrap();
        handle.await.unwrap();
        assert!(runtime.task_cancellations.is_quiescent());
    }
}
