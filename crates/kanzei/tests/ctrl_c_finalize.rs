//! D-085 E2:按 CLI 真实流程(开库→建会话→入队→提升→running→关库),
//! 模拟 Ctrl+C 分支调用 finalize_interrupt,再重开数据库验证终态。
//! 信号投递本身无法在测试基座可靠模拟(Windows 需共享控制台),
//! 但收尾入口与 CLI 的 ctrl_c 分支是同一函数。

use kanzei_core::{Delivery, SessionStore};

#[test]
fn 中断后重开数据库看到空闲态与取消的输入() {
    let dir = std::env::temp_dir().join(format!(
        "kanzei-d085-e2-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let state_path = dir.join("state.db");
    let session_id = "ses_cli_project";

    // 复刻 run_cli 的启动序列(crates/kanzei/src/main.rs:run_cli)。
    {
        let store = SessionStore::open(&state_path).unwrap();
        store.create_session(session_id, "C:/project", None).unwrap();
        store
            .admit_input(session_id, "input_1", "修复缺陷", Delivery::Queue)
            .unwrap();
        store.promote_next_queue(session_id).unwrap();
        store.set_status(session_id, "running").unwrap();
        store
            .append_event(
                session_id,
                "session.status_changed",
                &serde_json::json!({ "status": "running" }),
            )
            .unwrap();
    }

    // Ctrl+C 分支:进程被打断,重新开库收尾(与 main.rs ctrl_c 分支同一入口)。
    {
        let store = SessionStore::open(&state_path).unwrap();
        store.finalize_interrupt(session_id).unwrap();
    }

    // 桌面端视角:重开数据库,幽灵会话必须已消失。
    let store = SessionStore::open(&state_path).unwrap();
    let session = store.get_session(session_id).unwrap().unwrap();
    assert_eq!(session.status, "idle", "中断后不得残留 running");
    let event = store
        .latest_event(session_id, "session.status_changed")
        .unwrap()
        .unwrap();
    assert_eq!(event.payload["reason"], "stopped_by_user");
    assert!(store.list_pending_inputs(session_id).unwrap().is_empty());
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
