//! Exercise source admission and fixed-head budgeting through the real runner.
use anyhow::Context as _;
use kanzei_core::{run_once, AskFuture, AskPolicy, RunEvent, RunnerConfig};
use kanzei_harness::{
    fallible_source, refreshing_fallible_source, rule, Component, ContextRequirement,
    ContextSourceStatus, Effect, Harness, HarnessDraft, KanzeiConfig, ProfileKind, ResolveCtx,
    Tool, ToolCtx, ToolOutput,
};
use kanzei_llm::{LlmClient, Message, Part, ProxyConfig, Route};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Clone, Copy)]
enum Case {
    Stable,
    WireManifest,
    RefreshFailure,
    LargeSchema,
    FinalStep,
    BudgetViolation,
}

struct FileSource(ContextRequirement, Case);
impl Component for FileSource {
    fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
        let read = |ctx: &ResolveCtx| {
            let path = ctx.project_root.join("conventions.md");
            Ok(Some(
                std::fs::read_to_string(&path).with_context(|| path.display().to_string())?,
            ))
        };
        let source = match self.1 {
            Case::RefreshFailure => refreshing_fallible_source("fixture/conventions", self.0, read),
            _ => fallible_source("fixture/conventions", self.0, read),
        };
        draft.context.insert("fixture/conventions", source);
        if matches!(
            self.1,
            Case::RefreshFailure | Case::LargeSchema | Case::FinalStep | Case::BudgetViolation
        ) {
            draft.tools.insert("fixture", Arc::new(FixtureTool(self.1)));
            draft.permissions.push(rule("fixture", "*", Effect::Allow));
        }
        Ok(())
    }
}

struct FixtureTool(Case);
#[async_trait::async_trait]
impl Tool for FixtureTool {
    fn name(&self) -> &'static str {
        "fixture"
    }
    fn description(&self) -> String {
        "Read a repository document".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        match self.0 {
            Case::LargeSchema | Case::FinalStep => {
                json!({"type":"object","properties":{"path":{"type":"string","enum":(0..5000).map(|index| format!("modules/component-{index}/architecture/constraints-and-verification.md")).collect::<Vec<_>>()}}})
            }
            _ => json!({"type":"object","properties":{}}),
        }
    }
    async fn execute(&self, _: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        if matches!(self.0, Case::BudgetViolation) {
            std::fs::write(ctx.project_root.join("forbidden-side-effect"), "executed").unwrap();
            return ToolOutput::ok("provider ignored the final-step schema");
        }
        assert!(matches!(self.0, Case::RefreshFailure));
        std::fs::write(ctx.project_root.join("conventions.md"), [0xff, 0xfe]).unwrap();
        ToolOutput::ok("source changed between model steps")
    }
}

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new(label: &str) -> Self {
        Self(
            tempfile::Builder::new()
                .prefix(&format!("kz-context-run-{label}-"))
                .tempdir()
                .unwrap(),
        )
    }
}

#[test]
fn same_label_fixtures_keep_files_and_cleanup_independent() {
    let first = Fixture::new("soft-target");
    let second = Fixture::new("soft-target");
    assert_ne!(first.0.path(), second.0.path());
    std::fs::write(first.0.path().join("conventions.md"), "first policy").unwrap();
    std::fs::write(second.0.path().join("conventions.md"), "second policy").unwrap();
    assert_eq!(
        std::fs::read_to_string(first.0.path().join("conventions.md")).unwrap(),
        "first policy"
    );
    drop(first);
    assert_eq!(
        std::fs::read_to_string(second.0.path().join("conventions.md")).unwrap(),
        "second policy"
    );
}

