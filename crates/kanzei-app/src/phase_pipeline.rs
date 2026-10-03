//! 阶段流水线接线(R-173 批6):把 [`PhaseOrchestrator`] 接进桌面端的一轮运行。
//!
//! # 装配闸门由 RunMode::uses_phase_pipeline 统一计算
//!
//! 仅兼容显式开启的高级「勘察复核」配置。执行批次、自主推进与手动聊天默认
//! 都由模型按需调用 task；readonly 讨论不装配流水线。关闭子代理总开关时不派工。
//!
//! 非流水线运行可在子代理开启时由模型主动调用 `task`。显式流水线的必需委派由
//! 调度层保证，失败或空结果会阻止后续执行；不会把工具可用误当成已经完成协作。
//!
//! # 为什么勘察由编排对象派发,而不是让模型自己调 task
//!
//! 让模型自己派 task 也能并行(批4.5 已恢复这条路),但那样**屏障无从谈起**:
//! 模型什么时候派、派几个、派完没有,编排对象都不知道,`join_scouts` 拿不到任何
//! 可等待的终态。设计文档的「推荐勘察角色」本来就是一张**给编排器的角色表**,
//! 不是给模型的建议。所以这里按角色表直接派发,拿到的是一组确定的 future,
//! 汇总屏障才有东西可等(不变量 2)。
//!
//! 两条路走的是同一个 `run_subagent`(见 `kanzei_core::run_read_agent`),
//! 只读白名单、`ask` 恒 Deny、读槽登记与回收完全一致。
//!
//! # 一轮之内的形状
//!
//! ```text
//! baseline → scouting(N 个只读角色并行) → 汇总屏障 → synthesis
//!          → implementation(取写租约)→ [主对话那一次 run_once] → integration
//!          → 复核屏障(交出写租约)→ review(M 个只读角色并行)→ 复核汇总门
//!          → fixup(重新取租约,仅当复核有发现)→ finished
//! ```
//!
//! **主对话的 `run_once` 次数**:无复核发现时 1 次(与引入前相同);有发现时 2 次
//! (第二次是修正段,`prior` 接第一段的完整 messages,历史连续)。

use std::collections::HashSet;
use std::sync::Arc;

use kanzei_core::RunEvent;
use kanzei_core::{PhaseOrchestrator, ScoutTask, SubagentRuntime};
use kanzei_harness::orchestration::{
    BarrierKind, BarrierOutcome, PhaseObserver, ProjectExecutionCoordinator, ScoutOutcome,
    WriterLeaseRequest,
};
use kanzei_harness::ToolCtx;
use kanzei_llm::LlmClient;

/// 勘察角色表(设计文档「推荐勘察角色」)。
const SCOUT_ROLES: &[(&str, &str)] = &[
    (
        "architecture_scout",
        "crate、模块、入口与依赖方向:本次任务会碰到哪些 crate 和模块,它们的依赖方向是什么。",
    ),
    (
        "runtime_scout",
        "主代理、task、ProcessHandle、SessionRuntime 的调用链:本次任务涉及的运行时路径是怎么串起来的。",
    ),
    (
        "write_surface_scout",
        "文件、Git、tracker、memory、SQLite、后台进程的写入口:本次任务会写到哪里,有没有绕过托管入口的旁路。",
    ),
    (
        "test_scout",
        "现有测试与缺口:与本次任务相关的测试在哪、覆盖了什么、缺什么。",
    ),
    (
        "docs_scout",
        "requirements/defects/ideas/design 与代码状态的一致性:本次任务相关的文档说法与代码是否对得上。",
    ),
];

/// 复核角色表(设计文档 `review` 阶段完成门槛:契约、测试与交付质量)。
const REVIEW_ROLES: &[(&str, &str)] = &[
    (
        "contract_reviewer",
        "契约复核:改动是否符合它声称的验收/设计契约,有没有「声称完成但没有真实调用方」的死代码。",
    ),
    (
        "test_reviewer",
        "测试复核:本次改动有没有对应的自动化验证,断言是否真的能逮住它要防的回归。",
    ),
    (
        "delivery_reviewer",
        "交付质量复核:是否有半成品、占坑实现、与改动无关的顺手改动、或与既有约定冲突的地方。",
    ),
];

/// 复核代理在"没发现问题"时被要求回的哨兵串。
const NO_ISSUES: &str = "NO_ISSUES";

// Default batches run two complementary scouts concurrently, then one reviewer.
// full roster remains available, but a switch no longer launches eight generic
// reports for every follow-up message.
const BATCH_SCOUT: &[(&str, &str)] = &[
    ("batch_scout", "实现勘察：只读定位本批需求涉及的实现、调用关系和修改边界，给主代理具体文件依据。测试覆盖由另一名代理独立检查，不重复泛览整个项目。"),
    ("batch_test_scout", "验收勘察：独立检查本批需求的测试入口、边界条件与已有覆盖缺口，给出最小验证步骤。实现依赖由另一名代理检查，不修改代码、不重复做实现勘察。"),
];
const BATCH_REVIEW: &[(&str, &str)] = &[("batch_reviewer", "批次复核：只读核对本批实际 diff、验收条件与测试证据，指出具体缺陷及文件位置，核查是否误称交付；无问题时仅返回 NO_ISSUES。")];

