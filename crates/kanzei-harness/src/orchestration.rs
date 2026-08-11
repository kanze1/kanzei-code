//! 项目级执行编排契约(R-171):「并行查、串行写」的机械强制层。
//!
//! 本模块只定义契约(策略、租约请求/许可、协调器 trait、事件负载),
//! 不承载具体实现——内存实现放 kanzei-core,未来 OS 进程锁实现换插不换契约。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

type ReleaseCallback = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

/// 执行策略。`ReadParallelWriteSerial` 同时约束 task 使用阶段、writer 租约
/// 与普通工具执行模式;`Default` 保持现状(wave 并发、无租约)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExecutionPolicy {
    #[default]
    Default,
    ReadParallelWriteSerial,
}

impl ExecutionPolicy {
    pub fn is_serial_writer(&self) -> bool {
        matches!(self, ExecutionPolicy::ReadParallelWriteSerial)
    }
}

/// 写租约申请。`write_scope` 规范化后就是仲裁桶键;
/// run_id/process_id 是租约归属与审计身份。
///
/// # 仲裁范围不再等于项目(R-182 内容①)
///
/// R-171 的不变量 3 是「同一 project_root 同时最多一个 writer」,于是同一项目的
/// N 条线在 kzapp 里根本不能同时跑——第二条要等第一条**整轮**结束。2026-08-11
/// 的四组实测把这条顶翻了:跨 worktree 的编号撞车来自文档被 checkout 成两份,
/// 而同根并发由 R-138 的 `atomic_file::FileLock` 兜得住。用户定调改成
/// **分支干、合并、冲突检测解决、文档一份唯一**。
///
/// 据此本字段从 `project_root` 改名为 `write_scope`,含义由调用方说了算:
/// - **主对话的每一轮**传本轮**代码树**(线 = worktree)。于是两条线互不排队,
///   而同一棵树上的两个进程仍然排队——同树两个 writer 会互相覆盖文件,那不是
///   并行,是丢工作;
/// - **托管文档的单一性不再靠它**,由 `atomic_file::FileLock` 的毫秒级单次操作
///   持锁承担(docstore 早就是这个形态);
/// - 建树、tracker 快写等入口仍传**主根**——它们争的确实是主根那一份资产。
///
/// 排队实现原样保留,是为了将来要重新收紧时接口不用改(R-182 边界原文)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriterLeaseRequest {
    pub write_scope: PathBuf,
    pub run_id: String,
    pub process_id: String,
    pub reason: String,
}

/// 读槽申请(勘察/复核只读子代理)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadSlotRequest {
    pub project_root: PathBuf,
    pub run_id: String,
    pub process_id: String,
    pub agent_name: String,
}

/// 写许可:持有者独占项目写权。Drop 时调用注入的释放回调(协调器实现提供),
/// 保证正常/取消/panic 收尾任何路径都不会永久占用租约。
pub struct WriterLease {
    pub project_root: PathBuf,
    pub run_id: String,
    pub process_id: String,
    release: Option<ReleaseCallback>,
}

impl std::fmt::Debug for WriterLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriterLease")
            .field("project_root", &self.project_root)
            .field("run_id", &self.run_id)
            .field("process_id", &self.process_id)
            .finish()
    }
}

impl WriterLease {
    /// 协调器实现创建租约时注入释放回调;未注入(如测试直构)则 drop 为空操作。
    pub fn with_release(
        project_root: PathBuf,
        run_id: String,
        process_id: String,
        release: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        WriterLease {
            project_root,
            run_id,
            process_id,
            release: Some(std::sync::Arc::new(release)),
        }
    }
}

impl Drop for WriterLease {
    fn drop(&mut self) {
        if let Some(cb) = &self.release {
            cb(&self.run_id);
        }
    }
}

/// 读许可:只读并发不受限制,但复核阶段必须等 writer 释放后启动。
/// Drop 时调用注入的释放回调,保证子代理结束(含失败/取消)即从快照消失。
pub struct ReadPermit {
    pub project_root: PathBuf,
    /// 读槽身份键。**必须唯一**——同轮并行的 N 个子代理共用同一个 `agent_name`
    /// (都是 agent 定义名,如 `explore`),只有 run_id(= 父 tool call id)能区分它们。
    pub run_id: String,
    pub agent_name: String,
    release: Option<ReleaseCallback>,
}

