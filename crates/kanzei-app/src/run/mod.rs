//! 桌面 Agent Runtime 运行主链路(R-153 从 main.rs 拆出,R-253 二次拆解)。
//!
//! 独立理由:run.rs 承载「运行编排」这一整棵 application service 树——装配、
//! 事件归约、执行流水线、落库与协调;与编排零耦合的 IPC 已迁至 `crate::commands`,
//! 输入准入迁至 `run::input`,后续批次按生命周期继续拆(assembly/persistence/
//! execution/events/coordinator)。本文件最终收敛为 mod 声明与再导出。

pub(crate) mod assembly;
pub(crate) mod events;
pub(crate) mod execution;
pub(crate) mod input;
pub(crate) mod persistence;

pub(crate) use assembly::{assemble_run, RunAssembly};
pub(crate) use events::{build_ask_handler, build_event_handler};
pub(crate) use execution::{build_subagent_runtime, run_execution_loop};
pub(crate) use input::{admit_input, code_root_for, parse_delivery, promote_next_input};
pub(crate) use persistence::{finalize_round, persist_round_outcome};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_harness::auto_run::AutoRunCtx;
use serde_json::json;
use tauri::{Emitter, State, Window};

use crate::{
    conversation, ensure_default_process, process_session_id, record_live_trace, runtime_for,
    stop_runtime_and_finalize, take_pending_ask, with_session_id, AppState, LiveRun, PendingAsk,
    PromptAttachment, SessionRuntime,
};

/// D-297 验收③:TaskProgress 入参落 run.trace 时保留的字符上限。入参可能是完整
/// 工具调用 JSON(子代理勘察可带大文件内容),截断到 4K 字符足够复核调用意图,
/// 又不让单条轨迹事件把库体积与解析成本放大。
const TRACE_INPUT_KEEP_CHARS: usize = 4096;

/// R-236 B1：轮末触发线优先采用最近一次 provider 的真实 input usage；
/// 本轮没有有效 usage（冷启动、provider 未上报或返回 0）时才回落本地估算。
fn compaction_input_tokens(
    last_input_tokens: Option<u64>,
    messages: &[kanzei_llm::Message],
) -> u64 {
    last_input_tokens
        .filter(|tokens| *tokens > 0)
        .unwrap_or_else(|| kanzei_core::estimate_conversation_tokens(messages))
}

/// D-361:一条子代理 trace 是否该把工具名计入本轮画像,是则给出名字。
///
/// 只认 `phase == "end"`——那是子代理内部一次工具调用**已完成**的信号(subagent.rs
/// 由 ToolEnd 折算)。`start` 会重复计同一次调用,`usage`/`cancelled` 根本不带工具名。
/// 名字空白的 trace 不计:空名进画像等于凭空造出一个「有进展工具」。
fn subagent_round_tool(trace: &kanzei_core::TaskTrace) -> Option<&str> {
    if trace.phase != "end" {
        return None;
    }
    let name = trace.name.trim();
    (!name.is_empty()).then_some(name)
}

#[allow(clippy::too_many_arguments)] // 运行时依赖均由 AppState 拆分持有，改参会扰动 Tauri 调度链。
pub(crate) async fn run_task(
    window: &Window,
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
        state_path,
        store,
        run_id,
        promoted_input_id,
        prompt,
        initial_parts,
        typed_writer,
        typed_flush_task,
        run_started,
        run_epoch_ms,
        orchestration_trace,
        mut pipeline,
        _write_lease,
        ctx,
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
        &initial_parts,
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
        let ctx = AutoRunCtx {
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
#[tauri::command]
pub(crate) fn app_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "build": option_env!("KANZEI_BUILD_INFO").unwrap_or("dev"),
    })
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// R-143:自举循环轮末自动 push——本轮确有 git commit 成功才触发(检测位由
/// run_task 的 on_event 在 ToolStart(action=commit)+ ToolEnd(ok=true) 置位)。
/// push 失败只上报不阻断:自举循环不能被网络/远端状态卡住(验收②);
/// 与既有手动 git push 流程共存,自动 push 只是把轮末该推的提交推掉(验收③)。
pub(crate) async fn maybe_push_after_commit(
    committed: bool,
    cwd: &std::path::Path,
    on_stage: &(dyn Fn(&str, String) + Sync),
    on_trace: &(dyn Fn(serde_json::Value) + Sync),
) {
    if !committed {
        return;
    }
    on_stage("推送", "本轮有提交,自动 git push…".into());
    let mut command = tokio::process::Command::new("git");
    // D-369:auto_push 在桌面端(GUI 无控制台)跑 git push,不隐藏会被 Windows
    // 新建控制台窗口——每次自动提交后都弹黑窗。与 state.rs hidden_command 同纪律。
    #[cfg(windows)]
    {
        command.creation_flags(0x0800_0000);
    }
    let output = command.arg("-C").arg(cwd).arg("push").output().await;
    let entry = match output {
        Ok(out) if out.status.success() => {
            json!({ "kind": "push", "ok": true, "at": now_ms() })
        }
        Ok(out) => {
            let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let detail = if detail.is_empty() {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            } else {
                detail
            };
            on_stage("推送", format!("自动 push 失败(不阻断):{detail}"));
            json!({ "kind": "push", "ok": false, "error": detail, "at": now_ms() })
        }
        Err(error) => {
            on_stage("推送", format!("自动 push 失败(不阻断):{error}"));
            json!({ "kind": "push", "ok": false, "error": error.to_string(), "at": now_ms() })
        }
    };
    on_trace(entry);
}

