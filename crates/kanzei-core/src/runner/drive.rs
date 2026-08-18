//! 驱动域(R-155 B8):run_once / run_once_with_parts 整体搬迁,不动内部。
//! 设计 §C B8:本批只搬主体,任何抽函数动作都不在本条目做;
//! calls[i]↔results[i] 下标对齐不变式跨 tool_exec/redundancy/drive 三文件。
//! 设计 §C 要点 1:run_once 是动态分发边界——驱动、subagent、tool_exec 的所有
//! 异步递归都经它(子代理递归经 dyn Box 断开无限类型,见 subagent.rs)。
//! 签名即锁:改动必须同步 mod.rs 的导出与全部调用方。

// 所有符号经 super::* 平铺(mod.rs 的 use 与 pub use 子模块)。
use super::*;

mod question;
use question::execute_question;

/// R-183:命中规则的展示原文,用于 PermissionResolved.rule 轨迹(验收④)。
fn describe_rule(rule: &kanzei_harness::permission::Rule) -> String {
    format!("{} `{}` => {:?}", rule.action, rule.resource, rule.effect)
}

/// D-342:停止信号等待器。halt 未配置时永不就绪——select 里这个分支等价于不存在,
/// CLI(无停止通道)与引入前逐字节同行为。
async fn halt_signalled(halt: Option<&CancellationToken>) {
    match halt {
        Some(token) => token.cancelled().await,
        None => std::future::pending().await,
    }
}

/// D-342:停止后未执行的工具调用统一以「取消占位」配对,与权限拒绝占位同款形态,
/// 保证 calls[i]↔results[i] 对齐、历史里没有孤儿 ToolCall。
fn append_halted_tool_results(
    results: &mut Vec<Part>,
    calls: &[(String, String, serde_json::Value, String)],
    from_index: usize,
) {
    for (id, _, _, _) in calls.iter().skip(from_index) {
        results.push(Part::ToolResult {
            call_id: id.clone(),
            content: "cancelled: run stopped by user before this tool executed".into(),
            is_error: true,
        });
    }
}

fn commit_assistant_message(
    messages: &mut Vec<Message>,
    parts: Vec<Part>,
    step: u32,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) {
    let message = Message::assistant(parts);
    on_event(RunEvent::AssistantMessageCommitted {
        step,
        message: message.clone(),
    });
    messages.push(message);
}

/// R-249:`images` 追加在**所有** ToolResult 之后。
///
/// Anthropic 要求 tool_result 块位于 user 消息最前,图片前插会 400;而 results
/// 内部的 `results[i] ↔ calls[i]` 对齐由 note_step 的 debug_assert 锁着,也不允许
/// 在中间插入。两条约束合起来,唯一合法位置就是尾部。
fn commit_tool_results(
    messages: &mut Vec<Message>,
    results: Vec<Part>,
    images: Vec<Part>,
    step: u32,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) {
    let mut results = results;
    results.extend(images);
    let message = Message::tool_results(results);
    on_event(RunEvent::ToolResultsCommitted {
        step,
        message: message.clone(),
    });
    messages.push(message);
}

#[allow(clippy::too_many_arguments)] // 公开驱动边界，收拢为参数对象会同时扰动所有递归调用方。
pub fn run_once<'a>(
    client: &'a LlmClient,
    route: &'a Route,
    snapshot: &'a HarnessSnapshot,
    agent: &'a AgentDef,
    config: &'a RunnerConfig,
    ctx: &'a ToolCtx,
    prompt: &'a str,
    // D-185:开跑预检索的记忆提示块(R-106)。只作本轮 system 一次性注入,不拼进
    // prompt 字符串——否则随 User message 进 messages → 落 conversations →
    // 下轮 prior 回灌,逐轮累积 N 个 hint 块。
    memory_hints: Option<&'a str>,
    prior: &'a [Message],
    subagent: Option<&'a SubagentRuntime>,
    // R-246:统一资源 owner(可选),见 run_once_with_parts。
    line_runtime: Option<&'a LineRuntime>,
    on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<RunSummary>> + Send + 'a>> {
    run_once_with_parts(
        client,
        route,
        snapshot,
        agent,
        config,
        ctx,
        prompt,
        memory_hints,
        // run_once 不经勘察流水线(它是无阶段的直跑入口)。
        None,
        prior,
        None,
        subagent,
        line_runtime,
        on_event,
        ask,
    )
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
    // D-185:同 run_once,只进本轮 system,不进 messages/历史。
    memory_hints: Option<&'a str>,
    // 勘察阶段简报。与 memory_hints 同待遇——只进本轮 system,不进 messages。
    // 原先它被拼进 prompt 字符串,于是随 User message 进 messages → 落
    // conversations → 下轮 prior 回灌:上一轮的勘察结论会出现在下一轮的上下文里,
    // 而流水线每轮都会重新勘察,那份旧简报**在任何情况下都不是最新可用信息**。
    // 实测代价:agent 得自己推理「这看起来是上个会话的残留」再决定忽略,分辨成本
    // 与 token 照付。
    scout_brief: Option<&'a str>,
    // 之前轮次的完整消息历史(空 = 新对话)。
    prior: &'a [Message],
    initial_parts: Option<&'a [Part]>,
    // Some = 注册 task 工具,模型可派生并行子代理;子代理自身传 None(禁嵌套)。
    subagent: Option<&'a SubagentRuntime>,
    // R-246:统一资源 owner(可选)。Some 时后台子代理 spawn 的 JoinHandle 登记
    // 进 LineRuntime,dispose 时 cancel 后 await 全部退出;None(测试/无)跳过。
    line_runtime: Option<&'a LineRuntime>,
    on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<RunSummary>> + Send + 'a>> {
    Box::pin(async move {
        // R-202 批6:装配段(工具/specs/system 分块/消息初始化/各类运行态)抽离为
        // assemble_run_once,返回 RunOnceAssembly;halt/halted 借用 config 留本地。
        let RunOnceAssembly {
            tools,
            specs,
            context_report,
            stable_system,
            mut refreshable_baseline,
            mut messages,
            mut total_usage,
            mut last_input_tokens,
            mut final_text,
            max_steps,
            mut session_approved,
            mut session_rules,
            mut overflow_recoveries,
            mut futile_compactions,
            mut overflow_traces,
            mut calibration,
            mut redundancy,
            mut recall,
            mut step,
        } = assemble_run_once(
            snapshot,
            agent,
            config,
            prompt,
            memory_hints,
            scout_brief,
            prior,
            initial_parts,
            subagent,
        );
        let halt = config.halt.as_ref();
        let halted = || halt.is_some_and(|token| token.is_cancelled());
        loop {
            step += 1;
            // D-342 步首检查点:停止已置位就不再发起新的 provider 请求,以 halted
            // 正常收尾——messages 完整交还,调用方照常走轮末写回。
            if halted() {
                return Ok(RunSummary {
                    text: final_text,
                    usage: total_usage,
                    last_input_tokens,
                    steps: step.saturating_sub(1),
                    halted_by_user: true,
                    messages,
                    context_report: context_report.clone(),
                    overflow_traces: overflow_traces.clone(),
                });
            }
            // R-184:只有显式标记的动态源在轮内刷新。它作为本步临时 system 段
            // 替换,不 push 进 messages,因此内容变化不会把旧协作块逐轮堆进历史。
            if step > 1 {
                refreshable_baseline = snapshot.refreshable_system_baseline_with_report().0;
            }
            let mut system = stable_system.clone();
            if !refreshable_baseline.trim().is_empty() {
                system.push(refreshable_baseline.clone());
            }
            on_event(RunEvent::TurnStart { step, max_steps });
            let last_step = max_steps > 0 && step == max_steps;
            // 步数预算(D-173):旧配置中的 0 已在装配段转换为有限默认上限。
            // 到达最后一步时收走工具并要求模型用文本收敛，防止工具/模型循环无限延长。
            let budget_checkpoint = is_budget_checkpoint(step);

            // R-202 批6:轮内上下文预算(主动 prune/压缩/trim,含 D-206 无效计数
            // 与 R-219 保守默认 32k)抽离为 enforce_context_budget。
            enforce_context_budget(
                client,
                subagent,
                config,
                &system,
                &specs,
                &mut messages,
                calibration,
                &mut futile_compactions,
                &mut overflow_traces,
                on_event,
            )
            .await;

            // Provider 可能比本地配置更严格地计算上下文(尤其是工具 schema)。
            // 建流前和 HTTP 200 后 SSE 流内都可能报告 context overflow，必须走同一套
            // 有界恢复；本步工具要等流完整结束才执行，因此流内超限重放不会重复副作用。
            // R-202 批3:单步请求段(请求重建 → 建流 → SSE 消费 → overflow/transport
            // 重放)抽离为 stream_request_step。可变运行态经 &mut 传入,步内累积的
            // calibration/usage/恢复计数在 Stopped 提前退出时同样已生效。
            let outcome = stream_request_step(
                client,
                route,
                config,
                &system,
                &specs,
                &mut messages,
                step,
                last_step,
                budget_checkpoint,
                halt,
                on_event,
                &mut calibration,
                &mut last_input_tokens,
                &mut total_usage,
                &mut overflow_recoveries,
                &mut overflow_traces,
            )
            .await?;
            let (parts, calls, finish) = match outcome {
                StepOutcome::Completed {
                    parts,
                    calls,
                    finish,
                } => (parts, calls, finish),
                StepOutcome::Stopped => {
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        last_input_tokens,
                        steps: step,
                        halted_by_user: true,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
                    });
                }
            };

            // R-202 批6:步骤消息提交与纯文本/停止收尾抽离为 commit_step_messages;
            // 提前返回(calls 空 / 停止占位)以 StepMessageOutcome::Return 表达。
            match commit_step_messages(
                parts,
                &calls,
                &mut final_text,
                &mut messages,
                step,
                halt,
                on_event,
            ) {
                StepMessageOutcome::Proceed => {}
                StepMessageOutcome::Return { halted_by_user } => {
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        last_input_tokens,
                        steps: step,
                        halted_by_user,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
                    });
                }
            }

            // R-202 批4:task 子代理段(并行/后台两种派发 + 溢出上限 + 取消占位补齐)
            // 抽离为 run_subagent_calls,返回本步 task 结果表。
            let mut task_results: std::collections::HashMap<String, kanzei_harness::ToolOutput> =
                run_subagent_calls(
                    subagent,
                    client,
                    ctx,
                    config,
                    &calls,
                    halt,
                    line_runtime,
                    on_event,
                )
                .await;

            // R-202 批5:普通工具执行段(并行预检 + wave/串行两条路径)抽离为
            // execute_tool_calls。提前退出(串行路径停止 / 权限用户拒绝)以
            // ToolRunOutcome::Stopped 表达,由调用方构造 halted RunSummary。
            let images_supported = route.supports_images();
            let tool_outcome = execute_tool_calls(
                config,
                ctx,
                snapshot,
                &tools,
                &calls,
                subagent,
                &mut task_results,
                images_supported,
                halt,
                on_event,
                ask,
                &mut session_approved,
                &mut session_rules,
                &mut messages,
                step,
            )
            .await?;
            let (mut results, pending_images) = match tool_outcome {
                ToolRunOutcome::Results {
                    results,
                    pending_images,
                } => (results, pending_images),
                ToolRunOutcome::Stopped => {
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        last_input_tokens,
                        steps: step,
                        halted_by_user: true,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
                    });
                }
            };
            // R-202 批6:步骤收尾(冗余/召回注入 + 工具结果落库 + 步末检查点 +
            // MaxTokens/Refusal 终止 + last_step 收敛)抽离为 finalize_step。
            match finalize_step(
                ctx,
                &calls,
                &mut results,
                pending_images,
                &mut messages,
                step,
                &finish,
                last_step,
                halt,
                &mut redundancy,
                &mut recall,
                on_event,
            ) {
                StepFinalOutcome::Continue => {}
                StepFinalOutcome::Break => break,
                StepFinalOutcome::Return { halted_by_user } => {
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        last_input_tokens,
                        steps: step,
                        halted_by_user,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
                    });
                }
            }
        }

        Ok(RunSummary {
            text: final_text,
            usage: total_usage,
            last_input_tokens,
            steps: step,
            halted_by_user: false,
            messages,
            context_report,
            overflow_traces: overflow_traces.clone(),
        })
    })
}

