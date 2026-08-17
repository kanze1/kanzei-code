//! 运行装配域(R-253 批2,纯搬迁自 run/mod.rs)。
//!
//! 独立理由:装配是「把一轮跑起来需要的全部依赖准备好」——配置/harness/模型/
//! 鉴权/会话/typed 写入器/写租约/执行身份,一次 [`assemble_run`] 返回
//! [`RunAssembly`]。它与事件归约(events)、执行流水线(execution)、落库
//! (persistence)、协调(coordinator)各自独立成域:装配回答「需要什么」,
//! 执行回答「怎么跑」,持久化回答「跑完怎么落」,三个变更理由不再挤在同一文件
//! (照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):①取根必须在加载配置之前(R-177 内容⑧)——worktree 里的
//! kanzei.toml 是分支副本,读它会让线的行为取决于分支停在哪一代;②project_write_key
//! 与 worktree_key 必须分开取(写主根的串行、写代码的并行);⑥typed_flush_task 是
//! spawn 出来的弱引用定时任务,跨模块传递时不能被当成没人用的字段删掉;⑨stage 闭包
//! 签名保持 `&(dyn Fn(&str, String) + Sync)`,不各自改成泛型 impl Fn。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use kanzei_harness::{Component, Effect, HarnessDraft, KanzeiConfig, ResolveCtx, Rule, ToolCtx};
use serde_json::json;
use tauri::Emitter;

use crate::{
    prompt_attachment_parts, typed_events, with_session_id, LiveRun, PendingAsk, PromptAttachment,
};

/// R-253 批7b:进入运行域的调用契约三分组之一——**本轮输入**(`RoundRequest`)。
/// IPC 入口(run_prompt)解析后的「这一轮要跑什么」:提示词、附件、工作目录/主根/
/// 会话身份、投递方式、已准入输入、进程身份。
/// 生命周期:一次性——随本轮产生、随本轮消费,不跨轮复用。
pub(crate) struct RoundRequest {
    pub(crate) prompt: String,
    pub(crate) attachments: Option<Vec<PromptAttachment>>,
    pub(crate) project_dir: String,
    // R-141:项目主根由调用方(run_prompt)在 IPC 入口解析一次后显式传入,
    // 线路径内不再做根发现。worktree 线上线后 project_dir 是代码树、main_root
    // 仍是主根,两者不同——发现式取根在那时会拐进 worktree 里的 .kanzei 分支副本。
    pub(crate) main_root: PathBuf,
    pub(crate) session_id: String,
    pub(crate) delivery: kanzei_core::Delivery,
    pub(crate) promoted_input: Option<kanzei_core::AdmittedInput>,
    pub(crate) process_id: String,
}

/// R-253 批7b:调用契约三分组之二——**运行档位**(`RunMode`)。
/// 决定这一轮怎么跑:勘察复核总闸、tracker 写开关、模型/档位覆盖、自主推进与放行。
/// 生命周期:本轮级模式配置——装配期消费;轮末仍有残留用途(`phase_pipeline_enabled`
/// 决定写租约释放路径),故 run_task 先拷贝 bool 再整体移入装配。
pub(crate) struct RunMode {
    // 进程级「勘察复核」开关 = 阶段流水线总闸(2026-08-11 用户定调)。
    // 开 → 本轮强制走七阶段;关 → 一问一答。它**不**决定有没有子代理。
    pub(crate) phase_pipeline_enabled: bool,
    // 进程级「子代理」开关。关闭时本轮不构造 SubagentRuntime,工具面不含 task。
    pub(crate) subagents_enabled: bool,
    pub(crate) block_tracker_writes: bool,
    // 分支线 tracker 写入开关。主线永远不加此门禁;分支线默认关闭。
    pub(crate) profile: Option<String>,
    pub(crate) agent_name: Option<String>,
    pub(crate) model_override: Option<String>,
    pub(crate) work_priority: Option<String>,
    pub(crate) reasoning_override: Option<String>,
    pub(crate) autonomous: bool,
    pub(crate) auto_allow: bool,
}

