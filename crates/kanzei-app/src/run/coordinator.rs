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
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tauri::Emitter;

use crate::{
    conversation, record_live_trace, with_session_id, LiveRun, PendingAsk, PromptAttachment,
};

use super::assembly::{assemble_run, RunAssembly, RuntimeDeps};
use super::events::{build_ask_handler, build_event_handler};
use super::execution::{build_subagent_runtime, run_execution_loop};
use super::persistence::{finalize_round, persist_round_outcome};
use super::{emit_stage, maybe_push_after_commit};

/// R-202 批2:run_task(原 run.rs 的 Round Coordinator)。装配 → 事件循环 → 轮末收尾。
#[allow(clippy::too_many_arguments)] // 运行时依赖均由 AppState 拆分持有，改参会扰动 Tauri 调度链。
pub(crate) async fn run_task(
    window: &tauri::Window,
    asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    ask_seq: Arc<AtomicU64>,
    prompt: String,
    attachments: Option<Vec<PromptAttachment>>,
    project_dir: String,
    // R-141:项目主根由调用方(run_prompt)在 IPC 入口解析一次后显式传入,
    // 线路径内不再做根发现。worktree 线上线后 project_dir 是代码树、main_root
    // 仍是主根,两者不同——发现式取根在那时会拐进 worktree 里的 .kanzei 分支副本。
    main_root: PathBuf,
    session_id: String,
    // 进程级「勘察复核」开关 = 阶段流水线总闸(2026-08-11 用户定调)。
    // 开 → 本轮强制走七阶段;关 → 一问一答。它**不**决定有没有子代理。
    phase_pipeline_enabled: bool,
    // 分支线 tracker 写入开关。主线永远不加此门禁;分支线默认关闭。
    block_tracker_writes: bool,
    collaboration_probe: crate::collaboration::CollaborationProbe,
    current_stage: Arc<Mutex<String>>,
    profile: Option<String>,
    agent_name: Option<String>,
    model_override: Option<String>,
    work_priority: Option<String>,
    reasoning_override: Option<String>,
    conversation: Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    live_run: Arc<Mutex<LiveRun>>,
    // R-174:本会话的单条停止注册表。塞进 SubagentRuntime.cancellations 供
    // run_subagent 挂取消 token;stop_task 命令从 SessionRuntime 拿同一实例命中。
    task_cancellations: Arc<kanzei_core::TaskCancellations>,
    auto_runs: Arc<Mutex<HashMap<String, crate::auto_run::AutoRunController>>>,
    delivery: kanzei_core::Delivery,
    promoted_input: Option<kanzei_core::AdmittedInput>,
    // R-171:项目级协调器(所有 ProcessHandle 共享)。主对话 writer run
    // 在此获取写租约并持有到本轮结束;RAII 保证任何结束路径都释放。
    coordinator: Arc<kanzei_core::orchestration::MemoryCoordinator>,
    process_id: String,
    autonomous: bool,
    auto_allow: bool,
    // D-342 协作式停止:本会话的停止令牌槽(SessionRuntime.halt)与 run 代数。
    // run 开始时换代并安装新令牌;stop 取走令牌 cancel,run 在检查点 halted 收尾。
    halt_slot: Arc<Mutex<Option<kanzei_core::CancellationToken>>>,
    run_generation: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    // 阶段汇报:让前端每一步都有着落(用户反馈:要详细指示)。
    let stage = |name: &str, detail: String| {
        *current_stage.lock().unwrap() = name.to_string();
        emit_stage(window, &session_id, name, detail);
    };

    // D-342:换代 + 安装本 run 的停止令牌。换代在前——stop 的兜底硬杀按代数比对,
    // 装了新令牌还留着旧代数会让上一次停止的兜底误杀本 run。
    run_generation.fetch_add(1, Ordering::SeqCst);
    let halt_token = kanzei_core::CancellationToken::new();
    *halt_slot.lock().unwrap() = Some(halt_token.clone());

    let RunAssembly {
        deps,
        session,
        round,
    } = assemble_run(
        window,
        &stage,
        &project_dir,
        main_root,
        attachments,
        prompt,
        &session_id,
        phase_pipeline_enabled,
        block_tracker_writes,
        collaboration_probe,
        profile,
        agent_name,
        model_override,
        work_priority,
        reasoning_override,
        delivery,
        promoted_input,
        &coordinator,
        &process_id,
        autonomous,
        auto_allow,
        halt_token,
    )
    .await?;

    // R-253 批7:RunAssembly 三分后按需展开——move 型字段(typed_flush_task/pipeline)
    // 经分组变量取,其余字段经引用访问,避免部分 move 破坏整体借用。
    let RuntimeDeps {
        project_root,
        config,
        profile,
        rctx,
        snapshot,
        agent,
        work_priority,
        resolved,
        proxy,
        route,
        client,
        runner_config,
        ask_source,
    } = deps;
    let state_path = session.state_path.clone();
    // SessionStore 非 Sync:recover_messages 同步用后即弃,不跨 await 持引用
    // (危险点③——跨 await 持引用会破坏 future Send 约束)。move owned 而非借用。
    let store = session.store;
    let promoted_input_id = session.promoted_input_id.clone();
    let prompt = session.prompt.clone();
    let initial_parts = &session.initial_parts;
    let typed_writer = session.typed_writer.clone();
    let typed_flush_task = session.typed_flush_task;
    let run_id = round.run_id.clone();
    let run_started = round.run_started;
    let run_epoch_ms = round.run_epoch_ms;
    let orchestration_trace = round.orchestration_trace.clone();
    let mut pipeline = round.pipeline;
    let _write_lease = round._write_lease;
    let ctx = round.ctx;

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
    let live = live_run.clone();
    live.lock().unwrap().begin(
        &run_id,
        &promoted_input_id,
        &prompt,
        &resolved.provider_name,
        &resolved.model,
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
    let mut on_event = build_event_handler(
        emit_event,
        tool_started,
        trace_log,
        state_path.clone(),
        session_id.clone(),
        typed_writer.clone(),
        committed_this_round.clone(),
        pending_commit_call,
        subagent_tools.clone(),
    );

    let mut ask = build_ask_handler(
        asks,
        ask_seq,
        ask_source,
        window,
        ctx.project_root.clone(),
        session_id.clone(),
    );

    // 会话连续:同项目续上内存历史；应用重启后从事件日志恢复最近一次完整消息投影。
    // (同步完成——SessionStore 非 Sync,跨 await 持引用会破坏 future Send 约束,
    // 故 prior 在 run_task 恢复、run_execution_loop 只消费它,行为与内联时一致。)
    let persisted = conversation::recover_messages(&store, &session_id)?;
    let prior = conversation::conversation_prior(&conversation, &session_id, persisted);
    if !prior.is_empty() {
        stage("会话", format!("延续对话({} 条历史消息)", prior.len()));
    }

    // task 子代理运行时:独立只读快照;fast 角色缺席时两个档位都退回主模型。
    // **无条件构造**(2026-08-11 用户定调):模型自己派 `task` 这条路永远开着,不受
    // 「勘察复核」开关控制。构造与 prior 恢复无数据依赖(顺序互换行为不变,失败
    // 同样提前终止本轮),故先构造再进 run_execution_loop。
    let subagent_rt = build_subagent_runtime(
        &rctx,
        &config,
        &proxy,
        &resolved,
        &route,
        &coordinator,
        task_cancellations,
    )
    .await?;

    // R-202 批2:事件循环段——附件提示 → 记忆预检索 → 勘察 → 主循环
    // (run_once_with_parts)→ 复核修正(run_review_and_fixup),收敛为独立函数。
    let run_result = run_execution_loop(
        &stage,
        initial_parts,
        &prompt,
        &ctx,
        autonomous,
        &config,
        &mut pipeline,
        &subagent_rt,
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &mut on_event,
        &mut ask,
        &prior,
    )
    .await;
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
        &resolved,
        &run_id,
        &promoted_input_id,
        &run_started,
        run_epoch_ms,
        &live,
    );
    let summary = run_result?;

    let history_len = summary.messages.len();
    // R-076:本轮工具画像随 kz:done 带给前端,鞭挞据此判定「实质进展」——
    // 只算本轮切片,不含 prior,否则历史工具调用让每一轮都看着像有动作。
    let this_run_tools =
        kanzei_core::summarize_tools(&summary.messages[prior.len().min(summary.messages.len())..]);
    // R-169:自主推进判定后端化——轮末用 harness 状态机判定下一步,结果随
    // kz:done 带给前端执行(发下一条/NUDGE/停止);前端不再承载任何机械判定。
    let backlog = crate::auto_run::backlog_status(&project_root);
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
        let mut controllers = auto_runs.lock().unwrap();
        let ctrl = controllers.entry(session_id.clone()).or_default();
        let ctx = kanzei_harness::auto_run::AutoRunCtx {
            backlog,
            halted: summary.halted_by_user,
            steps: summary.steps,
            tools: &tools_vec,
            // R-199:档位条件下沉引擎——只有 dev-auto(profile=dev + agent=dev)
            // 允许自动推进;research/结对模式引擎判 Stop(ProfileMismatch),前端
            // 不再持有私有否决(armAutoContinue 的 autoContinueAllowed 已移除)。
            auto_allowed: matches!(profile, kanzei_harness::ProfileKind::Dev)
                && agent.name == "dev",
            // R-144:本轮关闭条目数(工具画像里 req/defect close 成功计数)。
            closed_this_round: crate::auto_run::closed_count_this_round(&summary),
            // R-144:核查阈值取自 cadence 配置;0 = 关闭该机制。
            verify_every_n: kanzei_harness::KanzeiConfig::load_at_root(&project_root)
                .map(|c| c.cadence.verify_every_n)
                .unwrap_or(0),
        };
        let action = crate::auto_run::decide_auto_run(ctrl, ctx);
        let mut payload = crate::auto_run::serialize_action(
            action,
            crate::auto_run::work_priority_enum(work_priority),
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
    finalize_round(
        &conversation,
        &session_id,
        &summary,
        &resolved,
        &config,
        &stage,
        &client,
        &subagent_rt,
        final_store,
        &live,
        &typed_writer,
        typed_flush_task,
        window,
        history_len,
        &this_run_tools,
        &auto_action_json,
        phase_pipeline_enabled,
        &writer_event,
        &ctx,
        &run_id,
        &process_id,
        &_write_lease,
        &halt_slot,
    )
    .await?;
    Ok(())
}
