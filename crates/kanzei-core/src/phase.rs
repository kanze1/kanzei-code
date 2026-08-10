//! 阶段编排对象(R-173):baseline → scouting → 汇总屏障 → implementation →
//! integration → 复核屏障 → review → fixup 的确定性状态机。
//!
//! 契约定义在 `kanzei-harness::orchestration`(Phase / BarrierOutcome / PhaseError /
//! PhaseObserver),本模块提供实现——与 R-171 的 `ProjectExecutionCoordinator`
//! → `MemoryCoordinator` 同构。
//!
//! # 装配是可选的
//!
//! 阶段流水线**只在自举/自主推进轮**由调用方显式构造(2026-08-10 用户定调);
//! 手动一问一答不构造本对象,行为与引入前逐字节相同。「不构造编排对象」就是关,
//! 不需要另设开关。
//!
//! # 两道屏障是迁移的唯一通路
//!
//! 不变量 2(勘察全终态前 writer 不启动)与不变量 9(writer 释放后复核才启动)
//! 不靠调用方自觉:`Phase::can_transition_to` 表里进入 `Synthesis` 的唯一入边是
//! `Scouting → Synthesis`,而唯一执行该迁移的方法是 [`PhaseOrchestrator::join_scouts`];
//! 进入 `Review` 的唯一入边是 `Integration → Review`,唯一执行者是
//! [`PhaseOrchestrator::enter_review`],它**必须先交出写租约**。绕不过去。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use kanzei_harness::orchestration::{
    BarrierKind, BarrierOutcome, ExecutionPolicy, OrchestrationEvent, Phase, PhaseError,
    PhaseObserver, ProjectExecutionCoordinator, ScoutOutcome, WriterLease, WriterLeaseRequest,
};

/// 一个勘察/复核任务。调用方把真实子代理调用(以及它的 `ToolOutput` → [`ScoutOutcome`]
/// 映射)装箱交进来;core 不认识 task 工具,只认识"会给出终态的 future"。
pub type ScoutTask = std::pin::Pin<Box<dyn std::future::Future<Output = ScoutOutcome> + Send>>;

/// 阶段编排对象。
///
/// 写租约由本对象**持有**、不外泄:`enter_implementation` 取,`enter_review` /
/// `finish` / `abort` 交,`Drop` 兜底(不变量 7)。调用方拿不到 `WriterLease`,
/// 也就无法"一边持租约一边复核"。
pub struct PhaseOrchestrator {
    coordinator: Arc<dyn ProjectExecutionCoordinator>,
    observer: Option<Arc<dyn PhaseObserver>>,
    project_root: PathBuf,
    run_id: String,
    process_id: String,
    phase: Phase,
    /// 只在 `phase.requires_writer_lease()` 的阶段为 `Some`。
    lease: Option<WriterLease>,
    /// 复核屏障是否已通过。`Review → Fixup` 是合法边,但没过屏障不许走——
    /// 阶段本身表达不了"这一阶段内部的事情做完了没有",所以要这一个标志。
    review_barrier_passed: bool,
    barrier_timeout: Duration,
}

impl PhaseOrchestrator {
    /// 起始于 `Baseline`。
    ///
    /// **此处不发事件**——观察者是 [`Self::with_observer`] 之后才装上的,在这里发
    /// 等于必然丢掉轨迹起点(第一版就踩了这个坑,被全链路轨迹测试逮住)。
    /// 起点由 `with_observer` 补发,见那里的说明。
    ///
    /// `barrier_timeout` 来自 `Limits::barrier_timeout_secs()`——外层兜底,
    /// 永远宽于单个子代理的墙钟上界。
    pub fn new(
        coordinator: Arc<dyn ProjectExecutionCoordinator>,
        project_root: PathBuf,
        run_id: impl Into<String>,
        process_id: impl Into<String>,
        barrier_timeout: Duration,
    ) -> Self {
        PhaseOrchestrator {
            coordinator,
            observer: None,
            project_root,
            run_id: run_id.into(),
            process_id: process_id.into(),
            phase: Phase::Baseline,
            lease: None,
            review_barrier_passed: false,
            barrier_timeout,
        }
    }

