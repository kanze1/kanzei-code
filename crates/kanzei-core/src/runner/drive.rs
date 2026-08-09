//! 驱动域(R-155 B8):run_once / run_once_with_parts 整体搬迁,不动内部。
//! 设计 §C B8:本批只搬主体,任何抽函数动作都不在本条目做;
//! calls[i]↔results[i] 下标对齐不变式跨 tool_exec/redundancy/drive 三文件。
//! 设计 §C 要点 1:run_once 是动态分发边界——驱动、subagent、tool_exec 的所有
//! 异步递归都经它(子代理递归经 dyn Box 断开无限类型,见 subagent.rs)。
//! 签名即锁:改动必须同步 mod.rs 的导出与全部调用方。

// 所有符号经 super::* 平铺(mod.rs 的 use 与 pub use 子模块)。
use super::*;

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
    let (baseline, mut context_report) = snapshot.system_baseline_with_report();
    if !agent.system.trim().is_empty() {
        context_report.insert(0, ("agent/system".into(), agent.system.chars().count()));
    }
    // 工具 schema 是每轮上下文里最大的一块之一(桌面 dev 档 26 个工具的完整 JSON
    // Schema),estimate_prompt_tokens 也把它算进 prompt。账单要回答"本轮上下文里
    // 有什么、各占多少",漏掉它等于漏掉最大的那一项(R-106)。
    let spec_chars: usize = specs
        .iter()
        .map(|spec| {
            spec.name.chars().count()
                + spec.description.chars().count()
                + spec.input_schema.to_string().chars().count()
        })
        .sum();
    if spec_chars > 0 {
        context_report.push(("tools/schema".into(), spec_chars));
    }
    let system: Vec<String> = [agent.system.clone(), baseline]
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();

    // prior 可能来自旧快照或跨进程恢复，先统一清洗孤儿工具 part，避免首次请求
    // 在尚未触发上下文压缩时就把非法消息交给 provider。
    let mut messages: Vec<Message> = crate::history::filter_message_history(prior);
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

    let mut overflow_recoveries = 0;
    // 主动压缩的连续无效计数(D-206),与被动恢复各记各的。只数"压了没用",
    // 成功的压缩清零——压缩是常规运营动作,不设总量配额。
    let mut futile_compactions = 0u32;
    let mut overflow_traces: Vec<String> = Vec::new();
    // 估算校准:len/4 粗估对中文 \uXXXX 转义、工具输出密集的会话有系统性偏差,
    // 预算线 0.7 的语义要靠真实 usage 反推的滑动因子校准才有意义。初始 1.0,
    // 每步拿到 provider 真实 input tokens 后 EMA 更新。
    let mut calibration = 1.0f64;
    let mut last_estimated = 0u64;
    // R-100 冗余机械门禁:按单次运行持有(跨轮清零),提醒追加进工具结果不阻断。
    let mut redundancy = RedundancyWatch::default();

    let mut step = 0u32;
    loop {
        step += 1;
        on_event(RunEvent::TurnStart { step, max_steps });
        let last_step = max_steps > 0 && step == max_steps;
        // 步数软预算(D-173):步数上限是 0(不设人为天花板),但"不封顶"不等于
        // "不盘点"。检查点只要求当轮盘一次剩余范围,不强制中止——实测长轮的
        // 成本失控几乎都始于无人察觉的目标漂移。
        let budget_checkpoint = is_budget_checkpoint(step);

        // 轮内上下文预算(D-176)。压缩检查原先只写在**一轮结束之后**,而长轮与
        // 自动续跑恰恰是最需要它的场景:一轮不结束就一次也轮不到。实测一次 41
        // 分钟的运行里检查点执行了 0 次,用户按停止后更是直接跳过收尾,全程只能
        // 等 provider 报 overflow 再被动裁剪。这里在每步开跑前主动估一次。
        if let Some(limit) = config.context_limit {
            let budget = (limit as f64 * config.limits.context_budget_ratio()) as u64;
            let before = budgeted_tokens(&system, &messages, &specs, calibration);
            if before > budget
                && futile_compactions < MAX_FUTILE_COMPACTIONS
                && messages.len() > 1
            {
                let dropped_messages = compact_with_digest(
                    client,
                    subagent,
                    &mut messages,
                    budget,
                    &mut overflow_traces,
                    config.limits.recent_verbatim_ratio(),
                )
                .await;
                if dropped_messages > 0 {
                    // 压了还超线:tail 太大或 head 太大。再砍 tail 到预算内,否则
                    // 下一步预算检查立刻再压——连续两次压缩 = 缓存前缀两次全量
                    // 重算(cache_write 双倍),省下的 token 不够补缓存成本。
                    // trim_tail 拿同一个 calibration:两边必须用同一把尺子量同一条
                    // 预算线,否则它按原始口径够线就收手,这里看还超线(D-203)。
                    if budgeted_tokens(&system, &messages, &specs, calibration) > budget {
                        trim_tail(
                            &mut messages,
                            &system,
                            &specs,
                            budget,
                            calibration,
                            &mut overflow_traces,
                        );
                    }
                    let after = budgeted_tokens(&system, &messages, &specs, calibration);
                    // D-206:只按"有没有用"记账。压回线内 = 压缩在正常工作,清零、
                    // 下次照压;压完(连 trim_tail 都上了)仍超线 = head+当前消息
                    // 本身超线,连续两次就停,交给撞墙后的被动恢复,别空转。
                    if after <= budget {
                        futile_compactions = 0;
                    } else {
                        futile_compactions += 1;
                    }
                    on_event(RunEvent::ContextCompacted {
                        before_tokens: before,
                        after_tokens: after,
                        budget_tokens: budget,
                        limit_tokens: limit,
                        dropped_messages,
                    });
                } else {
                    // 中段为空压不动:不发事件(没骗 UI),但要计无效——否则每步
                    // 白跑一次 compact,同样是注释里说的空转。
                    futile_compactions += 1;
                }
            }
        }

        // Provider 可能比本地配置更严格地计算上下文(尤其是工具 schema)。
        // 建流前和 HTTP 200 后 SSE 流内都可能报告 context overflow，必须走同一套
        // 有界恢复；本步工具要等流完整结束才执行，因此流内超限重放不会重复副作用。
        let mut stream_restarts: u32 = 0;
        let (parts, calls, finish) = loop {
        // 每次恢复都会改写 messages，请求必须在重试循环内重建；否则即使压缩了
        // 内存历史，发给 provider 的仍会是第一次克隆出的超长请求。
        let mut request_messages = messages.clone();
        // 最后一步收走工具强制收敛;必须同时明确告知(D-027:只收走不告知,
        // codex 仍试图调用工具,把调用 JSON 当纯文本狂喷并在思考里反复自我纠正)。
        if last_step {
            request_messages.push(Message::user_text(
                "(system) Final step of this run: tools are no longer available. Do NOT \
                 attempt any tool call and do NOT emit JSON — reply in plain text only, \
                 summarizing what was completed and what remains.",
            ));
        } else if budget_checkpoint {
            request_messages.push(Message::user_text(format!(
                "(system) Budget checkpoint — you are {step} steps into this run. This is not a \
                 stop signal and not a nudge to hurry: keep going. Before your next tool call, \
                 state in one or two lines what is DONE, what REMAINS, and whether the remaining \
                 work still belongs to the task you were given. If it has drifted into unrelated \
                 work, finish the original task first. If what remains needs a decision only the \
                 user can make, say so now in plain text instead of exploring further."
            )));
        }
        // 请求构造前记录本次估算:StepFinish 用真实 input tokens 反推校准因子,
        // 下一次预算判断就按校准后的口径来。tools 随 last_step 变化,估算必须
        // 与实际发出的请求同口径,否则校准因子被系统性偏差污染。
        let req_tools: &[ToolSpec] = if last_step { &[] } else { &specs };
        last_estimated = estimate_prompt_tokens(&system, &request_messages, req_tools);
        let request = LlmRequest {
            model: config.model.clone(),
            system: system.clone(),
            messages: request_messages,
            tools: req_tools.to_vec(),
            max_tokens: config.max_tokens,
            temperature: None,
            reasoning: config.reasoning,
            service_tier: config.service_tier.clone(),
        };
        let mut stream = match client
            .stream_with_retry_notice(route, &request, |attempt, delay| {
                on_event(RunEvent::Retry { attempt, max: kanzei_llm::client::MAX_TRANSPORT_RETRIES, delay_ms: delay.as_millis() });
            })
            .await
        {
            Err(error) if error.is_context_overflow() => {
                if recover_context_overflow(&mut messages, &mut overflow_recoveries, &mut overflow_traces) {
                    continue;
                }
                return Err(error.into());
            }
            result => result?,
        };
        let mut text_buffers: BTreeMap<usize, String> = BTreeMap::new();
        let mut reasoning_buffers: BTreeMap<usize, String> = BTreeMap::new();
        let mut parts: Vec<Part> = Vec::new();
        let mut calls: Vec<(String, String, serde_json::Value, String)> = Vec::new();
        let mut finish = FinishReason::EndTurn;
        let mut stream_error: Option<kanzei_llm::LlmError> = None;

        while let Some(event) = stream.next().await {
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    stream_error = Some(error);
                    break;
                }
            };
            match event {
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
                    calibration = update_calibration(calibration, last_estimated, usage.input);
                    total_usage = add_usage(total_usage, usage);
                    finish = reason.clone();
                    on_event(RunEvent::StepEnd { usage, reason });
                }
                _ => {}
            }
        }

        match stream_error {
            None => {
                for (_, text) in std::mem::take(&mut text_buffers) {
                    parts.push(Part::Text { text });
                }
                break (parts, calls, finish);
            }
            // Provider 也可能在 HTTP 200 的 SSE error 事件里报告上下文超限。
            // 此时本步工具尚未执行，压缩 messages 后安全地从头重放请求。
            Some(error) if error.is_context_overflow() => {
                if recover_context_overflow(&mut messages, &mut overflow_recoveries, &mut overflow_traces) {
                    continue;
                }
                return Err(error.into());
            }
            // 只重放传输层中断:协议错误重放只会原样复现,白烧钱。
            Some(error)
                if matches!(error, kanzei_llm::LlmError::Transport(_))
                    && stream_restarts < config.limits.stream_restarts() =>
            {
                stream_restarts += 1;
                let delay = std::time::Duration::from_millis(500 * stream_restarts as u64);
                tracing::warn!(
                    attempt = stream_restarts,
                    delay_ms = delay.as_millis(),
                    error = %error,
                    "stream broke mid-flight, re-requesting step"
                );
                on_event(RunEvent::StreamRestart {
                    attempt: stream_restarts,
                    max: config.limits.stream_restarts(),
                    delay_ms: delay.as_millis(),
                });
                tokio::time::sleep(delay).await;
            }
            Some(error) => return Err(error.into()),
        }
        };

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
                context_report: context_report.clone(),
                overflow_traces: overflow_traces.clone(),
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
                let max_tasks = config.limits.max_tasks_per_turn();
                let overflow = if task_calls.len() > max_tasks {
                    task_calls.split_off(max_tasks)
                } else {
                    Vec::new()
                };
                for (id, input, raw) in &task_calls {
                    on_event(RunEvent::ToolStart {
                        id: id.clone(),
                        name: "task".into(),
                        summary: summarize_input(input, raw),
                        input: input.clone(),
                    });
                }
                for (id, input, raw) in &overflow {
                    on_event(RunEvent::ToolStart {
                        id: id.clone(),
                        name: "task".into(),
                        summary: summarize_input(input, raw),
                        input: input.clone(),
                    });
                    let output = kanzei_harness::ToolOutput::error(format!(
                        "too many parallel subagent tasks; maximum per turn is {}",
                        max_tasks
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
                            None => {
                                drain_task_events(&mut rx, on_event);
                                break;
                            }
                        },
                        Some(event) = rx.recv() => on_event(event),
                    }
                }
            }
        }

        // R-097 批一：权限询问仍按旧路径串行处理(R-086 承接询问路由)；当本批
        // 不需要新 ask 时，普通工具按显式并发契约切成确定性 wave 并发执行。
        let can_parallel_tools = {
            let mut ready = true;
            let mut ordinary_count = 0usize;
            for (_, name, input, _) in &calls {
                if name == "task" && subagent.is_some() {
                    continue;
                }
                let Some(tool) = tools.iter().find(|tool| tool.name() == name) else {
                    ready = false;
                    break;
                };
                if name == "question" || input.is_null() {
                    ready = false;
                    break;
                }
                ordinary_count += 1;
                let action = tool.action();
                for resource in tool.resources_with_ctx(input, ctx) {
                    let resource = kanzei_harness::permission::normalize_resource(&resource);
                    if snapshot.evaluate(action, &resource) != Effect::Ask {
                        continue;
                    }
                    let key = (action.to_string(), resource.clone());
                    let approved = session_approved.contains(&key)
                        || session_rules.iter().any(|(known_action, pattern)| {
                            known_action == action
                                && kanzei_harness::permission::resource_match_for_action(
                                    known_action,
                                    pattern,
                                    &resource,
                                )
                        });
                    if !approved {
                        ready = false;
                        break;
                    }
                }
                if !ready {
                    break;
                }
            }
            ready && ordinary_count >= 2
        };

        // 并行 wave:results[i] 与 calls[i] 按下标对齐(R-155 设计要点 3),
        // 与串行路径共用同一对齐约定,note_step 里的 debug_assert 兜底锁住。
        let mut results = if can_parallel_tools {
            let mut slots: Vec<Option<Part>> =
                std::iter::repeat_with(|| None).take(calls.len()).collect();
            let mut prepared = Vec::new();
            for (index, (id, name, input, raw_input)) in calls.iter().cloned().enumerate() {
                if name == "task" && subagent.is_some() {
                    let output = task_results.remove(&id).unwrap_or_else(|| {
                        kanzei_harness::ToolOutput::error("internal: task result missing")
                    });
                    slots[index] = Some(Part::ToolResult {
                        call_id: id,
                        content: output.content,
                        is_error: output.is_error,
                    });
                    continue;
                }
                let tool = tools
                    .iter()
                    .find(|tool| tool.name() == name)
                    .expect("parallel batch was preflighted")
                    .clone();
                on_event(RunEvent::ToolStart {
                    id: id.clone(),
                    name: name.clone(),
                    summary: summarize_input(&input, &raw_input),
                    input: input.clone(),
                });
                let action = tool.action();
                let denied = tool
                    .resources_with_ctx(&input, ctx)
                    .into_iter()
                    .map(|resource| kanzei_harness::permission::normalize_resource(&resource))
                    .find(|resource| snapshot.evaluate(action, resource) == Effect::Deny);
                if let Some(resource) = denied {
                    on_event(RunEvent::PermissionResolved {
                        tool_call_id: id.clone(),
                        action: action.to_string(),
                        resource: resource.clone(),
                        decision: "deny",
                        source: "ruleset",
                    });
                    let output = kanzei_harness::ToolOutput::error(format!(
                        "permission denied by ruleset: {action} on `{resource}`.\n{}",
                        snapshot.denial_hint(action, &resource),
                    ));
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name,
                        ok: false,
                        preview: preview(&output.content),
                        display: None,
                    });
                    slots[index] = Some(Part::ToolResult {
                        call_id: id,
                        content: output.content,
                        is_error: true,
                    });
                    continue;
                }
                let concurrency = tool.concurrency(&input, ctx);
                prepared.push(PreparedToolCall {
                    index,
                    id,
                    name,
                    input,
                    tool,
                    concurrency,
                });
            }
            for (index, result) in execute_prepared_tools(prepared, ctx, config.limits.max_parallel_tools(), on_event).await {
                slots[index] = Some(result);
            }
            slots
                .into_iter()
                .map(|result| result.expect("every preflighted tool call must produce a result"))
                .collect()
        } else {
            let mut results = Vec::new();
        // 串行路径:按 calls 的原始顺序逐个执行并 push,results 与 calls 下标对齐
        // (R-155 设计要点 3)。calls.len() == results.len() 由 note_step 的 debug_assert 兜底。
        for (call_index, (id, name, input, raw_input)) in calls.iter().cloned().enumerate() {
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
                input: input.clone(),
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
            for resource in tool.resources_with_ctx(&input, ctx) {
                // 统一正斜杠 + 消解 . / ..,权限 pattern 不用关心平台,也不能被路径变体绕过:
                // `.kanzei/research/../../src/main.rs` 会被 `*.kanzei/research/*` 判为放行,
                // 而落盘时 join 会消解 ..,实际写到项目任意位置(D-050)。
                let normalized =
                    kanzei_harness::permission::normalize_resource(&resource);
                let mut resolved = |decision, source| {
                    on_event(RunEvent::PermissionResolved {
                        tool_call_id: id.clone(),
                        action: action.to_string(),
                        resource: normalized.clone(),
                        decision,
                        source,
                    });
                };
                match snapshot.evaluate(action, &normalized) {
                    Effect::Deny => {
                        resolved("deny", "ruleset");
                        gate_result = Gate::Deny(normalized);
                        break;
                    }
                    Effect::Ask => pending_ask.push(normalized),
                    Effect::Allow => {
                        resolved("allow", "ruleset");
                    }
                }
            }
            if matches!(gate_result, Gate::Pass) {
                for resource in pending_ask {
                    let key = (action.to_string(), resource.clone());
                    let mut resolved = |decision, source| {
                        on_event(RunEvent::PermissionResolved {
                            tool_call_id: id.clone(),
                            action: action.to_string(),
                            resource: resource.clone(),
                            decision,
                            source,
                        });
                    };
                    if session_approved.contains(&key) {
                        resolved("allow", "session_approved");
                        continue;
                    }
                    if session_rules.iter().any(|(a, pattern)| {
                        a == action
                            && kanzei_harness::permission::resource_match_for_action(a, pattern, &resource)
                    }) {
                        resolved("allow", "session_rule");
                        continue;
                    }
                    match ask(AskRequest::Permission { action: action.to_string(), resource: resource.clone() }).await {
                        AskResponse::Permission(AskReply::Deny) | AskResponse::Cancelled | AskResponse::Answer(_) => {
                            resolved("declined", "user");
                            gate_result = Gate::UserDeclined;
                            break;
                        }
                        AskResponse::Permission(AskReply::AllowOnce) => {
                            resolved("allow_once", "user");
                            session_approved.insert(key);
                        }
                        AskResponse::Permission(AskReply::AlwaysAllow) => {
                            resolved("always_allow", "user");
                            session_rules.push((
                                action.to_string(),
                                kanzei_harness::config::generalize_resource(action, &resource),
                            ));
                        }
                    }
                }
            }
            let output = match gate_result {
                // D-173:拒绝理由必须由实际注册的托管族推导,不能固定说
                // "use the dedicated tool"——那个工具可能根本不存在。
                Gate::Deny(resource) => kanzei_harness::ToolOutput::error(format!(
                    "permission denied by ruleset: {action} on `{resource}`.\n{}",
                    snapshot.denial_hint(action, &resource),
                )),
                Gate::UserDeclined => {
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name: name.clone(),
                        ok: false,
                        preview: "(user declined)".into(),
                        display: None,
                    });
                    append_declined_tool_results(&mut results, &calls, call_index);
                    messages.push(Message::tool_results(results));
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        steps: step,
                        halted_by_user: true,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
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
            results
        };
        // R-100:工具结果回喂前就地注入冗余提醒(不阻断)。
        // results 与 calls 按下标对齐(并行 wave 与串行路径同上),见 redundancy::note_step。
        redundancy.note_step(&ctx.project_root, &calls, &mut results);
        messages.push(Message::tool_results(results));

        if matches!(finish, FinishReason::MaxTokens | FinishReason::Refusal) {
            return Ok(RunSummary {
                text: final_text,
                usage: total_usage,
                steps: step,
                halted_by_user: false,
                messages,
                context_report: context_report.clone(),
                overflow_traces: overflow_traces.clone(),
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
        context_report,
        overflow_traces: overflow_traces.clone(),
    })
    })
}




