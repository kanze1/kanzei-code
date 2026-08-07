//! OpenAI Responses 协议(流式)。Codex 订阅后端(chatgpt.com/backend-api/codex)
//! 与 OpenAI 平台的 /v1/responses 都说这个。
//! 关键差异 vs Chat Completions:input 是条目数组(message/function_call/
//! function_call_output/reasoning);reasoning 以 encrypted_content 形式在多轮工具
//! 循环中必须原样回放(否则 codex 系模型直接退智),映射到 Part::Reasoning.signature。

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::error::LlmError;
use crate::event::{FinishReason, LlmEvent, Usage};
use crate::request::{LlmRequest, Part, Role};
use crate::sse::SseEvent;

use super::ProtocolState;

pub fn build_body(request: &LlmRequest) -> Value {
    let mut input: Vec<Value> = Vec::new();
    for message in &request.messages {
        match message.role {
            Role::User => {
                for part in &message.parts {
                    match part {
                        Part::Text { text } => input.push(json!({
                            "type": "message", "role": "user",
                            "content": [{"type": "input_text", "text": text}],
                        })),
                        Part::ToolResult { call_id, content, is_error } => {
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
                        // 多模态附件(R-014):Responses 的 input_image / input_file 条目。
                        Part::Image { media_type, data } => input.push(json!({
                            "type": "message", "role": "user",
                            "content": [{"type": "input_image", "image_url": format!("data:{media_type};base64,{data}")}],
                        })),
                        Part::Document { media_type, data } => input.push(json!({
                            "type": "message", "role": "user",
                            "content": [{"type": "input_file", "filename": "attachment.pdf", "file_data": format!("data:{media_type};base64,{data}")}],
                        })),
                        _ => {}
                    }
                }
            }
            Role::Assistant => {
                for part in &message.parts {
                    match part {
                        Part::Text { text } => input.push(json!({
                            "type": "message", "role": "assistant",
                            "content": [{"type": "output_text", "text": text}],
                        })),
                        // encrypted reasoning 原样回放(store:false 模式的硬要求)。
                        Part::Reasoning {
                            signature: Some(encrypted),
                            ..
                        } => input.push(json!({
                            "type": "reasoning",
                            "summary": [],
                            "encrypted_content": encrypted,
                        })),
                        Part::Reasoning {
                            signature: None, ..
                        } => {}
                        Part::ToolCall {
                            id,
                            name,
                            input: args,
                        } => input.push(json!({
                            "type": "function_call",
                            "call_id": id,
                            "name": name,
                            "arguments": args.to_string(),
                        })),
                        Part::ToolResult { .. } | Part::Image { .. } | Part::Document { .. } => {}
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
        "parallel_tool_calls": true,
        "reasoning": {"summary": "auto"},
        "store": false,
        "stream": true,
        "include": ["reasoning.encrypted_content"],
        // 注意:codex 订阅后端拒绝 max_output_tokens,输出长度由服务端管理。
    });
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                    "strict": false,
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    // reasoning 对象本来就在(summary=auto),按档位补 effort;关闭时保持原样不带 effort。
    if request.reasoning.enabled() {
        body["reasoning"]["effort"] = json!(request.reasoning.as_str());
    }
    body
}

#[derive(Default)]
struct PendingCall {
    call_id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub struct ResponsesState {
    started: bool,
    text_open: BTreeMap<u64, bool>,
    reasoning_open: bool,
    calls: BTreeMap<u64, PendingCall>,
    saw_tool_call: bool,
    finished: bool,
}

impl ProtocolState for ResponsesState {
    fn step(&mut self, event: &SseEvent) -> Result<Vec<LlmEvent>, LlmError> {
        if self.finished || event.data.trim() == "[DONE]" {
            return Ok(Vec::new());
        }
        let data: Value = serde_json::from_str(&event.data)
            .map_err(|e| LlmError::Protocol(format!("bad SSE data: {e}")))?;
        let kind = data["type"].as_str().unwrap_or(event.event.as_str());
        let mut out = Vec::new();

        match kind {
            "response.created" => {
                if !self.started {
                    self.started = true;
                    out.push(LlmEvent::StepStart);
                }
            }
            "response.output_item.added" => {
                let index = data["output_index"].as_u64().unwrap_or(0);
                let item = &data["item"];
                match item["type"].as_str().unwrap_or("") {
                    "function_call" => {
                        let call = PendingCall {
                            call_id: item["call_id"].as_str().unwrap_or("").to_string(),
                            name: item["name"].as_str().unwrap_or("").to_string(),
                            arguments: item["arguments"].as_str().unwrap_or("").to_string(),
                        };
                        out.push(LlmEvent::ToolInputStart {
                            index: index as usize,
                            id: call.call_id.clone(),
                            name: call.name.clone(),
                        });
                        self.calls.insert(index, call);
                    }
                    "reasoning" => {
                        self.reasoning_open = true;
                        out.push(LlmEvent::ReasoningStart {
                            index: index as usize,
                        });
                    }
                    _ => {}
                }
            }
            "response.output_text.delta" => {
                let index = data["output_index"].as_u64().unwrap_or(0);
                let delta = data["delta"].as_str().unwrap_or("");
                if !self.text_open.get(&index).copied().unwrap_or(false) {
                    self.text_open.insert(index, true);
                    out.push(LlmEvent::TextStart {
                        index: index as usize,
                    });
                }
                out.push(LlmEvent::TextDelta {
                    index: index as usize,
                    text: delta.to_string(),
                });
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                let index = data["output_index"].as_u64().unwrap_or(0);
                if !self.reasoning_open {
                    self.reasoning_open = true;
                    out.push(LlmEvent::ReasoningStart {
                        index: index as usize,
                    });
                }
                out.push(LlmEvent::ReasoningDelta {
                    index: index as usize,
                    text: data["delta"].as_str().unwrap_or("").to_string(),
                });
            }
            "response.function_call_arguments.delta" => {
                let index = data["output_index"].as_u64().unwrap_or(0);
                let delta = data["delta"].as_str().unwrap_or("");
                if let Some(call) = self.calls.get_mut(&index) {
                    call.arguments.push_str(delta);
                }
                out.push(LlmEvent::ToolInputDelta {
                    index: index as usize,
                    delta: delta.to_string(),
                });
            }
            "response.output_item.done" => {
                let index = data["output_index"].as_u64().unwrap_or(0);
                let item = &data["item"];
                match item["type"].as_str().unwrap_or("") {
                    "function_call" => {
                        self.saw_tool_call = true;
                        let pending = self.calls.remove(&index).unwrap_or_default();
                        // done 事件里的 arguments 是完整权威版本,优先于增量拼装。
                        let raw = item["arguments"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .unwrap_or(pending.arguments);
                        let raw = if raw.is_empty() {
                            "{}".to_string()
                        } else {
                            raw
                        };
                        let input = serde_json::from_str(&raw).unwrap_or(Value::Null);
                        out.push(LlmEvent::ToolCall {
                            id: item["call_id"]
                                .as_str()
                                .unwrap_or(&pending.call_id)
                                .to_string(),
                            name: item["name"].as_str().unwrap_or(&pending.name).to_string(),
                            input,
                            raw_input: raw,
                        });
                    }
                    "reasoning" => {
                        self.reasoning_open = false;
                        out.push(LlmEvent::ReasoningEnd {
                            index: index as usize,
                            signature: item["encrypted_content"].as_str().map(str::to_string),
                        });
                    }
                    "message" => {
                        if self.text_open.remove(&index).unwrap_or(false) {
                            out.push(LlmEvent::TextEnd {
                                index: index as usize,
                            });
                        }
                    }
                    _ => {}
                }
            }
            "response.completed" | "response.incomplete" => {
                self.finished = true;
                let usage = &data["response"]["usage"];
                let input = usage["input_tokens"].as_u64().unwrap_or(0);
                let cached = usage["input_tokens_details"]["cached_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                let reason = if kind == "response.incomplete" {
                    FinishReason::MaxTokens
                } else if self.saw_tool_call {
                    FinishReason::ToolUse
                } else {
                    FinishReason::EndTurn
                };
                out.push(LlmEvent::StepFinish {
                    reason,
                    usage: Usage {
                        input: input.saturating_sub(cached),
                        output: usage["output_tokens"].as_u64().unwrap_or(0),
                        reasoning: usage["output_tokens_details"]["reasoning_tokens"]
                            .as_u64()
                            .unwrap_or(0),
                        cache_read: cached,
                        cache_write: 0,
                    },
                });
            }
            "response.failed" => {
                let err = &data["response"]["error"];
                return Err(LlmError::classify_provider(
                    err["code"]
                        .as_str()
                        .unwrap_or("response_failed")
                        .to_string(),
                    err["message"]
                        .as_str()
                        .unwrap_or("response failed")
                        .to_string(),
                ));
            }
            "error" => {
                return Err(LlmError::classify_provider(
                    data["code"].as_str().unwrap_or("error").to_string(),
                    data["message"]
                        .as_str()
                        .unwrap_or(&data.to_string())
                        .to_string(),
                ));
            }
            _ => {} // 大量细粒度事件(*.in_progress/*.done 等)按需忽略
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Message, ReasoningEffort};

    fn feed(state: &mut ResponsesState, data: &str) -> Vec<LlmEvent> {
        state
            .step(&SseEvent {
                event: String::new(),
                data: data.into(),
            })
            .unwrap()
    }

    #[test]
    fn text_and_tool_flow() {
        let mut s = ResponsesState::default();
        assert_eq!(
            feed(&mut s, r#"{"type":"response.created","response":{}}"#),
            vec![LlmEvent::StepStart]
        );
        let ev = feed(
            &mut s,
            r#"{"type":"response.output_text.delta","output_index":0,"delta":"你好"}"#,
        );
        assert_eq!(ev[0], LlmEvent::TextStart { index: 0 });
        feed(
            &mut s,
            r#"{"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"call_9","name":"read","arguments":""}}"#,
        );
        feed(
            &mut s,
            r#"{"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"path\":\"a.txt\"}"}"#,
        );
        let ev = feed(
            &mut s,
            r#"{"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","call_id":"call_9","name":"read","arguments":"{\"path\":\"a.txt\"}"}}"#,
        );
        assert_eq!(
            ev,
            vec![LlmEvent::ToolCall {
                id: "call_9".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "a.txt"}),
                raw_input: r#"{"path":"a.txt"}"#.into(),
            }]
        );
        let ev = feed(
            &mut s,
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":40},"output_tokens":9,"output_tokens_details":{"reasoning_tokens":3}}}}"#,
        );
        assert_eq!(
            ev,
            vec![LlmEvent::StepFinish {
                reason: FinishReason::ToolUse,
                usage: Usage {
                    input: 60,
                    output: 9,
                    reasoning: 3,
                    cache_read: 40,
                    cache_write: 0
                },
            }]
        );
    }

    #[test]
    fn reasoning_encrypted_roundtrip() {
        let mut s = ResponsesState::default();
        feed(&mut s, r#"{"type":"response.created","response":{}}"#);
        feed(
            &mut s,
            r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning"}}"#,
        );
        let ev = feed(
            &mut s,
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","encrypted_content":"ENC123"}}"#,
        );
        assert_eq!(
            ev,
            vec![LlmEvent::ReasoningEnd {
                index: 0,
                signature: Some("ENC123".into())
            }]
        );

        // body 回放:带 signature 的 Reasoning part → reasoning 条目
        let req = LlmRequest {
            model: "gpt-5.1-codex".into(),
            system: vec!["sys".into()],
            messages: vec![Message::assistant(vec![
                Part::Reasoning {
                    text: "…".into(),
                    signature: Some("ENC123".into()),
                },
                Part::ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({}),
                },
            ])],
            tools: vec![],
            max_tokens: 100,
            temperature: None,
            reasoning: ReasoningEffort::Off,
        };
        let body = build_body(&req);
        assert_eq!(body["input"][0]["type"], "reasoning");
        assert_eq!(body["input"][0]["encrypted_content"], "ENC123");
        assert_eq!(body["input"][1]["type"], "function_call");
        assert_eq!(body["store"], false);
    }

    /// 思考强度→reasoning.effort;关闭时保留既有 summary=auto 且不带 effort。
    #[test]
    fn reasoning_effort_merges_into_reasoning_object() {
        let base = |effort| LlmRequest {
            model: "gpt-5-codex".into(),
            system: vec!["s".into()],
            messages: vec![Message::user_text("hi")],
            tools: vec![],
            max_tokens: 100,
            temperature: None,
            reasoning: effort,
        };
        let body = build_body(&base(ReasoningEffort::Off));
        assert_eq!(body["reasoning"]["summary"], "auto");
        assert!(body["reasoning"].get("effort").is_none());

        let body = build_body(&base(ReasoningEffort::Medium));
        assert_eq!(body["reasoning"]["summary"], "auto");
        assert_eq!(body["reasoning"]["effort"], "medium");
    }
}
