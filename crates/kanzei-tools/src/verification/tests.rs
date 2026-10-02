use super::*;
use kanzei_core::WorkUnitSpec;

fn fixture() -> (PathBuf, ToolCtx) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "kz-verification-test-{}-{}-{}",
        std::process::id(),
        now(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(&dir)
        .status()
        .unwrap()
        .success());
    std::fs::write(dir.join("source.txt"), "frozen").unwrap();
    std::fs::write(dir.join(".gitignore"), "ignored.txt\n.kanzei/\n").unwrap();
    std::fs::write(dir.join("ignored.txt"), "not source").unwrap();
    std::fs::write(dir.join(".kanzei/project/requirements.md"), "# Requirements\n\n## R-001 Reader experiment [doing]\n- 执行模型: work_units_v1\n- 验收: reader works\n- 优先级: P1\n").unwrap();
    let db = store(&dir).unwrap();
    for n in 1..=3 {
        db.create_work_unit(WorkUnitSpec {
            unit_id: format!("R-001/W{n}"),
            requirement_id: "R-001".into(),
            objective: format!("reader scenario {n}"),
            scope: vec!["source.txt".into()],
            dependencies: if n == 3 {
                vec!["R-001/W1".into()]
            } else {
                vec![]
            },
            acceptance: vec!["reader works".into()],
            verification: vec![],
            base_revision: "fixture".into(),
        })
        .unwrap();
    }
    db.append_work_fact("R-001/W1", WorkFact::Claimed { claimed_by: None })
        .unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    (dir, ctx)
}
fn request() -> VerificationRequest {
    VerificationRequest {
        unit_id: "R-001/W1".into(),
        criteria: vec!["reader works".into()],
        resource: format!("test-{:?}", SystemTime::now()),
        environment: "local test shell".into(),
    }
}
fn frozen_check() -> &'static str {
    if cfg!(windows) {
        "if ((Get-Content source.txt -Raw) -ne 'frozen') { exit 4 }; Write-Output 'verified frozen reader'"
    } else {
        "test \"$(cat source.txt)\" = frozen"
    }
}

#[tokio::test]
async fn frozen_verification_releases_wip_but_not_dependencies() {
    let (root, ctx) = fixture();
    let req = request();
    let job = prepare(&ctx, frozen_check(), 10_000, &req).unwrap();
    assert!(!job.snapshot.join("ignored.txt").exists());
    assert!(!job.snapshot.join(".kanzei").exists());
    assert_eq!(
        prepare(&ctx, frozen_check(), 10_000, &req).unwrap().id,
        job.id
    );
    let control = crate::work::resolve_work_decision(
        &root,
        &root,
        kanzei_harness::auto_run::WorkPriority::RequirementFirst,
    )
    .unwrap();
    assert_eq!(control.selected.unwrap().id, "R-001/W2");
    let db = store(&root).unwrap();
    let all = db.list_work_units(None).unwrap();
    assert!(!all
        .iter()
        .find(|u| u.unit_id == "R-001/W3")
        .unwrap()
        .dependencies_satisfied(&all));
    assert!(db
        .append_work_fact("R-001/W1", WorkFact::Completed)
        .is_err());
    std::fs::write(root.join("source.txt"), "new independent development").unwrap();
    worker::run(&root, &job.id).await.unwrap();
    assert_eq!(read_job(&root, &job.id).unwrap().status, "passed");
    let all = db.list_work_units(None).unwrap();
    assert_eq!(
        all.iter().find(|u| u.unit_id == "R-001/W1").unwrap().status,
        WorkUnitStatus::Done
    );
    assert!(all
        .iter()
        .find(|u| u.unit_id == "R-001/W3")
        .unwrap()
        .dependencies_satisfied(&all));
    assert_eq!(
        std::fs::read_to_string(root.join("source.txt")).unwrap(),
        "new independent development"
    );
    let count = db.list_work_events("R-001/W1").unwrap().len();
    worker::run(&root, &job.id).await.unwrap();
    assert_eq!(db.list_work_events("R-001/W1").unwrap().len(), count);
}

#[tokio::test]
async fn failure_is_local_and_keeps_independent_unit_executable() {
    let (root, ctx) = fixture();
    let job = prepare(&ctx, "exit 7", 10_000, &request()).unwrap();
    worker::run(&root, &job.id).await.unwrap();
    assert_eq!(read_job(&root, &job.id).unwrap().exit_code, Some(7));
    assert_eq!(
        store(&root)
            .unwrap()
            .get_work_unit("R-001/W1")
            .unwrap()
            .unwrap()
            .status,
        WorkUnitStatus::Blocked
    );
    let control = crate::work::resolve_work_decision(
        &root,
        &root,
        kanzei_harness::auto_run::WorkPriority::RequirementFirst,
    )
    .unwrap();
    assert_eq!(control.selected.unwrap().id, "R-001/W2");
}