    /// 装配事件汇报口(桌面端接 session_events)。不装配则编排照常工作,只是不留轨迹。
    ///
    /// 装配的同时补发一条当前阶段的 `phase_changed`,让轨迹有确定起点:
    /// 观察者接上的那一刻就该知道状态机在哪儿,而不是等到下一次迁移才第一次
    /// 听到消息。正常用法(`new().with_observer()`)下这条就是 `baseline`。
    pub fn with_observer(mut self, observer: Arc<dyn PhaseObserver>) -> Self {
        self.observer = Some(observer);
        self.emit(OrchestrationEvent::PhaseChanged {
            project_root: self.project_root.clone(),
            run_id: self.run_id.clone(),
            phase: self.phase.as_str().to_string(),
        });
        self
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 当前阶段的执行策略:写阶段串行、其余并行。
    pub fn execution_policy(&self) -> ExecutionPolicy {
        self.phase.execution_policy()
    }

    /// 当前阶段是否允许派发只读子代理。
    pub fn allows_read_agents(&self) -> bool {
        self.phase.allows_read_agents()
    }

    /// 当前是否持有写租约(测试与审计用)。
    pub fn holds_writer_lease(&self) -> bool {
        self.lease.is_some()
    }

    fn emit(&self, event: OrchestrationEvent) {
        if let Some(observer) = &self.observer {
            observer.observe(&event);
        }
    }

    /// 只检查不改状态——取租约这类有副作用的动作必须先过这一关,
    /// 否则非法调用会先占住租约再报错。
    fn guard_transition(&self, next: Phase) -> Result<(), PhaseError> {
        if self.phase.can_transition_to(next) {
            Ok(())
        } else {
            Err(PhaseError::IllegalTransition {
                from: self.phase,
                to: next,
            })
        }
    }

    fn commit_transition(&mut self, next: Phase) {
        self.phase = next;
        self.emit(OrchestrationEvent::PhaseChanged {
            project_root: self.project_root.clone(),
            run_id: self.run_id.clone(),
            phase: next.as_str().to_string(),
        });
    }

    /// 取写租约并记账。调用前必须已过 `guard_transition`。
    async fn acquire_lease(&mut self, reason: &str) -> Result<(), PhaseError> {
        self.emit(OrchestrationEvent::WriterQueued {
            project_root: self.project_root.clone(),
            run_id: self.run_id.clone(),
            process_id: self.process_id.clone(),
            reason: reason.to_string(),
        });
        let lease = self
            .coordinator
            .acquire_writer_lease(WriterLeaseRequest {
                project_root: self.project_root.clone(),
                run_id: self.run_id.clone(),
                process_id: self.process_id.clone(),
                reason: reason.to_string(),
            })
            .await
            .map_err(PhaseError::Coordinator)?;
        self.emit(OrchestrationEvent::WriterAcquired {
            project_root: self.project_root.clone(),
            run_id: self.run_id.clone(),
            process_id: self.process_id.clone(),
        });
        self.lease = Some(lease);
        Ok(())
    }

    /// 交出写租约。
    ///
    /// **不变量 9 的机械保证在这里**,三层:
    /// 1. 所有权移交——租约只能从 `self.lease` 里 `take` 出来,take 之后本对象
    ///    再也拿不到写权,"一边持租约一边复核"在类型上不可表达;
    /// 2. Drop 同步性——`WriterLease::drop` 直接调协调器的释放回调,回调内只取
    ///    `std::sync::Mutex`、没有 await 也不 spawn,所以 `drop(lease)` 这一行
    ///    返回时释放**已经完成**,不是异步承诺;
    /// 3. 快照复核——用独立于 Drop 路径的第二真源确认本 run 已不是 writer。
    ///    将来若有人给释放路径加了异步分支,这里当场失败而不是静默放行。
    ///
    /// 返回 `true` 表示确实持有并交出了租约。
    fn release_lease(&mut self) -> Result<bool, PhaseError> {
        let Some(lease) = self.lease.take() else {
            return Ok(false);
        };
        let run_id = lease.run_id.clone();
        let process_id = lease.process_id.clone();
        drop(lease); // 同步释放:返回即已生效
        let snapshot = self.coordinator.snapshot(&self.project_root);
        if snapshot.writer_run_id.as_deref() == Some(run_id.as_str()) {
            return Err(PhaseError::LeaseStillHeld { phase: self.phase });
        }
        self.emit(OrchestrationEvent::WriterReleased {
            project_root: self.project_root.clone(),
            run_id,
            process_id,
        });
        Ok(true)
    }

    // ---- 阶段迁移 --------------------------------------------------------

    /// `Baseline → Scouting`。
    pub fn enter_scouting(&mut self) -> Result<(), PhaseError> {
        self.guard_transition(Phase::Scouting)?;
        self.commit_transition(Phase::Scouting);
        Ok(())
    }

    /// **汇总屏障**(不变量 2):等全部勘察任务进入终态,然后 `Scouting → Synthesis`。
    ///
    /// 这是进入 `Synthesis` 的唯一通路,而 `Synthesis` 又是取写租约的唯一前驱
    /// (迁移表里没有 `Scouting → Implementation`)——所以"勘察没收敛就开写"
    /// 在状态机上走不通。
    ///
    /// 失败/超时**不中止**(2026-08-10 用户裁决②):勘察阶段尚未产生任何写入,
    /// 一个 scout 失败不构成一致性风险;但零结果必须让模型知道,见
    /// [`BarrierOutcome::model_notice`]。
    pub async fn join_scouts(
        &mut self,
        scouts: Vec<(String, ScoutTask)>,
    ) -> Result<BarrierOutcome, PhaseError> {
        self.guard_transition(Phase::Synthesis)?;
        let outcome = self.run_barrier(BarrierKind::Synthesis, scouts).await;
        self.commit_transition(Phase::Synthesis);
        Ok(outcome)
    }

    /// `Synthesis → Implementation`:取写租约。
    pub async fn enter_implementation(&mut self) -> Result<(), PhaseError> {
        self.guard_transition(Phase::Implementation)?;
        self.acquire_lease("implementation phase writer").await?;
        self.commit_transition(Phase::Implementation);
        Ok(())
    }

    /// `Implementation → Integration`:**同一租约继续**,不重取
    /// (不变量 4:不允许在两个工具调用之间切换写代理)。
    pub fn enter_integration(&mut self) -> Result<(), PhaseError> {
        self.guard_transition(Phase::Integration)?;
        if self.lease.is_none() {
            return Err(PhaseError::LeaseMissing {
                phase: Phase::Integration,
            });
        }
        self.commit_transition(Phase::Integration);
        Ok(())
    }

    /// **复核屏障之一 · 稳定快照门**(不变量 9):`Integration → Review`,
    /// 先交出写租约再进复核。见 [`Self::release_lease`] 的三层保证。
    ///
    /// 语义边界:本方法保证的是「**本 run 已交出写权**」,不是「项目全局无 writer」。
    /// 释放瞬间另一个 ProcessHandle 的排队 writer 会立刻接手——设计不变量 9 的原文
    /// 就是"等 writer 释放租约后再启动",精确对应。等全局静默会被后来的写者饿死,
    /// 那属于 P3 的跨进程稳定快照,不在本条。
    pub fn enter_review(&mut self) -> Result<(), PhaseError> {
        self.guard_transition(Phase::Review)?;
        if !self.release_lease()? {
            return Err(PhaseError::LeaseMissing {
                phase: Phase::Integration,
            });
        }
        self.commit_transition(Phase::Review);
        Ok(())
    }

    /// **复核屏障之二 · 汇总门**:等全部复核任务进入终态。
    ///
    /// 与汇总屏障不同,本方法**不迁移阶段**——`Review` 之后可能是 `Fixup`
    /// (有问题要修)也可能是 `Finished`(复核通过),由调用方按屏障结果决定。
    /// 但它会记下"屏障已过",[`Self::enter_fixup`] 拿这个作前置条件。
    pub async fn join_reviewers(
        &mut self,
        reviewers: Vec<(String, ScoutTask)>,
    ) -> Result<BarrierOutcome, PhaseError> {
        if self.phase != Phase::Review {
            return Err(PhaseError::IllegalTransition {
                from: self.phase,
                to: Phase::Review,
            });
        }
        let outcome = self.run_barrier(BarrierKind::Review, reviewers).await;
        self.review_barrier_passed = true;
        Ok(outcome)
    }

    /// `Review → Fixup`:**重新**获取写租约(与实现阶段那次是两段独立区间)。
    /// 必须先过复核汇总门,否则复核还没收敛就开修。
    pub async fn enter_fixup(&mut self) -> Result<(), PhaseError> {
        self.guard_transition(Phase::Fixup)?;
        if !self.review_barrier_passed {
            return Err(PhaseError::IllegalTransition {
                from: Phase::Review,
                to: Phase::Fixup,
            });
        }
        self.acquire_lease("fixup phase writer").await?;
        self.commit_transition(Phase::Fixup);
        Ok(())
    }

    /// 正常收工:交出租约(若持有)并进入 `Finished`。合法前驱见迁移表
    /// (`Synthesis`/`Integration`/`Review`/`Fixup`)。
    pub fn finish(&mut self) -> Result<(), PhaseError> {
        self.guard_transition(Phase::Finished)?;
        self.release_lease()?;
        self.commit_transition(Phase::Finished);
        Ok(())
    }

    /// 异常收尾(用户停止/取消/错误路径)。
    ///
    /// **不走合法迁移表**——不变量 7 要求任何结束路径都释放写租约并给出确定终态,
    /// 所以 abort 从任意阶段直达 `Finished`。这是一个显式命名的操作,不是
    /// "碰巧合法"的迁移:读代码的人一眼能看出这是异常路径。
    pub fn abort(&mut self, reason: &str) {
        if let Some(lease) = self.lease.take() {
            let run_id = lease.run_id.clone();
            let process_id = lease.process_id.clone();
            drop(lease);
            self.emit(OrchestrationEvent::WriterReleased {
                project_root: self.project_root.clone(),
                run_id,
                process_id,
            });
        }
        self.emit(OrchestrationEvent::WriterRecovered {
            project_root: self.project_root.clone(),
            run_id: self.run_id.clone(),
            reason: reason.to_string(),
        });
        self.commit_transition(Phase::Finished);
    }

    // ---- 屏障实现 --------------------------------------------------------

    /// 屏障:等全部任务进入终态,或触顶外层上界。
    ///
    /// **双层有界,任一层都不会永久挂起**:
    /// - 内层——每个勘察子代理已被 `Limits::subagent_timeout_secs` 的墙钟包住
    ///   (见 runner 的 task 派发),所以传进来的每个 future 本身就会收敛;
    /// - 外层——本方法再包一层 `barrier_timeout`。它永远宽于内层(配置层已夹紧),
    ///   正常情况下不触发;真触发说明内层失效,`barrier_timed_out` 与
    ///   `BarrierTimedOut` 事件把这件事留证,而不是静默挂死。
    async fn run_barrier(
        &mut self,
        kind: BarrierKind,
        tasks: Vec<(String, ScoutTask)>,
    ) -> BarrierOutcome {
        let mut outcome = BarrierOutcome::new(kind, tasks.len());
        if tasks.is_empty() {
            self.emit_barrier_reached(&outcome);
            return outcome;
        }
        // 未完成任务的名册:超时兜底时要知道"是谁没回来",否则只能报个数字。
        let mut pending: BTreeMap<usize, String> = tasks
            .iter()
            .enumerate()
            .map(|(index, (name, _))| (index, name.clone()))
            .collect();
        for name in pending.values() {
            self.emit(OrchestrationEvent::AgentStarted {
                project_root: self.project_root.clone(),
                run_id: self.run_id.clone(),
                agent_name: name.clone(),
            });
        }
        let mut jobs: futures::stream::FuturesUnordered<_> = tasks
            .into_iter()
            .enumerate()
            .map(|(index, (name, task))| async move { (index, name, task.await) })
            .collect();
        let deadline = tokio::time::sleep(self.barrier_timeout);
        tokio::pin!(deadline);
        let mut events = Vec::new();
        loop {
            tokio::select! {
                next = jobs.next() => match next {
                    Some((index, name, result)) => {
                        pending.remove(&index);
                        events.push(self.agent_terminal_event(&name, &result));
                        outcome.record(name, result);
                    }
                    None => break,
                },
                _ = &mut deadline => {
                    // 外层兜底:剩下没回来的一律记超时终态,屏障就此收敛。
                    outcome.barrier_timed_out = true;
                    let waited = self.barrier_timeout.as_secs();
                    for (_, name) in std::mem::take(&mut pending) {
                        let result = ScoutOutcome::TimedOut { after_secs: waited };
                        events.push(self.agent_terminal_event(&name, &result));
                        outcome.record(name, result);
                    }
                    events.push(OrchestrationEvent::BarrierTimedOut {
                        project_root: self.project_root.clone(),
                        run_id: self.run_id.clone(),
                        barrier: kind,
                        waited_secs: waited,
                    });
                    break;
                }
            }
        }
        for event in events {
            self.emit(event);
        }
        self.emit_barrier_reached(&outcome);
        outcome
    }

    fn agent_terminal_event(&self, name: &str, result: &ScoutOutcome) -> OrchestrationEvent {
        match result {
            ScoutOutcome::Completed => OrchestrationEvent::AgentCompleted {
                project_root: self.project_root.clone(),
                run_id: self.run_id.clone(),
                agent_name: name.to_string(),
                ok: true,
            },
            ScoutOutcome::Failed(reason) => OrchestrationEvent::AgentFailed {
                project_root: self.project_root.clone(),
                run_id: self.run_id.clone(),
                agent_name: name.to_string(),
                reason: format!("failed: {reason}"),
            },
            ScoutOutcome::TimedOut { after_secs } => OrchestrationEvent::AgentFailed {
                project_root: self.project_root.clone(),
                run_id: self.run_id.clone(),
                agent_name: name.to_string(),
                reason: format!("timed out after {after_secs}s"),
            },
        }
    }

    fn emit_barrier_reached(&self, outcome: &BarrierOutcome) {
        self.emit(OrchestrationEvent::BarrierReached {
            project_root: self.project_root.clone(),
            run_id: self.run_id.clone(),
            barrier: outcome.barrier,
            agent_count: outcome.agent_count,
            completed: outcome.completed,
            failed: outcome.failed,
            timed_out: outcome.timed_out,
        });
    }
}

impl Drop for PhaseOrchestrator {
    /// 不变量 7 的兜底:panic 收尾、提前 return、窗口退出——任何没走
    /// `finish`/`abort` 的结束路径,租约仍由 `WriterLease::drop` 释放,
    /// 这里补一条审计事件,让轨迹不会停在"某个 run 拿了租约就没下文"。
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take() {
            let run_id = lease.run_id.clone();
            let process_id = lease.process_id.clone();
            drop(lease);
            self.emit(OrchestrationEvent::WriterReleased {
                project_root: self.project_root.clone(),
                run_id,
                process_id,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::MemoryCoordinator;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// 记录事件的观察者:轨迹断言用。
    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl Recorder {
        fn types(&self) -> Vec<String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|(t, _)| t.clone())
                .collect()
        }
        fn payloads_of(&self, event_type: &str) -> Vec<serde_json::Value> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|(t, _)| t == event_type)
                .map(|(_, p)| p.clone())
                .collect()
        }
    }