/// R-253 批7b:调用契约三分组之三——**运行时句柄**(`RuntimeHandles`)。
/// AppState/SessionRuntime 里跨轮存活的共享句柄(asks 表、会话历史、live 画像、
/// 停止令牌槽、项目协调器……),Arc 克隆即持有,装配与轮末收尾共用同一批句柄。
/// 生命周期:会话级——跨轮存活,不随本轮结束而销毁。
pub(crate) struct RuntimeHandles {
    pub(crate) asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    pub(crate) ask_seq: Arc<AtomicU64>,
    pub(crate) collaboration_probe: crate::collaboration::CollaborationProbe,
    pub(crate) current_stage: Arc<Mutex<String>>,
    pub(crate) conversation: Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    pub(crate) live_run: Arc<Mutex<LiveRun>>,
    // R-174:本会话的单条停止注册表。塞进 SubagentRuntime.cancellations 供
    // run_subagent 挂取消 token;stop_task 命令从 SessionRuntime 拿同一实例命中。
    pub(crate) task_cancellations: Arc<kanzei_core::TaskCancellations>,
    pub(crate) auto_runs: Arc<Mutex<HashMap<String, crate::auto_run::AutoRunController>>>,
    // R-171:项目级协调器(所有 ProcessHandle 共享)。主对话 writer run
    // 在此获取写租约并持有到本轮结束;RAII 保证任何结束路径都释放。
    pub(crate) coordinator: Arc<kanzei_core::orchestration::MemoryCoordinator>,
    // D-342 协作式停止:本会话的停止令牌槽(SessionRuntime.halt)与 run 代数。
    // run 开始时换代并安装新令牌;stop 取走令牌 cancel,run 在检查点 halted 收尾。
    pub(crate) halt_slot: Arc<Mutex<Option<kanzei_core::CancellationToken>>>,
    pub(crate) run_generation: Arc<AtomicU64>,
}

/// R-202 批1:run_task 装配段的产物聚合。装配(配置/harness/模型/鉴权/会话/typed/
/// 写租约/执行身份)收敛为一次函数调用返回,run_task 主体只管三段编排
/// (装配 → 事件循环 → 轮末收尾),不再背负 300+ 行前置准备。
/// R-253 批7:装配产物按生命周期三分——`RuntimeDeps`(本轮不变的依赖:配置解析的
/// 产物)、`SessionContext`(会话事务:开库、准入、typed 写入器)、`RoundContext`
/// (单轮身份与编排:run id/timing/trace/pipeline/写租约/执行身份)。
/// 严禁做成一个 28 字段的 `RunContext`——那只是把 parameter monolith 换成
/// context monolith;对每一个参数组都要能说出它属于哪一层生命周期。
pub(crate) struct RuntimeDeps {
    pub(crate) project_root: PathBuf,
    pub(crate) config: Arc<KanzeiConfig>,
    pub(crate) profile: kanzei_harness::ProfileKind,
    pub(crate) rctx: ResolveCtx,
    pub(crate) snapshot: Arc<kanzei_harness::HarnessSnapshot>,
    pub(crate) agent: kanzei_harness::AgentDef,
    pub(crate) work_priority: &'static str,
    pub(crate) resolved: kanzei_harness::config::ResolvedModel,
    pub(crate) proxy: kanzei_llm::ProxyConfig,
    pub(crate) route: kanzei_llm::Route,
    pub(crate) client: kanzei_llm::LlmClient,
    pub(crate) runner_config: kanzei_core::RunnerConfig,
    pub(crate) ask_source: &'static str,
}

pub(crate) struct SessionContext {
    pub(crate) state_path: PathBuf,
    pub(crate) store: kanzei_core::SessionStore,
    pub(crate) promoted_input_id: String,
    pub(crate) prompt: String,
    pub(crate) initial_parts: Vec<kanzei_llm::Part>,
    pub(crate) typed_writer: Arc<Mutex<typed_events::TypedEventWriter>>,
    pub(crate) typed_flush_task: tauri::async_runtime::JoinHandle<()>,
}

