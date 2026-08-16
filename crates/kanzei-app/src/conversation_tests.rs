//! 会话内容、历史恢复与消息负载测试。

use super::{prompt_attachment_parts, with_session_id, PromptAttachment};
// R-153 批10:会话恢复相关已迁到 conversation 模块。
use crate::conversation::{conversation_prior, recover_messages_at, recover_messages_raw};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// R-242 批6/7:多个测试通过 KANZEI_PROJECTION_GATES 环境变量控制 gate,并行
/// 运行会互相覆盖同一 env 键——所有 env 相关测试共享这把锁串行执行。
static GATE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn prompt_attachments_become_image_and_document_parts() {
    let parts = prompt_attachment_parts(vec![
        PromptAttachment {
            file_name: "screen.png".into(),
            media_type: "image/png".into(),
            data: "PNGDATA".into(),
        },
        PromptAttachment {
            file_name: "notes.pdf".into(),
            media_type: "application/pdf".into(),
            data: "PDFDATA".into(),
        },
    ])
    .unwrap();
    assert!(
        matches!(&parts[0], kanzei_llm::Part::Image { media_type, data } if media_type == "image/png" && data == "PNGDATA")
    );
    assert!(
        matches!(&parts[1], kanzei_llm::Part::Document { media_type, data } if media_type == "application/pdf" && data == "PDFDATA")
    );
}

#[test]
fn session_id_is_added_to_event_payload() {
    let payload = with_session_id(serde_json::json!({"text": "hello"}), "ses_test#p2");
    assert_eq!(payload["sessionId"], "ses_test#p2");
    assert_eq!(payload["text"], "hello");
}

#[test]
fn session_id_does_not_change_non_object_payload() {
    assert_eq!(
        with_session_id(serde_json::Value::Null, "ses_test"),
        serde_json::Value::Null
    );
}

#[test]
fn conversation_prior_prefers_existing_memory_over_persisted_snapshot() {
    let conversation = Arc::new(Mutex::new(HashMap::new()));
    let persisted = vec![kanzei_llm::Message::user_text("恢复快照")];
    assert_eq!(
        conversation_prior(&conversation, "ses", persisted.clone())[0].parts,
        persisted[0].parts
    );
    let existing = vec![kanzei_llm::Message::user_text("内存旧快照")];
    conversation
        .lock()
        .unwrap()
        .insert("ses".into(), existing.clone());
    assert_eq!(
        conversation_prior(
            &conversation,
            "ses",
            vec![kanzei_llm::Message::user_text("最新持久化")]
        )[0]
        .parts,
        existing[0].parts
    );
}

