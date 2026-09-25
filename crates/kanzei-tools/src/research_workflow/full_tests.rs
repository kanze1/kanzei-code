use super::*;

fn fixture() -> PathBuf {
    let root = super::tests::root();
    super::tests::through_selection(&root);
    super::tests::advance_ok(&root, "define_mvp", super::tests::mvp());
    let mut state = load(&root, "demo").unwrap().unwrap();
    state.version = 2;
    state.max_mvp_runs = 20;
    state.stage = Stage::PlanFull;
    state.compute = Some(compute::PreparedCompute {
        spec: compute::ComputeSpec {
            environment_id: None,
            python: "python".into(),
            gpu_required: false,
            min_vram_mb: 0,
            requirements: None,
            upload_files: vec![],
        },
        kind: "local".into(),
        host: None,
        workdir: root.to_string_lossy().into(),
        python: "python".into(),
        snapshot: json!({"gpu_available":false}),
        prepared_at: now_ms(),
    });
    save(&root, &state).unwrap();
    root
}

fn plan() -> Value {
    let experiments: Vec<_> = [("b0","baseline",0),("b1","baseline",1),("m0","main",0),("m1","main",1),("a0","ablation",0),("r0","robustness",0)].iter().map(|(id,role,seed)| json!({"id":id,"role":role,"seed":seed,"description":"Controlled experiment"})).collect();
    json!({"full_plan":{"protocol":"protocol.md","experiments":experiments}})
}

fn matrix_run(
    state: &Workflow,
    spec: &ExperimentSpec,
    number: usize,
) -> kanzei_core::ResearchRunRecord {
    let compute = state.compute.as_ref().unwrap();
    serde_json::from_value(json!({
        "result_id":format!("E-001-{number}"),"exploration_id":"E-001","topic":"demo","status":"succeeded",
        "execution_json":json!({"kind":compute.kind,"host":compute.host,"workdir":compute.workdir,"environment_id":compute.spec.environment_id}).to_string(),
        "policy":"relaxed","lease_id":"","max_duration_ms":1000,"cleanup":"retain","started_at":now_ms(),"finished_at":now_ms(),
        "exit_code":0,"cancel_reason":null,"params_text":json!({"experiment_id":spec.id,"role":spec.role,"seed":spec.seed}).to_string(),"code_ref_json":"{}","environment_snapshot_ref":"","artifacts_json":"[]","metrics_last_json":"{\"score\":0.5}","progress_json":"{}","metrics_series_path":"","cost_json":"{}","callback_stats_json":"{}","heartbeat_at":null,"terminal_log_path":""
    })).unwrap()
}

fn record_matrix(root: &Path) -> Workflow {
    super::tests::advance_ok(root, "define_full", plan());
    let state = load(root, "demo").unwrap().unwrap();
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root)).unwrap();
    for (i, spec) in state
        .full_plan
        .as_ref()
        .unwrap()
        .experiments
        .iter()
        .enumerate()
    {
        let run = matrix_run(&state, spec, i + 10);
        store.upsert_research_run(&run).unwrap();
        super::tests::advance_ok(
            root,
            "record_full",
            json!({"full_results":[{"experiment_id":spec.id,"result_id":run.result_id}]}),
        );
    }
    load(root, "demo").unwrap().unwrap()
}

#[test]
fn full_matrix_rejects_results_from_another_environment() {
    let root = fixture();
    let state = super::tests::advance_ok(&root, "define_full", plan());
    let spec = &state.full_plan.as_ref().unwrap().experiments[0];
    let mut run = matrix_run(&state, spec, 10);
    let mut execution: Value = serde_json::from_str(&run.execution_json).unwrap();
    execution["workdir"] = json!("another-environment");
    run.execution_json = execution.to_string();
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    store.upsert_research_run(&run).unwrap();
    let error = advance(
        &root,
        "demo",
        state.revision,
        "record_full",
        &json!({
            "full_results":[{"experiment_id":spec.id,"result_id":run.result_id}]
        }),
    )
    .unwrap_err();
    assert!(error.contains("环境"), "{error}");
    let unchanged = load(&root, "demo").unwrap().unwrap();
    assert_eq!(unchanged.revision, state.revision);
    assert!(unchanged.full_results.is_empty());
}

