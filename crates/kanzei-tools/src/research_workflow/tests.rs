use super::*;
use kanzei_harness::{Tool, ToolCtx};

pub(super) fn root() -> PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kz-auto-research-{}-{id}", std::process::id()));
    let dir = root.join(".kanzei/research/demo");
    std::fs::create_dir_all(dir.join("explorations")).unwrap();
    for file in [
        "survey.md",
        "map.md",
        "protocol.md",
        "prepare.md",
        "interpret.md",
    ] {
        std::fs::write(
            dir.join(file),
            "# Evidence\nA reviewable artifact with references.\n",
        )
        .unwrap();
    }
    std::fs::write(
        dir.join("sources.md"),
        "# Sources\n\n## S-001 [active] Source\n- URL: https://example.org/paper\n- 年份: 2026\n",
    )
    .unwrap();
    std::fs::write(dir.join("explorations/E-001.md"), "---\nkind: exploration\nid: E-001\ntopic: demo\ntitle: test\nstatus: running\nhypothesis: test\ndepends_on:\nsupersedes:\nentry_refs:\nenvironment: ENV-test\nbudget: 1 gpu-hour\ncreated_at: 1\nupdated_at: 1\n---\n\n## 假设\ntest\n\n## 实验结果\n| 实验 | 参数 | 状态 | 关键指标 | 产物 | 结论 |\n| --- | --- | --- | --- | --- | --- |\n\n## 结论\n待定\n\n## 后续\n等待验证\n").unwrap();
    root
}

pub(super) fn advance_ok(root: &Path, action: &str, input: Value) -> Workflow {
    let revision = load(root, "demo").unwrap().unwrap().revision;
    advance(root, "demo", revision, action, &input).unwrap()
}

fn candidates() -> Value {
    json!({"artifact":"map.md", "directions":[{"id":"staleness", "title":"Memory staleness", "question":"Does stale memory hurt?",
        "rationale":"Prior work has limited controls", "uncertainty":"Needs direct measurement", "cost":"Two local runs", "validation":"Matched baseline",
        "source_ids":["S-001"]}]})
}

pub(super) fn through_selection(root: &Path) -> Workflow {
    start(root, "demo", PlanBudget::default(), 4).unwrap();
    // Preserve the original MVP-only project contract while new v2 cases cover paper delivery.
    let mut legacy = load(root, "demo").unwrap().unwrap();
    legacy.version = 1;
    save(root, &legacy).unwrap();
    advance_ok(root, "survey_complete", json!({"artifact":"survey.md"}));
    let map = advance_ok(root, "publish_map", candidates());
    assert!(!map.runnable());
    user_action(root, "demo", map.revision, "select", Some("staleness")).unwrap()
}

pub(super) fn mvp() -> Value {
    json!({"mvp":{"question":"Does stale memory hurt?", "hypothesis":"Removing stale items improves score", "baseline":"Unfiltered memory",
        "metric":"score", "success":"score improves at fixed budget", "failure":"no improvement", "exploration_id":"E-001", "protocol":"protocol.md"}})
}

#[test]
fn workflow_tool_schema_resolves_every_local_reference() {
    fn check(value: &Value, root: &Value) {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                    if let Some(pointer) = reference.strip_prefix('#') {
                        assert!(
                            root.pointer(pointer).is_some(),
                            "unresolved schema reference: {reference}"
                        );
                    }
                }
                for nested in object.values() {
                    check(nested, root);
                }
            }
            Value::Array(array) => {
                for nested in array {
                    check(nested, root);
                }
            }
            _ => {}
        }
    }
    let schema = ResearchWorkflowTool::default().input_schema();
    check(&schema, &schema);
}

#[test]
fn exploration_authoring_template_and_diagnostics_support_mvp_recovery() {
    let root = root();
    let state = through_selection(&root);
    let path = topic_dir(&root, "demo")
        .unwrap()
        .join("explorations/E-001.md");
    let template = exploration_template("demo");
    let parsed = kanzei_core::parse_exploration_markdown(&path, &template);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert!(state.guidance().contains("explorations/E-001.md"));
    std::fs::write(&path, "# Missing frontmatter").unwrap();
    let missing = advance(&root, "demo", state.revision, "define_mvp", &mvp()).unwrap_err();
    assert!(missing.contains("kind: exploration"));
    std::fs::write(&path, template.replace("## 后续", "## Next steps")).unwrap();
    let invalid = advance(&root, "demo", state.revision, "define_mvp", &mvp()).unwrap_err();
    assert!(invalid.contains("后续"));
    assert_eq!(
        load(&root, "demo").unwrap().unwrap().revision,
        state.revision
    );
    std::fs::write(&path, template).unwrap();
    assert_eq!(advance_ok(&root, "define_mvp", mvp()).stage, Stage::Prepare);
}

