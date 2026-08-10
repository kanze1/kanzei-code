use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_core::{run_once_with_parts, AskFuture, RunEvent};
use kanzei_harness::auto_run::AutoRunCtx;
use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use kanzei_harness::{ConfigComponent, Harness, KanzeiConfig, ResolveCtx, ToolCtx};
use serde_json::json;
use tauri::{Emitter, State, Window};
use tokio::sync::oneshot;

use crate::{
    conversation, ensure_default_process, flush_live_run, memory, process_session_id,
    prompt_attachment_parts, runtime_for, stop_runtime_and_finalize, take_pending_ask,
    with_session_id, AppState, LiveRun, PendingAsk, PromptAttachment, SessionRuntime,
};

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
    subagent_enabled: bool,
    profile: Option<String>,
    agent_name: Option<String>,
    model_override: Option<String>,
    work_priority: Option<String>,
    reasoning_override: Option<String>,
    conversation: Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    live_run: Arc<Mutex<LiveRun>>,
    auto_runs: Arc<Mutex<HashMap<String, crate::auto_run::AutoRunController>>>,
    delivery: kanzei_core::Delivery,
    promoted_input: Option<kanzei_core::AdmittedInput>,
    // R-171:项目级协调器(所有 ProcessHandle 共享)。主对话 writer run
    // 在此获取写租约并持有到本轮结束;RAII 保证任何结束路径都释放。
    coordinator: Arc<kanzei_core::orchestration::MemoryCoordinator>,
    process_id: String,
) -> anyhow::Result<()> {
    // 阶段汇报:让前端每一步都有着落(用户反馈:要详细指示)。
    let stage = |name: &str, detail: String| {
        emit_stage(window, &session_id, name, detail);
    };

    let cwd = PathBuf::from(&project_dir);
    anyhow::ensure!(cwd.is_dir(), "工作目录不存在: {project_dir}");

    stage("配置", format!("加载 {}", cwd.display()));
    let (config, config_warnings) = KanzeiConfig::load_with_warnings(&cwd)?;
    let config = Arc::new(config);
    report_config_warnings(window, &session_id, &config, &config_warnings);
    let profile = resolve_profile(profile.as_deref(), &config)?;
    // R-050 D1「运行时重定向主根」的落点:cwd 是代码工作树(将来 = worktree),
    // project_root 恒为主根——托管文档、state.db、记忆全部走它。
    let project_root = main_root;
    let rctx = ResolveCtx {
        profile,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };

    let harness = build_run_harness();
    let snapshot = harness.resolve(&rctx)?;
    let mut agent = snapshot.select_agent(agent_name.as_deref())?.clone();
    let work_priority = normalize_work_priority(work_priority.as_deref());
    append_dev_guidance(&mut agent.system, profile, work_priority);
    stage(
        "装配",
        format!(
            "harness 就绪:agent {} · {} 个工具",
            agent.name,
            snapshot.materialize_tools().len()
        ),
    );

    // 界面模型下拉直选优先于 agent 定义。
    let model_ref = resolve_model_ref(model_override, &agent.model);
    let resolved = config.resolve_model(&model_ref)?;
    let proxy = resolve_proxy(&config);
    stage(
        "鉴权",
        auth_stage_detail(
            &resolved.provider_name,
            &resolved.model,
            resolved.provider.auth.is_some(),
        ),
    );
    let route = kanzei_core::build_route(&resolved, &proxy).await?;
    stage("请求", "已发起,等待模型响应…".into());
    let client = new_llm_client(&proxy)?;
    let ctx = ToolCtx::new(cwd, project_root.clone());
    let mut runner_config = build_runner_config(
        &resolved,
        &config,
        reasoning_override.as_deref(),
        &ctx.project_root,
    );
    // R-171:主对话 writer 阶段强制串行写(普通工具 FIFO、task 禁用)。
    runner_config.execution_policy =
        kanzei_harness::orchestration::ExecutionPolicy::ReadParallelWriteSerial;

    let state_path = kanzei_core::project_state_path(&ctx.project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(&session_id, &ctx.project_root.display().to_string(), None)?;
    let is_new_input = promoted_input.is_none();
    let promoted = if let Some(input) = promoted_input {
        input
    } else {
        let input_id = format!(
            "input_{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        store.admit_input(&session_id, &input_id, &prompt, delivery)?;
        store.append_event(
            &session_id,
            "prompt.admitted",
            &json!({ "input_id": input_id, "delivery": if matches!(delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
        )?;
        store
            .promote_next_input(&session_id)?
            .ok_or_else(|| anyhow::anyhow!("无法提升已提交的桌面端输入"))?
    };
    if is_new_input {
        store.append_event(
            &session_id,
            "prompt.promoted",
            &json!({ "input_id": promoted.input_id, "delivery": if matches!(promoted.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
        )?;
    }
    let prompt = promoted.prompt;
    // promoted → running,并记住本轮身份与墙钟(D-173)。少了 running/completed 这段
    // 生命周期,跑完的输入永远停在 promoted,以后任何一次停止都会把它追认成 cancelled。
    let promoted_input_id = promoted.input_id.clone();
    store.start_input(&promoted_input_id)?;
    let run_id = format!(
        "run_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );
    let run_started = std::time::Instant::now();
    // 本轮开始墙钟毫秒:R-161 回填 recall_events 的 episode_id 用(开跑预检索
    // 先于 episode 落库,只能靠时间窗归因到本轮,与 CLI 同一口径)。
    let run_epoch_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default();
    store.set_status(&session_id, "running")?;
    append_run_notification(&store, &session_id, "running", "任务已开始", false)?;
    store.append_event(
        &session_id,
        "session.status_changed",
        &json!({ "status": "running" }),
    )?;
    // R-171 批3:主对话 writer run 获取项目级写租约并持有到本轮结束。
    // 权限询问发生在租约获取之前(设计不变量 6)——此处无询问,直接申请;
    // RAII:任何结束路径(正常/错误/取消/abort)都会 drop 释放,绝不永久占用。
    // 注意:acquire_writer_lease 在项目已有 writer 时会排队等待,这是「串行写」
    // 的强制点——第二个 ProcessHandle 必须等当前 writer 释放后才能拿到租约。
    // R-173 批5:writer 事件经 OrchestrationEvent 的**单一出口**落 session_events。
    // 这里原本是三处手写字符串 + 手拼 payload,与枚举没有类型联系——改名或加字段时
    // 编译器不会提醒,两边必然漂移。现在类型名与 payload 都由事件自己给出。
    let orchestration_trace = Arc::new(crate::orchestration_trace::SessionEventObserver::open(
        &state_path,
        &session_id,
    )?);
    let writer_event = |event: kanzei_harness::orchestration::OrchestrationEvent| {
        use kanzei_harness::orchestration::PhaseObserver;
        orchestration_trace.observe(&event);
    };
    // R-173 批6:阶段流水线**只在自主推进轮**装配。手动一问一答走 else 分支,
    // 与引入前逐字节相同——「不构造编排对象就是关」,没有第二个开关可以配错。
    let phase_pipeline_on = auto_runs
        .lock()
        .unwrap()
        .get(&session_id)
        .is_some_and(|ctrl| ctrl.enabled);
    let mut pipeline = if phase_pipeline_on {
        // R-173:勘察/复核用哪条路由由 `[models] scout` 决定。解析失败**不静默**——
        // 阶段面板上会有一行,然后按未配置处理(沿用 fast),而不是让整轮跑不成。
        let scout_route = match config.models.scout.as_deref() {
            None => None,
            Some(model_ref) => match config.resolve_model(model_ref) {
                Ok(resolved) => match kanzei_core::build_route(&resolved, &proxy).await {
                    Ok(route) => {
                        stage(
                            "勘察路由",
                            format!("{}:{}", resolved.provider_name, resolved.model),
                        );
                        Some(crate::phase_pipeline::ScoutRoute {
                            service_tier: config.service_tier_for(&resolved),
                            model: resolved.model.clone(),
                            route,
                        })
                    }
                    Err(error) => {
                        stage(
                            "勘察路由",
                            format!("{model_ref} 建链失败,回退 fast:{error}"),
                        );
                        None
                    }
                },
                Err(error) => {
                    stage(
                        "勘察路由",
                        format!("{model_ref} 解析失败,回退 fast:{error}"),
                    );
                    None
                }
            },
        };
        Some(crate::phase_pipeline::PhasePipeline::start(
            Arc::clone(&coordinator) as Arc<dyn ProjectExecutionCoordinator>,
            Arc::clone(&orchestration_trace)
                as Arc<dyn kanzei_harness::orchestration::PhaseObserver>,
            ctx.project_root.clone(),
            &run_id,
            &process_id,
            &config.limits,
            scout_route,
        ))
    } else {
        None
    };
    // 写租约的取得时机是两条路的**唯一实质差异**:
    // - 流水线开:**推迟到汇总屏障之后**由编排对象取(不变量 2——勘察全终态前
    //   writer 不得启动)。此处不取。
    // - 流水线关:与 R-171 完全一致,当场取、持有整轮。
    let plain_lease = if pipeline.is_some() {
        None
    } else {
        writer_event(
            kanzei_harness::orchestration::OrchestrationEvent::WriterQueued {
                project_root: ctx.project_root.clone(),
                run_id: run_id.clone(),
                process_id: process_id.clone(),
                reason: format!("session {session_id} writer run"),
            },
        );
        let lease = coordinator
            .acquire_writer_lease(crate::phase_pipeline::plain_writer_request(
                &ctx.project_root,
                &run_id,
                &process_id,
                &session_id,
            ))
            .await
            .map_err(|e| anyhow::anyhow!("无法获取项目写租约: {e}"))?;
        writer_event(
            kanzei_harness::orchestration::OrchestrationEvent::WriterAcquired {
                project_root: ctx.project_root.clone(),
                run_id: run_id.clone(),
                process_id: process_id.clone(),
            },
        );
        Some(lease)
    };
    // 持有到 run_task 返回(Release 事件在尾部显式写);异常/abort 路径
    // 由协调器快照可见,租约不泄漏。流水线路径的租约由编排对象持有。
    let _write_lease = plain_lease.map(|lease| WriterLeaseTrace { _lease: lease });
    // 注入执行身份:两把键**必须分开取**,serial 策略下普通工具 FIFO 串行 +
    // task 禁用(设计不变量 3/5)。
    //
    // R-141 拆开这两把键,服务的是 R-050 D1「运行时重定向主根」:worktree 线
    // 上线后,同一项目的 N 棵树以 cwd=worktree、project_root=主根 运行,于是——
    //
    // ① `project_write_key` = **规范化主根**,N 棵树必须**相同**。
    //    主根 `.kanzei` 的 tracker/记忆是所有线唯一的共享写点,键一旦随树分裂,
    //    跨进程单写仲裁就被绕过(两条线同时重写同一个 docstore = lost update)。
    //    这里取 normalized_project_root:它比 project_root 多一次 canonicalize,
    //    保证不同路径写法落进同一个仲裁桶,且与 run_prompt 算给会话 id/进程归属
    //    的那个身份键逐字节相同。(它内部那次 discover 对已解析的主根是 no-op,
    //    不是根发现——线路径不做根发现这条不变式仍然成立。)
    // ② `worktree_key` = **代码树**,N 棵树必须**不同**。
    //    它是工具内并发锁键,bash/git/edit 真实作用于 ctx.cwd;若拿主根当键,
    //    互不相干的两棵树会因为主根相同而彼此串锁、白白串行。
    //
    // 一句话:写主根的串行,写代码的并行。改任何一行前先确认这条不变式还成立。
    let project_write_key = crate::normalized_project_root(&ctx.project_root)
        .display()
        .to_string();
    let worktree_key = ctx.cwd.display().to_string();
    let mut ctx = ctx;
    ctx = ctx.with_identity(
        worktree_key,
        project_write_key,
        run_id.clone(),
        process_id.clone(),
    );
    let _ = window.emit(
        "kz:meta",
        with_session_id(
            json!({
                "profile": format!("{profile:?}").to_lowercase(),
                "agent": agent.name,
                "model": format!("{}:{}", resolved.provider_name, resolved.model),
                "contextLimit": resolved.provider.context_limit,
            }),
            &session_id,
        ),
    );

    let event_window = window.clone();
    let session_id_for_events = session_id.clone();
    let emit_event = move |name: &str, payload: serde_json::Value| {
        event_window.emit(name, with_session_id(payload, &session_id_for_events))
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
    let mut on_event = move |event: RunEvent| {
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
                    live.trace.push(json!({
                        "kind": "turn.started", "step": step, "at": now_ms(),
                    }));
                }
                emit_event("kz:turn", json!({ "step": step, "maxSteps": max_steps }))
            }
            RunEvent::Text(text) => emit_event("kz:text", json!({ "text": text })),
            RunEvent::Reasoning(text) => emit_event("kz:reasoning", json!({ "text": text })),
            RunEvent::ToolStart {
                id,
                name,
                summary,
                input,
            } => {
                tool_started
                    .lock()
                    .unwrap()
                    .insert(id.clone(), std::time::Instant::now());
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "tool.started", "id": id, "name": name,
                    "summary": summary, "at": now_ms(),
                }));
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
                preview,
                display,
            } => {
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "tool.completed", "id": id, "name": name, "ok": ok,
                    "durationMs": elapsed_ms(&id), "at": now_ms(),
                    // 失败原因要留档,成功的预览不必——轨迹不是第二份对话记录。
                    "error": (!ok).then(|| preview.chars().take(400).collect::<String>()),
                }));
                emit_event(
                    "kz:tool-end",
                    json!({ "id": id, "name": name, "ok": ok, "preview": preview, "display": display }),
                )
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
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "context.compacted", "before": before_tokens, "after": after_tokens,
                    "budget": budget_tokens, "limit": limit_tokens,
                    "dropped": dropped_messages, "at": now_ms(),
                }));
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
            RunEvent::PermissionResolved {
                tool_call_id,
                action,
                resource,
                decision,
                source,
            } => {
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "permission.resolved", "id": tool_call_id, "action": action,
                    "resource": resource, "decision": decision, "source": source, "at": now_ms(),
                }));
                Ok(())
            }
            // 子代理实时状态:挂到对应 task 块的进度行,并附带可展开的子工具轨迹。
            RunEvent::TaskProgress { id, text, trace } => {
                let payload = json!({
                    "id": id,
                    "text": text,
                    "trace": trace.map(|item| json!({
                        "child_id": item.child_id,
                        "phase": item.phase,
                        "name": item.name,
                        "summary": item.summary,
                        "ok": item.ok,
                        "preview": item.preview,
                        "display": item.display,
                    })),
                });
                trace_log.lock().unwrap().trace.push(payload.clone());
                emit_event("kz:task-progress", payload)
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
            } => emit_event(
                "kz:stream-restart",
                json!({
                    "attempt": attempt,
                    "max": max,
                    "delayMs": delay_ms,
                    "detail": format!("连接中断,重新请求本轮 {attempt}/{max},等待 {delay_ms}ms"),
                }),
            ),
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
    };

    let ask_window = window.clone();
    let ask_root = ctx.project_root.clone();
    let ask_session_id = session_id.clone();
    let mut ask = move |request: kanzei_core::AskRequest| -> AskFuture {
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
            } => (
                "question".into(),
                question.clone(),
                json!({ "kind": "question", "id": id, "question": question, "options": options, "default": default }),
            ),
        };
        let payload = with_session_id(payload, &ask_session_id);
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
    };

    // 会话连续:同项目续上内存历史；应用重启后从事件日志恢复最近一次完整消息投影。
    let persisted = conversation::recover_messages(&store, &session_id)?;
    let prior = conversation::conversation_prior(&conversation, &session_id, persisted);
    if !prior.is_empty() {
        stage("会话", format!("延续对话({} 条历史消息)", prior.len()));
    }

    // task 子代理运行时:独立只读快照;fast 角色缺席时两个档位都退回主模型。
    let subagent_rt = if subagent_enabled {
        let mut sub_harness = Harness::default();
        sub_harness
            .add(kanzei_tools::SubagentBase)
            .add(ConfigComponent);
        let sub_snapshot = sub_harness.resolve(&rctx)?;
        let fast = match config.resolve_model("fast") {
            Ok(r) => (kanzei_core::build_route(&r, &proxy).await)
                .ok()
                .map(|fr| (fr, r.model.clone(), config.service_tier_for(&r))),
            Err(_) => None,
        };
        let primary_tier = config.service_tier_for(&resolved);
        let fast_tier = fast
            .as_ref()
            .map(|(_, _, tier)| tier.clone())
            .unwrap_or_else(|| primary_tier.clone());
        Some(kanzei_core::SubagentRuntime {
            snapshot: sub_snapshot,
            agent: kanzei_tools::explore_agent(),
            fast: fast
                .map(|(r, m, _)| (r, m))
                .unwrap_or_else(|| (route.clone(), resolved.model.clone())),
            primary: (route.clone(), resolved.model.clone()),
            fast_service_tier: fast_tier,
            primary_service_tier: primary_tier,
            max_tokens: config.limits.subagent_max_tokens(),
            // 纯兜底(用户定调:不设短限),防子代理失控挂死整轮。
            timeout_secs: config.limits.subagent_timeout_secs(),
            limits: config.limits.clone(),
            // R-171 批6:task 子代理登记读槽(并行查身份可见,结束自动释放)。
            coordinator: Some(Arc::clone(&coordinator)
                as Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>),
        })
    } else {
        None
    };

    let initial_parts = prompt_attachment_parts(attachments.unwrap_or_default())?;
    if !initial_parts.is_empty() {
        let image_count = initial_parts
            .iter()
            .filter(|part| matches!(part, kanzei_llm::Part::Image { .. }))
            .count();
        let document_count = initial_parts
            .iter()
            .filter(|part| matches!(part, kanzei_llm::Part::Document { .. }))
            .count();
        stage(
            "附件",
            format!(
                "已接收 {} 个附件，转换为 {} 个图片、{} 个文档输入，准备发送给 agent",
                initial_parts.len(),
                image_count,
                document_count
            ),
        );
    }

    // 开跑预检索(R-106):prompt 命中既有记忆时前置索引提示块;历史存用户原文。
    let mut run_prompt = match kanzei_tools::memory::prompt_hints(&ctx.project_root, &prompt) {
        Some(hints) => format!("{hints}\n\n{prompt}"),
        None => prompt.clone(),
    };
    // R-173 批6 · 勘察阶段:按角色表并行派发只读代理 → 汇总屏障 → 取写租约。
    // 顺序不是靠这里写对,是靠状态机——`begin_implementation` 只能从 synthesis 进,
    // 而 synthesis 的唯一入边是 `scout` 里的汇总屏障(不变量 2)。
    if let Some(pipeline) = pipeline.as_mut() {
        match subagent_rt.as_ref() {
            Some(template) => {
                stage(
                    "勘察",
                    format!(
                        "并行只读勘察中(最多 {} 个角色)…",
                        config.limits.max_tasks_per_turn()
                    ),
                );
                match pipeline
                    .scout(&client, template, &ctx, &prompt, &mut on_event)
                    .await
                {
                    Ok(brief) => {
                        stage("屏障", "勘察全部进入终态,开始申请写租约".into());
                        run_prompt = format!("{brief}\n\n{run_prompt}");
                    }
                    Err(error) => {
                        // 勘察失败不该让这一轮跑不成:按无勘察继续,但**不静默**——
                        // 阶段面板上有这一行,轨迹里有 barrier 事件可查。
                        stage("勘察", format!("勘察阶段失败,本轮无勘察简报:{error}"));
                    }
                }
            }
            None => {
                // 子代理关闭时没有勘察能力:空屏障照样走一遍,轨迹里留下
                // agent_count=0 的 barrier,而不是让阶段序列缺一截。
                let _ = pipeline.scout_skipped().await;
            }
        }
        pipeline
            .begin_implementation()
            .await
            .map_err(|e| anyhow::anyhow!("无法进入实现阶段: {e}"))?;
    }
    let run_result = run_once_with_parts(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        &run_prompt,
        &prior,
        (!initial_parts.is_empty()).then_some(initial_parts.as_slice()),
        subagent_rt.as_ref(),
        &mut on_event,
        &mut ask,
    )
    .await;
    // R-173 批6 · 集成 → 复核屏障 → 复核 → 修正。
    //
    // 复核屏障(`review` 内的第一句)会**交出写租约**,所以复核代理审的是稳定快照
    // (不变量 9)。只有复核真有发现时才会有第二段 run_once;无发现时本轮的
    // run_once 次数与引入前一样是 1 次。
    let run_result = match (pipeline.as_mut(), run_result) {
        (Some(pipeline), Ok(summary)) => {
            let merged = run_review_and_fixup(
                pipeline,
                &client,
                &route,
                &snapshot,
                &agent,
                &runner_config,
                &ctx,
                &prompt,
                subagent_rt.as_ref(),
                summary,
                &mut on_event,
                &mut ask,
                &stage,
            )
            .await;
            pipeline.finish();
            merged
        }
        (Some(pipeline), Err(error)) => {
            // 运行失败:不变量 7——任意结束路径都要交出租约并给确定终态。
            pipeline.abort("run failed");
            Err(error)
        }
        (None, result) => result,
    };
    let store = match kanzei_core::SessionStore::open(&state_path) {
        Ok(store) => Some(store),
        Err(error) => {
            report_persistence_failure(window, &session_id, "打开会话数据库", error);
            None
        }
    };
    if let Some(store) = store.as_ref() {
        match &run_result {
            Ok(summary) => {
                if let Err(error) = store.set_status(&session_id, "idle") {
                    report_persistence_failure(window, &session_id, "写入 idle 状态", error);
                }
                if let Err(error) = store.append_event(
                    &session_id,
                    "session.status_changed",
                    &json!({ "status": "idle" }),
                ) {
                    report_persistence_failure(window, &session_id, "写入完成状态事件", error);
                }
                if let Err(error) = store.append_event(
                    &session_id,
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
                    report_persistence_failure(window, &session_id, "写入完成事件", error);
                }
                // 本轮切片:summary.messages = prior + 本轮;统计与失败提炼都只看本轮,
                // 否则历史失败反复上报、工具计数累计全历史(R-099 基线失真)。
                let this_run = &summary.messages[prior.len().min(summary.messages.len())..];
                // 轮末失败提炼与机械投递(R-105):不依赖模型自觉调用 memory_note。
                let signals = kanzei_core::summarize_failures(this_run);
                if !signals.is_empty() {
                    let memory = kanzei_tools::memory::MemoryStore::project(&ctx.project_root);
                    kanzei_tools::memory::harvest_failures(&memory, &signals);
                }
                // SOP 提炼(R-124):只在本轮确实完成了一个完整条目时触发,闸门在
                // completed_entry 里用代码强制。SOP 是用户的常用模板,所以只产候选,
                // 落到 global 候选箱等用户一键采纳——agent 不能自己决定入库。
                // 根因→fact(R-105):同一次收口把根因原料投项目 inbox,由 manager
                // 提炼成 fact——SOP 判 NOOP 时根因仍有记忆价值。
                if let Some(done) = kanzei_core::completed_entry(this_run) {
                    if let Some(global) = kanzei_tools::memory::MemoryStore::global() {
                        kanzei_tools::memory::harvest_sop(&global, &done, &prompt);
                    }
                    kanzei_tools::memory::harvest_entry_fact(
                        &kanzei_tools::memory::MemoryStore::project(&ctx.project_root),
                        &done,
                        &prompt,
                        &signals,
                    );
                }
                // episode 落库(R-106):机械轨迹画像。失败不阻塞收尾。
                if let Ok(episode_id) = store.append_episode(&kanzei_core::EpisodeRecord {
                    session_id: &session_id,
                    prompt_head: &prompt,
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
                    run_id: &run_id,
                    input_id: &promoted_input_id,
                    duration_ms: run_started.elapsed().as_millis() as u64,
                    // R-106:上下文溢出压缩丢弃的轨迹段沉淀为 episode 的一部分,
                    // 让溢出路径不再无声丢弃轨迹,复盘时可通过 episodes.overflow_json 查回。
                    overflow_json: &serde_json::to_string(&summary.overflow_traces)
                        .unwrap_or_default(),
                }) {
                    // R-161:本轮开跑预检索的 recall_events 归因到该 episode,可 join 查询。
                    let _ = store.link_recall_events_to_episode(episode_id, run_epoch_ms);
                }
                let _ = store.finish_input(&promoted_input_id, true);
                // 富 episode(带工具画像/上下文账单)已写,标记防重:停止路径的
                // flush_live_run 不该再补一条信息量更少的(D-179)。
                live.lock().unwrap().flushed = true;
                if let Err(error) =
                    append_run_notification(store, &session_id, "succeeded", "任务完成", false)
                {
                    report_persistence_failure(window, &session_id, "写入完成通知", error);
                }
                // 轮末记忆整理(R-105):独立任务消化 inbox 草稿,不阻塞完成事件。
                tauri::async_runtime::spawn(memory::consolidate_memory_inbox(project_dir.clone()));
            }
            Err(error) => {
                if let Err(persistence_error) = store.set_status(&session_id, "failed") {
                    report_persistence_failure(
                        window,
                        &session_id,
                        "写入失败状态",
                        persistence_error,
                    );
                }
                if let Err(persistence_error) = store.append_event(
                    &session_id,
                    "session.status_changed",
                    &json!({ "status": "failed" }),
                ) {
                    report_persistence_failure(
                        window,
                        &session_id,
                        "写入失败状态事件",
                        persistence_error,
                    );
                }
                if let Err(persistence_error) = store.append_event(
                    &session_id,
                    "run.failed",
                    &json!({ "error": error.to_string() }),
                ) {
                    report_persistence_failure(
                        window,
                        &session_id,
                        "写入失败事件",
                        persistence_error,
                    );
                }
                // 失败轮次原先在 `let summary = run_result?;` 处提前返回,轨迹与
                // episode 一并丢失——和被停止的轮次是同一个洞(D-179)。
                flush_live_run(store, &session_id, &live, "failed");
                let _ = store.finish_input(&promoted_input_id, false);
                if let Err(persistence_error) =
                    append_run_notification(store, &session_id, "failed", error.to_string(), false)
                {
                    report_persistence_failure(
                        window,
                        &session_id,
                        "写入失败通知",
                        persistence_error,
                    );
                }
            }
        }
    }
    let summary = run_result?;

    let history_len = summary.messages.len();
    // R-076:本轮工具画像随 kz:done 带给前端,鞭挞据此判定「实质进展」——
    // 只算本轮切片,不含 prior,否则历史工具调用让每一轮都看着像有动作。
    let this_run_tools =
        kanzei_core::summarize_tools(&summary.messages[prior.len().min(summary.messages.len())..]);
    // R-169:自主推进判定后端化——轮末用 harness 状态机判定下一步,结果随
    // kz:done 带给前端执行(发下一条/NUDGE/停止);前端不再承载任何机械判定。
    let backlog = crate::auto_run::backlog_status(&project_root);
    let tools_vec: Vec<String> = this_run_tools.keys().cloned().collect();
    let auto_action_json = {
        let mut controllers = auto_runs.lock().unwrap();
        let ctrl = controllers.entry(session_id.clone()).or_default();
        let ctx = AutoRunCtx {
            backlog,
            halted: summary.halted_by_user,
            steps: summary.steps,
            tools: &tools_vec,
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
    conversation
        .lock()
        .unwrap()
        .insert(session_id.clone(), summary.messages);

    // R-021 自动压缩:历史估算超过上下文上限 70% 时,fast 模型出纪要并替换历史。
    // 估算用 len/4(与压缩预检同源的粗粒度);失败保留原历史,绝不丢上下文。
    if let Some(limit) = resolved.provider.context_limit {
        let estimate = {
            let conversations = conversation.lock().unwrap();
            let conv = conversations.get(&session_id).cloned().unwrap_or_default();
            serde_json::to_string(&conv)
                .map(|s| s.len() as u64 / 4)
                .unwrap_or(0)
        };
        if estimate > limit * 7 / 10 {
            stage(
                "压缩",
                format!(
                    "历史约 {}k token,超过 {}k 的 70%,自动压缩中…",
                    estimate / 1000,
                    limit / 1000
                ),
            );
            let transcript = {
                let conversations = conversation.lock().unwrap();
                let conv = conversations.get(&session_id).cloned().unwrap_or_default();
                render_transcript(&conv)
            };
            match fast_summarize(&ctx.cwd, &transcript).await {
                Ok(digest) => {
                    conversation.lock().unwrap().insert(
                        session_id.clone(),
                        vec![kanzei_llm::Message::user_text(format!(
                            "(系统:此前对话已自动压缩为以下纪要,基于它继续)\n{digest}"
                        ))],
                    );
                    let _ = window.emit(
                        "kz:compacted",
                        with_session_id(json!({ "summary": digest }), &session_id),
                    );
                }
                Err(e) => stage("压缩", format!("压缩失败:{e}(保留原历史)")),
            }
        }
    }

    let messages = conversation
        .lock()
        .unwrap()
        .get(&session_id)
        .cloned()
        .unwrap_or_default();
    let trace = live.lock().unwrap().trace.clone();
    if let Some(store) = store.as_ref() {
        if !trace.is_empty() {
            if let Err(error) =
                store.append_event(&session_id, "run.trace", &json!({ "events": trace }))
            {
                report_persistence_failure(window, &session_id, "写入运行轨迹", error);
            }
        }
        if let Err(error) = store.append_event(
            &session_id,
            "conversation.updated",
            &json!({ "messages": messages }),
        ) {
            report_persistence_failure(window, &session_id, "写入对话历史", error);
        }
    }
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
            &session_id,
        ),
    );
    // R-171 批5:正常路径显式写 Released 事件(审计闭环 queued→acquired→released)。
    // 失败/取消路径由协调器快照保证租约不泄漏(WriterLease Drop 回调),审计不缺持有者。
    // R-173 批5:同样经 OrchestrationEvent 单一出口,与上面两条 writer 事件同源。
    //
    // 批6:**仅非流水线路径**发这一条。流水线路径的租约归编排对象管,它在复核屏障
    // 和收尾时已经各发过一次 released——这里再发一条会在轨迹里凭空多出一次释放,
    // 回放时看起来像"释放了两次"。
    if !phase_pipeline_on {
        writer_event(
            kanzei_harness::orchestration::OrchestrationEvent::WriterReleased {
                project_root: ctx.project_root.clone(),
                run_id: run_id.clone(),
                process_id: process_id.clone(),
            },
        );
    }
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

