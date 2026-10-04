//! 运行落库域(R-253 批3,纯搬迁自 run/mod.rs)。
//!
//! 独立理由:「怎么跑」与「跑完怎么落库」是两个变更理由——`persist_round_outcome`
//! 把一轮的摘要/状态/episode/通知写进会话库,`finalize_round` 做对话落库、轮末
//! 压缩、kz:done 与写租约收尾。它们不参与事件归约与执行流水线,独立成域后
//! 加一个落库字段不必读懂整个运行主链路(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):⑤`_write_lease` 是 RAII guard——`Drop` 补写 Released 事件
//! (D-303);正常路径由 `finalize_round` 显式发 Released 并 `mark_released()` 防重复,
//! **且仅非流水线路径发**(流水线路径的租约归编排对象管,再发一条会在轨迹里凭空
//! 多出一次释放)。⑥`typed_flush_task` 是 spawn 出来的弱引用定时任务,`finalize_round`
//! 在这里 `abort()` 它——跨模块传递时不能被当成没人用的字段删掉。⑨`stage` 闭包
//! 签名保持 `&(dyn Fn(&str, String) + Sync)`。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::json;
use tauri::Emitter;

use crate::{
    flush_live_run, flush_live_trace, memory, typed_events, with_session_id, LiveRun,
    MutexPoisonExt,
};

use super::assembly::{RuntimeDeps, RuntimeHandles, WriterLeaseTrace};
use super::{append_run_notification, compaction_input_tokens, report_persistence_failure};

/// R-253 批7b:`finalize_round` 参数分组——**会话事务层**:对话历史/会话身份/
/// typed 写入器与弱引用 flush 任务/轮末打开的 store。生命周期:会话级。
pub(crate) struct FinalizeSession<'a> {
    pub(crate) conversation: &'a Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    pub(crate) session_id: &'a str,
    pub(crate) typed_writer: &'a Arc<Mutex<typed_events::TypedEventWriter>>,
    pub(crate) typed_flush_task: tauri::async_runtime::JoinHandle<()>,
    pub(crate) final_store: Option<kanzei_core::SessionStore>,
}

/// R-253 批7b:`finalize_round` 参数分组——**单轮收尾层**:执行身份/写租约/轨迹出口/
/// 路径判定。生命周期:本轮级。
pub(crate) struct FinalizeRound<'a> {
    pub(crate) ctx: &'a kanzei_harness::ToolCtx,
    pub(crate) run_id: &'a str,
    pub(crate) process_id: &'a str,
    pub(crate) _write_lease: &'a Option<WriterLeaseTrace>,
    pub(crate) writer_event: &'a (dyn Fn(kanzei_harness::orchestration::OrchestrationEvent) + Sync),
}

/// R-253 批7b:`finalize_round` 参数分组——**本轮结果层**:摘要/历史长/工具画像/
/// kz:done 载荷。生命周期:本轮级,轮末一次性消费。
pub(crate) struct FinalizeOutcome<'a> {
    pub(crate) summary: &'a kanzei_core::RunSummary,
    pub(crate) history_len: usize,
    pub(crate) this_run_tools: &'a std::collections::BTreeMap<String, usize>,
    pub(crate) auto_action_json: &'a serde_json::Value,
    pub(crate) elapsed_ms: u64,
}

/// R-253 批7b:`finalize_round` 参数分组——**UI 汇报层**:事件投影窗口/进度闭包/
/// live 画像。生命周期:会话级,与运行域之外的 AppState 共享。
pub(crate) struct RoundReport<'a> {
    pub(crate) window: &'a tauri::Window,
    pub(crate) stage: &'a (dyn Fn(&str, String) + Sync),
    pub(crate) live: &'a Arc<Mutex<LiveRun>>,
}

/// R-319 B3:扩展事务的触发事件只记录授予事实；轮末再追加结果事件，避免把
/// 尚未发生的 commit/tracker 动作伪装成已完成。实际动作从同一 run_id 的 run.trace
/// 顺序重放，结果由本轮真实 run_result 决定。
fn record_transaction_budget_result(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    run_id: &str,
    result: &str,
) {
    let Ok(events) = store.list_events_by_type(session_id, 0, "run.trace") else {
        return;
    };
    let mut extension_seen = false;
    let mut actual_actions = Vec::new();
    for stored in events {
        if stored.payload["run_id"].as_str() != Some(run_id) {
            continue;
        }
        let Some(trace_events) = stored.payload["events"].as_array() else {
            continue;
        };
        for event in trace_events {
            if event["kind"].as_str() == Some("transaction_budget.extended") {
                extension_seen = true;
                continue;
            }
            if extension_seen && event["kind"].as_str() == Some("tool.completed") {
                if let Some(name) = event["name"].as_str() {
                    actual_actions.push(name.to_string());
                }
            }
        }
    }
    if !extension_seen {
        return;
    }
    let _ = store.append_event(
        session_id,
        "run.transaction_budget_result",
        &json!({
            "run_id": run_id,
            "result": result,
            "actual_actions": actual_actions,
        }),
    );
}

/// Optional reporting and memory work runs only after the durable outcome.
struct OutcomeEffects<'a> {
    report: &'a dyn Fn(&str, String),
    mobile: &'a dyn Fn(&str, &str),
    harvest: &'a dyn Fn(&[kanzei_llm::Message]),
    after_success: &'a dyn Fn(Option<i64>),
}

#[derive(Debug)]
struct RoundOutcomeNotCommitted(String);

impl std::fmt::Display for RoundOutcomeNotCommitted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(super) fn uncommitted_outcome(error: anyhow::Error) -> anyhow::Error {
    let message = error.to_string();
    error.context(RoundOutcomeNotCommitted(message))
}

pub(crate) fn is_uncommitted_outcome(error: &anyhow::Error) -> bool {
    error.is::<RoundOutcomeNotCommitted>()
}

#[derive(Debug)]
struct RoundOutcomeStopped(String);

