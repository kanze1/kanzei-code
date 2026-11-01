use super::*;
use std::{collections::VecDeque, sync::Mutex};
use tokio::{io::AsyncWriteExt, net::TcpListener};

struct ResponseTool(Mutex<VecDeque<ToolOutput>>, AtomicUsize);

struct LookupTool;
#[async_trait]
impl Tool for LookupTool {
    fn name(&self) -> &'static str {
        "lookup"
    }
    fn action(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> String {
        "Find material before responding".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object"})
    }
    async fn execute(&self, _: serde_json::Value, _: &ToolCtx) -> ToolOutput {
        ToolOutput::ok("retrieved material")
    }
}

#[async_trait]
impl Tool for ResponseTool {
    fn name(&self) -> &'static str {
        "respond"
    }
    fn action(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> String {
        "Deliver the final answer".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({"type":"object"})
    }
    fn response_mode(&self) -> kanzei_harness::tool::ToolResponseMode {
        kanzei_harness::tool::ToolResponseMode::FinalResponse
    }
    async fn execute(&self, _: serde_json::Value, _: &ToolCtx) -> ToolOutput {
        self.1.fetch_add(1, Ordering::SeqCst);
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected duplicate send")
    }
}

async fn run_case(
    outputs: Vec<ToolOutput>,
    batches: Vec<Vec<&str>>,
    reject_response: bool,
) -> (
    anyhow::Result<RunSummary>,
    usize,
    usize,
    Vec<String>,
    Vec<Message>,
) {
    let root = kanzei_base::test_support::TestDir::new("final-response");
    let path = crate::project_state_path(&root);
    let store = crate::SessionStore::open(&path).unwrap();
    store
        .create_session("ses", &root.to_string_lossy(), None)
        .unwrap();
    let mut writer = crate::TypedSessionWriter::new(&path, "ses", "turn");
    assert!(writer.user_message("input", Message::user_text("please reply")));
    let tool = Arc::new(ResponseTool(
        Mutex::new(outputs.into()),
        AtomicUsize::new(0),
    ));
    let mut harness = Harness::default();
    harness
        .add(Policy)
        .add(SnapshotTool(tool.clone()))
        .add(SnapshotTool(Arc::new(LookupTool)));
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::DevAuto,
            cwd: root.to_path_buf(),
            project_root: root.to_path_buf(),
            config: Arc::new(Default::default()),
        })
        .unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let count = requests.clone();
    let batches: Vec<Vec<String>> = batches
        .into_iter()
        .map(|b| b.into_iter().map(str::to_owned).collect())
        .collect();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            input::read_request(&mut socket).await;
            let index = count.fetch_add(1, Ordering::SeqCst);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
            let calls: Vec<_> = batches.get(index).into_iter().flatten().enumerate().map(|(i,name)| json!({"index":i,"id":format!("call-{index}-{i}"),"type":"function","function":{"name":name,"arguments":"{}"}})).collect();
            let (delta, reason) = if calls.is_empty() {
                (json!({"content":"CONFLICTING_LOCAL_ANSWER"}), "stop")
            } else {
                (json!({"tool_calls":calls}), "tool_calls")
            };
            for data in [
                json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}),
            ] {
                socket
                    .write_all(format!("data: {data}\n\n").as_bytes())
                    .await
                    .unwrap();
            }
            socket.write_all(b"data: [DONE]\n\n").await.unwrap();
        }
    });
    let mut visible = Vec::new();
    let client = LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let agent =
        serde_json::from_value(json!({"name":"fixture","model":"fixture","max_steps":5})).unwrap();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_once(
            &client,
            &Route::openai_at(&format!("http://{address}/v1"), None),
            &snapshot,
            &agent,
            &config(Default::default()),
            &ToolCtx::new(root.to_path_buf(), root.to_path_buf()),
            "please reply",
            None,
            &[],
            None,
            &mut |event| {
                let accepted = match event {
                    RunEvent::TurnStart {
                        step, max_steps, ..
                    } => {
                        writer.turn_started(step, max_steps);
                        true
                    }
                    RunEvent::AssistantMessageCommitted {
                        step,
                        message,
                        commit,
                    } => {
                        let ok = writer.assistant_committed(step, message);
                        if !ok {
                            commit.reject("assistant persistence rejected");
                        }
                        ok
                    }
                    RunEvent::ToolResultsCommitted {
                        step,
                        message,
                        commit,
                    } => {
                        let ok = writer.tool_results_committed(step, message);
                        if !ok {
                            commit.reject("result persistence rejected");
                        }
                        ok
                    }
                    RunEvent::ToolResponseCommitted {
                        step,
                        call_id,
                        message,
                        commit,
                    } => {
                        if reject_response {
                            commit.reject("response persistence rejected");
                            true
                        } else {
                            let ok = writer.tool_response_committed(step, &call_id, message);
                            if !ok {
                                commit.reject("response persistence rejected");
                            }
                            ok
                        }
                    }
                    RunEvent::Text(text) => {
                        visible.push(text);
                        true
                    }
                    _ => true,
                };
                assert!(accepted, "{:?}", writer.errors());
            },
            &mut |_| panic!("no approval needed"),
        ),
    )
    .await
    .unwrap();
    if let Ok(summary) = &outcome {
        writer.finish(summary.terminal());
    }
    server.abort();
    let facts = store.list_session_facts("ses").unwrap();
    let committed = facts
        .into_iter()
        .filter_map(|(_, fact)| match fact.fact {
            crate::SessionFact::AssistantMessageCommitted { message, .. } => Some(message),
            _ => None,
        })
        .collect();
    (
        outcome,
        requests.load(Ordering::SeqCst),
        tool.1.load(Ordering::SeqCst),
        visible,
        committed,
    )
}

