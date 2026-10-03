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
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::events::{append_event_tx, event_from_row};
use super::{SessionStore, StoreError, StoredEvent};

mod projection;
pub use projection::{
    compare_shadow, compare_shadow_for_turn, project_session_facts,
    project_session_facts_with_surface, summarize_shadow_reports, InterruptedAssistant,
    SessionProjection, SessionTurnTerminal, ShadowComparison, ShadowVerdictStats,
};

pub const SESSION_EVENT_FORMAT_VERSION: u32 = 1;

pub const LEGACY_SEEDED: &str = "session.legacy_seeded";
pub const TURN_STARTED: &str = "session.turn_started";
pub const USER_MESSAGE_COMMITTED: &str = "session.user_message_committed";
pub const STEERING_MESSAGE_COMMITTED: &str = "session.steering_message_committed";
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

pub(crate) const FACT_TYPES: [&str; 13] = [
    LEGACY_SEEDED,
    TURN_STARTED,
    USER_MESSAGE_COMMITTED,
    STEERING_MESSAGE_COMMITTED,
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
    SteeringMessageCommitted {
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
            Self::SteeringMessageCommitted { .. } => STEERING_MESSAGE_COMMITTED,
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
    input_ids: HashSet<String>,
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
            SessionFact::UserMessageCommitted { input_id, message } => {
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
                turn.input_ids.insert(input_id.clone());
            }
            SessionFact::SteeringMessageCommitted { input_id, message } => {
                if !turn.user_committed
                    || message.role != Role::User
                    || envelope.step_id.is_some()
                    || input_id.is_empty()
                    || turn.input_ids.contains(input_id)
                    || turn.calls.values().any(|c| !c.resolved)
                    || turn.drafts.values().any(|d| !d.finalized)
                {
                    return Err(SessionFactError::Invariant(
                        "steering requires a unique input at a completed message boundary".into(),
                    ));
                }
                turn.input_ids.insert(input_id.clone());
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

    /// 在同一写事务中核对库内 turn 状态、验证整批 fact 并连续追加。
    pub fn append_session_facts_checked(
        &self,
        session_id: &str,
        invariant: &mut SessionInvariant,
        facts: &[SessionFactEnvelope],
    ) -> Result<Vec<StoredEvent>, SessionFactError> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(StoreError::from)?;
        let mut next = invariant.clone();
        let stored = self.append_session_facts_tx(&tx, session_id, &mut next, facts)?;
        tx.commit().map_err(StoreError::from)?;
        *invariant = next;
        Ok(stored)
    }

    /// 复用本连接已有的写事务；invariant 必须是提交成功后才采用的局部副本。
    pub(super) fn append_session_facts_tx(
        &self,
        tx: &Transaction<'_>,
        session_id: &str,
        invariant: &mut SessionInvariant,
        facts: &[SessionFactEnvelope],
    ) -> Result<Vec<StoredEvent>, SessionFactError> {
        // R-242 批5 / D-417:库内 terminal 预检。调用方 invariant 只反映它自己的
        // 内存写入,不知道库内其它 writer/recovery 已写入的 terminal——不加预检
        // 时「terminal 之后的事实」仍会落库。预检必须在拿到 writer 的事务内:
        // 事务外读到「尚未 terminal」后等待别的 writer,其提交 terminal 后旧预检
        // 已经失效。命中即整批拒绝,不推进调用方 invariant。
        let mut seen = HashSet::new();
        let mut next = invariant.clone();
        for turn_id in facts.iter().map(|fact| fact.turn_id.as_str()) {
            if !seen.insert(turn_id) {
                continue;
            }
            if self.turn_has_terminal(session_id, turn_id)? {
                return Err(SessionFactError::Invariant(format!(
                    "turn {turn_id} already terminal"
                )));
            }
            // 独立 writer/recovery 也可能已提交非 terminal 事实。只重建本批涉及
            // 的 turn,避免用 caller 的旧 draft/call 状态验证当前数据库,也不让
            // 无关旧 turn 的历史脏条阻断合法新 turn。
            let mut statement = tx
                .prepare(
                    "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                     FROM session_events
                     WHERE session_id = ?1 AND json_extract(payload_json, '$.turn_id') = ?2
                     ORDER BY sequence",
                )
                .map_err(StoreError::from)?;
            let events = statement
                .query_map(params![session_id, turn_id], event_from_row)
                .map_err(StoreError::from)?;
            let mut current = SessionInvariant::default();
            for event in events {
                let event = event.map_err(StoreError::from)?;
                if let Some(fact) = decode_session_fact(&event)? {
                    current.apply(&fact)?;
                }
            }
            next.turns.remove(turn_id);
            if let Some(turn) = current.turns.remove(turn_id) {
                next.turns.insert(turn_id.to_string(), turn);
                if !next.turn_order.iter().any(|id| id == turn_id) {
                    next.turn_order.push(turn_id.to_string());
                }
            } else {
                next.turn_order.retain(|id| id != turn_id);
            }
        }
        for fact in facts {
            next.apply(fact)?;
        }
        let mut stored = Vec::with_capacity(facts.len());
        for fact in facts {
            let payload = serde_json::to_value(fact).map_err(StoreError::from)?;
            stored.push(append_event_tx(
                tx,
                session_id,
                fact.event_type(),
                &payload,
            )?);
        }
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
    ///
    /// 只播种当前对话地板([`conversation_floor`](Self::conversation_floor))之后的
    /// 快照。每次开跑都会走到这里,而投影器把段内最新的 seed 当基线:不设地板时,
    /// 「新对话」之前的旧快照会被播种进新段(违背 D-427「reset 后 prior 必须为空」),
    /// 删掉当前段后更早的旧快照又成了「未播种的最新」,下一轮被整段塞回当前对话。
    pub fn seed_latest_legacy_snapshot(
        &self,
        session_id: &str,
    ) -> Result<Option<StoredEvent>, SessionFactError> {
        // source 与 floor 必须来自持有 writer 的同一快照；否则等待 writer 期间
        // 另一个连接提交 reset 后,旧 source 会被播种到新段。
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(StoreError::from)?;
        let Some(source) = self.latest_event(session_id, "conversation.updated")? else {
            return Ok(None);
        };
        if self
            .conversation_floor(session_id)?
            .is_some_and(|floor| source.sequence <= floor)
        {
            return Ok(None);
        }
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
                source_event_id: source.event_id.clone(),
                source_sequence: source.sequence,
                // hash 仍按真实 messages 算:它是 provenance 的完整性锚点,
                // 回读源事件后可以据此发现源被改写(事件本应只追加)。
                source_hash: stable_json_hash(&messages),
                // D-375:引用而非副本——空 Vec 经 skip_serializing_if 不进 JSON。
                messages: Vec::new(),
            },
        );
        // 跨进程重入时核对完整来源身份；fork 的外源序号不是本会话 source。
        let duplicate: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM session_events
                 WHERE session_id = ?1 AND event_type = ?2
                   AND json_extract(payload_json, '$.fact.source_sequence') = ?3
                   AND json_extract(payload_json, '$.fact.source_event_id') = ?4",
                params![session_id, LEGACY_SEEDED, source.sequence, source.event_id],
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
        let _owner = super::session_execution::lock_recovery(self, session_id)?;
        self.recover_interrupted_session_facts_owned(session_id, reason, None)
    }

    pub(super) fn recover_interrupted_session_facts_owned(
        &self,
        session_id: &str,
        reason: &str,
        current_input: Option<&str>,
    ) -> Result<RecoveryReport, SessionFactError> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(StoreError::from)?;
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
        if !recovery.is_empty() {
            self.append_session_facts_tx(&tx, session_id, &mut invariant, &recovery)?;
        }
        // An exclusive owner proves previous executions are gone. Preserve its
        // current claimed input, fail only orphaned claims, and leave pending
        // queues/steers available to their existing drain owner.
        super::inbox::fail_orphaned_execution_inputs_tx(&tx, session_id, current_input)?;
        tx.commit().map_err(StoreError::from)?;
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
    let _owner = super::session_execution::lock_recovery(store, session_id)?;
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
    _execution_owner: Option<std::sync::Arc<super::session_execution::SessionExecutionGuard>>,
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
            _execution_owner: None,
        }
    }

    pub(super) fn with_execution_owner(
        mut self,
        owner: std::sync::Arc<super::session_execution::SessionExecutionGuard>,
    ) -> Self {
        self._execution_owner = Some(owner);
        self
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

    /// Return the durable admission receipt; recorded errors are diagnostics,
    /// and a rejected initial user fact must not admit provider execution.
    pub fn user_message(&mut self, input_id: &str, message: Message) -> bool {
        self.append(vec![SessionFactEnvelope::new(
            &self.turn_id,
            None,
            SessionFact::UserMessageCommitted {
                input_id: input_id.into(),
                message,
            },
        )])
    }

    pub fn steering_message(&mut self, input_id: &str, message: Message) -> bool {
        self.append(vec![SessionFactEnvelope::new(
            &self.turn_id,
            None,
            SessionFact::SteeringMessageCommitted {
                input_id: input_id.into(),
                message,
            },
        )])
    }

    /// A delivery is visible only when promotion, typed facts and input receipts
    /// all commit. A failed delivery leaves the whole FIFO pending for retry.
    pub fn take_pending_steers(&mut self) -> Result<Vec<super::AdmittedInput>, SessionFactError> {
        let mut next_invariant = self.invariant.clone();
        let result = (|| {
            if self.terminal {
                return Err(SessionFactError::Invariant("turn already finalized".into()));
            }
            let store = SessionStore::open(&self.state_path)?;
            let tx = Transaction::new_unchecked(&store.connection, TransactionBehavior::Immediate)
                .map_err(StoreError::from)?;
            let inputs = store.promote_where_tx(&tx, &self.session_id, "steer", false)?;
            let facts = inputs
                .iter()
                .map(|input| {
                    SessionFactEnvelope::new(
                        &self.turn_id,
                        None,
                        SessionFact::SteeringMessageCommitted {
                            input_id: input.input_id.clone(),
                            message: Message::user_text(input.prompt.clone()),
                        },
                    )
                })
                .collect::<Vec<_>>();
            store.append_session_facts_tx(&tx, &self.session_id, &mut next_invariant, &facts)?;
            for input in &inputs {
                if !super::inbox::finish_input_in(&tx, &input.input_id, true)? {
                    return Err(SessionFactError::Invariant(format!(
                        "input {} is not active",
                        input.input_id
                    )));
                }
            }
            tx.commit().map_err(StoreError::from)?;
            Ok(inputs)
        })();
        match &result {
            Ok(_) => self.invariant = next_invariant,
            Err(error) => self.errors.push(error.to_string()),
        }
        result
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

    pub fn assistant_committed(&mut self, source_step: u32, message: Message) -> bool {
        if self.terminal {
            return false;
        }
        if self.source_step != Some(source_step) {
            self.errors.push(format!(
                "assistant commit source step {source_step} != active source step {:?}",
                self.source_step
            ));
            return false;
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
            true
        } else {
            false
        }
    }

    pub fn tool_results_committed(&mut self, source_step: u32, message: Message) -> bool {
        if self.terminal {
            return false;
        }
        if self.source_step != Some(source_step) {
            self.errors.push(format!(
                "tool results source step {source_step} != active source step {:?}",
                self.source_step
            ));
            return false;
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
            true
        } else {
            false
        }
    }

    pub fn finish(&mut self, terminal: SessionTurnTerminal) {
        if self.terminal {
            return;
        }
        self.flush_draft();
        let facts = self.terminal_facts(terminal);
        if self.append(facts) {
            self.mark_terminal();
        }
    }

    /// Commit the desktop round's terminal, input, lifecycle and result together.
    /// A rejected transaction leaves this writer open for a Failed retry.
    pub fn finish_with_input_outcome(
        &mut self,
        input_id: &str,
        terminal: SessionTurnTerminal,
        run_payload: &serde_json::Value,
    ) -> bool {
        if self.terminal {
            self.errors.push("round outcome already finalized".into());
            return false;
        }
        let stopped = matches!(&terminal, SessionTurnTerminal::Stopped);
        let ok = !matches!(&terminal, SessionTurnTerminal::Failed(_));
        self.flush_draft();
        let facts = self.terminal_facts(terminal);
        let mut next_invariant = self.invariant.clone();
        let result = (|| -> Result<(), SessionFactError> {
            let store = SessionStore::open(&self.state_path)?;
            let tx = Transaction::new_unchecked(&store.connection, TransactionBehavior::Immediate)
                .map_err(StoreError::from)?;
            store.append_session_facts_tx(&tx, &self.session_id, &mut next_invariant, &facts)?;
            let input_status: Option<String> = tx
                .query_row(
                    "SELECT status FROM session_inputs WHERE input_id = ?1 AND session_id = ?2",
                    params![input_id, self.session_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(StoreError::from)?;
            // D-342 stops cancel the input before the cooperative runner returns.
            // Preserve that cancellation; other terminal inputs cannot be reassigned.
            if !(stopped && input_status.as_deref() == Some("cancelled"))
                && (!matches!(input_status.as_deref(), Some("promoted" | "running"))
                    || !super::inbox::finish_input_in(&tx, input_id, ok)?)
            {
                return Err(SessionFactError::Invariant(format!(
                    "input {input_id} is not active in session {}",
                    self.session_id,
                )));
            }
            let status = if ok { "idle" } else { "failed" };
            super::session::set_status_in(&tx, &self.session_id, status)?;
            append_event_tx(
                &tx,
                &self.session_id,
                "session.status_changed",
                &serde_json::json!({ "status": status }),
            )?;
            append_event_tx(
                &tx,
                &self.session_id,
                if ok { "run.completed" } else { "run.failed" },
                run_payload,
            )?;
            tx.commit().map_err(StoreError::from)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.invariant = next_invariant;
                self.mark_terminal();
                true
            }
            Err(error) => {
                self.errors.push(error.to_string());
                false
            }
        }
    }

    fn terminal_facts(&self, terminal: SessionTurnTerminal) -> Vec<SessionFactEnvelope> {
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
        facts
    }

    fn mark_terminal(&mut self) {
        self.draft.finalized = true;
        self.open_calls.clear();
        self.terminal = true;
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

    /// 条目切换的候选 surface。来源必须仍匹配耐久投影；失败不得确认替换。
    pub fn commit_work_context_surface(
        &mut self,
        report: &crate::runner::WorkContextReport,
        source: &[Message],
        surface: &[Message],
    ) -> bool {
        if self.terminal || !self.open_calls.is_empty() || !self.errors.is_empty() {
            return false;
        }
        let result = (|| -> Result<bool, SessionFactError> {
            let store = SessionStore::open(&self.state_path)?;
            // Capture before every projection read; any writer during those reads
            // must invalidate CAS even when the source comparison already passed.
            let sequence = store.latest_event_sequence(&self.session_id)?;
            let floor = store.conversation_floor(&self.session_id)?.unwrap_or(0);
            let facts: Vec<_> = store
                .list_session_facts(&self.session_id)?
                .into_iter()
                .filter(|(event, _)| event.sequence > floor)
                .collect();
            let current =
                match store.latest_completed_compaction_surface(&self.session_id, floor)? {
                    Some((sequence, surface)) => {
                        project_session_facts_with_surface(&facts, Some(sequence), Some(surface))
                    }
                    None => project_session_facts(&facts),
                };
            if current.surface_messages.as_slice() != source {
                return Ok(false);
            }
            store.append_run_compaction_transaction_checked(
                &self.session_id,
                &format!("{}:work-context:{}", self.turn_id, self.logical_step),
                &serde_json::json!({ "reason": "work_item_switch", "report": report }),
                &serde_json::to_value(surface).map_err(StoreError::from)?,
                sequence,
            )?;
            Ok(true)
        })();
        match result {
            Ok(saved) => saved,
            Err(error) => {
                tracing::warn!(%error, "条目上下文投影保存失败，保留原上下文继续运行");
                false
            }
        }
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    pub fn record_error(&mut self, error: impl std::fmt::Display) {
        self.errors.push(error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::store;
    use serde_json::json;

    fn outcome_fixture() -> (PathBuf, SessionStore, TypedSessionWriter) {
        static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "kz-outcome-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = root.join("state.db");
        let store = SessionStore::open(&path).unwrap();
        store.create_session("ses", "test", None).unwrap();
        store
            .admit_input("ses", "input", "prompt", super::super::Delivery::Queue)
            .unwrap();
        store.promote_next_input("ses").unwrap().unwrap();
        assert!(store.start_input("input").unwrap());
        store.set_status("ses", "running").unwrap();
        let mut writer = TypedSessionWriter::new(&path, "ses", "turn");
        writer.user_message("input", Message::user_text("prompt"));
        writer.turn_started(1, 0);
        writer.push_text("unfinished");
        writer.flush_draft();
        (root, store, writer)
    }

    #[test]
    fn user_admission_ack_reports_sql_rejection_and_allows_exact_retry() {
        let (root, store, previous) = outcome_fixture();
        drop(previous);
        let mut writer = TypedSessionWriter::new(&root.join("state.db"), "ses", "admission");
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_admission BEFORE INSERT ON session_events
             WHEN NEW.event_type='session.user_message_committed'
             BEGIN SELECT RAISE(ABORT,'injected user rejection'); END;",
            )
            .unwrap();
        assert!(!writer.user_message("input", Message::user_text("rejected")));
        assert!(writer
            .errors()
            .last()
            .unwrap()
            .contains("injected user rejection"));
        assert!(store
            .list_session_facts("ses")
            .unwrap()
            .iter()
            .all(|(_, fact)| fact.turn_id != "admission"));
        store
            .connection
            .execute_batch("DROP TRIGGER reject_admission")
            .unwrap();
        assert!(writer.user_message("input", Message::user_text("accepted")));
        // A prior recorded error is not an admission receipt. The successful
        // exact retry updates the authoritative invariant only after commit.
        assert!(!writer.errors().is_empty());
        let facts = store.list_session_facts("ses").unwrap();
        assert_eq!(
            facts
                .iter()
                .filter(|(_, fact)| fact.turn_id == "admission")
                .count(),
            1
        );
        writer.finish(SessionTurnTerminal::Failed("done".into()));
        assert!(writer.is_terminal());
        assert!(!writer.user_message("input", Message::user_text("late callback")));
        assert_eq!(
            store
                .list_session_facts("ses")
                .unwrap()
                .iter()
                .filter(|(_, fact)| fact.turn_id == "admission")
                .count(),
            2
        );
        drop(writer);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn input_outcome_rejections_roll_back_terminal_input_status_and_memory() {
        for rejected in [
            "terminal",
            "input",
            "status",
            "status_event",
            "result_event",
        ] {
            let (root, store, mut writer) = outcome_fixture();
            let trigger = match rejected {
                "terminal" => "CREATE TRIGGER reject_outcome BEFORE INSERT ON session_events WHEN NEW.event_type='session.turn_completed' BEGIN SELECT RAISE(ABORT, 'reject terminal'); END",
                "input" => "CREATE TRIGGER reject_outcome BEFORE UPDATE OF status ON session_inputs WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT, 'reject input'); END",
                "status" => "CREATE TRIGGER reject_outcome BEFORE UPDATE OF status ON sessions WHEN NEW.status='idle' BEGIN SELECT RAISE(ABORT, 'reject status'); END",
                "status_event" => "CREATE TRIGGER reject_outcome BEFORE INSERT ON session_events WHEN NEW.event_type='session.status_changed' AND json_extract(NEW.payload_json, '$.status')='idle' BEGIN SELECT RAISE(ABORT, 'reject status event'); END",
                _ => "CREATE TRIGGER reject_outcome BEFORE INSERT ON session_events WHEN NEW.event_type='run.completed' BEGIN SELECT RAISE(ABORT, 'reject result event'); END",
            };
            store.connection.execute_batch(trigger).unwrap();
            let before = store.latest_event_sequence("ses").unwrap();
            assert!(
                !writer.finish_with_input_outcome(
                    "input",
                    SessionTurnTerminal::Completed,
                    &json!({"steps":1})
                ),
                "{rejected}"
            );
            assert!(!writer.is_terminal(), "{rejected}");
            assert!(!writer.invariant.turns["turn"].terminal, "{rejected}");
            assert!(!writer.draft.finalized, "{rejected}");
            assert_eq!(
                store.latest_event_sequence("ses").unwrap(),
                before,
                "{rejected}"
            );
            assert_eq!(
                store.input_status("input").unwrap().as_deref(),
                Some("running"),
                "{rejected}"
            );
            assert_eq!(
                store.get_session("ses").unwrap().unwrap().status,
                "running",
                "{rejected}"
            );
            assert!(
                writer.finish_with_input_outcome(
                    "input",
                    SessionTurnTerminal::Failed("rejected success".into()),
                    &json!({"error":"rejected success"})
                ),
                "{rejected}: {:?}",
                writer.errors()
            );
            assert!(writer.is_terminal());
            assert!(writer.invariant.turns["turn"].terminal);
            assert_eq!(
                store.input_status("input").unwrap().as_deref(),
                Some("failed")
            );
            assert_eq!(store.get_session("ses").unwrap().unwrap().status, "failed");
            assert!(store
                .list_events_by_type("ses", 0, TURN_COMPLETED)
                .unwrap()
                .is_empty());
            assert_eq!(
                store
                    .list_events_by_type("ses", 0, TURN_FAILED)
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                store
                    .list_events_by_type("ses", 0, "run.failed")
                    .unwrap()
                    .len(),
                1
            );
            drop(writer);
            drop(store);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn input_outcome_preserves_cancelled_stopped_and_rejects_completed_cancelled() {
        let (root, store, mut writer) = outcome_fixture();
        store.finalize_interrupt("ses").unwrap();
        let before = store.latest_event_sequence("ses").unwrap();
        assert!(!writer.finish_with_input_outcome(
            "input",
            SessionTurnTerminal::Completed,
            &json!({})
        ));
        assert!(!writer.is_terminal());
        assert_eq!(store.latest_event_sequence("ses").unwrap(), before);
        assert!(writer.finish_with_input_outcome(
            "input",
            SessionTurnTerminal::Stopped,
            &json!({"halted_by_user":true})
        ));
        assert_eq!(
            store.input_status("input").unwrap().as_deref(),
            Some("cancelled")
        );
        assert_eq!(store.get_session("ses").unwrap().unwrap().status, "idle");
        assert_eq!(
            store
                .list_events_by_type("ses", 0, TURN_STOPPED)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .list_events_by_type("ses", 0, "run.completed")
                .unwrap()
                .len(),
            1
        );
        drop(writer);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn input_outcome_rejects_other_session_missing_input_and_preexisting_terminal() {
        let (root, store, mut writer) = outcome_fixture();
        store.create_session("other", "test", None).unwrap();
        store
            .admit_input("other", "foreign", "other", super::super::Delivery::Queue)
            .unwrap();
        store.promote_next_input("other").unwrap().unwrap();
        let before = store.latest_event_sequence("ses").unwrap();
        for input in ["foreign", "missing"] {
            assert!(!writer.finish_with_input_outcome(
                input,
                SessionTurnTerminal::Completed,
                &json!({})
            ));
            assert!(!writer.is_terminal());
            assert_eq!(store.latest_event_sequence("ses").unwrap(), before);
            assert_eq!(
                store.input_status("input").unwrap().as_deref(),
                Some("running")
            );
            assert_eq!(
                store.input_status("foreign").unwrap().as_deref(),
                Some("promoted")
            );
        }
        writer.finish(SessionTurnTerminal::Completed);
        assert!(writer.is_terminal());
        assert!(!writer.finish_with_input_outcome(
            "input",
            SessionTurnTerminal::Completed,
            &json!({})
        ));
        assert_eq!(
            store.input_status("input").unwrap().as_deref(),
            Some("running")
        );
        assert!(store
            .list_events_by_type("ses", 0, "run.completed")
            .unwrap()
            .is_empty());
        drop(writer);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    thread_local! {
        static WRITER_WAIT: std::cell::RefCell<Option<(
            std::sync::mpsc::Sender<()>,
            std::sync::mpsc::Receiver<()>,
        )>> = const { std::cell::RefCell::new(None) };
    }

    fn pause_at_writer_lock(
        store: &SessionStore,
        blocked: std::sync::mpsc::Sender<()>,
        resume: std::sync::mpsc::Receiver<()>,
    ) {
        WRITER_WAIT.with(|wait| *wait.borrow_mut() = Some((blocked, resume)));
        store
            .connection
            .busy_handler(Some(|_| {
                WRITER_WAIT.with(|wait| {
                    wait.borrow_mut().take().is_some_and(|(blocked, resume)| {
                        blocked.send(()).is_ok()
                            && resume.recv_timeout(Duration::from_secs(10)).is_ok()
                    })
                })
            }))
            .unwrap();
    }

    fn temp_state_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kanzei-typed-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

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
        assert!(!writer.assistant_committed(
            2,
            Message::assistant(vec![Part::Text {
                text: "late assistant".into(),
            }]),
        ));
        assert!(!writer.tool_results_committed(
            2,
            Message::assistant(vec![Part::ToolResult {
                call_id: "late-call".into(),
                content: "late result".into(),
                is_error: false,
            }]),
        ));
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
    fn checked_append_rechecks_terminal_after_waiting_for_writer() {
        let root = temp_state_root("terminal-race");
        let state_path = root.join("state.db");
        let store = SessionStore::open(&state_path).unwrap();
        store.create_session("ses_test", "C:/proj", None).unwrap();
        let mut invariant = SessionInvariant::default();
        store
            .append_session_facts_checked(
                "ses_test",
                &mut invariant,
                &[envelope(
                    "turn",
                    Some(1),
                    SessionFact::TurnStarted { max_steps: 1 },
                )],
            )
            .unwrap();
        let before = invariant.recovery_facts("check");
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (start_tx, start_rx) = std::sync::mpsc::channel();
        let (blocked_tx, blocked_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let writer_path = state_path.clone();
        let writer = std::thread::spawn(move || {
            let store = SessionStore::open(&writer_path).unwrap();
            pause_at_writer_lock(&store, blocked_tx, resume_rx);
            ready_tx.send(()).unwrap();
            start_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let result = store.append_session_facts_checked(
                "ses_test",
                &mut invariant,
                &[envelope(
                    "turn",
                    Some(1),
                    SessionFact::AssistantDraftAppended {
                        message_id: "late".into(),
                        chunk_index: 0,
                        text: "terminal 后不得持久化".into(),
                    },
                )],
            );
            (result, invariant.recovery_facts("check"))
        });
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let tx = store.connection.unchecked_transaction().unwrap();
        let terminal = envelope("turn", None, SessionFact::TurnCompleted);
        append_event_tx(
            &tx,
            "ses_test",
            terminal.event_type(),
            &serde_json::to_value(terminal).unwrap(),
        )
        .unwrap();
        start_tx.send(()).unwrap();
        blocked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        tx.commit().unwrap();
        resume_tx.send(()).unwrap();
        let (result, after) = writer.join().unwrap();
        assert!(result.unwrap_err().to_string().contains("already terminal"));
        assert_eq!(after, before, "拒绝不得推进 caller invariant");
        assert_eq!(store.list_session_facts("ses_test").unwrap().len(), 2);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_seed_rechecks_floor_after_waiting_for_reset_writer() {
        let root = temp_state_root("seed-reset-race");
        let state_path = root.join("state.db");
        let store = SessionStore::open(&state_path).unwrap();
        store.create_session("ses_test", "C:/proj", None).unwrap();
        let legacy = store
            .append_event(
                "ses_test",
                "conversation.updated",
                &json!({"messages": [Message::user_text("reset 前的历史")]}),
            )
            .unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (start_tx, start_rx) = std::sync::mpsc::channel();
        let (blocked_tx, blocked_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let seed_path = state_path.clone();
        let seed = std::thread::spawn(move || {
            let store = SessionStore::open(&seed_path).unwrap();
            pause_at_writer_lock(&store, blocked_tx, resume_rx);
            ready_tx.send(()).unwrap();
            start_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            store.seed_latest_legacy_snapshot("ses_test")
        });
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let tx = store.connection.unchecked_transaction().unwrap();
        let reset = append_event_tx(
            &tx,
            "ses_test",
            "conversation.reset",
            &json!({"cleared": true, "source": "cli"}),
        )
        .unwrap();
        start_tx.send(()).unwrap();
        blocked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        tx.commit().unwrap();
        resume_tx.send(()).unwrap();
        assert!(seed.join().unwrap().unwrap().is_none());
        assert!(store
            .list_latest_segment_facts("ses_test")
            .unwrap()
            .is_empty());
        assert_eq!(
            store.conversation_floor("ses_test").unwrap(),
            Some(reset.sequence)
        );
        assert!(store
            .list_events_by_type("ses_test", 0, LEGACY_SEEDED)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .latest_event("ses_test", "conversation.updated")
                .unwrap()
                .unwrap()
                .event_id,
            legacy.event_id,
            "reset 与拒绝播种均不得删除原历史"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_rechecks_finalized_draft_after_waiting_for_writer() {
        let root = temp_state_root("recovery-race");
        let state_path = root.join("state.db");
        let store = SessionStore::open(&state_path).unwrap();
        store.create_session("ses_test", "C:/proj", None).unwrap();
        let mut invariant = SessionInvariant::default();
        store
            .append_session_facts_checked(
                "ses_test",
                &mut invariant,
                &[
                    envelope("turn", Some(1), SessionFact::TurnStarted { max_steps: 1 }),
                    envelope(
                        "turn",
                        Some(1),
                        SessionFact::AssistantDraftAppended {
                            message_id: "draft".into(),
                            chunk_index: 0,
                            text: "完整回答".into(),
                        },
                    ),
                ],
            )
            .unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (start_tx, start_rx) = std::sync::mpsc::channel();
        let (blocked_tx, blocked_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let recovery_path = state_path.clone();
        let recovery = std::thread::spawn(move || {
            let store = SessionStore::open(&recovery_path).unwrap();
            pause_at_writer_lock(&store, blocked_tx, resume_rx);
            ready_tx.send(()).unwrap();
            start_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            store.recover_interrupted_session_facts("ses_test", "process_restarted")
        });
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let tx = store.connection.unchecked_transaction().unwrap();
        let message = assistant("完整回答");
        let commit = envelope(
            "turn",
            Some(1),
            SessionFact::AssistantMessageCommitted {
                message_id: "draft".into(),
                content_hash: stable_message_hash(&message),
                message,
            },
        );
        append_event_tx(
            &tx,
            "ses_test",
            commit.event_type(),
            &serde_json::to_value(commit).unwrap(),
        )
        .unwrap();
        start_tx.send(()).unwrap();
        blocked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        tx.commit().unwrap();
        resume_tx.send(()).unwrap();
        assert_eq!(recovery.join().unwrap().unwrap().closed_events, 1);
        let facts = store.list_session_facts("ses_test").unwrap();
        assert_eq!(facts.len(), 4);
        assert!(!facts
            .iter()
            .any(|(_, fact)| matches!(fact.fact, SessionFact::AssistantMessageInterrupted { .. })));
        let mut replay = SessionInvariant::default();
        for (_, fact) in &facts {
            replay.apply(fact).unwrap();
        }
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checked_append_refreshes_touched_turn_without_losing_other_turns() {
        let store = store();
        let mut first = SessionInvariant::default();
        store
            .append_session_facts_checked(
                "ses_test",
                &mut first,
                &[
                    envelope("other", Some(1), SessionFact::TurnStarted { max_steps: 1 }),
                    envelope("turn", Some(1), SessionFact::TurnStarted { max_steps: 1 }),
                ],
            )
            .unwrap();
        let mut second = first.clone();
        let message = assistant("另一 writer 已提交");
        let commit = envelope(
            "turn",
            Some(1),
            SessionFact::AssistantMessageCommitted {
                message_id: "message".into(),
                content_hash: stable_message_hash(&message),
                message,
            },
        );
        store
            .append_session_facts_checked("ses_test", &mut second, std::slice::from_ref(&commit))
            .unwrap();
        let before = first.recovery_facts("check");
        assert!(store
            .append_session_facts_checked("ses_test", &mut first, &[commit])
            .unwrap_err()
            .to_string()
            .contains("already finalized"));
        assert_eq!(first.recovery_facts("check"), before);
        store
            .append_session_facts_checked(
                "ses_test",
                &mut first,
                &[envelope(
                    "turn",
                    Some(2),
                    SessionFact::TurnStarted { max_steps: 1 },
                )],
            )
            .unwrap();
        assert_eq!(first.turn_order, ["other", "turn"]);
        assert!(first.turns.contains_key("other"));
        assert!(first.turns["turn"].drafts["message"].finalized);
    }

    #[test]
    fn checked_append_write_failure_rolls_back_batch_and_caller_state() {
        let store = store();
        store
            .connection
            .execute_batch(
                "CREATE TEMP TRIGGER fail_draft BEFORE INSERT ON session_events
                 WHEN NEW.event_type = 'session.assistant_draft_appended'
                 BEGIN SELECT RAISE(ABORT, 'injected draft write failure'); END;",
            )
            .unwrap();
        let facts = [
            envelope("turn", Some(1), SessionFact::TurnStarted { max_steps: 1 }),
            envelope(
                "turn",
                Some(1),
                SessionFact::AssistantDraftAppended {
                    message_id: "draft".into(),
                    chunk_index: 0,
                    text: "不得留下半批事实".into(),
                },
            ),
        ];
        let mut invariant = SessionInvariant::default();
        assert!(store
            .append_session_facts_checked("ses_test", &mut invariant, &facts)
            .unwrap_err()
            .to_string()
            .contains("injected draft write failure"));
        assert!(store.list_session_facts("ses_test").unwrap().is_empty());
        assert!(invariant.recovery_facts("check").is_empty());
        store
            .connection
            .execute_batch("DROP TRIGGER fail_draft;")
            .unwrap();
        store
            .append_session_facts_checked("ses_test", &mut invariant, &facts)
            .unwrap();
        assert_eq!(store.list_session_facts("ses_test").unwrap().len(), 2);
        assert_eq!(invariant.recovery_facts("check").len(), 2);
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
        store
            .append_session_facts_checked(
                "ses_test",
                &mut SessionInvariant::default(),
                &[envelope(
                    "new-turn",
                    None,
                    SessionFact::UserMessageCommitted {
                        input_id: "new-input".into(),
                        message: Message::user_text("历史脏 turn 不得阻断合法新输入"),
                    },
                )],
            )
            .unwrap();
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

    #[test]
    fn tool_result_group_keeps_transcript_index_after_interrupted_draft() {
        let root = temp_state_root("projection");
        let state_path = root.join("state.db");
        let store = SessionStore::open(&state_path).unwrap();
        store.create_session("ses_test", "C:/proj", None).unwrap();

        let mut interrupted = TypedSessionWriter::new(&state_path, "ses_test", "old-turn");
        interrupted.user_message("old-input", Message::user_text("先前问题"));
        interrupted.turn_started(1, 2);
        interrupted.push_text("已显示但未完成的草稿");
        interrupted.finish(SessionTurnTerminal::Failed("connection lost".into()));
        assert!(interrupted.errors().is_empty());

        let calls = Message::assistant(vec![
            Part::ToolCall {
                id: "call-a".into(),
                name: "read_a".into(),
                input: json!({}),
            },
            Part::ToolCall {
                id: "call-b".into(),
                name: "read_b".into(),
                input: json!({}),
            },
        ]);
        let results = Message::tool_results(vec![
            Part::ToolResult {
                call_id: "call-a".into(),
                content: "result a".into(),
                is_error: false,
            },
            Part::ToolResult {
                call_id: "call-b".into(),
                content: "result b".into(),
                is_error: false,
            },
        ]);
        let mut next = TypedSessionWriter::new(&state_path, "ses_test", "new-turn");
        next.user_message("new-input", Message::user_text("后续问题"));
        next.turn_started(1, 2);
        next.assistant_committed(1, calls.clone());
        next.tool_results_committed(1, results.clone());
        next.finish(SessionTurnTerminal::Completed);
        assert!(next.errors().is_empty());

        let facts = store.list_session_facts("ses_test").unwrap();
        let mut replay = SessionInvariant::default();
        for (_, fact) in &facts {
            replay.apply(fact).unwrap();
        }
        let projection = project_session_facts(&facts);
        assert_eq!(projection.surface_messages.len(), 4);
        assert_eq!(projection.transcript_messages.len(), 5);
        assert_eq!(projection.surface_messages[2], calls);
        assert_eq!(projection.surface_messages[3], results);
        assert_eq!(projection.transcript_messages[3], calls);
        assert_eq!(projection.transcript_messages[4], results);
        assert_eq!(projection.interrupted_assistants.len(), 1);
        assert!(projection.interrupted_assistants[0].materialized);
        assert!(matches!(
            &projection.transcript_messages[1].parts[0],
            Part::Text { text } if text.contains("已显示但未完成的草稿")
                && text.contains("生成中断")
        ));
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delayed_legacy_seed_preserves_phone_fact_after_source() {
        let store = store();
        let history = vec![Message::user_text("旧会话历史")];
        let source = store
            .append_event(
                "ses_test",
                "conversation.updated",
                &json!({ "messages": history }),
            )
            .unwrap();
        let phone = Message::user_text("手机发来的新输入");
        let persisted = store
            .append_session_facts_checked(
                "ses_test",
                &mut SessionInvariant::default(),
                &[envelope(
                    "mobile:phone-input",
                    None,
                    SessionFact::UserMessageCommitted {
                        input_id: "phone-input".into(),
                        message: phone.clone(),
                    },
                )],
            )
            .unwrap();
        assert_eq!((source.sequence, persisted[0].sequence), (1, 2));

        prepare_typed_session(&store, "ses_test").unwrap();
        let facts = store.list_latest_segment_facts("ses_test").unwrap();
        assert_eq!(
            facts
                .iter()
                .find(|(_, envelope)| matches!(envelope.fact, SessionFact::LegacySeeded { .. }))
                .unwrap()
                .0
                .sequence,
            3
        );
        let projection = project_session_facts(&facts);
        let expected = vec![history[0].clone(), phone];
        assert_eq!(projection.seed_source_sequence, Some(source.sequence));
        assert_eq!(projection.surface_messages, expected);
        assert_eq!(projection.transcript_messages, expected);
        prepare_typed_session(&store, "ses_test").unwrap();
        assert_eq!(store.list_session_facts("ses_test").unwrap(), facts);
    }

    #[test]
    fn newer_legacy_source_covers_prior_facts_but_keeps_later_input() {
        let store = store();
        let earlier = Message::user_text("快照已包含的输入");
        store
            .append_session_facts_checked(
                "ses_test",
                &mut SessionInvariant::default(),
                &[envelope(
                    "earlier",
                    None,
                    SessionFact::UserMessageCommitted {
                        input_id: "earlier".into(),
                        message: earlier.clone(),
                    },
                )],
            )
            .unwrap();
        let history = vec![earlier, assistant("快照内的回复")];
        let source = store
            .append_event(
                "ses_test",
                "conversation.updated",
                &json!({ "messages": history }),
            )
            .unwrap();
        let later = Message::user_text("快照之后独立提交的输入");
        store
            .append_session_facts_checked(
                "ses_test",
                &mut SessionInvariant::default(),
                &[envelope(
                    "later",
                    None,
                    SessionFact::UserMessageCommitted {
                        input_id: "later".into(),
                        message: later.clone(),
                    },
                )],
            )
            .unwrap();
        prepare_typed_session(&store, "ses_test").unwrap();
        let facts = store.list_latest_segment_facts("ses_test").unwrap();
        let projection = project_session_facts(&facts);
        let expected = history.into_iter().chain([later]).collect::<Vec<_>>();
        assert_eq!(projection.seed_source_sequence, Some(source.sequence));
        assert_eq!(projection.surface_messages, expected);
        assert_eq!(projection.transcript_messages, expected);
    }

    #[test]
    fn external_fork_seed_preserves_target_facts_before_and_after_publication() {
        let store = store();
        let before = Message::user_text("fork seed 前的目标会话输入");
        let after = Message::user_text("fork seed 后的目标会话输入");
        let mut invariant = SessionInvariant::default();
        store
            .append_session_facts_checked(
                "ses_test",
                &mut invariant,
                &[envelope(
                    "before",
                    None,
                    SessionFact::UserMessageCommitted {
                        input_id: "before".into(),
                        message: before.clone(),
                    },
                )],
            )
            .unwrap();
        let history = vec![Message::user_text("fork 原会话内容")];
        store
            .append_session_facts_checked(
                "ses_test",
                &mut invariant,
                &[envelope(
                    "fork",
                    None,
                    SessionFact::LegacySeeded {
                        source_event_id: "fork:other-session:1000".into(),
                        source_sequence: 1000,
                        source_hash: stable_json_hash(&history),
                        messages: history.clone(),
                    },
                )],
            )
            .unwrap();
        store
            .append_session_facts_checked(
                "ses_test",
                &mut invariant,
                &[envelope(
                    "after",
                    None,
                    SessionFact::UserMessageCommitted {
                        input_id: "after".into(),
                        message: after.clone(),
                    },
                )],
            )
            .unwrap();
        let projection = project_session_facts(&store.list_session_facts("ses_test").unwrap());
        let expected = vec![history[0].clone(), before, after];
        assert_eq!(projection.seed_source_sequence, Some(1000));
        assert_eq!(projection.surface_messages, expected);
        assert_eq!(projection.transcript_messages, expected);
    }

    #[test]
    fn external_fork_sequence_does_not_block_same_session_legacy_seed() {
        let store = store();
        let fork_history = vec![Message::user_text("fork 原会话内容")];
        store
            .append_session_facts_checked(
                "ses_test",
                &mut SessionInvariant::default(),
                &[envelope(
                    "fork",
                    None,
                    SessionFact::LegacySeeded {
                        source_event_id: "fork:other-session:2".into(),
                        source_sequence: 2,
                        source_hash: stable_json_hash(&fork_history),
                        messages: fork_history.clone(),
                    },
                )],
            )
            .unwrap();
        let history = vec![fork_history[0].clone(), assistant("目标会话新增的回复")];
        let source = store
            .append_event(
                "ses_test",
                "conversation.updated",
                &json!({ "messages": history }),
            )
            .unwrap();
        assert_eq!(source.sequence, 2);
        prepare_typed_session(&store, "ses_test").unwrap();
        let facts = store.list_latest_segment_facts("ses_test").unwrap();
        assert_eq!(facts.len(), 2, "外源序号碰撞不得阻止本会话新快照播种");
        let SessionFact::LegacySeeded {
            source_event_id, ..
        } = &facts[1].1.fact
        else {
            panic!("新快照必须有独立 seed");
        };
        assert_eq!(source_event_id, &source.event_id);
        let projection = project_session_facts(&facts);
        assert_eq!(projection.surface_messages, history);
        assert_eq!(projection.transcript_messages, history);
        assert!(store
            .seed_latest_legacy_snapshot("ses_test")
            .unwrap()
            .is_none());
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

    /// UI-0926 #1:播种尊重当前对话地板。reset 之前、或「删掉当前段」之前的旧快照
    /// 不得被播种进当前段(D-427);只删旧段不影响当前段快照的播种。
    #[test]
    fn reset与删除之后不再播种旧快照() {
        let seeded_after = |barrier: Option<(&str, serde_json::Value)>| {
            let store = store();
            store
                .append_event(
                    "ses_test",
                    "conversation.updated",
                    &json!({"messages": [Message::user_text("旧对话")]}),
                )
                .unwrap();
            if let Some((event_type, payload)) = barrier {
                store
                    .append_event("ses_test", event_type, &payload)
                    .unwrap();
            }
            let seeded = store.seed_latest_legacy_snapshot("ses_test").unwrap();
            let facts = store.list_latest_segment_facts("ses_test").unwrap();
            (seeded.is_some(), facts.len())
        };
        assert_eq!(
            seeded_after(Some(("conversation.reset", json!({"cleared": true})))),
            (false, 0),
            "新对话之后不得把 reset 前的旧快照播种进新段"
        );
        assert_eq!(
            seeded_after(Some((
                super::super::events::SEGMENT_DELETED,
                json!({"start": 0, "end": null, "events": 3}),
            ))),
            (false, 0),
            "删掉当前段之后不得把更早的旧快照播种回来"
        );
        assert_eq!(
            seeded_after(Some((
                super::super::events::SEGMENT_DELETED,
                json!({"start": 0, "end": 0, "events": 1}),
            ))),
            (true, 1),
            "只删旧段时当前段的快照照常播种"
        );
        assert_eq!(seeded_after(None), (true, 1), "无地板时照常播种");
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
