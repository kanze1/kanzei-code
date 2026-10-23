use kanzei_core::SessionStore;
use kanzei_runtime::schedules::execute;
use kanzei_tools::schedules::*;
use serde_json::json;
const TEXT:&str="---\nname: check\nenabled: true\nwhen: 每 15 分钟\nsteps:\n  - run: 'echo \"quoted # literal\"' # comment\n  - prompt: |\n      inspect the output\n      preserve this line\nwriteback:\n  - notify\n  - memory_inbox\n  - file: reports/answer.md\n  - idea\n---\nHuman explanation\n";
fn project() -> kanzei_base::test_support::TestDir {
    let path = kanzei_base::test_support::TestDir::new("kz-schedule-runtime-");
    std::fs::create_dir_all(path.join(".kanzei/schedules")).unwrap();
    std::fs::write(path.join(".kanzei/schedules/check.md"), TEXT).unwrap();
    path
}
#[tokio::test]
async fn real_command_run_isolated_history_file_writeback_and_duplicate_skipped() {
    let root = project();
    let mut def = parse(TEXT, "check").unwrap();
    def.steps = vec![Step::Run("echo SCHEDULE_ACTUAL_OUTPUT".into())];
    def.writeback = vec!["file:reports/result.md".into()];
    std::fs::write(path(&root, "check").unwrap(), render(&def)).unwrap();
    let outcome = execute(root.to_path_buf(), def.clone(), 1, "manual".into())
        .await
        .unwrap()
        .unwrap();
    assert!(outcome.ok, "{:?}", outcome.error);
    assert!(outcome.text.contains("SCHEDULE_ACTUAL_OUTPUT"));
    assert!(std::fs::read_to_string(root.join("reports/result.md"))
        .unwrap()
        .contains("SCHEDULE_ACTUAL_OUTPUT"));
    let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    assert!(store
        .list_events(&kanzei_core::project_session_id(&root), 0)
        .unwrap()
        .is_empty());
    assert!(store
        .latest_event(&outcome.run_session_id, "conversation.updated")
        .unwrap()
        .is_some());
    assert!(execute(root.to_path_buf(), def, 1, "manual".into())
        .await
        .unwrap()
        .is_none());
    drop(store);
}
#[tokio::test]
async fn three_failed_real_commands_disable_definition() {
    let root = project();
    let mut def = parse(TEXT, "check").unwrap();
    def.steps = vec![Step::Run("exit 7".into())];
    def.writeback = vec![];
    std::fs::write(path(&root, "check").unwrap(), render(&def)).unwrap();
    for slot in 1..=3 {
        let outcome = execute(root.to_path_buf(), def.clone(), slot, "manual".into())
            .await
            .unwrap()
            .unwrap();
        assert!(!outcome.ok);
        assert!(outcome.error.unwrap().contains("7"));
    }
    assert!(
        !parse(
            &std::fs::read_to_string(path(&root, "check").unwrap()).unwrap(),
            "check"
        )
        .unwrap()
        .enabled
    );
    assert!(history(&root, Some("check"), 20)
        .unwrap()
        .iter()
        .any(|event| event["type"] == "schedule.auto_disabled"));
}
#[test]
fn stale_dispatch_does_not_run_a_replaced_definition() {
    let root = project();
    let def = parse(TEXT, "check").unwrap();
    std::fs::write(path(&root, "check").unwrap(), set_enabled(TEXT, false)).unwrap();
    assert!(!claim(&root, &def, 42, "old-dispatch", "timer").unwrap());
    assert!(history(&root, Some("check"), 10).unwrap().is_empty());
}
#[tokio::test]
async fn rearming_starts_a_new_failure_streak() {
    let root = project();
    let mut def = parse(TEXT, "check").unwrap();
    def.steps = vec![Step::Run("exit 7".into())];
    def.writeback = vec![];
    std::fs::write(path(&root, "check").unwrap(), render(&def)).unwrap();
    for slot in 1..=2 {
        execute(root.to_path_buf(), def.clone(), slot, "manual".into())
            .await
            .unwrap()
            .unwrap();
    }
    record(
        &root,
        "schedule.armed",
        &json!({"name":"check","at_ms":chrono::Local::now().timestamp_millis()}),
    )
    .unwrap();
    execute(root.to_path_buf(), def.clone(), 3, "manual".into())
        .await
        .unwrap()
        .unwrap();
    assert!(
        parse(
            &std::fs::read_to_string(path(&root, "check").unwrap()).unwrap(),
            "check"
        )
        .unwrap()
        .enabled
    );
}
