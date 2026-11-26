//! 运行类 Tauri command(R-253 批6b,纯搬迁自 run/mod.rs)。
//!
//! 独立理由:IPC 入口与运行编排分离——run_prompt(外层 scheduler,排队/联动/根发现)、
//! stop_run/stop_task(停止)、pending_asks_get/answer_ask(权限/提问应答)、run_metrics/
//! run_metrics_by_category(运行指标)都是「UI 调用 → AppState 操作」的薄层,不承载
//! 编排逻辑;留在 run 模块只会让「运行主链路」继续膨胀(照 files_view.rs 模式)。
//!
//! 依赖:run_task(Round Coordinator)在 crate::run::coordinator,输入准入在
//! crate::run::input,共享 helper(now_ms/emit_stage 等)在 crate::run。

use crate::app_events::AppEvents;
pub(crate) mod tool_process;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::json;
use tauri::{Emitter, State};

use crate::{
    normalized_project_root, pending_ask_payload, process_session_id, request_runtime_stop,
    runtime_for, take_pending_ask, with_session_id, AppState, PromptAttachment, SessionRuntime,
};

use crate::run::assembly::{RoundRequest, RunMode, RuntimeHandles};
use crate::run::coordinator::run_task;
use crate::run::input::{
    admit_input, code_root_for, has_pending_queue_prompt, parse_delivery, promote_next_input,
    QueuedInput,
};

pub(crate) mod handoff;
mod input_mode;

/// A durable restricted recipient cannot be upgraded by a stale composer payload.
fn recipient_mode(
    general: bool,
    stored: Option<String>,
    requested: Option<String>,
    agent: Option<String>,
) -> (Option<String>, Option<String>) {
    let profile = if matches!(stored.as_deref(), Some("readonly" | "isolated")) {
        stored
    } else if general {
        crate::general_chat::normalize_profile(requested.or(stored))
    } else {
        requested.or(stored)
    };
    let agent = match profile.as_deref() {
        Some("isolated") => Some("isolated".into()),
        Some("readonly") => Some("readonly".into()),
        _ if general => Some("general".into()),
        _ => agent,
    };
    (profile, agent)
}

/// Assembly failures have no round finalizer. A rejected durable outcome must
/// retain its transaction's input state rather than receive a separate write.
pub(crate) fn finish_failed_promoted_input(
    store: &kanzei_core::SessionStore,
    input_id: &str,
    error: &anyhow::Error,
) -> Result<bool, kanzei_core::StoreError> {
    if crate::run::persistence::is_uncommitted_outcome(error)
        || crate::run::persistence::is_stopped_outcome(error)
    {
        return Ok(false);
    }
    store.finish_input(input_id, false)
}

pub(crate) fn run_error_idle_reason(error: &anyhow::Error) -> &'static str {
    if crate::run::persistence::is_stopped_outcome(error) {
        "stopped"
    } else {
        "failed"
    }
}

/// R-171 批5:写租约轨迹 guard——持有租约到 run_task 返回。
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
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    let asks = runtime.asks.lock().unwrap();
    let values: Vec<_> = asks
        .iter()
        .map(|(id, pending)| pending_ask_payload(*id, pending))
        .collect();
    Ok(values)
}

pub(crate) fn persist_always_allow(
    project_root: &Path,
    action: &str,
    resource: &str,
) -> Result<(kanzei_core::AskReply, PathBuf), String> {
    let pattern = kanzei_harness::config::generalize_resource(action, resource);
    let path = kanzei_harness::project_trust::remember_permission(project_root, action, &pattern)
        .map_err(|error| error.to_string())?;
    Ok((kanzei_core::AskReply::AlwaysAllow, path))
}

/// 把一条被拦下的操作记成项目放行规则(UX-147):并行线/自主轮遇到要批准的操作会被 NonInteractive 当场拦下,
/// 对话里的工具行因此挂一个「放行并记住」——这里就是它的落点。与权限卡的「允许并记住」同一条持久化路径、
/// 同一个口径:只记**完全相同的这一条**(同一操作 + 同一资源),写用户目录中的项目授权记录。
#[tauri::command]
pub(crate) fn permission_rule_add(
    project_dir: String,
    action: String,
    resource: String,
) -> Result<(), String> {
    if action.trim().is_empty() || resource.trim().is_empty() {
        return Err("缺少操作或资源,无法记成放行规则".into());
    }
    let root = normalized_project_root(Path::new(&project_dir));
    persist_always_allow(&root, &action, &resource).map(|_| ())
}

