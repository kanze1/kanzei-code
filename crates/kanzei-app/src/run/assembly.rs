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
use kanzei_harness::{KanzeiConfig, ResolveCtx, ToolCtx};
use serde_json::json;
use tauri::Emitter;

use crate::state::MutexPoisonExt;
use crate::{
    prompt_attachment_parts, typed_events, with_session_id, LiveRun, PendingAsk, PromptAttachment,
};

mod components;
use components::{DiscussionBoundary, TrackerWritePolicyComponent};

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
    pub(crate) execution_owner: Arc<kanzei_core::store::session_execution::SessionExecutionGuard>,
    pub(crate) delivery: kanzei_core::Delivery,
    pub(crate) promoted_input: Option<kanzei_core::AdmittedInput>,
    pub(crate) process_id: String,
    pub(crate) work_item_id: Option<String>,
}

/// R-253 批7b:调用契约三分组之二——**运行档位**(`RunMode`)。
/// 决定这一轮怎么跑:子代理准入、tracker 写开关、模型/档位覆盖、自主推进与放行。
/// 每轮读取的执行模式与协作倾向。
pub(crate) struct RunMode {
    pub(crate) execution_batch: bool,
    // 模型自主选择任务分解；引擎执行档位与并发上限。
    pub(crate) subagent_mode: kanzei_harness::SubagentMode,
    pub(crate) block_tracker_writes: bool,
    // 分支线 tracker 写入开关。主线永远不加此门禁;分支线默认关闭。
    pub(crate) profile: Option<String>,
    pub(crate) research_topic: Option<String>,
    pub(crate) agent_name: Option<String>,
    pub(crate) model_override: Option<String>,
    pub(crate) work_priority: Option<String>,
    pub(crate) reasoning_override: Option<String>,
    pub(crate) autonomous: bool,
    pub(crate) auto_allow: bool,
}

impl RunMode {
    pub(crate) fn ask_policy(&self) -> kanzei_core::AskPolicy {
        if !self.autonomous {
            kanzei_core::AskPolicy::Interactive
        } else if self.auto_allow {
            kanzei_core::AskPolicy::AutoAllow
        } else {
            kanzei_core::AskPolicy::NonInteractive
        }
    }
    /// Product mode is independent of whether this input was automatically submitted.
    /// A paired conversation with a goal must never acquire the project queue gates.
    pub(crate) fn uses_project_workflow(&self) -> bool {
        self.profile
            .as_deref()
            .is_none_or(|profile| profile == "dev")
            && self
                .agent_name
                .as_deref()
                .is_none_or(|agent| agent == "dev")
    }
}

/// R-253 批7b:调用契约三分组之三——**运行时句柄**(`RuntimeHandles`)。
/// AppState/SessionRuntime 里跨轮存活的共享句柄(asks 表、会话历史、live 画像、
/// 停止令牌槽、项目协调器……),Arc 克隆即持有,装配与轮末收尾共用同一批句柄。
/// 生命周期:会话级——跨轮存活,不随本轮结束而销毁。
pub(crate) struct RuntimeHandles {
    pub(crate) lifecycle: Arc<Mutex<()>>,
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
/// (单轮身份与编排:run id/timing/trace/写租约/执行身份)。
/// 严禁做成一个 28 字段的 `RunContext`——那只是把 parameter monolith 换成
/// context monolith;对每一个参数组都要能说出它属于哪一层生命周期。
pub(crate) struct RuntimeDeps {
    pub(crate) project_root: PathBuf,
    pub(crate) research_topic: Option<String>,
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
    pub(crate) prior: Vec<kanzei_llm::Message>,
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
    pub(crate) work_item_id: Option<String>,
    pub(crate) run_started: std::time::Instant,
    pub(crate) run_epoch_ms: i64,
    pub(crate) orchestration_trace: Arc<crate::orchestration_trace::SessionEventObserver>,
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
    let general = crate::general_chat::is_general_root(&request.main_root);
    let cwd = if general {
        let path =
            kanzei_harness::general_conversation_workspace(&request.main_root, &request.session_id);
        std::fs::create_dir_all(&path)?;
        path
    } else {
        PathBuf::from(&request.project_dir)
    };
    anyhow::ensure!(cwd.is_dir(), "工作目录不存在: {}", request.project_dir);

    // R-050 D1「运行时重定向主根」的落点:cwd 是代码工作树(线上线后 = worktree),
    // project_root 恒为主根——托管文档、state.db、记忆全部走它。
    // 取根必须在**加载配置之前**:R-177 内容⑧,配置是主根资产,worktree 里的
    // `.kanzei/kanzei.toml` 是被 git checkout 出来的分支副本,读它等于让线的行为
    // 取决于分支停在哪一代。
    let project_root = request.main_root.clone();
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

