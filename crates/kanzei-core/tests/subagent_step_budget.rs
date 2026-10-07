use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use kanzei_core::{run_subagent, RunEvent, SubagentOptions, SubagentRuntime};
use kanzei_harness::{
    rule, AgentDef, AgentMode, Component, Effect, Harness, HarnessDraft, KanzeiConfig, ProfileKind,
    ResolveCtx, Tool, ToolCtx, ToolOutput,
};
use kanzei_llm::{LlmClient, ProxyConfig, Route};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Probe(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl Tool for Probe {
    fn name(&self) -> &'static str {
        "budget_probe"
    }
    fn description(&self) -> String {
        "Return an observation without external effects".into()
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"step":{"type":"integer"}}})
    }
    async fn execute(&self, _input: Value, _ctx: &ToolCtx) -> ToolOutput {
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolOutput::ok("observed")
    }
}

struct ProbeComponent(Arc<AtomicUsize>);

impl Component for ProbeComponent {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        draft
            .tools
            .insert("budget_probe", Arc::new(Probe(self.0.clone())));
        draft
            .permissions
            .push(rule("budget_probe", "*", Effect::Allow));
        Ok(())
    }
}

async fn respond(listener: &TcpListener, body: Value) {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    let header_end = loop {
        let n = socket.read(&mut buffer).await.unwrap();
        assert!(n > 0);
        request.extend_from_slice(&buffer[..n]);
        if let Some(pos) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while request.len() < header_end + length {
        let n = socket.read(&mut buffer).await.unwrap();
        assert!(n > 0);
        request.extend_from_slice(&buffer[..n]);
    }
    let data = format!("data: {body}\n\ndata: [DONE]\n\n");
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        data.len()
    );
    socket.write_all(header.as_bytes()).await.unwrap();
    socket.write_all(data.as_bytes()).await.unwrap();
}

#[tokio::test]
async fn task_crosses_legacy_default_and_explicit_step_limits() {
    // Exercise the actual delegation entry, model stream and tool loop, including
    // a persona declared primary: the task boundary still makes it a subagent.
    for (steps, mode) in [(0, AgentMode::Subagent), (1, AgentMode::Primary)] {
        let root = std::env::temp_dir().join(format!(
            "kanzei-subagent-step-budget-{}-{steps}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let mut harness = Harness::default();
        harness.add(ProbeComponent(count.clone()));
        let snapshot = harness
            .resolve(&ResolveCtx {
                profile: ProfileKind::Dev,
                cwd: root.clone(),
                project_root: root.clone(),
                config: Arc::new(KanzeiConfig::default()),
            })
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let route = Route::openai_at(
            &format!("http://{}/v1", listener.local_addr().unwrap()),
            None,
        );
        let server = tokio::spawn(async move {
            for step in 1..=33 {
                respond(&listener, json!({"choices":[{"index":0,"delta":{"tool_calls":[{
                    "index":0,"id":format!("probe-{step}"),"type":"function",
                    "function":{"name":"budget_probe","arguments":json!({"step":step}).to_string()}
                }]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1}})).await;
            }
            respond(&listener, json!({"choices":[{"index":0,"delta":{"content":"finished"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}})).await;
        });
        let rt = SubagentRuntime {
            options: SubagentOptions::default(),
            snapshot,
            agent: AgentDef {
                name: "probe".into(),
                profile: Default::default(),
                model: "fast".into(),
                mode,
                steps,
                system: "Observe until the model ends the task".into(),
            },
            roster: vec![],
            fast: (route.clone(), "mock".into()),
            primary: (route, "mock".into()),
            fast_service_tier: None,
            primary_service_tier: None,
            compact: None,
            max_tokens: 128,
            timeout_secs: 15,
            limits: Default::default(),
            coordinator: None,
            writable: false,
            ask_router: None,
            change_log: None,
            cancellations: None,
            background: false,
            background_results: None,
            background_events: None,
            transcripts: None,
            background_notifications: None,
            transcript_sink: None,
            transcript_provider: None,
        };
        let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let output = run_subagent(
            &client,
            &rt,
            &ToolCtx::new(root.clone(), root.clone()),
            "budget-test",
            &json!({"prompt":"Observe all requested steps"}),
            sender,
        )
        .await;
        server.abort();
        assert!(!output.is_error, "{output:?}");
        assert_eq!(output.code, None, "must not end at a budget checkpoint");
        assert_eq!(output.content, "finished");
        assert_eq!(count.load(Ordering::SeqCst), 33);
        let mut last_turn = String::new();
        while let Ok(event) = events.try_recv() {
            if let RunEvent::TaskProgress {
                text, trace: None, ..
            } = event
            {
                last_turn = text;
            }
        }
        assert!(last_turn.contains("34"), "{last_turn}");
        assert!(
            !last_turn.contains('/'),
            "UI must not advertise a finite budget"
        );
        assert!(root.is_absolute() && root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
