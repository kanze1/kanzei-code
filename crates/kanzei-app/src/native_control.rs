//! The visible desktop lease owns its stop signal, action ledger and Escape hook.
//! Stop revokes the capability first; only a drained executor may confirm stopped.
use crate::MutexPoisonExt;
use kanzei_harness::ToolCtx;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{LazyLock, Mutex, OnceLock},
};
use tauri::{Emitter, Listener, Manager};
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
mod windows;
const TRACE_LIMIT: usize = 48;
// Identifies only our executor's synthetic keys, not assistive/user input.
pub(crate) const INPUT_TAG: usize = 0x4b5a444b;
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static PLATFORM: OnceLock<Result<(), String>> = OnceLock::new();
static CONTROL: LazyLock<Mutex<ControlState>> = LazyLock::new(Mutex::default);
static PRESENT: LazyLock<Mutex<()>> = LazyLock::new(Mutex::default);

#[derive(Clone, Serialize)]
pub(crate) struct Owner {
    pub id: String,
    pub project_dir: PathBuf,
    pub process_id: String,
    pub session_id: String,
    pub run_id: String,
    pub window_id: String,
    pub title: String,
}
#[derive(Clone, Serialize)]
struct Action {
    id: u64,
    action: String,
    target: String,
    status: String,
    bounds: Option<Value>,
    point: Option<Value>,
    started_at: i64,
    ended_at: Option<i64>,
}
#[derive(Default)]
struct ControlState {
    revision: u64,
    owner: Option<Owner>,
    token: CancellationToken,
    phase: Phase,
    trace: VecDeque<Action>,
    error: Option<String>,
    helper_pid: Option<u32>,
}
#[derive(Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    #[default]
    Idle,
    Running,
    Stopping,
    Stopped,
    Ended,
    StopFailed,
}
#[derive(Clone)]
pub(crate) struct Lease {
    pub id: String,
    pub cancel: CancellationToken,
}

impl ControlState {
    fn snapshot(&self) -> Value {
        json!({"revision":self.revision,"phase":self.phase,"owner":self.owner,"trace":self.trace,"error":self.error,"helper_pid":self.helper_pid})
    }
    fn claim(&mut self, owner: Owner) -> Result<Lease, String> {
        if self.phase == Phase::Stopping {
            return Err("原生控制正在停止，尚未释放执行权".into());
        }
        if self.phase != Phase::Running
            && self.owner.as_ref().is_some_and(|current| {
                current.process_id == owner.process_id && current.run_id == owner.run_id
            })
        {
            return Err("原生控制的这一轮已结束，旧动作不能重新取得控制权".into());
        }
        if self.helper_pid.is_some() {
            return Err("原生输入进程尚未退出".into());
        }
        if self.phase == Phase::Running {
            let current = self.owner.as_ref().ok_or("原生控制归属缺失")?;
            if current.process_id != owner.process_id
                || current.project_dir != owner.project_dir
                || current.run_id != owner.run_id
                || current.window_id != owner.window_id
            {
                return Err("另一段对话正在控制电脑，请先停止当前控制".into());
            }
        } else {
            self.owner = Some(owner);
            self.token = CancellationToken::new();
            self.trace.clear();
            self.error = None;
        }
        if self.token.is_cancelled() {
            return Err("原生控制能力已撤销，请重新选择窗口".into());
        }
        self.phase = Phase::Running;
        self.revision += 1;
        Ok(Lease {
            id: self.owner.as_ref().unwrap().id.clone(),
            cancel: self.token.clone(),
        })
    }
    fn cancel(&mut self, id: &str) -> Result<Owner, String> {
        if self.phase == Phase::Idle {
            return Err("原生控制已经收起，旧停止请求已失效".into());
        }
        let owner = self
            .owner
            .as_ref()
            .filter(|owner| owner.id == id)
            .cloned()
            .ok_or("控制归属已变化，旧停止请求未转移到新任务")?;
        self.token.cancel();
        if self.phase != Phase::Stopped {
            self.phase = Phase::Stopping;
        }
        self.revision += 1;
        Ok(owner)
    }
    fn confirm(&mut self, id: &str) -> Result<(), String> {
        if self.phase == Phase::Stopping
            && self.owner.as_ref().is_some_and(|owner| owner.id == id)
            && self.token.is_cancelled()
        {
            if self.helper_pid.is_some() {
                return Err("原生输入进程尚未退出，停止未确认".into());
            }
            self.phase = Phase::Stopped;
            self.error = None;
            self.revision += 1;
        }
        Ok(())
    }
    fn request_stop(&mut self, id: &str) -> Result<Owner, String> {
        if !matches!(
            self.phase,
            Phase::Running | Phase::Stopping | Phase::StopFailed
        ) {
            return Err("这一轮控制已经结束，旧停止按钮已失效".into());
        }
        self.cancel(id)
    }
    fn cancel_owner(
        &mut self,
        process: &str,
        run: Option<&str>,
        window: Option<&str>,
    ) -> Result<Option<Owner>, String> {
        if matches!(self.phase, Phase::Idle | Phase::Stopped) {
            return Ok(None);
        }
        let owner = self
            .owner
            .as_ref()
            .filter(|owner| {
                owner.process_id == process
                    && run.is_none_or(|run| owner.run_id == run)
                    && window.is_none_or(|window| owner.window_id == window)
            })
            .cloned();
        owner.map(|owner| self.cancel(&owner.id)).transpose()
    }
}