pub(crate) async fn push_ollama_models(
    items: &mut Vec<serde_json::Value>,
    name: &str,
    base_url: &str,
) {
    let tags_url = format!("{}/api/tags", base_url.trim_end_matches("/v1"));
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    else {
        return;
    };
    let Ok(resp) = client.get(&tags_url).send().await else {
        return;
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return;
    };
    for m in v["models"].as_array().unwrap_or(&Vec::new()) {
        if let Some(n) = m["name"].as_str() {
            items.push(json!({ "id": format!("{name}:{n}"), "label": format!("{name}:{n}") }));
        }
    }
}

pub(crate) fn emit_stage(window: &Window, session_id: &str, name: &str, detail: String) {
    let _ = window.emit(
        "kz:status",
        with_session_id(json!({ "stage": name, "detail": detail }), session_id),
    );
}

#[allow(dead_code)] // 供独立启动路径复用；主运行链当前走 build_run_harness 后的统一 route。
pub(crate) async fn build_model_route(
    resolved: &kanzei_harness::config::ResolvedModel,
    proxy: &kanzei_llm::ProxyConfig,
) -> anyhow::Result<kanzei_llm::Route> {
    kanzei_core::build_route(resolved, proxy).await
}

/// R-173 批6:集成 → 复核屏障 → 复核 → 修正。
///
/// 只在阶段流水线开启(自主推进轮)时被调用。返回**合并后**的 RunSummary:
/// - 无复核发现:原样返回实现段的 summary,本轮 run_once 次数 = 1(与引入前一致);
/// - 有复核发现:跑一段修正 run_once,`prior` 接实现段的完整 `messages`,
///   所以返回的 `messages` 是「实现段 + 修正段」的连续历史,
///   `prior.len()` 之后的切片仍然正好是本轮全部内容(轮末统计口径不变)。
#[allow(clippy::too_many_arguments)] // 复核/修正要重放整条 run_once 参数链,收拢成参数对象只会多一层壳。
pub(crate) async fn run_review_and_fixup(
    pipeline: &mut crate::phase_pipeline::PhasePipeline,
    client: &kanzei_llm::LlmClient,
    route: &kanzei_llm::Route,
    snapshot: &Arc<kanzei_harness::HarnessSnapshot>,
    agent: &kanzei_harness::AgentDef,
    runner_config: &kanzei_core::RunnerConfig,
    ctx: &ToolCtx,
    prompt: &str,
    subagent_rt: Option<&kanzei_core::SubagentRuntime>,
    summary: kanzei_core::RunSummary,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
    ask: &mut (dyn FnMut(kanzei_core::AskRequest) -> AskFuture + Send),
    stage: &(dyn Fn(&str, String) + Sync),
) -> anyhow::Result<kanzei_core::RunSummary> {
    if let Err(error) = pipeline.begin_integration() {
        tracing::warn!(%error, "进入集成阶段失败,跳过复核");
        return Ok(summary);
    }
    let Some(template) = subagent_rt else {
        // 没有子代理能力:仍然走复核屏障(交出写租约),只是没有角色可派。
        if let Err(error) = pipeline.review_skipped().await {
            tracing::warn!(%error, "空复核屏障失败");
        }
        return Ok(summary);
    };
    stage("复核", "写租约已交出,并行只读复核中…".into());
    let findings = match pipeline
        .review(client, template, ctx, prompt, &summary.text, on_event)
        .await
    {
        Ok(findings) => findings,
        Err(error) => {
            stage("复核", format!("复核阶段失败,跳过修正:{error}"));
            return Ok(summary);
        }
    };
    let Some(findings) = findings else {
        stage("复核", "复核无发现,本轮收工".into());
        return Ok(summary);
    };
    stage("修正", "复核有发现,重新获取写租约执行修正…".into());
    if let Err(error) = pipeline.begin_fixup().await {
        stage("修正", format!("无法进入修正阶段:{error}"));
        return Ok(summary);
    }
    let fixup = run_once_with_parts(
        client,
        route,
        snapshot,
        agent,
        runner_config,
        ctx,
        &crate::phase_pipeline::fixup_prompt(&findings),
        // 历史接续:修正段的 prior 就是实现段跑完的完整 messages。
        &summary.messages,
        None,
        subagent_rt,
        on_event,
        ask,
    )
    .await;
    match fixup {
        Ok(mut merged) => {
            // 合并口径:token/步数是两段之和(用户看到的是"这一条消息花了多少"),
            // messages/context_report 取修正段的——它已经含实现段全历史。
            merged.usage.input += summary.usage.input;
            merged.usage.output += summary.usage.output;
            merged.usage.reasoning += summary.usage.reasoning;
            merged.usage.cache_read += summary.usage.cache_read;
            merged.usage.cache_write += summary.usage.cache_write;
            merged.steps += summary.steps;
            merged.halted_by_user |= summary.halted_by_user;
            let mut traces = summary.overflow_traces;
            traces.extend(merged.overflow_traces);
            merged.overflow_traces = traces;
            Ok(merged)
        }
        Err(error) => {
            // 修正段失败不推翻实现段的成果:实现段已经落盘的改动仍然有效,
            // 本轮按实现段的结果收尾,失败在阶段面板上可见。
            stage("修正", format!("修正段失败,按实现段结果收尾:{error}"));
            Ok(summary)
        }
    }
}

