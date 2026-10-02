use super::*;
use kanzei_core::{Delivery, SessionStore};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct Fixture {
    root: PathBuf,
    state: AppState,
    source_id: String,
    source_session: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "kz-discussion-handoff-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        let root = normalized_project_root(&root);
        let state = AppState::default();
        let mut source = crate::ensure_default_process(&state, &root);
        source.id = format!("p2|{}", root.display());
        source.profile = Arc::new(Mutex::new(Some("readonly".into())));
        let source_id = source.id.clone();
        let source_session = process_session_id(&root, Some(&source_id));
        state
            .processes
            .lock()
            .unwrap()
            .insert(source_id.clone(), source);
        let f = Self {
            root,
            state,
            source_id,
            source_session,
        };
        f.store()
            .create_session(&f.source_session, &f.root.display().to_string(), None)
            .unwrap();
        f
    }

    fn store(&self) -> SessionStore {
        SessionStore::open(&kanzei_core::project_state_path(&self.root)).unwrap()
    }

    fn save(&self, session: &str, messages: Vec<Message>) {
        self.store()
            .create_session(session, &self.root.display().to_string(), None)
            .unwrap();
        self.store()
            .append_event(
                session,
                "conversation.updated",
                &json!({"messages": messages}),
            )
            .unwrap();
    }

    fn snapshot(&self) -> Result<String, String> {
        attach_snapshot(
            &self.state,
            HandoffSource {
                project_dir: self.root.display().to_string(),
                process_id: self.source_id.clone(),
            },
            "按讨论推进",
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

#[test]
fn handoff_preserves_early_scope_roles_evidence_and_source_without_reasoning_or_truncation() {
    let f = Fixture::new();
    let long_scope = format!(
        "Windows和Android，离线收藏。{}末尾约束：邀请注册",
        "业务细节".repeat(5000)
    );
    f.save(
        &f.source_session,
        vec![
            Message::user_text(long_scope.clone()),
            Message::assistant(vec![
                Part::Reasoning {
                    text: "PRIVATE_REASONING".into(),
                    signature: Some("PRIVATE_SIGNATURE".into()),
                },
                Part::Text {
                    text: "采用自建数据库同步个人评分".into(),
                },
                Part::ToolCall {
                    id: "tool1".into(),
                    name: "read".into(),
                    input: json!({"path":"notes.md"}),
                },
            ]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "tool1".into(),
                content: "工具依据：版本1".into(),
                is_error: false,
            }]),
            Message::user_text("只保留私有数据，不接入公共账号"),
        ],
    );
    f.save(
        "other-discussion",
        vec![Message::user_text("OTHER_DISCUSSION_SECRET")],
    );
    let prompt = f.snapshot().unwrap();
    assert!(prompt.starts_with("按讨论推进\n\n"));
    assert!(prompt.contains(&long_scope));
    assert!(prompt.contains("[助手]\n采用自建数据库同步个人评分"));
    assert!(prompt.contains("[工具结果 tool1 · 成功]\n工具依据：版本1"));
    assert!(prompt.contains(&format!("来源项目：{}", f.root.display())));
    assert!(prompt.contains(&format!("来源对话：{}", f.source_id)));
    assert!(prompt.contains(&format!("来源会话：{}", f.source_session)));
    assert!(!prompt.contains("PRIVATE_REASONING"));
    assert!(!prompt.contains("PRIVATE_SIGNATURE"));
    assert!(!prompt.contains("OTHER_DISCUSSION_SECRET"));
    assert!(prompt.find(&long_scope).unwrap() < prompt.find("只保留私有数据").unwrap());
}

#[test]
fn handoff_uses_only_the_current_source_segment() {
    let f = Fixture::new();
    f.save(
        &f.source_session,
        vec![Message::user_text("OLD_SEGMENT_SCOPE")],
    );
    f.store()
        .append_event(
            &f.source_session,
            "conversation.reset",
            &json!({"cleared":true}),
        )
        .unwrap();
    assert!(f.snapshot().unwrap_err().contains("没有可交接的上下文"));
    f.save(
        &f.source_session,
        vec![Message::user_text("当前讨论约定：收藏与评分")],
    );
    let prompt = f.snapshot().unwrap();
    assert!(prompt.contains("当前讨论约定：收藏与评分"));
    assert!(!prompt.contains("OLD_SEGMENT_SCOPE"));
}