pub(crate) fn install(app: &tauri::AppHandle, main: &tauri::WebviewWindow) {
    let _ = APP.set(app.clone());
    #[cfg(windows)]
    let ready = windows::install(app);
    #[cfg(not(windows))]
    let ready = Err("原生控制提示与急停需要 Windows".into());
    let _ = PLATFORM.set(ready);
    for name in ["kz:done", "kz:stopped", "kz:idle", "kz:error"] {
        app.listen(name, move |event| {
            let Ok(payload) = serde_json::from_str::<Value>(event.payload()) else {
                return;
            };
            if name == "kz:error" && payload["terminal"] != true {
                return;
            }
            let mut state = CONTROL.lock_or_recover();
            if state.phase != Phase::Running
                || !state
                    .owner
                    .as_ref()
                    .is_some_and(|owner| payload["sessionId"] == owner.session_id)
            {
                return;
            }
            let owner = state.owner.as_ref().unwrap().clone();
            let drain = state.helper_pid.is_some() || name == "kz:stopped";
            state.token.cancel();
            state.phase = if drain { Phase::Stopping } else { Phase::Ended };
            state.revision += 1;
            drop(state);
            if drain {
                crate::native_desktop::revoke(&owner.process_id, &owner.window_id);
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = crate::native_desktop::release(
                        &owner.process_id,
                        Some(&owner.run_id),
                        Some(&owner.window_id),
                    )
                    .await
                    {
                        tracing::error!(%error, "terminal native cleanup not confirmed");
                    }
                });
            }
            if let Err(error) = publish() {
                tracing::error!(%error, "native control terminal display failed");
            }
        });
    }
    let owner_app = app.clone();
    main.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let app = owner_app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(id) = current_id() {
                    if let Err(error) = native_desktop_stop(id).await {
                        tracing::error!(%error, "main window native stop not confirmed");
                    }
                }
                let owner = CONTROL.lock_or_recover().owner.clone();
                if let Some(owner) = owner {
                    if let Err(error) = crate::native_desktop::release(
                        &owner.process_id,
                        Some(&owner.run_id),
                        Some(&owner.window_id),
                    )
                    .await
                    {
                        tracing::error!(%error, "main window input cleanup not confirmed");
                    }
                }
                #[cfg(windows)]
                windows::shutdown();
                // These auxiliary windows belong to main, not an independent UI.
                app.exit(0);
            });
        }
    });
}
fn current_id() -> Option<String> {
    let state = CONTROL.lock_or_recover();
    #[cfg(windows)]
    let pending = windows::stop_pending();
    #[cfg(not(windows))]
    let pending = false;
    (state.phase == Phase::Running || pending)
        .then(|| state.owner.as_ref().map(|owner| owner.id.clone()))
        .flatten()
}
pub(crate) async fn escape() {
    if let Some(id) = current_id() {
        let ended = {
            let state = CONTROL.lock_or_recover();
            (state.phase == Phase::Ended)
                .then(|| state.owner.clone())
                .flatten()
        };
        if let Some(owner) = ended {
            if let Err(error) = crate::native_desktop::release(
                &owner.process_id,
                Some(&owner.run_id),
                Some(&owner.window_id),
            )
            .await
            {
                tracing::error!(%error, "finished native control cleanup not confirmed");
            }
            return;
        }
        if let Err(error) = native_desktop_stop(id).await {
            tracing::error!(%error, "native Escape stop not confirmed");
        }
    }
}
fn publish() -> Result<(), String> {
    // Serialize before taking a snapshot: an older publisher must never apply
    // running window state or re-arm Escape after a newer stop acknowledgement.
    let _present = PRESENT.lock_or_recover();
    let state = CONTROL.lock_or_recover().snapshot();
    let app = APP.get().ok_or("原生控制界面未初始化")?;
    #[cfg(windows)]
    windows::present(app, &state)?;
    app.emit("kz:native-control", state)
        .map_err(|error| error.to_string())
}
pub(crate) async fn begin(ctx: &ToolCtx, window_id: &str, title: &str) -> Result<Lease, String> {
    PLATFORM
        .get()
        .ok_or("原生控制界面未初始化")?
        .as_ref()
        .map_err(Clone::clone)?;
    #[cfg(windows)]
    windows::ready().await?;
    let owner = Owner {
        id: crate::native_desktop::identity(),
        project_dir: ctx.project_root.clone(),
        process_id: ctx.process_id.clone().ok_or("原生控制需要对话归属")?,
        session_id: ctx.session_id.clone().ok_or("原生控制需要 session 归属")?,
        run_id: ctx.run_id.clone().ok_or("原生控制需要 run 归属")?,
        window_id: window_id.into(),
        title: title.into(),
    };
    let mut state = CONTROL.lock_or_recover();
    if APP.get().and_then(|app| app.get_window("main")).is_none() {
        return Err("主窗口已关闭，未取得原生控制权".into());
    }
    #[cfg(windows)]
    if windows::stop_pending() && state.phase != Phase::Stopped {
        return Err("已收到 Esc，正在停止原生控制".into());
    }
    let fresh = state.phase != Phase::Running;
    // Revalidate while holding the control lock. A stop may have revoked the
    // selection while this invocation was waiting for its surfaces to load.
    if crate::native_desktop::selected(Some(&owner.process_id))
        .is_none_or(|selection| selection["id"] != owner.window_id)
    {
        return Err("原生窗口能力已撤销，未取得控制权".into());
    }
    let lease = state.claim(owner)?;
    drop(state);
    #[cfg(windows)]
    if fresh {
        windows::new_lease();
    }
    #[cfg(not(windows))]
    let _ = fresh;
    if let Err(error) = publish() {
        let _ = cancel(&lease.id);
        return Err(error);
    }
    Ok(lease)
}
pub(crate) fn start_action(
    lease: &Lease,
    action: &str,
    target: &Value,
    point: Option<Value>,
) -> Result<u64, String> {
    #[cfg(windows)]
    if windows::stop_pending() {
        return Err("已收到 Esc，未发送后续动作".into());
    }
    let mut state = CONTROL.lock_or_recover();
    if state.phase != Phase::Running
        || lease.cancel.is_cancelled()
        || !state
            .owner
            .as_ref()
            .is_some_and(|owner| owner.id == lease.id)
    {
        return Err("原生控制已停止，未发送后续动作".into());
    }
    state.revision += 1;
    let id = state.revision;
    state.trace.push_back(Action {
        id,
        action: action.into(),
        target: target["name"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(120)
            .collect(),
        status: "executing".into(),
        bounds: target.get("bounds").cloned(),
        point,
        started_at: chrono::Utc::now().timestamp_millis(),
        ended_at: None,
    });
    while state.trace.len() > TRACE_LIMIT {
        state.trace.pop_front();
    }
    drop(state);
    publish()?;
    Ok(id)
}
pub(crate) fn finish_action(lease: &Lease, id: u64, status: &str) {
    let mut state = CONTROL.lock_or_recover();
    if !state
        .owner
        .as_ref()
        .is_some_and(|owner| owner.id == lease.id)
    {
        return;
    }
    if let Some(action) = state.trace.iter_mut().find(|action| action.id == id) {
        action.status = status.into();
        action.ended_at = Some(chrono::Utc::now().timestamp_millis());
    }
    state.revision += 1;
    drop(state);
    if let Err(error) = publish() {
        tracing::error!(%error, "native action display failed");
    }
}
pub(crate) fn helper_pid(lease: &Lease, pid: Option<u32>) {
    let mut state = CONTROL.lock_or_recover();
    if state
        .owner
        .as_ref()
        .is_some_and(|owner| owner.id == lease.id)
    {
        state.helper_pid = pid;
        state.revision += 1;
    }
}
pub(crate) fn observed_bounds(lease: &Lease, id: u64, bounds: Value) {
    let mut state = CONTROL.lock_or_recover();
    if state
        .owner
        .as_ref()
        .is_some_and(|owner| owner.id == lease.id)
    {
        if let Some(action) = state.trace.iter_mut().find(|action| action.id == id) {
            action.bounds = Some(bounds);
        }
    }
}
pub(crate) fn cancel(id: &str) -> Result<Owner, String> {
    let owner = CONTROL.lock_or_recover().cancel(id)?;
    revoke_and_publish(owner)
}
fn revoke_and_publish(owner: Owner) -> Result<Owner, String> {
    crate::native_desktop::revoke(&owner.process_id, &owner.window_id);
    publish()?;
    Ok(owner)
}
pub(crate) fn cancel_owner(
    process: &str,
    run: Option<&str>,
    window: Option<&str>,
) -> Result<Option<Owner>, String> {
    let owner = CONTROL
        .lock_or_recover()
        .cancel_owner(process, run, window)?;
    owner.map(revoke_and_publish).transpose()
}
pub(crate) fn confirm_stopped(id: &str) -> Result<(), String> {
    CONTROL.lock_or_recover().confirm(id)?;
    publish()
}
#[tauri::command]
pub(crate) fn native_desktop_status() -> Value {
    let mut value = CONTROL.lock_or_recover().snapshot();
    value["available"] = json!(PLATFORM.get().is_some_and(Result::is_ok));
    if let Some(Err(error)) = PLATFORM.get() {
        value["error"] = json!(error);
    }
    value
}
#[tauri::command]
pub(crate) fn native_desktop_surface_ready(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    return windows::surface_ready(window.label());
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("原生控制提示需要 Windows".into())
    }
}
#[tauri::command]
pub(crate) async fn native_desktop_stop(control_id: String) -> Result<Value, String> {
    let owner = CONTROL.lock_or_recover().request_stop(&control_id)?;
    let owner = revoke_and_publish(owner)?;
    let app = APP.get().ok_or("原生控制界面未初始化")?;
    let result = async {
        crate::runtime_service::stop_owned(
            app,
            owner.project_dir.clone(),
            owner.process_id.clone(),
        )
        .await?;
        // The GUI owns the actuator even if a restarted runtime no longer has
        // its registration. Confirm its exact lease locally after model stop.
        crate::native_desktop::release(
            &owner.process_id,
            Some(&owner.run_id),
            Some(&owner.window_id),
        )
        .await
    }
    .await;
    if let Err(error) = result {
        let mut state = CONTROL.lock_or_recover();
        if state
            .owner
            .as_ref()
            .is_some_and(|owner| owner.id == control_id)
        {
            state.phase = Phase::StopFailed;
            state.error = Some(error.clone());
            state.revision += 1;
        }
        drop(state);
        publish()?;
        return Err(format!("已撤销窗口能力，但任务停止尚未确认：{error}"));
    }
    // A command receipt describes this lease, not whichever controller may
    // have become current after it was released.
    Ok(json!({"control_id":control_id,"stopped":true}))
}
#[tauri::command]
pub(crate) fn native_desktop_dismiss(control_id: String) -> Result<(), String> {
    let mut state = CONTROL.lock_or_recover();
    if !state
        .owner
        .as_ref()
        .is_some_and(|owner| owner.id == control_id)
    {
        return Err("控制归属已变化".into());
    }
    if matches!(
        state.phase,
        Phase::Running | Phase::Stopping | Phase::StopFailed
    ) {
        return Err("控制尚未安全结束，不能隐藏提示".into());
    }
    state.phase = Phase::Idle;
    state.revision += 1;
    drop(state);
    publish()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner(id: &str, process: &str, run: &str) -> Owner {
        Owner {
            id: id.into(),
            project_dir: "project".into(),
            process_id: process.into(),
            session_id: process.into(),
            run_id: run.into(),
            window_id: id.into(),
            title: "form".into(),
        }
    }
    #[test]
    fn cancelled_control_cannot_resume_or_redirect_and_confirmation_waits_for_release() {
        let mut state = ControlState::default();
        let lease = state.claim(owner("first", "a", "run-a")).unwrap();
        assert!(state.cancel_owner("b", None, None).unwrap().is_none());
        assert!(state
            .cancel_owner("a", Some("other-run"), None)
            .unwrap()
            .is_none());
        assert!(state
            .cancel_owner("a", Some("run-a"), Some("other-window"))
            .unwrap()
            .is_none());
        assert!(!lease.cancel.is_cancelled());
        assert!(state.claim(owner("other", "b", "run-b")).is_err());
        state.cancel("first").unwrap();
        assert!(lease.cancel.is_cancelled());
        assert_eq!(state.snapshot()["phase"], "stopping");
        assert!(state.claim(owner("next", "a", "new-run")).is_err());
        state.helper_pid = Some(123);
        assert!(state.confirm("first").is_err());
        assert_eq!(state.snapshot()["phase"], "stopping");
        state.helper_pid = None;
        state.error = Some("prior stop not acknowledged".into());
        state.confirm("first").unwrap();
        assert_eq!(state.snapshot()["phase"], "stopped");
        assert!(state.error.is_none());
        assert!(state.request_stop("first").is_err());
        assert!(state.claim(owner("first", "a", "run-a")).is_err());
        state.claim(owner("next", "a", "new-run")).unwrap();
        assert!(state.cancel("first").is_err());
        state.confirm("first").unwrap();
        assert_eq!(state.snapshot()["phase"], "running");
        state.phase = Phase::Ended;
        assert!(state.request_stop("next").is_err());
    }
}
