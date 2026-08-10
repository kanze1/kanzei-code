//! 内存项目级执行协调器(R-171 批3):「并行查、串行写」的确定性状态机。
//! 契约定义在 kanzei-harness::orchestration,本模块提供首个实现——
//! 桌面端 AppState 按规范化主根共享;CLI 用单运行实例。

use kanzei_harness::orchestration::{
    CoordinatorSnapshot, OrchestrationEvent, ProjectExecutionCoordinator, ReadPermit,
    ReadSlotRequest, WriterLease, WriterLeaseRequest,
};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// 规范化主根:统一大小写与分隔符,保证跨进程/跨调用键一致。
pub fn normalize_project_root(root: &std::path::Path) -> String {
    root.to_string_lossy().replace('\\', "/").to_lowercase()
}

struct ProjectState {
    /// 当前 writer run_id(None = 无写者)。
    writer_run_id: Option<String>,
    writer_process_id: Option<String>,
    /// 排队中的写者(先到先得)。
    waiting: VecDeque<WaitingWriter>,
    /// 活跃只读代理(run_id → agent_name)。
    readers: BTreeMap<String, String>,
    /// 最近一次事件(审计可读)。
    last_event: Option<OrchestrationEvent>,
}

struct WaitingWriter {
    run_id: String,
    process_id: String,
    reason: String,
    tx: Option<oneshot::Sender<Result<WriterLease, String>>>,
}

impl ProjectState {
    fn new() -> Self {
        ProjectState {
            writer_run_id: None,
            writer_process_id: None,
            waiting: VecDeque::new(),
            readers: BTreeMap::new(),
            last_event: None,
        }
    }
}

/// 内存协调器:按项目主根分桶,每项目独立仲裁。内部共享状态,多进程句柄共用同一实例。
pub struct MemoryCoordinator {
    inner: Arc<MemoryCoordinatorInner>,
}

struct MemoryCoordinatorInner {
    projects: Mutex<BTreeMap<String, ProjectState>>,
}

impl Default for MemoryCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MemoryCoordinator {
    fn clone(&self) -> Self {
        MemoryCoordinator {
            inner: self.inner.clone(),
        }
    }
}