impl std::fmt::Display for RoundOutcomeStopped {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(crate) fn stopped_outcome(error: anyhow::Error) -> anyhow::Error {
    let message = error.to_string();
    error.context(RoundOutcomeStopped(message))
}

pub(crate) fn is_stopped_outcome(error: &anyhow::Error) -> bool {
    error.is::<RoundOutcomeStopped>()
}

pub(crate) struct PersistedRoundOutcome {
    pub(crate) store: kanzei_core::SessionStore,
    pub(crate) stopped: bool,
}

/// 轮末落库:核心状态先原子提交，通知和记忆后处理随后执行。
#[allow(clippy::too_many_arguments)]
pub(crate) fn persist_round_outcome(
    state_path: &std::path::Path,
    window: &tauri::Window,
    session_id: &str,
    run_result: &mut Result<kanzei_core::RunSummary, anyhow::Error>,
    runtime: &crate::SessionRuntime,
    halt_token: &kanzei_core::CancellationToken,
    typed_writer: &Arc<Mutex<typed_events::TypedEventWriter>>,
    prior: &[kanzei_llm::Message],
    ctx: &kanzei_harness::ToolCtx,
    prompt: &str,
    resolved: &kanzei_harness::config::ResolvedModel,
    run_id: &str,
    promoted_input_id: &str,
    run_started: &std::time::Instant,
    run_epoch_ms: i64,
    live: &Arc<Mutex<LiveRun>>,
) -> anyhow::Result<PersistedRoundOutcome> {
    let report = |operation: &str, error: String| {
        report_persistence_failure(window, session_id, operation, error);
    };
    let mobile = |title: &str, body: &str| {
        if let Ok(message) = crate::mobile_notify::notify_mobile(title, body) {
            tracing::debug!("{message}");
        }
    };
    let harvest = |messages: &[kanzei_llm::Message]| {
        kanzei_tools::memory::harvest_end_of_run(&ctx.project_root, prompt, messages);
    };
    let after_success = |episode_id: Option<i64>| {
        let project_dir = ctx.project_root.display().to_string();
        tauri::async_runtime::spawn(async move {
            match memory::consolidate_memory_inbox(project_dir, episode_id).await {
                Ok(report) if report.has_failures() => tracing::warn!("{}", report.summary()),
                Ok(report) => tracing::debug!("{}", report.summary()),
                Err(error) => tracing::warn!("memory inbox consolidation failed: {error}"),
            }
        });
        let _ = kanzei_tools::memory::reconcile_candidates(
            &ctx.project_root,
            episode_id,
            kanzei_tools::memory::CANDIDATE_MAX_AGE_DAYS,
        );
    };
    persist_round_outcome_with_effects(
        state_path,
        session_id,
        run_result,
        runtime,
        halt_token,
        typed_writer,
        prior,
        ctx,
        prompt,
        resolved,
        run_id,
        promoted_input_id,
        run_started,
        run_epoch_ms,
        live,
        &OutcomeEffects {
            report: &report,
            mobile: &mobile,
            harvest: &harvest,
            after_success: &after_success,
        },
    )
}

pub(super) fn commit_outcome(
    writer: &Arc<Mutex<typed_events::TypedEventWriter>>,
    input_id: &str,
    terminal: typed_events::TerminalFact,
    payload: &serde_json::Value,
) -> anyhow::Result<()> {
    let mut writer = writer.lock_or_recover();
    if writer.finish_with_input_outcome(input_id, terminal, payload) {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "提交运行结果失败: {}",
            writer
                .errors()
                .last()
                .map(String::as_str)
                .unwrap_or("typed outcome rejected")
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn persist_round_outcome_with_effects(
    state_path: &std::path::Path,
    session_id: &str,
    run_result: &mut Result<kanzei_core::RunSummary, anyhow::Error>,
    runtime: &crate::SessionRuntime,
    halt_token: &kanzei_core::CancellationToken,
    typed_writer: &Arc<Mutex<typed_events::TypedEventWriter>>,
    prior: &[kanzei_llm::Message],
    ctx: &kanzei_harness::ToolCtx,
    prompt: &str,
    resolved: &kanzei_harness::config::ResolvedModel,
    run_id: &str,
    promoted_input_id: &str,
    run_started: &std::time::Instant,
    run_epoch_ms: i64,
    live: &Arc<Mutex<LiveRun>>,
    effects: &OutcomeEffects<'_>,
) -> anyhow::Result<PersistedRoundOutcome> {
    let store = kanzei_core::SessionStore::open(state_path).map_err(|error| {
        (effects.report)("打开对话数据库", error.to_string());
        uncommitted_outcome(anyhow::Error::from(error))
    })?;
    // Stop and final commit use the same lifecycle ordering. The runner may have
    // returned before a winning stop cancelled this input; recheck its own token.
    // This guard never covers notifications or other optional external effects.
    let lifecycle = runtime.lifecycle.lock_or_recover();
    let stopped_by_user = halt_token.is_cancelled();
    if let Ok(summary) = run_result.as_mut() {
        summary.halted_by_user |= stopped_by_user;
    }
    let outcome = match &*run_result {
        Ok(summary) => {
            // Publish archive pointers while the input still protects their liveness.
            persist_runner_surface_if_changed(
                &store,
                &ctx.project_root,
                session_id,
                run_id,
                prior,
                summary,
            )
            .map_err(anyhow::Error::from)
            .and_then(|()| {
                commit_outcome(
                    typed_writer,
                    promoted_input_id,
                    if summary.halted_by_user {
                        typed_events::TerminalFact::Stopped
                    } else {
                        typed_events::TerminalFact::Completed
                    },
                    &json!({
                        "steps": summary.steps,
                        "halted_by_user": summary.halted_by_user,
                        "input": summary.usage.input,
                        "output": summary.usage.output,
                        "context": summary.context_report,
                    }),
                )
            })
        }
        Err(error) if stopped_by_user => commit_outcome(
            typed_writer,
            promoted_input_id,
            typed_events::TerminalFact::Stopped,
            &json!({ "halted_by_user": true, "error": error.to_string() }),
        ),
        Err(error) => Err(anyhow::anyhow!("{error}")),
    };
    if let Err(error) = outcome {
        // The rejected Completed transaction rolled back all critical state.
        // A Failed retry uses the same atomic boundary, never a partial fallback.
        let failure_result = commit_outcome(
            typed_writer,
            promoted_input_id,
            typed_events::TerminalFact::Failed(error.to_string()),
            &json!({ "error": error.to_string() }),
        );
        drop(lifecycle);
        if run_result.is_ok() {
            (effects.report)("提交运行结果", error.to_string());
        }
        if let Err(failure_error) = failure_result {
            (effects.report)("提交失败结果", failure_error.to_string());
            return Err(uncommitted_outcome(
                error.context(format!("失败结果也未提交: {failure_error}")),
            ));
        }
        record_transaction_budget_result(&store, session_id, run_id, "failed");
        typed_writer.lock_or_recover().write_shadow_report(prior);
        flush_live_run(&store, session_id, live, "failed");
        if let Err(notification_error) =
            append_run_notification(&store, session_id, "failed", error.to_string(), false)
        {
            (effects.report)("写入失败通知", notification_error.to_string());
        }
        (effects.mobile)("kanzei 任务失败", &format!("运行失败: {error}"));
        // Preserve the original provider error and its typed classification in
        // coordinator; this success only acknowledges the durable Failed outcome.
        if run_result.is_err() {
            return Ok(PersistedRoundOutcome {
                store,
                stopped: false,
            });
        }
        return Err(error);
    }
    drop(lifecycle);
    // The provider error remains available to coordinator's caller, but a stop
    // that won before durable finalization owns this terminal. Do not publish a
    // successful provider result or mark its cancelled input failed.
    if run_result.is_err() {
        typed_writer.lock_or_recover().write_shadow_report(prior);
        flush_live_run(&store, session_id, live, "halted");
        return Ok(PersistedRoundOutcome {
            store,
            stopped: true,
        });
    }
    let summary = run_result
        .as_ref()
        .expect("successful durable outcome has a summary");
    record_transaction_budget_result(&store, session_id, run_id, "completed");
    let this_run = &summary.round_messages;
    (effects.harvest)(this_run);
    // episode 落库(R-106):机械轨迹画像。失败不阻塞收尾。
    // R-213:当轮 episode_id 代填给轮末 memory manager(同 CLI 路径)。
    let mut current_episode_id: Option<i64> = None;
    if let Ok(episode_id) = store.append_episode(&kanzei_core::EpisodeRecord {
        session_id,
        prompt_head: prompt,
        outcome: if summary.halted_by_user {
            "halted"
        } else {
            "completed"
        },
        steps: summary.steps,
        input_tokens: summary.usage.input,
        output_tokens: summary.usage.output,
        tools_json: &serde_json::to_string(&kanzei_core::summarize_tools(this_run))
            .unwrap_or_default(),
        context_json: &serde_json::to_string(&summary.context_report).unwrap_or_default(),
        // R-099 调用画像:与冗余治理共用同一份口径,别处不再各算各的。
        metrics_json: &serde_json::to_string(&kanzei_core::summarize_metrics(this_run))
            .unwrap_or_default(),
        // D-173:轮次归属与墙钟。缺了它们,复盘只能从"当前配置"反推模型,
        // 而配置随时会变——最基本的事实都无法证伪。
        provider: &resolved.provider_name,
        model: &resolved.model,
        run_id,
        input_id: promoted_input_id,
        duration_ms: run_started.elapsed().as_millis() as u64,
        // R-106:上下文溢出压缩丢弃的轨迹段沉淀为 episode 的一部分,
        // 让溢出路径不再无声丢弃轨迹,复盘时可通过 episodes.overflow_json 查回。
        overflow_json: &serde_json::to_string(&summary.overflow_traces).unwrap_or_default(),
    }) {
        // R-161:本轮开跑预检索的 recall_events 归因到该 episode,可 join 查询。
        let _ = store.link_recall_events_to_episode(episode_id, run_epoch_ms);
        if let Err(error) = store.record_episode_recoveries(episode_id, this_run) {
            tracing::warn!(%error, episode_id, "记忆恢复证据写入失败，候选保持未验证");
        }
        current_episode_id = Some(episode_id);
    }
    live.lock_or_recover().flushed = true;
    if summary.halted_by_user {
        if let Err(error) =
            append_run_notification(&store, session_id, "stopped", "任务已停止", false)
        {
            (effects.report)("写入停止通知", error.to_string());
        }
        (effects.mobile)("kanzei 任务已停止", "运行已按停止/拒绝收尾");
    } else {
        if let Err(error) =
            append_run_notification(&store, session_id, "succeeded", "任务完成", false)
        {
            (effects.report)("写入完成通知", error.to_string());
        }
        (effects.mobile)("kanzei 任务完成", "运行已成功结束");
        (effects.after_success)(current_episode_id);
    }
    Ok(PersistedRoundOutcome {
        store,
        stopped: summary.halted_by_user,
    })
}

struct RoundCompaction {
    transaction_id: String,
    summary: serde_json::Value,
    source_surface: Vec<kanzei_llm::Message>,
}

impl RoundCompaction {
    fn persist_and_publish(
        &self,
        store: &kanzei_core::SessionStore,
        session_id: &str,
        messages: &[kanzei_llm::Message],
        conversation: &Mutex<HashMap<String, Vec<kanzei_llm::Message>>>,
        publish: &dyn Fn(&serde_json::Value),
    ) -> Result<(), kanzei_core::StoreError> {
        self.persist(store, session_id, messages, conversation)?;
        if let Some(digest) = self.summary["digest"].as_str() {
            publish(&json!({
                "summary": digest,
                "dropped": self.summary["dropped"],
                "before": self.summary["before"],
                "after": self.summary["after"],
            }));
        } else if self.summary["source"] == "round_prune" {
            publish(&self.summary);
        }
        Ok(())
    }

    fn persist(
        &self,
        store: &kanzei_core::SessionStore,
        session_id: &str,
        messages: &[kanzei_llm::Message],
        conversation: &Mutex<HashMap<String, Vec<kanzei_llm::Message>>>,
    ) -> Result<(), kanzei_core::StoreError> {
        let mut cache = conversation.lock_or_recover();
        let sequence = store.latest_event_sequence(session_id)?;
        let current = crate::conversation::project_latest_segment(store, session_id)
            .map_err(kanzei_core::StoreError::InvalidInput)?;
        if current != self.source_surface {
            return Err(kanzei_core::StoreError::InvalidInput(
                "压缩来源已变化，保留已提交上下文".into(),
            ));
        }
        store.append_run_compaction_transaction_checked(
            session_id,
            &self.transaction_id,
            &self.summary,
            &serde_json::to_value(messages)?,
            sequence,
        )?;
        cache.insert(session_id.to_string(), messages.to_vec());
        Ok(())
    }
}

fn prune_round_surface(
    project_root: &std::path::Path,
    messages: &mut [kanzei_llm::Message],
    limits: &kanzei_harness::config::Limits,
    run_id: &str,
    before: u64,
) -> Option<(usize, RoundCompaction)> {
    let source_surface = messages.to_vec();
    let cleared = kanzei_core::prune_conversation_with_archive(
        messages,
        limits.prune_protect_tokens(),
        limits.prune_min_gain_tokens(),
        project_root,
    );
    (cleared > 0).then(|| {
        (
            cleared,
            RoundCompaction {
                transaction_id: format!("{run_id}:prune"),
                summary: json!({
                    "source": "round_prune",
                    "cleared": cleared,
                    "before": before,
                    "after": kanzei_core::estimate_conversation_tokens(messages),
                }),
                source_surface,
            },
        )
    })
}

fn persist_runner_surface_if_changed(
    store: &kanzei_core::SessionStore,
    project_root: &std::path::Path,
    session_id: &str,
    run_id: &str,
    prior: &[kanzei_llm::Message],
    summary: &kanzei_core::RunSummary,
) -> Result<(), kanzei_core::StoreError> {
    let mut source = kanzei_core::filter_message_history(prior);
    source.extend(summary.round_messages.iter().cloned());
    if source == summary.messages {
        return Ok(());
    }
    let _publication = kanzei_core::store::artifact_liveness::lock_publication(project_root)?;
    let sequence = store.latest_event_sequence(session_id)?;
    let current = crate::conversation::project_latest_segment(store, session_id)
        .map_err(kanzei_core::StoreError::InvalidInput)?;
    if current == summary.messages || current != source {
        return Ok(());
    }
    store.append_run_compaction_transaction_checked(
        session_id,
        &format!("{run_id}:runner-compaction"),
        &json!({"source":"round_run", "overflow_traces": summary.overflow_traces}),
        &serde_json::to_value(&summary.messages)?,
        sequence,
    )?;
    Ok(())
}

/// R-202 批2:run_task 轮末收尾段后半——typed surface 事务/typed shadow 报告
/// → kz:done → 写租约 Released → 停止令牌回收。legacy snapshot 在投影真源切换后
/// 只读保留，不再由正常收尾新增 conversation.updated。
/// R-253 批7b:按生命周期分组收参——`&RuntimeDeps`(不变依赖)/`&RuntimeHandles`
/// (会话级句柄:conversation/live/halt_slot)/`FinalizeSession`(会话事务)/
/// `FinalizeRound`(单轮收尾)/`FinalizeOutcome`(本轮结果)/`RoundReport`(UI 汇报)/
/// `subagent_rt`(压缩用的执行上下文),共 7 参,消 too_many。
pub(crate) async fn finalize_round(
    deps: &RuntimeDeps,
    handles: &RuntimeHandles,
    session: FinalizeSession<'_>,
    round: FinalizeRound<'_>,
    outcome: FinalizeOutcome<'_>,
    report: RoundReport<'_>,
    _subagent_rt: &Option<kanzei_core::SubagentRuntime>,
) -> anyhow::Result<()> {
    let conversation = session.conversation;
    let session_id = session.session_id;
    let summary = outcome.summary;
    let window = report.window;
    let stage = report.stage;
    let live = report.live;
    let typed_writer = session.typed_writer;
    let ctx = round.ctx;
    let run_id = round.run_id;
    let process_id = round.process_id;
    let _write_lease = round._write_lease;
    let writer_event = round.writer_event;
    let final_store = session.final_store;
    let typed_flush_task = session.typed_flush_task;
    let history_len = outcome.history_len;
    let this_run_tools = outcome.this_run_tools;
    let auto_action_json = outcome.auto_action_json;
    let elapsed_ms = outcome.elapsed_ms;
    let config = &deps.config;
    let resolved = &deps.resolved;
    let client = &deps.client;
    let halt_slot = &handles.halt_slot;
    if let Some(store) = final_store.as_ref() {
        let refreshed = {
            let mut cache = conversation.lock_or_recover();
            crate::conversation::project_latest_segment(store, session_id).map(|messages| {
                cache.insert(session_id.to_string(), messages);
            })
        };
        if let Err(error) = refreshed {
            report_persistence_failure(window, session_id, "读取已提交上下文", error);
        }
    }

    let mut compaction: Option<RoundCompaction> = None;
    let mut compacted_messages = None;
    let mut _publication = None;

    // R-236 B1:轮末压缩走 core 同一份 compact_with_digest——保任务定义、保近期
    // 工作区逐字、只压中段、纪要过质量闸,失败回落原文节选。R-021 那套「整段历史
    // → 单条 300 字纪要」已删:那正是 D-181 在 core 侧修掉的失败模式(压完模型
    // 不知道自己做过什么),也是用户实测「打断插任务模型失忆」的主因之一。
    // 触发线与轮内同一把尺(compaction_budget:limit − max(output, buffer));
    // 估算同一口径(附件按固定成本,不按 base64 字节——消灭带附件必误触发)。
    if let Some(limit) = resolved
        .provider
        .context_limit
        .filter(|_| final_store.is_some())
    {
        let budget = kanzei_core::compaction_budget(
            limit,
            config.limits.max_tokens(),
            config.limits.compact_buffer_tokens(),
        );
        let mut conv = conversation
            .lock_or_recover()
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        let mut estimate = compaction_input_tokens(summary.last_input_tokens, &conv);
        // R-236 B4:轮末同样 L0 先行——机械清旧工具结果,清完够线就不动 LLM 纪要。
        if estimate > budget && conv.len() > 1 {
            // persist_round_outcome has already marked the session idle. Protect
            // archive creation until the replacement surface is durably committed.
            _publication = Some(
                kanzei_core::store::artifact_liveness::acquire_publication(&ctx.project_root)
                    .await?,
            );
            if let Some((cleared, pending)) = prune_round_surface(
                &ctx.project_root,
                &mut conv,
                &config.limits,
                run_id,
                estimate,
            ) {
                compaction = Some(pending);
                let after_prune = kanzei_core::estimate_conversation_tokens(&conv);
                stage(
                    "压缩",
                    format!(
                        "机械清理候选:拟清理 {cleared} 条旧工具结果({}k → {}k token)",
                        estimate / 1000,
                        after_prune / 1000
                    ),
                );
                estimate = after_prune;
            }
        }
        if estimate > budget && conv.len() > 1 {
            stage(
                "压缩",
                format!(
                    "对话历史约 {}k token 超预算 {}k(上限 {}k),压缩中段…",
                    estimate / 1000,
                    budget / 1000,
                    limit / 1000
                ),
            );
            let mut compact_traces = Vec::new();
            let mut digest_model =
                kanzei_tools::run::build_digest_model(config, &deps.proxy, resolved, &deps.route)
                    .await;
            digest_model.archive_root = Some(ctx.project_root.clone());
            let source_surface = compaction
                .as_ref()
                .map(|pending| pending.source_surface.clone())
                .unwrap_or_else(|| conv.clone());
            let dropped = kanzei_core::compact_conversation_with_model(
                client,
                Some(&digest_model),
                &mut conv,
                budget,
                &mut compact_traces,
                config.limits.recent_verbatim_ratio(),
            )
            .await;
            if dropped > 0 {
                let after = kanzei_core::estimate_conversation_tokens(&conv);
                // 纪要预览:替换消息的正文(UI 压缩条目用)。
                let digest_preview = conv
                    .iter()
                    .flat_map(|m| &m.parts)
                    .find_map(|p| match p {
                        kanzei_llm::Part::Text { text } if text.starts_with("(系统:此前") => {
                            Some(text.clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                compaction = Some(RoundCompaction {
                    transaction_id: format!("{run_id}:compaction"),
                    summary: json!({
                    "digest": digest_preview,
                    "dropped": dropped,
                    "before": estimate,
                    "after": after,
                    }),
                    source_surface,
                });
                // 被压段的轨迹摘要随轮末落 live trace,复盘可查(与轮内 overflow 同源语义)。
                for trace in compact_traces {
                    let mut live = live.lock_or_recover();
                    live.trace
                        .push(json!({ "kind": "compaction.dropped", "detail": trace }));
                }
                stage(
                    "压缩",
                    format!(
                        "压缩候选已生成:{}k → {}k token,拟压掉 {dropped} 条中段消息",
                        estimate / 1000,
                        after / 1000
                    ),
                );
            } else {
                // 中段为空压不动(超线来自任务定义/近期工作区本身):保留原历史,
                // 交给轮内的 trim_tail/被动恢复,不在轮末冒进。
                stage("压缩", "中段为空压不动,保留原历史".into());
            }
        }
        if compaction.is_some() {
            compacted_messages = Some(conv);
        }
    }

    let mut messages = compacted_messages.unwrap_or_else(|| {
        conversation
            .lock_or_recover()
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    });
    if let Some(store) = final_store.as_ref() {
        // 轨迹已在运行中按事件增量写入；这里仅补写实时写入失败的尾部，避免
        // 轮末再把整轮复制一遍造成回放重复。
        flush_live_trace(store, session_id, live);
        if let Some(pending) = compaction.take() {
            let publish = |payload: &serde_json::Value| {
                let before = payload["before"].as_u64().unwrap_or_default() / 1000;
                let after = payload["after"].as_u64().unwrap_or_default() / 1000;
                if payload["source"] == "round_prune" {
                    stage(
                        "压缩",
                        format!(
                            "已机械清理 {} 条旧工具结果({before}k → {after}k token)",
                            payload["cleared"].as_u64().unwrap_or_default()
                        ),
                    );
                    return;
                }
                stage(
                    "压缩",
                    format!(
                        "压缩完成:{}k → {}k token,压掉 {} 条中段消息",
                        before,
                        after,
                        payload["dropped"].as_u64().unwrap_or_default(),
                    ),
                );
                let _ = window.emit("kz:compacted", with_session_id(payload.clone(), session_id));
            };
            if let Err(error) =
                pending.persist_and_publish(store, session_id, &messages, conversation, &publish)
            {
                messages = conversation
                    .lock_or_recover()
                    .get(session_id)
                    .cloned()
                    .unwrap_or_default();
                report_persistence_failure(window, session_id, "写入压缩事务", error);
            }
        }
        // R-242 验收⑦：正常轮末不再追加 conversation.updated。已有 legacy snapshot
        // 仍由 legacy 回退/历史读取路径只读消费；typed facts 与完整 compaction surface
        // 承担当前会话的恢复真源。
        typed_writer
            .lock_or_recover()
            .write_shadow_report(&messages);
    }
    typed_flush_task.abort();
    let _ = window.emit(
        "kz:done",
        with_session_id(
            json!({
                "steps": summary.steps,
                "halted": summary.halted_by_user,
                "history": history_len,
                "elapsedMs": elapsed_ms,
                "input": summary.usage.input,
                "output": summary.usage.output,
                "cacheRead": summary.usage.cache_read,
                "cacheWrite": summary.usage.cache_write,
                "tools": this_run_tools,
                "autoAction": auto_action_json,
            }),
            session_id,
        ),
    );
    // R-171 批5:正常路径显式写 Released 事件(审计闭环 queued→acquired→released)。
    // 失败/取消路径由协调器快照保证租约不泄漏(WriterLease Drop 回调),审计不缺持有者。
    // R-173 批5:同样经 OrchestrationEvent 单一出口,与上面两条 writer 事件同源。
    //
    // 仅持有实际写租约的运行发释放事件。
    if let Some(trace) = _write_lease {
        writer_event(
            kanzei_harness::orchestration::OrchestrationEvent::WriterReleased {
                project_root: ctx.project_root.clone(),
                run_id: run_id.to_string(),
                process_id: process_id.to_string(),
            },
        );
        // 正常路径已落 Released,标记 guard 避免 Drop 重复补写(D-303)。
        trace.mark_released();
    }
    // D-342:本 run 收尾,收回停止令牌(stop 已 take 过则本来就是 None,幂等)。
    halt_slot.lock_or_recover().take();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn runner_outcome_fixture(
        provider_failed: bool,
        halt: Option<kanzei_core::CancellationToken>,
    ) -> (
        std::path::PathBuf,
        kanzei_core::SessionStore,
        Arc<Mutex<typed_events::TypedEventWriter>>,
        Result<kanzei_core::RunSummary, anyhow::Error>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "kz-c5-outcome-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = root.join("state.db");
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store
            .create_session("ses", root.to_str().unwrap(), None)
            .unwrap();
        store
            .admit_input("ses", "input", "prompt", kanzei_core::Delivery::Queue)
            .unwrap();
        store.promote_next_input("ses").unwrap().unwrap();
        assert!(store.start_input("input").unwrap());
        store.set_status("ses", "running").unwrap();
        let writer = Arc::new(Mutex::new(typed_events::TypedEventWriter::new(
            &path, "ses", "run",
        )));
        writer
            .lock_or_recover()
            .user_message("input", kanzei_llm::Message::user_text("prompt"));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            // The real client performs two pre-stream retries for 503 responses.
            for _ in 0..if provider_failed { 3 } else { 1 } {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                let header_end = loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(p) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
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
                let (status, content_type, body) = if provider_failed {
                    (
                        "503 Service Unavailable",
                        "application/json",
                        json!({"error":{"message":"temporary outage"}}).to_string(),
                    )
                } else {
                    (
                        "200 OK",
                        "text/event-stream",
                        format!(
                            "data: {}\n\ndata: [DONE]\n\n",
                            json!({"choices":[{"index":0,"delta":{"content":"done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}})
                        ),
                    )
                };
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
                socket.write_all(body.as_bytes()).await.unwrap();
            }
        });
        let ctx = kanzei_harness::ToolCtx::new(root.clone(), root.clone());
        let snapshot = kanzei_harness::Harness::default()
            .resolve(&kanzei_harness::ResolveCtx {
                profile: kanzei_harness::ProfileKind::Dev,
                cwd: root.clone(),
                project_root: root.clone(),
                config: Arc::new(kanzei_harness::KanzeiConfig::default()),
            })
            .unwrap();
        let agent = serde_json::from_value(
            json!({"name":"fixture","profile":"dev","mode":"primary","steps":1,"system":"test"}),
        )
        .unwrap();
        let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
        let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), None);
        let config = kanzei_core::RunnerConfig {
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
            ask_policy: kanzei_core::AskPolicy::NonInteractive,
            halt,
        };
        let sink_writer = writer.clone();
        let mut sink = move |event| {
            use kanzei_core::RunEvent;
            let mut writer = sink_writer.lock_or_recover();
            match event {
                RunEvent::TurnStart {
                    step, max_steps, ..
                } => writer.turn_started(step, max_steps),
                RunEvent::Text(text) => writer.push_text(&text),
                RunEvent::AssistantMessageCommitted {
                    step,
                    message,
                    commit,
                } => {
                    if !writer.assistant_committed(step, message) {
                        commit.reject(
                            writer
                                .errors()
                                .last()
                                .cloned()
                                .unwrap_or_else(|| "assistant rejected".into()),
                        );
                    }
                }
                RunEvent::ToolResultsCommitted {
                    step,
                    message,
                    commit,
                } => {
                    if !writer.tool_results_committed(step, message) {
                        commit.reject(
                            writer
                                .errors()
                                .last()
                                .cloned()
                                .unwrap_or_else(|| "tool results rejected".into()),
                        );
                    }
                }
                _ => {}
            }
        };
        let mut ask = |_| -> kanzei_core::AskFuture {
            Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            kanzei_core::run_once(
                &client,
                &route,
                &snapshot,
                &agent,
                &config,
                &ctx,
                "prompt",
                None,
                &[],
                None,
                None,
                &mut sink,
                &mut ask,
            ),
        )
        .await
        .unwrap();
        server.await.unwrap();
        if provider_failed {
            let error = result
                .as_ref()
                .err()
                .expect("the real provider returned 503");
            assert!(error.chain().any(|cause| matches!(
                cause.downcast_ref::<kanzei_llm::LlmError>(),
                Some(kanzei_llm::LlmError::Http { status: 503, .. })
            )));
        } else {
            assert_eq!(result.as_ref().unwrap().text, "done");
        }
        assert!(!writer.lock_or_recover().is_terminal());
        assert_eq!(
            store
                .list_events_by_type("ses", 0, "session.assistant_message_committed")
                .unwrap()
                .len(),
            usize::from(!provider_failed)
        );
        (root, store, writer, result)
    }

    async fn completed_outcome_fixture() -> (
        std::path::PathBuf,
        kanzei_core::SessionStore,
        Arc<Mutex<typed_events::TypedEventWriter>>,
        kanzei_core::RunSummary,
    ) {
        let (root, store, writer, result) = runner_outcome_fixture(false, None).await;
        (root, store, writer, result.unwrap())
    }

    #[derive(Clone, Copy, PartialEq)]
    enum OutcomeStop {
        None,
        Cooperative,
        AfterProvider,
        AfterCommit,
    }

    async fn check_durable_outcome(
        rejected: Option<&str>,
        stop: OutcomeStop,
        both_terminals: bool,
    ) {
        let halt_token = kanzei_core::CancellationToken::new();
        let (root, store, writer, result) =
            runner_outcome_fixture(false, Some(halt_token.clone())).await;
        let mut summary = result.unwrap();
        let path = root.join("state.db");
        let sql = rusqlite::Connection::open(&path).unwrap();
        let runtime = Arc::new(crate::SessionRuntime::default());
        *runtime.halt.lock_or_recover() = Some(halt_token.clone());
        runtime
            .running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let stopped = matches!(stop, OutcomeStop::Cooperative | OutcomeStop::AfterProvider);
        if stop == OutcomeStop::Cooperative {
            store.finalize_interrupt("ses").unwrap();
            summary.halted_by_user = true;
        }
        if stop == OutcomeStop::AfterProvider {
            assert!(
                !summary.halted_by_user,
                "runner has already returned success"
            );
            crate::stop_runtime_and_finalize(&runtime, &store, &path, "ses").unwrap();
            assert!(halt_token.is_cancelled());
            runtime
                .running
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(rejected) = rejected {
            let trigger = match rejected {
                "terminal" => "CREATE TRIGGER reject_success BEFORE INSERT ON session_events WHEN NEW.event_type='session.turn_completed' BEGIN SELECT RAISE(ABORT, 'reject terminal'); END",
                "input" => "CREATE TRIGGER reject_success BEFORE UPDATE OF status ON session_inputs WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT, 'reject input'); END",
                "status" => "CREATE TRIGGER reject_success BEFORE UPDATE OF status ON sessions WHEN NEW.status='idle' BEGIN SELECT RAISE(ABORT, 'reject status'); END",
                _ => "CREATE TRIGGER reject_success BEFORE INSERT ON session_events WHEN NEW.event_type='run.completed' BEGIN SELECT RAISE(ABORT, 'reject result'); END",
            };
            sql.execute_batch(trigger).unwrap();
        }
        if both_terminals {
            sql.execute_batch("CREATE TRIGGER reject_failed BEFORE INSERT ON session_events WHEN NEW.event_type='session.turn_failed' BEGIN SELECT RAISE(ABORT, 'reject failed'); END").unwrap();
        }
        let reports = Mutex::new(Vec::new());
        let mobile_messages = Mutex::new(Vec::new());
        let mobile_bodies = Mutex::new(Vec::new());
        let post_success = Mutex::new(Vec::new());
        let harvests = Mutex::new(Vec::new());
        let report = |op: &str, error: String| {
            assert!(
                runtime.lifecycle.try_lock().is_ok(),
                "report holds lifecycle"
            );
            reports.lock_or_recover().push((op.to_owned(), error));
        };
        let mobile = |title: &str, body: &str| {
            assert!(
                runtime.lifecycle.try_lock().is_ok(),
                "notify holds lifecycle"
            );
            // External publication must observe the already committed outcome.
            let expected = if title.ends_with("失败") {
                "run.failed"
            } else {
                "run.completed"
            };
            assert_eq!(
                store.list_events_by_type("ses", 0, expected).unwrap().len(),
                1
            );
            mobile_messages.lock_or_recover().push(title.to_owned());
            mobile_bodies.lock_or_recover().push(body.to_owned());
        };
        let harvest = |messages: &[kanzei_llm::Message]| {
            assert!(
                runtime.lifecycle.try_lock().is_ok(),
                "harvest holds lifecycle"
            );
            harvests.lock_or_recover().push(messages.to_vec());
        };
        let after_success = |episode_id| {
            assert!(
                runtime.lifecycle.try_lock().is_ok(),
                "memory holds lifecycle"
            );
            post_success.lock_or_recover().push(episode_id);
        };
        let live = Arc::new(Mutex::new(LiveRun::default()));
        live.lock_or_recover()
            .begin("run", "input", "prompt", "mock", "mock");
        let resolved = kanzei_harness::config::ResolvedModel {
            provider_name: "mock".into(),
            model: "mock".into(),
            provider: kanzei_harness::config::ProviderConfig {
                protocol: "openai".into(),
                base_url: "http://127.0.0.1:1/v1".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: None,
            },
        };
        let before = store.latest_event_sequence("ses").unwrap();
        let mut run_result = Ok(summary);
        let result = persist_round_outcome_with_effects(
            &path,
            "ses",
            &mut run_result,
            &runtime,
            &halt_token,
            &writer,
            &[],
            &kanzei_harness::ToolCtx::new(root.clone(), root.clone()),
            "prompt",
            &resolved,
            "run",
            "input",
            &std::time::Instant::now(),
            0,
            &live,
            &OutcomeEffects {
                report: &report,
                mobile: &mobile,
                harvest: &harvest,
                after_success: &after_success,
            },
        );
        if stop == OutcomeStop::AfterProvider {
            assert_eq!(
                store
                    .list_events_by_type("ses", 0, "session.turn_stopped")
                    .unwrap()
                    .len(),
                1,
                "the winning stop must commit a durable typed terminal, errors: {:?}",
                writer.lock_or_recover().errors()
            );
        }
        assert_eq!(run_result.as_ref().unwrap().halted_by_user, stopped);
        if stop == OutcomeStop::AfterCommit {
            assert!(result.is_ok());
            crate::stop_runtime_and_finalize(&runtime, &store, &path, "ses").unwrap();
            runtime
                .running
                .store(false, std::sync::atomic::Ordering::SeqCst);
            assert!(store
                .list_events_by_type("ses", 0, "session.turn_stopped")
                .unwrap()
                .is_empty());
            assert!(store
                .list_events_by_type("ses", 0, "session.turn_failed")
                .unwrap()
                .is_empty());
        }
        let notifications = store.replay_notifications("ses", 0, 20).unwrap();
        let episodes = store.list_episodes("ses", 20).unwrap();
        if rejected.is_some() {
            let error = match result {
                Ok(_) => panic!("rejected persistence must not authorize successful finalize"),
                Err(error) => error,
            };
            assert!(error.to_string().contains("reject"), "{error:#}");
            assert!(store
                .list_events_by_type("ses", 0, "session.turn_completed")
                .unwrap()
                .is_empty());
            assert!(store
                .list_events_by_type("ses", 0, "run.completed")
                .unwrap()
                .is_empty());
            assert!(!episodes.iter().any(|x| x.2 == "completed"));
            assert!(!notifications.iter().any(|x| x.status == "succeeded"));
            assert!(post_success.lock_or_recover().is_empty());
            assert!(harvests.lock_or_recover().is_empty());
            if both_terminals {
                assert!(is_uncommitted_outcome(&error));
                assert!(!crate::commands::run::finish_failed_promoted_input(
                    &store, "input", &error
                )
                .unwrap());
                assert!(!writer.lock_or_recover().is_terminal());
                assert_eq!(store.latest_event_sequence("ses").unwrap(), before);
                assert_eq!(
                    store.input_status("input").unwrap().as_deref(),
                    Some("running")
                );
                assert_eq!(store.get_session("ses").unwrap().unwrap().status, "running");
                assert!(notifications.is_empty());
                assert!(episodes.is_empty());
                assert!(mobile_messages.lock_or_recover().is_empty());
            } else {
                assert!(writer.lock_or_recover().is_terminal());
                assert_eq!(
                    store.input_status("input").unwrap().as_deref(),
                    Some("failed")
                );
                assert_eq!(store.get_session("ses").unwrap().unwrap().status, "failed");
                assert_eq!(
                    store
                        .list_events_by_type("ses", 0, "session.turn_failed")
                        .unwrap()
                        .len(),
                    1
                );
                assert_eq!(
                    store
                        .list_events_by_type("ses", 0, "run.failed")
                        .unwrap()
                        .len(),
                    1
                );
                assert_eq!(
                    notifications
                        .iter()
                        .map(|x| x.status.as_str())
                        .collect::<Vec<_>>(),
                    ["failed"]
                );
                assert_eq!(
                    episodes.iter().map(|x| x.2.as_str()).collect::<Vec<_>>(),
                    ["failed"]
                );
                assert_eq!(*mobile_messages.lock_or_recover(), ["kanzei 任务失败"]);
            }
        } else {
            assert!(result.is_ok());
            drop(result);
            assert!(writer.lock_or_recover().is_terminal());
            assert_eq!(
                store.input_status("input").unwrap().as_deref(),
                Some(if stopped { "cancelled" } else { "completed" })
            );
            assert_eq!(store.get_session("ses").unwrap().unwrap().status, "idle");
            assert_eq!(
                store
                    .list_events_by_type(
                        "ses",
                        0,
                        if stopped {
                            "session.turn_stopped"
                        } else {
                            "session.turn_completed"
                        }
                    )
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                store
                    .list_events_by_type("ses", 0, "run.completed")
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                notifications
                    .iter()
                    .map(|x| x.status.as_str())
                    .collect::<Vec<_>>(),
                [if stopped { "stopped" } else { "succeeded" }]
            );
            assert_eq!(
                episodes.iter().map(|x| x.2.as_str()).collect::<Vec<_>>(),
                [if stopped { "halted" } else { "completed" }]
            );
            assert_eq!(post_success.lock_or_recover().len(), usize::from(!stopped));
            assert_eq!(harvests.lock_or_recover().len(), 1);
            assert_eq!(
                *mobile_messages.lock_or_recover(),
                [if stopped {
                    "kanzei 任务已停止"
                } else {
                    "kanzei 任务完成"
                }]
            );
            if stopped {
                assert!(notifications
                    .iter()
                    .all(|notice| notice.summary == "任务已停止" && !notice.requires_action));
                assert!(!notifications
                    .iter()
                    .any(|notice| notice.status == "succeeded"));
                assert_eq!(*mobile_bodies.lock_or_recover(), ["运行已按停止/拒绝收尾"]);
            }
            assert!(reports.lock_or_recover().is_empty());
        }
        drop(writer);
        drop(sql);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    async fn check_provider_error_outcome(stopped: bool) {
        let halt_token = kanzei_core::CancellationToken::new();
        let (root, store, writer, mut original) = if stopped {
            runner_outcome_fixture(true, Some(halt_token.clone())).await
        } else {
            let (root, store, writer, _) = completed_outcome_fixture().await;
            let error = Err(anyhow::Error::from(kanzei_llm::LlmError::Http {
                status: 503,
                body: "temporary outage".into(),
            })
            .context("primary provider"));
            (root, store, writer, error)
        };
        assert!(crate::auto_run::is_transient_run_error(match &original {
            Err(error) => error,
            Ok(_) => unreachable!("fixture starts with a provider failure"),
        }));
        let resolved = kanzei_harness::config::ResolvedModel {
            provider_name: "mock".into(),
            model: "mock".into(),
            provider: kanzei_harness::config::ProviderConfig {
                protocol: "openai".into(),
                base_url: "http://127.0.0.1:1/v1".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: None,
            },
        };
        let report = |_: &str, _: String| {};
        let mobile = |_: &str, _: &str| {
            assert!(
                !stopped,
                "a stopped provider error must not publish success/failure"
            );
        };
        let harvest =
            |_: &[kanzei_llm::Message]| panic!("failed round must not harvest success memory");
        let after_success = |_| panic!("failed round must not schedule success work");
        let live = Arc::new(Mutex::new(LiveRun::default()));
        live.lock_or_recover()
            .begin("run", "input", "prompt", "mock", "mock");
        let runtime = Arc::new(crate::SessionRuntime::default());
        *runtime.halt.lock_or_recover() = Some(halt_token.clone());
        if stopped {
            runtime
                .running
                .store(true, std::sync::atomic::Ordering::SeqCst);
            crate::stop_runtime_and_finalize(&runtime, &store, &root.join("state.db"), "ses")
                .unwrap();
            runtime
                .running
                .store(false, std::sync::atomic::Ordering::SeqCst);
            assert!(halt_token.is_cancelled());
        }
        let controller = crate::auto_run::AutoRunController {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(
            super::super::coordinator::should_retry_failed_outcome(&controller, &halt_token),
            !stopped
        );
        let durable = persist_round_outcome_with_effects(
            &root.join("state.db"),
            "ses",
            &mut original,
            &runtime,
            &halt_token,
            &writer,
            &[],
            &kanzei_harness::ToolCtx::new(root.clone(), root.clone()),
            "prompt",
            &resolved,
            "run",
            "input",
            &std::time::Instant::now(),
            0,
            &live,
            &OutcomeEffects {
                report: &report,
                mobile: &mobile,
                harvest: &harvest,
                after_success: &after_success,
            },
        );
        assert!(
            durable.is_ok(),
            "the durable Failed/Stopped result is a committed outcome, errors: {:?}",
            writer.lock_or_recover().errors()
        );
        let committed_stop = durable.as_ref().unwrap().stopped;
        assert_eq!(committed_stop, stopped);
        drop(durable);
        assert!(crate::auto_run::is_transient_run_error(match &original {
            Err(error) => error,
            Ok(_) => unreachable!("fixture starts with a provider failure"),
        }));
        let caller_error = match original {
            Err(error) if committed_stop => stopped_outcome(error),
            Err(error) => error,
            Ok(_) => unreachable!("the original provider error is retained"),
        };
        assert!(crate::auto_run::is_transient_run_error(&caller_error));
        let idle_reason = crate::commands::run::run_error_idle_reason(&caller_error);
        assert_eq!(idle_reason, if stopped { "stopped" } else { "failed" });
        assert_eq!(
            idle_reason == "failed",
            !stopped,
            "direct caller emits terminal error only for a failed disposition"
        );
        assert!(!crate::commands::run::finish_failed_promoted_input(
            &store,
            "input",
            &caller_error
        )
        .unwrap());
        assert_eq!(
            store.input_status("input").unwrap().as_deref(),
            Some(if stopped { "cancelled" } else { "failed" })
        );
        assert_eq!(
            store.get_session("ses").unwrap().unwrap().status,
            if stopped { "idle" } else { "failed" }
        );
        assert_eq!(
            store
                .list_events_by_type(
                    "ses",
                    0,
                    if stopped {
                        "session.turn_stopped"
                    } else {
                        "session.turn_failed"
                    }
                )
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .list_events_by_type("ses", 0, "run.completed")
                .unwrap()
                .len(),
            usize::from(stopped)
        );
        if stopped {
            assert!(store
                .list_events_by_type("ses", 0, "session.turn_failed")
                .unwrap()
                .is_empty());
            assert!(store
                .list_events_by_type("ses", 0, "session.turn_completed")
                .unwrap()
                .is_empty());
            assert!(store.replay_notifications("ses", 0, 20).unwrap().is_empty());
            assert_eq!(
                store
                    .list_episodes("ses", 20)
                    .unwrap()
                    .iter()
                    .map(|x| x.2.as_str())
                    .collect::<Vec<_>>(),
                ["halted"]
            );
            assert_eq!(
                store
                    .latest_event("ses", "run.completed")
                    .unwrap()
                    .unwrap()
                    .payload["halted_by_user"],
                true
            );
        }
        drop(writer);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn durable_failed_outcome_preserves_original_provider_error_classification() {
        check_provider_error_outcome(false).await;
    }

    #[tokio::test]
    async fn durable_outcome_stop_after_real_provider_error_preserves_error_and_stopped_terminal() {
        check_provider_error_outcome(true).await;
    }

    #[tokio::test]
    async fn durable_outcome_rejects_completed_after_real_provider_success() {
        check_durable_outcome(Some("terminal"), OutcomeStop::None, false).await;
    }
    #[tokio::test]
    async fn durable_outcome_rejects_input_completion_after_real_provider_success() {
        check_durable_outcome(Some("input"), OutcomeStop::None, false).await;
    }
    #[tokio::test]
    async fn durable_outcome_rejects_idle_status_after_real_provider_success() {
        check_durable_outcome(Some("status"), OutcomeStop::None, false).await;
    }
    #[tokio::test]
    async fn durable_outcome_rejects_result_event_after_real_provider_success() {
        check_durable_outcome(Some("event"), OutcomeStop::None, false).await;
    }
    #[tokio::test]
    async fn durable_outcome_does_not_fake_failure_when_both_terminals_rejected() {
        check_durable_outcome(Some("terminal"), OutcomeStop::None, true).await;
    }
    #[tokio::test]
    async fn durable_outcome_commits_before_success_publication() {
        check_durable_outcome(None, OutcomeStop::None, false).await;
    }
    #[tokio::test]
    async fn durable_outcome_accepts_cooperative_stop_with_cancelled_input() {
        check_durable_outcome(None, OutcomeStop::Cooperative, false).await;
    }
    #[tokio::test]
    async fn durable_outcome_stop_wins_after_provider_returns() {
        check_durable_outcome(None, OutcomeStop::AfterProvider, false).await;
    }
    #[tokio::test]
    async fn durable_outcome_stop_after_commit_preserves_completed_input_and_terminal() {
        check_durable_outcome(None, OutcomeStop::AfterCommit, false).await;
    }

    #[tokio::test]
    async fn round_prune_only_surface_commits_before_idle_gc_and_recovers_exactly() {
        let root = std::env::temp_dir().join(format!(
            "kz-round-prune-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state_path = kanzei_core::project_state_path(&root);
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        store
            .create_session("ses-prune", root.to_str().unwrap(), None)
            .unwrap();
        let mut messages = vec![
            kanzei_llm::Message::user_text("任务定义"),
            kanzei_llm::Message::assistant(vec![kanzei_llm::Part::ToolCall {
                id: "old".into(),
                name: "read".into(),
                input: json!({"path":"old.rs"}),
            }]),
            kanzei_llm::Message::tool_results(vec![kanzei_llm::Part::ToolResult {
                call_id: "old".into(),
                content: "x".repeat(8000),
                is_error: false,
            }]),
            kanzei_llm::Message::user_text("第二轮"),
            kanzei_llm::Message::user_text("当前轮"),
        ];
        let original = messages.clone();
        store
            .append_event(
                "ses-prune",
                "conversation.updated",
                &json!({"messages":messages}),
            )
            .unwrap();
        let limits = kanzei_harness::config::Limits {
            prune_protect_tokens: Some(0),
            prune_min_gain_tokens: Some(1),
            ..Default::default()
        };
        let before = kanzei_core::estimate_conversation_tokens(&messages);
        let publication = kanzei_core::store::artifact_liveness::acquire_publication(&root)
            .await
            .unwrap();
        let (cleared, pending) =
            prune_round_surface(&root, &mut messages, &limits, "run-prune", before).unwrap();
        assert_eq!(cleared, 1);
        let conversation = Mutex::new(HashMap::from([("ses-prune".to_string(), original.clone())]));
        assert_eq!(
            conversation.lock_or_recover()["ses-prune"],
            original,
            "pruning stays local until commit"
        );
        assert!(
            before > 1000 && kanzei_core::estimate_conversation_tokens(&messages) < 1000,
            "L0 alone must bring the surface under budget, without any LLM"
        );
        assert!(store.cleanup_storage(&root).is_err());
        let read_only = kanzei_core::SessionStore::open_read_only(&state_path).unwrap();
        assert!(
            pending
                .persist(&read_only, "ses-prune", &messages, &conversation)
                .is_err(),
            "a real SQLite write failure must not publish the local surface"
        );
        assert_eq!(conversation.lock_or_recover()["ses-prune"], original);
        assert_eq!(
            crate::conversation::project_latest_segment(&store, "ses-prune").unwrap(),
            original
        );
        assert!(store
            .latest_completed_compaction_surface("ses-prune", 0)
            .unwrap()
            .is_none());
        drop(read_only);
        pending
            .persist(&store, "ses-prune", &messages, &conversation)
            .unwrap();
        assert_eq!(conversation.lock_or_recover()["ses-prune"], messages);
        drop(publication);
        let cleaned = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                match store.cleanup_storage(&root) {
                    Ok(cleaned) => break cleaned,
                    Err(kanzei_core::StoreError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        tokio::task::yield_now().await;
                    }
                    Err(error) => panic!("unexpected GC failure after commit: {error}"),
                }
            }
        })
        .await
        .expect("publication worker must release after commit");
        assert!(cleaned.deleted_artifacts.is_empty());
        assert_eq!(
            store
                .artifact_cleanup_plan(&root)
                .unwrap()
                .referenced_artifact_files,
            1
        );
        let (_, recovered) = store
            .latest_completed_compaction_surface("ses-prune", 0)
            .unwrap()
            .unwrap();
        assert_eq!(recovered, messages);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn in_loop_prune_surface_is_committed_before_last_active_input_finishes() {
        let root = std::env::temp_dir().join(format!(
            "kz-inloop-prune-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state_path = kanzei_core::project_state_path(&root);
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        store
            .create_session("ses-inloop", root.to_str().unwrap(), None)
            .unwrap();
        let mut messages = vec![
            kanzei_llm::Message::user_text("任务定义"),
            kanzei_llm::Message::assistant(vec![kanzei_llm::Part::ToolCall {
                id: "old".into(),
                name: "read".into(),
                input: json!({"path":"old.rs"}),
            }]),
            kanzei_llm::Message::tool_results(vec![kanzei_llm::Part::ToolResult {
                call_id: "old".into(),
                content: "x".repeat(8000),
                is_error: false,
            }]),
            kanzei_llm::Message::user_text("第二轮"),
            kanzei_llm::Message::user_text("当前轮"),
        ];
        store
            .append_event(
                "ses-inloop",
                "conversation.updated",
                &json!({"messages":messages}),
            )
            .unwrap();
        store.seed_latest_legacy_snapshot("ses-inloop").unwrap();
        store
            .admit_input(
                "ses-inloop",
                "input-inloop",
                "当前轮",
                kanzei_core::Delivery::Queue,
            )
            .unwrap();
        assert!(store.promote_next_input("ses-inloop").unwrap().is_some());
        store.set_status("ses-inloop", "idle").unwrap();
        let original = messages.clone();
        assert_eq!(
            kanzei_core::prune_conversation_with_archive(&mut messages, 0, 1, &root),
            1
        );
        assert!(
            store.cleanup_storage(&root).is_err(),
            "promoted input still protects the in-loop archive"
        );
        assert!(
            kanzei_core::estimate_conversation_tokens(&messages) < 1000,
            "no end-of-round compression will run"
        );
        let summary = kanzei_core::RunSummary {
            text: String::new(),
            usage: Default::default(),
            last_input_tokens: None,
            steps: 1,
            halted_by_user: false,
            step_limit_reached: false,
            messages: messages.clone(),
            context_report: Vec::new(),
            overflow_traces: Vec::new(),
            round_messages: original[3..].to_vec(),
        };
        persist_runner_surface_if_changed(
            &store,
            &root,
            "ses-inloop",
            "run-inloop",
            &original[..3],
            &summary,
        )
        .unwrap();
        store.finish_input("input-inloop", true).unwrap();
        assert!(store
            .cleanup_storage(&root)
            .unwrap()
            .deleted_artifacts
            .is_empty());
        assert_eq!(
            store
                .artifact_cleanup_plan(&root)
                .unwrap()
                .referenced_artifact_files,
            1
        );
        assert_eq!(
            crate::conversation::project_latest_segment(&store, "ses-inloop").unwrap(),
            messages
        );
        let count = store.list_events("ses-inloop", 0).unwrap().len();
        persist_runner_surface_if_changed(
            &store,
            &root,
            "ses-inloop",
            "run-next",
            &original[..3],
            &summary,
        )
        .unwrap();
        assert_eq!(
            store.list_events("ses-inloop", 0).unwrap().len(),
            count,
            "unchanged surface has no duplicate transaction"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn round_prune_without_gain_keeps_surface_and_has_no_pending_transaction() {
        let mut messages = vec![kanzei_llm::Message::user_text("本轮没有旧工具结果")];
        let original = messages.clone();
        assert!(prune_round_surface(
            std::path::Path::new("unused"),
            &mut messages,
            &kanzei_harness::config::Limits::default(),
            "run-normal",
            1
        )
        .is_none());
        assert_eq!(messages, original);
    }

    #[test]
    fn compaction_publishes_only_after_real_source_cas_commit() {
        let root = std::env::temp_dir().join(format!(
            "kz-c5-compaction-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("state.db");
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store
            .create_session("ses", root.to_str().unwrap(), None)
            .unwrap();
        let mut writer = kanzei_core::TypedSessionWriter::new(&path, "ses", "run");
        writer.user_message("input", kanzei_llm::Message::user_text("old source"));
        writer.turn_started(1, 1);
        writer.assistant_committed(
            1,
            kanzei_llm::Message::assistant(vec![kanzei_llm::Part::Text {
                text: "answer".into(),
            }]),
        );
        writer.finish(kanzei_core::SessionTurnTerminal::Completed);
        let source = crate::conversation::project_latest_segment(&store, "ses").unwrap();
        let pending = RoundCompaction {
            transaction_id: "round-compact".into(),
            summary: json!({"digest":"summary","dropped":2,"before":3000,"after":1000}),
            source_surface: source.clone(),
        };
        let mobile = kanzei_llm::Message::user_text("late phone");
        store
            .append_mobile_message(
                "ses",
                "phone",
                mobile.clone(),
                &json!({"text":"late phone"}),
            )
            .unwrap();
        let current = crate::conversation::project_latest_segment(&store, "ses").unwrap();
        assert_eq!(current.last(), Some(&mobile));
        let cache = Mutex::new(HashMap::from([("ses".to_owned(), current.clone())]));
        let publication = Mutex::new(Vec::new());
        let surface = vec![kanzei_llm::Message::user_text("summary surface")];
        let publish = |payload: &serde_json::Value| {
            assert_eq!(
                crate::conversation::project_latest_segment(&store, "ses").unwrap(),
                surface
            );
            assert_eq!(cache.lock_or_recover()["ses"], surface);
            publication.lock_or_recover().push(payload.clone());
        };
        assert!(pending
            .persist_and_publish(&store, "ses", &surface, &cache, &publish)
            .is_err());
        assert!(publication.lock_or_recover().is_empty());
        assert_eq!(cache.lock_or_recover()["ses"], current);
        let pending = RoundCompaction {
            source_surface: current,
            ..pending
        };
        let sql = rusqlite::Connection::open(&path).unwrap();
        sql.execute_batch("CREATE TRIGGER reject_surface BEFORE INSERT ON session_events WHEN NEW.event_type='surface_replaced' BEGIN SELECT RAISE(ABORT, 'reject surface'); END").unwrap();
        assert!(pending
            .persist_and_publish(&store, "ses", &surface, &cache, &publish)
            .is_err());
        assert!(publication.lock_or_recover().is_empty());
        sql.execute_batch("DROP TRIGGER reject_surface").unwrap();
        pending
            .persist_and_publish(&store, "ses", &surface, &cache, &publish)
            .unwrap();
        assert_eq!(
            *publication.lock_or_recover(),
            [json!({"summary":"summary","dropped":2,"before":3000,"after":1000})]
        );
        drop(sql);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn late_mobile_fact_is_not_replaced_by_ordinary_or_stale_compacted_summary() {
        let root = std::env::temp_dir().join(format!(
            "kz-late-fact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = kanzei_core::project_state_path(&root);
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store
            .create_session("ses", root.to_str().unwrap(), None)
            .unwrap();
        let user = kanzei_llm::Message::user_text("本轮请求");
        let answer = kanzei_llm::Message::assistant(vec![kanzei_llm::Part::Text {
            text: "本轮完成".into(),
        }]);
        let mut writer = kanzei_core::TypedSessionWriter::new(&path, "ses", "run");
        writer.user_message("input", user.clone());
        writer.turn_started(1, 1);
        writer.assistant_committed(1, answer.clone());
        writer.finish(kanzei_core::SessionTurnTerminal::Completed);
        assert!(writer.errors().is_empty(), "{:?}", writer.errors());
        let source = vec![user, answer];
        let mobile = kanzei_llm::Message::user_text("后到的手机输入");
        let mut invariant = kanzei_core::SessionInvariant::default();
        for (_, fact) in store.list_session_facts("ses").unwrap() {
            invariant.apply(&fact).unwrap();
        }
        store
            .append_session_facts_checked(
                "ses",
                &mut invariant,
                &[kanzei_core::SessionFactEnvelope::new(
                    "mobile",
                    None,
                    kanzei_core::SessionFact::UserMessageCommitted {
                        input_id: "mobile-input".into(),
                        message: mobile.clone(),
                    },
                )],
            )
            .unwrap();
        let current = crate::conversation::project_latest_segment(&store, "ses").unwrap();
        let cache = Mutex::new(HashMap::from([("ses".to_string(), current.clone())]));
        let count = store.list_events("ses", 0).unwrap().len();
        let mut summary = kanzei_core::RunSummary {
            text: String::new(),
            usage: Default::default(),
            last_input_tokens: None,
            steps: 1,
            halted_by_user: false,
            step_limit_reached: false,
            messages: source.clone(),
            context_report: Vec::new(),
            overflow_traces: Vec::new(),
            round_messages: source.clone(),
        };
        persist_runner_surface_if_changed(&store, &root, "ses", "run", &[], &summary).unwrap();
        summary.messages = vec![kanzei_llm::Message::user_text("旧 source 的压缩结果")];
        persist_runner_surface_if_changed(&store, &root, "ses", "run-stale", &[], &summary)
            .unwrap();
        let pending = RoundCompaction {
            transaction_id: "end-stale".into(),
            summary: json!({}),
            source_surface: source,
        };
        assert!(pending
            .persist(&store, "ses", &summary.messages, &cache)
            .is_err());
        assert_eq!(cache.lock_or_recover()["ses"], current);
        assert_eq!(store.list_events("ses", 0).unwrap().len(), count);
        assert_eq!(
            crate::conversation::project_latest_segment(&store, "ses")
                .unwrap()
                .last(),
            Some(&mobile)
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pipeline_aggregate_does_not_duplicate_first_user_or_publish_unmatched_source() {
        let root = std::env::temp_dir().join(format!(
            "kz-pipeline-source-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = kanzei_core::project_state_path(&root);
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store
            .create_session("ses", root.to_str().unwrap(), None)
            .unwrap();
        let prior = vec![kanzei_llm::Message::user_text("旧任务定义")];
        store
            .append_event("ses", "conversation.updated", &json!({"messages":prior}))
            .unwrap();
        store.seed_latest_legacy_snapshot("ses").unwrap();
        let user = kanzei_llm::Message::user_text("当前用户请求");
        let answer = kanzei_llm::Message::assistant(vec![kanzei_llm::Part::Text {
            text: "实施完成".into(),
        }]);
        let fixup_user = kanzei_llm::Message::user_text("内部修正请求");
        let fixed = kanzei_llm::Message::assistant(vec![kanzei_llm::Part::Text {
            text: "修正完成".into(),
        }]);
        let mut writer = kanzei_core::TypedSessionWriter::new(&path, "ses", "run");
        writer.user_message("input", user.clone());
        writer.turn_started(1, 2);
        writer.assistant_committed(1, answer.clone());
        // Actual pipeline fixup emits assistant events but does not add a typed
        // initial-user fact for its internal prompt. It cannot certify a changed source.
        writer.turn_started(2, 2);
        writer.assistant_committed(2, fixed.clone());
        writer.finish(kanzei_core::SessionTurnTerminal::Completed);
        assert!(writer.errors().is_empty(), "{:?}", writer.errors());
        let mut receipt = vec![user.clone(), answer];
        receipt.extend([fixup_user, fixed]); // execution.rs merges the two receipts.
        let mut messages = prior.clone();
        messages.extend(receipt.iter().cloned());
        assert_eq!(
            messages.iter().filter(|message| **message == user).count(),
            1
        );
        let committed = crate::conversation::project_latest_segment(&store, "ses").unwrap();
        let cache = Mutex::new(HashMap::from([("ses".to_string(), committed.clone())]));
        let count = store.list_events("ses", 0).unwrap().len();
        let mut summary = kanzei_core::RunSummary {
            text: String::new(),
            usage: Default::default(),
            last_input_tokens: None,
            steps: 2,
            halted_by_user: false,
            step_limit_reached: false,
            messages,
            context_report: Vec::new(),
            overflow_traces: Vec::new(),
            round_messages: receipt,
        };
        persist_runner_surface_if_changed(&store, &root, "ses", "run", &prior, &summary).unwrap();
        let source_surface = summary.messages.clone();
        summary.messages = vec![kanzei_llm::Message::user_text("来源未匹配的压缩候选")];
        persist_runner_surface_if_changed(&store, &root, "ses", "run-changed", &prior, &summary)
            .unwrap();
        let pending = RoundCompaction {
            transaction_id: "pipeline".into(),
            summary: json!({}),
            source_surface,
        };
        assert!(pending
            .persist(&store, "ses", &summary.messages, &cache)
            .is_err());
        assert_eq!(store.list_events("ses", 0).unwrap().len(), count);
        assert_eq!(cache.lock_or_recover()["ses"], committed);
        assert_eq!(
            crate::conversation::project_latest_segment(&store, "ses").unwrap(),
            committed
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn transaction_budget_result_records_actual_actions_and_outcome() {
        let root = std::env::temp_dir().join(format!(
            "kz-transaction-result-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state_path = root.join("state.db");
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        store.create_session("ses-result", "C:/proj", None).unwrap();
        store
            .append_event(
                "ses-result",
                "run.trace",
                &json!({
                    "run_id": "run-result",
                    "events": [
                        {"kind": "transaction_budget.extended"},
                        {"kind": "tool.completed", "name": "git"},
                        {"kind": "tool.completed", "name": "req"}
                    ]
                }),
            )
            .unwrap();

        record_transaction_budget_result(&store, "ses-result", "run-result", "completed");

        let results = store
            .list_events_by_type("ses-result", 0, "run.transaction_budget_result")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].payload["result"], "completed");
        assert_eq!(results[0].payload["actual_actions"], json!(["git", "req"]));
        std::fs::remove_dir_all(root).ok();
    }
}
