//! Local, bounded acceptance through the real configured model and production research harness.
//! No scripted scientific decisions or paper body; the test-user selects the first published direction.
use kanzei_core::{AskFuture, AskPolicy, AskRequest, AskResponse, RunEvent};
use kanzei_harness::{config::KanzeiConfig, HarnessIntensity, ProfileKind, ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};
use kanzei_tools::research_workflow::{self as workflow, ResearchWorkflowContext, Stage};
use serde_json::{json, Value};
use std::{io::Write, path::PathBuf, sync::Arc, time::Duration};

fn record(file: &mut std::fs::File, value: Value) {
    writeln!(file, "{value}").expect("write acceptance trace");
    file.flush().expect("flush acceptance trace");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let destination = PathBuf::from(std::env::args().nth(1).ok_or_else(|| {
        anyhow::anyhow!("usage: auto_research_live <output-directory> [--resume]")
    })?);
    let resume = std::env::args().nth(2).as_deref() == Some("--resume");
    anyhow::ensure!(
        resume == destination.exists(),
        "Choose a new directory, or explicitly --resume an existing case"
    );
    let config_root = std::env::current_dir()?;
    let (config, _) = KanzeiConfig::load_with_warnings_at_root(&config_root)?;
    let config = Arc::new(config);
    std::fs::create_dir_all(&destination)?;
    let root = destination.canonicalize()?;
    let topic = "summation-accuracy";
    std::fs::create_dir_all(root.join(".kanzei/research").join(topic))?;
    if !resume {
        workflow::start(&root, topic, Default::default(), 14).map_err(anyhow::Error::msg)?;
    }
    let initial = workflow::load(&root, topic)
        .map_err(anyhow::Error::msg)?
        .ok_or_else(|| anyhow::anyhow!("Missing saved workflow"))?;
    let attempt = if resume {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        root.join(format!("acceptance-resume-{stamp}"))
    } else {
        root.clone()
    };
    std::fs::create_dir_all(&attempt)?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(value) => ProxyConfig::Explicit(value.into()),
    };
    let resolved = config.resolve_model("primary")?;
    std::fs::write(
        attempt.join("acceptance-config.json"),
        serde_json::to_vec_pretty(&json!({
            "model":format!("{}:{}",resolved.provider_name,resolved.model),
            "execution":"local CPU only","max_turns":16,"steps_per_turn":24,"max_runs":14,
            "test_user":"select first candidate; require one follow-up round; never supply experiments or prose"
        }))?,
    )?;
    let route = kanzei_core::build_route(&resolved, &proxy).await?;
    let client = LlmClient::new(&proxy)?;
    let rctx = ResolveCtx {
        profile: ProfileKind::Research,
        cwd: root.clone(),
        project_root: root.clone(),
        config: config.clone(),
    };
    let harness = kanzei_tools::run::build_harness(
        |_| {},
        |harness| {
            harness.add(ResearchWorkflowContext(topic.into()));
        },
    );
    let snapshot = harness.resolve(&rctx)?;
    let mut agent = snapshot.select_agent(None)?.clone();
    agent.steps = 24;
    let mut runner = kanzei_tools::run::build_runner_config(
        &resolved,
        &config,
        None,
        &root,
        AskPolicy::NonInteractive,
        None,
    );
    runner.intensity = HarnessIntensity::Paired;
    runner.max_tokens = 4096;
    runner.recall = None;
    runner.limits.transport_retries = Some(1);
    runner.limits.rate_limit_retries = Some(1);
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let mut trace = std::fs::File::create(attempt.join("model-events.jsonl"))?;
    if resume && (initial.paused || initial.waiting_reason.is_some()) {
        record(
            &mut trace,
            json!({"actor":"test_user","action":"resume","revision":initial.revision}),
        );
        workflow::user_action(&root, topic, initial.revision, "resume", None)
            .map_err(anyhow::Error::msg)?;
    }
    let mut prior = Vec::new();
    let mut selections = usize::from(initial.selected_direction.is_some());
    let mut unchanged = 0;
    let mut verdict = "turn_budget_exhausted".to_string();
    let mut tokens = json!([]);
    for turn in 1..=16 {
        let state = workflow::load(&root, topic)
            .map_err(anyhow::Error::msg)?
            .unwrap();
        if state.stage == Stage::Completed {
            verdict = if state.full_rounds.is_empty() {
                "missing_followup_round"
            } else {
                "passed"
            }
            .into();
            break;
        }
        if state.stage == Stage::ChooseDirection && selections == 0 {
            let direction = state
                .directions
                .first()
                .ok_or_else(|| anyhow::anyhow!("map has no candidates"))?;
            record(
                &mut trace,
                json!({"actor":"test_user","action":"select","direction":direction.id}),
            );
            workflow::user_action(&root, topic, state.revision, "select", Some(&direction.id))
                .map_err(anyhow::Error::msg)?;
            selections += 1;
            prior.clear(); // Exercise recovery from durable topic artifacts after the selection boundary.
            continue;
        }
        if !state.runnable() {
            verdict = format!(
                "waiting: {}",
                state.waiting_reason.as_deref().unwrap_or("user selection")
            );
            break;
        }
        let prompt = format!(
            r#"Continue the local AUTO research acceptance task for topic {topic} until the next user-selection boundary or completed PDF. Use production research_workflow tools and current revisions. The scope is a small synthetic CPU experiment comparing floating-point summation methods under cancellation. You choose the precise falsifiable question, benchmark, controls and interpretation from actual evidence; use Python standard library only, with each process under 30 seconds. Local Python is `python`; prepare_compute must use gpu_required=false, no environment_id, no dependency installation. Never SSH or touch any path outside this test workspace.
Read one or two primary sources (the official Python math.fsum documentation is a suitable starting point), register sources, and publish a research map. The test-user will select the first candidate after you stop at choose_direction. All code, analysis and paper prose must be authored by you using tools; run actual baseline/MVP/full experiments and consume their persisted metrics. Use finite scalar callbacks with @@kanzei JSON, and preserve errors rather than inventing observations. Complete the standard six-item full matrix first. After the first full analysis, use submit_analysis next=iterate with a meaningful reason to add one stress condition; reuse unchanged successful items via reuse_results and run the new condition. After the second analysis, write and compile a concise English article. Do not claim broad scientific significance. Do not use research_write for this workflow; use paper_init, submit_paper, review_paper and compile_paper. Correct tool failures and keep moving within the 14-run budget. Read full schemas instead of guessing field names. Sources/findings use source/finding tools. Current checkpoint:
{}"#,
            state.guidance()
        );
        let mut on_event = |event| match event {
            RunEvent::ToolStart {
                id, name, input, ..
            } => {
                println!(
                    "turn {turn} {name} {}",
                    input["action"].as_str().unwrap_or("")
                );
                record(
                    &mut trace,
                    json!({"turn":turn,"event":"tool_start","id":id,"name":name,"input":input}),
                );
            }
            RunEvent::ToolEnd {
                id,
                name,
                ok,
                preview,
                ..
            } => record(
                &mut trace,
                json!({"turn":turn,"event":"tool_end","id":id,"name":name,"ok":ok,"output":preview}),
            ),
            RunEvent::AssistantMessageCommitted { message, .. }
            | RunEvent::ToolResultsCommitted { message, .. } => record(
                &mut trace,
                json!({"turn":turn,"event":"message","message":message}),
            ),
            _ => {}
        };
        let mut ask = |_: AskRequest| -> AskFuture { Box::pin(async { AskResponse::Cancelled }) };
        let output = tokio::time::timeout(
            Duration::from_secs(600),
            kanzei_core::run_once(
                &client,
                &route,
                &snapshot,
                &agent,
                &runner,
                &ctx,
                &prompt,
                None,
                &prior,
                None,
                None,
                &mut on_event,
                &mut ask,
            ),
        )
        .await;
        let summary = match output {
            Ok(Ok(summary)) => summary,
            Ok(Err(error)) => {
                verdict = format!("model_error: {error}");
                break;
            }
            Err(_) => {
                verdict = "model_turn_timeout".into();
                break;
            }
        };
        tokens
            .as_array_mut()
            .unwrap()
            .push(json!({"turn":turn,"usage":summary.usage,"steps":summary.steps}));
        prior = summary.messages;
        std::fs::write(
            attempt.join("conversation.json"),
            serde_json::to_vec_pretty(&prior)?,
        )?;
        let current = workflow::load(&root, topic)
            .map_err(anyhow::Error::msg)?
            .unwrap();
        unchanged = if current.revision == state.revision {
            unchanged + 1
        } else {
            0
        };
        println!(
            "turn {turn} checkpoint {:?} revision {}",
            current.stage, current.revision
        );
        if unchanged >= 3 {
            verdict = "no_checkpoint_progress".into();
            break;
        }
    }
    let state = workflow::load(&root, topic)
        .map_err(anyhow::Error::msg)?
        .unwrap();
    if state.stage == Stage::Completed && !state.full_rounds.is_empty() {
        verdict = "passed".into();
    }
    let runs = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))?
        .list_research_runs(topic)?;
    let report = json!({"status":verdict,"stage":state.stage,"revision":state.revision,"rounds":state.full_rounds.len()+1,
        "run_count":runs.len(),"paper":state.paper,"tokens":tokens,"scope":"Real configured LLM; local synthetic CPU case; test-user selection; no remote execution"});
    std::fs::write(
        attempt.join("acceptance-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    anyhow::ensure!(
        verdict == "passed",
        "Live acceptance did not complete: {verdict}"
    );
    Ok(())
}
