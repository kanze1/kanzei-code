//! 内存项目级执行协调器(R-171 批3):「并行查、串行写」的确定性状态机。
//! 契约定义在 kanzei-harness::orchestration,本模块提供首个实现——
//! 桌面端 AppState 按规范化主根共享;CLI 用单运行实例。

use kanzei_harness::orchestration::{
    CoordinatorSnapshot, OrchestrationEvent, PhaseObserver, ProjectExecutionCoordinator,
    ReadPermit, ReadSlotRequest, WriterLease, WriterLeaseRequest,
};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// 规范化主根(写仲裁桶键):剥 Windows 扩展长度前缀、统一分隔符与大小写、去尾分隔符。
///
/// **前缀必须剥**(R-173 批4 修,依据由 R-141 批2 整理):原实现只做
/// `replace('\\', "/")` + `to_lowercase()`,于是同一个项目按调用方不同落进两个桶——
///
/// - `//?/c:/users/…`:worktree 创建/合并/放弃走 `app/state.rs` 的
///   `normalized_project_root`,内含 `std::fs::canonicalize`,产出带 `\\?\` 的形态;
/// - `c:/users/…`:主对话 writer 租约、quick_req、defect_review 走裸
///   `discover_project_root`。
///
/// 后果是 R-171 验收⑤「worktree 写入无法绕过协调器」在 Windows 上**实际被绕过**:
/// 两组写入口互相看不见对方的租约。阶段编排管得住同一个桶里的顺序,管不住两个桶,
/// 所以这个洞必须在**这一个点**上堵死——在任一调用方单独改成 canonical 只是把
/// 一个错桶换成另一个错桶(那样 run.rs 与 processes.rs 对上了,subagents.rs 又被甩出去)。
///
/// 与 `kanzei-harness` 的 `dir_key` 不同,这里的大小写/分隔符归一**不分平台**:
/// 本键只活在内存里、不进哈希也不落库,没有历史兼容包袱;在 Linux 上过度归一
/// 最多让两个仅大小写不同的项目多串行一会儿,而在 Windows 上归一不足是会丢写序的
/// 真窟窿。两种错误的代价不对称,所以往严的一边取。
pub fn normalize_project_root(root: &std::path::Path) -> String {
    let raw = root.to_string_lossy();
    // 前缀是反斜杠形态,必须在统一分隔符**之前**剥。
    let stripped = raw
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| raw.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| raw.to_string());
    stripped
        .replace('\\', "/")
        .to_lowercase()
        .trim_end_matches('/')
        .to_string()
}

struct ProjectState {
    /// 当前 writer run_id(None = 无写者)。
    writer_run_id: Option<String>,
    writer_process_id: Option<String>,
    /// 排队中的写者(先到先得)。
    waiting: VecDeque<WaitingWriter>,
    /// 活跃只读代理(run_id → agent_name)。
    readers: BTreeMap<String, String>,
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
        }
    }
}

/// 写租约申请的即时结果:当场拿到,或进队列等唤醒。
enum LeaseOutcome {
    Granted(WriterLease),
    Queued(oneshot::Receiver<Result<WriterLease, String>>),
}

/// 内存协调器:按项目主根分桶,每项目独立仲裁。内部共享状态,多进程句柄共用同一实例。
pub struct MemoryCoordinator {
    inner: Arc<MemoryCoordinatorInner>,
}