#[test]
fn full_matrix_does_not_rerun_successful_unregistered_item() {
    let root = fixture();
    let state = super::tests::advance_ok(&root, "define_full", plan());
    let spec = &state.full_plan.as_ref().unwrap().experiments[0];
    let completed = matrix_run(&state, spec, 10);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    store.upsert_research_run(&completed).unwrap();
    let mut retry = matrix_run(&state, spec, 11);
    retry.status = "running".into();
    retry.finished_at = None;
    let error = record_run_start(&root, &retry).unwrap_err();
    assert!(
        error.contains(&completed.result_id) && error.contains("record_full"),
        "{error}"
    );
    assert!(store.get_research_run(&retry.result_id).unwrap().is_none());
    let recovered = super::tests::advance_ok(
        &root,
        "record_full",
        json!({
            "full_results":[{"experiment_id":spec.id,"result_id":completed.result_id}]
        }),
    );
    assert_eq!(recovered.full_results.len(), 1);
}

#[test]
fn full_matrix_allows_retry_after_failed_or_unusable_results() {
    for case in [
        "failed",
        "cancelled",
        "missing_metric",
        "wrong_seed",
        "wrong_role",
        "old_plan",
        "wrong_environment",
        "other_exploration",
    ] {
        let root = fixture();
        let state = super::tests::advance_ok(&root, "define_full", plan());
        let spec = &state.full_plan.as_ref().unwrap().experiments[0];
        let mut previous = matrix_run(&state, spec, 10);
        match case {
            "failed" | "cancelled" => previous.status = case.into(),
            "missing_metric" => previous.metrics_last_json = "{}".into(),
            "wrong_seed" => {
                previous.params_text =
                    json!({"experiment_id":spec.id,"role":spec.role,"seed":99}).to_string()
            }
            "wrong_role" => {
                previous.params_text =
                    json!({"experiment_id":spec.id,"role":"main","seed":spec.seed}).to_string()
            }
            "old_plan" => previous.started_at = state.full_plan.as_ref().unwrap().created_at - 1,
            "wrong_environment" => previous.execution_json = "{}".into(),
            "other_exploration" => previous.exploration_id = "E-002".into(),
            _ => unreachable!(),
        }
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store.upsert_research_run(&previous).unwrap();
        let mut retry = matrix_run(&state, spec, 11);
        retry.status = "running".into();
        retry.finished_at = None;
        record_run_start(&root, &retry).unwrap_or_else(|error| panic!("{case}: {error}"));
        let recovery = lifecycle::recovery(&root, &state).unwrap();
        assert_eq!(recovery["pending_full_results"], json!([]), "{case}");
        assert_eq!(
            recovery["active_runs"][0]["result_id"], retry.result_id,
            "{case}"
        );
    }
}