    let work_priority = normalize_work_priority(mode.work_priority.as_deref());
    let mut harness = build_run_harness(
        mode.block_tracker_writes,
        Some(handles.collaboration_probe.clone()),
    );
    let project_workflow = !general && mode.uses_project_workflow();
    if project_workflow {
        harness.add(kanzei_tools::work::WorkControlContext(
            crate::auto_run::work_priority_enum(work_priority),
        ));
    }
    if let Some(topic) = &mode.research_topic {
        harness.add(kanzei_tools::research_workflow::ResearchWorkflowContext(
            topic.clone(),
        ));
    }
    let snapshot = harness.resolve(&rctx)?;
    let mut agent = snapshot.select_agent(mode.agent_name.as_deref())?.clone();
    crate::skills::append_explicit_instructions(
        &mut agent.system,
        request
            .promoted_input
            .as_ref()
            .map(|input| input.prompt.as_str())
            .unwrap_or(&request.prompt),
    )?;
    if project_workflow {
        append_dev_guidance(&mut agent.system, profile, work_priority, &config);
    }
    if !mode.subagent_mode.enabled() {
        agent.system.push('\n');
        agent.system.push_str(mode.subagent_mode.guidance());
    }
    if let Some(topic) = mode.research_topic.as_deref() {
        agent.system.push_str(&format!("\n\n当前研究课题: {topic}。本会话的研究工件位于 .kanzei/research/{topic}/。来源、发现、计划、实验和报告均使用该 topic；其他课题仅在用户明确要求比较时读取，不改变当前课题归属。"));
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
    let mut ctx = ToolCtx::new(cwd.clone(), project_root.clone())
        .with_session_id(request.session_id.clone())
        .with_work_priority(crate::auto_run::work_priority_enum(work_priority));
    ctx.project_workflow = project_workflow;
    // R-256:RunnerConfig 构造与 CLI 共用 kanzei_tools::run::build_runner_config(对照表 #12)。
    let mut runner_config = kanzei_tools::run::build_runner_config(
        &resolved,
        &config,
        mode.reasoning_override.as_deref(),
        &ctx.project_root,
        // D-281:自动轮默认 NonInteractive(避免后台 ASK 挂起弹窗);用户勾选
        // 自动放行后传 AutoAllow——权限询问直接放行并落 PermissionResolved
        // 事件,不再静默 declined(开关因此对鞭挞/自主推进轮生效)。
        mode.ask_policy(),
        // D-342:主对话 run 全部接停止令牌(协作式停止的接收端)。
        Some(halt_token.clone()),
    );
    runner_config.digest_model =
        Some(kanzei_tools::run::build_digest_model(&config, &proxy, &resolved, &route).await);
    runner_config.digest_model.as_mut().unwrap().archive_root = Some(rctx.project_root.clone());
    // R-322:门禁强度按 agent 取默认值。build_runner_config 是 CLI/桌面共用的
    // 构造器,它给的是保守默认(Autonomous = 引入前行为);桌面端知道当前 agent,
    // 在这里落到真实档位。轮末判定用的强度取自同一个函数(coordinator.rs),
    // 两处必须同源——否则会出现「运行时按结伴跑、轮末按自主判」的错位。
    let runner_config = kanzei_core::RunnerConfig {
        hosted_tools: Vec::new(),
        intensity: crate::auto_run::intensity_for_agent(&agent.name),
        ..runner_config
    };
    let ask_source = if mode.autonomous {
        "autonomous"
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

    let session_id = request.session_id.clone();
    let mut deps = RuntimeDeps {
        project_root,
        research_topic: mode.research_topic.clone(),
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
    };
    let (session, round) =
        prepare_session(stage, request, &mode, handles, &halt_token, &mut deps, ctx).await?;
    let _ = window.emit(
        "kz:meta",
        with_session_id(
            run_meta_payload(
                deps.profile,
                &deps.agent.name,
                &deps.resolved,
                &deps.runner_config,
            ),
            &session_id,
        ),
    );

    Ok(RunAssembly {
        deps,
        session,
        round,
    })
}

/// Own the actual claimed input through every fallible startup operation.
/// This concrete preparation path is shared by the Window wrapper and real DB tests.
#[allow(clippy::too_many_arguments)]
async fn prepare_session(
    stage: &(dyn Fn(&str, String) + Sync),
    request: RoundRequest,
    mode: &RunMode,
    handles: &RuntimeHandles,
    halt_token: &kanzei_core::CancellationToken,
    deps: &mut RuntimeDeps,
    ctx: ToolCtx,
) -> anyhow::Result<(SessionContext, RoundContext)> {
    let profile = deps.profile;
    let agent = &mut deps.agent;
    let general = crate::general_chat::is_general_root(&ctx.project_root);
    let state_path = kanzei_core::project_state_path(&ctx.project_root);
    let mut store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(
        &request.session_id,
        &ctx.project_root.display().to_string(),
        None,
    )?;
    let promoted = if let Some(input) = request.promoted_input {
        input
    } else {
        let input_id = format!(
            "input_{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        if let Some(item_id) = request.work_item_id.as_deref() {
            store.admit_work_input(&request.session_id, &input_id, &request.prompt, item_id)?;
        } else if mode.execution_batch || mode.autonomous {
            store.admit_batch_input(
                &request.session_id,
                &input_id,
                &request.prompt,
                request.delivery,
            )?;
        } else {
            store.admit_input(
                &request.session_id,
                &input_id,
                &request.prompt,
                request.delivery,
            )?;
        }
        store
            .promote_next_input(&request.session_id)?
            .ok_or_else(|| anyhow::anyhow!("无法提升已提交的桌面端输入"))?
    };
    let promoted_input_id = promoted.input_id.clone();
    let mut startup_writer = None;
    let prepared = async {
        let work_item_id = store.input_work_item(&request.session_id, &promoted.input_id)?;
        anyhow::ensure!(
            work_item_id.is_none() || ctx.project_workflow,
            "领取需求请切换到自主推进；结伴开发按当前对话执行"
        );
        anyhow::ensure!(
            work_item_id.is_none()
                || (profile == kanzei_harness::ProfileKind::Dev && !mode.block_tracker_writes),
            "当前对话不能领取需求，请返回可写的开发主对话开始"
        );
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
        anyhow::ensure!(
            store.start_input(&promoted_input_id)?,
            "桌面输入已变化，未开始重复执行"
        );
        let completion_goal = handles
            .auto_runs
            .lock_or_recover()
            .get(&request.session_id)
            .and_then(|controller| controller.goal.clone());
        // 完成声明契约只给开发档(UX-008):只读讨论与研究档没有 work 工具、轮末也不消费
        // handoff,注入只会让模型把范围/目标/证据当散文讲给用户。判据见 context_prompt_for。
        if ctx.project_workflow || completion_goal.is_some() {
            if let Some(contract) = kanzei_harness::handoff::context_prompt_for(
                profile,
                &promoted_input_id,
                completion_goal.as_deref(),
            ) {
                agent.system.push_str(&contract);
            }
        }
        // Capture once before any write/claim by this run. Later working
        // tree observations are not evidence of what existed at this boundary.
        let (run_id, _baseline_context) = super::baseline::prepare(
            if general { &ctx.project_root } else { &ctx.cwd },
            &mut store,
            &request.session_id,
            &mut agent.system,
        )
        .await?;
        // R-241 shadow 双写：先从最新 legacy snapshot 幂等 seed，并闭合上次强杀留下的
        // open draft/tool；再提交本轮 user fact。恢复必须持有同数据库/session 执行权。
        request
            .execution_owner
            .prepare(&store, &request.session_id, Some(&promoted_input_id))?;
        let typed_writer = Arc::new(Mutex::new(request.execution_owner.writer(&run_id)));
        startup_writer = Some(Arc::clone(&typed_writer));
        // Snapshot history before admitting this turn's user fact. Reading it later
        // would feed the current input to the model twice, including async callbacks.
        let persisted = if crate::projection_gate::read_path_uses_projection("runner_prior") {
            crate::conversation::project_latest_segment(&store, &request.session_id)
                .map_err(anyhow::Error::msg)?
        } else {
            crate::conversation::recover_messages(&store, &request.session_id)?
        };
        let prior = crate::conversation::conversation_prior(
            &handles.conversation,
            &request.session_id,
            persisted,
        );
        let user_committed = typed_writer.lock_or_recover().user_message(
            &promoted_input_id,
            kanzei_llm::Message {
                role: kanzei_llm::Role::User,
                parts: typed_user_parts,
            },
        );
        anyhow::ensure!(
            user_committed,
            "用户输入未持久化: {}",
            typed_writer
                .lock_or_recover()
                .errors()
                .last()
                .map(String::as_str)
                .unwrap_or("用户事实提交被拒绝")
        );
        let run_started = std::time::Instant::now();
        // 本轮开始墙钟毫秒:R-161 回填 recall_events 的 episode_id 用(开跑预检索
        // 先于 episode 落库,只能靠时间窗归因到本轮,与 CLI 同一口径)。
        let run_epoch_ms = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        store.set_status(&request.session_id, "running")?;
        super::append_run_notification(
            &store,
            &request.session_id,
            "running",
            "任务已开始",
            false,
        )?;
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
        let plain_lease = if profile == kanzei_harness::ProfileKind::Readonly {
            None
        } else {
            use kanzei_harness::orchestration::{
                CoordinationObserver, OrchestrationEvent, WriterLeaseRequest,
            };
            stage("排队", "等待当前代码树写入槽…".into());
            orchestration_trace.observe(&OrchestrationEvent::WriterQueued {
                project_root: ctx.project_root.clone(),
                run_id: run_id.clone(),
                process_id: request.process_id.to_string(),
                reason: format!("session {} writer run", request.session_id),
            });
            let lease = handles
                .coordinator
                .acquire_writer_lease(WriterLeaseRequest {
                    write_scope: ctx.cwd.clone(),
                    run_id: run_id.clone(),
                    process_id: request.process_id.to_string(),
                    reason: format!("session {} writer run", request.session_id),
                })
                .await
                .map_err(|e| anyhow::anyhow!("无法获取写租约: {e}"))?;
            orchestration_trace.observe(&OrchestrationEvent::WriterAcquired {
                project_root: ctx.project_root.clone(),
                run_id: run_id.clone(),
                process_id: request.process_id.to_string(),
            });
            Some(lease)
        };
        // 持有到 run_task 返回(Release 事件在尾部显式写);异常/abort 路径由
        // WriterLeaseTrace::drop 补写 Released,acquired/released 始终成对(D-303)。
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
        // 主根文档写入与代码树工具并发分别仲裁。
        //
        // R-141 拆开这两把键,服务的是 R-050 D1「运行时重定向主根」:worktree 线
        // 上线后,同一项目的 N 棵树以 cwd=worktree、project_root=主根 运行,于是——
        //
        // ① `project_write_key` = **规范化主根**,N 棵树必须**相同**。
        //    主根 `.kanzei` 的 tracker/记忆是所有线唯一的共享写点,键一旦随树分裂,
        //    跨进程单写仲裁就被绕过(两条线同时重写同一个 docstore = lost update)。
        //    这里取 normalized_project_root:它比 project_root 多一次 canonicalize,
        //    保证不同路径写法落进同一个仲裁桶,且与 run_prompt 算给会话 id/进程归属
        //    的那个身份键逐字节相同。它只 canonicalize 显式选中的主根,不向祖先
        //    发现项目;线路径不做根发现这条不变式仍然成立。
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
        ctx.execution_coordinator = Some(kanzei_harness::orchestration::ExecutionCoordinator(
            handles.coordinator.clone(),
        ));
        ctx = ctx.with_identity(
            worktree_key,
            project_write_key,
            run_id.clone(),
            request.process_id.to_string(),
        );
        Ok::<_, anyhow::Error>((
            prior,
            prompt,
            initial_parts,
            RoundContext {
                run_id,
                work_item_id,
                run_started,
                run_epoch_ms,
                orchestration_trace,

                _write_lease,
                ctx,
            },
        ))
    }
    .await;
    let (prior, prompt, initial_parts, round) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            return Err(finish_startup_failure(
                &store,
                &request.session_id,
                &promoted_input_id,
                startup_writer.as_ref(),
                &handles.lifecycle,
                halt_token,
                error,
            ))
        }
    };
    let typed_writer = startup_writer.expect("successful startup initialized its writer");
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
    Ok((
        SessionContext {
            prior,
            state_path,
            store,
            promoted_input_id,
            prompt,
            initial_parts,
            typed_writer,
            typed_flush_task,
        },
        round,
    ))
}

fn finish_startup_failure(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    input_id: &str,
    writer: Option<&Arc<Mutex<typed_events::TypedEventWriter>>>,
    lifecycle: &Mutex<()>,
    halt_token: &kanzei_core::CancellationToken,
    error: anyhow::Error,
) -> anyhow::Error {
    let stopped;
    let outcome = {
        let _lifecycle = lifecycle.lock_or_recover();
        stopped = halt_token.is_cancelled();
        if let Some(writer) = writer {
            super::persistence::commit_outcome(
                writer,
                input_id,
                if stopped {
                    typed_events::TerminalFact::Stopped
                } else {
                    typed_events::TerminalFact::Failed(error.to_string())
                },
                &json!({"stage":"assembly", "halted_by_user":stopped, "error":error.to_string()}),
            )
        } else {
            // No current typed turn exists yet. Never close a historical turn or
            // invent successful baseline provenance just to finalize this input.
            (|| -> anyhow::Result<()> {
                if stopped && store.input_status(input_id)?.as_deref() == Some("cancelled") {
                    return Ok(());
                }
                anyhow::ensure!(
                    store.finish_input(input_id, false)?,
                    "启动输入状态已变化，未提交失败结果"
                );
                Ok(())
            })()
        }
    };
    if let Err(outcome_error) = outcome {
        return super::persistence::uncommitted_outcome(
            error.context(format!("启动收尾未提交: {outcome_error}")),
        );
    }
    let status = if stopped { "stopped" } else { "failed" };
    let summary = if stopped {
        "任务已停止".to_string()
    } else {
        error.to_string()
    };
    if let Err(notification_error) =
        super::append_run_notification(store, session_id, status, &summary, false)
    {
        tracing::warn!(%notification_error, "启动结果通知写入失败");
    }
    if stopped {
        super::persistence::stopped_outcome(error)
    } else {
        error
    }
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
        use kanzei_harness::orchestration::CoordinationObserver;
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
    // 通用执行与节奏在 tools profile 中单源注入，桌面只补自身能力边界。
    let _ = (work_priority, config);
    system.push_str("Desktop: ui inspection tools inspect the running Kanzei UI, not an arbitrary project's browser or mobile app. Before committing shared work, use `collaboration_status` to check ownership.");
}

/// kz:meta 载荷:本轮**实际**使用的模型、思考档与 Fast mode(UI-0926 #3)。
///
/// 状态栏据此显示「上一轮实际使用」;reasoning/codexFastMode 直接取自已构造好的
/// RunnerConfig(即真正发出去的请求参数),不再只报模型——此前开跑后也看不到思考档,
/// 与输入框上方「下一轮将使用」不一致时前端会据此重取 model_effective。
pub(crate) fn run_meta_payload(
    profile: kanzei_harness::ProfileKind,
    agent_name: &str,
    resolved: &kanzei_harness::config::ResolvedModel,
    runner_config: &kanzei_core::RunnerConfig,
) -> serde_json::Value {
    json!({
        "profile": format!("{profile:?}").to_lowercase(),
        "agent": agent_name,
        "model": format!("{}:{}", resolved.provider_name, resolved.model),
        "contextLimit": resolved.provider.context_limit,
        "reasoning": runner_config.reasoning.as_str(),
        "codexFastMode": runner_config.service_tier.is_some(),
    })
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
            harness.add(DiscussionBoundary);
            harness.add(TrackerWritePolicyComponent {
                block: block_tracker_writes,
            });
            if let Some(probe) = collaboration_probe.as_ref() {
                harness.add(crate::collaboration::CollaborationComponent {
                    probe: probe.clone(),
                });
            }
            harness.add(crate::general_chat::GeneralChatBoundary);
        },
    )
}

