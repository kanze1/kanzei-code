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
    FinishReason, LlmClient, LlmEvent, LlmRequest, Message, Part, ReasoningEffort, Role, Route,
    ToolSpec, Usage,
};

pub struct RunnerConfig {
    pub model: String,
    pub max_tokens: u32,
    /// 思考强度;由调用方(CLI/桌面端)按配置或运行时选择传入。
    pub reasoning: ReasoningEffort,
}

/// 单轮子代理上限：并行仍保持，但避免模型一次生成过多请求拖垮连接/本地模型。
pub const MAX_TASKS_PER_TURN: usize = 8;

/// task 子代理运行时(R-004/R-012)。快照由调用方用 SubagentBase 组件构建,
/// 代码层面只含只读工具——子代理无人应答权限询问,必须做到零 ask。
pub struct SubagentRuntime {
    pub snapshot: Arc<HarnessSnapshot>,
    pub agent: AgentDef,
    /// (route, model id):fast = 本地小模型跑机械检索。
    pub fast: (Route, String),
    /// primary = 主模型,给需要理解代码的任务。
    pub primary: (Route, String),
    pub max_tokens: u32,
    /// 单个子代理的墙钟上限(秒):本地模型多轮可能极慢,必须有界。
    pub timeout_secs: u64,
}

fn task_spec() -> ToolSpec {
    ToolSpec {
        name: "task".into(),
        description: "Delegate a narrow read-only exploration task (find files, call \
                      sites, usages; read and summarize code) to a subagent with tools \
                      read/glob/grep. Params: prompt (self-contained instruction saying \
                      exactly what to find and what to report back); optional model: \
                      \"fast\" (default, local model, mechanical searches) | \"primary\" \
                      (tasks needing code comprehension). Multiple task calls in one \
                      turn run in parallel."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Self-contained task: what to find and exactly what to report back"
                },
                "model": {
                    "type": "string",
                    "enum": ["fast", "primary"],
                    "description": "fast = local small model (default); primary = main model"
                }
            },
            "required": ["prompt"]
        }),
    }
}

#[derive(Clone, Debug)]
pub struct TaskTrace {
    pub child_id: String,
    pub phase: String,
    pub name: String,
    pub summary: Option<String>,
    pub ok: Option<bool>,
    pub preview: Option<String>,
    pub display: Option<serde_json::Value>,
}

/// 面向 UI 的运行事件(CLI/桌面端都消费这一层,不直接碰 LlmEvent)。
pub enum RunEvent {
    /// 一轮 provider 调用开始(UI 画轮次分隔)。
    TurnStart {
        step: u32,
        max_steps: u32,
    },
    Text(String),
    Reasoning(String),
    ToolStart {
        /// 工具调用 id:并行工具(task)结束顺序不定,UI 靠它配对 start/end。
        id: String,
        name: String,
        summary: String,
    },
    ToolEnd {
        id: String,
        name: String,
        ok: bool,
        preview: String,
        /// 结构化展示(diff/终端块),见 ToolOutput::display。
        display: Option<serde_json::Value>,
    },
    /// 子代理运行中的实时状态(轮次/正在用的工具),挂在对应 task 块上。
    TaskProgress {
        id: String,
        text: String,
        trace: Option<TaskTrace>,
    },
    /// 流建立前的临时网络错误重试,不会重放已建立流或工具副作用。
    Retry { attempt: u32, max: u32, delay_ms: u128 },
    StepEnd {
        usage: Usage,
        reason: FinishReason,
    },
}