#[tokio::test]
async fn workflow_get_recovers_pending_results_and_budget_without_mutating_state() {
    use kanzei_harness::{Tool, ToolCtx};

    let root = fixture();
    let state = super::tests::advance_ok(&root, "define_full", plan());
    let specs = &state.full_plan.as_ref().unwrap().experiments;
    let completed = matrix_run(&state, &specs[0], 10);
    let mut failed = matrix_run(&state, &specs[1], 11);
    failed.status = "failed".into();
    let mut active = matrix_run(&state, &specs[2], 12);
    active.status = "running".into();
    active.finished_at = None;
    let mut old = matrix_run(&state, &specs[3], 13);
    old.started_at = state.started_at - 1;
    let mut foreign = matrix_run(&state, &specs[4], 14);
    foreign.topic = "other".into();
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    for run in [&completed, &failed, &active, &old, &foreign] {
        store.upsert_research_run(run).unwrap();
    }
    let paused = user_action(&root, "demo", state.revision, "pause", None).unwrap();
    let resumed = user_action(&root, "demo", paused.revision, "resume", None).unwrap();
    let before = std::fs::read(path(&root, "demo").unwrap()).unwrap();
    let output = ResearchWorkflowTool {
        topic: Some("demo".into()),
    }
    .execute(
        json!({"action":"get","topic":"demo"}),
        &ToolCtx::new(root.clone(), root.clone()),
    )
    .await;
    assert!(!output.is_error, "{}", output.content);
    let view: Value = serde_json::from_str(&output.content).unwrap();
    assert_eq!(view["workflow"]["revision"], resumed.revision);
    assert_eq!(
        view["recovery"]["experiment_budget"],
        json!({"limit":20,"used":3,"remaining":17})
    );
    assert_eq!(
        view["recovery"]["active_runs"],
        json!([{"result_id":active.result_id,"exploration_id":"E-001","status":"running"}])
    );
    assert_eq!(
        view["recovery"]["pending_full_results"],
        json!([{"experiment_id":specs[0].id,"result_id":completed.result_id}])
    );
    assert_eq!(std::fs::read(path(&root, "demo").unwrap()).unwrap(), before);
    let recorded = super::tests::advance_ok(
        &root,
        "record_full",
        json!({
            "full_results":view["recovery"]["pending_full_results"]
        }),
    );
    let recovery = lifecycle::recovery(&root, &recorded).unwrap();
    assert_eq!(recovery["pending_full_results"], json!([]));
    assert_eq!(recovery["experiment_budget"]["used"], 3);
    let reduced = set_run_budget(&root, "demo", recorded.revision, 2).unwrap();
    assert_eq!(
        lifecycle::recovery(&root, &reduced).unwrap()["experiment_budget"]["remaining"],
        0
    );
}

#[test]
fn mvp_rejects_results_from_before_compute_preparation() {
    let root = fixture();
    let mut state = load(&root, "demo").unwrap().unwrap();
    state.stage = Stage::Prepare;
    save(&root, &state).unwrap();
    let spec = serde_json::from_value(plan()["full_plan"]["experiments"][0].clone()).unwrap();
    let mut run = matrix_run(&state, &spec, 10);
    run.started_at = state.compute.as_ref().unwrap().prepared_at - 1;
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    store.upsert_research_run(&run).unwrap();
    let error = advance(
        &root,
        "demo",
        state.revision,
        "environment_ready",
        &json!({
            "artifact":"prepare.md","baseline_result":run.result_id
        }),
    )
    .unwrap_err();
    assert!(error.contains("环境准备"), "{error}");
    assert_eq!(
        load(&root, "demo").unwrap().unwrap().revision,
        state.revision
    );
}

#[test]
fn mvp_rejects_duplicate_results() {
    let root = fixture();
    let mut state = load(&root, "demo").unwrap().unwrap();
    state.stage = Stage::RunMvp;
    save(&root, &state).unwrap();
    let spec = serde_json::from_value(plan()["full_plan"]["experiments"][0].clone()).unwrap();
    let run = matrix_run(&state, &spec, 10);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    store.upsert_research_run(&run).unwrap();
    let error = advance(
        &root,
        "demo",
        state.revision,
        "record_mvp",
        &json!({
            "result_ids":[run.result_id,run.result_id]
        }),
    )
    .unwrap_err();
    assert!(error.contains("重复"), "{error}");
    assert_eq!(load(&root, "demo").unwrap().unwrap().stage, Stage::RunMvp);
}

