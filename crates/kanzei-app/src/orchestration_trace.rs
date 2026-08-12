//! 编排事件轨迹(R-173 批5):把 [`OrchestrationEvent`] 落进 `session_events`。
//!
//! R-171 的写租约事件是**手写字符串**直接 `append_event`——类型名与 payload 形状
//! 各处各抄一份,与 `OrchestrationEvent` 枚举没有任何类型联系,两边可以随意漂移
//! 且编译器不会吭声。本模块把落库收成一条路:事件自己说自己叫什么
//! (`event_type()`)、自己序列化自己(`payload()`),调用方只管发事件。

use std::path::Path;
use std::sync::Mutex;

use kanzei_harness::orchestration::{OrchestrationEvent, PhaseObserver};

/// 把编排事件写进某个会话事件流的观察者。
///
/// 自带一条 SQLite 连接:`SessionStore::open` 每次都会跑一遍迁移,按事件开连接
/// 太浪费,而 `SessionStore` 内含 `rusqlite::Connection`(Send 非 Sync),
/// 所以用 `Mutex` 包一层满足 `PhaseObserver: Send + Sync`。
pub(crate) struct SessionEventObserver {
    store: Mutex<kanzei_core::SessionStore>,
    session_id: String,
}

impl SessionEventObserver {
    pub(crate) fn open(state_path: &Path, session_id: &str) -> anyhow::Result<Self> {
        Ok(SessionEventObserver {
            store: Mutex::new(kanzei_core::SessionStore::open(state_path)?),
            session_id: session_id.to_string(),
        })
    }
}

