use super::*;
use async_trait::async_trait;
use kanzei_harness::{Component, Harness, HarnessDraft, ResolveCtx, ToolOutput};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;

mod input;
mod large_results;
mod mutation;

#[tokio::test]
async fn ambient_hosted_declarations_are_rejected_before_provider_io_or_approval() {
    let root = std::env::temp_dir().join("kz-ambient-hosted-rejection");
    let mut harness = Harness::default();
    harness.add(Policy);
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(Default::default()),
        })
        .unwrap();
    let agent: AgentDef = serde_json::from_value(
        json!({"name":"fixture","profile":"dev-auto","mode":"primary","steps":3,"system":"fixture"}),
    )
    .unwrap();
    let mut config = config(kanzei_harness::orchestration::ExecutionPolicy::Default);
    config.hosted_tools = vec![json!({"type":"web_search"})];
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let route = Route::openai_responses_at(
        &format!("http://{}", listener.local_addr().unwrap()),
        vec![],
    );
    let ctx = ToolCtx::new(root.clone(), root);
    let mut events = Vec::new();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &config,
            &ctx,
            "search",
            None,
            &[],
            None,
            &mut |event| events.push(event),
            &mut |_| panic!("ambient hosted tools must not reach an approval callback"),
        ),
    )
    .await
    .unwrap();
    assert!(outcome
        .err()
        .unwrap()
        .to_string()
        .contains("Ambient provider-hosted tools are unsupported"));
    assert!(events.is_empty());
    assert_eq!(
        listener.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[derive(Default)]
struct CleanupProbe {
    started: Notify,
    cleanup_pending: Notify,
    release_cleanup: Notify,
    cleaned: AtomicBool,
}

#[async_trait]
impl Tool for CleanupProbe {
    fn read_only_support(&self) -> kanzei_harness::tool::ReadOnlySupport {
        kanzei_harness::tool::ReadOnlySupport::ReadOnly
    }
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

struct SnapshotTool(Arc<dyn Tool>);
impl Component for SnapshotTool {
    fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
        draft.tools.insert(self.0.name(), self.0.clone());
        Ok(())
    }
}

#[test]
fn readonly_child_boundary_rejects_an_incorrectly_constructed_snapshot() {
    let root = std::env::temp_dir();
    let rctx = ResolveCtx {
        profile: kanzei_harness::ProfileKind::DevAuto,
        cwd: root.clone(),
        project_root: root,
        config: Arc::new(Default::default()),
    };
    let mut harness = Harness::default();
    harness
        .add(Policy)
        .add(SnapshotTool(Arc::new(CleanupProbe::default())));
    let snapshot = harness.resolve(&rctx).unwrap();
    assert!(crate::runner::subagent::validate_readonly_snapshot(&snapshot).is_ok());
    harness.add(SnapshotTool(Arc::new(MutationProbe::default())));
    let snapshot = harness.resolve(&rctx).unwrap();
    assert!(
        crate::runner::subagent::validate_readonly_snapshot(&snapshot)
            .unwrap_err()
            .contains("git")
    );
}

struct MutationProbe(AtomicUsize, kanzei_harness::tool::ReadOnlySupport);

impl Default for MutationProbe {
    fn default() -> Self {
        Self(
            AtomicUsize::new(0),
            kanzei_harness::tool::ReadOnlySupport::Unsupported,
        )
    }
}

#[async_trait]
impl Tool for MutationProbe {
    fn read_only_support(&self) -> kanzei_harness::tool::ReadOnlySupport {
        self.1
    }
    fn name(&self) -> &'static str {
        "git"
    }
    fn action(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> String {
        "forged broad authorization fixture".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object"})
    }
    fn resources(&self, _: &serde_json::Value) -> Vec<String> {
        vec!["allowed".into()]
    }
    fn concurrency(&self, _: &serde_json::Value, ctx: &ToolCtx) -> kanzei_harness::ToolConcurrency {
        kanzei_harness::ToolConcurrency::write_worktree(ctx)
    }
    async fn execute(&self, _: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        self.0.fetch_add(1, Ordering::SeqCst);
        std::fs::write(ctx.cwd.join("forbidden-mutation"), "changed").unwrap();
        ToolOutput::ok("changed")
    }
}

#[tokio::test]
async fn readonly_execution_rejects_mutations_with_broad_permissions_in_serial_and_parallel_paths()
{
    use kanzei_harness::{tool::ReadOnlySupport, ExecutionPolicy, ProfileKind};
    let directory = kanzei_base::test_support::TestDir::new("kz-readonly-dispatch-");
    let root = directory.to_path_buf();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    for (profile, policy) in [
        (ProfileKind::DevAuto, ExecutionPolicy::ReadOnly),
        (ProfileKind::Readonly, ExecutionPolicy::Default),
        (
            ProfileKind::Readonly,
            ExecutionPolicy::ReadParallelWriteSerial,
        ),
    ] {
        let mut harness = Harness::default();
        harness.add(Policy);
        let snapshot = harness
            .resolve(&ResolveCtx {
                profile,
                cwd: root.clone(),
                project_root: root.clone(),
                config: Arc::new(Default::default()),
            })
            .unwrap();
        for support in [ReadOnlySupport::Unsupported, ReadOnlySupport::ByAction] {
            let probe = Arc::new(MutationProbe(AtomicUsize::new(0), support));
            let tools: Vec<Arc<dyn Tool>> = vec![probe.clone()];
            let mut cfg = config(policy);
            cfg.ask_policy = AskPolicy::AutoAllow;
            for count in [1, 2] {
                let calls = (0..count)
                    .map(|index| {
                        (
                            format!("mutation-{index}"),
                            "git".into(),
                            json!({"action":"commit"}),
                            "{}".into(),
                        )
                    })
                    .collect::<Vec<_>>();
                let outcome = execute_tool_calls(
                    &cfg,
                    &ctx,
                    &snapshot,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    &tools,
                    &calls,
                    None,
                    &mut HashMap::new(),
                    false,
                    None,
                    &mut |_| {},
                    &mut |_| panic!("readonly rejection cannot ask for a scope upgrade"),
                    &mut HashSet::new(),
                    &mut vec![("read".into(), "*".into())],
                    &mut Vec::new(),
                    1,
                )
                .await
                .unwrap();
                let ToolRunOutcome::Results { results, .. } = outcome else {
                    panic!("must return paired denied results")
                };
                assert_eq!(results.len(), count);
                assert!(results.iter().all(|part| matches!(part, Part::ToolResult {is_error:true, content, ..} if content.contains("read-only execution scope"))));
            }
            assert_eq!(
                probe.0.load(Ordering::SeqCst),
                0,
                "profile={profile:?}, policy={policy:?}, support={support:?}"
            );
        }
    }
    assert!(!root.join("forbidden-mutation").exists());
}

#[derive(Default)]
struct SensitiveProbe(AtomicUsize);

#[async_trait]
impl Tool for SensitiveProbe {
    fn name(&self) -> &'static str {
        "sensitive_probe"
    }
    fn action(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> String {
        "credential consent fixture".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object"})
    }
    fn permission_resources(
        &self,
        _: &serde_json::Value,
        _: &ToolCtx,
    ) -> Vec<kanzei_harness::permission::Resource> {
        vec![kanzei_harness::permission::Resource::File(".env".into())]
    }
    fn concurrency(&self, _: &serde_json::Value, _: &ToolCtx) -> kanzei_harness::ToolConcurrency {
        kanzei_harness::ToolConcurrency::Shared("sensitive-fixture".into())
    }
    async fn execute(&self, _: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let resource = kanzei_harness::permission::FileResource::resolve(
            &ctx.cwd,
            std::path::Path::new(".env"),
        )
        .unwrap();
        assert_eq!(
            kanzei_harness::permission::execution_rules()
                .unwrap()
                .evaluate_file_with_rule("read", &resource)
                .0,
            Effect::Allow,
            "the approved grant must reach actual I/O, including parallel calls"
        );
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutput::ok(std::fs::read_to_string(ctx.cwd.join(".env")).unwrap())
    }
}

#[tokio::test]
async fn credentials_ignore_auto_allow_and_broad_session_rules_but_honor_exact_user_consent() {
    for ask_policy in [
        AskPolicy::AutoAllow,
        AskPolicy::Interactive,
        AskPolicy::DeferredQuestions,
    ] {
        let root = std::env::temp_dir().join(format!(
            "kz-sensitive-execution-{}-{ask_policy:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".env"), "FIXTURE_SECRET").unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let mut harness = Harness::default();
        harness.add(Policy);
        let snapshot = harness
            .resolve(&ResolveCtx {
                profile: kanzei_harness::ProfileKind::DevAuto,
                cwd: root.clone(),
                project_root: root.clone(),
                config: Arc::new(Default::default()),
            })
            .unwrap();
        let mut config = config(kanzei_harness::orchestration::ExecutionPolicy::Default);
        config.ask_policy = ask_policy;
        let probe = Arc::new(SensitiveProbe::default());
        let tools: Vec<Arc<dyn Tool>> = vec![probe.clone()];
        let calls = (0..2)
            .map(|index| {
                (
                    format!("secret-{index}"),
                    "sensitive_probe".into(),
                    json!({}),
                    "{}".into(),
                )
            })
            .collect::<Vec<_>>();
        let mut approved = HashSet::new();
        let mut session_rules = vec![("read".into(), "*".into())];
        let questions = AtomicUsize::new(0);
        for _ in 0..2 {
            let mut ask = |request| -> AskFuture {
                assert!(
                    matches!(request, AskRequest::Permission { ref resource, .. } if resource == ".env")
                );
                questions.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { AskResponse::Permission(AskReply::AllowOnce) })
            };
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
                false,
                None,
                &mut |_| {},
                &mut ask,
                &mut approved,
                &mut session_rules,
                &mut Vec::new(),
                1,
            )
            .await
            .unwrap();
            let ToolRunOutcome::Results { results, .. } = outcome else {
                panic!("consent fixture cannot stop")
            };
            assert_eq!(results.len(), 2);
        }
        let interactive = ask_policy.allows_permission_prompt();
        assert_eq!(
            probe.0.load(Ordering::SeqCst),
            if interactive { 4 } else { 0 }
        );
        assert_eq!(questions.load(Ordering::SeqCst), usize::from(interactive));
        std::fs::remove_dir_all(root).unwrap();
    }
}
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
            profile: kanzei_harness::ProfileKind::DevAuto,
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
            profile: kanzei_harness::ProfileKind::DevAuto,
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
