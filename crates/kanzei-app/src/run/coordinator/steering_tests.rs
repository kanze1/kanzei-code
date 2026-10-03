use super::*;
use kanzei_core::{RunEvent, SessionFact, SessionStore, SessionTurnTerminal};
use kanzei_harness::{
    rule, Component, Effect, Harness, HarnessDraft, KanzeiConfig, ProfileKind, ResolveCtx, Tool,
    ToolCtx,
};
use kanzei_llm::{Message, Part, Role};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "kz-c7-steering-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct CountingTool(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl Tool for CountingTool {
    fn name(&self) -> &'static str {
        "steering_probe"
    }

    fn description(&self) -> String {
        "Count a real tool batch before receiving new input".into()
    }

    fn input_schema(&self) -> Value {
        json!({"type":"object", "properties":{}})
    }

    async fn execute(&self, _input: Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        self.0.fetch_add(1, Ordering::SeqCst);
        kanzei_harness::ToolOutput::ok("tool batch executed")
    }
}

struct ProbeComponent(Arc<AtomicUsize>);

impl Component for ProbeComponent {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        draft
            .tools
            .insert("steering_probe", Arc::new(CountingTool(self.0.clone())));
        draft
            .permissions
            .push(rule("steering_probe", "*", Effect::Allow));
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
enum FailurePoint {
    None,
    RecoveredDraft,
    SecondSteer,
    InputFinish,
    StopBeforeTake,
}

fn execute_sql(path: &Path, sql: &str) {
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

fn input_states(path: &Path) -> BTreeMap<String, String> {
    let connection = rusqlite::Connection::open(path).unwrap();
    let mut query = connection
        .prepare("SELECT input_id, status FROM session_inputs ORDER BY created_at, rowid")
        .unwrap();
    query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

async fn respond(listener: &TcpListener, body: Value, requests: &Mutex<Vec<Value>>) {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    let header_end = loop {
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        request.extend_from_slice(&buffer[..read]);
        if let Some(position) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    while request.len() < header_end + length {
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        request.extend_from_slice(&buffer[..read]);
    }
    requests
        .lock_or_recover()
        .push(serde_json::from_slice(&request[header_end..header_end + length]).unwrap());
    let data = format!("data: {body}\n\ndata: [DONE]\n\n");
    let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            data.len()
        );
    socket.write_all(header.as_bytes()).await.unwrap();
    socket.write_all(data.as_bytes()).await.unwrap();
}

fn user_texts(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| message.role == Role::User)
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

async fn run_fixture(point: FailurePoint) -> Value {
    let fixture = Fixture::new();
    let path = fixture.0.join("state.db");
    let store = SessionStore::open(&path).unwrap();
    store
        .create_session("ses", fixture.0.to_str().unwrap(), None)
        .unwrap();
    let owner = Arc::new(kanzei_core::store::session_execution::try_acquire(&path, "ses").unwrap());
    owner.prepare(&store, "ses", None).unwrap();
    store
        .admit_input("ses", "initial", "prompt", kanzei_core::Delivery::Queue)
        .unwrap();
    assert_eq!(
        store.promote_next_input("ses").unwrap().unwrap().input_id,
        "initial"
    );
    assert!(store.start_input("initial").unwrap());
    store.set_status("ses", "running").unwrap();
    let writer = Arc::new(Mutex::new(owner.writer("turn")));
    assert!(writer
        .lock_or_recover()
        .user_message("initial", Message::user_text("prompt")));
    match point {
        FailurePoint::RecoveredDraft => execute_sql(
            &path,
            "CREATE TRIGGER reject_draft BEFORE INSERT ON session_events
                 WHEN NEW.event_type = 'session.assistant_draft_appended'
                 BEGIN SELECT RAISE(ABORT, 'fixture rejects first draft'); END;",
        ),
        FailurePoint::SecondSteer => execute_sql(
            &path,
            "CREATE TRIGGER reject_second_steer BEFORE INSERT ON session_events
                 WHEN NEW.event_type = 'session.steering_message_committed'
                   AND json_extract(NEW.payload_json, '$.fact.input_id') = 'steer-b'
                 BEGIN SELECT RAISE(ABORT, 'fixture rejects second steer'); END;",
        ),
        FailurePoint::InputFinish => execute_sql(
            &path,
            "CREATE TRIGGER reject_steer_finish BEFORE UPDATE ON session_inputs
                 WHEN NEW.input_id = 'steer-a' AND NEW.status = 'completed'
                 BEGIN SELECT RAISE(ABORT, 'fixture rejects input completion'); END;",
        ),
        FailurePoint::None | FailurePoint::StopBeforeTake => {}
    }
    let runtime = Arc::new(crate::SessionRuntime::default());
    let halt_token = kanzei_core::CancellationToken::new();
    *runtime.halt.lock_or_recover() = Some(halt_token.clone());
    runtime.running.store(true, Ordering::SeqCst);
    let mut ctx = ToolCtx::new(fixture.0.clone(), fixture.0.clone()).with_session_id("ses".into());
    ctx.input_inbox = Some(steering_inbox(
        writer.clone(),
        runtime.clone(),
        halt_token.clone(),
    ));
    if matches!(point, FailurePoint::StopBeforeTake) {
        let inbox = ctx.input_inbox.take().unwrap();
        let inbox_runtime = runtime.clone();
        let inbox_path = path.clone();
        let takes = AtomicUsize::new(0);
        ctx.input_inbox = Some(kanzei_harness::InputInbox::new(move || {
            // On the second real take, Runner already passed its own halt check.
            // Stop wins before the production closure obtains lifecycle.
            if takes.fetch_add(1, Ordering::SeqCst) == 1 {
                let store = SessionStore::open(&inbox_path).unwrap();
                crate::stop_runtime_and_finalize(&inbox_runtime, &store, &inbox_path, "ses")
                    .unwrap();
                let _lifecycle = inbox_runtime.lifecycle.lock_or_recover();
                assert!(inbox_runtime.running.load(Ordering::SeqCst));
                assert!(inbox_runtime.halt.lock_or_recover().is_none());
                // This matches a legal manual steer while cooperative Stop has
                // cancelled the old input but the old run is still settling.
                store
                    .admit_input(
                        "ses",
                        "steer-after-stop",
                        "new task after stop",
                        kanzei_core::Delivery::Steer,
                    )
                    .unwrap();
            }
            inbox.take()
        }));
    }

    let executions = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_requests = requests.clone();
    let server = tokio::spawn(async move {
        respond(
            &listener,
            json!({"choices":[{
                "index":0,
                "delta":{"content":"x".repeat(3000), "tool_calls":[{
                    "index":0,"id":"probe-call","type":"function",
                    "function":{"name":"steering_probe","arguments":"{}"}
                }]}, "finish_reason":"tool_calls"
            }],"usage":{"prompt_tokens":2,"completion_tokens":1}}),
            &server_requests,
        )
        .await;
        respond(
            &listener,
            json!({"choices":[{
                "index":0,"delta":{"content":"done"},"finish_reason":"stop"
            }],"usage":{"prompt_tokens":2,"completion_tokens":1}}),
            &server_requests,
        )
        .await;
    });
    let mut harness = Harness::default();
    harness.add(ProbeComponent(executions.clone()));
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: fixture.0.clone(),
            project_root: fixture.0.clone(),
            config: Arc::new(KanzeiConfig::default()),
        })
        .unwrap();
    let agent = serde_json::from_value(json!({
        "name":"fixture","profile":"dev","mode":"primary","steps":3,"system":"test"
    }))
    .unwrap();
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), None);
    let config = kanzei_core::RunnerConfig {
        hosted_tools: vec![],
        digest_model: None,
        intensity: kanzei_harness::HarnessIntensity::Autonomous,
        model: "mock".into(),
        max_tokens: 128,
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: None,
        context_limit: None,
        limits: Default::default(),
        recall: None,
        execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
        ask_policy: kanzei_core::AskPolicy::NonInteractive,
        halt: Some(halt_token.clone()),
    };

    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let emit_events = events.clone();
    let emit_writer = writer.clone();
    let emit_path = path.clone();
    let draft_retried = Arc::new(AtomicBool::new(false));
    let emit_draft_retried = draft_retried.clone();
    let result = {
        let mut handler = build_event_handler(
            UiEventSink::new(
                move |name, payload| {
                    emit_events
                        .lock_or_recover()
                        .push(json!({"name":name,"payload":payload}));
                    if name == "kz:text"
                        && matches!(point, FailurePoint::RecoveredDraft)
                        && !emit_draft_retried.load(Ordering::SeqCst)
                        && !emit_writer.lock_or_recover().errors().is_empty()
                    {
                        // The actual Text sink already attempted and lost this SQL write.
                        // Remove only that real failure so assistant commit can retry the buffer.
                        execute_sql(&emit_path, "DROP TRIGGER reject_draft;");
                        emit_draft_retried.store(true, Ordering::SeqCst);
                    }
                    Ok(())
                },
                "ses".into(),
                "turn".into(),
            ),
            TypedEventSink::new(writer.clone()),
            TraceSink::new(
                runtime.live.clone(),
                path.clone(),
                "ses".into(),
                "turn".into(),
            ),
            MetricsSink::new(
                Arc::new(Mutex::new(HashMap::new())),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                Arc::new(Mutex::new(Default::default())),
                Arc::new(Mutex::new(Default::default())),
                Arc::new(AtomicU32::new(0)),
                Arc::new(Mutex::new(None)),
            ),
        );
        let admit_path = path.clone();
        let mut admitted = false;
        let mut sink = |event| {
            let result_receipt = match &event {
                RunEvent::ToolResultsCommitted { commit, .. } => Some(commit.clone()),
                _ => None,
            };
            handler(event);
            if let Some(receipt) = result_receipt {
                assert!(
                    receipt.check().is_ok(),
                    "the actual completed tool batch must be durable"
                );
                if !admitted {
                    let store = SessionStore::open(&admit_path).unwrap();
                    for (id, prompt) in [
                        ("steer-a", "steer a"),
                        ("steer-b", "steer b"),
                        ("steer-c", "steer c"),
                    ] {
                        store
                            .admit_input("ses", id, prompt, kanzei_core::Delivery::Steer)
                            .unwrap();
                    }
                    admitted = true;
                }
            }
        };
        let mut ask = |_| -> kanzei_core::AskFuture {
            Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            kanzei_core::run_once(
                &client,
                &route,
                &snapshot,
                &agent,
                &config,
                &ctx,
                "prompt",
                None,
                &[],
                None,
                None,
                &mut sink,
                &mut ask,
            ),
        )
        .await
        .unwrap()
    };
    server.abort();
    let _ = server.await;
    let states_before_outcome = input_states(&path);
    let received_inputs: Vec<String> = events
        .lock_or_recover()
        .iter()
        .filter(|event| event["name"] == "kz:input-received")
        .map(|event| event["payload"]["inputId"].as_str().unwrap().to_string())
        .collect();
    let (outcome_error, round_inputs) = match &result {
        Ok(summary) => (None, Some(user_texts(&summary.round_messages))),
        Err(error) => (Some(error.to_string()), None),
    };
    // This is the same C5 durable finalizer used by the desktop coordinator.
    super::super::persistence::commit_outcome(
        &writer,
        "initial",
        match &result {
            Ok(summary) if summary.halted_by_user => SessionTurnTerminal::Stopped,
            Ok(_) => SessionTurnTerminal::Completed,
            Err(error) => SessionTurnTerminal::Failed(error.to_string()),
        },
        &json!({"run_id":"turn","fixture":"C7"}),
    )
    .unwrap();
    runtime.running.store(false, Ordering::SeqCst);
    let facts = store.list_session_facts("ses").unwrap();
    let steering_ids: Vec<_> = facts
        .iter()
        .filter_map(|(_, fact)| match &fact.fact {
            SessionFact::SteeringMessageCommitted { input_id, .. } => Some(input_id.clone()),
            _ => None,
        })
        .collect();
    let draft_count = facts
        .iter()
        .filter(|(_, fact)| matches!(&fact.fact, SessionFact::AssistantDraftAppended { .. }))
        .count();
    let retried_first_draft_count = facts
        .iter()
        .filter(|(_, fact)| {
            fact.step_id == Some(1)
                && matches!(&fact.fact, SessionFact::AssistantDraftAppended { text, .. }
                        if text == &"x".repeat(3000))
        })
        .count();
    let terminal_types: Vec<_> = facts
        .iter()
        .filter_map(|(_, fact)| match &fact.fact {
            SessionFact::TurnCompleted => Some("completed"),
            SessionFact::TurnFailed { .. } => Some("failed"),
            SessionFact::TurnStopped => Some("stopped"),
            _ => None,
        })
        .collect();
    let promoted_ids: Vec<_> = store
        .list_events_by_type("ses", 0, "prompt.promoted")
        .unwrap()
        .into_iter()
        .filter_map(|event| {
            event.payload["input_id"]
                .as_str()
                .filter(|id| *id != "initial")
                .map(str::to_owned)
        })
        .collect();
    let states_after_outcome = input_states(&path);
    owner.prepare(&store, "ses", None).unwrap();
    let proof = json!({
        "scenario":format!("{point:?}"), "outcome_error":outcome_error,
        "requests":requests.lock_or_recover().clone(),
        "tool_executions":executions.load(Ordering::SeqCst),
        "states_before_outcome":states_before_outcome,
        "states_after_outcome":states_after_outcome,
        "states_after_recovery":input_states(&path),
        "steering_ids":steering_ids,"promoted_ids":promoted_ids,
        "received_inputs":received_inputs,"round_inputs":round_inputs,
        "draft_retried":draft_retried.load(Ordering::SeqCst),"draft_count":draft_count,
        "retried_first_draft_count":retried_first_draft_count,"terminal_types":terminal_types,
        "halt_cancelled":halt_token.is_cancelled(),
        "halt_slot_empty":runtime.halt.lock_or_recover().is_none(),
        "typed_errors":writer.lock_or_recover().errors(),
        "session_status":store.get_session("ses").unwrap().unwrap().status,
    });
    eprintln!("C7_PROOF={proof}");
    proof
}

