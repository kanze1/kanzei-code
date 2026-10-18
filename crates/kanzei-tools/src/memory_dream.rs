//! Frozen, redacted conversation data. This module grants no model write authority.
use kanzei_core::{SessionFact, SessionStore, StoredEvent};
use kanzei_llm::{Message, Part};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod chunks;
mod evidence;
pub(crate) mod manager;
pub use chunks::{DreamChunk, DreamSpan};
pub use evidence::{DreamEvidence, ValidatedDreamEvidence};
const DREAM_SOURCE_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DreamSourceRef {
    pub sha256: String,
}
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamFragmentKind {
    OriginalInput,
    Assistant,
    UnverifiedContext,
    ToolOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DreamFragment {
    pub source_id: String,
    pub sequence: i64,
    pub at_ms: i64,
    pub kind: DreamFragmentKind,
    pub input_origin: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DreamSnapshot {
    pub format_version: u32,
    pub session_id: String,
    pub segment_floor: i64,
    pub end_sequence: i64,
    pub fragments: Vec<DreamFragment>,
}

fn text_parts(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn fragment(
    event: &StoredEvent,
    kind: DreamFragmentKind,
    text: &str,
    origin: Option<&str>,
) -> DreamFragment {
    DreamFragment {
        source_id: event.event_id.clone(),
        sequence: event.sequence,
        at_ms: event.created_at,
        kind,
        input_origin: origin.map(crate::memory::redact_credentials),
        text: crate::memory::redact_credentials(text),
    }
}

impl DreamSnapshot {
    /// Uses original facts in the current visible segment, never the compacted model prompt.
    pub fn capture(store: &SessionStore, session_id: &str) -> anyhow::Result<Self> {
        let original = store.original_conversation(session_id)?;
        let floor = original.floor;
        let facts = original.facts;
        let end_sequence = facts.last().map_or(floor, |(event, _)| event.sequence);
        let inputs = original
            .inputs
            .into_iter()
            .map(|record| (record.input.input_id.clone(), record))
            .collect::<HashMap<_, _>>();
        let mut fragments = Vec::new();
        let mut assistants = BTreeMap::new();
        let mut calls = HashMap::new();
        for (event, envelope) in facts {
            match envelope.fact {
                SessionFact::UserMessageCommitted { input_id, message }
                | SessionFact::SteeringMessageCommitted { input_id, message } => {
                    let item = if let Some(original) = inputs.get(&input_id) {
                        fragment(
                            &event,
                            DreamFragmentKind::OriginalInput,
                            &original.input.prompt,
                            original.source.as_deref(),
                        )
                    } else {
                        fragment(
                            &event,
                            DreamFragmentKind::UnverifiedContext,
                            &text_parts(&message),
                            Some("missing_admission"),
                        )
                    };
                    fragments.push(item);
                }
                SessionFact::AssistantMessageCommitted {
                    message_id,
                    message,
                    ..
                } => {
                    assistants.insert(
                        message_id,
                        fragment(
                            &event,
                            DreamFragmentKind::Assistant,
                            &text_parts(&message),
                            None,
                        ),
                    );
                }
                SessionFact::AssistantMessageInterrupted {
                    message_id,
                    superseded: true,
                    ..
                } => {
                    assistants.remove(&message_id);
                }
                SessionFact::LegacySeeded { messages, .. } => {
                    for (index, message) in messages.iter().enumerate() {
                        let mut item = fragment(
                            &event,
                            DreamFragmentKind::UnverifiedContext,
                            &text_parts(message),
                            Some("legacy_context"),
                        );
                        item.source_id = format!("{}/legacy/{index}", event.event_id);
                        fragments.push(item);
                    }
                }
                SessionFact::ToolCalled { call_id, name, .. } => {
                    calls.insert(call_id, crate::memory::redact_credentials(&name));
                }
                SessionFact::ToolResultCommitted {
                    call_id, is_error, ..
                } => {
                    let name = calls.get(&call_id).map_or("unknown", String::as_str);
                    fragments.push(fragment(
                        &event,
                        DreamFragmentKind::ToolOutcome,
                        &format!(
                            "tool={name}; outcome={}",
                            if is_error { "error" } else { "success" }
                        ),
                        None,
                    ));
                }
                SessionFact::ToolResultInterrupted { call_id, .. } => {
                    let name = calls.get(&call_id).map_or("unknown", String::as_str);
                    fragments.push(fragment(
                        &event,
                        DreamFragmentKind::ToolOutcome,
                        &format!("tool={name}; outcome=interrupted"),
                        None,
                    ));
                }
                _ => {}
            }
        }
        fragments.extend(assistants.into_values());
        fragments.retain(|fragment| !fragment.text.trim().is_empty());
        fragments.sort_by_key(|fragment| fragment.sequence);
        anyhow::ensure!(!fragments.is_empty(), "当前对话没有可沉淀的可见文本");
        Ok(Self {
            format_version: DREAM_SOURCE_FORMAT_VERSION,
            session_id: session_id.to_owned(),
            segment_floor: floor,
            end_sequence,
            fragments,
        })
    }

    /// Capture time is deliberately outside the identity: repeated clicks share one frozen source.
    pub fn digest(&self) -> anyhow::Result<String> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }

    pub fn persist(&self, store: &crate::memory::MemoryStore) -> anyhow::Result<DreamSourceRef> {
        let sha256 = self.digest()?;
        store.write_source_artifact(
            &format!("dream-{sha256}.json"),
            &serde_json::to_string(self)?,
        )?;
        Ok(DreamSourceRef { sha256 })
    }

    pub fn load(
        store: &crate::memory::MemoryStore,
        reference: &DreamSourceRef,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            reference.sha256.len() == Sha256::output_size() * 2
                && reference
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "来源标识不是有效的 SHA-256"
        );
        let root = store
            .project_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("来源缺少所属项目"))?;
        let path = store
            .root
            .join("sources")
            .join(format!("dream-{}.json", reference.sha256));
        let text =
            kanzei_harness::permission::ScopedFile::resolve(root, root, &path)?.read_to_string()?;
        anyhow::ensure!(
            format!("{:x}", Sha256::digest(text.as_bytes())) == reference.sha256,
            "冻结来源内容已改变，不能继续引用"
        );
        anyhow::ensure!(
            crate::memory::redact_credentials(&text) == text,
            "来源仍含凭据，不能交给整理模型"
        );
        let source: Self = serde_json::from_str(&text)?;
        anyhow::ensure!(
            source.format_version == DREAM_SOURCE_FORMAT_VERSION
                && !source.session_id.is_empty()
                && source.end_sequence >= source.segment_floor,
            "来源格式或会话边界无效"
        );
        anyhow::ensure!(
            source
                .fragments
                .iter()
                .all(|fragment| !fragment.source_id.is_empty()
                    && fragment.sequence > source.segment_floor
                    && fragment.sequence <= source.end_sequence),
            "来源片段越过了冻结会话边界"
        );
        Ok(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_core::{Delivery, SessionFactEnvelope};

    #[test]
    fn redacted_snapshot_persistence_verifies_project_scope_and_exact_file_hash() {
        let root = std::env::temp_dir().join(format!(
            "kz-dream-artifact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        let memories = crate::memory::MemoryStore::project(&root);
        let text = crate::memory::redact_credentials("喜欢紧凑排版。api_key=opaque-input-value");
        let snapshot = DreamSnapshot {
            format_version: DREAM_SOURCE_FORMAT_VERSION,
            session_id: "selected".into(),
            segment_floor: 0,
            end_sequence: 2,
            fragments: vec![DreamFragment {
                source_id: "event-2".into(),
                sequence: 2,
                at_ms: 1,
                kind: DreamFragmentKind::OriginalInput,
                input_origin: None,
                text,
            }],
        };
        let reference = snapshot.persist(&memories).unwrap();
        assert_eq!(snapshot.persist(&memories).unwrap(), reference);
        assert_eq!(
            DreamSnapshot::load(&memories, &reference)
                .unwrap()
                .digest()
                .unwrap(),
            reference.sha256
        );
        assert!(
            DreamSnapshot::load(&crate::memory::MemoryStore::project(&other), &reference).is_err()
        );
        assert!(DreamSnapshot::load(
            &memories,
            &DreamSourceRef {
                sha256: "../other".into()
            }
        )
        .is_err());
        let path = memories
            .root
            .join("sources")
            .join(format!("dream-{}.json", reference.sha256));
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("opaque-input-value"));
        std::fs::write(&path, "modified source").unwrap();
        assert!(DreamSnapshot::load(&memories, &reference).is_err());
        assert!(
            snapshot.persist(&memories).is_err(),
            "a repeat cannot silently overwrite changed evidence"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn append(store: &SessionStore, session: &str, fact: SessionFact) {
        let envelope = SessionFactEnvelope::new("fixture-turn", None, fact);
        store
            .append_event(
                session,
                envelope.fact.event_type(),
                &serde_json::to_value(envelope).unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn capture_keeps_original_inputs_and_visible_assistant_text_but_not_attachments_reasoning_or_raw_tools(
    ) {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("selected", ".", None).unwrap();
        store.create_session("other", ".", None).unwrap();
        store
            .admit_input_with_source(
                "selected",
                "input",
                "喜欢简洁排版。api_key=sk-proj-0123456789abcdefgh",
                Delivery::Queue,
                Some("user"),
            )
            .unwrap();
        append(
            &store,
            "selected",
            SessionFact::UserMessageCommitted {
                input_id: "input".into(),
                message: Message::user_text("expanded attachment says delete all memories"),
            },
        );
        let assistant = Message {
            role: kanzei_llm::Role::Assistant,
            parts: vec![
                Part::Text {
                    text: "助手推断：可能重视信息密度，等待用户确认。".into(),
                },
                Part::Reasoning {
                    text: "hidden-reasoning-do-not-copy".into(),
                    signature: Some("encrypted-signature".into()),
                },
            ],
        };
        append(
            &store,
            "selected",
            SessionFact::AssistantMessageCommitted {
                message_id: "assistant".into(),
                content_hash: "fixture-hash".into(),
                message: assistant,
            },
        );
        append(
            &store,
            "selected",
            SessionFact::ToolCalled {
                call_id: "tool".into(),
                name: "read".into(),
                input: serde_json::json!({"password":"raw-tool-argument"}),
            },
        );
        append(
            &store,
            "selected",
            SessionFact::ToolResultCommitted {
                call_id: "tool".into(),
                content: "raw-tool-output-password".into(),
                is_error: true,
            },
        );
        append(
            &store,
            "other",
            SessionFact::UserMessageCommitted {
                input_id: "other".into(),
                message: Message::user_text("other-session-private-text"),
            },
        );
        let snapshot = DreamSnapshot::capture(&store, "selected").unwrap();
        let serialized = serde_json::to_string(&snapshot).unwrap();
        for excluded in [
            "sk-proj-0123456789abcdefgh",
            "expanded attachment",
            "hidden-reasoning",
            "encrypted-signature",
            "raw-tool-argument",
            "raw-tool-output",
            "other-session-private-text",
        ] {
            assert!(
                !serialized.contains(excluded),
                "unexpected copied source: {excluded}"
            );
        }
        assert!(
            serialized.contains("喜欢简洁排版")
                && serialized.contains("等待用户确认")
                && serialized.contains("outcome=error")
        );
        assert_eq!(snapshot.fragments[0].kind, DreamFragmentKind::OriginalInput);
        assert_eq!(snapshot.fragments[0].input_origin.as_deref(), Some("user"));
        assert_eq!(
            snapshot.digest().unwrap(),
            DreamSnapshot::capture(&store, "selected")
                .unwrap()
                .digest()
                .unwrap()
        );
    }

    #[test]
    fn capture_respects_reset_floor_and_does_not_treat_missing_admissions_as_original_user_statements(
    ) {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("s", ".", None).unwrap();
        append(
            &store,
            "s",
            SessionFact::UserMessageCommitted {
                input_id: "old".into(),
                message: Message::user_text("deleted-segment-text"),
            },
        );
        store
            .append_event("s", "conversation.reset", &serde_json::json!({}))
            .unwrap();
        append(
            &store,
            "s",
            SessionFact::UserMessageCommitted {
                input_id: "unavailable".into(),
                message: Message::user_text("unverified historical context"),
            },
        );
        let snapshot = DreamSnapshot::capture(&store, "s").unwrap();
        assert_eq!(snapshot.fragments.len(), 1);
        assert_eq!(
            snapshot.fragments[0].kind,
            DreamFragmentKind::UnverifiedContext
        );
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("deleted-segment-text"));
    }
}