async fn mock_provider(case: Case) -> (Route, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let route = Route::openai_at(
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        None,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let server_calls = calls.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut bytes = [0; 4096];
            let header_end = loop {
                let count = socket.read(&mut bytes).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&bytes[..count]);
                if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let length = String::from_utf8_lossy(&request[..header_end])
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            while request.len() < header_end + length {
                let count = socket.read(&mut bytes).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&bytes[..count]);
            }
            if matches!(case, Case::FinalStep | Case::BudgetViolation) {
                let body: serde_json::Value =
                    serde_json::from_slice(&request[header_end..]).unwrap();
                assert!(
                    body.get("tools")
                        .is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty)),
                    "the final convergence request must not send resident schemas: {body}"
                );
            }
            let index = server_calls.fetch_add(1, Ordering::SeqCst);
            let response = if (matches!(case, Case::RefreshFailure) && index == 0)
                || matches!(case, Case::BudgetViolation)
            {
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"change-source","type":"function","function":{"name":"fixture","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":1}})
            } else if matches!(case, Case::WireManifest) {
                use sha2::{Digest, Sha256};
                let digest = format!(
                    "{:x}",
                    Sha256::digest(&request[header_end..header_end + length])
                );
                json!({"choices":[{"index":0,"delta":{"content":digest},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":1}})
            } else {
                json!({"choices":[{"index":0,"delta":{"content":"done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":1}})
            };
            let body = format!("data: {response}\n\ndata: [DONE]\n\n");
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
    });
    (route, calls, server)
}

async fn run_fixture(
    fixture: &Fixture,
    requirement: ContextRequirement,
    prior: &[Message],
    case: Case,
) -> (
    anyhow::Result<kanzei_core::RunSummary>,
    Vec<RunEvent>,
    usize,
) {
    run_fixture_with_limit(fixture, requirement, prior, case, Some(32_000)).await
}

async fn run_fixture_with_limit(
    fixture: &Fixture,
    requirement: ContextRequirement,
    prior: &[Message],
    case: Case,
    context_limit: Option<u64>,
) -> (
    anyhow::Result<kanzei_core::RunSummary>,
    Vec<RunEvent>,
    usize,
) {
    let mut harness = Harness::default();
    harness.add(FileSource(requirement, case));
    let snapshot = harness
        .resolve(&ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: fixture.0.path().to_path_buf(),
            project_root: fixture.0.path().to_path_buf(),
            config: Arc::new(KanzeiConfig::default()),
        })
        .unwrap();
    let steps = if matches!(case, Case::FinalStep | Case::BudgetViolation) {
        1
    } else {
        3
    };
    let agent = serde_json::from_value(json!({"name":"fixture","profile":"dev","mode":"primary","steps":steps,"system":"Follow every project convention in full."})).unwrap();
    let config = RunnerConfig {
        digest_model: None,
        intensity: kanzei_harness::HarnessIntensity::Autonomous,
        model: "mock".into(),
        max_tokens: 4096,
        reasoning: Default::default(),
        service_tier: None,
        hosted_tools: vec![],
        context_limit,
        limits: Default::default(),
        recall: None,
        execution_policy: Default::default(),
        ask_policy: AskPolicy::Interactive,
        halt: None,
    };
    let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
    let (route, calls, server) = mock_provider(case).await;
    let mut events = Vec::new();
    let mut on_event = |event| events.push(event);
    let mut ask = |_| -> AskFuture { panic!("read-only fixture must not ask") };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &config,
            &ToolCtx::new(
                fixture.0.path().to_path_buf(),
                fixture.0.path().to_path_buf(),
            ),
            "Continue the task",
            None,
            prior,
            None,
            &mut on_event,
            &mut ask,
        ),
    )
    .await
    .expect("context admission must finish without network waits");
    server.abort();
    let _ = server.await;
    (result, events, calls.load(Ordering::SeqCst))
}

