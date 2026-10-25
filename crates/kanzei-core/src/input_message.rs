//! One input projection for live execution and durable session facts.
use kanzei_base::{InputAttachment, MessageOrigin};
use kanzei_harness::{ExternalSourceKind, ToolOutput};
use kanzei_llm::{Message, Part, Role};

pub fn input_message(
    origin: MessageOrigin,
    text: &str,
    attachments: &[InputAttachment],
    initial_parts: &[Part],
) -> Message {
    let mut parts = Vec::new();
    if !text.is_empty() {
        parts.push(Part::Text { text: text.into() });
    }
    if !attachments.is_empty() {
        let content = ToolOutput::ok(serde_json::json!({"attachments":attachments}).to_string())
            .with_external_source(ExternalSourceKind::Attachment, "input attachments")
            .model_content();
        parts.push(Part::Text { text: content });
    }
    parts.extend_from_slice(initial_parts);
    Message {
        role: Role::User,
        origin,
        parts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_and_history_filter_preserve_input_and_attachment_sources() {
        let attachment = InputAttachment {
            name: "notes.txt".into(),
            media_type: "text/plain".into(),
            path: "artifacts/content.txt".into(),
            original_path: "artifacts/original".into(),
            bytes: 12,
            content_hash: "fixture".into(),
        };
        for origin in [
            MessageOrigin::Human,
            MessageOrigin::Engine,
            MessageOrigin::Agent,
        ] {
            let message = input_message(
                origin,
                "  exact input  ",
                std::slice::from_ref(&attachment),
                &[],
            );
            let persisted = serde_json::to_string(&message).unwrap();
            let recovered: Message = serde_json::from_str(&persisted).unwrap();
            let filtered = crate::history::filter_message_history(&[recovered]);
            assert_eq!(filtered, vec![message.clone()]);
            assert_eq!(
                message.parts[0],
                Part::Text {
                    text: "  exact input  ".into()
                }
            );
            let Part::Text { text } = &message.parts[1] else {
                panic!("missing attachment reference")
            };
            let frame: serde_json::Value = serde_json::from_str(
                text.strip_prefix(kanzei_harness::EXTERNAL_SOURCE_PREFIX)
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(frame["authority"], "untrusted_data");
            assert_eq!(frame["source"]["kind"], "attachment");
            let body: serde_json::Value =
                serde_json::from_str(frame["content"].as_str().unwrap()).unwrap();
            assert_eq!(body["attachments"][0]["name"], "notes.txt");
        }
    }
}
