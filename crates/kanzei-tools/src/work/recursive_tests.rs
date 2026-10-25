use super::*;
use kanzei_harness::{Tool, ToolCtx};

#[tokio::test]
async fn direction_review_schedules_yield_then_replan_and_preserves_done_results() {
    let root = kanzei_base::test_support::TestDir::new("work-revision-tools-");
    let docs = DocStore::open(&root, &REQUIREMENTS);
    docs.save(&[Entry {
        id: "R-001".into(),
        title: "agent".into(),
        status: "doing".into(),
        severity: None,
        fields: vec![("执行模型".into(), "work_units_v1".into())],
    }])
    .unwrap();
    DocStore::open(&root, &DEFECTS).save(&[]).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let goal = ok(&ctx,json!({"action":"create_unit","requirement_id":"R-001","objective":"agent","acceptance":["works"],"base_revision":"fixture"})).await;
    let id = goal["unit_id"].as_str().unwrap();
    let db = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    db.append_work_fact(
        id,
        kanzei_core::WorkFact::Claimed {
            claimed_by: Some("another-line".into()),
        },
    )
    .unwrap();
    let impact = ok(&ctx, json!({"action":"impact","id":id})).await;
    let request = ok(&ctx,json!({"action":"begin_revision","id":id,"expected_versions":impact["versions"],"direction":"local first","reason":"user changes direction"})).await;
    let rejected = WorkTool.execute(json!({"action":"revision_yield","id":id,"revision_id":request["revision_id"],"reason":"cannot acknowledge another executor"}), &ctx).await;
    assert!(rejected.is_error);
    assert_eq!(
        resolve_work_decision(&root, &root, ctx.work_priority)
            .unwrap()
            .decision,
        WorkDecision::Blocked
    );
    // The real owner acknowledges its stop; the planning tool never impersonates it.
    db.append_work_fact(
        id,
        kanzei_core::WorkFact::RevisionYielded {
            revision_id: request["revision_id"].as_str().unwrap().into(),
            reason: "owner has stopped work".into(),
        },
    )
    .unwrap();
    let selected = selection(&ctx);
    assert_eq!(
        selected.work_unit_context.unwrap().revision_action,
        Some(WorkRevisionAction::Resolve)
    );
    let policy = context::resolved_control_prompt(&root, &root, ctx.work_priority);
    assert!(policy.contains("apply_revision"));
    assert!(!policy.contains("work claim"));
    assert!(!policy.contains("work verify"));
    assert!(!policy.contains("work refine"));
    let stale = WorkTool.execute(json!({"action":"apply_revision","id":id,"revision_id":request["revision_id"],"expected_versions":impact["versions"],"revision_decisions":[{"unit_id":id,"disposition":"retain","reason":"still valid"}]}),&ctx).await;
    assert!(stale.is_error);
    let impact = ok(&ctx, json!({"action":"impact","id":id})).await;
    ok(&ctx,json!({"action":"apply_revision","id":id,"revision_id":request["revision_id"],"expected_versions":impact["versions"],"revision_decisions":[{"unit_id":id,"disposition":"retain","reason":"still valid"}]})).await;
    db.append_work_fact(id, kanzei_core::WorkFact::Claimed { claimed_by: None })
        .unwrap();
    let impact = ok(&ctx, json!({"action":"impact","id":id})).await;
    let request = ok(&ctx,json!({"action":"begin_revision","id":id,"expected_versions":impact["versions"],"direction":"local first with offline support","reason":"refine direction"})).await;
    let yielding = selection(&ctx);
    assert_eq!(
        yielding.work_unit_context.as_ref().unwrap().revision_action,
        Some(WorkRevisionAction::Yield)
    );
    let guidance = context::conditional_guidance(
        &resolve_work_selection(&ctx.cwd, &ctx.project_root, ctx.work_priority).unwrap(),
    );
    assert!(guidance.contains("work revision_yield"));
    assert!(!guidance.contains("work claim"));
    assert!(!guidance.contains("work verify"));
    let claim = WorkTool
        .execute(json!({"action":"claim","id":id}), &ctx)
        .await;
    assert_eq!(claim.code.as_deref(), Some("WORK_REVIEW_PENDING"));
    ok(&ctx,json!({"action":"revision_yield","id":id,"revision_id":request["revision_id"],"reason":"stopped at checkpoint"})).await;
    let impact = ok(&ctx, json!({"action":"impact","id":id})).await;
    ok(&ctx,json!({"action":"apply_revision","id":id,"revision_id":request["revision_id"],"expected_versions":impact["versions"],"revision_decisions":[{"unit_id":id,"disposition":"retain","reason":"offline already covered"}]})).await;
    for fact in [
        kanzei_core::WorkFact::Claimed { claimed_by: None },
        kanzei_core::WorkFact::VerificationStarted,
        kanzei_core::WorkFact::EvidenceAdded {
            evidence: kanzei_core::WorkEvidence {
                criterion: "works".into(),
                evidence_refs: vec!["receipt".into()],
            },
        },
        kanzei_core::WorkFact::Completed,
    ] {
        db.append_work_fact(id, fact).unwrap();
    }
    let impact = ok(&ctx, json!({"action":"impact","id":id})).await;
    ok(&ctx,json!({"action":"begin_revision","id":id,"expected_versions":impact["versions"],"direction":"reassess finished work","reason":"new constraint"})).await;
    let selected = selection(&ctx);
    assert_eq!(selected.id, id);
    assert_eq!(selected.outcome_action, None);
    assert_eq!(
        selected.work_unit_context.unwrap().revision_action,
        Some(WorkRevisionAction::Resolve)
    );
    let result = crate::tracker::TrackerTool {
        tool_name: "req",
        noun: "需求",
        kind: &REQUIREMENTS,
        requires_refs: None,
    }
    .execute(json!({"action":"close","id":"R-001"}), &ctx)
    .await;
    assert!(result.is_error);
    assert!(
        result.content.contains("待复核 Work Unit"),
        "{}",
        result.content
    );
    assert_eq!(db.get_work_unit(id).unwrap().unwrap().evidence.len(), 1);
}

