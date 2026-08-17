//! 运行协调域(R-253 批6,纯搬迁自 run/mod.rs)。
//!
//! 独立理由:run_task 是 Round Coordinator——把装配/事件循环/轮末收尾三段编排起来,
//! 并持有「同一轮怎么衔接」的决策(prior 恢复、子代理运行时构造、自动推进判定、
//! 自动 push)。它与装配(assembly)、执行(execution)、落库(persistence)各自独立:
//! 协调回答「这段编排怎么走」,装配回答「需要什么」,执行回答「怎么跑」,
//! 落库回答「跑完怎么落」(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):③`prior` 的恢复必须留在这里——`SessionStore` 非 `Sync`,
//! 跨 `await` 持引用会破坏 future 的 `Send` 约束;`run_execution_loop` 只消费
//! `&[Message]`。⑤`_write_lease` RAII guard 的 Release 事件配对见 persistence.rs。
//! ⑨`stage` 闭包签名保持 `&(dyn Fn(&str, String) + Sync)`。

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde_json::json;
use tauri::Emitter;

use crate::{conversation, record_live_trace, with_session_id};

use super::assembly::{assemble_run, RoundRequest, RunAssembly, RunMode, RuntimeHandles};
use super::events::{
    build_ask_handler, build_event_handler, MetricsSink, TraceSink, TypedEventSink, UiEventSink,
};
use super::execution::run_execution_loop;
use super::persistence::{
    finalize_round, persist_round_outcome, FinalizeOutcome, FinalizeRound, FinalizeSession,
    RoundReport,
};
use super::{emit_stage, maybe_push_after_commit};