struct MemoryCoordinatorInner {
    projects: Mutex<BTreeMap<String, ProjectState>>,
    /// R-173:事件汇报口。None = 只记内存快照(CLI/测试);桌面端装配后
    /// 写租约的排队/取得/释放全部进 session_events(批5 接线点)。
    observer: Option<Arc<dyn PhaseObserver>>,
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
                observer: None,
            }),
        }
    }

    /// R-173:带事件汇报口的协调器。观察者由调用方实现(桌面端写 session_events)。
    pub fn with_observer(observer: Arc<dyn PhaseObserver>) -> Self {
        MemoryCoordinator {
            inner: Arc::new(MemoryCoordinatorInner {
                projects: Mutex::new(BTreeMap::new()),
                observer: Some(observer),
            }),
        }
    }

    /// 把待发事件交给观察者。
    ///
    /// **必须在释放 projects 锁之后调用**——观察者是外部代码(桌面端会写 SQLite),
    /// 在持锁状态下回调有重入死锁的风险。
    fn notify(&self, events: Vec<OrchestrationEvent>) {
        let Some(observer) = self.inner.observer.as_ref() else {
            return;
        };
        for event in &events {
            observer.observe(event);
        }
    }

    /// 归还租约的统一路径:WriterLease drop 时回调这里。
    /// 唤醒下一个排队写者,保证租约区间不重叠、FIFO 顺序可审计。
    ///
    /// **D-271:交接必须在锁外投递。** `oneshot::Sender::send` 在接收端已丢弃时
    /// 把 `WriterLease` **原样退回**,而 `WriterLease::drop` 的回调正是本函数——
    /// 在临界区内 drop 退回的租约会二次锁同一把非重入 `std::sync::Mutex`,
    /// 该项目的写仲裁自此永久挂死,只能重启进程。触发条件是「排队者被取消/超时
    /// 后持有者才释放」,任务级并行下这正是常态。所以锁内只把 `(tx, lease)` 收进
    /// 局部变量(与 `pending` 事件同一手法),锁释放后再 send。
    fn release_writer(&self, root_key: &str, run_id: &str) {
        let mut pending = Vec::new();
        let mut handoff: Option<(oneshot::Sender<Result<WriterLease, String>>, WriterLease)> = None;
        {
            let mut guard = self.inner.projects.lock().unwrap();
            let Some(state) = guard.get_mut(root_key) else {
                return;
            };
            if state.writer_run_id.as_deref() != Some(run_id) {
                return; // 非持有者(已释放或不属于本项目)
            }
            // R-173 修缺陷 B:离任者身份必须在清空**之前**取出。原实现先把
            // writer_process_id 置 None,再在下面读同一字段填进 Released 事件,
            // 于是 process_id 恒为空串——审计里看不出是谁交的权。
            let released_process_id = state.writer_process_id.clone().unwrap_or_default();
            state.writer_run_id = None;
            state.writer_process_id = None;
            // R-173 修缺陷 A:Released 无条件先发。原实现把它写在交接分支**之后**,
            // 一旦有排队者就提前 return,离任 writer 的 Released 永远发不出来——
            // 审计流成了 A.acquired → B.acquired,中间断档,看不出 A 何时交的权、
            // 两段写区间的边界在哪。
            pending.push(OrchestrationEvent::WriterReleased {
                project_root: PathBuf::from(root_key),
                run_id: run_id.to_string(),
                process_id: released_process_id,
            });
            // 取到**第一个还带发送端**的排队者。`tx` 已被 `cancel_waiter` 取走的
            // 条目理论上同批就从队列移除了,万一残留必须跳过而不是就此收手——
            // 否则写者位空着、后面的排队者再也等不到唤醒。
            while let Some(w) = state.waiting.pop_front() {
                let Some(tx) = w.tx else { continue };
                // 审计:唤醒排队写者的申请原因。
                let _wake_reason = &w.reason;
                // 租约只在确定有人接手时才构造:在锁内构造又在锁内 drop,
                // 同样会撞上 D-271 的重入死锁。
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
                pending.push(OrchestrationEvent::WriterAcquired {
                    project_root: PathBuf::from(root_key),
                    run_id: w.run_id.clone(),
                    process_id: w.process_id.clone(),
                });
                handoff = Some((tx, lease));
                break;
            }
        }
        self.notify(pending);
        if let Some((tx, lease)) = handoff {
            // 接收端已丢弃(排队者被取消/abort/超时)时租约原样退回,必须**显式**落地:
            // 此刻已不持锁,drop 回调重入 release_writer 是安全的,租约会顺次传给
            // 再下一个排队者。用 `let _ =` 吞掉则写权停在一个不存在的持有者身上,
            // 后续所有 acquire 永久排队——这正是 D-271 的失败形态。
            if let Err(Ok(orphan)) = tx.send(Ok(lease)) {
                drop(orphan);
            }
        }
    }

    /// 归还读槽(批6):按 agent_name 移除登记并记录 AgentCompleted 事件。
    /// 由 ReadPermit 的 drop 回调调用——子代理结束(含失败/取消)即回收。
    /// R-173 批4.5 修:按 **run_id** 回收,不按 agent_name。
    ///
    /// 原实现在 `readers` 里找第一条 value == agent_name 的记录删掉。同轮并行的
    /// N 个子代理 agent_name **全部相同**(都是 agent 定义名 `explore`),于是
    /// "随便挑一条同名的删掉":个数对得上,身份是错的——AgentCompleted 会挂在
    /// 另一个还在跑的子代理的 run_id 上。R-171 时这条路不可达所以没暴露,
    /// 批4.5 让并行查恢复后它立刻变成真实的审计错误。
    fn release_reader(&self, root_key: &str, run_id: &str) {
        let mut pending = Vec::new();
        {
            let mut guard = self.inner.projects.lock().unwrap();
            let Some(state) = guard.get_mut(root_key) else {
                return;
            };
            if let Some(agent_name) = state.readers.remove(run_id) {
                pending.push(OrchestrationEvent::AgentCompleted {
                    project_root: PathBuf::from(root_key),
                    run_id: run_id.to_string(),
                    agent_name,
                    ok: true,
                });
            }
        }
        self.notify(pending);
    }
}