impl std::fmt::Debug for ReadPermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadPermit")
            .field("project_root", &self.project_root)
            .field("run_id", &self.run_id)
            .field("agent_name", &self.agent_name)
            .finish()
    }
}

impl ReadPermit {
    /// 协调器实现创建读槽时注入释放回调;未注入(如测试直构)则 drop 为空操作。
    ///
    /// 回调收到的是 **run_id**,不是 agent_name(R-173 批4.5 修:原实现按
    /// agent_name 回收,而并行子代理的 agent_name 全部相同,协调器只能"随便挑一条
    /// 同名的删掉"——个数对得上,身份是错的,AgentCompleted 会报错运行的 run_id)。
    pub fn with_release(
        project_root: PathBuf,
        run_id: String,
        agent_name: String,
        release: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        ReadPermit {
            project_root,
            run_id,
            agent_name,
            release: Some(std::sync::Arc::new(release)),
        }
    }
}

impl Drop for ReadPermit {
    fn drop(&mut self) {
        if let Some(cb) = &self.release {
            cb(&self.run_id);
        }
    }
}

/// 协调器快照(可观察性):谁在排队、谁持有写权、各项目读代理数。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoordinatorSnapshot {
    pub project_root: PathBuf,
    pub writer: Option<String>,
    pub writer_run_id: Option<String>,
    pub waiting_writers: Vec<String>,
    pub active_readers: Vec<String>,
}

/// 编排事件负载(R-171 批5:进 session_events)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrchestrationEvent {
    WriterQueued {
        project_root: PathBuf,
        run_id: String,
        process_id: String,
        reason: String,
    },
    WriterAcquired {
        project_root: PathBuf,
        run_id: String,
        process_id: String,
    },
    WriterReleased {
        project_root: PathBuf,
        run_id: String,
        process_id: String,
    },
    WriterCancelled {
        project_root: PathBuf,
        run_id: String,
    },
    WriterRecovered {
        project_root: PathBuf,
        run_id: String,
        reason: String,
    },
    PhaseChanged {
        project_root: PathBuf,
        run_id: String,
        phase: String,
    },
    AgentStarted {
        project_root: PathBuf,
        run_id: String,
        agent_name: String,
    },
    AgentCompleted {
        project_root: PathBuf,
        run_id: String,
        agent_name: String,
        ok: bool,
    },
    /// R-173:勘察/复核子代理进入失败终态(设计文档 `orchestration.agent_failed`)。
    /// R-171 只有 AgentCompleted{ok},失败原因无处安放——屏障统计要区分
    /// 「失败」与「超时」,原因文本必须能回放。
    AgentFailed {
        project_root: PathBuf,
        run_id: String,
        agent_name: String,
        reason: String,
    },
    /// R-173:屏障达成。R-171 原变体只有 agent_count,回放答不出
    /// 「几个成功、几个失败、几个超时」——而这正是屏障唯一有价值的信息。
    BarrierReached {
        project_root: PathBuf,
        run_id: String,
        barrier: BarrierKind,
        agent_count: usize,
        completed: usize,
        failed: usize,
        timed_out: usize,
    },
    /// R-173:屏障自身触顶上界(而非等到全部任务自然终态)。
    /// 内层子代理已有墙钟上界,正常永不触发;真触发说明内层失效,必须留证。
    BarrierTimedOut {
        project_root: PathBuf,
        run_id: String,
        barrier: BarrierKind,
        waited_secs: u64,
    },
}

