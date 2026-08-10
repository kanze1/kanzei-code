//! 项目级执行编排契约(R-171):「并行查、串行写」的机械强制层。
//!
//! 本模块只定义契约(策略、租约请求/许可、协调器 trait、事件负载),
//! 不承载具体实现——内存实现放 kanzei-core,未来 OS 进程锁实现换插不换契约。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

/// 写租约申请。规范化 project_root 是跨进程仲裁键;
/// run_id/process_id 是租约归属与审计身份。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriterLeaseRequest {
    pub project_root: PathBuf,
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
    release: Option<std::sync::Arc<dyn Fn(&str) + Send + Sync>>,
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
#[derive(Debug)]
pub struct ReadPermit {
    pub project_root: PathBuf,
    pub agent_name: String,
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
    BarrierReached {
        project_root: PathBuf,
        run_id: String,
        agent_count: usize,
    },
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
    fn snapshot(&self, project_root: &PathBuf) -> CoordinatorSnapshot;
}

#[cfg(test)]
mod tests {
    use super::ExecutionPolicy;

    #[test]
    fn serial_writer_policy_flag() {
        assert!(!ExecutionPolicy::Default.is_serial_writer());
        assert!(ExecutionPolicy::ReadParallelWriteSerial.is_serial_writer());
    }
}