#[tokio::test]
async fn revision_checks_every_affected_outcome_before_freezing_any_node() {
    let root = kanzei_base::test_support::TestDir::new("work-cross-outcome-review-");
    let docs = DocStore::open(&root, &REQUIREMENTS);
    let mut entries = ["R-001", "R-002"]
        .into_iter()
        .map(|id| Entry {
            id: id.into(),
            title: id.into(),
            status: "todo".into(),
            severity: None,
            fields: vec![("执行模型".into(), "work_units_v1".into())],
        })
        .collect::<Vec<_>>();
    docs.save(&entries).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    ok(&ctx,json!({"action":"create_unit","requirement_id":"R-001","objective":"producer","acceptance":["works"],"base_revision":"fixture"})).await;
    ok(&ctx,json!({"action":"create_unit","requirement_id":"R-002","objective":"consumer","dependencies":["R-001/W1"],"base_revision":"fixture"})).await;
    entries[1].status = "blocked".into();
    docs.save(&entries).unwrap();
    let impact = ok(&ctx, json!({"action":"impact","id":"R-001/W1"})).await;
    let result = WorkTool.execute(json!({"action":"begin_revision","id":"R-001/W1","expected_versions":impact["versions"],"direction":"new direction","reason":"scope changed"}),&ctx).await;
    assert!(result.is_error);
    assert!(result.content.contains("R-002"));
    let db = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    assert!(db
        .list_work_units(None)
        .unwrap()
        .iter()
        .all(|node| !node.review_pending()));
}

