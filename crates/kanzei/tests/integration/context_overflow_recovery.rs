use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;

use crate::common;

async fn read_request(stream: &mut TcpStream) -> Value {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0, "request closed before headers completed");
        request.extend_from_slice(&chunk[..count]);
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let content_length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0, "request closed before body completed");
        request.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&request[header_end..header_end + content_length]).unwrap()
}

async fn serve_sequence(listener: TcpListener, responses: Vec<Value>) -> Vec<Value> {
    let mut requests = Vec::new();
    for response in responses {
        // 桩服务器绝不无限等待:请求数一旦对不上,要的是变红而不是挂死整个门禁。
        let (mut stream, _) = tokio::time::timeout(std::time::Duration::from_secs(20), listener.accept())
        .await
        .expect("等待模型请求超时。多半是生产侧的轮次数变了(例如权限被拒导致本轮提前收口),桩服务器还在等下一次请求——改测试对齐新契约,不要让 cargo test --workspace 静默挂死")
        .unwrap();
        requests.push(read_request(&mut stream).await);
        let body = format!("data: {response}\n\ndata: [DONE]\n\n");
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(body.as_bytes()).await.unwrap();
    }
    requests
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

fn temp_root(name: &str) -> std::path::PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kanzei-{name}-{}-{suffix}", std::process::id()))
}

async fn run_cli_with_prior(
    test_name: &str,
    prior: Vec<kanzei_llm::Message>,
    prompt: &str,
    responses: Vec<Value>,
) -> (std::path::PathBuf, std::process::Output, Vec<Value>) {
    let root = temp_root(test_name);
    let home_guard = common::TestHome::new(test_name);
    let project = root.join("project");
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

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
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kz"));
    cmd.args(["run", prompt]).current_dir(&project);
    // 全局根隔离(见 tests/common/mod.rs,R-200):三连缺一即退回读开发者真实配置(D-292)。
    home_guard.apply(&mut cmd);
    let output = cmd
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

fn persisted_messages(project: &std::path::Path) -> Vec<kanzei_llm::Message> {
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(project)).unwrap();
    let session_id = kanzei_core::project_session_id(project);
    let event = store
        .list_events(&session_id, 0)
        .unwrap()
        .into_iter()
        .rev()
        .find(|event| event.event_type == "conversation.updated")
        .expect("successful recovery should persist the compacted conversation");
    serde_json::from_value(event.payload["messages"].clone()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sse_context_overflow_compacts_history_and_persists_recovered_summary() {
    const TAIL_MARKER: &str = "TAIL_MUST_BE_DROPPED_BY_BOUNDED_COMPACTION";
    let old = format!("{}{TAIL_MARKER}", "A".repeat(9_000));
    let prior = vec![
        kanzei_llm::Message::user_text(old),
        kanzei_llm::Message::assistant(vec![kanzei_llm::Part::Text {
            text: "旧回复".into(),
        }]),
    ];
    let (project, output, requests) = run_cli_with_prior(
        "stream-overflow-compact",
        prior,
        "继续当前任务",
        vec![overflow_response(), success_response("压缩恢复成功")],
    )
    .await;

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(requests.len(), 2);
    let first = requests[0].to_string();
    let second = requests[1].to_string();
    assert!(first.contains(TAIL_MARKER));
    assert!(second.contains("压缩记录"));
    assert!(second.contains("继续当前任务"));
    assert!(!second.contains(TAIL_MARKER));

    let persisted = persisted_messages(&project);
    let persisted_json = serde_json::to_string(&persisted).unwrap();
    assert!(persisted_json.contains("压缩记录"));
    assert!(persisted_json.contains("压缩恢复成功"));
    assert!(!persisted_json.contains(TAIL_MARKER));

    // R-106:被裁剪段先沉淀 episode 再重置——压缩丢弃的历史有迹可查,
    // 而不是被静默丢弃。CLI 轮末把 summary.overflow_traces 写入 episodes.overflow_json。
    let store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project)).unwrap();
    let traces = store
        .recent_overflow_traces(&kanzei_core::project_session_id(&project), 10)
        .unwrap();
    assert_eq!(traces.len(), 1, "一次有界压缩应留下一条溢出轨迹 episode");
    assert!(
        traces[0].1.contains("dropped_messages"),
        "轨迹应含丢弃段画像: {}",
        traces[0].1
    );
    assert!(
        traces[0].1.contains("preview"),
        "轨迹应含文本预览: {}",
        traces[0].1
    );
    drop(store);

    std::fs::remove_dir_all(project.parent().unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_sse_context_overflow_retries_with_only_current_user_message() {
    const OLD_MARKER: &str = "OLD_HISTORY_REMOVED_AGGRESSIVELY";
    let prior = vec![kanzei_llm::Message::user_text(OLD_MARKER)];
    let (project, output, requests) = run_cli_with_prior(
        "stream-overflow-aggressive",
        prior,
        "只保留这个当前任务",
        vec![
            overflow_response(),
            overflow_response(),
            success_response("激进压缩恢复成功"),
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
    let second = requests[1].to_string();
    let third = requests[2].to_string();
    assert!(second.contains("压缩记录"));
    assert!(second.contains(OLD_MARKER));
    assert!(!third.contains("压缩记录"));
    assert!(!third.contains(OLD_MARKER));
    assert!(third.contains("只保留这个当前任务"));

    let persisted = persisted_messages(&project);
    let persisted_json = serde_json::to_string(&persisted).unwrap();
    assert!(!persisted_json.contains(OLD_MARKER));
    assert!(persisted_json.contains("只保留这个当前任务"));
    assert!(persisted_json.contains("激进压缩恢复成功"));

    // R-106:两级压缩各沉淀一条轨迹(有界 + 激进),episode 可查回。
    // 一次运行只落一条 episode,两条轨迹在 overflow_json 的同一个 JSON 数组里。
    let store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&project)).unwrap();
    let traces = store
        .recent_overflow_traces(&kanzei_core::project_session_id(&project), 10)
        .unwrap();
    assert_eq!(traces.len(), 1, "一次运行应只落一条带轨迹的 episode");
    let parsed: serde_json::Value = serde_json::from_str(&traces[0].1).unwrap();
    let entries = parsed.as_array().expect("overflow_json 应为 JSON 数组");
    assert_eq!(entries.len(), 2, "两次压缩应沉淀两条轨迹: {}", traces[0].1);
    assert!(
        traces[0].1.contains(OLD_MARKER),
        "被丢弃的旧历史应出现在轨迹里: {}",
        traces[0].1
    );
    drop(store);

    std::fs::remove_dir_all(project.parent().unwrap()).unwrap();
}
