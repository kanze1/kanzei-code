use super::*;
use std::sync::atomic::Ordering;

struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "kz-workbench-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(project_root(&root.display().to_string()))
    }
    fn path(&self) -> String {
        self.0.display().to_string()
    }
    fn tracker(&self, text: &str) {
        std::fs::create_dir_all(self.0.join(".kanzei/project")).unwrap();
        std::fs::write(self.0.join(REQUIREMENTS.rel_path), text).unwrap();
    }
    fn prefs(&self) -> AppPrefs {
        AppPrefs {
            projects: vec![self.path()],
            current: Some(self.path()),
            ..Default::default()
        }
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn overview_and_details_do_not_initialize_empty_projects_or_runtime() {
    let root = Project::new();
    let state = AppState::default();
    let prefs = root.prefs();
    let first = overview(&state, &prefs);
    let second = overview(&state, &prefs);
    assert_eq!(
        first["projects"][0]["content_revision"],
        second["projects"][0]["content_revision"]
    );
    assert_eq!(first["projects"][0]["freshness"], "fresh");
    assert!(details(&state, &prefs, None)["projects"][0]["decisions"]
        .as_array()
        .unwrap()
        .is_empty());
    recover_project(&state, &root.0).unwrap();
    assert!(!root.0.join(".kanzei").exists());
    assert!(state.processes.lock().unwrap().is_empty());
    assert!(state.runtimes.lock().unwrap().is_empty());
}

#[test]
fn same_ids_stay_project_scoped_and_unknown_batches_have_no_denominator() {
    let reader = Project::new();
    let other = Project::new();
    reader.tracker("## R-001 Reader [doing]\n- 批次: 2/5\n- 优先级: P1\n");
    other.tracker("## R-001 Other [doing]\n- 批次: 1/3\n\n## R-002 Unplanned [doing]\n");
    let prefs = AppPrefs {
        projects: vec![reader.path(), other.path()],
        ..Default::default()
    };
    let state = AppState::default();
    let snapshot = overview(&state, &prefs);
    let a = &snapshot["projects"][0]["current_items"][0];
    let b = &snapshot["projects"][1]["current_items"][0];
    assert_eq!(a["id"], b["id"]);
    assert_ne!(a["project_id"], b["project_id"]);
    assert_eq!(
        a["batches"],
        json!({"done":2,"total":5,"source":"markdown"})
    );
    assert_eq!(b["batches"]["total"], 3);
    assert!(snapshot["projects"][1]["current_items"][1]["batches"]["total"].is_null());
    let selected = details(&state, &prefs, Some(&other.path()));
    assert_eq!(selected["projects"].as_array().unwrap().len(), 1);
    assert_eq!(selected["projects"][0]["path"], other.path());
    assert!(!reader.0.join(".kanzei/state.db").exists());
    assert!(!reader
        .0
        .join(".kanzei/project/requirements.md.lock")
        .exists());
}

#[test]
fn empty_child_does_not_read_its_parents_project() {
    let parent = Project::new();
    parent.tracker("## R-001 Parent [doing]\n- 批次: 2/5\n");
    let child = parent.0.join("child");
    std::fs::create_dir(&child).unwrap();
    let path = child.display().to_string();
    let prefs = AppPrefs {
        projects: vec![path.clone()],
        ..Default::default()
    };
    let result = overview(&AppState::default(), &prefs);
    assert_eq!(result["projects"][0]["current_items_total"], 0);
    assert!(!child.join(".kanzei").exists());
}

#[test]
fn failed_read_retains_previous_summary_and_marks_stale() {
    let root = Project::new();
    root.tracker("## R-001 Keep me [doing]\n- 批次: 2/5\n");
    let state = AppState::default();
    let prefs = root.prefs();
    let first = overview(&state, &prefs);
    // A malformed/inaccessible state is not equivalent to an empty project.
    std::fs::write(root.0.join(".kanzei/state.db"), "not a database").unwrap();
    let failed = overview(&state, &prefs);
    assert_eq!(failed["projects"][0]["freshness"], "stale");
    assert!(failed["projects"][0]["error"].is_string());
    assert_eq!(
        failed["projects"][0]["current_items"],
        first["projects"][0]["current_items"]
    );
    assert_eq!(
        failed["projects"][0]["observed_at"],
        first["projects"][0]["observed_at"]
    );
    let missing = root.0.join("missing").display().to_string();
    let unseen = overview(
        &state,
        &AppPrefs {
            projects: vec![missing],
            ..Default::default()
        },
    );
    assert_eq!(unseen["projects"][0]["freshness"], "unavailable");
    assert!(unseen["projects"][0].get("counts").is_none());
}

#[test]
fn historical_doing_is_not_running_and_only_successful_claim_associates_default() {
    let root = Project::new();
    root.tracker("## R-001 Old [doing]\n\n## R-002 Actual [doing]\n\n## R-003 Failed [doing]\n");
    let state = AppState::default();
    let process = crate::ensure_default_process(&state, &root.0);
    let session = crate::process_session_id(&root.0, Some(&process.id));
    crate::runtime_for(&state, &session)
        .running
        .store(true, Ordering::SeqCst);
    crate::runtime_for(&state, &session)
        .live
        .lock()
        .unwrap()
        .begin("turn", "input", "", "", "");
    let store = SessionStore::open(&kanzei_core::project_state_path(&root.0)).unwrap();
    store.create_session(&session, &root.path(), None).unwrap();
    let before = project_summary(&state, &root.prefs(), &root.path()).unwrap();
    assert_eq!(before["running_lines"], 1);
    assert!(before["current_items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["running"] == false));
    for (id, failed) in [("R-002", false), ("R-003", true)] {
        let call = format!("claim-{id}");
        store
            .append_event(
                &session,
                "session.tool_called",
                &json!({"turn_id":"turn", "fact":{
            "name":"work","input":{"action":"claim","id":id},"call_id":call}}),
            )
            .unwrap();
        store.append_event(&session, "session.tool_result_committed", &json!({"turn_id":"turn", "fact":{
            "call_id":call,"is_error":failed,"content":json!({"claimed":id,"lifecycle_status":"doing"}).to_string()}})).unwrap();
    }
    let count_before = store.list_events(&session, 0).unwrap().len();
    let after = project_summary(&state, &root.prefs(), &root.path()).unwrap();
    assert_eq!(after["current_items"][0]["id"], "R-002");
    assert_eq!(after["current_items"][0]["running"], true);
    assert_eq!(after["current_items"][1]["running"], false);
    assert_eq!(store.list_events(&session, 0).unwrap().len(), count_before);
    assert!(state.restored_projects.lock().unwrap().is_empty());
    // A new run can be unrelated. The old successful receipt is still history, not proof
    // that this new run is executing the old doing item.
    let runtime = crate::runtime_for(&state, &session);
    runtime.run_generation.fetch_add(1, Ordering::SeqCst);
    runtime
        .live
        .lock()
        .unwrap()
        .begin("new-turn", "new-input", "unrelated question", "", "");
    let new_run = project_summary(&state, &root.prefs(), &root.path()).unwrap();
    assert!(new_run["current_items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["running"] == false));
    assert!(new_run["lines"][0]["current_item_id"].is_null());
}

#[test]
fn summary_payload_stays_bounded_without_full_tracker_fields() {
    let root = Project::new();
    let text = (1..=90)
        .map(|i| {
            format!(
                "## R-{i:03} Item {i} [doing]\n- 批次: 0/5\n- 验收: {}\n",
                "large body ".repeat(400)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    root.tracker(&text);
    let result = project_summary(&AppState::default(), &root.prefs(), &root.path()).unwrap();
    assert_eq!(result["current_items_total"], 90);
    assert_eq!(result["current_items"].as_array().unwrap().len(), 2);
    assert!(result.to_string().len() < 6000);
    assert!(!result.to_string().contains("large body"));
}

#[test]
fn git_batch_progress_uses_shared_derivation_not_markdown_count() {
    let root = Project::new();
    root.tracker("## R-001 Reader [doing]\n- 批次: 4/5\n");
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "feat: R-001 B1-B2 reader",
        ],
    ] {
        assert!(crate::state::hidden_command("git")
            .current_dir(&root.0)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    let result = project_summary(&AppState::default(), &root.prefs(), &root.path()).unwrap();
    assert_eq!(
        result["current_items"][0]["batches"],
        json!({"done":2,"total":5,"source":"git"})
    );
}

#[test]
fn workspace_overview_形状与ipc契约一致() {
    let root = Project::new();
    root.tracker("## R-001 Reader [doing]\n- 批次: 2/5\n- 优先级: P1\n");
    let state = AppState::default();
    let process = crate::ensure_default_process(&state, &root.0);
    let session = crate::process_session_id(&root.0, Some(&process.id));
    let store = SessionStore::open(&kanzei_core::project_state_path(&root.0)).unwrap();
    store.create_session(&session, &root.path(), None).unwrap();
    store
        .append_event(
            &session,
            "session.tool_called",
            &json!({"turn_id":"turn", "fact":{
        "name":"work","input":{"action":"claim","id":"R-001"},"call_id":"claim"}}),
        )
        .unwrap();
    store.append_event(&session, "session.tool_result_committed", &json!({"turn_id":"turn", "fact":{
        "call_id":"claim","is_error":false,"content":"{\"claimed\":\"R-001\",\"lifecycle_status\":\"doing\"}"}})).unwrap();
    let result = overview(&state, &root.prefs());
    crate::ipc_contract::tests::check_contract(
        "workspace_overview",
        crate::ipc_contract::shape(&result),
        "workspace_overview IPC 形状变化",
    );
}

#[test]
fn ten_idle_projects_have_bounded_payload_and_no_new_runtime_state() {
    let projects = (0..10).map(|_| Project::new()).collect::<Vec<_>>();
    for root in &projects {
        root.tracker("## R-001 Reader [doing]\n- 批次: 2/5\n");
    }
    let prefs = AppPrefs {
        projects: projects.iter().map(Project::path).collect(),
        ..Default::default()
    };
    let state = AppState::default();
    let mut times = Vec::new();
    let mut bytes = 0;
    for _ in 0..10 {
        let started = std::time::Instant::now();
        let snapshot = overview(&state, &prefs);
        times.push(started.elapsed().as_micros());
        bytes = snapshot.to_string().len();
    }
    times.sort_unstable();
    eprintln!(
        "workbench 10 idle projects: p50={}us p95={}us payload={} bytes",
        times[4], times[9], bytes
    );
    assert!(bytes < 60_000);
    assert!(state.runtimes.lock().unwrap().is_empty());
    assert!(state.processes.lock().unwrap().is_empty());
    for root in projects {
        assert!(!root.0.join(".kanzei/state.db").exists());
        assert_eq!(
            std::fs::read_dir(root.0.join(".kanzei/project"))
                .unwrap()
                .count(),
            1
        );
    }
}

#[test]
fn startup_recovery_is_idempotent_and_does_not_override_live_settings() {
    let root = Project::new();
    let state = AppState::default();
    let process = crate::ensure_default_process(&state, &root.0);
    crate::processes::registry::persist_process(&root.0, &process).unwrap();
    recover_project(&state, &root.0).unwrap();
    process.subagents_enabled.store(false, Ordering::SeqCst);
    recover_project(&state, &root.0).unwrap();
    assert!(!process.subagents_enabled.load(Ordering::SeqCst));
    assert_eq!(state.processes.lock().unwrap().len(), 1);
    assert!(state.runtimes.lock().unwrap().is_empty());
}

#[test]
fn multiple_unit_claims_share_one_parent_row_and_a_new_focus_does_not_run_old_claims() {
    let entries = docstore::parse(
        &REQUIREMENTS,
        "## R-001 Reader [doing]\n- 取得线: branch-a\n- 批次: 2/5\n",
    );
    let lines = vec![
        json!({"id":"p1","label":"p1","branch":"branch-a","running":true,"current_item_id":"R-002"}),
        json!({"id":"p2","label":"p2","branch":"branch-b","running":true,"current_item_id":"R-001"}),
    ];
    let claims = vec![
        ("R-001".into(), Some("branch-a".into())),
        ("R-001".into(), Some("branch-b".into())),
    ];
    let summary = item_summary("reader", "req", &entries[0], None, &lines, &claims);
    assert_eq!(summary["owner_lines"].as_array().unwrap().len(), 2);
    assert_eq!(summary["owner_lines"][0]["running"], false);
    assert_eq!(summary["owner_lines"][1]["running"], true);
    assert_eq!(summary["running"], true);
    assert_eq!(summary["id"], "R-001");
    let reassigned = docstore::parse(
        &REQUIREMENTS,
        "## R-001 Reader [doing]\n- 取得线: branch-c\n",
    );
    let moved = item_summary("reader", "req", &reassigned[0], None, &lines, &[]);
    assert_eq!(
        moved["running"], false,
        "a current-run receipt cannot override a newer tracker reassignment"
    );
}

#[test]
fn summary_cache_is_bounded_by_project_count_and_payload_bytes() {
    let mut cache = SummaryCache::new();
    for i in 0..(MAX_CACHED_PROJECTS + 20) {
        remember_summary(
            &mut cache,
            &format!("p-{i}"),
            &json!({"observed_at":i,"title":"x"}),
        );
    }
    assert_eq!(cache.len(), MAX_CACHED_PROJECTS);
    assert!(!cache.contains_key("p-0"));
    for i in 0..20 {
        remember_summary(
            &mut cache,
            &format!("large-{i}"),
            &json!({"observed_at":1000+i,"title":"x".repeat(256*1024)}),
        );
    }
    assert!(cache.values().map(|(_, bytes)| *bytes).sum::<usize>() <= MAX_CACHE_BYTES);
    assert!(cache.contains_key("large-19"));
    let previous = cache.get("large-19").unwrap().0.clone();
    remember_summary(
        &mut cache,
        "large-19",
        &json!({"title":"x".repeat(MAX_CACHE_BYTES + 1)}),
    );
    assert_eq!(cache.get("large-19").unwrap().0, previous);
}

#[test]
fn overview_success_does_not_disguise_a_detail_read_failure() {
    let root = Project::new();
    root.tracker("## R-001 Reader [doing]\n- 批次: 2/5\n");
    let store = SessionStore::open(&kanzei_core::project_state_path(&root.0)).unwrap();
    let journal = kanzei_core::project_session_id(&root.0);
    store.create_session(&journal, &root.path(), None).unwrap();
    std::fs::create_dir_all(root.0.join(".kanzei/verification")).unwrap();
    let record = root
        .0
        .join(".kanzei/verification/v-00000000000000000000000000000000.json");
    std::fs::write(&record, "invalid JSON").unwrap();
    let state = AppState::default();
    let overview = overview(&state, &root.prefs());
    assert_eq!(overview["projects"][0]["freshness"], "fresh");
    let detail = details(&state, &root.prefs(), Some(&root.path()));
    assert_eq!(detail["projects"][0]["freshness"], "unavailable");
    assert!(detail["projects"][0]["error"].is_string());
    assert!(
        detail["projects"][0].get("decisions").is_none(),
        "a failed detail cannot replace old review data with an empty success"
    );
    assert_eq!(std::fs::read_to_string(record).unwrap(), "invalid JSON");
}

#[test]
fn missing_database_with_existing_verification_is_not_reported_as_empty() {
    let root = Project::new();
    std::fs::create_dir_all(root.0.join(".kanzei/verification")).unwrap();
    std::fs::write(root.0.join(".kanzei/verification/job.json"), "{}").unwrap();
    let result = overview(&AppState::default(), &root.prefs());
    assert_eq!(result["projects"][0]["freshness"], "unavailable");
    assert!(result["projects"][0]["error"]
        .as_str()
        .unwrap()
        .contains("状态库缺失"));
    assert!(!root.0.join(".kanzei/state.db").exists());
}

/// UX-009:state.db 里登记、还没进内存表的线也带展示名/标题/类型/序号/最近活动,
/// 并按数字序排(p2 在 p10 之前),界面不再拿 pN 当名字。
#[test]
fn lines_带命名事实并按数字序排() {
    let root = Project::new();
    let store = SessionStore::open(&kanzei_core::project_state_path(&root.0)).unwrap();
    let stored = |id: &str, profile: Option<&str>| kanzei_core::StoredProcess {
        process_id: format!("{id}|{}", root.path()),
        origin_project: root.path(),
        project_dir: root.path(),
        worktree_path: None,
        model: None,
        profile: profile.map(String::from),
        research_topic: None,
        reasoning: None,
        manual_models: Vec::new(),
        phase_pipeline: false,
        subagents_enabled: true,
        tracker_writes_enabled: false,
        updated_at: 1,
    };
    store
        .upsert_process(&stored("p10", Some("readonly")))
        .unwrap();
    store
        .upsert_process(&stored("p2", Some("readonly")))
        .unwrap();
    store.upsert_process(&stored("p3", None)).unwrap();
    let p2 = format!("p2|{}", root.path());
    let session = crate::process_session_id(&root.0, Some(&p2));
    store
        .set_session_title(&session, &root.path(), Some("方案对照"))
        .unwrap();
    drop(store);

    let result = overview(&AppState::default(), &root.prefs());
    let lines = result["projects"][0]["lines"].as_array().unwrap();
    let column = |key: &str| {
        lines
            .iter()
            .map(|line| line[key].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        column("ordinal"),
        [json!(2), json!(3), json!(10)],
        "数字序,不是 id 字符串序"
    );
    assert_eq!(
        column("label"),
        [json!("方案对照"), json!("独立任务 3"), json!("讨论 10")]
    );
    assert_eq!(
        column("kind"),
        [json!("discussion"), json!("task"), json!("discussion")]
    );
    assert_eq!(
        column("title"),
        [json!("方案对照"), Value::Null, Value::Null]
    );
    assert_eq!(
        column("title_custom"),
        [json!(true), json!(false), json!(false)]
    );
    assert!(lines[0]["updated_at"].is_number(), "改名建出了会话行");
    assert!(lines[1]["updated_at"].is_null() && lines[2]["updated_at"].is_null());
}
