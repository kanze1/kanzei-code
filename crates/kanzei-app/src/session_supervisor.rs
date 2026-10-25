//! The host survives worker cancellation and owns the resource completion boundary.
use crate::{MutexPoisonExt, SessionRuntime};
use kanzei_core::session_lifecycle::{Event, Phase};
use std::{path::Path, sync::Arc};

pub(crate) async fn worker_result(
    mut worker: tauri::async_runtime::JoinHandle<anyhow::Result<()>>,
    cancellation: kanzei_core::CancellationToken,
    grace: std::time::Duration,
) -> anyhow::Result<()> {
    let joined = tokio::select! {
        result = &mut worker => result,
        _ = cancellation.cancelled() => {
            tokio::select! {
                result = &mut worker => result,
                _ = tokio::time::sleep(grace) => {
                    worker.abort();
                    worker.await
                }
            }
        }
    };
    match joined {
        Ok(result) => result,
        Err(tauri::Error::JoinError(error))
            if error.is_cancelled() && cancellation.is_cancelled() =>
        {
            Err(crate::run::persistence::stopped_outcome(anyhow::anyhow!(
                "运行已取消，执行任务已回收"
            )))
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn settle_resources(
    runtime: &SessionRuntime,
    root: &Path,
    process: &str,
) -> Result<(), String> {
    runtime.task_cancellations.cancel_all();
    if let Some(team) =
        kanzei_tools::team::find(root, &crate::process_session_id(root, Some(process)))
    {
        team.cancel_workers();
    }
    runtime.task_cancellations.settled().await;
    if let Some(team) =
        kanzei_tools::team::find(root, &crate::process_session_id(root, Some(process)))
    {
        team.stop_and_settle().await.map_err(|e| e.to_string())?;
    }
    for terminal in kanzei_tools::background::list(root)
        .into_iter()
        .filter(|terminal| terminal.owner.process_id == process)
    {
        kanzei_tools::background::stop_result(&terminal.id).await?;
    }
    crate::desktop_bridge::release_native(process).await
}

static EVENTS: std::sync::OnceLock<crate::app_events::AppEvents> = std::sync::OnceLock::new();
pub(crate) fn install(app: &crate::app_events::AppEvents) {
    let _ = EVENTS.set(app.clone());
}
pub(crate) fn publish(session: &str, snapshot: &kanzei_core::session_lifecycle::Snapshot) {
    use tauri::Emitter;
    if let Some(app) = EVENTS.get() {
        let _ = app.emit(
            "kz:lifecycle",
            crate::with_session_id(serde_json::json!({"runtime":snapshot}), session),
        );
    }
}

/// Compatibility render notifications are derived from the authoritative receipt.
pub(crate) fn publish_idle(
    app: &crate::app_events::AppEvents,
    runtime: &SessionRuntime,
    session: &str,
    reason: &str,
) {
    use tauri::Emitter;
    let snapshot = runtime.snapshot();
    if snapshot.phase != Phase::Idle {
        return;
    }
    let payload = crate::with_session_id(
        serde_json::json!({"reason":reason,"runtime":snapshot,"cancelled_queue":snapshot.stop.as_ref().map_or(0,|s|s.cancelled_inputs)}),
        session,
    );
    if reason == "stopped" {
        let _ = app.emit("kz:stopped", payload.clone());
    }
    let _ = app.emit("kz:idle", payload);
}

/// The foreground parent reaps run workers. Idle/background stops and exclusive
/// mutations attach one surviving parent here, under the same admission lock.
pub(crate) fn ensure_stop_supervisor(
    runtime: &Arc<SessionRuntime>,
    root: &Path,
    process: &str,
) -> Result<(), String> {
    use kanzei_core::session_lifecycle::Activity;
    let _lifecycle = runtime.lifecycle.lock_or_recover();
    let s = runtime.snapshot();
    if !runtime.stopping() {
        return Ok(());
    }
    let mut current = runtime.current_run.lock_or_recover();
    if matches!(s.activity, Some(Activity::Cleanup | Activity::Run))
        && s.error.is_none()
        && current.as_ref().is_some_and(|h| !h.inner().is_finished())
    {
        return Ok(());
    }
    let previous = current.take();
    runtime.transition(Event::ReaperAttached)?;
    let runtime = runtime.clone();
    let root = root.to_path_buf();
    let process = process.to_string();
    *current = Some(tauri::async_runtime::spawn(async move {
        let result = async {
            if let Some(previous) = previous {
                previous.await.map_err(|e| e.to_string())?;
            }
            loop {
                let changed = runtime.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if runtime.snapshot().phase == Phase::Reaping {
                    break;
                }
                changed.await;
            }
            settle_resources(&runtime, &root, &process).await?;
            let session = crate::process_session_id(&root, Some(&process));
            let path = kanzei_core::project_state_path(&root);
            let owner = kanzei_core::session_execution::try_acquire(&path, &session)
                .map_err(|e| e.to_string())?;
            let store = kanzei_core::SessionStore::open(&path).map_err(|e| e.to_string())?;
            let _lifecycle = runtime.lifecycle.lock_or_recover();
            owner.settle_stopped(&store).map_err(|e| e.to_string())?;
            let stop = runtime.snapshot().stop.ok_or("停止操作缺失")?;
            store
                .settle_session_stop(&session, &stop)
                .map_err(|e| e.to_string())?;
            drop(owner);
            runtime.halt.lock_or_recover().take();
            runtime.transition(Event::StopSettled)?;
            runtime.transition(Event::Settled("stopped".into()))?;
            Ok::<_, String>(session)
        }
        .await;
        match result {
            Ok(session) => {
                if let Some(app) = EVENTS.get() {
                    publish_idle(app, &runtime, &session, "stopped");
                }
                wake_pending(&session);
            }
            Err(error) => {
                let _ = runtime.transition(Event::Failed(error));
            }
        }
    }));
    Ok(())
}

pub(crate) async fn close_runtime(
    runtime: &Arc<SessionRuntime>,
    root: &Path,
    process: &str,
) -> Result<(), String> {
    {
        let _lifecycle = runtime.lifecycle.lock_or_recover();
        if runtime.snapshot().existence == kanzei_core::session_lifecycle::Existence::Closed {
            return Ok(());
        }
        runtime.transition(Event::CloseRequested)?;
    }
    let session = crate::process_session_id(root, Some(process));
    {
        let path = kanzei_core::project_state_path(root);
        let store = kanzei_core::SessionStore::open(&path).map_err(|e| e.to_string())?;
        crate::request_runtime_stop(runtime, &store, &path, &session).map_err(|e| e.to_string())?;
    }
    ensure_stop_supervisor(runtime, root, process)?;
    loop {
        let changed = runtime.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let snapshot = runtime.snapshot();
        match snapshot.phase {
            Phase::Idle => break,
            Phase::Blocked => {
                return Err(snapshot.error.unwrap_or_else(|| "对话回收未完成".into()))
            }
            _ => changed.await,
        }
    }
    let handle = runtime.current_run.lock_or_recover().take();
    if let Some(handle) = handle {
        handle.await.map_err(|e| e.to_string())?;
    }
    runtime.transition(Event::Closed)
}

/// Wake the durable inbox after an exclusive mutation or idle resource stop.
/// Actual admission and queue order remain owned by schedule_run's lock.
pub(crate) fn wake_pending(session: &str) {
    use tauri::Manager;
    let Some(app) = EVENTS.get().cloned() else {
        return;
    };
    let session = session.to_string();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<crate::AppState>();
        let runtime = crate::runtime_for(&state, &session);
        let s = runtime.snapshot();
        if s.existence != kanzei_core::session_lifecycle::Existence::Open || s.phase != Phase::Idle
        {
            return;
        }
        let process = state
            .processes
            .lock_or_recover()
            .values()
            .find(|p| crate::process_session_id(&p.project_dir.0, Some(&p.id)) == session)
            .cloned();
        let Some(process) = process else {
            return;
        };
        let root = &process.project_dir.0;
        let result = (|| {
            let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
                .map_err(|e| e.to_string())?;
            let inputs = store
                .list_pending_inputs(&session)
                .map_err(|e| e.to_string())?;
            let Some(input) = inputs.first() else {
                return Ok(());
            };
            let options = runtime.callback_options.lock_or_recover().clone();
            crate::commands::run::schedule_run(
                app.clone(),
                &state,
                root.display().to_string(),
                Some(process.id.clone()),
                crate::commands::run::Submission::Saved {
                    session_id: session.clone(),
                    input_id: input.input_id.clone(),
                },
                options,
            )
            .map(|_| ())
        })();
        if let Err(error) = result {
            let _ = runtime.transition(Event::Failed(error));
        }
    });
}
