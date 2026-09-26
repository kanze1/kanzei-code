//! Verify actual provider requests past step 32, not just the budget helper.
use kanzei_harness::{
    AgentDef, AgentMode, Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx,
};
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn primary_continues_past_32_while_subagent_finishes_at_32() {
    for (mode, expected_steps) in [(AgentMode::Primary, 34u32), (AgentMode::Subagent, 32)] {
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
        let requests = server.await.unwrap();
        let has_tools = |step: usize| {
            requests[step - 1]["tools"]
                .as_array()
                .is_some_and(|t| !t.is_empty())
        };
        assert!(has_tools(31));
        if mode == AgentMode::Primary {
            assert!(has_tools(32));
            assert!(has_tools(33));
        } else {
            assert!(!has_tools(32));
        }
    }
}