#[tauri::command]
pub(crate) async fn answer_ask(
    window: AppEvents,
    state: State<'_, AppState>,
    id: u64,
    reply: String,
) -> Result<(), String> {
    let Some(pending) = take_pending_ask(&state, id) else {
        return Err("请求已结束或不存在".into());
    };
    if matches!(pending.request, kanzei_core::AskRequest::Question { .. }) {
        let response = if reply.trim().is_empty() || reply == "cancel" {
            kanzei_core::AskResponse::Cancelled
        } else {
            kanzei_core::AskResponse::Answer(reply)
        };
        if pending.sender.send(response).is_err() {
            return Err("发起者已停止".into());
        }
        return Ok(());
    }
    // 「允许并记住」写项目配置失败时:本次已按拒绝回喂模型,同时把失败原因作为错误返回前端(UX-148:
    // 原先只发一条 kz:status 日志,用户点了「总是允许」却什么提示都没有,以为已经记住了)。
    let mut save_error: Option<String> = None;
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
                    save_error = Some(error);
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
    match save_error {
        Some(error) => Err(format!("规则保存失败:{error};本次已拒绝")),
        None => Ok(()),
    }
}

#[tauri::command]
pub(crate) async fn stop_run(
    window: AppEvents,
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
) -> Result<(), String> {
    let (root, process) = stop_target(&project_dir, process_id.as_deref())?;
    let session = process_session_id(&root, Some(process));
    let runtime = runtime_for(&state, &session);
    stop_session(&state, &root, &session)?;
    let stop = runtime.snapshot().stop;
    let _ = window.emit(
        "kz:stopping",
        with_session_id(
            json!({"cancelled_queue": stop.map_or(0, |s| s.cancelled_inputs),"runtime":runtime.snapshot()}),
            &session,
        ),
    );
    crate::session_supervisor::ensure_stop_supervisor(&runtime, &root, process)?;
    Ok(())
}

fn stop_target<'a>(
    project_dir: &str,
    process_id: Option<&'a str>,
) -> Result<(PathBuf, &'a str), String> {
    if project_dir.trim().is_empty() {
        return Err("请选择一个项目".into());
    }
    let process = process_id
        .filter(|id| !id.trim().is_empty())
        .ok_or("请选择一段对话")?;
    Ok((normalized_project_root(Path::new(project_dir)), process))
}

/// A stop command always addresses one conversation. Service-wide shutdown has
/// its own owner and must never be inferred from a missing command argument.
fn stop_session(state: &AppState, root: &Path, session: &str) -> Result<Option<usize>, String> {
    let runtime = state.runtimes.lock().unwrap().get(session).cloned();
    let Some(runtime) = runtime else {
        return Ok(None);
    };
    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|error| error.to_string())?;
    let stopped = request_runtime_stop(&runtime, &store, &state_path, session)
        .map_err(|error| error.to_string())?;
    Ok(Some(stopped))
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
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    let hit = runtime.task_cancellations.cancel(&task_id);
    if !hit {
        return Err(format!("子代理 {task_id} 不在运行中或已结束"));
    }
    Ok(true)
}

#[cfg(test)]
fn take_pending_for_wake(
    project_dir: &str,
    session_id: &str,
    is_running: bool,
) -> Result<Option<kanzei_core::AdmittedInput>, String> {
    if is_running {
        return Ok(None);
    }
    promote_next_input(project_dir, session_id).map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)] // Tauri command 参数名是前端 IPC 契约，不能合并为不兼容对象。
