//! OpenAI Chat Completions 兼容协议(流式)。
//! 覆盖:OpenAI、Ollama、LM Studio、llama.cpp server、DeepSeek、Kimi、Groq、OpenRouter 等。
//! 兼容要点:tool_calls 按 index 增量拼装;reasoning_content(DeepSeek/Kimi 风格)归一为
//! Reasoning 事件;usage 靠 stream_options.include_usage 在尾部 chunk 到达;`data: [DONE]` 收尾。

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::error::LlmError;
use crate::event::{FinishReason, LlmEvent, Usage};
use crate::request::{LlmRequest, Part, Role};
use crate::sse::SseEvent;

use super::ProtocolState;

pub fn build_body(request: &LlmRequest) -> Value {
    let mut messages: Vec<Value> = request
        .system
        .iter()
        .map(|s| json!({"role": "system", "content": s}))
        .collect();

    for message in &request.messages {
        match message.role {
            Role::User => {
                for part in &message.parts {
                    match part {
                        Part::Text { text } => {
                            messages.push(json!({"role": "user", "content": text}))
                        }
                        Part::Image { media_type, data } => {
                            // OpenAI Chat Completions 规范的部件类型是 "image_url"(D-028:
                            // 写成 "image" 会被 moonshot 等严格校验的 provider 400 拒收)。
                            messages.push(json!({"role": "user", "content": [{"type":"image_url","image_url":{"url":format!("data:{media_type};base64,{data}")}}]}))
                        }
                        Part::Document { media_type, data } => {
                            // Chat Completions 没有统一的 PDF 输入协议;发送为 data URL,
                            // 支持该扩展的 provider 可直接消费,其它 provider 会返回明确错误。
                            messages.push(json!({"role": "user", "content": [{"type":"file","file":{"filename":"attachment","file_data":format!("data:{media_type};base64,{data}")}}]}))
                        }
                        Part::ToolResult { call_id, content, is_error } => {
                            // OpenAI 无 is_error 字段,错误语义并入正文首行。
                            let content = if *is_error {
                                format!("[tool error]\n{content}")
                            } else {
                                content.clone()
                            };
                            messages.push(json!({
                                "role": "tool", "tool_call_id": call_id, "content": content,
                            }));
                        }
                        _ => {}
                    }
                }
            }
            Role::Assistant => {
                let text: String = message
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let tool_calls: Vec<Value> = message
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::ToolCall { id, name, input } => Some(json!({
                            "id": id,
                            "type": "function",
                            "function": {"name": name, "arguments": input.to_string()},
                        })),
                        _ => None,
                    })
                    .collect();
                let mut m = json!({"role": "assistant"});
                m["content"] = if text.is_empty() {
                    Value::Null
                } else {
                    Value::String(text)
                };
                if !tool_calls.is_empty() {
                    m["tool_calls"] = Value::Array(tool_calls);
                }
                messages.push(m);
            }
        }
    }

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true},
        "max_tokens": request.max_tokens,
    });
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    },
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    if let Some(t) = request.temperature {
        body["temperature"] = json!(t);
    }
    // 推理模型(o 系/gpt-5 等)用 reasoning_effort 档位;关闭时不发,兼容不认该字段的 provider。
    if request.reasoning.enabled() {
        body["reasoning_effort"] = json!(request.reasoning.as_str());
    }
    body
}

#[derive(Default)]
struct PendingCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub struct OpenAiState {
    started: bool,
    text_open: bool,
    reasoning_open: bool,
    calls: BTreeMap<u64, PendingCall>,
    calls_emitted: bool,
    finish: Option<FinishReason>,
    usage: Usage,
    finished: bool,
}

impl OpenAiState {
    /// 关闭未闭合的文本/推理块并放出累计的 tool calls(幂等)。
    fn settle(&mut self, out: &mut Vec<LlmEvent>) {
        if self.text_open {
            self.text_open = false;
            out.push(LlmEvent::TextEnd { index: 0 });
        }
        if self.reasoning_open {
            self.reasoning_open = false;
            out.push(LlmEvent::ReasoningEnd {
                index: 0,
                signature: None,
            });
        }
        if !self.calls_emitted {
            self.calls_emitted = true;
            for (_, call) in std::mem::take(&mut self.calls) {
                let raw = if call.arguments.is_empty() {
                    "{}".to_string()
                } else {
                    call.arguments
                };
                let input = serde_json::from_str(&raw).unwrap_or(Value::Null);
                out.push(LlmEvent::ToolCall {
                    id: call.id,
                    name: call.name,
                    input,
                    raw_input: raw,
                });
            }
        }
    }
}