impl OrchestrationEvent {
    /// 落库事件类型名——**唯一出口**。
    ///
    /// R-171 的事件是「枚举一套、落库字符串一套」:枚举只写进内存 last_event,
    /// session_events 走调用方手写的字符串字面量,两边可以任意漂移且无人发现。
    /// R-173 把类型名收进枚举自身,发射方只能经此落库。
    ///
    /// 注意 writer.* 保留 `orchestration.` 前缀:设计文档写的是 `writer.queued`,
    /// 但既有 state.db 里已落的行是 `orchestration.writer.queued`,回放要认旧数据,
    /// 以磁盘上的既成事实为准。
    pub fn event_type(&self) -> &'static str {
        match self {
            OrchestrationEvent::WriterQueued { .. } => "orchestration.writer.queued",
            OrchestrationEvent::WriterAcquired { .. } => "orchestration.writer.acquired",
            OrchestrationEvent::WriterReleased { .. } => "orchestration.writer.released",
            OrchestrationEvent::WriterCancelled { .. } => "orchestration.writer.cancelled",
            OrchestrationEvent::WriterRecovered { .. } => "orchestration.writer.recovered",
            OrchestrationEvent::PhaseChanged { .. } => "orchestration.phase_changed",
            OrchestrationEvent::AgentStarted { .. } => "orchestration.agent_started",
            OrchestrationEvent::AgentCompleted { .. } => "orchestration.agent_completed",
            OrchestrationEvent::AgentFailed { .. } => "orchestration.agent_failed",
            OrchestrationEvent::BarrierReached { .. } => "orchestration.barrier_reached",
            OrchestrationEvent::BarrierTimedOut { .. } => "orchestration.barrier_timed_out",
        }
    }

    /// 落库 payload——与 [`Self::event_type`] 同一出口。
    /// writer.queued/acquired/released 的字段是 R-171 既有落库形状的超集,
    /// 旧行仍可按同样的键回放。
    pub fn payload(&self) -> serde_json::Value {
        match self {
            OrchestrationEvent::WriterQueued {
                project_root,
                run_id,
                process_id,
                reason,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
                "process_id": process_id,
                "reason": reason,
            }),
            OrchestrationEvent::WriterAcquired {
                project_root,
                run_id,
                process_id,
            }
            | OrchestrationEvent::WriterReleased {
                project_root,
                run_id,
                process_id,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
                "process_id": process_id,
            }),
            OrchestrationEvent::WriterCancelled {
                project_root,
                run_id,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
            }),
            OrchestrationEvent::WriterRecovered {
                project_root,
                run_id,
                reason,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
                "reason": reason,
            }),
            OrchestrationEvent::PhaseChanged {
                project_root,
                run_id,
                phase,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
                "phase": phase,
            }),
            OrchestrationEvent::AgentStarted {
                project_root,
                run_id,
                agent_name,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
                "agent_name": agent_name,
            }),
            OrchestrationEvent::AgentCompleted {
                project_root,
                run_id,
                agent_name,
                ok,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
                "agent_name": agent_name,
                "ok": ok,
            }),
            OrchestrationEvent::AgentFailed {
                project_root,
                run_id,
                agent_name,
                reason,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
                "agent_name": agent_name,
                "reason": reason,
            }),
            OrchestrationEvent::BarrierReached {
                project_root,
                run_id,
                barrier,
                agent_count,
                completed,
                failed,
                timed_out,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
                "barrier": barrier.as_str(),
                "agent_count": agent_count,
                "completed": completed,
                "failed": failed,
                "timed_out": timed_out,
            }),
            OrchestrationEvent::BarrierTimedOut {
                project_root,
                run_id,
                barrier,
                waited_secs,
            } => serde_json::json!({
                "project_root": project_root.display().to_string(),
                "run_id": run_id,
                "barrier": barrier.as_str(),
                "waited_secs": waited_secs,
            }),
        }
    }
}

/// 项目级执行协调器接口。首个实现由桌面端 AppState 按规范化主根共享;
/// CLI 使用单运行实现;未来多 OS 进程再换文件锁/持久 lease 实现。
#[async_trait::async_trait]
pub trait ProjectExecutionCoordinator: Send + Sync {
    /// 申请只读槽(勘察/复核阶段并行子代理用)。
    async fn acquire_read_slot(&self, request: ReadSlotRequest) -> Result<ReadPermit, String>;
    /// 申请写租约。权限询问必须发生在调用此方法之前;拿到租约后跨工具调用持有,
    /// 直到运行结束/取消/失败收尾统一释放。
    async fn acquire_writer_lease(
        &self,
        request: WriterLeaseRequest,
    ) -> Result<WriterLease, String>;
    /// 取消排队中的写申请(等待者收到确定终态)。
    fn cancel_waiter(&self, run_id: &str);
    /// 快照(活动面板/事件消费)。
    fn snapshot(&self, project_root: &Path) -> CoordinatorSnapshot;
}