/// R-171 批5:写租约轨迹 guard——持有租约到 run_task 返回。
/// Released 事件在 run_task 正常路径显式写(见 run_task 尾部);异常/abort
/// 路径由协调器快照可见(租约已释放),审计不丢持有者身份。
pub(crate) struct WriterLeaseTrace {
    pub(crate) _lease: kanzei_harness::orchestration::WriterLease,
}

impl Drop for WriterLeaseTrace {
    fn drop(&mut self) {
        // 租约释放由 WriterLease 自身的 Drop 回调完成;此处仅确保持有至函数末尾。
    }
}

pub(crate) fn build_runner_config(
    resolved: &kanzei_harness::config::ResolvedModel,
    config: &kanzei_harness::config::KanzeiConfig,
    reasoning_override: Option<&str>,
    project_root: &std::path::Path,
) -> kanzei_core::RunnerConfig {
    kanzei_core::RunnerConfig {
        model: resolved.model.clone(),
        max_tokens: config.limits.max_tokens(),
        reasoning: resolve_reasoning_override(
            reasoning_override,
            config.models.reasoning.as_deref(),
        ),
        service_tier: config.service_tier_for(resolved),
        context_limit: resolved.provider.context_limit,
        limits: config.limits.clone(),
        // R-162 事件触发召回:工具失败瞬间注入相关记忆 Packet(验收⑤ 桌面端)。
        recall: Some(Box::new(kanzei_tools::memory::FailureRecallPolicy::new(
            project_root,
        ))),
        // R-171:桌面端主对话(writer)默认 Default,批3 接协调器后由调用方
        // 按执行策略传入 ReadParallelWriteSerial。
        execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
    }
}