impl ProtocolState for OpenAiState {
    fn step(&mut self, event: &SseEvent) -> Result<Vec<LlmEvent>, LlmError> {
        let mut out = Vec::new();
        if self.finished {
            return Ok(out);
        }
        if event.data.trim() == "[DONE]" {
            self.finished = true;
            self.settle(&mut out);
            out.push(LlmEvent::StepFinish {
                reason: self.finish.clone().unwrap_or(FinishReason::EndTurn),
                usage: self.usage,
            });
            return Ok(out);
        }

        let data: Value = serde_json::from_str(&event.data)
            .map_err(|e| LlmError::Protocol(format!("bad SSE data: {e}")))?;

        if let Some(err) = data.get("error").filter(|e| !e.is_null()) {
            return Err(LlmError::classify_provider(
                err["type"]
                    .as_str()
                    .or(err["code"].as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                err["message"]
                    .as_str()
                    .unwrap_or(&err.to_string())
                    .to_string(),
            ));
        }

        if !self.started {
            self.started = true;
            out.push(LlmEvent::StepStart);
        }

        if let Some(usage) = data.get("usage").filter(|u| !u.is_null()) {
            let prompt = usage["prompt_tokens"].as_u64().unwrap_or(0);
            let cached = usage["prompt_tokens_details"]["cached_tokens"]
                .as_u64()
                .unwrap_or(0);
            self.usage.input = prompt.saturating_sub(cached);
            self.usage.cache_read = cached;
            self.usage.output = usage["completion_tokens"].as_u64().unwrap_or(0);
            self.usage.reasoning = usage["completion_tokens_details"]["reasoning_tokens"]
                .as_u64()
                .unwrap_or(0);
        }

        let Some(choice) = data["choices"].get(0) else {
            return Ok(out);
        };
        let delta = &choice["delta"];

        // DeepSeek/Kimi 风格思维链字段。
        let reasoning = delta["reasoning_content"]
            .as_str()
            .or(delta["reasoning"].as_str());
        if let Some(text) = reasoning.filter(|t| !t.is_empty()) {
            if !self.reasoning_open {
                self.reasoning_open = true;
                out.push(LlmEvent::ReasoningStart { index: 0 });
            }
            out.push(LlmEvent::ReasoningDelta {
                index: 0,
                text: text.to_string(),
            });
        }

        if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            if self.reasoning_open {
                self.reasoning_open = false;
                out.push(LlmEvent::ReasoningEnd {
                    index: 0,
                    signature: None,
                });
            }
            if !self.text_open {
                self.text_open = true;
                out.push(LlmEvent::TextStart { index: 0 });
            }
            out.push(LlmEvent::TextDelta {
                index: 0,
                text: text.to_string(),
            });
        }

        if let Some(tool_calls) = delta["tool_calls"].as_array() {
            for tc in tool_calls {
                let index = tc["index"].as_u64().unwrap_or(0);
                let is_new = !self.calls.contains_key(&index);
                let call = self.calls.entry(index).or_default();
                if let Some(id) = tc["id"].as_str() {
                    call.id.push_str(id);
                }
                if let Some(name) = tc["function"]["name"].as_str() {
                    call.name.push_str(name);
                }
                if is_new {
                    out.push(LlmEvent::ToolInputStart {
                        index: index as usize,
                        id: call.id.clone(),
                        name: call.name.clone(),
                    });
                }
                if let Some(args) = tc["function"]["arguments"]
                    .as_str()
                    .filter(|a| !a.is_empty())
                {
                    call.arguments.push_str(args);
                    out.push(LlmEvent::ToolInputDelta {
                        index: index as usize,
                        delta: args.to_string(),
                    });
                }
            }
        }

        if let Some(reason) = choice["finish_reason"].as_str() {
            self.finish = Some(map_finish(reason));
            // finish_reason 一到就 settle:即使服务端不发 [DONE],工具调用也不丢。
            self.settle(&mut out);
        }
        Ok(out)
    }
}