pub(crate) fn emit_stage(window: &Window, session_id: &str, name: &str, detail: String) {
    let _ = window.emit(
        "kz:status",
        with_session_id(json!({ "stage": name, "detail": detail }), session_id),
    );
}

/// R-171 批5:写租约轨迹 guard——持有租约到 run_task 返回。
///
/// 正常路径在 run_task 尾部显式写 Released;异常/abort/停止路径走到这里时,
/// Drop 补写 Released 事件,保证 queued→acquired→released 在 session_events
/// 里成对可回放(D-303 验收②)。补写经同一 `OrchestrationEvent` 出口,与
/// 正常路径的 Released 同源;`released` 标志防止正常路径已写时重复落一条。
#[tauri::command]
pub(crate) fn pending_asks_get(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    let asks = runtime.asks.lock().unwrap();
    Ok(asks
        .iter()
        .map(|(id, pending)| crate::pending_ask_payload(*id, pending))
        .collect())
}

pub(crate) fn persist_always_allow(
    project_root: &Path,
    action: &str,
    resource: &str,
) -> Result<(kanzei_core::AskReply, PathBuf), String> {
    let pattern = kanzei_harness::config::generalize_resource(action, resource);
    let path = kanzei_harness::config::append_allow_rule(project_root, action, &pattern)
        .map_err(|error| error.to_string())?;
    Ok((kanzei_core::AskReply::AlwaysAllow, path))
}

#[tauri::command]
pub(crate) fn answer_ask(window: Window, state: State<'_, AppState>, id: u64, reply: String) {
    let Some(pending) = take_pending_ask(&state, id) else {
        return;
    };
    if matches!(pending.request, kanzei_core::AskRequest::Question { .. }) {
        let response = if reply.trim().is_empty() || reply == "cancel" {
            kanzei_core::AskResponse::Cancelled
        } else {
            kanzei_core::AskResponse::Answer(reply)
        };
        let _ = pending.sender.send(response);
        return;
    }
    let decision = match reply.as_str() {
        "always" => {
            let pattern =
                kanzei_harness::config::generalize_resource(&pending.action, &pending.resource);
            match persist_always_allow(&pending.project_root, &pending.action, &pending.resource) {
                Ok((reply, path)) => {
                    let _ = window.emit("kz:status", with_session_id(json!({ "stage": "权限", "detail": format!("已记住:{} {pattern} → {}", pending.action, path.display()) }), &pending.session_id));
                    reply
                }
                Err(error) => {
                    let _ = window.emit("kz:status", with_session_id(json!({ "stage": "权限", "detail": format!("规则保存失败:{error};本次拒绝") }), &pending.session_id));
                    kanzei_core::AskReply::Deny
                }
            }
        }
        "once" => kanzei_core::AskReply::AllowOnce,
        _ => kanzei_core::AskReply::Deny,
    };
    let _ = pending
        .sender
        .send(kanzei_core::AskResponse::Permission(decision));
}

