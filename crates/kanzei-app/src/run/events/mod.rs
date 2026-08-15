//! 运行事件域(R-253 批5,纯搬迁自 run/mod.rs)。
//!
//! 独立理由:事件归约是「把 RunEvent 投影到各消费者」的独立变更理由——`build_event_
//! handler` 对每个事件同时做 UI 投影/typed 持久化/trace 落库/指标累计/运行时状态,
//! `build_ask_handler` 把权限/提问请求经 kz:ask 发给前端并挂 pending asks 表。
//! 两者与装配/执行/落库正交:加一个事件类型或改投影方式,不必读懂整个运行主链路
//! (照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):⑦R-143 的两个 `AtomicBool` 有 swap 语义(`round_pending.swap(false)`
//! 是「取并清」,不是「读」)——拆 sink 时这对状态必须整体归 MetricsSink,不能一个在
//! UI sink、一个在 metrics sink(批9 处理,本次原样搬迁)。⑧D-361 的 `subagent_tools`
//! 是跨模块状态:这里边跑边收,run_task 轮末合并进 tools_vec 供鞭挞判定——拆 sink 后
//! MetricsSink 要能把它交出来(批9 处理,本次原样搬迁)。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use kanzei_core::{AskFuture, RunEvent};
use serde_json::json;
use tauri::Emitter;
use tokio::sync::oneshot;

use crate::{record_live_trace_at_path, typed_events, with_session_id, LiveRun, PendingAsk};

use super::{now_ms, subagent_round_tool, TRACE_INPUT_KEEP_CHARS};