#[tauri::command]
pub(crate) async fn run_prompt(
    window: AppEvents,
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
    research_topic: Option<String>,
    execution_batch: Option<bool>,
    work_item_id: Option<String>,
    handoff_source: Option<handoff::HandoffSource>,
    wake_queued: Option<bool>,
    // D-764:这一轮由引擎自动推进投递,不是用户按键。前端自动续跑闸门发枪时置真;
    // 引擎侧的 Submission::Automatic 不需要这个参数(提交形态本身就是证据)。
    auto_round: Option<bool>,
    command_id: Option<String>,
) -> Result<(), String> {
    let command = command_id.map(|id| InputCommand {
        id,
        payload_hash: kanzei_core::stable_json_hash(&json!([
            prompt,
            project_dir,
            process_id,
            profile,
            agent,
            model,
            work_priority,
            delivery,
            attachments,
            autonomous,
            auto_allow,
            research_topic,
            execution_batch,
            work_item_id,
            handoff_source,
            auto_round
        ])),
    });
    if wake_queued.unwrap_or(false) {
        if !prompt.trim().is_empty() || attachments.as_ref().is_some_and(|items| !items.is_empty())
        {
            return Err("唤醒已有队列时不能附带新的输入".into());
        }
        let root = normalized_project_root(Path::new(&project_dir));
        let owner = process_session_id(&root, process_id.as_deref());
        let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
            .map_err(|e| e.to_string())?;
        let pending = store
            .list_pending_inputs(&owner)
            .map_err(|e| e.to_string())?;
        let Some(input) = pending.first() else {
            return Ok(());
        };
        return schedule_run(
            window,
            &state,
            project_dir,
            process_id,
            Submission::Saved {
                session_id: owner,
                input_id: input.input_id.clone(),
            },
            RunOptions {
                command: None,
                execution_batch: execution_batch.unwrap_or(false),
                work_item_id,
                profile,
                agent,
                model,
                work_priority,
                research_topic,
                autonomous: autonomous.unwrap_or(false),
                auto_allow: auto_allow.unwrap_or(false),
                // 唤醒已有输入:归属在当初投递时定过,这里不重新判定。
                auto_round: false,
            },
        )
        .map(|_| ());
    }
    let prompt = match handoff_source {
        Some(source) => handoff::attach_snapshot(&state, source, &prompt)?,
        None => prompt,
    };
    let delivery = if work_item_id.is_some() {
        kanzei_core::Delivery::Queue
    } else {
        parse_delivery(delivery.as_deref()).map_err(|e| e.to_string())?
    };
    schedule_run(
        window,
        &state,
        project_dir,
        process_id,
        Submission::Prompt {
            prompt,
            delivery,
            attachments,
        },
        RunOptions {
            command,
            execution_batch: execution_batch.unwrap_or(false) || work_item_id.is_some(),
            work_item_id,
            profile,
            agent,
            model,
            work_priority,
            research_topic,
            autonomous: autonomous.unwrap_or(false),
            auto_allow: auto_allow.unwrap_or(false),
            auto_round: auto_round.unwrap_or(false),
        },
    )
    .map(|_| ())
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct InputCommand {
    pub id: String,
    pub payload_hash: String,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct RunOptions {
    #[serde(default)]
    pub(crate) command: Option<InputCommand>,
    pub(crate) execution_batch: bool,
    #[serde(default)]
    pub(crate) work_item_id: Option<String>,
    pub(crate) profile: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) work_priority: Option<String>,
    pub(crate) research_topic: Option<String>,
    pub(crate) autonomous: bool,
    pub(crate) auto_allow: bool,
    /// D-764:引擎自动推进投递的这一轮(与用户按键相对)。
    #[serde(default)]
    pub(crate) auto_round: bool,
}

pub(crate) enum Submission {
    Automatic {
        prompt: String,
        generation: u64,
        async_generation: u64,
    },
    Notice {
        session_id: String,
        input_id: String,
        generation: u64,
    },
    Prompt {
        prompt: String,
        delivery: kanzei_core::Delivery,
        attachments: Option<Vec<PromptAttachment>>,
    },
    /// A durable reply already exists. Wake its owner without admitting a second prompt.
    Saved {
        session_id: String,
        input_id: String,
    },
}

/// D-764:哪些枪是引擎放的。`Submission::Automatic` 形态本身就是证据;UI 的自动续跑闸门
/// 走 `Submission::Prompt`,靠 `auto_round` 声明。用户按键两者都不成立,文本原样进上下文。
fn is_engine_round(submission: &Submission, auto_round: bool) -> bool {
    matches!(submission, Submission::Automatic { .. }) || auto_round
}

pub(crate) fn schedule_run(
    window: AppEvents,
    state: &AppState,
    project_dir: String,
    process_id: Option<String>,
    submission: Submission,
    options: RunOptions,
) -> Result<&'static str, String> {
    let _admission = crate::runtime_service::admit_work()?;
    let is_notice = matches!(&submission, Submission::Notice { .. });
    let RunOptions {
        command,
        execution_batch,
        work_item_id,
        profile,
        agent,
        model,
        work_priority,
        research_topic: _,
        autonomous,
        auto_allow,
        auto_round,
    } = options;
    // D-764:投递点定归属。必须在 Automatic→Prompt 归一之前判定,否则两种来源已被压成
    // 同一种形态,再分不出来。
    let engine_round = is_engine_round(&submission, auto_round);
    // 规范化主根:会话 id 与进程归属的身份键(canonicalize 过,形态唯一)。
    let project_root = normalized_project_root(Path::new(&project_dir));
    // R-141:IPC 显式选中的主根只解析一次并传给 run_task;worktree cwd 另行选择。
    // canonical helper 已去掉普通路径的 verbatim 前缀,身份键与托管文档/配置不能
    // 因为缺少 `.kanzei` 各自落进不同根,也不能继承父项目。
    let main_root = project_root.clone();
    let process = crate::processes::registry::resolve_conversation(
        state,
        &project_root,
        process_id.as_deref(),
    )?;
    if let Some(worktree) = process.worktree_path.as_ref() {
        if !worktree.0.is_dir() {
            crate::processes::unregister_parallel_process(state, &project_root, &process.id)?;
            return Err("这个对话的工作树已不存在，已移除失效登记；请选择其它对话后重试".into());
        }
    }
    let worktree_opt = process
        .worktree_path
        .as_ref()
        .map(|worktree| worktree.0.display().to_string());
    let code_root = code_root_for(worktree_opt.as_deref(), &project_dir);
    let session_id = process_session_id(&project_root, Some(&process.id));
    if let Submission::Saved {
        session_id: expected,
        ..
    }
    | Submission::Notice {
        session_id: expected,
        ..
    } = &submission
    {
        if expected != &session_id {
            return Err("原对话已变化；回复已保存，请返回原对话重试".into());
        }
    }
    let general = crate::general_chat::is_general_root(&project_root);
    if general {
        crate::general_chat::enforce(&process);
    }
    let stored_profile = process.profile.lock().unwrap().clone();
    let (profile, agent) = recipient_mode(general, stored_profile, profile, agent);
    let discussion = profile.as_deref() == Some("readonly");
    let execution_batch = !general
        && !discussion
        && profile.as_deref() != Some("isolated")
        && agent.as_deref() != Some("dev-pair")
        && execution_batch;
    let autonomous = !general && !discussion && autonomous;
    let model = model.or_else(|| process.model.lock().unwrap().clone());
    let reasoning = process.reasoning.lock().unwrap().clone();
    let runtime = runtime_for(state, &session_id);
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    if runtime.snapshot().existence != kanzei_core::session_lifecycle::Existence::Open {
        return Err("对话正在关闭".into());
    }
    if runtime.snapshot().phase == kanzei_core::session_lifecycle::Phase::Blocked {
        return Err(runtime
            .snapshot()
            .error
            .unwrap_or_else(|| "对话回收未完成，请重试停止".into()));
    }
    if let Submission::Notice { generation, .. } = &submission {
        if *generation != runtime.async_generation.load(Ordering::SeqCst) {
            return Ok("stopped");
        }
    }
    let submission = match submission {
        Submission::Automatic {
            prompt,
            generation,
            async_generation,
        } => {
            let enabled = state
                .auto_runs
                .lock()
                .unwrap()
                .get(&session_id)
                .is_some_and(|ctrl| {
                    ctrl.enabled && !ctrl.state.paused && !ctrl.state.stop_after_round
                });
            if !enabled
                || generation != runtime.generation()
                || async_generation != runtime.async_generation.load(Ordering::SeqCst)
                || runtime.is_running()
            {
                return Ok("superseded");
            }
            Submission::Prompt {
                prompt,
                delivery: kanzei_core::Delivery::Queue,
                attachments: None,
            }
        }
        other => other,
    };
    // Every prompt is durable before execution acquisition, including the first
    // message. Retransmission resumes that same input instead of admitting twice.
    let submission = match submission {
        Submission::Prompt {
            prompt,
            delivery,
            attachments,
        } => {
            if autonomous
                && runtime.is_running()
                && delivery == kanzei_core::Delivery::Queue
                && has_pending_queue_prompt(&project_dir, &session_id, &prompt)
                    .map_err(|e| e.to_string())?
            {
                return Ok("queued");
            }
            let input = admit_input(
                &project_dir,
                &session_id,
                QueuedInput {
                    prompt: &prompt,
                    attachments: attachments.unwrap_or_default(),
                    delivery,
                    origin: if engine_round {
                        kanzei_base::MessageOrigin::Engine
                    } else {
                        kanzei_base::MessageOrigin::Human
                    },
                    execution_batch: execution_batch || autonomous,
                    work_item_id: work_item_id.as_deref(),
                    command: command.as_ref(),
                },
            )
            .map_err(|e| e.to_string())?;
            if delivery == kanzei_core::Delivery::Steer && !runtime.stopping() {
                if let Some(mailbox) = runtime.async_mailbox.lock().unwrap().as_ref() {
                    mailbox.request_input();
                }
            }
            if command.is_some() {
                let store =
                    kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&main_root))
                        .map_err(|e| e.to_string())?;
                if store
                    .input_status(&input.input_id)
                    .map_err(|e| e.to_string())?
                    .as_deref()
                    != Some("pending")
                {
                    return Ok("consumed");
                }
            }
            Submission::Saved {
                session_id: session_id.clone(),
                input_id: input.input_id,
            }
        }
        other => other,
    };
    if runtime.is_running() {
        return Ok("queued");
    }
    let execution_owner = Arc::new(
        kanzei_core::session_execution::try_acquire(
            &kanzei_core::project_state_path(&main_root),
            &session_id,
        )
        .map_err(|e| e.to_string())?,
    );
    let input_id = match submission {
        Submission::Saved { input_id, .. } | Submission::Notice { input_id, .. } => input_id,
        _ => unreachable!("all prompts pass durable admission"),
    };
    let initial_input =
        match crate::run::input::resume_saved_input(&project_dir, &session_id, &input_id, false)
            .map_err(|e| e.to_string())?
        {
            crate::run::input::SavedInputResume::Start(input) => input,
            crate::run::input::SavedInputResume::Queued => return Ok("queued"),
            crate::run::input::SavedInputResume::Consumed => return Ok("consumed"),
        };
    let prompt = initial_input.prompt.clone();
    let delivery = initial_input.delivery;
    let initial_input = Some(initial_input);
    let attachments = None;
    if !is_notice {
        *runtime.callback_options.lock().unwrap() = RunOptions {
            command: None,
            execution_batch: false,
            work_item_id: None,
            profile: profile.clone(),
            agent: agent.clone(),
            model: model.clone(),
            work_priority: work_priority.clone(),
            research_topic: None,
            autonomous: false,
            auto_allow,
            // 回调选项描述「下一轮怎么发」;自动轮的归属由 submission 形态与 auto_round 判定。
            auto_round: false,
        };
    }
    runtime.begin_round()?;
    let asks = runtime.asks.clone();
    let ask_seq = state.ask_seq.clone();
    let lifecycle = runtime.lifecycle.clone();
    let conversation = runtime.conversation.clone();
    let live_run = runtime.live.clone();
    let task_cancellations = runtime.task_cancellations.clone();
    // D-342:停止令牌槽与 run 代数随 run_task 走(协作式停止接线)。
    let halt_slot = runtime.halt.clone();
    let runtime_for_task = runtime.clone();
    let current_stage = runtime.stage.clone();
    // R-169:自主推进状态机在 AppState,spawn 前 clone 出来(闭包不能引用 State)。
    let auto_runs = state.auto_runs.clone();
    // R-171:项目级协调器与进程身份传给 writer run(写租约申请用)。
    let coordinator = state.coordinator.clone();
    let process_id_for_run = process.id.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut next_input = initial_input;
        let mut next_prompt = prompt;
        let mut next_attachments = attachments;
        loop {
            let promoted_id = next_input.as_ref().map(|input| input.input_id.clone());
            let cancellation = runtime_for_task
                .halt
                .lock()
                .unwrap()
                .as_ref()
                .expect("scheduler owns its round token")
                .clone();
            let worker = {
                let window = window.clone();
                let main_root = main_root.clone();
                let session_id = session_id.clone();
                let code_root = code_root.clone();
                let execution_owner = execution_owner.clone();
                let next_prompt = next_prompt.clone();
                let mut next_input = next_input.take();
                let mut next_attachments = next_attachments.take();
                let process_id_for_run = process_id_for_run.clone();
                let profile = profile.clone();
                let agent = agent.clone();
                let model = model.clone();
                let reasoning = reasoning.clone();
                let process = process.clone();
                let lifecycle = lifecycle.clone();
                let asks = asks.clone();
                let ask_seq = ask_seq.clone();
                let current_stage = current_stage.clone();
                let conversation = conversation.clone();
                let live_run = live_run.clone();
                let task_cancellations = task_cancellations.clone();
                let auto_runs = auto_runs.clone();
                let coordinator = coordinator.clone();
                let halt_slot = halt_slot.clone();
                tauri::async_runtime::spawn(async move {
                    // Each queued input owns its execution intent. An ordinary
                    // preparation message cannot inherit the previous round's autonomy.
                    // Failed metadata reads follow the same terminal failure path as assembly.
                    let execution = input_mode::resolve(
                        &main_root,
                        &session_id,
                        next_input.as_ref(),
                        execution_batch,
                        autonomous,
                    )?;
                    run_task(
                        &window,
                        RoundRequest {
                            input_origin: if engine_round {
                                kanzei_base::MessageOrigin::Engine
                            } else {
                                kanzei_base::MessageOrigin::Human
                            },
                            prompt: next_prompt,
                            attachments: next_attachments.take(),
                            project_dir: code_root.clone(),
                            main_root: main_root.clone(),
                            session_id: session_id.clone(),
                            execution_owner: execution_owner.clone(),
                            delivery,
                            promoted_input: next_input.take(),
                            process_id: process_id_for_run.clone(),
                        },
                        RunMode {
                            execution_batch: !discussion && execution.execution_batch,
                            // 排队输入共用这个循环；每轮重读,运行中改开关在下一轮生效。
                            subagent_mode: process.subagent_mode(),
                            profile: profile.clone(),
                            agent_name: agent.clone(),
                            model_override: model.clone(),
                            reasoning_override: reasoning.clone(),
                            autonomous: execution.autonomous,
                            auto_allow,
                        },
                        RuntimeHandles {
                            lifecycle: lifecycle.clone(),
                            asks: asks.clone(),
                            ask_seq: ask_seq.clone(),
                            current_stage: current_stage.clone(),
                            conversation: conversation.clone(),
                            live_run: live_run.clone(),
                            task_cancellations: task_cancellations.clone(),
                            auto_runs: auto_runs.clone(),
                            coordinator: coordinator.clone(),
                            halt_slot: halt_slot.clone(),
                        },
                    )
                    .await
                })
            };
            let result = crate::session_supervisor::worker_result(
                worker,
                cancellation,
                std::time::Duration::from_secs(crate::state::STOP_GRACE_SECS),
            )
            .await;
            execution_owner.writers_settled().await;
            if let Err(error) =
                runtime_for_task.transition(kanzei_core::session_lifecycle::Event::WorkerEnded)
            {
                let _ = window.emit(
                    "kz:error",
                    with_session_id(json!({"message":error,"terminal":true}), &session_id),
                );
                return;
            }
            let mut reason = result
                .as_ref()
                .err()
                .map(run_error_idle_reason)
                .unwrap_or("completed");
            if let Err(error) = &result {
                if crate::run::persistence::is_uncommitted_outcome(error) {
                    let _ = runtime_for_task.transition(
                        kanzei_core::session_lifecycle::Event::Failed(error.to_string()),
                    );
                    return;
                }
                if reason == "failed" {
                    let recovery = kanzei_core::SessionStore::open(
                        &kanzei_core::project_state_path(&main_root),
                    )
                    .map_err(|e| e.to_string())
                    .and_then(|store| {
                        execution_owner
                            .settle_failed(&store, &error.to_string())
                            .map_err(|e| e.to_string())
                    });
                    if let Err(error) = recovery {
                        let _ = runtime_for_task
                            .transition(kanzei_core::session_lifecycle::Event::Failed(error));
                        return;
                    }

                    let _ = window.emit(
                        "kz:error",
                        with_session_id(
                            json!({"message":error.to_string(),"terminal":false}),
                            &session_id,
                        ),
                    );
                }
                if let Some(id) = promoted_id.as_deref() {
                    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(
                        &main_root,
                    ));
                    match store.and_then(|store| finish_failed_promoted_input(&store, id, error)) {
                        Ok(_) => {}
                        Err(error) => {
                            let _ = runtime_for_task.transition(
                                kanzei_core::session_lifecycle::Event::Failed(error.to_string()),
                            );
                            return;
                        }
                    }
                }
            }
            let input = loop {
                // Admission, Stop and the last drain decision share this lock.
                // A stop arriving after this decision owns a separate idle reaper.
                {
                    let _lifecycle = lifecycle.lock().unwrap();
                    if !runtime_for_task.stopping() {
                        let closed = runtime_for_task.snapshot().existence
                            != kanzei_core::session_lifecycle::Existence::Open;
                        let promoted = if closed {
                            Ok(None)
                        } else {
                            promote_next_input(&project_dir, &session_id)
                        };
                        match promoted {
                            Ok(Some(input)) => {
                                if let Err(error) = runtime_for_task.begin_round() {
                                    let _ = runtime_for_task.transition(
                                        kanzei_core::session_lifecycle::Event::Failed(error),
                                    );
                                    return;
                                }
                                break input;
                            }
                            Ok(None) => {
                                return finish_scheduler(
                                    &window,
                                    &runtime_for_task,
                                    &session_id,
                                    reason,
                                    execution_owner,
                                )
                            }
                            Err(error) => {
                                let _ = runtime_for_task.transition(
                                    kanzei_core::session_lifecycle::Event::Failed(
                                        error.to_string(),
                                    ),
                                );
                                let _ = window.emit(
                                    "kz:error",
                                    with_session_id(
                                        json!({"message":error.to_string(),"terminal":true}),
                                        &session_id,
                                    ),
                                );
                                return;
                            }
                        }
                    }
                }
                let settled = async {
                    crate::session_supervisor::settle_resources(
                        &runtime_for_task,
                        &main_root,
                        &process_id_for_run,
                    )
                    .await?;
                    let _lifecycle = lifecycle.lock().unwrap();
                    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(
                        &main_root,
                    ))
                    .map_err(|error| error.to_string())?;
                    if result.is_err() {
                        crate::flush_live_run(&store, &session_id, &live_run, "halted");
                    }
                    execution_owner
                        .settle_stopped(&store)
                        .map_err(|error| error.to_string())?;
                    let stop = runtime_for_task.snapshot().stop.ok_or("停止操作缺失")?;
                    store
                        .settle_session_stop(&session_id, &stop)
                        .map_err(|error| error.to_string())?;
                    runtime_for_task.transition(kanzei_core::session_lifecycle::Event::StopSettled)
                }
                .await;
                if let Err(error) = settled {
                    let _ = runtime_for_task
                        .transition(kanzei_core::session_lifecycle::Event::Failed(error.clone()));
                    let _ = window.emit(
                        "kz:error",
                        with_session_id(json!({"message":error,"terminal":true}), &session_id),
                    );
                    return;
                }
                reason = "stopped";
            };
            next_input = Some(input.clone());
            next_prompt = input.prompt.clone();
            let _ = window.emit("kz:status", with_session_id(json!({ "stage": "排队", "detail": format!("开始执行排队输入（{}）", input.input_id) }), &session_id));
        }
    });
    *runtime.current_run.lock().unwrap() = Some(handle);
    Ok("started")
}