// ---------------------------------------------------------------------------
// R-173 阶段编排契约
//
// 装配是**可选的**:不构造编排对象的调用方(手动一问一答、CLI 单运行)行为
// 与引入前逐字节相同——本节全是新增类型,没有任何一处改变既有执行路径。
// 阶段流水线只在自举/自主推进轮由调用方显式装配(2026-08-10 用户定调)。
// ---------------------------------------------------------------------------

/// 七阶段(设计文档「阶段契约」表)。`Finished` 是收尾终态,不在表内,
/// 用于让状态机有确定出口。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Baseline,
    Scouting,
    /// 汇总屏障之后、取写租约之前。writer 只能从这里启动——
    /// 迁移表里没有 `Scouting → Implementation`,这就是不变量 2 的机械形态。
    Synthesis,
    Implementation,
    Integration,
    Review,
    Fixup,
    Finished,
}

impl Phase {
    /// 全部阶段(穷举测试用:迁移表必须对 8×8 组合有确定答案)。
    pub const ALL: [Phase; 8] = [
        Phase::Baseline,
        Phase::Scouting,
        Phase::Synthesis,
        Phase::Implementation,
        Phase::Integration,
        Phase::Review,
        Phase::Fixup,
        Phase::Finished,
    ];

    /// 合法迁移表——**唯一真源**,非法迁移由编排对象返回
    /// [`PhaseError::IllegalTransition`] 挡下。
    ///
    /// 表里刻意没有的边,每一条都对应一个不变量:
    /// - 无 `Scouting → Implementation`:勘察必须先经汇总屏障(不变量 2)。
    /// - 无 `Implementation → Review`:复核必须在集成之后、且经复核屏障(不变量 9)。
    /// - 无 `Fixup → Review`:修正后不再开新一轮复核,避免无界循环。
    ///
    /// 异常收尾(停止/取消/panic)不走本表——那是编排对象上另一个显式命名的
    /// abort 操作(不变量 7),不是"碰巧合法"的迁移。
    pub fn can_transition_to(self, next: Phase) -> bool {
        use Phase::*;
        matches!(
            (self, next),
            (Baseline, Scouting)
                | (Scouting, Synthesis)
                | (Synthesis, Implementation)
                | (Synthesis, Finished)
                | (Implementation, Integration)
                | (Integration, Review)
                | (Integration, Finished)
                | (Review, Fixup)
                | (Review, Finished)
                | (Fixup, Finished)
        )
    }

    /// 该阶段是否要求持有写租约(不变量 4:租约覆盖实现与集成,不在两次工具调用之间切换)。
    pub fn requires_writer_lease(self) -> bool {
        matches!(
            self,
            Phase::Implementation | Phase::Integration | Phase::Fixup
        )
    }

    /// 该阶段是否允许派发只读子代理(不变量 5:writer 阶段禁用 task)。
    pub fn allows_read_agents(self) -> bool {
        matches!(self, Phase::Scouting | Phase::Review)
    }

    /// 该阶段的执行策略。
    ///
    /// **R-182 内容①:写阶段不再强制普通工具全程串行。** 原口径是「写阶段
    /// max in-flight = 1」,而单轮内互不冲突的工具本来就该并发——冲突判定早已由
    /// 每个工具自己声明的 [`crate::ToolConcurrency`] 承担:写工具一律
    /// `write_worktree(ctx)`,同一棵树上的两次写自然互斥,读工具 `shared_worktree`
    /// 之间无冲突。再加一层「整个阶段串行」是重复且过严的强制,代价是每一轮
    /// 读文件都得排队。
    ///
    /// `ReadParallelWriteSerial` **不删**:它仍是一个可显式设置的策略,
    /// 想把某条运行整体收紧回串行时直接设它即可(R-182 边界:接口保留)。
    pub fn execution_policy(self) -> ExecutionPolicy {
        ExecutionPolicy::Default
    }