#[tokio::test]
async fn draft_goal_uses_system_identity_and_requires_criteria_before_execution() {
    let root = kanzei_base::test_support::TestDir::new("work-draft-tools-");
    for args in [
        vec!["init", "--quiet"],
        vec![
            "-c",
            "user.name=Kanzei Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "source baseline",
        ],
    ] {
        let output = std::process::Command::new("git")
            .current_dir(&root)
            .args(&args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let docs = DocStore::open(&root, &REQUIREMENTS);
    docs.save(&[Entry {
        id: "R-001".into(),
        title: "personal agent".into(),
        status: "todo".into(),
        severity: None,
        fields: vec![
            ("执行模型".into(), "work_units_v1".into()),
            ("目标".into(), "personal agent".into()),
        ],
    }])
    .unwrap();
    DocStore::open(&root, &DEFECTS).save(&[]).unwrap();
    let ctx =
        ToolCtx::new(root.clone(), root.clone()).with_work_priority(WorkPriority::RequirementFirst);
    let goal = ok(&ctx, json!({"action":"create_unit","requirement_id":"R-001","objective":"personal agent","node":{"kind":"goal"}})).await;
    assert_eq!(goal["acceptance"], json!([]));
    assert!(!goal["base_revision"].as_str().unwrap().is_empty());
    let planning = selection(&ctx);
    assert_eq!(
        planning.work_unit_context.as_ref().unwrap().readiness.step,
        kanzei_core::WorkNodeStep::Plan
    );
    let guidance = context::conditional_guidance(
        &resolve_work_selection(&ctx.cwd, &ctx.project_root, ctx.work_priority).unwrap(),
    );
    assert!(guidance.contains("work refine"));
    assert!(!guidance.contains("work claim"));
    assert!(!guidance.contains("work verify"));
    let rejected = WorkTool
        .execute(json!({"action":"claim","id":goal["unit_id"]}), &ctx)
        .await;
    assert!(rejected.is_error);
    assert!(
        rejected.content.contains("完成条件"),
        "{}",
        rejected.content
    );
    assert_eq!(docs.load().unwrap()[0].status, "todo");
    let full = ok(&ctx, json!({"action":"get_unit","id":goal["unit_id"]})).await;
    assert_eq!(full["unit"]["source_sequence"], goal["source_sequence"]);
    let mut contract = full["contract"].clone();
    contract["acceptance"] = json!(["works"]);
    let ready = ok(&ctx, json!({"action":"refine","id":goal["unit_id"],"expected_sequence":goal["source_sequence"],"reason":"define observable result","refinement":contract})).await;
    let split = ok(&ctx, json!({"action":"decompose","id":goal["unit_id"],"expected_sequence":ready["source_sequence"],"reason":"identify missing research","children":[{"key":"research","objective":"compare memory options","node":{"kind":"research","covers":["works"]}}]})).await;
    assert_eq!(split["children"][0]["acceptance"], json!([]));
    assert_eq!(
        selection(&ctx).work_unit_context.unwrap().readiness.step,
        kanzei_core::WorkNodeStep::Plan
    );
}

#[tokio::test]
async fn planning_is_available_before_prior_art_but_type_names_cannot_bypass_execution() {
    let root = kanzei_base::test_support::TestDir::new("work-plan-gates-");
    let docs = DocStore::open(&root, &REQUIREMENTS);
    docs.save(&[Entry {
        id: "R-001".into(),
        title: "goal".into(),
        status: "todo".into(),
        severity: None,
        fields: vec![
            ("执行模型".into(), "work_units_v1".into()),
            ("目标".into(), "goal".into()),
            ("先行调研".into(), "missing-prior-art.md".into()),
        ],
    }])
    .unwrap();
    let ctx = ToolCtx::new(root.to_path_buf(), root.to_path_buf());
    let goal = ok(&ctx, json!({"action":"create_unit","requirement_id":"R-001","objective":"goal","acceptance":["works"],"base_revision":"fixture"})).await;
    let split = ok(&ctx, json!({"action":"decompose","id":goal["unit_id"],"expected_sequence":goal["source_sequence"],"reason":"resolve prerequisites without starting implementation","children":[{
        "key":"research","objective":"resolve prior art","acceptance":["report"],"node":{"kind":"research","covers":["works"],"custom_type":{"name":"性能分析","description":"measure","deliverable":"report"}}
    }]})).await;
    let claim = WorkTool
        .execute(
            json!({"action":"claim","id":split["ids"]["research"]}),
            &ctx,
        )
        .await;
    assert_eq!(claim.code.as_deref(), Some("PRIOR_ART_REQUIRED"));
    let mut entries = docs.load().unwrap();
    entries[0].status = "blocked".into();
    docs.save(&entries).unwrap();
    let node = split["children"][0].clone();
    let rejected = WorkTool.execute(json!({"action":"decompose","id":node["unit_id"],"expected_sequence":node["source_sequence"],"reason":"should reject","children":[{"key":"x","objective":"x","acceptance":["x"],"node":{"covers":["report"]}}]}), &ctx).await;
    assert_eq!(rejected.code.as_deref(), Some("WORK_OUTCOME_NOT_PLANNABLE"));
}

async fn ok(ctx: &ToolCtx, input: serde_json::Value) -> serde_json::Value {
    let output = WorkTool.execute(input, ctx).await;
    assert!(!output.is_error, "{}", output.content);
    serde_json::from_str(&output.content).unwrap()
}

fn selection(ctx: &ToolCtx) -> WorkItem {
    resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority)
        .unwrap()
        .selected
        .expect("an actionable node")
}

async fn complete(ctx: &ToolCtx, id: &str) {
    ok(ctx, json!({"action":"claim","id":id})).await;
    ok(ctx, json!({"action":"verify","id":id})).await;
    ok(
        ctx,
        json!({"action":"evidence","id":id,"criterion":"works","evidence_refs":["proof.txt"]}),
    )
    .await;
    ok(ctx, json!({"action":"complete","id":id})).await;
}

#[tokio::test]
async fn recursive_tool_pipeline_selects_frontier_and_returns_to_parent_acceptance() {
    let root = kanzei_base::test_support::TestDir::new("recursive-work-tools-");
    DocStore::open(&root, &REQUIREMENTS)
        .save(&[Entry {
            id: "R-001".into(),
            title: "personal agent".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![
                ("执行模型".into(), "work_units_v1".into()),
                ("目标".into(), "deliver personal agent".into()),
            ],
        }])
        .unwrap();
    DocStore::open(&root, &DEFECTS).save(&[]).unwrap();
    std::fs::write(
        root.join("proof.txt"),
        "fixture experiment and integration receipt",
    )
    .unwrap();
    let ctx =
        ToolCtx::new(root.clone(), root.clone()).with_work_priority(WorkPriority::RequirementFirst);
    let goal = ok(&ctx, json!({"action":"create_unit", "requirement_id":"R-001", "objective":"personal agent", "acceptance":["works"], "base_revision":"fixture", "node":{"kind":"goal","questions":["which harness?"]}})).await;
    assert_eq!(
        selection(&ctx).work_unit_context.unwrap().readiness.step,
        kanzei_core::WorkNodeStep::Plan
    );
    let claimed = ok(&ctx, json!({"action":"claim","id":goal["unit_id"]})).await;
    let split = ok(&ctx, json!({"action":"decompose", "id":goal["unit_id"],"expected_sequence":claimed["source_sequence"],"reason":"learn before choosing implementation", "children":[
        {"key":"research","objective":"compare harness choices","acceptance":["works"],"node":{"kind":"research","covers":["works"]}},
        {"key":"implementation","objective":"implement selected harness","dependencies":["@research"],"acceptance":["works"],"node":{"kind":"requirement","covers":["works"]}}
    ]})).await;
    assert_eq!(split["ids"]["research"], "R-001/W2");
    assert_eq!(selection(&ctx).id, "R-001/W2");
    let research = ok(&ctx, json!({"action":"claim","id":"R-001/W2"})).await;
    ok(&ctx, json!({"action":"decompose","id":"R-001/W2","expected_sequence":research["source_sequence"],"reason":"need a documented example","children":[
        {"key":"document","objective":"document one model-tool execution","acceptance":["works"],"node":{"kind":"documentation","covers":["works"]}}
    ]})).await;
    let frontier = selection(&ctx);
    assert_eq!(frontier.id, "R-001/W4");
    assert_eq!(
        frontier
            .work_unit_context
            .unwrap()
            .ancestors
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        vec!["R-001/W2", "R-001/W1"]
    );
    complete(&ctx, "R-001/W4").await;
    let research = selection(&ctx);
    assert_eq!(research.id, "R-001/W2");
    assert_eq!(
        research.work_unit_context.unwrap().readiness.step,
        kanzei_core::WorkNodeStep::Integrate
    );
    complete(&ctx, "R-001/W2").await;
    let implementation = selection(&ctx).work_unit_context.unwrap();
    assert_eq!(implementation.unit.unit_id, "R-001/W3");
    assert_eq!(implementation.ancestors[0].acceptance, vec!["works"]);
    assert_eq!(implementation.required_results[0].id, "R-001/W2");
    assert_eq!(
        implementation.required_results[0].evidence[0].evidence_refs,
        vec!["proof.txt"]
    );
    complete(&ctx, "R-001/W3").await;
    let parent = selection(&ctx);
    assert_eq!(parent.id, "R-001/W1");
    assert_eq!(
        parent.work_unit_context.unwrap().readiness.step,
        kanzei_core::WorkNodeStep::Plan
    );
    let current = ok(&ctx, json!({"action":"get_unit","id":"R-001/W1"})).await;
    let mut contract = current["contract"].clone();
    contract["node"]["questions"] = json!([]);
    contract["node"]["explanation"] = json!("research and executable example justify the design");
    ok(&ctx, json!({"action":"refine","id":"R-001/W1","expected_sequence":current["unit"]["source_sequence"],"reason":"resolved by research","refinement":contract})).await;
    complete(&ctx, "R-001/W1").await;
    let graph = ok(&ctx, json!({"action":"graph","requirement_id":"R-001"})).await;
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 4);
    assert!(graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node["unit"]["status"] == "done"));
    let control = resolve_work_decision(&root, &root, ctx.work_priority).unwrap();
    assert_ne!(
        control.decision,
        WorkDecision::Empty,
        "unit completion cannot substitute for Outcome acceptance"
    );
}

