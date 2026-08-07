//! Anthropic Messages 协议:body 构造 + SSE 状态机。
//! 参照 docs/reference/src/anthropic-messages.ts 与官方流式事件序列:
//! message_start → content_block_start/delta/stop* → message_delta → message_stop。

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::error::LlmError;
use crate::event::{FinishReason, LlmEvent, Usage};
use crate::request::{LlmRequest, Part, Role};
use crate::sse::SseEvent;

use super::ProtocolState;

pub fn build_body(request: &LlmRequest) -> Value {
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_tokens,
        "stream": true,
    });

    if !request.system.is_empty() {
        // cache 断点放在 system 末块:Context Epoch 内 baseline 字节不变,稳定命中。
        let last = request.system.len() - 1;
        let system: Vec<Value> = request
            .system
            .iter()
            .enumerate()
            .map(|(i, text)| {
                let mut block = json!({"type": "text", "text": text});
                if i == last {
                    block["cache_control"] = json!({"type": "ephemeral"});
                }
                block
            })
            .collect();
        body["system"] = Value::Array(system);
    }

    let mut messages: Vec<Value> = request.messages.iter().map(message_to_value).collect();
    // 第二个 cache 断点:最后一条消息的最后一个内容块(增量对话前缀复用)。
    if let Some(blocks) = messages
        .last_mut()
        .and_then(|m| m["content"].as_array_mut())
    {
        if let Some(last_block) = blocks.last_mut() {
            last_block["cache_control"] = json!({"type": "ephemeral"});
        }
    }
    body["messages"] = Value::Array(messages);

    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    // thinking 开启时:①budget 必须小于 max_tokens,不够就把上限抬到 budget 之上;
    // ②API 要求 temperature 保持默认,显式带上会被拒。
    if let Some(budget) = request.reasoning.budget_tokens() {
        let min_output = budget.saturating_add(4096);
        if request.max_tokens < min_output {
            body["max_tokens"] = json!(min_output);
        }
        body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
    } else if let Some(t) = request.temperature {
        body["temperature"] = json!(t);
    }
    body
}

fn message_to_value(message: &crate::request::Message) -> Value {
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content: Vec<Value> = message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(json!({"type": "text", "text": text})),
            Part::Image { media_type, data } => Some(json!({
                "type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}
            })),
            Part::Document { media_type, data } => Some(json!({
                "type": "document", "source": {"type": "base64", "media_type": media_type, "data": data}
            })),
            Part::Reasoning { .. } => None,
            Part::ToolCall { id, name, input } => Some(json!({
                "type": "tool_use", "id": id, "name": name, "input": input,
            })),
            Part::ToolResult { call_id, content, is_error } => {
                let mut v = json!({
                    "type": "tool_result", "tool_use_id": call_id, "content": content,
                });
                if *is_error {
                    v["is_error"] = json!(true);
                }
                Some(v)
            }
        })
        .collect();
    json!({"role": role, "content": content})
}