fn assert_normal_delivery(proof: &Value) {
    assert!(proof["outcome_error"].is_null(), "{proof}");
    assert_eq!(proof["requests"].as_array().unwrap().len(), 2, "{proof}");
    assert_eq!(proof["tool_executions"], 1);
    assert_eq!(proof["session_status"], "idle");
    assert_eq!(proof["terminal_types"], json!(["completed"]));
    assert_eq!(
        proof["steering_ids"],
        json!(["steer-a", "steer-b", "steer-c"])
    );
    assert_eq!(proof["received_inputs"], proof["steering_ids"]);
    assert_eq!(
        proof["round_inputs"],
        json!(["prompt", "steer a", "steer b", "steer c"])
    );
    for id in ["initial", "steer-a", "steer-b", "steer-c"] {
        assert_eq!(proof["states_after_recovery"][id], "completed", "{proof}");
    }
    let second_request = proof["requests"][1].to_string();
    let positions: Vec<_> = ["steer a", "steer b", "steer c"]
        .iter()
        .map(|text| {
            second_request
                .find(text)
                .expect("provider received this FIFO input")
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

fn assert_batch_rollback(proof: &Value) {
    assert!(proof["outcome_error"].is_string(), "{proof}");
    assert_eq!(proof["requests"].as_array().unwrap().len(), 1, "{proof}");
    assert_eq!(proof["tool_executions"], 1);
    assert_eq!(proof["session_status"], "failed");
    assert_eq!(proof["terminal_types"], json!(["failed"]));
    assert_eq!(proof["received_inputs"], json!([]), "{proof}");
    assert_eq!(
        proof["steering_ids"],
        json!([]),
        "rejected delivery has no committed prefix: {proof}"
    );
    assert_eq!(proof["promoted_ids"], json!([]), "{proof}");
    assert_eq!(proof["states_after_recovery"]["initial"], "failed");
    for id in ["steer-a", "steer-b", "steer-c"] {
        assert_eq!(proof["states_before_outcome"][id], "pending", "{proof}");
        assert_eq!(proof["states_after_recovery"][id], "pending", "{proof}");
    }
}

#[tokio::test]
async fn steering_normal_fifo_reaches_real_provider_and_receipt() {
    assert_normal_delivery(&run_fixture(FailurePoint::None).await);
}

#[tokio::test]
async fn steering_recovered_draft_error_does_not_reject_successful_input() {
    let proof = run_fixture(FailurePoint::RecoveredDraft).await;
    assert_eq!(
        proof["draft_retried"], true,
        "real SQL draft rejection was observed"
    );
    assert_eq!(
        proof["retried_first_draft_count"], 1,
        "the rejected first step's retained draft was retried successfully"
    );
    assert!(!proof["typed_errors"].as_array().unwrap().is_empty());
    assert_normal_delivery(&proof);
}

#[tokio::test]
async fn steering_second_fact_rejection_rolls_back_entire_delivery() {
    assert_batch_rollback(&run_fixture(FailurePoint::SecondSteer).await);
}

#[tokio::test]
async fn steering_input_completion_rejection_rolls_back_its_fact() {
    assert_batch_rollback(&run_fixture(FailurePoint::InputFinish).await);
}

#[tokio::test]
async fn steering_stop_before_take_does_not_consume_new_admission_in_old_turn() {
    let proof = run_fixture(FailurePoint::StopBeforeTake).await;
    assert!(proof["outcome_error"].is_null(), "{proof}");
    assert_eq!(proof["halt_cancelled"], true);
    assert_eq!(proof["halt_slot_empty"], true);
    assert_eq!(proof["requests"].as_array().unwrap().len(), 1, "{proof}");
    assert_eq!(proof["tool_executions"], 1);
    assert_eq!(proof["terminal_types"], json!(["stopped"]));
    assert_eq!(proof["received_inputs"], json!([]), "{proof}");
    assert_eq!(proof["steering_ids"], json!([]), "{proof}");
    for id in ["initial", "steer-a", "steer-b", "steer-c"] {
        assert_eq!(proof["states_after_recovery"][id], "cancelled", "{proof}");
    }
    assert_eq!(
        proof["states_after_recovery"]["steer-after-stop"], "pending",
        "{proof}"
    );
}