#[tokio::test]
async fn cancel_and_late_result_cannot_complete_replaced_unit() {
    let (root, ctx) = fixture();
    let job = prepare(&ctx, "exit 0", 10_000, &request()).unwrap();
    cancel(&root, &job.id).unwrap();
    worker::run(&root, &job.id).await.unwrap();
    assert_eq!(read_job(&root, &job.id).unwrap().status, "cancelled");
    let (root, ctx) = fixture();
    let job = prepare(&ctx, "exit 0", 10_000, &request()).unwrap();
    store(&root)
        .unwrap()
        .append_work_fact(
            "R-001/W1",
            WorkFact::Superseded {
                reason: "user correction".into(),
            },
        )
        .unwrap();
    worker::run(&root, &job.id).await.unwrap();
    assert_eq!(read_job(&root, &job.id).unwrap().status, "superseded");
    assert_eq!(
        store(&root)
            .unwrap()
            .get_work_unit("R-001/W1")
            .unwrap()
            .unwrap()
            .status,
        WorkUnitStatus::Superseded
    );
}

#[tokio::test]
async fn source_mutation_by_test_invalidates_passing_exit_code() {
    let (root, ctx) = fixture();
    let command = if cfg!(windows) {
        "[IO.File]::WriteAllText((Join-Path (Get-Location) 'source.txt'), 'changed')"
    } else {
        "printf changed > source.txt"
    };
    let job = prepare(&ctx, command, 10_000, &request()).unwrap();
    worker::run(&root, &job.id).await.unwrap();
    assert_eq!(read_job(&root, &job.id).unwrap().status, "stale");
    assert_eq!(
        store(&root)
            .unwrap()
            .get_work_unit("R-001/W1")
            .unwrap()
            .unwrap()
            .status,
        WorkUnitStatus::Blocked
    );
}

#[test]
fn lost_worker_is_recovered_without_rerunning_commands() {
    let (root, ctx) = fixture();
    let mut job = prepare(&ctx, "exit 0", 10_000, &request()).unwrap();
    job.updated_at = now() - 60_000;
    crate::atomic_file::write_atomic_bytes(
        &job_file(&root, &job.id).unwrap(),
        &serde_json::to_vec(&job).unwrap(),
    )
    .unwrap();
    recover(&root).unwrap();
    assert_eq!(read_job(&root, &job.id).unwrap().status, "interrupted");
    assert_eq!(
        store(&root)
            .unwrap()
            .get_work_unit("R-001/W1")
            .unwrap()
            .unwrap()
            .status,
        WorkUnitStatus::Blocked
    );
    assert!(!job.log_path.exists());
}

#[test]
fn invalid_evidence_rolls_back_entire_verification_result() {
    let (root, ctx) = fixture();
    let job = prepare(&ctx, "exit 0", 10_000, &request()).unwrap();
    let db = store(&root).unwrap();
    let before = db.get_work_unit("R-001/W1").unwrap();
    let events = db.list_work_events("R-001/W1").unwrap().len();
    assert!(db
        .append_work_facts(
            "R-001/W1",
            vec![
                WorkFact::VerificationConcluded {
                    job_id: job.id,
                    passed: true,
                    reason: "passed".into()
                },
                WorkFact::EvidenceAdded {
                    evidence: kanzei_core::WorkEvidence {
                        criterion: "unrelated acceptance".into(),
                        evidence_refs: vec!["wrong".into()]
                    }
                },
            ]
        )
        .is_err());
    assert_eq!(db.get_work_unit("R-001/W1").unwrap(), before);
    assert_eq!(db.list_work_events("R-001/W1").unwrap().len(), events);
}

#[tokio::test]
async fn timeout_stops_worker_and_does_not_create_success_evidence() {
    let (root, ctx) = fixture();
    let command = if cfg!(windows) {
        "Start-Sleep -Seconds 20"
    } else {
        "sleep 20"
    };
    let job = prepare(&ctx, command, 1000, &request()).unwrap();
    worker::run(&root, &job.id).await.unwrap();
    assert_eq!(read_job(&root, &job.id).unwrap().status, "timed_out");
    assert!(store(&root)
        .unwrap()
        .get_work_unit("R-001/W1")
        .unwrap()
        .unwrap()
        .evidence
        .is_empty());
}
