//! 运行类 Tauri command(R-253 批6b,纯搬迁自 run/mod.rs)。
//!
//! 独立理由:IPC 入口与运行编排分离——run_prompt(外层 scheduler,排队/联动/根发现)、
//! stop_run/stop_task(停止)、pending_asks_get/answer_ask(权限/提问应答)、run_metrics/
//! run_metrics_by_category(运行指标)都是「UI 调用 → AppState 操作」的薄层,不承载
//! 编排逻辑;留在 run 模块只会让「运行主链路」继续膨胀(照 files_view.rs 模式)。
//!
//! 依赖:run_task(Round Coordinator)在 crate::run::coordinator,输入准入在
//! crate::run::input,共享 helper(now_ms/emit_stage 等)在 crate::run。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::json;
use tauri::{Emitter, State, Window};

use crate::{
    ensure_default_process, normalized_project_root, pending_ask_payload, process_session_id,
    runtime_for, stop_runtime_and_finalize, take_pending_ask, with_session_id, AppState,
    PromptAttachment, SessionRuntime,
};

use crate::run::assembly::{RoundRequest, RunMode, RuntimeHandles};
use crate::run::coordinator::run_task;
use crate::run::input::{
    admit_input, code_root_for, has_pending_queue_prompt, parse_delivery, promote_next_input,
};

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
    Ok(asks
        .iter()
        .map(|(id, pending)| pending_ask_payload(*id, pending))
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
        .map(|cwd| normalized_project_root(&cwd));
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
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    let hit = runtime.task_cancellations.cancel(&task_id);
    if !hit {
        return Err(format!("子代理 {task_id} 不在运行中或已结束"));
    }
    Ok(true)
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
) -> Result<(), String> {
    let autonomous = autonomous.unwrap_or(false);
    // D-281:自动放行开关——autonomous/parallel 轮在用户勾选时传 AutoAllow。
    let auto_allow = auto_allow.unwrap_or(false);
    let delivery = parse_delivery(delivery.as_deref()).map_err(|e| e.to_string())?;
    // 规范化主根:会话 id 与进程归属的身份键(canonicalize 过,形态唯一)。
    let project_root = normalized_project_root(Path::new(&project_dir));
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
    let research_topic = crate::research_topics::validate_run_topic(
        &project_root,
        profile.as_deref(),
        process.research_topic.lock().unwrap().as_deref(),
        research_topic.as_deref(),
    )?;
    let model = model.or_else(|| process.model.lock().unwrap().clone());
    let reasoning = process.reasoning.lock().unwrap().clone();
    let phase_pipeline_enabled = process.phase_pipeline_enabled.load(Ordering::SeqCst);
    let subagents_enabled = process.subagents_enabled.load(Ordering::SeqCst);
    let block_tracker_writes =
        process.worktree_path.is_some() && !process.tracker_writes_enabled.load(Ordering::SeqCst);
    let runtime = runtime_for(&state, &session_id);
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    {
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
                return Ok(());
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
                RoundRequest {
                    prompt: next_prompt,
                    attachments: next_attachments.take(),
                    project_dir: code_root.clone(),
                    main_root: main_root.clone(),
                    session_id: session_id.clone(),
                    delivery,
                    promoted_input: next_input.take(),
                    process_id: process_id_for_run.clone(),
                },
                RunMode {
                    phase_pipeline_enabled,
                    subagents_enabled,
                    block_tracker_writes,
                    profile: profile.clone(),
                    research_topic: research_topic.clone(),
                    agent_name: agent.clone(),
                    model_override: model.clone(),
                    work_priority: work_priority.clone(),
                    reasoning_override: reasoning.clone(),
                    autonomous,
                    auto_allow,
                },
                RuntimeHandles {
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

/// R-338 B3:读取可重建的 task 运行画像；前端只消费此 projection，不自行分组。
#[tauri::command]
pub(crate) fn run_metrics_by_task(project_dir: String) -> Result<serde_json::Value, String> {
    let root = PathBuf::from(&project_dir);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|error| error.to_string())?;
    let projection = store.task_metrics().map_err(|error| error.to_string())?;
    serde_json::to_value(projection).map_err(|error| error.to_string())
}

/// R-240:从 prompt_head 提取需求 ID(`R-123` / `D-321`),取第一个命中。
/// 自举/取活轮的 prompt 以条目标题开头(R-xxx …),用户轮通常无——据此归类。
pub(crate) fn extract_ticket_id(prompt_head: &str) -> Option<String> {
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
        let session_id = kanzei_core::project_session_id(&root);
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store
            .create_session(&session_id, &root.display().to_string(), None)
            .unwrap();
        (root, session_id)
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

    #[test]
    fn run_metrics_command_reads_real_episode_projection() {
        let (root, session_id) = fixture("episodes");
        append_episode(&root, &session_id, "R-296 command metrics");

        let output = run_metrics(root.display().to_string(), Some(1)).unwrap();
        assert_eq!(output["rounds"][0]["prompt"], "R-296 command metrics");
        assert_eq!(output["rounds"][0]["outcome"], "completed");
        assert_eq!(output["rounds"][0]["inputTokens"], 17);
        assert_eq!(output["rounds"][0]["measured"], true);
        std::fs::remove_dir_all(root).ok();
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
    #[test]
    fn run_metrics_by_task_command_reads_real_task_projection() {
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

        let output = run_metrics_by_task(root.display().to_string()).unwrap();
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

        let old_output = run_metrics(root.display().to_string(), Some(10)).unwrap();
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
#[tauri::command]
pub(crate) fn open_delivered_path(
    project_dir: String,
    path: String,
    mode: String,
) -> Result<(), String> {
    let root = std::path::Path::new(&project_dir)
        .canonicalize()
        .map_err(|error| format!("项目根不可解析: {error}"))?;
    let target = std::path::Path::new(&path)
        .canonicalize()
        .map_err(|error| format!("路径不可解析: {error}"))?;
    if !target.starts_with(&root) {
        return Err(format!("拒绝打开工作树之外的路径: {}", target.display()));
    }
    if !target.is_file() {
        return Err(format!("不是文件: {}", target.display()));
    }
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