pub(crate) struct RoundContext {
    pub(crate) run_id: String,
    pub(crate) run_started: std::time::Instant,
    pub(crate) run_epoch_ms: i64,
    pub(crate) orchestration_trace: Arc<crate::orchestration_trace::SessionEventObserver>,
    pub(crate) pipeline: Option<crate::phase_pipeline::PhasePipeline>,
    pub(crate) _write_lease: Option<WriterLeaseTrace>,
    pub(crate) ctx: ToolCtx,
}

/// 装配产物(内部结构,由三个生命周期分组组成)。
pub(crate) struct RunAssembly {
    pub(crate) deps: RuntimeDeps,
    pub(crate) session: SessionContext,
    pub(crate) round: RoundContext,
}

/// R-202 批1:run_task 的装配段(原 :85-399)——从 run_task 内联的 300+ 行收敛为
/// 独立函数。行为零变更:时序、阶段汇报、错误信息、状态机转移与事件顺序与内联时
/// 完全一致;所有装配产物经 [`RunAssembly`] 一次返回,run_task 解构后继续三段编排。
/// stage 闭包由调用方传入(捕获 current_stage/window/session_id,装配与轮末共用)。
/// R-253 批7b:调用参数按生命周期分组打包——`RoundRequest`(本轮输入)/`RunMode`
/// (运行档位)/`&RuntimeHandles`(会话级句柄,装配只借用:collaboration_probe 经
/// Clone 进 harness,coordinator 经 Arc::clone),加 window/stage/halt_token 三个
/// 装配独有输入,共 6 参,消 too_many。禁止再退化成 20+ 扁平参数。
pub(crate) async fn assemble_run(
    window: &tauri::Window,
    stage: &(dyn Fn(&str, String) + Sync),
    request: RoundRequest,
    mode: RunMode,
    handles: &RuntimeHandles,
    halt_token: kanzei_core::CancellationToken,
) -> anyhow::Result<RunAssembly> {
    let cwd = PathBuf::from(&request.project_dir);
    anyhow::ensure!(cwd.is_dir(), "工作目录不存在: {}", request.project_dir);

    // R-050 D1「运行时重定向主根」的落点:cwd 是代码工作树(线上线后 = worktree),
    // project_root 恒为主根——托管文档、state.db、记忆全部走它。
    // 取根必须在**加载配置之前**:R-177 内容⑧,配置是主根资产,worktree 里的
    // `.kanzei/kanzei.toml` 是被 git checkout 出来的分支副本,读它等于让线的行为
    // 取决于分支停在哪一代。
    let project_root = request.main_root;
    stage("配置", format!("加载 {}", project_root.display()));
    let (config, config_warnings) = KanzeiConfig::load_with_warnings_at_root(&project_root)?;
    let config = Arc::new(config);
    report_config_warnings(window, &request.session_id, &config, &config_warnings);
    let profile = resolve_profile(mode.profile.as_deref(), &config)?;
    let rctx = ResolveCtx {
        profile,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };

    let harness = build_run_harness(
        mode.block_tracker_writes,
        Some(handles.collaboration_probe.clone()),
    );
    let snapshot = harness.resolve(&rctx)?;
    let mut agent = snapshot.select_agent(mode.agent_name.as_deref())?.clone();
    let work_priority = normalize_work_priority(mode.work_priority.as_deref());
    append_dev_guidance(&mut agent.system, profile, work_priority, &config);
    // 裁决**一轮只算一次**:它内部有 4 次 git 调用(含 git diff --binary HEAD)。
    // 同一份快照既进 system prompt,也作为任务上下文灌给勘察/复核角色——同源同刻,
    // 主代理与 8 个角色看到的必然是同一条条目。
    let control_state = (profile == kanzei_harness::ProfileKind::Dev).then(|| {
        kanzei_tools::resolve_work_decision(
            &cwd,
            &project_root,
            crate::auto_run::work_priority_enum(work_priority),
        )
    });
    if let Some(state) = control_state.as_ref() {
        agent
            .system
            .push_str(&kanzei_tools::resolved_control_prompt_of(state.clone()));
    }
    stage(
        "装配",
        format!(
            "harness 就绪:agent {} · {} 个工具",
            agent.name,
            snapshot.materialize_tools().len()
        ),
    );

    // 界面模型下拉直选优先于 agent 定义(R-178 P2 五层链 ①②③:本轮直选 → 线持久
    // 选择 → agent 默认;④⑤ 由 config.resolve_model 承担)。桌面与 CLI 共用
    // kanzei_harness::config::resolve_model_chain,同一真源。
    let model_ref = kanzei_harness::config::resolve_model_chain(
        mode.model_override.as_deref(),
        None,
        &agent.model,
    );
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
    let client = new_llm_client(&proxy)?;
    let ctx = ToolCtx::new(cwd.clone(), project_root.clone())
        .with_work_priority(crate::auto_run::work_priority_enum(work_priority));
    // R-256:RunnerConfig 构造与 CLI 共用 kanzei_tools::run::build_runner_config(对照表 #12)。
    let runner_config = kanzei_tools::run::build_runner_config(
        &resolved,
        &config,
        mode.reasoning_override.as_deref(),
        &ctx.project_root,
        // D-281:自动轮默认 NonInteractive(避免后台 ASK 挂起弹窗);用户勾选
        // 自动放行后传 AutoAllow——权限询问直接放行并落 PermissionResolved
        // 事件,不再静默 declined(开关因此对鞭挞/自主推进轮生效)。
        if mode.autonomous || request.process_id.starts_with("p|") {
            if mode.auto_allow {
                kanzei_core::AskPolicy::AutoAllow
            } else {
                kanzei_core::AskPolicy::NonInteractive
            }
        } else {
            kanzei_core::AskPolicy::Interactive
        },
        // D-342:主对话 run 全部接停止令牌(协作式停止的接收端)。
        Some(halt_token),
    );
    let ask_source = if mode.autonomous {
        "autonomous"
    } else if request.process_id.starts_with("p|") {
        "parallel"
    } else {
        "primary"
    };
    // R-182 内容①:不再无条件强制串行写。
    //
    // R-171 在这里无条件设 ReadParallelWriteSerial,于是主对话**每一轮**的普通工具
    // 都 max in-flight = 1 —— 连三次 read 都要排队。冲突判定本来就由每个工具自己
    // 声明的 ToolConcurrency 承担(写工具一律 write_worktree(ctx),同一棵树上的两次
    // 写自然互斥;读工具 shared_worktree 之间无冲突),阶段再加一层是重复且过严。
    // 现在留 RunnerConfig 的默认值(Default),要收紧就显式设策略。

    let state_path = kanzei_core::project_state_path(&ctx.project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(
        &request.session_id,
        &ctx.project_root.display().to_string(),
        None,
    )?;
    let is_new_input = request.promoted_input.is_none();
    let promoted = if let Some(input) = request.promoted_input {
        input
    } else {
        let input_id = format!(
            "input_{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        store.admit_input(
            &request.session_id,
            &input_id,
            &request.prompt,
            request.delivery,
        )?;
        store.append_event(
            &request.session_id,
            "prompt.admitted",
            &json!({ "input_id": input_id, "delivery": if matches!(request.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
        )?;
        store
            .promote_next_input(&request.session_id)?
            .ok_or_else(|| anyhow::anyhow!("无法提升已提交的桌面端输入"))?
    };
    if is_new_input {
        store.append_event(
            &request.session_id,
            "prompt.promoted",
            &json!({ "input_id": promoted.input_id, "delivery": if matches!(promoted.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
        )?;
    }
    let prompt = promoted.prompt;
    let initial_parts = prompt_attachment_parts(request.attachments.unwrap_or_default())?;
    let mut typed_user_parts = initial_parts.clone();
    if !prompt.is_empty() {
        typed_user_parts.insert(
            0,
            kanzei_llm::Part::Text {
                text: prompt.clone(),
            },
        );
    }
    // promoted → running,并记住本轮身份与墙钟(D-173)。少了 running/completed 这段
    // 生命周期,跑完的输入永远停在 promoted,以后任何一次停止都会把它追认成 cancelled。
    let promoted_input_id = promoted.input_id.clone();
    store.start_input(&promoted_input_id)?;
    let run_id = format!(
        "run_{}",
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );
    // R-241 shadow 双写：先从最新 legacy snapshot 幂等 seed，并闭合上次强杀留下的
    // open draft/tool；再提交本轮 user fact。失败只留在 writer report，不改变旧主路径。
    let typed_writer = Arc::new(Mutex::new(typed_events::TypedEventWriter::new(
        &state_path,
        &request.session_id,
        &run_id,
    )));
    if let Err(error) = typed_events::prepare_session(&store, &request.session_id) {
        typed_writer.lock().unwrap().record_error(error);
    }
    typed_writer.lock().unwrap().user_message(
        &promoted_input_id,
        kanzei_llm::Message {
            role: kanzei_llm::Role::User,
            parts: typed_user_parts,
        },
    );
    // 单独的弱引用定时 flush：provider 静默时仍满足 750ms 持久化上界；run 正常
    // 终态后观察到 terminal 退出，run 被强制 abort 后所有强引用释放，Weak 失效退出。
    let typed_flush_writer = Arc::downgrade(&typed_writer);
    let typed_flush_task = tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            interval.tick().await;
            let Some(writer) = typed_flush_writer.upgrade() else {
                break;
            };
            let mut writer = writer.lock().unwrap();
            writer.flush_due();
            if writer.is_terminal() {
                break;
            }
        }
    });
    let run_started = std::time::Instant::now();
    // 本轮开始墙钟毫秒:R-161 回填 recall_events 的 episode_id 用(开跑预检索
    // 先于 episode 落库,只能靠时间窗归因到本轮,与 CLI 同一口径)。
    let run_epoch_ms = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default();
    store.set_status(&request.session_id, "running")?;
    super::append_run_notification(&store, &request.session_id, "running", "任务已开始", false)?;
    store.append_event(
        &request.session_id,
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
        &request.session_id,
    )?);
    // 阶段流水线的装配闸门 = 进程级「勘察复核」开关(2026-08-11 用户定调)。
    // 开着 → 每个任务都走七阶段(手动对话也走);关着 → 不构造编排对象,与引入前
    // 逐字节相同。闸门与构造都在 phase_pipeline::start_if_enabled 里,那里可以脱离
    // Tauri Window 直接测(见 phase_pipeline_tests.rs 的两条闸门测试)。
    //
    // 这里以前读的是 auto_runs[session].enabled(自主推进/鞭挞)。换掉之后自主推进
    // **不再**自带流水线——它只管「轮末要不要自动发下一条」。
    let pipeline = crate::phase_pipeline::start_if_enabled(
        mode.phase_pipeline_enabled,
        &config,
        &proxy,
        Arc::clone(&handles.coordinator) as Arc<dyn ProjectExecutionCoordinator>,
        Arc::clone(&orchestration_trace) as Arc<dyn kanzei_harness::orchestration::PhaseObserver>,
        ctx.project_root.clone(),
        // R-182 内容①:流水线路径的写租约同样按代码树仲裁。
        ctx.cwd.clone(),
        &run_id,
        &request.process_id,
        stage,
    )
    .await
    // 把本轮冻结的裁决快照灌给勘察/复核角色。角色表的 brief 是写死的通用描述
    // (「本次任务会写到哪里」),没有「本次任务」的指代物它就只能回答本仓库的
    // 写入面——D-368 那轮 write_surface_scout 返回 store/processes.rs 即此。
    .map(|pipeline| {
        pipeline.with_task_context(
            control_state
                .as_ref()
                .and_then(|state| state.as_ref().ok())
                .and_then(crate::phase_pipeline::render_task_context),
        )
    });
    // 写租约的取得时机是两条路的**唯一实质差异**,判定抽在 phase_pipeline 里
    // 以便直接测(见 `acquire_plain_lease_if_needed` 的文档与它的定向测试)。
    // 不带独立 worktree 的并行线与主线共用同一棵代码树,必须先等写入槽。
    // 这一段等待发生在真正发出模型请求之前,不能把它投影成“等待模型响应”。
    if pipeline.is_none() {
        stage(
            "排队",
            "等待当前代码树写入槽；同一工作树上的线路将按顺序执行…".into(),
        );
    }
    let plain_lease = crate::phase_pipeline::acquire_plain_lease_if_needed(
        pipeline.is_some(),
        handles.coordinator.as_ref(),
        orchestration_trace.as_ref(),
        &ctx.project_root,
        // R-182 内容①:仲裁范围 = 本轮代码树。线绑了 worktree 就在自己那棵树上
        // 仲裁写权,两条线互不排队——这是「同一项目 N 条线能同时跑」的落点。
        &ctx.cwd,
        &run_id,
        &request.process_id,
        &request.session_id,
    )
    .await
    .map_err(|e| anyhow::anyhow!("无法获取写租约: {e}"))?;
    // 持有到 run_task 返回(Release 事件在尾部显式写);异常/abort 路径由
    // WriterLeaseTrace::drop 补写 Released,acquired/released 始终成对(D-303)。
    // 流水线路径的租约由编排对象持有。
    let _write_lease = plain_lease.map(|lease| {
        WriterLeaseTrace::new(
            lease,
            Arc::clone(&orchestration_trace),
            ctx.project_root.clone(),
            run_id.clone(),
            request.process_id.to_string(),
        )
    });
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
        request.process_id.to_string(),
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
            &request.session_id,
        ),
    );

    Ok(RunAssembly {
        deps: RuntimeDeps {
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
        },
        session: SessionContext {
            state_path,
            store,
            promoted_input_id,
            prompt,
            initial_parts,
            typed_writer,
            typed_flush_task,
        },
        round: RoundContext {
            run_id,
            run_started,
            run_epoch_ms,
            orchestration_trace,
            pipeline,
            _write_lease,
            ctx,
        },
    })
}

pub(crate) struct WriterLeaseTrace {
    pub(crate) _lease: kanzei_harness::orchestration::WriterLease,
    observer: Arc<crate::orchestration_trace::SessionEventObserver>,
    project_root: std::path::PathBuf,
    run_id: String,
    process_id: String,
    released: std::sync::atomic::AtomicBool,
}

impl WriterLeaseTrace {
    pub(crate) fn new(
        lease: kanzei_harness::orchestration::WriterLease,
        observer: Arc<crate::orchestration_trace::SessionEventObserver>,
        project_root: std::path::PathBuf,
        run_id: String,
        process_id: String,
    ) -> Self {
        Self {
            _lease: lease,
            observer,
            project_root,
            run_id,
            process_id,
            released: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// 正常路径在写 Released 事件后调用,标记已释放,Drop 不再补写。
    pub(crate) fn mark_released(&self) {
        self.released
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for WriterLeaseTrace {
    fn drop(&mut self) {
        if self.released.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        // 异常/abort/停止路径:租约已由 WriterLease Drop 回调释放,这里补写审计事件,
        // 让 acquired/released 在会话事件流里成对。落库失败只记日志,不阻断收尾。
        use kanzei_harness::orchestration::PhaseObserver;
        self.observer.observe(
            &kanzei_harness::orchestration::OrchestrationEvent::WriterReleased {
                project_root: self.project_root.clone(),
                run_id: self.run_id.clone(),
                process_id: self.process_id.clone(),
            },
        );
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
    config: &kanzei_harness::config::KanzeiConfig,
) {
    if profile != kanzei_harness::ProfileKind::Dev {
        return;
    }
    system.push('\n');
    system.push('\n');
    system.push_str(kanzei_tools::frontend_inspection_guidance());
    system.push_str(&work_priority_guidance(work_priority));
    system.push_str(&cadence_guidance(&config.cadence));
    system.push_str(
        "\n\nAuthority boundary: you are the primary agent. Own file edits, diff review, commits, merges, and release/package actions. Any `task` subagent is read-only reconnaissance and must never write/edit, run bash, change git state, merge, or publish. Collaboration commit discipline: stage ONLY the explicit files you changed; never use `git add .` or another directory-wide stage. Immediately before every commit, call `collaboration_status`, re-run `git status`, and inspect the staged diff/hash so another line's unfinished work cannot be swept into your commit.",
    );
}

pub(crate) fn build_run_harness(
    block_tracker_writes: bool,
    collaboration_probe: Option<crate::collaboration::CollaborationProbe>,
) -> kanzei_harness::Harness {
    // R-256 批4:与 CLI 共用 kanzei_tools::run::build_harness(对照表 #5 公共部分单点);
    // FrontendTools 在 Markdown 前(middle),TrackerWritePolicy/Collaboration 在
    // Config 后(tail),顺序与原来逐字节一致。
    kanzei_tools::run::build_harness(
        |harness| {
            harness.add(crate::harness_ext::FrontendToolsComponent);
            // R-221 B1:桌面端也注册 readonly 档位；组件只在 ProfileKind::Readonly 生效。
            harness.add(kanzei_tools::ReadonlyProfile);
        },
        |harness| {
            harness.add(TrackerWritePolicyComponent {
                block: block_tracker_writes,
            });
            if let Some(probe) = collaboration_probe.as_ref() {
                harness.add(crate::collaboration::CollaborationComponent {
                    probe: probe.clone(),
                });
            }
        },
    )
}

/// R-177 F11:分支线默认只读主根 tracker。规则放在 ConfigComponent 之后,
/// 因而用户的通用 kanzei.toml allow 不能意外打开这条线级显式开关。
struct TrackerWritePolicyComponent {
    block: bool,
}

impl Component for TrackerWritePolicyComponent {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        if !self.block {
            return Ok(());
        }
        for action in [
            "req", "defect", "idea", "decision", "source", "finding", "work",
        ] {
            draft.permissions.push_denial_note(
                Rule {
                    action: action.into(),
                    resource: "write:*".into(),
                    effect: Effect::Deny,
                },
                "当前分支线未开启 tracker 写入；读取仍可用。请在该线设置中显式开启后再修改唯一主根文档。",
            );
        }
        Ok(())
    }
}

pub(crate) fn work_priority_guidance(work_priority: &str) -> String {
    format!(
        "\n\nWork selection mode input for this run: {work_priority}. The engine resolves this \
         mode together with WIP, dependencies and blockers into the structured \
         resolved-control-state below; that decision is authoritative."
    )
}

/// D-245 验收①/③通路:把 kanzei.toml `[cadence]` 的生效节奏注入 system prompt。
/// R-170 剥离前端渲染后配置就成了死资产(设置页照写、无任何消费方)——这里的注入
/// 让文件里写的值**真的决定行为**。只注入**与 §1.4 默认不同的档位**:全部默认时
/// 输出空串,不污染既有的默认节奏语义(conventions §1.4 仍是默认真源)。
/// 语义口径与 §1.4 逐条对应,由引擎按配置直接声明,不靠模型自己猜。
pub(crate) fn cadence_guidance(cadence: &kanzei_harness::config::Cadence) -> String {
    use kanzei_harness::config::{
        CommitCadence, FullTestCadence, PushCadence, TargetedTestCadence,
    };
    let mut parts: Vec<String> = Vec::new();
    match cadence.full_test {
        FullTestCadence::EveryCommit => {
            parts.push("full test suite runs before EVERY commit".into())
        }
        FullTestCadence::EveryNBatches => parts.push(format!(
            "full test suite runs every {} batches",
            cadence.full_test_batches.unwrap_or(1)
        )),
        FullTestCadence::ReleaseOnly => parts.push(
            "full test suite runs only before release (verify.ps1), not during normal dev".into(),
        ),
        FullTestCadence::EntryClose => {}
    }
    if cadence.targeted_test == TargetedTestCadence::Off {
        parts.push(
            "targeted tests are OFF: pick verification scope yourself, matching the change surface"
                .into(),
        );
    }
    match cadence.commit {
        CommitCadence::PerEntry => {
            parts.push("commit granularity: one commit per whole entry".into())
        }
        CommitCadence::PerBatch => {}
    }
    match cadence.push {
        PushCadence::PerCommit => parts.push("push after every commit".into()),
        PushCadence::Periodic => {
            parts.push("push on a periodic schedule, not after every entry".into())
        }
        PushCadence::PerEntry => {}
    }
    if cadence.verify_every_n > 0 {
        parts.push(format!(
            "auto-verify: a read-only acceptance check round runs every {} closed entries",
            cadence.verify_every_n
        ));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("\n\nVerification/commit cadence (from kanzei.toml [cadence], overrides section 1.4 defaults): {}", parts.join("; "))
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
    window: &tauri::Window,
    session_id: &str,
    config: &kanzei_harness::config::KanzeiConfig,
    config_warnings: &[String],
) {
    for warning in config_warnings {
        super::emit_stage(window, session_id, "配置", warning.clone());
    }
    for warning in config.bash_permission_warnings() {
        super::emit_stage(window, session_id, "权限", warning);
    }
}

#[cfg(test)]
mod tests {
    use super::{append_dev_guidance, build_run_harness, cadence_guidance};
    use kanzei_harness::ProfileKind;

    #[test]
    fn 桌面装配线注册_readonly_档位并保留只读权限() {
        let root = std::path::PathBuf::from("C:/kanzei-r221-desktop");
        let ctx = kanzei_harness::ResolveCtx {
            profile: ProfileKind::Readonly,
            cwd: root.clone(),
            project_root: root,
            config: std::sync::Arc::new(kanzei_harness::KanzeiConfig::default()),
        };
        let snapshot = build_run_harness(false, None).resolve(&ctx).unwrap();
        let agent = snapshot.select_agent(Some("readonly")).unwrap();
        assert_eq!(agent.name, "readonly");
        assert_eq!(
            snapshot.evaluate("read", "*"),
            kanzei_harness::Effect::Allow
        );
        assert_eq!(snapshot.evaluate("bash", "*"), kanzei_harness::Effect::Deny);
        assert_eq!(
            snapshot.evaluate("git", "status"),
            kanzei_harness::Effect::Allow
        );
    }

    #[test]
    fn 开发提示词强制逐文件暂存并在提交前刷新协作状态() {
        let config = kanzei_harness::config::KanzeiConfig::default();
        let mut system = String::new();
        append_dev_guidance(&mut system, ProfileKind::Dev, "defect-first", &config);
        assert!(system.contains("stage ONLY the explicit files you changed"));
        assert!(system.contains("never use `git add .`"));
        assert!(system.contains("Immediately before every commit"));
        assert!(system.contains("call `collaboration_status`"));

        let mut research = String::new();
        append_dev_guidance(
            &mut research,
            ProfileKind::Research,
            "defect-first",
            &config,
        );
        assert!(research.is_empty(), "提交纪律只属于开发档位");
    }

    // D-245 验收①通路:cadence_guidance 只注入与 §1.4 默认不同的档位;全默认时
    // 空串(不污染既有语义);显式配置时文本里出现对应节奏。
    #[test]
    fn cadence指引_全默认空串_显式配置注入() {
        use kanzei_harness::config::{
            Cadence, CommitCadence, FullTestCadence, PushCadence, TargetedTestCadence,
        };
        // 全默认 → 空串,不污染既有 system prompt。
        assert_eq!(cadence_guidance(&Cadence::default()), "");
        // 显式全档位 → 五条节奏都注入。
        let custom = Cadence {
            full_test: FullTestCadence::EveryNBatches,
            full_test_batches: Some(3),
            targeted_test: TargetedTestCadence::Off,
            commit: CommitCadence::PerEntry,
            push: PushCadence::PerCommit,
            verify_every_n: 0,
        };
        let text = cadence_guidance(&custom);
        assert!(text.contains("every 3 batches"), "{text}");
        assert!(text.contains("targeted tests are OFF"), "{text}");
        assert!(text.contains("one commit per whole entry"), "{text}");
        assert!(text.contains("push after every commit"), "{text}");
        assert!(text.contains("kanzei.toml [cadence]"), "{text}");
        // 注入点:append_dev_guidance 确实把指引拼进 system prompt。
        let config = kanzei_harness::config::KanzeiConfig {
            cadence: custom,
            ..Default::default()
        };
        let mut system = String::new();
        append_dev_guidance(&mut system, ProfileKind::Dev, "defect-first", &config);
        assert!(system.contains("every 3 batches"), "{system}");
        // Research 档位不注入(验证节奏只属于开发档位)。
        let mut research = String::new();
        append_dev_guidance(
            &mut research,
            ProfileKind::Research,
            "defect-first",
            &config,
        );
        assert!(research.is_empty());
    }
}
