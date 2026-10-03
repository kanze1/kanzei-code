//! 提交模型消息和工具结果，并保留本轮未压缩消息。
use super::halt::append_halted_tool_results;
use super::{CancellationToken, Message, MessageCommitReceipt, Part, RunEvent};

pub(super) fn commit_assistant_message(
    messages: &mut Vec<Message>,
    parts: Vec<Part>,
    step: u32,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> anyhow::Result<()> {
    let message = Message::assistant(parts);
    let commit = MessageCommitReceipt::default();
    on_event(RunEvent::AssistantMessageCommitted {
        step,
        message: message.clone(),
        commit: commit.clone(),
    });
    commit.check().map_err(anyhow::Error::msg)?;
    messages.push(message);
    Ok(())
}

/// R-249:`images` 追加在**所有** ToolResult 之后。
///
/// Anthropic 要求 tool_result 块位于 user 消息最前,图片前插会 400;而 results
/// 内部的 `results[i] ↔ calls[i]` 对齐由 note_step 的 debug_assert 锁着,也不允许
/// 在中间插入。两条约束合起来,唯一合法位置就是尾部。
pub(super) fn commit_tool_results(
    messages: &mut Vec<Message>,
    results: Vec<Part>,
    images: Vec<Part>,
    step: u32,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> anyhow::Result<()> {
    let mut results = results;
    results.extend(images);
    let message = Message::tool_results(results);
    let commit = MessageCommitReceipt::default();
    on_event(RunEvent::ToolResultsCommitted {
        step,
        message: message.clone(),
        commit: commit.clone(),
    });
    commit.check().map_err(anyhow::Error::msg)?;
    messages.push(message);
    Ok(())
}

/// D-655:只记录主 runner 提交的本轮消息;事件发生在消息进入可压缩 history 前。
pub(super) fn record_round_message(round_messages: &mut Vec<Message>, event: &RunEvent) {
    match event {
        RunEvent::AssistantMessageCommitted {
            message, commit, ..
        }
        | RunEvent::ToolResultsCommitted {
            message, commit, ..
        } if commit.check().is_ok() => round_messages.push(message.clone()),
        RunEvent::InputMessageCommitted { message, .. } => round_messages.push(message.clone()),
        _ => {}
    }
}

/// R-202 批6:步骤消息提交段的产物。
pub(super) enum StepMessageOutcome {
    /// 消息已提交、存在待执行工具调用,继续工具批执行。
    Proceed,
    /// 提前收尾(calls 为空 = 纯文本步 / D-342 停止占位),调用方构造 RunSummary。
    Return { halted_by_user: bool },
}

/// R-202 批6:步骤消息提交——final_text 提取、assistant 消息落库、以及
/// 「无工具调用」与「产出了调用但停止已置位」两条提前收尾路径。
///
/// 提交拒绝返回错误；成功提交保留原来的收尾语义：
/// - final_text 只取 Text part 拼接(推理/工具调用不进收尾文本);
/// - calls 为空 → halted_by_user 如实反映停止状态(D-342);
/// - 停止已置位 → 全部调用以取消占位配对后 halted 收尾。
pub(super) fn commit_step_messages(
    parts: Vec<Part>,
    calls: &[(String, String, serde_json::Value, String)],
    final_text: &mut String,
    messages: &mut Vec<Message>,
    step: u32,
    halt: Option<&CancellationToken>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> anyhow::Result<StepMessageOutcome> {
    *final_text = parts
        .iter()
        .filter_map(|p| match p {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if !parts.is_empty() {
        commit_assistant_message(messages, parts, step, on_event)?;
    }

    if calls.is_empty() {
        return Ok(StepMessageOutcome::Return {
            // D-342:纯文本步收尾时停止可能已置位,如实标 halted。
            halted_by_user: halt.is_some_and(|token| token.is_cancelled()),
        });
    }

    // D-342:模型产出了工具调用但停止已置位——一个工具都不执行,全部以
    // 取消占位配对(与权限拒绝同款形态),halted 正常收尾。
    if halt.is_some_and(|token| token.is_cancelled()) {
        let mut results = Vec::new();
        append_halted_tool_results(&mut results, calls, 0);
        // 本步工具一个都没执行,不可能有图片。
        commit_tool_results(messages, results, Vec::new(), step, on_event)?;
        return Ok(StepMessageOutcome::Return {
            halted_by_user: true,
        });
    }
    Ok(StepMessageOutcome::Proceed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_messages_never_enter_runtime_or_round_history() {
        let prior = Message::user_text("already committed");
        let mut messages = vec![prior.clone()];
        let mut round = Vec::new();
        let mut sink = |event| {
            match &event {
                RunEvent::AssistantMessageCommitted { commit, .. }
                | RunEvent::ToolResultsCommitted { commit, .. } => {
                    commit.reject("fixture durable rejection");
                }
                _ => unreachable!(),
            }
            record_round_message(&mut round, &event);
        };
        assert!(commit_assistant_message(
            &mut messages,
            vec![Part::Text {
                text: "rejected".into()
            }],
            1,
            &mut sink,
        )
        .is_err());
        assert!(commit_tool_results(
            &mut messages,
            vec![Part::ToolResult {
                call_id: "call".into(),
                content: "rejected result".into(),
                is_error: false,
            }],
            Vec::new(),
            1,
            &mut sink,
        )
        .is_err());
        assert_eq!(messages, vec![prior]);
        assert!(round.is_empty());
    }

    #[test]
    fn steering_receipt_matches_actual_typed_projection_in_message_order() {
        let root = std::env::temp_dir().join(format!(
            "kz-steering-receipt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.db");
        let store = crate::SessionStore::open(&path).unwrap();
        store
            .create_session("ses", root.to_str().unwrap(), None)
            .unwrap();
        let first = Message::user_text("初始用户请求");
        let steering = Message::user_text("实际消费的插话");
        let answer = Message::assistant(vec![Part::Text {
            text: "按照插话完成".into(),
        }]);
        let mut writer = crate::TypedSessionWriter::new(&path, "ses", "run");
        writer.user_message("input", first.clone());
        let mut receipt = vec![first.clone()];
        // App InputInbox persists the steer before the runner publishes this event;
        // CLI persists the same event in its sink. Both have one durable message.
        writer.steering_message("steer", steering.clone());
        record_round_message(
            &mut receipt,
            &RunEvent::InputMessageCommitted {
                input_id: "steer".into(),
                message: steering.clone(),
            },
        );
        writer.turn_started(1, 1);
        writer.assistant_committed(1, answer.clone());
        record_round_message(
            &mut receipt,
            &RunEvent::AssistantMessageCommitted {
                step: 1,
                message: answer.clone(),
                commit: Default::default(),
            },
        );
        writer.finish(crate::SessionTurnTerminal::Completed);
        assert!(writer.errors().is_empty(), "{:?}", writer.errors());
        let projected = crate::project_session_facts(&store.list_session_facts("ses").unwrap());
        assert_eq!(receipt, vec![first, steering, answer]);
        assert_eq!(receipt, projected.surface_messages);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}
