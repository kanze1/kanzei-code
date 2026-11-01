//! Exercise real HTTP cancellation, step commits and request prefix projection.
use kanzei_core::{run_once, AskFuture, AskPolicy, RunEvent, RunnerConfig};
use kanzei_harness::{Component, Harness, HarnessDraft, ResolveCtx, Tool, ToolCtx, ToolOutput};
use kanzei_llm::{LlmClient, LlmRequest, Message, Part, ProxyConfig, RequestPriority, Route};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{oneshot, Notify},
};

fn config() -> RunnerConfig {
    RunnerConfig {
        run_context: vec![("runtime/run-facts".into(), "run=A; started=first".into())],
        digest_model: None,
        intensity: Default::default(),
        model: "fixture".into(),
        max_tokens: 256,
        reasoning: Default::default(),
        service_tier: None,
        hosted_tools: vec![],
        context_limit: Some(64_000),
        limits: Default::default(),
        recall: None,
        execution_policy: Default::default(),
        ask_policy: AskPolicy::NonInteractive,
        halt: None,
    }
}

async fn read_request(socket: &mut TcpStream) -> Value {
    let mut data = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let count = socket.read(&mut buffer).await.unwrap();
        assert_ne!(count, 0);
        data.extend_from_slice(&buffer[..count]);
        if let Some(start) = data.windows(4).position(|b| b == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&data[..start]);
            let length: usize = head
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            if data.len() >= start + 4 + length {
                return serde_json::from_slice(&data[start + 4..start + 4 + length]).unwrap();
            }
        }
    }
}

async fn respond(socket: &mut TcpStream, delta: Value, finish: &str) {
    let body = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":delta,"finish_reason":finish}],"usage":{"prompt_tokens":100,"completion_tokens":5}})
    );
    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
}

fn agent() -> kanzei_harness::AgentDef {
    serde_json::from_value(json!({"name":"fixture","profile":"dev-auto","mode":"primary","steps":4,"system":"Stable instructions"})).unwrap()
}

#[tokio::test]
#[ignore = "requires an idle local model, KANZEI_LOCAL_PROBE_URL/MODEL and lifecycle evidence"]
async fn real_local_changing_runtime_context_reuses_history_cache() {
    let url = std::env::var("KANZEI_LOCAL_PROBE_URL").expect("set local model URL");
    let model = std::env::var("KANZEI_LOCAL_PROBE_MODEL").expect("set local model name");
    let evidence = std::path::PathBuf::from(
        std::env::var("KANZEI_RUN_EVIDENCE").expect("run through artifact-lifecycle exec"),
    );
    let root = tempfile::tempdir().unwrap();
    let version = Arc::new(AtomicUsize::new(0));
    let snapshot = snapshot(root.path(), Sources(version.clone()));
    let route = Route::openai_at(&url, None);
    let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
    let prior = vec![
        Message::user_text(
            "Historical record: keep the verified source and its scope.\n".repeat(600),
        ),
        Message::assistant(vec![Part::Text {
            text: "Acknowledged historical records.".into(),
        }]),
    ];
    let mut samples = Vec::new();
    for index in 0..2 {
        let mut config = config();
        config.model = model.clone();
        config.max_tokens = 64;
        config.run_context[0].1 = format!("run={index}; started={index}");
        version.store(index, Ordering::SeqCst);
        let start = std::time::Instant::now();
        let mut first = None;
        let summary = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            run_once(
                &client,
                &route,
                &snapshot,
                &agent(),
                &config,
                &ToolCtx::new(root.path().into(), root.path().into()),
                "只回复：收到",
                Some(&format!("MEMORY_VERSION_{index}")),
                &prior,
                None,
                &mut |event| {
                    if matches!(event, RunEvent::Text(_) | RunEvent::Reasoning(_)) {
                        first.get_or_insert_with(|| start.elapsed().as_secs_f64() * 1000.0);
                    }
                },
                &mut |_| -> AskFuture { panic!("no tool asks") },
            ),
        )
        .await
        .unwrap()
        .unwrap();
        samples.push(json!({"trial":index,"first_token_ms":first.expect("model must generate"),"input_tokens":summary.usage.context_input_tokens(),"cache_read_tokens":summary.usage.cache_read,"output_tokens":summary.usage.output}));
    }
    assert!(
        samples[1]["cache_read_tokens"].as_u64().unwrap() > 0,
        "changing runtime snapshots must retain a real cached history prefix"
    );
    let report = json!({"model":model,"thinking":"default, unchanged","boundary":"real run_once; identical long history; changed run facts, memory recall and refreshing environment","results":samples});
    std::fs::write(
        evidence.join("prefix-measurements.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
}

fn snapshot(
    root: &std::path::Path,
    component: impl Component + 'static,
) -> Arc<kanzei_harness::HarnessSnapshot> {
    let mut harness = Harness::default();
    harness.add(component);
    harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::DevAuto,
            cwd: root.into(),
            project_root: root.into(),
            config: Arc::new(Default::default()),
        })
        .unwrap()
}

