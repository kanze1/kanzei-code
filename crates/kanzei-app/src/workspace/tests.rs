use super::*;

struct Project {
    root: PathBuf,
    _directory: kanzei_base::test_support::TestDir,
}
impl Project {
    fn new() -> Self {
        let directory = kanzei_base::test_support::TestDir::new("kz-workbench-");
        Self {
            root: project_root(&directory.display().to_string()),
            _directory: directory,
        }
    }
    fn path(&self) -> String {
        self.root.display().to_string()
    }
    fn prefs(&self) -> AppPrefs {
        AppPrefs {
            projects: vec![self.path()],
            current: Some(self.path()),
            ..Default::default()
        }
    }
}
#[test]
fn overview_does_not_initialize_empty_projects_or_runtime() {
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
    recover_project(&state, &root.root).unwrap();
    assert!(!root.root.join(".kanzei").exists());
    assert!(state.processes.lock().unwrap().is_empty());
    assert!(state.runtimes.lock().unwrap().is_empty());
}

#[test]
fn empty_child_does_not_read_its_parents_project() {
    let parent = Project::new();
    std::fs::create_dir_all(parent.root.join(".kanzei")).unwrap();
    let child = parent.root.join("child");
    std::fs::create_dir(&child).unwrap();
    let path = child.display().to_string();
    let prefs = AppPrefs {
        projects: vec![path.clone()],
        ..Default::default()
    };
    let result = overview(&AppState::default(), &prefs);
    assert_eq!(result["projects"][0]["freshness"], "fresh");
    assert!(!child.join(".kanzei").exists());
}

#[test]
fn failed_read_retains_previous_summary_and_marks_stale() {
    let root = Project::new();
    let state = AppState::default();
    let prefs = root.prefs();
    let first = overview(&state, &prefs);
    std::fs::create_dir_all(root.root.join(".kanzei")).unwrap();
    std::fs::write(root.root.join(".kanzei/state.db"), "not a database").unwrap();
    let failed = overview(&state, &prefs);
    assert_eq!(failed["projects"][0]["freshness"], "stale");
    assert!(failed["projects"][0]["error"].is_string());
    assert_eq!(
        failed["projects"][0]["observed_at"],
        first["projects"][0]["observed_at"]
    );
    let missing = root.root.join("missing").display().to_string();
    let unseen = overview(
        &state,
        &AppPrefs {
            projects: vec![missing],
            ..Default::default()
        },
    );
    assert_eq!(unseen["projects"][0]["freshness"], "unavailable");
}

#[test]
fn ten_idle_projects_have_bounded_payload_and_no_new_runtime_state() {
    let projects = (0..10).map(|_| Project::new()).collect::<Vec<_>>();
    let prefs = AppPrefs {
        projects: projects.iter().map(Project::path).collect(),
        ..Default::default()
    };
    let state = AppState::default();
    let snapshot = overview(&state, &prefs);
    assert!(snapshot.to_string().len() < 60_000);
    assert!(state.runtimes.lock().unwrap().is_empty());
    assert!(state.processes.lock().unwrap().is_empty());
    for root in projects {
        assert!(!root.root.join(".kanzei/state.db").exists());
    }
}

#[test]
fn startup_recovery_is_idempotent_and_does_not_override_live_settings() {
    let root = Project::new();
    let state = AppState::default();
    let process = crate::ensure_default_process(&state, &root.root);
    crate::processes::registry::persist_process(&root.root, &process).unwrap();
    recover_project(&state, &root.root).unwrap();
    *process.subagent_mode.lock().unwrap() = kanzei_harness::SubagentMode::Off;
    recover_project(&state, &root.root).unwrap();
    assert!(!(*process.subagent_mode.lock().unwrap()).enabled());
    assert_eq!(state.processes.lock().unwrap().len(), 1);
    assert!(state.runtimes.lock().unwrap().is_empty());
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

/// UX-009:state.db 里登记、还没进内存表的线也带展示名/标题/类型/序号/最近活动,
/// 并按数字序排(p2 在 p10 之前),界面不再拿 pN 当名字。
#[test]
fn lines_带命名事实并按数字序排() {
    let root = Project::new();
    let store = SessionStore::open(&kanzei_core::project_state_path(&root.root)).unwrap();
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

        subagent_mode: kanzei_harness::SubagentMode::Auto,
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
    let session = crate::process_session_id(&root.root, Some(&p2));
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
        [json!("方案对照"), json!("新对话"), json!("新讨论")]
    );
    assert_eq!(
        column("kind"),
        [
            json!("discussion"),
            json!("conversation"),
            json!("discussion")
        ]
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
