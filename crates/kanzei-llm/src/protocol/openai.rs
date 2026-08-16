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
                        Part::ToolResult {
                            call_id,
                            content,
                            is_error,
                        } => {
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
                // Chat Completions 无法回放未带签名的 Reasoning part。若这一轮
                // assistant 只有 reasoning（或空文本），旧实现仍会发
                // {"role":"assistant","content":null}，严格 provider 会以
                // "content or tool_calls must be set" 拒绝整个下一次请求。
                // Responses/Anthropic 各自保留它们可表达的 reasoning；此协议只跳过
                // 这个不可序列化、对后续上下文没有文本或工具语义的占位消息。
                if text.is_empty() && tool_calls.is_empty() {
                    continue;
                }
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
    /// D-424:决定这一帧 tool_call delta 落进哪个累加槽。
    ///
    /// 旧实现是 `tc["index"].as_u64().unwrap_or(0)`——`index` 缺席(或 provider 把
    /// 多条调用都标成同一个 index)时,所有调用挤进 0 号槽,而槽里的 id/name/arguments
    /// 都是 `push_str` 累加的。实测(opencode zen 网关把模型吐的 Hermes XML 二次转成
    /// tool_calls 时就这样):`work` + `read` 两条调用被拼成一条 name=`workread`、
    /// id=`call_332b…call_d6d7…`、arguments 是两段 JSON 首尾相接的畸形调用,引擎侧
    /// 报「unknown tool `workread`」,整轮取活直接死掉。
    ///
    /// 规则:`index` 是权威键,但一个已被别的 id 占住的槽不接受新 id——那种情况另起
    /// 一槽。`index` 缺席时,带**新** id 的帧开新调用,不带 id 的帧是上一条的续帧。
    /// 合规 provider(id 只在首帧给、index always present)走不到任何一条分支。
    fn slot_for(&self, index: Option<u64>, id: Option<&str>) -> u64 {
        let last = self.calls.keys().next_back().copied();
        let next = last.map_or(0, |k| k + 1);
        let occupied_by_other = |slot: u64| {
            id.is_some_and(|id| {
                self.calls
                    .get(&slot)
                    .is_some_and(|c| !c.id.is_empty() && c.id != id)
            })
        };
        match index {
            Some(i) if occupied_by_other(i) => next,
            Some(i) => i,
            None => match last {
                Some(k) if !occupied_by_other(k) => k,
                Some(_) => next,
                None => 0,
            },
        }
    }

    /// 关闭未闭合的文本/推理块(幂等)。finish_reason 一到就该关——文本确实到此为止,
    /// 与「工具调用什么时候放出去」是两件事(D-424 把后者挪到了流末)。
    fn close_blocks(&mut self, out: &mut Vec<LlmEvent>) {
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
    }

    /// 关闭未闭合的块并放出累计的 tool calls(幂等)。只在真正的流末调用。
    fn settle(&mut self, out: &mut Vec<LlmEvent>) {
        self.close_blocks(out);
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
            return Err(LlmError::classify_provider_with_code(
                err["type"].as_str().unwrap_or("unknown").to_string(),
                err["code"].as_str().map(str::to_string),
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
                let incoming_id = tc["id"].as_str().unwrap_or("");
                let index = self.slot_for(
                    tc["index"].as_u64(),
                    Some(incoming_id).filter(|s| !s.is_empty()),
                );
                let is_new = !self.calls.contains_key(&index);
                let call = self.calls.entry(index).or_default();
                // 整条 id 每帧重发的 provider 不能把它接成两遍(续帧式的分片 id 仍照接)。
                if !incoming_id.is_empty() && incoming_id != call.id {
                    call.id.push_str(incoming_id);
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
            // D-424:只记 finish_reason,**不**在这里收尾。此前这里 settle 是为了
            // 兜「服务端不发 [DONE]」,代价是 finish_reason 之后还在来的参数增量被
            // 永久丢弃(calls_emitted 已置位)——放出去的就是 `{"action":"claim","id": `
            // 这种半截 JSON。兜底改由流末 finish() 承担,两头都不丢。
            self.finish = Some(map_finish(reason));
            self.close_blocks(&mut out);
        }
        Ok(out)
    }

    /// 流末收尾:[DONE] 没来过就在这里放调用 + StepFinish(幂等,来过就是空)。
    fn finish(&mut self) -> Vec<LlmEvent> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        self.finished = true;
        self.settle(&mut out);
        if self.started {
            out.push(LlmEvent::StepFinish {
                reason: self.finish.clone().unwrap_or(FinishReason::EndTurn),
                usage: self.usage,
            });
        }
        out
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
        // D-424:finish_reason 只记原因,不再当收尾——参数增量可能还在后面。
        let ev = feed(
            &mut s,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls","index":0}]}"#,
        );
        assert!(ev.is_empty(), "finish_reason 不该提前放出工具调用: {ev:?}");
        let ev = feed(&mut s, "[DONE]");
        assert_eq!(
            ev,
            vec![
                LlmEvent::ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path": "a.txt"}),
                    raw_input: r#"{"path":"a.txt"}"#.into(),
                },
                LlmEvent::StepFinish {
                    reason: FinishReason::ToolUse,
                    usage: Usage::default()
                }
            ]
        );
    }

    /// D-424:provider 把两条调用挤在同一个 index(或干脆不发 index)时,旧实现
    /// 全塞进 0 号槽,而槽里的 id/name/arguments 都是 push_str 累加的——`work`+`read`
    /// 会拼成 name=`workread`、参数是两段 JSON 首尾相接的畸形调用,引擎报
    /// 「unknown tool `workread`」。实测来自 opencode zen 网关(2026-08-17)。
    #[test]
    fn 同槽或缺_index_的两条调用不得被拼成一条() {
        for frames in [
            // ① 两条都标 index:0
            [
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"work","arguments":"{\"action\":\"claim\"}"}}]},"index":0}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_b","function":{"name":"read","arguments":"{\"path\":\"a.txt\"}"}}]},"index":0}]}"#,
            ],
            // ② 干脆不发 index
            [
                r#"{"choices":[{"delta":{"tool_calls":[{"id":"call_a","function":{"name":"work","arguments":"{\"action\":\"claim\"}"}}]},"index":0}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"id":"call_b","function":{"name":"read","arguments":"{\"path\":\"a.txt\"}"}}]},"index":0}]}"#,
            ],
        ] {
            let mut s = OpenAiState::default();
            for frame in frames {
                feed(&mut s, frame);
            }
            let calls: Vec<_> = feed(&mut s, "[DONE]")
                .into_iter()
                .filter(|e| matches!(e, LlmEvent::ToolCall { .. }))
                .collect();
            assert_eq!(
                calls,
                vec![
                    LlmEvent::ToolCall {
                        id: "call_a".into(),
                        name: "work".into(),
                        input: serde_json::json!({"action": "claim"}),
                        raw_input: r#"{"action":"claim"}"#.into(),
                    },
                    LlmEvent::ToolCall {
                        id: "call_b".into(),
                        name: "read".into(),
                        input: serde_json::json!({"path": "a.txt"}),
                        raw_input: r#"{"path":"a.txt"}"#.into(),
                    }
                ],
                "两条调用被合并了(旧实现拼出 workread)"
            );
        }
    }

    /// D-424:缺 index 时不带 id 的帧是**续帧**,必须接在上一条上,不能另起一槽。
    #[test]
    fn 缺_index_时无_id_的帧是上一条的续帧() {
        let mut s = OpenAiState::default();
        feed(
            &mut s,
            r#"{"choices":[{"delta":{"tool_calls":[{"id":"call_1","function":{"name":"read","arguments":"{\"path\":"}}]},"index":0}]}"#,
        );
        feed(
            &mut s,
            r#"{"choices":[{"delta":{"tool_calls":[{"function":{"arguments":"\"a.txt\"}"}}]},"index":0}]}"#,
        );
        let calls: Vec<_> = feed(&mut s, "[DONE]")
            .into_iter()
            .filter(|e| matches!(e, LlmEvent::ToolCall { .. }))
            .collect();
        assert_eq!(
            calls,
            vec![LlmEvent::ToolCall {
                id: "call_1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "a.txt"}),
                raw_input: r#"{"path":"a.txt"}"#.into(),
            }]
        );
    }

    /// D-424:finish_reason 之后还在来参数增量的 provider——旧实现在 finish_reason
    /// 处 settle,把后续增量永久锁在门外,放出去的是 `{"action":"claim","id": ` 这类
    /// 半截 JSON。收尾挪到流末后,完整参数才是放出去的那份。且不发 [DONE] 也不丢。
    #[test]
    fn finish_reason_之后的参数增量不丢且不发_done_也能收尾() {
        let mut s = OpenAiState::default();
        feed(
            &mut s,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"work","arguments":"{\"action\": \"claim\", \"id\": "}}]},"index":0}]}"#,
        );
        feed(
            &mut s,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls","index":0}]}"#,
        );
        feed(
            &mut s,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"D-419\"}"}}]},"index":0}]}"#,
        );
        // 服务端不发 [DONE],流直接断——收尾由 client 调的 finish() 承担。
        assert_eq!(
            ProtocolState::finish(&mut s),
            vec![
                LlmEvent::ToolCall {
                    id: "call_1".into(),
                    name: "work".into(),
                    input: serde_json::json!({"action": "claim", "id": "D-419"}),
                    raw_input: r#"{"action": "claim", "id": "D-419"}"#.into(),
                },
                LlmEvent::StepFinish {
                    reason: FinishReason::ToolUse,
                    usage: Usage::default()
                }
            ]
        );
        // 幂等:再调一次是空。
        assert!(ProtocolState::finish(&mut s).is_empty());
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
    fn rate_limit_kind_with_token_message_is_not_context_overflow() {
        let mut s = OpenAiState::default();
        let err = s
            .step(&SseEvent {
                event: String::new(),
                data: r#"{"error":{"type":"rate_limit_error","message":"token limit reached for this minute"}}"#.into(),
            })
            .unwrap_err();
        assert!(err.is_rate_limited());
        assert!(!err.is_context_overflow());
    }

    #[test]
    fn context_length_code_is_not_hidden_by_generic_error_type() {
        let mut s = OpenAiState::default();
        let err = s
            .step(&SseEvent {
                event: String::new(),
                data: r#"{"error":{"type":"invalid_request_error","code":"context_length_exceeded","message":"Your input exceeds the context window of this model"}}"#.into(),
            })
            .unwrap_err();
        assert!(err.is_context_overflow());
        assert!(!err.is_rate_limited());
    }

    #[test]
    fn rate_limit_type_still_wins_when_code_is_more_specific() {
        let mut s = OpenAiState::default();
        let err = s
            .step(&SseEvent {
                event: String::new(),
                data: r#"{"error":{"type":"rate_limit_error","code":"insufficient_quota","message":"token limit reached for this minute"}}"#.into(),
            })
            .unwrap_err();
        assert!(err.is_rate_limited());
        assert!(!err.is_context_overflow());
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
            service_tier: None,
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

    #[test]
    fn body_skips_assistant_history_without_text_or_tool_call() {
        let req = LlmRequest {
            model: "deepseek".into(),
            system: vec!["sys".into()],
            messages: vec![
                Message::user_text("第一问"),
                Message::assistant(vec![Part::Reasoning {
                    text: "仅内部思考".into(),
                    signature: None,
                }]),
                Message::assistant(vec![Part::Text {
                    text: String::new(),
                }]),
                Message::user_text("第二问"),
            ],
            tools: vec![],
            max_tokens: 100,
            temperature: None,
            reasoning: ReasoningEffort::Off,
            service_tier: None,
        };

        let body = build_body(&req);
        let messages = body["messages"]
            .as_array()
            .expect("messages must be an array");
        assert_eq!(
            messages.len(),
            3,
            "system 加两条用户消息；空 assistant 不得出站"
        );
        assert!(messages.iter().all(|message| {
            message["role"] != "assistant"
                || message["content"].is_string()
                || message["tool_calls"].is_array()
        }));
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
            service_tier: None,
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