#[test]
fn recover_messages_filters_orphan_tool_calls_from_persisted_snapshot() {
    let root = std::env::temp_dir().join(format!(
        "kanzei-app-history-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let store = kanzei_core::SessionStore::open(&root.join("state.db")).unwrap();
    store
        .create_session("ses_history", &root.display().to_string(), None)
        .unwrap();
    let messages = vec![
        kanzei_llm::Message::user_text("保留文本"),
        kanzei_llm::Message::assistant(vec![kanzei_llm::Part::ToolCall {
            id: "orphan".into(),
            name: "bash".into(),
            input: serde_json::json!({"command": "echo orphan"}),
        }]),
    ];
    store
        .append_event(
            "ses_history",
            "conversation.updated",
            &serde_json::json!({"messages": messages}),
        )
        .unwrap();
    let recovered = recover_messages_at(&store, "ses_history", None).unwrap();
    assert_eq!(recovered.len(), 1);
    let raw = recover_messages_raw(&store, "ses_history", None).unwrap();
    assert!(
        raw.iter().map(|m| m.parts.len()).sum::<usize>()
            > recovered.iter().map(|m| m.parts.len()).sum::<usize>()
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn conversation_get_gate_controls_projection_vs_legacy() {
    // R-242 批6:conversation_get 在 gate 开启(白名单含该路径)时从事件投影
    // surface,关闭(白名单剔除)时回退 legacy 快照,行为与切换前一致(验收⑥)。
    let _gate_guard = GATE_ENV_LOCK.lock().unwrap();
    use kanzei_core::{SessionFact, SessionFactEnvelope, SessionInvariant, SessionStore};
    use kanzei_llm::{Message, Part};

    let root = std::env::temp_dir().join(format!(
        "kanzei-app-gate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let canonical = crate::normalized_project_root(&root);
    let session_id = crate::process_session_id(&canonical, None);
    let state_path = kanzei_core::project_state_path(&canonical);
    {
        let store = SessionStore::open(&state_path).unwrap();
        store
            .create_session(&session_id, &canonical.display().to_string(), None)
            .unwrap();
        // legacy 快照:旧历史(轮末 conversation.updated)。
        store
            .append_event(
                &session_id,
                "conversation.updated",
                &serde_json::json!({ "messages": [Message::user_text("旧历史")] }),
            )
            .unwrap();
        // typed facts:seed 旧快照 + 新轮消息。
        kanzei_core::prepare_typed_session(&store, &session_id).unwrap();
        let mut invariant = SessionInvariant::default();
        let assistant = Message::assistant(vec![Part::Text {
            text: "新回答".into(),
        }]);
        let facts = [
            SessionFactEnvelope::new(
                "run-gate-test",
                None,
                SessionFact::UserMessageCommitted {
                    input_id: "i".into(),
                    message: Message::user_text("新提问"),
                },
            ),
            SessionFactEnvelope::new(
                "run-gate-test",
                Some(1),
                SessionFact::TurnStarted { max_steps: 1 },
            ),
            SessionFactEnvelope::new(
                "run-gate-test",
                Some(1),
                SessionFact::AssistantMessageCommitted {
                    message_id: "m".into(),
                    content_hash: kanzei_core::store::stable_message_hash(&assistant),
                    message: assistant,
                },
            ),
            SessionFactEnvelope::new("run-gate-test", None, SessionFact::TurnCompleted),
        ];
        store
            .append_session_facts_checked(&session_id, &mut invariant, &facts)
            .unwrap();
    }

    // gate 关:白名单不含 conversation_get → legacy 快照(仅旧历史)。
    std::env::set_var("KANZEI_PROJECTION_GATES", "runner_prior");
    let legacy =
        crate::conversation::conversation_get(canonical.display().to_string(), None, None).unwrap();
    assert_eq!(legacy.len(), 1, "legacy 快照只有旧历史");
    assert_eq!(
        legacy[0].parts[0],
        Part::Text {
            text: "旧历史".into()
        }
    );

    // gate 开:白名单含 conversation_get → 事件投影(seed 旧历史 + 新轮消息)。
    std::env::set_var("KANZEI_PROJECTION_GATES", "conversation_get");
    let projected =
        crate::conversation::conversation_get(canonical.display().to_string(), None, None).unwrap();
    let texts: Vec<String> = projected
        .iter()
        .filter_map(|m| match &m.parts[0] {
            Part::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["旧历史", "新提问", "新回答"]);
    std::env::remove_var("KANZEI_PROJECTION_GATES");

    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn project_latest_segment_resets_to_new_empty_segment() {
    // R-242 批7(验收④):conversation.reset 后新 segment prior 为空(仅含
    // reset 之后的事实);旧段事实保留在事件日志不进入新段 surface;重复 reset
    // 幂等(连续边界间无事实 → 空,不报错)。
    use kanzei_core::{SessionFact, SessionFactEnvelope, SessionInvariant, SessionStore};
    use kanzei_llm::{Message, Part};

    let root = std::env::temp_dir().join(format!(
        "kanzei-app-segment-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let canonical = crate::normalized_project_root(&root);
    let session_id = crate::process_session_id(&canonical, None);
    let store = SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
    store
        .create_session(&session_id, &canonical.display().to_string(), None)
        .unwrap();
    let mut invariant = SessionInvariant::default();
    let mut write_turn = |store: &SessionStore, turn: &str, question: &str, answer: &str| {
        let assistant = Message::assistant(vec![Part::Text {
            text: answer.into(),
        }]);
        let facts = [
            SessionFactEnvelope::new(
                turn,
                None,
                SessionFact::UserMessageCommitted {
                    input_id: format!("i-{turn}"),
                    message: Message::user_text(question),
                },
            ),
            SessionFactEnvelope::new(turn, Some(1), SessionFact::TurnStarted { max_steps: 1 }),
            SessionFactEnvelope::new(
                turn,
                Some(1),
                SessionFact::AssistantMessageCommitted {
                    message_id: format!("m-{turn}"),
                    content_hash: kanzei_core::store::stable_message_hash(&assistant),
                    message: assistant,
                },
            ),
            SessionFactEnvelope::new(turn, None, SessionFact::TurnCompleted),
        ];
        store
            .append_session_facts_checked(&session_id, &mut invariant, &facts)
            .unwrap();
    };
    // 旧段:reset 前的一轮完整对话。
    write_turn(&store, "run-old", "旧问题", "旧回答");
    // 清空:追加 conversation.reset 开启新段。
    store
        .append_event(
            &session_id,
            "conversation.reset",
            &serde_json::json!({ "cleared": true }),
        )
        .unwrap();
    // 新段:reset 后的一轮完整对话。
    write_turn(&store, "run-new", "新问题", "新回答");
    // 重复 reset(连续边界,新段内无新事实)——幂等,不报错。
    store
        .append_event(
            &session_id,
            "conversation.reset",
            &serde_json::json!({ "cleared": true }),
        )
        .unwrap();

    // 最新 segment(最后 reset 之后)为空 → prior 空。
    let after_double_reset =
        crate::conversation::project_latest_segment(&store, &session_id).unwrap();
    assert!(
        after_double_reset.is_empty(),
        "重复 reset 后新段 prior 应为空"
    );
    // 旧段事实保留:全量投影仍能读回旧轮(旧段可审计的数据仍存在)。
    let all = kanzei_core::project_session_facts(&store.list_session_facts(&session_id).unwrap());
    assert!(
        all.surface_messages.len() >= 4,
        "旧段事实应保留在事件日志: {}",
        all.surface_messages.len()
    );
    drop(store);

    // 单个 reset 后(新段有消息)→ 只含新段。
    let root2 = std::env::temp_dir().join(format!(
        "kanzei-app-segment2-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root2).unwrap();
    let canonical2 = crate::normalized_project_root(&root2);
    let session_id2 = crate::process_session_id(&canonical2, None);
    let store2 = SessionStore::open(&kanzei_core::project_state_path(&canonical2)).unwrap();
    store2
        .create_session(&session_id2, &canonical2.display().to_string(), None)
        .unwrap();
    let mut invariant2 = SessionInvariant::default();
    let mut write_turn2 = |store: &SessionStore, turn: &str, question: &str, answer: &str| {
        let assistant = Message::assistant(vec![Part::Text {
            text: answer.into(),
        }]);
        let facts = [
            SessionFactEnvelope::new(
                turn,
                None,
                SessionFact::UserMessageCommitted {
                    input_id: format!("i-{turn}"),
                    message: Message::user_text(question),
                },
            ),
            SessionFactEnvelope::new(turn, Some(1), SessionFact::TurnStarted { max_steps: 1 }),
            SessionFactEnvelope::new(
                turn,
                Some(1),
                SessionFact::AssistantMessageCommitted {
                    message_id: format!("m-{turn}"),
                    content_hash: kanzei_core::store::stable_message_hash(&assistant),
                    message: assistant,
                },
            ),
            SessionFactEnvelope::new(turn, None, SessionFact::TurnCompleted),
        ];
        store
            .append_session_facts_checked(&session_id2, &mut invariant2, &facts)
            .unwrap();
    };
    write_turn2(&store2, "run-old2", "旧问题2", "旧回答2");
    store2
        .append_event(
            &session_id2,
            "conversation.reset",
            &serde_json::json!({ "cleared": true }),
        )
        .unwrap();
    write_turn2(&store2, "run-new2", "新问题2", "新回答2");
    let latest = crate::conversation::project_latest_segment(&store2, &session_id2).unwrap();
    let texts: Vec<String> = latest
        .iter()
        .filter_map(|m| match &m.parts[0] {
            Part::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["新问题2", "新回答2"]);
    drop(store2);

    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(root2).unwrap();
}

#[test]
fn conversation_get_falls_back_to_legacy_when_no_typed_facts() {
    // R-242 批7(mobile 回归防护):会话无任何 typed facts(未被 typed writer
    // 接管,如 mobile 线程)时,gate 开启的投影路径回退 legacy 快照——手机
    // 注入的 conversation.updated 仍可被 conversation_get 读到,不丢消息。
    let _gate_guard = GATE_ENV_LOCK.lock().unwrap();
    use kanzei_llm::{Message, Part};

    let root = std::env::temp_dir().join(format!(
        "kanzei-app-empty-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let canonical = crate::normalized_project_root(&root);
    let session_id = crate::process_session_id(&canonical, None);
    let store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
    store
        .create_session(&session_id, &canonical.display().to_string(), None)
        .unwrap();
    store
        .append_event(
            &session_id,
            "conversation.updated",
            &serde_json::json!({ "messages": [Message::user_text("手机注入消息")] }),
        )
        .unwrap();
    drop(store);

    // gate 开(白名单含 conversation_get)→ 无 typed facts → 回退 legacy 快照。
    std::env::set_var("KANZEI_PROJECTION_GATES", "conversation_get");
    let history =
        crate::conversation::conversation_get(canonical.display().to_string(), None, None).unwrap();
    assert_eq!(history.len(), 1, "空投影回退 legacy 快照");
    assert_eq!(
        history[0].parts[0],
        Part::Text {
            text: "手机注入消息".into()
        }
    );
    std::env::remove_var("KANZEI_PROJECTION_GATES");

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn conversation_list_projected_segments_by_reset_boundary() {
    // R-242 批7(验收①/④):gate 开启时 conversation_list 按 conversation.reset
    // 分段,旧段保留(可审计)、新段可见。
    let _gate_guard = GATE_ENV_LOCK.lock().unwrap();
    use kanzei_core::{SessionFact, SessionFactEnvelope, SessionInvariant, SessionStore};
    use kanzei_llm::{Message, Part};

    let root = std::env::temp_dir().join(format!(
        "kanzei-app-listseg-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let canonical = crate::normalized_project_root(&root);
    let session_id = crate::process_session_id(&canonical, None);
    let store = SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
    store
        .create_session(&session_id, &canonical.display().to_string(), None)
        .unwrap();
    let mut invariant = SessionInvariant::default();
    let mut write_turn = |store: &SessionStore, turn: &str, question: &str, answer: &str| {
        let assistant = Message::assistant(vec![Part::Text {
            text: answer.into(),
        }]);
        let facts = [
            SessionFactEnvelope::new(
                turn,
                None,
                SessionFact::UserMessageCommitted {
                    input_id: format!("i-{turn}"),
                    message: Message::user_text(question),
                },
            ),
            SessionFactEnvelope::new(turn, Some(1), SessionFact::TurnStarted { max_steps: 1 }),
            SessionFactEnvelope::new(
                turn,
                Some(1),
                SessionFact::AssistantMessageCommitted {
                    message_id: format!("m-{turn}"),
                    content_hash: kanzei_core::store::stable_message_hash(&assistant),
                    message: assistant,
                },
            ),
            SessionFactEnvelope::new(turn, None, SessionFact::TurnCompleted),
        ];
        store
            .append_session_facts_checked(&session_id, &mut invariant, &facts)
            .unwrap();
    };
    write_turn(&store, "run-a", "第一段问题", "第一段回答");
    store
        .append_event(
            &session_id,
            "conversation.reset",
            &serde_json::json!({ "cleared": true }),
        )
        .unwrap();
    write_turn(&store, "run-b", "第二段问题", "第二段回答");
    drop(store);

    std::env::set_var("KANZEI_PROJECTION_GATES", "conversation_list");
    let segments =
        crate::conversation::conversation_list(canonical.display().to_string(), None).unwrap();
    std::env::remove_var("KANZEI_PROJECTION_GATES");
    assert_eq!(segments.len(), 2, "reset 划分两段,旧段可审计");
    let titles: Vec<String> = segments
        .iter()
        .filter_map(|s| s["title"].as_str().map(String::from))
        .collect();
    assert_eq!(titles, vec!["第一段问题", "第二段问题"]);

    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn conversation_delete_removes_projected_segment() {
    // D-421:投影模式下 conversation_delete 收到的是投影段末的 typed fact sequence,
    // 必须删除整段(typed facts + 快照)而不是只删快照——否则「删不掉」。
    use kanzei_core::{SessionFact, SessionFactEnvelope, SessionInvariant, SessionStore};
    use kanzei_llm::{Message, Part};
    let _gate_guard = GATE_ENV_LOCK.lock().unwrap();

    let root = std::env::temp_dir().join(format!(
        "kanzei-app-delseg-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let canonical = crate::normalized_project_root(&root);
    let session_id = crate::process_session_id(&canonical, None);
    let store = SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
    store
        .create_session(&session_id, &canonical.display().to_string(), None)
        .unwrap();
    let mut invariant = SessionInvariant::default();
    let mut write_turn = |store: &SessionStore, turn: &str, question: &str, answer: &str| {
        let assistant = Message::assistant(vec![Part::Text {
            text: answer.into(),
        }]);
        let facts = [
            SessionFactEnvelope::new(
                turn,
                None,
                SessionFact::UserMessageCommitted {
                    input_id: format!("i-{turn}"),
                    message: Message::user_text(question),
                },
            ),
            SessionFactEnvelope::new(turn, Some(1), SessionFact::TurnStarted { max_steps: 1 }),
            SessionFactEnvelope::new(
                turn,
                Some(1),
                SessionFact::AssistantMessageCommitted {
                    message_id: format!("m-{turn}"),
                    content_hash: kanzei_core::store::stable_message_hash(&assistant),
                    message: assistant,
                },
            ),
            SessionFactEnvelope::new(turn, None, SessionFact::TurnCompleted),
        ];
        store
            .append_session_facts_checked(&session_id, &mut invariant, &facts)
            .unwrap();
    };
    write_turn(&store, "run-a", "第一段问题", "第一段回答");
    store
        .append_event(
            &session_id,
            "conversation.reset",
            &serde_json::json!({ "cleared": true }),
        )
        .unwrap();
    write_turn(&store, "run-b", "第二段问题", "第二段回答");
    drop(store);

    let before =
        crate::conversation::conversation_list(canonical.display().to_string(), None).unwrap();
    assert_eq!(before.len(), 2, "reset 划分两段");
    // 新段最后 typed fact 的 sequence(UI 勾选传的就是它)。
    let new_segment_seq = before[1]["sequence"].as_i64().expect("新段应有 sequence");

    let deleted = crate::conversation::conversation_delete(
        canonical.display().to_string(),
        vec![new_segment_seq],
        None,
    )
    .unwrap();
    assert!(deleted > 0, "投影段删除应删多于 0 条,实得 {deleted}");

    let after =
        crate::conversation::conversation_list(canonical.display().to_string(), None).unwrap();
    assert_eq!(after.len(), 1, "删除后只剩旧段");
    assert_eq!(after[0]["title"], "第一段问题");

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn user_message_survives_kill_without_terminal() {
    // R-242 验收②(user 安全边界强杀):user_message_committed 落库后进程被杀
    // (无 terminal),重启后投影仍含该 user 消息(已发生事实不丢失)。
    use kanzei_core::{SessionFact, SessionFactEnvelope, SessionInvariant, SessionStore};
    use kanzei_llm::{Message, Part};

    let root = std::env::temp_dir().join(format!(
        "kanzei-app-userkill-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let canonical = crate::normalized_project_root(&root);
    let session_id = crate::process_session_id(&canonical, None);
    let store = SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
    store
        .create_session(&session_id, &canonical.display().to_string(), None)
        .unwrap();
    let mut invariant = SessionInvariant::default();
    // 只有 user message 落库,随后"进程被杀"——无 turn_started/terminal。
    store
        .append_session_facts_checked(
            &session_id,
            &mut invariant,
            &[SessionFactEnvelope::new(
                "run-killed",
                None,
                SessionFact::UserMessageCommitted {
                    input_id: "i".into(),
                    message: Message::user_text("强杀前已提交的提问"),
                },
            )],
        )
        .unwrap();
    drop(store);

    // 重启后(新 store 连接)投影最新 segment:user 消息不丢。
    let store = SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
    let latest = crate::conversation::project_latest_segment(&store, &session_id).unwrap();
    assert_eq!(latest.len(), 1, "user 已提交消息在强杀后不丢");
    assert_eq!(
        latest[0].parts[0],
        Part::Text {
            text: "强杀前已提交的提问".into()
        }
    );
    drop(store);

    std::fs::remove_dir_all(root).unwrap();
}
