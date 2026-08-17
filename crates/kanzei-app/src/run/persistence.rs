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

use crate::{flush_live_run, flush_live_trace, memory, typed_events, with_session_id, LiveRun};

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
    /// 本轮是否流水线路径:决定写租约 Released 事件是否由本函数补发——流水线路径的
    /// 租约归编排对象管(复核屏障/收尾已各发一次),再发一条会在轨迹里重复释放。
    pub(crate) phase_pipeline_enabled: bool,
}

/// R-253 批7b:`finalize_round` 参数分组——**本轮结果层**:摘要/历史长/工具画像/
/// kz:done 载荷。生命周期:本轮级,轮末一次性消费。
pub(crate) struct FinalizeOutcome<'a> {
    pub(crate) summary: &'a kanzei_core::RunSummary,
    pub(crate) history_len: usize,
    pub(crate) this_run_tools: &'a std::collections::BTreeMap<String, usize>,
    pub(crate) auto_action_json: &'a serde_json::Value,
}

/// R-253 批7b:`finalize_round` 参数分组——**UI 汇报层**:事件投影窗口/进度闭包/
/// live 画像。生命周期:会话级,与运行域之外的 AppState 共享。
pub(crate) struct RoundReport<'a> {
    pub(crate) window: &'a tauri::Window,
    pub(crate) stage: &'a (dyn Fn(&str, String) + Sync),
    pub(crate) live: &'a Arc<Mutex<LiveRun>>,
}