enum Block {
    Text,
    Thinking {
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
}

#[derive(Default)]
pub struct AnthropicState {
    blocks: HashMap<usize, Block>,
    usage: Usage,
    stop_reason: Option<FinishReason>,
}

impl ProtocolState for AnthropicState {
    fn step(&mut self, event: &SseEvent) -> Result<Vec<LlmEvent>, LlmError> {
        let data: Value = match serde_json::from_str(&event.data) {
            Ok(v) => v,
            Err(_) if event.data.is_empty() => Value::Null,
            Err(e) => return Err(LlmError::Protocol(format!("bad SSE data: {e}"))),
        };
        let kind = data["type"].as_str().unwrap_or(event.event.as_str());
        let mut out = Vec::new();
        match kind {
            "message_start" => {
                let usage = &data["message"]["usage"];
                self.usage.input = usage["input_tokens"].as_u64().unwrap_or(0);
                self.usage.cache_read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                self.usage.cache_write = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                out.push(LlmEvent::StepStart);
            }
            "content_block_start" => {
                let index = data["index"].as_u64().unwrap_or(0) as usize;
                let block = &data["content_block"];
                match block["type"].as_str().unwrap_or("") {
                    "text" => {
                        self.blocks.insert(index, Block::Text);
                        out.push(LlmEvent::TextStart { index });
                    }
                    "thinking" | "redacted_thinking" => {
                        self.blocks
                            .insert(index, Block::Thinking { signature: None });
                        out.push(LlmEvent::ReasoningStart { index });
                    }
                    "tool_use" => {
                        let id = block["id"].as_str().unwrap_or("").to_string();
                        let name = block["name"].as_str().unwrap_or("").to_string();
                        self.blocks.insert(
                            index,
                            Block::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                                input_json: String::new(),
                            },
                        );
                        out.push(LlmEvent::ToolInputStart { index, id, name });
                    }
                    other => {
                        return Err(LlmError::Protocol(format!(
                            "unknown content block: {other}"
                        )))
                    }
                }
            }
            "content_block_delta" => {
                let index = data["index"].as_u64().unwrap_or(0) as usize;
                let delta = &data["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => out.push(LlmEvent::TextDelta {
                        index,
                        text: delta["text"].as_str().unwrap_or("").to_string(),
                    }),
                    "thinking_delta" => out.push(LlmEvent::ReasoningDelta {
                        index,
                        text: delta["thinking"].as_str().unwrap_or("").to_string(),
                    }),
                    "input_json_delta" => {
                        let partial = delta["partial_json"].as_str().unwrap_or("");
                        if let Some(Block::ToolUse { input_json, .. }) = self.blocks.get_mut(&index)
                        {
                            input_json.push_str(partial);
                        }
                        out.push(LlmEvent::ToolInputDelta {
                            index,
                            delta: partial.to_string(),
                        });
                    }
                    "signature_delta" => {
                        if let Some(Block::Thinking { signature }) = self.blocks.get_mut(&index) {
                            let sig = delta["signature"].as_str().unwrap_or("");
                            signature.get_or_insert_with(String::new).push_str(sig);
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = data["index"].as_u64().unwrap_or(0) as usize;
                match self.blocks.remove(&index) {
                    Some(Block::Text) => out.push(LlmEvent::TextEnd { index }),
                    Some(Block::Thinking { signature }) => {
                        out.push(LlmEvent::ReasoningEnd { index, signature })
                    }
                    Some(Block::ToolUse {
                        id,
                        name,
                        input_json,
                    }) => {
                        let raw = if input_json.is_empty() {
                            "{}".to_string()
                        } else {
                            input_json
                        };
                        // 解析失败不报错:input 置 Null,raw_input 交给上层修复回路。
                        let input = serde_json::from_str(&raw).unwrap_or(Value::Null);
                        out.push(LlmEvent::ToolCall {
                            id,
                            name,
                            input,
                            raw_input: raw,
                        });
                    }
                    None => {}
                }
            }
            "message_delta" => {
                if let Some(reason) = data["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(map_stop_reason(reason));
                }
                if let Some(output) = data["usage"]["output_tokens"].as_u64() {
                    self.usage.output = output;
                }
            }
            "message_stop" => {
                out.push(LlmEvent::StepFinish {
                    reason: self.stop_reason.clone().unwrap_or(FinishReason::EndTurn),
                    usage: self.usage,
                });
            }
            "ping" | "" => {}
            "error" => {
                let err = &data["error"];
                return Err(LlmError::classify_provider(
                    err["type"].as_str().unwrap_or("unknown").to_string(),
                    err["message"]
                        .as_str()
                        .unwrap_or("unknown error")
                        .to_string(),
                ));
            }
            other => {
                tracing::debug!(kind = other, "ignoring unknown SSE event");
            }
        }
        Ok(out)
    }
}

fn map_stop_reason(reason: &str) -> FinishReason {
    match reason {
        "end_turn" => FinishReason::EndTurn,
        "max_tokens" => FinishReason::MaxTokens,
        "stop_sequence" => FinishReason::StopSequence,
        "tool_use" => FinishReason::ToolUse,
        "refusal" => FinishReason::Refusal,
        other => FinishReason::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Message, ReasoningEffort, ToolSpec};

    fn feed(state: &mut AnthropicState, event: &str, data: &str) -> Vec<LlmEvent> {
        state
            .step(&SseEvent {
                event: event.into(),
                data: data.into(),
            })
            .unwrap()
    }

    #[test]
    fn full_tool_call_stream() {
        let mut s = AnthropicState::default();
        let ev = feed(
            &mut s,
            "message_start",
            r#"{"type":"message_start","message":{"usage":{"input_tokens":100,"cache_read_input_tokens":80,"cache_creation_input_tokens":5}}}"#,
        );
        assert_eq!(ev, vec![LlmEvent::StepStart]);

        feed(
            &mut s,
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        );
        let ev = feed(
            &mut s,
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你好"}}"#,
        );
        assert_eq!(
            ev,
            vec![LlmEvent::TextDelta {
                index: 0,
                text: "你好".into()
            }]
        );
        feed(
            &mut s,
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        );

        feed(
            &mut s,
            "content_block_start",
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"read"}}"#,
        );
        feed(
            &mut s,
            "content_block_delta",
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,
        );
        feed(
            &mut s,
            "content_block_delta",
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"a.txt\"}"}}"#,
        );
        let ev = feed(
            &mut s,
            "content_block_stop",
            r#"{"type":"content_block_stop","index":1}"#,
        );
        assert_eq!(
            ev,
            vec![LlmEvent::ToolCall {
                id: "tu_1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "a.txt"}),
                raw_input: r#"{"path":"a.txt"}"#.into(),
            }]
        );

        feed(
            &mut s,
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}"#,
        );
        let ev = feed(&mut s, "message_stop", r#"{"type":"message_stop"}"#);
        assert_eq!(
            ev,
            vec![LlmEvent::StepFinish {
                reason: FinishReason::ToolUse,
                usage: Usage {
                    input: 100,
                    output: 42,
                    reasoning: 0,
                    cache_read: 80,
                    cache_write: 5
                },
            }]
        );
    }

    #[test]
    fn malformed_tool_json_becomes_null_not_error() {
        let mut s = AnthropicState::default();
        feed(
            &mut s,
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"tu_2","name":"edit"}}"#,
        );
        feed(
            &mut s,
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\": oops"}}"#,
        );
        let ev = feed(
            &mut s,
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        );
        match &ev[0] {
            LlmEvent::ToolCall {
                input, raw_input, ..
            } => {
                assert_eq!(*input, Value::Null);
                assert_eq!(raw_input, "{\"path\": oops");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn overflow_error_is_classified() {
        let mut s = AnthropicState::default();
        let err = s
            .step(&SseEvent {
                event: "error".into(),
                data: r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 250000 tokens > 200000"}}"#.into(),
            })
            .unwrap_err();
        assert!(err.is_context_overflow());
    }

    #[test]
    fn body_places_cache_breakpoints() {
        let req = LlmRequest {
            model: "claude-sonnet-5".into(),
            system: vec!["agent prompt".into(), "harness baseline".into()],
            messages: vec![Message::user_text("hi"), Message::user_text("again")],
            tools: vec![ToolSpec {
                name: "read".into(),
                description: "read a file".into(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
            max_tokens: 1024,
            temperature: None,
            reasoning: ReasoningEffort::Off,
        };
        let body = build_body(&req);
        assert!(body["system"][0].get("cache_control").is_none());
        assert_eq!(body["system"][1]["cache_control"]["type"], "ephemeral");
        assert!(body["messages"][0]["content"][0]
            .get("cache_control")
            .is_none());
        assert_eq!(
            body["messages"][1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(body["tools"][0]["name"], "read");
    }

    /// 思考强度→thinking 预算;开启时必须抬高 max_tokens 且不带 temperature。
    #[test]
    fn thinking_budget_and_max_tokens_follow_effort() {
        let base = |effort, max_tokens, temperature| LlmRequest {
            model: "claude-sonnet-5".into(),
            system: vec!["s".into()],
            messages: vec![Message::user_text("hi")],
            tools: vec![],
            max_tokens,
            temperature,
            reasoning: effort,
        };

        // 关闭:不发 thinking,temperature 照常透传
        let body = build_body(&base(ReasoningEffort::Off, 1024, Some(0.5)));
        assert!(body.get("thinking").is_none());
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["max_tokens"], 1024);

        // 开启:带 budget,max_tokens 被抬到 budget 之上,且不带 temperature
        let body = build_body(&base(ReasoningEffort::Medium, 1024, Some(0.5)));
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 12288);
        assert!(body.get("temperature").is_none());
        assert!(body["max_tokens"].as_u64().unwrap() > 12288);

        // 用户已给足输出上限时不下调
        let body = build_body(&base(ReasoningEffort::Low, 65536, None));
        assert_eq!(body["max_tokens"], 65536);
        assert_eq!(body["thinking"]["budget_tokens"], 4096);
    }
}