pub(crate) fn new_llm_client(
    proxy: &kanzei_llm::ProxyConfig,
) -> anyhow::Result<kanzei_llm::LlmClient> {
    Ok(kanzei_llm::LlmClient::new(proxy)?)
}

pub(crate) fn auth_stage_detail(provider_name: &str, model: &str, has_auth: bool) -> String {
    format!(
        "{}:{}{}",
        provider_name,
        model,
        if has_auth {
            "(订阅登录态,可能刷新令牌)"
        } else {
            ""
        }
    )
}

pub(crate) fn resolve_model_ref(model_override: Option<String>, agent_model: &str) -> String {
    model_override
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| agent_model.to_string())
}

pub(crate) fn resolve_reasoning_override(
    override_value: Option<&str>,
    configured_value: Option<&str>,
) -> kanzei_llm::ReasoningEffort {
    override_value
        .or(configured_value)
        .map(kanzei_llm::ReasoningEffort::parse)
        .unwrap_or_default()
}

pub(crate) fn resolve_proxy(
    config: &kanzei_harness::config::KanzeiConfig,
) -> kanzei_llm::ProxyConfig {
    match config.proxy.as_deref() {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(proxy) => kanzei_llm::ProxyConfig::Explicit(proxy.to_string()),
    }
}

pub(crate) fn append_dev_guidance(
    system: &mut String,
    profile: kanzei_harness::ProfileKind,
    work_priority: &str,
) {
    if profile != kanzei_harness::ProfileKind::Dev {
        return;
    }
    system.push('\n');
    system.push('\n');
    system.push_str(kanzei_tools::frontend_inspection_guidance());
    system.push_str(&work_priority_guidance(work_priority));
}

