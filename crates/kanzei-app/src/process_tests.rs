//! 进程、会话运行时与停止收尾测试。

use super::{
    process_session_id, runtime_for, stop_runtime_and_finalize, take_pending_ask, AppState,
    PendingAsk, SessionRuntime,
};
// R-153 批4:default_process_id 已迁到 state 模块。
use crate::processes::{
    persist_process, restore_processes_from_store, restore_processes_from_store_once,
};
use crate::state::{default_process_id, ensure_default_process, process_info};
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
    store
        .create_session(session_id, &root.display().to_string(), None)
        .unwrap();
    store
        .admit_input(
            session_id,
            "promoted",
            "先执行",
            kanzei_core::Delivery::Queue,
        )
        .unwrap();
    store
        .admit_input(
            session_id,
            "pending",
            "后执行",
            kanzei_core::Delivery::Queue,
        )
        .unwrap();
    assert_eq!(
        store
            .promote_next_input(session_id)
            .unwrap()
            .unwrap()
            .input_id,
        "promoted"
    );
    let runtime = Arc::new(SessionRuntime::default());
    runtime.running.store(true, Ordering::SeqCst);
    assert_eq!(
        stop_runtime_and_finalize(&runtime, &store, &root.join("state.db"), session_id).unwrap(),
        2
    );
    assert!(!runtime.running.load(Ordering::SeqCst));
    assert!(store.list_pending_inputs(session_id).unwrap().is_empty());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

