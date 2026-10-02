use std::collections::{HashMap, HashSet, VecDeque};

use kanzei_llm::{Message, Part, Role};

/// 清洗会话历史中的孤儿工具调用/结果，避免坏快照继续污染后续请求。
/// 只删除无法配对的工具 part，普通文本和已经配对的工具轮次保持原顺序。
pub fn filter_message_history(messages: &[Message]) -> Vec<Message> {
    let mut pending: HashMap<String, VecDeque<(usize, usize)>> = HashMap::new();
    let mut paired_calls = HashSet::new();
    let mut paired_results = HashSet::new();

    for (message_index, message) in messages.iter().enumerate() {
        match message.role {
            Role::Assistant => {
                for (part_index, part) in message.parts.iter().enumerate() {
                    if let Part::ToolCall { id, .. } = part {
                        pending
                            .entry(id.clone())
                            .or_default()
                            .push_back((message_index, part_index));
                    }
                }
            }
            Role::User => {
                for (part_index, part) in message.parts.iter().enumerate() {
                    if let Part::ToolResult { call_id, .. } = part {
                        let Some(queue) = pending.get_mut(call_id) else {
                            continue;
                        };
                        let Some(call_position) = queue.pop_front() else {
                            continue;
                        };
                        paired_calls.insert(call_position);
                        paired_results.insert((message_index, part_index));
                    }
                }
            }
        }
    }

    messages
        .iter()
        .enumerate()
        .filter_map(|(message_index, message)| {
            let parts: Vec<Part> = message
                .parts
                .iter()
                .enumerate()
                .filter_map(|(part_index, part)| match part {
                    Part::ToolCall { .. }
                        if message.role == Role::Assistant
                            && paired_calls.contains(&(message_index, part_index)) =>
                    {
                        Some(part.clone())
                    }
                    Part::ToolResult { .. }
                        if message.role == Role::User
                            && paired_results.contains(&(message_index, part_index)) =>
                    {
                        Some(part.clone())
                    }
                    Part::ToolCall { .. } | Part::ToolResult { .. } => None,
                    _ => Some(part.clone()),
                })
                .collect();
            (!parts.is_empty()).then_some(Message {
                role: message.role,
                parts,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::filter_message_history;
    use kanzei_llm::{Message, Part};

    fn call(id: &str) -> Part {
        Part::ToolCall {
            id: id.into(),
            name: "write".into(),
            input: serde_json::json!({}),
        }
    }

    fn result(id: &str, content: &str, is_error: bool) -> Part {
        Part::ToolResult {
            call_id: id.into(),
            content: content.into(),
            is_error,
        }
    }

    #[test]
    fn keeps_legal_pairs_and_removes_orphans_without_losing_text() {
        let messages = vec![
            Message::user_text("任务"),
            Message::assistant(vec![
                Part::Text {
                    text: "执行".into(),
                },
                call("ok"),
                call("orphan"),
            ]),
            Message::tool_results(vec![result("ok", "真实结果", false)]),
        ];
        let filtered = filter_message_history(&messages);
        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].parts, messages[0].parts);
        assert_eq!(filtered[1].parts.len(), 2);
        assert!(matches!(filtered[1].parts[1], Part::ToolCall { ref id, .. } if id == "ok"));
        assert_eq!(filtered[2].parts, vec![result("ok", "真实结果", false)]);
    }

    #[test]
    fn keeps_error_and_cancel_results_and_drops_unknown_results() {
        let messages = vec![
            Message::assistant(vec![call("denied"), call("cancelled")]),
            Message::tool_results(vec![
                result("denied", "permission denied", true),
                result("cancelled", "cancelled", true),
                result("unknown", "orphan", true),
            ]),
        ];
        let filtered = filter_message_history(&messages);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].parts.len(), 2);
        assert_eq!(filtered[1].parts.len(), 2);
        assert!(
            matches!(filtered[1].parts[0], Part::ToolResult { ref call_id, .. } if call_id == "denied")
        );
        assert!(
            matches!(filtered[1].parts[1], Part::ToolResult { ref call_id, .. } if call_id == "cancelled")
        );
    }

    #[test]
    fn duplicate_ids_pair_one_to_one_in_order() {
        let messages = vec![
            Message::assistant(vec![call("same"), call("same")]),
            Message::tool_results(vec![result("same", "one", false)]),
        ];
        let filtered = filter_message_history(&messages);
        assert_eq!(filtered[0].parts.len(), 1);
        assert_eq!(filtered[1].parts.len(), 1);
    }
}
