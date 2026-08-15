//! 工具执行域(R-155 B6):wave 构建与并行执行(PreparedToolCall/
//! build_tool_execution_waves_with/execute_prepared_tools)、权限拒绝占位
//! (append_declined_tool_results)、Gate 门禁与并发上限常量。
//! PreparedToolCall 六字段提 pub(super)(测试按字面量构造)。

use futures::StreamExt;
use kanzei_harness::{tool::repair_hint, Effect, HarnessSnapshot, Tool, ToolConcurrency, ToolCtx};
use kanzei_llm::{Message, Part};
use std::sync::Arc;

use crate::runner::compaction::summarize_input;
use crate::runner::drive::{append_halted_tool_results, commit_tool_results, halt_signalled};
use crate::runner::event::{preview, AskFuture, AskReply, AskRequest, AskResponse, RunEvent};
use crate::runner::{AskPolicy, CancellationToken, RunnerConfig, SubagentRuntime};

/// 同一无冲突 wave 的普通工具并发上限；超过时按原调用顺序切 wave。
/// 测试锚点:生产调用方(execute_prepared_tools 参数)由 drive 层传值。
#[allow(dead_code)]
pub const MAX_PARALLEL_TOOLS_PER_WAVE: usize = 8;
pub(crate) fn append_declined_tool_results(
    results: &mut Vec<Part>,
    calls: &[(String, String, serde_json::Value, String)],
    declined_index: usize,
) {
    for (index, (id, _, _, _)) in calls.iter().enumerate().skip(declined_index) {
        let content = if index == declined_index {
            "permission request declined by user"
        } else {
            "tool call cancelled because a previous permission request was declined"
        };
        results.push(Part::ToolResult {
            call_id: id.clone(),
            content: content.into(),
            is_error: true,
        });
    }
}

pub(crate) struct PreparedToolCall {
    pub(super) index: usize,
    pub(super) id: String,
    pub(super) name: String,
    pub(super) input: serde_json::Value,
    pub(super) tool: Arc<dyn Tool>,
    pub(super) concurrency: ToolConcurrency,
}

pub(crate) fn build_tool_execution_waves_with(
    max_parallel: usize,
    calls: Vec<PreparedToolCall>,
) -> Vec<Vec<PreparedToolCall>> {
    let mut waves = Vec::new();
    let mut current: Vec<PreparedToolCall> = Vec::new();
    for call in calls {
        let conflicts = current
            .iter()
            .any(|other| call.concurrency.conflicts_with(&other.concurrency));
        if !current.is_empty() && (conflicts || current.len() >= max_parallel) {
            waves.push(std::mem::take(&mut current));
        }
        current.push(call);
    }
    if !current.is_empty() {
        waves.push(current);
    }
    waves
}

/// R-249:把 ToolOutput 的图片转成 llm Part;provider 不支持时降级为文本说明。
///
/// 返回 `(图片 Part, 需要追加到工具结果文本的说明)`。
///
/// 并行与串行两条执行路径都要做这层转换,必须共用一份——media_type 口径分叉的话,
/// 同一个工具在两条路径下会给 provider 发出不同的请求体,而这种差异只在其中一条
/// 路径上复现,极难定位。
///
/// **为什么降级要发生在这里,而不是靠 client.rs 那道硬拒绝**:那条会让整个请求
/// 失败,而图片一旦进了 messages 就跟着历史每轮重发——等于一次 read 图片就把这条
/// 对话在该 provider 上永久打死。所以图片必须在进历史**之前**被拦下,并如实告诉
/// 模型它没拿到图,好让它改走别的手段。静默丢弃是最坏的一种:模型会以为自己看过了。
pub(crate) fn tool_images_to_parts(
    output: &kanzei_harness::ToolOutput,
    images_supported: bool,
) -> (Vec<Part>, Option<String>) {
    if output.images.is_empty() {
        return (Vec::new(), None);
    }
    if !images_supported {
        return (
            Vec::new(),
            Some(format!(
                "\n[image not delivered: the active provider does not accept image input; \
                 {} image(s) were dropped. You did NOT see them — do not describe their contents.]",
                output.images.len()
            )),
        );
    }
    let parts = output
        .images
        .iter()
        .map(|image| Part::Image {
            media_type: image.media_type.clone(),
            data: image.data.clone(),
        })
        .collect();
    (parts, None)
}