async fn assert_admitted_above_soft_target(context_limit: Option<u64>, paragraphs: usize) {
    let fixture = Fixture::new("soft-target");
    let conventions = "Policy: retain source, boundary, constraint, verification, and recovery.\n"
        .repeat(paragraphs);
    std::fs::write(fixture.0.path().join("conventions.md"), &conventions).unwrap();
    let prior = vec![
        Message::user_text("Existing task history must remain intact"),
        Message::assistant(vec![Part::Text {
            text: "Previous verified result".into(),
        }]),
    ];
    let (result, events, calls) = run_fixture_with_limit(
        &fixture,
        ContextRequirement::Required,
        &prior,
        Case::Stable,
        context_limit,
    )
    .await;
    let summary =
        result.expect("soft reserve and cold calibration cannot reject a valid or unknown window");
    assert_eq!(calls, 1);
    assert_eq!(summary.text, "done");
    assert_eq!(
        serde_json::to_value(&summary.messages[..prior.len()]).unwrap(),
        serde_json::to_value(&prior).unwrap(),
        "an unattainable soft target must not cause futile history compaction"
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        RunEvent::ContextCompacted { .. } | RunEvent::ContextPruned { .. }
    )));
    assert!(summary
        .context_report
        .iter()
        .any(|(key, chars)| key == "fixture/conventions"
            && *chars == conventions.trim().chars().count()));
}

#[tokio::test]
async fn unknown_window_does_not_turn_fallback_into_a_hard_limit() {
    // Raw fixed head > 32k: only the provider knows whether its actual window fits.
    assert_admitted_above_soft_target(None, 5000).await;
}

#[tokio::test]
async fn known_window_allows_head_above_soft_buffer_but_within_input_capacity() {
    // Raw fixed head ~24k: above 21,334 soft target, below 32,000 - 4,096 input capacity.
    assert_admitted_above_soft_target(Some(32_000), 1350).await;
}

#[tokio::test]
async fn cold_start_safety_factor_is_not_a_capacity_measurement() {
    // Raw fixed head ~15k fits; the cold-start 2x margin alone exceeds input capacity.
    assert_admitted_above_soft_target(Some(32_000), 850).await;
}

#[tokio::test]
async fn final_step_budgets_only_the_schemas_actually_sent_to_the_provider() {
    let fixture = Fixture::new("final-step");
    std::fs::write(
        fixture.0.path().join("conventions.md"),
        "complete small conventions",
    )
    .unwrap();
    let (result, events, calls) =
        run_fixture(&fixture, ContextRequirement::Required, &[], Case::FinalStep).await;
    assert_eq!(result.unwrap().text, "done");
    assert_eq!(
        calls, 1,
        "unused large schemas must not prevent the final request"
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        RunEvent::ContextCompacted { .. } | RunEvent::ContextPruned { .. }
    )));
}

#[tokio::test]
async fn required_source_failure_stops_before_any_provider_call() {
    let fixture = Fixture::new("required");
    std::fs::write(fixture.0.path().join("conventions.md"), [0xff, 0xfe]).unwrap();
    let (result, events, calls) =
        run_fixture(&fixture, ContextRequirement::Required, &[], Case::Stable).await;
    let error = result
        .err()
        .expect("invalid mandatory conventions must reject the run")
        .to_string();
    assert!(
        error.contains("fixture/conventions") && error.contains("conventions.md"),
        "{error}"
    );
    assert_eq!(calls, 0);
    assert!(events.iter().any(|event| matches!(event, RunEvent::ContextSources { sources } if sources.iter().any(|source| source.status == ContextSourceStatus::Failed && source.error.is_some()))));
}

#[tokio::test]
async fn optional_source_failure_is_reported_while_the_run_can_continue() {
    let fixture = Fixture::new("optional");
    std::fs::write(fixture.0.path().join("conventions.md"), [0xff, 0xfe]).unwrap();
    let (result, events, calls) =
        run_fixture(&fixture, ContextRequirement::Optional, &[], Case::Stable).await;
    assert_eq!(result.unwrap().text, "done");
    assert_eq!(calls, 1);
    assert!(events.iter().any(|event| matches!(event, RunEvent::ContextSources { sources } if sources.iter().any(|source| source.key == "fixture/conventions" && source.requirement == ContextRequirement::Optional && source.error.is_some()))));
    let snapshot = events
        .iter()
        .find_map(|event| match event {
            RunEvent::ContextRequest { snapshot } => Some(snapshot),
            _ => None,
        })
        .unwrap();
    assert!(snapshot
        .manifest
        .source_reads
        .iter()
        .any(|source| source["key"] == "fixture/conventions" && source["status"] == "failed"));
    assert!(!snapshot
        .manifest
        .blocks
        .iter()
        .any(|block| block.label == "fixture/conventions"));
}

