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
            OrchestrationEvent::AgentStarted { .. } => "orchestration.agent_started",
            OrchestrationEvent::AgentCompleted { .. } => "orchestration.agent_completed",
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
        }
    }
}

/// 项目级执行协调器接口。首个实现由桌面端 AppState 按规范化主根共享;
/// CLI 使用单运行实现;未来多 OS 进程再换文件锁/持久 lease 实现。
#[async_trait::async_trait]
// async_trait generates must_use on the already-must-use boxed Future.
#[allow(clippy::double_must_use)]
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

/// Shared with tools that must acquire another worktree's write scope.
#[derive(Clone)]
pub struct ExecutionCoordinator(pub std::sync::Arc<dyn ProjectExecutionCoordinator>);

impl std::fmt::Debug for ExecutionCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionCoordinator")
            .finish_non_exhaustive()
    }
}

/// 编排事件汇报口。
///
/// core 不依赖 SessionStore,所以事件落库由调用方(桌面端 app)实现本 trait
/// 包装 `SessionStore::append_event`;不装配 observer 时编排对象照常工作,
/// 只是不留轨迹。
pub trait CoordinationObserver: Send + Sync {
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
}
