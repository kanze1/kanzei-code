//! Preserve durable history while sending one full copy of identical file observations.
use std::borrow::Cow;
use std::collections::HashMap;

use kanzei_llm::{Message, Part, Role};
use sha2::{Digest, Sha256};

pub struct ObservationProjection<'a> {
    pub messages: Cow<'a, [Message]>,
    pub repeated_results: usize,
    pub omitted_bytes: usize,
}

/// Only successful, text-only, engine-framed file observations qualify. Tool IDs,
/// call inputs, user instructions, mutations, errors and the newest full copy remain.
pub fn project_observation_context(messages: &[Message]) -> ObservationProjection<'_> {
    let mut calls = HashMap::new();
    for message in messages
        .iter()
        .filter(|message| message.role == Role::Assistant)
    {
        for part in &message.parts {
            if let Part::ToolCall { id, name, input } = part {
                if matches!(
                    name.as_str(),
                    "read" | "grep" | "glob" | "files" | "symbols"
                ) {
                    calls.insert(id.as_str(), (name, input));
                }
            }
        }
    }
    let mut latest = HashMap::<String, &str>::new();
    let mut replacements = Vec::new();
    let mut omitted_bytes = 0;
    for (message_index, message) in messages.iter().enumerate().rev() {
        if message.role != Role::User
            || message
                .parts
                .iter()
                .any(|part| matches!(part, Part::Image { .. } | Part::Document { .. }))
        {
            continue;
        }
        for (part_index, part) in message.parts.iter().enumerate().rev() {
            let Part::ToolResult {
                call_id,
                content,
                is_error: false,
            } = part
            else {
                continue;
            };
            let Some((name, input)) = calls.get(call_id.as_str()) else {
                continue;
            };
            let Some(raw) = content.strip_prefix(kanzei_harness::tool::EXTERNAL_SOURCE_PREFIX)
            else {
                continue;
            };
            let Ok(frame) = serde_json::from_str::<serde_json::Value>(raw) else {
                continue;
            };
            if frame["authority"] != "untrusted_data"
                || frame["source"]["kind"] != "file"
                || !frame["content"].is_string()
            {
                continue;
            }
            let mut digest = Sha256::new();
            digest.update(
                serde_json::to_vec(&(name, input, content)).expect("JSON values serialize"),
            );
            let hash = format!("{:x}", digest.finalize());
            if let Some(retained_call) = latest.get(&hash) {
                let reference = format!(
                    "[observation_reference]\n{}",
                    serde_json::json!({
                        "identical_to_call_id":retained_call,"sha256":hash,
                        "source":frame["source"],
                        "note":"Identical bytes are retained in that later tool result in this request. This only deduplicates the observation; it does not prove the file stayed unchanged between calls."
                    })
                );
                if reference.len() < content.len() {
                    omitted_bytes += content.len() - reference.len();
                    replacements.push((message_index, part_index, reference));
                }
            } else {
                latest.insert(hash, call_id);
            }
        }
    }
    let repeated_results = replacements.len();
    let messages = if replacements.is_empty() {
        Cow::Borrowed(messages)
    } else {
        let mut projected = messages.to_vec();
        for (message, part, reference) in replacements {
            if let Part::ToolResult { content, .. } = &mut projected[message].parts[part] {
                *content = reference;
            }
        }
        Cow::Owned(projected)
    };
    ObservationProjection {
        messages,
        repeated_results,
        omitted_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn read(id: &str, name: &str, bytes: &str, error: bool) -> [Message; 2] {
        [
            Message::assistant(vec![Part::ToolCall {
                id: id.into(),
                name: name.into(),
                input: serde_json::json!({"path":"source.rs"}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: id.into(),
                content: kanzei_harness::ToolOutput::ok(bytes)
                    .with_external_source(kanzei_harness::ExternalSourceKind::File, "source.rs")
                    .model_content(),
                is_error: error,
            }]),
        ]
    }
    #[test]
    fn projection_retains_evidence_and_history_and_only_deduplicates_exact_reads() {
        let source = "actual source bytes 中文\n".repeat(200);
        let mut messages = vec![Message::user_text(
            "Find the invariant and cite its exact source.",
        )];
        messages.extend(read("read1", "read", &source, false));
        messages.extend(read("read2", "read", &source, false));
        messages.extend(read("changed", "read", "changed source", false));
        messages.extend(read("write", "write", &source, false));
        messages.extend(read("error", "read", &source, true));
        let original = messages.clone();
        let projection = project_observation_context(&messages);
        assert_eq!(projection.repeated_results, 1);
        assert!(projection.omitted_bytes > source.len() / 2);
        assert_eq!(
            messages, original,
            "persistent history must remain byte-for-byte intact"
        );
        let Part::ToolResult { content, .. } = &projection.messages[2].parts[0] else {
            panic!()
        };
        assert!(content.contains("read2") && content.starts_with("[observation_reference]"));
        assert_eq!(projection.messages[4..], original[4..]);
        let Part::ToolResult { content, .. } = &projection.messages[4].parts[0] else {
            panic!()
        };
        assert!(content.contains(&source.replace('\n', "\\n")));
    }
    #[test]
    fn image_results_and_different_sources_are_never_merged() {
        let source = "observed bytes".repeat(200);
        let mut messages = read("a", "read", &source, false).to_vec();
        messages.extend(read("b", "read", &source, false));
        messages[3].parts.push(Part::Image {
            media_type: "image/png".into(),
            data: "different pixels".into(),
        });
        assert_eq!(project_observation_context(&messages).repeated_results, 0);
        messages[3].parts.pop();
        if let Part::ToolResult { content, .. } = &mut messages[3].parts[0] {
            *content = content.replace("source.rs", "other.rs");
        }
        assert_eq!(project_observation_context(&messages).repeated_results, 0);
    }
}