/// R-202 批2:run_task(原 run.rs 的 Round Coordinator)。装配 → 事件循环 → 轮末收尾。
/// R-253 批7b:调用参数按生命周期三分——`RoundRequest`(本轮输入)/`RunMode`(运行档位)/
/// `RuntimeHandles`(会话级句柄),共 4 参,消 too_many。三组均见 assembly.rs 的
/// 生命周期说明,禁止再退化成 30+ 扁平参数。
pub(crate) async fn run_task(
    window: &tauri::Window,
    request: RoundRequest,
    mode: RunMode,
    handles: RuntimeHandles,
) -> anyhow::Result<()> {
    // 阶段汇报:让前端每一步都有着落(用户反馈:要详细指示)。
    // session_id/process_id 在 request 整体移入装配后仍需使用,先克隆供全程引用;
    // phase_pipeline_enabled/autonomous 是运行档位中轮末/执行段仍要消费的残留位。
    let session_id = request.session_id.clone();
    let process_id = request.process_id.clone();
    let phase_pipeline_enabled = mode.phase_pipeline_enabled;
    let subagents_enabled = mode.subagents_enabled;
    let autonomous = mode.autonomous;
    let stage = |name: &str, detail: String| {
        *handles.current_stage.lock().unwrap() = name.to_string();
        emit_stage(window, &session_id, name, detail);
    };

    // D-342:换代 + 安装本 run 的停止令牌。换代在前——stop 的兜底硬杀按代数比对,
    // 装了新令牌还留着旧代数会让上一次停止的兜底误杀本 run。
    handles.run_generation.fetch_add(1, Ordering::SeqCst);
    let halt_token = kanzei_core::CancellationToken::new();
    *handles.halt_slot.lock().unwrap() = Some(halt_token.clone());

    let RunAssembly {
        deps,
        session,
        mut round,
    } = assemble_run(window, &stage, request, mode, &handles, halt_token).await?;

    // R-253 批7:RunAssembly 三分后按需取字段——session 的 move 型字段(store/
    // typed_flush_task)取出,其余经引用访问;deps/round 保持整体(不再解构),
    // 供执行循环与轮末收尾按生命周期分组传入。
    let state_path = session.state_path.clone();
    // SessionStore 非 Sync:recover_messages 同步用后即弃,不跨 await 持引用
    // (危险点③——跨 await 持引用会破坏 future Send 约束)。move owned 而非借用。
    let store = session.store;
    let promoted_input_id = session.promoted_input_id.clone();
    let prompt = session.prompt.clone();
    let initial_parts = &session.initial_parts;
    let typed_writer = session.typed_writer.clone();
    let typed_flush_task = session.typed_flush_task;
    // round 在 run_execution_loop 期间整体被 &mut 借用(执行循环驱动 round.pipeline),
    // 轮末字段(ctx/_write_lease/run_started/run_epoch_ms)在调用返回后移出;
    // 这里只 clone 调用前后都要用的身份字段。
    let run_id = round.run_id.clone();
    let orchestration_trace = round.orchestration_trace.clone();

    let event_window = window.clone();
    let session_id_for_events = session_id.clone();
    let emit_event = move |name: &str, payload: serde_json::Value| {
        event_window.emit(name, with_session_id(payload, &session_id_for_events))
    };
    // R-202 批1:writer 事件闭包原在装配段内联,随 assemble_run 收敛后由
    // 解构出的 orchestration_trace 重建——单一出口语义不变(OrchestrationEvent 落
    // session_events),正常路径收尾与 WriterLeaseTrace::drop 兜底都用它。
    let writer_event = |event: kanzei_harness::orchestration::OrchestrationEvent| {
        use kanzei_harness::orchestration::PhaseObserver;
        orchestration_trace.observe(&event);
    };
    // 轨迹与统计写进 runtime 的 live 画像,停止路径才够得着(D-179)。
    let live = handles.live_run.clone();
    live.lock().unwrap().begin(
        &run_id,
        &promoted_input_id,
        &prompt,
        &deps.resolved.provider_name,
        &deps.resolved.model,
    );
    let trace_log = live.clone();
    // D-173 可观测性:主代理的工具调用原先只实时发给 UI,一条也不落库——
    // 于是"时间花在模型、shell 还是等用户""用户点了几次权限"事后统统无从查证,
    // 只能从最终对话快照反推。这里按 id 记开始时刻,收尾时连耗时一起写进 run.trace。
    let tool_started: Arc<Mutex<HashMap<String, std::time::Instant>>> =
        Arc::new(Mutex::new(HashMap::new()));
    // R-143:自举循环批次提交后自动 push 的检测位。ToolStart(action=commit)置 pending,
    // ToolEnd(ok=true)把 pending 提升为 committed;失败/非 commit 只清 pending。
    // 轮末(decide_auto_run 之后)读 committed,true 才触发 push。
    let committed_this_round = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pending_commit_call = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // D-361:本轮子代理内部用过的工具名。事件处理器边跑边收,轮末并进鞭挞的工具
    // 画像——委派出去的活也是活,不能因为主轮只留下一个 task 调用就判成空转。
    let subagent_tools: Arc<Mutex<std::collections::BTreeSet<String>>> =
        Arc::new(Mutex::new(std::collections::BTreeSet::new()));
    // R-253 批8:事件处理器按投影拆四 sink——UI/typed/trace/metrics 各自持有
    // 自己的状态,新增 RunEvent 只碰对应 sink(验收⑤)。
    let mut on_event = build_event_handler(
        UiEventSink::new(emit_event),
        TypedEventSink::new(typed_writer.clone()),
        TraceSink::new(trace_log, state_path.clone(), session_id.clone()),
        MetricsSink::new(
            tool_started,
            committed_this_round.clone(),
            pending_commit_call,
            subagent_tools.clone(),
        ),
    );

    let mut ask = build_ask_handler(
        handles.asks.clone(),
        handles.ask_seq.clone(),
        deps.ask_source,
        window,
        round.ctx.project_root.clone(),
        session_id.clone(),
    );

    // 会话连续:同项目续上内存历史；应用重启后从事件日志恢复最近一次完整消息投影。
    // (同步完成——SessionStore 非 Sync,跨 await 持引用会破坏 future Send 约束,
    // 故 prior 在 run_task 恢复、run_execution_loop 只消费它,行为与内联时一致。)
    // R-242 批6/7:runner prior 真源切到事件投影(最新 segment surface,reset 后
    // 新段 prior 为空);gate 回退时走 legacy recover_messages(快照 + filter)。
    let persisted = if crate::projection_gate::read_path_uses_projection("runner_prior") {
        conversation::project_latest_segment(&store, &session_id)
            .map_err(|error| anyhow::anyhow!(error))?
    } else {
        conversation::recover_messages(&store, &session_id)?
    };
    let prior = conversation::conversation_prior(&handles.conversation, &session_id, persisted);
    if !prior.is_empty() {
        stage("会话", format!("延续对话({} 条历史消息)", prior.len()));
    }

    // task 子代理运行时:独立只读快照;fast 角色缺席时两个档位都退回主模型。
    // **无条件构造**(2026-08-11 用户定调):模型自己派 `task` 这条路永远开着,不受
    // 「勘察复核」开关控制。构造与 prior 恢复无数据依赖(顺序互换行为不变,失败
    // 同样提前终止本轮),故先构造再进 run_execution_loop。
    // task 子代理运行时:R-256 与 CLI 共用 kanzei_tools::run::build_subagent_runtime,
    // 桌面差异(登记读槽 R-171 批6、单条停止注册表 R-174)经参数注入。
    // R-279:子代理 transcript 事件落库/恢复回调——sink 写主会话 session_events
    // (subagent.transcript 快照事件),provider 按 subagent_transcript gate 从事件
    // 恢复续跑 prior(gate 关回退进程内 TranscriptStore)。
    let transcript_sink_state = state_path.clone();
    let transcript_sink_session = session_id.clone();
    let transcript_sink: Option<kanzei_core::BackgroundEventSink> = Some(std::sync::Arc::new(
        move |_call_id: &str, payload: serde_json::Value| {
            if let Ok(store) = kanzei_core::SessionStore::open(&transcript_sink_state) {
                let _ =
                    store.append_event(&transcript_sink_session, "subagent.transcript", &payload);
            }
        },
    ));
    let transcript_provider_state = state_path.clone();
    let transcript_provider_session = session_id.clone();
    let transcript_provider: Option<kanzei_core::SubagentTranscriptProvider> = Some(
        std::sync::Arc::new(move |call_id: &str| -> Option<Vec<kanzei_llm::Message>> {
            if !crate::projection_gate::read_path_uses_projection("subagent_transcript") {
                return None; // gate 关:回退进程内 TranscriptStore,行为与切换前一致
            }
            let store = kanzei_core::SessionStore::open(&transcript_provider_state).ok()?;
            store
                .recover_subagent_transcript(&transcript_provider_session, call_id)
                .ok()
                .flatten()
        }),
    );
    let subagent_rt = if subagents_enabled {
        kanzei_tools::run::build_subagent_runtime(
            &deps.rctx,
            &deps.config,
            &deps.proxy,
            &deps.resolved,
            &deps.route,
            Some(Arc::clone(&handles.coordinator)
                as Arc<
                    dyn kanzei_harness::orchestration::ProjectExecutionCoordinator,
                >),
            Some(handles.task_cancellations.clone()),
            transcript_sink,
            transcript_provider,
        )
        .await?
    } else {
        None
    };

    // R-202 批2:事件循环段——附件提示 → 记忆预检索 → 勘察 → 主循环
    // (run_once_with_parts)→ 复核修正(run_review_and_fixup),收敛为独立函数。
    // R-253 批7b:执行输入打包(ExecutionInput)+ 生命周期分组(deps/round)。
    let execution_input = crate::run::execution::ExecutionInput {
        stage: &stage,
        initial_parts,
        prompt: &prompt,
        autonomous,
        subagent_rt: &subagent_rt,
        prior: &prior,
    };
    let run_result =
        run_execution_loop(&deps, &mut round, &execution_input, &mut on_event, &mut ask).await;
    // R-253 批7b:调用返回后 round 可整体取回——执行循环只借用了 round.ctx 与
    // round.pipeline(不相交字段),其余轮末字段在此移出供收尾段使用。
    let run_started = round.run_started;
    let run_epoch_ms = round.run_epoch_ms;
    let ctx = round.ctx;
    let _write_lease = round._write_lease;
    // R-202 批2:轮末收尾段前半(终态落库:typed 终态/会话状态/episode/轮末采集)收敛。
    let final_store = persist_round_outcome(
        &state_path,
        window,
        &session_id,
        &run_result,
        &typed_writer,
        &prior,
        &ctx,
        &prompt,
        &deps.resolved,
        &run_id,
        &promoted_input_id,
        &run_started,
        run_epoch_ms,
        &live,
    );
    let summary = match run_result {
        Ok(summary) => summary,
        Err(error) => {
            // D-403:失败轮不再在轮末判定之前提前返回。自动链已武装(rounds>0)
            // 时,把失败按瞬态/致命分类送进同一个 auto_run 状态机:瞬态退避重试
            // (连续 MAX_FAILED_ROUNDS 轮才停),致命立即停;停摆经通知桥发手机。
            // 手动单轮(rounds==0)保持原行为——用户在场,错误直接返回。
            let rate_limited = crate::auto_run::is_rate_limited_run_error(&error);
            let transient = !rate_limited && crate::auto_run::is_transient_run_error(&error);
            let auto_payload = {
                let mut controllers = handles.auto_runs.lock().unwrap();
                let ctrl = controllers.entry(session_id.clone()).or_default();
                if ctrl.state.rounds == 0 {
                    None
                } else {
                    let ctx = kanzei_harness::auto_run::AutoRunCtx {
                        backlog: crate::auto_run::backlog_status(&deps.project_root),
                        halted: false,
                        steps: 0,
                        tools: &[],
                        auto_allowed: matches!(deps.profile, kanzei_harness::ProfileKind::Dev)
                            && deps.agent.name == "dev",
                        closed_this_round: 0,
                        verify_every_n: 0,
                        round_failure: Some(if rate_limited {
                            kanzei_harness::auto_run::RoundFailure::RateLimited
                        } else if transient {
                            kanzei_harness::auto_run::RoundFailure::Transient
                        } else {
                            kanzei_harness::auto_run::RoundFailure::Fatal
                        }),
                    };
                    let action = crate::auto_run::decide_auto_run(ctrl, ctx);
                    if let kanzei_harness::auto_run::AutoRunAction::Stop(
                        kanzei_harness::auto_run::AutoStopReason::RepeatedFailure(n),
                    ) = action
                    {
                        // 过夜停摆必须让人知道:经 LAN 推送桥发手机通知(尽力而为)。
                        if let Ok(message) = crate::mobile_notify::notify_mobile(
                            "kanzei 自动运行停摆",
                            &format!("连续 {n} 轮运行失败,自动推进已停止。最后错误: {error}"),
                        ) {
                            tracing::debug!("{message}");
                        }
                    }
                    let mut payload = crate::auto_run::serialize_action(
                        action,
                        crate::auto_run::work_priority_enum(deps.work_priority),
                    );
                    payload["rounds"] = json!(ctrl.state.rounds);
                    payload["max"] = json!(ctrl.state.max_rounds);
                    Some(payload)
                }
            };
            if let Some(auto) = auto_payload {
                let _ = window.emit(
                    "kz:auto-fail",
                    with_session_id(
                        json!({ "autoAction": auto, "error": error.to_string() }),
                        &session_id,
                    ),
                );
            }
            return Err(error);
        }
    };

    let history_len = summary.messages.len();
    // R-076:本轮工具画像随 kz:done 带给前端,鞭挞据此判定「实质进展」——
    // 只算本轮切片,不含 prior,否则历史工具调用让每一轮都看着像有动作。
    let this_run_tools =
        kanzei_core::summarize_tools(&summary.messages[prior.len().min(summary.messages.len())..]);
    // R-169:自主推进判定后端化——轮末用 harness 状态机判定下一步,结果随
    // kz:done 带给前端执行(发下一条/NUDGE/停止);前端不再承载任何机械判定。
    let backlog = crate::auto_run::backlog_status(&deps.project_root);
    // D-361:主轮画像 + 本轮子代理内部用过的工具。前者只切主 conversation,派出去的
    // 活在它里面只剩一个 task 调用;两者合并后,「委派」按子代理实际干了什么判定,
    // 而不是按主轮留下的那一行痕迹判定。子代理确实什么也没干时,合并后仍只有 task,
    // 空转判定照旧生效(has_progress_tools 的语义没被削弱)。
    let tools_vec: Vec<String> = {
        let mut names: std::collections::BTreeSet<String> =
            this_run_tools.keys().cloned().collect();
        names.extend(subagent_tools.lock().unwrap().iter().cloned());
        names.into_iter().collect()
    };
    let auto_action_json = {
        let mut controllers = handles.auto_runs.lock().unwrap();
        let ctrl = controllers.entry(session_id.clone()).or_default();
        let ctx = kanzei_harness::auto_run::AutoRunCtx {
            backlog,
            halted: summary.halted_by_user,
            steps: summary.steps,
            tools: &tools_vec,
            // R-199:档位条件下沉引擎——只有 dev-auto(profile=dev + agent=dev)
            // 允许自动推进;research/结对模式引擎判 Stop(ProfileMismatch),前端
            // 不再持有私有否决(armAutoContinue 的 autoContinueAllowed 已移除)。
            auto_allowed: matches!(deps.profile, kanzei_harness::ProfileKind::Dev)
                && deps.agent.name == "dev",
            // R-144:本轮关闭条目数(工具画像里 req/defect close 成功计数)。
            closed_this_round: crate::auto_run::closed_count_this_round(&summary),
            // R-144:核查阈值取自 cadence 配置;0 = 关闭该机制。
            verify_every_n: kanzei_harness::KanzeiConfig::load_at_root(&deps.project_root)
                .map(|c| c.cadence.verify_every_n)
                .unwrap_or(0),
            // D-403:本轮正常完成——失败轮走上面的失败分支,不经过这里。
            round_failure: None,
        };
        let action = crate::auto_run::decide_auto_run(ctrl, ctx);
        let mut payload = crate::auto_run::serialize_action(
            action,
            crate::auto_run::work_priority_enum(deps.work_priority),
        );
        // 判定和镜像值必须在同一把锁内取，避免后台会话完成时覆盖本会话的计数。
        payload["rounds"] = json!(ctrl.state.rounds);
        payload["max"] = json!(ctrl.state.max_rounds);
        payload
    };
    // R-143:自举循环批次提交后自动 push。仅当本轮确有 git commit 成功(检测位在
    // on_event 的 ToolStart/ToolEnd 置位);push 失败经 stage 可见但不阻断本轮收尾。
    let trace_state_path = state_path.clone();
    let trace_session_id = session_id.clone();
    let trace_live = live.clone();
    maybe_push_after_commit(
        committed_this_round.load(std::sync::atomic::Ordering::Relaxed),
        &ctx.cwd,
        &|name, detail| stage(name, detail),
        &|entry| {
            if let Ok(trace_store) = kanzei_core::SessionStore::open(&trace_state_path) {
                record_live_trace(&trace_store, &trace_session_id, &trace_live, entry);
            }
        },
    )
    .await;
    // R-202 批2:轮末收尾段后半(对话落库/轮末压缩/kz:done/租约释放/令牌回收)收敛。
    // R-253 批7b:参数按生命周期分组(FinalizeSession/FinalizeRound/FinalizeOutcome/
    // RoundReport + deps/handles/subagent_rt)。
    finalize_round(
        &deps,
        &handles,
        FinalizeSession {
            conversation: &handles.conversation,
            session_id: &session_id,
            typed_writer: &typed_writer,
            typed_flush_task,
            final_store,
        },
        FinalizeRound {
            ctx: &ctx,
            run_id: &run_id,
            process_id: &process_id,
            _write_lease: &_write_lease,
            writer_event: &writer_event,
            phase_pipeline_enabled,
        },
        FinalizeOutcome {
            summary: &summary,
            history_len,
            this_run_tools: &this_run_tools,
            auto_action_json: &auto_action_json,
        },
        RoundReport {
            window,
            stage: &stage,
            live: &live,
        },
        &subagent_rt,
    )
    .await?;
    Ok(())
}