pub(crate) fn build_run_harness() -> kanzei_harness::Harness {
    let mut harness = kanzei_harness::Harness::default();
    harness
        .add(kanzei_tools::BaseComponent)
        .add(kanzei_tools::DevProfile)
        .add(kanzei_tools::ResearchProfile)
        .add(crate::harness_ext::FrontendToolsComponent)
        .add(kanzei_harness::MarkdownComponent)
        .add(kanzei_harness::ConfigComponent);
    harness
}

pub(crate) fn work_priority_guidance(work_priority: &str) -> String {
    let (first, second) = if work_priority == "requirement-first" {
        ("requirements.md", "defects.md")
    } else {
        ("defects.md", "requirements.md")
    };
    format!("\n\nWork selection mode for this run: {work_priority}. Scan {first} from top to bottom first; only after it has no workable item scan {second}. This run's selected mode overrides the default queue order in the surrounding project context.")
}

/// R-141:只解析 profile,**不再顺手发现项目根**。
/// 根由 IPC 入口(run_prompt)解析一次后显式传进 run_task——把两件事捆在一个
/// 函数里,正是根发现能悄悄溜进线路径的原因。
pub(crate) fn resolve_profile(
    profile: Option<&str>,
    config: &kanzei_harness::config::KanzeiConfig,
) -> anyhow::Result<kanzei_harness::ProfileKind> {
    match profile.filter(|profile| !profile.is_empty()) {
        Some(profile) => profile
            .parse()
            .map_err(|error: String| anyhow::anyhow!(error)),
        None => Ok(config.default_profile()),
    }
}

