//! 驱动域(R-155 B8):run_once / run_once_with_parts 整体搬迁,不动内部。
//! R-155 B8 完成整体搬迁；后续把工具目录与消息提交拆到独立模块。
//! calls[i]↔results[i] 下标对齐不变式跨 tool_exec/redundancy/drive 三文件。
//! 设计 §C 要点 1:run_once 是动态分发边界——驱动、subagent、tool_exec 的所有
//! 异步递归都经它(子代理递归经 dyn Box 断开无限类型,见 subagent.rs)。
//! 签名即锁:改动必须同步 mod.rs 的导出与全部调用方。

// 所有符号经 super::* 平铺(mod.rs 的 use 与 pub use 子模块)。
use super::*;

mod question;
mod tool_catalog;
use tool_catalog::append_deferred_process_hint;
use tool_catalog::{auto_load_deferred_tool, run_tool_search, tool_spec};
mod history;
use history::{
    commit_step_messages, commit_tool_results, record_round_message, StepMessageOutcome,
};

mod task_results;
use task_results::{tool_result_part, tool_result_part_with_images};
mod permissions;
mod serial_tools;
use serial_tools::{execute_serial_tool_calls, SerialToolRequest};
mod parallel_tools;
use parallel_tools::{
    can_parallel_tool_calls, execute_parallel_tool_calls, ParallelPreflight, ParallelToolRequest,
};
mod assembly;
use assembly::{assemble_run_once, RunOnceAssembly};
mod context_budget;
use context_budget::enforce_context_budget;
mod halt;
use halt::{append_halted_tool_results, halt_signalled};
mod batch;