/// R-202 批2:构造 run_task 的 RunEvent 处理器闭包(原 run_task 内联的 on_event)。
/// D-173 可观测性:主代理工具调用实时转发 UI 并按 id 记开始时刻;R-143:git commit
/// 检测位在 ToolStart(action=commit)/ToolEnd(ok=true) 置位/提升;轨迹与 typed writer
/// 增量落库。返回的闭包与内联定义时行为逐字节一致。
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_event_handler(
    emit_event: impl Fn(&str, serde_json::Value) -> tauri::Result<()> + 'static,
    tool_started: Arc<Mutex<HashMap<String, std::time::Instant>>>,
    trace_log: Arc<Mutex<LiveRun>>,
    trace_state_path_for_events: PathBuf,
    trace_session_id_for_events: String,
    typed_writer_for_events: Arc<Mutex<typed_events::TypedEventWriter>>,
    committed_this_round: Arc<std::sync::atomic::AtomicBool>,
    pending_commit_call: Arc<std::sync::atomic::AtomicBool>,
    subagent_tools: Arc<Mutex<std::collections::BTreeSet<String>>>,
) -> impl FnMut(RunEvent) {
    let round_committed = committed_this_round;
    let round_pending = pending_commit_call;
    move |event: RunEvent| {
        let elapsed_ms = |id: &str| -> Option<u128> {
            tool_started
                .lock()
                .unwrap()
                .remove(id)
                .map(|at| at.elapsed().as_millis())
        };
        let _ = match event {
            RunEvent::TurnStart { step, max_steps } => {
                {
                    let mut live = trace_log.lock().unwrap();
                    live.steps = live.steps.max(step);
                }
                record_live_trace_at_path(
                    &trace_state_path_for_events,
                    &trace_session_id_for_events,
                    &trace_log,
                    json!({
                        "kind": "turn.started", "step": step, "at": now_ms(),
                    }),
                );
                typed_writer_for_events
                    .lock()
                    .unwrap()
                    .turn_started(step, max_steps);
                emit_event("kz:turn", json!({ "step": step, "maxSteps": max_steps }))
            }
            RunEvent::Text(text) => {
                typed_writer_for_events.lock().unwrap().push_text(&text);
                emit_event("kz:text", json!({ "text": text }))
            }
            RunEvent::Reasoning(text) => emit_event("kz:reasoning", json!({ "text": text })),
            RunEvent::AssistantMessageCommitted { step, message } => {
                typed_writer_for_events
                    .lock()
                    .unwrap()
                    .assistant_committed(step, message);
                Ok(())
            }
            RunEvent::ToolStart {
                id,
                name,
                summary,
                input,
            } => {
                // R-143:git commit 调用意图登记(成功与否由 ToolEnd ok 收口)。
                if name == "git" && input["action"].as_str() == Some("commit") {
                    round_pending.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                tool_started
                    .lock()
                    .unwrap()
                    .insert(id.clone(), std::time::Instant::now());
                record_live_trace_at_path(
                    &trace_state_path_for_events,
                    &trace_session_id_for_events,
                    &trace_log,
                    json!({
                        "kind": "tool.started", "id": id, "name": name,
                        "summary": summary, "at": now_ms(),
                    }),
                );
                emit_event(
                    "kz:tool-start",
                    json!({ "id": id, "name": name, "summary": summary, "input": input }),
                )
            }
            // 执行中的增量输出:只转发给 UI 实时追加,不进 trace——回放时
            // ToolEnd 的完整输出就是终态,逐段进度落盘只会把轨迹撑爆。
            RunEvent::ToolProgress { id, chunk } => {
                emit_event("kz:tool-progress", json!({ "id": id, "chunk": chunk }))
            }
            RunEvent::ToolEnd {
                id,
                name,
                ok,
                outcome,
                code,
                preview,
                display,
            } => {
                // R-143:git commit 成功后提升 committed 位(仅当本轮确实调用了 commit)。
                if name == "git" {
                    if ok && round_pending.swap(false, std::sync::atomic::Ordering::Relaxed) {
                        round_committed.store(true, std::sync::atomic::Ordering::Relaxed);
                    } else if !ok {
                        round_pending.store(false, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                record_live_trace_at_path(
                    &trace_state_path_for_events,
                    &trace_session_id_for_events,
                    &trace_log,
                    json!({
                        "kind": "tool.completed", "id": id, "name": name, "ok": ok,
                        "outcome": outcome, "code": code,
                        "durationMs": elapsed_ms(&id), "at": now_ms(),
                        // 失败原因要留档,成功的预览不必——轨迹不是第二份对话记录。
                        "error": (!ok).then(|| preview.chars().take(400).collect::<String>()),
                    }),
                );
                emit_event(
                    "kz:tool-end",
                    json!({ "id": id, "name": name, "ok": ok, "outcome": outcome, "code": code, "preview": preview, "display": display }),
                )
            }
            RunEvent::ToolResultsCommitted { step, message } => {
                typed_writer_for_events
                    .lock()
                    .unwrap()
                    .tool_results_committed(step, message);
                Ok(())
            }
            // 轮内主动压缩:UI 要看得见"什么时候让的路、让掉了多少",
            // 否则历史突然变短只会被当成 bug(D-176)。
            RunEvent::ContextCompacted {
                before_tokens,
                after_tokens,
                budget_tokens,
                limit_tokens,
                dropped_messages,
            } => {
                record_live_trace_at_path(
                    &trace_state_path_for_events,
                    &trace_session_id_for_events,
                    &trace_log,
                    json!({
                        "kind": "context.compacted", "before": before_tokens, "after": after_tokens,
                        "budget": budget_tokens, "limit": limit_tokens,
                        "dropped": dropped_messages, "at": now_ms(),
                    }),
                );
                emit_event(
                    "kz:status",
                    json!({
                        "stage": "压缩",
                        "detail": format!(
                            "上下文约 {}k 已达 {}k 预算线(上限 {}k),就地压缩为 {}k,裁掉 {dropped_messages} 条历史",
                            before_tokens / 1000, budget_tokens / 1000,
                            limit_tokens / 1000, after_tokens / 1000
                        ),
                    }),
                )
            }
            // R-236 B4:L0 机械清理——先于 LLM 纪要,零幻觉;轨迹留档让
            // 「压缩触发频率下降」可度量。
            RunEvent::ContextPruned {
                cleared_results,
                before_tokens,
                after_tokens,
            } => {
                record_live_trace_at_path(
                    &trace_state_path_for_events,
                    &trace_session_id_for_events,
                    &trace_log,
                    json!({
                        "kind": "context.pruned", "cleared": cleared_results,
                        "before": before_tokens, "after": after_tokens, "at": now_ms(),
                    }),
                );
                emit_event(
                    "kz:status",
                    json!({
                        "stage": "压缩",
                        "detail": format!(
                            "已机械清理 {cleared_results} 条旧工具结果({}k → {}k token),未动 LLM 纪要",
                            before_tokens / 1000, after_tokens / 1000
                        ),
                    }),
                )
            }
            RunEvent::PermissionResolved {
                tool_call_id,
                action,
                resource,
                decision,
                source,
                ..
            } => {
                record_live_trace_at_path(
                    &trace_state_path_for_events,
                    &trace_session_id_for_events,
                    &trace_log,
                    json!({
                        "kind": "permission.resolved", "id": tool_call_id, "action": action,
                        "resource": resource, "decision": decision, "source": source, "at": now_ms(),
                    }),
                );
                Ok(())
            }
            // 子代理实时状态:挂到对应 task 块的进度行,并附带可展开的子工具轨迹。
            RunEvent::TaskProgress { id, text, trace } => {
                // D-361:子代理内部完成的工具调用,名字上卷进本轮画像。主轮画像只切
                // 主 conversation 的消息(轮末 summarize_tools),子代理的 read/grep/edit
                // 全在它自己的消息里——主轮看得见的只有一次 task 调用,而 task 本身在
                // NON_PROGRESS_TOOLS 里。不上卷的话「整轮把活派给子代理」在鞭挞眼里
                // 等于什么都没干:第一轮 Nudge、第二轮 Stop(NoAction),越守规矩地委派
                // 越快自停,停止原因还误报成「空转」。
                if let Some(name) = trace.as_ref().and_then(subagent_round_tool) {
                    subagent_tools.lock().unwrap().insert(name.to_string());
                }
                // UI 实时事件保留完整入参(transcript 数据源,R-174);
                // 落库副本把入参截断到上限,避免大入参撑爆 run.trace(D-297 验收③)。
                let ui_payload = json!({
                    "id": id,
                    "text": text,
                    "trace": trace.as_ref().map(|item| json!({
                        "child_id": item.child_id,
                        "phase": item.phase,
                        "name": item.name,
                        "summary": item.summary,
                        "ok": item.ok,
                        "outcome": item.outcome,
                        "code": item.code,
                        "preview": item.preview,
                        "display": item.display,
                        "input": item.input,
                    })),
                });
                let stored_payload = match &trace {
                    Some(item) => json!({
                        "id": id,
                        "text": text,
                        "trace": json!({
                            "child_id": item.child_id,
                            "phase": item.phase,
                            "name": item.name,
                            "summary": item.summary,
                            "ok": item.ok,
                            "outcome": item.outcome,
                            "code": item.code,
                            "preview": item.preview,
                            "display": item.display,
                            "input": item.input.as_ref().map(|input| {
                                let text = serde_json::to_string(input).unwrap_or_default();
                                let kept: String =
                                    text.chars().take(TRACE_INPUT_KEEP_CHARS).collect();
                                json!(kept)
                            }),
                        }),
                    }),
                    None => ui_payload.clone(),
                };
                record_live_trace_at_path(
                    &trace_state_path_for_events,
                    &trace_session_id_for_events,
                    &trace_log,
                    stored_payload,
                );
                emit_event("kz:task-progress", ui_payload)
            }
            RunEvent::Retry {
                attempt,
                max,
                delay_ms,
            } => emit_event(
                "kz:status",
                json!({ "stage": "重试", "detail": format!("网络请求暂时失败,第 {attempt}/{max} 次重试,等待 {delay_ms}ms") }),
            ),
            // 本步工具尚未执行,重放零副作用;前端需丢弃本步已渲染的残缺输出。
            RunEvent::StreamRestart {
                attempt,
                max,
                delay_ms,
            } => {
                typed_writer_for_events.lock().unwrap().stream_restarted();
                emit_event(
                    "kz:stream-restart",
                    json!({
                    "attempt": attempt,
                    "max": max,
                    "delayMs": delay_ms,
                    "detail": format!("连接中断,重新请求本轮 {attempt}/{max},等待 {delay_ms}ms"),
                    }),
                )
            }
            // 每步累计:停止时 episode 才有真实 token 数,而不是写个 0 冒充。
            RunEvent::StepEnd { usage, .. } => {
                {
                    let mut live = trace_log.lock().unwrap();
                    live.input_tokens += usage.input;
                    live.output_tokens += usage.output;
                }
                emit_event(
                    "kz:step",
                    json!({
                        "input": usage.input, "output": usage.output,
                        "cacheRead": usage.cache_read, "cacheWrite": usage.cache_write,
                    }),
                )
            }
        };
    }
}

/// R-202 批2:构造 run_task 的权限/提问询问处理器闭包(原 run_task 内联的 ask)。
/// 请求经 kz:ask 事件发给前端,应答挂 pending asks 表等待 answer_ask 回填。
pub(crate) fn build_ask_handler(
    asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    ask_seq: Arc<AtomicU64>,
    ask_source: &'static str,
    window: &tauri::Window,
    project_root: PathBuf,
    session_id: String,
) -> impl FnMut(kanzei_core::AskRequest) -> AskFuture {
    let ask_window = window.clone();
    let ask_root = project_root;
    let ask_session_id = session_id;
    move |request: kanzei_core::AskRequest| -> AskFuture {
        let (sender, receiver) = oneshot::channel();
        let id = ask_seq.fetch_add(1, Ordering::SeqCst);
        let (action, resource, payload) = match &request {
            kanzei_core::AskRequest::Permission { action, resource } => (
                action.clone(),
                resource.clone(),
                json!({ "kind": "permission", "id": id, "action": action, "resource": resource, "remember": kanzei_harness::config::generalize_resource(action, resource) }),
            ),
            kanzei_core::AskRequest::Question {
                question,
                options,
                default,
                multiple,
            } => (
                "question".into(),
                question.clone(),
                json!({ "kind": "question", "id": id, "question": question, "options": options, "default": default, "multiple": multiple }),
            ),
        };
        let payload = with_session_id(payload, &ask_session_id);
        let payload = match payload {
            serde_json::Value::Object(mut object) => {
                object.insert("source".into(), json!(ask_source));
                serde_json::Value::Object(object)
            }
            other => other,
        };
        asks.lock().unwrap().insert(
            id,
            PendingAsk {
                sender,
                request,
                action,
                resource,
                project_root: ask_root.clone(),
                session_id: ask_session_id.clone(),
            },
        );
        let _ = ask_window.emit("kz:ask", payload);
        Box::pin(async move {
            receiver
                .await
                .unwrap_or(kanzei_core::AskResponse::Cancelled)
        })
    }
}
