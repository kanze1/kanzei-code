//! 进程、会话运行时与停止收尾测试。

use super::{process_session_id, runtime_for, stop_runtime_and_finalize, take_pending_ask, AppState, PendingAsk, SessionRuntime};
// R-153 批4:default_process_id 已迁到 state 模块。
use crate::state::default_process_id;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::oneshot;

#[test]
fn stopping_after_promote_cancels_promoted_and_pending_inputs_atomically() {
    let root = std::env::temp_dir().join(format!(
        "kanzei-app-stop-promoted-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = kanzei_core::SessionStore::open(&root.join("state.db")).unwrap();
    let session_id = "session_stop_promoted";
    store.create_session(session_id, &root.display().to_string(), None).unwrap();
    store.admit_input(session_id, "promoted", "先执行", kanzei_core::Delivery::Queue).unwrap();
    store.admit_input(session_id, "pending", "后执行", kanzei_core::Delivery::Queue).unwrap();
    assert_eq!(store.promote_next_input(session_id).unwrap().unwrap().input_id, "promoted");
    let runtime = SessionRuntime::default();
    runtime.running.store(true, Ordering::SeqCst);
    assert_eq!(stop_runtime_and_finalize(&runtime, &store, session_id).unwrap(), 2);
    assert!(!runtime.running.load(Ordering::SeqCst));
    assert!(store.list_pending_inputs(session_id).unwrap().is_empty());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn 停止时在飞轨迹与episode先落库再abort() {
    let root = std::env::temp_dir().join(format!(
        "kz-stop-flush-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = kanzei_core::SessionStore::open(&root.join("state.db")).unwrap();
    let session_id = "session_stop_flush";
    store.create_session(session_id, &root.display().to_string(), None).unwrap();
    let runtime = SessionRuntime::default();
    runtime.running.store(true, Ordering::SeqCst);
    {
        let mut live = runtime.live.lock().unwrap();
        live.begin("run_x", "input_x", "很长的一轮", "deepseek", "deepseek-v4-flash");
        live.steps = 37;
        live.trace.push(serde_json::json!({"kind": "tool.completed", "name": "bash"}));
    }
    stop_runtime_and_finalize(&runtime, &store, session_id).unwrap();
    let trace = store.latest_event(session_id, "run.trace").unwrap().unwrap();
    assert_eq!(trace.payload["outcome"], "halted");
    assert_eq!(store.list_episodes(session_id, 5).unwrap().len(), 1);
    stop_runtime_and_finalize(&runtime, &store, session_id).unwrap();
    assert_eq!(store.list_episodes(session_id, 5).unwrap().len(), 1);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn process_sessions_are_isolated_but_default_keeps_legacy_id() {
    let root = Path::new(r"C:\project");
    let default_id = default_process_id(root);
    assert_eq!(process_session_id(root, None), kanzei_core::project_session_id(root));
    assert_eq!(process_session_id(root, Some(&default_id)), kanzei_core::project_session_id(root));
    assert_ne!(process_session_id(root, Some("p1|C:\\project")), process_session_id(root, Some("p2|C:\\project")));
}

#[test]
fn session_runtime_is_reused_per_session_and_isolated_between_sessions() {
    let state = AppState::default();
    let first = runtime_for(&state, "ses_a");
    assert!(Arc::ptr_eq(&first, &runtime_for(&state, "ses_a")));
    assert!(!Arc::ptr_eq(&first, &runtime_for(&state, "ses_b")));
}

#[test]
fn pending_ask_lookup_stays_with_its_runtime_container() {
    let state = AppState::default();
    let first = runtime_for(&state, "ses_a");
    let second = runtime_for(&state, "ses_b");
    let (sender, _receiver) = oneshot::channel();
    first.asks.lock().unwrap().insert(7, PendingAsk {
        sender,
        request: kanzei_core::AskRequest::Question { question: "继续?".into(), options: Vec::new(), default: None },
        action: "question".into(), resource: "继续?".into(),
        project_root: "project-a".into(), session_id: "ses_a".into(),
    });
    assert_eq!(take_pending_ask(&state, 7).unwrap().session_id, "ses_a");
    assert!(take_pending_ask(&state, 7).is_none());
    assert!(second.asks.lock().unwrap().is_empty());
}