#[async_trait::async_trait]
impl ProjectExecutionCoordinator for MemoryCoordinator {
    async fn acquire_read_slot(&self, request: ReadSlotRequest) -> Result<ReadPermit, String> {
        // R-173 验收④的机械依据:本方法**全程不读 `writer_run_id`**,没有任何
        // 排队/阻塞分支,唯一返回路径是 Ok——所以 writer 活跃时只读勘察照常进行。
        // 与 acquire_writer_lease 的排队路径结构上完全独立。
        let key = normalize_project_root(&request.project_root);
        let agent_name = request.agent_name.clone();
        let run_id = request.run_id.clone();
        let mut pending = Vec::new();
        {
            let mut guard = self.inner.projects.lock().unwrap();
            let state = guard.entry(key.clone()).or_insert_with(ProjectState::new);
            state.readers.insert(run_id.clone(), agent_name.clone());
            pending.push(OrchestrationEvent::AgentStarted {
                project_root: request.project_root.clone(),
                run_id,
                agent_name: agent_name.clone(),
            });
        }
        self.notify(pending);
        // R-171 批6:读槽带释放回调——子代理结束即从快照消失(active_readers
        // 不再永久累积),保证「并行查」的身份可见且可回收。
        let permit = ReadPermit::with_release(
            request.project_root.clone(),
            request.run_id.clone(),
            agent_name.clone(),
            {
                let key = key.clone();
                let coord = self.clone();
                move |released_run_id| coord.release_reader(&key, released_run_id)
            },
        );
        Ok(permit)
    }