#[test]
fn persists_user_selection_and_rejects_stale_or_skipped_steps() {
    let root = root();
    let initial = start(&root, "demo", PlanBudget::default(), 4).unwrap();
    assert!(advance(
        &root,
        "demo",
        initial.revision,
        "publish_map",
        &candidates()
    )
    .is_err());
    let map = advance_ok(&root, "survey_complete", json!({"artifact":"survey.md"}));
    assert!(advance(
        &root,
        "demo",
        initial.revision,
        "publish_map",
        &candidates()
    )
    .is_err());
    assert_eq!(load(&root, "demo").unwrap().unwrap().revision, map.revision);
    let state = advance_ok(&root, "publish_map", candidates());
    assert!(advance(
        &root,
        "demo",
        state.revision,
        "select",
        &json!({"direction":"staleness"})
    )
    .is_err());
    assert!(advance(&root, "demo", state.revision, "define_mvp", &mvp()).is_err());
    assert!(check_run(&root, "demo", "E-001").is_err());
    let selected = user_action(&root, "demo", state.revision, "select", Some("staleness")).unwrap();
    assert_eq!(selected.stage, Stage::DesignMvp);
    let paused = user_action(&root, "demo", selected.revision, "pause", None).unwrap();
    assert!(!load(&root, "demo").unwrap().unwrap().runnable());
    assert!(advance(&root, "demo", paused.revision, "define_mvp", &mvp()).is_err());
    let resumed = user_action(&root, "demo", paused.revision, "resume", None).unwrap();
    assert!(resumed.runnable());
    assert_eq!(resumed.selected_direction.as_deref(), Some("staleness"));
}

#[test]
fn rejects_missing_unsafe_artifacts_and_unregistered_sources() {
    let root = root();
    let state = start(&root, "demo", PlanBudget::default(), 4).unwrap();
    for file in ["missing.md", "../demo/survey.md", "C:/outside.md", ""] {
        assert!(advance(
            &root,
            "demo",
            state.revision,
            "survey_complete",
            &json!({"artifact":file})
        )
        .is_err());
    }
    advance_ok(&root, "survey_complete", json!({"artifact":"survey.md"}));
    let mut map = candidates();
    map["directions"][0]["source_ids"] = json!(["S-999"]);
    assert!(advance(&root, "demo", 2, "publish_map", &map).is_err());
    assert_eq!(load(&root, "demo").unwrap().unwrap().stage, Stage::Map);
}

#[test]
fn concurrent_user_actions_cannot_overwrite_each_other() {
    let root = root();
    let state = through_selection(&root);
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || user_action(&root, "demo", state.revision, "pause", None))
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        load(&root, "demo").unwrap().unwrap().revision,
        state.revision + 1
    );
}

#[tokio::test]
async fn user_start_authorizes_search_but_agent_cannot_choose_or_cross_topics() {
    let root = root();
    start(&root, "demo", PlanBudget::default(), 4).unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let loop_result = crate::research_loop::ResearchLoopTool
        .execute(json!({"action":"start", "topic":"demo"}), &ctx)
        .await;
    assert!(!loop_result.is_error, "{}", loop_result.content);
    let budget_reset = crate::research_verify::ResearchVerifyTool.execute(json!({"action":"budget_set", "topic":"demo", "max_rounds":999, "max_tokens":999999, "max_concurrency":10}), &ctx).await;
    assert!(budget_reset.is_error);
    let tool = ResearchWorkflowTool {
        topic: Some("demo".into()),
    };
    assert!(
        tool.execute(json!({"action":"get", "topic":"other"}), &ctx)
            .await
            .is_error
    );
    assert!(
        tool.execute(
            json!({"action":"select", "topic":"demo", "revision":1}),
            &ctx
        )
        .await
        .is_error
    );
    assert!(!tool.input_schema()["properties"]["action"]["enum"]
        .as_array()
        .unwrap()
        .contains(&json!("select")));
}

async fn real_run(root: &Path, id: &str, command: &str) -> kanzei_harness::ToolOutput {
    crate::research_runner::ResearchRunnerTool
        .execute(
            json!({
                "action":"run", "topic":"demo", "exploration_id":"E-001", "result_id":id,
                "execution":{"kind":"local", "command":command, "workdir":root.to_string_lossy()},
                "max_duration_ms":10000, "policy":"relaxed"
            }),
            &ToolCtx::new(root.into(), root.into()),
        )
        .await
}