#[tokio::test]
async fn work_evidence_must_exist_when_added_and_when_completed() {
    let root = kanzei_base::test_support::TestDir::new("recursive-evidence-");
    let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    store
        .create_work_unit(kanzei_core::WorkUnitSpec {
            unit_id: "R-1/W1".into(),
            requirement_id: "R-1".into(),
            objective: "verify evidence".into(),
            scope: vec![],
            dependencies: vec![],
            acceptance: vec!["works".into()],
            verification: vec![],
            base_revision: "fixture".into(),
            node: Default::default(),
        })
        .unwrap();
    store
        .append_work_facts(
            "R-1/W1",
            vec![
                kanzei_core::WorkFact::Claimed { claimed_by: None },
                kanzei_core::WorkFact::VerificationStarted,
            ],
        )
        .unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let input = json!({"action":"evidence","id":"R-1/W1","criterion":"works","evidence_refs":["proof.txt"]});
    let rejected = WorkTool.execute(input.clone(), &ctx).await;
    assert_eq!(rejected.code.as_deref(), Some("WORK_EVIDENCE_UNAVAILABLE"));
    std::fs::write(root.join("proof.txt"), "observed result").unwrap();
    ok(&ctx, input).await;
    std::fs::remove_file(root.join("proof.txt")).unwrap();
    let rejected = WorkTool
        .execute(json!({"action":"complete","id":"R-1/W1"}), &ctx)
        .await;
    assert_eq!(rejected.code.as_deref(), Some("WORK_EVIDENCE_UNAVAILABLE"));
    assert_eq!(
        store.get_work_unit("R-1/W1").unwrap().unwrap().status,
        WorkUnitStatus::Verifying
    );
}

