//! Verify actual provider requests past step 32, not just the budget helper.
use kanzei_harness::{
    AgentDef, AgentMode, Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx,
};
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn primary_and_subagent_continue_past_32_without_a_step_limit() {
    for mode in [AgentMode::Primary, AgentMode::Subagent] {
        let expected_steps = 34u32;
        let home = super::common::TestHome::new("step-budget");
        std::fs::write(home.root.join("sample.txt"), "read-only probe").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for step in 1..=expected_steps {
                let delta = if step == expected_steps {
                    json!({"content": "done"})
                } else {
                    json!({"tool_calls": [{
                        "index": 0, "id": format!("read-{step}"), "type": "function",
                        "function": {"name": "read", "arguments": "{\"path\":\"sample.txt\"}"}
                    }]})
                };
                let raw = super::memory_hints_not_persisted::serve_response(&listener, json!({
                    "choices": [{"index": 0, "delta": delta,
                        "finish_reason": if step == expected_steps { "stop" } else { "tool_calls" }}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1}
                })).await;
                let text = String::from_utf8(raw).unwrap();
                let (_, body) = text.split_once("\r\n\r\n").unwrap();
                requests.push(serde_json::from_str::<serde_json::Value>(body).unwrap());
            }
            requests
        });
        let config = Arc::new(KanzeiConfig::default());
        let mut harness = Harness::default();
        harness.add(kanzei_tools::SubagentBase);
        let snapshot = harness
            .resolve(&ResolveCtx {
                profile: ProfileKind::Dev,
                cwd: home.root.clone(),
                project_root: home.root.clone(),
                config: config.clone(),
            })
            .unwrap();
        let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("test-key"));
        let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
        let agent = AgentDef {
            name: "budget-probe".into(),
            profile: kanzei_harness::ProfileScope::All,
            model: "mock".into(),
            mode,
            steps: 0,
            system: "read then finish".into(),
        };
        let runner = kanzei_core::RunnerConfig {
            hosted_tools: Vec::new(),
            digest_model: None,
            intensity: kanzei_harness::HarnessIntensity::Autonomous,
            model: "mock".into(),
            max_tokens: 256,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: None,
            context_limit: None,
            limits: config.limits.clone(),
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
            ask_policy: kanzei_core::AskPolicy::Interactive,
            halt: None,
        };
        let ctx = ToolCtx::new(home.root.clone(), home.root.clone());
        let mut events = |_event: kanzei_core::RunEvent| {};
        let mut ask = |_request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
            Box::pin(async {
                kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
            })
        };
        let summary = kanzei_core::run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner,
            &ctx,
            "read sample",
            None,
            &[],
            None,
            None,
            &mut events,
            &mut ask,
        )
        .await
        .unwrap();
        if summary.steps != expected_steps {
            server.abort();
        }
        assert_eq!(summary.steps, expected_steps, "{mode:?}");
        assert!(!summary.step_limit_reached, "{mode:?}");
        let requests = server.await.unwrap();
        let has_tools = |step: usize| {
            requests[step - 1]["tools"]
                .as_array()
                .is_some_and(|t| !t.is_empty())
        };
        assert!(has_tools(31));
        assert!(has_tools(32));
        assert!(has_tools(33));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn d773_long_writer_closes_saves_checkpoint_and_continues_without_asking() {
    struct FixtureCatalog;
    impl kanzei_harness::Component for FixtureCatalog {
        fn contribute(
            &self,
            draft: &mut kanzei_harness::HarnessDraft,
            _: &ResolveCtx,
        ) -> anyhow::Result<()> {
            // 终端夹具没有桌面专用工具；保留已经注册的延迟目录。
            draft
                .deferred_tools
                .retain(|name| draft.tools.get(name).is_some());
            Ok(())
        }
    }
    let home = super::common::TestHome::new("batch-delivery");
    std::fs::write(home.root.join("sample.txt"), "base").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "kanzei"],
        vec!["config", "user.email", "vraniumzwt@gmail.com"],
        vec!["add", "sample.txt"],
        vec!["commit", "-q", "-m", "base"],
    ] {
        let mut command = std::process::Command::new("git");
        command.args(args).current_dir(&home.root);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        assert!(command.output().unwrap().status.success());
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for step in 1..=42 {
            let delta = if step == 42 {
                json!({"content":"done"})
            } else if matches!(step, 33 | 34) {
                let command = if cfg!(windows) {
                    "& 'git' diff --check; git status --short"
                } else {
                    "git diff --check; git status --short"
                };
                json!({"tool_calls":[{
                    "index":0,"id":format!("validate-{step}"),"type":"function",
                    "function":{"name":"bash","arguments":json!({"command":command}).to_string()}
                }]})
            } else {
                json!({"tool_calls":[{
                    "index":0,"id":format!("write-{step}"),"type":"function",
                    "function":{"name":"write","arguments":json!({"path":"sample.txt","content":format!("step {step}")}).to_string()}
                }]})
            };
            let raw = super::memory_hints_not_persisted::serve_response(&listener, json!({
                "choices":[{"index":0,"delta":delta,"finish_reason":if step == 42 {"stop"} else {"tool_calls"}}],
                "usage":{"prompt_tokens":1,"completion_tokens":1}
            })).await;
            let text = String::from_utf8(raw).unwrap();
            requests.push(
                serde_json::from_str::<serde_json::Value>(text.split_once("\r\n\r\n").unwrap().1)
                    .unwrap(),
            );
        }
        requests
    });
    let config = Arc::new(KanzeiConfig::default());
    let mut harness = Harness::default();
    harness
        .add(kanzei_tools::BaseComponent)
        .add(kanzei_tools::DevProfile)
        .add(FixtureCatalog);
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: home.root.clone(),
            project_root: home.root.clone(),
            config: config.clone(),
        })
        .unwrap();
    let agent = AgentDef {
        name: "batch-probe".into(),
        profile: kanzei_harness::ProfileScope::All,
        model: "mock".into(),
        mode: AgentMode::Primary,
        steps: 0,
        system: "write the requested task".into(),
    };
    let runner = kanzei_core::RunnerConfig {
        hosted_tools: Vec::new(),
        digest_model: None,
        intensity: kanzei_harness::HarnessIntensity::Autonomous,
        model: "mock".into(),
        max_tokens: 256,
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: None,
        context_limit: None,
        limits: config.limits.clone(),
        recall: None,
        execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
        ask_policy: kanzei_core::AskPolicy::Interactive,
        halt: None,
    };
    let ctx = ToolCtx::new(home.root.clone(), home.root.clone());
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("test-key"));
    let mut blocked = 0;
    let mut validations = 0;
    let mut events = |event: kanzei_core::RunEvent| {
        if let kanzei_core::RunEvent::ToolEnd {
            name, ok, content, ..
        } = event
        {
            if !ok && content.contains("BATCH_CLOSING") {
                blocked += 1;
            }
            if name == "bash" {
                assert!(ok, "validation command was refused: {content}");
                assert!(content.contains("exit code: 0"), "{content}");
                validations += 1;
            }
        }
    };
    let mut asks = 0;
    let mut ask = |request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        if matches!(request, kanzei_core::AskRequest::Question { .. }) {
            asks += 1;
        }
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce) })
    };
    let summary = kanzei_core::run_once(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner,
        &ctx,
        "write sample",
        None,
        &[],
        None,
        None,
        &mut events,
        &mut ask,
    )
    .await
    .unwrap();
    assert_eq!(summary.steps, 42);
    assert!(!summary.halted_by_user);
    assert_eq!(asks, 0);
    assert_eq!(blocked, 6);
    assert_eq!(validations, 2);
    assert_eq!(
        std::fs::read_to_string(home.root.join("sample.txt")).unwrap(),
        "step 41"
    );
    let requests = server.await.unwrap();
    assert!(requests[32]
        .to_string()
        .contains("Close this implementation batch"));
    assert!(requests[40].to_string().contains("Batch checkpoint saved"));
    let parent = home.root.join(".kanzei/artifacts/batch-checkpoints");
    let directory = std::fs::read_dir(parent)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let checkpoint: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("checkpoint.json")).unwrap()).unwrap();
    assert_eq!(checkpoint["completed"], false);
    assert_eq!(
        std::fs::read_to_string(directory.join("0-after.bin")).unwrap(),
        "step 32"
    );
}