pub struct RunSummary {
    pub text: String,
    pub usage: Usage,
    pub steps: u32,
    /// 用户拒绝权限导致的提前停止。
    pub halted_by_user: bool,
    /// 本次运行结束时的完整消息历史(含本次),调用方保存后可作为下次 prior 传入,
    /// 实现跨消息连续对话(M2 落盘前的内存态方案)。
    pub messages: Vec<Message>,
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
#[derive(Clone, Debug)]
pub enum AskRequest {
    Permission { action: String, resource: String },
    Question { question: String, options: Vec<String>, default: Option<String> },
}

#[derive(Clone, Debug)]
pub enum AskResponse {
    Permission(AskReply),
    Answer(String),
    Cancelled,
}

/// 交互询问回调的返回值:异步等待权限决定或用户答案。
pub type AskFuture = std::pin::Pin<Box<dyn std::future::Future<Output = AskResponse> + Send>>;

#[allow(clippy::too_many_arguments)]
pub fn run_once<'a>(
    client: &'a LlmClient,
    route: &'a Route,
    snapshot: &'a HarnessSnapshot,
    agent: &'a AgentDef,
    config: &'a RunnerConfig,
    ctx: &'a ToolCtx,
    prompt: &'a str,
    prior: &'a [Message],
    subagent: Option<&'a SubagentRuntime>,
    on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<RunSummary>> + Send + 'a>> {
    run_once_with_parts(client, route, snapshot, agent, config, ctx, prompt, prior, None, subagent, on_event, ask)
}