    /// 事件 payload 里的阶段名(与 serde 表示一致)。
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Baseline => "baseline",
            Phase::Scouting => "scouting",
            Phase::Synthesis => "synthesis",
            Phase::Implementation => "implementation",
            Phase::Integration => "integration",
            Phase::Review => "review",
            Phase::Fixup => "fixup",
            Phase::Finished => "finished",
        }
    }
}

/// 屏障种类。两道屏障语义不同:汇总屏障挡 writer 启动,复核屏障挡 review 启动。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarrierKind {
    /// 汇总屏障:全部勘察任务进入终态前 writer 不得启动(不变量 2)。
    Synthesis,
    /// 复核屏障:writer 释放租约后复核才启动(不变量 9)。
    Review,
}

impl BarrierKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BarrierKind::Synthesis => "synthesis",
            BarrierKind::Review => "review",
        }
    }
}

/// 单个勘察/复核任务的终态。
///
/// **三个变体全是终态**——这是类型层面的「屏障不永久挂起」:没有
/// `Running`/`Pending` 变体可以表达,任务一旦被记录进屏障就必然已收敛。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoutOutcome {
    Completed,
    Failed(String),
    TimedOut { after_secs: u64 },
}

impl ScoutOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            ScoutOutcome::Completed => "completed",
            ScoutOutcome::Failed(_) => "failed",
            ScoutOutcome::TimedOut { .. } => "timed_out",
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, ScoutOutcome::Completed)
    }
}

/// 屏障结果:全部任务进入终态后的统计。
///
/// 计数只能经 [`Self::record`] 更新,所以计数与 `outcomes` 由构造方式保证一致,
/// 不会出现"统计说 3 成功、明细只有 2 条"的漂移。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarrierOutcome {
    pub barrier: BarrierKind,
    /// 屏障开始时登记的任务总数。
    pub agent_count: usize,
    pub completed: usize,
    pub failed: usize,
    pub timed_out: usize,
    /// 屏障自身触顶上界(而非等到全部任务自然终态)。
    pub barrier_timed_out: bool,
    pub outcomes: Vec<(String, ScoutOutcome)>,
}

impl BarrierOutcome {
    pub fn new(barrier: BarrierKind, agent_count: usize) -> Self {
        BarrierOutcome {
            barrier,
            agent_count,
            completed: 0,
            failed: 0,
            timed_out: 0,
            barrier_timed_out: false,
            outcomes: Vec::new(),
        }
    }

    /// 记录一个任务的终态。计数与明细同步更新,不可分。
    pub fn record(&mut self, agent_name: impl Into<String>, outcome: ScoutOutcome) {
        match &outcome {
            ScoutOutcome::Completed => self.completed += 1,
            ScoutOutcome::Failed(_) => self.failed += 1,
            ScoutOutcome::TimedOut { .. } => self.timed_out += 1,
        }
        self.outcomes.push((agent_name.into(), outcome));
    }

    /// 全部登记任务都已进入终态。
    pub fn all_terminal(&self) -> bool {
        self.outcomes.len() >= self.agent_count
    }

