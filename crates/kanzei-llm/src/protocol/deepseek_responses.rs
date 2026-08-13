//! DeepSeek Responses API 方言。
//!
//! DeepSeek 与 OpenAI/Codex 共用 `/responses` 的事件形状，但请求契约并不等价：
//! DeepSeek 是无状态接口，工具循环必须重发完整历史；思考内容以明文
//! `reasoning.content` 回放，也不接受 Codex 的 encrypted_content/include/store。

use serde_json::{json, Value};

use crate::request::{LlmRequest, Part, Role};

pub fn build_body(request: &LlmRequest) -> Value {
    let mut input: Vec<Value> = Vec::new();
    for message in &request.messages {
        match message.role {
            Role::User => {
                for part in &message.parts {
                    match part {
                        Part::Text { text } => input.push(json!({
                            "type": "message",
                            "role": "user",
                            "content": [{"type": "input_text", "text": text}],
                        })),
                        Part::ToolResult {
                            call_id,
                            content,
                            is_error,
                        } => {
                            let output = if *is_error {
                                format!("[tool error]\n{content}")
                            } else {
                                content.clone()
                            };
                            input.push(json!({
                                "type": "function_call_output",
                                "call_id": call_id,
                                "output": output,
                            }));
                        }
                        // DeepSeek Responses 当前仅支持文本输入。这里不伪造占位内容；
                        // 多模态能力由上层在 provider 能力校验中显式拒绝。
                        Part::Image { .. } | Part::Document { .. } => {}
                        Part::Reasoning { .. } | Part::ToolCall { .. } => {}
                    }
                }
            }
            Role::Assistant => {
                for part in &message.parts {
                    match part {
                        Part::Text { text } => input.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": text}],
                        })),
                        Part::Reasoning { text, .. } if !text.is_empty() => input.push(json!({
                            "type": "reasoning",
                            "content": [{"type": "reasoning_text", "text": text}],
                        })),
                        Part::ToolCall {
                            id,
                            name,
                            input: arguments,
                        } => input.push(json!({
                            "type": "function_call",
                            "call_id": id,
                            "name": name,
                            "arguments": arguments.to_string(),
                        })),
                        Part::Reasoning { .. }
                        | Part::ToolResult { .. }
                        | Part::Image { .. }
                        | Part::Document { .. } => {}
                    }
                }
            }
        }
    }

    let mut body = json!({
        "model": request.model,
        "instructions": request.system.join("\n\n"),
        "input": input,
        "tool_choice": "auto",
        "stream": true,
        "max_output_tokens": request.max_tokens,
    });
    if !request.tools.is_empty() {
        body["tools"] = Value::Array(
            request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    })
                })
                .collect(),
        );
    }
    if request.reasoning.enabled() {
        // DeepSeek 当前把 low/medium 兼容映射为 high；在请求侧直接归一化，
        // 避免调用者误以为得到了更低的原生档位。
        body["reasoning"] = json!({"effort": "high"});
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Message, ReasoningEffort, ToolSpec};

    #[test]
    fn deepseek_body_replays_plain_reasoning_without_codex_fields() {
        let request = LlmRequest {
            model: "deepseek-v4-flash".into(),
            system: vec!["system".into()],
            messages: vec![
                Message::assistant(vec![
                    Part::Reasoning {
                        text: "think".into(),
                        signature: Some("must-not-be-sent".into()),
                    },
                    Part::ToolCall {
                        id: "call_1".into(),
                        name: "read".into(),
                        input: json!({"path": "a.txt"}),
                    },
                ]),
                Message::tool_results(vec![Part::ToolResult {
                    call_id: "call_1".into(),
                    content: "ok".into(),
                    is_error: false,
                }]),
            ],
            tools: vec![ToolSpec {
                name: "read".into(),
                description: "read".into(),
                input_schema: json!({"type": "object"}),
            }],
            max_tokens: 321,
            temperature: Some(0.2),
            reasoning: ReasoningEffort::Medium,
            service_tier: Some("priority".into()),
        };

        let body = build_body(&request);
        assert_eq!(body["input"][0]["content"][0]["text"], "think");
        assert_eq!(body["input"][1]["type"], "function_call");
        assert_eq!(body["input"][2]["type"], "function_call_output");
        assert_eq!(body["max_output_tokens"], 321);
        assert_eq!(body["reasoning"]["effort"], "high");
        for unsupported in [
            "include",
            "store",
            "previous_response_id",
            "service_tier",
            "parallel_tool_calls",
            "temperature",
        ] {
            assert!(body.get(unsupported).is_none(), "不应发送 {unsupported}");
        }
        assert!(!body.to_string().contains("must-not-be-sent"));
        assert!(!body.to_string().contains("encrypted_content"));
    }
}