#[tokio::test]
async fn runner_inherits_prepared_environment_and_its_policy() {
    use kanzei_harness::{Tool, ToolCtx, ToolOutcome};

    let root = fixture();
    let mut state = load(&root, "demo").unwrap().unwrap();
    state.stage = Stage::Prepare;
    state.compute.as_mut().unwrap().spec.environment_id = Some("ENV-test".into());
    save(&root, &state).unwrap();
    std::fs::write(root.join(".kanzei/research/environments.md"), format!(
        "## ENV-test [active]\n- kind: local\n- host: \n- 归属: test\n- 执行策略: approval\n- gpu: unknown\n- workdir: {}\n- 运行时限: 10m\n- 计费: none\n- 凭据引用: \n- 准备步骤: prepared test environment\n",
        root.join("registered-parent").display()
    )).unwrap();
    std::fs::write(
        root.join("callback.txt"),
        "@@kanzei {\"t\":\"metric\",\"ts\":1,\"name\":\"score\",\"value\":0.4}\n",
    )
    .unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let mut input = json!({
        "action":"run","topic":"demo","exploration_id":"E-001","result_id":"E-001-01",
        "execution":{"kind":"local","command":if cfg!(windows) { "type callback.txt" } else { "cat callback.txt" }},
        "max_duration_ms":10000,"policy":"relaxed"
    });
    let tool = crate::research_runner::ResearchRunnerTool;
    let needs_confirmation = tool.execute(input.clone(), &ctx).await;
    assert_eq!(
        needs_confirmation.outcome,
        ToolOutcome::NeedsConfirmation,
        "{}",
        needs_confirmation.content
    );
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    assert!(store.get_research_run("E-001-01").unwrap().is_none());
    input["confirmed"] = json!(true);
    let output = tool.execute(input, &ctx).await;
    assert!(!output.is_error, "{}", output.content);
    let run = store.get_research_run("E-001-01").unwrap().unwrap();
    let execution: Value = serde_json::from_str(&run.execution_json).unwrap();
    assert_eq!(execution["environment_id"], "ENV-test");
    assert_eq!(
        execution["workdir"],
        state.compute.as_ref().unwrap().workdir
    );
    assert_eq!(run.policy, "approval");
    assert!(has_metric(&state, &run));
    super::tests::advance_ok(
        &root,
        "environment_ready",
        json!({
            "artifact":"prepare.md","baseline_result":run.result_id
        }),
    );
}