pub(crate) fn normalize_work_priority(value: Option<&str>) -> &'static str {
    match value {
        Some("requirement-first") => "requirement-first",
        _ => "defect-first",
    }
}

pub(crate) fn report_config_warnings(
    window: &Window,
    session_id: &str,
    config: &kanzei_harness::config::KanzeiConfig,
    config_warnings: &[String],
) {
    for warning in config_warnings {
        emit_stage(window, session_id, "配置", warning.clone());
    }
    for warning in config.bash_permission_warnings() {
        emit_stage(window, session_id, "权限", warning);
    }
}

#[tauri::command]
pub(crate) async fn models_list(project_dir: Option<String>) -> Result<serde_json::Value, String> {
    let cwd = project_dir
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .ok_or("no working dir")?;
    let config = kanzei_harness::config::KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for role in ["primary", "fast"] {
        if let Ok(resolved) = config.resolve_model(role) {
            items.push(json!({ "id": role, "label": format!("{role} → {}:{}", resolved.provider_name, resolved.model) }));
        }
    }
    for (name, provider) in &config.providers {
        if provider.auth.as_deref() == Some("codex") {
            for model in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
                items.push(
                    json!({ "id": format!("{name}:{model}"), "label": format!("{name}:{model}") }),
                );
            }
        } else if provider.auth.as_deref() == Some("claude") {
            for model in [
                "claude-opus-5",
                "claude-sonnet-5",
                "claude-haiku-4-5-20251001",
            ] {
                items.push(
                    json!({ "id": format!("{name}:{model}"), "label": format!("{name}:{model}") }),
                );
            }
        } else if provider.protocol == "openai" || provider.protocol == "openai-responses" {
            if provider.base_url.contains("11434") {
                push_ollama_models(&mut items, name, &provider.base_url).await;
                continue;
            }
            let key = provider
                .api_key
                .clone()
                .filter(|key| !key.trim().is_empty())
                .or_else(|| {
                    provider
                        .api_key_env
                        .as_deref()
                        .and_then(|env| std::env::var(env).ok())
                });
            let url = format!("{}/models", provider.base_url.trim_end_matches('/'));
            let proxy = match config.proxy.as_deref() {
                Some("off") => kanzei_llm::ProxyConfig::Disabled,
                Some("env") | None => kanzei_llm::ProxyConfig::Env,
                Some(custom) => kanzei_llm::ProxyConfig::Explicit(custom.to_string()),
            };
            let Ok(client) = kanzei_llm::proxy::build_http_client(&proxy) else {
                continue;
            };
            let mut request = client.get(&url).timeout(std::time::Duration::from_secs(6));
            if let Some(key) = &key {
                request = request.bearer_auth(key);
            }
            if let Ok(response) = request.send().await {
                if let Ok(value) = response.json::<serde_json::Value>().await {
                    for model in value["data"].as_array().unwrap_or(&Vec::new()) {
                        if let Some(id) = model["id"].as_str() {
                            items.push(json!({ "id": format!("{name}:{id}"), "label": format!("{name}:{id}") }));
                        }
                    }
                }
            }
        } else if provider.base_url.contains("11434") {
            push_ollama_models(&mut items, name, &provider.base_url).await;
        }
    }
    Ok(json!(items))
}

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