#[tokio::test]
async fn automatic_loop_observes_graph_frontier_and_substantive_progress() {
    use kanzei_harness::auto_run::BacklogStatus;
    let root = kanzei_base::test_support::TestDir::new("recursive-auto-");
    DocStore::open(&root, &REQUIREMENTS)
        .save(&[Entry {
            id: "R-001".into(),
            title: "goal".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![("执行模型".into(), "work_units_v1".into())],
        }])
        .unwrap();
    DocStore::open(&root, &DEFECTS).save(&[]).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    assert_eq!(
        crate::tracker::backlog_status(&root),
        BacklogStatus::Workable
    );
    let initial = progress_snapshot(&root).unwrap();
    let goal = ok(&ctx, json!({"action":"create_unit", "requirement_id":"R-001", "objective":"goal", "acceptance":["works"], "base_revision":"fixture"})).await;
    let created = progress_snapshot(&root).unwrap();
    assert_ne!(initial, created);
    let claimed = ok(&ctx, json!({"action":"claim","id":goal["unit_id"]})).await;
    assert_eq!(
        created,
        progress_snapshot(&root).unwrap(),
        "claim is not a result"
    );
    ok(&ctx, json!({"action":"decompose","id":goal["unit_id"],"expected_sequence":claimed["source_sequence"],"reason":"two independent results","children":[
        {"key":"first","objective":"first","acceptance":["works"],"node":{"covers":["works"]}},
        {"key":"second","objective":"second","acceptance":["works"],"node":{"covers":["works"]}}
    ]})).await;
    let decomposed = progress_snapshot(&root).unwrap();
    assert_ne!(created, decomposed);
    let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    store
        .append_work_facts(
            "R-001/W2",
            vec![kanzei_core::WorkFact::Blocked {
                reason: "external prerequisite".into(),
            }],
        )
        .unwrap();
    assert_eq!(
        crate::tracker::backlog_status(&root),
        BacklogStatus::Workable
    );
    assert_eq!(selection(&ctx).id, "R-001/W3");
    store
        .append_work_facts(
            "R-001/W3",
            vec![kanzei_core::WorkFact::Blocked {
                reason: "external prerequisite".into(),
            }],
        )
        .unwrap();
    assert_eq!(
        crate::tracker::backlog_status(&root),
        BacklogStatus::AllBlocked
    );
    assert_eq!(
        decomposed,
        progress_snapshot(&root).unwrap(),
        "blocking alone creates no result"
    );
    store
        .append_work_facts(
            "R-001/W3",
            vec![
                kanzei_core::WorkFact::Unblocked {
                    reason: "prerequisite supplied".into(),
                },
                kanzei_core::WorkFact::Claimed { claimed_by: None },
                kanzei_core::WorkFact::Checkpointed {
                    checkpoint: kanzei_core::WorkCheckpoint {
                        summary: "still working".into(),
                        next_action: "continue".into(),
                        decisions: vec![],
                        retrieval_refs: vec![],
                        observed_head: "fixture".into(),
                        observed_worktree_hash: "fixture".into(),
                    },
                },
            ],
        )
        .unwrap();
    assert_eq!(
        decomposed,
        progress_snapshot(&root).unwrap(),
        "checkpoint prose is not new evidence"
    );
    assert_eq!(
        crate::tracker::backlog_status(&root),
        BacklogStatus::Workable
    );
    std::fs::write(root.join("proof.txt"), "result").unwrap();
    ok(&ctx, json!({"action":"verify","id":"R-001/W3"})).await;
    ok(&ctx, json!({"action":"evidence","id":"R-001/W3","criterion":"works","evidence_refs":["proof.txt"]})).await;
    assert_ne!(decomposed, progress_snapshot(&root).unwrap());
}

#[tokio::test]
async fn compact_context_reports_omitted_criteria_and_full_contract_remains_available() {
    let root = kanzei_base::test_support::TestDir::new("recursive-context-");
    DocStore::open(&root, &REQUIREMENTS)
        .save(&[Entry {
            id: "R-001".into(),
            title: "goal".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![("执行模型".into(), "work_units_v1".into())],
        }])
        .unwrap();
    DocStore::open(&root, &DEFECTS).save(&[]).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let criteria = (1..=13)
        .map(|n| format!("criterion {n}"))
        .collect::<Vec<_>>();
    ok(&ctx, json!({"action":"create_unit","requirement_id":"R-001","objective":"bounded context","base_revision":"fixture","acceptance":criteria})).await;
    let compact = ok(&ctx, json!({"action":"next"})).await;
    assert!(compact["context_omissions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["original_items"] == 13
            && item["path"].as_str().unwrap().ends_with("acceptance")));
    assert!(compact["full_contract"]
        .as_str()
        .unwrap()
        .contains("get_unit"));
    let full = ok(&ctx, json!({"action":"get_unit","id":"R-001/W1"})).await;
    assert_eq!(full["contract"]["acceptance"], json!(criteria));
}