/// R-177 F11:分支线默认只读主根 tracker。规则放在 ConfigComponent 之后,
/// 因而用户的通用 kanzei.toml allow 不能意外打开这条线级显式开关。
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
mod startup_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Fixture {
        root: PathBuf,
        path: PathBuf,
        store: kanzei_core::SessionStore,
        runtime: Arc<crate::SessionRuntime>,
        handles: RuntimeHandles,
        deps: RuntimeDeps,
        mode: RunMode,
        owner: Option<Arc<kanzei_core::store::session_execution::SessionExecutionGuard>>,
        halt: kanzei_core::CancellationToken,
        requests: Arc<AtomicUsize>,
        server: tokio::task::JoinHandle<()>,
    }

    impl Fixture {
        async fn new(profile: kanzei_harness::ProfileKind) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "kz-c6-startup-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let path = kanzei_core::project_state_path(&root);
            let store = kanzei_core::SessionStore::open(&path).unwrap();
            store
                .create_session("ses", root.to_str().unwrap(), None)
                .unwrap();
            let owner =
                Arc::new(kanzei_core::store::session_execution::try_acquire(&path, "ses").unwrap());
            let runtime = Arc::new(crate::SessionRuntime::default());
            let halt = kanzei_core::CancellationToken::new();
            *runtime.halt.lock_or_recover() = Some(halt.clone());
            runtime.running.store(true, Ordering::SeqCst);
            let handles = RuntimeHandles {
                lifecycle: runtime.lifecycle.clone(),
                asks: runtime.asks.clone(),
                ask_seq: Arc::new(AtomicU64::new(1)),
                collaboration_probe: crate::collaboration::CollaborationProbe::new(
                    Arc::new(Mutex::new(HashMap::new())),
                    Arc::new(Mutex::new(HashMap::new())),
                    root.clone(),
                    "fixture".into(),
                ),
                current_stage: runtime.stage.clone(),
                conversation: runtime.conversation.clone(),
                live_run: runtime.live.clone(),
                task_cancellations: runtime.task_cancellations.clone(),
                auto_runs: Arc::new(Mutex::new(HashMap::new())),
                coordinator: Arc::new(kanzei_core::orchestration::MemoryCoordinator::new()),
                halt_slot: runtime.halt.clone(),
                run_generation: runtime.run_generation.clone(),
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let requests = Arc::new(AtomicUsize::new(0));
            let observed = requests.clone();
            let server = tokio::spawn(async move {
                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 4096];
                    let header_end = loop {
                        let n = socket.read(&mut buffer).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buffer[..n]);
                        if let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            break p + 4;
                        }
                    };
                    let length = String::from_utf8_lossy(&bytes[..header_end])
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    while bytes.len() < header_end + length {
                        let n = socket.read(&mut buffer).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    let payload: serde_json::Value =
                        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                    assert_eq!(payload["model"], "mock");
                    observed.fetch_add(1, Ordering::SeqCst);
                    let body = format!(
                        "data: {}\n\ndata: [DONE]\n\n",
                        json!({"choices":[{"index":0,"delta":{"content":"done"},"finish_reason":"stop"}]})
                    );
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
                    socket.write_all(body.as_bytes()).await.unwrap();
                }
            });
            // No config loader or credential resolver is used by this fixture.
            let mut config = KanzeiConfig::default();
            config.fill_defaults();
            let resolved = config.resolve_model("primary").unwrap();
            let config = Arc::new(config);
            let rctx = ResolveCtx {
                profile,
                cwd: root.clone(),
                project_root: root.clone(),
                config: config.clone(),
            };
            let snapshot = kanzei_harness::Harness::default().resolve(&rctx).unwrap();
            let agent = serde_json::from_value(json!({
                "name":"fixture", "profile":"dev", "mode":"primary", "steps":1, "system":"test"
            }))
            .unwrap();
            let runner_config = kanzei_core::RunnerConfig {
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
                halt: Some(halt.clone()),
            };
            let deps = RuntimeDeps {
                project_root: root.clone(),
                research_topic: None,
                config,
                profile,
                rctx,
                snapshot,
                agent,
                work_priority: "defect-first",
                resolved,
                proxy: kanzei_llm::ProxyConfig::Disabled,
                route: kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), None),
                client: kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap(),
                runner_config,
                ask_source: "primary",
            };
            let mode = RunMode {
                execution_batch: false,

                subagent_mode: kanzei_harness::SubagentMode::Off,
                block_tracker_writes: false,
                profile: Some(format!("{profile:?}").to_lowercase()),
                research_topic: None,
                agent_name: None,
                model_override: None,
                work_priority: None,
                reasoning_override: None,
                autonomous: false,
                auto_allow: false,
            };
            Self {
                root,
                path,
                store,
                runtime,
                handles,
                deps,
                mode,
                owner: Some(owner),
                halt,
                requests,
                server,
            }
        }

        fn request(&self, promoted_input: Option<kanzei_core::AdmittedInput>) -> RoundRequest {
            RoundRequest {
                prompt: "new prompt".into(),
                attachments: None,
                project_dir: self.root.display().to_string(),
                main_root: self.root.clone(),
                session_id: "ses".into(),
                execution_owner: self.owner.as_ref().unwrap().clone(),
                delivery: kanzei_core::Delivery::Queue,
                promoted_input,
                process_id: "fixture".into(),
                work_item_id: None,
            }
        }

        fn inject(&self, sql: &str) {
            let connection = rusqlite::Connection::open(&self.path).unwrap();
            connection.execute_batch(sql).unwrap();
        }

        async fn prepare(
            &mut self,
            request: RoundRequest,
        ) -> anyhow::Result<(SessionContext, RoundContext)> {
            let ctx =
                ToolCtx::new(self.root.clone(), self.root.clone()).with_session_id("ses".into());
            prepare_session(
                &|_, _| {},
                request,
                &self.mode,
                &self.handles,
                &self.halt,
                &mut self.deps,
                ctx,
            )
            .await
        }

        async fn execute_if_prepared(
            &self,
            prepared: anyhow::Result<(SessionContext, RoundContext)>,
        ) -> anyhow::Result<kanzei_core::RunSummary> {
            // This is deliberately executed even when a negative production
            // control wrongly admits an input whose user fact was rejected.
            let (session, round) = prepared?;
            let writer = session.typed_writer.clone();
            let mut sink = move |event| {
                let mut writer = writer.lock_or_recover();
                match event {
                    kanzei_core::RunEvent::TurnStart {
                        step, max_steps, ..
                    } => writer.turn_started(step, max_steps),
                    kanzei_core::RunEvent::Text(text) => writer.push_text(&text),
                    kanzei_core::RunEvent::AssistantMessageCommitted {
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
                    kanzei_core::RunEvent::ToolResultsCommitted {
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
                                    .unwrap_or_else(|| "tools rejected".into()),
                            );
                        }
                    }
                    _ => {}
                }
            };
            let mut ask = |_| -> kanzei_core::AskFuture {
                Box::pin(async {
                    kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny)
                })
            };
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                kanzei_core::run_once_with_parts(
                    &self.deps.client,
                    &self.deps.route,
                    &self.deps.snapshot,
                    &self.deps.agent,
                    &self.deps.runner_config,
                    &round.ctx,
                    &session.prompt,
                    None,
                    &session.prior,
                    Some(&session.initial_parts),
                    None,
                    None,
                    &mut sink,
                    &mut ask,
                ),
            )
            .await
            .unwrap();
            session.typed_flush_task.abort();
            let summary = result?;
            super::super::persistence::commit_outcome(
                &session.typed_writer,
                &session.promoted_input_id,
                typed_events::TerminalFact::Completed,
                &json!({"text":summary.text}),
            )
            .unwrap();
            super::super::append_run_notification(
                &session.store,
                "ses",
                "succeeded",
                "任务完成",
                false,
            )
            .unwrap();
            Ok(summary)
        }

        fn input_id(&self) -> String {
            self.store
                .latest_event("ses", "prompt.promoted")
                .unwrap()
                .unwrap()
                .payload["input_id"]
                .as_str()
                .unwrap()
                .into()
        }

        fn count(&self, kind: &str) -> usize {
            self.store
                .list_events_by_type("ses", 0, kind)
                .unwrap()
                .len()
        }

        fn notification_statuses(&self) -> Vec<String> {
            self.store
                .replay_notifications("ses", 0, 100)
                .unwrap()
                .into_iter()
                .map(|n| n.status)
                .collect()
        }

        fn assert_failed(&self, error: &anyhow::Error) {
            assert!(
                !super::super::persistence::is_uncommitted_outcome(error),
                "{error:?}"
            );
            assert_eq!(
                self.requests.load(Ordering::SeqCst),
                0,
                "startup rejection reached the real provider"
            );
            assert_eq!(
                self.store
                    .input_status(&self.input_id())
                    .unwrap()
                    .as_deref(),
                Some("failed")
            );
            assert_eq!(
                self.store.get_session("ses").unwrap().unwrap().status,
                "failed"
            );
            assert_eq!(self.count("session.turn_failed"), 1);
            assert_eq!(self.count("run.failed"), 1);
            assert_eq!(self.count("session.turn_completed"), 0);
            assert_eq!(
                self.notification_statuses().last().map(String::as_str),
                Some("failed")
            );
        }

        async fn assert_owner_released(&mut self) {
            drop(self.owner.take());
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    match kanzei_core::store::session_execution::try_acquire(&self.path, "ses") {
                        Ok(owner) => break drop(owner),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            // An aborted weak-flush task may still be dropping
                            // its final temporary strong reference.
                            tokio::task::yield_now().await;
                        }
                        Err(error) => panic!("owner release failed: {error}"),
                    }
                }
            })
            .await
            .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
            self.runtime.running.store(false, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn startup_internal_input_failure_closes_all_durable_state() {
        for rejected in ["notification", "status_event"] {
            let mut fixture = Fixture::new(kanzei_harness::ProfileKind::Readonly).await;
            fixture.inject(if rejected == "notification" {
                "CREATE TRIGGER reject_start BEFORE INSERT ON agent_notifications WHEN json_extract(NEW.payload_json,'$.status')='running' BEGIN SELECT RAISE(ABORT,'injected running notification'); END;"
            } else {
                "CREATE TRIGGER reject_start BEFORE INSERT ON session_events WHEN NEW.event_type='session.status_changed' AND json_extract(NEW.payload_json,'$.status')='running' BEGIN SELECT RAISE(ABORT,'injected running status event'); END;"
            });
            let request = fixture.request(None);
            let prepared = fixture.prepare(request).await;
            let error = fixture.execute_if_prepared(prepared).await.err().unwrap();
            assert!(error.to_string().contains("injected running"));
            fixture.assert_failed(&error);
            fixture.assert_owner_released().await;
        }
    }

    #[tokio::test]
    async fn startup_saved_input_failure_retains_next_pending_input() {
        let mut fixture = Fixture::new(kanzei_harness::ProfileKind::Readonly).await;
        fixture
            .store
            .admit_input("ses", "saved", "saved prompt", kanzei_core::Delivery::Queue)
            .unwrap();
        let promoted = fixture.store.promote_next_input("ses").unwrap().unwrap();
        fixture
            .store
            .admit_input("ses", "next", "next prompt", kanzei_core::Delivery::Queue)
            .unwrap();
        fixture.inject("CREATE TRIGGER reject_start BEFORE INSERT ON agent_notifications WHEN json_extract(NEW.payload_json,'$.status')='running' BEGIN SELECT RAISE(ABORT,'injected running notification'); END;");
        let request = fixture.request(Some(promoted));
        let prepared = fixture.prepare(request).await;
        let error = fixture.execute_if_prepared(prepared).await.err().unwrap();
        // Invoke the same direct caller fallback; it must not overwrite the
        // result transaction or touch the next pending input.
        assert!(!crate::commands::run::finish_failed_promoted_input(
            &fixture.store,
            "saved",
            &error
        )
        .unwrap());
        fixture.assert_failed(&error);
        assert_eq!(fixture.input_id(), "saved");
        assert_eq!(
            fixture.store.input_status("next").unwrap().as_deref(),
            Some("pending")
        );
        fixture.assert_owner_released().await;
    }

    #[tokio::test]
    async fn startup_user_rejection_never_reaches_real_provider() {
        let mut fixture = Fixture::new(kanzei_harness::ProfileKind::Readonly).await;
        fixture.inject("CREATE TRIGGER reject_user BEFORE INSERT ON session_events WHEN NEW.event_type='session.user_message_committed' BEGIN SELECT RAISE(ABORT,'injected user admission'); END;");
        let request = fixture.request(None);
        let prepared = fixture.prepare(request).await;
        let result = fixture.execute_if_prepared(prepared).await;
        assert_eq!(
            fixture.requests.load(Ordering::SeqCst),
            0,
            "rejected user admission must not reach the real provider"
        );
        let error = result.err().expect("rejected admission must fail startup");
        assert!(error.to_string().contains("injected user admission"));
        fixture.assert_failed(&error);
        assert_eq!(fixture.count("session.user_message_committed"), 0);
        fixture.assert_owner_released().await;
    }

    #[tokio::test]
    async fn startup_failed_outcome_rejection_rolls_back_and_protects_fallback() {
        for rejection in ["terminal", "input", "status", "event"] {
            let mut fixture = Fixture::new(kanzei_harness::ProfileKind::Readonly).await;
            fixture.inject("CREATE TRIGGER reject_start BEFORE INSERT ON agent_notifications WHEN json_extract(NEW.payload_json,'$.status')='running' BEGIN SELECT RAISE(ABORT,'injected startup'); END;");
            fixture.inject(match rejection {
                "terminal" => "CREATE TRIGGER reject_outcome BEFORE INSERT ON session_events WHEN NEW.event_type='session.turn_failed' BEGIN SELECT RAISE(ABORT,'injected failed terminal'); END;",
                "input" => "CREATE TRIGGER reject_outcome BEFORE UPDATE OF status ON session_inputs WHEN NEW.status='failed' BEGIN SELECT RAISE(ABORT,'injected failed input'); END;",
                "status" => "CREATE TRIGGER reject_outcome BEFORE UPDATE OF status ON sessions WHEN NEW.status='failed' BEGIN SELECT RAISE(ABORT,'injected failed status'); END;",
                _ => "CREATE TRIGGER reject_outcome BEFORE INSERT ON session_events WHEN NEW.event_type='run.failed' BEGIN SELECT RAISE(ABORT,'injected failed event'); END;",
            });
            let request = fixture.request(None);
            let prepared = fixture.prepare(request).await;
            let error = fixture.execute_if_prepared(prepared).await.err().unwrap();
            assert!(
                super::super::persistence::is_uncommitted_outcome(&error),
                "{error:?}"
            );
            let input_id = fixture.input_id();
            assert!(!crate::commands::run::finish_failed_promoted_input(
                &fixture.store,
                &input_id,
                &error
            )
            .unwrap());
            assert_eq!(
                fixture.store.input_status(&input_id).unwrap().as_deref(),
                Some("running")
            );
            assert_eq!(
                fixture.store.get_session("ses").unwrap().unwrap().status,
                "running"
            );
            assert_eq!(fixture.count("session.turn_failed"), 0);
            assert_eq!(fixture.count("run.failed"), 0);
            assert_eq!(fixture.count("session.turn_stopped"), 0);
            assert!(fixture.notification_statuses().is_empty());
            assert_eq!(fixture.requests.load(Ordering::SeqCst), 0);
            fixture.assert_owner_released().await;
        }
    }

    #[tokio::test]
    async fn startup_pre_writer_failures_only_finalize_current_input() {
        for baseline in [false, true] {
            let mut fixture = Fixture::new(kanzei_harness::ProfileKind::Readonly).await;
            fixture
                .store
                .admit_input(
                    "ses",
                    "old",
                    "old unfinished turn",
                    kanzei_core::Delivery::Queue,
                )
                .unwrap();
            fixture.store.promote_next_input("ses").unwrap().unwrap();
            assert!(fixture.store.start_input("old").unwrap());
            fixture.store.set_status("ses", "running").unwrap();
            let mut historical = fixture.owner.as_ref().unwrap().writer("historical");
            assert!(historical
                .user_message("old", kanzei_llm::Message::user_text("old unfinished turn")));
            historical.turn_started(1, 1);
            drop(historical);
            let mut request = fixture.request(None);
            if baseline {
                fixture.inject("CREATE TRIGGER reject_baseline BEFORE INSERT ON session_events WHEN NEW.event_type='run.baseline' BEGIN SELECT RAISE(ABORT,'injected baseline'); END;");
            } else {
                request.attachments = Some(vec![PromptAttachment {
                    file_name: "empty.png".into(),
                    media_type: "image/png".into(),
                    data: String::new(),
                }]);
            }
            let prepared = fixture.prepare(request).await;
            let error = fixture.execute_if_prepared(prepared).await.err().unwrap();
            assert!(error.to_string().contains(if baseline {
                "injected baseline"
            } else {
                "附件数据为空"
            }));
            assert_eq!(fixture.requests.load(Ordering::SeqCst), 0);
            assert_eq!(
                fixture
                    .store
                    .input_status(&fixture.input_id())
                    .unwrap()
                    .as_deref(),
                Some("failed")
            );
            assert_eq!(
                fixture.store.get_session("ses").unwrap().unwrap().status,
                "running",
                "a pre-writer failure must not reset a historical run's state"
            );
            assert_eq!(
                fixture.store.input_status("old").unwrap().as_deref(),
                Some("running")
            );
            assert_eq!(
                fixture.count("run.failed"),
                0,
                "there is no current run identity to finalize"
            );
            assert_eq!(
                fixture.count("run.baseline"),
                0,
                "failed capture must not invent baseline provenance"
            );
            assert_eq!(
                fixture.count("session.turn_failed"),
                0,
                "startup does not own the historical turn"
            );
            assert_eq!(fixture.count("session.turn_stopped"), 0);
            assert_eq!(fixture.store.list_session_facts("ses").unwrap().len(), 2);
            assert_eq!(fixture.notification_statuses(), vec!["failed"]);
            fixture.assert_owner_released().await;
        }
    }

    #[tokio::test]
    async fn startup_stop_during_actual_lease_wait_commits_stopped() {
        use kanzei_harness::orchestration::WriterLeaseRequest;
        let mut fixture = Fixture::new(kanzei_harness::ProfileKind::Dev).await;
        let coordinator = fixture.handles.coordinator.clone();
        let blocker = coordinator
            .acquire_writer_lease(WriterLeaseRequest {
                write_scope: fixture.root.clone(),
                run_id: "blocker".into(),
                process_id: "blocker".into(),
                reason: "barrier".into(),
            })
            .await
            .unwrap();
        let runtime = fixture.runtime.clone();
        let root = fixture.root.clone();
        let path = fixture.path.clone();
        let stopper = async {
            let waiting = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    if let Some(run_id) = coordinator.snapshot(&root).waiting_writers.first() {
                        break run_id.clone();
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            let store = kanzei_core::SessionStore::open(&path).unwrap();
            crate::stop_runtime_and_finalize(&runtime, &store, &path, "ses").unwrap();
            runtime.running.store(false, Ordering::SeqCst);
            coordinator.cancel_waiter(&waiting);
        };
        let request = fixture.request(None);
        let (prepared, ()) = tokio::join!(fixture.prepare(request), stopper);
        let error = fixture.execute_if_prepared(prepared).await.err().unwrap();
        assert!(
            super::super::persistence::is_stopped_outcome(&error),
            "{error:?}"
        );
        assert!(error.to_string().contains("无法获取写租约"));
        assert!(fixture.halt.is_cancelled());
        assert_eq!(fixture.requests.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture
                .store
                .input_status(&fixture.input_id())
                .unwrap()
                .as_deref(),
            Some("cancelled")
        );
        assert_eq!(
            fixture.store.get_session("ses").unwrap().unwrap().status,
            "idle"
        );
        assert_eq!(fixture.count("session.turn_stopped"), 1);
        assert_eq!(fixture.count("session.turn_failed"), 0);
        assert_eq!(fixture.count("run.completed"), 1);
        assert_eq!(
            fixture
                .store
                .latest_event("ses", "run.completed")
                .unwrap()
                .unwrap()
                .payload["halted_by_user"],
            true
        );
        assert_eq!(fixture.notification_statuses(), vec!["running", "stopped"]);
        assert!(coordinator.snapshot(&root).waiting_writers.is_empty());
        drop(blocker);
        fixture.assert_owner_released().await;
    }

    #[tokio::test]
    async fn startup_normal_provider_completes_and_releases_owner() {
        let mut fixture = Fixture::new(kanzei_harness::ProfileKind::Readonly).await;
        let request = fixture.request(None);
        let prepared = fixture.prepare(request).await;
        let summary = fixture.execute_if_prepared(prepared).await.unwrap();
        assert_eq!(summary.text, "done");
        assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);
        assert_eq!(
            fixture
                .store
                .input_status(&fixture.input_id())
                .unwrap()
                .as_deref(),
            Some("completed")
        );
        assert_eq!(
            fixture.store.get_session("ses").unwrap().unwrap().status,
            "idle"
        );
        assert_eq!(fixture.count("session.user_message_committed"), 1);
        assert_eq!(fixture.count("session.assistant_message_committed"), 1);
        assert_eq!(fixture.count("session.turn_completed"), 1);
        assert_eq!(
            fixture.notification_statuses(),
            vec!["running", "succeeded"]
        );
        fixture.assert_owner_released().await;
    }
}