#[tokio::test]
async fn successful_delivery_owns_live_summary_and_durable_answer_without_another_model_call() {
    let large = "large delivered response ".repeat(4_000);
    for text in ["actual sent text", "", large.as_str()] {
        let (outcome, requests, sends, visible, committed) =
            run_case(vec![ToolOutput::ok(text)], vec![vec!["respond"]], false).await;
        let summary = outcome.unwrap();
        assert_eq!((requests, sends, summary.steps), (1, 1, 1));
        assert_eq!(summary.text, text);
        assert_eq!(summary.outcome(), "completed");
        assert_eq!(visible.concat(), text);
        assert!(
            matches!(committed.last().unwrap().parts.as_slice(), [Part::Text {text:body}] if body==text)
        );
        assert_eq!(summary.round_messages.last(), summary.messages.last());
    }
}

#[tokio::test]
async fn failed_or_unknown_delivery_remains_an_observation_for_recovery() {
    let (outcome, requests, sends, _, _) = run_case(
        vec![ToolOutput::error("unknown transport result")],
        vec![vec!["respond"]],
        false,
    )
    .await;
    assert_eq!((requests, sends), (2, 1));
    assert_eq!(outcome.unwrap().text, "CONFLICTING_LOCAL_ANSWER");
}

#[tokio::test]
async fn batched_responses_are_paired_but_not_sent_until_called_alone() {
    let (outcome, requests, sends, _, _) = run_case(
        vec![ToolOutput::ok("sent after preparation")],
        vec![vec!["respond", "lookup"], vec!["respond"]],
        false,
    )
    .await;
    let summary = outcome.unwrap();
    assert_eq!((requests, sends), (2, 1));
    assert_eq!(summary.text, "sent after preparation");
    assert_eq!(
        summary
            .messages
            .iter()
            .flat_map(|m| &m.parts)
            .filter(|p| matches!(p, Part::ToolResult { is_error: true, .. }))
            .count(),
        1
    );
    assert!(summary.messages.iter().flat_map(|m| &m.parts).any(|p| matches!(p, Part::ToolResult {content,is_error:false,..} if content=="retrieved material")));
}

#[test]
fn delivered_response_identity_requires_resolution_and_cannot_be_committed_twice() {
    let root = kanzei_base::test_support::TestDir::new("response-identity");
    let path = crate::project_state_path(&root);
    let store = crate::SessionStore::open(&path).unwrap();
    store
        .create_session("ses", &root.to_string_lossy(), None)
        .unwrap();
    let mut writer = crate::TypedSessionWriter::new(&path, "ses", "turn");
    assert!(writer.user_message("input", Message::user_text("reply")));
    writer.turn_started(1, 5);
    assert!(writer.assistant_committed(
        1,
        Message::assistant(vec![Part::ToolCall {
            id: "call".into(),
            name: "respond".into(),
            input: json!({})
        }])
    ));
    let response = Message::assistant(vec![Part::Text {
        text: "delivered".into(),
    }]);
    assert!(!writer.tool_response_committed(1, "call", response.clone()));
    assert!(writer.tool_results_committed(
        1,
        Message::tool_results(vec![Part::ToolResult {
            call_id: "call".into(),
            content: "delivered".into(),
            is_error: false
        }])
    ));
    assert!(!writer.tool_response_committed(2, "call", response.clone()));
    assert!(!writer.tool_response_committed(1, "unknown", response.clone()));
    assert!(writer.tool_response_committed(1, "call", response.clone()));
    assert!(!writer.tool_response_committed(1, "call", response));
}

#[tokio::test]
async fn a_rejected_durable_response_is_never_shown_as_a_committed_answer() {
    let (outcome, requests, sends, visible, committed) = run_case(
        vec![ToolOutput::ok("delivered")],
        vec![vec!["respond"]],
        true,
    )
    .await;
    assert!(outcome
        .err()
        .unwrap()
        .to_string()
        .contains("response persistence rejected"));
    assert_eq!((requests, sends), (1, 1));
    assert!(visible.is_empty());
    assert_eq!(committed.len(), 1);
}