/// UI-0926 #8:编排角色的卡片短描述——角色简介冒号(全角或半角)之前那段,
/// 如「契约复核」「crate、模块、入口与依赖方向」。没有冒号时取整段。
fn role_description(brief: &str) -> &str {
    brief
        .split([':', '\u{ff1a}'])
        .next()
        .unwrap_or(brief)
        .trim()
}

fn bounded_roster(
    phase: &'static str,
    roster: &'static [(&'static str, &'static str)],
    cap: usize,
) -> Vec<(&'static str, &'static str)> {
    let selected: Vec<_> = roster.iter().take(cap).copied().collect();
    if selected.len() < roster.len() {
        tracing::warn!(
            phase,
            roster_cap = cap,
            available_roles = roster.len(),
            dispatched_roles = selected.len(),
            omitted_roles = roster.len() - selected.len(),
            "phase pipeline roster truncated by max_tasks_per_turn"
        );
    }
    selected
}

pub(crate) struct PhasePipeline {
    orchestrator: PhaseOrchestrator,
    /// Roles are display metadata; child identities are scoped to this run.
    run_id: String,
    /// 单阶段并行角色数上限。复用既有的 `max_tasks_per_turn`——它的语义本来就是
    /// 「一轮最多并行几个子代理」,不另立一个会配歪的新键。
    roster_cap: usize,
    required: bool,
    compact: bool,
    /// `[models] scout` 解析出的路由;None = 沿用模板的 fast。
    scout_route: Option<ScoutRoute>,
    /// 本轮开跑那一刻的裁决快照,**整轮冻结**。勘察与复核读同一份,与
    /// `agent.system` 里的 `<resolved-control-state>` 同源同刻。
    ///
    /// 存在的理由:角色表的 brief 是写死的通用描述(「本次任务会写到哪里」),
    /// 没有「本次任务」的指代物,于是 scout 回答的是**本仓库**的写入面。实测
    /// D-368 那轮,write_surface_scout 返回了 kanzei-core/src/store/processes.rs
    /// ——与 memory 树锁毫无关系,但它忠实回答了那个通用问题。
    ///
    /// 必须冻结而不是各阶段现算:复核发生在实现段之后,条目此刻可能已被推进或
    /// 关闭,重算会选到**下一条**,复核代理就拿着 B 条的验收去审 A 条的改动。
    task_context: Option<String>,
    baseline_context: Option<String>,
}

/// 一个角色的产出。
struct RoleReport {
    role: &'static str,
    text: String,
    ok: bool,
}

/// **阶段流水线的唯一装配闸门**:开关关着就返回 `None`,一个编排对象都不构造。
///
/// `enabled` 来自 `RunMode::uses_phase_pipeline`；`compact` 决定使用精简批次角色表
/// 还是高级完整角色表，`required` 决定协作失败是否阻止本批继续。
///
/// 闸门与构造合在一处**只为可测**:它原先内联在 `run_task` 里,而 `run_task` 需要
/// Tauri `Window` 才能调用,于是「开关关着时不构造编排对象」这条只能靠读代码确认。
/// 顺带保证了一件事——`[models] scout` 的解析(可能发起建链请求)只在开关打开时发生,
/// 关着的那条路上一次网络往返都不多花。
#[allow(clippy::too_many_arguments)] // 协调器/观察者/身份/配置/事件口缺一不可,包成结构体只是换个地方写。
pub(crate) async fn start_if_enabled(
    enabled: bool,
    config: &kanzei_harness::KanzeiConfig,
    proxy: &kanzei_llm::ProxyConfig,
    coordinator: Arc<dyn ProjectExecutionCoordinator>,
    observer: Arc<dyn PhaseObserver>,
    project_root: std::path::PathBuf,
    write_scope: std::path::PathBuf,
    run_id: &str,
    process_id: &str,
    stage: &(dyn Fn(&str, String) + Sync),
) -> Option<PhasePipeline> {
    if !enabled {
        return None;
    }
    let scout_route = resolve_scout_route(config, proxy, stage).await;
    Some(PhasePipeline::start(
        coordinator,
        observer,
        project_root,
        write_scope,
        run_id,
        process_id,
        &config.limits,
        scout_route,
    ))
}