#[cfg(test)]
mod tests {
    use super::{append_dev_guidance, build_run_harness};
    use kanzei_harness::ProfileKind;

    #[test]
    fn 所有用户对话使用同一交互策略() {
        let mut mode = super::RunMode {
            execution_batch: false,

            subagent_mode: kanzei_harness::SubagentMode::Auto,
            block_tracker_writes: false,
            profile: Some("dev".into()),
            research_topic: None,
            agent_name: None,
            model_override: None,
            work_priority: None,
            reasoning_override: None,
            autonomous: false,
            auto_allow: false,
        };
        assert!(matches!(
            mode.ask_policy(),
            kanzei_core::AskPolicy::Interactive
        ));
        mode.autonomous = true;
        mode.agent_name = Some("dev-pair".into());
        assert!(
            !mode.uses_project_workflow(),
            "a paired goal is independent of the project queue"
        );
        mode.agent_name = Some("dev".into());
        assert!(mode.uses_project_workflow());
        assert!(matches!(
            mode.ask_policy(),
            kanzei_core::AskPolicy::NonInteractive
        ));
        mode.auto_allow = true;
        assert!(matches!(
            mode.ask_policy(),
            kanzei_core::AskPolicy::AutoAllow
        ));
    }

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
        for (action, resource) in [
            ("req", "write:add"),
            ("work", "write:next"),
            ("deliver", "*"),
            ("memory_note", "*"),
            ("git", "commit"),
        ] {
            assert_eq!(
                snapshot.evaluate(action, resource),
                kanzei_harness::Effect::Deny,
                "discussion must not mutate {action}"
            );
        }
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
        assert!(system.contains("`collaboration_status`"));
        assert!(system.contains("running Kanzei UI"));
        assert!(!system.contains("cargo test"));