    /// 反静默降级(2026-08-10 用户裁决②):失败/超时/零结果**必须让模型知道**,
    /// 不能把空勘察当成"没查到东西"静默传下去——那正是 D-004 同族的静默降级。
    ///
    /// 返回 `None` 表示全部成功、无需额外告知;返回 `Some` 时调用方必须把这段
    /// 文本注入下一段模型输入。
    pub fn model_notice(&self) -> Option<String> {
        if self.agent_count == 0 {
            return None;
        }
        if self.failed == 0 && self.timed_out == 0 && !self.barrier_timed_out {
            return None;
        }
        let scope = match self.barrier {
            BarrierKind::Synthesis => "勘察",
            BarrierKind::Review => "复核",
        };
        let mut notice = if self.completed == 0 {
            format!(
                "(system) {scope}阶段 {} 个任务全部未产出结果(失败 {},超时 {})。\
                 你**没有**拿到任何{scope}结论,不要假装已经了解现状:\
                 要么在本阶段用自己的只读工具补查,要么明确说出缺的是什么。",
                self.agent_count, self.failed, self.timed_out
            )
        } else {
            format!(
                "(system) {scope}阶段 {} 个任务中 {} 个未产出结果(失败 {},超时 {})。\
                 下列范围没有被{scope}到,据此判断时要说明这一点:",
                self.agent_count,
                self.agent_count - self.completed,
                self.failed,
                self.timed_out
            )
        };
        for (agent, outcome) in &self.outcomes {
            match outcome {
                ScoutOutcome::Completed => {}
                ScoutOutcome::Failed(reason) => {
                    notice.push_str(&format!("\n- {agent}: 失败({reason})"));
                }
                ScoutOutcome::TimedOut { after_secs } => {
                    notice.push_str(&format!("\n- {agent}: 超时({after_secs}s 未返回)"));
                }
            }
        }
        if self.barrier_timed_out {
            notice.push_str("\n- 屏障自身触顶上界:上列终态可能不完整。");
        }
        Some(notice)
    }
}

/// 阶段编排错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PhaseError {
    #[error("非法阶段迁移: {from:?} → {to:?}")]
    IllegalTransition { from: Phase, to: Phase },
    #[error("阶段 {phase:?} 要求持有写租约,但当前没有")]
    LeaseMissing { phase: Phase },
    /// 复核屏障断言失败:交出租约后本 run 仍是 writer。
    #[error("进入 {phase:?} 前写租约未真正释放")]
    LeaseStillHeld { phase: Phase },
    #[error("协调器拒绝: {0}")]
    Coordinator(String),
}

/// 编排事件汇报口。
///
/// core 不依赖 SessionStore,所以事件落库由调用方(桌面端 app)实现本 trait
/// 包装 `SessionStore::append_event`;不装配 observer 时编排对象照常工作,
/// 只是不留轨迹。
pub trait PhaseObserver: Send + Sync {
    fn observe(&self, event: &OrchestrationEvent);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_writer_policy_flag() {
        assert!(!ExecutionPolicy::Default.is_serial_writer());
        assert!(ExecutionPolicy::ReadParallelWriteSerial.is_serial_writer());
    }

    /// 迁移表穷举:8×8 全部组合逐一断言,合法边恰好是这 10 条,多一条少一条都红。
    /// 这条测试是不变量 2/9 的机械锚点——有人偷偷加一条
    /// `Scouting → Implementation` 绕过汇总屏障,这里当场失败。
    #[test]
    fn 阶段迁移表穷举_合法边恰好十条() {
        use Phase::*;
        let legal: &[(Phase, Phase)] = &[
            (Baseline, Scouting),
            (Scouting, Synthesis),
            (Synthesis, Implementation),
            (Synthesis, Finished),
            (Implementation, Integration),
            (Integration, Review),
            (Integration, Finished),
            (Review, Fixup),
            (Review, Finished),
            (Fixup, Finished),
        ];
        let mut legal_count = 0usize;
        for from in Phase::ALL {
            for to in Phase::ALL {
                let expected = legal.contains(&(from, to));
                assert_eq!(
                    from.can_transition_to(to),
                    expected,
                    "迁移 {from:?} → {to:?} 判定与表不符"
                );
                if expected {
                    legal_count += 1;
                }
            }
        }
        assert_eq!(legal_count, legal.len(), "合法边数量必须与表一致");
        // 点名断言两条最关键的缺边(不变量 2 与不变量 9)。
        assert!(
            !Scouting.can_transition_to(Implementation),
            "勘察不得直接进实现:必须先过汇总屏障"
        );
        assert!(
            !Implementation.can_transition_to(Review),
            "实现不得直接进复核:必须先集成并交出写租约"
        );
    }

