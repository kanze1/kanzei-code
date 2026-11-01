//! QQ presents permission requests and questions, never the internal event envelope.
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Request {
    Permission {
        id: u64,
        action: String,
        resource: String,
    },
    Question {
        id: u64,
        question: String,
        options: Vec<kanzei_core::AskOption>,
        multiple: bool,
    },
}

pub(super) fn present(value: serde_json::Value) -> anyhow::Result<String> {
    let request: Request = serde_json::from_value(value)?;
    Ok(match request {
        Request::Permission {
            id,
            action,
            resource,
        } => {
            let operation = match action.as_str() {
                "bili_feed" => "浏览或搜索 B站视频",
                "bili_video" => "读取 B站视频信息和字幕",
                "bili_comments" => "读取 B站评论",
                "bili_comment_draft" => "保存 B站评论草稿",
                "websearch" => "联网搜索",
                "webfetch" => "读取网页",
                "read" => "读取文件",
                "write" => "写入文件",
                "edit" => "修改文件",
                "bash" => "执行命令",
                _ => &action,
            };
            let detail = if resource.is_empty() || resource == "*" {
                String::new()
            } else {
                format!("\n操作范围：{resource}")
            };
            format!("需要你批准：{operation}{detail}\n\n允许本次：/allow {id}\n拒绝：/deny {id}")
        }
        Request::Question {
            id,
            question,
            options,
            multiple,
        } => {
            let mut text = format!("大肥鱼想问你：\n{question}");
            for (index, option) in options.iter().enumerate() {
                text.push_str(&format!("\n{}. {}", index + 1, option.label));
                if let Some(note) = option.note.as_deref().filter(|note| !note.is_empty()) {
                    text.push_str(&format!("：{note}"));
                }
            }
            if multiple {
                text.push_str("\n可以选择多项。");
            }
            text.push_str(&format!("\n\n回答：/answer {id} 内容"));
            text
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn permission_renders_the_operation_without_internal_metadata_or_question_commands() {
        let text = present(json!({
            "kind":"permission", "id":1836532548426752_u64,
            "action":"bili_feed", "resource":"*", "remember":"*",
            "sessionId":"private-session", "source":"primary", "agentId":"private-agent"
        }))
        .unwrap();
        assert!(text.contains("需要你批准：浏览或搜索 B站视频"));
        assert!(text.contains("/allow 1836532548426752"));
        assert!(text.contains("/deny 1836532548426752"));
        for hidden in [
            "sessionId",
            "private-session",
            "private-agent",
            "remember",
            "primary",
            "{",
            "/answer",
        ] {
            assert!(!text.contains(hidden), "unexpected event detail: {hidden}");
        }
    }

    #[test]
    fn approval_keeps_the_specific_resource_reviewable() {
        let text = present(json!({"kind":"permission","id":2,"action":"websearch","resource":"https://html.duckduckgo.com/html/"})).unwrap();
        assert!(text.contains("联网搜索"));
        assert!(text.contains("操作范围：https://html.duckduckgo.com/html/"));
    }

    #[test]
    fn question_preserves_choices_and_only_offers_an_answer_command() {
        let text = present(json!({
            "kind":"question","id":3,"question":"接着聊哪个方向？",
            "options":[{"label":"音色","note":"讨论合成器"},{"label":"旋律"}],
            "multiple":true,"default":"音色","background":true,"sessionId":"private-session"
        }))
        .unwrap();
        assert!(text.contains("1. 音色：讨论合成器"));
        assert!(text.contains("2. 旋律"));
        assert!(text.contains("可以选择多项"));
        assert!(text.contains("/answer 3 内容"));
        for hidden in ["/allow", "/deny", "private-session", "background", "{"] {
            assert!(!text.contains(hidden));
        }
    }

    #[test]
    fn malformed_or_unknown_events_are_rejected_instead_of_forwarded() {
        assert!(
            present(json!({"kind":"tool-start","id":1,"input":{"secret":"internal"}})).is_err()
        );
        assert!(present(json!({"kind":"permission","id":1,"action":"bili_feed"})).is_err());
    }
}
