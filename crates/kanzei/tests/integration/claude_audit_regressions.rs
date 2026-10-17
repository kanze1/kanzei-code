//! Real-runner regressions for the 2026-10-17 changelog audit.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kanzei_core::{
    run_once, AskPolicy, AskReply, AskResponse, CancellationToken, DigestModel, RunEvent,
    RunnerConfig,
};
use kanzei_harness::{Component, Effect, Harness, HarnessDraft, ResolveCtx, Tool, ToolCtx};
use kanzei_llm::{LlmClient, Message, Part, ProxyConfig, Route};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Policy(Vec<Arc<dyn Tool>>);

impl Component for Policy {
    fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
        for tool in &self.0 {
            draft.tools.insert(tool.name(), tool.clone());
        }
        for action in ["read", "grep", "glob"] {
            draft.permissions.extend([
                kanzei_harness::rule(action, "*", Effect::Allow),
                kanzei_harness::rule(action, "private", Effect::Deny),
                kanzei_harness::rule(action, "private/*", Effect::Deny),
                kanzei_harness::rule(action, "blocked.txt", Effect::Deny),
                kanzei_harness::rule(action, "approval.txt", Effect::Ask),
            ]);
        }
        Ok(())
    }
}

fn snapshot(root: &Path, tools: Vec<Arc<dyn Tool>>) -> Arc<kanzei_harness::HarnessSnapshot> {
    let mut harness = Harness::default();
    harness.add(Policy(tools));
    harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::Dev,
            cwd: root.into(),
            project_root: root.into(),
            config: Arc::new(Default::default()),
        })
        .unwrap()
}

fn tools(root: &Path) -> Vec<Arc<dyn Tool>> {
    let mut harness = Harness::default();
    harness.add(kanzei_tools::BaseComponent);
    harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::Dev,
            cwd: root.into(),
            project_root: root.into(),
            config: Arc::new(Default::default()),
        })
        .unwrap()
        .materialize_tools()
}

