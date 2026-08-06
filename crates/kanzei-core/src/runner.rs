//! 极简 agent 循环(M0):prompt → 流式 LLM → 执行工具 → 续轮,直到无工具调用或步数用尽。
//! V2 语义中的"安全轮次边界/steer/queue"在 M2 引入;此处保持单 drain 单输入。

use std::collections::BTreeMap;

use futures::StreamExt;
use kanzei_harness::{tool::repair_hint, Tool, ToolCtx};
use kanzei_llm::{
    FinishReason, LlmClient, LlmEvent, LlmRequest, Message, Part, Route, ToolSpec, Usage,
};

pub struct RunnerConfig {
    pub model: String,
    pub max_tokens: u32,
    pub max_steps: u32,
    /// system 分块:agent 提示词 + harness baseline。
    pub system: Vec<String>,
}

/// 面向 UI 的运行事件(CLI/TUI 都消费这一层,不直接碰 LlmEvent)。
pub enum RunEvent {
    Text(String),
    Reasoning(String),
    ToolStart { name: String, summary: String },
    ToolEnd { name: String, ok: bool, preview: String },
    StepEnd { usage: Usage, reason: FinishReason },
}

pub struct RunSummary {
    pub text: String,
    pub usage: Usage,
    pub steps: u32,
}

pub async fn run_once(
    client: &LlmClient,
    route: &Route,
    tools: &[Box<dyn Tool>],
    config: &RunnerConfig,
    ctx: &ToolCtx,
    prompt: &str,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> anyhow::Result<RunSummary> {
    let specs: Vec<ToolSpec> = tools
        .iter()
        .map(|t| ToolSpec {
            name: t.name().to_string(),
            description: t.description(),
            input_schema: t.input_schema(),
        })
        .collect();

    let mut messages = vec![Message::user_text(prompt)];
    let mut total_usage = Usage::default();
    let mut final_text = String::new();

    for step in 1..=config.max_steps {
        let last_step = step == config.max_steps;
        let request = LlmRequest {
            model: config.model.clone(),
            system: config.system.clone(),
            messages: messages.clone(),
            // 最后一步收走工具,强制模型收敛(对应 V2 的 max-steps 处理)。
            tools: if last_step { vec![] } else { specs.clone() },
            max_tokens: config.max_tokens,
            temperature: None,
        };

        let mut stream = client.stream(route, &request).await?;
        let mut text_buffers: BTreeMap<usize, String> = BTreeMap::new();
        let mut parts: Vec<Part> = Vec::new();
        let mut calls: Vec<(String, String, serde_json::Value, String)> = Vec::new();
        let mut finish = FinishReason::EndTurn;

        while let Some(event) = stream.next().await {
            match event? {
                LlmEvent::TextDelta { index, text } => {
                    on_event(RunEvent::Text(text.clone()));
                    text_buffers.entry(index).or_default().push_str(&text);
                }
                LlmEvent::TextEnd { index } => {
                    if let Some(text) = text_buffers.remove(&index) {
                        parts.push(Part::Text { text });
                    }
                }
                LlmEvent::ReasoningDelta { text, .. } => {
                    on_event(RunEvent::Reasoning(text));
                }
                LlmEvent::ToolCall { id, name, input, raw_input } => {
                    parts.push(Part::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: if input.is_null() { serde_json::json!({}) } else { input.clone() },
                    });
                    calls.push((id, name, input, raw_input));
                }
                LlmEvent::StepFinish { usage, reason } => {
                    total_usage = add_usage(total_usage, usage);
                    finish = reason.clone();
                    on_event(RunEvent::StepEnd { usage, reason });
                }
                _ => {}
            }
        }
        // 流意外中断时兜底收尾未闭合的文本块。
        for (_, text) in std::mem::take(&mut text_buffers) {
            parts.push(Part::Text { text });
        }

        final_text = parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !parts.is_empty() {
            messages.push(Message::assistant(parts));
        }

        if calls.is_empty() {
            return Ok(RunSummary { text: final_text, usage: total_usage, steps: step });
        }

        let mut results = Vec::new();
        for (id, name, input, raw_input) in calls {
            let output = match tools.iter().find(|t| t.name() == name) {
                Some(tool) => {
                    let summary = summarize_input(&input, &raw_input);
                    on_event(RunEvent::ToolStart { name: name.clone(), summary });
                    if input.is_null() {
                        // 协议层解析失败 → 纠错反馈回喂(设计红线 1)。
                        repair_hint(tool.as_ref(), &raw_input, "tool input was not valid JSON")
                    } else {
                        tool.execute(input, ctx).await
                    }
                }
                None => kanzei_harness::ToolOutput::error(format!(
                    "unknown tool `{name}`; available: {}",
                    tools.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ")
                )),
            };
            on_event(RunEvent::ToolEnd {
                name: name.clone(),
                ok: !output.is_error,
                preview: preview(&output.content),
            });
            results.push(Part::ToolResult {
                call_id: id,
                content: output.content,
                is_error: output.is_error,
            });
        }
        messages.push(Message::tool_results(results));

        // 模型没请求工具却也没结束(如 max_tokens)的场合直接返回。
        if matches!(finish, FinishReason::MaxTokens | FinishReason::Refusal) {
            return Ok(RunSummary { text: final_text, usage: total_usage, steps: step });
        }
    }

    Ok(RunSummary { text: final_text, usage: total_usage, steps: config.max_steps })
}

fn add_usage(a: Usage, b: Usage) -> Usage {
    Usage {
        input: a.input + b.input,
        output: a.output + b.output,
        reasoning: a.reasoning + b.reasoning,
        cache_read: a.cache_read + b.cache_read,
        cache_write: a.cache_write + b.cache_write,
    }
}

fn summarize_input(input: &serde_json::Value, raw: &str) -> String {
    let rendered = if input.is_null() { raw.to_string() } else { input.to_string() };
    match rendered.char_indices().nth(160) {
        Some((idx, _)) => format!("{}…", &rendered[..idx]),
        None => rendered,
    }
}

fn preview(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("");
    let mut p = match first_line.char_indices().nth(120) {
        Some((idx, _)) => format!("{}…", &first_line[..idx]),
        None => first_line.to_string(),
    };
    let lines = content.lines().count();
    if lines > 1 {
        p.push_str(&format!(" (+{} lines)", lines - 1));
    }
    p
}