pub(crate) async fn fast_summarize(cwd: &Path, transcript: &str) -> Result<String, String> {
    use futures::StreamExt;
    let config = kanzei_harness::config::KanzeiConfig::load(cwd).map_err(|e| e.to_string())?;
    let resolved = config.resolve_model("fast").map_err(|e| e.to_string())?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(p) => kanzei_llm::ProxyConfig::Explicit(p.to_string()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = kanzei_llm::LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let request = kanzei_llm::LlmRequest { model: resolved.model.clone(), system: vec!["把下面的人机协作对话记录总结成简洁的中文纪要:做了什么、改了哪些文件、结论、遗留问题/下一步。markdown 列表,300 字以内。".into()], messages: vec![kanzei_llm::Message::user_text(transcript)], tools: vec![], max_tokens: 2048, temperature: None, reasoning: kanzei_llm::ReasoningEffort::Off, service_tier: config.service_tier_for(&resolved) };
    let mut stream = client
        .stream(&route, &request)
        .await
        .map_err(|e| e.to_string())?;
    let mut summary = String::new();
    while let Some(event) = stream.next().await {
        if let kanzei_llm::LlmEvent::TextDelta { text, .. } = event.map_err(|e| e.to_string())? {
            summary.push_str(&text);
        }
    }
    if summary.trim().is_empty() {
        return Err("模型没有产出总结(fast 模型是否在运行?)".into());
    }
    Ok(summary)
}

pub(crate) fn render_transcript(messages: &[kanzei_llm::Message]) -> String {
    let mut out = String::new();
    'outer: for message in messages {
        for part in &message.parts {
            match part {
                kanzei_llm::Part::Text { text } => {
                    out.push_str(match message.role {
                        kanzei_llm::Role::User => "[用户] ",
                        kanzei_llm::Role::Assistant => "[助手] ",
                    });
                    out.push_str(text);
                    out.push('\n');
                }
                kanzei_llm::Part::ToolCall { name, input, .. } => {
                    out.push_str(&format!("[工具调用] {name} {input}\n"))
                }
                kanzei_llm::Part::ToolResult { content, .. } => {
                    let snippet: String = content.chars().take(1500).collect();
                    out.push_str(&format!("[工具结果] {snippet}\n"));
                }
                _ => {}
            }
            if out.len() > 100_000 {
                break 'outer;
            }
        }
    }
    out
}

