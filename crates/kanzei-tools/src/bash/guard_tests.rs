use super::*;
use kanzei_core::{RecallHit, RecallPolicy, RecallTrigger, RunEvent, RunnerConfig};
use kanzei_harness::{Component, Harness, HarnessDraft, ResolveCtx};
use kanzei_llm::{LlmClient, Part, ProxyConfig, Route};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct BashFixture;
impl Component for BashFixture {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        draft.tools.insert("bash", Arc::new(BashTool::default()));
        draft.permissions.push(kanzei_harness::permission::Rule {
            action: "bash".into(),
            resource: "*".into(),
            effect: kanzei_harness::Effect::Allow,
        });
        Ok(())
    }
}

struct CountingRecall(Arc<AtomicUsize>);
impl RecallPolicy for CountingRecall {
    fn retrieve(&self, _trigger: &RecallTrigger) -> Result<Vec<RecallHit>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

async fn assert_long_command_contract(call_count: usize) {
    let root = super::tests::temp_project(&format!("guard-contract-{call_count}"));
    let command = format!("echo {}", "a".repeat(MAX_COMMAND_CHARS));
    let calls: Vec<_> = (0..call_count)
        .map(|index| {
            json!({"index":index,"id":format!("guard-{index}"),"type":"function",
                "function":{"name":"bash","arguments":json!({"command":command}).to_string()}})
        })
        .collect();
    let responses = [
        json!({"choices":[{"index":0,"delta":{"tool_calls":calls},"finish_reason":"tool_calls"}]}),
        json!({"choices":[{"index":0,"delta":{"content":"Use a file instead."},"finish_reason":"stop"}]}),
    ];
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            let header_end = loop {
                let count = stream.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
                if let Some(position) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break position + 4;
                }
            };
            let content_length = String::from_utf8_lossy(&request[..header_end])
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            while request.len() < header_end + content_length {
                let count = stream.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
            }
            requests
                .push(serde_json::from_slice::<serde_json::Value>(&request[header_end..]).unwrap());
            let body = format!("data: {response}\n\ndata: [DONE]\n\n");
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(body.as_bytes()).await.unwrap();
        }
        requests
    });
    let mut harness = Harness::default();
    harness.add(BashFixture);
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: kanzei_harness::ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(Default::default()),
        })
        .unwrap();
    let agent = kanzei_harness::AgentDef {
        name: "guard-fixture".into(),
        profile: kanzei_harness::ProfileScope::Development,
        model: "fixture".into(),
        mode: kanzei_harness::AgentMode::Primary,
        steps: 3,
        system: String::new(),
    };
    let recall = Arc::new(AtomicUsize::new(0));
    let config = RunnerConfig {
        hosted_tools: Vec::new(),
        digest_model: None,
        intensity: kanzei_harness::HarnessIntensity::Autonomous,
        model: "fixture".into(),
        max_tokens: 128,
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: None,
        context_limit: None,
        limits: Default::default(),
        recall: Some(Box::new(CountingRecall(recall.clone()))),
        execution_policy: Default::default(),
        ask_policy: kanzei_core::AskPolicy::Interactive,
        halt: None,
    };
    let ctx = ToolCtx::new(root.clone(), root.clone()).with_identity(
        "fixture-tree".into(),
        root.display().to_string(),
        "guard-contract".into(),
        "fixture-process".into(),
    );
    let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
    let route = Route::openai_at(&format!("http://{address}/v1"), None);
    let mut events = Vec::new();
    let summary = kanzei_core::run_once(
        &client,
        &route,
        &snapshot,
        &agent,
        &config,
        &ctx,
        "Run the fixture command.",
        None,
        &[],
        None,
        &mut |event| events.push(event),
        &mut |_| panic!("fixture rules already allow bash"),
    )
    .await
    .unwrap();
    let requests = server.await.unwrap();
    let metrics = kanzei_core::summarize_metrics(&summary.messages);
    let telemetry: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(".kanzei/artifacts/tool-failures/guard-contract.json")).unwrap(),
    )
    .unwrap();
    let results: Vec<_> = summary
        .messages
        .iter()
        .flat_map(|message| &message.parts)
        .filter_map(|part| match part {
            Part::ToolResult {
                content, is_error, ..
            } => Some((content, *is_error)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), call_count);
    for (content, is_error) in results {
        assert!(
            is_error,
            "the provider must still correct its rejected input"
        );
        assert!(content.starts_with("[tool_outcome=needs_correction code=COMMAND_TOO_LONG]"));
        assert!(!content.contains("permission denied"));
    }
    assert_eq!(metrics.failed_calls, 0);
    assert_eq!(metrics.tool_rejections, call_count);
    assert!(kanzei_core::summarize_failures(&summary.messages).is_empty());
    assert_eq!(recall.load(Ordering::SeqCst), 0);
    assert_eq!(telemetry["failure_count"], 0);
    assert_eq!(telemetry["outcome_counts"]["needs_correction"], call_count);
    assert!(telemetry["events"].as_array().unwrap().iter().all(|event| {
        event["class"] != "permission_denied"
            && event["outcome"] == "needs_correction"
            && event["code"] == "COMMAND_TOO_LONG"
    }));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event,
                RunEvent::ToolEnd { ok: false, outcome, code: Some(code), .. }
                    if outcome == "needs_correction" && code == "COMMAND_TOO_LONG"
            ))
            .count(),
        call_count
    );
    let delivered = requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), call_count);
    assert!(delivered.iter().all(|message| message["content"]
        .as_str()
        .unwrap()
        .starts_with("[tool error]\n[tool_outcome=needs_correction code=COMMAND_TOO_LONG]")));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn long_command_guard_is_correction_through_serial_runner() {
    tokio::time::timeout(Duration::from_secs(15), assert_long_command_contract(1))
        .await
        .unwrap();
}