/// 返回 (下标, ToolResult, 该结果附带的图片 Part)。
///
/// 图片**不能**混进 results 向量:那里 `results[i] ↔ calls[i]` 是硬约定
/// (note_step 的 debug_assert 锁着)。它们由调用方单独收集,统一追加到
/// tool_results 消息尾部——Anthropic 要求 tool_result 块排在 user 消息最前,
/// 所以只能后缀不能前插。
pub(crate) async fn execute_prepared_tools(
    calls: Vec<PreparedToolCall>,
    ctx: &ToolCtx,
    max_parallel: usize,
    images_supported: bool,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> Vec<(usize, Part, Vec<Part>)> {
    let mut results = Vec::new();
    // 进度旁路:每个调用带自己 id 的 ProgressHandle,共用一条通道;
    // 收集循环里边等完成边转发增量输出,UI 才能在长任务执行中看到"跑到哪了"。
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<kanzei_harness::progress::ProgressChunk>();
    for wave in build_tool_execution_waves_with(max_parallel, calls) {
        let mut jobs: futures::stream::FuturesUnordered<_> = wave
            .into_iter()
            .map(|call| {
                let progress_tx = progress_tx.clone();
                async move {
                    let PreparedToolCall {
                        index,
                        id,
                        name,
                        input,
                        tool,
                        concurrency: _,
                    } = call;
                    let handle =
                        kanzei_harness::progress::ProgressHandle::new(id.clone(), progress_tx);
                    // D-174:专用文档工具的执行区间就是它的合法写入窗口,后台守卫
                    // 据此把"专用工具改的"和"后台进程偷改的"分开。非写工具零开销。
                    let output = kanzei_harness::managed_fence::tool_scope(
                        &name,
                        kanzei_harness::progress::scope(handle, tool.execute(input, ctx)),
                    )
                    .await;
                    (index, id, name, output)
                }
            })
            .collect();
        loop {
            tokio::select! {
                // 先清进度再收终态:同一调用的增量输出必须排在它的 ToolEnd 之前。
                biased;
                Some((id, chunk)) = progress_rx.recv() => {
                    on_event(RunEvent::ToolProgress { id, chunk });
                }
                job = jobs.next() => {
                    let Some((index, id, name, output)) = job else { break };
                    while let Ok((pid, chunk)) = progress_rx.try_recv() {
                        on_event(RunEvent::ToolProgress { id: pid, chunk });
                    }
                    on_event(RunEvent::ToolEnd {
                        id: id.clone(),
                        name,
                        ok: !output.is_error,
                        outcome: output.outcome.as_str().into(),
                        code: output.code.map(str::to_owned),
                        preview: preview(&output.content),
                        display: output.display.clone(),
                    });
                    let mut model_content = output.model_content();
                    let (images, dropped_note) =
                        tool_images_to_parts(&output, images_supported);
                    if let Some(note) = dropped_note {
                        model_content.push_str(&note);
                    }
                    results.push((
                        index,
                        Part::ToolResult {
                            call_id: id,
                            content: model_content,
                            is_error: output.is_error,
                        },
                        images,
                    ));
                }
            }
        }
    }
    results.sort_by_key(|(index, _, _)| *index);
    results
}

pub(crate) enum Gate {
    Pass,
    Deny(String),
    NonInteractive(String),
    UserDeclined,
}

/// R-183:命中规则的展示原文,用于 PermissionResolved.rule 轨迹(验收④)。
fn describe_rule(rule: &kanzei_harness::permission::Rule) -> String {
    format!("{} `{}` => {:?}", rule.action, rule.resource, rule.effect)
}

/// R-202 批5:普通工具执行段的产物。
pub(crate) enum ToolRunOutcome {
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
pub(crate) async fn execute_tool_calls(
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
                let question = input
                    .get("question")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                let options = input
                    .get("options")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let default = input
                    .get("default")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let multiple = input
                    .get("multiple")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let output = if question.is_empty() {
                    kanzei_harness::ToolOutput::error("question must not be empty")
                } else if !config.ask_policy.allows_user_prompt() {
                    // 自举/并行线没有稳定的用户或代理间 ASK 通道；把问题转成
                    // 可回喂模型的工具错误，不能等待桌面答复。
                    kanzei_harness::ToolOutput::error(
                        "question unavailable in autonomous/parallel run: this line cannot ask the user; continue with available evidence",
                    )
                } else {
                    match ask(AskRequest::Question {
                        question: question.to_owned(),
                        options,
                        default,
                        multiple,
                    })
                    .await
                    {
                        AskResponse::Answer(answer) => {
                            kanzei_harness::ToolOutput::ok(format!("User answer: {answer}"))
                        }
                        AskResponse::Cancelled => {
                            kanzei_harness::ToolOutput::error("question cancelled by user")
                        }
                        AskResponse::Permission(_) => {
                            kanzei_harness::ToolOutput::error("invalid question response")
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
                        let handle =
                            kanzei_harness::progress::ProgressHandle::new(id.clone(), progress_tx);
                        // D-174:串行路径同样开合法写入窗口。writer 阶段
                        // 走的就是这条,漏掉它等于让专用工具的写入没有窗口
                        // 可归因,后台守卫会把它当成越界回滚掉。
                        let exec = kanzei_harness::managed_fence::tool_scope(
                            &name,
                            kanzei_harness::progress::scope(handle, tool.execute(input, ctx)),
                        );
                        tokio::pin!(exec);
                        let output = loop {
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
            });
            let mut model_content = output.model_content();
            let (images, dropped_note) = tool_images_to_parts(&output, images_supported);
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
    use kanzei_llm::Part;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct ProbeTool {
        name: &'static str,
        concurrency: ToolConcurrency,
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
    }

    #[async_trait]

    impl Tool for ProbeTool {
        fn name(&self) -> &'static str {
            self.name
        }

        fn description(&self) -> String {
            "test probe".into()
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        fn concurrency(&self, _input: &serde_json::Value, _ctx: &ToolCtx) -> ToolConcurrency {
            self.concurrency.clone()
        }

        async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> ToolOutput {
            let active = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(
                input["delay_ms"].as_u64().unwrap_or(10),
            ))
            .await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            let label = input["label"].as_str().unwrap_or("probe");
            if input["fail"].as_bool().unwrap_or(false) {
                ToolOutput::error(format!("{label} failed"))
            } else {
                ToolOutput::ok(format!("{label} ok"))
            }
        }
    }

    fn probe_call(
        index: usize,
        id: &str,
        input: serde_json::Value,
        tool: Arc<ProbeTool>,
    ) -> PreparedToolCall {
        PreparedToolCall {
            index,
            id: id.into(),
            name: tool.name().into(),
            concurrency: tool.concurrency(
                &input,
                &ToolCtx::new(std::env::temp_dir(), std::env::temp_dir()),
            ),
            input,
            tool,
        }
    }

    #[tokio::test]
    async fn 普通只读工具真实并发_失败隔离且结果按调用顺序归位() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let tool = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "call_slow",
                serde_json::json!({"label": "slow", "delay_ms": 60}),
                tool.clone(),
            ),
            probe_call(
                1,
                "call_fast_fail",
                serde_json::json!({"label": "fast", "delay_ms": 5, "fail": true}),
                tool,
            ),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir(), std::env::temp_dir());
        let mut completed = Vec::new();
        let mut on_event = |event| {
            if let RunEvent::ToolEnd { id, .. } = event {
                completed.push(id);
            }
        };
        let results = execute_prepared_tools(
            calls,
            &ctx,
            super::MAX_PARALLEL_TOOLS_PER_WAVE,
            true,
            &mut on_event,
        )
        .await;

        assert!(
            max_in_flight.load(Ordering::SeqCst) >= 2,
            "只读调用没有重叠执行"
        );
        assert_eq!(completed, vec!["call_fast_fail", "call_slow"]);
        assert!(matches!(
            &results[0].1,
            Part::ToolResult { call_id, is_error: false, content } if call_id == "call_slow" && content.contains("slow ok")
        ));
        assert!(matches!(
            &results[1].1,
            Part::ToolResult { call_id, is_error: true, content } if call_id == "call_fast_fail" && content.contains("fast failed")
        ));
    }

    #[tokio::test]
    async fn 同一工作树读写与写写冲突严格串行() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let writer = Arc::new(ProbeTool {
            name: "probe_write",
            concurrency: ToolConcurrency::WorktreeWrite("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let reader = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight,
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "write_1",
                serde_json::json!({"delay_ms": 15}),
                writer.clone(),
            ),
            probe_call(1, "read_1", serde_json::json!({"delay_ms": 15}), reader),
            probe_call(2, "write_2", serde_json::json!({"delay_ms": 15}), writer),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir(), std::env::temp_dir());
        let mut on_event = |_event| {};
        let results = execute_prepared_tools(
            calls,
            &ctx,
            super::MAX_PARALLEL_TOOLS_PER_WAVE,
            true,
            &mut on_event,
        )
        .await;

        assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
        assert_eq!(
            results
                .iter()
                .map(|(_, part, _)| match part {
                    Part::ToolResult { call_id, .. } => call_id.as_str(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>(),
            vec!["write_1", "read_1", "write_2"]
        );
    }

    #[test]
    fn declined_tool_batch_keeps_real_and_placeholder_results_paired() {
        let calls = vec![
            (
                "call_done".into(),
                "write".into(),
                serde_json::json!({}),
                "{}".into(),
            ),
            (
                "call_declined".into(),
                "edit".into(),
                serde_json::json!({}),
                "{}".into(),
            ),
            (
                "call_pending".into(),
                "bash".into(),
                serde_json::json!({}),
                "{}".into(),
            ),
        ];
        let mut results = vec![Part::ToolResult {
            call_id: "call_done".into(),
            content: "真实写入结果".into(),
            is_error: false,
        }];
        append_declined_tool_results(&mut results, &calls, 1);

        assert_eq!(results.len(), 3);
        assert!(matches!(
            &results[0],
            Part::ToolResult { call_id, content, is_error: false }
                if call_id == "call_done" && content == "真实写入结果"
        ));
        assert!(matches!(
            &results[1],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_declined" && content.contains("declined")
        ));
        assert!(matches!(
            &results[2],
            Part::ToolResult { call_id, is_error: true, content }
                if call_id == "call_pending" && content.contains("cancelled")
        ));
    }

    /// R-171 批2:writer 阶段 max in-flight=1(wave 上限 1 时逐条执行且不重叠)。
    /// drive 在 ReadParallelWriteSerial 下直接走串行路径不调 wave;此测试锚定
    /// wave 路径若被复用(如防御性回退)同样满足「任意时刻最多一个工具执行」。
    #[tokio::test]
    async fn max_parallel_1_强制串行_结果按下标归位() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let tool = Arc::new(ProbeTool {
            name: "probe_read",
            concurrency: ToolConcurrency::Shared("worktree:test".into()),
            in_flight: in_flight.clone(),
            max_in_flight: max_in_flight.clone(),
        });
        let calls = vec![
            probe_call(
                0,
                "call_1",
                serde_json::json!({"label": "first", "delay_ms": 20}),
                tool.clone(),
            ),
            probe_call(
                1,
                "call_2",
                serde_json::json!({"label": "second", "delay_ms": 20}),
                tool.clone(),
            ),
            probe_call(
                2,
                "call_3",
                serde_json::json!({"label": "third", "delay_ms": 20}),
                tool,
            ),
        ];
        let ctx = ToolCtx::new(std::env::temp_dir(), std::env::temp_dir());
        let mut on_event = |_event| {};
        let results = execute_prepared_tools(calls, &ctx, 1, true, &mut on_event).await;

        assert_eq!(
            max_in_flight.load(Ordering::SeqCst),
            1,
            "writer 阶段任意时刻最多一个工具执行"
        );
        assert_eq!(results.len(), 3);
        // 结果按下标与调用顺序对齐。
        for (idx, (call_index, part, images)) in results.iter().enumerate() {
            assert!(images.is_empty(), "纯文本工具不得产生图片 Part");
            assert_eq!(*call_index, idx);
            let expect_id = format!("call_{}", idx + 1);
            assert!(matches!(
                part,
                Part::ToolResult { call_id, is_error: false, .. }
                    if *call_id == expect_id
            ));
        }
    }

    // ---- R-249:图片投递与降级 ----

    fn output_with_images(n: usize) -> ToolOutput {
        ToolOutput::ok("done").with_images(
            (0..n)
                .map(|i| kanzei_harness::ToolImage {
                    media_type: "image/png".into(),
                    data: format!("payload{i}"),
                })
                .collect(),
        )
    }

    #[test]
    fn images_pass_through_when_provider_supports_them() {
        let (parts, note) = tool_images_to_parts(&output_with_images(2), true);
        assert_eq!(parts.len(), 2);
        assert!(note.is_none(), "支持图片时不应产生降级说明");
        assert!(matches!(
            &parts[0],
            Part::Image { media_type, data } if media_type == "image/png" && data == "payload0"
        ));
    }

    #[test]
    fn images_degrade_to_explicit_note_when_unsupported() {
        // 关键不变式:不支持时**一个 Image part 都不能进历史**。进了历史就会跟着
        // 每一轮重发,client.rs 的硬拒绝会让这条对话在该 provider 上永久不可用。
        let (parts, note) = tool_images_to_parts(&output_with_images(3), false);
        assert!(parts.is_empty(), "不支持图片时不得放行任何 Image part");
        let note = note.expect("必须给出显式降级说明,不能静默丢弃");
        assert!(note.contains('3'), "说明里要写清丢了几张: {note}");
        assert!(
            note.contains("did NOT see"),
            "必须明确告诉模型它没看到图,否则它会照着文本编内容: {note}"
        );
    }

    #[test]
    fn no_images_means_no_note_on_either_path() {
        // 回归:纯文本工具的返回在两种能力下都必须逐字节不变。
        for supported in [true, false] {
            let (parts, note) = tool_images_to_parts(&ToolOutput::ok("plain"), supported);
            assert!(parts.is_empty());
            assert!(note.is_none());
        }
    }
}
