//! 阶段流水线接线(R-173 批6):把 [`PhaseOrchestrator`] 接进桌面端的一轮运行。
//!
//! # 只在自主推进轮装配
//!
//! 手动一问一答**不构造本对象**,运行路径与引入前逐字节相同——「不构造编排对象
//! 就是关」(2026-08-10 用户定调),没有第二个布尔开关可以配错。构造闸门是
//! `run.rs` 里那一句 `AutoRunController::enabled`。
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
        "requirements/defects/goals/design 与代码状态的一致性:本次任务相关的文档说法与代码是否对得上。",
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

pub(crate) struct PhasePipeline {
    orchestrator: PhaseOrchestrator,
    /// 单阶段并行角色数上限。复用既有的 `max_tasks_per_turn`——它的语义本来就是
    /// 「一轮最多并行几个子代理」,不另立一个会配歪的新键。
    roster_cap: usize,
    /// `[models] scout` 解析出的路由;None = 沿用模板的 fast。
    scout_route: Option<ScoutRoute>,
}

/// 一个角色的产出。
struct RoleReport {
    role: &'static str,
    text: String,
    ok: bool,
}

impl PhasePipeline {
    pub(crate) fn start(
        coordinator: Arc<dyn ProjectExecutionCoordinator>,
        observer: Arc<dyn PhaseObserver>,
        project_root: std::path::PathBuf,
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
        .with_observer(observer);
        PhasePipeline {
            orchestrator,
            roster_cap: limits.max_tasks_per_turn(),
            scout_route,
        }
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
        let roles: Vec<(&'static str, &'static str)> =
            SCOUT_ROLES.iter().take(self.roster_cap).copied().collect();
        let prompts: Vec<String> = roles
            .iter()
            .map(|(role, brief)| scout_prompt(role, brief, task_prompt))
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
        Ok(render_brief("勘察简报", &reports, outcome.model_notice()))
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
        // 派发即上报:UI 先拿到块,子代理的轮次/工具进度随后挂上去。
        for ((role, brief), prompt) in roles.iter().zip(prompts.iter()) {
            on_event(RunEvent::ToolStart {
                id: role.to_string(),
                name: "task".into(),
                summary: format!("{role} · {}", brief.chars().take(40).collect::<String>()),
                input: serde_json::json!({
                    "prompt": prompt,
                    "phase": phase,
                    "role": role,
                }),
            });
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RunEvent>();
        let reports = Arc::new(std::sync::Mutex::new(Vec::<RoleReport>::new()));
        let tasks: Vec<(String, ScoutTask<'_>)> = roles
            .iter()
            .zip(runtimes.iter())
            .zip(prompts.iter())
            .map(|(((role, _), rt), prompt)| {
                let reports = reports.clone();
                let tx = tx.clone();
                let task: ScoutTask<'_> = Box::pin(async move {
                    let output =
                        kanzei_core::run_read_agent(client, rt, ctx, role, prompt, tx).await;
                    let outcome = if output.is_error {
                        ScoutOutcome::Failed(output.content.chars().take(200).collect())
                    } else {
                        ScoutOutcome::Completed
                    };
                    reports.lock().unwrap().push(RoleReport {
                        role,
                        text: output.content,
                        ok: !output.is_error,
                    });
                    outcome
                });
                (role.to_string(), task)
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
        for (role, _) in roles {
            let report = reports.iter().find(|r| r.role == *role);
            let ok = report.map(|r| r.ok).unwrap_or(false);
            let preview = report
                .map(|r| r.text.lines().next().unwrap_or("").to_string())
                .unwrap_or_else(|| "(超时,未产出结果)".into());
            on_event(RunEvent::ToolEnd {
                id: role.to_string(),
                name: "task".into(),
                ok,
                preview,
                display: None,
            });
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
        let roles: Vec<(&'static str, &'static str)> =
            REVIEW_ROLES.iter().take(self.roster_cap).copied().collect();
        let prompts: Vec<String> = roles
            .iter()
            .map(|(role, brief)| review_prompt(role, brief, task_prompt, run_summary))
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
        let mut agent = template.agent.clone();
        agent.name = role.to_string();
        let (fast, primary, fast_tier, primary_tier) = match &self.scout_route {
            Some(scout) => (
                (scout.route.clone(), scout.model.clone()),
                (scout.route.clone(), scout.model.clone()),
                scout.service_tier.clone(),
                scout.service_tier.clone(),
            ),
            None => (
                template.fast.clone(),
                template.primary.clone(),
                template.fast_service_tier.clone(),
                template.primary_service_tier.clone(),
            ),
        };
        SubagentRuntime {
            snapshot: template.snapshot.clone(),
            agent,
            fast,
            primary,
            fast_service_tier: fast_tier,
            primary_service_tier: primary_tier,
            max_tokens: template.max_tokens,
            timeout_secs: template.timeout_secs,
            limits: template.limits.clone(),
            coordinator: template.coordinator.clone(),
        }
    }
}

fn scout_prompt(role: &str, brief: &str, task_prompt: &str) -> String {
    format!(
        "你是只读勘察代理 `{role}`,工具只有 read/glob/grep。\n\
         职责:{brief}\n\n\
         本轮任务(原文):\n{task_prompt}\n\n\
         只回事实,每条都带文件路径与行号。不要提改动建议,不要下结论说该怎么做。\n\
         与本轮任务无关的发现不要写。什么都没找到就直接说没找到。"
    )
}

fn review_prompt(role: &str, brief: &str, task_prompt: &str, run_summary: &str) -> String {
    format!(
        "你是只读复核代理 `{role}`,工具只有 read/glob/grep。\n\
         职责:{brief}\n\n\
         本轮任务(原文):\n{task_prompt}\n\n\
         本轮执行方的自述结果:\n{run_summary}\n\n\
         去代码里核对上面的自述是否属实。只报**确有问题**的地方,每条带文件路径与行号。\n\
         没有发现问题就只回 `{NO_ISSUES}` 四个字,不要写任何别的东西。"
    )
}

/// 把角色产出拼成给模型看的一块文本。
fn render_brief(title: &str, reports: &[RoleReport], notice: Option<String>) -> String {
    let mut out = format!("[{title}]");
    if let Some(notice) = notice {
        // 反静默降级:失败/超时/零结果必须出现在模型看得见的地方。
        out.push('\n');
        out.push_str(&notice);
    }
    for report in reports {
        if !report.ok {
            continue; // 失败的已经由 notice 点名,正文不再塞错误串
        }
        let text = report.text.trim();
        if text.is_empty() {
            continue;
        }
        out.push_str(&format!("\n\n## {}\n{}", report.role, text));
    }
    out
}

/// 复核发现。全部角色都回 `NO_ISSUES` 且没有失败 → `None`(不需要修正段)。
///
/// 判据刻意**失败即有发现**:复核代理失败/超时时返回 `Some`,让修正段跑一次而不是
/// 当成"没问题"放过去。宁可多跑一段,不可把没复核过的东西当成复核通过。
fn findings(reports: &[RoleReport], notice: Option<String>) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();
    for report in reports {
        if !report.ok {
            continue; // 失败由 notice 承载
        }
        let text = report.text.trim();
        if text.is_empty() || text == NO_ISSUES {
            continue;
        }
        blocks.push(format!("## {}\n{}", report.role, text));
    }
    if blocks.is_empty() && notice.is_none() {
        return None;
    }
    let mut out = String::from("[复核发现]");
    if let Some(notice) = notice {
        out.push('\n');
        out.push_str(&notice);
    }
    for block in blocks {
        out.push_str("\n\n");
        out.push_str(&block);
    }
    Some(out)
}

/// 修正段给模型的提示。
pub(crate) fn fixup_prompt(findings: &str) -> String {
    format!(
        "{findings}\n\n\
         (system) 以上是只读复核代理在你**释放写权之后**、对稳定快照做的核对。\n\
         逐条判断:确有问题的就修掉并给出最小验证;判断为误报的,明确说出理由。\n\
         不要为了显得做了事而改无关的地方。全部都是误报就直说,不用改任何文件。"
    )
}

/// 写租约申请(非流水线路径用):手动一问一答仍走 R-171 的老形状。
pub(crate) fn plain_writer_request(
    project_root: &std::path::Path,
    run_id: &str,
    process_id: &str,
    session_id: &str,
) -> WriterLeaseRequest {
    WriterLeaseRequest {
        project_root: project_root.to_path_buf(),
        run_id: run_id.to_string(),
        process_id: process_id.to_string(),
        reason: format!("session {session_id} writer run"),
    }
}

/// 写租约的取得时机——两条路的**唯一实质差异**,也是不变量 2 在生产路径上的落点。
///
/// - `pipeline_on = true`(自主推进轮):**当场不取**,返回 `Ok(None)`。租约推迟到
///   汇总屏障之后由编排对象在 `begin_implementation` 里取——勘察全部进入终态之前
///   项目里不得出现 writer。
/// - `pipeline_on = false`(手动一问一答):与 R-171 完全一致,当场取、持有整轮,
///   并发 queued/acquired 两条事件。
///
/// 抽成独立函数**只为可测**:这个判定原先内联在 `run_task` 里,而 `run_task` 需要
/// Tauri `Window` 才能调用,于是"流水线开启时不取租约"这条只能靠读代码确认。
pub(crate) async fn acquire_plain_lease_if_needed(
    pipeline_on: bool,
    coordinator: &dyn ProjectExecutionCoordinator,
    observer: &dyn PhaseObserver,
    project_root: &std::path::Path,
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
            project_root,
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

    fn report(role: &'static str, text: &str, ok: bool) -> RoleReport {
        RoleReport {
            role,
            text: text.into(),
            ok,
        }
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

    #[test]
    fn 勘察角色提示是自足的只读指令() {
        for (role, brief) in SCOUT_ROLES {
            let prompt = scout_prompt(role, brief, "修 R-173");
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