struct Sources(Arc<AtomicUsize>);
impl Component for Sources {
    fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
        let version = self.0.clone();
        draft.context.insert(
            "current/environment",
            kanzei_harness::refreshing_source("current/environment", move |_| {
                Some(format!(
                    "environment version={}",
                    version.load(Ordering::SeqCst)
                ))
            }),
        );
        Ok(())
    }
}

#[tokio::test]
async fn changing_run_facts_memory_and_environment_preserves_system_and_history_prefix() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let route = Route::openai_at(
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        None,
    );
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            requests.push(read_request(&mut socket).await);
            respond(&mut socket, json!({"content":"done"}), "stop").await;
        }
        requests
    });
    let root = tempfile::tempdir().unwrap();
    let version = Arc::new(AtomicUsize::new(1));
    let snapshot = snapshot(root.path(), Sources(version.clone()));
    let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
    let prior = vec![
        Message::user_text("OLD_HISTORY ".repeat(2000)),
        Message::assistant(vec![Part::Text {
            text: "previous answer".into(),
        }]),
    ];
    for index in 0..2 {
        let mut config = config();
        config.run_context[0].1 = format!("run={index}; started={index}");
        version.store(index, Ordering::SeqCst);
        let summary = run_once(
            &client,
            &route,
            &snapshot,
            &agent(),
            &config,
            &ToolCtx::new(root.path().into(), root.path().into()),
            "current input",
            Some(&format!("MEMORY_VERSION_{index}")),
            &prior,
            None,
            &mut |_| {},
            &mut |_| -> AskFuture { panic!("no tool asks") },
        )
        .await
        .unwrap();
        let history = serde_json::to_string(&summary.messages).unwrap();
        assert!(!history.contains("MEMORY_VERSION"));
        assert!(!history.contains("environment version"));
        assert!(!history.contains("started="));
        assert_eq!(&summary.messages[..prior.len()], prior.as_slice());
    }
    let requests = server.await.unwrap();
    // The whole stable system plus the long historical messages are byte-identical.
    assert_eq!(
        requests[0]["messages"].as_array().unwrap()[..3],
        requests[1]["messages"].as_array().unwrap()[..3]
    );
    assert_eq!(requests[0]["tools"], requests[1]["tools"]);
    assert!(!requests[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("started="));
    assert!(requests[0].to_string().contains("MEMORY_VERSION_0"));
    assert!(requests[1].to_string().contains("MEMORY_VERSION_1"));
    assert!(requests[1].to_string().contains("environment version=1"));
}

struct Counter(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl Tool for Counter {
    fn name(&self) -> &'static str {
        "count"
    }
    fn description(&self) -> String {
        "A counted committed side effect".into()
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object"})
    }
    async fn execute(&self, _: Value, _: &ToolCtx) -> ToolOutput {
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutput::ok("committed exactly once")
    }
}
impl Component for Counter {
    fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
        draft
            .tools
            .insert("count", Arc::new(Counter(self.0.clone())));
        draft.permissions.push(kanzei_harness::rule(
            "count",
            "*",
            kanzei_harness::Effect::Allow,
        ));
        Ok(())
    }
}