/// D-342:有活跃 run(令牌在槽、running=true)时,停止走**协作式**——令牌被
/// cancel、pending ask 被清、队列被取消,但 running 不立即翻 false、不立即 abort:
/// run 要在检查点自行收尾并走轮末写回,这正是「被打断轮对话消失」的修复本体。
#[test]
fn 协作式停止_置位令牌不立即终态化_队列仍即刻取消() {
    let root = std::env::temp_dir().join(format!(
        "kz-stop-graceful-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = kanzei_core::SessionStore::open(&root.join("state.db")).unwrap();
    let session_id = "session_stop_graceful";
    store
        .create_session(session_id, &root.display().to_string(), None)
        .unwrap();
    store
        .admit_input(session_id, "queued", "排队中", kanzei_core::Delivery::Queue)
        .unwrap();
    let runtime = Arc::new(SessionRuntime::default());
    runtime.running.store(true, Ordering::SeqCst);
    let token = kanzei_core::CancellationToken::new();
    *runtime.halt.lock().unwrap() = Some(token.clone());
    let (sender, _receiver) = oneshot::channel();
    runtime.asks.lock().unwrap().insert(
        1,
        PendingAsk {
            sender,
            request: kanzei_core::AskRequest::Permission {
                action: "bash".into(),
                resource: "cargo test".into(),
            },
            action: "bash".into(),
            resource: "cargo test".into(),
            project_root: root.clone(),
            session_id: session_id.into(),
        },
    );

    let cancelled =
        stop_runtime_and_finalize(&runtime, &store, &root.join("state.db"), session_id).unwrap();

    assert_eq!(cancelled, 1, "排队输入立即取消");
    assert!(token.is_cancelled(), "停止令牌必须被置位");
    assert!(runtime.halt.lock().unwrap().is_none(), "令牌被 stop 取走");
    assert!(runtime.asks.lock().unwrap().is_empty(), "pending ask 被清");
    assert_eq!(
        runtime.stop_watchdogs.lock().unwrap().len(),
        1,
        "协作式停止必须保留 watchdog 句柄，不能静默 detach"
    );
    assert!(
        runtime.running.load(Ordering::SeqCst),
        "协作式停止不立即翻 running——run 要自行收尾走轮末写回"
    );
    drop(store);
    std::fs::remove_dir_all(root).ok();
}

/// D-342:兜底硬杀判定——只有「代数未换 && 仍在运行」才硬杀;宽限期内新开的
/// run(代数 +1)不受上一次停止的兜底波及。
#[test]
fn 兜底硬杀只认停止时那一代() {
    use crate::state::stale_run_needs_abort;
    assert!(stale_run_needs_abort(7, 7, true), "同代且在跑:该硬杀");
    assert!(!stale_run_needs_abort(8, 7, true), "换代:新 run 不能被误杀");
    assert!(!stale_run_needs_abort(7, 7, false), "已收尾:无需硬杀");
}

#[test]
fn 结束的_stop_watchdog句柄可被回收() {
    use crate::state::reap_stop_watchdogs;
    let runtime = SessionRuntime::default();
    runtime
        .stop_watchdogs
        .lock()
        .unwrap()
        .push(std::thread::spawn(|| {}));
    std::thread::sleep(std::time::Duration::from_millis(10));
    reap_stop_watchdogs(&runtime);
    assert!(runtime.stop_watchdogs.lock().unwrap().is_empty());
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
    store
        .create_session(session_id, &root.display().to_string(), None)
        .unwrap();
    let runtime = Arc::new(SessionRuntime::default());
    runtime.running.store(true, Ordering::SeqCst);
    {
        let mut live = runtime.live.lock().unwrap();
        live.begin(
            "run_x",
            "input_x",
            "很长的一轮",
            "deepseek",
            "deepseek-v4-flash",
        );
        live.steps = 37;
        live.trace
            .push(serde_json::json!({"kind": "tool.completed", "name": "bash"}));
    }
    // 令牌槽为空(无活跃令牌)→ 停止走立即终态化路径,行为与 D-342 之前一致。
    stop_runtime_and_finalize(&runtime, &store, &root.join("state.db"), session_id).unwrap();
    let trace = store
        .latest_event(session_id, "run.trace")
        .unwrap()
        .unwrap();
    assert_eq!(trace.payload["outcome"], "halted");
    assert_eq!(store.list_episodes(session_id, 5).unwrap().len(), 1);
    stop_runtime_and_finalize(&runtime, &store, &root.join("state.db"), session_id).unwrap();
    assert_eq!(store.list_episodes(session_id, 5).unwrap().len(), 1);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn process_sessions_are_isolated_but_default_keeps_legacy_id() {
    let root = Path::new(r"C:\project");
    let default_id = default_process_id(root);
    assert_eq!(
        process_session_id(root, None),
        kanzei_core::project_session_id(root)
    );
    assert_eq!(
        process_session_id(root, Some(&default_id)),
        kanzei_core::project_session_id(root)
    );
    assert_ne!(
        process_session_id(root, Some("p1|C:\\project")),
        process_session_id(root, Some("p2|C:\\project"))
    );
}

/// 「勘察复核」(阶段流水线总闸)的默认值必须是**关**。
///
/// 2026-08-11 用户定调:「我如果显式打开子代理,应该每个任务强制触发」——默认开就
/// 不叫显式,而且默认开等于给每一轮无差别加 5 个勘察 + 3 个复核子代理的成本。
/// 这条同时钉住给前端的回显字段(`ProcessInfo.phase_pipeline`),它是界面勾选框的初值。
#[test]
fn 勘察复核开关默认关闭() {
    let state = AppState::default();
    let root = std::env::temp_dir().join(format!(
        "kz-pipeline-default-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let process = ensure_default_process(&state, &root);
    assert!(
        !process.phase_pipeline_enabled.load(Ordering::SeqCst),
        "默认进程的「勘察复核」必须默认关闭(要显式打开才强制走七阶段)"
    );
    assert!(
        process.subagents_enabled.load(Ordering::SeqCst),
        "默认进程的「子代理」必须默认开启"
    );
    assert!(
        !process_info(&state, &process).phase_pipeline,
        "回显给前端的默认值也必须是关,否则界面勾选框与真实闸门对不上"
    );
    assert!(process_info(&state, &process).subagents_enabled);
    std::fs::remove_dir_all(&root).ok();
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
    first.asks.lock().unwrap().insert(
        7,
        PendingAsk {
            sender,
            request: kanzei_core::AskRequest::Question {
                question: "继续?".into(),
                options: Vec::new(),
                default: None,
                multiple: false,
            },
            action: "question".into(),
            resource: "继续?".into(),
            project_root: "project-a".into(),
            session_id: "ses_a".into(),
        },
    );
    assert_eq!(take_pending_ask(&state, 7).unwrap().session_id, "ses_a");
    assert!(take_pending_ask(&state, 7).is_none());
    assert!(second.asks.lock().unwrap().is_empty());
}

/// R-178 D3:进程落库 → 模拟重启 → 从库恢复,模型/profile/reasoning/勘察复核
/// 完整回填,页签(线)重新出现。验收①的核心往返。
#[test]
fn process_persist_then_restart_restores_line_state() {
    let root = std::env::temp_dir().join(format!(
        "kz-process-persist-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let canonical = crate::normalized_project_root(&root);

    // ---- 第一次运行:一条非默认线,线级字段已设置,落库 ----
    let line = crate::state::ProcessHandle {
        id: format!("p1|{}", canonical.display()),
        origin_project: crate::ProjectRoot(canonical.clone()),
        project_dir: crate::ProjectRoot(canonical.clone()),
        worktree_path: None,
        branch: None,
        model: Arc::new(std::sync::Mutex::new(Some(
            "deepseek:deepseek-v4-flash".into(),
        ))),
        profile: Arc::new(std::sync::Mutex::new(Some("dev".into()))),
        research_topic: Arc::new(std::sync::Mutex::new(None)),
        reasoning: Arc::new(std::sync::Mutex::new(Some("high".into()))),
        manual_models: Arc::new(std::sync::Mutex::new(Vec::new())),
        phase_pipeline_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        subagents_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        tracker_writes_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    };
    persist_process(&canonical, &line).unwrap();

    // ---- 模拟重启:全新 AppState(空内存),从 state.db 恢复 ----
    let restarted = AppState::default();
    restore_processes_from_store(&restarted, &canonical).unwrap();
    let processes = restarted.processes.lock().unwrap();
    let restored = processes.get(&line.id).expect("重启后线页签丢失");
    assert_eq!(restored.origin_project.0, canonical);
    assert_eq!(
        restored.model.lock().unwrap().as_deref(),
        Some("deepseek:deepseek-v4-flash")
    );
    assert_eq!(restored.profile.lock().unwrap().as_deref(), Some("dev"));
    assert_eq!(restored.reasoning.lock().unwrap().as_deref(), Some("high"));
    assert!(restored.phase_pipeline_enabled.load(Ordering::SeqCst));
    assert!(restored.subagents_enabled.load(Ordering::SeqCst));
    assert!(restored.tracker_writes_enabled.load(Ordering::SeqCst));
    drop(processes);

    // ---- 默认进程(id 相同)的字段也能回填,不复建存在性 ----
    let restarted2 = AppState::default();
    let default = ensure_default_process(&restarted2, &canonical);
    restore_processes_from_store(&restarted2, &canonical).unwrap();
    let processes2 = restarted2.processes.lock().unwrap();
    assert!(processes2.get(&line.id).is_some(), "线页签必须恢复");
    assert!(
        processes2.get(&default.id).is_some(),
        "默认进程存在性由 ensure 保证"
    );
    drop(processes2);
    // 默认进程本身的字段也持久化:更新后重启可见。
    *default.model.lock().unwrap() = Some("anthropic:claude-sonnet-5".into());
    persist_process(&canonical, &default).unwrap();
    let restarted3 = AppState::default();
    restore_processes_from_store(&restarted3, &canonical).unwrap();
    let restored_default = restarted3
        .processes
        .lock()
        .unwrap()
        .get(&default.id)
        .unwrap()
        .clone();
    assert_eq!(
        restored_default.model.lock().unwrap().as_deref(),
        Some("anthropic:claude-sonnet-5")
    );

    std::fs::remove_dir_all(&root).ok();
}

/// 状态轮询是只读采样：同一运行期重复进入 process_list/collaboration_snapshot
/// 不得把 state.db 的旧快照覆盖掉用户刚改的内存设置。
#[test]
fn repeated_process_restore_does_not_overwrite_live_settings() {
    let root = std::env::temp_dir().join(format!(
        "kz-process-refresh-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let canonical = crate::normalized_project_root(&root);
    let stored = crate::state::ProcessHandle {
        id: format!("p1|{}", canonical.display()),
        origin_project: crate::ProjectRoot(canonical.clone()),
        project_dir: crate::ProjectRoot(canonical.clone()),
        worktree_path: None,
        branch: None,
        model: Arc::new(std::sync::Mutex::new(Some("old-model".into()))),
        profile: Arc::new(std::sync::Mutex::new(Some("dev".into()))),
        research_topic: Arc::new(std::sync::Mutex::new(None)),
        reasoning: Arc::new(std::sync::Mutex::new(Some("medium".into()))),
        manual_models: Arc::new(std::sync::Mutex::new(Vec::new())),
        phase_pipeline_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        subagents_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        tracker_writes_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    persist_process(&canonical, &stored).unwrap();

    let state = AppState::default();
    restore_processes_from_store_once(&state, &canonical).unwrap();
    let live = state
        .processes
        .lock()
        .unwrap()
        .get(&stored.id)
        .unwrap()
        .clone();
    *live.model.lock().unwrap() = Some("new-model".into());
    *live.reasoning.lock().unwrap() = Some("high".into());

    restore_processes_from_store_once(&state, &canonical).unwrap();
    assert_eq!(live.model.lock().unwrap().as_deref(), Some("new-model"));
    assert_eq!(live.reasoning.lock().unwrap().as_deref(), Some("high"));
    std::fs::remove_dir_all(&root).ok();
}

/// R-178 D3:线级状态按主项目隔离——A 项目的线不会在 B 项目恢复(D-170 式)。
#[test]
fn process_restore_is_isolated_per_project() {
    let root_a = std::env::temp_dir().join(format!(
        "kz-proj-a-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root_b = std::env::temp_dir().join(format!(
        "kz-proj-b-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    let canonical_a = crate::normalized_project_root(&root_a);
    let canonical_b = crate::normalized_project_root(&root_b);

    // A 项目写一条线。
    let line = crate::state::ProcessHandle {
        id: format!("p1|{}", canonical_a.display()),
        origin_project: crate::ProjectRoot(canonical_a.clone()),
        project_dir: crate::ProjectRoot(canonical_a.clone()),
        worktree_path: None,
        branch: None,
        model: Arc::new(std::sync::Mutex::new(Some(
            "deepseek:deepseek-v4-flash".into(),
        ))),
        profile: Arc::new(std::sync::Mutex::new(None)),
        research_topic: Arc::new(std::sync::Mutex::new(None)),
        reasoning: Arc::new(std::sync::Mutex::new(None)),
        manual_models: Arc::new(std::sync::Mutex::new(Vec::new())),
        phase_pipeline_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        subagents_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        tracker_writes_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    persist_process(&canonical_a, &line).unwrap();

    // B 项目恢复时看不到 A 的线。
    let restarted_b = AppState::default();
    restore_processes_from_store(&restarted_b, &canonical_b).unwrap();
    assert!(restarted_b
        .processes
        .lock()
        .unwrap()
        .get(&line.id)
        .is_none());

    std::fs::remove_dir_all(&root_a).ok();
    std::fs::remove_dir_all(&root_b).ok();
}
