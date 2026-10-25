use super::*;
use sha2::Digest;
use tokio::{io::AsyncWriteExt, net::TcpListener};

#[tokio::test]
async fn restored_oversized_task_result_reaches_responses_as_a_recoverable_reference() {
    let root = kanzei_base::test_support::TestDir::new("kz-history-large-task-");
    let original = "x".repeat(11_527_136);
    let hash = format!("{:x}", sha2::Sha256::digest(original.as_bytes()));
    let prior = vec![
        Message::assistant(vec![Part::ToolCall {
            id: "old-task-diff".into(),
            name: "task".into(),
            input: json!({"action":"diff","id":"candidate"}),
        }]),
        Message::tool_results(vec![Part::ToolResult {
            call_id: "old-task-diff".into(),
            content: original.clone(),
            is_error: false,
        }]),
    ];
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = super::input::read_request(&mut socket).await;
        let result = request["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .unwrap();
        assert_eq!(result["call_id"], "old-task-diff");
        let output = result["output"].as_str().unwrap();
        assert!(
            output.len() <= 10_485_760,
            "provider's per-output limit must hold"
        );
        assert!(output.starts_with("[tool_result_externalized"));
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        for event in [
            json!({"type":"response.output_text.delta","output_index":0,"delta":"RECOVERED"}),
            json!({"type":"response.completed","response":{"usage":{"input_tokens":1,"output_tokens":1}}}),
        ] {
            socket
                .write_all(format!("data: {event}\n\n").as_bytes())
                .await
                .unwrap();
        }
    });
    let mut harness = Harness::default();
    harness.add(Policy);
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::DevAuto,
            cwd: root.to_path_buf(),
            project_root: root.to_path_buf(),
            config: Arc::new(Default::default()),
        })
        .unwrap();
    let ctx = ToolCtx::new(root.to_path_buf(), root.to_path_buf());
    let agent =
        serde_json::from_value(json!({"name":"fixture","model":"fixture","max_steps":1})).unwrap();
    let client = LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let route = Route::openai_responses_at(&format!("http://{address}/v1"), vec![]);
    let mut cfg = config(Default::default());
    // Keep token compaction out of the test: a per-result invariant must hold
    // even with a very large configured context window.
    cfg.context_limit = Some(100_000_000);
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &cfg,
            &ctx,
            "continue",
            None,
            &prior,
            None,
            &mut |_| {},
            &mut |_| panic!("recovering engine-owned result storage requires no approval"),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    server.await.unwrap();
    assert_eq!(outcome.text, "RECOVERED");
    assert!(
        matches!(&prior[1].parts[0], Part::ToolResult {content, ..} if content == &original),
        "the caller's durable history must remain intact"
    );
    assert_eq!(
        std::fs::read(root.join(format!(
            ".kanzei/artifacts/tool-results/tool-task-{hash}.txt"
        )))
        .unwrap(),
        original.as_bytes()
    );
}