fn config() -> RunnerConfig {
    RunnerConfig {
        digest_model: None,
        intensity: Default::default(),
        model: "main-probe".into(),
        max_tokens: 128,
        reasoning: Default::default(),
        service_tier: None,
        hosted_tools: vec![],
        context_limit: Some(128_000),
        limits: Default::default(),
        recall: None,
        execution_policy: Default::default(),
        ask_policy: AskPolicy::Interactive,
        halt: None,
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Value {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    let end = loop {
        let n = socket.read(&mut chunk).await.unwrap();
        assert_ne!(n, 0);
        raw.extend_from_slice(&chunk[..n]);
        if let Some(end) = raw.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = String::from_utf8_lossy(&raw[..end]);
    let count: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    while raw.len() < end + count {
        let n = socket.read(&mut chunk).await.unwrap();
        assert_ne!(n, 0);
        raw.extend_from_slice(&chunk[..n]);
    }
    serde_json::from_slice(&raw[end..end + count]).unwrap()
}

async fn send_response(socket: &mut tokio::net::TcpStream, value: Value) {
    let body = format!("data: {value}\n\ndata: [DONE]\n\n");
    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    socket.write_all(head.as_bytes()).await.unwrap();
    socket.write_all(body.as_bytes()).await.unwrap();
}

async fn run_calls(
    root: &Path,
    calls: Vec<(&str, Value)>,
    parallel: bool,
) -> (kanzei_core::RunSummary, Vec<(String, String)>) {
    let selected = tools(root)
        .into_iter()
        .filter(|tool| calls.iter().any(|(name, _)| *name == tool.name()))
        .collect();
    let snapshot = snapshot(root, selected);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let tool_calls: Vec<_> = calls.into_iter().enumerate().map(|(index, (name, input))| json!({"index":index,"id":format!("call-{index}"),"type":"function","function":{"name":name,"arguments":input.to_string()}})).collect();
    let server = tokio::spawn(async move {
        for step in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            let response = if step == 0 {
                json!({"choices":[{"index":0,"delta":{"tool_calls":tool_calls},"finish_reason":"tool_calls"}]})
            } else {
                json!({"choices":[{"index":0,"delta":{"content":"done"},"finish_reason":"stop"}]})
            };
            send_response(&mut socket, response).await;
        }
    });
    let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
    let route = Route::openai_at(&format!("http://{address}/v1"), None);
    let agent =
        serde_json::from_value(json!({"name":"fixture","model":"main-probe","steps":4})).unwrap();
    let mut config = config();
    config.execution_policy = if parallel {
        kanzei_harness::ExecutionPolicy::Default
    } else {
        kanzei_harness::ExecutionPolicy::ReadParallelWriteSerial
    };
    let ctx = ToolCtx::new(root.into(), root.into());
    let mut decisions = Vec::new();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &config,
            &ctx,
            "perform the provided tool calls",
            None,
            &[],
            None,
            &mut |event| {
                if let RunEvent::PermissionResolved {
                    resource, decision, ..
                } = event
                {
                    decisions.push((resource, decision.into()));
                }
            },
            &mut |_| Box::pin(async { AskResponse::Permission(AskReply::Deny) }),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    server.await.unwrap();
    (result, decisions)
}

fn results(summary: &kanzei_core::RunSummary) -> Vec<(&str, bool)> {
    summary
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            Part::ToolResult {
                content, is_error, ..
            } => Some((content.as_str(), *is_error)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn provider_cache_split_does_not_change_the_context_usage_anchor() {
    for cached in [0, 306_176, 314_705] {
        let home = crate::common::TestHome::new("cached-context-usage");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            send_response(&mut socket, json!({"choices":[{"index":0,"delta":{"content":"done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":314705,"prompt_tokens_details":{"cached_tokens":cached},"completion_tokens":1}})).await;
        });
        let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
        let route = Route::openai_at(&format!("http://{address}/v1"), None);
        let agent =
            serde_json::from_value(json!({"name":"fixture","model":"main-probe","steps":1}))
                .unwrap();
        let snapshot = snapshot(&home.root, vec![]);
        let ctx = ToolCtx::new(home.root.clone(), home.root.clone());
        let result = run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &config(),
            &ctx,
            "measure context",
            None,
            &[],
            None,
            &mut |_| {},
            &mut |_| Box::pin(async { AskResponse::Permission(AskReply::Deny) }),
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(
            result.usage.input,
            314_705 - cached,
            "billing remains uncached-only"
        );
        assert_eq!(result.usage.cache_read, cached);
        assert_eq!(
            result.last_input_tokens,
            Some(314_705),
            "context capacity must include cached input, including fully cached requests"
        );
    }
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn physical_paths_and_search_roots_are_denied_in_serial_and_parallel_dispatch() {
    let home = crate::common::TestHome::new("physical-path-permissions");
    let root = &home.root;
    std::fs::create_dir(root.join("private")).unwrap();
    std::fs::write(root.join("private/secret.txt"), "DENIED_PRIVATE_MARKER").unwrap();
    std::fs::write(root.join("allowed.txt"), "ALLOWED_MARKER").unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("public"))
            .arg(root.join("private"))
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("private"), root.join("public")).unwrap();
    for parallel in [false, true] {
        let calls = vec![
            ("read", json!({"path":"private/secret.txt"})),
            ("read", json!({"path":"public/secret.txt"})),
            ("read", json!({"file_path":root.join("public/secret.txt")})),
            (
                "grep",
                json!({"dir":"private","pattern":"DENIED_PRIVATE_MARKER"}),
            ),
            (
                "grep",
                json!({"path":root.join("public/secret.txt"),"pattern":"DENIED_PRIVATE_MARKER"}),
            ),
            ("glob", json!({"cwd":"public","pattern":"**/*.txt"})),
            ("read", json!({"path":"allowed.txt"})),
        ];
        let (summary, decisions) = run_calls(root, calls, parallel).await;
        let outputs = results(&summary);
        assert_eq!(outputs.len(), 7);
        for (content, error) in &outputs[..6] {
            assert!(*error, "{content}");
            assert!(!content.contains("DENIED_PRIVATE_MARKER"));
            assert!(content.contains("permission denied"), "{content}");
        }
        assert!(!outputs[6].1);
        assert!(outputs[6].0.contains("ALLOWED_MARKER"));
        assert_eq!(
            decisions
                .iter()
                .filter(|(_, decision)| decision == "deny")
                .count(),
            6
        );
        assert!(decisions.iter().all(|(resource, _)| resource != "*"));
    }
    // Remove the junction explicitly before recursive fixture cleanup.
    #[cfg(windows)]
    std::fs::remove_dir(root.join("public")).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(root.join("public")).unwrap();
}

#[tokio::test]
async fn searches_exclude_denied_and_unapproved_descendants_without_leaking_names_or_content() {
    let home = crate::common::TestHome::new("search-descendants");
    let root = &home.root;
    std::fs::create_dir(root.join("private")).unwrap();
    std::fs::write(root.join("private/secret.txt"), "SEARCH_MARKER private").unwrap();
    std::fs::write(root.join("approval.txt"), "SEARCH_MARKER ask").unwrap();
    std::fs::write(root.join("blocked.txt"), "SEARCH_MARKER denied file").unwrap();
    std::fs::write(root.join("allowed.txt"), "SEARCH_MARKER allowed").unwrap();
    for parallel in [false, true] {
        let (summary, _) = run_calls(
            root,
            vec![
                ("grep", json!({"pattern":"SEARCH_MARKER"})),
                ("grep", json!({"pattern":"SEARCH_MARKER","count":true})),
                ("grep", json!({"pattern":"SEARCH_MARKER","context":1})),
                ("grep", json!({"pattern":"SEARCH_MARKER","files_only":true})),
                ("glob", json!({"pattern":"**/*.txt"})),
            ],
            parallel,
        )
        .await;
        for (content, error) in results(&summary) {
            assert!(!error, "{content}");
            assert!(content.contains("allowed.txt"), "{content}");
            assert!(content.contains("permission restricted"), "{content}");
            assert!(!content.contains("secret.txt"), "{content}");
            assert!(!content.contains("approval.txt"), "{content}");
            assert!(!content.contains("blocked.txt"), "{content}");
            assert!(!content.contains("SEARCH_MARKER private"), "{content}");
            assert!(!content.contains("SEARCH_MARKER ask"), "{content}");
        }
    }
}

#[tokio::test]
async fn cancelling_compaction_preserves_history_and_prevents_retry_or_main_requests() {
    for phase in ["headers", "stream", "retry"] {
        let home = crate::common::TestHome::new(phase);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = requests.clone();
        let server = tokio::spawn(async move {
            let mut ready_tx = Some(ready_tx);
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_request(&mut socket).await;
                assert_eq!(request["model"], "digest-probe");
                let attempt = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if phase == "retry" && attempt == 0 {
                    send_response(&mut socket, json!({"choices":[{"index":0,"delta":{"content":"bad summary"},"finish_reason":"stop"}]})).await;
                    continue;
                }
                if phase == "stream" {
                    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n").await.unwrap();
                }
                ready_tx.take().unwrap().send(()).unwrap();
                // Keep the HTTP response pending until the client drops it.
                let mut byte = [0u8; 1];
                let _ = socket.read(&mut byte).await;
            }
        });
        let cancel = CancellationToken::new();
        let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
        let route = Route::openai_at(&format!("http://{address}/v1"), None);
        let agent =
            serde_json::from_value(json!({"name":"fixture","model":"main-probe","steps":4}))
                .unwrap();
        let snapshot = snapshot(&home.root, vec![]);
        let ctx = ToolCtx::new(home.root.clone(), home.root.clone());
        let mut config = config();
        config.context_limit = Some(8000);
        config.limits.compact_buffer_tokens = Some(1000);
        config.halt = Some(cancel.clone());
        config.digest_model = Some(DigestModel {
            route: route.clone(),
            model: "digest-probe".into(),
            service_tier: None,
            focus: None,
            archive_root: None,
        });
        let mut history = vec![Message::user_text("调查工具并保留证据")];
        for _ in 0..6 {
            history.push(Message::assistant(vec![Part::Text {
                text: "工作过程，保留原始证据。".repeat(4000),
            }]));
            history.push(Message::user_text("继续只读调查"));
        }
        let expected = serde_json::to_value(&history).unwrap();
        let compacted_events = std::sync::atomic::AtomicUsize::new(0);
        let mut event = |event| {
            if matches!(event, RunEvent::ContextCompacted { .. }) {
                compacted_events.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        };
        let mut ask = |_| {
            Box::pin(async { AskResponse::Permission(AskReply::Deny) }) as kanzei_core::AskFuture
        };
        let run = run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &config,
            &ctx,
            "继续调查",
            None,
            &history,
            None,
            &mut event,
            &mut ask,
        );
        tokio::pin!(run);
        tokio::select! {
            _ = &mut run => panic!("runner returned before cancellation"),
            ready = tokio::time::timeout(Duration::from_secs(5), ready_rx) => ready.unwrap().unwrap(),
        }
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), &mut run)
            .await
            .expect("compaction must stop without server release")
            .unwrap();
        assert!(result.halted_by_user);
        assert_eq!(
            serde_json::to_value(&result.messages[..history.len()]).unwrap(),
            expected
        );
        assert!(result.overflow_traces.is_empty());
        assert_eq!(
            requests.load(std::sync::atomic::Ordering::SeqCst),
            if phase == "retry" { 2 } else { 1 }
        );
        assert_eq!(
            compacted_events.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        server.abort();
        let _ = server.await;
    }
}