/// R-202 批3:单步请求段的产物。
enum StepOutcome {
    /// 本步正常结束:消息已提交前的 (parts, calls, finish) 三元组,由调用方决定
    /// 是提交 assistant 消息还是进入工具执行。
    Completed {
        parts: Vec<Part>,
        calls: Vec<(String, String, serde_json::Value, String)>,
        finish: FinishReason,
    },
    /// D-342:流内收到停止信号,本步半成品丢弃。调用方构造 halted RunSummary。
    Stopped,
}

/// R-202 批3:单步请求段——从当前 messages 重建请求、建流、消费 SSE 事件,
/// 并在 context overflow / transport 中断时按既有规则恢复重放。
///
/// 职责边界与既有行为逐字节对齐(行为零变更):
/// - 重试循环内每次恢复都重克隆 messages 重建请求(压缩必须落进发出内容);
/// - StepFinish 更新 calibration / last_input_tokens / total_usage / overflow 恢复衰减;
/// - halted_mid_stream(流内停止)与恢复失败、协议错误都是提前退出,不再走工具执行。
///
/// 可变运行态全部经 &mut 传入:即使以 Stopped 提前退出,步内已累积的
/// calibration/usage/恢复计数也如实保留,调用方照常构造 RunSummary。
#[allow(clippy::too_many_arguments)] // 内部段函数,不对外暴露签名(R-202)。
async fn stream_request_step(
    client: &LlmClient,
    route: &Route,
    config: &RunnerConfig,
    system: &[String],
    specs: &[ToolSpec],
    messages: &mut Vec<Message>,
    step: u32,
    last_step: bool,
    budget_checkpoint: bool,
    halt: Option<&CancellationToken>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
    calibration: &mut f64,
    last_input_tokens: &mut Option<u64>,
    total_usage: &mut Usage,
    overflow_recoveries: &mut u32,
    overflow_traces: &mut Vec<String>,
) -> anyhow::Result<StepOutcome> {
    let mut stream_restarts: u32 = 0;
    loop {
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
        let req_tools: &[ToolSpec] = if last_step { &[] } else { specs };
        let last_estimated = estimate_prompt_tokens(system, &request_messages, req_tools);
        let request = LlmRequest {
            model: config.model.clone(),
            system: system.to_vec(),
            messages: request_messages,
            tools: req_tools.to_vec(),
            max_tokens: config.max_tokens,
            temperature: None,
            reasoning: config.reasoning,
            service_tier: config.service_tier.clone(),
        };
        let mut stream = match client
            .stream_with_retry_notice(route, &request, |attempt, delay| {
                on_event(RunEvent::Retry {
                    attempt,
                    max: kanzei_llm::client::MAX_TRANSPORT_RETRIES,
                    delay_ms: delay.as_millis(),
                });
            })
            .await
        {
            Err(error) if error.is_context_overflow() => {
                if recover_context_overflow(messages, overflow_recoveries, overflow_traces) {
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
        let mut halted_mid_stream = false;

        loop {
            // D-342 流内检查点:停止时丢弃本步半成品(messages 尚未被本步
            // 触碰),立即收尾——不等模型把整步吐完。
            let event = tokio::select! {
                event = stream.next() => match event {
                    Some(event) => event,
                    None => break,
                },
                _ = halt_signalled(halt) => {
                    halted_mid_stream = true;
                    break;
                }
            };
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
                    *calibration = update_calibration(*calibration, last_estimated, usage.input);
                    if usage.input > 0 {
                        *last_input_tokens = Some(usage.input);
                    }
                    *total_usage = add_usage(*total_usage, usage);
                    finish = reason.clone();
                    // R-219:恢复计数随成功衰减——被动恢复成功且后续步正常结束,
                    // 计数逐步回落,长跑不会因早先一次 overflow 就永久锁定在
                    // 「已恢复 2 次,下一次直接终止」。每成功一步减 1(封底 0),
                    // 让恢复额度在长时间稳定运行后重新充满。
                    *overflow_recoveries = decay_overflow_recoveries(*overflow_recoveries);
                    on_event(RunEvent::StepEnd { usage, reason });
                }
                _ => {}
            }
        }

        if halted_mid_stream {
            return Ok(StepOutcome::Stopped);
        }
        match stream_error {
            None => {
                for (_, text) in std::mem::take(&mut text_buffers) {
                    parts.push(Part::Text { text });
                }
                return Ok(StepOutcome::Completed {
                    parts,
                    calls,
                    finish,
                });
            }
            // Provider 也可能在 HTTP 200 的 SSE error 事件里报告上下文超限。
            // 此时本步工具尚未执行，压缩 messages 后安全地从头重放请求。
            Some(error) if error.is_context_overflow() => {
                if recover_context_overflow(messages, overflow_recoveries, overflow_traces) {
                    continue;
                }
                return Err(error.into());
            }
            // D-402:SSE 流内的限流/过载(server_is_overloaded 一族)——本步尚无
            // 任何产出(无文本/推理增量、无工具调用)时重放是安全的,与「流一旦
            // 建立不重放」的副作用纪律不冲突:没有副作用可重复。有产出则照旧
            // 抛给上层,绝不冒重复副作用的险。
            Some(error)
                if error.is_rate_limited()
                    && parts.is_empty()
                    && calls.is_empty()
                    && text_buffers.is_empty()
                    && reasoning_buffers.is_empty()
                    && stream_restarts < config.limits.stream_restarts() =>
            {
                stream_restarts += 1;
                let delay = std::time::Duration::from_millis(2000 * stream_restarts as u64);
                tracing::warn!(
                    attempt = stream_restarts,
                    delay_ms = delay.as_millis(),
                    error = %error,
                    "provider overloaded mid-stream with empty step, re-requesting"
                );
                on_event(RunEvent::StreamRestart {
                    attempt: stream_restarts,
                    max: config.limits.stream_restarts(),
                    delay_ms: delay.as_millis(),
                });
                tokio::time::sleep(delay).await;
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
    }
}

/// R-202 批4:task 子代理段——同轮多个 task 的并行/后台两种派发、每轮数量上限、
/// 进度通道事件转发与 D-342 停止时的取消占位补齐。
///
/// 行为与原内联段逐字节对齐(行为零变更):
/// - 前台模式:FuturesUnordered 并行执行,完成一个立即 ToolEnd,全部结束后
///   drain 进度通道;halt 提前退出时缺席结果以取消占位补齐配对。
/// - 后台模式:立即 spawn 每个 task,本轮 ToolResult 填「已后台派发」占位,
///   生命周期事件与终态通知按 R-175 落 session_events / background_results。
/// - 溢出数量上限(max_tasks_per_turn)的调用以 ToolOutput::error 即时回喂。
#[allow(clippy::too_many_arguments)] // 内部段函数,不对外暴露签名(R-202);R-246 增 line_runtime。
async fn run_subagent_calls(
    subagent: Option<&SubagentRuntime>,
    client: &LlmClient,
    ctx: &ToolCtx,
    config: &RunnerConfig,
    calls: &[(String, String, serde_json::Value, String)],
    halt: Option<&CancellationToken>,
    line_runtime: Option<&LineRuntime>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> std::collections::HashMap<String, kanzei_harness::ToolOutput> {
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
                    outcome: output.outcome.as_str().into(),
                    code: output.code.map(str::to_owned),
                    preview: preview(&output.content),
                    display: output.display.clone(),
                    artifact: output.artifact.clone(),
                });
                task_results.insert(id.clone(), output);
            }
            // 进度通道:子代理内部事件(轮次/工具)转成 TaskProgress 实时上抛,
            // 完成一个立刻报一个 ToolEnd——不再等最慢的,UI 全程有反馈。
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RunEvent>();
            if rt.background {
                // R-175 B1b:后台模式——派发即返回,主代理本轮不等待。
                // 每个 task 立即 spawn:client/rt/ctx clone 后移入 async 块,
                // 返回 JoinHandle;本轮 ToolResult 填「已后台派发,句柄 <id>」
                // 占位,真实结果经 progress 通道回传 ToolEnd + 写入
                // background_results 暂存,供后续轮次查询(验收②)。
                // 读槽:run_subagent 内 `_read_permit` 是函数局部变量,随
                // run_subagent 返回即释放(函数返回 = 子代理跑完)——后台
                // 化不需要额外显式 drop,因为 spawn 块 await 的就是
                // run_subagent 的完整生命周期。progress 通道 rx 在主代理侧
                // drop,子代理内 send 失败静默忽略(既有 `let _ =` 容忍),
                // 完成结果走 background_results,不依赖事件流。
                for (id, input, _) in &task_calls {
                    let tx = tx.clone();
                    // `client`/`rt`/`ctx` 是 &'a 引用;`(&T).clone()` 会解析到
                    // `impl Clone for &T`(返回引用),move 进 'static async 块就
                    // 逃逸。`(*x).clone()` 强制调用值类型的 Clone,产生 owned。
                    let client: LlmClient = (*client).clone();
                    let rt: SubagentRuntime = (*rt).clone();
                    let ctx: ToolCtx = ctx.clone();
                    let call_id = id.clone();
                    let input = input.clone();
                    let results = rt.background_results.clone();
                    let events = rt.background_events.clone();
                    let notifications = rt.background_notifications.clone();
                    let timeout_secs = rt.timeout_secs;
                    // R-246 批3:后台 spawn 的 JoinHandle 交给 LineRuntime 登记,
                    // dispose 时 cancel 后 await 全部退出(验收②三种终态静止)。
                    let handle = tokio::spawn(async move {
                        // R-175 B5:派发即记 running 事件——重启后从 session_events
                        // 找「有 running 无后续终态」的 id,即上次未终结的子代理
                        // (验收③:注册表能列出并给确定处置,不留幽灵条目)。
                        if let Some(sink) = events.as_ref() {
                            sink(
                                &call_id,
                                serde_json::json!({
                                    "kind": "task.lifecycle",
                                    "id": call_id,
                                    "state": "running",
                                    "ok": null,
                                    "preview": null,
                                }),
                            );
                        }
                        let bound = std::time::Duration::from_secs(timeout_secs);
                        let mut output = match tokio::time::timeout(
                            bound,
                            run_subagent(&client, &rt, &ctx, &call_id, &input, tx),
                        )
                        .await
                        {
                            Ok(output) => output,
                            Err(_) => kanzei_harness::ToolOutput::error(format!(
                                "subagent hit the {}s wall-clock safety limit — split the task into narrower pieces",
                                timeout_secs
                            )),
                        };
                        materialize_tool_output(&mut output, &ctx, "task");
                        if let Some(results) = results {
                            results
                                .lock()
                                .unwrap()
                                .insert(call_id.clone(), output.clone());
                        }
                        // R-175 B2:生命周期事件落 session_events——完成/失败/超时
                        // 统一记一条 task.lifecycle(含终态),可回放、可审计。
                        if let Some(sink) = events {
                            sink(
                                &call_id,
                                serde_json::json!({
                                    "kind": "task.lifecycle",
                                    "id": call_id,
                                    "state": if output.is_error { "failed" } else { "done" },
                                    "ok": !output.is_error,
                                    "preview": preview(&output.content),
                                    "artifact": output.artifact.clone(),
                                }),
                            );
                        }
                        // R-175 B4:发通知回主对话——复用 agent_notifications 表。
                        // 三终态(完成/失败/超时)统一走这里,status ∈ done|failed|
                        // timeout;主对话据此知道后台子代理的确定归宿(验收⑦)。
                        if let Some(notify) = notifications {
                            notify(&call_id, if output.is_error { "failed" } else { "done" });
                        }
                    });
                    // R-246 批3:登记 spawn 句柄——dispose 时 cancel 后 await 全部
                    // join,保证返回前子代理已静止(三种终态都在 run_subagent
                    // 返回时释放读槽)。LineRuntime 为 None(测试/CLI 无)时跳过。
                    if let Some(line_rt) = line_runtime {
                        line_rt.track_child_agent(handle);
                    }
                    task_results.insert(
                        id.clone(),
                        kanzei_harness::ToolOutput::ok(format!(
                            "已后台派发,句柄 {id};真实结果将经通知/后续轮次查询回传(R-175 后台模式)"
                        )),
                    );
                }
                drop(rx);
                // 主代理不 drain、不 select! 等待——直接继续普通工具段。
            } else {
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
                            Some((id, mut output)) => {
                                materialize_tool_output(&mut output, ctx, "task");
                                on_event(RunEvent::ToolEnd {
                                    id: id.clone(),
                                    name: "task".into(),
                                    ok: !output.is_error,
                                    outcome: output.outcome.as_str().into(),
                                    code: output.code.map(str::to_owned),
                                    preview: preview(&output.content),
                                    display: output.display.clone(),
                                    artifact: output.artifact.clone(),
                                });
                                task_results.insert(id, output);
                            }
                            None => {
                                drain_task_events(&mut rx, on_event);
                                break;
                            }
                        },
                        Some(event) = rx.recv() => on_event(event),
                        // D-342:停止时不再等剩余子代理(futures 随 break 被
                        // drop,注册守卫 RAII 释放读槽);缺席结果在下面统一
                        // 用取消占位补齐配对。
                        _ = halt_signalled(halt) => {
                            drain_task_events(&mut rx, on_event);
                            break;
                        }
                    }
                }
                // D-342:halt 提前退出时补齐缺席的 task 结果;正常路径全员
                // 已有终态,entry 不命中,零行为差异。
                for (id, _, _) in &task_calls {
                    task_results.entry(id.clone()).or_insert_with(|| {
                        kanzei_harness::ToolOutput::error("cancelled: run stopped by user")
                    });
                }
            }
        }
    }
    task_results
}
/// R-202 批5:普通工具执行段的产物。
enum ToolRunOutcome {
    /// 正常完成:results 与 calls 按下标对齐,pending_images 在 commit_tool_results
    /// 合流(R-249)。
    Results {
        results: Vec<Part>,
        pending_images: Vec<Part>,
    },
    /// 串行路径提前退出(D-342 工具间停止检查点 / 权限用户拒绝):调用方构造
    /// halted RunSummary,此时 messages 已含取消/拒绝占位的 ToolResults。
    Stopped,
}