/// Caller holds lifecycle. Publish the old run's terminal state before a reply may wake a new one.
fn finish_scheduler(
    window: &AppEvents,
    runtime: &SessionRuntime,
    session_id: &str,
    reason: &str,
    execution_owner: Arc<kanzei_core::session_execution::SessionExecutionGuard>,
) {
    release_scheduler(runtime, reason, execution_owner);
    // Retain the JoinHandle so a shutdown owner can await the final task drop.
    crate::session_supervisor::publish_idle(window, runtime, session_id, reason);
}

/// Caller holds lifecycle; idle means a subsequent submission can acquire ownership.
fn release_scheduler(
    runtime: &SessionRuntime,
    reason: &str,
    execution_owner: Arc<kanzei_core::session_execution::SessionExecutionGuard>,
) {
    drop(execution_owner);
    runtime.halt.lock().unwrap().take();
    *runtime.stage.lock().unwrap() = if reason == "failed" {
        "失败"
    } else {
        "空闲"
    }
    .into();
    runtime
        .transition(kanzei_core::session_lifecycle::Event::Settled(
            reason.into(),
        ))
        .expect("joined supervisor can settle");
}

#[cfg(test)]
mod tests {
    use kanzei_core::SessionStore;
    use std::path::PathBuf;

    #[test]
    fn idle_scheduler_releases_ownership_before_the_next_submission() {
        use std::sync::Arc;
        for reason in ["completed", "failed", "halted"] {
            let (root, session) = fixture(reason);
            let path = kanzei_core::project_state_path(&root);
            let owner =
                Arc::new(kanzei_core::session_execution::try_acquire(&path, &session).unwrap());
            let runtime = crate::SessionRuntime::default();
            runtime.activate_for_test();
            assert!(kanzei_core::session_execution::try_acquire(&path, &session).is_err());
            {
                let _lifecycle = runtime.lifecycle.lock().unwrap();
                runtime
                    .transition(kanzei_core::session_lifecycle::Event::WorkerEnded)
                    .unwrap();
                super::release_scheduler(&runtime, reason, owner);
                assert!(!runtime.is_running());
                let next = kanzei_core::session_execution::try_acquire(&path, &session)
                    .expect("idle must permit the next execution owner immediately");
                drop(next);
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    /// D-764:自动续跑轮不得冒用用户名义。判定看投递形态与 auto_round,标注发生在准入点。

    #[test]
    fn stored_restricted_recipient_defeats_a_stale_dev_sender() {
        for general in [false, true] {
            for stored in ["isolated", "readonly"] {
                let (profile, agent) = super::recipient_mode(
                    general,
                    Some(stored.into()),
                    Some("dev-auto".into()),
                    Some("dev-pair".into()),
                );
                assert_eq!(profile.as_deref(), Some(stored));
                assert_eq!(agent.as_deref(), Some(stored));
            }
        }
        assert_eq!(
            super::recipient_mode(true, Some("dev-auto".into()), Some("isolated".into()), None),
            (Some("isolated".into()), Some("isolated".into()))
        );
    }

    #[test]
    fn stop_run_rejects_empty_project_and_missing_conversation() {
        for project in ["", "  "] {
            assert!(super::stop_target(project, Some("p1|project")).is_err());
        }
        for process in [None, Some(""), Some("  ")] {
            assert!(super::stop_target("project", process).is_err());
        }
    }

    #[test]
    fn stop_run_cancels_only_the_selected_conversation_queue() {
        let (root, _) = fixture("stop-selected");
        let state = crate::AppState::default();
        let selected_process = format!("p1|{}", root.display());
        let other_process = format!("p2|{}", root.display());
        let session = crate::process_session_id(&root, Some(&selected_process));
        let other = crate::process_session_id(&root, Some(&other_process));
        assert_ne!(
            session, other,
            "fixture must address distinct conversations"
        );
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        for (owner, input) in [(&session, "selected-input"), (&other, "other-input")] {
            store
                .create_session(owner, &root.display().to_string(), None)
                .unwrap();
            store
                .admit_input(owner, input, "pending prompt", kanzei_core::Delivery::Queue)
                .unwrap();
            crate::runtime_for(&state, owner);
        }
        assert_eq!(store.list_pending_inputs(&session).unwrap().len(), 1);
        assert_eq!(store.list_pending_inputs(&other).unwrap().len(), 1);
        assert_eq!(
            super::stop_session(&state, &root, &session).unwrap(),
            Some(1)
        );
        assert!(store.list_pending_inputs(&session).unwrap().is_empty());
        assert_eq!(store.list_pending_inputs(&other).unwrap().len(), 1);
        assert_eq!(
            super::stop_session(&state, &root, "absent-session").unwrap(),
            None
        );
        assert_eq!(
            state.runtimes.lock().unwrap().len(),
            2,
            "an idle lookup must not create an actor or address every actor"
        );
        drop(store);
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn assembly_failure_still_finishes_its_promoted_input() {
        let (root, session) = fixture("assembly-failure");
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store
            .admit_input(&session, "input", "prompt", kanzei_core::Delivery::Queue)
            .unwrap();
        store.promote_next_input(&session).unwrap().unwrap();
        let error = anyhow::anyhow!("assembly rejected model configuration");
        assert!(super::finish_failed_promoted_input(&store, "input", &error).unwrap());
        assert_eq!(
            store.input_status("input").unwrap().as_deref(),
            Some("failed")
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn fixture(tag: &str) -> (PathBuf, String) {
        let root = std::env::temp_dir().join(format!(
            "kz-command-run-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        // 命令按身份根(canonical + 去 `\\?\`)算会话 id,夹具必须用同一个根建会话。
        let root = crate::normalized_project_root(&root);
        let session_id = kanzei_core::project_session_id(&root);
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store
            .create_session(&session_id, &root.display().to_string(), None)
            .unwrap();
        (root, session_id)
    }

    #[test]
    fn wake_mode_promotes_only_an_existing_queue_when_idle() {
        let (root, session_id) = fixture("wake-queue");
        let root_text = root.display().to_string();
        let queued = super::admit_input(
            &root_text,
            &session_id,
            super::QueuedInput {
                prompt: "apply the reviewed correction",
                attachments: Vec::new(),
                delivery: kanzei_core::Delivery::Queue,
                origin: kanzei_base::MessageOrigin::Human,
                execution_batch: false,
                work_item_id: None,
                command: None,
            },
        )
        .unwrap();
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();

        assert!(super::take_pending_for_wake(&root_text, &session_id, true)
            .unwrap()
            .is_none());
        assert_eq!(store.list_pending_inputs(&session_id).unwrap().len(), 1);

        let promoted = super::take_pending_for_wake(&root_text, &session_id, false)
            .unwrap()
            .expect("idle wake should promote the queued correction");
        assert_eq!(promoted.input_id, queued.input_id);
        assert_eq!(promoted.prompt, "apply the reviewed correction");
        assert!(store.list_pending_inputs(&session_id).unwrap().is_empty());
        assert!(super::take_pending_for_wake(&root_text, &session_id, false)
            .unwrap()
            .is_none());

        drop(store);
        std::fs::remove_dir_all(root).ok();
    }
}