#[tokio::test]
async fn long_command_guard_is_correction_through_parallel_runner() {
    tokio::time::timeout(Duration::from_secs(15), assert_long_command_contract(2))
        .await
        .unwrap();
}

#[tokio::test]
async fn long_command_guard_covers_input_aliases_without_starting_the_body() {
    let ctx = ToolCtx::new(
        "missing-command-directory".into(),
        "missing-command-directory".into(),
    );
    for field in ["command", "cmd", "script"] {
        let output = BashTool::default()
            .execute(json!({field:"a".repeat(MAX_COMMAND_CHARS + 1)}), &ctx)
            .await;
        assert_eq!(output.outcome, kanzei_harness::ToolOutcome::NeedsCorrection);
        assert_eq!(output.code.as_deref(), Some("COMMAND_TOO_LONG"));
        assert!(output.content.contains("文件中转"));
        assert!(
            !output.content.contains("workdir does not exist"),
            "guard must precede body workdir/spawn logic"
        );
    }
}

#[test]
fn long_command_guard_counts_utf16_units_at_the_exact_boundary() {
    use kanzei_harness::tool_pipeline::ToolGuard;
    let at_limit = "😀".repeat(MAX_COMMAND_CHARS / 2);
    assert!(CommandLengthGuard::new(&at_limit).check().is_continue());
    assert!(CommandLengthGuard::new(&(at_limit + "a"))
        .check()
        .is_break());
}

#[test]
fn bash_permission_aliases_preserve_command_workdir_and_denial() {
    use kanzei_harness::{Effect, Rule, Ruleset};
    let tool = BashTool::default();
    let ctx = ToolCtx::new("code-tree".into(), "project-root".into()).with_identity(
        "tree".into(),
        "project".into(),
        "run".into(),
        "process".into(),
    );
    let mapped_dir = kanzei_harness::permission::normalize_resource(
        &ctx.project_root.join("subdir").display().to_string(),
    );
    let raw_rule = json!({"command":"git commit", "workdir":"subdir"}).to_string();
    let contextual_rule = json!({"command":"git commit", "workdir":mapped_dir}).to_string();
    for field in ["command", "cmd", "script"] {
        let plain = json!({field:"git commit"});
        let located = json!({field:"git commit", "workdir":"subdir"});
        assert_eq!(tool.resources(&plain), ["git commit"]);
        assert_eq!(
            tool.resources(&located).as_slice(),
            std::slice::from_ref(&raw_rule)
        );
        assert_eq!(
            tool.resources_with_ctx(&located, &ctx).as_slice(),
            std::slice::from_ref(&contextual_rule)
        );
        for (resource, pattern) in [
            (tool.resources(&plain)[0].clone(), "git commit".to_owned()),
            (tool.resources(&located)[0].clone(), raw_rule.clone()),
            (
                tool.resources_with_ctx(&located, &ctx)[0].clone(),
                contextual_rule.clone(),
            ),
        ] {
            let mut rules = Ruleset::default();
            rules.push(Rule {
                action: "bash".into(),
                resource: "*".into(),
                effect: Effect::Allow,
            });
            rules.push_hard_deny(Rule {
                action: "bash".into(),
                resource: pattern,
                effect: Effect::Deny,
            });
            assert_eq!(
                rules.evaluate("bash", &resource),
                Effect::Deny,
                "{field}: {resource}"
            );
        }
    }
}

#[tokio::test]
async fn bash_permission_invalid_input_stays_unknown_and_cannot_execute() {
    let tool = BashTool::default();
    let ctx = ToolCtx::new("missing-directory".into(), "missing-directory".into());
    for input in [
        json!({}),
        json!({"cmd":42}),
        json!({"command":"git status","workdir":42}),
    ] {
        assert_eq!(tool.resources(&input), ["*"]);
        let resources = tool.resources_with_ctx(&input, &ctx);
        let resource: serde_json::Value = serde_json::from_str(&resources[0]).unwrap();
        assert_eq!(resource["command"], "*");
        let output = tool.execute(input, &ctx).await;
        assert_eq!(output.code.as_deref(), Some("INVALID_TOOL_INPUT"));
        assert_eq!(output.outcome, kanzei_harness::ToolOutcome::NeedsCorrection);
    }
}
