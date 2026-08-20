//! 跨 app、memory、research 的运行体验事件契约与持久事实记录器(R-284 B2)。
//!
//! `kz:*` 是兼容事件；`kz:experience` 是 UI 投影名；`experience.fact` 是
//! session_events 中的持久事实类型。表现事件永远不经过本模块的持久写入口。

use std::sync::atomic::{AtomicU64, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{SessionStore, StoreError, StoredEvent};

pub const EXPERIENCE_EVENT_NAME: &str = "kz:experience";
pub const EXPERIENCE_FACT_EVENT_TYPE: &str = "experience.fact";
pub const EXPERIENCE_SCHEMA_VERSION: u8 = 1;

static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceEventClass {
    Fact,
    Presentation,
    Delta,
}

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
        Self::new_scoped(
            event_type,
            class,
            session_id,
            None,
            run_id,
            None,
            None,
            payload,
            occurred_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_scoped(
        event_type: impl Into<String>,
        class: ExperienceEventClass,
        session_id: impl Into<String>,
        project_id: Option<String>,
        run_id: Option<String>,
        topic_id: Option<String>,
        entity_id: Option<String>,
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
            project_id,
            run_id,
            topic_id,
            entity_id,
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

    /// Append a fact once per event_id. Replaying the same durable fact returns
    /// the existing row, so callers do not repeat downstream side effects.
    pub fn append_fact_if_new(&self, store: &SessionStore) -> Result<StoredEvent, StoreError> {
        self.validate().map_err(StoreError::InvalidInput)?;
        if self.class != ExperienceEventClass::Fact {
            return Err(StoreError::InvalidInput(
                "only fact events may be persisted".into(),
            ));
        }
        let existing =
            store.list_events_by_type(&self.session_id, 0, EXPERIENCE_FACT_EVENT_TYPE)?;
        if let Some(event) = existing.into_iter().find(|event| {
            event.payload.get("event_id") == Some(&Value::String(self.event_id.clone()))
        }) {
            return Ok(event);
        }
        store.append_event(
            &self.session_id,
            EXPERIENCE_FACT_EVENT_TYPE,
            &self.clone().into_value(),
        )
    }
}

pub fn replay_facts(
    store: &SessionStore,
    session_id: &str,
    after_sequence: i64,
) -> Result<Vec<ExperienceEvent>, StoreError> {
    store
        .list_events_by_type(session_id, after_sequence, EXPERIENCE_FACT_EVENT_TYPE)?
        .into_iter()
        .filter_map(
            |stored| match serde_json::from_value::<ExperienceEvent>(stored.payload) {
                Ok(event) if event.validate().is_ok() => Some(Ok(event)),
                Ok(_) | Err(_) => None,
            },
        )
        .collect()
}

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
    fn schema_and_legacy_payload_remain_snake_case() {
        let schema = serde_json::to_value(schemars::schema_for!(ExperienceEvent)).unwrap();
        let properties = schema["properties"].as_object().unwrap();
        assert!(properties.contains_key("schema_version"));
        assert!(!properties.contains_key("schemaVersion"));
        let event = from_legacy(
            "kz:tool-start",
            "session-1",
            Some("run-1".into()),
            serde_json::json!({"toolCallId":"call-1"}),
            42,
        )
        .unwrap();
        assert_eq!(event.payload["tool_call_id"], "call-1");
    }

    #[test]
    fn fact_append_is_idempotent_and_replayable() {
        let store = SessionStore::open_in_memory().unwrap();
        store
            .create_session("session-1", "project-1", None)
            .unwrap();
        let event = ExperienceEvent::new_scoped(
            "memory_consolidation_completed",
            ExperienceEventClass::Fact,
            "session-1",
            Some("project-1".into()),
            None,
            None,
            Some("memory-1".into()),
            serde_json::json!({"pending_after": 0}),
            1,
        )
        .unwrap();
        let first = event.append_fact_if_new(&store).unwrap();
        let second = event.append_fact_if_new(&store).unwrap();
        assert_eq!(first.sequence, second.sequence);
        let replay = replay_facts(&store, "session-1", 0).unwrap();
        assert_eq!(replay, vec![event]);
    }

    #[test]
    fn unknown_legacy_event_is_diagnostic() {
        let event_name = ["kz", "future-event"].join(":");
        let event =
            from_legacy(&event_name, "session-1", None, Value::Object(Map::new()), 1).unwrap();
        assert_eq!(event.event_type, "unknown_event");
        assert_eq!(event.payload["legacy_event"], event_name);
    }
}