/// 勘察/复核用哪条路由由 `[models] scout` 决定。解析失败**不静默**——阶段面板上
/// 会有一行,然后按未配置处理(沿用 fast),而不是让整轮跑不成。
async fn resolve_scout_route(
    config: &kanzei_harness::KanzeiConfig,
    proxy: &kanzei_llm::ProxyConfig,
    stage: &(dyn Fn(&str, String) + Sync),
) -> Option<ScoutRoute> {
    let model_ref = config.models.scout.as_deref()?;
    let resolved = match config.resolve_model(model_ref) {
        Ok(resolved) => resolved,
        Err(error) => {
            stage(
                "勘察路由",
                format!("{model_ref} 解析失败,回退 fast:{error}"),
            );
            return None;
        }
    };
    match kanzei_core::build_route(&resolved, proxy).await {
        Ok(route) => {
            stage(
                "勘察路由",
                format!("{}:{}", resolved.provider_name, resolved.model),
            );
            Some(ScoutRoute {
                context_limit: resolved.provider.context_limit,
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
    }
}

impl PhasePipeline {
    #[allow(clippy::too_many_arguments)] // 同上:主根与仲裁范围是两个不同的东西。
    pub(crate) fn start(
        coordinator: Arc<dyn ProjectExecutionCoordinator>,
        observer: Arc<dyn PhaseObserver>,
        project_root: std::path::PathBuf,
        write_scope: std::path::PathBuf,
        run_id: &str,
        process_id: &str,
        limits: &kanzei_harness::config::Limits,
        scout_route: Option<ScoutRoute>,
    ) -> Self {
        let orchestrator = PhaseOrchestrator::new(
            coordinator,
            project_root,
            run_id,
            process_id,
            std::time::Duration::from_secs(limits.barrier_timeout_secs()),
        )
        .with_write_scope(write_scope)
        .with_observer(observer);
        PhasePipeline {
            orchestrator,
            run_id: run_id.to_owned(),
            roster_cap: limits.max_tasks_per_turn(),
            required: false,
            compact: false,
            scout_route,
            task_context: None,
            baseline_context: None,
        }
    }

    /// 灌入本轮冻结的任务上下文(引擎裁决的 selected 条目快照)。
    ///
    /// 走 builder 而不是加进 `start` 的参数表:`start` 已经 8 个参数并带
    /// too_many_arguments 豁免,再加一个只会逼所有测试跟着改签名。
    pub(crate) fn with_task_context(mut self, context: Option<String>) -> Self {
        self.task_context = context;
        self
    }

    pub(crate) fn with_baseline_context(mut self, context: String) -> Self {
        self.baseline_context = Some(context);
        self
    }

    pub(crate) fn with_required_delegation(mut self, compact: bool) -> Self {
        self.required = true;
        self.compact = compact;
        self
    }

    pub(crate) fn required(&self) -> bool {
        self.required
    }

    /// 勘察阶段:按角色表并行派发只读代理,过汇总屏障,返回给模型看的勘察简报。
    ///
    /// 返回的简报里**一定**包含失败/超时的点名(见 `BarrierOutcome::model_notice`)——
    /// 零结果不能静默传下去。
    pub(crate) async fn scout(
        &mut self,
        client: &LlmClient,
        template: &SubagentRuntime,
        ctx: &ToolCtx,
        task_prompt: &str,
        on_event: &mut (dyn FnMut(RunEvent) + Send),
    ) -> anyhow::Result<String> {
        self.orchestrator.enter_scouting()?;
        let roles = bounded_roster(
            "scouting",
            if self.compact {
                BATCH_SCOUT
            } else {
                SCOUT_ROLES
            },
            self.roster_cap,
        );
        // 先 clone 出来:闭包对 self 的不可变借用会与随后的 `self.dispatch_roles`
        // (&mut self)打架。
        let task_context = self.task_context.clone();
        let prompts: Vec<String> = roles
            .iter()
            .map(|(role, brief)| scout_prompt(role, brief, task_prompt, task_context.as_deref()))
            .collect();
        let (reports, outcome) = self
            .dispatch_roles(
                BarrierKind::Synthesis,
                &roles,
                &prompts,
                client,
                template,
                ctx,
                on_event,
            )
            .await?;
        let mut brief = render_brief("勘察简报", &reports, outcome.model_notice());
        brief.push_str("\n已有勘察覆盖以上职责。新增 explore/plan 委派前，明确指出简报尚未覆盖的独立问题及所需证据；不要重复同范围泛览。实现和独立验证仍可按需要派发。");
        Ok(brief)
    }

    /// 并行派发一批只读角色并过屏障,过程中把子代理的内部进度实时上抛。
    ///
    /// 进度走的是**既有事件形状**(`ToolStart` / `TaskProgress` / `ToolEnd`),
    /// 与模型自己派 task 时一模一样——UI 侧不需要第二套渲染,R-174 的子代理面板
    /// 消费的也是同一份数据。区别只在 `input` 里多带了 `phase` 与 `role` 两个字段,
    /// 面板据此分区,不必猜。
    #[allow(clippy::too_many_arguments)] // 角色表、提示、路由、事件口四样缺一不可,包成结构体只是换个地方写。
    async fn dispatch_roles(
        &mut self,
        kind: BarrierKind,
        roles: &[(&'static str, &'static str)],
        prompts: &[String],
        client: &LlmClient,
        template: &SubagentRuntime,
        ctx: &ToolCtx,
        on_event: &mut (dyn FnMut(RunEvent) + Send),
    ) -> anyhow::Result<(Vec<RoleReport>, BarrierOutcome)> {
        let phase = match kind {
            BarrierKind::Synthesis => "scouting",
            BarrierKind::Review => "review",
        };
        let runtimes: Vec<SubagentRuntime> = roles
            .iter()
            .map(|(role, _)| self.runtime_as(template, role))
            .collect();
        let ids: Vec<String> = roles
            .iter()
            .map(|(role, _)| format!("{}:{role}", self.run_id))
            .collect();
        // 派发即上报:UI 先拿到块,子代理的轮次/工具进度随后挂上去。
        for (((role, brief), prompt), id) in roles.iter().zip(prompts.iter()).zip(&ids) {
            on_event(RunEvent::ToolStart {
                id: id.clone(),
                name: "task".into(),
                summary: format!("{role} · {}", brief.chars().take(40).collect::<String>()),
                input: serde_json::json!({
                    "prompt": prompt,
                    "phase": phase,
                    "role": role,
                    // UI-0926 #8:卡片短描述,与模型派 task 时的 description 同一字段。
                    "description": role_description(brief),
                }),
            });
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RunEvent>();
        let reports = Arc::new(std::sync::Mutex::new(Vec::<RoleReport>::new()));
        let ended_roles = Arc::new(std::sync::Mutex::new(HashSet::<String>::new()));
        let tasks: Vec<(String, ScoutTask<'_>)> = roles
            .iter()
            .zip(runtimes.iter())
            .zip(prompts.iter())
            .zip(&ids)
            .map(|((((role, _), rt), prompt), id)| {
                let reports = reports.clone();
                let ended_roles = ended_roles.clone();
                let tx = tx.clone();
                let task: ScoutTask<'_> = Box::pin(async move {
                    // UI-0926 #8:终态码随 ToolEnd 上抛(空答 subagent_empty_answer、
                    // 被停 subagent_cancelled 等沿用子代理自己的码,墙钟超时补
                    // subagent_timeout),UI 按码分类,不再按文案猜。文案不变。
                    let (outcome, text, ok, code) =
                        match kanzei_core::run_read_agent(client, rt, ctx, id, prompt, tx.clone())
                            .await
                        {
                            // 顺序要紧:空结果走 ToolOutput::noop(is_error=true),
                            // 放在下面的 is_error 分支之后会被吞进 Failed。
                            output if output.code == Some("subagent_empty_answer") => {
                                (ScoutOutcome::Empty, output.content, false, output.code)
                            }
                            output if output.code == Some("subagent_timeout") => (
                                ScoutOutcome::TimedOut {
                                    after_secs: rt.timeout_secs,
                                },
                                output.content,
                                false,
                                output.code,
                            ),
                            output if output.is_error => (
                                ScoutOutcome::Failed(output.content.chars().take(200).collect()),
                                output.content,
                                false,
                                output.code,
                            ),
                            output => (ScoutOutcome::Completed, output.content, true, None),
                        };
                    let preview = text.clone();
                    let (content, content_bytes) = kanzei_core::ui_tool_content(&preview);
                    reports.lock().unwrap().push(RoleReport { role, text, ok });
                    // 角色自己的 future 一返回就发 ToolEnd。TaskCancellationGuard 也在
                    // 此刻释放,因此终态事件必须先于屏障收尾,否则 UI 会把已不可取消的
                    // 角色继续显示成 running。
                    let end = RunEvent::ToolEnd {
                        id: id.clone(),
                        name: "task".into(),
                        ok,
                        outcome: if ok { "success" } else { "failed" }.into(),
                        code: code.map(str::to_owned),
                        preview,
                        content,
                        content_bytes,
                        display: None,
                        artifact: None,
                    };
                    if tx.send(end).is_ok() {
                        ended_roles.lock().unwrap().insert(id.clone());
                    }
                    outcome
                });
                (id.clone(), task)
            })
            .collect();
        // 边等屏障边转发进度——与 drive.rs 派 task 时同一个形状。
        // 只等屏障、不等通道关闭:`tx` 在本作用域一直活着,recv 不会提前返回 None。
        let outcome = {
            // 两个屏障方法返回的是两个不同的匿名 future 类型,装箱统一。
            type Barrier<'f> = std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<
                                BarrierOutcome,
                                kanzei_harness::orchestration::PhaseError,
                            >,
                        > + Send
                        + 'f,
                >,
            >;
            let mut barrier: Barrier<'_> = match kind {
                BarrierKind::Synthesis => Box::pin(self.orchestrator.join_scouts(tasks)),
                BarrierKind::Review => Box::pin(self.orchestrator.join_reviewers(tasks)),
            };
            loop {
                tokio::select! {
                    biased;
                    Some(event) = rx.recv() => on_event(event),
                    done = &mut barrier => break done?,
                }
            }
        };
        // 屏障已过 = 所有角色都终态,通道里剩下的是最后一批进度,清干净再收尾。
        while let Ok(event) = rx.try_recv() {
            on_event(event);
        }
        let reports = std::mem::take(&mut *reports.lock().unwrap());
        let ended_roles = ended_roles.lock().unwrap();
        for ((role, _), id) in roles.iter().zip(&ids) {
            if ended_roles.contains(id) {
                continue;
            }
            let report = reports.iter().find(|r| r.role == *role);
            let ok = report.map(|r| r.ok).unwrap_or(false);
            let preview = report
                .map(|r| r.text.clone())
                .unwrap_or_else(|| "(超时,未产出结果)".into());
            let (content, content_bytes) = kanzei_core::ui_tool_content(&preview);
            on_event(RunEvent::ToolEnd {
                id: id.clone(),
                name: "task".into(),
                ok,
                outcome: if ok { "success" } else { "failed" }.into(),
                // UI-0926 #8:报告缺失 = 屏障触顶时角色仍未返回,按超时收尾。
                code: report.is_none().then(|| "subagent_timeout".to_string()),
                preview,
                content,
                content_bytes,
                display: None,
                artifact: None,
            });
        }
        if self.required
            && (reports.len() != roles.len()
                || reports.is_empty()
                || reports
                    .iter()
                    .any(|report| !report.ok || report.text.trim().is_empty()))
        {
            anyhow::bail!("协作受阻：子任务未成功返回；本批次保留现场，请重试或关闭子代理后继续");
        }
        Ok((reports, outcome))
    }

    /// 子代理被关掉时的勘察阶段:**空屏障照样走一遍**。
    ///
    /// 不是跳过——轨迹里要留下 `agent_count = 0` 的 barrier 事件,回放时能看出
    /// 「这一轮没有勘察」与「这一轮压根没有勘察阶段」的区别。
    pub(crate) async fn scout_skipped(&mut self) -> anyhow::Result<()> {
        self.orchestrator.enter_scouting()?;
        self.orchestrator.join_scouts(Vec::new()).await?;
        Ok(())
    }

    /// 子代理被关掉时的复核阶段:同样交出写租约、走空屏障。
    /// 交租约这一步不能省——它是不变量 9 的落点,与有没有复核角色无关。
    pub(crate) async fn review_skipped(&mut self) -> anyhow::Result<()> {
        self.orchestrator.enter_review()?;
        self.orchestrator.join_reviewers(Vec::new()).await?;
        Ok(())
    }

    /// 取写租约进入实现阶段。**只能在汇总屏障之后调用**——状态机会挡住其它顺序。
    pub(crate) async fn begin_implementation(&mut self) -> anyhow::Result<()> {
        self.orchestrator.enter_implementation().await?;
        Ok(())
    }

    /// 主对话那一次 run_once 返回后进入集成阶段(同一租约,不重取)。
    pub(crate) fn begin_integration(&mut self) -> anyhow::Result<()> {
        self.orchestrator.enter_integration()?;
        Ok(())
    }

    /// 复核阶段:先**交出写租约**(不变量 9,由状态机机械保证),再并行派发复核角色。
    ///
    /// 返回 `Some(findings)` 表示复核有发现、需要修正段;`None` 表示无需修正。
    pub(crate) async fn review(
        &mut self,
        client: &LlmClient,
        template: &SubagentRuntime,
        ctx: &ToolCtx,
        task_prompt: &str,
        run_summary: &str,
        on_event: &mut (dyn FnMut(RunEvent) + Send),
    ) -> anyhow::Result<Option<String>> {
        // 这一句就是复核屏障:租约在这里被交出,之后才可能进 review。
        self.orchestrator.enter_review()?;
        let roles = bounded_roster(
            "review",
            if self.compact {
                BATCH_REVIEW
            } else {
                REVIEW_ROLES
            },
            self.roster_cap,
        );
        // 用**冻结**的那份,不在这里重算裁决:复核发生在实现段之后,条目此刻可能
        // 已被推进或关闭,重算会选到下一条,复核代理就拿着 B 条的验收去审 A 条。
        let task_context = match (&self.task_context, &self.baseline_context) {
            (Some(task), Some(baseline)) => Some(format!("{task}\n{baseline}")),
            (task, baseline) => task.clone().or_else(|| baseline.clone()),
        };
        let prompts: Vec<String> = roles
            .iter()
            .map(|(role, brief)| {
                review_prompt(
                    role,
                    brief,
                    task_prompt,
                    run_summary,
                    task_context.as_deref(),
                )
            })
            .collect();
        let (reports, outcome) = self
            .dispatch_roles(
                BarrierKind::Review,
                &roles,
                &prompts,
                client,
                template,
                ctx,
                on_event,
            )
            .await?;
        Ok(findings(&reports, outcome.model_notice()))
    }

    /// 修正阶段:**重新**取写租约(与实现阶段那次是两段独立区间)。
    pub(crate) async fn begin_fixup(&mut self) -> anyhow::Result<()> {
        self.orchestrator.enter_fixup().await?;
        Ok(())
    }

    pub(crate) fn finish(&mut self) {
        if let Err(error) = self.orchestrator.finish() {
            // 收尾失败不该把用户这一轮的结果吞掉:租约由 Drop 兜底(不变量 7),
            // 这里只留日志。
            tracing::warn!(%error, "阶段流水线收尾失败");
            self.orchestrator.abort("finish failed");
        }
    }

    /// 异常收尾(运行出错/用户停止):任意阶段直达终态并交出租约。
    pub(crate) fn abort(&mut self, reason: &str) {
        self.orchestrator.abort(reason);
    }
}

/// 编排派发的只读代理用哪条路由。`None` = 沿用模板的 `fast`(与引入前一致)。
pub(crate) struct ScoutRoute {
    pub(crate) context_limit: Option<u64>,
    pub(crate) route: kanzei_llm::Route,
    pub(crate) model: String,
    pub(crate) service_tier: Option<String>,
}

impl PhasePipeline {
    /// 按角色克隆一份子代理运行时。
    ///
    /// 角色名要落到 `rt.agent.name`,因为读槽登记用的就是它——同轮并行的角色靠
    /// `run_id` 区分身份、靠 `agent_name` 显示是谁(见 `ReadPermit`)。
    ///
    /// 路由:配了 `[models] scout` 就**把 fast 和 primary 两个槽都换成它**。
    /// 这样不必给 `SubagentRuntime` 加第三个槽,也不必改 `run_subagent` 的选择逻辑——
    /// 无论它挑哪个槽,拿到的都是用户为勘察指定的那条路由。
    fn runtime_as(&self, template: &SubagentRuntime, role: &str) -> SubagentRuntime {
        // Clone the task runtime as a whole: cancellation, transcripts, model and tool
        // snapshots stay on the shared execution path when new fields are added.
        let mut runtime = template.clone();
        runtime.options.host = None;
        runtime.agent.name = role.to_string();
        let reviewing =
            role.contains("review") || REVIEW_ROLES.iter().any(|(name, _)| *name == role);
        if reviewing {
            runtime.agent.model = "primary".into();
        }
        if let Some(scout) = self.scout_route.as_ref().filter(|_| !reviewing) {
            runtime.fast = (scout.route.clone(), scout.model.clone());
            runtime.primary = runtime.fast.clone();
            runtime.fast_service_tier = scout.service_tier.clone();
            runtime.primary_service_tier = scout.service_tier.clone();
            runtime.options.fast_context_limit = scout.context_limit;
            runtime.options.primary_context_limit = scout.context_limit;
        }
        runtime
    }
}

/// 把裁决快照渲染成给角色看的「本轮条目」块。没有 selected 就没有块。
///
/// 只挑对「这次要动什么」有信息量的字段:全量字段会把角色的注意力冲散,而角色
/// 要回答的是「本任务碰哪里」,不是复述条目。
mod prompts;
use prompts::{findings, render_brief, review_prompt, scout_prompt};
pub(crate) use prompts::{fixup_prompt, render_task_context};

/// 写租约申请(非流水线路径用):手动一问一答仍走 R-171 的老形状。
///
/// `write_scope` 是**本轮代码树**(R-182 内容①):线绑了 worktree 就在自己那棵
/// 树上仲裁,两条线互不排队;主树进程传项目目录,与改前逐字节同义。
pub(crate) fn plain_writer_request(
    write_scope: &std::path::Path,
    run_id: &str,
    process_id: &str,
    session_id: &str,
) -> WriterLeaseRequest {
    WriterLeaseRequest {
        write_scope: write_scope.to_path_buf(),
        run_id: run_id.to_string(),
        process_id: process_id.to_string(),
        reason: format!("session {session_id} writer run"),
    }
}

/// 写租约的取得时机——两条路的**唯一实质差异**,也是不变量 2 在生产路径上的落点。
///
/// - `pipeline_on = true`(「勘察复核」开着):**当场不取**,返回 `Ok(None)`。租约推迟到
///   汇总屏障之后由编排对象在 `begin_implementation` 里取——勘察全部进入终态之前
///   项目里不得出现 writer。
/// - `pipeline_on = false`(「勘察复核」关着,一问一答):与 R-171 完全一致,当场取、
///   持有整轮,并发 queued/acquired 两条事件。
///
/// 抽成独立函数**只为可测**:这个判定原先内联在 `run_task` 里,而 `run_task` 需要
/// Tauri `Window` 才能调用,于是"流水线开启时不取租约"这条只能靠读代码确认。
#[allow(clippy::too_many_arguments)] // 审计根与仲裁范围必须分开传,合并回一个参数正是本批要拆掉的错误。
pub(crate) async fn acquire_plain_lease_if_needed(
    pipeline_on: bool,
    coordinator: &dyn ProjectExecutionCoordinator,
    observer: &dyn PhaseObserver,
    project_root: &std::path::Path,
    // R-182 内容①:仲裁范围是本轮代码树,与事件里的主根分开传——
    // 审计字段说的是「哪个项目」,仲裁桶说的是「哪棵树」,两者不是一回事。
    write_scope: &std::path::Path,
    run_id: &str,
    process_id: &str,
    session_id: &str,
) -> Result<Option<kanzei_harness::orchestration::WriterLease>, String> {
    use kanzei_harness::orchestration::OrchestrationEvent;
    if pipeline_on {
        return Ok(None);
    }
    observer.observe(&OrchestrationEvent::WriterQueued {
        project_root: project_root.to_path_buf(),
        run_id: run_id.to_string(),
        process_id: process_id.to_string(),
        reason: format!("session {session_id} writer run"),
    });
    let lease = coordinator
        .acquire_writer_lease(plain_writer_request(
            write_scope,
            run_id,
            process_id,
            session_id,
        ))
        .await?;
    observer.observe(&OrchestrationEvent::WriterAcquired {
        project_root: project_root.to_path_buf(),
        run_id: run_id.to_string(),
        process_id: process_id.to_string(),
    });
    Ok(Some(lease))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scout_route_replaces_context_limits_without_changing_review_runtime() {
        struct Observer;
        impl PhaseObserver for Observer {
            fn observe(&self, _: &kanzei_harness::orchestration::OrchestrationEvent) {}
        }
        let root = std::env::temp_dir();
        let config = Arc::new(kanzei_harness::config::KanzeiConfig::default());
        let snapshot = kanzei_harness::Harness::default()
            .resolve(&kanzei_harness::ResolveCtx {
                profile: kanzei_harness::ProfileKind::Dev,
                cwd: root.clone(),
                project_root: root.clone(),
                config: config.clone(),
            })
            .unwrap();
        let route = kanzei_llm::Route::openai_at("http://127.0.0.1:1/v1", Some("fixture"));
        let mut template = SubagentRuntime {
            options: Default::default(),
            roster: Vec::new(),
            snapshot,
            agent: kanzei_tools::explore_agent(),
            fast: (route.clone(), "fast".into()),
            primary: (route.clone(), "primary".into()),
            fast_service_tier: None,
            primary_service_tier: None,
            compact: None,
            max_tokens: 256,
            timeout_secs: 30,
            limits: config.limits.clone(),
            coordinator: None,
            writable: false,
            ask_router: None,
            change_log: None,
            cancellations: None,
            background: false,
            background_results: None,
            background_events: None,
            transcripts: None,
            background_notifications: None,
            transcript_sink: None,
            transcript_provider: None,
        };
        template.options.fast_context_limit = Some(64_000);
        template.options.primary_context_limit = Some(128_000);
        for context_limit in [Some(32_000), None] {
            let pipeline = PhasePipeline::start(
                Arc::new(kanzei_core::orchestration::MemoryCoordinator::new()),
                Arc::new(Observer),
                root.clone(),
                root.clone(),
                "fixture",
                "fixture",
                &config.limits,
                Some(ScoutRoute {
                    context_limit,
                    route: route.clone(),
                    model: "scout".into(),
                    service_tier: None,
                }),
            );
            let scout = pipeline.runtime_as(&template, "runtime_scout");
            assert_eq!(scout.fast.1, "scout");
            assert_eq!(scout.options.fast_context_limit, context_limit);
            assert_eq!(scout.options.primary_context_limit, context_limit);
            let review = pipeline.runtime_as(&template, "contract_reviewer");
            assert_eq!(review.primary.1, "primary");
            assert_eq!(review.options.primary_context_limit, Some(128_000));
            assert_eq!(template.options.fast_context_limit, Some(64_000));
        }
    }

    fn report(role: &'static str, text: &str, ok: bool) -> RoleReport {
        RoleReport {
            role,
            text: text.into(),
            ok,
        }
    }

    /// UI-0926 #8:卡片短描述取简介冒号前那段,全角/半角冒号都认。
    #[test]
    fn 角色短描述取冒号前一段() {
        assert_eq!(role_description("契约复核:改动是否符合契约"), "契约复核");
        assert_eq!(role_description(" 测试复核\u{ff1a}有没有测试"), "测试复核");
        assert_eq!(role_description("没有冒号的简介"), "没有冒号的简介");
        for (role, brief) in SCOUT_ROLES.iter().chain(REVIEW_ROLES) {
            let description = role_description(brief);
            assert!(
                // 60 = 前端描述截断上限,超了就会在卡片上被省略号截掉。
                !description.is_empty() && description.chars().count() <= 60,
                "{role} 的短描述应当非空且不超过卡片截断上限: {description}"
            );
        }
    }

    #[test]
    fn 角色表截断仍返回配置上限并记录可诊断边界() {
        let selected = bounded_roster("test", SCOUT_ROLES, 2);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].0, "architecture_scout");
        assert_eq!(selected[1].0, "runtime_scout");
        assert!(SCOUT_ROLES.len() > selected.len());
    }

    #[test]
    fn 角色表不超过并行上限且可被配置收窄() {
        // roster_cap 复用 max_tasks_per_turn:配成 2 就只派 2 个角色。
        let roles: Vec<&str> = SCOUT_ROLES.iter().take(2).map(|(r, _)| *r).collect();
        assert_eq!(roles, vec!["architecture_scout", "runtime_scout"]);
        assert!(
            SCOUT_ROLES.len() <= kanzei_harness::config::Limits::default().max_tasks_per_turn(),
            "默认并行上限必须容得下完整勘察角色表,否则默认配置下就有角色被悄悄砍掉"
        );
    }

    #[test]
    fn 复核全部无问题时不进修正段() {
        let reports = vec![
            report("contract_reviewer", NO_ISSUES, true),
            report("test_reviewer", "  NO_ISSUES  ", true),
            report("delivery_reviewer", "", true),
        ];
        assert!(
            findings(&reports, None).is_none(),
            "全部回 NO_ISSUES 时不应触发修正段"
        );
    }

    #[test]
    fn 复核有发现时进修正段且点名角色() {
        let reports = vec![
            report("contract_reviewer", NO_ISSUES, true),
            report("test_reviewer", "run.rs:520 新增分支没有测试", true),
        ];
        let found = findings(&reports, None).expect("有发现必须触发修正段");
        assert!(found.contains("test_reviewer"));
        assert!(found.contains("run.rs:520"));
        assert!(
            !found.contains("contract_reviewer"),
            "回了 NO_ISSUES 的角色不该出现在发现里"
        );
        // 修正段提示必须要求逐条判断,而不是无脑照改。
        let prompt = fixup_prompt(&found);
        assert!(prompt.contains("误报"), "必须允许模型判定误报: {prompt}");
        assert!(prompt.contains("释放写权之后"), "必须说明这是稳定快照复核");
    }

    /// 复核代理自己失败/超时时**不能**当成"复核通过"。
    #[test]
    fn 复核失败不得被当成通过() {
        let reports = vec![report("contract_reviewer", NO_ISSUES, true)];
        let notice = Some("(system) 复核阶段 2 个任务中 1 个未产出结果…".to_string());
        let found = findings(&reports, notice).expect("有复核代理没跑出结果时必须进修正段");
        assert!(found.contains("未产出结果"));
    }

    /// 勘察简报必须把零结果原样带给模型(反静默降级)。
    #[test]
    fn 勘察简报带上失败点名() {
        let reports = vec![report(
            "architecture_scout",
            "core/runner/drive.rs:57",
            true,
        )];
        let notice = Some("(system) 勘察阶段 2 个任务中 1 个未产出结果(失败 1,超时 0)。".into());
        let brief = render_brief("勘察简报", &reports, notice);
        assert!(brief.contains("未产出结果"), "失败必须出现在简报里");
        assert!(brief.contains("architecture_scout"));
        assert!(brief.contains("drive.rs:57"));
    }

    /// 角色提示必须带上「本次任务」的指代物。
    ///
    /// 角色表的 brief 是写死的通用描述(「本次任务会写到哪里」),没有指代物时
    /// scout 只能回答**本仓库**的写入面——D-368 那轮 write_surface_scout 返回了
    /// kanzei-core/src/store/processes.rs,与 memory 树锁毫无关系,但它忠实回答了
    /// 那个通用问题。
    #[test]
    fn 角色提示带上本轮条目与指代锚() {
        let context = "D-368 [fixing] 围栏窗口内 .kanzei/memory 并发写被误回滚\n- 进展: 批次 0/1";
        for (role, brief) in SCOUT_ROLES {
            let prompt = scout_prompt(role, brief, "继续推进", Some(context));
            assert!(prompt.contains("D-368"), "{role} 提示里必须有条目 id");
            assert!(prompt.contains("[本轮条目]"), "{role} 缺条目块");
            assert!(prompt.contains("就是「本次任务」"), "{role} 缺指代锚");
            assert!(
                prompt.contains("批次 0/1"),
                "{role} 应看到进展字段——「已经做到哪」是判断相关性的关键"
            );
        }
        for (role, brief) in REVIEW_ROLES {
            let prompt = review_prompt(role, brief, "继续推进", "改完了", Some(context));
            assert!(
                prompt.contains("D-368"),
                "{role} 复核同样要看到条目,否则只能核对自述而非验收"
            );
        }
        // 无裁决(非 dev 档或队列为空)时不拼空块,免得给角色一段空壳。
        let bare = scout_prompt("architecture_scout", "b", "任务", None);
        assert!(
            !bare.contains("[本轮条目]"),
            "无上下文时不应出现空块: {bare}"
        );
    }

    #[test]
    fn 勘察角色提示是自足的只读指令() {
        for (role, brief) in SCOUT_ROLES {
            let prompt = scout_prompt(role, brief, "修 R-173", None);
            assert!(
                prompt.contains("只有 read/glob/grep"),
                "{role} 必须声明只读"
            );
            assert!(prompt.contains("修 R-173"), "{role} 必须带上本轮任务原文");
            assert!(
                prompt.contains("不要提改动建议"),
                "{role} 不得越权给实施建议(设计文档:子代理不得直接实施建议)"
            );
        }
    }
}
