use super::*;
use kanzei_harness::orchestration::{
    CoordinationObserver, ExecutionCoordinator, OrchestrationEvent, ProjectExecutionCoordinator,
    WriterLeaseRequest,
};
use kanzei_harness::tool_pipeline::execute_coordinated;

#[derive(Default)]
struct Events(std::sync::Mutex<Vec<OrchestrationEvent>>);
impl CoordinationObserver for Events {
    fn observe(&self, event: &OrchestrationEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

struct BlockingMutation {
    active: Arc<AtomicUsize>,
    release: Notify,
}

#[async_trait]
impl Tool for BlockingMutation {
    fn name(&self) -> &'static str {
        "write"
    }
    fn action(&self) -> &'static str {
        "write"
    }
    fn description(&self) -> String {
        "controlled actual mutation".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object"})
    }
    fn concurrency(&self, _: &serde_json::Value, ctx: &ToolCtx) -> kanzei_harness::ToolConcurrency {
        kanzei_harness::ToolConcurrency::write_worktree(ctx)
    }
    async fn execute(&self, _: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        assert_eq!(
            self.active.fetch_add(1, Ordering::SeqCst),
            0,
            "mutation intervals overlap"
        );
        assert_eq!(
            ctx.execution_coordinator
                .as_ref()
                .unwrap()
                .snapshot(&ctx.cwd)
                .writer_run_id,
            ctx.run_id
        );
        self.release.notified().await;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(ctx.cwd.join("writes.txt"))
            .unwrap();
        writeln!(file, "{}", ctx.run_id.as_deref().unwrap()).unwrap();
        assert_eq!(self.active.fetch_sub(1, Ordering::SeqCst), 1);
        ToolOutput::ok("written")
    }
}

struct ActualRead;
#[async_trait]
impl Tool for ActualRead {
    fn name(&self) -> &'static str {
        "read"
    }
    fn action(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> String {
        "actual read during another mutation".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object"})
    }
    fn concurrency(&self, _: &serde_json::Value, ctx: &ToolCtx) -> kanzei_harness::ToolConcurrency {
        kanzei_harness::ToolConcurrency::shared_worktree(ctx)
    }
    async fn execute(&self, _: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        ToolOutput::ok(std::fs::read_to_string(ctx.cwd.join("known.txt")).unwrap())
    }
}

fn context(root: &std::path::Path, id: &str, coordinator: &ExecutionCoordinator) -> ToolCtx {
    let mut ctx = ToolCtx::new(root.into(), root.into()).with_identity(
        root.display().to_string(),
        root.display().to_string(),
        id.into(),
        id.into(),
    );
    ctx.execution_coordinator = Some(coordinator.clone());
    ctx
}

#[tokio::test]
async fn mutations_are_serial_but_real_reads_do_not_wait_and_every_lease_is_released() {
    let root = std::env::temp_dir().join(format!("kz-mutation-intervals-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("known.txt"), "source evidence").unwrap();
    let raw = Arc::new(crate::orchestration::MemoryCoordinator::new());
    let events = Arc::new(Events::default());
    let coordinator = ExecutionCoordinator::new(raw.clone()).with_observer(events.clone());
    let a_ctx = context(&root, "run-a", &coordinator);
    let b_ctx = context(&root, "run-b", &coordinator);
    let active = Arc::new(AtomicUsize::new(0));
    let a = BlockingMutation {
        active: active.clone(),
        release: Notify::new(),
    };
    let b = BlockingMutation {
        active: active.clone(),
        release: Notify::new(),
    };
    let first = execute_coordinated(&a, json!({}), &a_ctx, None);
    let second = execute_coordinated(&b, json!({}), &b_ctx, None);
    tokio::pin!(first, second);
    assert!(futures::poll!(&mut first).is_pending());
    assert!(futures::poll!(&mut second).is_pending());
    assert_eq!(raw.snapshot(&root).waiting_writers, vec!["run-b"]);
    let read = execute_coordinated(&ActualRead, json!({}), &b_ctx, None).await;
    assert_eq!(read.content, "source evidence");
    a.release.notify_one();
    assert!(!first.await.is_error);
    assert!(futures::poll!(&mut second).is_pending());
    b.release.notify_one();
    assert!(!second.await.is_error);
    assert_eq!(
        std::fs::read_to_string(root.join("writes.txt")).unwrap(),
        "run-a\nrun-b\n"
    );
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert!(raw.snapshot(&root).writer_run_id.is_none());
    assert!(raw.snapshot(&root).waiting_writers.is_empty());
    let events = events.0.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, OrchestrationEvent::WriterAcquired { .. }))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, OrchestrationEvent::WriterReleased { .. }))
            .count(),
        2
    );
    drop(events);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cancelled_mutation_wait_drops_its_queue_claim_without_running_the_tool() {
    let root = std::env::temp_dir().join(format!("kz-mutation-cancel-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let raw = Arc::new(crate::orchestration::MemoryCoordinator::new());
    let coordinator = ExecutionCoordinator::new(raw.clone());
    let blocker = raw
        .acquire_writer_lease(WriterLeaseRequest {
            write_scope: root.clone(),
            run_id: "blocker".into(),
            process_id: "blocker".into(),
            reason: "fixture".into(),
        })
        .await
        .unwrap();
    let ctx = context(&root, "cancelled-run", &coordinator);
    let probe = MutationProbe::default();
    let halt = CancellationToken::new();
    let execute = execute_coordinated(&probe, json!({}), &ctx, Some(&halt));
    tokio::pin!(execute);
    assert!(futures::poll!(&mut execute).is_pending());
    assert_eq!(raw.snapshot(&root).waiting_writers, vec!["cancelled-run"]);
    halt.cancel();
    assert_eq!(execute.await.code, Some("RUN_CANCELLED"));
    assert!(raw.snapshot(&root).waiting_writers.is_empty());
    assert_eq!(probe.0.load(Ordering::SeqCst), 0);
    assert!(!root.join("forbidden-mutation").exists());
    drop(blocker);
    assert!(raw.snapshot(&root).writer_run_id.is_none());
    std::fs::remove_dir_all(root).unwrap();
}