    impl PhaseObserver for Recorder {
        fn observe(&self, event: &OrchestrationEvent) {
            self.events
                .lock()
                .unwrap()
                .push((event.event_type().to_string(), event.payload()));
        }
    }

    fn root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kz-phase-{tag}-{}", std::process::id()))
    }

    fn orchestrator(
        tag: &str,
        coord: &MemoryCoordinator,
        recorder: &Arc<Recorder>,
        barrier_ms: u64,
    ) -> PhaseOrchestrator {
        PhaseOrchestrator::new(
            Arc::new(coord.clone()) as Arc<dyn ProjectExecutionCoordinator>,
            root(tag),
            "run_phase",
            "proc_phase",
            Duration::from_millis(barrier_ms),
        )
        .with_observer(recorder.clone() as Arc<dyn PhaseObserver>)
    }

    fn done(name: &str) -> (String, ScoutTask) {
        (name.into(), Box::pin(async { ScoutOutcome::Completed }))
    }

    // ---- 批2:状态机 -----------------------------------------------------

    /// 非法迁移被挡下且**不改变状态**——失败的调用不能把状态机推到半路。
    #[tokio::test]
    async fn 非法迁移被拒且状态不变() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let mut orch = orchestrator("illegal", &coord, &recorder, 1_000);
        assert_eq!(orch.phase(), Phase::Baseline);

        // baseline 直接进实现:表里没有这条边。
        let err = orch.enter_implementation().await.unwrap_err();
        assert!(matches!(
            err,
            PhaseError::IllegalTransition {
                from: Phase::Baseline,
                to: Phase::Implementation
            }
        ));
        assert_eq!(orch.phase(), Phase::Baseline, "失败的迁移不得改变状态");
        assert!(!orch.holds_writer_lease(), "非法迁移不得占住写租约");
        assert!(
            coord.snapshot(&root("illegal")).writer_run_id.is_none(),
            "非法迁移不得在协调器里留下写者"
        );

        // 勘察后直接进实现:必须先过汇总屏障(不变量 2)。
        orch.enter_scouting().unwrap();
        let err = orch.enter_implementation().await.unwrap_err();
        assert!(matches!(
            err,
            PhaseError::IllegalTransition {
                from: Phase::Scouting,
                ..
            }
        ));
        assert_eq!(orch.phase(), Phase::Scouting);
    }

    /// 集成阶段没有租约时不许迁移(不变量 4:租约必须跨实现与集成连续持有)。
    #[tokio::test]
    async fn 集成阶段缺租约被拒() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let mut orch = orchestrator("nolease", &coord, &recorder, 1_000);
        orch.enter_scouting().unwrap();
        orch.join_scouts(vec![]).await.unwrap();
        orch.enter_implementation().await.unwrap();
        assert!(orch.holds_writer_lease());
        // 人为把租约抽走(模拟"实现阶段中途丢了写权"),集成必须拒绝继续。
        orch.lease = None;
        let err = orch.enter_integration().unwrap_err();
        assert!(matches!(
            err,
            PhaseError::LeaseMissing {
                phase: Phase::Integration
            }
        ));
    }

    // ---- 批3:汇总屏障 ---------------------------------------------------

    /// 验收①:两个只读任务真实重叠,且屏障返回前 writer 没有启动。
    #[tokio::test]
    async fn 勘察真实重叠且屏障前writer不启动() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let mut orch = orchestrator("overlap", &coord, &recorder, 5_000);
        orch.enter_scouting().unwrap();

        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let make = |in_flight: Arc<AtomicUsize>, max: Arc<AtomicUsize>, ms: u64| -> ScoutTask {
            Box::pin(async move {
                let active = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(active, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(ms)).await;
                in_flight.fetch_sub(1, Ordering::SeqCst);
                ScoutOutcome::Completed
            })
        };
        let scouts = vec![
            (
                "architecture_scout".to_string(),
                make(in_flight.clone(), max_in_flight.clone(), 60),
            ),
            (
                "runtime_scout".to_string(),
                make(in_flight.clone(), max_in_flight.clone(), 30),
            ),
        ];
        // 屏障返回之前,协调器里不能有写者。
        let snapshot_root = root("overlap");
        assert!(coord.snapshot(&snapshot_root).writer_run_id.is_none());
        let outcome = orch.join_scouts(scouts).await.unwrap();
        assert!(
            coord.snapshot(&snapshot_root).writer_run_id.is_none(),
            "汇总屏障期间 writer 不得启动(不变量 2)"
        );

        assert!(
            max_in_flight.load(Ordering::SeqCst) >= 2,
            "两个勘察任务没有真实重叠执行"
        );
        assert_eq!(outcome.agent_count, 2);
        assert_eq!(outcome.completed, 2);
        assert!(outcome.all_terminal());
        assert!(!outcome.barrier_timed_out);
        assert!(outcome.model_notice().is_none(), "全成功不该打扰模型");
        assert_eq!(orch.phase(), Phase::Synthesis);
    }

    /// 三种终态各自归位,且**失败不中止**:流水线照常进入实现阶段。
    #[tokio::test]
    async fn 三终态收敛_失败不中止且零结果告知模型() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        // 屏障上界 80ms:第三个任务永不返回,靠外层兜底收成超时。
        let mut orch = orchestrator("terminal", &coord, &recorder, 80);
        orch.enter_scouting().unwrap();
        let scouts: Vec<(String, ScoutTask)> = vec![
            (
                "ok_scout".into(),
                Box::pin(async { ScoutOutcome::Completed }),
            ),
            (
                "bad_scout".into(),
                Box::pin(async { ScoutOutcome::Failed("provider 500".into()) }),
            ),
            ("stuck_scout".into(), Box::pin(futures::future::pending())),
        ];
        let outcome = orch.join_scouts(scouts).await.unwrap();

        assert_eq!(
            (outcome.completed, outcome.failed, outcome.timed_out),
            (1, 1, 1),
            "三种终态必须各自归位"
        );
        assert!(outcome.all_terminal(), "屏障返回时全部任务必须已终态");
        assert!(outcome.barrier_timed_out, "永不返回的任务应由外层兜底");
        let notice = outcome.model_notice().expect("有失败必须告知模型");
        assert!(notice.contains("bad_scout") && notice.contains("stuck_scout"));

        // 失败不中止:照常能进实现阶段。
        assert_eq!(orch.phase(), Phase::Synthesis);
        orch.enter_implementation().await.unwrap();
        assert!(orch.holds_writer_lease());

        let types = recorder.types();
        assert!(types.contains(&"orchestration.barrier_timed_out".to_string()));
        assert!(types.contains(&"orchestration.agent_failed".to_string()));
        let reached = recorder.payloads_of("orchestration.barrier_reached");
        assert_eq!(reached.len(), 1);
        assert_eq!(reached[0]["barrier"], "synthesis");
        assert_eq!(reached[0]["completed"], 1);
        assert_eq!(reached[0]["failed"], 1);
        assert_eq!(reached[0]["timed_out"], 1);
    }

    /// 空勘察也要收敛并留下屏障事件——零任务不是"跳过屏障"。
    #[tokio::test]
    async fn 零勘察任务也经过屏障() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let mut orch = orchestrator("empty", &coord, &recorder, 1_000);
        orch.enter_scouting().unwrap();
        let outcome = orch.join_scouts(vec![]).await.unwrap();
        assert_eq!(outcome.agent_count, 0);
        assert!(outcome.all_terminal());
        assert!(outcome.model_notice().is_none());
        assert_eq!(orch.phase(), Phase::Synthesis);
        assert_eq!(
            recorder.payloads_of("orchestration.barrier_reached").len(),
            1
        );
    }

    // ---- 批4:复核屏障 ---------------------------------------------------

    /// 验收③:复核在 writer 释放之后启动,审查的是稳定快照。
    #[tokio::test]
    async fn 复核屏障_交出租约后才进复核() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let mut orch = orchestrator("review", &coord, &recorder, 1_000);
        let dir = root("review");
        orch.enter_scouting().unwrap();
        orch.join_scouts(vec![done("scout")]).await.unwrap();
        orch.enter_implementation().await.unwrap();
        orch.enter_integration().unwrap();
        assert_eq!(
            coord.snapshot(&dir).writer_run_id.as_deref(),
            Some("run_phase"),
            "集成阶段仍持有写租约"
        );

        orch.enter_review().unwrap();
        assert_eq!(orch.phase(), Phase::Review);
        assert!(!orch.holds_writer_lease(), "进复核后不得再持有写租约");
        assert!(
            coord.snapshot(&dir).writer_run_id.is_none(),
            "复核阶段协调器里不得还有本 run 的写者(不变量 9)"
        );

        // 事件顺序:released 必须排在 phase_changed(review) 之前。
        let types = recorder.types();
        let released = types
            .iter()
            .position(|t| t == "orchestration.writer.released")
            .expect("必须有 released 事件");
        let phase_events = recorder.payloads_of("orchestration.phase_changed");
        assert_eq!(
            phase_events.last().unwrap()["phase"],
            "review",
            "最后一次阶段变更应为 review"
        );
        let review_pos = types
            .iter()
            .enumerate()
            .filter(|(_, t)| *t == "orchestration.phase_changed")
            .map(|(i, _)| i)
            .next_back()
            .unwrap();
        assert!(
            released < review_pos,
            "writer.released 必须早于 phase_changed(review)"
        );
    }

    /// 没持租约就想进复核 → 拒绝(而不是静默放行成"稳定快照")。
    #[tokio::test]
    async fn 未持租约进复核被拒() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let mut orch = orchestrator("norelease", &coord, &recorder, 1_000);
        orch.enter_scouting().unwrap();
        orch.join_scouts(vec![]).await.unwrap();
        orch.enter_implementation().await.unwrap();
        orch.enter_integration().unwrap();
        orch.lease = None; // 模拟租约意外丢失
        let err = orch.enter_review().unwrap_err();
        assert!(matches!(err, PhaseError::LeaseMissing { .. }));
        assert_eq!(orch.phase(), Phase::Integration, "拒绝后必须留在集成阶段");
    }

    /// 复核汇总门未过就想进修正 → 拒绝。
    #[tokio::test]
    async fn 复核未收敛不得进修正() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let mut orch = orchestrator("fixupgate", &coord, &recorder, 1_000);
        orch.enter_scouting().unwrap();
        orch.join_scouts(vec![]).await.unwrap();
        orch.enter_implementation().await.unwrap();
        orch.enter_integration().unwrap();
        orch.enter_review().unwrap();
        let err = orch.enter_fixup().await.unwrap_err();
        assert!(matches!(err, PhaseError::IllegalTransition { .. }));
        assert!(!orch.holds_writer_lease(), "被拒的修正不得占住写租约");
        // 过了复核汇总门就能进,并且重新拿到租约。
        orch.join_reviewers(vec![done("reviewer")]).await.unwrap();
        orch.enter_fixup().await.unwrap();
        assert_eq!(orch.phase(), Phase::Fixup);
        assert!(orch.holds_writer_lease());
    }

    /// 验收④:writer 活跃时只读勘察照常拿到读槽(读写共存)。
    ///
    /// 这是 R-171 `MemoryCoordinator::acquire_read_slot` 的**既有**性质(该函数
    /// 全程不读 `writer_run_id`、唯一返回路径是 Ok),本条只补验证与消费者。
    #[tokio::test]
    async fn writer活跃时读槽仍可获取() {
        use kanzei_harness::orchestration::ReadSlotRequest;
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let mut orch = orchestrator("coexist", &coord, &recorder, 1_000);
        let dir = root("coexist");
        orch.enter_scouting().unwrap();
        orch.join_scouts(vec![]).await.unwrap();
        orch.enter_implementation().await.unwrap();
        assert_eq!(
            coord.snapshot(&dir).writer_run_id.as_deref(),
            Some("run_phase")
        );

        // 写者活跃 → 只读勘察不被阻塞。
        let permit = coord
            .acquire_read_slot(ReadSlotRequest {
                project_root: dir.clone(),
                run_id: "reader_1".into(),
                process_id: "proc_reader".into(),
                agent_name: "live_scout".into(),
            })
            .await
            .expect("writer 活跃时读槽必须仍可获取");
        let snapshot = coord.snapshot(&dir);
        assert_eq!(snapshot.writer_run_id.as_deref(), Some("run_phase"));
        assert_eq!(snapshot.active_readers, vec!["live_scout".to_string()]);
        drop(permit);
        assert!(coord.snapshot(&dir).active_readers.is_empty());
    }

    /// 不变量 7:没走 finish/abort 的结束路径,租约照样释放,下一个写者能拿到。
    #[tokio::test]
    async fn 编排对象被丢弃时释放租约() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let dir = root("dropped");
        {
            let mut orch = orchestrator("dropped", &coord, &recorder, 1_000);
            orch.enter_scouting().unwrap();
            orch.join_scouts(vec![]).await.unwrap();
            orch.enter_implementation().await.unwrap();
            assert!(coord.snapshot(&dir).writer_run_id.is_some());
        } // 作用域结束 = panic 收尾的等价路径
        assert!(
            coord.snapshot(&dir).writer_run_id.is_none(),
            "编排对象析构必须释放写租约(不变量 7)"
        );
        assert!(recorder
            .types()
            .contains(&"orchestration.writer.released".to_string()));
    }

    /// abort 从任意阶段直达终态并释放租约。
    #[tokio::test]
    async fn abort从任意阶段收敛并释放租约() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let dir = root("abort");
        let mut orch = orchestrator("abort", &coord, &recorder, 1_000);
        orch.enter_scouting().unwrap();
        orch.join_scouts(vec![]).await.unwrap();
        orch.enter_implementation().await.unwrap();
        orch.abort("用户停止");
        assert_eq!(orch.phase(), Phase::Finished);
        assert!(!orch.holds_writer_lease());
        assert!(coord.snapshot(&dir).writer_run_id.is_none());
        let recovered = recorder.payloads_of("orchestration.writer.recovered");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0]["reason"], "用户停止");
    }

    /// 全链路轨迹:七阶段跑完一遍,事件序列可回放(验收②的 core 侧锚点)。
    #[tokio::test]
    async fn 全阶段闭环轨迹可回放() {
        let coord = MemoryCoordinator::new();
        let recorder = Arc::new(Recorder::default());
        let mut orch = orchestrator("loop", &coord, &recorder, 1_000);
        orch.enter_scouting().unwrap();
        orch.join_scouts(vec![done("scout_a"), done("scout_b")])
            .await
            .unwrap();
        orch.enter_implementation().await.unwrap();
        orch.enter_integration().unwrap();
        orch.enter_review().unwrap();
        orch.join_reviewers(vec![done("reviewer_a")]).await.unwrap();
        orch.enter_fixup().await.unwrap();
        orch.finish().unwrap();
        assert_eq!(orch.phase(), Phase::Finished);

        let phases: Vec<String> = recorder
            .payloads_of("orchestration.phase_changed")
            .iter()
            .map(|p| p["phase"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(
            phases,
            vec![
                "baseline",
                "scouting",
                "synthesis",
                "implementation",
                "integration",
                "review",
                "fixup",
                "finished",
            ],
            "七阶段必须按设计文档的顺序留下完整轨迹"
        );

        let types = recorder.types();
        // 写租约取了两次(实现 + 修正),交了两次,区间不重叠。
        assert_eq!(
            types
                .iter()
                .filter(|t| *t == "orchestration.writer.acquired")
                .count(),
            2
        );
        assert_eq!(
            types
                .iter()
                .filter(|t| *t == "orchestration.writer.released")
                .count(),
            2
        );
        // 两道屏障各一次,种类正确。
        let barriers: Vec<String> = recorder
            .payloads_of("orchestration.barrier_reached")
            .iter()
            .map(|p| p["barrier"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(barriers, vec!["synthesis", "review"]);
        assert!(coord.snapshot(&root("loop")).writer_run_id.is_none());
    }
}
