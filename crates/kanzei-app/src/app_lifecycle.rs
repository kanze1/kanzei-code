//! 单进程桌面应用生命周期管理：启动聚合引导、请求准入控制与优雅停机。
use crate::{AppState, MutexPoisonExt};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tauri::Manager;

const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(crate::state::STOP_GRACE_SECS + 15);

static RETIRING: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// 正在提交的运行任务准入守卫；关闭/更新停机期间拒绝接纳新任务。
pub(crate) struct Admission;

impl Drop for Admission {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn admit_work() -> Result<Admission, String> {
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    let guard = Admission;
    if RETIRING.load(Ordering::SeqCst) {
        return Err("应用正在退出或更新，暂不接受新任务".into());
    }
    Ok(guard)
}

pub(crate) fn begin_shutdown() -> Result<(), String> {
    RETIRING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map(|_| ())
        .map_err(|_| "应用已在关闭或更新中".into())
}

pub(crate) fn cancel_update_shutdown() {
    RETIRING.store(false, Ordering::SeqCst);
}

fn in_flight() -> usize {
    IN_FLIGHT.load(Ordering::SeqCst)
}

/// 更新前停机：确保全部活跃会话和后台终端完全静止，再交接给安装助手。
pub(crate) async fn shutdown_for_update(app: &tauri::AppHandle) -> Result<(), String> {
    quiesce(app).await.map(|_| ())
}

/// 聚合启动引导：单次 IPC 返回首屏与导航所需的全部静态配置，避免冷启动多轮往返瀑布流。
#[tauri::command]
pub(crate) async fn app_bootstrap() -> Result<Value, String> {
    let app_info = crate::run::app_info();
    let ui_prefs = crate::prefs::ui_prefs_get();
    let projects_prefs = crate::projects::projects_get();
    let projects = serde_json::to_value(&projects_prefs).map_err(|e| e.to_string())?;
    let general_chat_root = crate::general_chat::general_chat_location()?;
    let (draft_items, drafts_error) = match crate::composer_drafts::composer_drafts_load().await {
        Ok(items) => (items, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    let conversation_mode = ui_prefs
        .get("ui_layout")
        .and_then(|layout| layout.get("conversation_mode"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let target = if conversation_mode == "general" {
        json!({
            "kind": "general",
            "projectDir": general_chat_root,
        })
    } else if let Some(current) = projects_prefs.current.as_deref() {
        json!({
            "kind": "project",
            "projectDir": current,
        })
    } else {
        json!({
            "kind": "workspace",
            "projectDir": Value::Null,
        })
    };
    Ok(json!({
        "appInfo": app_info,
        "uiPrefs": ui_prefs,
        "projects": projects,
        "generalChatRoot": general_chat_root,
        "composerDrafts": {
            "items": draft_items,
            "error": drafts_error,
        },
        "target": target,
    }))
}

/// 优雅关闭当前进程内全部运行中的会话与后台终端；全部静止后返回回执。
#[tauri::command]
pub(crate) async fn app_shutdown(app: tauri::AppHandle) -> Result<Value, String> {
    quiesce(&app).await
}

pub(crate) async fn quiesce(app: &tauri::AppHandle) -> Result<Value, String> {
    begin_shutdown()?;
    finish_shutdown(app).await
}

async fn finish_shutdown(app: &tauri::AppHandle) -> Result<Value, String> {
    let result = tokio::time::timeout(SHUTDOWN_DEADLINE, settle(app))
        .await
        .map_err(|_| "后台尚有未静止的运行或命令，关闭未完成，未退出进程".to_string())
        .and_then(|result| result);
    if result.is_err() {
        cancel_update_shutdown();
    }
    result.map(|()| json!({"stopped": true, "state": "quiescent"}))
}

async fn settle(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    for runtime in state.runtimes.lock_or_recover().values() {
        let _lifecycle = runtime.lifecycle.lock_or_recover();
        runtime.transition(kanzei_core::session_lifecycle::Event::CloseRequested)?;
        runtime.retire_async();
        runtime.task_cancellations.cancel_all();
        if let Some(token) = runtime.halt.lock_or_recover().as_ref() {
            token.cancel();
        }
        runtime.asks.lock_or_recover().clear();
    }
    // 等所有已准入但尚未落表的任务完成，避免在快照之后冒出新的 runtime。
    let mut poll = tokio::time::interval(Duration::from_millis(50));
    while in_flight() > 0 {
        poll.tick().await;
    }
    let state = app.state::<AppState>();
    let processes = state
        .processes
        .lock_or_recover()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let runtimes = state.runtimes.lock_or_recover().clone();
    let mut roots: Vec<std::path::PathBuf> = crate::prefs::load_prefs()
        .projects
        .into_iter()
        .map(|p| crate::normalized_project_root(std::path::Path::new(&p)))
        .filter(|p| p.is_dir())
        .collect();
    if let Ok(general) = crate::general_chat::storage_root() {
        if general.is_dir() {
            roots.push(crate::normalized_project_root(&general));
        }
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
            crate::session_supervisor::close_runtime(runtime, root, &process.id).await?;
        }
    }
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
        let idle = in_flight() == 0 && runtimes.values().all(|runtime| owners_quiescent(runtime));
        if idle {
            break;
        }
        poll.tick().await;
    }
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
    stop_terminals(&roots).await
}

pub(crate) fn owners_quiescent(runtime: &crate::SessionRuntime) -> bool {
    !runtime.is_running()
        && !runtime.is_compacting()
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

/// 窗口被操作系统直接销毁时的兜底清理：若未触发过主动停机，则立即尽力终止活跃会话与终端。
pub(crate) fn cleanup_on_window_destroyed(app: &tauri::AppHandle) {
    if begin_shutdown().is_err() {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = finish_shutdown(&handle).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_retires_opened_empty_conversations_without_creating_history() {
        let root = kanzei_base::test_support::TestDir::new("kz-shutdown-empty-");
        let state = AppState::default();
        let process = crate::processes::registry::ensure_conversation(&state, &root).unwrap();
        let session = crate::process_session_id(&root, Some(&process.id));
        let runtime = crate::runtime_for(&state, &session);
        crate::session_supervisor::close_runtime(&runtime, &root, &process.id)
            .await
            .unwrap();
        assert!(owners_quiescent(&runtime));
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        assert!(store.get_session(&session).unwrap().is_none());
        assert!(store.list_events(&session, 0).unwrap().is_empty());
        assert_eq!(
            runtime.snapshot().existence,
            kanzei_core::session_lifecycle::Existence::Closed
        );
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
        assert!(!runtime.is_running());
        assert!(
            !owners_quiescent(&runtime),
            "idle UI and cancellation are not owner completion"
        );
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
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

    #[tokio::test]
    async fn app_bootstrap_aggregates_core_startup_payload() {
        let payload = app_bootstrap().await.unwrap();
        assert!(payload["appInfo"]["version"].is_string());
        assert!(payload["uiPrefs"].is_object());
        assert!(payload["projects"]["projects"].is_array());
        assert!(payload["composerDrafts"]["items"].is_array());
        assert!(matches!(
            payload["target"]["kind"].as_str(),
            Some("project" | "general" | "workspace")
        ));
    }
}