#[tokio::test]
async fn completes_mvp_using_real_local_runner_and_preserves_failure_interpretation() {
    let root = root();
    through_selection(&root);
    advance_ok(&root, "define_mvp", mvp());
    let callback = r#"@@kanzei {"t":"metric","ts":1,"name":"score","value":0.4}"#;
    std::fs::write(root.join("callback.txt"), format!("{callback}\n")).unwrap();
    let command = if cfg!(windows) {
        "type callback.txt"
    } else {
        "cat callback.txt"
    };
    let baseline = real_run(&root, "E-001-01", command).await;
    assert!(!baseline.is_error, "{}", baseline.content);
    let duplicate = real_run(&root, "E-001-01", "exit 2").await;
    assert!(duplicate.is_error, "不能覆盖既有实验记录");
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    let mut foreign = store.get_research_run("E-001-01").unwrap().unwrap();
    assert_eq!(foreign.status, "succeeded");
    foreign.result_id = "foreign-result".into();
    foreign.topic = "other".into();
    store.upsert_research_run(&foreign).unwrap();
    let revision = load(&root, "demo").unwrap().unwrap().revision;
    assert!(advance(
        &root,
        "demo",
        revision,
        "environment_ready",
        &json!({"artifact":"prepare.md", "baseline_result":"foreign-result"})
    )
    .is_err());
    advance_ok(
        &root,
        "environment_ready",
        json!({"artifact":"prepare.md", "baseline_result":"E-001-01"}),
    );
    let failed = real_run(&root, "E-001-02", "exit 2").await;
    assert!(failed.is_error);
    let state = advance_ok(&root, "record_mvp", json!({"result_ids":["E-001-02"]}));
    assert!(advance(
        &root,
        "demo",
        state.revision,
        "interpret",
        &json!({"artifact":"interpret.md", "verdict":"supported", "next":"complete"})
    )
    .is_err());
    let state = advance_ok(
        &root,
        "interpret",
        json!({"artifact":"interpret.md", "verdict":"inconclusive", "next":"iterate"}),
    );
    assert_eq!(state.stage, Stage::DesignMvp);
    advance_ok(&root, "define_mvp", mvp());
    advance_ok(
        &root,
        "environment_ready",
        json!({"artifact":"prepare.md", "baseline_result":"E-001-01"}),
    );
    let passed = real_run(&root, "E-001-03", command).await;
    assert!(!passed.is_error, "{}", passed.content);
    advance_ok(&root, "record_mvp", json!({"result_ids":["E-001-03"]}));
    let complete = advance_ok(
        &root,
        "interpret",
        json!({"artifact":"interpret.md", "verdict":"supported", "next":"complete"}),
    );
    assert_eq!(complete.stage, Stage::Completed);
    assert!(!complete.runnable());
    assert!(check_run(&root, "demo", "E-001").is_err());
    let recovered = load(&root, "demo").unwrap().unwrap();
    assert_eq!(recovered.result_ids, vec!["E-001-03"]);
    assert!(recovered
        .history
        .iter()
        .any(|h| h["previous"]["verdict"] == "inconclusive"));
}

#[tokio::test]
async fn runner_budget_counts_failed_runs_and_rejects_another_exploration() {
    let root = root();
    through_selection(&root);
    advance_ok(&root, "define_mvp", mvp());
    assert!(check_run(&root, "demo", "E-002").is_err());
    assert!(
        real_run(&root, "../bad", "echo must-not-run")
            .await
            .is_error
    );
    assert!(
        real_run(&root, "E-001-1234567890", "echo must-not-run")
            .await
            .is_error
    );
    for id in ["E-001-01", "E-001-02", "E-001-03", "E-001-04"] {
        let _ = real_run(&root, id, "exit 2").await;
    }
    assert!(check_run(&root, "demo", "E-001")
        .unwrap_err()
        .contains("预算"));
    assert!(
        real_run(&root, "E-001-05", "echo must-not-run")
            .await
            .is_error
    );
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    assert!(store.get_research_run("E-001-05").unwrap().is_none());
    let state = load(&root, "demo").unwrap().unwrap();
    assert!(advance(
        &root,
        "demo",
        state.revision,
        "budget",
        &json!({"max_mvp_runs":10})
    )
    .is_err());
    set_run_budget(&root, "demo", state.revision, 5).unwrap();
    assert!(check_run(&root, "demo", "E-001").is_ok());
}
