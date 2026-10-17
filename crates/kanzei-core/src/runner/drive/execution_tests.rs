use super::*;
use async_trait::async_trait;
use kanzei_harness::{Component, Harness, HarnessDraft, ResolveCtx, ToolOutput};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
struct CleanupProbe {
    started: Notify,
    cleanup_pending: Notify,
    release_cleanup: Notify,
    cleaned: AtomicBool,
}

#[async_trait]
impl Tool for CleanupProbe {
    fn name(&self) -> &'static str {
        "execution_probe"
    }
    fn action(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> String {
        "cancellation ownership fixture".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object"})
    }
    fn resources(&self, _: &serde_json::Value) -> Vec<String> {
        vec!["allowed".into()]
    }
    fn concurrency(&self, _: &serde_json::Value, _: &ToolCtx) -> kanzei_harness::ToolConcurrency {
        kanzei_harness::ToolConcurrency::Shared("fixture".into())
    }
    async fn execute(&self, _: serde_json::Value, _: &ToolCtx) -> ToolOutput {
        panic!("runner must enter the cancellable execution boundary")
    }
    async fn execute_cancellable(
        &self,
        input: serde_json::Value,
        _: &ToolCtx,
        halt: Option<&CancellationToken>,
    ) -> ToolOutput {
        assert_eq!(
            kanzei_harness::permission::execution_effect("read", "private"),
            Some(Effect::Deny)
        );
        if input["finish"] == true {
            return ToolOutput::ok("already complete");
        }
        self.started.notify_one();
        halt.unwrap().cancelled().await;
        self.cleanup_pending.notify_one();
        self.release_cleanup.notified().await;
        self.cleaned.store(true, Ordering::SeqCst);
        ToolOutput::error("cancelled after cleanup")
    }
}

struct Policy;
impl Component for Policy {
    fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
        draft.permissions.extend([
            kanzei_harness::rule("read", "*", Effect::Allow),
            kanzei_harness::rule("read", "private", Effect::Deny),
        ]);
        Ok(())
    }
}

fn config(policy: kanzei_harness::orchestration::ExecutionPolicy) -> RunnerConfig {
    RunnerConfig {
        digest_model: None,
        intensity: Default::default(),
        model: "fixture".into(),
        max_tokens: 1,
        reasoning: Default::default(),
        service_tier: None,
        hosted_tools: vec![],
        context_limit: None,
        limits: Default::default(),
        recall: None,
        execution_policy: policy,
        ask_policy: AskPolicy::Interactive,
        halt: None,
    }
}

async fn assert_execution_boundary(policy: kanzei_harness::orchestration::ExecutionPolicy) {
    let root = std::env::temp_dir().join(format!(
        "kz-execution-boundary-{}-{:?}",
        std::process::id(),
        policy
    ));
    std::fs::create_dir_all(&root).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let mut harness = Harness::default();
    harness.add(Policy);
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(Default::default()),
        })
        .unwrap();
    let config = config(policy);
    let probe = Arc::new(CleanupProbe::default());
    let tools: Vec<Arc<dyn Tool>> = vec![probe.clone()];
    let calls = [true, false]
        .into_iter()
        .enumerate()
        .map(|(index, finish)| {
            (
                format!("call-{index}"),
                "execution_probe".into(),
                json!({"finish":finish}),
                "{}".into(),
            )
        })
        .collect::<Vec<_>>();
    let halt = CancellationToken::new();
    let returned = AtomicBool::new(false);
    let execution = async {
        let mut events = Vec::new();
        let outcome = execute_tool_calls(
            &config,
            &ctx,
            &snapshot,
            &mut Vec::new(),
            &mut Vec::new(),
            &tools,
            &calls,
            None,
            &mut HashMap::new(),
            true,
            Some(&halt),
            &mut |event| events.push(event),
            &mut |_| panic!("fixture requires no approval"),
            &mut HashSet::new(),
            &mut Vec::new(),
            &mut Vec::new(),
            1,
            &batch::Batch::new(false),
        )
        .await
        .unwrap();
        returned.store(true, Ordering::SeqCst);
        (outcome, events)
    };
    let control = async {
        probe.started.notified().await;
        halt.cancel();
        probe.cleanup_pending.notified().await;
        assert!(
            !returned.load(Ordering::SeqCst),
            "runner returned while resource cleanup was pending"
        );
        probe.release_cleanup.notify_one();
    };
    let ((outcome, events), ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(execution, control)
    })
    .await
    .unwrap();
    assert!(probe.cleaned.load(Ordering::SeqCst));
    let ToolRunOutcome::Results { results, .. } = outcome else {
        panic!("expected paired tool results")
    };
    assert!(
        matches!(&results[0], Part::ToolResult{call_id, content, is_error:false} if call_id=="call-0" && content.contains("already complete"))
    );
    assert!(
        matches!(&results[1], Part::ToolResult{call_id, content, is_error:true} if call_id=="call-1" && content.contains("cancelled after cleanup"))
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, RunEvent::ToolEnd { .. }))
            .count(),
        2
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn serial_cancellation_awaits_cleanup_and_preserves_policy_and_completed_results() {
    assert_execution_boundary(
        kanzei_harness::orchestration::ExecutionPolicy::ReadParallelWriteSerial,
    )
    .await;
}

#[tokio::test]
async fn parallel_cancellation_awaits_cleanup_and_preserves_policy_and_completed_results() {
    assert_execution_boundary(kanzei_harness::orchestration::ExecutionPolicy::Default).await;
}

#[tokio::test]
async fn server_retry_after_is_visible_and_remains_cancellable() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 8192];
        let _ = socket.read(&mut request).await.unwrap();
        let body = "data: {\"error\":{\"type\":\"rate_limit_error\",\"message\":\"wait\",\"retry_after\":60}}\n\n";
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
    });
    let root = std::env::temp_dir();
    let snapshot = Harness::default()
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(Default::default()),
        })
        .unwrap();
    let ctx = ToolCtx::new(root.clone(), root);
    let client = LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let route = Route::openai_at(&format!("http://{address}/v1"), None);
    let agent =
        serde_json::from_value(json!({"name":"fixture","model":"fixture","max_steps":4})).unwrap();
    let halt = CancellationToken::new();
    let mut config = config(Default::default());
    config.halt = Some(halt.clone());
    let mut observed = false;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &config,
            &ctx,
            "run",
            None,
            &[],
            None,
            &mut |event| {
                if let RunEvent::StreamRestart { delay_ms, .. } = event {
                    assert_eq!(delay_ms, 60_000);
                    observed = true;
                    halt.cancel();
                }
            },
            &mut |_| panic!("no tool approval expected"),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(observed);
    assert!(result.halted_by_user);
    server.await.unwrap();
}
