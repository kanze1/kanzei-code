//! 运行类 Tauri command(R-253 批6b,纯搬迁自 run/mod.rs)。
//!
//! 独立理由:IPC 入口与运行编排分离——run_prompt(外层 scheduler,排队/联动/根发现)、
//! stop_run/stop_task(停止)、pending_asks_get/answer_ask(权限/提问应答)、run_metrics/
//! run_metrics_by_category(运行指标)都是「UI 调用 → AppState 操作」的薄层,不承载
//! 编排逻辑;留在 run 模块只会让「运行主链路」继续膨胀(照 files_view.rs 模式)。
//!
//! 依赖:run_task(Round Coordinator)在 crate::run::coordinator,输入准入在
//! crate::run::input,共享 helper(now_ms/emit_stage 等)在 crate::run。

pub(crate) mod tool_process;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::json;
use tauri::{Emitter, State, Window};

use crate::{
    normalized_project_root, pending_ask_payload, process_session_id, runtime_for,
    stop_runtime_and_finalize, take_pending_ask, with_session_id, AppState, PromptAttachment,
    SessionRuntime,
};

use crate::run::assembly::{RoundRequest, RunMode, RuntimeHandles};
use crate::run::coordinator::run_task;
use crate::run::input::{
    admit_input, code_root_for, has_pending_queue_prompt, parse_delivery, promote_next_input,
};

mod handoff;
mod input_mode;

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
    let mut values: Vec<_> = asks
        .iter()
        .map(|(id, pending)| pending_ask_payload(*id, pending))
        .collect();
    for item in crate::durable_questions::pending(&root, Some(&session_id))? {
        if !values.iter().any(|v| v["id"] == item["id"]) {
            values.push(item);
        }
    }
    Ok(values)
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