    /// 阶段属性:写租约、task 可用性与执行策略三者必须互相自洽。
    ///
    /// **R-182 内容① 改写**(而非删除):原断言是「写阶段必须是串行策略」,
    /// 现在被守护的性质换成了「**没有任何阶段**再强制普通工具全程串行」——
    /// 单轮内的冲突判定归 `ToolConcurrency`,阶段不再重复施加一层过严的强制。
    /// 写阶段禁 task(不变量 5)那一半原样保留。
    #[test]
    fn 阶段属性自洽_无阶段强制串行且写阶段仍禁task() {
        for phase in Phase::ALL {
            assert!(
                !phase.execution_policy().is_serial_writer(),
                "{phase:?} 不得再靠阶段强制普通工具全程串行(R-182 内容①)"
            );
            if phase.requires_writer_lease() {
                assert!(
                    !phase.allows_read_agents(),
                    "{phase:?} 是写阶段,不得派发 task 子代理(不变量 5)"
                );
            }
        }
        // 策略枚举本身不删:显式设它仍然是串行写,留给将来重新收紧。
        assert!(ExecutionPolicy::ReadParallelWriteSerial.is_serial_writer());
        // 只读并行阶段恰好是勘察与复核两个。
        let parallel: Vec<Phase> = Phase::ALL
            .into_iter()
            .filter(|p| p.allows_read_agents())
            .collect();
        assert_eq!(parallel, vec![Phase::Scouting, Phase::Review]);
    }

    /// 事件类型名单一出口:全变体覆盖且互不重复。
    /// R-171 的漂移面(枚举一套、落库手写字符串一套)由此收掉。
    #[test]
    fn 事件类型名全变体唯一() {
        let root = std::path::PathBuf::from("C:/proj");
        let events = vec![
            OrchestrationEvent::WriterQueued {
                project_root: root.clone(),
                run_id: "r".into(),
                process_id: "p".into(),
                reason: "why".into(),
            },
            OrchestrationEvent::WriterAcquired {
                project_root: root.clone(),
                run_id: "r".into(),
                process_id: "p".into(),
            },
            OrchestrationEvent::WriterReleased {
                project_root: root.clone(),
                run_id: "r".into(),
                process_id: "p".into(),
            },
            OrchestrationEvent::WriterCancelled {
                project_root: root.clone(),
                run_id: "r".into(),
            },
            OrchestrationEvent::WriterRecovered {
                project_root: root.clone(),
                run_id: "r".into(),
                reason: "crash".into(),
            },
            OrchestrationEvent::PhaseChanged {
                project_root: root.clone(),
                run_id: "r".into(),
                phase: "scouting".into(),
            },
            OrchestrationEvent::AgentStarted {
                project_root: root.clone(),
                run_id: "r".into(),
                agent_name: "scout-a".into(),
            },
            OrchestrationEvent::AgentCompleted {
                project_root: root.clone(),
                run_id: "r".into(),
                agent_name: "scout-a".into(),
                ok: true,
            },
            OrchestrationEvent::AgentFailed {
                project_root: root.clone(),
                run_id: "r".into(),
                agent_name: "scout-b".into(),
                reason: "boom".into(),
            },
            OrchestrationEvent::BarrierReached {
                project_root: root.clone(),
                run_id: "r".into(),
                barrier: BarrierKind::Synthesis,
                agent_count: 3,
                completed: 1,
                failed: 1,
                timed_out: 1,
            },
            OrchestrationEvent::BarrierTimedOut {
                project_root: root,
                run_id: "r".into(),
                barrier: BarrierKind::Review,
                waited_secs: 1800,
            },
        ];
        let mut seen = std::collections::BTreeSet::new();
        for event in &events {
            let name = event.event_type();
            assert!(
                name.starts_with("orchestration."),
                "事件类型名必须带 orchestration. 前缀: {name}"
            );
            assert!(seen.insert(name), "事件类型名重复: {name}");
            // payload 必须是对象且带 run_id,否则回放无法归属到运行。
            let payload = event.payload();
            assert!(payload.is_object(), "{name} 的 payload 不是对象");
            assert_eq!(payload["run_id"], "r", "{name} 的 payload 缺 run_id");
            assert_eq!(
                payload["project_root"], "C:/proj",
                "{name} 的 payload 缺 project_root"
            );
        }
        assert_eq!(seen.len(), events.len());
    }

