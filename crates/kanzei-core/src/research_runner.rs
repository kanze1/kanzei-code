//! Research Experiment Runner 的 Terminal Callback 内核(R-344 B1)。
//!
//! 这里不启动进程、不写 state.db；它只把一行 stdout/stderr 判定为结构化事件或
//! 终端日志，并返回可持久化的 callback 统计。进程与存储接线由专用工具完成。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const CALLBACK_PREFIX: &str = "@@kanzei ";
pub const MAX_CALLBACK_LINE_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallbackStats {
    pub parsed: u64,
    pub malformed: u64,
    pub truncated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchCallbackEvent {
    pub event_type: String,
    pub timestamp_ms: Option<i64>,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedCallbackLine {
    pub event: Option<ResearchCallbackEvent>,
    pub terminal_log: Option<String>,
    pub parsed: bool,
    pub malformed: bool,
    pub truncated: bool,
}

impl ParsedCallbackLine {
    fn terminal(line: &str) -> Self {
        Self {
            event: None,
            terminal_log: Some(line.to_string()),
            parsed: false,
            malformed: false,
            truncated: false,
        }
    }

    fn malformed(line: &str) -> Self {
        Self {
            event: None,
            terminal_log: Some(line.to_string()),
            parsed: false,
            malformed: true,
            truncated: false,
        }
    }

    fn truncated(line: &str) -> Self {
        Self {
            event: Some(ResearchCallbackEvent {
                event_type: "message".to_string(),
                timestamp_ms: None,
                payload: serde_json::json!({
                    "level": "warn",
                    "text": format!(
                        "@@kanzei callback line exceeded {} bytes and was not parsed",
                        MAX_CALLBACK_LINE_BYTES
                    ),
                }),
            }),
            terminal_log: Some(line.to_string()),
            parsed: false,
            malformed: false,
            truncated: true,
        }
    }

    fn event(event: ResearchCallbackEvent, terminal_log: Option<String>) -> Self {
        Self {
            event: Some(event),
            terminal_log,
            parsed: true,
            malformed: false,
            truncated: false,
        }
    }

    pub fn apply_stats(&self, stats: &mut CallbackStats) {
        stats.parsed += u64::from(self.parsed);
        stats.malformed += u64::from(self.malformed);
        stats.truncated += u64::from(self.truncated);
    }
}

/// 解析单行终端输出。
///
/// 普通输出原样进入 `terminal_log`；合法但未知的 callback 事件也保留原文，避免
/// 新协议事件被旧 runner 猜错。坏 JSON/坏事件只返回诊断形态，不向调用方抛出异常。
pub fn parse_callback_line(line: &str) -> ParsedCallbackLine {
    let line = line.trim_end_matches('\r');
    if !line.starts_with(CALLBACK_PREFIX) {
        return ParsedCallbackLine::terminal(line);
    }
    if line.len() > MAX_CALLBACK_LINE_BYTES {
        return ParsedCallbackLine::truncated(line);
    }

    let payload_text = line[CALLBACK_PREFIX.len()..].trim();
    let value: Value = match serde_json::from_str(payload_text) {
        Ok(value) => value,
        Err(_) => return ParsedCallbackLine::malformed(line),
    };
    let object = match value.as_object() {
        Some(object) => object,
        None => return ParsedCallbackLine::malformed(line),
    };
    let event_type = match object.get("t").and_then(Value::as_str) {
        Some(event_type) if !event_type.trim().is_empty() => event_type.to_string(),
        _ => return ParsedCallbackLine::malformed(line),
    };
    if validate_event(&event_type, object).is_err() {
        return ParsedCallbackLine::malformed(line);
    }
    let timestamp_ms = object.get("ts").and_then(Value::as_i64);
    let event = ResearchCallbackEvent {
        event_type: event_type.clone(),
        timestamp_ms,
        payload: value,
    };
    let terminal_log = (!is_known_event(&event_type)).then(|| line.to_string());
    ParsedCallbackLine::event(event, terminal_log)
}

fn validate_event(event_type: &str, object: &Map<String, Value>) -> Result<(), &'static str> {
    match event_type {
        "stage" => required_non_empty_string(object, "name"),
        "metric" => {
            required_non_empty_string(object, "name")?;
            if let Some(value) = object.get("value") {
                if !value.is_number() {
                    return Err("metric.value must be numeric");
                }
            }
            Ok(())
        }
        "progress" => {
            required_number(object, "done")?;
            required_number(object, "total")
        }
        "artifact" => {
            required_non_empty_string(object, "kind")?;
            required_non_empty_string(object, "path")
        }
        "checkpoint" => required_non_empty_string(object, "path"),
        "message" => {
            required_non_empty_string(object, "level")?;
            required_non_empty_string(object, "text")
        }
        "heartbeat" => Ok(()),
        "result" => {
            if let Some(status) = object.get("status") {
                if !status.is_string() {
                    return Err("result.status must be a string");
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn required_non_empty_string(
    object: &Map<String, Value>,
    key: &'static str,
) -> Result<(), &'static str> {
    match object.get(key).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Ok(()),
        _ => Err("required non-empty string is missing"),
    }
}

fn required_number(object: &Map<String, Value>, key: &'static str) -> Result<(), &'static str> {
    match object.get(key) {
        Some(value) if value.is_number() => Ok(()),
        _ => Err("required number is missing"),
    }
}

fn is_known_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "stage"
            | "metric"
            | "progress"
            | "artifact"
            | "checkpoint"
            | "message"
            | "heartbeat"
            | "result"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_callback_events_and_keeps_payload() {
        let mut stats = CallbackStats::default();
        for line in [
            r#"@@kanzei {"t":"stage","ts":10,"name":"train"}"#,
            r#"@@kanzei {"t":"metric","ts":11,"name":"acc","value":0.9,"step":2}"#,
            r#"@@kanzei {"t":"progress","done":2,"total":10,"unit":"epoch"}"#,
            r#"@@kanzei {"t":"artifact","kind":"figure","path":"out/a.png"}"#,
            r#"@@kanzei {"t":"checkpoint","path":"ckpt/2.pt"}"#,
            r#"@@kanzei {"t":"message","level":"info","text":"ready"}"#,
            r#"@@kanzei {"t":"heartbeat"}"#,
            r#"@@kanzei {"t":"result","status":"succeeded","summary":"ok"}"#,
        ] {
            let parsed = parse_callback_line(line);
            assert!(parsed.event.is_some(), "{line}");
            assert!(parsed.terminal_log.is_none(), "{line}");
            parsed.apply_stats(&mut stats);
        }
        assert_eq!(
            stats,
            CallbackStats {
                parsed: 8,
                ..Default::default()
            }
        );
        let metric =
            parse_callback_line(r#"@@kanzei {"t":"metric","name":"acc","value":0.9,"step":2}"#);
        assert_eq!(metric.event.unwrap().payload["step"], 2);
    }

    #[test]
    fn malformed_and_unknown_callback_lines_are_non_fatal_and_counted() {
        let mut stats = CallbackStats::default();
        let malformed = parse_callback_line("@@kanzei {not-json");
        malformed.apply_stats(&mut stats);
        assert!(malformed.event.is_none());
        assert_eq!(
            malformed.terminal_log.as_deref(),
            Some("@@kanzei {not-json")
        );
        assert!(malformed.malformed);

        let unknown = parse_callback_line(r#"@@kanzei {"t":"future_event","value":1}"#);
        unknown.apply_stats(&mut stats);
        assert_eq!(unknown.event.unwrap().event_type, "future_event");
        assert_eq!(
            unknown.terminal_log.as_deref(),
            Some("@@kanzei {\"t\":\"future_event\",\"value\":1}")
        );
        assert_eq!(
            stats,
            CallbackStats {
                parsed: 1,
                malformed: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn oversized_callback_is_truncated_with_visible_warning_without_touching_plain_logs() {
        let long = format!("{}{}", CALLBACK_PREFIX, "x".repeat(MAX_CALLBACK_LINE_BYTES));
        let parsed = parse_callback_line(&long);
        let event = parsed.event.unwrap();
        assert_eq!(event.event_type, "message");
        assert!(event.payload["text"].as_str().unwrap().contains("8192"));
        assert!(parsed.terminal_log.is_some());
        assert!(parsed.truncated);

        let plain = "x".repeat(MAX_CALLBACK_LINE_BYTES + 1);
        let plain_parsed = parse_callback_line(&plain);
        assert!(plain_parsed.event.is_none());
        assert_eq!(plain_parsed.terminal_log.as_deref(), Some(plain.as_str()));
        assert!(!plain_parsed.truncated);
    }

    #[test]
    fn invalid_known_event_is_reported_as_original_log() {
        let parsed = parse_callback_line(r#"@@kanzei {"t":"stage"}"#);
        assert!(parsed.event.is_none());
        assert!(parsed.malformed);
        assert_eq!(
            parsed.terminal_log.as_deref(),
            Some(r#"@@kanzei {"t":"stage"}"#)
        );
    }
}
