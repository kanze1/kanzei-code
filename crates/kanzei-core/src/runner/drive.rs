//! 驱动域(R-155 B8):run_once / run_once_with_parts 整体搬迁,不动内部。
//! 设计 §C B8:本批只搬主体,任何抽函数动作都不在本条目做;
//! calls[i]↔results[i] 下标对齐不变式跨 tool_exec/redundancy/drive 三文件。
//! 设计 §C 要点 1:run_once 是动态分发边界——驱动、subagent、tool_exec 的所有
//! 异步递归都经它(子代理递归经 dyn Box 断开无限类型,见 subagent.rs)。
//! 签名即锁:改动必须同步 mod.rs 的导出与全部调用方。

// 所有符号经 super::* 平铺(mod.rs 的 use 与 pub use 子模块)。
use super::*;

/// D-342:停止信号等待器。halt 未配置时永不就绪——select 里这个分支等价于不存在,
/// CLI(无停止通道)与引入前逐字节同行为。
pub(super) async fn halt_signalled(halt: Option<&CancellationToken>) {
    match halt {
        Some(token) => token.cancelled().await,
        None => std::future::pending().await,
    }
}

/// D-342:停止后未执行的工具调用统一以「取消占位」配对,与权限拒绝占位同款形态,
/// 保证 calls[i]↔results[i] 对齐、历史里没有孤儿 ToolCall。
pub(super) fn append_halted_tool_results(
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
pub(super) fn commit_tool_results(
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
            // 步数软预算(D-173):步数上限是 0(不设人为天花板),但"不封顶"不等于
            // "不盘点"。检查点只要求当轮盘一次剩余范围,不强制中止——实测长轮的
            // 成本失控几乎都始于无人察觉的目标漂移。
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
                run_subagent_calls(subagent, client, ctx, config, &calls, halt, on_event).await;

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

/// R-202 批4:task 子代理段——同轮多个 task 的并行/后台两种派发、每轮数量上限、
/// 进度通道事件转发与 D-342 停止时的取消占位补齐。
///
/// 行为与原内联段逐字节对齐(行为零变更):
/// - 前台模式:FuturesUnordered 并行执行,完成一个立即 ToolEnd,全部结束后
///   drain 进度通道;halt 提前退出时缺席结果以取消占位补齐配对。
/// - 后台模式:立即 spawn 每个 task,本轮 ToolResult 填「已后台派发」占位,
///   生命周期事件与终态通知按 R-175 落 session_events / background_results。
/// - 溢出数量上限(max_tasks_per_turn)的调用以 ToolOutput::error 即时回喂。
async fn run_subagent_calls(
    subagent: Option<&SubagentRuntime>,
    client: &LlmClient,
    ctx: &ToolCtx,
    config: &RunnerConfig,
    calls: &[(String, String, serde_json::Value, String)],
    halt: Option<&CancellationToken>,
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
                    tokio::spawn(async move {
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
                        let output = match tokio::time::timeout(
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
                            Some((id, output)) => {
                                on_event(RunEvent::ToolEnd {
                                    id: id.clone(),
                                    name: "task".into(),
                                    ok: !output.is_error,
                                    outcome: output.outcome.as_str().into(),
                                    code: output.code.map(str::to_owned),
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
/// R-202 批7:段函数独立单测(验收①)。commit_step_messages / finalize_step 是
/// 抽离后具备独立输入输出边界的纯逻辑段,不依赖 provider/网络/重夹具即可验证。
#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_llm::Role;
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
