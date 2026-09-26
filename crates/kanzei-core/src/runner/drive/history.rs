//! 提交模型消息和工具结果，并保留本轮未压缩消息。
use super::{Message, Part, RunEvent};

pub(super) fn commit_assistant_message(
    messages: &mut Vec<Message>,
    parts: Vec<Part>,
    step: u32,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) {
    let message = Message::assistant(parts);
    on_event(RunEvent::AssistantMessageCommitted {
        step,
        message: message.clone(),
    });
    messages.push(message);
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
) {
    let mut results = results;
    results.extend(images);
    let message = Message::tool_results(results);
    on_event(RunEvent::ToolResultsCommitted {
        step,
        message: message.clone(),
    });
    messages.push(message);
}

/// D-655:只记录主 runner 提交的本轮消息;事件发生在消息进入可压缩 history 前。
pub(super) fn record_round_message(round_messages: &mut Vec<Message>, event: &RunEvent) {
    match event {
        RunEvent::AssistantMessageCommitted { message, .. }
        | RunEvent::ToolResultsCommitted { message, .. } => round_messages.push(message.clone()),
        _ => {}
    }
}