#[test]
fn handoff_uses_new_typed_discussion_facts_over_stale_legacy_and_respects_reset() {
    use kanzei_core::{SessionFact, SessionFactEnvelope, SessionInvariant};

    let _gate_guard = crate::conversation_tests::GATE_ENV_LOCK.lock().unwrap();
    struct RestoreGate(Option<std::ffi::OsString>);
    impl Drop for RestoreGate {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("KANZEI_PROJECTION_GATES", value),
                None => std::env::remove_var("KANZEI_PROJECTION_GATES"),
            }
        }
    }
    let _restore = RestoreGate(std::env::var_os("KANZEI_PROJECTION_GATES"));
    std::env::set_var("KANZEI_PROJECTION_GATES", "conversation_get");
    let f = Fixture::new();
    f.save(
        &f.source_session,
        vec![Message::user_text("STALE_LEGACY_SCOPE：只做网页")],
    );
    let store = f.store();
    kanzei_core::prepare_typed_session(&store, &f.source_session).unwrap();
    let mut invariant = SessionInvariant::default();
    let mut write_turn = |run: &str, question: &str, answer: &str| {
        let assistant = Message::assistant(vec![Part::Text {
            text: answer.into(),
        }]);
        store
            .append_session_facts_checked(
                &f.source_session,
                &mut invariant,
                &[
                    SessionFactEnvelope::new(
                        run,
                        None,
                        SessionFact::UserMessageCommitted {
                            input_id: format!("input-{run}"),
                            message: Message::user_text(question),
                        },
                    ),
                    SessionFactEnvelope::new(
                        run,
                        Some(1),
                        SessionFact::TurnStarted { max_steps: 1 },
                    ),
                    SessionFactEnvelope::new(
                        run,
                        Some(1),
                        SessionFact::AssistantMessageCommitted {
                            message_id: format!("message-{run}"),
                            content_hash: kanzei_core::store::stable_message_hash(&assistant),
                            message: assistant,
                        },
                    ),
                    SessionFactEnvelope::new(run, None, SessionFact::TurnCompleted),
                ],
            )
            .unwrap();
    };
    write_turn(
        "discussion-1",
        "TYPED_SCOPE_ONE：Windows和Android支持离线收藏",
        "使用自建数据库同步评分",
    );
    write_turn(
        "discussion-2",
        "TYPED_SCOPE_TWO：邀请注册，不公开个人评分",
        "先实现收藏与评分闭环",
    );
    let legacy =
        crate::conversation::recover_messages_raw(&store, &f.source_session, None).unwrap();
    assert_eq!(legacy.len(), 1, "typed新讨论没有回写旧legacy快照");
    assert!(!readable_transcript(&legacy).contains("TYPED_SCOPE_ONE"));
    let prompt = f.snapshot().unwrap();
    assert!(prompt.contains("TYPED_SCOPE_ONE：Windows和Android支持离线收藏"));
    assert!(prompt.contains("TYPED_SCOPE_TWO：邀请注册，不公开个人评分"));
    assert!(prompt.contains("[助手]\n使用自建数据库同步评分"));
    store
        .append_event(
            &f.source_session,
            "conversation.reset",
            &json!({"cleared":true}),
        )
        .unwrap();
    assert!(f.snapshot().unwrap_err().contains("没有可交接的上下文"));
    write_turn(
        "discussion-3",
        "TYPED_CURRENT_SCOPE：当前段只整理轻小说",
        "当前讨论已确认",
    );
    let after_reset = f.snapshot().unwrap();
    assert!(after_reset.contains("TYPED_CURRENT_SCOPE：当前段只整理轻小说"));
    for old in ["STALE_LEGACY_SCOPE", "TYPED_SCOPE_ONE", "TYPED_SCOPE_TWO"] {
        assert!(!after_reset.contains(old), "不能混入reset之前的历史：{old}");
    }
}

#[test]
fn queued_handoff_is_durable_and_does_not_follow_later_source_edits_or_reset() {
    let f = Fixture::new();
    f.save(
        &f.source_session,
        vec![Message::user_text("交接时范围：支持离线")],
    );
    let frozen = f.snapshot().unwrap();
    let main_session = process_session_id(&f.root, None);
    {
        let store = f.store();
        store
            .create_session(&main_session, &f.root.display().to_string(), None)
            .unwrap();
        store
            .admit_batch_input(&main_session, "handoff-input", &frozen, Delivery::Queue)
            .unwrap();
    }
    f.save(
        &f.source_session,
        vec![Message::user_text("LATER_CHANGED_SCOPE")],
    );
    crate::conversation::clear_conversation(
        &f.state,
        &f.root.display().to_string(),
        Some(&f.source_id),
    )
    .unwrap();
    let reopened = f.store();
    let queued = reopened.list_pending_inputs(&main_session).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].prompt, frozen);
    assert!(!queued[0].prompt.contains("LATER_CHANGED_SCOPE"));
    let promoted = reopened.promote_next_input(&main_session).unwrap().unwrap();
    assert_eq!(promoted.prompt, frozen);
    assert!(reopened
        .input_is_execution_batch(&main_session, "handoff-input")
        .unwrap());
}

#[test]
fn handoff_rejects_running_missing_and_wrong_project_sources() {
    let f = Fixture::new();
    f.save(&f.source_session, vec![Message::user_text("范围")]);
    let runtime = runtime_for(&f.state, &f.source_session);
    runtime.running.store(true, Ordering::SeqCst);
    assert!(f.snapshot().unwrap_err().contains("仍在运行"));
    runtime.running.store(false, Ordering::SeqCst);
    let other = Fixture::new();
    let error = attach_snapshot(
        &f.state,
        HandoffSource {
            project_dir: other.root.display().to_string(),
            process_id: f.source_id.clone(),
        },
        "按讨论推进",
    )
    .unwrap_err();
    assert!(error.contains("不属于指定项目"));
    f.state.processes.lock().unwrap().remove(&f.source_id);
    assert!(f.snapshot().unwrap_err().contains("来源对话已关闭"));
}

#[test]
fn handoff_marks_binary_attachments_without_copying_internal_payloads() {
    let transcript = readable_transcript(&[Message {
        role: Role::User,
        parts: vec![
            Part::Image {
                media_type: "image/png".into(),
                data: "PRIVATE_IMAGE_DATA".into(),
            },
            Part::Document {
                media_type: "application/pdf".into(),
                data: "PRIVATE_PDF_DATA".into(),
            },
        ],
    }]);
    assert!(transcript.contains("来源图片附件：image/png"));
    assert!(transcript.contains("来源文档附件：application/pdf"));
    assert!(!transcript.contains("PRIVATE_IMAGE_DATA"));
    assert!(!transcript.contains("PRIVATE_PDF_DATA"));
}
