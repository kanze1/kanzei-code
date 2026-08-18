//! `question` 工具的交互结果解析。
//!
//! 该模块只负责把 question 输入与 ASK 响应转换为 ToolOutput；调用方仍负责
//! ToolStart/ToolEnd 事件、ToolResult 配对和消息提交，保持工具循环的顺序不变。

use super::*;

pub(super) async fn execute_question(
    config: &RunnerConfig,
    input: &serde_json::Value,
    ask: &mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> kanzei_harness::ToolOutput {
    let question = input
        .get("question")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim();
    let options = input
        .get("options")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let default = input
        .get("default")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let multiple = input
        .get("multiple")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    if question.is_empty() {
        kanzei_harness::ToolOutput::error("question must not be empty")
    } else if !config.ask_policy.allows_user_prompt() {
        // 自举/并行线没有稳定的用户或代理间 ASK 通道；把问题转成
        // 可回喂模型的工具错误，不能等待桌面答复。
        kanzei_harness::ToolOutput::error(
            "question unavailable in autonomous/parallel run: this line cannot ask the user; continue with available evidence",
        )
    } else {
        match ask(AskRequest::Question {
            question: question.to_owned(),
            options,
            default,
            multiple,
        })
        .await
        {
            AskResponse::Answer(answer) => {
                kanzei_harness::ToolOutput::ok(format!("User answer: {answer}"))
            }
            AskResponse::Cancelled => {
                kanzei_harness::ToolOutput::error("question cancelled by user")
            }
            AskResponse::Permission(_) => {
                kanzei_harness::ToolOutput::error("invalid question response")
            }
        }
    }
}