#[tokio::test]
async fn interactive_cancels_background_http_then_background_resumes_without_repeating_tools() {
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();
        let route=Route::openai_at(&format!("http://{}/v1",listener.local_addr().unwrap()),None);
        let entered=Arc::new(Notify::new());let started=entered.clone();
        let (cancelled_tx,cancelled_rx)=oneshot::channel();
        let server=tokio::spawn(async move {
            let (mut socket,_)=listener.accept().await.unwrap();read_request(&mut socket).await;
            respond(&mut socket,json!({"tool_calls":[{"index":0,"id":"committed-tool","function":{"name":"count","arguments":"{}"}}]}),"tool_calls").await;drop(socket);
            let (mut socket,_)=listener.accept().await.unwrap();let interrupted=read_request(&mut socket).await;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"discard this partial\"},\"finish_reason\":null}]}\n\n").await.unwrap();
            let mut byte=[0];assert_eq!(socket.read(&mut byte).await.unwrap(),0,"yield must close the active HTTP response");cancelled_tx.send(()).unwrap();drop(socket);
            let (mut socket,_)=listener.accept().await.unwrap();let foreground=read_request(&mut socket).await;assert!(foreground.to_string().contains("INTERACTIVE"));respond(&mut socket,json!({"content":"foreground done"}),"stop").await;drop(socket);
            let (mut socket,_)=listener.accept().await.unwrap();let resumed=read_request(&mut socket).await;assert_eq!(interrupted,resumed,"resume the uncommitted step with existing tool receipts");respond(&mut socket,json!({"content":"background done"}),"stop").await;
        });
        let count=Arc::new(AtomicUsize::new(0));let root=tempfile::tempdir().unwrap();let snap=snapshot(root.path(),Counter(count.clone()));
        let background=LlmClient::new(&ProxyConfig::Disabled).unwrap().with_priority(RequestPriority::Maintenance);
        let interactive=LlmClient::new(&ProxyConfig::Disabled).unwrap();
        let context=ToolCtx::new(root.path().into(),root.path().into());
        let background_route=route.clone();
        let task=tokio::spawn(async move {
            let mut interrupted=0;
            let summary=run_once(&background,&background_route,&snap,&agent(),&config(),&context,"BACKGROUND",None,&[],None,&mut |event|{
                if matches!(&event,RunEvent::Text(text) if text=="discard this partial") { started.notify_one(); }
                if matches!(event,RunEvent::ResponseInterrupted){interrupted+=1}
            },&mut |_|->AskFuture{panic!("permission is allowed")}).await.unwrap();
            (summary,interrupted)
        });
        entered.notified().await;
        let request=LlmRequest{model:"fixture".into(),system:vec![],messages:vec![Message::user_text("INTERACTIVE")],tools:vec![],hosted_tools:vec![],max_tokens:32,temperature:None,reasoning:Default::default(),service_tier:None};
        let events=interactive.complete(&route,&request).await.unwrap();
        assert!(events.iter().any(|event|matches!(event,kanzei_llm::LlmEvent::TextDelta{text,..} if text=="foreground done")));
        cancelled_rx.await.unwrap();
        let (summary,interrupted)=task.await.unwrap();
        assert_eq!(summary.text,"background done");assert_eq!(interrupted,1);assert_eq!(summary.steps,2);assert_eq!(count.load(Ordering::SeqCst),1);
        assert!(!serde_json::to_string(&summary.messages).unwrap().contains("discard this partial"));
        server.await.unwrap();
    }).await.expect("HTTP cancellation and resumption must not deadlock");
}

#[tokio::test]
async fn background_prefill_yields_before_headers_and_collected_completion_resumes() {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let route = Route::openai_at(&format!("http://{}/v1", listener.local_addr().unwrap()), None);
        let (entered, received) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let first = read_request(&mut socket).await;
            entered.send(()).unwrap(); // Model is processing its prompt; no headers yet.
            let mut byte = [0];
            assert_eq!(socket.read(&mut byte).await.unwrap(), 0);
            drop(socket);
            let (mut socket, _) = listener.accept().await.unwrap();
            let foreground = read_request(&mut socket).await;
            assert!(foreground.to_string().contains("foreground"));
            respond(&mut socket, json!({"content":"interactive result"}), "stop").await;
            drop(socket);
            let (mut socket, _) = listener.accept().await.unwrap();
            assert_eq!(read_request(&mut socket).await, first);
            respond(&mut socket, json!({"content":"complete background result"}), "stop").await;
        });
        let request = LlmRequest {model:"fixture".into(),system:vec![],messages:vec![Message::user_text("background")],tools:vec![],hosted_tools:vec![],max_tokens:32,temperature:None,reasoning:Default::default(),service_tier:None};
        let background_request = request.clone();
        let background_route = route.clone();
        let task = tokio::spawn(async move {
            LlmClient::new(&ProxyConfig::Disabled).unwrap().with_priority(RequestPriority::Maintenance)
                .complete(&background_route, &background_request).await.unwrap()
        });
        received.await.unwrap();
        let interactive_request = LlmRequest {messages:vec![Message::user_text("foreground")], ..request};
        LlmClient::new(&ProxyConfig::Disabled).unwrap().complete(&route, &interactive_request).await.unwrap();
        let events = task.await.unwrap();
        assert!(events.iter().any(|event| matches!(event, kanzei_llm::LlmEvent::TextDelta{text,..} if text=="complete background result")));
        server.await.unwrap();
    }).await.expect("prefill cancellation must release admission before foreground dispatch");
}