#[tauri::command]
pub(crate) async fn summarize_chat(
    project_dir: String,
    transcript: String,
) -> Result<serde_json::Value, String> {
    let cwd = PathBuf::from(&project_dir);
    let summary = fast_summarize(&cwd, &transcript).await?;
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let dir = root.join(".kanzei").join("summaries");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("summary-{secs}.md"));
    std::fs::write(&path, &summary).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "summary": summary, "path": path.display().to_string() }))
}

#[tauri::command]
pub(crate) fn stop_run(
    window: Window,
    state: State<'_, AppState>,
    project_dir: Option<String>,
    process_id: Option<String>,
) {
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
        .filter(|(session_id, runtime)| {
            target_session
                .as_ref()
                .is_none_or(|target| target == *session_id)
                && runtime.running.load(Ordering::SeqCst)
        })
        .map(|(_, runtime)| runtime.clone())
        .collect();
    if runtimes.is_empty() {
        let _ = window.emit(
            "kz:error",
            with_session_id(
                json!({ "message": "目标项目当前没有可停止的运行" }),
                target_session.as_deref().unwrap_or(""),
            ),
        );
        return;
    }
    let mut cancelled = None;
    for runtime in runtimes {
        let result = target_project.clone().map(|root| {
            let session_id = target_session
                .clone()
                .unwrap_or_else(|| kanzei_core::project_session_id(&root));
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
                .and_then(|store| stop_runtime_and_finalize(&runtime, &store, &session_id))
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
            let _ = window.emit(
                "kz:error",
                with_session_id(
                    json!({ "message": format!("停止时清理排队输入失败: {error}") }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
            let _ = window.emit(
                "kz:stopped",
                with_session_id(
                    json!({ "cancelled_queue": 0 }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
        }
    }
    if let Some(root) = target_project {
        let window = window.clone();
        let session = target_session.clone().unwrap_or_default();
        tauri::async_runtime::spawn(async move {
            let killed = kanzei_tools::kill_background_processes(&root).await;
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
}

fn parse_delivery(value: Option<&str>) -> anyhow::Result<kanzei_core::Delivery> {
    match value.unwrap_or("queue") {
        "steer" => Ok(kanzei_core::Delivery::Steer),
        "queue" => Ok(kanzei_core::Delivery::Queue),
        other => Err(anyhow::anyhow!("未知输入交付模式: {other}")),
    }
}

fn admit_input(
    project_dir: &str,
    session_id: &str,
    prompt: &str,
    delivery: kanzei_core::Delivery,
) -> anyhow::Result<kanzei_core::AdmittedInput> {
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project_root))?;
    store.create_session(session_id, &project_root.display().to_string(), None)?;
    let input_id = format!(
        "input_{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let input = store.admit_input(session_id, &input_id, prompt, delivery)?;
    store.append_event(session_id, "prompt.admitted", &json!({ "input_id": input_id, "delivery": if matches!(delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }))?;
    Ok(input)
}

fn promote_next_input(
    project_dir: &str,
    session_id: &str,
) -> anyhow::Result<Option<kanzei_core::AdmittedInput>> {
    let project_root = crate::normalized_project_root(Path::new(project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project_root))?;
    let Some(input) = store.promote_next_input(session_id)? else {
        return Ok(None);
    };
    store.append_event(session_id, "prompt.promoted", &json!({ "input_id": input.input_id, "delivery": if matches!(input.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }))?;
    Ok(Some(input))
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
        with_session_id(json!({ "message": message }), session_id),
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
) -> Result<(), String> {
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
        if process.project_dir != project_root.display().to_string() {
            return Err("进程不属于当前项目".into());
        }
        process
    } else {
        ensure_default_process(&state, &project_root)
    };
    let session_id = process_session_id(&project_root, Some(&process.id));
    let profile = profile.or_else(|| process.profile.lock().unwrap().clone());
    let model = model.or_else(|| process.model.lock().unwrap().clone());
    let reasoning = process.reasoning.lock().unwrap().clone();
    let subagent_enabled = process.subagent_enabled.load(Ordering::SeqCst);
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
    let runtime_for_task = runtime.clone();
    // R-169:自主推进状态机在 AppState,spawn 前 clone 出来(闭包不能引用 State)。
    let auto_runs = state.auto_runs.clone();
    // R-171:项目级协调器与进程身份传给 writer run(写租约申请用)。
    let coordinator = state.coordinator.clone();
    let process_id_for_run = process.id.clone();
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
                project_dir.clone(),
                main_root.clone(),
                session_id.clone(),
                subagent_enabled,
                profile.clone(),
                agent.clone(),
                model.clone(),
                work_priority.clone(),
                reasoning.clone(),
                conversation.clone(),
                live_run.clone(),
                auto_runs.clone(),
                delivery,
                next_input.take(),
                coordinator.clone(),
                process_id_for_run.clone(),
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
                        json!({ "message": format!("{message}{hint}") }),
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
                            with_session_id(json!({ "message": error.to_string() }), &session_id),
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

#[cfg(test)]
mod assembly_tests {
    use kanzei_harness::{ConfigComponent, Harness, KanzeiConfig, ProfileKind, ResolveCtx};
    use kanzei_tools::{BaseComponent, DevProfile, ResearchProfile};
    use std::path::PathBuf;
    use std::sync::Arc;

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
}
