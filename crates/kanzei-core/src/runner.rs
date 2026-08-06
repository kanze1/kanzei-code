//! agent 循环(M1):harness 快照驱动——工具物化过权限、system 由 Context Source 拼装、
//! 每次工具调用过硬门禁(deny 回喂模型 / ask 问用户,用户拒绝则整轮停,与 V2 语义一致)。
//! steer/queue/持久化调度在 M2 引入。

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use kanzei_harness::{
    tolerant_parse, tool::repair_hint, AgentDef, Effect, HarnessSnapshot, Tool, ToolCtx,
};
use kanzei_llm::{
    FinishReason, LlmClient, LlmEvent, LlmRequest, Message, Part, Route, ToolSpec, Usage,
};

pub struct RunnerConfig {
    pub model: String,
    pub max_tokens: u32,
}

/// 面向 UI 的运行事件(CLI/桌面端都消费这一层,不直接碰 LlmEvent)。
pub enum RunEvent {
    /// 一轮 provider 调用开始(UI 画轮次分隔)。
    TurnStart { step: u32, max_steps: u32 },
    Text(String),
    Reasoning(String),
    ToolStart { name: String, summary: String },
    ToolEnd {
        name: String,
        ok: bool,
        preview: String,
        /// 结构化展示(diff/终端块),见 ToolOutput::display。
        display: Option<serde_json::Value>,
    },
    StepEnd { usage: Usage, reason: FinishReason },
}

pub struct RunSummary {
    pub text: String,
    pub usage: Usage,
    pub steps: u32,
    /// 用户拒绝权限导致的提前停止。
    pub halted_by_user: bool,
}

/// 权限询问的用户决定。AlwaysAllow 的持久化由 UI 层负责(写入项目配置),
/// runner 只负责本次会话内不再重复询问。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskReply {
    Deny,
    AllowOnce,
    AlwaysAllow,
}

/// 权限询问回调的返回值:异步等待用户决定(CLI 同步问、桌面端走事件+oneshot)。
pub type AskFuture = std::pin::Pin<Box<dyn std::future::Future<Output = AskReply> + Send>>;

