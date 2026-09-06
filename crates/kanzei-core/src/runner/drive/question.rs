//! `question` 工具的交互结果解析。
//!
//! 该模块只负责把 question 输入与 ASK 响应转换为 ToolOutput；调用方仍负责
//! ToolStart/ToolEnd 事件、ToolResult 配对和消息提交，保持工具循环的顺序不变。

use super::*;

pub(super) async fn execute_question(
    ask_policy: AskPolicy,
    input: &serde_json::Value,
    ask: &mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> kanzei_harness::ToolOutput {
    let question = input
        .get("question")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim();
    // R-328:选项既吃裸字符串也吃 {label, note}——见 AskOption::from_json。
    let options: Vec<AskOption> = input
        .get("options")
        .and_then(|value| value.as_array())
        .map(|items| items.iter().filter_map(AskOption::from_json).collect())
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
    } else if !ask_policy.allows_user_prompt() {
        deferred_question(question, &options)
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

fn deferred_question(question: &str, options: &[AskOption]) -> kanzei_harness::ToolOutput {
    // ToolEnd 的 content/display 随既有会话事件持久化，实时和历史回放共用回复入口。
    // 只登记澄清问题；权限请求仍由原 ASK 策略拒绝，不能由选项默认值代替授权。
    let pending = serde_json::json!({
        "kind": "pending_question", "question": question, "options": options,
        "instruction": "当前没有答案。问题保存在工具记录，用户可稍后回复。若阻塞当前任务，使用 tracker 阻塞字段记录问题和解除条件:用户，再 work next。不要假定默认答案或重复提问。",
    });
    kanzei_harness::ToolOutput::needs_confirmation("QUESTION_PENDING", pending.to_string())
        .with_display(pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_question_preserves_question_without_synthesizing_answer() {
        let output = deferred_question("选择哪个 provider？", &[]);
        assert_eq!(output.code, Some("QUESTION_PENDING"));
        assert!(output.is_error);
        assert_eq!(
            output.display.as_ref().unwrap()["question"],
            "选择哪个 provider？"
        );
        assert!(output.content.contains("当前没有答案"));
        assert!(output.content.contains("解除条件:用户"));
    }

    #[tokio::test]
    async fn autonomous_question_returns_pending_without_waiting_or_using_default() {
        let mut ask = |_request| -> AskFuture { panic!("自主问题不得等待交互 ASK") };
        for policy in [AskPolicy::NonInteractive, AskPolicy::AutoAllow] {
            let output = execute_question(
                policy,
                &serde_json::json!({
                    "question":"选择 provider", "options":["本地","远程"], "default":"远程",
                }),
                &mut ask,
            )
            .await;
            assert_eq!(output.code, Some("QUESTION_PENDING"));
            assert!(!output.model_content().contains("User answer:"));
            assert!(output.display.as_ref().unwrap().get("default").is_none());
        }
    }

    #[tokio::test]
    async fn interactive_question_still_requires_real_answer() {
        let mut ask = |request| -> AskFuture {
            assert!(matches!(request, AskRequest::Question { .. }));
            Box::pin(async { AskResponse::Answer("本地".into()) })
        };
        let output = execute_question(
            AskPolicy::Interactive,
            &serde_json::json!({"question":"选择 provider"}),
            &mut ask,
        )
        .await;
        assert!(!output.is_error);
        assert_eq!(output.content, "User answer: 本地");
    }
}