/// R-202 批5:普通工具执行段——并行预检(can_parallel_tools)与 wave/串行两条
/// 执行路径的整体抽离。
///
/// 行为与原内联段逐字节对齐(行为零变更):
/// - 预检:serial_writer 强制串行;否则普通工具(非 task/question/null-input)按
///   权限 Ask 判定是否已批准,全部批准且数量 ≥2 才走确定性 wave。
/// - 并行 wave:results[i] ↔ calls[i] 下标对齐;wave 对停止敏感,select 退出即
///   drop 在飞 future,缺席槽位以取消占位补齐。
/// - 串行路径:按 calls 顺序执行;工具间停止检查点与权限 UserDeclined 都是
///   commit_tool_results 后提前收尾,以 ToolRunOutcome::Stopped 表达。
/// - task 结果归位(task_results.remove)与权限门禁(Deny/Ask/Allow + session
///   记忆)逻辑原样保留。
#[allow(clippy::too_many_arguments)] // 内部段函数,不对外暴露签名(R-202)。
async fn execute_tool_calls(
    config: &RunnerConfig,
    ctx: &ToolCtx,
    snapshot: &HarnessSnapshot,
    tools: &[Arc<dyn Tool>],
    calls: &[(String, String, serde_json::Value, String)],
    subagent: Option<&SubagentRuntime>,
    task_results: &mut std::collections::HashMap<String, kanzei_harness::ToolOutput>,
    images_supported: bool,
    halt: Option<&CancellationToken>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
    ask: &mut (dyn FnMut(AskRequest) -> AskFuture + Send),
    session_approved: &mut std::collections::HashSet<(String, String)>,
    session_rules: &mut Vec<(String, String)>,
    messages: &mut Vec<Message>,
    step: u32,
) -> anyhow::Result<ToolRunOutcome> {
    let halted = || halt.is_some_and(|token| token.is_cancelled());
    // R-171 批2:writer 阶段(ReadWriteSerial)强制普通工具串行——
    // max in-flight=1 且结果按模型调用顺序归位(验收③)。设计文档不变量 5。
    let serial_writer = config.execution_policy.is_serial_writer();
    // R-097 批一：权限询问仍按旧路径串行处理(R-086 承接询问路由)；当本批
    // 不需要新 ask 时，普通工具按显式并发契约切成确定性 wave 并发执行。
    let can_parallel_tools = if serial_writer {
        false
    } else {
        let mut ready = true;
        let mut ordinary_count = 0usize;
        for (_, name, input, _) in calls {
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
                // D-269:bash 的资源是 shell 文本,不能走路径规范化(非单射会把
                // 一条授权放大成整个原像类)。三个评估站点必须用同一个分流函数。
                let resource =
                    kanzei_harness::permission::normalize_resource_for_action(action, &resource);
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
    // R-249:图片附件与 results 分开收集,合流在 commit_tool_results。
    let mut pending_images: Vec<Part> = Vec::new();
    let results = if can_parallel_tools {
        let mut slots: Vec<Option<Part>> =
            std::iter::repeat_with(|| None).take(calls.len()).collect();
        let mut prepared = Vec::new();
        for (index, (id, name, input, raw_input)) in calls.iter().cloned().enumerate() {
            if name == "task" && subagent.is_some() {
                let output = task_results.remove(&id).unwrap_or_else(|| {
                    kanzei_harness::ToolOutput::error("internal: task result missing")
                });
                let model_content = output.model_content();
                slots[index] = Some(Part::ToolResult {
                    call_id: id,
                    content: model_content,
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
                // D-269:同上面的并行预检站点,bash 走原样,路径类仍走 normalize_resource。
                .map(|resource| {
                    kanzei_harness::permission::normalize_resource_for_action(action, &resource)
                })
                .find(|resource| snapshot.evaluate(action, resource) == Effect::Deny);
            if let Some(resource) = denied {
                // R-183:deny 判定带命中的规则原文(验收④轨迹;硬 deny 无普通规则 → None)。
                let rule = snapshot
                    .evaluate_with_rule(action, &resource)
                    .1
                    .map(describe_rule);
                on_event(RunEvent::PermissionResolved {
                    tool_call_id: id.clone(),
                    action: action.to_string(),
                    resource: resource.clone(),
                    decision: "deny",
                    source: "ruleset",
                    rule,
                });
                let output = kanzei_harness::ToolOutput::error(format!(
                    "permission denied by ruleset: {action} on `{resource}`.\n{}",
                    snapshot.denial_hint(action, &resource),
                ));
                on_event(RunEvent::ToolEnd {
                    id: id.clone(),
                    name,
                    ok: false,
                    outcome: output.outcome.as_str().into(),
                    code: output.code.map(str::to_owned),
                    preview: preview(&output.content),
                    display: None,
                    artifact: None,
                });
                let model_content = output.model_content();
                slots[index] = Some(Part::ToolResult {
                    call_id: id,
                    content: model_content,
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
        // D-342:并行 wave 对停止敏感——select 退出即 drop 在飞工具 future,
        // 缺席槽位用取消占位补齐,calls↔results 配对不破。块作用域保证 wave
        // future(借着 on_event)在补占位前已释放。
        let wave_results = {
            let wave = execute_prepared_tools(
                prepared,
                ctx,
                config.limits.max_parallel_tools(),
                images_supported,
                on_event,
            );
            tokio::pin!(wave);
            tokio::select! {
                results = &mut wave => Some(results),
                _ = halt_signalled(halt) => None,
            }
        };
        match wave_results {
            Some(list) => {
                for (index, result, images) in list {
                    slots[index] = Some(result);
                    // R-249:按 index 升序抵达,追加顺序即调用顺序。
                    pending_images.extend(images);
                }
            }
            None => {
                for (index, (id, _, _, _)) in calls.iter().enumerate() {
                    if slots[index].is_none() {
                        slots[index] = Some(Part::ToolResult {
                            call_id: id.clone(),
                            content: "cancelled: run stopped by user during execution".into(),
                            is_error: true,
                        });
                    }
                }
            }
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
            // D-342 工具间检查点:上一个工具执行期间收到停止,剩余调用全部
            // 取消占位配对后 halted 收尾——已完成的结果原样保留在历史里。
            if halted() {
                append_halted_tool_results(&mut results, calls, call_index);
                commit_tool_results(
                    messages,
                    results,
                    std::mem::take(&mut pending_images),
                    step,
                    on_event,
                );
                return Ok(ToolRunOutcome::Stopped);
            }
            // task 不过权限门禁:子代理快照在代码层面只含只读工具(硬门禁在构造,不在评估)。
            // ToolEnd 已在并行阶段按完成顺序上报过,这里只归位结果。
            if name == "task" && subagent.is_some() {
                let output = task_results.remove(&id).unwrap_or_else(|| {
                    kanzei_harness::ToolOutput::error("internal: task result missing")
                });
                let model_content = output.model_content();
                results.push(Part::ToolResult {
                    call_id: id,
                    content: model_content,
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
                let output = execute_question(config, &input, ask).await;
                on_event(RunEvent::ToolEnd {
                    id: id.clone(),
                    name: name.clone(),
                    ok: !output.is_error,
                    outcome: output.outcome.as_str().into(),
                    code: output.code.map(str::to_owned),
                    preview: preview(&output.content),
                    display: output.display.clone(),
                    artifact: output.artifact.clone(),
                });
                let model_content = output.model_content();
                results.push(Part::ToolResult {
                    call_id: id,
                    content: model_content,
                    is_error: output.is_error,
                });
                continue;
            }

            // ---- 硬门禁:权限 Ruleset(deny 回喂模型;ask 问用户,拒绝停整轮)----
            let action = tool.action();
            let mut gate_result = Gate::Pass;
            let mut pending_ask: Vec<String> = Vec::new();
            for resource in tool.resources_with_ctx(&input, ctx) {
                // 路径类资源:统一正斜杠 + 消解 . / ..,权限 pattern 不用关心平台,也不能
                // 被路径变体绕过:`.kanzei/research/../../src/main.rs` 会被
                // `*.kanzei/research/*` 判为放行,而落盘时 join 会消解 ..,实际写到项目
                // 任意位置(D-050)。
                // bash 资源是 shell 文本,同一套规范化在它身上是提权通道(D-269):
                // `..` 会把前一段整段弹掉,注入语句藏在被弹掉的那一段里。这里落到
                // session_rules 的 pattern 也是本函数的产物——bash 走原样,注入段里的
                // `*` 才能活到 pattern 成形,D-051 的串联降级才不会被绕开。
                let normalized =
                    kanzei_harness::permission::normalize_resource_for_action(action, &resource);
                // R-183:ruleset 判定带命中的规则原文(验收④轨迹)。
                let mut resolved = |decision, source, rule: Option<String>| {
                    on_event(RunEvent::PermissionResolved {
                        tool_call_id: id.clone(),
                        action: action.to_string(),
                        resource: normalized.clone(),
                        decision,
                        source,
                        rule,
                    });
                };
                match snapshot.evaluate_with_rule(action, &normalized) {
                    (Effect::Deny, rule) => {
                        resolved("deny", "ruleset", rule.map(describe_rule));
                        gate_result = Gate::Deny(normalized);
                        break;
                    }
                    (Effect::Ask, _) => pending_ask.push(normalized),
                    (Effect::Allow, rule) => {
                        resolved("allow", "ruleset", rule.map(describe_rule));
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
                            // R-183:会话层/策略层决策无规则原文可归属。
                            rule: None,
                        });
                    };
                    if session_approved.contains(&key) {
                        resolved("allow", "session_approved");
                        continue;
                    }
                    if session_rules.iter().any(|(a, pattern)| {
                        a == action
                            && kanzei_harness::permission::resource_match_for_action(
                                a, pattern, &resource,
                            )
                    }) {
                        resolved("allow", "session_rule");
                        continue;
                    }
                    match config.ask_policy {
                        // D-281:自动放行——权限询问直接放行并落事件,不短路、
                        // 不再需要前端替答(前端 07-events.js 只处理 Interactive 轮)。
                        AskPolicy::AutoAllow => {
                            resolved("allow", "auto_allow");
                            continue;
                        }
                        _ if !config.ask_policy.allows_user_prompt() => {
                            resolved("declined", "noninteractive");
                            gate_result = Gate::NonInteractive(format!(
                                "permission requires user approval: {action} on `{resource}`; autonomous/parallel run skipped it",
                            ));
                            break;
                        }
                        _ => {}
                    }
                    match ask(AskRequest::Permission {
                        action: action.to_string(),
                        resource: resource.clone(),
                    })
                    .await
                    {
                        AskResponse::Permission(AskReply::Deny)
                        | AskResponse::Cancelled
                        | AskResponse::Answer(_) => {
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
                Gate::NonInteractive(message) => kanzei_harness::ToolOutput::error(message),
                Gate::UserDeclined => {
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name: name.clone(),
                        ok: false,
                        outcome: "failed".into(),
                        code: Some("USER_DECLINED".into()),
                        preview: "(user declined)".into(),
                        display: None,
                        artifact: None,
                    });
                    append_declined_tool_results(&mut results, calls, call_index);
                    commit_tool_results(
                        messages,
                        results,
                        std::mem::take(&mut pending_images),
                        step,
                        on_event,
                    );
                    return Ok(ToolRunOutcome::Stopped);
                }
                Gate::Pass => {
                    if input.is_null() {
                        repair_hint(tool.as_ref(), &raw_input, "tool input was not valid JSON")
                    } else {
                        // 串行路径同样接进度旁路:bash 常因权限询问走到这里,
                        // 长命令(装依赖/发版)的增量输出边执行边转发给 UI。
                        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<
                            kanzei_harness::progress::ProgressChunk,
                        >();
                        // D-174:串行路径同样开合法写入窗口。writer 阶段
                        // 走的就是这条,漏掉它等于让专用工具的写入没有窗口
                        // 可归因,后台守卫会把它当成越界回滚掉。
                        // R-259:执行包装(wrap_execute:progress 注入)收编——
                        // 串行/并行共用同一 wrapper;halted 前置拦截也在 wrapper。
                        let exec = kanzei_harness::managed_fence::tool_scope(
                            &name,
                            kanzei_harness::tool_pipeline::wrap_execute(
                                id.clone(),
                                Some(progress_tx),
                                Some(&halted),
                                tool.execute(input, ctx),
                            ),
                        );
                        tokio::pin!(exec);
                        let mut output = loop {
                            tokio::select! {
                                biased;
                                Some((pid, chunk)) = progress_rx.recv() => {
                                    on_event(RunEvent::ToolProgress { id: pid, chunk });
                                }
                                output = &mut exec => break output,
                                // D-342:执行中的工具对停止敏感——drop future
                                // 即中断执行(bash 子进程随之回收),以取消
                                // 错误配对;下一轮 for 循环的检查点负责收尾。
                                _ = halt_signalled(halt) => {
                                    break kanzei_harness::ToolOutput::error(
                                        "cancelled: run stopped by user during execution",
                                    );
                                }
                            }
                        };
                        while let Ok((pid, chunk)) = progress_rx.try_recv() {
                            on_event(RunEvent::ToolProgress { id: pid, chunk });
                        }
                        materialize_tool_output(&mut output, ctx, &name);
                        output
                    }
                }
            };
            on_event(RunEvent::ToolEnd {
                id: id.clone(),
                name: name.clone(),
                ok: !output.is_error,
                outcome: output.outcome.as_str().into(),
                code: output.code.map(str::to_owned),
                preview: preview(&output.content),
                display: output.display.clone(),
                artifact: output.artifact.clone(),
            });
            let mut model_content = output.model_content();
            let (images, dropped_note) =
                crate::runner::tool_exec::tool_images_to_parts(&output, images_supported);
            if let Some(note) = dropped_note {
                model_content.push_str(&note);
            }
            pending_images.extend(images);
            results.push(Part::ToolResult {
                call_id: id,
                content: model_content,
                is_error: output.is_error,
            });
        }
        results
    };
    Ok(ToolRunOutcome::Results {
        results,
        pending_images,
    })
}
/// R-202 批6:run_once_with_parts 装配段产物。
///
/// 装配(工具/specs/system 分块/消息初始化/各类运行态)一次性完成,调用方解构后
/// 变量名与内联时代逐字节一致,后续主循环零改动。halt/halted 借用 config 不进入
/// struct(生命周期留在调用方)。
struct RunOnceAssembly<'a> {
    tools: Vec<Arc<dyn Tool>>,
    specs: Vec<ToolSpec>,
    context_report: Vec<(String, usize)>,
    stable_system: Vec<String>,
    refreshable_baseline: String,
    messages: Vec<Message>,
    total_usage: Usage,
    last_input_tokens: Option<u64>,
    final_text: String,
    max_steps: u32,
    session_approved: std::collections::HashSet<(String, String)>,
    session_rules: Vec<(String, String)>,
    overflow_recoveries: u32,
    futile_compactions: u32,
    overflow_traces: Vec<String>,
    calibration: f64,
    redundancy: RedundancyWatch,
    recall: RecallWatch<'a>,
    step: u32,
}

/// R-202 批6:run_once_with_parts 装配段——工具物化、task 注册、system 分块
/// (agent/baseline/记忆提示)、context_report 账单、prior 清洗与用户消息装载、
/// 以及全部轮级运行态(usage/停止文本/会话批准/压缩计数/校准/冗余门禁/召回)
/// 的初始化。
///
/// 行为与原内联段逐字节对齐(行为零变更):
/// - task 注册仍只发生在 subagent.is_some()(R-173 批4.5 口径,见内联注释);
/// - baseline 进 Context Epoch,refreshable_baseline 每步在调用方刷新;
/// - messages 先经 filter_message_history 清洗孤儿工具 part,再装载用户消息;
/// - halted 闭包与 config.halt 生命周期耦合,留在调用方构造。
///
/// R-173/R-280:task 只在子代理运行时存在时加入工具面。把判定抽成纯函数，
/// 让「关闭即不注册」成为可直接断言的构造层契约，而不是注册后再拒绝。
fn append_subagent_spec(specs: &mut Vec<ToolSpec>, subagents_enabled: bool) {
    if subagents_enabled {
        specs.push(task_spec());
    }
}

#[cfg(test)]
mod subagent_tool_surface_tests {
    use super::*;

    #[test]
    fn disabled_subagents_do_not_add_task_to_tool_specs() {
        let mut specs = Vec::new();
        append_subagent_spec(&mut specs, false);
        assert!(specs.iter().all(|spec| spec.name != "task"));
    }

    #[test]
    fn enabled_subagents_add_task_to_tool_specs() {
        let mut specs = Vec::new();
        append_subagent_spec(&mut specs, true);
        assert!(specs.iter().any(|spec| spec.name == "task"));
    }
}

#[allow(clippy::too_many_arguments)] // 内部段函数,不对外暴露签名(R-202)。
fn assemble_run_once<'a>(
    snapshot: &HarnessSnapshot,
    agent: &AgentDef,
    config: &'a RunnerConfig,
    prompt: &str,
    memory_hints: Option<&str>,
    scout_brief: Option<&str>,
    prior: &[Message],
    initial_parts: Option<&[Part]>,
    subagent: Option<&SubagentRuntime>,
) -> RunOnceAssembly<'a> {
    let tools: Vec<Arc<dyn Tool>> = snapshot.materialize_tools();
    let mut specs: Vec<ToolSpec> = tools
        .iter()
        .map(|t| ToolSpec {
            name: t.name().to_string(),
            description: t.description(),
            input_schema: t.input_schema(),
        })
        .collect();
    append_subagent_spec(&mut specs, subagent.is_some());

    // system 分块:agent 提示词 + harness baseline(M2 起 baseline 进 Context Epoch)。
    let (baseline, mut context_report) = snapshot.stable_system_baseline_with_report();
    let (refreshable_baseline, refreshable_report) =
        snapshot.refreshable_system_baseline_with_report();
    context_report.extend(refreshable_report);
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
    // D-185:记忆提示块是稳定 system 段(每步都在,同 agent/system),但**不进
    // messages**——messages 是持久化与回灌的载体,进去就会被逐轮累积回灌。
    // context_report 单独记 memory/hints,让注入 token 账单能看到 hint 段的占比。
    if let Some(hints) = memory_hints {
        if !hints.trim().is_empty() {
            context_report.push(("memory/hints".into(), hints.chars().count()));
        }
    }
    // 勘察简报同 D-185 待遇:稳定 system 段,不进 messages。它原先被拼进 prompt,
    // 于是随 User message 落进 conversations,下一轮作为 prior 回灌——而流水线每轮
    // 都会重新勘察,回灌的那份旧简报永远不是最新可用信息,只是让 agent 多花一次
    // 「这是不是上轮残留」的分辨成本。单独记账,让它的 token 占比可见。
    if let Some(brief) = scout_brief {
        if !brief.trim().is_empty() {
            context_report.push(("scout/brief".into(), brief.chars().count()));
        }
    }
    let mut stable_system: Vec<String> = [agent.system.clone(), baseline]
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();
    if let Some(hints) = memory_hints {
        if !hints.trim().is_empty() {
            stable_system.push(hints.to_string());
        }
    }
    if let Some(brief) = scout_brief {
        if !brief.trim().is_empty() {
            stable_system.push(brief.to_string());
        }
    }

    // prior 可能来自旧快照或跨进程恢复，先统一清洗孤儿工具 part，避免首次请求
    // 在尚未触发上下文压缩时就把非法消息交给 provider。
    let mut messages: Vec<Message> = crate::history::filter_message_history(prior);
    let user_parts = match initial_parts {
        Some(parts) => {
            let mut parts = parts.to_vec();
            if !prompt.is_empty() {
                parts.insert(
                    0,
                    Part::Text {
                        text: prompt.to_string(),
                    },
                );
            }
            parts
        }
        None => vec![Part::Text {
            text: prompt.to_string(),
        }],
    };
    messages.push(Message {
        role: Role::User,
        parts: user_parts,
    });
    let total_usage = Usage::default();
    // R-236 B1:轮末优先使用最近一次 provider usage.input；None 仅表示本轮没有有效 usage。
    let last_input_tokens: Option<u64> = None;
    // D-342:停止检查点用。提前初始化——halted 提前返回时它是「最近一步的文本」。
    let final_text = String::new();
    // 存量 agent 可能仍带 steps=0；运行边界统一转换，不能让旧配置绕过有限上限。
    let max_steps = kanzei_harness::effective_agent_steps(agent.steps);
    // 本次运行内已放行的 (action, resource):同一资源不重复问(用户反馈:别烦我)。
    let session_approved: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    // "总是允许"的会话内即时生效层(D-006):快照是开跑时定死的,新写入的规则
    // 本次运行读不到——泛化 pattern 记在这里,同类资源当场不再询问。
    let session_rules: Vec<(String, String)> = Vec::new();

    let overflow_recoveries = 0;
    // 主动压缩的连续无效计数(D-206),与被动恢复各记各的。只数"压了没用",
    // 成功的压缩清零——压缩是常规运营动作,不设总量配额。
    let futile_compactions = 0u32;
    let overflow_traces: Vec<String> = Vec::new();
    // 估算校准:len/4 粗估对中文 \uXXXX 转义、工具输出密集的会话有系统性偏差,
    // 预算线 0.7 的语义要靠真实 usage 反推的滑动因子校准才有意义。初始 1.0,
    // 每步拿到 provider 真实 input tokens 后 EMA 更新。
    let calibration = 1.0f64;
    // R-100 冗余机械门禁:按单次运行持有(跨轮清零),提醒追加进工具结果不阻断。
    let redundancy = RedundancyWatch::default();
    // R-162 事件触发召回:工具失败瞬间把相关记忆 Packet 注入下一请求前。
    // 策略从 config 借用(不拥有);None = 关闭召回,零行为变化。
    let recall = RecallWatch::new(config.recall.as_deref());

    let step = 0u32;
    RunOnceAssembly {
        tools,
        specs,
        context_report,
        stable_system,
        refreshable_baseline,
        messages,
        total_usage,
        last_input_tokens,
        final_text,
        max_steps,
        session_approved,
        session_rules,
        overflow_recoveries,
        futile_compactions,
        overflow_traces,
        calibration,
        redundancy,
        recall,
        step,
    }
}
/// R-202 批6:步骤消息提交段的产物。
enum StepMessageOutcome {
    /// 消息已提交、存在待执行工具调用,继续工具批执行。
    Proceed,
    /// 提前收尾(calls 为空 = 纯文本步 / D-342 停止占位),调用方构造 RunSummary。
    Return { halted_by_user: bool },
}

/// R-202 批6:步骤消息提交——final_text 提取、assistant 消息落库、以及
/// 「无工具调用」与「产出了调用但停止已置位」两条提前收尾路径。
///
/// 行为与原内联段逐字节对齐(行为零变更):
/// - final_text 只取 Text part 拼接(推理/工具调用不进收尾文本);
/// - calls 为空 → halted_by_user 如实反映停止状态(D-342);
/// - 停止已置位 → 全部调用以取消占位配对后 halted 收尾。
fn commit_step_messages(
    parts: Vec<Part>,
    calls: &[(String, String, serde_json::Value, String)],
    final_text: &mut String,
    messages: &mut Vec<Message>,
    step: u32,
    halt: Option<&CancellationToken>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> StepMessageOutcome {
    *final_text = parts
        .iter()
        .filter_map(|p| match p {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if !parts.is_empty() {
        commit_assistant_message(messages, parts, step, on_event);
    }

    if calls.is_empty() {
        return StepMessageOutcome::Return {
            // D-342:纯文本步收尾时停止可能已置位,如实标 halted。
            halted_by_user: halt.is_some_and(|token| token.is_cancelled()),
        };
    }

    // D-342:模型产出了工具调用但停止已置位——一个工具都不执行,全部以
    // 取消占位配对(与权限拒绝同款形态),halted 正常收尾。
    if halt.is_some_and(|token| token.is_cancelled()) {
        let mut results = Vec::new();
        append_halted_tool_results(&mut results, calls, 0);
        // 本步工具一个都没执行,不可能有图片。
        commit_tool_results(messages, results, Vec::new(), step, on_event);
        return StepMessageOutcome::Return {
            halted_by_user: true,
        };
    }
    StepMessageOutcome::Proceed
}

/// R-202 批6:步骤收尾段的产物。
enum StepFinalOutcome {
    /// 本步工具结果已落库,进入下一轮。
    Continue,
    /// last_step 收敛:跳出主循环,走最终 RunSummary 构造。
    Break,
    /// 步末停止检查点 / MaxTokens·Refusal 终止:调用方构造 RunSummary。
    Return { halted_by_user: bool },
}

/// R-202 批6:步骤收尾——冗余提醒与失败召回注入、工具结果落库、步末停止检查点、
/// MaxTokens/Refusal 终止判定与 last_step 收敛。
///
/// 行为与原内联段逐字节对齐(行为零变更):
/// - redundancy.note_step / recall.note_step 先于结果回喂(R-100/R-162 同钩位);
/// - commit_tool_results 合流 pending_images(R-249);
/// - 步末检查点要求本步工具全部有终态(真实或取消占位),配对完整才收尾;
/// - MaxTokens/Refusal 与 last_step 的收敛顺序不变。
#[allow(clippy::too_many_arguments)] // 内部段函数,不对外暴露签名(R-202)。
fn finalize_step(
    ctx: &ToolCtx,
    calls: &[(String, String, serde_json::Value, String)],
    results: &mut Vec<Part>,
    pending_images: Vec<Part>,
    messages: &mut Vec<Message>,
    step: u32,
    finish: &FinishReason,
    last_step: bool,
    halt: Option<&CancellationToken>,
    redundancy: &mut RedundancyWatch,
    recall: &mut RecallWatch<'_>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> StepFinalOutcome {
    // R-100:工具结果回喂前就地注入冗余提醒(不阻断)。
    // results 与 calls 按下标对齐(并行 wave 与串行路径同上),见 redundancy::note_step。
    redundancy.note_step(&ctx.project_root, calls, results);
    // R-162:工具失败瞬间的事件触发召回(同款钩位,先于结果回喂)。
    // 命中则追加 [记忆命中 …] Packet 文本,不阻断、不改 is_error。
    recall.note_step(calls, results);
    commit_tool_results(
        messages,
        std::mem::take(results),
        pending_images,
        step,
        on_event,
    );

    // D-342 步末检查点:本步工具已全部有终态(真实或取消占位),停止在此
    // 收尾——配对完整,下一轮 prior 无孤儿。并行 wave 被停止打断的路径
    // 从这里返回。
    if halt.is_some_and(|token| token.is_cancelled()) {
        return StepFinalOutcome::Return {
            halted_by_user: true,
        };
    }
    if matches!(finish, FinishReason::MaxTokens | FinishReason::Refusal) {
        return StepFinalOutcome::Return {
            halted_by_user: false,
        };
    }
    if last_step {
        return StepFinalOutcome::Break;
    }
    StepFinalOutcome::Continue
}
/// R-202 批6:轮内上下文预算——每步开跑前的主动 prune / 压缩 / trim。
///
/// 行为与原内联段逐字节对齐(行为零变更):
/// - R-219:context_limit 未知时按保守默认 32k 启用主动预算,首次 tracing::warn
///   点名一次(可见不阻塞);
/// - R-236 B1:触发线 headroom 公式;R-236 B4:L0 prune 先行,凑不满最小收益自弃;
/// - D-206:只按"有没有用"记账——压回线内清零,压不动(连 trim_tail 都上了)仍
///   超线则递增,连续 MAX_FUTILE_COMPACTIONS 次后交给撞墙的被动恢复;
/// - D-203:trim_tail 与预算检查用同一个 calibration 口径。
#[allow(clippy::too_many_arguments)] // 内部段函数,不对外暴露签名(R-202)。
async fn enforce_context_budget(
    client: &LlmClient,
    subagent: Option<&SubagentRuntime>,
    config: &RunnerConfig,
    system: &[String],
    specs: &[ToolSpec],
    messages: &mut Vec<Message>,
    calibration: f64,
    futile_compactions: &mut u32,
    overflow_traces: &mut Vec<String>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) {
    // 轮内上下文预算(D-176)。压缩检查原先只写在**一轮结束之后**,而长轮与
    // 自动续跑恰恰是最需要它的场景:一轮不结束就一次也轮不到。实测一次 41
    // 分钟的运行里检查点执行了 0 次,用户按停止后更是直接跳过收尾,全程只能
    // 等 provider 报 overflow 再被动裁剪。这里在每步开跑前主动估一次。
    // R-219:context_limit 未知(白名单外 provider)时按保守默认 32k 启用主动
    // 预算——未知不等于没有上限,放任涨到撞墙会把整个 run 拖进被动恢复;
    // 启动时 tracing::warn 点名一次(可见不阻塞),不打断运行。
    let effective_limit = config.context_limit.unwrap_or_else(|| {
        tracing::warn!(
            "provider 无已知 context_limit,按保守默认 32k 做轮内预算; \
             撞墙前的主动压缩只降级不终止(第三次 overflow 仍会被动终止)"
        );
        32_000
    });
    {
        // R-236 B1:触发线换 headroom 公式(与轮末同一把尺,见 compaction_budget)。
        let budget = compaction_budget(
            effective_limit,
            config.max_tokens,
            config.limits.compact_buffer_tokens(),
        );
        let mut before = budgeted_tokens(system, messages, specs, calibration);
        // R-236 B4:L0 prune 先行——超线时先机械清旧工具结果(零幻觉零
        // LLM),清完够线就不必动纪要;凑不满最小收益 prune 自己会放弃。
        if before > budget && messages.len() > 1 {
            let cleared = prune_old_tool_results(
                messages,
                config.limits.prune_protect_tokens(),
                config.limits.prune_min_gain_tokens(),
            );
            if cleared > 0 {
                let after_prune = budgeted_tokens(system, messages, specs, calibration);
                on_event(RunEvent::ContextPruned {
                    cleared_results: cleared,
                    before_tokens: before,
                    after_tokens: after_prune,
                });
                before = after_prune;
            }
        }
        if before > budget && *futile_compactions < MAX_FUTILE_COMPACTIONS && messages.len() > 1 {
            let dropped_messages = compact_with_digest(
                client,
                subagent,
                messages,
                budget,
                overflow_traces,
                config.limits.recent_verbatim_ratio(),
            )
            .await;
            if dropped_messages > 0 {
                // 压了还超线:tail 太大或 head 太大。再砍 tail 到预算内,否则
                // 下一步预算检查立刻再压——连续两次压缩 = 缓存前缀两次全量
                // 重算(cache_write 双倍),省下的 token 不够补缓存成本。
                // trim_tail 拿同一个 calibration:两边必须用同一把尺子量同一条
                // 预算线,否则它按原始口径够线就收手,这里看还超线(D-203)。
                if budgeted_tokens(system, messages, specs, calibration) > budget {
                    trim_tail(
                        messages,
                        system,
                        specs,
                        budget,
                        calibration,
                        overflow_traces,
                    );
                }
                let after = budgeted_tokens(system, messages, specs, calibration);
                // D-206:只按"有没有用"记账。压回线内 = 压缩在正常工作,清零、
                // 下次照压;压完(连 trim_tail 都上了)仍超线 = head+当前消息
                // 本身超线,连续两次就停,交给撞墙后的被动恢复,别空转。
                if after <= budget {
                    *futile_compactions = 0;
                } else {
                    *futile_compactions += 1;
                }
                on_event(RunEvent::ContextCompacted {
                    before_tokens: before,
                    after_tokens: after,
                    budget_tokens: budget,
                    limit_tokens: effective_limit,
                    dropped_messages,
                });
            } else {
                // 中段为空压不动:不发事件(没骗 UI),但要计无效——否则每步
                // 白跑一次 compact,同样是注释里说的空转。
                *futile_compactions += 1;
            }
        }
    }
}
/// R-202 批7:段函数独立单测(验收①)。commit_step_messages / finalize_step 是
/// 抽离后具备独立输入输出边界的纯逻辑段,不依赖 provider/网络/重夹具即可验证。
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bash_call(id: &str) -> (String, String, serde_json::Value, String) {
        (
            id.to_string(),
            "bash".to_string(),
            json!({ "command": "echo hi" }),
            "{}".to_string(),
        )
    }

    #[test]
    fn commit_step_messages_纯文本步_更新final_text并提交assistant() {
        let mut messages: Vec<Message> = Vec::new();
        let mut final_text = String::new();
        let mut events: Vec<RunEvent> = Vec::new();
        let outcome = commit_step_messages(
            vec![
                Part::Text {
                    text: "第一段".into(),
                },
                Part::Text {
                    text: "第二段".into(),
                },
            ],
            &[],
            &mut final_text,
            &mut messages,
            1,
            None,
            &mut |ev| events.push(ev),
        );
        // 纯文本步:calls 为空 → Return{halted_by_user:false},停止未置位。
        assert!(matches!(
            outcome,
            StepMessageOutcome::Return {
                halted_by_user: false
            }
        ));
        // final_text 只拼 Text part。
        assert_eq!(final_text, "第一段\n第二段");
        // assistant 消息已落库,AssistantMessageCommitted 已上抛。
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].role, Role::Assistant));
        assert!(events
            .iter()
            .any(|ev| matches!(ev, RunEvent::AssistantMessageCommitted { .. })));
    }

    #[test]
    fn commit_step_messages_有工具调用_继续执行() {
        let mut messages: Vec<Message> = Vec::new();
        let mut final_text = String::new();
        let calls = vec![bash_call("c1")];
        let outcome = commit_step_messages(
            vec![Part::Text {
                text: "plan".into(),
            }],
            &calls,
            &mut final_text,
            &mut messages,
            1,
            None,
            &mut |_| {},
        );
        // 有工具调用且未停止 → Proceed,交给工具批执行。
        assert!(matches!(outcome, StepMessageOutcome::Proceed));
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn commit_step_messages_停止已置位_取消占位收尾() {
        let token = CancellationToken::new();
        token.cancel();
        let mut messages: Vec<Message> = Vec::new();
        let mut final_text = String::new();
        let calls = vec![bash_call("c1")];
        let outcome = commit_step_messages(
            vec![Part::Text { text: "t".into() }],
            &calls,
            &mut final_text,
            &mut messages,
            1,
            Some(&token),
            &mut |_| {},
        );
        // 模型产出了调用但停止已置位:一个工具都不执行,取消占位配对后 halted 收尾。
        assert!(matches!(
            outcome,
            StepMessageOutcome::Return {
                halted_by_user: true
            }
        ));
        // assistant + tool_results(取消占位,user 角色)。
        assert_eq!(messages.len(), 2);
        let last = messages.last().unwrap();
        assert!(matches!(last.role, Role::User));
        assert!(last
            .parts
            .iter()
            .any(|p| matches!(p, Part::ToolResult { is_error: true, .. })));
    }

    #[test]
    fn finalize_step_正常落库_继续下一轮() {
        let ctx = ToolCtx::new(std::path::PathBuf::from("."), std::path::PathBuf::from("."));
        let mut messages: Vec<Message> = Vec::new();
        let mut results = vec![Part::ToolResult {
            call_id: "c1".into(),
            content: "ok".into(),
            is_error: false,
        }];
        let calls = vec![bash_call("c1")];
        let outcome = finalize_step(
            &ctx,
            &calls,
            &mut results,
            Vec::new(),
            &mut messages,
            1,
            &FinishReason::EndTurn,
            false,
            None,
            &mut RedundancyWatch::default(),
            &mut RecallWatch::new(None),
            &mut |_| {},
        );
        assert!(matches!(outcome, StepFinalOutcome::Continue));
        // 工具结果以 user 角色落库,结果已从 results 取走(mem::take)。
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].role, Role::User));
        assert!(results.is_empty());
    }

    #[test]
    fn finalize_step_max_tokens_返回非halted() {
        let ctx = ToolCtx::new(std::path::PathBuf::from("."), std::path::PathBuf::from("."));
        let mut messages: Vec<Message> = Vec::new();
        // note_step 的 debug_assert 要求 calls↔results 下标对齐(对齐不变式)。
        let mut results = vec![Part::ToolResult {
            call_id: "c1".into(),
            content: "ok".into(),
            is_error: false,
        }];
        let calls = vec![bash_call("c1")];
        let outcome = finalize_step(
            &ctx,
            &calls,
            &mut results,
            Vec::new(),
            &mut messages,
            1,
            &FinishReason::MaxTokens,
            false,
            None,
            &mut RedundancyWatch::default(),
            &mut RecallWatch::new(None),
            &mut |_| {},
        );
        // MaxTokens/Refusal:步末终止但非用户停止。
        assert!(matches!(
            outcome,
            StepFinalOutcome::Return {
                halted_by_user: false
            }
        ));
    }

    #[test]
    fn finalize_step_last_step_break收敛() {
        let ctx = ToolCtx::new(std::path::PathBuf::from("."), std::path::PathBuf::from("."));
        let mut messages: Vec<Message> = Vec::new();
        let mut results = vec![Part::ToolResult {
            call_id: "c1".into(),
            content: "ok".into(),
            is_error: false,
        }];
        let calls = vec![bash_call("c1")];
        let outcome = finalize_step(
            &ctx,
            &calls,
            &mut results,
            Vec::new(),
            &mut messages,
            1,
            &FinishReason::EndTurn,
            true,
            None,
            &mut RedundancyWatch::default(),
            &mut RecallWatch::new(None),
            &mut |_| {},
        );
        assert!(matches!(outcome, StepFinalOutcome::Break));
    }

    #[test]
    fn finalize_step_停止置位_返回halted() {
        let token = CancellationToken::new();
        token.cancel();
        let ctx = ToolCtx::new(std::path::PathBuf::from("."), std::path::PathBuf::from("."));
        let mut messages: Vec<Message> = Vec::new();
        let mut results = vec![Part::ToolResult {
            call_id: "c1".into(),
            content: "ok".into(),
            is_error: false,
        }];
        let calls = vec![bash_call("c1")];
        let outcome = finalize_step(
            &ctx,
            &calls,
            &mut results,
            Vec::new(),
            &mut messages,
            1,
            &FinishReason::EndTurn,
            false,
            Some(&token),
            &mut RedundancyWatch::default(),
            &mut RecallWatch::new(None),
            &mut |_| {},
        );
        assert!(matches!(
            outcome,
            StepFinalOutcome::Return {
                halted_by_user: true
            }
        ));
    }
}