/// 轮末落库:状态/事件/episode/通知(原 run.rs persist_round_outcome)。
/// 注:不能收 SessionContext/RoundContext 整体——run_task 内部分字段被 move 给
/// 子函数后 struct 不可整体借用,故仍传展开字段(保留 allow)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn persist_round_outcome(
    state_path: &std::path::Path,
    window: &tauri::Window,
    session_id: &str,
    run_result: &Result<kanzei_core::RunSummary, anyhow::Error>,
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
) -> Option<kanzei_core::SessionStore> {
    let final_store = match kanzei_core::SessionStore::open(state_path) {
        Ok(store) => Some(store),
        Err(error) => {
            report_persistence_failure(window, session_id, "打开会话数据库", error);
            None
        }
    };
    if let Some(store) = final_store.as_ref() {
        match run_result {
            Ok(summary) => {
                typed_writer
                    .lock()
                    .unwrap()
                    .finish(if summary.halted_by_user {
                        typed_events::TerminalFact::Stopped
                    } else {
                        typed_events::TerminalFact::Completed
                    });
                if let Err(error) = store.set_status(session_id, "idle") {
                    report_persistence_failure(window, session_id, "写入 idle 状态", error);
                }
                if let Err(error) = store.append_event(
                    session_id,
                    "session.status_changed",
                    &json!({ "status": "idle" }),
                ) {
                    report_persistence_failure(window, session_id, "写入完成状态事件", error);
                }
                if let Err(error) = store.append_event(
                    session_id,
                    "run.completed",
                    &json!({
                        "steps": summary.steps,
                        "halted_by_user": summary.halted_by_user,
                        "input": summary.usage.input,
                        "output": summary.usage.output,
                        // 上下文账单(R-106):各注入源字符数,UI 与度量共用。
                        "context": summary.context_report,
                    }),
                ) {
                    report_persistence_failure(window, session_id, "写入完成事件", error);
                }
                // 本轮切片:summary.messages = prior + 本轮;统计与失败提炼都只看本轮,
                // 否则历史失败反复上报、工具计数累计全历史(R-099 基线失真)。
                let this_run = &summary.messages[prior.len().min(summary.messages.len())..];
                // 轮末采集(D-229/D-214):CLI 与桌面端共用 harvest_end_of_run——失败提炼
                // → 条目收口判定 → SOP 候选(项目 inbox,落库目标 global)→ 根因 fact
                // 候选(项目 inbox)。候选箱语义不变:SOP 只产候选等用户一键采纳,agent 不自决入库。
                kanzei_tools::memory::harvest_end_of_run(&ctx.project_root, prompt, this_run);
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
                    context_json: &serde_json::to_string(&summary.context_report)
                        .unwrap_or_default(),
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
                    overflow_json: &serde_json::to_string(&summary.overflow_traces)
                        .unwrap_or_default(),
                }) {
                    // R-161:本轮开跑预检索的 recall_events 归因到该 episode,可 join 查询。
                    let _ = store.link_recall_events_to_episode(episode_id, run_epoch_ms);
                    current_episode_id = Some(episode_id);
                }
                let _ = store.finish_input(promoted_input_id, true);
                // 富 episode(带工具画像/上下文账单)已写,标记防重:停止路径的
                // flush_live_run 不该再补一条信息量更少的(D-179)。
                live.lock().unwrap().flushed = true;
                if let Err(error) =
                    append_run_notification(store, session_id, "succeeded", "任务完成", false)
                {
                    report_persistence_failure(window, session_id, "写入完成通知", error);
                }
                // R-270 批4:完成事件经现成 LAN 推送桥发手机系统通知(尽力而为,
                // 无桥时只记诊断不阻塞)。
                if let Ok(message) =
                    crate::mobile_notify::notify_mobile("kanzei 任务完成", "运行已成功结束")
                {
                    tracing::debug!("{message}");
                }
                // 轮末记忆整理(R-105):独立任务消化 inbox 草稿,不阻塞完成事件。
                // 传**主根**:记忆是主根一份的资产,而 project_dir 线上线后是 worktree,
                // 传它会让 memory 内部的发现式取根拐进分支副本(R-177 内容⑧同一条判据)。
                let project_dir = ctx.project_root.display().to_string();
                tauri::async_runtime::spawn(async move {
                    match memory::consolidate_memory_inbox(project_dir, current_episode_id).await {
                        Ok(report) if report.has_failures() => {
                            tracing::warn!("{}", report.summary());
                        }
                        Ok(report) => tracing::debug!("{}", report.summary()),
                        Err(error) => tracing::warn!("memory inbox consolidation failed: {error}"),
                    }
                });
                // D-341/R-195:轮末自动处置 candidate——有真实当轮 episode 且复发≥3 的
                // 自动 promote,超期未处置的自动 deprecated 归档,其余保持 candidate。
                // 与 inbox 消化解耦(没有草稿也要跑)且机械判定不走 LLM;失败不阻塞收尾。
                let _ = kanzei_tools::memory::reconcile_candidates(
                    &ctx.project_root,
                    current_episode_id,
                    kanzei_tools::memory::CANDIDATE_MAX_AGE_DAYS,
                );
            }
            Err(error) => {
                typed_writer
                    .lock()
                    .unwrap()
                    .finish(typed_events::TerminalFact::Failed(error.to_string()));
                typed_writer.lock().unwrap().write_shadow_report(prior);
                if let Err(persistence_error) = store.set_status(session_id, "failed") {
                    report_persistence_failure(
                        window,
                        session_id,
                        "写入失败状态",
                        persistence_error,
                    );
                }
                if let Err(persistence_error) = store.append_event(
                    session_id,
                    "session.status_changed",
                    &json!({ "status": "failed" }),
                ) {
                    report_persistence_failure(
                        window,
                        session_id,
                        "写入失败状态事件",
                        persistence_error,
                    );
                }
                if let Err(persistence_error) = store.append_event(
                    session_id,
                    "run.failed",
                    &json!({ "error": error.to_string() }),
                ) {
                    report_persistence_failure(
                        window,
                        session_id,
                        "写入失败事件",
                        persistence_error,
                    );
                }
                // 失败轮次原先在 `let summary = run_result?;` 处提前返回,轨迹与
                // episode 一并丢失——和被停止的轮次是同一个洞(D-179)。
                flush_live_run(store, session_id, live, "failed");
                let _ = store.finish_input(promoted_input_id, false);
                if let Err(persistence_error) =
                    append_run_notification(store, session_id, "failed", error.to_string(), false)
                {
                    report_persistence_failure(
                        window,
                        session_id,
                        "写入失败通知",
                        persistence_error,
                    );
                }
                // R-270 批4:失败事件经 LAN 推送桥发手机系统通知(尽力而为)。
                if let Ok(message) = crate::mobile_notify::notify_mobile(
                    "kanzei 任务失败",
                    &format!("运行失败: {error}"),
                ) {
                    tracing::debug!("{message}");
                }
            }
        }
    }
    final_store
}