    /// 既有落库形状兼容:R-171 已写进 state.db 的三种 writer 事件,
    /// 类型名与关键字段不能变——否则历史行回放不出来。
    #[test]
    fn writer事件落库形状与r171既有行兼容() {
        let root = std::path::PathBuf::from("C:/proj");
        let queued = OrchestrationEvent::WriterQueued {
            project_root: root.clone(),
            run_id: "run_1".into(),
            process_id: "proc_1".into(),
            reason: "session writer run".into(),
        };
        assert_eq!(queued.event_type(), "orchestration.writer.queued");
        let payload = queued.payload();
        assert_eq!(payload["run_id"], "run_1");
        assert_eq!(payload["process_id"], "proc_1");
        assert_eq!(payload["project_root"], "C:/proj");

        let acquired = OrchestrationEvent::WriterAcquired {
            project_root: root.clone(),
            run_id: "run_1".into(),
            process_id: "proc_1".into(),
        };
        assert_eq!(acquired.event_type(), "orchestration.writer.acquired");
        let released = OrchestrationEvent::WriterReleased {
            project_root: root,
            run_id: "run_1".into(),
            process_id: "proc_1".into(),
        };
        assert_eq!(released.event_type(), "orchestration.writer.released");
        // R-171 的 released 行只有 run_id/process_id,新形状是超集。
        assert_eq!(released.payload()["process_id"], "proc_1");
    }

    /// 屏障统计:record 同步更新计数与明细,不会漂移。
    #[test]
    fn 屏障统计与明细同步_三终态各自归类() {
        let mut outcome = BarrierOutcome::new(BarrierKind::Synthesis, 3);
        assert!(!outcome.all_terminal());
        outcome.record("architecture_scout", ScoutOutcome::Completed);
        outcome.record("test_scout", ScoutOutcome::Failed("provider 500".into()));
        outcome.record("docs_scout", ScoutOutcome::TimedOut { after_secs: 900 });
        assert!(outcome.all_terminal());
        assert_eq!(
            (outcome.completed, outcome.failed, outcome.timed_out),
            (1, 1, 1)
        );
        assert_eq!(outcome.outcomes.len(), 3);
        let labels: Vec<&str> = outcome.outcomes.iter().map(|(_, o)| o.label()).collect();
        assert_eq!(labels, vec!["completed", "failed", "timed_out"]);
    }

    /// 反静默降级(裁决②):全成功不打扰;有失败要点名;全失败必须说"你没拿到任何结论"。
    #[test]
    fn 屏障零结果必须告知模型_不静默降级() {
        // 全成功:不打扰。
        let mut all_ok = BarrierOutcome::new(BarrierKind::Synthesis, 2);
        all_ok.record("a", ScoutOutcome::Completed);
        all_ok.record("b", ScoutOutcome::Completed);
        assert!(all_ok.model_notice().is_none());

        // 部分失败:点名缺的是哪一块。
        let mut partial = BarrierOutcome::new(BarrierKind::Synthesis, 2);
        partial.record("a", ScoutOutcome::Completed);
        partial.record("write_surface_scout", ScoutOutcome::Failed("boom".into()));
        let notice = partial.model_notice().expect("有失败必须告知模型");
        assert!(notice.contains("write_surface_scout"), "必须点名失败的任务");
        assert!(notice.contains("boom"), "必须带上失败原因");

        // 全部失败:必须明说没拿到任何结论,不能让模型以为"查过了没发现"。
        let mut none_ok = BarrierOutcome::new(BarrierKind::Synthesis, 2);
        none_ok.record("a", ScoutOutcome::Failed("x".into()));
        none_ok.record("b", ScoutOutcome::TimedOut { after_secs: 900 });
        let notice = none_ok.model_notice().expect("零结果必须告知模型");
        assert!(
            notice.contains("全部未产出结果"),
            "零结果的告知必须显式,不能只报数字: {notice}"
        );
        assert!(notice.contains("不要假装已经了解现状"), "必须阻断静默降级");

        // 复核屏障用「复核」措辞,不串词。
        let mut review = BarrierOutcome::new(BarrierKind::Review, 1);
        review.record("r", ScoutOutcome::Failed("x".into()));
        assert!(review.model_notice().unwrap().contains("复核阶段"));
    }
}