    async fn acquire_writer_lease(
        &self,
        request: WriterLeaseRequest,
    ) -> Result<WriterLease, String> {
        let key = normalize_project_root(&request.project_root);
        // 排队决策与事件写入在同步块内完成;await 在 guard 释放之后,
        // 避免 std::sync::MutexGuard 跨 await 使 future 不 Send。
        let mut pending = Vec::new();
        let outcome = {
            let mut guard = self.inner.projects.lock().unwrap();
            let state = guard.entry(key.clone()).or_insert_with(ProjectState::new);
            if state.writer_run_id.is_none() {
                state.writer_run_id = Some(request.run_id.clone());
                state.writer_process_id = Some(request.process_id.clone());
                pending.push(OrchestrationEvent::WriterAcquired {
                    project_root: request.project_root.clone(),
                    run_id: request.run_id.clone(),
                    process_id: request.process_id.clone(),
                });
                LeaseOutcome::Granted(WriterLease::with_release(
                    request.project_root.clone(),
                    request.run_id.clone(),
                    request.process_id.clone(),
                    {
                        let key = key.clone();
                        let coord = self.clone();
                        move |released_run_id| coord.release_writer(&key, released_run_id)
                    },
                ))
            } else {
                // 有写者:排队等待,先到先得。
                let (tx, rx) = oneshot::channel();
                state.waiting.push_back(WaitingWriter {
                    run_id: request.run_id.clone(),
                    process_id: request.process_id.clone(),
                    reason: request.reason.clone(),
                    tx: Some(tx),
                });
                pending.push(OrchestrationEvent::WriterQueued {
                    project_root: request.project_root.clone(),
                    run_id: request.run_id.clone(),
                    process_id: request.process_id.clone(),
                    reason: request.reason.clone(),
                });
                LeaseOutcome::Queued(rx)
            }
        };
        self.notify(pending);
        match outcome {
            LeaseOutcome::Granted(lease) => Ok(lease),
            LeaseOutcome::Queued(rx) => match rx.await {
                Ok(Ok(lease)) => Ok(lease),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("writer lease cancelled: sender dropped".into()),
            },
        }
    }