#[allow(clippy::too_many_arguments)]
pub fn run_once_with_parts<'a>(
    client: &'a LlmClient,
    route: &'a Route,
    snapshot: &'a HarnessSnapshot,
    agent: &'a AgentDef,
    config: &'a RunnerConfig,
    ctx: &'a ToolCtx,
    prompt: &'a str,
    // 之前轮次的完整消息历史(空 = 新对话)。
    prior: &'a [Message],
    initial_parts: Option<&'a [Part]>,
    // Some = 注册 task 工具,模型可派生并行子代理;子代理自身传 None(禁嵌套)。
    subagent: Option<&'a SubagentRuntime>,
    on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<RunSummary>> + Send + 'a>> {
    Box::pin(async move {
    let tools: Vec<Arc<dyn Tool>> = snapshot.materialize_tools();
    let mut specs: Vec<ToolSpec> = tools
        .iter()
        .map(|t| ToolSpec {
            name: t.name().to_string(),
            description: t.description(),
            input_schema: t.input_schema(),
        })
        .collect();
    if subagent.is_some() {
        specs.push(task_spec());
    }

    // system 分块:agent 提示词 + harness baseline(M2 起 baseline 进 Context Epoch)。
    let system: Vec<String> = [agent.system.clone(), snapshot.system_baseline()]
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();

    let mut messages: Vec<Message> = prior.to_vec();
    let user_parts = match initial_parts {
        Some(parts) => {
            let mut parts = parts.to_vec();
            if !prompt.is_empty() {
                parts.insert(0, Part::Text { text: prompt.to_string() });
            }
            parts
        }
        None => vec![Part::Text { text: prompt.to_string() }],
    };
    messages.push(Message { role: Role::User, parts: user_parts });
    let mut total_usage = Usage::default();
    let mut final_text = String::new();
    // steps 语义:0 = 无上限(用户定调:不设人为轮数天花板——停止权在用户按钮
    // 与上下文管理,不在计数器)。>0 时保留封顶,最后一步收工具+收尾指令。
    let max_steps = agent.steps;
    // 本次运行内已放行的 (action, resource):同一资源不重复问(用户反馈:别烦我)。
    let mut session_approved: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    // "总是允许"的会话内即时生效层(D-006):快照是开跑时定死的,新写入的规则
    // 本次运行读不到——泛化 pattern 记在这里,同类资源当场不再询问。
    let mut session_rules: Vec<(String, String)> = Vec::new();

    let mut overflow_recovered = false;

    let mut step = 0u32;
    loop {
        step += 1;
        on_event(RunEvent::TurnStart { step, max_steps });
        let last_step = max_steps > 0 && step == max_steps;
        // 最后一步收走工具强制收敛;必须同时明确告知(D-027:只收走不告知,
        // codex 仍试图调用工具,把调用 JSON 当纯文本狂喷并在思考里反复自我纠正)。
        let request_messages = if last_step {
            let mut wrapped = messages.clone();
            wrapped.push(Message::user_text(
                "(system) Final step of this run: tools are no longer available. Do NOT \
                 attempt any tool call and do NOT emit JSON — reply in plain text only, \
                 summarizing what was completed and what remains.",
            ));
            wrapped
        } else {
            messages.clone()
        };
        let request = LlmRequest {
            model: config.model.clone(),
            system: system.clone(),
            messages: request_messages,
            tools: if last_step { vec![] } else { specs.clone() },
            max_tokens: config.max_tokens,
            temperature: None,
            reasoning: config.reasoning,
        };

        // Provider 可能比本地配置更严格地计算上下文(尤其是工具 schema)。
        // 请求尚未建立时可以安全压缩本轮的旧工具轨迹并重试一次；重试上限
        // 是硬限制，避免超限错误造成死循环。流已经建立后不做重放，防止工具副作用重复执行。
        let mut stream = match client
            .stream_with_retry_notice(route, &request, |attempt, delay| {
                on_event(RunEvent::Retry { attempt, max: kanzei_llm::client::MAX_TRANSPORT_RETRIES, delay_ms: delay.as_millis() });
            })
            .await
        {
            Err(error) if error.is_context_overflow() && !overflow_recovered => {
                overflow_recovered = true;
                compact_messages_for_retry(&mut messages);
                let retry_request = LlmRequest {
                    messages: messages.clone(),
                    ..request
                };
                match client
                    .stream_with_retry_notice(route, &retry_request, |attempt, delay| {
                        on_event(RunEvent::Retry { attempt, max: kanzei_llm::client::MAX_TRANSPORT_RETRIES, delay_ms: delay.as_millis() });
                    })
                    .await
                {
                    Err(error) if error.is_context_overflow() => {
                        // 第一次压缩仍可能被超大的 system/tool schema 或当前输入
                        // 拒绝；第二次只保留当前用户消息，且不再继续重试。
                        compact_messages_aggressively(&mut messages);
                        let final_request = LlmRequest {
                            messages: messages.clone(),
                            ..retry_request
                        };
                        client
                            .stream_with_retry_notice(route, &final_request, |attempt, delay| {
                                on_event(RunEvent::Retry { attempt, max: kanzei_llm::client::MAX_TRANSPORT_RETRIES, delay_ms: delay.as_millis() });
                            })
                            .await?
                    }
                    result => result?,
                }
            }
            result => result?,
        };
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
                LlmEvent::ToolCall {
                    id,
                    name,
                    input,
                    raw_input,
                } => {
                    // 协议层解析失败 → 宽容修复(尾逗号/单引号/裸键/围栏)。
                    let input = if input.is_null() {
                        tolerant_parse(&raw_input).unwrap_or(serde_json::Value::Null)
                    } else {
                        input
                    };
                    parts.push(Part::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: if input.is_null() {
                            serde_json::json!({})
                        } else {
                            input.clone()
                        },
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
            return Ok(RunSummary {
                text: final_text,
                usage: total_usage,
                steps: step,
                halted_by_user: false,
                messages,
            });
        }

        // ---- task 子代理:同轮多个 task 并行执行。只读快照无副作用,与任何工具并发都安全 ----
        let mut task_results: std::collections::HashMap<String, kanzei_harness::ToolOutput> =
            std::collections::HashMap::new();
        if let Some(rt) = subagent {
            let mut task_calls: Vec<(String, serde_json::Value, String)> = calls
                .iter()
                .filter(|(_, name, _, _)| name == "task")
                .map(|(id, _, input, raw)| (id.clone(), input.clone(), raw.clone()))
                .collect();
            if !task_calls.is_empty() {
                let overflow = if task_calls.len() > MAX_TASKS_PER_TURN {
                    task_calls.split_off(MAX_TASKS_PER_TURN)
                } else {
                    Vec::new()
                };
                for (id, input, raw) in &task_calls {
                    on_event(RunEvent::ToolStart {
                        id: id.clone(),
                        name: "task".into(),
                        summary: summarize_input(input, raw),
                    });
                }
                for (id, input, raw) in &overflow {
                    on_event(RunEvent::ToolStart {
                        id: id.clone(),
                        name: "task".into(),
                        summary: summarize_input(input, raw),
                    });
                    let output = kanzei_harness::ToolOutput::error(format!(
                        "too many parallel subagent tasks; maximum per turn is {}",
                        MAX_TASKS_PER_TURN
                    ));
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name: "task".into(),
                        ok: false,
                        preview: preview(&output.content),
                        display: output.display.clone(),
                    });
                    task_results.insert(id.clone(), output);
                }
                // 进度通道:子代理内部事件(轮次/工具)转成 TaskProgress 实时上抛,
                // 完成一个立刻报一个 ToolEnd——不再等最慢的,UI 全程有反馈。
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RunEvent>();
                let mut jobs: futures::stream::FuturesUnordered<_> = task_calls
                    .iter()
                    .map(|(id, input, _)| {
                        let tx = tx.clone();
                        async move {
                            let bound = std::time::Duration::from_secs(rt.timeout_secs);
                            let output = match tokio::time::timeout(
                                bound,
                                run_subagent(client, rt, ctx, id, input, tx),
                            )
                            .await
                            {
                                Ok(output) => output,
                                // 纯兜底(默认 15 分钟):防失控,不是性能预算。
                                Err(_) => kanzei_harness::ToolOutput::error(format!(
                                    "subagent hit the {}s wall-clock safety limit — split the task into narrower pieces",
                                    rt.timeout_secs
                                )),
                            };
                            (id.clone(), output)
                        }
                    })
                    .collect();
                drop(tx);
                loop {
                    tokio::select! {
                        next = jobs.next() => match next {
                            Some((id, output)) => {
                                on_event(RunEvent::ToolEnd {
                                    id: id.clone(),
                                    name: "task".into(),
                                    ok: !output.is_error,
                                    preview: preview(&output.content),
                                    display: output.display.clone(),
                                });
                                task_results.insert(id, output);
                            }
                            None => break,
                        },
                        Some(event) = rx.recv() => on_event(event),
                    }
                }
            }
        }

        let mut results = Vec::new();
        for (id, name, input, raw_input) in calls {
            // task 不过权限门禁:子代理快照在代码层面只含只读工具(硬门禁在构造,不在评估)。
            // ToolEnd 已在并行阶段按完成顺序上报过,这里只归位结果。
            if name == "task" && subagent.is_some() {
                let output = task_results.remove(&id).unwrap_or_else(|| {
                    kanzei_harness::ToolOutput::error("internal: task result missing")
                });
                results.push(Part::ToolResult {
                    call_id: id,
                    content: output.content,
                    is_error: output.is_error,
                });
                continue;
            }
            let Some(tool) = tools.iter().find(|t| t.name() == name) else {
                results.push(Part::ToolResult {
                    call_id: id,
                    content: format!(
                        "unknown tool `{name}`; available: {}",
                        tools
                            .iter()
                            .map(|t| t.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    is_error: true,
                });
                continue;
            };
            on_event(RunEvent::ToolStart {
                id: id.clone(),
                name: name.clone(),
                summary: summarize_input(&input, &raw_input),
            });

            // question 是交互工具，不再叠加权限询问；答案作为工具结果回喂模型。
            if name == "question" {
                let question = input.get("question").and_then(|v| v.as_str()).unwrap_or("").trim();
                let options = input.get("options").and_then(|v| v.as_array()).map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
                let default = input.get("default").and_then(|v| v.as_str()).map(str::to_owned);
                let output = if question.is_empty() {
                    kanzei_harness::ToolOutput::error("question must not be empty")
                } else {
                    match ask(AskRequest::Question { question: question.to_owned(), options, default }).await {
                        AskResponse::Answer(answer) => kanzei_harness::ToolOutput::ok(format!("User answer: {answer}")),
                        AskResponse::Cancelled => kanzei_harness::ToolOutput::error("question cancelled by user"),
                        AskResponse::Permission(_) => kanzei_harness::ToolOutput::error("invalid question response"),
                    }
                };
                on_event(RunEvent::ToolEnd { id: id.clone(), name: name.clone(), ok: !output.is_error, preview: preview(&output.content), display: output.display.clone() });
                results.push(Part::ToolResult { call_id: id, content: output.content, is_error: output.is_error });
                continue;
            }

            // ---- 硬门禁:权限 Ruleset(deny 回喂模型;ask 问用户,拒绝停整轮)----
            let action = tool.action();
            let mut gate_result = Gate::Pass;
            let mut pending_ask: Vec<String> = Vec::new();
            for resource in tool.resources(&input) {
                // 统一正斜杠 + 消解 . / ..,权限 pattern 不用关心平台,也不能被路径变体绕过:
                // `.kanzei/research/../../src/main.rs` 会被 `*.kanzei/research/*` 判为放行,
                // 而落盘时 join 会消解 ..,实际写到项目任意位置(D-050)。
                let normalized =
                    kanzei_harness::permission::normalize_resource(&resource);
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
                        a == action
                            && kanzei_harness::permission::wildcard_match(pattern, &resource)
                    }) {
                        continue;
                    }
                    match ask(AskRequest::Permission { action: action.to_string(), resource: resource.clone() }).await {
                        AskResponse::Permission(AskReply::Deny) | AskResponse::Cancelled | AskResponse::Answer(_) => {
                            gate_result = Gate::UserDeclined;
                            break;
                        }
                        AskResponse::Permission(AskReply::AllowOnce) => {
                            session_approved.insert(key);
                        }
                        AskResponse::Permission(AskReply::AlwaysAllow) => {
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
                        id: id.clone(),
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
                        messages,
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
                id: id.clone(),
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
            return Ok(RunSummary {
                text: final_text,
                usage: total_usage,
                steps: step,
                halted_by_user: false,
                messages,
            });
        }
        if last_step {
            break;
        }
    }

    Ok(RunSummary {
        text: final_text,
        usage: total_usage,
        steps: step,
        halted_by_user: false,
        messages,
    })
    })
}

enum Gate {
    Pass,
    Deny(String),
    UserDeclined,
}

/// 跑一个子代理:独立的只读快照 + 空历史,结果文本即 tool result。
/// 子代理内 ask 一律 Deny(无人应答);run_once 递归经 dyn Box 断开无限类型。
/// 内部轮次/工具事件折叠成 TaskProgress 经 progress 通道上抛(UI 实时可见)。
async fn run_subagent(
    client: &LlmClient,
    rt: &SubagentRuntime,
    ctx: &ToolCtx,
    parent_call_id: &str,
    input: &serde_json::Value,
    progress: tokio::sync::mpsc::UnboundedSender<RunEvent>,
) -> kanzei_harness::ToolOutput {
    let prompt = ["prompt", "task", "instruction", "query"]
        .iter()
        .find_map(|k| input.get(k).and_then(|v| v.as_str()))
        .unwrap_or("")
        .trim()
        .to_string();
    if prompt.is_empty() {
        return kanzei_harness::ToolOutput::error(
            "task requires a `prompt` string: a self-contained exploration instruction",
        );
    }
    let (route, model) = match input.get("model").and_then(|v| v.as_str()) {
        Some("primary") => (&rt.primary.0, &rt.primary.1),
        _ => (&rt.fast.0, &rt.fast.1),
    };
    let config = RunnerConfig {
        model: model.clone(),
        max_tokens: rt.max_tokens,
        // 子代理是机械检索,不开思考:省钱且避免本地小模型不认该参数。
        reasoning: ReasoningEffort::Off,
    };
    let mut on_event = |event: RunEvent| {
        let text = match &event {
            RunEvent::TurnStart { step, max_steps } => Some(if *max_steps > 0 {
                format!("第 {step}/{max_steps} 轮")
            } else {
                format!("第 {step} 轮")
            }),
            RunEvent::ToolStart { name, summary, .. } => {
                let head: String = summary.chars().take(80).collect();
                Some(format!("{name} {head}"))
            }
            _ => None,
        };
        let trace = match event {
            RunEvent::ToolStart { id, name, summary } => Some(TaskTrace {
                child_id: id,
                phase: "start".into(),
                name,
                summary: Some(summary),
                ok: None,
                preview: None,
                display: None,
            }),
            RunEvent::ToolEnd { id, name, ok, preview, display } => Some(TaskTrace {
                child_id: id,
                phase: "end".into(),
                name,
                summary: None,
                ok: Some(ok),
                preview: Some(preview),
                display,
            }),
            _ => None,
        };
        if let Some(text) = text {
            let _ = progress.send(RunEvent::TaskProgress {
                id: parent_call_id.to_string(),
                text,
                trace: trace.clone(),
            });
        } else if trace.is_some() {
            let _ = progress.send(RunEvent::TaskProgress {
                id: parent_call_id.to_string(),
                text: "子代理工具完成".into(),
                trace,
            });
        }
    };    let mut ask = |_request: AskRequest| -> AskFuture {
        Box::pin(async { AskResponse::Permission(AskReply::Deny) })
    };
    // run_once 本身返回 boxed future,递归的无限类型在其签名处已断开。
    let fut = run_once(
        client,
        route,
        &rt.snapshot,
        &rt.agent,
        &config,
        ctx,
        &prompt,
        &[],
        None,
        &mut on_event,
        &mut ask,
    );
    match fut.await {
        Ok(summary) => {
            let text = if summary.text.trim().is_empty() {
                "(subagent finished without a text answer)".to_string()
            } else {
                summary.text
            };
            kanzei_harness::ToolOutput::ok(text)
        }
        Err(e) => kanzei_harness::ToolOutput::error(format!("subagent failed: {e}")),
    }
}

fn compact_messages_for_retry(messages: &mut Vec<Message>) {
    let Some(current_index) = messages.iter().rposition(|message| message.role == Role::User) else {
        return;
    };
    let current = messages[current_index].clone();
    let mut history = String::new();
    for message in messages.iter().take(current_index) {
        for part in &message.parts {
            let text = match part {
                Part::Text { text } => text,
                Part::ToolResult { content, .. } => content,
                _ => continue,
            };
            if history.len() >= 8_000 {
                break;
            }
            let remaining = 8_000 - history.len();
            let snippet: String = text.chars().take(remaining).collect();
            history.push_str(&snippet);
            history.push('\n');
        }
    }
    messages.clear();
    if !history.trim().is_empty() {
        messages.push(Message::user_text(format!(
            "以下是此前工具执行结果的压缩记录，仅供继续当前任务参考：\n{}",
            history.trim_end()
        )));
    }
    messages.push(current);
}

fn compact_messages_aggressively(messages: &mut Vec<Message>) {
    if let Some(current) = messages
        .iter()
        .rfind(|message| message.role == Role::User)
        .cloned()
    {
        messages.clear();
        messages.push(current);
    }
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
    let rendered = if input.is_null() {
        raw.to_string()
    } else {
        input.to_string()
    };
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

#[cfg(test)]
mod tests {
    use super::compact_messages_for_retry;
    use kanzei_llm::{Message, Part};

    #[test]
    fn compact_retry_keeps_prompt_and_bounded_tool_history() {
        let mut messages = vec![
            Message::user_text("原始任务"),
            Message::assistant(vec![Part::Text {
                text: "旧回复".into(),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_1".into(),
                content: "工具结果".into(),
                is_error: false,
            }]),
            Message::user_text("当前任务"),
        ];

        compact_messages_for_retry(&mut messages);

        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].parts[0], Part::Text { ref text } if text.contains("工具结果")));
        assert!(matches!(messages[1].parts[0], Part::Text { ref text } if text == "当前任务"));
    }
}
