//! A completed tool item is not permission to execute before the turn completes.
use kanzei_harness::{
    AgentDef, AgentMode, Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx,
};
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn incomplete_responses_turn_never_executes_an_already_emitted_tool_call() {
    let home = super::common::TestHome::new("incomplete-stream");
    std::fs::write(home.root.join("sample.txt"), "must not be read by a tool").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        {
            let mut reader = tokio::io::BufReader::new(&mut socket);
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).await.unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
            }
            reader.read_exact(&mut vec![0; length]).await.unwrap();
        }
        let item = json!({"type":"function_call","call_id":"read-1","name":"read","arguments":"{\"path\":\"sample.txt\"}"});
        let body = format!(
            "data: {}\n\n",
            json!({"type":"response.output_item.done","output_index":0,"item":item})
        );
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        socket.write_all(response.as_bytes()).await.unwrap();
        // Clean HTTP EOF, deliberately without response.completed/incomplete.
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
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let route = kanzei_llm::Route::openai_responses_at(&format!("http://{address}"), vec![]);
    let agent = AgentDef {
        name: "incomplete-stream".into(),
        profile: kanzei_harness::ProfileScope::All,
        model: "mock".into(),
        mode: AgentMode::Primary,
        steps: 2,
        system: "read sample".into(),
    };
    let runner = kanzei_core::RunnerConfig {
        hosted_tools: vec![],
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
    let mut tool_starts = 0;
    let mut step_ends = 0;
    let mut events = |event| match event {
        kanzei_core::RunEvent::ToolStart { .. } => tool_starts += 1,
        kanzei_core::RunEvent::StepEnd { .. } => step_ends += 1,
        _ => {}
    };
    let mut ask = |_request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce) })
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        kanzei_core::run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner,
            &ctx,
            "read sample.txt",
            None,
            &[],
            None,
            None,
            &mut events,
            &mut ask,
        ),
    )
    .await
    .unwrap();
    server.await.unwrap();
    assert!(result.is_err());
    assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("completion event"));
    assert_eq!(
        tool_starts, 0,
        "an incomplete model turn must not execute tools"
    );
    assert_eq!(
        step_ends, 0,
        "HTTP EOF must not be reported as successful completion"
    );
}

#[tokio::test]
async fn redacted_thinking_survives_runner_history_without_a_fake_tool_event() {
    let home = super::common::TestHome::new("redacted-thinking");
    std::fs::write(home.root.join("sample.txt"), "must not be read by a tool").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        {
            let mut reader = tokio::io::BufReader::new(&mut socket);
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).await.unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
            }
            reader.read_exact(&mut vec![0; length]).await.unwrap();
        }
        let frames = [
            json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"redacted_thinking","data":"opaque"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"done"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
            json!({"type":"message_stop"}),
        ];
        let body: String = frames
            .iter()
            .map(|frame| format!("data: {frame}\n\n"))
            .collect();
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        socket.write_all(response.as_bytes()).await.unwrap();
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
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let route = kanzei_llm::Route::anthropic_with_headers_at(&format!("http://{address}"), vec![]);
    let agent = AgentDef {
        name: "redacted-thinking".into(),
        profile: kanzei_harness::ProfileScope::All,
        model: "mock".into(),
        mode: AgentMode::Primary,
        steps: 2,
        system: "read sample".into(),
    };
    let runner = kanzei_core::RunnerConfig {
        hosted_tools: vec![],
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
    let mut tool_starts = 0;
    let mut step_ends = 0;
    let mut hosted_events = 0;
    let mut events = |event| match event {
        kanzei_core::RunEvent::ToolStart { .. } => tool_starts += 1,
        kanzei_core::RunEvent::StepEnd { .. } => step_ends += 1,
        kanzei_core::RunEvent::HostedTool { .. } => hosted_events += 1,
        _ => {}
    };
    let mut ask = |_request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce) })
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        kanzei_core::run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner,
            &ctx,
            "read sample.txt",
            None,
            &[],
            None,
            None,
            &mut events,
            &mut ask,
        ),
    )
    .await
    .unwrap();
    server.await.unwrap();
    let summary = result.unwrap();
    let filtered = kanzei_core::history::filter_message_history(&summary.messages);
    let serialized = serde_json::to_string(&filtered).unwrap();
    let restored: Vec<kanzei_llm::Message> = serde_json::from_str(&serialized).unwrap();
    assert!(restored.iter().flat_map(|m| &m.parts).any(|part| matches!(part,
        kanzei_llm::Part::Hosted {kind, raw, ..} if kind == "redacted_thinking" && raw["data"] == "opaque")));
    assert_eq!(tool_starts, 0);
    assert_eq!(step_ends, 1);
    assert_eq!(hosted_events, 0, "opaque reasoning is not a web search");
}