    fn cancel_waiter(&self, run_id: &str) {
        let mut pending = Vec::new();
        {
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
                            true
                        } else {
                            false
                        }
                    };
                    if removed {
                        // 记录取消事件(审计:排队中的写者被取消)。
                        pending.push(OrchestrationEvent::WriterCancelled {
                            project_root: PathBuf::from(key),
                            run_id: run_id.to_string(),
                        });
                        state.waiting.remove(idx);
                    } else {
                        idx += 1;
                    }
                }
            }
        }
        self.notify(pending);
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

    /// D-271 反证:排队者被 abort 之后,持有者释放租约不得死锁。
    ///
    /// 失败形态是**整个进程挂死**而不是断言变红,所以 `drop` 放在独立 OS 线程上,
    /// 用 `recv_timeout` 把死锁翻译成可判定的超时失败。修复前:release_writer 在
    /// 临界区内 `tx.send(Ok(lease))`,接收端已丢弃 → 租约原样退回 → `let _ =`
    /// 当场 drop → Drop 回调二次锁同一把非重入 Mutex → 该项目写仲裁永久挂死。
    #[tokio::test]
    async fn 排队者被abort后释放租约不死锁并越位交给下一个() {
        let dir = std::env::temp_dir().join(format!("kz-orch-d271-{}", std::process::id()));
        let coord = MemoryCoordinator::new();
        let lease_a = coord
            .acquire_writer_lease(req("run-a", &dir))
            .await
            .unwrap();

        // B 排队后被 abort:oneshot 的接收端随 future 一起丢弃,但队列条目还在。
        let coord_b = coord.clone();
        let dir_b = dir.clone();
        let task_b =
            tokio::spawn(async move { coord_b.acquire_writer_lease(req("run-b", &dir_b)).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            coord.snapshot(&dir).waiting_writers,
            vec!["run-b".to_string()]
        );
        task_b.abort();
        let _ = task_b.await;

        // C 排在 B 后面,接收端存活。
        let coord_c = coord.clone();
        let dir_c = dir.clone();
        let task_c =
            tokio::spawn(async move { coord_c.acquire_writer_lease(req("run-c", &dir_c)).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            coord.snapshot(&dir).waiting_writers,
            vec!["run-b".to_string(), "run-c".to_string()]
        );

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            drop(lease_a);
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("release_writer 卡住:退回的租约在临界区内 drop,二次锁同一把 Mutex(D-271)");

        let lease_c = tokio::time::timeout(Duration::from_secs(5), task_c)
            .await
            .expect("C 应被唤醒")
            .unwrap()
            .unwrap();
        assert_eq!(lease_c.run_id, "run-c", "写权必须越过被 abort 的 B 交给 C");
        let snap = coord.snapshot(&dir);
        assert_eq!(snap.writer_run_id.as_deref(), Some("run-c"));
        assert!(snap.waiting_writers.is_empty());
        drop(lease_c);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 写仲裁桶键必须把同一项目的各种路径形态收敛到一个桶。
    #[test]
    fn 写仲裁桶键剥扩展长度前缀_各形态同桶() {
        let bare = PathBuf::from(r"C:\Users\kanzei\Documents\kanzei code");
        let canonical = PathBuf::from(r"\\?\C:\Users\kanzei\Documents\kanzei code");
        let slashed = PathBuf::from("C:/Users/kanzei/Documents/kanzei code/");
        let key = normalize_project_root(&bare);
        assert_eq!(key, "c:/users/kanzei/documents/kanzei code");
        assert_eq!(
            normalize_project_root(&canonical),
            key,
            "canonicalize 产出的 \\\\?\\ 形态必须与裸路径落进同一个桶"
        );
        assert_eq!(
            normalize_project_root(&slashed),
            key,
            "正斜杠写法与尾分隔符不得另开一个桶"
        );
        // UNC:\\?\UNC\server\share 与 \\server\share 是同一个位置。
        assert_eq!(
            normalize_project_root(&PathBuf::from(r"\\?\UNC\server\share")),
            normalize_project_root(&PathBuf::from(r"\\server\share")),
        );
    }

    /// 端到端:两种路径形态必须争同一个写租约。
    ///
    /// R-171 验收⑤「worktree 写入无法绕过协调器」在 Windows 上曾经**实际被绕过**——
    /// worktree 创建/合并走 canonicalize 后的 `\\?\` 形态申请租约,主对话 writer 走
    /// 裸 `discover_project_root`,两者落进不同桶,于是可以同时持有写权、互相看不见。
    /// 只断言桶键字符串相等不足以证明修好了,必须证明它们真的互相排队。
    #[tokio::test]
    async fn 不同路径形态的写租约必须互相排队() {
        let tag = std::process::id();
        let bare = PathBuf::from(format!(r"C:\kz-bucket-{tag}"));
        let canonical = PathBuf::from(format!(r"\\?\C:\kz-bucket-{tag}"));
        let coord = MemoryCoordinator::new();
        let lease_bare = coord
            .acquire_writer_lease(req("run-bare", &bare))
            .await
            .unwrap();

        let coord2 = coord.clone();
        let canonical2 = canonical.clone();
        let task = tokio::spawn(async move {
            coord2
                .acquire_writer_lease(req("run-canonical", &canonical2))
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !task.is_finished(),
            "\\\\?\\ 形态必须与裸形态竞争同一个写租约,不得同时持有"
        );
        // 从任一形态查快照,看到的都是同一个桶。
        for form in [&bare, &canonical] {
            let snapshot = coord.snapshot(form);
            assert_eq!(
                snapshot.writer_run_id.as_deref(),
                Some("run-bare"),
                "两种形态必须看到同一个桶里的写者"
            );
            assert_eq!(snapshot.waiting_writers, vec!["run-canonical".to_string()]);
        }

        drop(lease_bare);
        let lease_canonical = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("释放后另一形态应被唤醒")
            .unwrap();
        assert_eq!(lease_canonical.run_id, "run-canonical");
        assert_eq!(
            coord.snapshot(&bare).writer_run_id.as_deref(),
            Some("run-canonical"),
            "接手者从裸形态也必须可见"
        );
    }

    /// 记录事件的观察者:R-173 批4 用来证明两个缺陷确实修好了。
    #[derive(Default)]
    struct Recorder {
        events: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl kanzei_harness::orchestration::PhaseObserver for Recorder {
        fn observe(&self, event: &OrchestrationEvent) {
            self.events
                .lock()
                .unwrap()
                .push((event.event_type().to_string(), event.payload()));
        }
    }

    impl Recorder {
        fn seen(&self) -> Vec<(String, serde_json::Value)> {
            self.events.lock().unwrap().clone()
        }
    }

    /// R-173 修缺陷 A + B:交接路径上离任 writer 的 Released **必须**发出,
    /// 且带真实 process_id。
    ///
    /// 这两个 bug 从 R-171 起就存在,一直不可见——当时事件的唯一去处是一个叫
    /// `last_event` 的内存单槽,而那个单槽自始至终没有任何生产代码读它(R-173
    /// 勘察时才发现,批5 已连字段一起删掉)。事件一旦真的落库,它们立刻变成可见的
    /// 审计错误:审计流会是 A.acquired → B.acquired,看不出 A 何时交的权、
    /// 两段写区间的边界在哪。
    #[tokio::test]
    async fn 写租约交接时离任者released不断档且身份完整() {
        let dir = std::env::temp_dir().join(format!("kz-orch-handoff-{}", std::process::id()));
        let recorder = Arc::new(Recorder::default());
        let coord = MemoryCoordinator::with_observer(
            recorder.clone() as Arc<dyn kanzei_harness::orchestration::PhaseObserver>
        );
        let lease_a = coord
            .acquire_writer_lease(req("run-a", &dir))
            .await
            .unwrap();
        let coord2 = coord.clone();
        let dir2 = dir.clone();
        let task = tokio::spawn(async move {
            coord2
                .acquire_writer_lease(req("run-b", &dir2))
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(lease_a); // 交接:A 释放 → B 立刻接手
        let lease_b = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("run-b 应被唤醒")
            .unwrap();

        let seen = recorder.seen();
        let types: Vec<&str> = seen.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            types,
            vec![
                "orchestration.writer.acquired", // A 取得
                "orchestration.writer.queued",   // B 排队
                "orchestration.writer.released", // A 交权 ← 缺陷 A:原实现这条根本不发
                "orchestration.writer.acquired", // B 接手
            ],
            "交接路径必须留下完整的 released→acquired 序列,不得断档"
        );
        // 缺陷 B:离任者的 process_id 必须是真实身份,不是空串。
        let released = &seen[2].1;
        assert_eq!(released["run_id"], "run-a");
        assert_eq!(
            released["process_id"], "proc-run-a",
            "离任 writer 的 process_id 不能是空串"
        );
        drop(lease_b);
        // 最后一次释放(无排队者)同样带真实身份。
        let seen = recorder.seen();
        let (last_type, last_payload) = seen.last().unwrap();
        assert_eq!(last_type, "orchestration.writer.released");
        assert_eq!(last_payload["run_id"], "run-b");
        assert_eq!(last_payload["process_id"], "proc-run-b");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 读槽事件也走同一个观察者出口,且 writer 活跃时读槽不被阻塞(验收④)。
    #[tokio::test]
    async fn 读槽事件进观察者_且与写租约共存() {
        let dir = std::env::temp_dir().join(format!("kz-orch-coexist-{}", std::process::id()));
        let recorder = Arc::new(Recorder::default());
        let coord = MemoryCoordinator::with_observer(
            recorder.clone() as Arc<dyn kanzei_harness::orchestration::PhaseObserver>
        );
        let _writer = coord
            .acquire_writer_lease(req("run-w", &dir))
            .await
            .unwrap();
        let permit = coord
            .acquire_read_slot(ReadSlotRequest {
                project_root: dir.clone(),
                run_id: "r1".into(),
                process_id: "p1".into(),
                agent_name: "live_scout".into(),
            })
            .await
            .expect("writer 活跃时读槽必须仍可获取");
        let snapshot = coord.snapshot(&dir);
        assert_eq!(snapshot.writer_run_id.as_deref(), Some("run-w"));
        assert_eq!(snapshot.active_readers, vec!["live_scout".to_string()]);
        drop(permit);
        let types: Vec<String> = recorder.seen().into_iter().map(|(t, _)| t).collect();
        assert!(types.contains(&"orchestration.agent_started".to_string()));
        assert!(types.contains(&"orchestration.agent_completed".to_string()));
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