impl MemoryCoordinator {
    pub fn new() -> Self {
        MemoryCoordinator {
            inner: Arc::new(MemoryCoordinatorInner {
                projects: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// 供事件订阅方读取(批5 接 session_events 前,先留审计入口)。
    pub fn last_event(&self, root: &std::path::Path) -> Option<OrchestrationEvent> {
        let key = normalize_project_root(root);
        self.inner
            .projects
            .lock()
            .unwrap()
            .get(&key)
            .and_then(|s| s.last_event.clone())
    }

    /// 归还租约的统一路径:WriterLease drop 时回调这里。
    /// 唤醒下一个排队写者,保证租约区间不重叠、FIFO 顺序可审计。
    fn release_writer(&self, root_key: &str, run_id: &str) {
        let mut guard = self.inner.projects.lock().unwrap();
        let Some(state) = guard.get_mut(root_key) else {
            return;
        };
        if state.writer_run_id.as_deref() != Some(run_id) {
            return; // 非持有者(已释放或不属于本项目)
        }
        state.writer_run_id = None;
        state.writer_process_id = None;
        if let Some(w) = state.waiting.pop_front() {
            let _wake_reason = &w.reason; // 审计:唤醒排队写者的申请原因。
            let lease = WriterLease::with_release(
                PathBuf::from(root_key),
                w.run_id.clone(),
                w.process_id.clone(),
                {
                    let key = root_key.to_string();
                    let coord = self.clone();
                    move |released_run_id| coord.release_writer(&key, released_run_id)
                },
            );
            state.writer_run_id = Some(w.run_id.clone());
            state.writer_process_id = Some(w.process_id.clone());
            state.last_event = Some(OrchestrationEvent::WriterAcquired {
                project_root: PathBuf::from(root_key),
                run_id: w.run_id.clone(),
                process_id: w.process_id.clone(),
            });
            if let Some(tx) = w.tx {
                let _ = tx.send(Ok(lease));
            }
            return;
        }
        state.last_event = Some(OrchestrationEvent::WriterReleased {
            project_root: PathBuf::from(root_key),
            run_id: run_id.to_string(),
            process_id: state.writer_process_id.clone().unwrap_or_default(),
        });
    }

    /// 归还读槽(批6):按 agent_name 移除登记并记录 AgentCompleted 事件。
    /// 由 ReadPermit 的 drop 回调调用——子代理结束(含失败/取消)即回收。
    fn release_reader(&self, root_key: &str, agent_name: &str) {
        let mut guard = self.inner.projects.lock().unwrap();
        let Some(state) = guard.get_mut(root_key) else {
            return;
        };
        let removed = {
            let run_id = state
                .readers
                .iter()
                .find(|(_, name)| name.as_str() == agent_name)
                .map(|(rid, _)| rid.clone());
            if let Some(rid) = &run_id {
                state.readers.remove(rid);
            }
            run_id
        };
        if let Some(run_id) = removed {
            state.last_event = Some(OrchestrationEvent::AgentCompleted {
                project_root: PathBuf::from(root_key),
                run_id,
                agent_name: agent_name.to_string(),
                ok: true,
            });
        }
    }
}

#[async_trait::async_trait]
impl ProjectExecutionCoordinator for MemoryCoordinator {
    async fn acquire_read_slot(&self, request: ReadSlotRequest) -> Result<ReadPermit, String> {
        let key = normalize_project_root(&request.project_root);
        let agent_name = request.agent_name.clone();
        let run_id = request.run_id.clone();
        {
            let mut guard = self.inner.projects.lock().unwrap();
            let state = guard.entry(key.clone()).or_insert_with(ProjectState::new);
            state.readers.insert(run_id.clone(), agent_name.clone());
            state.last_event = Some(OrchestrationEvent::AgentStarted {
                project_root: request.project_root.clone(),
                run_id,
                agent_name: agent_name.clone(),
            });
        }
        // R-171 批6:读槽带释放回调——子代理结束即从快照消失(active_readers
        // 不再永久累积),保证「并行查」的身份可见且可回收。
        let permit = ReadPermit::with_release(request.project_root.clone(), agent_name.clone(), {
            let key = key.clone();
            let coord = self.clone();
            move |released_agent| coord.release_reader(&key, released_agent)
        });
        Ok(permit)
    }

    async fn acquire_writer_lease(
        &self,
        request: WriterLeaseRequest,
    ) -> Result<WriterLease, String> {
        let key = normalize_project_root(&request.project_root);
        // 排队决策与事件写入在同步块内完成;await 在 guard 释放之后,
        // 避免 std::sync::MutexGuard 跨 await 使 future 不 Send。
        let rx = {
            let mut guard = self.inner.projects.lock().unwrap();
            let state = guard.entry(key.clone()).or_insert_with(ProjectState::new);
            if state.writer_run_id.is_none() {
                state.writer_run_id = Some(request.run_id.clone());
                state.writer_process_id = Some(request.process_id.clone());
                state.last_event = Some(OrchestrationEvent::WriterAcquired {
                    project_root: request.project_root.clone(),
                    run_id: request.run_id.clone(),
                    process_id: request.process_id.clone(),
                });
                let lease = WriterLease::with_release(
                    request.project_root,
                    request.run_id,
                    request.process_id,
                    {
                        let key = key.clone();
                        let coord = self.clone();
                        move |released_run_id| coord.release_writer(&key, released_run_id)
                    },
                );
                return Ok(lease);
            }
            // 有写者:排队等待,先到先得。
            let (tx, rx) = oneshot::channel();
            state.waiting.push_back(WaitingWriter {
                run_id: request.run_id.clone(),
                process_id: request.process_id.clone(),
                reason: request.reason.clone(),
                tx: Some(tx),
            });
            state.last_event = Some(OrchestrationEvent::WriterQueued {
                project_root: request.project_root.clone(),
                run_id: request.run_id.clone(),
                process_id: request.process_id.clone(),
                reason: request.reason.clone(),
            });
            rx
        };
        match rx.await {
            Ok(Ok(lease)) => Ok(lease),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("writer lease cancelled: sender dropped".into()),
        }
    }

    fn cancel_waiter(&self, run_id: &str) {
        let mut guard = self.inner.projects.lock().unwrap();
        for (key, state) in guard.iter_mut() {
            let mut idx = 0usize;
            while idx < state.waiting.len() {
                let removed = {
                    let w = &mut state.waiting[idx];
                    if w.run_id == run_id {
                        if let Some(tx) = w.tx.take() {
                            let _ = tx.send(Err("cancelled by user".into()));
                        }
                        // 记录取消事件(审计:排队中的写者被取消)。
                        state.last_event = Some(OrchestrationEvent::WriterCancelled {
                            project_root: PathBuf::from(key),
                            run_id: run_id.to_string(),
                        });
                        true
                    } else {
                        false
                    }
                };
                if removed {
                    state.waiting.remove(idx);
                } else {
                    idx += 1;
                }
            }
        }
    }

    fn snapshot(&self, project_root: &Path) -> CoordinatorSnapshot {
        let key = normalize_project_root(project_root);
        let guard = self.inner.projects.lock().unwrap();
        let state = guard.get(&key);
        match state {
            Some(s) => CoordinatorSnapshot {
                project_root: project_root.to_path_buf(),
                writer: s.writer_process_id.clone(),
                writer_run_id: s.writer_run_id.clone(),
                waiting_writers: s.waiting.iter().map(|w| w.run_id.clone()).collect(),
                active_readers: s.readers.values().cloned().collect(),
            },
            None => CoordinatorSnapshot {
                project_root: project_root.to_path_buf(),
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::orchestration::ProjectExecutionCoordinator;
    use std::time::Duration;

    fn req(run_id: &str, root: &std::path::Path) -> WriterLeaseRequest {
        WriterLeaseRequest {
            project_root: root.to_path_buf(),
            run_id: run_id.into(),
            process_id: format!("proc-{run_id}"),
            reason: "test".into(),
        }
    }

    #[tokio::test]
    async fn writer_lease_is_exclusive_and_fifo() {
        let dir = std::env::temp_dir().join(format!("kz-orch-{}", std::process::id()));
        let coord = MemoryCoordinator::new();
        // 第一个写者立即拿到。
        let lease_a = coord
            .acquire_writer_lease(req("run-a", &dir))
            .await
            .unwrap();
        assert_eq!(lease_a.run_id, "run-a");
        let snap = coord.snapshot(&dir);
        assert_eq!(snap.writer_run_id.as_deref(), Some("run-a"));
        assert!(snap.waiting_writers.is_empty());
        // 第二个写者排队。
        let coord2 = coord.clone();
        let dir2 = dir.clone();
        let task = tokio::spawn(async move {
            let lease = coord2
                .acquire_writer_lease(req("run-b", &dir2))
                .await
                .unwrap();
            assert_eq!(lease.run_id, "run-b");
            lease // 送回,避免闭包结束时 drop 释放租约
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let snap = coord.snapshot(&dir);
        assert_eq!(snap.writer_run_id.as_deref(), Some("run-a"));
        assert_eq!(snap.waiting_writers, vec!["run-b".to_string()]);
        // 释放 a → b 拿到,区间不重叠。
        drop(lease_a);
        let lease_b = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("run-b 应被唤醒")
            .unwrap();
        let snap = coord.snapshot(&dir);
        assert_eq!(snap.writer_run_id.as_deref(), Some("run-b"));
        assert!(snap.waiting_writers.is_empty());
        drop(lease_b);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn read_slots_are_parallel_and_snapshot_visible() {
        let dir = std::env::temp_dir().join(format!("kz-orch-read-{}", std::process::id()));
        let coord = MemoryCoordinator::new();
        let a = coord
            .acquire_read_slot(ReadSlotRequest {
                project_root: dir.clone(),
                run_id: "r1".into(),
                process_id: "p1".into(),
                agent_name: "scout-a".into(),
            })
            .await
            .unwrap();
        let b = coord
            .acquire_read_slot(ReadSlotRequest {
                project_root: dir.clone(),
                run_id: "r2".into(),
                process_id: "p2".into(),
                agent_name: "scout-b".into(),
            })
            .await
            .unwrap();
        assert_eq!(a.agent_name, "scout-a");
        assert_eq!(b.agent_name, "scout-b");
        let snap = coord.snapshot(&dir);
        assert_eq!(snap.active_readers.len(), 2);
        assert!(snap.active_readers.contains(&"scout-a".to_string()));
        assert!(snap.active_readers.contains(&"scout-b".to_string()));
        // R-171 批6:读槽 RAII 释放——子代理结束即从快照消失,不永久累积。
        drop(a);
        let snap = coord.snapshot(&dir);
        assert_eq!(snap.active_readers.len(), 1);
        assert!(snap.active_readers.contains(&"scout-b".to_string()));
        drop(b);
        let snap = coord.snapshot(&dir);
        assert!(snap.active_readers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn cancel_waiter_gives_deterministic_terminal_state() {
        let dir = std::env::temp_dir().join(format!("kz-orch-cancel-{}", std::process::id()));
        let coord = MemoryCoordinator::new();
        let _lease = coord
            .acquire_writer_lease(req("run-a", &dir))
            .await
            .unwrap();
        let coord2 = coord.clone();
        let dir2 = dir.clone();
        let task = tokio::spawn(async move {
            let r = coord2.acquire_writer_lease(req("run-b", &dir2)).await;
            assert!(r.is_err(), "排队中的写者被取消应收到错误终态");
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        coord.cancel_waiter("run-b");
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("取消后等待者应结束")
            .unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn lease_drop_on_panic_path_releases_for_next_writer() {
        let dir = std::env::temp_dir().join(format!("kz-orch-panic-{}", std::process::id()));
        let coord = MemoryCoordinator::new();
        {
            // 模拟 panic 收尾:持有者作用域结束(等价于 unwinding 时 drop)。
            let _lease = coord
                .acquire_writer_lease(req("run-panic", &dir))
                .await
                .unwrap();
        }
        // 租约已随作用域结束释放,下一个写者可立即拿到。
        let lease2 = coord
            .acquire_writer_lease(req("run-next", &dir))
            .await
            .unwrap();
        assert_eq!(lease2.run_id, "run-next");
        std::fs::remove_dir_all(&dir).ok();
    }
}