#[tokio::test]
async fn oversized_complete_conventions_refuse_before_compaction_or_provider_calls() {
    let fixture = Fixture::new("large");
    let mut conventions = kanzei_harness::DEFAULT_CONVENTIONS.to_string();
    for index in 0..900 {
        conventions.push_str(&format!("\n## 模块 {index} 的职责与验收\n所有变更必须保留来源、依赖边界、测试证据与失败原因。未完成的工作不得标为完成；用户确认的约束必须完整遵守。\n"));
    }
    conventions.push_str("\n最后一条必须规则：保留恢复入口和完整原文。\n");
    std::fs::write(fixture.0.path().join("conventions.md"), &conventions).unwrap();
    let prior = vec![
        Message::user_text("此前任务"),
        Message::assistant(vec![Part::Text {
            text: "之前已经验证过的结论。".repeat(200),
        }]),
    ];
    let (result, events, calls) =
        run_fixture(&fixture, ContextRequirement::Required, &prior, Case::Stable).await;
    let error = result
        .err()
        .expect("fixed head alone exceeds the window")
        .to_string();
    for expected in [
        "固定上下文",
        "fixture/conventions",
        "schema",
        "32000",
        "精简",
        "context_limit/max_tokens",
    ] {
        assert!(error.contains(expected), "{error}");
    }
    assert_eq!(calls, 0, "neither digest nor main provider may be called");
    assert!(!events.iter().any(|event| matches!(
        event,
        RunEvent::ContextCompacted { .. } | RunEvent::ContextPruned { .. }
    )));
    assert!(events.iter().any(|event| matches!(event, RunEvent::ContextSources { sources } if sources.iter().any(|source| source.chars == conventions.trim().chars().count() && source.status == ContextSourceStatus::Loaded))));
    assert_eq!(
        std::fs::read_to_string(fixture.0.path().join("conventions.md")).unwrap(),
        conventions
    );
}

#[tokio::test]
async fn refreshed_required_failure_stops_next_request_but_optional_failure_updates_ledger() {
    for requirement in [ContextRequirement::Required, ContextRequirement::Optional] {
        let fixture = Fixture::new("refresh");
        std::fs::write(
            fixture.0.path().join("conventions.md"),
            "initial conventions",
        )
        .unwrap();
        let (result, events, calls) =
            run_fixture(&fixture, requirement, &[], Case::RefreshFailure).await;
        assert!(events.iter().any(|event| matches!(event, RunEvent::ContextSources { sources } if sources.iter().any(|source| source.refresh_each_step && source.status == ContextSourceStatus::Loaded))));
        assert!(events.iter().any(|event| matches!(event, RunEvent::ContextSources { sources } if sources.iter().any(|source| source.refresh_each_step && source.status == ContextSourceStatus::Failed && source.error.as_ref().unwrap().contains("conventions.md")))));
        match requirement {
            ContextRequirement::Required => {
                assert!(result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("fixture/conventions"));
                assert_eq!(calls, 1, "second provider request must never start");
            }
            ContextRequirement::Optional => {
                let summary = result.unwrap();
                assert_eq!(summary.text, "done");
                assert_eq!(calls, 2);
                assert!(
                    !summary
                        .context_report
                        .iter()
                        .any(|(key, _)| key == "fixture/conventions"),
                    "failed refresh must remove stale source size from the ledger"
                );
            }
        }
    }
}

