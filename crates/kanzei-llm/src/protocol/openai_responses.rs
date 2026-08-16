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
    // Codex Fast mode:同一模型走 priority 服务档位,未开启时不带该字段。
    if let Some(service_tier) = request.service_tier.as_deref() {
        body["service_tier"] = json!(service_tier);
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

impl ResponsesState {
    /// 把一条攒够的 function_call 物化成 ToolCall 事件。
    ///
    /// `authoritative` 是 done 事件里的完整 arguments(有就压过增量拼装);
    /// `call_id`/`name` 同理——收尾兜底路径没有这些字段,传 None 用累积值。
    fn emit_tool_call(
        &mut self,
        pending: PendingCall,
        call_id: Option<&str>,
        name: Option<&str>,
        authoritative: Option<&str>,
        out: &mut Vec<LlmEvent>,
    ) {
        self.saw_tool_call = true;
        let raw = authoritative
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
            id: call_id
                .filter(|s| !s.is_empty())
                .unwrap_or(&pending.call_id)
                .to_string(),
            name: name
                .filter(|s| !s.is_empty())
                .unwrap_or(&pending.name)
                .to_string(),
            input,
            raw_input: raw,
        });
    }
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
            "response.function_call_arguments.done" => {
                // 方言差异:有的网关只在这里给完整 arguments,后面不再补
                // output_item.done。收下当权威值,增量拼装作兜底。
                let index = data["output_index"].as_u64().unwrap_or(0);
                if let (Some(call), Some(args)) =
                    (self.calls.get_mut(&index), data["arguments"].as_str())
                {
                    if !args.is_empty() {
                        call.arguments = args.to_string();
                    }
                }
            }
            "response.output_item.done" => {
                let index = data["output_index"].as_u64().unwrap_or(0);
                let item = &data["item"];
                match item["type"].as_str().unwrap_or("") {
                    "function_call" => {
                        let pending = self.calls.remove(&index).unwrap_or_default();
                        // done 事件里的 arguments 是完整权威版本,优先于增量拼装。
                        self.emit_tool_call(
                            pending,
                            item["call_id"].as_str(),
                            item["name"].as_str(),
                            item["arguments"].as_str(),
                            &mut out,
                        );
                    }
                    "reasoning" => {
                        self.reasoning_open = false;
                        out.push(LlmEvent::ReasoningEnd {
                            index: index as usize,
                            signature: item["encrypted_content"].as_str().map(str::to_string),
                        });
                    }
                    "message" if self.text_open.remove(&index).unwrap_or(false) => {
                        out.push(LlmEvent::TextEnd {
                            index: index as usize,
                        });
                    }
                    _ => {}
                }
            }
            "response.completed" | "response.incomplete" => {
                self.finished = true;
                // D-422:收尾时把仍挂在 self.calls 里的调用一次性物化。
                //
                // ToolCall 事件此前**只**在 output_item.done 里产出,而 output_item.done
                // 不是所有方言都发:opencode zen 网关上的 mimo 系模型只发
                // output_item.added + function_call_arguments.delta,然后直接
                // response.completed。于是整轮的工具调用攒在 self.calls 里被静默丢弃,
                // 表现成「跑完一轮 0 次工具调用」,再被鞭挞判成「无动作」——模型明明
                // 调了工具,用户看到的却是连续两轮空转后「可能条目已完成或确实无可推进项」。
                //
                // 参数残缺(截断轮)不物化:宁可少一条,也不把 input=null 的半截调用喂给
                // runner——那会变成一次必然失败的工具执行,比丢掉更难查。
                let pending: Vec<PendingCall> =
                    std::mem::take(&mut self.calls).into_values().collect();
                for call in pending {
                    let raw = if call.arguments.is_empty() {
                        "{}"
                    } else {
                        call.arguments.as_str()
                    };
                    if serde_json::from_str::<Value>(raw).is_err() {
                        continue;
                    }
                    self.emit_tool_call(call, None, None, None, &mut out);
                }
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
                // Responses SSE 的 error 事件有两层包装:
                // {"type":"error","error":{"code":...,"message":...}}。
                // 不能把外层 type 当 provider kind,否则上下文超限会变成
                // unknown/unknown,runner 无法进入压缩重试。
                let err = data.get("error").unwrap_or(&data);
                let kind = err["code"]
                    .as_str()
                    .or_else(|| err["type"].as_str())
                    .or_else(|| data["code"].as_str())
                    .unwrap_or("error");
                let message = err["message"]
                    .as_str()
                    .or_else(|| data["message"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| data.to_string());
                return Err(LlmError::classify_provider(kind.to_string(), message));
            }
            _ => {} // 大量细粒度事件(*.in_progress/*.done 等)按需忽略
        }
        // D-422:response.created 同样不是所有方言都发(mimo 系就不发)。步骤起点
        // 缺席会让上层分步记账落空,按「本流第一条有产出的事件」补一次——合规
        // provider 的 created 先到,这里永远命中不了,行为不变(同 openai.rs 的懒起点)。
        if !self.started && !out.is_empty() {
            self.started = true;
            out.insert(0, LlmEvent::StepStart);
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

    /// D-422:缺 response.created / output_item.done 的方言(实测 opencode zen 网关上的
    /// mimo-v2.5 与 mimo-v2.5-pro,事件序列逐字来自 2026-08-17 的抓包)。工具调用只靠
    /// added + arguments.delta 两类事件到齐,收尾必须把它物化——否则整轮 0 次工具调用。
    #[test]
    fn 缺_output_item_done_的方言也能拿到工具调用() {
        let mut s = ResponsesState::default();
        let ev = feed(
            &mut s,
            r#"{"type":"response.output_item.added","output_index":0,"item":{"id":"call_7db8","type":"function_call","name":"read","call_id":"call_7db8","arguments":""}}"#,
        );
        // created 缺席:起点由第一条有产出的事件补上。
        assert_eq!(
            ev,
            vec![
                LlmEvent::StepStart,
                LlmEvent::ToolInputStart {
                    index: 0,
                    id: "call_7db8".into(),
                    name: "read".into(),
                },
            ]
        );
        for delta in [r#"{\"path\": "#, r#"\""#, "./", "README.md", r#"\""#, "}"] {
            feed(
                &mut s,
                &format!(
                    r#"{{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{delta}"}}"#
                ),
            );
        }
        // mimo 的 response.completed 连 usage 都不带,直接收尾。
        let ev = feed(
            &mut s,
            r#"{"type":"response.completed","response":{"id":"r1","model":"mimo-v2.5-pro"}}"#,
        );
        assert_eq!(
            ev,
            vec![
                LlmEvent::ToolCall {
                    id: "call_7db8".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path": "./README.md"}),
                    raw_input: r#"{"path": "./README.md"}"#.into(),
                },
                LlmEvent::StepFinish {
                    reason: FinishReason::ToolUse,
                    usage: Usage::default(),
                },
            ]
        );
    }

    /// 截断轮:参数只拼到一半就 response.incomplete。半截 JSON 物化出来是 input=null 的
    /// 假调用,执行必失败且难查——收尾兜底必须跳过它,finish 仍是 MaxTokens。
    #[test]
    fn 收尾兜底不物化参数残缺的调用() {
        let mut s = ResponsesState::default();
        feed(
            &mut s,
            r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_1","name":"read","arguments":""}}"#,
        );
        feed(
            &mut s,
            r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"path\": \"a"}"#,
        );
        let ev = feed(
            &mut s,
            r#"{"type":"response.incomplete","response":{"usage":{"input_tokens":10,"output_tokens":2}}}"#,
        );
        assert_eq!(
            ev,
            vec![LlmEvent::StepFinish {
                reason: FinishReason::MaxTokens,
                usage: Usage {
                    input: 10,
                    output: 2,
                    reasoning: 0,
                    cache_read: 0,
                    cache_write: 0
                },
            }]
        );
    }

    #[test]
    fn rate_limit_kind_with_token_message_is_not_context_overflow() {
        let mut s = ResponsesState::default();
        let err = s
            .step(&SseEvent {
                event: String::new(),
                data: r#"{"type":"error","code":"rate_limit_error","message":"token limit reached for this minute"}"#.into(),
            })
            .unwrap_err();
        assert!(err.is_rate_limited());
        assert!(!err.is_context_overflow());
    }

    #[test]
    fn nested_error_context_length_exceeded_triggers_context_overflow() {
        let mut s = ResponsesState::default();
        let err = s
            .step(&SseEvent {
                event: String::new(),
                data: r#"{"type":"error","error":{"code":"context_length_exceeded","message":"Your input exceeds the context window of this model. Please adjust your input and try again.","param":"input","type":"invalid_request_error"},"sequence_number":2}"#.into(),
            })
            .unwrap_err();
        assert!(err.is_context_overflow());
        assert!(!err.is_rate_limited());
        assert!(err
            .to_string()
            .contains("Your input exceeds the context window"));
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
            service_tier: None,
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
            service_tier: None,
        };
        let body = build_body(&base(ReasoningEffort::Off));
        assert_eq!(body["reasoning"]["summary"], "auto");
        assert!(body["reasoning"].get("effort").is_none());

        let body = build_body(&base(ReasoningEffort::Medium));
        assert_eq!(body["reasoning"]["summary"], "auto");
        assert_eq!(body["reasoning"]["effort"], "medium");

        let mut fast = base(ReasoningEffort::Off);
        fast.service_tier = Some("priority".into());
        assert_eq!(build_body(&fast)["service_tier"], "priority");
    }
}