/// R-183:命中规则的展示原文,用于 PermissionResolved.rule 轨迹(验收④)。
fn describe_rule(rule: &kanzei_harness::permission::Rule) -> String {
    format!("{} `{}` => {:?}", rule.action, rule.resource, rule.effect)
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
            mut specs,
            mut context_report,
            stable_system,
            mut refreshable_baseline,
            mut messages,
            mut total_usage,
            mut last_input_tokens,
            mut last_estimated_tokens,
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
            prior,
            initial_parts,
            subagent,
        );
        recall = recall.with_run_id(ctx.run_id.clone());
        // D-655:消息历史会在轮中被压缩/裁剪,不能再用 prior.len() 反推本轮。
        // 事件提交前复制本轮消息,作为不受结构性删短影响的统计真源。
        let round_messages = std::sync::Arc::new(std::sync::Mutex::new(
            messages.last().cloned().into_iter().collect::<Vec<_>>(),
        ));
        let round_messages_for_events = std::sync::Arc::clone(&round_messages);
        let outer_on_event = on_event;
        let mut on_event = move |event: RunEvent| {
            let pending_round = match &event {
                RunEvent::AssistantMessageCommitted {
                    step,
                    message,
                    commit,
                } => Some(RunEvent::AssistantMessageCommitted {
                    step: *step,
                    message: message.clone(),
                    commit: commit.clone(),
                }),
                RunEvent::ToolResultsCommitted {
                    step,
                    message,
                    commit,
                } => Some(RunEvent::ToolResultsCommitted {
                    step: *step,
                    message: message.clone(),
                    commit: commit.clone(),
                }),
                RunEvent::InputMessageCommitted { input_id, message } => {
                    Some(RunEvent::InputMessageCommitted {
                        input_id: input_id.clone(),
                        message: message.clone(),
                    })
                }
                _ => None,
            };
            outer_on_event(event);
            if let Some(pending_round) = pending_round {
                record_round_message(
                    &mut round_messages_for_events.lock().unwrap(),
                    &pending_round,
                );
            }
        };
        let halt = config.halt.as_ref();
        let halted = || halt.is_some_and(|token| token.is_cancelled());
        // 收尾落盘一旦进入就保持到本轮结束——延长的每一步都要重复这条指令,
        // 只在授予的那一步说一次,模型下一步就忘了自己在收尾。
        let mut winddown_until_end = false;
        // 已授予的延长步数,**跨步保持**。
        //
        // `budget_extension` 每步新建,而授予判据是一次性的(`extension_used`):
        // 授予后的下一步再问,拿到的是 0。若每步都用当步信号重算上限,授予当步
        // 之后 `effective_max_steps` 会掉回 `max_steps`,而 `step` 已经越过它,
        // `step == effective_max_steps` 永远不再成立——步数上限就此失效,整轮只能
        // 靠模型自己停止调用工具才会结束。收尾提交的形状(只差一次 commit 就收手)
        // 掩盖了这一点;收尾落盘的触发面宽得多,不修就会真的跑出无上限的轮。
        let mut granted_extension: u32 = 0;
        let mut item_context = super::item_context::ItemContext::default();
        let mut batch = batch::Batch::new(
            ctx.project_workflow
                && config.intensity == kanzei_harness::HarnessIntensity::Autonomous
                && max_steps == 0
                && tools
                    .iter()
                    .any(|tool| matches!(tool.name(), "edit" | "write" | "insert")),
        );
        loop {
            step += 1;
            // D-342 步首检查点:停止已置位就不再发起新的 provider 请求,以 halted
            // 正常收尾——messages 完整交还,调用方照常走轮末写回。
            if halted() {
                if let Some(host) = subagent.and_then(|rt| rt.options.host.as_ref()) {
                    host.stop_all();
                }
                recall.finish(RecallRunOutcome::Halted);
                return Ok(RunSummary {
                    text: final_text,
                    usage: total_usage,
                    last_input_tokens,
                    steps: step.saturating_sub(1),
                    halted_by_user: true,
                    step_limit_reached: false,
                    messages,
                    context_report: context_report.clone(),
                    overflow_traces: overflow_traces.clone(),
                    round_messages: round_messages.lock().unwrap().clone(),
                });
            }
            // Deliver new input only after the preceding tool/result batch.
            if let Some(inbox) = &ctx.input_inbox {
                for input in inbox.take().map_err(anyhow::Error::msg)? {
                    let message = Message::user_text(input.text);
                    on_event(RunEvent::InputMessageCommitted {
                        input_id: input.id,
                        message: message.clone(),
                    });
                    messages.push(message);
                }
            }
            // R-184:动态源只替换本步 system 段，不累积进 messages。
            if step > 1 {
                refreshable_baseline = snapshot.refreshable_system_baseline_with_report().0;
            }
            let mut system = stable_system.clone();
            if !refreshable_baseline.trim().is_empty() {
                system.push(refreshable_baseline.clone());
            }
            let budget_extension = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            let winddown_signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            on_event(RunEvent::TurnStart {
                step,
                max_steps,
                budget_extension: budget_extension.clone(),
                winddown: winddown_signal.clone(),
            });
            granted_extension = granted_extension.max(
                budget_extension
                    .load(std::sync::atomic::Ordering::Relaxed)
                    .min(2),
            );
            // 收尾落盘信号一旦置位就保持到本轮结束——延长的每一步都要重复
            // 这条指令,只在授予的那一步说一次,模型下一步就忘了自己在收尾。
            if winddown_signal.load(std::sync::atomic::Ordering::Relaxed) {
                winddown_until_end = true;
            }
            let last_step = reached_step_ceiling(step, max_steps, granted_extension);
            // 显式预算和 task 默认预算参与收敛；主代理的 0 保持无步数截断。
            // 到达最后一步时收走工具并要求模型用文本收敛，防止工具/模型循环无限延长。
            let budget_checkpoint = is_budget_checkpoint(step);
            let winddown = winddown_until_end;
            let batch_prompt = batch.prepare(step, ctx);

            // 完整工具组已提交后、下一次请求前整理条目边界；独立于 token 超线。
            item_context.prepare(&mut messages, ctx, &mut on_event);

            // R-202 批6:轮内上下文预算(主动 prune/压缩/trim,含 D-206 无效计数
            // 与 R-219 保守默认 32k)抽离为 enforce_context_budget。
            enforce_context_budget(
                client,
                subagent,
                config,
                &ctx.project_root,
                &system,
                &specs,
                route.kind,
                &mut messages,
                last_input_tokens,
                last_estimated_tokens,
                calibration,
                &mut futile_compactions,
                &mut overflow_traces,
                &mut on_event,
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
                winddown,
                batch_prompt.as_deref(),
                halt,
                &mut on_event,
                &mut calibration,
                &mut last_input_tokens,
                &mut last_estimated_tokens,
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
                    recall.finish(RecallRunOutcome::Halted);
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        last_input_tokens,
                        steps: step,
                        halted_by_user: true,
                        step_limit_reached: false,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
                        round_messages: round_messages.lock().unwrap().clone(),
                    });
                }
            };

            // R-202 批6:步骤消息提交与纯文本/停止收尾抽离为 commit_step_messages;
            // 提前返回(calls 空 / 停止占位)以 StepMessageOutcome::Return 表达。
            if let Some(host) = subagent.and_then(|rt| rt.options.host.as_ref()) {
                host.set_parent_context(&messages);
            }
            match commit_step_messages(
                parts,
                &calls,
                &mut final_text,
                &mut messages,
                step,
                halt,
                &mut on_event,
            )? {
                StepMessageOutcome::Proceed => {}
                StepMessageOutcome::Return { mut halted_by_user } => {
                    if let Some(host) = subagent.and_then(|rt| rt.options.host.as_ref()) {
                        if halted_by_user {
                            host.stop_all();
                        } else if ctx.async_mailbox.is_none() && host.has_updates() {
                            let collection = kanzei_harness::tool_pipeline::wrap_execute(
                                String::new(),
                                None,
                                None,
                                host.execute(String::new(), serde_json::json!({"action":"wait"})),
                            );
                            let result = if let Some(token) = halt {
                                tokio::select! {
                                    output = collection => Some(output),
                                    _ = token.cancelled() => { host.stop_all(); halted_by_user = true; None }
                                }
                            } else {
                                Some(collection.await)
                            };
                            if let Some(result) = result {
                                messages.push(Message::user_text(format!("子任务实际结果（任务返回不等于验收或交付）：\n{}\n请检查结果，处理失败，必要时用 task diff/adopt 整合改动并运行验证，再向用户报告。不要仅复述已派发。",result.content)));
                                continue;
                            }
                        }
                    }
                    if !halted_by_user && batch.is_closing() {
                        if let Some(checkpoint) = batch.finish_and_resume(step, ctx) {
                            messages.push(Message::user_text(checkpoint));
                            continue;
                        }
                    }
                    recall.finish(if halted_by_user {
                        RecallRunOutcome::Halted
                    } else {
                        RecallRunOutcome::Completed
                    });
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        last_input_tokens,
                        steps: step,
                        halted_by_user,
                        step_limit_reached: !halted_by_user && last_step,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
                        round_messages: round_messages.lock().unwrap().clone(),
                    });
                }
            }

            // R-202 批4:task 子代理段(并行/后台两种派发 + 溢出上限 + 取消占位补齐)
            // 抽离为 run_subagent_calls,返回本步 task 结果表。
            let mut task_results: std::collections::HashMap<String, kanzei_harness::ToolOutput> =
                run_subagent_calls(
                    if batch.is_closing() { None } else { subagent },
                    client,
                    ctx,
                    config,
                    &calls,
                    halt,
                    line_runtime,
                    &mut on_event,
                )
                .await;

            // R-202 批5:普通工具执行段(并行预检 + wave/串行两条路径)抽离为
            // execute_tool_calls。提前退出(串行路径停止 / 权限用户拒绝)以
            // ToolRunOutcome::Stopped 表达,由调用方构造 halted RunSummary。
            let images_supported = route.supports_images();
            batch.before_calls(&calls, ctx);
            let tool_outcome = execute_tool_calls(
                config,
                ctx,
                snapshot,
                &mut specs,
                &mut context_report,
                &tools,
                &calls,
                subagent,
                &mut task_results,
                images_supported,
                halt,
                &mut on_event,
                ask,
                &mut session_approved,
                &mut session_rules,
                &mut messages,
                step,
                &batch,
            )
            .await?;
            let (mut results, pending_images) = match tool_outcome {
                ToolRunOutcome::Results {
                    results,
                    pending_images,
                } => (results, pending_images),
                ToolRunOutcome::Stopped => {
                    recall.finish(RecallRunOutcome::Halted);
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        last_input_tokens,
                        steps: step,
                        halted_by_user: true,
                        step_limit_reached: false,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
                        round_messages: round_messages.lock().unwrap().clone(),
                    });
                }
            };
            batch.observe(&calls, &results, ctx);
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
                &mut on_event,
            )? {
                StepFinalOutcome::Continue => {}
                StepFinalOutcome::Break => break,
                StepFinalOutcome::Return { halted_by_user } => {
                    recall.finish(if halted_by_user {
                        RecallRunOutcome::Halted
                    } else {
                        RecallRunOutcome::Completed
                    });
                    return Ok(RunSummary {
                        text: final_text,
                        usage: total_usage,
                        last_input_tokens,
                        steps: step,
                        halted_by_user,
                        step_limit_reached: !halted_by_user && last_step,
                        messages,
                        context_report: context_report.clone(),
                        overflow_traces: overflow_traces.clone(),
                        round_messages: round_messages.lock().unwrap().clone(),
                    });
                }
            }
        }

        recall.finish(RecallRunOutcome::Completed);
        let round_messages = round_messages.lock().unwrap().clone();
        Ok(RunSummary {
            text: final_text,
            usage: total_usage,
            last_input_tokens,
            steps: step,
            halted_by_user: false,
            step_limit_reached: true,
            messages,
            context_report,
            overflow_traces: overflow_traces.clone(),
            round_messages,
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
    winddown: bool,
    batch_prompt: Option<&str>,
    halt: Option<&CancellationToken>,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
    calibration: &mut f64,
    last_input_tokens: &mut Option<u64>,
    last_estimated_tokens: &mut Option<u64>,
    total_usage: &mut Usage,
    overflow_recoveries: &mut u32,
    overflow_traces: &mut Vec<String>,
) -> anyhow::Result<StepOutcome> {
    let mut stream_restarts: u32 = 0;
    loop {
        // 每次恢复都会改写 messages，请求必须在重试循环内重建；否则即使压缩了
        // 内存历史，发给 provider 的仍会是第一次克隆出的超长请求。
        let mut request_messages = messages.clone();
        if let Some(prompt) = batch_prompt {
            request_messages.push(Message::user_text(prompt));
        }
        // 最后一步收走工具强制收敛;必须同时明确告知(D-027:只收走不告知,
        // codex 仍试图调用工具,把调用 JSON 当纯文本狂喷并在思考里反复自我纠正)。
        if last_step {
            // 这一步没有工具,所以这段文字**改变不了任何持久状态**:自动续跑的判定
            // (AutoRunCtx)里根本没有文本字段,下一轮也不读它。凡是没写进 tracker /
            // 提交 / 测试记录的东西,写在这里等于丢掉。
            //
            // 所以措辞必须堵住"拿报告当落盘替代品"这条路——实测形态是模型在这里
            // 写下「D-044/045/046 已修复待关闭」,而条目里一个字没有,下一轮从零重建。
            // 同时限长:它只是给人看的摘要,不是记录,没必要复述一遍 tracker。
            request_messages.push(Message::user_text(
                "(system) Final step of this run: tools are no longer available. Do NOT \
                 attempt any tool call and do NOT emit JSON — reply in plain text only. \
                 This text is a DIGEST FOR A HUMAN, not a record: nothing you write here \
                 is read by the next round, and anything not already in the tracker, a \
                 commit, or a test record is lost. Do not restate the tracker. At most 5 \
                 lines, no nested bullets: what changed, what is left, and — if anything \
                 that matters failed to reach the tracker — say exactly what and why.",
            ));
        } else if winddown {
            // 步数预算已经吃完,本轮改了源码却一次都没提交。再多写一行代码
            // 都会随进程结束蒸发,下一轮还得靠 git status / diff / 重读把现场拼回来。
            // 这几步的唯一任务是把半个事务落成可恢复的检查点。
            request_messages.push(Message::user_text(
                "(system) Wind-down: the step budget for this run is exhausted and you have \
                 edited source without committing. Do NOT start or continue any implementation \
                 work — anything you write now will be lost. Use the remaining steps ONLY to \
                 make the current state durable and resumable: run the relevant test/build if \
                 one is cheap and already set up, stage and commit the work in progress with an \
                 honest message about what is and is not done, and write the real stopping point \
                 into the tracker entry's progress field (which files changed, what remains, what \
                 the next concrete step is). If committing is not possible, say precisely why in \
                 plain text.",
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
        let last_estimated = estimate_prompt_tokens_for_protocol(
            system,
            &request_messages,
            req_tools,
            Some(route.kind),
        );
        let request = LlmRequest {
            model: config.model.clone(),
            system: system.to_vec(),
            messages: request_messages,
            tools: req_tools.to_vec(),
            hosted_tools: vec![],
            max_tokens: config.max_tokens,
            temperature: None,
            reasoning: config.reasoning,
            service_tier: config.service_tier.clone(),
        };
        // Stop can win while the inbox or context maintenance runs after the
        // step-start check. Never poll the provider first when already stopped.
        let opened = tokio::select! {
            biased;
            _ = halt_signalled(halt) => return Ok(StepOutcome::Stopped),
            result = client
            .stream_with_retry_notice_with_limits(
                route,
                &request,
                config.limits.transport_retries(),
                config.limits.rate_limit_retries(),
                |attempt, delay| {
                    on_event(RunEvent::Retry {
                        attempt,
                        max: config
                            .limits
                            .transport_retries()
                            .max(config.limits.rate_limit_retries()),
                        delay_ms: delay.as_millis(),
                    });
                },
            )
            => result,
        };
        let mut stream = match opened {
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
        let mut hosted_calls: BTreeMap<usize, (String, String)> = BTreeMap::new();

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
                LlmEvent::HostedStart {
                    index,
                    id,
                    name,
                    channel,
                    protocol,
                } => {
                    hosted_calls.insert(index, (id.clone(), name.clone()));
                    on_event(RunEvent::HostedTool {
                        id,
                        name,
                        kind: "web_search_call".into(),
                        status: "running".into(),
                        detail: serde_json::json!({"channel":channel,"protocol":protocol}),
                    });
                }
                LlmEvent::HostedItem {
                    index,
                    channel,
                    protocol,
                    kind,
                    raw,
                } => {
                    // Opaque thinking is replay state, not a provider tool invocation.
                    if kind == "redacted_thinking" {
                        parts.push(Part::Hosted {
                            channel,
                            protocol,
                            kind,
                            raw,
                        });
                        continue;
                    }
                    let (id, name) = if kind == "server_tool_use" {
                        hosted_calls
                            .get(&index)
                            .cloned()
                            .unwrap_or_else(|| (format!("hosted-{index}"), "web_search".into()))
                    } else if kind == "web_search_call" {
                        hosted_calls
                            .remove(&index)
                            .unwrap_or_else(|| (format!("hosted-{index}"), "web_search".into()))
                    } else {
                        (format!("hosted-{kind}-{index}"), "web_search".into())
                    };
                    let detail = match kind.as_str() {
                        "web_search_call" => serde_json::json!({
                            "status":raw["status"],
                            "action":raw["action"],
                        }),
                        "citations" => raw.clone(),
                        _ => serde_json::json!({"type":raw["type"],"status":raw["status"]}),
                    };
                    parts.push(Part::Hosted {
                        channel,
                        protocol,
                        kind: kind.clone(),
                        raw,
                    });
                    on_event(RunEvent::HostedTool {
                        id,
                        name,
                        kind,
                        status: "completed".into(),
                        detail,
                    });
                }
                LlmEvent::StepFinish { usage, reason } => {
                    *calibration = update_calibration(*calibration, last_estimated, usage.input);
                    if usage.input > 0 {
                        *last_input_tokens = Some(usage.input);
                        *last_estimated_tokens = Some(last_estimated);
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
/// - 溢出数量上限(max_tasks_per_turn)的调用以 failed(subagent_limit) 即时回喂。
/// - 前台模式被整轮停止时,缺席的 task 补取消占位并补发 ToolEnd(subagent_cancelled)。
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
            if !rt.options.mode.enabled() {
                for (id, input, raw) in &task_calls {
                    on_event(RunEvent::ToolStart {
                        id: id.clone(),
                        name: "task".into(),
                        summary: summarize_input(input, raw),
                        input: input.clone(),
                    });
                    let output = kanzei_harness::ToolOutput::failed(
                        "subagent_disabled",
                        "Subagents are disabled for this conversation",
                    );
                    on_event(RunEvent::tool_end(id.clone(), "task".into(), &output));
                    task_results.insert(id.clone(), output);
                }
                return task_results;
            }
            let max_tasks = config.limits.max_tasks_per_turn();
            let dispatch_slots = rt.options.host.is_none().then(|| {
                std::sync::Arc::new(tokio::sync::Semaphore::new(
                    rt.options
                        .mode
                        .concurrency(&config.limits)
                        .max(1)
                        .min(config.limits.subagent_global_concurrency()),
                ))
            });
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
                // UI-0926 #8:稳定码 subagent_limit(UI 判「未启动」),文案不变。
                let output = kanzei_harness::ToolOutput::failed(
                    "subagent_limit",
                    format!(
                        "too many parallel subagent tasks; maximum per turn is {}",
                        max_tasks
                    ),
                );
                on_event(RunEvent::tool_end(id.clone(), "task".into(), &output));
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
                    let dispatch_slots = dispatch_slots.clone();
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
                        let _permit = match dispatch_slots {
                            Some(slots) => slots.acquire_owned().await.ok(),
                            None => None,
                        };
                        let mut output =
                            run_subagent(&client, &rt, &ctx, &call_id, &input, tx).await;
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
                        let dispatch_slots = dispatch_slots.clone();
                        async move {
                            let _permit = match dispatch_slots {
                                Some(slots) => slots.acquire_owned().await.ok(),
                                None => None,
                            };
                            let output = run_subagent(client, rt, ctx, id, input, tx).await;
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
                                on_event(RunEvent::tool_end(
                                    id.clone(),
                                    "task".into(),
                                    &output,
                                ));
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
                // 已有终态,不命中,零行为差异。
                // UI-0926 #8:补占位的同时补发 ToolEnd(code=subagent_cancelled)。
                // 原先只补历史不发事件,界面上这些子代理永远停在「运行中」;
                // 事件经 on_event 走与真实终态同一条投影链(UI/轨迹/指标)。
                for (id, _, _) in &task_calls {
                    if task_results.contains_key(id) {
                        continue;
                    }
                    let output = kanzei_harness::ToolOutput::failed(
                        "subagent_cancelled",
                        "cancelled: run stopped by user",
                    );
                    on_event(RunEvent::tool_end(id.clone(), "task".into(), &output));
                    task_results.insert(id.clone(), output);
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
    specs: &mut Vec<ToolSpec>,
    context_report: &mut Vec<(String, usize)>,
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
    batch: &batch::Batch,
) -> anyhow::Result<ToolRunOutcome> {
    for (_, name, _, _) in calls {
        auto_load_deferred_tool(snapshot, name, specs, context_report);
    }
    // R-171 批2:writer 阶段(ReadWriteSerial)强制普通工具串行——
    // max in-flight=1 且结果按模型调用顺序归位(验收③)。设计文档不变量 5。
    let serial_writer = config.execution_policy.is_serial_writer();
    // R-097 批一：权限询问仍按旧路径串行处理(R-086 承接询问路由)；当本批
    // 不需要新 ask 时，普通工具按显式并发契约切成确定性 wave 并发执行。
    let can_parallel_tools = can_parallel_tool_calls(ParallelPreflight {
        ctx,
        snapshot,
        tools,
        calls,
        has_subagent: subagent.is_some(),
        session_approved,
        session_rules,
        force_serial: serial_writer || batch.is_closing(),
    });

    // 并行 wave 与串行路径共用同一对齐约定；并行细节已迁移到 parallel_tools.rs。
    let (results, pending_images) = if can_parallel_tools {
        execute_parallel_tool_calls(ParallelToolRequest {
            config,
            ctx,
            snapshot,
            tools,
            calls,
            subagent,
            task_results,
            images_supported,
            halt,
            on_event,
        })
        .await
    } else {
        return execute_serial_tool_calls(SerialToolRequest {
            config,
            ctx,
            snapshot,
            specs,
            context_report,
            tools,
            calls,
            subagent,
            task_results,
            images_supported,
            halt,
            on_event,
            ask,
            session_approved,
            session_rules,
            messages,
            step,
            pending_images: &mut Vec::new(),
            batch,
        })
        .await;
    };
    Ok(ToolRunOutcome::Results {
        results,
        pending_images,
    })
}
/// 本步是否已到达(或越过)含延长在内的步数上限。
///
/// `>=` 而不是 `==`:相等判据只要被跨过一次就再也不成立,把"上限"变成一个可以
/// 错过的点。配合调用方跨步保持的 `granted_extension`,授予延长之后上限仍然成立。
pub(crate) fn reached_step_ceiling(step: u32, max_steps: u32, granted_extension: u32) -> bool {
    let effective = max_steps.saturating_add(granted_extension);
    effective > 0 && step >= effective
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
) -> anyhow::Result<StepFinalOutcome> {
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
    )?;

    // D-342 步末检查点:本步工具已全部有终态(真实或取消占位),停止在此
    // 收尾——配对完整,下一轮 prior 无孤儿。并行 wave 被停止打断的路径
    // 从这里返回。
    if halt.is_some_and(|token| token.is_cancelled()) {
        return Ok(StepFinalOutcome::Return {
            halted_by_user: true,
        });
    }
    if matches!(finish, FinishReason::MaxTokens | FinishReason::Refusal) {
        return Ok(StepFinalOutcome::Return {
            halted_by_user: false,
        });
    }
    if last_step {
        return Ok(StepFinalOutcome::Break);
    }
    Ok(StepFinalOutcome::Continue)
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

    /// D-655:轮中压缩只改可发送的 history,事件提交形成的本轮真源仍完整。
    /// 同一份真源同时供 tools/metrics/failure 三种轮末统计,避免各调用点各自切片。
    #[test]
    fn 本轮真源不随压缩删短_history_统计仍只含本轮() {
        let current_call = Message::assistant(vec![Part::ToolCall {
            id: "current-1".into(),
            name: "edit".into(),
            input: json!({ "path": "current.rs" }),
        }]);
        let current_result = Message::tool_results(vec![Part::ToolResult {
            call_id: "current-1".into(),
            content: "old_string not found in current.rs".into(),
            is_error: true,
        }]);
        let mut round_messages = vec![Message::user_text("本轮指令")];
        record_round_message(
            &mut round_messages,
            &RunEvent::AssistantMessageCommitted {
                step: 1,
                message: current_call,
                commit: Default::default(),
            },
        );
        record_round_message(
            &mut round_messages,
            &RunEvent::ToolResultsCommitted {
                step: 1,
                message: current_result,
                commit: Default::default(),
            },
        );
        record_round_message(
            &mut round_messages,
            &RunEvent::AssistantMessageCommitted {
                step: 2,
                commit: Default::default(),
                message: Message::assistant(vec![Part::ToolCall {
                    id: "current-2".into(),
                    name: "edit".into(),
                    input: json!({ "path": "current.rs" }),
                }]),
            },
        );
        record_round_message(
            &mut round_messages,
            &RunEvent::ToolResultsCommitted {
                step: 2,
                commit: Default::default(),
                message: Message::tool_results(vec![Part::ToolResult {
                    call_id: "current-2".into(),
                    content: "old_string not found in current.rs".into(),
                    is_error: true,
                }]),
            },
        );

        // 模拟压缩把 prior 与本轮消息在可发送 history 中删短;真源不参与该修改。
        let mut compacted_history = vec![Message::user_text("压缩后的纪要")];
        compacted_history.truncate(1);
        assert_eq!(summarize_tools(&round_messages).get("edit"), Some(&2));
        assert_eq!(summarize_metrics(&round_messages).total_calls, 2);
        assert_eq!(summarize_failures(&round_messages).len(), 1);
        assert!(summarize_tools(&compacted_history).is_empty());
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
            outcome.unwrap(),
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
        assert!(matches!(outcome.unwrap(), StepMessageOutcome::Proceed));
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
            outcome.unwrap(),
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
        assert!(matches!(outcome.unwrap(), StepFinalOutcome::Continue));
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
            outcome.unwrap(),
            StepFinalOutcome::Return {
                halted_by_user: false
            }
        ));
    }

    /// 授予收尾延长之后,步数上限必须仍然成立。
    ///
    /// 反例形态:授予判据是一次性的,授予当步之后再问拿到的是 0;若每步都用当步
    /// 信号重算上限并按 `step == effective` 判定,`step` 已越过 `max_steps`,相等
    /// 永远不再成立——整轮只能靠模型自己停手才结束。收尾提交的形状(只差一次
    /// commit 就收手)掩盖了它;收尾落盘的触发面宽得多,不修就会跑出无上限的轮。
    #[test]
    fn 授予延长后步数上限仍然成立() {
        // 无延长:到点即收敛。
        assert!(!reached_step_ceiling(31, 32, 0));
        assert!(reached_step_ceiling(32, 32, 0));

        // 授予 2 步:延长期内不收敛,到新上限收敛。
        assert!(!reached_step_ceiling(33, 32, 2));
        assert!(reached_step_ceiling(34, 32, 2));

        // 关键回归:即使延长额度丢失(退回 0),越过上限的步也必须收敛,
        // 而不是因为"不相等"一路跑下去。
        assert!(reached_step_ceiling(33, 32, 0));
        assert!(reached_step_ceiling(99, 32, 0));

        // max_steps=0 表示无上限,任何步都不收敛。
        assert!(!reached_step_ceiling(9999, 0, 0));
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
        assert!(matches!(outcome.unwrap(), StepFinalOutcome::Break));
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
            outcome.unwrap(),
            StepFinalOutcome::Return {
                halted_by_user: true
            }
        ));
    }
}

#[cfg(test)]
mod commit_ack_tests {
    use super::*;
    use kanzei_harness::{
        rule, Component, Effect, Harness, HarnessDraft, KanzeiConfig, ProfileKind, ResolveCtx,
    };
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct CountingTool(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl Tool for CountingTool {
        fn name(&self) -> &'static str {
            "counting_probe"
        }
        fn description(&self) -> String {
            "Count actual executions without external effects".into()
        }
        fn input_schema(&self) -> Value {
            json!({"type":"object","properties":{}})
        }
        async fn execute(&self, _input: Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
            self.0.fetch_add(1, Ordering::SeqCst);
            kanzei_harness::ToolOutput::ok("executed once")
        }
    }

    struct ProbeComponent(Arc<AtomicUsize>);

    impl Component for ProbeComponent {
        fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
            draft
                .tools
                .insert("counting_probe", Arc::new(CountingTool(self.0.clone())));
            draft
                .permissions
                .push(rule("counting_probe", "*", Effect::Allow));
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    enum FailurePoint {
        RecoveredAssistant,
        ToolResultSql,
        None,
        Stateless,
    }

    async fn respond(listener: &TcpListener, body: Value, requests: &AtomicUsize) {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        let header_end = loop {
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
            if let Some(pos) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let length = String::from_utf8_lossy(&request[..header_end])
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while request.len() < header_end + length {
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
        }
        requests.fetch_add(1, Ordering::SeqCst);
        let data = format!("data: {body}\n\ndata: [DONE]\n\n");
        let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        data.len()
    );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(data.as_bytes()).await.unwrap();
    }

    async fn run_real_commit_fixture(point: FailurePoint) {
        let root = std::env::temp_dir().join(format!(
            "kz-c3-commit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.db");
        let store = crate::SessionStore::open(&path).unwrap();
        store
            .create_session("ses", root.to_str().unwrap(), None)
            .unwrap();
        if matches!(point, FailurePoint::ToolResultSql) {
            rusqlite::Connection::open(&path)
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER reject_result BEFORE INSERT ON session_events
             WHEN NEW.event_type = 'session.tool_result_committed'
             BEGIN SELECT RAISE(ABORT, 'fixture rejects durable result'); END;",
                )
                .unwrap();
        }
        let executions = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let mut harness = Harness::default();
        harness.add(ProbeComponent(executions.clone()));
        let snapshot = harness
            .resolve(&ResolveCtx {
                profile: ProfileKind::Dev,
                cwd: root.clone(),
                project_root: root.clone(),
                config: Arc::new(KanzeiConfig::default()),
            })
            .unwrap();
        let agent: AgentDef = serde_json::from_value(json!({
            "name":"fixture", "profile":"dev", "mode":"primary", "steps":3, "system":"test"
        }))
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_requests = requests.clone();
        let server = tokio::spawn(async move {
            respond(
                &listener,
                json!({"choices":[{
            "index":0,
            "delta":{"tool_calls":[{"index":0,"id":"call","type":"function",
                "function":{"name":"counting_probe","arguments":"{}"}}]},
            "finish_reason":"tool_calls"
        }],"usage":{"prompt_tokens":1,"completion_tokens":1}}),
                &server_requests,
            )
            .await;
            // Old code really asks for this second step. Fixed failure code ends before it.
            respond(
                &listener,
                json!({"choices":[{
            "index":0,"delta":{"content":"done"},"finish_reason":"stop"
        }],"usage":{"prompt_tokens":1,"completion_tokens":1}}),
                &server_requests,
            )
            .await;
        });
        let client = LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
        let route = Route::openai_at(&format!("http://{address}/v1"), None);
        let config = RunnerConfig {
            hosted_tools: vec![],
            digest_model: None,
            intensity: kanzei_harness::HarnessIntensity::Autonomous,
            model: "mock".into(),
            max_tokens: 128,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: None,
            context_limit: None,
            limits: Default::default(),
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
            ask_policy: AskPolicy::NonInteractive,
            halt: None,
        };
        let ctx = ToolCtx::new(root.clone(), root.clone()).with_session_id("ses".into());
        let mut writer = crate::TypedSessionWriter::new(&path, "ses", "turn");
        if !matches!(point, FailurePoint::Stateless) {
            writer.user_message("input", Message::user_text("run probe"));
        }
        let mut recovered = false;
        let outcome =
            {
                let mut sink =
                    |event| match event {
                        _ if matches!(point, FailurePoint::Stateless) => {}
                        RunEvent::TurnStart {
                            step, max_steps, ..
                        } => {
                            writer.turn_started(step, max_steps);
                            if matches!(point, FailurePoint::RecoveredAssistant) && !recovered {
                                // Real independent connection/prepare recovery closes this still-live turn.
                                let recovery_store = crate::SessionStore::open(&path).unwrap();
                                crate::prepare_typed_session(&recovery_store, "ses").unwrap();
                                recovered = true;
                            }
                        }
                        RunEvent::Text(text) => writer.push_text(&text),
                        RunEvent::AssistantMessageCommitted {
                            step,
                            message,
                            commit,
                        } => {
                            let persisted = writer.assistant_committed(step, message);
                            if !persisted {
                                commit.reject(
                                    writer.errors().last().cloned().unwrap_or_else(|| {
                                        "typed assistant commit rejected".into()
                                    }),
                                );
                            }
                        }
                        RunEvent::ToolResultsCommitted {
                            step,
                            message,
                            commit,
                        } => {
                            let persisted = writer.tool_results_committed(step, message);
                            if !persisted {
                                commit.reject(
                                    writer.errors().last().cloned().unwrap_or_else(|| {
                                        "typed tool result commit rejected".into()
                                    }),
                                );
                            }
                        }
                        _ => {}
                    };
                let mut ask = |_| -> AskFuture {
                    Box::pin(async { AskResponse::Permission(AskReply::Deny) })
                };
                tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    run_once(
                        &client,
                        &route,
                        &snapshot,
                        &agent,
                        &config,
                        &ctx,
                        "run probe",
                        None,
                        &[],
                        None,
                        None,
                        &mut sink,
                        &mut ask,
                    ),
                )
                .await
                .unwrap()
            };
        server.abort();
        let _ = server.await;
        let facts = store.list_session_facts("ses").unwrap();
        let durable_calls = facts
            .iter()
            .filter(|(_, fact)| matches!(fact.fact, crate::SessionFact::ToolCalled { .. }))
            .count();
        let durable_results = facts
            .iter()
            .filter(|(_, fact)| matches!(fact.fact, crate::SessionFact::ToolResultCommitted { .. }))
            .count();
        let actual_executions = executions.load(Ordering::SeqCst);
        let actual_requests = requests.load(Ordering::SeqCst);
        let typed_errors = writer.errors().to_vec();
        drop(writer);
        drop(store);
        // Cleanup precedes assertions so the deliberate old-code negative does not leak fixtures.
        std::fs::remove_dir_all(root).unwrap();
        match point {
            FailurePoint::RecoveredAssistant => {
                assert!(outcome.is_err(), "rejected commit must fail the runner; requests={actual_requests}, effects={actual_executions}, durable_calls={durable_calls}, durable_results={durable_results}");
                assert!(!typed_errors.is_empty());
                assert_eq!((durable_calls, durable_results), (0, 0));
                assert_eq!(
                    actual_requests, 1,
                    "no next provider request after rejection"
                );
                assert_eq!(actual_executions, 0, "no tool before durable ToolCalled");
            }
            FailurePoint::ToolResultSql => {
                assert!(
                    outcome.is_err(),
                    "failed durable result must fail the runner; requests={actual_requests}, effects={actual_executions}, durable_calls={durable_calls}, durable_results={durable_results}"
                );
                assert!(!typed_errors.is_empty());
                assert_eq!((durable_calls, durable_results), (1, 0));
                assert_eq!(
                    actual_requests, 1,
                    "do not feed an uncommitted result to another step"
                );
                assert_eq!(
                    actual_executions, 1,
                    "never repeat an already executed tool"
                );
            }
            FailurePoint::None => {
                let summary = outcome.unwrap();
                assert!(typed_errors.is_empty());
                assert_eq!((durable_calls, durable_results), (1, 1));
                assert_eq!((actual_requests, actual_executions), (2, 1));
                assert_eq!(summary.round_messages, summary.messages);
            }
            FailurePoint::Stateless => {
                let summary = outcome.unwrap();
                assert!(typed_errors.is_empty());
                assert_eq!((durable_calls, durable_results), (0, 0));
                assert_eq!((actual_requests, actual_executions), (2, 1));
                assert_eq!(summary.round_messages, summary.messages);
            }
        }
    }

    #[tokio::test]
    async fn recovered_turn_rejects_tool_declaration_before_actual_execution() {
        run_real_commit_fixture(FailurePoint::RecoveredAssistant).await;
    }
    #[tokio::test]
    async fn failed_tool_result_commit_stops_before_next_provider_request() {
        run_real_commit_fixture(FailurePoint::ToolResultSql).await;
    }
    #[tokio::test]
    async fn successful_durable_commit_keeps_actual_tool_and_round_receipts() {
        run_real_commit_fixture(FailurePoint::None).await;
    }

    #[tokio::test]
    async fn stateless_reader_runs_tools_without_a_durable_ack_consumer() {
        run_real_commit_fixture(FailurePoint::Stateless).await;
    }
}