#[tokio::test]
async fn oversized_tool_schema_is_diagnosed_even_when_system_is_small() {
    let fixture = Fixture::new("schema");
    std::fs::write(
        fixture.0.path().join("conventions.md"),
        "complete small conventions",
    )
    .unwrap();
    let (result, events, calls) = run_fixture(
        &fixture,
        ContextRequirement::Required,
        &[],
        Case::LargeSchema,
    )
    .await;
    let error = result
        .err()
        .expect("schema is part of the fixed floor")
        .to_string();
    assert!(
        error.contains("tools/schema") && error.contains("按需加载"),
        "{error}"
    );
    assert_eq!(calls, 0);
    assert!(!events.iter().any(|event| matches!(
        event,
        RunEvent::ContextCompacted { .. } | RunEvent::ContextPruned { .. }
    )));
}

#[tokio::test]
async fn exhausted_budget_rejects_provider_tool_calls_before_side_effects() {
    let fixture = Fixture::new("budget-violation");
    std::fs::write(
        fixture.0.path().join("conventions.md"),
        "Never exceed the tool budget",
    )
    .unwrap();
    let (result, events, calls) = run_fixture(
        &fixture,
        ContextRequirement::Required,
        &[],
        Case::BudgetViolation,
    )
    .await;
    let summary = result.unwrap();
    assert!(summary.step_limit_reached);
    assert_eq!(summary.outcome(), "incomplete");
    assert_eq!(calls, 1);
    assert!(
        !fixture.0.path().join("forbidden-side-effect").exists(),
        "a provider tool call must not execute after the budget is exhausted"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        RunEvent::ToolEnd { code: Some(code), .. } if code == "STEP_BUDGET_EXHAUSTED"
    )));
}

#[tokio::test]
async fn request_manifest_records_exact_named_sources_and_final_tool_surface() {
    use kanzei_core::context_manifest::ContextManifest;
    let fixture = Fixture::new("manifest");
    let original = "中文约定：保留原始验证证据。";
    std::fs::write(fixture.0.path().join("conventions.md"), original).unwrap();
    let (result, events, calls) =
        run_fixture(&fixture, ContextRequirement::Required, &[], Case::FinalStep).await;
    result.unwrap();
    assert_eq!(calls, 1);
    let snapshot = events
        .iter()
        .find_map(|event| match event {
            RunEvent::ContextRequest { snapshot } => Some(snapshot),
            _ => None,
        })
        .expect("a dispatched request needs a manifest");
    let manifest: &ContextManifest = &snapshot.manifest;
    let source = manifest
        .blocks
        .iter()
        .find(|block| block.label == "fixture/conventions")
        .unwrap();
    assert_eq!(snapshot.contents[&source.sha256], original);
    assert_eq!(source.chars, original.chars().count());
    assert_eq!(source.bytes, original.len());
    assert!(
        manifest
            .blocks
            .iter()
            .all(|block| block.kind != "tool_schema"),
        "final convergence removes schemas from both actual request and manifest"
    );
    assert_eq!(manifest.wire_sha256.len(), 64);
    assert!(manifest.wire_bytes > original.len());
    std::fs::write(fixture.0.path().join("conventions.md"), "changed later").unwrap();
    assert_eq!(
        snapshot.contents[&source.sha256], original,
        "later file changes cannot rewrite request evidence"
    );
}

#[tokio::test]
async fn request_fingerprint_matches_bytes_received_by_the_provider() {
    let fixture = Fixture::new("wire-manifest");
    std::fs::write(
        fixture.0.path().join("conventions.md"),
        "中文规则与来源版本",
    )
    .unwrap();
    let (result, events, _) = run_fixture(
        &fixture,
        ContextRequirement::Required,
        &[],
        Case::WireManifest,
    )
    .await;
    let actual_wire_sha256 = result.unwrap().text;
    let snapshot = events
        .iter()
        .find_map(|event| match event {
            RunEvent::ContextRequest { snapshot } => Some(snapshot),
            _ => None,
        })
        .unwrap();
    assert_eq!(actual_wire_sha256, snapshot.manifest.wire_sha256);
}
