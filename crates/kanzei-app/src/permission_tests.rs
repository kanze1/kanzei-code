//! 权限请求与 Always Allow 持久化测试。

use super::{pending_ask_payload, PendingAsk};
// R-153 批10:总是允许的落库迁到 run 模块。
use crate::run::assembly::build_run_harness;
use crate::run::persist_always_allow;
use kanzei_harness::{Effect, KanzeiConfig, ProfileKind, ResolveCtx, Rule};
use std::sync::Arc;
use tokio::sync::oneshot;

#[test]
fn pending_ask_payload_can_rebuild_permission_dialog() {
    let (sender, _receiver) = oneshot::channel();
    let pending = PendingAsk {
        sender,
        request: kanzei_core::AskRequest::Permission {
            action: "bash".into(),
            resource: "{\"command\":\"echo x\",\"workdir\":\"C:/project\"}".into(),
        },
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
fn pending_ask_payload_carries_question_multiple() {
    // D-337:question 档位的 multiple 必须经 pending_ask_payload 透传,否则重启恢复
    // 或切换会话重弹时,多选档位会静默退化成"点一个即提交"。
    let (sender, _receiver) = oneshot::channel();
    let pending = PendingAsk {
        sender,
        request: kanzei_core::AskRequest::Question {
            question: "哪个?".into(),
            options: vec!["甲".into(), "乙".into()],
            default: None,
            multiple: true,
        },
        action: "question".into(),
        resource: "哪个?".into(),
        project_root: "C:/project".into(),
        session_id: "session#q1".into(),
    };
    let payload = pending_ask_payload(9, &pending);
    assert_eq!(payload["kind"], "question");
    assert_eq!(payload["multiple"], true);
    assert_eq!(payload["options"][0], "甲");
    assert_eq!(payload["sessionId"], "session#q1");

    // 默认档位(未声明)透传 false,不把历史问题误判成多选。
    let (sender, _receiver) = oneshot::channel();
    let single = PendingAsk {
        sender,
        request: kanzei_core::AskRequest::Question {
            question: "哪个?".into(),
            options: vec!["甲".into()],
            default: None,
            multiple: false,
        },
        action: "question".into(),
        resource: "哪个?".into(),
        project_root: "C:/project".into(),
        session_id: "session#q2".into(),
    };
    assert_eq!(pending_ask_payload(10, &single)["multiple"], false);
}

#[test]
fn persist_always_allow_success_returns_always_allow_and_path() {
    let root = std::env::temp_dir().join(format!(
        "kanzei-app-always-ok-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".kanzei")).unwrap();
    let (reply, path) = persist_always_allow(&root, "bash", "git status").unwrap();
    assert_eq!(reply, kanzei_core::AskReply::AlwaysAllow);
    assert_eq!(path, root.join(".kanzei/kanzei.toml"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn persist_always_allow_failure_returns_deny_path() {
    let root = std::env::temp_dir().join(format!(
        "kanzei-app-always-fail-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".kanzei")).unwrap();
    std::fs::write(root.join(".kanzei/kanzei.toml"), "[invalid\n").unwrap();
    assert!(persist_always_allow(&root, "bash", "git status").is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn branch_tracker_switch_off_keeps_reads_and_rejects_writes_with_reason() {
    let root = std::env::temp_dir().join(format!(
        "kz-tracker-policy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".kanzei")).unwrap();
    let mut config = KanzeiConfig::default();
    // 用户通用配置即使放行 tracker,也不能越过当前分支线自己的显式关闭开关。
    config.permissions.rules.push(Rule {
        action: "req".into(),
        resource: "write:*".into(),
        effect: Effect::Allow,
    });
    let ctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: root.clone(),
        project_root: root.clone(),
        config: Arc::new(config),
    };
    let snapshot = build_run_harness(true, None).resolve(&ctx).unwrap();
    let names = snapshot
        .materialize_tools()
        .iter()
        .map(|tool| tool.name())
        .collect::<Vec<_>>();
    assert!(names.contains(&"req"), "只禁写不能摘掉整个 req 工具");
    assert_eq!(snapshot.evaluate("req", "write:add"), Effect::Deny);
    assert_eq!(snapshot.evaluate("req", "read:list"), Effect::Ask);
    let hint = snapshot.denial_hint("req", "write:add");
    assert!(hint.contains("未开启 tracker 写入"), "{hint}");

    let enabled = build_run_harness(false, None).resolve(&ctx).unwrap();
    assert_eq!(enabled.evaluate("req", "write:add"), Effect::Allow);
    std::fs::remove_dir_all(root).unwrap();
}
