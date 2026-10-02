//! R-364 B2:exercise tool discovery/loading through the real CLI runner and provider request path.
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;

async fn read_request(stream: &mut TcpStream) -> Value {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(
            count > 0,
            "provider request closed before headers completed"
        );
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let content_length = String::from_utf8_lossy(&bytes[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0, "provider request closed before body completed");
        bytes.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + content_length]).unwrap()
}

async fn serve_sequence(listener: TcpListener, responses: Vec<Value>) -> Vec<Value> {
    let mut requests = Vec::new();
    for response in responses {
        let (mut stream, _) =
            tokio::time::timeout(std::time::Duration::from_secs(20), listener.accept())
                .await
                .expect("timed out waiting for a provider request")
                .unwrap();
        requests.push(read_request(&mut stream).await);
        let body = format!("data: {response}\n\ndata: [DONE]\n\n");
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).await.unwrap();
        stream.write_all(body.as_bytes()).await.unwrap();
    }
    requests
}

fn tool_call_response(name: &str, input: Value) -> Value {
    json!({
        "choices": [{
            "index": 0,
            "delta": {"tool_calls": [{
                "index": 0,
                "id": "call_deferred",
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": serde_json::to_string(&input).unwrap()
                }
            }]},
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    })
}

fn success_response(text: &str) -> Value {
    json!({
        "choices": [{
            "index": 0,
            "delta": {"content": text},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    })
}

fn overflow_response() -> Value {
    json!({
        "error": {
            "type": "invalid_request_error",
            "code": "context_length_exceeded",
            "message": "Your input exceeds the context window of this model"
        }
    })
}

async fn run_cli(
    label: &str,
    prior: Vec<kanzei_llm::Message>,
    responses: Vec<Value>,
) -> (std::path::PathBuf, std::process::Output, Vec<Value>) {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let parent =
        std::env::temp_dir().join(format!("kz-r364-{label}-{}-{suffix}", std::process::id()));
    let project = parent.join("project");
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();
    let home_guard = super::common::TestHome::new(label);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    std::fs::write(
        project.join(".kanzei/kanzei.toml"),
        format!(
            "[models]\nprimary = \"mock:test-model\"\n\n[providers.mock]\nprotocol = \"openai\"\nbase_url = \"http://{address}/v1\"\n"
        ),
    )
    .unwrap();

    let store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project)).unwrap();
    let session_id = kanzei_core::project_session_id(&project);
    store
        .create_session(&session_id, &project.display().to_string(), None)
        .unwrap();
    store
        .append_event(
            &session_id,
            "conversation.updated",
            &json!({"messages": prior}),
        )
        .unwrap();
    drop(store);

    let server = tokio::spawn(serve_sequence(listener, responses));
    let mut command = Command::new(env!("CARGO_BIN_EXE_kz"));
    command
        .args(["run", "continue the current task"])
        .current_dir(&project);
    home_guard.apply(&mut command);
    let output = command
        .env("KANZEI_MODEL", "mock:test-model")
        .env("KANZEI_AGENT", "dev-pair")
        .env("KANZEI_PROFILE", "dev")
        .env("KANZEI_PROXY", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .unwrap();
    let requests = server.await.unwrap();
    (project, output, requests)
}

fn tool_names(request: &Value) -> Vec<&str> {
    request["tools"]
        .as_array()
        .expect("OpenAI request must contain tools")
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_loading_survives_sse_overflow_and_is_billed() {
    let (project, output, requests) = run_cli(
        "search-overflow",
        Vec::new(),
        vec![
            tool_call_response("tool_search", json!({"query": "select:process"})),
            overflow_response(),
            success_response("recovered"),
        ],
    )
    .await;
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(requests.len(), 3);
    assert!(!tool_names(&requests[0]).contains(&"process"));
    assert!(tool_names(&requests[0]).contains(&"tool_search"));
    assert!(requests[0].to_string().contains("<deferred-tools>"));
    for request in &requests[1..] {
        assert_eq!(
            tool_names(request)
                .iter()
                .filter(|name| **name == "process")
                .count(),
            1
        );
    }

    let store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project)).unwrap();
    let context = store
        .latest_episode_context(&kanzei_core::project_session_id(&project))
        .unwrap()
        .expect("completed run must persist its context bill");
    assert!(context.contains("tools/catalog"), "{context}");
    assert!(context.contains("tools/loaded:process"), "{context}");
    drop(store);
    std::fs::remove_dir_all(project.parent().unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_deferred_call_auto_loads_and_executes() {
    let (project, output, requests) = run_cli(
        "direct-call",
        Vec::new(),
        vec![
            tool_call_response("memory_stats", json!({})),
            success_response("done"),
        ],
    )
    .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(requests.len(), 2);
    assert!(!tool_names(&requests[0]).contains(&"memory_stats"));
    assert_eq!(
        tool_names(&requests[1])
            .iter()
            .filter(|name| **name == "memory_stats")
            .count(),
        1
    );
    assert!(requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| {
            message["role"] == "tool"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
        }));
    std::fs::remove_dir_all(project.parent().unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prior_tool_call_seeds_deferred_schema_for_the_next_run() {
    let prior = vec![
        kanzei_llm::Message::assistant(vec![kanzei_llm::Part::ToolCall {
            id: "prior_call".into(),
            name: "memory_stats".into(),
            input: json!({}),
        }]),
        kanzei_llm::Message::tool_results(vec![kanzei_llm::Part::ToolResult {
            call_id: "prior_call".into(),
            content: "previous result".into(),
            is_error: false,
        }]),
    ];
    let (project, output, requests) =
        run_cli("prior-seed", prior, vec![success_response("resumed")]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(requests.len(), 1);
    assert!(tool_names(&requests[0]).contains(&"memory_stats"));
    std::fs::remove_dir_all(project.parent().unwrap()).unwrap();
}
