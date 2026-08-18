//! R-241：版本化 Session 事实、提交前不变量与确定性投影。
//!
//! typed facts 复用既有 `session_events`：表层 `event_type` 便于索引，payload 内的
//! `SessionFactEnvelope` 承载 format version、turn/step 身份和强类型事实。存储层
//! 继续由 `append_event_tx` 在 `BEGIN IMMEDIATE` 事务内分配 sequence；这里不建立
//! 第二张事件表，也不改变 legacy `conversation.updated` 的读写语义。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kanzei_llm::{Message, Part, Role};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::events::append_event_tx;
use super::{SessionStore, StoreError, StoredEvent};

pub const SESSION_EVENT_FORMAT_VERSION: u32 = 1;

pub const LEGACY_SEEDED: &str = "session.legacy_seeded";
pub const TURN_STARTED: &str = "session.turn_started";
pub const USER_MESSAGE_COMMITTED: &str = "session.user_message_committed";
pub const ASSISTANT_DRAFT_APPENDED: &str = "session.assistant_draft_appended";
pub const ASSISTANT_MESSAGE_COMMITTED: &str = "session.assistant_message_committed";
pub const ASSISTANT_MESSAGE_INTERRUPTED: &str = "session.assistant_message_interrupted";
pub const TOOL_CALLED: &str = "session.tool_called";
pub const TOOL_RESULT_COMMITTED: &str = "session.tool_result_committed";
pub const TOOL_RESULT_INTERRUPTED: &str = "session.tool_result_interrupted";
pub const TURN_STOPPED: &str = "session.turn_stopped";
pub const TURN_COMPLETED: &str = "session.turn_completed";
pub const TURN_FAILED: &str = "session.turn_failed";
/// R-279:子代理 transcript 事件(快照式,payload 含 call_id + 完整消息历史)。
/// 非 typed fact(不进 SessionFact 枚举),不影响主会话投影。
pub const SUBAGENT_TRANSCRIPT: &str = "subagent.transcript";