/// R-202 批2:run_task 轮末收尾段后半——对话落库 → 轮末压缩(R-236 B1/B4)→
/// conversation.updated 与 typed shadow 报告 → kz:done → 写租约 Released →
/// 停止令牌回收。行为零变更:压缩触发线/口径/事件顺序与内联时一致。
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
    subagent_rt: &Option<kanzei_core::SubagentRuntime>,
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
    let phase_pipeline_enabled = round.phase_pipeline_enabled;
    let final_store = session.final_store;
    let typed_flush_task = session.typed_flush_task;
    let history_len = outcome.history_len;
    let this_run_tools = outcome.this_run_tools;
    let auto_action_json = outcome.auto_action_json;
    let config = &deps.config;
    let resolved = &deps.resolved;
    let client = &deps.client;
    let halt_slot = &handles.halt_slot;
    conversation
        .lock()
        .unwrap()
        .insert(session_id.to_string(), summary.messages.clone());

    // R-236 B1:轮末压缩走 core 同一份 compact_with_digest——保任务定义、保近期
    // 工作区逐字、只压中段、纪要过质量闸,失败回落原文节选。R-021 那套「整段历史
    // → 单条 300 字纪要」已删:那正是 D-181 在 core 侧修掉的失败模式(压完模型
    // 不知道自己做过什么),也是用户实测「打断插任务模型失忆」的主因之一。
    // 触发线与轮内同一把尺(compaction_budget:limit − max(output, buffer));
    // 估算同一口径(附件按固定成本,不按 base64 字节——消灭带附件必误触发)。
    if let Some(limit) = resolved.provider.context_limit {
        let budget = kanzei_core::compaction_budget(
            limit,
            config.limits.max_tokens(),
            config.limits.compact_buffer_tokens(),
        );
        let mut conv = conversation
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        let mut estimate = compaction_input_tokens(summary.last_input_tokens, &conv);
        // R-236 B4:轮末同样 L0 先行——机械清旧工具结果,清完够线就不动 LLM 纪要。
        if estimate > budget && conv.len() > 1 {
            let cleared = kanzei_core::prune_conversation(
                &mut conv,
                config.limits.prune_protect_tokens(),
                config.limits.prune_min_gain_tokens(),
            );
            if cleared > 0 {
                let after_prune = kanzei_core::estimate_conversation_tokens(&conv);
                stage(
                    "压缩",
                    format!(
                        "已机械清理 {cleared} 条旧工具结果({}k → {}k token)",
                        estimate / 1000,
                        after_prune / 1000
                    ),
                );
                estimate = after_prune;
                conversation
                    .lock()
                    .unwrap()
                    .insert(session_id.to_string(), conv.clone());
            }
        }
        if estimate > budget && conv.len() > 1 {
            stage(
                "压缩",
                format!(
                    "会话历史约 {}k token 超预算 {}k(上限 {}k),压缩中段…",
                    estimate / 1000,
                    budget / 1000,
                    limit / 1000
                ),
            );
            let mut compact_traces = Vec::new();
            let dropped = kanzei_core::compact_conversation(
                client,
                subagent_rt.as_ref(),
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
                conversation
                    .lock()
                    .unwrap()
                    .insert(session_id.to_string(), conv);
                // 被压段的轨迹摘要随轮末落 live trace,复盘可查(与轮内 overflow 同源语义)。
                for trace in compact_traces {
                    let mut live = live.lock().unwrap();
                    live.trace
                        .push(json!({ "kind": "compaction.dropped", "detail": trace }));
                }
                stage(
                    "压缩",
                    format!(
                        "压缩完成:{}k → {}k token,压掉 {dropped} 条中段消息",
                        estimate / 1000,
                        after / 1000
                    ),
                );
                let _ = window.emit(
                    "kz:compacted",
                    with_session_id(
                        json!({ "summary": digest_preview, "dropped": dropped, "before": estimate, "after": after }),
                        session_id,
                    ),
                );
            } else {
                // 中段为空压不动(超线来自任务定义/近期工作区本身):保留原历史,
                // 交给轮内的 trim_tail/被动恢复,不在轮末冒进。
                stage("压缩", "中段为空压不动,保留原历史".into());
            }
        }
    }

    let messages = conversation
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .unwrap_or_default();
    if let Some(store) = final_store.as_ref() {
        // 轨迹已在运行中按事件增量写入；这里仅补写实时写入失败的尾部，避免
        // 轮末再把整轮复制一遍造成回放重复。
        flush_live_trace(store, session_id, live);
        // 轮末快照继续写入(验收⑦顺延:上下文压缩摘要仍经 conversation.updated
        // 持久化,停止前需 compaction 事件化,见 R-242 进展)。
        if let Err(error) = store.append_event(
            session_id,
            "conversation.updated",
            &json!({ "messages": messages }),
        ) {
            report_persistence_failure(window, session_id, "写入对话历史", error);
        }
        typed_writer.lock().unwrap().write_shadow_report(&messages);
    }
    typed_flush_task.abort();
    let _ = window.emit(
        "kz:done",
        with_session_id(
            json!({
                "steps": summary.steps,
                "halted": summary.halted_by_user,
                "history": history_len,
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
    // 批6:**仅非流水线路径**发这一条。流水线路径的租约归编排对象管,它在复核屏障
    // 和收尾时已经各发过一次 released——这里再发一条会在轨迹里凭空多出一次释放,
    // 回放时看起来像"释放了两次"。
    if !phase_pipeline_enabled {
        writer_event(
            kanzei_harness::orchestration::OrchestrationEvent::WriterReleased {
                project_root: ctx.project_root.clone(),
                run_id: run_id.to_string(),
                process_id: process_id.to_string(),
            },
        );
        // 正常路径已落 Released,标记 guard 避免 Drop 重复补写(D-303)。
        if let Some(trace) = _write_lease {
            trace.mark_released();
        }
    }
    // D-342:本 run 收尾,收回停止令牌(stop 已 take 过则本来就是 None,幂等)。
    halt_slot.lock().unwrap().take();
    Ok(())
}
