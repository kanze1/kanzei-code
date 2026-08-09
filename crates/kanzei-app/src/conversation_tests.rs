//! 会话内容、历史恢复与消息负载测试。

use super::{prompt_attachment_parts, with_session_id, PromptAttachment};
// R-153 批10:会话恢复相关已迁到 conversation 模块。
use crate::conversation::{conversation_prior, recover_messages_at, recover_messages_raw};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[test]
fn prompt_attachments_become_image_and_document_parts() {
    let parts = prompt_attachment_parts(vec![
        PromptAttachment { file_name: "screen.png".into(), media_type: "image/png".into(), data: "PNGDATA".into() },
        PromptAttachment { file_name: "notes.pdf".into(), media_type: "application/pdf".into(), data: "PDFDATA".into() },
    ]).unwrap();
    assert!(matches!(&parts[0], kanzei_llm::Part::Image { media_type, data } if media_type == "image/png" && data == "PNGDATA"));
    assert!(matches!(&parts[1], kanzei_llm::Part::Document { media_type, data } if media_type == "application/pdf" && data == "PDFDATA"));
}

#[test]
fn session_id_is_added_to_event_payload() {
    let payload = with_session_id(serde_json::json!({"text": "hello"}), "ses_test#p2");
    assert_eq!(payload["sessionId"], "ses_test#p2");
    assert_eq!(payload["text"], "hello");
}

#[test]
fn session_id_does_not_change_non_object_payload() {
    assert_eq!(with_session_id(serde_json::Value::Null, "ses_test"), serde_json::Value::Null);
}

#[test]
fn conversation_prior_prefers_existing_memory_over_persisted_snapshot() {
    let conversation = Arc::new(Mutex::new(HashMap::new()));
    let persisted = vec![kanzei_llm::Message::user_text("恢复快照")];
    assert_eq!(conversation_prior(&conversation, "ses", persisted.clone())[0].parts, persisted[0].parts);
    let existing = vec![kanzei_llm::Message::user_text("内存旧快照")];
    conversation.lock().unwrap().insert("ses".into(), existing.clone());
    assert_eq!(conversation_prior(&conversation, "ses", vec![kanzei_llm::Message::user_text("最新持久化")])[0].parts, existing[0].parts);
}

#[test]
fn recover_messages_filters_orphan_tool_calls_from_persisted_snapshot() {
    let root = std::env::temp_dir().join(format!("kanzei-app-history-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&root).unwrap();
    let store = kanzei_core::SessionStore::open(&root.join("state.db")).unwrap();
    store.create_session("ses_history", &root.display().to_string(), None).unwrap();
    let messages = vec![
        kanzei_llm::Message::user_text("保留文本"),
        kanzei_llm::Message::assistant(vec![kanzei_llm::Part::ToolCall { id: "orphan".into(), name: "bash".into(), input: serde_json::json!({"command": "echo orphan"}) }]),
    ];
    store.append_event("ses_history", "conversation.updated", &serde_json::json!({"messages": messages})).unwrap();
    let recovered = recover_messages_at(&store, "ses_history", None).unwrap();
    assert_eq!(recovered.len(), 1);
    let raw = recover_messages_raw(&store, "ses_history", None).unwrap();
    assert!(raw.iter().map(|m| m.parts.len()).sum::<usize>() > recovered.iter().map(|m| m.parts.len()).sum::<usize>());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
