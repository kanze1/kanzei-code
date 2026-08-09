//! 权限请求与 Always Allow 持久化测试。

use super::{pending_ask_payload, PendingAsk};
// R-153 批10:总是允许的落库迁到 run 模块。
use crate::run::persist_always_allow;
use tokio::sync::oneshot;

#[test]
fn pending_ask_payload_can_rebuild_permission_dialog() {
    let (sender, _receiver) = oneshot::channel();
    let pending = PendingAsk {
        sender,
        request: kanzei_core::AskRequest::Permission { action: "bash".into(), resource: "{\"command\":\"echo x\",\"workdir\":\"C:/project\"}".into() },
        action: "bash".into(),
        resource: "{\"command\":\"echo x\",\"workdir\":\"C:/project\"}".into(),
        project_root: "C:/project".into(),
        session_id: "session#p2".into(),
    };
    let payload = pending_ask_payload(7, &pending);
    assert_eq!(payload["id"], 7);
    assert_eq!(payload["kind"], "permission");
    assert_eq!(payload["sessionId"], "session#p2");
    assert_eq!(payload["action"], "bash");
}

#[test]
fn persist_always_allow_success_returns_always_allow_and_path() {
    let root = std::env::temp_dir().join(format!("kanzei-app-always-ok-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(root.join(".kanzei")).unwrap();
    let (reply, path) = persist_always_allow(&root, "bash", "git status").unwrap();
    assert_eq!(reply, kanzei_core::AskReply::AlwaysAllow);
    assert_eq!(path, root.join(".kanzei/kanzei.toml"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn persist_always_allow_failure_returns_deny_path() {
    let root = std::env::temp_dir().join(format!("kanzei-app-always-fail-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(root.join(".kanzei")).unwrap();
    std::fs::write(root.join(".kanzei/kanzei.toml"), "[invalid\n").unwrap();
    assert!(persist_always_allow(&root, "bash", "git status").is_err());
    std::fs::remove_dir_all(root).unwrap();
}