#[test]
fn full_matrix_requires_controls_paired_seeds_and_budget() {
    let root = fixture();
    let state = load(&root, "demo").unwrap().unwrap();
    let mut bad = plan();
    bad["full_plan"]["experiments"][1]["seed"] = json!(5);
    assert!(advance(&root, "demo", state.revision, "define_full", &bad).is_err());
    let mut duplicate = plan();
    let mut extra = duplicate["full_plan"]["experiments"][0].clone();
    extra["id"] = json!("duplicate-seed");
    duplicate["full_plan"]["experiments"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    assert!(
        advance(&root, "demo", state.revision, "define_full", &duplicate)
            .unwrap_err()
            .contains("重复计入")
    );
    let state = set_run_budget(&root, "demo", state.revision, 2).unwrap();
    assert!(
        advance(&root, "demo", state.revision, "define_full", &plan())
            .unwrap_err()
            .contains("预算")
    );
    set_run_budget(&root, "demo", state.revision, 20).unwrap();
    let state = record_matrix(&root);
    assert_eq!(state.stage, Stage::Analyze);
    assert_eq!(state.full_results.len(), 6);
    assert!(topic_dir(&root, "demo")
        .unwrap()
        .join("results.json")
        .is_file());
}

#[tokio::test]
async fn paper_rejects_invented_numbers_unknown_citations_and_changed_inputs() {
    let root = fixture();
    record_matrix(&root);
    let state = load(&root, "demo").unwrap().unwrap();
    assert!(advance(
        &root,
        "demo",
        state.revision,
        "submit_analysis",
        &json!({"artifact":"interpret.md","verdict":"invented"})
    )
    .is_err());
    assert!(!topic_dir(&root, "demo").unwrap().join("report.md").exists());
    super::tests::advance_ok(
        &root,
        "submit_analysis",
        json!({"artifact":"interpret.md","verdict":"inconclusive"}),
    );
    assert!(topic_dir(&root, "demo")
        .unwrap()
        .join("report.md")
        .is_file());
    let state = load(&root, "demo").unwrap().unwrap();
    paper::initialize(
        &root,
        "demo",
        state.revision,
        &json!({"title":"Evidence review test"}),
    )
    .unwrap();
    let dir = topic_dir(&root, "demo").unwrap();
    let text=format!("\\documentclass{{article}}\n\\begin{{document}}\n\\begin{{abstract}}Controlled test.\\end{{abstract}}\n\\section{{Introduction}}\n{}\n\\section{{Methods}}\n{}\n\\section{{Results}}\nObserved mean was 0.5.\n\\input{{results-table}}\n\\section{{Limitations}}This is a fixture, not research evidence.\n\\section{{Conclusion}}The finite test supports only its protocol.\\cite{{S-001}}\n\\input{{references}}\n\\end{{document}}", "Explicit scope and provenance. ".repeat(15), "Matched seeds and identical inputs. ".repeat(15));
    std::fs::write(dir.join("latex/paper.tex"), &text).unwrap();
    let state = load(&root, "demo").unwrap().unwrap();
    let ids: Vec<_> = state
        .full_results
        .iter()
        .map(|r| r.result_id.clone())
        .collect();
    let mut claims =
        json!([{"text":"Observed mean was 0.5.","result_ids":ids,"metric":"score","value":0.9}]);
    let state = super::tests::advance_ok(&root, "submit_paper", json!({"claims":claims}));
    assert!(
        advance(&root, "demo", state.revision, "review_paper", &json!({}))
            .unwrap_err()
            .contains("不一致")
    );
    claims[0]["value"] = json!(0.5);
    super::tests::advance_ok(&root, "submit_paper", json!({"claims":claims}));
    std::fs::write(dir.join("latex/paper.tex"), text.replace("S-001", "S-999")).unwrap();
    let state = load(&root, "demo").unwrap().unwrap();
    assert!(
        advance(&root, "demo", state.revision, "review_paper", &json!({}))
            .unwrap_err()
            .contains("S-999")
    );
    std::fs::write(dir.join("latex/paper.tex"), text).unwrap();
    let state = super::tests::advance_ok(&root, "review_paper", json!({}));
    std::fs::write(dir.join("latex/results-table.tex"), "Changed table").unwrap();
    assert!(paper::compile(&root, "demo", state.revision)
        .await
        .unwrap_err()
        .contains("已修改"));
    assert_eq!(
        load(&root, "demo").unwrap().unwrap().stage,
        Stage::CompilePaper
    );
}

#[test]
fn legacy_mvp_completion_can_extend_without_erasing_results() {
    let root = fixture();
    let mut state = load(&root, "demo").unwrap().unwrap();
    state.version = 1;
    state.stage = Stage::Completed;
    state.result_ids = vec!["E-001-01".into()];
    save(&root, &state).unwrap();
    let extended = user_action(&root, "demo", state.revision, "extend", None).unwrap();
    assert_eq!(extended.stage, Stage::PlanFull);
    assert_eq!(extended.version, 2);
    assert_eq!(extended.result_ids, state.result_ids);
}

fn followup_plan(state: &Workflow) -> Value {
    let mut input = plan();
    input["reuse_results"] =
        serde_json::to_value(&state.full_rounds.last().unwrap().results).unwrap();
    input["full_plan"]["experiments"].as_array_mut().unwrap().push(json!({
        "id":"robustness-extra","role":"robustness","seed":7,"description":"Additional stress condition"
    }));
    input
}

fn request_followup(root: &Path) -> Workflow {
    super::tests::advance_ok(
        root,
        "submit_analysis",
        json!({
            "artifact":"interpret.md","verdict":"inconclusive","next":"iterate","reason":"Add a stress condition before drawing conclusions"
        }),
    )
}

#[test]
fn followup_preserves_prior_evidence_and_only_budgets_new_runs() {
    let root = fixture();
    let first = record_matrix(&root);
    let dir = topic_dir(&root, "demo").unwrap();
    let old_protocol = std::fs::read(dir.join("protocol.md")).unwrap();
    let old_results = std::fs::read(dir.join("results.json")).unwrap();
    let reopened = request_followup(&root);
    assert_eq!(reopened.stage, Stage::PlanFull);
    assert_eq!(reopened.full_rounds[0].results.len(), 6);
    assert!(reopened.full_results.is_empty());
    let archive = dir.join(&reopened.full_rounds[0].artifact_root);
    std::fs::write(
        dir.join("protocol.md"),
        "# Revised protocol\nAdd one stress condition.",
    )
    .unwrap();
    assert_eq!(
        std::fs::read(archive.join("protocol.md")).unwrap(),
        old_protocol
    );
    assert_eq!(
        std::fs::read(archive.join("results.json")).unwrap(),
        old_results
    );
    let budgeted = set_run_budget(&root, "demo", reopened.revision, 7).unwrap();
    let state = advance(
        &root,
        "demo",
        budgeted.revision,
        "define_full",
        &followup_plan(&reopened),
    )
    .unwrap();
    assert_eq!(state.full_results.len(), 6);
    assert!(
        state.full_plan.as_ref().unwrap().created_at
            >= first.full_plan.as_ref().unwrap().created_at
    );
    let spec = state
        .full_plan
        .as_ref()
        .unwrap()
        .experiments
        .last()
        .unwrap();
    let mut run = matrix_run(&state, spec, 20);
    run.metrics_last_json = "{\"score\":0.9}".into();
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    store.upsert_research_run(&run).unwrap();
    let analyzed = super::tests::advance_ok(
        &root,
        "record_full",
        json!({"full_results":[{"experiment_id":spec.id,"result_id":run.result_id}]}),
    );
    assert_eq!(analyzed.stage, Stage::Analyze);
    let summary: Value =
        serde_json::from_slice(&std::fs::read(dir.join("results.json")).unwrap()).unwrap();
    assert_eq!(summary["round"], 2);
    assert_eq!(summary["rows"].as_array().unwrap().len(), 7);
    assert_eq!(summary["groups"]["robustness"]["mean"], 0.7);
    assert_eq!(summary["rows"][0]["reused_from_round"], 1);
    assert_eq!(summary["rows"][6]["reused_from_round"], Value::Null);
    assert_eq!(
        std::fs::read(archive.join("results.json")).unwrap(),
        old_results
    );
    assert_eq!(
        lifecycle::recovery(&root, &analyzed).unwrap()["experiment_budget"]["remaining"],
        0
    );
}

#[test]
fn followup_rejects_changed_duplicate_unknown_and_unfunded_reuse() {
    let root = fixture();
    let analyzed = record_matrix(&root);
    assert!(advance(
        &root,
        "demo",
        analyzed.revision,
        "submit_analysis",
        &json!({"artifact":"interpret.md","verdict":"inconclusive","next":"iterate"})
    )
    .unwrap_err()
    .contains("reason"));
    assert!(!topic_dir(&root, "demo").unwrap().join("report.md").exists());
    let state = request_followup(&root);
    for case in ["changed", "duplicate", "unknown", "no_new_runs"] {
        let mut input = followup_plan(&state);
        match case {
            "changed" => {
                input["full_plan"]["experiments"][0]["description"] =
                    json!("Different intervention")
            }
            "duplicate" => {
                let duplicate = input["reuse_results"][0].clone();
                input["reuse_results"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            "unknown" => input["reuse_results"][0]["result_id"] = json!("E-001-999"),
            "no_new_runs" => {
                input["full_plan"]["experiments"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            _ => {}
        }
        assert!(
            advance(&root, "demo", state.revision, "define_full", &input).is_err(),
            "{case}"
        );
        assert_eq!(
            load(&root, "demo").unwrap().unwrap().revision,
            state.revision
        );
    }
    let limited = set_run_budget(&root, "demo", state.revision, 6).unwrap();
    assert!(advance(
        &root,
        "demo",
        limited.revision,
        "define_full",
        &followup_plan(&state)
    )
    .unwrap_err()
    .contains("预算"));
    let mut changed_env = set_run_budget(&root, "demo", limited.revision, 20).unwrap();
    changed_env.compute.as_mut().unwrap().prepared_at = now_ms() + 1;
    save(&root, &changed_env).unwrap();
    assert!(advance(
        &root,
        "demo",
        changed_env.revision,
        "define_full",
        &followup_plan(&state)
    )
    .unwrap_err()
    .contains("环境准备"));
}

#[test]
fn completed_paper_can_reopen_without_losing_pdf_or_overwriting_prose() {
    let root = fixture();
    record_matrix(&root);
    let writing = super::tests::advance_ok(
        &root,
        "submit_analysis",
        json!({"artifact":"interpret.md","verdict":"supported"}),
    );
    let mut completed = paper::initialize(
        &root,
        "demo",
        writing.revision,
        &json!({"title":"First round"}),
    )
    .unwrap();
    let dir = topic_dir(&root, "demo").unwrap();
    let prose = std::fs::read(dir.join("latex/paper.tex")).unwrap();
    std::fs::write(dir.join("latex/paper.pdf"), b"%PDF-test-preserve-original").unwrap();
    std::fs::write(dir.join("delivery.json"), "{\"round\":1}").unwrap();
    let paper = completed.paper.as_mut().unwrap();
    paper.pdf = Some("latex/paper.pdf".into());
    paper.manifest = Some("delivery.json".into());
    paper.reviewed_hash = Some("old-review".into());
    completed.stage = Stage::Completed;
    save(&root, &completed).unwrap();
    let reopened = user_action(&root, "demo", completed.revision, "revise_full", None).unwrap();
    assert_eq!(reopened.stage, Stage::PlanFull);
    assert!(reopened.paper.is_none());
    assert!(reopened.runnable());
    let archive = dir.join(&reopened.full_rounds[0].artifact_root);
    assert_eq!(
        std::fs::read(archive.join("latex/paper.pdf")).unwrap(),
        b"%PDF-test-preserve-original"
    );
    assert_eq!(
        std::fs::read(archive.join("latex/paper.tex")).unwrap(),
        prose
    );
    assert!(user_action(&root, "demo", completed.revision, "revise_full", None).is_err());
    let state = super::tests::advance_ok(&root, "define_full", followup_plan(&reopened));
    let spec = state
        .full_plan
        .as_ref()
        .unwrap()
        .experiments
        .last()
        .unwrap();
    let mut run = matrix_run(&state, spec, 20);
    run.metrics_last_json = "{\"score\":1.0}".into();
    kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .unwrap()
        .upsert_research_run(&run)
        .unwrap();
    super::tests::advance_ok(
        &root,
        "record_full",
        json!({"full_results":[{"experiment_id":spec.id,"result_id":run.result_id}]}),
    );
    let writing = super::tests::advance_ok(
        &root,
        "submit_analysis",
        json!({"artifact":"interpret.md","verdict":"inconclusive"}),
    );
    let initialized = paper::initialize(&root, "demo", writing.revision, &json!({})).unwrap();
    assert_eq!(std::fs::read(dir.join("latex/paper.tex")).unwrap(), prose);
    assert!(initialized.paper.as_ref().unwrap().reviewed_hash.is_none());
    assert!(initialized.paper.as_ref().unwrap().pdf.is_none());
    assert!(std::fs::read_to_string(dir.join("latex/results-table.tex"))
        .unwrap()
        .contains("0.750000"));
    assert!(
        !std::fs::read_to_string(archive.join("latex/results-table.tex"))
            .unwrap()
            .contains("0.750000")
    );
}

#[test]
fn full_analysis_pivot_preserves_history_and_requires_user_selection() {
    let root = fixture();
    record_matrix(&root);
    let state = super::tests::advance_ok(
        &root,
        "submit_analysis",
        json!({
            "artifact":"interpret.md","verdict":"rejected","next":"pivot","reason":"The mechanism is not supported; choose a different question"
        }),
    );
    assert_eq!(state.stage, Stage::ChooseDirection);
    assert_eq!(state.full_rounds.len(), 1);
    assert!(state.selected_direction.is_none());
    assert!(state.mvp.is_none());
    assert!(!state.runnable());
    assert!(check_run(&root, "demo", "E-001").is_err());
}
