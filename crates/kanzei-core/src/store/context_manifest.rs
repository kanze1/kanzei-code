//! Immutable request manifests. Content is addressed once per conversation segment;
//! request events contain references, never a second copy of the whole history.
use std::collections::{BTreeMap, HashMap, HashSet};

use kanzei_llm::{LlmRequest, Part, Role, Route};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{SessionStore, StoreError, StoredEvent};

pub const CONTEXT_REQUEST: &str = "context.request";
pub const CONTEXT_CONTENT: &str = "context.content";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextBlock {
    pub position: usize,
    pub kind: String,
    pub role: String,
    pub label: String,
    pub sha256: String,
    pub chars: usize,
    pub bytes: usize,
    pub inspectable: bool,
    pub source: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextManifest {
    pub format_version: u32,
    pub model: String,
    pub protocol: String,
    pub wire_sha256: String,
    pub wire_bytes: usize,
    pub estimated_tokens: u64,
    pub message_count: usize,
    pub repeated_results: usize,
    pub omitted_bytes: usize,
    #[serde(default)]
    pub adjustments: Vec<Value>,
    #[serde(default)]
    pub source_reads: Vec<Value>,
    pub contributions: Vec<(String, usize)>,
    pub blocks: Vec<ContextBlock>,
}

#[derive(Clone, Debug)]
pub struct ContextSnapshot {
    pub manifest: ContextManifest,
    pub contents: BTreeMap<String, String>,
}

#[derive(Default)]
pub struct ContextCapture<'a> {
    pub contributions: &'a [(String, usize)],
    pub system_sources: &'a [Vec<(String, usize)>],
    pub source_reads: &'a [kanzei_harness::ContextSourceReport],
    pub estimated_tokens: u64,
    pub repeated_results: usize,
    pub omitted_bytes: usize,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl ContextSnapshot {
    pub fn capture(request: &LlmRequest, route: &Route, capture: ContextCapture<'_>) -> Self {
        let ContextCapture {
            contributions,
            system_sources,
            source_reads,
            estimated_tokens,
            repeated_results,
            omitted_bytes,
        } = capture;
        let wire = kanzei_llm::protocol::build_body_for_route(
            route.kind,
            request,
            &route.hosted_channel(&request.model),
        );
        let wire = wire.to_string();
        let mut snapshot = Self {
            manifest: ContextManifest {
                format_version: 1,
                model: request.model.clone(),
                protocol: format!("{:?}", route.kind),
                wire_sha256: hash(wire.as_bytes()),
                wire_bytes: wire.len(),
                estimated_tokens,
                message_count: request.messages.len(),
                repeated_results,
                omitted_bytes,
                adjustments: Vec::new(),
                source_reads: source_reads.iter().map(|source| json!(source)).collect(),
                contributions: contributions.to_vec(),
                blocks: Vec::new(),
            },
            contents: BTreeMap::new(),
        };
        for (index, text) in request.system.iter().enumerate() {
            let mut offset = 0;
            for (span_index, (key, bytes)) in system_sources[index].iter().enumerate() {
                if span_index > 0 {
                    offset += 2;
                }
                let end = offset + bytes;
                let content = text
                    .get(offset..end)
                    .expect("context source span must match the assembled system block");
                snapshot.text(
                    "system",
                    "system",
                    key,
                    content,
                    Some(json!({"system_index":index,"offset_bytes":offset,"length_bytes":bytes})),
                );
                offset = end;
            }
            assert_eq!(
                offset,
                text.len(),
                "context source spans must cover the assembled system block"
            );
        }
        let calls: HashMap<_, _> = request
            .messages
            .iter()
            .flat_map(|message| &message.parts)
            .filter_map(|part| match part {
                Part::ToolCall { id, name, input } => Some((id.as_str(), (name, input))),
                _ => None,
            })
            .collect();
        for (index, message) in request.messages.iter().enumerate() {
            let role = match message.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            for (part_index, part) in message.parts.iter().enumerate() {
                let label = format!("messages/{}/{}", index + 1, part_index + 1);
                match part {
                    Part::Text { text } => snapshot.text("message", role, &label, text, None),
                    Part::Reasoning { text, .. } => {
                        snapshot.text("reasoning", role, &label, text, None)
                    }
                    Part::ToolCall { id, name, input } => snapshot.text(
                        "tool_call",
                        role,
                        name,
                        &input.to_string(),
                        Some(json!({"call_id":id,"tool":name})),
                    ),
                    Part::ToolResult {
                        call_id,
                        content,
                        is_error,
                    } => {
                        let mut source = json!({"call_id":call_id,"is_error":is_error});
                        if let Some((name, input)) = calls.get(call_id.as_str()) {
                            source["tool"] = json!(name);
                            let selectors = [
                                "path",
                                "url",
                                "ref",
                                "offset",
                                "limit",
                                "from_line",
                                "num_lines",
                                "action",
                                "line",
                            ]
                            .into_iter()
                            .filter_map(|key| {
                                input.get(key).map(|value| (key.to_owned(), value.clone()))
                            })
                            .collect::<serde_json::Map<String, Value>>();
                            source["input"] = Value::Object(selectors);
                            source["input_sha256"] = json!(hash(input.to_string().as_bytes()));
                        }
                        if let Some(frame) = content
                            .strip_prefix(kanzei_harness::tool::EXTERNAL_SOURCE_PREFIX)
                            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                            .filter(|frame| frame["authority"] == "untrusted_data")
                        {
                            source["observation"] = frame["source"].clone();
                        }
                        let name = source["tool"].as_str().unwrap_or(call_id).to_owned();
                        snapshot.text("tool_result", role, &name, content, Some(source));
                    }
                    Part::Image { media_type, data } | Part::Document { media_type, data } => {
                        // Binary attachment data already belongs to the original input.
                        // Retain identity/size without duplicating its base64 in a ledger.
                        snapshot.manifest.blocks.push(ContextBlock {
                            position: snapshot.manifest.blocks.len(),
                            kind: "attachment".into(),
                            role: role.into(),
                            label: media_type.clone(),
                            sha256: hash(data.as_bytes()),
                            chars: 0,
                            bytes: data.len(),
                            inspectable: false,
                            source: None,
                        });
                    }
                    Part::Hosted { kind, raw, .. } => {
                        snapshot.text("hosted", role, kind, &raw.to_string(), None)
                    }
                }
            }
        }
        for tool in &request.tools {
            snapshot.text(
                "tool_schema",
                "tool",
                &tool.name,
                &json!(tool).to_string(),
                None,
            );
        }
        snapshot
    }

    fn text(&mut self, kind: &str, role: &str, label: &str, text: &str, source: Option<Value>) {
        let sha256 = hash(text.as_bytes());
        self.contents
            .entry(sha256.clone())
            .or_insert_with(|| text.into());
        self.manifest.blocks.push(ContextBlock {
            position: self.manifest.blocks.len(),
            kind: kind.into(),
            role: role.into(),
            label: label.into(),
            sha256,
            chars: text.chars().count(),
            bytes: text.len(),
            inspectable: true,
            source,
        });
    }
}

impl SessionStore {
    pub fn append_context_request(
        &self,
        session: &str,
        turn: &str,
        step: u32,
        snapshot: &ContextSnapshot,
    ) -> Result<StoredEvent, StoreError> {
        for (sha256, text) in &snapshot.contents {
            if hash(text.as_bytes()) != *sha256 {
                return Err(StoreError::InvalidInput(
                    "context content fingerprint mismatch".into(),
                ));
            }
        }
        for block in snapshot
            .manifest
            .blocks
            .iter()
            .filter(|block| block.inspectable)
        {
            if !snapshot.contents.contains_key(&block.sha256) {
                return Err(StoreError::InvalidInput(
                    "context manifest references missing content".into(),
                ));
            }
        }
        let tx =
            rusqlite::Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let floor = self.conversation_floor(session)?.unwrap_or(0);
        let mut statement = tx.prepare("SELECT json_extract(payload_json,'$.sha256') FROM session_events WHERE session_id=?1 AND event_type=?2 AND sequence>?3")?;
        let known = statement
            .query_map(params![session, CONTEXT_CONTENT, floor], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<HashSet<_>, _>>()?;
        drop(statement);
        for (sha256, text) in &snapshot.contents {
            if !known.contains(sha256) {
                super::events::append_event_tx(
                    &tx,
                    session,
                    CONTEXT_CONTENT,
                    &json!({"sha256":sha256,"text":text}),
                )?;
            }
        }
        let event = super::events::append_event_tx(
            &tx,
            session,
            CONTEXT_REQUEST,
            &json!({"turn_id":turn,"step":step,"manifest":snapshot.manifest}),
        )?;
        tx.commit()?;
        Ok(event)
    }

    /// An explicit sequence freezes the selected request. Reset/deleted segments
    /// cannot reappear in a new conversation through a cached source projection.
    pub fn context_requests(
        &self,
        session: &str,
        sequence: Option<i64>,
    ) -> Result<Vec<StoredEvent>, StoreError> {
        let floor = self.conversation_floor(session)?.unwrap_or(0);
        let mut statement = self.connection.prepare("SELECT event_id,session_id,sequence,event_type,payload_json,created_at FROM session_events WHERE session_id=?1 AND event_type=?2 AND sequence>?3 AND (?4 IS NULL OR sequence=?4) ORDER BY sequence DESC LIMIT 30")?;
        let rows = statement.query_map(
            params![session, CONTEXT_REQUEST, floor, sequence],
            crate::store::events::event_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn context_block(
        &self,
        session: &str,
        request_sequence: i64,
        sha256: &str,
    ) -> Result<Option<String>, StoreError> {
        let requests = self.context_requests(session, Some(request_sequence))?;
        let Some(request) = requests.first() else {
            return Ok(None);
        };
        let manifest: ContextManifest =
            serde_json::from_value(request.payload["manifest"].clone())?;
        if manifest.format_version != 1 {
            return Err(StoreError::InvalidInput(
                "unsupported context manifest format".into(),
            ));
        }
        if !manifest
            .blocks
            .iter()
            .any(|block| block.inspectable && block.sha256 == sha256)
        {
            return Ok(None);
        }
        let floor = self.conversation_floor(session)?.unwrap_or(0);
        let value: Option<String> = self.connection.query_row("SELECT json_extract(payload_json,'$.text') FROM session_events WHERE session_id=?1 AND event_type=?2 AND sequence>?3 AND sequence<?4 AND json_extract(payload_json,'$.sha256')=?5 ORDER BY sequence DESC LIMIT 1",params![session,CONTEXT_CONTENT,floor,request_sequence,sha256],|row|row.get(0)).optional()?;
        Ok(value.filter(|text| hash(text.as_bytes()) == sha256))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contents_are_deduplicated_and_requests_remain_immutable_and_scoped() {
        let root = tempfile::tempdir().unwrap();
        let store = SessionStore::open(&root.path().join("state.db")).unwrap();
        store.create_session("a", "project", None).unwrap();
        store.create_session("b", "project", None).unwrap();
        let mut snapshot = ContextSnapshot {
            manifest: ContextManifest {
                format_version: 1,
                model: "fixture".into(),
                protocol: "fixture".into(),
                wire_sha256: "wire".into(),
                wire_bytes: 0,
                estimated_tokens: 12,
                message_count: 1,
                repeated_results: 0,
                omitted_bytes: 0,
                adjustments: vec![],
                source_reads: vec![],
                contributions: vec![],
                blocks: vec![],
            },
            contents: BTreeMap::new(),
        };
        snapshot.text("system", "system", "rules", "中文规则 v1", None);
        let first = store
            .append_context_request("a", "turn", 1, &snapshot)
            .unwrap();
        let sha = snapshot.manifest.blocks[0].sha256.clone();
        store
            .append_context_request("a", "turn", 2, &snapshot)
            .unwrap();
        assert_eq!(
            store
                .list_events("a", 0)
                .unwrap()
                .iter()
                .filter(|e| e.event_type == CONTEXT_CONTENT)
                .count(),
            1
        );
        snapshot.manifest.blocks.clear();
        snapshot.contents.clear();
        snapshot.text("system", "system", "rules", "中文规则 v2", None);
        let second = store
            .append_context_request("a", "turn", 3, &snapshot)
            .unwrap();
        assert_eq!(
            store
                .context_block("a", first.sequence, &sha)
                .unwrap()
                .as_deref(),
            Some("中文规则 v1")
        );
        assert!(store
            .context_block("a", second.sequence, &sha)
            .unwrap()
            .is_none());
        assert!(store
            .context_block("b", first.sequence, &sha)
            .unwrap()
            .is_none());
        store
            .append_event("a", "conversation.reset", &json!({}))
            .unwrap();
        assert!(store.context_requests("a", None).unwrap().is_empty());
        assert!(store
            .context_block("a", first.sequence, &sha)
            .unwrap()
            .is_none());
        store
            .append_context_request("a", "next", 1, &snapshot)
            .unwrap();
        assert_eq!(store.context_requests("a", None).unwrap().len(), 1);
    }
}