        let mut research = String::new();
        append_dev_guidance(
            &mut research,
            ProfileKind::Research,
            "defect-first",
            &config,
        );
        assert!(research.is_empty(), "提交纪律只属于开发档位");
    }

    /// UI-0926 #3:kz:meta 带上本轮实际的思考档与 Fast mode,取自真正发出去的 RunnerConfig。
    #[test]
    fn run_meta_payload_reports_reasoning_and_fast_mode() {
        let mut config = kanzei_harness::KanzeiConfig::default();
        config.models.reasoning = Some("high".into());
        config.fill_defaults(); // primary=codex:gpt-5.6-luna → Fast mode 内置开启
        let resolved = config.resolve_model("primary").unwrap();
        let runner = kanzei_tools::run::build_runner_config(
            &resolved,
            &config,
            None,
            std::path::Path::new("C:/kanzei-run-meta"),
            kanzei_core::AskPolicy::Interactive,
            None,
        );
        let payload = super::run_meta_payload(ProfileKind::Dev, "dev", &resolved, &runner);
        assert_eq!(payload["model"], "codex:gpt-5.6-luna");
        assert_eq!(payload["reasoning"], "high");
        assert_eq!(payload["codexFastMode"], true);
        assert_eq!(payload["profile"], "dev");
        assert_eq!(payload["agent"], "dev");
        // 本线覆盖成 xhigh:载荷报的是覆盖后的实际档位,不是配置默认档。
        let runner = kanzei_tools::run::build_runner_config(
            &resolved,
            &config,
            Some("xhigh"),
            std::path::Path::new("C:/kanzei-run-meta"),
            kanzei_core::AskPolicy::Interactive,
            None,
        );
        let payload = super::run_meta_payload(ProfileKind::Dev, "dev", &resolved, &runner);
        assert_eq!(payload["reasoning"], "xhigh");
    }
}
