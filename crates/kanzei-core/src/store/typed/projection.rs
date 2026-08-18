use std::collections::HashMap;

use kanzei_llm::{Message, Part};
use serde::{Deserialize, Serialize};

use super::{
    stable_json_hash, SessionFact, SessionFactEnvelope, StoredEvent, SESSION_EVENT_FORMAT_VERSION,
};

pub enum SessionTurnTerminal {
    Completed,
    Stopped,
    Failed(String),
}

impl SessionTurnTerminal {
    pub(super) fn reason(&self) -> &str {
        match self {
            Self::Completed => "completed",
            Self::Stopped => "stopped_by_user",
            Self::Failed(error) => error,
        }
    }

    pub(super) fn into_fact(self) -> SessionFact {
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
