//! 权限请求与 Always Allow 持久化测试。

use super::{pending_ask_payload, PendingAsk};
// R-153 批10:总是允许的落库迁到 run 模块。
use crate::commands::run::persist_always_allow;
use tokio::sync::oneshot;

#[test]
fn pending_ask_payload_can_rebuild_permission_dialog() {
    let (sender, _receiver) = oneshot::channel();
    let mut pending = PendingAsk {
        source: String::new(),
        agent_id: None,
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
    pending.source = "检查导出文件".into();
    pending.agent_id = Some("child-export".into());
    let restored = pending_ask_payload(7, &pending);
    assert_eq!(restored["source"], "检查导出文件");
    assert_eq!(restored["agentId"], "child-export");
    assert_eq!(restored["sessionId"], payload["sessionId"]);
    assert_eq!(restored["resource"], payload["resource"]);
}

#[test]
fn pending_ask_payload_carries_question_multiple() {
    // D-337:question 档位的 multiple 必须经 pending_ask_payload 透传,否则重启恢复
    // 或切换会话重弹时,多选档位会静默退化成"点一个即提交"。
    let (sender, _receiver) = oneshot::channel();
    let pending = PendingAsk {
        source: String::new(),
        agent_id: None,
        sender,
        request: kanzei_core::AskRequest::Question {
            question: "哪个?".into(),
            options: vec!["甲".into(), "乙".into()],
            default: None,
            multiple: true,
            background: false,
            callback_id: None,
        },
        action: "question".into(),
        resource: "哪个?".into(),
        project_root: "C:/project".into(),
        session_id: "session#q1".into(),
    };
    let payload = pending_ask_payload(9, &pending);
    assert_eq!(payload["kind"], "question");
    assert_eq!(payload["multiple"], true);
    // R-328:选项上线为 {label, note?} 对象。桌面 UI 两种形态都吃(历史事件重放
    // 里仍是裸字符串);移动 PWA 不渲染选项,不受影响。
    assert_eq!(payload["options"][0]["label"], "甲");
    assert!(
        payload["options"][0].get("note").is_none(),
        "无注解时不该发出空 note 字段"
    );
    assert_eq!(payload["sessionId"], "session#q1");

    // 默认档位(未声明)透传 false,不把历史问题误判成多选。
    let (sender, _receiver) = oneshot::channel();
    let single = PendingAsk {
        source: String::new(),
        agent_id: None,
        sender,
        request: kanzei_core::AskRequest::Question {
            question: "哪个?".into(),
            options: vec!["甲".into()],
            default: None,
            multiple: false,
            background: false,
            callback_id: None,
        },
        action: "question".into(),
        resource: "哪个?".into(),
        project_root: "C:/project".into(),
        session_id: "session#q2".into(),
    };
    assert_eq!(pending_ask_payload(10, &single)["multiple"], false);
}

#[test]
fn permanent_permissions_live_in_user_registry_and_failed_storage_does_not_grant() {
    kanzei_base::test_support::isolated_test(
        module_path!(),
        "permanent_permissions_live_in_user_registry_and_failed_storage_does_not_grant",
        || {
            let root = std::env::temp_dir().join(format!(
                "kz-app-permissions-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let home = root.join("user-home");
            crate::settings::with_kanzei_home(&home, || {
                let (reply, path) = persist_always_allow(&root, "bash", "git status").unwrap();
                assert_eq!(reply, kanzei_core::AskReply::AlwaysAllow);
                assert_eq!(path, home.join("project-trust.json"));
                let dir = root.display().to_string();
                crate::commands::run::permission_rule_add(
                    dir.clone(),
                    "bash".into(),
                    "git log".into(),
                )
                .unwrap();
                let rules = crate::settings::permission_rules_get(dir.clone()).unwrap();
                assert_eq!(rules["rules"][0]["resource"], "git status");
                assert_eq!(rules["rules"][1]["resource"], "git log");
                assert!(!root.join(".kanzei/kanzei.toml").exists());
                assert!(crate::commands::run::permission_rule_add(
                    dir.clone(),
                    " ".into(),
                    "x".into()
                )
                .is_err());
                assert!(
                    crate::commands::run::permission_rule_add(dir, "bash".into(), "".into())
                        .is_err()
                );
                std::fs::write(path, "invalid JSON").unwrap();
                assert!(persist_always_allow(&root, "bash", "git diff").is_err());
            });
            std::fs::remove_dir_all(root).unwrap();
        },
    );
}