fn map_finish(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::EndTurn,
        "length" => FinishReason::MaxTokens,
        "tool_calls" | "function_call" => FinishReason::ToolUse,
        "content_filter" => FinishReason::Refusal,
        other => FinishReason::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Message, ReasoningEffort};

    fn feed(state: &mut OpenAiState, data: &str) -> Vec<LlmEvent> {
        state
            .step(&SseEvent {
                event: String::new(),
                data: data.into(),
            })
            .unwrap()
    }

    #[test]
    fn text_then_done() {
        let mut s = OpenAiState::default();
        let ev = feed(
            &mut s,
            r#"{"choices":[{"delta":{"content":"你"},"index":0}]}"#,
        );
        assert_eq!(
            ev,
            vec![
                LlmEvent::StepStart,
                LlmEvent::TextStart { index: 0 },
                LlmEvent::TextDelta {
                    index: 0,
                    text: "你".into()
                }
            ]
        );
        feed(
            &mut s,
            r#"{"choices":[{"delta":{"content":"好"},"index":0}]}"#,
        );
        let ev = feed(
            &mut s,
            r#"{"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#,
        );
        assert_eq!(ev, vec![LlmEvent::TextEnd { index: 0 }]);
        let ev = feed(
            &mut s,
            r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":4}}}"#,
        );
        assert!(ev.is_empty());
        let ev = feed(&mut s, "[DONE]");
        assert_eq!(
            ev,
            vec![LlmEvent::StepFinish {
                reason: FinishReason::EndTurn,
                usage: Usage {
                    input: 6,
                    output: 2,
                    reasoning: 0,
                    cache_read: 4,
                    cache_write: 0
                },
            }]
        );
    }

    #[test]
    fn incremental_tool_call_assembly() {
        let mut s = OpenAiState::default();
        feed(
            &mut s,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":""}}]},"index":0}]}"#,
        );
        feed(
            &mut s,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":\"a"}}]},"index":0}]}"#,
        );
        feed(
            &mut s,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":".txt\"}"}}]},"index":0}]}"#,
        );
        let ev = feed(
            &mut s,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls","index":0}]}"#,
        );
        assert_eq!(
            ev,
            vec![LlmEvent::ToolCall {
                id: "call_1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "a.txt"}),
                raw_input: r#"{"path":"a.txt"}"#.into(),
            }]
        );
        // settle 幂等:[DONE] 不重复放出 ToolCall。
        let ev = feed(&mut s, "[DONE]");
        assert_eq!(
            ev,
            vec![LlmEvent::StepFinish {
                reason: FinishReason::ToolUse,
                usage: Usage::default()
            }]
        );
    }

    #[test]
    fn reasoning_content_maps_to_reasoning_events() {
        let mut s = OpenAiState::default();
        let ev = feed(
            &mut s,
            r#"{"choices":[{"delta":{"reasoning_content":"思考中"},"index":0}]}"#,
        );
        assert_eq!(
            ev,
            vec![
                LlmEvent::StepStart,
                LlmEvent::ReasoningStart { index: 0 },
                LlmEvent::ReasoningDelta {
                    index: 0,
                    text: "思考中".into()
                }
            ]
        );
        let ev = feed(
            &mut s,
            r#"{"choices":[{"delta":{"content":"答案"},"index":0}]}"#,
        );
        assert_eq!(
            ev[0],
            LlmEvent::ReasoningEnd {
                index: 0,
                signature: None
            }
        );
    }

    #[test]
    fn body_maps_tool_results_to_tool_role() {
        let req = LlmRequest {
            model: "qwen3".into(),
            system: vec!["sys".into()],
            messages: vec![
                Message::user_text("hi"),
                Message::assistant(vec![Part::ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path": "a.txt"}),
                }]),
                Message::tool_results(vec![Part::ToolResult {
                    call_id: "call_1".into(),
                    content: "data".into(),
                    is_error: false,
                }]),
            ],
            tools: vec![],
            max_tokens: 100,
            temperature: None,
            reasoning: ReasoningEffort::Off,
        };
        let body = build_body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(
            body["messages"][2]["tool_calls"][0]["function"]["name"],
            "read"
        );
        assert_eq!(body["messages"][3]["role"], "tool");
        assert_eq!(body["messages"][3]["tool_call_id"], "call_1");
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    /// 思考强度→reasoning_effort;关闭时不得出现该字段(旧 provider 不认)。
    #[test]
    fn reasoning_effort_is_sent_only_when_enabled() {
        let base = |effort| LlmRequest {
            model: "gpt-5".into(),
            system: vec!["s".into()],
            messages: vec![Message::user_text("hi")],
            tools: vec![],
            max_tokens: 100,
            temperature: None,
            reasoning: effort,
        };
        assert!(build_body(&base(ReasoningEffort::Off))
            .get("reasoning_effort")
            .is_none());
        assert_eq!(
            build_body(&base(ReasoningEffort::High))["reasoning_effort"],
            "high"
        );
        assert_eq!(
            build_body(&base(ReasoningEffort::Low))["reasoning_effort"],
            "low"
        );
    }
}