/// 把一条被拦下的操作记成项目放行规则(UX-147):并行线/自主轮遇到要批准的操作会被 NonInteractive 当场拦下,
/// 对话里的工具行因此挂一个「放行并记住」——这里就是它的落点。与权限卡的「允许并记住」同一条持久化路径、
/// 同一个口径:只记**完全相同的这一条**(同一操作 + 同一资源),写项目 `.kanzei/kanzei.toml`。
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
    window: Window,
    state: State<'_, AppState>,
    id: u64,
    reply: String,
) -> Result<(), String> {
    let saved = if let Some((root, _)) = crate::durable_questions::find(id) {
        Some((
            root.clone(),
            kanzei_harness::pending_question::respond(
                &root,
                id,
                &reply,
                &format!("ask:{id}"),
                reply.trim().is_empty() || reply == "cancel",
            )?,
        ))
    } else {
        None
    };
    let Some(pending) = take_pending_ask(&state, id) else {
        if let Some((root, value)) = saved {
            crate::durable_questions::deliver(&window, &state, &root, value).await?;
            let _ = window.emit("kz:ask-resolved", json!({"id":id}));
            return Ok(());
        }
        return Err("问题已结束，请刷新「待我处理」".into());
    };
    if matches!(pending.request, kanzei_core::AskRequest::Question { .. }) {
        let response = if reply.trim().is_empty() || reply == "cancel" {
            kanzei_core::AskResponse::Cancelled
        } else {
            kanzei_core::AskResponse::Answer(reply)
        };
        if pending.sender.send(response).is_err() {
            if let Some((root, value)) = saved {
                crate::durable_questions::deliver(&window, &state, &root, value).await?;
            } else {
                return Err("发起者已停止".into());
            }
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
pub(crate) fn stop_run(
    window: Window,
    state: State<'_, AppState>,
    project_dir: Option<String>,
    process_id: Option<String>,
) -> Result<(), String> {
    let target_project = project_dir
        .as_ref()
        .map(PathBuf::from)
        .map(|cwd| normalized_project_root(&cwd));
    if target_project.is_some() && process_id.as_deref().is_none_or(str::is_empty) {
        return Err("请选择一段对话".into());
    }
    let target_session = target_project
        .as_ref()
        .map(|root| process_session_id(root, process_id.as_deref()));
    if let (Some(root), Some(owner)) = (&target_project, &target_session) {
        kanzei_harness::pending_question::cancel_owner(root, owner, None)?;
    }
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
            if let Some(team) = kanzei_tools::team::find(&root, &session_id) {
                team.stop_all();
            }
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
        let target_process = process_id.expect("project-scoped stop requires a conversation");
        tauri::async_runtime::spawn(async move {
            let killed =
                kanzei_tools::kill_background_processes_for_process(&root, &target_process).await;
            if killed > 0 {
                let _ = window.emit(
                    "kz:status",
                    with_session_id(
                        json!({ "stage": "停止", "detail": format!("已回收 {killed} 个后台终端") }),
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
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    let hit = runtime.task_cancellations.cancel(&task_id);
    if !hit {
        if let Some(team) = kanzei_tools::team::find(&root, &session_id) {
            team.stop(&task_id).map_err(|e| e.to_string())?;
            return Ok(true);
        }
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
    research_topic: Option<String>,
    execution_batch: Option<bool>,
    work_item_id: Option<String>,
    handoff_source: Option<handoff::HandoffSource>,
    wake_queued: Option<bool>,
) -> Result<(), String> {
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
                execution_batch: execution_batch.unwrap_or(false),
                work_item_id,
                profile,
                agent,
                model,
                work_priority,
                research_topic,
                autonomous: autonomous.unwrap_or(false),
                auto_allow: auto_allow.unwrap_or(false),
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
            execution_batch: execution_batch.unwrap_or(false) || work_item_id.is_some(),
            work_item_id,
            profile,
            agent,
            model,
            work_priority,
            research_topic,
            autonomous: autonomous.unwrap_or(false),
            auto_allow: auto_allow.unwrap_or(false),
        },
    )
    .map(|_| ())
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct RunOptions {
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

pub(crate) fn schedule_run(
    window: Window,
    state: &AppState,
    project_dir: String,
    process_id: Option<String>,
    submission: Submission,
    options: RunOptions,
) -> Result<&'static str, String> {
    let is_notice = matches!(&submission, Submission::Notice { .. });
    let RunOptions {
        execution_batch,
        work_item_id,
        profile,
        agent,
        model,
        work_priority,
        research_topic,
        autonomous,
        auto_allow,
    } = options;
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
    // The visible discussion recipient cannot be upgraded by a stale composer
    // sending its previous dev profile/agent. Its stored boundary is authoritative.
    let discussion = stored_profile.as_deref() == Some("readonly");
    let profile = if general {
        Some("dev".into())
    } else if discussion {
        stored_profile
    } else {
        profile.or(stored_profile)
    };
    let agent = if general {
        Some("general".into())
    } else if discussion {
        Some("readonly".into())
    } else {
        agent
    };
    let execution_batch =
        !general && !discussion && agent.as_deref() != Some("dev-pair") && execution_batch;
    let autonomous = !general && !discussion && autonomous;
    let research_topic = crate::research_topics::validate_run_topic(
        &project_root,
        profile.as_deref(),
        process.research_topic.lock().unwrap().as_deref(),
        research_topic.as_deref(),
    )?;
    let model = model.or_else(|| process.model.lock().unwrap().clone());
    let reasoning = process.reasoning.lock().unwrap().clone();
    let block_tracker_writes =
        process.worktree_path.is_some() && !process.tracker_writes_enabled.load(Ordering::SeqCst);
    if let Some(id) = work_item_id.as_deref() {
        crate::run::work_start::validate_requested_item(id)?;
        if general
            || discussion
            || matches!(profile.as_deref(), Some("readonly" | "research"))
            || block_tracker_writes
        {
            return Err("当前对话不能领取需求，请返回主对话开始".into());
        }
    }
    let runtime = runtime_for(state, &session_id);
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    if runtime.compacting.load(Ordering::SeqCst) {
        return Err("当前对话正在压缩，完成后再发送".into());
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
                || generation != runtime.run_generation.load(Ordering::SeqCst)
                || async_generation != runtime.async_generation.load(Ordering::SeqCst)
                || runtime.running.load(Ordering::SeqCst)
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
    // The local lifecycle guard preserves queue/steer behavior; an idle local
    // runtime must also own this database/session before claiming a saved input.
    let execution_owner = if runtime.running.load(Ordering::SeqCst) {
        None
    } else {
        Some(Arc::new(
            kanzei_core::store::session_execution::try_acquire(
                &kanzei_core::project_state_path(&main_root),
                &session_id,
            )
            .map_err(|error| error.to_string())?,
        ))
    };
    let (prompt, delivery, attachments, initial_input) = match submission {
        Submission::Automatic { .. } => {
            unreachable!("automatic submission checked under lifecycle lock")
        }
        Submission::Saved { input_id, .. } | Submission::Notice { input_id, .. } => {
            let promoted = crate::run::input::resume_saved_input(
                &project_dir,
                &session_id,
                &input_id,
                runtime.running.load(Ordering::SeqCst),
            )
            .map_err(|e| e.to_string())?;
            match promoted {
                crate::run::input::SavedInputResume::Start(input) => {
                    (input.prompt.clone(), input.delivery, None, Some(input))
                }
                crate::run::input::SavedInputResume::Queued => return Ok("queued"),
                crate::run::input::SavedInputResume::Consumed => return Ok("consumed"),
            }
        }
        Submission::Prompt {
            prompt,
            delivery,
            attachments,
        } => {
            if runtime.running.load(Ordering::SeqCst) {
                if attachments.as_ref().is_some_and(|items| !items.is_empty()) {
                    return Err("当前任务运行中不能排队附件，请等待本轮完成后再发送".into());
                }
                // 自动续跑的重复事件可能在窗口关闭/重开或事件重放后再次到达；
                // 同文案的 pending 输入只保留一份。手动发送仍允许重复排队。
                if autonomous
                    && matches!(delivery, kanzei_core::Delivery::Queue)
                    && has_pending_queue_prompt(&project_dir, &session_id, &prompt)
                        .map_err(|e| e.to_string())?
                {
                    return Ok("queued");
                }
                let queued = admit_input(
                    &project_dir,
                    &session_id,
                    &prompt,
                    delivery,
                    execution_batch || autonomous,
                    work_item_id.as_deref(),
                )
                .map_err(|e| e.to_string())?;
                let _ = window.emit("kz:status", with_session_id(json!({ "stage": "排队", "detail": format!("已排队，前方输入将依次执行（{}）", queued.input_id) }), &session_id));
                return Ok("queued");
            }
            (prompt, delivery, attachments, None)
        }
    };
    let execution_owner =
        execution_owner.ok_or_else(|| "当前对话运行状态已变化，未启动第二个执行者".to_string())?;
    if !is_notice {
        *runtime.callback_options.lock().unwrap() = RunOptions {
            execution_batch: false,
            work_item_id: None,
            profile: profile.clone(),
            agent: agent.clone(),
            model: model.clone(),
            work_priority: work_priority.clone(),
            research_topic: research_topic.clone(),
            autonomous: false,
            auto_allow,
        };
    }
    runtime.running.store(true, Ordering::SeqCst);
    let asks = runtime.asks.clone();
    let ask_seq = state.ask_seq.clone();
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
        let mut next_input = initial_input;
        let mut next_prompt = prompt;
        let mut next_attachments = attachments;
        let mut next_work_item_id = work_item_id;
        loop {
            let promoted_id = next_input.as_ref().map(|input| input.input_id.clone());
            let result = async {
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
                        prompt: next_prompt,
                        attachments: next_attachments.take(),
                        project_dir: code_root.clone(),
                        main_root: main_root.clone(),
                        session_id: session_id.clone(),
                        execution_owner: execution_owner.clone(),
                        delivery,
                        promoted_input: next_input.take(),
                        process_id: process_id_for_run.clone(),
                        work_item_id: next_work_item_id.take(),
                    },
                    RunMode {
                        execution_batch: !discussion && execution.execution_batch,
                        // 排队输入共用这个循环；每轮重读,运行中改开关在下一轮生效。
                        phase_pipeline_enabled: !execution.callback
                            && process.phase_pipeline_enabled.load(Ordering::SeqCst),
                        subagents_enabled: !discussion
                            && process.subagents_enabled.load(Ordering::SeqCst),
                        block_tracker_writes,
                        profile: profile.clone(),
                        research_topic: research_topic.clone(),
                        agent_name: agent.clone(),
                        model_override: model.clone(),
                        work_priority: work_priority.clone(),
                        reasoning_override: reasoning.clone(),
                        autonomous: execution.autonomous,
                        auto_allow,
                    },
                    RuntimeHandles {
                        lifecycle: lifecycle.clone(),
                        asks: asks.clone(),
                        ask_seq: ask_seq.clone(),
                        collaboration_probe: collaboration_probe.clone(),
                        current_stage: current_stage.clone(),
                        conversation: conversation.clone(),
                        live_run: live_run.clone(),
                        task_cancellations: task_cancellations.clone(),
                        auto_runs: auto_runs.clone(),
                        coordinator: coordinator.clone(),
                        halt_slot: halt_slot.clone(),
                        run_generation: run_generation.clone(),
                    },
                )
                .await
            }
            .await;
            if let Err(e) = &result {
                if run_error_idle_reason(e) == "failed" {
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
            }
            if let Err(error) = &result {
                let _lifecycle = lifecycle.lock().unwrap();
                // Assembly can fail before the normal round finalizer sees the promoted reply.
                if let Some(id) = promoted_id {
                    if let Ok(store) = kanzei_core::SessionStore::open(
                        &kanzei_core::project_state_path(&main_root),
                    ) {
                        let _ = finish_failed_promoted_input(&store, &id, error);
                    }
                }
                finish_scheduler(
                    &window,
                    &runtime_for_task,
                    &session_id,
                    run_error_idle_reason(error),
                );
                break;
            }
            next_input = {
                let _lifecycle = lifecycle.lock().unwrap();
                match promote_next_input(&project_dir, &session_id) {
                    Ok(input) => {
                        if input.is_none() {
                            finish_scheduler(&window, &runtime_for_task, &session_id, "completed");
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
                        finish_scheduler(&window, &runtime_for_task, &session_id, "failed");
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
    });
    *runtime.current_run.lock().unwrap() = Some(handle);
    if !runtime.running.load(Ordering::SeqCst) {
        runtime.current_run.lock().unwrap().take();
    }
    Ok("started")
}

/// Caller holds lifecycle. Publish the old run's terminal state before a reply may wake a new one.
fn finish_scheduler(window: &Window, runtime: &SessionRuntime, session_id: &str, reason: &str) {
    *runtime.stage.lock().unwrap() = if reason == "failed" {
        "失败"
    } else {
        "空闲"
    }
    .into();
    runtime.current_run.lock().unwrap().take();
    let _ = window.emit(
        "kz:idle",
        with_session_id(json!({ "reason": reason }), session_id),
    );
    runtime.running.store(false, Ordering::SeqCst);
}

/// 运行画像:该项目下全部对话(主对话、讨论、并行线 `#p*`)最近若干轮,按时间倒序。
/// 返回形状固定为 `{ rounds: [...] }`,每轮另带 `sessionId`(所属会话,前端据此跳到那段对话)。
/// 概览页会周期性拉它,所以读库放进阻塞线程池,不占窗口命令线程。
#[tauri::command]
pub(crate) async fn run_metrics(
    project_dir: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || run_metrics_rounds(&project_dir, limit))
        .await
        .map_err(|error| format!("运行画像查询任务失败: {error}"))?
}

fn run_metrics_rounds(
    project_dir: &str,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    // 与会话落库同一个身份根:projectDir 写法不同(斜杠、`\\?\` 前缀)也得落在同一组 session_id 上。
    let root = normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let base_session_id = kanzei_core::project_session_id(&root);
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let rows = store
        .recent_project_episodes(&base_session_id, limit)
        .map_err(|e| e.to_string())?;
    let parse = |text: &str| {
        serde_json::from_str::<serde_json::Value>(text).unwrap_or(serde_json::json!({}))
    };
    let rounds: Vec<serde_json::Value> = rows
        .into_iter()
        .map(
            |(
                session_id,
                at,
                prompt,
                outcome,
                steps,
                input,
                output,
                tools,
                context,
                metrics,
                duration_ms,
            )| {
                json!({
                    "sessionId": session_id,
                    "at": at,
                    "prompt": prompt,
                    "outcome": outcome,
                    "steps": steps,
                    "inputTokens": input,
                    "outputTokens": output,
                    "tools": parse(&tools),
                    "context": parse(&context),
                    "metrics": parse(&metrics),
                    "measured": metrics.trim() != "{}" && !metrics.trim().is_empty(),
                    // Historical rows predate duration observation and contain zero.
                    "durationMs": (duration_ms > 0).then_some(duration_ms),
                })
            },
        )
        .collect();
    Ok(json!({ "rounds": rounds }))
}

/// R-338 B3:读取可重建的 task 运行画像；前端只消费此 projection，不自行分组。
#[tauri::command]
pub(crate) async fn run_metrics_by_task(project_dir: String) -> Result<serde_json::Value, String> {
    // 历史审计和 SQLite 打开均可能阻塞；整个查询留在阻塞线程池，释放窗口事件循环。
    tauri::async_runtime::spawn_blocking(move || {
        let root = PathBuf::from(&project_dir);
        let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
            .map_err(|error| error.to_string())?;
        let projection = store.task_metrics().map_err(|error| error.to_string())?;
        serde_json::to_value(projection).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("任务画像查询任务失败: {error}"))?
}

/// R-240:从 prompt_head 提取需求 ID(`R-123` / `D-321`),取第一个命中。
/// 自举/取活轮的 prompt 以条目标题开头(R-xxx …),用户轮通常无——据此归类。
pub(crate) fn extract_ticket_id(prompt_head: &str) -> Option<String> {
    let bytes = prompt_head.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        // 前一个字符是字母/数字/下划线说明 `R-`/`D-` 只是单词的一截(ERROR-5、PR-12),不是条目号。
        let at_boundary = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if at_boundary && (bytes[i] == b'R' || bytes[i] == b'D') && bytes[i + 1] == b'-' {
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
pub(crate) fn ticket_complexity(project_root: &Path, id: &str) -> Option<String> {
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
pub(crate) fn aggregate_run_metrics(
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
mod tests {
    use super::{run_metrics, run_metrics_by_category, run_metrics_by_task};
    use kanzei_core::store::TaskOutcome;
    use kanzei_core::{EpisodeRecord, SessionStore};
    use std::path::{Path, PathBuf};

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
            "apply the reviewed correction",
            kanzei_core::Delivery::Queue,
            false,
            None,
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

    #[test]
    fn delivered_path_accepts_project_or_explicit_worktree_root_only() {
        let (root, _) = fixture("delivered-path");
        let worktree = root.with_file_name(format!(
            "{}-worktree",
            root.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&worktree).unwrap();
        let project_file = root.join("result.html");
        let worktree_file = worktree.join("result.html");
        let outside_root = worktree.with_file_name(format!(
            "{}-outside",
            root.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&outside_root).unwrap();
        let outside_file = outside_root.join("outside.html");
        std::fs::write(&project_file, "project").unwrap();
        std::fs::write(&worktree_file, "worktree").unwrap();
        std::fs::write(&outside_file, "outside").unwrap();
        let root_text = root.display().to_string();
        let worktree_text = worktree.display().to_string();

        assert!(
            super::resolve_delivered_path(&root_text, &project_file.display().to_string()).is_ok()
        );
        assert!(
            super::resolve_delivered_path(&root_text, &worktree_file.display().to_string())
                .is_err()
        );
        assert!(super::resolve_delivered_path_with_worktree(
            &root_text,
            &worktree_file.display().to_string(),
            Some(Path::new(&worktree_text)),
        )
        .is_ok());
        assert!(super::resolve_delivered_path_with_worktree(
            &root_text,
            &outside_file.display().to_string(),
            Some(Path::new(&worktree_text)),
        )
        .is_err());

        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(outside_root).ok();
        std::fs::remove_dir_all(worktree).ok();
    }

    fn append_episode(root: &Path, session_id: &str, prompt: &str) {
        let store = SessionStore::open(&kanzei_core::project_state_path(root)).unwrap();
        store
            .append_episode(&EpisodeRecord {
                session_id,
                prompt_head: prompt,
                outcome: "completed",
                steps: 3,
                input_tokens: 17,
                output_tokens: 5,
                tools_json: "{\"read\":1}",
                context_json: "{}",
                metrics_json: "{\"duration_ms\":12}",
                provider: "test-provider",
                model: "test-model",
                run_id: "run-test",
                input_id: "input-test",
                duration_ms: 12,
                overflow_json: "[]",
            })
            .unwrap();
    }

    #[tokio::test]
    async fn run_metrics_command_reads_real_episode_projection() {
        let (root, session_id) = fixture("episodes");
        append_episode(&root, &session_id, "R-296 command metrics");

        let output = run_metrics(root.display().to_string(), Some(1))
            .await
            .unwrap();
        assert_eq!(output["rounds"][0]["prompt"], "R-296 command metrics");
        assert_eq!(output["rounds"][0]["outcome"], "completed");
        assert_eq!(output["rounds"][0]["inputTokens"], 17);
        assert_eq!(output["rounds"][0]["durationMs"], 12);
        assert_eq!(output["rounds"][0]["measured"], true);
        assert_eq!(output["rounds"][0]["sessionId"], session_id);
        std::fs::remove_dir_all(root).ok();
    }

    /// UX-095:运行画像聚合项目下全部会话,并行线(`#p*`)与讨论的轮次不再缺失。
    #[tokio::test]
    async fn run_metrics_command_aggregates_parallel_line_sessions() {
        let (root, session_id) = fixture("episodes-lines");
        append_episode(&root, &session_id, "主对话一轮");
        std::thread::sleep(std::time::Duration::from_millis(3));
        let line_session = format!("{session_id}#p2");
        append_episode(&root, &line_session, "并行线一轮");
        // 同一目录的另一种写法(正斜杠)也要落在同一组会话上。
        let slashed = root.display().to_string().replace('\\', "/");

        let output = run_metrics(slashed, None).await.unwrap();
        let rounds = output["rounds"].as_array().unwrap();
        assert_eq!(rounds.len(), 2, "主对话与并行线的轮次都在");
        assert_eq!(rounds[0]["prompt"], "并行线一轮", "按时间倒序合并");
        assert_eq!(rounds[0]["sessionId"], line_session.as_str());
        assert_eq!(rounds[1]["sessionId"], session_id.as_str());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn extract_ticket_id_要求词边界() {
        use super::extract_ticket_id;
        assert_eq!(extract_ticket_id("ERROR-5 PR-12 CR-7"), None);
        assert_eq!(extract_ticket_id("my_R-5"), None);
        assert_eq!(
            extract_ticket_id("修复D-321 后继续"),
            Some("D-321".into()),
            "紧贴汉字也算词边界"
        );
        assert_eq!(extract_ticket_id("(R-12)"), Some("R-12".into()));
        assert_eq!(extract_ticket_id("PR-12 与 R-9"), Some("R-9".into()));
    }

    #[test]
    fn run_metrics_by_category_command_uses_requirement_complexity_source() {
        let (root, session_id) = fixture("category");
        std::fs::write(
            root.join(".kanzei/project/requirements.md"),
            "## R-296 Tauri command 与 run 链路测试基座\n- 复杂度: 大\n",
        )
        .unwrap();
        append_episode(&root, &session_id, "R-296 command metrics");

        let output = run_metrics_by_category(root.display().to_string(), Some(1)).unwrap();
        let group = output["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|group| group["kind"] == "R" && group["complexity"] == "大")
            .expect("R-296 应从 requirements.md 分类");
        assert_eq!(group["count"], 1);
        assert_eq!(group["sumInput"], 17);
        assert_eq!(output["uncategorized"]["count"], 0);
        std::fs::remove_dir_all(root).ok();
    }
    #[tokio::test]
    async fn run_metrics_by_task_command_reads_real_task_projection() {
        let (root, session_id) = fixture("task-metrics");
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store
            .append_episode(&EpisodeRecord {
                session_id: &session_id,
                prompt_head: "legacy round retained by old API",
                outcome: "completed",
                steps: 2,
                input_tokens: 13,
                output_tokens: 7,
                tools_json: "{}",
                context_json: "[]",
                metrics_json: "{}",
                provider: "test-provider",
                model: "test-model",
                run_id: "run-legacy-command",
                input_id: "input-legacy-command",
                duration_ms: 22,
                overflow_json: "[]",
            })
            .unwrap();
        let episode_id = store
            .append_episode(&EpisodeRecord {
                session_id: &session_id,
                prompt_head: "task projection command",
                outcome: "completed",
                steps: 4,
                input_tokens: 31,
                output_tokens: 11,
                tools_json: "{}",
                context_json: "[]",
                metrics_json: "{}",
                provider: "test-provider",
                model: "test-model",
                run_id: "run-task-command",
                input_id: "input-task-command",
                duration_ms: 44,
                overflow_json: "[]",
            })
            .unwrap();
        store
            .append_task_started(
                &session_id,
                "task-command-closed",
                Some("命令 task"),
                Some("input-task-command"),
            )
            .unwrap();
        store
            .append_task_membership_added(
                &session_id,
                "task-command-closed",
                "membership-command",
                Some("input-task-command"),
                Some(episode_id),
            )
            .unwrap();
        store
            .append_task_closed(
                &session_id,
                "task-command-closed",
                TaskOutcome::Completed,
                "agent",
                None,
            )
            .unwrap();
        store
            .append_task_started(&session_id, "task-command-open", None, None)
            .unwrap();

        let output = run_metrics_by_task(root.display().to_string())
            .await
            .unwrap();
        assert_eq!(output["completed_tasks"].as_array().unwrap().len(), 1);
        assert_eq!(output["in_progress_tasks"].as_array().unwrap().len(), 1);
        assert_eq!(output["trend"]["closed_task_count"], 1);
        assert_eq!(
            output["completed_tasks"][0]["task_id"],
            "task-command-closed"
        );
        assert_eq!(
            output["completed_tasks"][0]["rounds"][0]["episode_id"],
            episode_id
        );
        assert_eq!(output["legacy"]["classification"], "legacy_unassigned");
        assert_eq!(output["legacy"]["episode_count"], 1);
        assert_eq!(output["audit"]["total_episode_count"], 2);
        assert_eq!(output["audit"]["assigned_episode_count"], 1);

        let old_output = run_metrics(root.display().to_string(), Some(10))
            .await
            .unwrap();
        let old_rounds = old_output["rounds"].as_array().unwrap();
        assert_eq!(
            old_rounds.len(),
            2,
            "旧 rounds API 必须继续看见 legacy episode"
        );
        assert!(old_rounds
            .iter()
            .any(|round| round["prompt"] == "legacy round retained by old API"));
        std::fs::remove_dir_all(root).ok();
    }
}

/// R-329:打开或在资源管理器中定位一份已交付的文件。
///
/// 路径经前端往返回来,这里**重做一次**工具侧的同一判定(canonicalize 后必须
/// 落在项目根内)。本仓的威胁模型里没有敌对前端,这道校验挡的是**意外**——
/// 载荷被历史重放、路径拼错、或将来某处忘了先过 deliver 的校验就直接调它。
/// `project_dir` 由前端给,与本 crate 其余命令同一惯例。
/// 交付路径的根内校验(open_delivered_path 与预览的 delivered_image 共用):canonicalize 后
/// 必须落在项目根内且是文件。
/// Resolve a delivered file inside the project root or an explicitly validated worktree.
fn resolve_delivered_path_with_worktree(
    project_dir: &str,
    path: &str,
    worktree_root: Option<&Path>,
) -> Result<std::path::PathBuf, String> {
    let root = Path::new(project_dir)
        .canonicalize()
        .map_err(|error| format!("项目根不可解析: {error}"))?;
    let target = Path::new(path)
        .canonicalize()
        .map_err(|error| format!("路径不可解析: {error}"))?;
    if !target.is_file() {
        return Err(format!("不是文件: {}", target.display()));
    }
    let in_project = target.starts_with(&root);
    let in_worktree = worktree_root
        .and_then(|path| path.canonicalize().ok())
        .is_some_and(|worktree| target.starts_with(worktree));
    if !in_project && !in_worktree {
        return Err(format!("拒绝打开工作树之外的路径: {}", target.display()));
    }
    Ok(target)
}

pub(crate) fn resolve_delivered_path(
    project_dir: &str,
    path: &str,
) -> Result<std::path::PathBuf, String> {
    resolve_delivered_path_with_worktree(project_dir, path, None)
}

fn resolve_delivered_path_for_line(
    state: &AppState,
    project_dir: &str,
    path: &str,
    process_id: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let process = process_id.and_then(|id| state.processes.lock().unwrap().get(id).cloned());
    let worktree_root = process
        .filter(|process| process.origin_project.0 == project_root)
        .and_then(|process| process.worktree_path.map(|worktree| worktree.0));
    resolve_delivered_path_with_worktree(project_dir, path, worktree_root.as_deref())
}

#[tauri::command]
pub(crate) fn open_delivered_path(
    state: State<'_, AppState>,
    project_dir: String,
    path: String,
    mode: String,
    process_id: Option<String>,
) -> Result<(), String> {
    let target =
        resolve_delivered_path_for_line(&state, &project_dir, &path, process_id.as_deref())?;
    let status = if mode == "reveal" {
        // explorer /select 会打开父目录并选中该文件。它的退出码不遵循常规约定
        // (成功也可能非 0),所以只在**启动失败**时报错,不看退出码。
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&target)
            .spawn()
            .map(|_| ())
    } else {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(&target)
            .spawn()
            .map(|_| ())
    };
    status.map_err(|error| format!("启动失败: {error}"))
}