#[allow(clippy::too_many_arguments)]
pub async fn run_once(
    client: &LlmClient,
    route: &Route,
    snapshot: &HarnessSnapshot,
    agent: &AgentDef,
    config: &RunnerConfig,
    ctx: &ToolCtx,
    prompt: &str,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
    ask: &mut (dyn FnMut(String, String) -> AskFuture + Send),
) -> anyhow::Result<RunSummary> {
    let tools: Vec<Arc<dyn Tool>> = snapshot.materialize_tools();
    let specs: Vec<ToolSpec> = tools
        .iter()
        .map(|t| ToolSpec {
            name: t.name().to_string(),
            description: t.description(),
            input_schema: t.input_schema(),
        })
        .collect();

    // system 分块:agent 提示词 + harness baseline(M2 起 baseline 进 Context Epoch)。
    let system: Vec<String> = [agent.system.clone(), snapshot.system_baseline()]
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();

    let mut messages = vec![Message::user_text(prompt)];
    let mut total_usage = Usage::default();
    let mut final_text = String::new();
    let max_steps = agent.steps.max(1);
    // 本次运行内已放行的 (action, resource):同一资源不重复问(用户反馈:别烦我)。
    let mut session_approved: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    // "总是允许"的会话内即时生效层(D-006):快照是开跑时定死的,新写入的规则
    // 本次运行读不到——泛化 pattern 记在这里,同类资源当场不再询问。
    let mut session_rules: Vec<(String, String)> = Vec::new();

    for step in 1..=max_steps {
        on_event(RunEvent::TurnStart { step, max_steps });
        let last_step = step == max_steps;
        let request = LlmRequest {
            model: config.model.clone(),
            system: system.clone(),
            messages: messages.clone(),
            // 最后一步收走工具,强制模型收敛(对应 V2 的 max-steps 处理)。
            tools: if last_step { vec![] } else { specs.clone() },
            max_tokens: config.max_tokens,
            temperature: None,
        };

        let mut stream = client.stream(route, &request).await?;
        let mut text_buffers: BTreeMap<usize, String> = BTreeMap::new();
        let mut reasoning_buffers: BTreeMap<usize, String> = BTreeMap::new();
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
                LlmEvent::ReasoningDelta { index, text } => {
                    on_event(RunEvent::Reasoning(text.clone()));
                    reasoning_buffers.entry(index).or_default().push_str(&text);
                }
                // reasoning 连同 signature(codex 的 encrypted_content)入历史,
                // Responses 协议多轮工具循环必须回放;其他协议的 builder 自行忽略。
                LlmEvent::ReasoningEnd { index, signature } => {
                    let text = reasoning_buffers.remove(&index).unwrap_or_default();
                    if !text.is_empty() || signature.is_some() {
                        parts.push(Part::Reasoning { text, signature });
                    }
                }
                LlmEvent::ToolCall { id, name, input, raw_input } => {
                    // 协议层解析失败 → 宽容修复(尾逗号/单引号/裸键/围栏)。
                    let input = if input.is_null() {
                        tolerant_parse(&raw_input).unwrap_or(serde_json::Value::Null)
                    } else {
                        input
                    };
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
            return Ok(RunSummary { text: final_text, usage: total_usage, steps: step, halted_by_user: false });
        }

        let mut results = Vec::new();
        for (id, name, input, raw_input) in calls {
            let Some(tool) = tools.iter().find(|t| t.name() == name) else {
                results.push(Part::ToolResult {
                    call_id: id,
                    content: format!(
                        "unknown tool `{name}`; available: {}",
                        tools.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ")
                    ),
                    is_error: true,
                });
                continue;
            };
            on_event(RunEvent::ToolStart {
                name: name.clone(),
                summary: summarize_input(&input, &raw_input),
            });

            // ---- 硬门禁:权限 Ruleset(deny 回喂模型;ask 问用户,拒绝停整轮)----
            let action = tool.action();
            let mut gate_result = Gate::Pass;
            let mut pending_ask: Vec<String> = Vec::new();
            for resource in tool.resources(&input) {
                // 统一正斜杠,权限 pattern 不用关心平台。
                let normalized = resource.replace('\\', "/");
                match snapshot.evaluate(action, &normalized) {
                    Effect::Deny => {
                        gate_result = Gate::Deny(normalized);
                        break;
                    }
                    Effect::Ask => pending_ask.push(normalized),
                    Effect::Allow => {}
                }
            }
            if matches!(gate_result, Gate::Pass) {
                for resource in pending_ask {
                    let key = (action.to_string(), resource.clone());
                    if session_approved.contains(&key) {
                        continue;
                    }
                    if session_rules.iter().any(|(a, pattern)| {
                        a == action && kanzei_harness::permission::wildcard_match(pattern, &resource)
                    }) {
                        continue;
                    }
                    match ask(action.to_string(), resource.clone()).await {
                        AskReply::Deny => {
                            gate_result = Gate::UserDeclined;
                            break;
                        }
                        AskReply::AllowOnce => {
                            session_approved.insert(key);
                        }
                        AskReply::AlwaysAllow => {
                            session_rules.push((
                                action.to_string(),
                                kanzei_harness::config::generalize_resource(action, &resource),
                            ));
                        }
                    }
                }
            }
            let output = match gate_result {
                Gate::Deny(resource) => kanzei_harness::ToolOutput::error(format!(
                    "permission denied by ruleset: {action} on `{resource}`. \
                     This resource is policy-managed; use the dedicated tool for it.",
                )),
                Gate::UserDeclined => {
                    on_event(RunEvent::ToolEnd {
                        name: name.clone(),
                        ok: false,
                        preview: "(user declined)".into(),
                        display: None,
                    });
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        steps: step,
                        halted_by_user: true,
                    });
                }
                Gate::Pass => {
                    if input.is_null() {
                        repair_hint(tool.as_ref(), &raw_input, "tool input was not valid JSON")
                    } else {
                        tool.execute(input, ctx).await
                    }
                }
            };
            on_event(RunEvent::ToolEnd {
                name: name.clone(),
                ok: !output.is_error,
                preview: preview(&output.content),
                display: output.display.clone(),
            });
            results.push(Part::ToolResult {
                call_id: id,
                content: output.content,
                is_error: output.is_error,
            });
        }
        messages.push(Message::tool_results(results));

        if matches!(finish, FinishReason::MaxTokens | FinishReason::Refusal) {
            return Ok(RunSummary { text: final_text, usage: total_usage, steps: step, halted_by_user: false });
        }
    }

    Ok(RunSummary { text: final_text, usage: total_usage, steps: max_steps, halted_by_user: false })
}

enum Gate {
    Pass,
    Deny(String),
    UserDeclined,
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