pub(crate) const FACT_TYPES: [&str; 12] = [
    LEGACY_SEEDED,
    TURN_STARTED,
    USER_MESSAGE_COMMITTED,
    ASSISTANT_DRAFT_APPENDED,
    ASSISTANT_MESSAGE_COMMITTED,
    ASSISTANT_MESSAGE_INTERRUPTED,
    TOOL_CALLED,
    TOOL_RESULT_COMMITTED,
    TOOL_RESULT_INTERRUPTED,
    TURN_STOPPED,
    TURN_COMPLETED,
    TURN_FAILED,
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionFact {
    LegacySeeded {
        source_event_id: String,
        source_sequence: i64,
        source_hash: String,
        /// D-375:**不落库**——seed 是对 `conversation.updated` 的引用,不是它的副本。
        ///
        /// 旧实现把整份 messages 抄进 seed,于是影子层比它影子的对象还贵:实测主库里
        /// 33 条 legacy_seeded 占 29.4MB,而被影子的 82 条 conversation.updated 只有
        /// 13.3MB(全库 132MB 的 22% 花在这份副本上),且每出现一个新快照就再抄一份。
        ///
        /// 现在写入端置空、`skip_serializing_if` 让它根本不进 JSON;读取端
        /// `list_session_facts` 按 source_event_id 回读源事件填回来(见 `rehydrate_seed`)。
        /// 投影器 `project_session_facts` 保持纯函数,签名与行为都不变。
        /// `serde(default)` 让**存量**带 messages 的 seed 继续读得出来(非空即直接用,
        /// 不回读),新旧共存不需要停机。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        messages: Vec<Message>,
    },
    TurnStarted {
        max_steps: u32,
    },
    UserMessageCommitted {
        input_id: String,
        message: Message,
    },
    AssistantDraftAppended {
        message_id: String,
        chunk_index: u32,
        text: String,
    },
    AssistantMessageCommitted {
        message_id: String,
        content_hash: String,
        message: Message,
    },
    AssistantMessageInterrupted {
        message_id: String,
        reason: String,
        superseded: bool,
    },
    ToolCalled {
        call_id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResultCommitted {
        call_id: String,
        content: String,
        is_error: bool,
    },
    ToolResultInterrupted {
        call_id: String,
        reason: String,
    },
    TurnStopped,
    TurnCompleted,
    TurnFailed {
        error: String,
    },
}

impl SessionFact {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::LegacySeeded { .. } => LEGACY_SEEDED,
            Self::TurnStarted { .. } => TURN_STARTED,
            Self::UserMessageCommitted { .. } => USER_MESSAGE_COMMITTED,
            Self::AssistantDraftAppended { .. } => ASSISTANT_DRAFT_APPENDED,
            Self::AssistantMessageCommitted { .. } => ASSISTANT_MESSAGE_COMMITTED,
            Self::AssistantMessageInterrupted { .. } => ASSISTANT_MESSAGE_INTERRUPTED,
            Self::ToolCalled { .. } => TOOL_CALLED,
            Self::ToolResultCommitted { .. } => TOOL_RESULT_COMMITTED,
            Self::ToolResultInterrupted { .. } => TOOL_RESULT_INTERRUPTED,
            Self::TurnStopped => TURN_STOPPED,
            Self::TurnCompleted => TURN_COMPLETED,
            Self::TurnFailed { .. } => TURN_FAILED,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionFactEnvelope {
    pub format_version: u32,
    pub turn_id: String,
    pub step_id: Option<u32>,
    pub fact: SessionFact,
}

impl SessionFactEnvelope {
    pub fn new(turn_id: impl Into<String>, step_id: Option<u32>, fact: SessionFact) -> Self {
        Self {
            format_version: SESSION_EVENT_FORMAT_VERSION,
            turn_id: turn_id.into(),
            step_id,
            fact,
        }
    }

    pub fn event_type(&self) -> &'static str {
        self.fact.event_type()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionFactError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("session fact invariant violation: {0}")]
    Invariant(String),
}

#[derive(Debug, Clone, Default)]
struct DraftInvariant {
    step: u32,
    next_chunk: u32,
    text: String,
    finalized: bool,
}

#[derive(Debug, Clone)]
struct ToolInvariant {
    step: u32,
    resolved: bool,
}

#[derive(Debug, Clone, Default)]
struct TurnInvariant {
    user_committed: bool,
    current_step: Option<u32>,
    terminal: bool,
    drafts: HashMap<String, DraftInvariant>,
    draft_order: Vec<String>,
    declared_calls: HashMap<String, u32>,
    calls: HashMap<String, ToolInvariant>,
    call_order: Vec<String>,
}

/// 写入前的单 Session 状态机。`apply` 先在克隆上验证，失败不会污染调用方状态。
#[derive(Debug, Clone, Default)]
pub struct SessionInvariant {
    turns: HashMap<String, TurnInvariant>,
    turn_order: Vec<String>,
}

impl SessionInvariant {
    pub fn apply(&mut self, envelope: &SessionFactEnvelope) -> Result<(), SessionFactError> {
        if envelope.format_version != SESSION_EVENT_FORMAT_VERSION {
            return Err(SessionFactError::Invariant(format!(
                "unsupported format_version {}",
                envelope.format_version
            )));
        }
        let mut next = self.clone();
        next.apply_inner(envelope)?;
        *self = next;
        Ok(())
    }

    fn apply_inner(&mut self, envelope: &SessionFactEnvelope) -> Result<(), SessionFactError> {
        if matches!(envelope.fact, SessionFact::LegacySeeded { .. }) {
            if envelope.step_id.is_some() {
                return Err(SessionFactError::Invariant(
                    "legacy seed cannot carry step_id".into(),
                ));
            }
            return Ok(());
        }
        if envelope.turn_id.trim().is_empty() {
            return Err(SessionFactError::Invariant("turn_id is empty".into()));
        }
        if !self.turns.contains_key(&envelope.turn_id) {
            self.turn_order.push(envelope.turn_id.clone());
        }
        let turn = self.turns.entry(envelope.turn_id.clone()).or_default();
        if turn.terminal {
            return Err(SessionFactError::Invariant(format!(
                "turn {} already terminal",
                envelope.turn_id
            )));
        }

        let require_step = || {
            envelope.step_id.ok_or_else(|| {
                SessionFactError::Invariant(format!("{} requires step_id", envelope.event_type()))
            })
        };
        match &envelope.fact {
            SessionFact::LegacySeeded { .. } => unreachable!(),
            SessionFact::UserMessageCommitted { message, .. } => {
                if envelope.step_id.is_some() || message.role != Role::User {
                    return Err(SessionFactError::Invariant(
                        "user message must have role=user and no step_id".into(),
                    ));
                }
                if turn.user_committed {
                    return Err(SessionFactError::Invariant(
                        "duplicate user message in one turn".into(),
                    ));
                }
                turn.user_committed = true;
            }
            SessionFact::TurnStarted { .. } => {
                let step = require_step()?;
                if step == 0 || turn.current_step.is_some_and(|current| step <= current) {
                    return Err(SessionFactError::Invariant(format!(
                        "step {step} is not strictly increasing"
                    )));
                }
                turn.current_step = Some(step);
            }
            SessionFact::AssistantDraftAppended {
                message_id,
                chunk_index,
                text,
            } => {
                let step = require_current_step(turn, require_step()?)?;
                if text.is_empty() {
                    return Err(SessionFactError::Invariant("empty assistant draft".into()));
                }
                if !turn.drafts.contains_key(message_id) {
                    turn.draft_order.push(message_id.clone());
                }
                let draft = turn
                    .drafts
                    .entry(message_id.clone())
                    .or_insert(DraftInvariant {
                        step,
                        ..DraftInvariant::default()
                    });
                if draft.step != step || draft.finalized || draft.next_chunk != *chunk_index {
                    return Err(SessionFactError::Invariant(format!(
                        "invalid draft chunk {message_id}#{chunk_index}"
                    )));
                }
                draft.next_chunk += 1;
                draft.text.push_str(text);
            }
            SessionFact::AssistantMessageCommitted {
                message_id,
                content_hash,
                message,
            } => {
                let step = require_current_step(turn, require_step()?)?;
                if message.role != Role::Assistant {
                    return Err(SessionFactError::Invariant(
                        "assistant commit must have role=assistant".into(),
                    ));
                }
                if stable_message_hash(message) != *content_hash {
                    return Err(SessionFactError::Invariant(
                        "assistant content_hash mismatch".into(),
                    ));
                }
                if !turn.drafts.contains_key(message_id) {
                    turn.draft_order.push(message_id.clone());
                }
                let draft = turn
                    .drafts
                    .entry(message_id.clone())
                    .or_insert(DraftInvariant {
                        step,
                        ..DraftInvariant::default()
                    });
                if draft.step != step || draft.finalized {
                    return Err(SessionFactError::Invariant(format!(
                        "assistant message {message_id} already finalized or crosses step"
                    )));
                }
                let committed_text = message
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        Part::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>();
                if draft.next_chunk > 0 && draft.text != committed_text {
                    return Err(SessionFactError::Invariant(format!(
                        "assistant draft replay mismatch for {message_id}"
                    )));
                }
                draft.finalized = true;
                for part in &message.parts {
                    if let Part::ToolCall { id, .. } = part {
                        if turn.declared_calls.insert(id.clone(), step).is_some() {
                            return Err(SessionFactError::Invariant(format!(
                                "duplicate declared tool call {id}"
                            )));
                        }
                    }
                }
            }
            SessionFact::AssistantMessageInterrupted { message_id, .. } => {
                let step = require_current_step(turn, require_step()?)?;
                let draft = turn.drafts.get_mut(message_id).ok_or_else(|| {
                    SessionFactError::Invariant(format!(
                        "interrupted assistant {message_id} has no draft"
                    ))
                })?;
                if draft.step != step || draft.finalized {
                    return Err(SessionFactError::Invariant(format!(
                        "assistant interruption {message_id} is duplicate or crosses step"
                    )));
                }
                draft.finalized = true;
            }
            SessionFact::ToolCalled { call_id, .. } => {
                let step = require_current_step(turn, require_step()?)?;
                if turn.declared_calls.get(call_id) != Some(&step) {
                    return Err(SessionFactError::Invariant(format!(
                        "tool call {call_id} was not declared by assistant in step {step}"
                    )));
                }
                if turn.calls.contains_key(call_id) {
                    return Err(SessionFactError::Invariant(format!(
                        "duplicate tool call {call_id}"
                    )));
                }
                turn.calls.insert(
                    call_id.clone(),
                    ToolInvariant {
                        step,
                        resolved: false,
                    },
                );
                turn.call_order.push(call_id.clone());
            }
            SessionFact::ToolResultCommitted { call_id, .. }
            | SessionFact::ToolResultInterrupted { call_id, .. } => {
                let step = require_current_step(turn, require_step()?)?;
                let call = turn.calls.get_mut(call_id).ok_or_else(|| {
                    SessionFactError::Invariant(format!(
                        "tool result {call_id} has no matching call"
                    ))
                })?;
                if call.step != step {
                    return Err(SessionFactError::Invariant(format!(
                        "tool result {call_id} crosses step {} -> {step}",
                        call.step
                    )));
                }
                if call.resolved {
                    return Err(SessionFactError::Invariant(format!(
                        "duplicate tool result {call_id}"
                    )));
                }
                call.resolved = true;
            }
            SessionFact::TurnStopped
            | SessionFact::TurnCompleted
            | SessionFact::TurnFailed { .. } => {
                if envelope.step_id.is_some() {
                    return Err(SessionFactError::Invariant(
                        "turn terminal cannot carry step_id".into(),
                    ));
                }
                let open_draft = turn.drafts.values().any(|draft| !draft.finalized);
                let open_call = turn.calls.values().any(|call| !call.resolved);
                if open_draft || open_call {
                    return Err(SessionFactError::Invariant(
                        "turn terminal with open assistant draft or tool call".into(),
                    ));
                }
                turn.terminal = true;
            }
        }
        Ok(())
    }

    /// 为崩溃后仍开放的事实生成确定性闭合事件；调用方把它们与 failed terminal
    /// 放在同一事务提交，不会重新执行任何工具。
    pub fn recovery_facts(&self, reason: &str) -> Vec<SessionFactEnvelope> {
        let mut facts = Vec::new();
        for turn_id in &self.turn_order {
            let Some(turn) = self.turns.get(turn_id) else {
                continue;
            };
            if turn.terminal {
                continue;
            }
            for message_id in &turn.draft_order {
                if let Some(draft) = turn.drafts.get(message_id) {
                    if !draft.finalized && draft.next_chunk > 0 {
                        facts.push(SessionFactEnvelope::new(
                            turn_id,
                            Some(draft.step),
                            SessionFact::AssistantMessageInterrupted {
                                message_id: message_id.clone(),
                                reason: reason.into(),
                                superseded: false,
                            },
                        ));
                    }
                }
            }
            for call_id in &turn.call_order {
                if let Some(call) = turn.calls.get(call_id) {
                    if !call.resolved {
                        facts.push(SessionFactEnvelope::new(
                            turn_id,
                            Some(call.step),
                            SessionFact::ToolResultInterrupted {
                                call_id: call_id.clone(),
                                reason: reason.into(),
                            },
                        ));
                    }
                }
            }
            facts.push(SessionFactEnvelope::new(
                turn_id,
                None,
                SessionFact::TurnFailed {
                    error: reason.into(),
                },
            ));
        }
        facts
    }
}

fn require_current_step(turn: &TurnInvariant, step: u32) -> Result<u32, SessionFactError> {
    if turn.current_step != Some(step) {
        return Err(SessionFactError::Invariant(format!(
            "event step {step} does not match current step {:?}",
            turn.current_step
        )));
    }
    Ok(step)
}

pub fn stable_json_hash<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub fn stable_message_hash(message: &Message) -> String {
    stable_json_hash(message)
}

fn is_fact_type(event_type: &str) -> bool {
    FACT_TYPES.contains(&event_type)
}

pub fn decode_session_fact(
    event: &StoredEvent,
) -> Result<Option<SessionFactEnvelope>, SessionFactError> {
    if !is_fact_type(&event.event_type) {
        return Ok(None);
    }
    let envelope: SessionFactEnvelope =
        serde_json::from_value(event.payload.clone()).map_err(StoreError::from)?;
    if envelope.event_type() != event.event_type {
        return Err(SessionFactError::Invariant(format!(
            "event_type {} disagrees with payload {}",
            event.event_type,
            envelope.event_type()
        )));
    }
    Ok(Some(envelope))
}

impl SessionStore {
    /// 库内该 turn 是否已有 terminal 事实(R-242 批5 / D-417)。
    ///
    /// 调用方内存 invariant 只反映它自己的写入;库内可能已有其它 writer /
    /// recovery 写入的 terminal(崩溃恢复闭合了主 writer 还在推进的 turn)。
    fn turn_has_terminal(&self, session_id: &str, turn_id: &str) -> Result<bool, SessionFactError> {
        let count: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM session_events
                 WHERE session_id = ?1 AND event_type IN (?2, ?3, ?4)
                   AND json_extract(payload_json, '$.turn_id') = ?5",
                params![
                    session_id,
                    TURN_STOPPED,
                    TURN_COMPLETED,
                    TURN_FAILED,
                    turn_id
                ],
                |row| row.get(0),
            )
            .map_err(StoreError::from)?;
        Ok(count > 0)
    }

    /// 一批 fact 在内存中完整过 invariant 后，于同一 SQLite 事务连续追加。
    pub fn append_session_facts_checked(
        &self,
        session_id: &str,
        invariant: &mut SessionInvariant,
        facts: &[SessionFactEnvelope],
    ) -> Result<Vec<StoredEvent>, SessionFactError> {
        // R-242 批5 / D-417:库内 terminal 预检。调用方 invariant 只反映它自己的
        // 内存写入,不知道库内其它 writer/recovery 已写入的 terminal——不加预检
        // 时「terminal 之后的事实」仍会落库,形成脏序列,让此后每一轮 prepare
        // 重建 invariant 失败、typed_write_errors 永久非零(实证:真实库 43 条
        // shadow report 携带同一 already-terminal 错误)。预检命中即整批拒绝。
        let batch_turns: HashSet<&str> = facts.iter().map(|fact| fact.turn_id.as_str()).collect();
        for turn_id in batch_turns {
            if self.turn_has_terminal(session_id, turn_id)? {
                return Err(SessionFactError::Invariant(format!(
                    "turn {turn_id} already terminal"
                )));
            }
        }
        let mut next = invariant.clone();
        for fact in facts {
            next.apply(fact)?;
        }
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(StoreError::from)?;
        let mut stored = Vec::with_capacity(facts.len());
        for fact in facts {
            let payload = serde_json::to_value(fact).map_err(StoreError::from)?;
            stored.push(append_event_tx(
                &tx,
                session_id,
                fact.event_type(),
                &payload,
            )?);
        }
        tx.commit().map_err(StoreError::from)?;
        *invariant = next;
        Ok(stored)
    }

    /// 读取本会话所有已知 format 的 typed facts；其它 session.* 事件原样跳过。
    ///
    /// D-375:LegacySeeded 只存引用,这里按 source_event_id 回读源快照把 messages 填回,
    /// 于是 `project_session_facts` 依旧是拿到完整 fact 的纯函数,调用方零改动。
    pub fn list_session_facts(
        &self,
        session_id: &str,
    ) -> Result<Vec<(StoredEvent, SessionFactEnvelope)>, SessionFactError> {
        let mut facts: Vec<(StoredEvent, SessionFactEnvelope)> = self
            .list_events(session_id, 0)?
            .into_iter()
            .filter_map(|event| match decode_session_fact(&event) {
                Ok(Some(fact)) => Some(Ok((event, fact))),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<_, _>>()?;
        for (_, envelope) in facts.iter_mut() {
            self.rehydrate_seed(&mut envelope.fact)?;
        }
        Ok(facts)
    }

    /// R-242/D-514:读取最新 conversation.reset 之后的 typed facts。
    /// reset 前事件继续保留在日志中，供旧 segment 审计与历史读取使用。
    pub fn list_latest_segment_facts(
        &self,
        session_id: &str,
    ) -> Result<Vec<(StoredEvent, SessionFactEnvelope)>, SessionFactError> {
        let facts = self.list_session_facts(session_id)?;
        let boundary = self
            .list_events_by_type(session_id, 0, "conversation.reset")?
            .into_iter()
            .map(|event| event.sequence)
            .next_back();
        Ok(match boundary {
            Some(sequence) => facts
                .into_iter()
                .filter(|(event, _)| event.sequence > sequence)
                .collect(),
            None => facts,
        })
    }

    /// D-375:把只存引用的 LegacySeeded 补回 messages。
    ///
    /// 非空(存量 seed 自带副本)直接返回,不回读。源事件已被删除时留空并**不报错**:
    /// `clear_conversation` 与按序号删快照都会合法地抹掉源,那时这条 seed 本来就失去
    /// 意义,下一个快照会生成新的 seed;报错会让整条读路径为一条历史垃圾崩掉。
    fn rehydrate_seed(&self, fact: &mut SessionFact) -> Result<(), SessionFactError> {
        let SessionFact::LegacySeeded {
            source_event_id,
            messages,
            ..
        } = fact
        else {
            return Ok(());
        };
        if !messages.is_empty() {
            return Ok(());
        }
        let payload: Option<String> = self
            .connection
            .query_row(
                "SELECT payload_json FROM session_events WHERE event_id = ?1",
                params![source_event_id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)?;
        let Some(payload) = payload else {
            return Ok(());
        };
        let value: serde_json::Value = serde_json::from_str(&payload).map_err(StoreError::from)?;
        *messages = serde_json::from_value(
            value
                .get("messages")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .map_err(StoreError::from)?;
        Ok(())
    }

    /// 从最新 legacy conversation.updated 生成带 provenance 的 seed。同一 source event
    /// 重复调用为 no-op；新快照出现时追加新的 seed，投影器以最新 seed 为基线。
    pub fn seed_latest_legacy_snapshot(
        &self,
        session_id: &str,
    ) -> Result<Option<StoredEvent>, SessionFactError> {
        let Some(source) = self.latest_event(session_id, "conversation.updated")? else {
            return Ok(None);
        };
        let messages: Vec<Message> = serde_json::from_value(
            source
                .payload
                .get("messages")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .map_err(StoreError::from)?;
        let existing: Option<String> = self
            .connection
            .query_row(
                "SELECT payload_json FROM session_events
                 WHERE session_id = ?1 AND event_type = ?2
                 ORDER BY sequence DESC LIMIT 1",
                params![session_id, LEGACY_SEEDED],
                |row| row.get(0),
            )
            .optional()
            .map_err(StoreError::from)?;
        if let Some(existing) = existing {
            let envelope: SessionFactEnvelope =
                serde_json::from_str(&existing).map_err(StoreError::from)?;
            if matches!(
                envelope.fact,
                SessionFact::LegacySeeded {
                    source_sequence,
                    ref source_event_id,
                    ..
                } if source_sequence == source.sequence && source_event_id == &source.event_id
            ) {
                return Ok(None);
            }
        }
        let envelope = SessionFactEnvelope::new(
            format!("legacy:{}", source.sequence),
            None,
            SessionFact::LegacySeeded {
                source_event_id: source.event_id,
                source_sequence: source.sequence,
                // hash 仍按真实 messages 算:它是 provenance 的完整性锚点,
                // 回读源事件后可以据此发现源被改写(事件本应只追加)。
                source_hash: stable_json_hash(&messages),
                // D-375:引用而非副本——空 Vec 经 skip_serializing_if 不进 JSON。
                messages: Vec::new(),
            },
        );
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(StoreError::from)?;
        // 跨进程重入时在事务内再核对一次同一 source_sequence。
        let duplicate: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM session_events
                 WHERE session_id = ?1 AND event_type = ?2
                   AND json_extract(payload_json, '$.fact.source_sequence') = ?3",
                params![session_id, LEGACY_SEEDED, source.sequence],
                |row| row.get(0),
            )
            .map_err(StoreError::from)?;
        if duplicate > 0 {
            tx.rollback().map_err(StoreError::from)?;
            return Ok(None);
        }
        let payload = serde_json::to_value(&envelope).map_err(StoreError::from)?;
        let stored = append_event_tx(&tx, session_id, LEGACY_SEEDED, &payload)?;
        tx.commit().map_err(StoreError::from)?;
        Ok(Some(stored))
    }

    /// 启动新 turn 前闭合上次崩溃留下的 draft/call；不重放有副作用工具。
    pub fn recover_interrupted_session_facts(
        &self,
        session_id: &str,
        reason: &str,
    ) -> Result<RecoveryReport, SessionFactError> {
        let facts = self.list_session_facts(session_id)?;
        let mut invariant = SessionInvariant::default();
        let mut skipped_post_terminal = 0usize;
        for (_, fact) in &facts {
            match invariant.apply(fact) {
                Ok(()) => {}
                // R-242 批5 / D-417:历史脏序列——旧版 append 不查库内既有
                // terminal,曾产生「terminal 之后的事实仍落库」的脏条。跳过不
                // 阻塞 prepare;未来由 append_session_facts_checked 的库内
                // terminal 预检杜绝新脏序列。
                Err(SessionFactError::Invariant(message))
                    if message.contains("already terminal") =>
                {
                    skipped_post_terminal += 1;
                }
                Err(error) => return Err(error),
            }
        }
        let recovery = invariant.recovery_facts(reason);
        if recovery.is_empty() {
            return Ok(RecoveryReport {
                closed_events: 0,
                skipped_post_terminal,
            });
        }
        self.append_session_facts_checked(session_id, &mut invariant, &recovery)?;
        Ok(RecoveryReport {
            closed_events: recovery.len(),
            skipped_post_terminal,
        })
    }

    /// R-279:从事件日志恢复指定子代理的最新 transcript(快照式事件恢复)。
    ///
    /// 事件类型 `subagent.transcript`(非 typed fact),payload 含 call_id +
    /// 完整消息历史;多个事件(同 id 多次运行)取最新。无匹配返回 None。
    pub fn recover_subagent_transcript(
        &self,
        session_id: &str,
        call_id: &str,
    ) -> Result<Option<Vec<Message>>, SessionFactError> {
        let mut latest = None;
        for event in self.list_events_by_type(session_id, 0, SUBAGENT_TRANSCRIPT)? {
            if event.payload["call_id"].as_str() == Some(call_id) {
                if let Ok(messages) =
                    serde_json::from_value::<Vec<Message>>(event.payload["messages"].clone())
                {
                    latest = Some(messages);
                }
            }
        }
        Ok(latest)
    }
}

const DRAFT_BATCH_CHARS: usize = 2 * 1024;
const DRAFT_BATCH_AGE: Duration = Duration::from_millis(750);

/// `recover_interrupted_session_facts` 的结果(R-242 批5 / D-417)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryReport {
    /// 本次闭合的 open draft/call 事件数(含 turn failed)。
    pub closed_events: usize,
    /// 重建 invariant 时跳过的历史「terminal 后追加」脏条数。
    /// 旧版 append 不查库内既有 terminal 的产物;未来由库内 terminal
    /// 预检杜绝新脏序列,此计数只反映存量历史数据。
    pub skipped_post_terminal: usize,
}

/// 新 turn 开始前的兼容准备：latest legacy snapshot 幂等 seed，再闭合上一次
/// 进程崩溃遗留的 open draft/tool。两步都只追加事实，不改 legacy snapshot。
pub fn prepare_typed_session(
    store: &SessionStore,
    session_id: &str,
) -> Result<(), SessionFactError> {
    store.seed_latest_legacy_snapshot(session_id)?;
    store.recover_interrupted_session_facts(session_id, "process_restarted")?;
    Ok(())
}

struct WriterDraft {
    step: u32,
    attempt: u32,
    chunk_index: u32,
    buffer: String,
    last_flush: Instant,
    finalized: bool,
}

impl Default for WriterDraft {
    fn default() -> Self {
        Self {
            step: 0,
            attempt: 0,
            chunk_index: 0,
            buffer: String::new(),
            last_flush: Instant::now(),
            finalized: true,
        }
    }
}

impl WriterDraft {
    fn begin_step(&mut self, step: u32) {
        self.step = step;
        self.attempt = 0;
        self.chunk_index = 0;
        self.buffer.clear();
        self.last_flush = Instant::now();
        self.finalized = false;
    }

    fn message_id(&self, turn_id: &str) -> String {
        format!("{turn_id}:assistant:{}:{}", self.step, self.attempt)
    }

    fn has_persistable_text(&self) -> bool {
        self.chunk_index > 0 || !self.buffer.is_empty()
    }
}

/// CLI 与桌面端共用的 typed fact writer。它不持有 SQLite connection，可安全跨
/// async 事件保存；每次 flush 短开连接，所有事实仍走 core 的 invariant + 事务入口。
pub struct TypedSessionWriter {
    state_path: PathBuf,
    session_id: String,
    turn_id: String,
    invariant: SessionInvariant,
    draft: WriterDraft,
    logical_step: u32,
    source_step: Option<u32>,
    open_calls: HashSet<String>,
    errors: Vec<String>,
    terminal: bool,
}

impl TypedSessionWriter {
    pub fn new(state_path: &Path, session_id: &str, turn_id: &str) -> Self {
        Self {
            state_path: state_path.to_path_buf(),
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            invariant: SessionInvariant::default(),
            draft: WriterDraft::default(),
            logical_step: 0,
            source_step: None,
            open_calls: HashSet::new(),
            errors: Vec::new(),
            terminal: false,
        }
    }

    fn append(&mut self, facts: Vec<SessionFactEnvelope>) -> bool {
        if facts.is_empty() {
            return true;
        }
        let store = match SessionStore::open(&self.state_path) {
            Ok(store) => store,
            Err(error) => {
                self.errors.push(error.to_string());
                return false;
            }
        };
        match store.append_session_facts_checked(&self.session_id, &mut self.invariant, &facts) {
            Ok(_) => true,
            Err(error) => {
                self.errors.push(error.to_string());
                false
            }
        }
    }

    pub fn user_message(&mut self, input_id: &str, message: Message) {
        self.append(vec![SessionFactEnvelope::new(
            &self.turn_id,
            None,
            SessionFact::UserMessageCommitted {
                input_id: input_id.into(),
                message,
            },
        )]);
    }

    /// source step 是单次 runner 调用内的局部编号；流水线可能多次从 1 开始。
    /// writer 另分配单调 logical step 落库，避免同一用户 turn 内跨阶段撞号。
    pub fn turn_started(&mut self, source_step: u32, max_steps: u32) {
        if self.terminal {
            return;
        }
        self.logical_step = self.logical_step.saturating_add(1);
        if self.append(vec![SessionFactEnvelope::new(
            &self.turn_id,
            Some(self.logical_step),
            SessionFact::TurnStarted { max_steps },
        )]) {
            self.source_step = Some(source_step);
            self.draft.begin_step(self.logical_step);
        }
    }

    pub fn push_text(&mut self, text: &str) {
        if self.terminal || text.is_empty() || self.draft.finalized {
            return;
        }
        self.draft.buffer.push_str(text);
        if self.draft.buffer.chars().count() >= DRAFT_BATCH_CHARS
            || self.draft.last_flush.elapsed() >= DRAFT_BATCH_AGE
        {
            self.flush_draft();
        }
    }

    fn flush_draft(&mut self) {
        if self.terminal || self.draft.buffer.is_empty() || self.draft.finalized {
            return;
        }
        let text = self.draft.buffer.clone();
        let chunk_index = self.draft.chunk_index;
        let message_id = self.draft.message_id(&self.turn_id);
        if self.append(vec![SessionFactEnvelope::new(
            &self.turn_id,
            Some(self.draft.step),
            SessionFact::AssistantDraftAppended {
                message_id,
                chunk_index,
                text,
            },
        )]) {
            self.draft.buffer.clear();
            self.draft.chunk_index += 1;
            self.draft.last_flush = Instant::now();
        }
    }

    /// 定时器调用：provider 暂停发 delta 时仍保证可见文本在 750ms 内落一批。
    pub fn flush_due(&mut self) {
        if !self.terminal
            && !self.draft.buffer.is_empty()
            && self.draft.last_flush.elapsed() >= DRAFT_BATCH_AGE
        {
            self.flush_draft();
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal
    }

    pub fn stream_restarted(&mut self) {
        if self.terminal {
            return;
        }
        self.flush_draft();
        if self.draft.chunk_index > 0 && !self.draft.finalized {
            let message_id = self.draft.message_id(&self.turn_id);
            if self.append(vec![SessionFactEnvelope::new(
                &self.turn_id,
                Some(self.draft.step),
                SessionFact::AssistantMessageInterrupted {
                    message_id,
                    reason: "stream_restarted".into(),
                    superseded: true,
                },
            )]) {
                self.draft.finalized = true;
            }
        }
        self.draft.attempt += 1;
        self.draft.chunk_index = 0;
        self.draft.buffer.clear();
        self.draft.last_flush = Instant::now();
        self.draft.finalized = false;
    }

    pub fn assistant_committed(&mut self, source_step: u32, message: Message) {
        if self.terminal {
            return;
        }
        if self.source_step != Some(source_step) {
            self.errors.push(format!(
                "assistant commit source step {source_step} != active source step {:?}",
                self.source_step
            ));
            return;
        }
        self.flush_draft();
        let message_id = self.draft.message_id(&self.turn_id);
        let mut facts = vec![SessionFactEnvelope::new(
            &self.turn_id,
            Some(self.draft.step),
            SessionFact::AssistantMessageCommitted {
                message_id,
                content_hash: stable_message_hash(&message),
                message: message.clone(),
            },
        )];
        let mut calls = Vec::new();
        for part in &message.parts {
            if let Part::ToolCall { id, name, input } = part {
                calls.push(id.clone());
                facts.push(SessionFactEnvelope::new(
                    &self.turn_id,
                    Some(self.draft.step),
                    SessionFact::ToolCalled {
                        call_id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    },
                ));
            }
        }
        if self.append(facts) {
            self.draft.finalized = true;
            self.open_calls.extend(calls);
        }
    }

    pub fn tool_results_committed(&mut self, source_step: u32, message: Message) {
        if self.terminal {
            return;
        }
        if self.source_step != Some(source_step) {
            self.errors.push(format!(
                "tool results source step {source_step} != active source step {:?}",
                self.source_step
            ));
            return;
        }
        let mut facts = Vec::new();
        let mut resolved = Vec::new();
        for part in message.parts {
            if let Part::ToolResult {
                call_id,
                content,
                is_error,
            } = part
            {
                resolved.push(call_id.clone());
                facts.push(SessionFactEnvelope::new(
                    &self.turn_id,
                    Some(self.draft.step),
                    SessionFact::ToolResultCommitted {
                        call_id,
                        content,
                        is_error,
                    },
                ));
            }
        }
        if self.append(facts) {
            for call_id in resolved {
                self.open_calls.remove(&call_id);
            }
        }
    }

    pub fn finish(&mut self, terminal: SessionTurnTerminal) {
        if self.terminal {
            return;
        }
        self.flush_draft();
        let mut facts = Vec::new();
        if self.draft.has_persistable_text() && !self.draft.finalized {
            facts.push(SessionFactEnvelope::new(
                &self.turn_id,
                Some(self.draft.step),
                SessionFact::AssistantMessageInterrupted {
                    message_id: self.draft.message_id(&self.turn_id),
                    reason: terminal.reason().into(),
                    superseded: false,
                },
            ));
        }
        let mut call_ids = self.open_calls.iter().cloned().collect::<Vec<_>>();
        call_ids.sort();
        for call_id in call_ids {
            facts.push(SessionFactEnvelope::new(
                &self.turn_id,
                Some(self.draft.step),
                SessionFact::ToolResultInterrupted {
                    call_id,
                    reason: terminal.reason().into(),
                },
            ));
        }
        facts.push(SessionFactEnvelope::new(
            &self.turn_id,
            None,
            terminal.into_fact(),
        ));
        if self.append(facts) {
            self.draft.finalized = true;
            self.open_calls.clear();
            self.terminal = true;
        }
    }

    pub fn write_shadow_report(&mut self, legacy: &[Message]) {
        let store = match SessionStore::open(&self.state_path) {
            Ok(store) => store,
            Err(error) => {
                self.errors.push(error.to_string());
                return;
            }
        };
        let facts = match store.list_latest_segment_facts(&self.session_id) {
            Ok(facts) => facts,
            Err(error) => {
                self.errors.push(error.to_string());
                return;
            }
        };
        let projection = project_session_facts(&facts);
        let mut comparison =
            match serde_json::to_value(compare_shadow_for_turn(&projection, legacy, &self.turn_id))
            {
                Ok(comparison) => comparison,
                Err(error) => {
                    self.errors.push(error.to_string());
                    return;
                }
            };
        comparison["turn_id"] = serde_json::json!(self.turn_id);
        comparison["typed_write_errors"] = serde_json::json!(self.errors);
        if let Err(error) =
            store.append_event(&self.session_id, "session.shadow_compared", &comparison)
        {
            self.errors.push(error.to_string());
        }
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    pub fn record_error(&mut self, error: impl std::fmt::Display) {
        self.errors.push(error.to_string());
    }
}

pub enum SessionTurnTerminal {
    Completed,
    Stopped,
    Failed(String),
}

impl SessionTurnTerminal {
    fn reason(&self) -> &str {
        match self {
            Self::Completed => "completed",
            Self::Stopped => "stopped_by_user",
            Self::Failed(error) => error,
        }
    }

    fn into_fact(self) -> SessionFact {
        match self {
            Self::Completed => SessionFact::TurnCompleted,
            Self::Stopped => SessionFact::TurnStopped,
            Self::Failed(error) => SessionFact::TurnFailed { error },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptedAssistant {
    pub turn_id: String,
    pub step_id: u32,
    pub message_id: String,
    pub text: String,
    pub reason: String,
    pub superseded: bool,
    pub materialized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionProjection {
    pub format_version: u32,
    pub seed_source_sequence: Option<i64>,
    pub surface_messages: Vec<Message>,
    pub transcript_messages: Vec<Message>,
    pub interrupted_assistants: Vec<InterruptedAssistant>,
    pub diagnostics: Vec<String>,
}

#[derive(Default)]
struct ProjectedDraft {
    step: u32,
    text: String,
}

/// 相同事件序列必得逐字节相同 JSON；interrupted draft 进入 transcript/diagnostic，
/// 不进入模型 surface，也不会伪装成完整 assistant 回答。
pub fn project_session_facts(events: &[(StoredEvent, SessionFactEnvelope)]) -> SessionProjection {
    project_session_facts_with_surface(events, None, None)
}

/// 在完整 transcript 投影上追加已提交的 compaction surface。
/// surface 只替代模型上下文；transcript、诊断和中断草稿仍来自全部原始 typed facts。
pub fn project_session_facts_with_surface(
    events: &[(StoredEvent, SessionFactEnvelope)],
    surface_sequence: Option<i64>,
    surface_messages: Option<Vec<Message>>,
) -> SessionProjection {
    let seed_index = events
        .iter()
        .rposition(|(_, envelope)| matches!(envelope.fact, SessionFact::LegacySeeded { .. }));
    let mut projection = SessionProjection {
        format_version: SESSION_EVENT_FORMAT_VERSION,
        seed_source_sequence: None,
        surface_messages: Vec::new(),
        transcript_messages: Vec::new(),
        interrupted_assistants: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut drafts: HashMap<(String, String), ProjectedDraft> = HashMap::new();
    let mut result_group: Option<(String, u32, usize)> = None;
    let start = seed_index.unwrap_or(usize::MAX);
    if let Some(index) = seed_index {
        if let SessionFact::LegacySeeded {
            source_sequence,
            messages,
            ..
        } = &events[index].1.fact
        {
            projection.seed_source_sequence = Some(*source_sequence);
            projection.surface_messages = messages.clone();
            projection.transcript_messages = messages.clone();
        }
    }
    let iter_start = if start == usize::MAX { 0 } else { start + 1 };
    for (_, envelope) in events.iter().skip(iter_start) {
        let is_tool_result = matches!(
            envelope.fact,
            SessionFact::ToolResultCommitted { .. } | SessionFact::ToolResultInterrupted { .. }
        );
        if !is_tool_result {
            result_group = None;
        }
        match &envelope.fact {
            SessionFact::LegacySeeded { .. } | SessionFact::TurnStarted { .. } => {}
            SessionFact::UserMessageCommitted { message, .. } => {
                projection.surface_messages.push(message.clone());
                projection.transcript_messages.push(message.clone());
            }
            SessionFact::AssistantDraftAppended {
                message_id, text, ..
            } => {
                let draft = drafts
                    .entry((envelope.turn_id.clone(), message_id.clone()))
                    .or_insert(ProjectedDraft {
                        step: envelope.step_id.unwrap_or_default(),
                        text: String::new(),
                    });
                draft.text.push_str(text);
            }
            SessionFact::AssistantMessageCommitted {
                message_id,
                message,
                ..
            } => {
                drafts.remove(&(envelope.turn_id.clone(), message_id.clone()));
                projection.surface_messages.push(message.clone());
                projection.transcript_messages.push(message.clone());
            }
            SessionFact::AssistantMessageInterrupted {
                message_id,
                reason,
                superseded,
            } => {
                if let Some(draft) = drafts.remove(&(envelope.turn_id.clone(), message_id.clone()))
                {
                    push_interrupted_assistant(
                        &mut projection,
                        envelope,
                        message_id,
                        draft,
                        reason,
                        *superseded,
                        true,
                    );
                }
            }
            SessionFact::ToolCalled { .. } => {}
            SessionFact::ToolResultCommitted {
                call_id,
                content,
                is_error,
            } => push_projected_tool_result(
                &mut projection,
                &mut result_group,
                envelope,
                Part::ToolResult {
                    call_id: call_id.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                },
            ),
            SessionFact::ToolResultInterrupted { call_id, reason } => {
                projection.diagnostics.push(format!(
                    "tool {call_id} interrupted in {}: {reason}",
                    envelope.turn_id
                ));
                push_projected_tool_result(
                    &mut projection,
                    &mut result_group,
                    envelope,
                    Part::ToolResult {
                        call_id: call_id.clone(),
                        content: format!("interrupted: {reason}"),
                        is_error: true,
                    },
                );
            }
            SessionFact::TurnStopped => projection
                .diagnostics
                .push(format!("turn {} stopped", envelope.turn_id)),
            SessionFact::TurnFailed { error } => projection
                .diagnostics
                .push(format!("turn {} failed: {error}", envelope.turn_id)),
            SessionFact::TurnCompleted => {}
        }
    }
    let mut pending = drafts.into_iter().collect::<Vec<_>>();
    pending.sort_by(|a, b| a.0.cmp(&b.0));
    for ((turn_id, message_id), draft) in pending {
        let envelope = SessionFactEnvelope::new(
            turn_id,
            Some(draft.step),
            SessionFact::AssistantMessageInterrupted {
                message_id: message_id.clone(),
                reason: "missing_terminal".into(),
                superseded: false,
            },
        );
        push_interrupted_assistant(
            &mut projection,
            &envelope,
            &message_id,
            draft,
            "missing_terminal",
            false,
            false,
        );
    }
    if let (Some(sequence), Some(surface)) = (surface_sequence, surface_messages) {
        let suffix: Vec<_> = events
            .iter()
            .filter(|(event, _)| event.sequence > sequence)
            .cloned()
            .collect();
        let suffix_surface = project_session_facts(&suffix).surface_messages;
        projection.surface_messages = surface.into_iter().chain(suffix_surface).collect();
    }
    projection
}

fn push_interrupted_assistant(
    projection: &mut SessionProjection,
    envelope: &SessionFactEnvelope,
    message_id: &str,
    draft: ProjectedDraft,
    reason: &str,
    superseded: bool,
    materialized: bool,
) {
    projection
        .interrupted_assistants
        .push(InterruptedAssistant {
            turn_id: envelope.turn_id.clone(),
            step_id: draft.step,
            message_id: message_id.into(),
            text: draft.text.clone(),
            reason: reason.into(),
            superseded,
            materialized,
        });
    if !superseded && !draft.text.is_empty() {
        projection
            .transcript_messages
            .push(Message::assistant(vec![Part::Text {
                text: format!("{}\n\n[生成中断：{}]", draft.text, reason),
            }]));
    }
    projection.diagnostics.push(format!(
        "assistant {message_id} interrupted in {}: {reason}",
        envelope.turn_id
    ));
}

fn push_projected_tool_result(
    projection: &mut SessionProjection,
    result_group: &mut Option<(String, u32, usize)>,
    envelope: &SessionFactEnvelope,
    part: Part,
) {
    let step = envelope.step_id.unwrap_or_default();
    let same_group = result_group
        .as_ref()
        .is_some_and(|(turn, grouped_step, _)| turn == &envelope.turn_id && *grouped_step == step);
    if same_group {
        let index = result_group.as_ref().map(|(_, _, index)| *index).unwrap();
        projection.surface_messages[index].parts.push(part.clone());
        projection.transcript_messages[index].parts.push(part);
        return;
    }
    let index = projection.surface_messages.len();
    projection
        .surface_messages
        .push(Message::tool_results(vec![part.clone()]));
    projection
        .transcript_messages
        .push(Message::tool_results(vec![part]));
    *result_group = Some((envelope.turn_id.clone(), step, index));
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShadowComparison {
    pub equal: bool,
    pub legacy_hash: String,
    pub projection_hash: String,
    pub legacy_messages: usize,
    pub projected_messages: usize,
    pub first_mismatch: Option<usize>,
    pub interrupted_assistants: usize,
    pub diagnostics: Vec<String>,
    /// R-242 批4：差异归因。equal=false 时若为 true，说明该差异属于预期
    /// （失败轮 legacy 快照不更新 / 快照被清空 / 快照滞后于事件日志），
    /// 不影响「未知差异=0」的 shadow gate 达标判定；false 表示 equal 或未知差异。
    /// serde default 保证旧 shadow_compared 事件（无此字段）按 unknown 统计。
    #[serde(default)]
    pub expected_mismatch: bool,
    /// 预期差异类别：failed_turn / empty_legacy / stale_snapshot / compacted_snapshot；equal 与未知差异为 None。
    #[serde(default)]
    pub mismatch_class: Option<String>,
}

pub fn compare_shadow(projection: &SessionProjection, legacy: &[Message]) -> ShadowComparison {
    compare_shadow_with_diagnostics(projection, legacy, projection.diagnostics.clone())
}

/// 当前 turn 的 shadow 比较只携带当前 turn 的诊断，避免历史失败污染新 turn。
pub fn compare_shadow_for_turn(
    projection: &SessionProjection,
    legacy: &[Message],
    turn_id: &str,
) -> ShadowComparison {
    let diagnostics = projection
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to_turn(diagnostic, turn_id))
        .cloned()
        .collect();
    compare_shadow_with_diagnostics(projection, legacy, diagnostics)
}

fn compare_shadow_with_diagnostics(
    projection: &SessionProjection,
    legacy: &[Message],
    diagnostics: Vec<String>,
) -> ShadowComparison {
    let max = legacy.len().max(projection.surface_messages.len());
    let first_mismatch =
        (0..max).find(|index| legacy.get(*index) != projection.surface_messages.get(*index));
    let equal = first_mismatch.is_none();
    let (expected_mismatch, mismatch_class) = classify_mismatch(
        equal,
        first_mismatch,
        !diagnostics.is_empty(),
        projection,
        legacy,
    );
    ShadowComparison {
        equal,
        legacy_hash: stable_json_hash(&legacy),
        projection_hash: stable_json_hash(&projection.surface_messages),
        legacy_messages: legacy.len(),
        projected_messages: projection.surface_messages.len(),
        first_mismatch,
        interrupted_assistants: projection.interrupted_assistants.len(),
        diagnostics,
        expected_mismatch,
        mismatch_class,
    }
}

fn diagnostic_belongs_to_turn(diagnostic: &str, turn_id: &str) -> bool {
    diagnostic.contains(&format!("turn {turn_id} "))
        || diagnostic.contains(&format!(" in {turn_id}:"))
}

/// 差异归因（R-242 批4）：把可解释的差异标记为预期，剩余的 !equal 才是未知差异。
///
/// 三种预期场景（2026-08-16 对 60 条真实 shadow_compared 的全量取证，
/// 11 条 equal=false 全部归入预期、未知差异=0）加上 compaction 后的尾部 surface：
/// 1. failed_turn：投影 diagnostics 非空 —— 失败轮（process_restarted/transport
///    error/HTTP 503/turn failed）legacy 快照不更新而事件日志全程记录，投影恢复
///    失败轮草稿/中断消息（R-242 验收②③要的特性，不是差异）；
/// 2. empty_legacy：legacy 快照为空而投影非空 —— 快照被重建/清空而事件日志完整；
/// 3. stale_snapshot：legacy 是投影的完整前缀（前段逐条一致、投影更长）——
///    legacy 快照低频滞后于事件日志（会话内 conversation.updated 次数远少于
///    事件数），first_mismatch 落在 legacy 末端之后；
/// 4. compacted_snapshot：legacy 是 projection 的精确尾部 —— surface 已被压缩
///    替换，但原始事件仍保留，等待 R-243 事件化 compaction。
fn classify_mismatch(
    equal: bool,
    first_mismatch: Option<usize>,
    has_failure_diagnostic: bool,
    projection: &SessionProjection,
    legacy: &[Message],
) -> (bool, Option<String>) {
    if equal {
        return (false, None);
    }
    if has_failure_diagnostic {
        (true, Some("failed_turn".into()))
    } else if legacy.is_empty() {
        (true, Some("empty_legacy".into()))
    } else if legacy.len() < projection.surface_messages.len()
        && first_mismatch.is_some_and(|m| m >= legacy.len())
    {
        (true, Some("stale_snapshot".into()))
    } else if legacy.len() < projection.surface_messages.len()
        && projection.surface_messages.ends_with(legacy)
    {
        // Compaction 尚未事件化时,legacy snapshot 可能已经替换成较短的最新 surface,
        // 而 typed projection 仍保留压缩前的原始事件。只要 legacy 精确等于 projection
        // 的尾部,这是可解释的 surface 重建差异,不是中间事实被改写。
        (true, Some("compacted_snapshot".into()))
    } else {
        (false, None)
    }
}

/// R-242 批4：shadow gate 达标统计（验收⑤口径）。
///
/// 对 session.shadow_compared 事件按「未知差异=0」判定：equal 或 expected_mismatch
/// 的 turn 视为达标，unknown_mismatch 与 typed_write_error_turns 任一非零即不达标。
/// 旧事件（无 expected_mismatch 字段）按 unknown 统计，不静默放行。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ShadowVerdictStats {
    pub total: usize,
    pub equal: usize,
    pub expected_mismatch: usize,
    pub unknown_mismatch: usize,
    pub typed_write_error_turns: usize,
}

pub fn summarize_shadow_reports(events: &[StoredEvent]) -> ShadowVerdictStats {
    let mut stats = ShadowVerdictStats::default();
    for event in events {
        if event.event_type != "session.shadow_compared" {
            continue;
        }
        stats.total += 1;
        if event.payload["equal"].as_bool().unwrap_or(false) {
            stats.equal += 1;
        } else if event.payload["expected_mismatch"]
            .as_bool()
            .unwrap_or(false)
        {
            stats.expected_mismatch += 1;
        } else {
            stats.unknown_mismatch += 1;
        }
        if event.payload["typed_write_errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty())
        {
            stats.typed_write_error_turns += 1;
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::store;
    use serde_json::json;

    fn envelope(turn: &str, step: Option<u32>, fact: SessionFact) -> SessionFactEnvelope {
        SessionFactEnvelope::new(turn, step, fact)
    }

    fn assistant(text: &str) -> Message {
        Message::assistant(vec![Part::Text { text: text.into() }])
    }

    #[test]
    fn typed_fact_roundtrip_and_format_version() {
        let store = store();
        let mut invariant = SessionInvariant::default();
        let message = Message::user_text("你好");
        let facts = [
            envelope(
                "turn-1",
                None,
                SessionFact::UserMessageCommitted {
                    input_id: "input-1".into(),
                    message,
                },
            ),
            envelope("turn-1", Some(1), SessionFact::TurnStarted { max_steps: 4 }),
            envelope(
                "turn-1",
                Some(1),
                SessionFact::AssistantDraftAppended {
                    message_id: "m1".into(),
                    chunk_index: 0,
                    text: "完成".into(),
                },
            ),
        ];
        store
            .append_session_facts_checked("ses_test", &mut invariant, &facts)
            .unwrap();
        let read = store.list_session_facts("ses_test").unwrap();
        assert_eq!(read.len(), 3);
        assert_eq!(read[0].1, facts[0]);
        assert_eq!(read[2].1.format_version, SESSION_EVENT_FORMAT_VERSION);
    }

    #[test]
    fn invalid_batch_is_rejected_before_any_fact_is_persisted() {
        let store = store();
        let mut invariant = SessionInvariant::default();
        let mut unsupported =
            envelope("turn-1", Some(1), SessionFact::TurnStarted { max_steps: 4 });
        unsupported.format_version = SESSION_EVENT_FORMAT_VERSION + 1;
        let facts = [
            envelope(
                "turn-1",
                None,
                SessionFact::UserMessageCommitted {
                    input_id: "input-1".into(),
                    message: Message::user_text("不会半写入"),
                },
            ),
            unsupported,
        ];

        assert!(store
            .append_session_facts_checked("ses_test", &mut invariant, &facts)
            .unwrap_err()
            .to_string()
            .contains("unsupported format_version"));
        assert!(store.list_events("ses_test", 0).unwrap().is_empty());
    }

    #[test]
    fn flush_due_persists_short_draft_after_age_bound() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-typed-flush-{}-{}",
            std::process::id(),
            super::super::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state_path = root.join("state.db");
        {
            let store = SessionStore::open(&state_path).unwrap();
            store.create_session("ses", "test", None).unwrap();
        }
        let mut writer = TypedSessionWriter::new(&state_path, "ses", "turn");
        writer.turn_started(1, 1);
        writer.push_text("短草稿");
        {
            let store = SessionStore::open(&state_path).unwrap();
            assert!(store
                .list_events_by_type("ses", 0, ASSISTANT_DRAFT_APPENDED)
                .unwrap()
                .is_empty());
        }

        writer.draft.last_flush = Instant::now() - DRAFT_BATCH_AGE;
        writer.flush_due();
        {
            let store = SessionStore::open(&state_path).unwrap();
            assert_eq!(
                store
                    .list_events_by_type("ses", 0, ASSISTANT_DRAFT_APPENDED)
                    .unwrap()
                    .len(),
                1
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_writer_ignores_late_callbacks_without_errors_or_extra_terminal() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-typed-terminal-{}-{}",
            std::process::id(),
            super::super::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state_path = root.join("state.db");
        {
            let store = SessionStore::open(&state_path).unwrap();
            store.create_session("ses", "test", None).unwrap();
        }
        let mut writer = TypedSessionWriter::new(&state_path, "ses", "turn");
        writer.turn_started(1, 1);
        writer.finish(SessionTurnTerminal::Failed("transport error".into()));
        writer.turn_started(2, 1);
        writer.push_text("late delta");
        writer.stream_restarted();
        writer.assistant_committed(
            2,
            Message::assistant(vec![Part::Text {
                text: "late assistant".into(),
            }]),
        );
        writer.tool_results_committed(
            2,
            Message::assistant(vec![Part::ToolResult {
                call_id: "late-call".into(),
                content: "late result".into(),
                is_error: false,
            }]),
        );
        writer.flush_due();
        writer.finish(SessionTurnTerminal::Completed);

        let store = SessionStore::open(&state_path).unwrap();
        let facts = store.list_session_facts("ses").unwrap();
        assert!(
            writer.errors().is_empty(),
            "late callbacks added errors: {:?}",
            writer.errors()
        );
        assert_eq!(
            facts
                .iter()
                .filter(|(_, envelope)| matches!(envelope.fact, SessionFact::TurnStarted { .. }))
                .count(),
            1
        );
        assert_eq!(
            facts
                .iter()
                .filter(|(_, envelope)| matches!(envelope.fact, SessionFact::TurnFailed { .. }))
                .count(),
            1
        );
        assert!(!facts.iter().any(|(_, envelope)| matches!(
            envelope.fact,
            SessionFact::TurnCompleted
                | SessionFact::AssistantMessageCommitted { .. }
                | SessionFact::ToolResultCommitted { .. }
        )));
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invariant_rejects_duplicate_result_cross_step_and_post_terminal() {
        let mut invariant = SessionInvariant::default();
        let message = Message::assistant(vec![Part::ToolCall {
            id: "c1".into(),
            name: "read".into(),
            input: serde_json::json!({"path":"a"}),
        }]);
        for fact in [
            envelope("t", Some(1), SessionFact::TurnStarted { max_steps: 3 }),
            envelope(
                "t",
                Some(1),
                SessionFact::AssistantMessageCommitted {
                    message_id: "m1".into(),
                    content_hash: stable_message_hash(&message),
                    message,
                },
            ),
            envelope(
                "t",
                Some(1),
                SessionFact::ToolCalled {
                    call_id: "c1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path":"a"}),
                },
            ),
            envelope(
                "t",
                Some(1),
                SessionFact::ToolResultCommitted {
                    call_id: "c1".into(),
                    content: "ok".into(),
                    is_error: false,
                },
            ),
        ] {
            invariant.apply(&fact).unwrap();
        }
        assert!(invariant
            .apply(&envelope(
                "t",
                Some(1),
                SessionFact::ToolResultCommitted {
                    call_id: "c1".into(),
                    content: "again".into(),
                    is_error: false,
                },
            ))
            .unwrap_err()
            .to_string()
            .contains("duplicate tool result"));
        let mut cross = invariant.clone();
        cross
            .apply(&envelope(
                "t",
                Some(2),
                SessionFact::TurnStarted { max_steps: 3 },
            ))
            .unwrap();
        assert!(cross
            .apply(&envelope(
                "t",
                Some(2),
                SessionFact::ToolResultInterrupted {
                    call_id: "c1".into(),
                    reason: "late".into(),
                },
            ))
            .is_err());
        invariant
            .apply(&envelope("t", None, SessionFact::TurnCompleted))
            .unwrap();
        assert!(invariant
            .apply(&envelope(
                "t",
                Some(2),
                SessionFact::TurnStarted { max_steps: 3 },
            ))
            .is_err());
    }

    #[test]
    fn append_rejects_facts_for_turn_already_terminal_in_db() {
        // R-242 批5 / D-417:调用方内存 invariant 不知道库内已存在的 terminal
        // (跨 writer / recovery 写入),append 必须整批拒绝而不是继续落库。
        let store = store();
        let mut writer_invariant = SessionInvariant::default();
        store
            .append_session_facts_checked(
                "ses_test",
                &mut writer_invariant,
                &[
                    envelope(
                        "turn-x",
                        None,
                        SessionFact::UserMessageCommitted {
                            input_id: "i".into(),
                            message: Message::user_text("q"),
                        },
                    ),
                    envelope("turn-x", Some(1), SessionFact::TurnStarted { max_steps: 1 }),
                    envelope("turn-x", None, SessionFact::TurnStopped),
                ],
            )
            .unwrap();
        // 新 writer:内存 invariant 完全不知道库内 turn-x 已 terminal。
        let mut fresh = SessionInvariant::default();
        let error = store
            .append_session_facts_checked(
                "ses_test",
                &mut fresh,
                &[envelope(
                    "turn-x",
                    None,
                    SessionFact::UserMessageCommitted {
                        input_id: "i2".into(),
                        message: Message::user_text("q2"),
                    },
                )],
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("already terminal"), "{error}");
        // 整批拒绝,未落任何新事件(第一批 3 条仍在)。
        assert_eq!(store.list_events("ses_test", 0).unwrap().len(), 3);
    }

    #[test]
    fn recover_tolerates_historical_post_terminal_append() {
        // R-242 批5 / D-417:旧版 append 不查库内既有 terminal,曾产生
        // 「terminal 之后的事实仍落库」的历史脏序列。prepare 重建 invariant
        // 遇脏条必须跳过而非失败,否则每轮 prepare 都报错、typed_write_errors
        // 永久非零。未来脏序列由库内 terminal 预检杜绝。
        let store = store();
        let dirty = [
            SessionFactEnvelope::new("t", Some(1), SessionFact::TurnStarted { max_steps: 0 }),
            SessionFactEnvelope::new(
                "t",
                None,
                SessionFact::TurnFailed {
                    error: "crash".into(),
                },
            ),
            // terminal 之后仍落库的脏条(直接 append_event 绕过 checked 入口)。
            SessionFactEnvelope::new(
                "t",
                Some(1),
                SessionFact::ToolResultCommitted {
                    call_id: "c".into(),
                    content: "ok".into(),
                    is_error: false,
                },
            ),
        ];
        for fact in &dirty {
            store
                .append_event(
                    "ses_test",
                    fact.event_type(),
                    &serde_json::to_value(fact).unwrap(),
                )
                .unwrap();
        }
        // 修复前:重建 invariant 撞 already terminal 报错;修复后:跳过脏条,成功。
        let report = store
            .recover_interrupted_session_facts("ses_test", "process_restarted")
            .unwrap();
        // turn t 已 terminal,无 open draft/call → 无 recovery 闭合事件;
        // 历史脏条(terminal 后追加的 tool result)被跳过计数。
        assert_eq!(report.closed_events, 0);
        assert_eq!(report.skipped_post_terminal, 1);
    }
    #[test]
    fn recover_subagent_transcript_reads_latest_event_for_call_id() {
        // R-279:subagent.transcript 事件按 call_id 恢复,同 id 多次运行取最新。
        let store = store();
        store.create_session("ses", "t", None).unwrap();
        store
            .append_event(
                "ses",
                SUBAGENT_TRANSCRIPT,
                &serde_json::json!({
                    "call_id": "sub-a",
                    "messages": [Message::user_text("第一跑")]
                }),
            )
            .unwrap();
        store
            .append_event(
                "ses",
                SUBAGENT_TRANSCRIPT,
                &serde_json::json!({
                    "call_id": "sub-b",
                    "messages": [Message::user_text("其它子代理")]
                }),
            )
            .unwrap();
        store
            .append_event(
                "ses",
                SUBAGENT_TRANSCRIPT,
                &serde_json::json!({
                    "call_id": "sub-a",
                    "messages": [Message::user_text("第一跑"), Message::user_text("续跑")]
                }),
            )
            .unwrap();
        let recovered = store
            .recover_subagent_transcript("ses", "sub-a")
            .unwrap()
            .expect("sub-a 应有 transcript");
        assert_eq!(recovered.len(), 2, "同 id 多次运行取最新事件");
        // call_id 过滤:其它子代理的事件不串扰;无匹配返回 None。
        let other = store
            .recover_subagent_transcript("ses", "sub-b")
            .unwrap()
            .unwrap();
        assert_eq!(other.len(), 1);
        assert!(store
            .recover_subagent_transcript("ses", "sub-missing")
            .unwrap()
            .is_none());
    }

    #[test]
    fn latest_segment_facts_respect_reset_and_keep_old_facts_auditable() {
        let store = store();
        let old = assistant("旧 segment");
        let mut first = SessionInvariant::default();
        store
            .append_session_facts_checked(
                "ses_test",
                &mut first,
                &[
                    envelope(
                        "old-turn",
                        None,
                        SessionFact::UserMessageCommitted {
                            input_id: "old-input".into(),
                            message: Message::user_text("旧问题"),
                        },
                    ),
                    envelope(
                        "old-turn",
                        Some(1),
                        SessionFact::TurnStarted { max_steps: 1 },
                    ),
                    envelope(
                        "old-turn",
                        Some(1),
                        SessionFact::AssistantMessageCommitted {
                            message_id: "old-message".into(),
                            content_hash: stable_message_hash(&old),
                            message: old,
                        },
                    ),
                    envelope("old-turn", None, SessionFact::TurnCompleted),
                ],
            )
            .unwrap();
        store
            .append_event(
                "ses_test",
                "conversation.reset",
                &json!({ "cleared": true }),
            )
            .unwrap();
        let current = assistant("新 segment");
        let mut second = SessionInvariant::default();
        store
            .append_session_facts_checked(
                "ses_test",
                &mut second,
                &[
                    envelope(
                        "new-turn",
                        None,
                        SessionFact::UserMessageCommitted {
                            input_id: "new-input".into(),
                            message: Message::user_text("新问题"),
                        },
                    ),
                    envelope(
                        "new-turn",
                        Some(1),
                        SessionFact::TurnStarted { max_steps: 1 },
                    ),
                    envelope(
                        "new-turn",
                        Some(1),
                        SessionFact::AssistantMessageCommitted {
                            message_id: "new-message".into(),
                            content_hash: stable_message_hash(&current),
                            message: current,
                        },
                    ),
                    envelope("new-turn", None, SessionFact::TurnCompleted),
                ],
            )
            .unwrap();

        let all = store.list_session_facts("ses_test").unwrap();
        let latest = store.list_latest_segment_facts("ses_test").unwrap();
        assert_eq!(all.len(), 8, "旧 segment 事实必须继续可审计");
        assert_eq!(latest.len(), 4, "新 segment 只应包含 reset 之后的事实");
        let projection = project_session_facts(&latest);
        assert_eq!(projection.surface_messages.len(), 2);
        assert_eq!(
            projection.surface_messages[0].parts,
            Message::user_text("新问题").parts
        );
        assert_eq!(
            projection.surface_messages[1].parts,
            Message::assistant(vec![Part::Text {
                text: "新 segment".into()
            }])
            .parts
        );

        store
            .append_event(
                "ses_test",
                "conversation.reset",
                &json!({ "cleared": true }),
            )
            .unwrap();
        assert!(store
            .list_latest_segment_facts("ses_test")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn completed_compaction_surface_only_replaces_model_context() {
        let store = store();
        let original = vec![Message::user_text("原始事实"), assistant("原始回答")];
        let surface = vec![Message::user_text("压缩后的 surface")];
        let original_event = store
            .append_event(
                "ses_test",
                "conversation.updated",
                &json!({"messages": original}),
            )
            .unwrap();
        let compaction = store
            .append_compaction_transaction(
                "ses_test",
                "cmp-surface",
                &json!({"digest":"原始事实"}),
                &serde_json::to_value(&surface).unwrap(),
            )
            .unwrap();
        let facts = vec![(
            original_event,
            envelope(
                "seed",
                None,
                SessionFact::LegacySeeded {
                    source_event_id: "legacy".into(),
                    source_sequence: 1,
                    source_hash: "hash".into(),
                    messages: original.clone(),
                },
            ),
        )];
        let projection = project_session_facts_with_surface(
            &facts,
            Some(compaction[3].sequence),
            Some(surface.clone()),
        );
        assert_eq!(projection.surface_messages, surface);
        assert_eq!(projection.transcript_messages, original);
    }

    #[test]
    fn interrupted_draft_is_transcript_only_and_projection_is_deterministic() {
        let store = store();
        let mut invariant = SessionInvariant::default();
        let facts = [
            envelope(
                "t",
                None,
                SessionFact::UserMessageCommitted {
                    input_id: "i".into(),
                    message: Message::user_text("问题"),
                },
            ),
            envelope("t", Some(1), SessionFact::TurnStarted { max_steps: 0 }),
            envelope(
                "t",
                Some(1),
                SessionFact::AssistantDraftAppended {
                    message_id: "m".into(),
                    chunk_index: 0,
                    text: "生成到一半".into(),
                },
            ),
        ];
        store
            .append_session_facts_checked("ses_test", &mut invariant, &facts)
            .unwrap();
        let events = store.list_session_facts("ses_test").unwrap();
        let first = project_session_facts(&events);
        let second = project_session_facts(&events);
        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        assert_eq!(first.surface_messages.len(), 1, "draft 不进模型 surface");
        assert_eq!(
            first.transcript_messages.len(),
            2,
            "transcript 保留中断草稿"
        );
        assert_eq!(first.interrupted_assistants[0].text, "生成到一半");
        assert!(!first.interrupted_assistants[0].materialized);
    }

    /// D-375 验收①②:seed 落库是**引用**,读出来才补回 messages。
    #[test]
    fn legacy_seed_落库不含整包副本但读出来完整() {
        let store = store();
        let history: Vec<Message> = (0..40)
            .map(|i| Message::user_text(format!("第 {i} 条历史消息,凑出可观测的体积差")))
            .collect();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({ "messages": history }),
            )
            .unwrap();
        let seed = store
            .seed_latest_legacy_snapshot("ses_test")
            .unwrap()
            .unwrap();

        // ① 落库形态:payload 里根本没有 messages 这个键。
        assert!(
            seed.payload["fact"].get("messages").is_none(),
            "seed 又把整包 messages 抄进了落库形态(D-375):{}",
            seed.payload
        );
        let source_bytes = store
            .latest_event("ses_test", "conversation.updated")
            .unwrap()
            .unwrap()
            .payload
            .to_string()
            .len();
        let seed_bytes = seed.payload.to_string().len();
        assert!(
            seed_bytes * 10 < source_bytes,
            "seed({seed_bytes}B)相对源快照({source_bytes}B)没有数量级收缩,副本大概率又回来了"
        );

        // ② 读出来必须完整:投影器是纯函数,拿到的 fact 要跟从前一样带全 messages。
        let facts = store.list_session_facts("ses_test").unwrap();
        match &facts[0].1.fact {
            SessionFact::LegacySeeded { messages, .. } => {
                assert_eq!(messages.len(), 40, "回读没有把 messages 填回来");
                assert_eq!(messages[7], history[7]);
            }
            other => panic!("首条应为 LegacySeeded,实得 {other:?}"),
        }
        let projection = project_session_facts(&facts);
        assert_eq!(projection.surface_messages.len(), 40, "投影基线丢了");
        assert_eq!(projection.transcript_messages.len(), 40);
    }

    /// D-375 验收③:存量 seed 自带整包副本,必须照旧读得出来(不回读、不报错)。
    #[test]
    fn 存量带副本的seed照旧可读() {
        let store = store();
        let messages = vec![Message::user_text("存量副本")];
        // 手工写一条旧形态 seed:fact.messages 在 payload 里。
        let envelope = SessionFactEnvelope::new(
            "legacy:legacy-old".to_string(),
            None,
            SessionFact::LegacySeeded {
                source_event_id: "evt_不存在的源".to_string(),
                source_sequence: 1,
                source_hash: stable_json_hash(&messages),
                messages: messages.clone(),
            },
        );
        store
            .append_event(
                "ses_test",
                LEGACY_SEEDED,
                &serde_json::to_value(&envelope).unwrap(),
            )
            .unwrap();
        let facts = store.list_session_facts("ses_test").unwrap();
        match &facts[0].1.fact {
            // 源事件根本不存在,却仍然读得出内容 —— 副本没被回读逻辑覆盖掉。
            SessionFact::LegacySeeded { messages: m, .. } => assert_eq!(m, &messages),
            other => panic!("实得 {other:?}"),
        }
    }

    /// D-375 验收④:源快照被合法删除(clear_conversation / 按序号删)后,
    /// 读路径留空并继续,不为一条历史垃圾整体报错。
    #[test]
    fn 源快照被删后seed留空且不报错() {
        let store = store();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"messages":[Message::user_text("会被清掉")]}),
            )
            .unwrap();
        store.seed_latest_legacy_snapshot("ses_test").unwrap();
        store.clear_conversation("ses_test").unwrap();
        let facts = store.list_session_facts("ses_test").expect("不得整体报错");
        match &facts[0].1.fact {
            SessionFact::LegacySeeded { messages, .. } => assert!(messages.is_empty()),
            other => panic!("实得 {other:?}"),
        }
    }

    #[test]
    fn legacy_seed_is_idempotent_and_keeps_provenance() {
        let store = store();
        store
            .append_event(
                "ses_test",
                "conversation.updated",
                &serde_json::json!({"messages":[Message::user_text("旧历史")]}),
            )
            .unwrap();
        assert!(store
            .seed_latest_legacy_snapshot("ses_test")
            .unwrap()
            .is_some());
        assert!(store
            .seed_latest_legacy_snapshot("ses_test")
            .unwrap()
            .is_none());
        let facts = store.list_session_facts("ses_test").unwrap();
        assert_eq!(facts.len(), 1);
        match &facts[0].1.fact {
            SessionFact::LegacySeeded {
                source_event_id,
                source_sequence,
                source_hash,
                messages,
            } => {
                assert_eq!(source_event_id, "evt_ses_test_1");
                assert_eq!(*source_sequence, 1);
                assert!(source_hash.starts_with("sha256:"));
                assert_eq!(
                    messages[0].parts[0],
                    Part::Text {
                        text: "旧历史".into()
                    }
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn shadow_comparison_covers_normal_stop_denial_error_and_partial_tools() {
        fn compare_case(messages: Vec<Message>, terminal: SessionFact) -> ShadowComparison {
            let store = store();
            let mut invariant = SessionInvariant::default();
            let mut facts = vec![envelope(
                "case",
                None,
                SessionFact::UserMessageCommitted {
                    input_id: "i".into(),
                    message: messages[0].clone(),
                },
            )];
            if messages.len() > 1 {
                facts.push(envelope(
                    "case",
                    Some(1),
                    SessionFact::TurnStarted { max_steps: 1 },
                ));
                if messages[1].role == Role::Assistant {
                    let assistant = messages[1].clone();
                    facts.push(envelope(
                        "case",
                        Some(1),
                        SessionFact::AssistantMessageCommitted {
                            message_id: "m".into(),
                            content_hash: stable_message_hash(&assistant),
                            message: assistant.clone(),
                        },
                    ));
                    for part in &assistant.parts {
                        if let Part::ToolCall { id, name, input } = part {
                            facts.push(envelope(
                                "case",
                                Some(1),
                                SessionFact::ToolCalled {
                                    call_id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                },
                            ));
                        }
                    }
                }
            }
            for message in messages.iter().skip(2) {
                for part in &message.parts {
                    if let Part::ToolResult {
                        call_id,
                        content,
                        is_error,
                    } = part
                    {
                        facts.push(envelope(
                            "case",
                            Some(1),
                            SessionFact::ToolResultCommitted {
                                call_id: call_id.clone(),
                                content: content.clone(),
                                is_error: *is_error,
                            },
                        ));
                    }
                }
            }
            facts.push(envelope("case", None, terminal));
            store
                .append_session_facts_checked("ses_test", &mut invariant, &facts)
                .unwrap();
            let projected = project_session_facts(&store.list_session_facts("ses_test").unwrap());
            compare_shadow(&projected, &messages)
        }

        let normal = vec![Message::user_text("q"), assistant("done")];
        assert!(compare_case(normal, SessionFact::TurnCompleted).equal);

        let stopped = vec![Message::user_text("q"), assistant("stopped safely")];
        assert!(compare_case(stopped, SessionFact::TurnStopped).equal);

        for content in ["permission denied", "tool failed"] {
            let assistant_call = Message::assistant(vec![Part::ToolCall {
                id: "c".into(),
                name: "bash".into(),
                input: serde_json::json!({"command":"x"}),
            }]);
            let messages = vec![
                Message::user_text("q"),
                assistant_call,
                Message::tool_results(vec![Part::ToolResult {
                    call_id: "c".into(),
                    content: content.into(),
                    is_error: true,
                }]),
            ];
            assert!(compare_case(messages, SessionFact::TurnStopped).equal);
        }

        let partial = vec![
            Message::user_text("q"),
            Message::assistant(vec![
                Part::ToolCall {
                    id: "a".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path":"a"}),
                },
                Part::ToolCall {
                    id: "b".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path":"b"}),
                },
            ]),
            Message::tool_results(vec![
                Part::ToolResult {
                    call_id: "a".into(),
                    content: "ok".into(),
                    is_error: false,
                },
                Part::ToolResult {
                    call_id: "b".into(),
                    content: "cancelled".into(),
                    is_error: true,
                },
            ]),
        ];
        assert!(compare_case(partial, SessionFact::TurnStopped).equal);
    }

    #[test]
    fn shadow_mismatch_classification_distinguishes_expected_from_unknown() {
        fn projection_with(messages: Vec<Message>, diagnostics: Vec<String>) -> SessionProjection {
            SessionProjection {
                format_version: SESSION_EVENT_FORMAT_VERSION,
                seed_source_sequence: None,
                surface_messages: messages,
                transcript_messages: Vec::new(),
                interrupted_assistants: Vec::new(),
                diagnostics,
            }
        }

        // equal：无差异 → 不标记预期
        let c = compare_shadow(
            &projection_with(vec![assistant("a")], vec![]),
            &[assistant("a")],
        );
        assert!(c.equal);
        assert!(!c.expected_mismatch);
        assert_eq!(c.mismatch_class, None);

        // failed_turn：diagnostics 非空（失败轮 legacy 快照不更新）→ 预期
        let c = compare_shadow(
            &projection_with(
                vec![assistant("ok"), Message::user_text("q2")],
                vec!["turn t failed: boom".into()],
            ),
            &[assistant("ok")],
        );
        assert!(!c.equal);
        assert!(c.expected_mismatch);
        assert_eq!(c.mismatch_class.as_deref(), Some("failed_turn"));

        // empty_legacy：legacy 快照为空而投影非空 → 预期
        let c = compare_shadow(&projection_with(vec![assistant("ok")], vec![]), &[]);
        assert!(!c.equal);
        assert!(c.expected_mismatch);
        assert_eq!(c.mismatch_class.as_deref(), Some("empty_legacy"));

        // stale_snapshot：legacy 是投影完整前缀（快照滞后）→ 预期
        let c = compare_shadow(
            &projection_with(vec![assistant("a"), assistant("b"), assistant("c")], vec![]),
            &[assistant("a"), assistant("b")],
        );
        assert!(!c.equal);
        assert!(c.expected_mismatch);
        assert_eq!(c.mismatch_class.as_deref(), Some("stale_snapshot"));

        // compacted_snapshot：legacy 是 projection 的精确尾部（surface 已被压缩替换）→ 预期
        let c = compare_shadow(
            &projection_with(
                vec![assistant("old"), assistant("kept-a"), assistant("kept-b")],
                vec![],
            ),
            &[assistant("kept-a"), assistant("kept-b")],
        );
        assert!(!c.equal);
        assert!(c.expected_mismatch);
        assert_eq!(c.mismatch_class.as_deref(), Some("compacted_snapshot"));

        // unknown：中间一条不同（legacy 非前缀、投影非更长）→ 未知差异
        let c = compare_shadow(
            &projection_with(vec![assistant("a"), assistant("c")], vec![]),
            &[assistant("a"), assistant("b")],
        );
        assert!(!c.equal);
        assert!(!c.expected_mismatch);
        assert_eq!(c.mismatch_class, None);

        // unknown：legacy 比投影长（快照反超事件日志，需人工排查）→ 未知差异
        let c = compare_shadow(
            &projection_with(vec![assistant("a")], vec![]),
            &[assistant("a"), assistant("b")],
        );
        assert!(!c.equal);
        assert!(!c.expected_mismatch);
        assert_eq!(c.mismatch_class, None);
    }

    #[test]
    fn shadow_turn_diagnostics_do_not_leak_between_turns() {
        let projection = SessionProjection {
            format_version: SESSION_EVENT_FORMAT_VERSION,
            seed_source_sequence: None,
            surface_messages: vec![assistant("old"), Message::user_text("current")],
            transcript_messages: Vec::new(),
            interrupted_assistants: Vec::new(),
            diagnostics: vec!["turn turn-old failed: transport".into()],
        };
        let current = compare_shadow_for_turn(&projection, &[assistant("old")], "turn-current");
        assert!(!current.equal);
        assert_eq!(current.mismatch_class.as_deref(), Some("stale_snapshot"));
        assert!(current.diagnostics.is_empty());

        let failed_projection = SessionProjection {
            diagnostics: vec![
                "turn turn-old failed: transport".into(),
                "turn turn-current failed: timeout".into(),
            ],
            ..projection
        };
        let failed =
            compare_shadow_for_turn(&failed_projection, &[assistant("old")], "turn-current");
        assert!(failed.expected_mismatch);
        assert_eq!(failed.mismatch_class.as_deref(), Some("failed_turn"));
        assert_eq!(
            failed.diagnostics,
            vec!["turn turn-current failed: timeout"]
        );
    }

    #[test]
    fn summarize_shadow_reports_counts_verdicts_and_write_errors() {
        let store = store();
        store.create_session("ses", "t", None).unwrap();
        for payload in [
            json!({"equal": true, "typed_write_errors": []}),
            json!({"equal": false, "expected_mismatch": true, "mismatch_class": "failed_turn", "typed_write_errors": []}),
            json!({"equal": false, "expected_mismatch": false, "typed_write_errors": []}),
            // 旧事件无 expected_mismatch 字段 → 按 unknown 统计，不静默放行
            json!({"equal": false, "typed_write_errors": ["boom"]}),
        ] {
            store
                .append_event("ses", "session.shadow_compared", &payload)
                .unwrap();
        }
        // 无关事件类型不计数
        store
            .append_event("ses", "conversation.updated", &json!({"messages": []}))
            .unwrap();
        let events = store.list_events("ses", 0).unwrap();
        let stats = summarize_shadow_reports(&events);
        assert_eq!(stats.total, 4);
        assert_eq!(stats.equal, 1);
        assert_eq!(stats.expected_mismatch, 1);
        assert_eq!(stats.unknown_mismatch, 2);
        assert_eq!(stats.typed_write_error_turns, 1);
    }

    #[test]
    fn recovery_materializes_open_draft_and_tool_without_reexecution() {
        let store = store();
        let assistant = Message::assistant(vec![
            Part::Text {
                text: "half".into(),
            },
            Part::ToolCall {
                id: "c".into(),
                name: "bash".into(),
                input: serde_json::json!({"command":"side-effect"}),
            },
        ]);
        let mut invariant = SessionInvariant::default();
        let facts = [
            envelope("crash", Some(1), SessionFact::TurnStarted { max_steps: 0 }),
            envelope(
                "crash",
                Some(1),
                SessionFact::AssistantDraftAppended {
                    message_id: "m".into(),
                    chunk_index: 0,
                    text: "half".into(),
                },
            ),
            envelope(
                "crash",
                Some(1),
                SessionFact::AssistantMessageCommitted {
                    message_id: "m".into(),
                    content_hash: stable_message_hash(&assistant),
                    message: assistant,
                },
            ),
            envelope(
                "crash",
                Some(1),
                SessionFact::ToolCalled {
                    call_id: "c".into(),
                    name: "bash".into(),
                    input: serde_json::json!({"command":"side-effect"}),
                },
            ),
        ];
        store
            .append_session_facts_checked("ses_test", &mut invariant, &facts)
            .unwrap();
        let report = store
            .recover_interrupted_session_facts("ses_test", "process_restarted")
            .unwrap();
        assert_eq!(report.closed_events, 2, "tool interrupted + turn failed");
        assert_eq!(report.skipped_post_terminal, 0);
        assert_eq!(
            store
                .list_events_by_type("ses_test", 0, TOOL_RESULT_INTERRUPTED)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .recover_interrupted_session_facts("ses_test", "again")
                .unwrap()
                .closed_events,
            0,
            "重复恢复幂等"
        );
    }

    #[test]
    fn recovery_materializes_open_assistant_draft_as_interrupted() {
        let store = store();
        let mut invariant = SessionInvariant::default();
        let facts = [
            envelope(
                "draft-crash",
                Some(1),
                SessionFact::TurnStarted { max_steps: 0 },
            ),
            envelope(
                "draft-crash",
                Some(1),
                SessionFact::AssistantDraftAppended {
                    message_id: "m".into(),
                    chunk_index: 0,
                    text: "已经向用户显示的半段回答".into(),
                },
            ),
        ];
        store
            .append_session_facts_checked("ses_test", &mut invariant, &facts)
            .unwrap();
        assert_eq!(
            store
                .recover_interrupted_session_facts("ses_test", "process_restarted")
                .unwrap()
                .closed_events,
            2,
            "assistant interrupted + turn failed"
        );
        let projection = project_session_facts(&store.list_session_facts("ses_test").unwrap());
        assert_eq!(projection.surface_messages.len(), 0);
        assert_eq!(projection.transcript_messages.len(), 1);
        assert_eq!(
            projection.interrupted_assistants[0].text,
            "已经向用户显示的半段回答"
        );
        assert!(projection.interrupted_assistants[0].materialized);
        assert_eq!(
            store
                .recover_interrupted_session_facts("ses_test", "again")
                .unwrap()
                .closed_events,
            0
        );
    }

    #[test]
    fn invariant_rejects_commit_whose_message_disagrees_with_draft_replay() {
        let mut invariant = SessionInvariant::default();
        invariant
            .apply(&envelope(
                "t",
                Some(1),
                SessionFact::TurnStarted { max_steps: 0 },
            ))
            .unwrap();
        invariant
            .apply(&envelope(
                "t",
                Some(1),
                SessionFact::AssistantDraftAppended {
                    message_id: "m".into(),
                    chunk_index: 0,
                    text: "draft".into(),
                },
            ))
            .unwrap();
        let message = assistant("different");
        let error = invariant
            .apply(&envelope(
                "t",
                Some(1),
                SessionFact::AssistantMessageCommitted {
                    message_id: "m".into(),
                    content_hash: stable_message_hash(&message),
                    message,
                },
            ))
            .unwrap_err();
        assert!(error.to_string().contains("draft replay mismatch"));
    }

    #[test]
    fn concurrent_checked_typed_appends_keep_atomic_unique_sequence() {
        let root = std::env::temp_dir().join(format!(
            "kz_typed_concurrent_{}_{}",
            std::process::id(),
            super::super::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.db");
        let initializer = SessionStore::open(&path).unwrap();
        initializer
            .create_session("ses", "C:/project", None)
            .unwrap();
        drop(initializer);
        let handles = (0..4)
            .map(|worker| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let store = SessionStore::open(&path).unwrap();
                    let mut invariant = SessionInvariant::default();
                    let fact = envelope(
                        &format!("turn-{worker}"),
                        None,
                        SessionFact::UserMessageCommitted {
                            input_id: format!("input-{worker}"),
                            message: Message::user_text(format!("q{worker}")),
                        },
                    );
                    store
                        .append_session_facts_checked("ses", &mut invariant, &[fact])
                        .unwrap()[0]
                        .sequence
                })
            })
            .collect::<Vec<_>>();
        let mut sequences = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        assert_eq!(sequences, vec![1, 2, 3, 4]);
        let _ = std::fs::remove_dir_all(root);
    }
}