impl PhaseObserver for SessionEventObserver {
    /// 落库失败只记日志,不打断运行。
    ///
    /// 这不是偷懒:`observe` 返回 `()`,契约上就没有传播错误的通道——因为编排
    /// 事件是**可观察性**,不是正确性。写租约本身的仲裁由协调器保证,轨迹写不进去
    /// 顶多是事后查不到,不该连带把用户正在跑的那一轮弄挂。
    fn observe(&self, event: &OrchestrationEvent) {
        // 中毒锁照样用:观察者只做追加写,没有需要保护的跨调用不变式,
        // 因一次失败就永久停止记录反而丢的更多。
        let store = self
            .store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(error) =
            store.append_event(&self.session_id, event.event_type(), &event.payload())
        {
            tracing::warn!(
                session_id = %self.session_id,
                event_type = event.event_type(),
                %error,
                "编排事件落库失败(不影响本轮运行)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_core::orchestration::MemoryCoordinator;
    use kanzei_core::PhaseOrchestrator;
    use kanzei_harness::orchestration::{
        ProjectExecutionCoordinator, ScoutOutcome, WriterLeaseRequest,
    };
    use std::sync::Arc;
    use std::time::Duration;

    fn temp_db(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-orch-trace-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("state.db")
    }

    fn scout(outcome: ScoutOutcome) -> kanzei_core::ScoutTask<'static> {
        Box::pin(async move { outcome })
    }

    /// 验收②的 app 侧锚点:一条运行跑完七阶段,事件按序落进 session_events 并可回放。
    #[tokio::test]
    async fn 全阶段轨迹落库并可按序回放() {
        let db = temp_db("loop");
        let session_id = "ses_r173";
        {
            let store = kanzei_core::SessionStore::open(&db).unwrap();
            store.create_session(session_id, "C:/proj", None).unwrap();
        }
        let observer = Arc::new(SessionEventObserver::open(&db, session_id).unwrap());
        let coordinator = Arc::new(MemoryCoordinator::new());
        let mut orchestrator = PhaseOrchestrator::new(
            coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>,
            std::path::PathBuf::from("C:/proj"),
            "run_1",
            "proc_1",
            Duration::from_secs(5),
        )
        .with_observer(observer as Arc<dyn PhaseObserver>);

        orchestrator.enter_scouting().unwrap();
        orchestrator
            .join_scouts(vec![
                ("architecture_scout".into(), scout(ScoutOutcome::Completed)),
                (
                    "test_scout".into(),
                    scout(ScoutOutcome::Failed("provider 500".into())),
                ),
            ])
            .await
            .unwrap();
        orchestrator.enter_implementation().await.unwrap();
        orchestrator.enter_integration().unwrap();
        orchestrator.enter_review().unwrap();
        orchestrator
            .join_reviewers(vec![(
                "contract_reviewer".into(),
                scout(ScoutOutcome::Completed),
            )])
            .await
            .unwrap();
        orchestrator.enter_fixup().await.unwrap();
        orchestrator.finish().unwrap();

        // 回放:另开一条连接读回来,证明确实落盘而不是只在内存里。
        let store = kanzei_core::SessionStore::open(&db).unwrap();
        let events = store.list_events(session_id, 0).unwrap();
        let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(
            types,
            vec![
                "orchestration.phase_changed",   // baseline(观察者装配时的起点)
                "orchestration.phase_changed",   // scouting
                "orchestration.agent_started",   // architecture_scout
                "orchestration.agent_started",   // test_scout
                "orchestration.agent_completed", // architecture_scout 成功
                "orchestration.agent_failed",    // test_scout 失败
                "orchestration.barrier_reached", // 汇总屏障
                "orchestration.phase_changed",   // synthesis
                "orchestration.writer.queued",
                "orchestration.writer.acquired",
                "orchestration.phase_changed",   // implementation
                "orchestration.phase_changed",   // integration
                "orchestration.writer.released", // 复核屏障:先交租约
                "orchestration.phase_changed",   // review
                "orchestration.agent_started",   // contract_reviewer
                "orchestration.agent_completed",
                "orchestration.barrier_reached", // 复核汇总门
                "orchestration.writer.queued",
                "orchestration.writer.acquired",
                "orchestration.phase_changed", // fixup
                "orchestration.writer.released",
                "orchestration.phase_changed", // finished
            ],
            "七阶段完整轨迹必须按序落库"
        );

        // sequence 单调,回放顺序即真实顺序。
        let sequences: Vec<i64> = events.iter().map(|e| e.sequence).collect();
        assert!(
            sequences.windows(2).all(|w| w[0] < w[1]),
            "事件 sequence 必须单调递增"
        );

        let phases: Vec<String> = events
            .iter()
            .filter(|e| e.event_type == "orchestration.phase_changed")
            .map(|e| e.payload["phase"].as_str().unwrap_or_default().to_string())
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
            ]
        );

        // 屏障统计可回放:失败/超时不是静默吞掉的。
        let barriers: Vec<&kanzei_core::StoredEvent> = events
            .iter()
            .filter(|e| e.event_type == "orchestration.barrier_reached")
            .collect();
        assert_eq!(barriers.len(), 2);
        assert_eq!(barriers[0].payload["barrier"], "synthesis");
        assert_eq!(barriers[0].payload["agent_count"], 2);
        assert_eq!(barriers[0].payload["completed"], 1);
        assert_eq!(barriers[0].payload["failed"], 1);
        assert_eq!(barriers[1].payload["barrier"], "review");

        // 复核屏障:writer.released 必须早于 phase_changed(review)。
        let released_at = types
            .iter()
            .position(|t| *t == "orchestration.writer.released")
            .unwrap();
        let review_at = events
            .iter()
            .position(|e| {
                e.event_type == "orchestration.phase_changed" && e.payload["phase"] == "review"
            })
            .unwrap();
        assert!(
            released_at < review_at,
            "复核必须在写租约释放之后启动(不变量 9)"
        );
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    /// D-303 验收②:plain 路径异常/停止时,WriterLeaseTrace Drop 补写 Released,
    /// acquired/released 在 session_events 里成对可回放。
    #[tokio::test]
    async fn writer_lease_trace_drop补写released_异常路径审计成对() {
        use kanzei_harness::orchestration::OrchestrationEvent;
        let db = temp_db("lease-drop");
        let session_id = "ses_lease_drop";
        {
            let store = kanzei_core::SessionStore::open(&db).unwrap();
            store.create_session(session_id, "C:/proj", None).unwrap();
        }
        let observer = Arc::new(SessionEventObserver::open(&db, session_id).unwrap());
        // 模拟 run.rs plain 路径:queued + acquired 由 acquire_plain_lease_if_needed 发,
        // 租约由 WriterLeaseTrace 持有;正常尾部会 mark_released,这里故意不调,
        // 模拟异常/abort 路径直接 drop——Drop 必须补写 released。
        let coordinator = MemoryCoordinator::new();
        observer.observe(&OrchestrationEvent::WriterQueued {
            project_root: std::path::PathBuf::from("C:/proj"),
            run_id: "run_abort".into(),
            process_id: "proc_abort".into(),
            reason: "session writer run".into(),
        });
        observer.observe(&OrchestrationEvent::WriterAcquired {
            project_root: std::path::PathBuf::from("C:/proj"),
            run_id: "run_abort".into(),
            process_id: "proc_abort".into(),
        });
        let lease = coordinator
            .acquire_writer_lease(WriterLeaseRequest {
                write_scope: std::path::PathBuf::from("C:/proj"),
                run_id: "run_abort".into(),
                process_id: "proc_abort".into(),
                reason: "test".into(),
            })
            .await
            .unwrap();
        let _trace = crate::run::WriterLeaseTrace::new(
            lease,
            observer,
            std::path::PathBuf::from("C:/proj"),
            "run_abort".into(),
            "proc_abort".into(),
        );
        // 不调 mark_released 直接 drop(_trace 离开作用域),模拟异常路径。
        drop(_trace);

        let store = kanzei_core::SessionStore::open(&db).unwrap();
        let events = store.list_events(session_id, 0).unwrap();
        let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(
            types,
            vec![
                "orchestration.writer.queued",
                "orchestration.writer.acquired",
                "orchestration.writer.released"
            ],
            "异常路径 Drop 必须补写 released,acquired/released 成对"
        );
        assert_eq!(events[2].payload["run_id"], "run_abort");
        assert_eq!(events[2].payload["process_id"], "proc_abort");
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    /// 协调器自己发的写租约事件(跨进程排队/交接)同样经单一出口落库。
    #[tokio::test]
    async fn 协调器写租约事件经同一出口落库() {
        let db = temp_db("coord");
        let session_id = "ses_coord";
        {
            let store = kanzei_core::SessionStore::open(&db).unwrap();
            store.create_session(session_id, "C:/proj", None).unwrap();
        }
        let observer = Arc::new(SessionEventObserver::open(&db, session_id).unwrap());
        let coordinator = MemoryCoordinator::with_observer(observer as Arc<dyn PhaseObserver>);
        let root = std::path::PathBuf::from("C:/proj");
        let lease = coordinator
            .acquire_writer_lease(WriterLeaseRequest {
                write_scope: root.clone(),
                run_id: "run_a".into(),
                process_id: "proc_a".into(),
                reason: "test".into(),
            })
            .await
            .unwrap();
        drop(lease);

        let store = kanzei_core::SessionStore::open(&db).unwrap();
        let events = store.list_events(session_id, 0).unwrap();
        let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(
            types,
            vec![
                "orchestration.writer.acquired",
                "orchestration.writer.released"
            ]
        );
        // 落库的字段形状与 R-171 手写字符串那版一致(超集),历史行仍可同键回放。
        assert_eq!(events[0].payload["run_id"], "run_a");
        assert_eq!(events[0].payload["process_id"], "proc_a");
        assert_eq!(events[1].payload["process_id"], "proc_a");
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }
}
