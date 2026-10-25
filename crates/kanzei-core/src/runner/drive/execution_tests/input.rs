use super::*;
use kanzei_harness::{AsyncMailbox, AsyncNotice, InputInbox};
use std::sync::Mutex;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub(super) async fn read_request(socket: &mut TcpStream) -> serde_json::Value {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let end = loop {
        let n = socket.read(&mut buffer).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buffer[..n]);
        if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let length: usize = String::from_utf8_lossy(&bytes[..end])
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() < end + length {
        let n = socket.read(&mut buffer).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buffer[..n]);
    }
    serde_json::from_slice(&bytes[end..end + length]).unwrap()
}

async fn event(socket: &mut TcpStream, delta: serde_json::Value, reason: Option<&str>) {
    let data = json!({"choices":[{"index":0,"delta":delta,"finish_reason":reason}]});
    socket
        .write_all(format!("data: {data}\n\n").as_bytes())
        .await
        .unwrap();
}

#[tokio::test]
async fn conversation_with_an_embedded_checkout_reaches_the_provider() {
    let root = kanzei_base::test_support::TestDir::new("conversation-nested-checkout");
    for path in [root.to_path_buf(), root.join("release-checkout")] {
        std::fs::create_dir_all(&path).unwrap();
        let initialized = std::process::Command::new("git")
            .current_dir(&path)
            .args(["init", "-q"])
            .output()
            .unwrap();
        assert!(initialized.status.success());
        std::fs::write(path.join("source.txt"), "source").unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        assert!(request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["content"] == "RESUME_ORIGINAL_CONVERSATION"));
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        event(&mut socket, json!({"content":"RESUMED"}), None).await;
        event(&mut socket, json!({}), Some("stop")).await;
        socket.write_all(b"data: [DONE]\n\n").await.unwrap();
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
    let route = Route::openai_at(&format!("http://{address}/v1"), None);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &config(Default::default()),
            &ctx,
            "RESUME_ORIGINAL_CONVERSATION",
            None,
            &[],
            None,
            &mut |_| {},
            &mut |_| panic!("source capture requires no approval"),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.text, "RESUMED");
    server.await.unwrap();
}

#[derive(Clone, Copy, Debug)]
enum Boundary {
    Headers,
    Stream,
    Retry,
}

async fn assert_input_interrupts(boundary: Boundary) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let pending = Arc::new(Mutex::new(Vec::new()));
    let mailbox = AsyncMailbox::new(|_| Ok(()));
    let admit = {
        let pending = pending.clone();
        let mailbox = mailbox.clone();
        Arc::new(move || {
            pending.lock().unwrap().push(AsyncNotice {
                origin: kanzei_base::MessageOrigin::Human,
                attachments: Vec::new(),
                id: "new-question".into(),
                text: "ANSWER_THIS_NOW".into(),
            });
            mailbox.request_input();
        })
    };
    let server_admit = admit.clone();
    let server = tokio::spawn(async move {
        let (mut old, _) = listener.accept().await.unwrap();
        read_request(&mut old).await;
        match boundary {
            Boundary::Headers => server_admit(),
            Boundary::Stream => {
                old.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
                event(
                    &mut old,
                    json!({"reasoning_content":"DISCARDED_REASONING"}),
                    None,
                )
                .await;
                event(&mut old, json!({"tool_calls":[{"index":0,"id":"uncommitted-tool","type":"function","function":{"name":"execution_probe","arguments":"{}"}}]}), None).await;
                event(&mut old, json!({"content":"DISCARDED_DRAFT"}), None).await;
            }
            Boundary::Retry => {
                let body = "data: {\"error\":{\"type\":\"rate_limit_error\",\"message\":\"wait\",\"retry_after\":60}}\n\n";
                old.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        }
        // Keep the original response open: handling input must not depend on it.
        let (mut next, _) = listener.accept().await.unwrap();
        let request = read_request(&mut next).await;
        assert!(request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["content"] == "ANSWER_THIS_NOW"));
        assert!(!request.to_string().contains("DISCARDED_"));
        assert!(!request.to_string().contains("uncommitted-tool"));
        next.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        event(&mut next, json!({"content":"NEW_ANSWER"}), None).await;
        event(&mut next, json!({}), Some("stop")).await;
        next.write_all(b"data: [DONE]\n\n").await.unwrap();
    });
    let root = std::env::temp_dir();
    let mut harness = Harness::default();
    harness
        .add(Policy)
        .add(SnapshotTool(Arc::new(CleanupProbe::default())));
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(Default::default()),
        })
        .unwrap();
    let mut ctx = ToolCtx::new(root.clone(), root);
    ctx.async_mailbox = Some(mailbox.clone());
    ctx.input_inbox = Some(InputInbox::new(move || {
        Ok(std::mem::take(&mut *pending.lock().unwrap()))
    }));
    let agent =
        serde_json::from_value(json!({"name":"fixture","model":"fixture","max_steps":4})).unwrap();
    let client = LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let route = Route::openai_at(&format!("http://{address}/v1"), None);
    let mut interrupted = 0;
    let mut committed = Vec::new();
    let mut admitted = false;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &config(Default::default()),
            &ctx,
            "OLD_REQUEST",
            None,
            &[],
            None,
            &mut |event| {
                let should_admit = matches!(
                    (&boundary, &event),
                    (Boundary::Stream, RunEvent::Text(_))
                        | (Boundary::Retry, RunEvent::StreamRestart { .. })
                );
                if should_admit && !admitted {
                    admit();
                    admitted = true;
                }
                match event {
                    RunEvent::ResponseInterrupted => interrupted += 1,
                    RunEvent::AssistantMessageCommitted { message, .. } => committed.push(message),
                    RunEvent::ToolStart { .. } => panic!("uncommitted tool calls cannot execute"),
                    _ => {}
                }
            },
            &mut |_| panic!("uncommitted calls cannot request approval"),
        ),
    )
    .await
    .expect("human input must preempt the held provider response")
    .unwrap();
    assert_eq!(
        interrupted, 1,
        "exactly one provisional response discarded: {boundary:?}"
    );
    assert_eq!(committed.len(), 1);
    assert_eq!(result.text, "NEW_ANSWER");
    assert_eq!(
        result.steps, 1,
        "discarded response does not spend a completed step"
    );
    assert!(!result.halted_by_user);
    assert!(!mailbox.is_closed());
    assert!(!serde_json::to_string(&result.messages)
        .unwrap()
        .contains("DISCARDED_"));
    server.await.unwrap();
}

#[tokio::test]
async fn human_input_interrupts_provider_opening_without_waiting_for_headers() {
    assert_input_interrupts(Boundary::Headers).await;
}
#[tokio::test]
async fn human_input_discards_partial_text_reasoning_and_unexecuted_tools() {
    assert_input_interrupts(Boundary::Stream).await;
}
#[tokio::test]
async fn human_input_interrupts_a_long_provider_retry_wait() {
    assert_input_interrupts(Boundary::Retry).await;
}
