//! 运行体验事件契约(R-284 B1)。
//!
//! `kz:*` 是现有兼容事件；`kz:experience` 是跨 memory/research/voice/run
//! 共用的结构化包络。包络字段和 payload 统一使用 snake_case，表现层只能消费
//! 它，不能把动画结果写回事实源。

use std::sync::atomic::{AtomicU64, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const EXPERIENCE_EVENT_NAME: &str = "kz:experience";
pub const EXPERIENCE_SCHEMA_VERSION: u8 = 1;

static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// 事件对业务事实的承诺边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceEventClass {
    /// 可写入事件账本、可重放且必须幂等。
    Fact,
    /// 只用于动画、音频和工作台瞬时反馈，可以丢帧。
    Presentation,
    /// 高频增量，消费者可以合并后再表现。
    Delta,
}

/// R-284 的统一事件包络。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExperienceEvent {
    pub schema_version: u8,
    pub event_id: String,
    pub event_type: String,
    pub class: ExperienceEventClass,
    pub occurred_at: u64,
    pub session_id: String,
    pub project_id: Option<String>,
    pub run_id: Option<String>,
    pub topic_id: Option<String>,
    pub entity_id: Option<String>,
    pub payload: Value,
}

impl ExperienceEvent {
    pub fn new(
        event_type: impl Into<String>,
        class: ExperienceEventClass,
        session_id: impl Into<String>,
        run_id: Option<String>,
        payload: Value,
        occurred_at: u64,
    ) -> Result<Self, String> {
        let event = Self {
            schema_version: EXPERIENCE_SCHEMA_VERSION,
            event_id: format!(
                "experience-{}",
                EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            event_type: event_type.into(),
            class,
            occurred_at,
            session_id: session_id.into(),
            project_id: None,
            run_id,
            topic_id: None,
            entity_id: None,
            payload,
        };
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != EXPERIENCE_SCHEMA_VERSION {
            return Err(format!(
                "unsupported experience schema_version {}",
                self.schema_version
            ));
        }
        if self.event_id.trim().is_empty() {
            return Err("experience event_id must not be empty".into());
        }
        if self.session_id.trim().is_empty() {
            return Err("experience session_id must not be empty".into());
        }
        if !is_snake_case(&self.event_type) {
            return Err(format!(
                "experience event_type is not snake_case: {}",
                self.event_type
            ));
        }
        if !self.payload.is_object() {
            return Err("experience payload must be a JSON object".into());
        }
        Ok(())
    }

    pub fn into_value(self) -> Value {
        serde_json::to_value(self).expect("ExperienceEvent is serializable")
    }
}

/// Converts a legacy `kz:*` payload at the adapter boundary. The old events remain
/// unchanged for compatibility; only the new envelope receives the normalized copy.
pub fn normalize_json(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (to_snake_case(&key), normalize_json(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(normalize_json).collect()),
        other => other,
    }
}

/// Existing run events mapped into the shared vocabulary. Unknown legacy events
/// are retained as `unknown_event` so the frontend can diagnose them without crashing.
pub fn legacy_event_type(name: &str) -> (&'static str, ExperienceEventClass) {
    match name {
        "kz:turn" => ("run_started", ExperienceEventClass::Fact),
        "kz:text" => ("text_delta", ExperienceEventClass::Delta),
        "kz:reasoning" => ("reasoning_delta", ExperienceEventClass::Delta),
        "kz:tool-start" => ("tool_started", ExperienceEventClass::Fact),
        "kz:tool-progress" => ("tool_progressed", ExperienceEventClass::Delta),
        "kz:tool-end" => ("tool_completed", ExperienceEventClass::Fact),
        "kz:permission-resolved" => ("permission_resolved", ExperienceEventClass::Fact),
        "kz:task-progress" => ("task_progressed", ExperienceEventClass::Delta),
        "kz:stream-restart" => ("stream_restarted", ExperienceEventClass::Presentation),
        "kz:step" => ("usage_delta", ExperienceEventClass::Delta),
        "kz:status" => ("run_status_changed", ExperienceEventClass::Presentation),
        _ => ("unknown_event", ExperienceEventClass::Presentation),
    }
}

pub fn from_legacy(
    name: &str,
    session_id: &str,
    run_id: Option<String>,
    payload: Value,
    occurred_at: u64,
) -> Result<ExperienceEvent, String> {
    let (event_type, class) = legacy_event_type(name);
    let mut payload = normalize_json(payload);
    if event_type == "unknown_event" {
        if let Value::Object(ref mut object) = payload {
            object.insert("legacy_event".into(), Value::String(name.into()));
        }
    }
    ExperienceEvent::new(event_type, class, session_id, run_id, payload, occurred_at)
}

fn is_snake_case(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
        && !value.starts_with('_')
        && !value.ends_with('_')
}

fn to_snake_case(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 4);
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else if character == '-' || character == ' ' {
            output.push('_');
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    #[test]
    fn schema_uses_snake_case_and_exposes_three_event_classes() {
        let schema = serde_json::to_value(schemars::schema_for!(ExperienceEvent)).unwrap();
        let properties = schema["properties"].as_object().unwrap();
        assert!(properties.contains_key("schema_version"));
        assert!(properties.contains_key("event_id"));
        assert!(properties.contains_key("session_id"));
        assert!(!properties.contains_key("schemaVersion"));
        let class_schema =
            serde_json::to_value(schemars::schema_for!(ExperienceEventClass)).unwrap();
        let rendered = serde_json::to_string(&class_schema).unwrap();
        assert!(rendered.contains("fact"));
        assert!(rendered.contains("presentation"));
        assert!(rendered.contains("delta"));
    }

    #[test]
    fn legacy_payload_is_normalized_and_round_trips() {
        let event = from_legacy(
            "kz:tool-start",
            "session-1",
            Some("run-1".into()),
            serde_json::json!({"toolCallId":"call-1", "maxSteps": 3, "nestedValue":{"cacheRead": 2}}),
            42,
        )
        .unwrap();
        assert_eq!(event.event_type, "tool_started");
        assert_eq!(event.class, ExperienceEventClass::Fact);
        assert_eq!(event.payload["tool_call_id"], "call-1");
        assert_eq!(event.payload["max_steps"], 3);
        assert_eq!(event.payload["nested_value"]["cache_read"], 2);
        let decoded: ExperienceEvent = serde_json::from_value(event.clone().into_value()).unwrap();
        assert_eq!(decoded, event);
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn unknown_legacy_event_is_diagnostic_not_a_contract_failure() {
        let event_name = ["kz", "future-event"].join(":");
        let event =
            from_legacy(&event_name, "session-1", None, Value::Object(Map::new()), 1).unwrap();
        assert_eq!(event.event_type, "unknown_event");
        assert_eq!(event.payload["legacy_event"], event_name);
    }

    #[test]
    fn invalid_envelope_is_rejected() {
        let error = ExperienceEvent::new(
            "ToolStarted",
            ExperienceEventClass::Fact,
            "session-1",
            None,
            serde_json::json!({}),
            1,
        )
        .unwrap_err();
        assert!(error.contains("snake_case"));
    }
}