#[tauri::command]
pub(crate) fn stop_run(
    window: Window,
    state: State<'_, AppState>,
    project_dir: Option<String>,
    process_id: Option<String>,
) -> Result<(), String> {
    let target_project = project_dir
        .as_ref()
        .map(PathBuf::from)
        .map(|cwd| crate::normalized_project_root(&cwd));
    let target_session = target_project
        .as_ref()
        .map(|root| process_session_id(root, process_id.as_deref()));
    let runtimes: Vec<Arc<SessionRuntime>> = state
        .runtimes
        .lock()
        .unwrap()
        .iter()
        .filter(|(session_id, _runtime)| {
            target_session
                .as_ref()
                .is_none_or(|target| target == *session_id)
        })
        .map(|(_, runtime)| runtime.clone())
        .collect();
    if runtimes.is_empty() {
        let _ = window.emit(
            "kz:stopped",
            with_session_id(
                json!({ "cancelled_queue": 0, "already_idle": true }),
                target_session.as_deref().unwrap_or(""),
            ),
        );
        return Ok(());
    }
    let mut cancelled = None;
    for runtime in runtimes {
        let result = target_project.clone().map(|root| {
            let session_id = target_session
                .clone()
                .unwrap_or_else(|| kanzei_core::project_session_id(&root));
            let state_path = kanzei_core::project_state_path(&root);
            kanzei_core::SessionStore::open(&state_path).and_then(|store| {
                stop_runtime_and_finalize(&runtime, &store, &state_path, &session_id)
            })
        });
        cancelled = result;
    }
    match cancelled.transpose() {
        Ok(count) => {
            let _ = window.emit(
                "kz:stopped",
                with_session_id(
                    json!({ "cancelled_queue": count.unwrap_or(0) }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
        }
        Err(error) => {
            let message = format!("停止时清理排队输入失败: {error}");
            let _ = window.emit(
                "kz:error",
                with_session_id(
                    json!({ "message": message.clone(), "terminal": false }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
            return Err(message);
        }
    }
    if let Some(root) = target_project {
        let window = window.clone();
        let session = target_session.clone().unwrap_or_default();
        let target_process = process_id.unwrap_or_else(|| crate::state::default_process_id(&root));
        tauri::async_runtime::spawn(async move {
            let killed =
                kanzei_tools::kill_background_processes_for_process(&root, &target_process).await;
            if killed > 0 {
                let _ = window.emit(
                    "kz:status",
                    with_session_id(
                        json!({ "stage": "停止", "detail": format!("已回收 {killed} 个后台进程") }),
                        &session,
                    ),
                );
            }
        });
    }
    Ok(())
}

/// R-174:单条停止一个运行中的子代理(模型 task 或编排角色)。
/// 命中 id 后 run_subagent 的取消分支立即触发,该子代理以「被停」终态收尾,
/// 读槽随 future drop 由 RAII 释放——不会像 stop_run 那样停掉整轮主对话。
#[tauri::command]
pub(crate) fn stop_task(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
    task_id: String,
) -> Result<bool, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    let hit = runtime.task_cancellations.cancel(&task_id);
    if !hit {
        return Err(format!("子代理 {task_id} 不在运行中或已结束"));
    }
    Ok(true)
}

pub(crate) fn report_persistence_failure(
    window: &Window,
    session_id: &str,
    operation: &str,
    error: impl std::fmt::Display,
) {
    let message = format!("运行结果已保留，但{operation}失败: {error}");
    tracing::warn!("{message}");
    let _ = window.emit(
        "kz:error",
        with_session_id(json!({ "message": message, "terminal": false }), session_id),
    );
}

pub(crate) fn append_run_notification(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    status: &str,
    summary: impl Into<String>,
    requires_action: bool,
) -> anyhow::Result<()> {
    store.append_notification_atomic(session_id, status, &summary.into(), requires_action)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Tauri command 参数名是前端 IPC 契约，不能合并为不兼容对象。
#[tauri::command]
pub(crate) async fn run_prompt(
    window: Window,
    state: State<'_, AppState>,
    prompt: String,
    project_dir: String,
    profile: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    work_priority: Option<String>,
    delivery: Option<String>,
    attachments: Option<Vec<PromptAttachment>>,
    process_id: Option<String>,
    autonomous: Option<bool>,
    auto_allow: Option<bool>,
) -> Result<(), String> {
    let autonomous = autonomous.unwrap_or(false);
    // D-281:自动放行开关——autonomous/parallel 轮在用户勾选时传 AutoAllow。
    let auto_allow = auto_allow.unwrap_or(false);
    let delivery = parse_delivery(delivery.as_deref()).map_err(|e| e.to_string())?;
    // 规范化主根:会话 id 与进程归属的身份键(canonicalize 过,形态唯一)。
    let project_root = crate::normalized_project_root(Path::new(&project_dir));
    // R-141:这里是 IPC 入口,根发现只做这一次,结果显式传给 run_task。
    // 与上面那个刻意分开:main_root 是**文件系统形态**的主根(托管文档、state.db、
    // 权限规则里的绝对路径都按它落盘),不做 canonicalize——换成 `\\?\` 前缀形态
    // 会让用户已写的绝对路径放行规则一夜失配。
    let main_root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let process = if let Some(process_id) = process_id.as_deref() {
        let process = state
            .processes
            .lock()
            .unwrap()
            .get(process_id)
            .cloned()
            .ok_or_else(|| format!("进程不存在: {process_id}"))?;
        // R-177 内容②:归属按 `origin_project` 判定。`project_dir` 已被 F4 定死为
        // 恒主根,两值今天恒等;改的是**意图**——归属问的是「这条线是从哪个项目开出来
        // 的」,不是「它此刻在哪棵树上跑」。将来若 project_dir 再指向别处,这里不会
        // 跟着把线自己拒掉。
        if process.origin_project.0 != project_root {
            return Err("进程不属于当前项目".into());
        }
        process
    } else {
        ensure_default_process(&state, &project_root)
    };
    if let Some(worktree) = process.worktree_path.as_ref() {
        if !worktree.0.is_dir() {
            crate::processes::unregister_parallel_process(&state, &project_root, &process.id)?;
            return Err(format!(
                "隔离工作树已不存在，已移除失效线路 {}；请切回主线后重试",
                process.id
            ));
        }
    }
    let worktree_opt = process
        .worktree_path
        .as_ref()
        .map(|worktree| worktree.0.display().to_string());
    let code_root = code_root_for(worktree_opt.as_deref(), &project_dir);
    let session_id = process_session_id(&project_root, Some(&process.id));
    let profile = profile.or_else(|| process.profile.lock().unwrap().clone());
    let model = model.or_else(|| process.model.lock().unwrap().clone());
    let reasoning = process.reasoning.lock().unwrap().clone();
    let phase_pipeline_enabled = process.phase_pipeline_enabled.load(Ordering::SeqCst);
    let block_tracker_writes =
        process.worktree_path.is_some() && !process.tracker_writes_enabled.load(Ordering::SeqCst);
    let runtime = runtime_for(&state, &session_id);
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    {
        if runtime.running.load(Ordering::SeqCst) {
            if attachments.as_ref().is_some_and(|items| !items.is_empty()) {
                return Err("当前任务运行中不能排队附件，请等待本轮完成后再发送".into());
            }
            let queued = admit_input(&project_dir, &session_id, &prompt, delivery)
                .map_err(|e| e.to_string())?;
            let _ = window.emit("kz:status", with_session_id(json!({ "stage": "排队", "detail": format!("已排队，前方输入将依次执行（{}）", queued.input_id) }), &session_id));
            return Ok(());
        }
        runtime.running.store(true, Ordering::SeqCst);
    }
    let asks = runtime.asks.clone();
    let ask_seq = state.ask_seq.clone();
    let running = runtime.running.clone();
    let lifecycle = runtime.lifecycle.clone();
    let conversation = runtime.conversation.clone();
    let live_run = runtime.live.clone();
    let task_cancellations = runtime.task_cancellations.clone();
    // D-342:停止令牌槽与 run 代数随 run_task 走(协作式停止接线)。
    let halt_slot = runtime.halt.clone();
    let run_generation = runtime.run_generation.clone();
    let runtime_for_task = runtime.clone();
    let current_stage = runtime.stage.clone();
    // R-169:自主推进状态机在 AppState,spawn 前 clone 出来(闭包不能引用 State)。
    let auto_runs = state.auto_runs.clone();
    // R-171:项目级协调器与进程身份传给 writer run(写租约申请用)。
    let coordinator = state.coordinator.clone();
    let process_id_for_run = process.id.clone();
    let collaboration_probe = crate::collaboration::CollaborationProbe::new(
        state.processes.clone(),
        state.runtimes.clone(),
        project_root.clone(),
        process.id.clone(),
    )
    .with_coordinator(Arc::clone(&coordinator)
        as Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>);
    let handle = tauri::async_runtime::spawn(async move {
        let mut next_input = None;
        let mut next_prompt = prompt;
        let mut next_attachments = attachments;
        let mut idle_reason = "completed";
        loop {
            let result = run_task(
                &window,
                asks.clone(),
                ask_seq.clone(),
                next_prompt,
                next_attachments.take(),
                code_root.clone(),
                main_root.clone(),
                session_id.clone(),
                phase_pipeline_enabled,
                block_tracker_writes,
                collaboration_probe.clone(),
                current_stage.clone(),
                profile.clone(),
                agent.clone(),
                model.clone(),
                work_priority.clone(),
                reasoning.clone(),
                conversation.clone(),
                live_run.clone(),
                task_cancellations.clone(),
                auto_runs.clone(),
                delivery,
                next_input.take(),
                coordinator.clone(),
                process_id_for_run.clone(),
                autonomous,
                auto_allow,
                halt_slot.clone(),
                run_generation.clone(),
            )
            .await;
            if let Err(e) = &result {
                let message = e.to_string();
                let lower = message.to_lowercase();
                let hint = if ["timed out", "timeout", "connect", "dns", "connection"]
                    .iter()
                    .any(|k| lower.contains(k))
                {
                    "\n提示:疑似网络不通。若需代理,在设置页把代理设为「指定地址」(如 http://127.0.0.1:12000)后重试;本地模型(ollama)不受代理影响。"
                } else {
                    ""
                };
                let _ = window.emit(
                    "kz:error",
                    with_session_id(
                        json!({ "message": format!("{message}{hint}"), "terminal": true }),
                        &session_id,
                    ),
                );
            }
            if result.is_err() {
                let _lifecycle = lifecycle.lock().unwrap();
                running.store(false, Ordering::SeqCst);
                idle_reason = "failed";
                break;
            }
            next_input = {
                let _lifecycle = lifecycle.lock().unwrap();
                match promote_next_input(&project_dir, &session_id) {
                    Ok(input) => {
                        if input.is_none() {
                            running.store(false, Ordering::SeqCst);
                        }
                        input
                    }
                    Err(error) => {
                        let _ = window.emit(
                            "kz:error",
                            with_session_id(
                                json!({ "message": error.to_string(), "terminal": true }),
                                &session_id,
                            ),
                        );
                        running.store(false, Ordering::SeqCst);
                        idle_reason = "failed";
                        None
                    }
                }
            };
            let Some(input) = next_input.clone() else {
                break;
            };
            next_prompt = input.prompt.clone();
            let _ = window.emit("kz:status", with_session_id(json!({ "stage": "排队", "detail": format!("开始执行排队输入（{}）", input.input_id) }), &session_id));
        }
        let _ = window.emit(
            "kz:idle",
            with_session_id(json!({ "reason": idle_reason }), &session_id),
        );
        *runtime_for_task.stage.lock().unwrap() = if idle_reason == "failed" {
            "失败".into()
        } else {
            "空闲".into()
        };
        runtime_for_task.current_run.lock().unwrap().take();
    });
    *runtime.current_run.lock().unwrap() = Some(handle);
    if !runtime.running.load(Ordering::SeqCst) {
        runtime.current_run.lock().unwrap().take();
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn run_metrics(
    project_dir: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let root = std::path::PathBuf::from(&project_dir);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let session_id = kanzei_core::project_session_id(&root);
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let rows = store
        .recent_episodes(&session_id, limit)
        .map_err(|e| e.to_string())?;
    let rounds: Vec<serde_json::Value> = rows.into_iter().map(|(at, prompt, outcome, steps, input, output, tools, context, metrics)| {
        let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap_or(serde_json::json!({}));
        serde_json::json!({ "at": at, "prompt": prompt, "outcome": outcome, "steps": steps, "inputTokens": input, "outputTokens": output, "tools": parse(&tools), "context": parse(&context), "metrics": parse(&metrics), "measured": metrics.trim() != "{}" && !metrics.trim().is_empty() })
    }).collect();
    Ok(serde_json::json!({ "rounds": rounds }))
}

/// R-240:从 prompt_head 提取需求 ID(`R-123` / `D-321`),取第一个命中。
/// 自举/取活轮的 prompt 以条目标题开头(R-xxx …),用户轮通常无——据此归类。
fn extract_ticket_id(prompt_head: &str) -> Option<String> {
    let bytes = prompt_head.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if (bytes[i] == b'R' || bytes[i] == b'D') && bytes[i + 1] == b'-' {
            let mut j = i + 2;
            let mut num = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                num.push(bytes[j] as char);
                j += 1;
            }
            if !num.is_empty() {
                return Some(format!("{}-{num}", bytes[i] as char));
            }
        }
        i += 1;
    }
    None
}

/// R-240:从需求/缺陷文档解析 `<id>` 的复杂度字段(小/中/大)。
/// 读 `.kanzei/project/requirements.md` 与 `defects.md`,`## {id} ` 段落内扫
/// `- 复杂度: X` 行。找不到或字段缺失返回 None(归类为「未知」)。
fn ticket_complexity(project_root: &Path, id: &str) -> Option<String> {
    for name in ["requirements.md", "defects.md"] {
        let text = std::fs::read_to_string(project_root.join(".kanzei/project").join(name)).ok()?;
        let marker = format!("## {id} ");
        let Some(pos) = text.find(&marker) else {
            continue;
        };
        let rest = &text[pos..];
        let section_end = rest.find("\n## ").unwrap_or(rest.len());
        for line in rest[..section_end].lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("- 复杂度:") {
                let value = value.trim();
                if value == "小" || value == "中" || value == "大" {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// R-240:按 (类型, 复杂度) 聚合运行指标。纯函数,可单测。
/// rows 取 (prompt_head, outcome, steps, input_tokens, output_tokens)。
/// 返回:groups 数组(每项 count/sumInput/sumOutput/sumSteps/avgSteps + 分类键)
/// + uncategorized(未提取到需求 ID 的轮次合计)。
fn aggregate_run_metrics(
    rows: &[(String, String, u32, u64, u64)],
    metas: &HashMap<String, String>,
) -> serde_json::Value {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, String), (u32, u64, u64, u64)> = BTreeMap::new();
    let mut other: (u32, u64, u64, u64) = (0, 0, 0, 0);
    for (prompt, _, steps, input, output) in rows {
        let target = match extract_ticket_id(prompt) {
            Some(id) => {
                let kind = if id.starts_with('D') { "D" } else { "R" };
                let complexity = metas
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| "未知".to_string());
                groups
                    .entry((kind.to_string(), complexity))
                    .or_insert((0, 0, 0, 0))
            }
            None => &mut other,
        };
        target.0 += 1;
        target.1 += input;
        target.2 += output;
        target.3 += *steps as u64;
    }
    let group_values: Vec<serde_json::Value> = groups
        .into_iter()
        .map(
            |((kind, complexity), (count, sum_input, sum_output, sum_steps))| {
                serde_json::json!({
                    "kind": kind,
                    "complexity": complexity,
                    "count": count,
                    "sumInput": sum_input,
                    "sumOutput": sum_output,
                    "sumSteps": sum_steps,
                    "avgInput": if count > 0 { sum_input as f64 / count as f64 } else { 0.0 },
                    "avgOutput": if count > 0 { sum_output as f64 / count as f64 } else { 0.0 },
                    "avgSteps": if count > 0 { sum_steps as f64 / count as f64 } else { 0.0 },
                })
            },
        )
        .collect();
    serde_json::json!({
        "groups": group_values,
        "uncategorized": {
            "count": other.0,
            "sumInput": other.1,
            "sumOutput": other.2,
            "sumSteps": other.3,
        }
    })
}

/// R-240:按需求类型(R-/D-)与复杂度(小/中/大)聚合的运行时指标。
/// 数据源 = episodes 表(prompt_head 提取需求 ID → requirements/defects 文档取复杂度)。
/// 返回 groups(按分类的 count/token 合计与均值)+ uncategorized(无 ID 轮)。
#[tauri::command]
pub(crate) fn run_metrics_by_category(
    project_dir: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let root = PathBuf::from(&project_dir);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let session_id = kanzei_core::project_session_id(&root);
    let limit = limit.unwrap_or(200).clamp(1, 1000);
    let rows = store
        .recent_episodes(&session_id, limit)
        .map_err(|e| e.to_string())?;
    let mut metas: HashMap<String, String> = HashMap::new();
    for (_, prompt, _, _, _, _, _, _, _) in &rows {
        if let Some(id) = extract_ticket_id(prompt) {
            metas.entry(id.clone()).or_insert_with(|| {
                ticket_complexity(&root, &id).unwrap_or_else(|| "未知".to_string())
            });
        }
    }
    let mapped: Vec<(String, String, u32, u64, u64)> = rows
        .iter()
        .map(|(_, prompt, outcome, steps, input, output, _, _, _)| {
            (prompt.clone(), outcome.clone(), *steps, *input, *output)
        })
        .collect();
    Ok(aggregate_run_metrics(&mapped, &metas))
}

#[cfg(test)]
mod worktree_run_tests {
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-run-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei")).unwrap();
        dir
    }

    /// R-177 验收③:配置取**主根**那一份,worktree 里的分支副本改了也不生效。
    /// run_task 用的就是这个入口(`load_with_warnings_at_root(&project_root)`),
    /// 配套的机械判据是本文件里发现式取根的配置入口零命中。
    #[test]
    fn 配置从主根加载_worktree副本改了不生效() {
        let main_root = temp_dir("cfg-main");
        let worktree = temp_dir("cfg-tree");
        std::fs::write(
            main_root.join(".kanzei/kanzei.toml"),
            "[profile]\ndefault = \"dev\"\n",
        )
        .unwrap();
        std::fs::write(
            worktree.join(".kanzei/kanzei.toml"),
            "[profile]\ndefault = \"research\"\n",
        )
        .unwrap();
        let (config, _) =
            kanzei_harness::KanzeiConfig::load_with_warnings_at_root(&main_root).unwrap();
        assert_eq!(
            config.profile.default.as_deref(),
            Some("dev"),
            "必须读主根那份配置;读到 research 说明取了 worktree 的分支副本"
        );
        std::fs::remove_dir_all(&worktree).ok();
        std::fs::remove_dir_all(&main_root).ok();
    }
}

#[cfg(test)]
mod auto_push_tests {
    use super::maybe_push_after_commit;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-push-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        git(&repo, &["config", "user.name", "Kanzei Test"]);
        dir
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} 失败: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// 有提交 + 有 remote → push 成功,origin 收到该提交,轨迹记录 ok:true。
    #[tokio::test]
    async fn 本轮有提交_推送成功_远端收到() {
        let dir = temp_repo("ok");
        let repo = dir.join("repo");
        let remote = dir.join("remote.git");
        Command::new("git")
            .args(["init", "-q", "--bare"])
            .arg(&remote)
            .output()
            .unwrap();
        // bare 仓库默认 HEAD 分支名随 git 版本/config 漂移(master 或 main),
        // 钉死 refs/heads/main 让 rev-parse 断言与本地仓库分支名一致。
        git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        std::fs::write(repo.join("a.txt"), "hello\n").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-q", "-m", "第一条"]);
        // simple 模式无 upstream 的 `git push` 会拒绝;先建立 upstream(等价于
        // 手动 push 流程已跑过),再验证「轮末自动 push 把后续提交推上去」。
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        std::fs::write(repo.join("b.txt"), "second\n").unwrap();
        git(&repo, &["add", "b.txt"]);
        git(&repo, &["commit", "-q", "-m", "第二条"]);
        let local_head = git(&repo, &["rev-parse", "HEAD"]);

        let stages = std::sync::Mutex::new(Vec::new());
        let traces = std::sync::Mutex::new(Vec::new());
        maybe_push_after_commit(
            true,
            &repo,
            &|name, detail| stages.lock().unwrap().push(format!("{name}:{detail}")),
            &|entry| traces.lock().unwrap().push(entry),
        )
        .await;

        let stages = stages.into_inner().unwrap();
        let traces = traces.into_inner().unwrap();
        let remote_head = git(&remote, &["rev-parse", "main"]);
        assert_eq!(remote_head, local_head, "远端必须收到本轮提交");
        assert!(
            traces.iter().any(|e| e["ok"] == true),
            "轨迹应记 push 成功: {traces:?}"
        );
        assert!(
            !stages.iter().any(|s| s.contains("失败")),
            "成功路径不该报失败: {stages:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 本轮没有 commit(检测位 false)→ 根本不触发 push,零 stage/零 trace。
    #[tokio::test]
    async fn 本轮无提交_不触发push() {
        let dir = temp_repo("none");
        let repo = dir.join("repo");
        let stages = std::sync::Mutex::new(Vec::new());
        let traces = std::sync::Mutex::new(Vec::new());
        maybe_push_after_commit(
            false,
            &repo,
            &|name, detail| stages.lock().unwrap().push(format!("{name}:{detail}")),
            &|entry| traces.lock().unwrap().push(entry),
        )
        .await;
        assert!(
            traces.into_inner().unwrap().is_empty(),
            "无提交不应产生 push 轨迹"
        );
        assert!(
            stages.into_inner().unwrap().is_empty(),
            "无提交不应产生任何 stage 输出"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 有提交但没有 remote → push 失败,但函数不 panic、不阻断,失败经 stage 可见。
    #[tokio::test]
    async fn 有提交无remote_失败可见不panic() {
        let dir = temp_repo("noremote");
        let repo = dir.join("repo");
        std::fs::write(repo.join("a.txt"), "hello\n").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-q", "-m", "第一条"]);

        let stages = std::sync::Mutex::new(Vec::new());
        let traces = std::sync::Mutex::new(Vec::new());
        maybe_push_after_commit(
            true,
            &repo,
            &|name, detail| stages.lock().unwrap().push(format!("{name}:{detail}")),
            &|entry| traces.lock().unwrap().push(entry),
        )
        .await;

        let stages = stages.into_inner().unwrap();
        let traces = traces.into_inner().unwrap();
        assert!(
            stages.iter().any(|s| s.contains("失败")),
            "失败必须经 stage 可见: {stages:?}"
        );
        assert!(
            traces.iter().any(|e| e["ok"] == false),
            "轨迹应记 push 失败: {traces:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod assembly_tests {
    use kanzei_harness::{ConfigComponent, Harness, KanzeiConfig, ProfileKind, ResolveCtx};
    use kanzei_tools::{BaseComponent, DevProfile, ResearchProfile};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// D-361:只有「子代理内部一次工具调用已完成」的 trace 才计入本轮画像。
    #[test]
    fn 子代理画像上卷只认已完成的工具调用() {
        let trace = |phase: &str, name: &str| kanzei_core::TaskTrace {
            child_id: "child-1".into(),
            phase: phase.into(),
            name: name.into(),
            summary: None,
            ok: None,
            outcome: None,
            code: None,
            preview: None,
            display: None,
            input: None,
            usage: None,
        };
        assert_eq!(
            super::subagent_round_tool(&trace("end", "edit")),
            Some("edit"),
            "子代理调完 edit 必须上卷进画像"
        );
        assert_eq!(
            super::subagent_round_tool(&trace("start", "edit")),
            None,
            "start 会与 end 重复计同一次调用,不计"
        );
        assert_eq!(
            super::subagent_round_tool(&trace("usage", "")),
            None,
            "usage trace 不带工具名,不计"
        );
        assert_eq!(
            super::subagent_round_tool(&trace("cancelled", "")),
            None,
            "取消 trace 不带工具名,不计"
        );
        assert_eq!(
            super::subagent_round_tool(&trace("end", "   ")),
            None,
            "空名不得凭空造出一个「有进展工具」"
        );
    }

    #[test]
    fn 轮末压缩触发优先使用provider真实usage_无usage才估算() {
        let messages = vec![kanzei_llm::Message::user_text("本地估算内容")];
        let summary = kanzei_core::RunSummary {
            text: String::new(),
            usage: kanzei_llm::Usage::default(),
            last_input_tokens: Some(321),
            steps: 1,
            halted_by_user: false,
            messages: messages.clone(),
            context_report: vec![],
            overflow_traces: vec![],
        };
        assert_eq!(
            super::compaction_input_tokens(summary.last_input_tokens, &messages),
            321
        );

        let cold = kanzei_core::RunSummary {
            last_input_tokens: None,
            ..summary
        };
        assert_eq!(
            super::compaction_input_tokens(cold.last_input_tokens, &messages),
            kanzei_core::estimate_conversation_tokens(&messages)
        );
    }

    /// D-195:运行装配线必须注册前端自查段点名的每个工具。
    #[test]
    fn 桌面装配线必须注册前端自查段点名的每个工具() {
        let root = PathBuf::from("C:/kanzei-d195-app-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(BaseComponent)
            .add(DevProfile)
            .add(ResearchProfile)
            .add(crate::harness_ext::FrontendToolsComponent)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let tools: Vec<String> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();
        let mentioned =
            kanzei_tools::prompt_tool_mentions(kanzei_tools::frontend_inspection_guidance());
        assert_eq!(mentioned.len(), 5);
        for tool in mentioned {
            assert!(
                tools.contains(&tool),
                "缺少前端自查工具 `{tool}`;已注册: {tools:?}"
            );
        }
    }

    // ══ R-240:按需求类型/复杂度聚合运行指标 ══

    #[test]
    fn extract_ticket_id_识别r与d条目() {
        use super::extract_ticket_id;
        assert_eq!(
            extract_ticket_id("R-202 run_task 拆分"),
            Some("R-202".into())
        );
        assert_eq!(extract_ticket_id("D-321 修复"), Some("D-321".into()));
        assert_eq!(extract_ticket_id("继续推进，规则按系统提示执行"), None);
        assert_eq!(extract_ticket_id(""), None);
        // 非需求编号的 R- 不误认(后面无数字)。
        assert_eq!(extract_ticket_id("README 说明"), None);
        // 多个编号取第一个。
        assert_eq!(
            extract_ticket_id("R-183 与 R-186 联动"),
            Some("R-183".into())
        );
    }

    #[test]
    fn ticket_complexity_从文档段落解析() {
        use super::ticket_complexity;
        let dir = std::env::temp_dir().join(format!(
            "kz-ticket-meta-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-101 示例 [doing]\n- 优先级: P0\n- 复杂度: 中\n- 标签: 核心\n\n## R-102 无复杂度 [doing]\n- 优先级: P1\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".kanzei/project/defects.md"),
            "# Defects\n\n## D-201 缺陷 [fixing]\n- 复杂度: 小\n",
        )
        .unwrap();
        assert_eq!(ticket_complexity(&dir, "R-101").as_deref(), Some("中"));
        assert_eq!(ticket_complexity(&dir, "D-201").as_deref(), Some("小"));
        assert_eq!(
            ticket_complexity(&dir, "R-102"),
            None,
            "无复杂度字段 → None"
        );
        assert_eq!(
            ticket_complexity(&dir, "R-999"),
            None,
            "不存在的条目 → None"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn aggregate_run_metrics_按类型与复杂度分组() {
        use super::aggregate_run_metrics;
        use std::collections::HashMap;
        let rows: Vec<(String, String, u32, u64, u64)> = vec![
            (
                "R-101 中复杂度任务".into(),
                "completed".into(),
                5,
                1000,
                200,
            ),
            (
                "R-101 中复杂度任务(二次)".into(),
                "completed".into(),
                3,
                800,
                100,
            ),
            ("R-102 小复杂度任务".into(), "completed".into(), 2, 300, 50),
            ("D-201 缺陷修复".into(), "completed".into(), 1, 150, 30),
            ("继续推进".into(), "completed".into(), 4, 500, 80),
        ];
        let mut metas = HashMap::new();
        metas.insert("R-101".into(), "中".into());
        metas.insert("R-102".into(), "小".into());
        metas.insert("D-201".into(), "小".into());
        let out = aggregate_run_metrics(&rows, &metas);
        let groups = out["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 3, "三个分类:R-中/R-小/D-小");
        let r101 = groups
            .iter()
            .find(|g| g["kind"] == "R" && g["complexity"] == "中")
            .unwrap();
        assert_eq!(r101["count"], 2);
        assert_eq!(r101["sumInput"], 1800);
        assert_eq!(r101["sumOutput"], 300);
        assert_eq!(r101["sumSteps"], 8);
        let d201 = groups
            .iter()
            .find(|g| g["kind"] == "D" && g["complexity"] == "小")
            .unwrap();
        assert_eq!(d201["count"], 1);
        // 无 ID 轮进 uncategorized。
        assert_eq!(out["uncategorized"]["count"], 1);
        assert_eq!(out["uncategorized"]["sumInput"], 500);
    }
}
