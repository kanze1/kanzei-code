use super::*;
use kanzei_harness::{KanzeiConfig, ProfileKind};
use kanzei_llm::{ProxyConfig, Route};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn fixture_worker(team: &AgentTeam) -> (AgentJob, Arc<ChildWorker>) {
    let job = team
        .0
        .store
        .update("question-child", |job| job.state = "running".into())
        .unwrap();
    let token = Arc::new(ChildWorker {
        cancel: CancellationToken::new(),
        owner: claim_child(&team.0.store, &job.id).unwrap(),
    });
    team.0
        .active
        .lock()
        .unwrap()
        .insert(job.id.clone(), token.clone());
    (job, token)
}

fn background_question(id: u64) -> kanzei_core::AskRequest {
    kanzei_core::AskRequest::Question {
        question: format!("question {id}"),
        options: vec![],
        default: None,
        multiple: false,
        background: true,
        callback_id: Some(format!("callback-{id}")),
    }
}

fn owner_fixture_process(
    team: &AgentTeam,
    url: &str,
    action: &str,
    revision: u64,
) -> std::process::Command {
    let profile = team.0.root.join(format!("owner-profile-{action}"));
    std::fs::create_dir_all(profile.join(".kanzei")).unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "team::tests::child_owner_process_fixture",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("D2_OWNER_FIXTURE_ROOT", &team.0.root)
        .env("D2_OWNER_FIXTURE_OWNER", &team.0.owner)
        .env("D2_OWNER_FIXTURE_URL", url)
        .env("D2_OWNER_FIXTURE_ACTION", action)
        .env("D2_OWNER_FIXTURE_REVISION", revision.to_string())
        .env("KANZEI_HOME", profile.join(".kanzei"))
        .env("HOME", &profile)
        .env("USERPROFILE", &profile)
        .env("APPDATA", profile.join("AppData/Roaming"))
        .env("LOCALAPPDATA", profile.join("AppData/Local"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command
}

// Invoked only as a real second process by the regression below. It does not
// share TEAMS or active workers with the process that owns the first request.
#[tokio::test]
#[ignore = "subprocess fixture; requires D2_OWNER_FIXTURE_ROOT"]
async fn child_owner_process_fixture() {
    let root = PathBuf::from(std::env::var_os("D2_OWNER_FIXTURE_ROOT").expect("fixture root"));
    let owner = std::env::var("D2_OWNER_FIXTURE_OWNER").unwrap();
    let url = std::env::var("D2_OWNER_FIXTURE_URL").unwrap();
    let action = std::env::var("D2_OWNER_FIXTURE_ACTION").unwrap();
    let fixture_id =
        std::env::var("D2_OWNER_FIXTURE_ID").unwrap_or_else(|_| "cross-process-child".into());
    let id = fixture_id.as_str();
    let store = store_for_inspection(&root, &owner).unwrap();
    let before = store.get(id).unwrap();
    if action == "cleanup-failed" {
        assert!(cleanup_failed(&before));
        assert!(!crate::background::has_child_cleanup_records(
            &root, &owner, id
        ));
        let remote = team(root, &url, &owner);
        let error = remote
            .ui_command(
                json!({"action":"resume","id":id,"prompt":"remote repair cannot guess baseline"}),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("原执行者"), "{error}");
        assert_eq!(
            serde_json::to_value(store.get(id).unwrap()).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
    } else if action == "wait" {
        assert_eq!(before.state, "running");
        let remote = team(root.clone(), &url, &owner);
        let waiting = remote.ui_command(json!({"action":"wait","id":id}));
        tokio::pin!(waiting);
        tokio::select! {
            result = &mut waiting => panic!("remote live wait returned early: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(200)) => {},
        }
        std::fs::write(
            root.join("d2-remote-waiting"),
            "waiting while first owner/request held",
        )
        .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(8), waiting)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result[0]["state"], "done");
        assert_eq!(store.get(id).unwrap().attempt, 1);
    } else if action == "busy" {
        assert_eq!(
            before.state, "running",
            "inspection cannot interrupt a live remote worker"
        );
        assert_eq!(
            before.revision,
            std::env::var("D2_OWNER_FIXTURE_REVISION")
                .unwrap()
                .parse::<u64>()
                .unwrap()
        );
        let remote = team(root, &url, &owner);
        let error = remote
            .ui_command(json!({"action":"resume","id":id,"prompt":"remote busy follow-up"}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("其他执行者"), "{error}");
        let error = remote.stop(id).unwrap_err();
        assert!(error.to_string().contains("其他执行者"), "{error}");
        let after = store.get(id).unwrap();
        assert_eq!(
            serde_json::to_value(&after).unwrap(),
            serde_json::to_value(&before).unwrap(),
            "busy mutation must not edit the durable job"
        );
    } else {
        assert_eq!(action, "resume");
        assert_eq!(before.state, "done");
        let remote = team(root, &url, &owner);
        remote
            .ui_command(json!({"action":"resume","id":id,"prompt":"explicit idle takeover"}))
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(8),
            remote.command("", json!({"action":"wait","id":id})),
        )
        .await
        .unwrap()
        .unwrap();
        let after = remote.resolve(id).unwrap();
        assert_eq!(after.state, "done");
        assert_eq!(after.attempt, 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_process_inspection_and_resume_preserve_live_child_and_idle_can_transfer() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let team = team(project(), &url, "cross-process-team-owner");
    team.command(
        "cross-process-child",
        json!({"agent":"plan","prompt":"first held assignment"}),
    )
    .await
    .unwrap();
    let (mut first, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(request(&mut first)
        .await
        .to_string()
        .contains("first held assignment"));
    let before = team.resolve("cross-process-child").unwrap();
    assert_eq!(before.state, "running");
    let released_owner = Arc::downgrade(&team.0.active.lock().unwrap()[&before.id].owner);
    assert!(team.0.runtime.lock().unwrap().transcript_sink.is_none());
    assert!(team.0.runtime.lock().unwrap().transcript_provider.is_none());
    let mut busy = owner_fixture_process(&team, &url, "busy", before.revision);
    let output = tokio::task::spawn_blocking(move || busy.output().unwrap())
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "second process failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::to_value(team.resolve(&before.id).unwrap()).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), listener.accept())
            .await
            .is_err(),
        "a busy remote resume cannot start a second model request"
    );
    let mut remote_wait = owner_fixture_process(&team, &url, "wait", before.revision)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !team.0.root.join("d2-remote-waiting").exists() {
            assert!(
                remote_wait.try_wait().unwrap().is_none(),
                "remote wait must not prematurely reject a live owner"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(team.resolve(&before.id).unwrap()).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "read-only remote wait cannot send a provider request"
    );

    // Ownership is per child, not per team or parent round: a sibling can run
    // while the first child's real model response is still held.
    team.command(
        "independent-child",
        json!({"agent":"plan","prompt":"independent sibling"}),
    )
    .await
    .unwrap();
    let (mut sibling, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(request(&mut sibling)
        .await
        .to_string()
        .contains("independent sibling"));
    respond(&mut sibling, json!({"content":"sibling complete"})).await;
    respond(&mut first, json!({"content":"first complete"})).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
    let waited = tokio::task::spawn_blocking(move || remote_wait.wait_with_output().unwrap())
        .await
        .unwrap();
    assert!(
        waited.status.success(),
        "remote wait failed: {}\n{}",
        String::from_utf8_lossy(&waited.stdout),
        String::from_utf8_lossy(&waited.stderr)
    );
    // A terminal DB row can be visible just before final cleanup drops its guard.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match try_child_owner(&team.0.store, &before.id) {
                Ok(owner) => {
                    drop(owner);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                Err(error) => panic!("{error}"),
            }
        }
    })
    .await
    .unwrap();
    // The first team remains in TEAMS. Its idle strong Arc does not pin ownership.
    assert!(
        released_owner.upgrade().is_none(),
        "the real worker/provider/sink references must all finish, not only active removal"
    );
    assert!(find(&team.0.root, &team.0.owner).is_some());
    let resumed = owner_fixture_process(&team, &url, "resume", 0)
        .spawn()
        .unwrap();
    let (mut second, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let continued = request(&mut second).await.to_string();
    assert!(continued.contains("explicit idle takeover"));
    assert!(continued.contains("first held assignment"));
    respond(&mut second, json!({"content":"remote takeover complete"})).await;
    let output = tokio::task::spawn_blocking(move || resumed.wait_with_output().unwrap())
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "idle takeover failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(team.resolve(&before.id).unwrap().attempt, 2);
}

async fn owned_process_fixture(
    team: &AgentTeam,
    id: &str,
    attempt: u32,
) -> Arc<crate::background::BackgroundProcess> {
    let child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "team::tests::owned_background_process_fixture",
            "--ignored",
            "--nocapture",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let process = crate::background::register_with_mailbox(
        child,
        "D2 owned identity fixture".into(),
        &team.0.root,
        &team.0.root,
        crate::background::BackgroundOwner {
            run_id: format!("{}:{id}:{attempt}", team.0.owner),
            process_id: id.into(),
            write_key: format!("{}:{id}", team.0.owner),
        },
        crate::managed::ManagedSnapshot::capture(&team.0.root),
        false,
        None,
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !process.output().contains("D2_BACKGROUND_READY") {
            assert!(process.is_running());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    process
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_db_different_parents_same_call_id_keep_model_and_cleanup_independent() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let root = project();
    let first_team = team(root.clone(), &url, "parent-session-a");
    let second_team = team(root, &url, "parent-session-b");
    let id = "call_1";
    first_team
        .command(id, json!({"agent":"plan","prompt":"parent A assignment"}))
        .await
        .unwrap();
    let (mut first, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(request(&mut first)
        .await
        .to_string()
        .contains("parent A assignment"));
    second_team
        .command(id, json!({"agent":"plan","prompt":"parent B assignment"}))
        .await
        .unwrap();
    let (mut second, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(request(&mut second)
        .await
        .to_string()
        .contains("parent B assignment"));
    assert_eq!(
        first_team.0.store.state_path(),
        second_team.0.store.state_path()
    );
    assert_eq!(
        try_child_owner(&first_team.0.store, id)
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        try_child_owner(&second_team.0.store, id)
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    let first_process = owned_process_fixture(&first_team, id, 1).await;
    let second_process = owned_process_fixture(&second_team, id, 1).await;
    let before = second_team.resolve(id).unwrap();
    first_team.stop(id).unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        first_team.command("", json!({"action":"wait","id":id})),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!crate::shell::process_alive(first_process.pid().unwrap()));
    assert!(second_process.is_running());
    assert!(crate::shell::process_alive(second_process.pid().unwrap()));
    assert_eq!(
        serde_json::to_value(second_team.resolve(id).unwrap()).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    assert!(try_child_owner(&first_team.0.store, id).is_ok());
    assert_eq!(
        try_child_owner(&second_team.0.store, id)
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    first_team
        .ui_command(json!({"action":"resume","id":id,"prompt":"parent A explicit second attempt"}))
        .await
        .unwrap();
    let (mut third, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let routed = request(&mut third).await.to_string();
    assert!(routed.contains("parent A explicit second attempt"));
    assert!(!routed.contains("parent B assignment"));
    assert_eq!(first_team.resolve(id).unwrap().attempt, 2);
    assert_eq!(second_team.resolve(id).unwrap().attempt, 1);
    respond(&mut third, json!({"content":"A complete"})).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        first_team.command("", json!({"action":"wait","id":id})),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(second_process.is_running());
    assert!(crate::shell::process_alive(second_process.pid().unwrap()));
    second_team.stop(id).unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        second_team.command("", json!({"action":"wait","id":id})),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!crate::shell::process_alive(second_process.pid().unwrap()));
    drop(first);
    drop(second);
}

#[tokio::test]
async fn worker_registration_unwind_releases_child_but_late_callback_keeps_owner() {
    let team = answered_question_team("worker-unwind-owner");
    let (job, worker) = fixture_worker(&team);
    let released_owner = Arc::downgrade(&worker.owner);
    team.0
        .store
        .checkpoint(&job.id, &[Message::user_text("retained history")])
        .unwrap();
    let late_callback = team.transcript_sink(&job.id, &worker, job.attempt);
    let registration = WorkerRegistration {
        team: team.clone(),
        id: job.id.clone(),
        worker: worker.clone(),
    };
    drop(worker);
    let exited = tokio::spawn(async move {
        let _registration = registration;
        panic!("controlled worker unwind");
    })
    .await;
    assert!(exited.is_err());
    assert!(team.0.active.lock().unwrap().is_empty());
    late_callback(
        &job.id,
        json!({"messages":[Message::user_text("stale callback overwrite")]}),
    );
    assert!(
        serde_json::to_string(&team.0.store.history(&job.id).unwrap())
            .unwrap()
            .contains("retained history")
    );
    let error = try_child_owner(&team.0.store, &job.id)
        .err()
        .expect("late callback still owns child");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    assert!(released_owner.upgrade().is_some());
    drop(late_callback);
    assert!(released_owner.upgrade().is_none());
    let inspected = store_for_inspection(&team.0.root, &team.0.owner).unwrap();
    // This team still exists locally; explicitly exercise orphan recovery after
    // the final callback reference releases, without dropping the team registry.
    recover_interrupted(&inspected).unwrap();
    assert_eq!(inspected.get(&job.id).unwrap().state, "interrupted");
    assert!(try_child_owner(&team.0.store, &job.id).is_ok());
}

#[test]
fn attached_team_does_not_retain_parent_round_owner_in_transcript_callbacks() {
    let original = team(project(), "http://127.0.0.1:9/v1", "parent-callback-owner");
    let parent =
        Arc::new(try_acquire(original.0.store.state_path(), "parent-round-fixture").unwrap());
    let released = Arc::downgrade(&parent);
    let mut runtime = original.0.runtime.lock().unwrap().clone();
    let sink_owner = parent.clone();
    runtime.transcript_sink = Some(Arc::new(move |_, _| {
        let _owner = &sink_owner;
    }));
    runtime.transcript_provider = Some(Arc::new(move |_| {
        let _owner = &parent;
        Some(vec![])
    }));
    let config = original.0.config.lock().unwrap().clone();
    let attached = AgentTeam::attach(
        config,
        original.0.ctx.clone(),
        runtime,
        original.0.client.clone(),
        None,
    )
    .unwrap();
    assert!(Arc::ptr_eq(&attached.0, &original.0));
    assert!(attached.0.runtime.lock().unwrap().transcript_sink.is_none());
    assert!(attached
        .0
        .runtime
        .lock()
        .unwrap()
        .transcript_provider
        .is_none());
    assert!(
        released.upgrade().is_none(),
        "a cached team cannot pin its parent's round owner"
    );
    assert!(try_acquire(original.0.store.state_path(), "parent-round-fixture").is_ok());
}

#[test]
#[ignore = "owned background process fixture"]
fn owned_background_process_fixture() {
    use std::io::Write;
    println!("D2_BACKGROUND_READY");
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finishing_callback_panic_reaps_real_process_before_last_owner_release() {
    let _serial = crate::background::tests::serial().lock().await;
    let _fence = crate::background::tests::fence_guard();
    let team = answered_question_team("finishing-panic-owner");
    let (job, worker) = fixture_worker(&team);
    let released_owner = Arc::downgrade(&worker.owner);
    let baseline = crate::managed::ManagedSnapshot::capture(&team.0.root);
    let child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "team::tests::owned_background_process_fixture",
            "--ignored",
            "--nocapture",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    // Keep the original Windows process object alive until all PID assertions finish.
    // Otherwise parallel process launches can reuse its PID after Child::wait drops it.
    #[cfg(windows)]
    let _process_identity = {
        // SAFETY: child owns this valid handle while it is borrowed and duplicated.
        let handle = unsafe {
            std::os::windows::io::BorrowedHandle::borrow_raw(child.raw_handle().unwrap())
        };
        handle.try_clone_to_owned().unwrap()
    };
    let process = crate::background::register_with_mailbox(
        child,
        "D2 owned test fixture".into(),
        &team.0.root,
        &team.0.root,
        crate::background::BackgroundOwner {
            run_id: format!("{}:{}:{}", team.0.owner, job.id, job.attempt),
            process_id: job.id.clone(),
            write_key: "finishing-panic".into(),
        },
        baseline,
        false,
        None,
    )
    .await
    .unwrap();
    let pid = process.pid().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !process.output().contains("D2_BACKGROUND_READY") {
            assert!(process.is_running());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let (reached, final_reconcile) = tokio::sync::oneshot::channel();
    let reached = Arc::new(Mutex::new(Some(reached)));
    let (release, barrier) = std::sync::mpsc::channel();
    let barrier = Mutex::new(barrier);
    let background_id = process.id.clone();
    crate::background::set_final_guard_hook(Some(Arc::new(move |id| {
        if id == background_id {
            if let Some(reached) = reached.lock().unwrap().take() {
                let _ = reached.send(());
                let _ = barrier.lock().unwrap().recv();
            }
        }
    })));
    struct ResetHook;
    impl Drop for ResetHook {
        fn drop(&mut self) {
            crate::background::set_final_guard_hook(None);
        }
    }
    let _hook = ResetHook;
    team.0
        .store
        .update(&job.id, |j| j.state = "done".into())
        .unwrap();
    team.queue_message(
        &job.id,
        "main",
        "explicit continuation before settlement",
        Some("panic-continuation".into()),
    )
    .unwrap();
    let (seen, observed) = std::sync::mpsc::channel();
    let root = team.0.root.clone();
    let id = job.id.clone();
    *team.0.event.lock().unwrap() = Some(Arc::new(move |job| {
        if job.state == "queued" {
            let store = TeamStore::open(&root, &job.owner).unwrap();
            let busy = try_child_owner(&store, &id).err().unwrap().kind()
                == std::io::ErrorKind::WouldBlock;
            seen.send((busy, crate::shell::process_alive(pid))).unwrap();
            panic!("controlled finishing event panic");
        }
    }));
    // Failed work first reaps and joins the real process/last managed reconcile.
    // Its accepted continuation then emits queued and panics during settlement.
    let finishing_team = team.clone();
    let finishing_id = job.id.clone();
    let registration = WorkerRegistration {
        team: team.clone(),
        id: job.id.clone(),
        worker: worker.clone(),
    };
    drop(worker);
    let finishing = tokio::spawn(async move {
        let _registration = registration;
        let result = Err(anyhow::anyhow!("controlled runner failure"));
        finishing_team
            .settle_worker(&finishing_id, &_registration.worker, &result, &[])
            .await;
    });
    tokio::time::timeout(Duration::from_secs(5), final_reconcile)
        .await
        .unwrap()
        .unwrap();
    assert!(!crate::shell::process_alive(pid));
    assert!(
        !finishing.is_finished(),
        "the final detached reconcile is not complete"
    );
    assert_eq!(
        try_child_owner(&team.0.store, &job.id)
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    release.send(()).unwrap();
    finishing.await.unwrap();
    assert_eq!(
        observed.recv().unwrap(),
        (true, false),
        "the event runs with ownership after the old writing process was reaped"
    );
    assert!(!process.is_running());
    assert!(
        !crate::shell::process_alive(pid),
        "cleanup must prove actual owned PID exit"
    );
    assert!(released_owner.upgrade().is_none());
    assert!(try_child_owner(&team.0.store, &job.id).is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_final_restore_rejects_admission_until_real_retry_succeeds() {
    let _serial = crate::background::tests::serial().lock().await;
    let _fence = crate::background::tests::fence_guard();
    let team = answered_question_team("restore-failure-owner");
    let (job, worker) = fixture_worker(&team);
    let path = team.0.root.join(".kanzei/project/requirements.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "original protected content").unwrap();
    let baseline = crate::managed::ManagedSnapshot::capture(&team.0.root);
    let child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "team::tests::owned_background_process_fixture",
            "--ignored",
            "--nocapture",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let process = crate::background::register_with_mailbox(
        child,
        "D2 restore failure fixture".into(),
        &team.0.root,
        &team.0.root,
        crate::background::BackgroundOwner {
            run_id: format!("{}:{}:{}", team.0.owner, job.id, job.attempt),
            process_id: job.id.clone(),
            write_key: "restore-failure".into(),
        },
        baseline,
        false,
        None,
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !process.output().contains("D2_BACKGROUND_READY") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let quarantine = team.0.root.join(".kanzei/quarantine");
    std::fs::write(&quarantine, "blocks evidence preservation").unwrap();
    std::fs::write(&path, "unquarantined candidate must survive failure").unwrap();
    let registration = WorkerRegistration {
        team: team.clone(),
        id: job.id.clone(),
        worker: worker.clone(),
    };
    team.settle_worker(
        &job.id,
        &worker,
        &Err(anyhow::anyhow!("controlled failed work")),
        &[],
    )
    .await;
    drop(registration);
    drop(worker);
    assert!(!process.is_running());
    assert!(!crate::shell::process_alive(process.pid().unwrap()));
    let failed = team.resolve(&job.id).unwrap();
    assert_eq!(failed.state, "failed");
    assert!(cleanup_failed(&failed), "{}", failed.latest);
    assert!(process.output().contains("回滚失败"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "unquarantined candidate must survive failure"
    );
    let error = team
        .queue_message(
            &job.id,
            "main",
            "explicit retry",
            Some("failed-restore-retry".into()),
        )
        .unwrap_err();
    assert!(error.to_string().contains("回滚失败"), "{error}");
    assert_eq!(
        serde_json::to_value(team.resolve(&job.id).unwrap()).unwrap(),
        serde_json::to_value(&failed).unwrap(),
        "failed admission cannot write a message or state"
    );
    assert!(team.0.active.lock().unwrap().is_empty());
    let mut remote = owner_fixture_process(
        &team,
        "http://127.0.0.1:9/v1",
        "cleanup-failed",
        failed.revision,
    );
    remote.env("D2_OWNER_FIXTURE_ID", &job.id);
    let output = tokio::task::spawn_blocking(move || remote.output().unwrap())
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_file(&quarantine).unwrap();
    team.queue_message(
        &job.id,
        "main",
        "explicit retry",
        Some("failed-restore-retry".into()),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "original protected content"
    );
    assert!(quarantine.is_dir());
    team.stop(&job.id).unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn failed_launch_state_write_does_not_leave_a_phantom_worker_and_retry_can_start() {
    let team = team(
        project(),
        "http://127.0.0.1:9/v1",
        "launch-write-failure-owner",
    );
    let db = rusqlite::Connection::open(kanzei_core::project_state_path(&team.0.root)).unwrap();
    db.execute_batch("CREATE TRIGGER reject_queued_write BEFORE UPDATE ON agent_team_jobs BEGIN SELECT RAISE(ABORT, 'injected queued write failure'); END;").unwrap();
    let error = team
        .command(
            "failed-launch-child",
            json!({"agent":"plan","prompt":"initial work"}),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("injected queued write failure"));
    assert!(
        team.0.active.lock().unwrap().is_empty(),
        "a failed state write must not register a worker that was never spawned"
    );
    assert_eq!(team.resolve("failed-launch-child").unwrap().state, "queued");
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        team.command("", json!({"action":"wait","id":"failed-launch-child"})),
    )
    .await
    .expect("a launch that returned Err must not leave wait pending")
    .unwrap_err();
    assert!(error.to_string().contains("没有运行中的执行者"));
    db.execute_batch("DROP TRIGGER reject_queued_write;")
        .unwrap();
    team.ui_command(
        json!({"action":"resume","id":"failed-launch-child","prompt":"explicit retry"}),
    )
    .await
    .unwrap();
    assert!(team
        .0
        .active
        .lock()
        .unwrap()
        .contains_key("failed-launch-child"));
    team.stop_all();
    tokio::time::timeout(
        Duration::from_secs(3),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        team.resolve("failed-launch-child").unwrap().state,
        "stopped"
    );
}

#[tokio::test]
async fn failed_completion_state_write_releases_owner_and_wait_returns_error() {
    let team = answered_question_team("completion-write-failure-owner");
    let (job, token) = fixture_worker(&team);
    let db = rusqlite::Connection::open(kanzei_core::project_state_path(&team.0.root)).unwrap();
    db.execute_batch("CREATE TRIGGER reject_final_state BEFORE UPDATE ON agent_team_jobs BEGIN SELECT RAISE(ABORT, 'injected final state failure'); END;").unwrap();
    assert!(team.finish_worker(
        &job.id,
        &token,
        &Err(anyhow::anyhow!("worker failed")),
        true,
        &[]
    ));
    assert!(team.0.active.lock().unwrap().is_empty());
    assert_eq!(team.resolve(&job.id).unwrap().state, "running");
    drop(token);
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        team.command("", json!({"action":"wait","id":job.id})),
    )
    .await
    .expect("failed persistence cannot keep a completed worker's wait alive")
    .unwrap_err();
    assert!(error.to_string().contains("没有运行中的执行者"));
    db.execute_batch("DROP TRIGGER reject_final_state;")
        .unwrap();
    team.stop(&job.id).unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopped");
    team.ui_command(
        json!({"action":"resume","id":job.id,"prompt":"explicit retry after save failure"}),
    )
    .await
    .unwrap();
    team.stop_all();
    tokio::time::timeout(
        Duration::from_secs(3),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopped");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wait_cannot_observe_new_job_before_its_worker_is_registered() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let team = team(
        project(),
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        "spawn-wait-owner",
    );
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let first = std::sync::atomic::AtomicBool::new(true);
    *team.0.event.lock().unwrap() = Some(Arc::new(move |job| {
        if job.id == "spawn-wait-child" && first.swap(false, std::sync::atomic::Ordering::SeqCst) {
            entered_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
        }
    }));
    let spawning_team = team.clone();
    let spawning = tokio::spawn(async move {
        spawning_team
            .command(
                "spawn-wait-child",
                json!({"agent":"plan","prompt":"normal dispatch"}),
            )
            .await
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let waiting_team = team.clone();
    let mut waiting = tokio::spawn(async move {
        waiting_team
            .command("", json!({"action":"wait","id":"spawn-wait-child"}))
            .await
    });
    let blocked = tokio::time::timeout(Duration::from_millis(100), &mut waiting)
        .await
        .is_err();
    release_tx.send(()).unwrap();
    spawning.await.unwrap().unwrap();
    assert!(
        blocked,
        "a valid dispatch cannot look like a failed, ownerless launch"
    );
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    request(&mut stream).await;
    respond(&mut stream, json!({"content":"normal dispatch finished"})).await;
    tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(team.resolve("spawn-wait-child").unwrap().state, "done");
}

#[test]
fn concurrent_registration_creates_one_state_owner() {
    let original = answered_question_team("registry-owner");
    TEAMS
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .remove(&key(&original.0.root, &original.0.owner));
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let first_team = original.clone();
    let first = std::thread::spawn(move || {
        get_or_register(&first_team.0.root, &first_team.0.owner, || {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(first_team.clone())
        })
        .unwrap()
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let creations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let second_creations = creations.clone();
    let second_team = original.clone();
    let second = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = get_or_register(&second_team.0.root, &second_team.0.owner, || {
            second_creations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(second_team.clone())
        })
        .unwrap();
        done_tx.send(()).unwrap();
        result
    });
    started_rx.recv().unwrap();
    let blocked = done_rx.recv_timeout(Duration::from_millis(100)).is_err();
    release_tx.send(()).unwrap();
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    assert!(
        blocked,
        "a second attach must wait for the first registration"
    );
    assert_eq!(creations.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(Arc::ptr_eq(&first.0, &second.0));
    let reattached = team(
        original.0.root.clone(),
        "http://127.0.0.1:9/v1",
        &original.0.owner,
    );
    assert!(Arc::ptr_eq(&first.0, &reattached.0));
}

#[tokio::test]
async fn stop_cancels_worker_even_when_pending_question_record_is_corrupt() {
    let team = answered_question_team("stop-corrupt-question-owner");
    let (job, token) = fixture_worker(&team);
    let questions = team.0.root.join(".kanzei/runtime/questions");
    std::fs::create_dir_all(&questions).unwrap();
    std::fs::write(questions.join("99.json"), "corrupt question record").unwrap();
    let error = team.stop(&job.id).unwrap_err();
    assert!(error.to_string().contains("问题记录损坏"));
    assert!(
        token.is_cancelled(),
        "question cleanup failure must not keep the worker alive"
    );
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopping");
    assert!(matches!(
        team.begin_ask(&job, &token, background_question(100), None, None)
            .await,
        kanzei_core::AskResponse::Cancelled
    ));
    assert!(team.finish_worker(
        &job.id,
        &token,
        &Err(anyhow::anyhow!("cancelled")),
        true,
        &[]
    ));
    drop(token);
    team.command("", json!({"action":"wait"})).await.unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopped");
}

#[tokio::test]
async fn cancelled_cleanup_does_not_relaunch_queued_followup_after_stop_save_failure() {
    let team = answered_question_team("stop-queued-save-failure-owner");
    let (job, token) = fixture_worker(&team);
    // A worker has saved done but has not released its token. A legitimate UI
    // continuation is accepted in this exact window and writes queued.
    team.0
        .store
        .update(&job.id, |j| j.state = "done".into())
        .unwrap();
    team.ui_command(json!({"action":"resume","id":job.id,"prompt":"accepted continuation"}))
        .await
        .unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().state, "queued");
    let db = rusqlite::Connection::open(kanzei_core::project_state_path(&team.0.root)).unwrap();
    db.execute_batch("CREATE TRIGGER reject_cancel_state BEFORE UPDATE ON agent_team_jobs WHEN json_extract(NEW.job, '$.state') IN ('stopping','stopped') BEGIN SELECT RAISE(ABORT, 'injected cancel state failure'); END;").unwrap();
    assert!(team
        .stop(&job.id)
        .unwrap_err()
        .to_string()
        .contains("injected cancel state failure"));
    assert!(token.is_cancelled());
    assert!(team.finish_worker(&job.id, &token, &Ok(()), true, &[]));
    assert_eq!(team.resolve(&job.id).unwrap().state, "queued");
    assert!(
        team.0.active.lock().unwrap().is_empty(),
        "failed stop persistence must not replay a cancelled worker's queue"
    );
    db.execute_batch("DROP TRIGGER reject_cancel_state;")
        .unwrap();
    drop(token);
    team.stop(&job.id).unwrap();
    team.command("", json!({"action":"wait"})).await.unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopped");
}

#[tokio::test]
async fn stop_all_cancels_known_worker_even_when_job_store_cannot_be_read() {
    let team = answered_question_team("stop-unreadable-store-owner");
    let (job, token) = fixture_worker(&team);
    let db = rusqlite::Connection::open(kanzei_core::project_state_path(&team.0.root)).unwrap();
    db.execute_batch("ALTER TABLE agent_team_jobs RENAME TO hidden_jobs;")
        .unwrap();
    team.stop_all();
    assert!(
        token.is_cancelled(),
        "a known worker's cancellation cannot depend on reading its SQL row"
    );
    assert!(team.finish_worker(
        &job.id,
        &token,
        &Err(anyhow::anyhow!("cancelled")),
        true,
        &[]
    ));
    assert!(team.0.active.lock().unwrap().is_empty());
    assert!(team.command("", json!({"action":"wait"})).await.is_err());
    db.execute_batch("ALTER TABLE hidden_jobs RENAME TO agent_team_jobs;")
        .unwrap();
    drop(token);
    team.stop(&job.id).unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopped");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_all_cancels_real_worker_when_state_write_fails_and_does_not_relaunch() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let team = team(
        project(),
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        "stop-write-failure-owner",
    );
    team.command(
        "stop-failure-child",
        json!({"agent":"plan","prompt":"first assignment"}),
    )
    .await
    .unwrap();
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    request(&mut stream).await;
    team.ui_command(
        json!({"action":"resume","id":"stop-failure-child","prompt":"queued before stop"}),
    )
    .await
    .unwrap();
    let job = team.resolve("stop-failure-child").unwrap();
    let token = team.0.active.lock().unwrap().get(&job.id).unwrap().clone();
    let db = rusqlite::Connection::open(kanzei_core::project_state_path(&team.0.root)).unwrap();
    db.execute_batch("CREATE TRIGGER reject_stop_state BEFORE UPDATE ON agent_team_jobs BEGIN SELECT RAISE(ABORT, 'injected stop state failure'); END;").unwrap();
    team.stop_all();
    assert!(
        token.is_cancelled(),
        "stop must cancel the real worker even if its SQL write fails"
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while !team.0.active.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancelled worker must exit despite persistent state save failure");
    assert_eq!(team.resolve(&job.id).unwrap().state, "running");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), listener.accept())
            .await
            .is_err(),
        "stop cannot replay the queue after cleanup save failure"
    );
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let router_calls = calls.clone();
    let router: TeamAsk = Arc::new(move |_, _| {
        router_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { kanzei_core::AskResponse::Answer("unexpected".into()) })
    });
    assert!(matches!(
        team.begin_ask(&job, &token, background_question(52), Some(&router), None)
            .await,
        kanzei_core::AskResponse::Cancelled
    ));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(team
        .worker_update(&job.id, &token, Some(job.attempt), |j| j.state =
            "running".into())
        .is_err());
    drop(token);
    assert!(team
        .command("", json!({"action":"wait","id":job.id}))
        .await
        .unwrap_err()
        .to_string()
        .contains("没有运行中的执行者"));
    db.execute_batch("DROP TRIGGER reject_stop_state;").unwrap();
    team.stop(&job.id).unwrap();
    team.ui_command(
        json!({"action":"resume","id":job.id,"prompt":"explicit dispatch after fault removed"}),
    )
    .await
    .unwrap();
    team.stop_all();
    tokio::time::timeout(
        Duration::from_secs(3),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopped");
}

#[test]
fn inspection_rechecks_registry_after_an_absent_snapshot_and_recovers_only_orphans() {
    let original = answered_question_team("inspection-owner");
    TEAMS
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .remove(&key(&original.0.root, &original.0.owner));
    assert!(find(&original.0.root, &original.0.owner).is_none());
    // Attach wins after the UI's absent snapshot but before recovery begins.
    let live = team(
        original.0.root.clone(),
        "http://127.0.0.1:9/v1",
        &original.0.owner,
    );
    let (job, token) = fixture_worker(&live);
    let inspected = store_for_inspection(&live.0.root, &live.0.owner).unwrap();
    assert_eq!(inspected.get(&job.id).unwrap().state, "running");
    assert!(Arc::ptr_eq(
        live.0.active.lock().unwrap().get(&job.id).unwrap(),
        &token
    ));
    // A process restart really has no owner; recovery preserves transcript and
    // never creates a worker or automatically replays its writes.
    live.0
        .store
        .checkpoint(&job.id, &[Message::user_text("retained history")])
        .unwrap();
    TEAMS
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .remove(&key(&live.0.root, &live.0.owner));
    // Losing an in-process registry entry is not proof that its OS owner died.
    assert_eq!(
        store_for_inspection(&live.0.root, &live.0.owner)
            .unwrap()
            .get(&job.id)
            .unwrap()
            .state,
        "running"
    );
    live.0.active.lock().unwrap().remove(&job.id);
    drop(token);
    let orphan = store_for_inspection(&live.0.root, &live.0.owner).unwrap();
    assert_eq!(orphan.get(&job.id).unwrap().state, "interrupted");
    assert_eq!(orphan.history(&job.id).unwrap().len(), 1);
    assert!(find(&live.0.root, &live.0.owner).is_none());
    let revision = orphan.get(&job.id).unwrap().revision;
    assert_eq!(
        store_for_inspection(&live.0.root, &live.0.owner)
            .unwrap()
            .get(&job.id)
            .unwrap()
            .revision,
        revision
    );
}

#[tokio::test]
async fn stop_after_success_before_worker_cleanup_finishes_stopped() {
    let team = answered_question_team("successful-stop-owner");
    let (job, token) = fixture_worker(&team);
    team.0
        .store
        .update(&job.id, |job| job.state = "done".into())
        .unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let finishing_team = team.clone();
    let finishing_id = job.id.clone();
    let finishing = std::thread::spawn(move || {
        let result = Ok(());
        ready_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        assert!(!finishing_team.finish_worker(&finishing_id, &token, &result, false, &[]));
        assert!(finishing_team.finish_worker(&finishing_id, &token, &result, true, &[]));
    });
    ready_rx.recv().unwrap();
    team.stop(&job.id).unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopping");
    release_tx.send(()).unwrap();
    finishing.join().unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopped");
    assert!(team.0.active.lock().unwrap().is_empty());
    tokio::time::timeout(
        Duration::from_secs(1),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cleanup_hands_queued_resume_to_one_new_worker_and_old_cleanup_cannot_remove_it() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixture = answered_question_team("cleanup-resume-owner");
    // Reattach updates the fixture's model route while preserving its state owner.
    let team = team(
        fixture.0.root.clone(),
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        &fixture.0.owner,
    );
    let (job, old) = fixture_worker(&team);
    team.0
        .store
        .update(&job.id, |job| job.state = "done".into())
        .unwrap();
    team.queue_message(
        &job.id,
        "main",
        "explicit follow-up",
        Some("explicit-follow-up".into()),
    )
    .unwrap();
    assert!(Arc::ptr_eq(
        team.0.active.lock().unwrap().get(&job.id).unwrap(),
        &old
    ));
    assert!(team.finish_worker(&job.id, &old, &Ok(()), false, &[]));
    let current = team.0.active.lock().unwrap().get(&job.id).unwrap().clone();
    assert!(!Arc::ptr_eq(&current, &old));
    assert!(team.finish_worker(
        &job.id,
        &old,
        &Err(anyhow::anyhow!("late old error")),
        true,
        &[]
    ));
    assert!(Arc::ptr_eq(
        team.0.active.lock().unwrap().get(&job.id).unwrap(),
        &current
    ));
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(request(&mut stream)
        .await
        .to_string()
        .contains("explicit follow-up"));
    respond(&mut stream, json!({"content":"follow-up completed"})).await;
    drop(old);
    drop(current);
    team.command("", json!({"action":"wait"})).await.unwrap();
    assert_eq!(team.resolve(&job.id).unwrap().attempt, 2);
    assert_eq!(team.resolve(&job.id).unwrap().state, "done");
}

#[tokio::test]
async fn ask_registration_and_stop_share_lifecycle_and_late_asks_do_not_persist() {
    let team = answered_question_team("ask-stop-owner");
    let (job, token) = fixture_worker(&team);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let root = team.0.root.clone();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let called = calls.clone();
    let router: TeamAsk = Arc::new(move |job, _| {
        called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        entered_tx.send(()).unwrap();
        release_rx.lock().unwrap().recv().unwrap();
        kanzei_harness::pending_question::save(
            &root,
            &json!({
                "payload":{"id":17,"sessionId":job.owner,"agentId":job.id},
                "callback_id":"callback-17","state":"pending"
            }),
        )
        .unwrap();
        Box::pin(std::future::pending())
    });
    let asking_team = team.clone();
    let asking_job = job.clone();
    let asking_token = token.clone();
    let asking_router = router.clone();
    let asking = std::thread::spawn(move || {
        asking_team.begin_ask(
            &asking_job,
            &asking_token,
            background_question(17),
            Some(&asking_router),
            None,
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    let stopping_team = team.clone();
    let stopping_id = job.id.clone();
    let stopping = std::thread::spawn(move || {
        stopping_team.stop(&stopping_id).unwrap();
        stop_tx.send(()).unwrap();
    });
    let blocked = stop_rx.recv_timeout(Duration::from_millis(100)).is_err();
    release_tx.send(()).unwrap();
    let waiting_reply = asking.join().unwrap();
    stopping.join().unwrap();
    assert!(
        blocked,
        "stop must wait until synchronous question registration ends"
    );
    // The reply can remain pending while stop completes: no lock spans its wait.
    drop(waiting_reply);
    assert_eq!(
        kanzei_harness::pending_question::get(&team.0.root, 17).unwrap()["state"],
        "cancelled"
    );
    assert!(matches!(
        team.begin_ask(&job, &token, background_question(18), Some(&router), None)
            .await,
        kanzei_core::AskResponse::Cancelled
    ));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(team
        .worker_update(&job.id, &token, Some(job.attempt), |j| j.state =
            "running".into())
        .is_err());
    assert_eq!(team.resolve(&job.id).unwrap().state, "stopping");
    assert!(team.finish_worker(&job.id, &token, &Ok(()), true, &[]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn timed_out_worker_hands_off_new_explicit_resume_without_replaying_original_messages() {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let team = team(
        root,
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        "timeout-resume-owner",
    );
    team.0.runtime.lock().unwrap().timeout_secs = 2;
    team.command(
        "timed-out-child",
        json!({"agent":"plan","prompt":"first request"}),
    )
    .await
    .unwrap();
    let (mut first, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    request(&mut first).await;
    // A successful UI continuation is queued while the first real model request
    // is held open; it must survive the worker's subsequent timeout Err.
    team.ui_command(json!({"action":"resume","id":"timed-out-child","prompt":"explicit follow-up after timeout"})).await.unwrap();
    let (mut second, _) = tokio::time::timeout(Duration::from_secs(6), listener.accept())
        .await
        .unwrap()
        .expect("accepted continuation must launch after timeout cleanup");
    let continued = request(&mut second).await.to_string();
    assert!(continued.contains("explicit follow-up after timeout"));
    assert!(
        continued.contains("first request"),
        "the checkpointed original assignment must survive timeout"
    );
    respond(&mut second, json!({"content":"continued successfully"})).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(team.resolve("timed-out-child").unwrap().state, "done");
    assert_eq!(team.resolve("timed-out-child").unwrap().attempt, 2);
    // Stop-in-progress is explicitly rejected; once stop is complete, the same
    // public UI resume is a valid new dispatch and keeps its new worker.
    team.stop("timed-out-child").unwrap();
    assert_eq!(team.resolve("timed-out-child").unwrap().state, "stopped");
    team.ui_command(
        json!({"action":"resume","id":"timed-out-child","prompt":"explicit follow-up after stop"}),
    )
    .await
    .unwrap();
    let (mut third, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(request(&mut third)
        .await
        .to_string()
        .contains("explicit follow-up after stop"));
    respond(&mut third, json!({"content":"resumed after stop"})).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(team.resolve("timed-out-child").unwrap().state, "done");
    assert_eq!(team.resolve("timed-out-child").unwrap().attempt, 3);

    let orphan = answered_question_team("original-failure-owner");
    let (job, token) = fixture_worker(&orphan);
    orphan
        .0
        .store
        .update(&job.id, |job| {
            job.state = "queued".into();
            job.messages.push(AgentMessage {
                id: "original-queued".into(),
                from: "main".into(),
                text: "initial work".into(),
                state: "queued".into(),
                at: now(),
            });
        })
        .unwrap();
    assert!(orphan.finish_worker(
        &job.id,
        &token,
        &Err(anyhow::anyhow!("preparation failed")),
        false,
        &["original-queued".into()]
    ));
    assert_eq!(orphan.resolve(&job.id).unwrap().state, "failed");
    assert!(
        orphan.0.active.lock().unwrap().is_empty(),
        "original pending input must not trigger an automatic failure loop"
    );
}

#[tokio::test]
async fn ask_rejects_retired_token_and_old_attempt_but_admits_current_attempt() {
    let team = answered_question_team("ask-identity-owner");
    let (job, old) = fixture_worker(&team);
    let current = Arc::new(ChildWorker {
        cancel: CancellationToken::new(),
        owner: old.owner.clone(),
    });
    team.0
        .active
        .lock()
        .unwrap()
        .insert(job.id.clone(), current.clone());
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let called = calls.clone();
    let router: TeamAsk = Arc::new(move |_, _| {
        called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { kanzei_core::AskResponse::Answer("current answer".into()) })
    });
    assert!(matches!(
        team.begin_ask(&job, &old, background_question(20), Some(&router), None)
            .await,
        kanzei_core::AskResponse::Cancelled
    ));
    let next = team
        .0
        .store
        .update(&job.id, |job| job.attempt += 1)
        .unwrap();
    assert!(matches!(
        team.begin_ask(&job, &current, background_question(21), Some(&router), None)
            .await,
        kanzei_core::AskResponse::Cancelled
    ));
    assert!(
        matches!(team.begin_ask(&next, &current, background_question(22), Some(&router), None).await, kanzei_core::AskResponse::Answer(answer) if answer == "current answer")
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(team.finish_worker(&job.id, &current, &Ok(()), false, &[]));
}

#[tokio::test]
async fn adoption_serializes_done_check_apply_and_commit_against_resume() {
    let team = answered_question_team("adopt-resume-owner");
    let (tree, base) = workspace::prepare(&team.0.root, &fresh_id()).unwrap();
    std::fs::write(tree.join("adopted.txt"), "candidate bytes").unwrap();
    let (head, _) = workspace::result(&tree, &base, "adopted").unwrap();
    team.0
        .store
        .update("question-child", |job| {
            job.base = Some(base);
            job.head = Some(head);
        })
        .unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let adopting_team = team.clone();
    let adopting = std::thread::spawn(move || {
        adopting_team.adopt_with("question-child", |root, base, head| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            workspace::adopt(root, base, head)
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (message_tx, message_rx) = std::sync::mpsc::channel();
    let messaging_team = team.clone();
    let runtime = tokio::runtime::Handle::current();
    let messaging = std::thread::spawn(move || {
        let _runtime = runtime.enter();
        let result = messaging_team.queue_message("question-child", "main", "new attempt", None);
        message_tx.send(result.is_ok()).unwrap();
        result
    });
    let blocked = message_rx.recv_timeout(Duration::from_millis(100)).is_err();
    release_tx.send(()).unwrap();
    assert_eq!(adopting.join().unwrap().unwrap().outcome, "adopted");
    let message = messaging.join().unwrap();
    assert!(
        blocked,
        "resume must wait for adoption's check/apply/commit"
    );
    assert!(message.is_err());
    assert_eq!(team.resolve("question-child").unwrap().state, "done");
    assert!(team.0.active.lock().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(team.0.root.join("adopted.txt")).unwrap(),
        "candidate bytes"
    );
}

fn answered_question_team(owner: &str) -> AgentTeam {
    let root = project();
    let team = team(root.clone(), "http://127.0.0.1:9/v1", owner);
    let job: AgentJob = serde_json::from_value(json!({
        "id":"question-child","owner":owner,"project_dir":root,"process_id":null,
        "name":"question-child","role":"plan","model":"fast","model_tier":"fast",
        "prompt":"original","schema":null,"state":"done","outcome":"candidate",
        "latest":"done","result":"result","worktree":null,"base":null,"head":null,
        "files":[],"depends_on":[],"created_at":1,"updated_at":1,"attempt":1,"revision":1,
        "reported":1,"messages":[],"trace":[],"trace_seq":0,"notify_on_completion":false
    }))
    .unwrap();
    team.0.store.insert(&job, &[]).unwrap();
    kanzei_harness::pending_question::save(
        &root,
        &json!({
            "payload":{"id":1,"sessionId":owner,"agentId":"question-child"},
            "callback_id":"question-callback","state":"pending"
        }),
    )
    .unwrap();
    kanzei_harness::pending_question::answer(&root, 1, "yes", "reply").unwrap();
    team
}

fn question_notice() -> AsyncNotice {
    AsyncNotice {
        id: "question-callback".into(),
        text: "answer yes".into(),
    }
}

#[tokio::test]
async fn durable_question_callbacks_do_not_revive_stopped_children_but_explicit_resume_does() {
    let team = answered_question_team("question-stop-owner");
    assert_eq!(team.resolve("question-child").unwrap().state, "done");
    // This is the snapshot the former app precheck trusted before async assembly.
    let snapshot = kanzei_harness::pending_question::get(&team.0.root, 1).unwrap();
    team.stop("question-child").unwrap();
    assert_eq!(snapshot["state"], "answered");
    assert!(team
        .question_reply("question-child", 1, question_notice())
        .is_err());
    assert!(team.0.active.lock().unwrap().is_empty());
    assert_eq!(team.resolve("question-child").unwrap().state, "stopped");
    team.ui_command(json!({"action":"resume","id":"question-child","prompt":"explicit new work"}))
        .await
        .unwrap();
    assert!(team.0.active.lock().unwrap().contains_key("question-child"));
    team.stop_all();
    team.command("", json!({"action":"wait"})).await.unwrap();
    assert!(team
        .question_reply("question-child", 1, question_notice())
        .is_err());
}

#[tokio::test]
async fn current_question_reply_keeps_callback_identity_and_retries_admit_one_message() {
    let team = answered_question_team("question-current-owner");
    team.question_reply("question-child", 1, question_notice())
        .unwrap();
    team.question_reply("question-child", 1, question_notice())
        .unwrap();
    let job = team.resolve("question-child").unwrap();
    assert_eq!(job.messages.len(), 1);
    assert_eq!(job.messages[0].id, "question-callback");
    assert_eq!(job.messages[0].from, "callback");
    team.stop_all();
    team.command("", json!({"action":"wait"})).await.unwrap();
}

#[tokio::test]
async fn stop_all_serializes_with_a_question_callback_between_admission_and_launch() {
    use std::sync::mpsc;
    let team = answered_question_team("question-race-owner");
    let (entered, entering) = mpsc::channel();
    let (resume, resuming) = mpsc::channel();
    let resuming = Mutex::new(resuming);
    let once = std::sync::atomic::AtomicBool::new(false);
    *team.0.event.lock().unwrap() = Some(Arc::new(move |job| {
        if job.state == "queued"
            && job.messages.iter().any(|m| m.from == "callback")
            && !once.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            entered.send(()).unwrap();
            resuming.lock().unwrap().recv().unwrap();
        }
    }));
    let callback_team = team.clone();
    let executor = tokio::runtime::Handle::current();
    let callback = std::thread::spawn(move || {
        let _executor = executor.enter();
        callback_team
            .question_reply("question-child", 1, question_notice())
            .unwrap();
    });
    entering.recv().unwrap();
    kanzei_harness::pending_question::cancel_owner(&team.0.root, &team.0.owner, None).unwrap();
    let stopping_team = team.clone();
    let (stopping, started) = mpsc::channel();
    let (stopped, finished) = mpsc::channel();
    let stop = std::thread::spawn(move || {
        stopping.send(()).unwrap();
        stopping_team.stop_all();
        stopped.send(()).unwrap();
    });
    started.recv().unwrap();
    // The callback owns lifecycle at a known event hook, before launch_locked.
    let completion = finished.recv_timeout(Duration::from_millis(100));
    resume.send(()).unwrap();
    callback.join().unwrap();
    stop.join().unwrap();
    assert!(
        matches!(completion, Err(mpsc::RecvTimeoutError::Timeout)),
        "stop must wait for callback admission before selecting active workers"
    );
    assert!(team.0.active.lock().unwrap()["question-child"].is_cancelled());
    team.command("", json!({"action":"wait"})).await.unwrap();
    assert_eq!(team.resolve("question-child").unwrap().state, "stopped");
    assert!(team
        .question_reply("question-child", 1, question_notice())
        .is_err());
}

#[tokio::test]
async fn model_status_queries_are_small_and_history_is_explicitly_paged() {
    let team = team(project(), "http://127.0.0.1:9/v1", "summary-owner");
    let history = (0..25)
        .map(|i| Message::user_text(format!("HISTORY_PAGE_{i}")))
        .collect::<Vec<_>>();
    let job = AgentJob {
        id: "large-record".into(),
        owner: "summary-owner".into(),
        project_dir: "project".into(),
        process_id: None,
        name: "Explore".into(),
        role: "explore".into(),
        model: "mock".into(),
        model_tier: "fast".into(),
        prompt: format!("PRIVATE_PROMPT {}", "p".repeat(100_000)),
        schema: None,
        state: "done".into(),
        outcome: "candidate".into(),
        latest: "r".repeat(100_000),
        result: "字".repeat(100_000),
        worktree: None,
        base: None,
        head: None,
        files: Vec::new(),
        depends_on: Vec::new(),
        created_at: 10,
        updated_at: 20,
        attempt: 1,
        revision: 2,
        reported: 0,
        messages: vec![AgentMessage {
            id: "old-message".into(),
            from: "main".into(),
            text: "PRIVATE_MESSAGE".repeat(10_000),
            state: "processed".into(),
            at: 15,
        }],
        trace: vec![json!({"preview":"PRIVATE_TRACE".repeat(10_000)})],
        trace_seq: 1,
        notify_on_completion: false,
        replaces: None,
        notified: 0,
    };
    team.0.store.insert(&job, &history).unwrap();
    let summary = team
        .command("", json!({"action":"get","id":job.id}))
        .await
        .unwrap();
    assert!(summary.get("history").is_none());
    assert!(summary["job"].get("messages").is_none());
    assert!(summary["job"].get("trace").is_none());
    assert_eq!(
        summary["job"]["result"].as_str().unwrap().chars().count(),
        2400
    );
    assert_eq!(summary["job"]["result_chars"], 100_000);
    assert_eq!(summary["job"]["result_truncated"], true);
    assert!(summary.to_string().len() < 9000);
    let list = team.command("", json!({"action":"list"})).await.unwrap();
    assert_eq!(list["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(
        list["jobs"][0]["result"].as_str().unwrap().chars().count(),
        400
    );
    assert!(!list.to_string().contains("PRIVATE_"));
    assert!(!list.to_string().contains("HISTORY_PAGE"));
    let first = team
        .command(
            "",
            json!({"action":"get","id":job.id,"view":"history","limit":2}),
        )
        .await
        .unwrap();
    assert_eq!(first["history"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(first["history"]["next_offset"], 2);
    let second = team
        .command(
            "",
            json!({"action":"get","id":job.id,"view":"history","offset":2,"limit":2}),
        )
        .await
        .unwrap();
    assert!(second["history"]["items"]
        .to_string()
        .contains("HISTORY_PAGE_2"));
    assert!(!second["history"]["items"]
        .to_string()
        .contains("HISTORY_PAGE_0"));
    let capped = team
        .command(
            "",
            json!({"action":"get","id":job.id,"view":"history","limit":100_000}),
        )
        .await
        .unwrap();
    assert_eq!(capped["history"]["items"].as_array().unwrap().len(), 20);
    let ui = team
        .ui_command(json!({"action":"get","id":job.id}))
        .await
        .unwrap();
    assert_eq!(ui["history"].as_array().unwrap().len(), 25);
    assert!(ui["job"]["prompt"]
        .as_str()
        .unwrap()
        .contains("PRIVATE_PROMPT"));
    let full = team
        .command("", json!({"action":"get","id":job.id,"view":"result"}))
        .await
        .unwrap();
    assert_eq!(full["result"].as_str().unwrap().chars().count(), 100_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn collect_is_nonblocking_while_two_real_children_run_and_notify_the_parent() {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let team = team(
        root,
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        "async-owner",
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    team.set_mailbox(AsyncMailbox::new(move |n| {
        tx.send(n).map_err(|e| e.to_string())
    }));
    team.command(
        "async-one",
        json!({"agent":"explore","prompt":"inspect implementation"}),
    )
    .await
    .unwrap();
    team.command(
        "async-two",
        json!({"agent":"plan","prompt":"inspect acceptance"}),
    )
    .await
    .unwrap();
    let (mut first, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let one = request(&mut first).await;
    let (mut second, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let two = request(&mut second).await;
    assert_ne!(
        one["model"], two["model"],
        "two distinct real requests before either response"
    );
    let snapshot = tokio::time::timeout(
        Duration::from_millis(500),
        team.command("", json!({"action":"collect"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(snapshot, json!([]));
    assert_eq!(
        team.list()
            .unwrap()
            .iter()
            .filter(|j| j.state == "running")
            .count(),
        2
    );
    respond(&mut first, json!({"content":"IMPLEMENTATION_EVIDENCE"})).await;
    respond(&mut second, json!({"content":"ACCEPTANCE_EVIDENCE"})).await;
    let results = tokio::time::timeout(
        Duration::from_secs(5),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(results.as_array().unwrap().len(), 2);
    let mut ids = std::collections::HashSet::new();
    while ids.len() < 2 {
        let notice = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(notice.text.contains("EVIDENCE"));
        ids.insert(notice.id);
    }
    let collected = team.command("", json!({"action":"collect"})).await.unwrap();
    assert_eq!(collected.as_array().unwrap().len(), 2);
    assert!(
        !team.has_updates(),
        "collect acknowledges the exact completed revisions"
    );
    team.message("main", "async-one", "additional finding")
        .await
        .unwrap();
    assert!(rx.recv().await.unwrap().text.contains("additional finding"));
}

#[tokio::test]
async fn restart_preserves_old_history_and_peer_messages_cannot_revive_stopped_child() {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let team = team(
        root,
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        "restart-owner",
    );
    team.command(
        "old-child",
        json!({"agent":"plan","prompt":"original objective"}),
    )
    .await
    .unwrap();
    let (mut stream, _) = listener.accept().await.unwrap();
    request(&mut stream).await;
    respond(&mut stream, json!({"content":"OLD_RESULT"})).await;
    team.command("", json!({"action":"wait","id":"old-child"}))
        .await
        .unwrap();
    team.stop("old-child").unwrap();
    assert!(team
        .message("old-child", "peer", "late reply")
        .await
        .is_err());
    let restart = team
        .command("", json!({"action":"restart","id":"old-child"}))
        .await
        .unwrap();
    assert_ne!(restart["id"], "old-child");
    let (mut stream, _) = listener.accept().await.unwrap();
    let input = request(&mut stream).await;
    assert!(
        !input["messages"].to_string().contains("OLD_RESULT"),
        "restart uses a fresh context"
    );
    respond(&mut stream, json!({"content":"NEW_RESULT"})).await;
    team.command("", json!({"action":"wait"})).await.unwrap();
    let old = team
        .command(
            "",
            json!({"action":"get","id":"old-child","view":"history"}),
        )
        .await
        .unwrap();
    assert_eq!(old["job"]["state"], "stopped");
    assert!(old["history"].to_string().contains("OLD_RESULT"));
    let new = team.resolve(restart["id"].as_str().unwrap()).unwrap();
    assert_eq!(new.replaces.as_deref(), Some("old-child"));
}

fn project() -> PathBuf {
    let root = std::env::temp_dir().join(fresh_id());
    std::fs::create_dir_all(&root).unwrap();
    workspace::git(&root, &["init"]).unwrap();
    workspace::git(&root, &["config", "core.autocrlf", "false"]).unwrap();
    workspace::git(&root, &["config", "user.email", "test@local"]).unwrap();
    workspace::git(&root, &["config", "user.name", "test"]).unwrap();
    std::fs::write(root.join(".gitignore"), ".kanzei/state.db*\n").unwrap();
    std::fs::write(root.join("base.txt"), "original\n").unwrap();
    workspace::git(&root, &["add", "."]).unwrap();
    workspace::git(&root, &["commit", "-m", "base"]).unwrap();
    root
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_user_questions_release_slots_for_an_independent_fifth_child() {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let team = team(
        root,
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        "question-slots",
    );
    team.0.runtime.lock().unwrap().options.ask_policy = Some(kanzei_core::AskPolicy::Interactive);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    team.set_ask_router(Arc::new(move |job, _| {
        tx.send(job.id.clone()).unwrap();
        Box::pin(std::future::pending())
    }));
    for i in 0..4 {
        team.command(
            &format!("asking-{i}"),
            json!({"agent":"plan","prompt":"ask user for a required fact"}),
        )
        .await
        .unwrap();
        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        request(&mut stream).await;
        respond(&mut stream,json!({"tool_calls":[{"index":0,"id":format!("q-{i}"),"type":"function","function":{"name":"question","arguments":"{\"question\":\"Required fact?\"}"}}]})).await;
        let asking_id = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(asking_id, format!("asking-{i}"));
        // The router callback announces its reply future before that future is
        // polled. Wait for the actual slot-release/state transition, so the fifth
        // child is dispatched only after all four questions are truly suspended.
        tokio::time::timeout(Duration::from_secs(2), async {
            while team.resolve(&asking_id).unwrap().state != "waiting_user" {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("question must release its slot and enter waiting_user");
    }
    team.command(
        "independent-fifth",
        json!({"agent":"explore","prompt":"independent inspection"}),
    )
    .await
    .unwrap();
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    request(&mut stream).await;
    respond(&mut stream, json!({"content":"FIFTH_COMPLETED"})).await;
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        team.command("", json!({"action":"wait","id":"independent-fifth"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result[0]["state"], "done");
    assert_eq!(
        team.list()
            .unwrap()
            .iter()
            .filter(|j| j.state == "waiting_user")
            .count(),
        4
    );
    team.stop_all();
}
async fn request(stream: &mut tokio::net::TcpStream) -> Value {
    let mut bytes = Vec::new();
    let mut buf = [0; 4096];
    let end = loop {
        let n = stream.read(&mut buf).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
        if let Some(p) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            break p + 4;
        }
    };
    let size = String::from_utf8_lossy(&bytes[..end])
        .lines()
        .find_map(|s| {
            s.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .map(|(_, v)| v.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while bytes.len() < end + size {
        let n = stream.read(&mut buf).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
    }
    serde_json::from_slice(&bytes[end..end + size]).unwrap()
}
async fn respond(stream: &mut tokio::net::TcpStream, delta: Value) {
    let data = json!({"choices":[{"index":0,"delta":delta,"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}});
    let body = format!("data: {data}\n\ndata: [DONE]\n\n");
    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
    stream.write_all(response.as_bytes()).await.unwrap();
}
fn team(root: PathBuf, url: &str, owner: &str) -> AgentTeam {
    let ctx = ToolCtx::new(root.clone(), root.clone()).with_session_id(owner.into());
    team_with_ctx(root, url, ctx)
}

fn team_with_ctx(root: PathBuf, url: &str, ctx: ToolCtx) -> AgentTeam {
    let config = Arc::new(KanzeiConfig::load(&root).unwrap());
    let rctx = ResolveCtx {
        cwd: root.clone(),
        project_root: root.clone(),
        profile: ProfileKind::Dev,
        config: config.clone(),
    };
    let mut harness = Harness::default();
    harness.add(crate::SubagentBase);
    let route = Route::openai_at(url, Some("test"));
    let runtime = SubagentRuntime {
        options: kanzei_core::SubagentOptions {
            ask_policy: Some(kanzei_core::AskPolicy::AutoAllow),
            reasoning: kanzei_llm::ReasoningEffort::High,
            ..Default::default()
        },
        snapshot: harness.resolve(&rctx).unwrap(),
        agent: crate::explore_agent(),
        roster: vec![crate::plan_agent()],
        fast: (route.clone(), "fast-test".into()),
        primary: (route, "primary-test".into()),
        fast_service_tier: None,
        primary_service_tier: None,
        compact: None,
        max_tokens: 512,
        timeout_secs: 15,
        limits: config.limits.clone(),
        coordinator: Some(Arc::new(
            kanzei_core::orchestration::MemoryCoordinator::new(),
        )),
        writable: false,
        ask_router: None,
        change_log: None,
        cancellations: None,
        background: false,
        background_results: None,
        background_events: None,
        transcripts: None,
        background_notifications: None,
        transcript_sink: None,
        transcript_provider: None,
    };
    AgentTeam::attach(
        rctx,
        ctx,
        runtime,
        LlmClient::new(&ProxyConfig::Disabled).unwrap(),
        None,
    )
    .unwrap()
}

#[tokio::test]
async fn off_rejects_new_and_restart_without_stopping_existing_task() {
    let team = answered_question_team(&fresh_id());
    let (job, worker) = fixture_worker(&team);
    team.set_policy(kanzei_harness::SubagentMode::Off, &Default::default());
    assert!(team
        .command(
            "new-off",
            json!({"action":"spawn","agent":"explore","prompt":"find a file"})
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("关闭"));
    assert!(team
        .command("restart-off", json!({"action":"restart","id":job.id}))
        .await
        .is_err());
    assert!(!worker.is_cancelled());
    assert_eq!(team.list().unwrap().len(), 1);
    assert!(team
        .command("", json!({"action":"get","id":job.id}))
        .await
        .is_ok());
    team.command("", json!({"action":"stop","id":job.id}))
        .await
        .unwrap();
    assert!(worker.is_cancelled());
    let team = answered_question_team(&fresh_id());
    let job = team.resolve("question-child").unwrap();
    team.set_policy(kanzei_harness::SubagentMode::Off, &Default::default());
    assert!(team
        .ui_command(json!({"action":"resume","id":job.id,"prompt":"continue"}))
        .await
        .unwrap_err()
        .to_string()
        .contains("关闭"));
    team.set_policy(kanzei_harness::SubagentMode::Auto, &Default::default());
    team.ui_command(json!({"action":"resume","id":job.id,"prompt":"continue"}))
        .await
        .unwrap();
    assert!(team.0.active.lock().unwrap().contains_key(&job.id));
    team.stop_all();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auto_and_ultra_enforce_configured_capacity_and_queue_extra_jobs() {
    for (mode, limit) in [
        (kanzei_harness::SubagentMode::Auto, 1),
        (kanzei_harness::SubagentMode::Ultra, 3),
    ] {
        let root = project();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let team = team(root, &url, &fresh_id());
        let limits = kanzei_harness::config::Limits {
            subagent_auto_concurrency: Some(1),
            subagent_ultra_concurrency: Some(3),
            ..Default::default()
        };
        team.set_policy(mode, &limits);
        let (sender, mut incoming) = tokio::sync::mpsc::channel(3);
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                request(&mut stream).await;
                sender.send(stream).await.unwrap();
            }
        });
        for i in 0..3 {
            team.command(
                &format!("capacity-{i}"),
                json!({"action":"spawn","agent":"explore","prompt":"report one finding"}),
            )
            .await
            .unwrap();
        }
        let mut streams = Vec::new();
        for _ in 0..limit {
            streams.push(
                tokio::time::timeout(std::time::Duration::from_secs(10), incoming.recv())
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        if limit < 3 {
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(40), incoming.recv())
                    .await
                    .is_err()
            );
            // A reused team releases the remaining queued work when switched to Ultra.
            team.set_policy(kanzei_harness::SubagentMode::Ultra, &limits);
            for _ in limit..3 {
                streams.push(
                    tokio::time::timeout(std::time::Duration::from_secs(10), incoming.recv())
                        .await
                        .unwrap()
                        .unwrap(),
                );
            }
        }
        for stream in &mut streams {
            respond(stream, json!({"content":"evidence"})).await;
        }
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            team.command("", json!({"action":"wait"})),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(team.list().unwrap().iter().all(|job| job.state == "done"));
        server.await.unwrap();
    }
}

#[test]
fn snapshot_preserves_dirty_parent_index_and_conflicting_adoption_is_atomic() {
    let root = project();
    std::fs::write(root.join("base.txt"), "staged user change\n").unwrap();
    workspace::git(&root, &["add", "base.txt"]).unwrap();
    std::fs::write(root.join("base.txt"), "user change\n").unwrap();
    let before = workspace::git(&root, &["diff", "--cached"]).unwrap();
    let (tree, base) = workspace::prepare(&root, &fresh_id()).unwrap();
    assert_eq!(
        std::fs::read_to_string(tree.join("base.txt")).unwrap(),
        "user change\n"
    );
    assert_eq!(
        workspace::git(&root, &["diff", "--cached"]).unwrap(),
        before
    );
    std::fs::write(tree.join("base.txt"), "child change\n").unwrap();
    std::fs::write(tree.join("new.txt"), "new\n").unwrap();
    let (head, files) = workspace::result(&tree, &base, "result").unwrap();
    assert_eq!(files.len(), 2);
    assert!(workspace::git(
        &root,
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/kanzei/agent-results"
        ]
    )
    .unwrap()
    .contains(&head));
    std::fs::write(root.join("base.txt"), "newer user change\n").unwrap();
    assert!(workspace::adopt(&root, &base, &head).is_err());
    assert!(!root.join("new.txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("base.txt")).unwrap(),
        "newer user change\n"
    );
    std::fs::write(root.join("base.txt"), "user change\n").unwrap();
    workspace::adopt(&root, &base, &head).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("base.txt")).unwrap(),
        "child change\n"
    );
    assert!(root.join("new.txt").exists());
    assert_eq!(
        workspace::git(&root, &["diff", "--cached"]).unwrap(),
        before
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_writer_background_isolated_result_then_explicit_adoption() {
    writer_background_isolated_result_then_explicit_adoption(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paired_project_writer_background_isolated_result_then_explicit_adoption() {
    writer_background_isolated_result_then_explicit_adoption(false).await;
}

async fn writer_background_isolated_result_then_explicit_adoption(project_workflow: bool) {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let first = request(&mut stream).await;
        respond(&mut stream,json!({"tool_calls":[{"index":0,"id":"write-child","type":"function","function":{"name":"write","arguments":"{\"path\":\"child.txt\",\"content\":\"child result\"}"}}]})).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        let second = request(&mut stream).await;
        respond(
            &mut stream,
            json!({"content":"Created child.txt; ready for parent review."}),
        )
        .await;
        (first, second)
    });
    let mut ctx = ToolCtx::new(root.clone(), root.clone()).with_session_id("writer-owner".into());
    ctx.project_workflow = project_workflow;
    let team = team_with_ctx(root.clone(), &url, ctx);
    let ack = team
        .command(
            "writer",
            json!({"agent":"implement","prompt":"Create child.txt"}),
        )
        .await
        .unwrap();
    assert_eq!(ack["state"], "queued");
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        team.command("", json!({"action":"wait","id":"writer"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result[0]["state"], "done", "{result}");
    assert_eq!(result[0]["outcome"], "candidate");
    assert!(
        team.has_updates(),
        "UI inspection cannot consume the parent result"
    );
    let delivered = team
        .execute(String::new(), json!({"action":"wait","id":"writer"}))
        .await;
    assert!(!delivered.is_error);
    assert!(
        !team.has_updates(),
        "The parent has received the actual result"
    );
    assert!(!root.join("child.txt").exists());
    assert_eq!(
        std::fs::read_to_string(
            PathBuf::from(result[0]["worktree"].as_str().unwrap()).join("child.txt")
        )
        .unwrap(),
        "child result"
    );
    assert!(team
        .0
        .store
        .get("writer")
        .unwrap()
        .trace
        .iter()
        .any(|t| t["phase"] == "usage"));
    assert!(team
        .0
        .store
        .get("writer")
        .unwrap()
        .trace
        .iter()
        .any(|t| t["phase"] == "end"
            && t["run_id"] == "writer-owner:writer:1"
            && t["at"].is_u64()));
    let (first, second) = server.await.unwrap();
    assert_eq!(first["model"], "primary-test");
    assert!(second.to_string().contains("write-child"));
    let adopted = team
        .command("", json!({"action":"adopt","id":"writer"}))
        .await
        .unwrap();
    assert_eq!(adopted["outcome"], "adopted");
    assert!(
        team.has_updates(),
        "UI adoption still needs to reach the parent"
    );
    team.acknowledge_result(&result[0]).unwrap();
    assert!(
        team.has_updates(),
        "An older result cannot consume a newer adoption"
    );
    let delivered = team
        .execute(String::new(), json!({"action":"adopt","id":"writer"}))
        .await;
    assert!(!delivered.is_error);
    assert!(
        !team.has_updates(),
        "A returned adoption must not trigger another final answer"
    );
    assert!(root.join("child.txt").exists());
    assert_eq!(workspace::git(&root, &["diff", "--cached"]).unwrap(), "");
    assert!(team
        .command(
            "",
            json!({"action":"message","id":"writer","prompt":"modify again"})
        )
        .await
        .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn planning_resumes_saved_history_and_messages_are_owned() {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let first = request(&mut stream).await;
        respond(&mut stream, json!({"content":"first plan evidence"})).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        let second = request(&mut stream).await;
        respond(&mut stream, json!({"content":"updated plan evidence"})).await;
        (first, second)
    });
    let team = team(root.clone(), &url, "plan-owner");
    team.command(
        "planner",
        json!({"agent":"plan","prompt":"First planning request","background":false}),
    )
    .await
    .unwrap();
    assert!(team
        .0
        .store
        .history("planner")
        .unwrap()
        .iter()
        .any(|m| serde_json::to_string(m)
            .unwrap()
            .contains("first plan evidence")));
    assert!(TeamStore::open(&root, "different-owner")
        .unwrap()
        .get("planner")
        .is_err());
    // The checkpoint may have stopped at its step ceiling. An explicit successful
    // continuation must restore history and clear the old correction outcome.
    team.update("planner", |job| {
        job.state = "failed".into();
        job.outcome = "needs_correction".into();
    })
    .unwrap();
    team.message("planner", "main", "Focus on second acceptance condition")
        .await
        .unwrap();
    team.command("", json!({"action":"wait"})).await.unwrap();
    let jobs = team.command("", json!({"action":"collect"})).await.unwrap();
    assert_eq!(jobs[0]["attempt"], 2);
    assert_eq!(jobs[0]["state"], "done");
    assert_eq!(jobs[0]["outcome"], "candidate");
    assert!(!team.has_updates());
    let (first, second) = server.await.unwrap();
    assert_eq!(first["model"], "primary-test");
    assert!(second.to_string().contains("first plan evidence"));
    assert!(second.to_string().contains("second acceptance condition"));
    assert!(team
        .0
        .store
        .get("planner")
        .unwrap()
        .messages
        .iter()
        .all(|m| m.state == "processed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dependency_failure_blocks_worker_and_stop_cancels_only_selected_task() {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = request(&mut stream).await;
        let _ = seen_tx.send(());
        tokio::time::sleep(Duration::from_secs(20)).await;
    });
    let team = team(root, &url, "stop-owner");
    team.command("first", json!({"agent":"explore","prompt":"Long request"}))
        .await
        .unwrap();
    seen_rx.await.unwrap();
    team.command(
        "second",
        json!({"agent":"plan","prompt":"Depends on first","depends_on":["first"]}),
    )
    .await
    .unwrap();
    team.command("", json!({"action":"stop","id":"first"}))
        .await
        .unwrap();
    let results = tokio::time::timeout(
        Duration::from_secs(5),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(results[0]["state"], "stopped");
    assert_eq!(results[1]["state"], "blocked");
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_children_fork_context_and_steering_are_delivered_before_next_request() {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let team = team(root.clone(), &url, "parallel-owner");
    team.set_parent(&[Message::user_text("PARENT_CONTEXT_EVIDENCE")]);
    team.command(
        "a",
        json!({"agent":"plan","prompt":"PLAN_A","context":"fork"}),
    )
    .await
    .unwrap();
    team.command("b", json!({"agent":"plan","prompt":"PLAN_B"}))
        .await
        .unwrap();
    let (mut a, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let first = request(&mut a).await;
    let (mut b, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let second = request(&mut b).await;
    let a_first = first.to_string().contains("PLAN_A");
    assert_eq!(
        first.to_string().contains("PARENT_CONTEXT_EVIDENCE"),
        a_first
    );
    assert_eq!(
        second.to_string().contains("PARENT_CONTEXT_EVIDENCE"),
        !a_first
    );
    // Both requests are in flight before either returns; messaging one preserves the other.
    team.message("a", "b", "STEERING_NEXT_REQUEST")
        .await
        .unwrap();
    let tool = json!({"tool_calls":[{"index":0,"id":"read-original","type":"function","function":{"name":"read","arguments":"{\"path\":\"base.txt\"}"}}]});
    if a_first {
        respond(&mut a, tool).await;
        respond(&mut b, json!({"content":"B finished"})).await;
    } else {
        respond(&mut b, tool).await;
        respond(&mut a, json!({"content":"B finished"})).await;
    }
    let (mut next, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let steered = request(&mut next).await;
    assert!(steered.to_string().contains("STEERING_NEXT_REQUEST"));
    respond(&mut next, json!({"content":"A followed updated direction"})).await;
    tokio::time::timeout(
        Duration::from_secs(8),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        team.command("", json!({"action":"collect"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(result
        .as_array()
        .unwrap()
        .iter()
        .all(|j| j["state"] == "done"));
    assert!(!team.has_updates());
    assert!(team
        .0
        .store
        .get("a")
        .unwrap()
        .messages
        .iter()
        .all(|m| m.state == "processed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dependent_readers_read_actual_candidate_files() {
    for role in ["verify", "plan"] {
        dependent_reader_reads_actual_candidate_files(role).await;
    }
}

async fn dependent_reader_reads_actual_candidate_files(role: &str) {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        request(&mut stream).await;
        respond(&mut stream,json!({"tool_calls":[{"index":0,"id":"write-candidate","type":"function","function":{"name":"write","arguments":"{\"path\":\"candidate.txt\",\"content\":\"ACTUAL_CANDIDATE_BYTES\"}"}}]})).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        request(&mut stream).await;
        respond(&mut stream, json!({"content":"candidate ready"})).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        request(&mut stream).await;
        respond(&mut stream,json!({"tool_calls":[{"index":0,"id":"verify-candidate","type":"function","function":{"name":"read","arguments":"{\"path\":\"candidate.txt\"}"}}]})).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        let evidence = request(&mut stream).await;
        respond(&mut stream, json!({"content":"candidate verified"})).await;
        evidence
    });
    let team = team(root.clone(), &url, "dependency-owner");
    team.command(
        "writer",
        json!({"agent":"implement","prompt":"Produce candidate"}),
    )
    .await
    .unwrap();
    team.command(
        "verifier",
        json!({"agent":role,"prompt":"Read candidate evidence","depends_on":["writer"]}),
    )
    .await
    .unwrap();
    let jobs = tokio::time::timeout(
        Duration::from_secs(20),
        team.command("", json!({"action":"collect"})),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        jobs.as_array()
            .unwrap()
            .iter()
            .all(|j| j["state"] == "done"),
        "{jobs}"
    );
    let evidence = server.await.unwrap();
    assert!(evidence["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .any(|m| m.to_string().contains("ACTUAL_CANDIDATE_BYTES")));
    assert!(!root.join("candidate.txt").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn role_memory_persists_and_stale_writers_cannot_overwrite_it() {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        request(&mut stream).await;
        respond(&mut stream, json!({"content":"original evidence"})).await;
    });
    let team = team(root.clone(), &url, "memory-owner");
    team.command("memory-plan", json!({"agent":"plan","description":"project-planner","prompt":"inspect project","background":false})).await.unwrap();
    server.await.unwrap();
    let mut harness = Harness::default();
    harness.add(super::tools::TeamTools {
        team: team.clone(),
        id: "memory-plan".into(),
        tree: root.clone(),
        writing: false,
    });
    let snapshot = harness.resolve(&team.0.config.lock().unwrap()).unwrap();
    let memory = snapshot
        .materialize_tools()
        .into_iter()
        .find(|t| t.name() == "agent_memory")
        .unwrap();
    let before = memory.execute(json!({}), &team.0.ctx).await;
    let before: Value = serde_json::from_str(&before.content).unwrap();
    let saved = memory.execute(json!({"revision":before["revision"],"content":"base.txt:1 contains the baseline evidence"}), &team.0.ctx).await;
    assert!(!saved.is_error, "{}", saved.content);
    let stale = memory
        .execute(
            json!({"revision":before["revision"],"content":"stale overwrite"}),
            &team.0.ctx,
        )
        .await;
    assert!(stale.is_error);
    let current = memory.execute(json!({}), &team.0.ctx).await;
    assert!(current.content.contains("baseline evidence"));
    assert!(snapshot
        .refreshable_system_baseline_with_report()
        .0
        .contains("baseline evidence"));
    let value: Value = serde_json::from_str(&current.content).unwrap();
    assert!(PathBuf::from(value["path"].as_str().unwrap())
        .starts_with(root.join(".kanzei/agent-memory")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_child_result_is_a_failure_not_completion() {
    let root = project();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        request(&mut stream).await;
        respond(&mut stream, json!({"content":""})).await;
    });
    let team = team(root, &url, "empty-owner");
    team.command(
        "empty",
        json!({"agent":"plan","prompt":"return evidence","background":false}),
    )
    .await
    .unwrap();
    assert_eq!(team.0.store.get("empty").unwrap().state, "failed");
    server.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_persona_is_visible_and_preserves_system_without_a_step_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let team = team(
        project(),
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        "persona-owner",
    );
    let mut persona = crate::plan_agent();
    persona.name = "auditor".into();
    persona.system = "CUSTOM_PERSONA_EVIDENCE".into();
    persona.steps = 7;
    team.0.runtime.lock().unwrap().roster.push(persona);
    assert!(team.spec().input_schema["properties"]["agent"]["enum"]
        .as_array()
        .unwrap()
        .contains(&json!("auditor")));
    team.command(
        "persona",
        json!({"agent":"auditor","prompt":"Inspect source"}),
    )
    .await
    .unwrap();
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let payload = request(&mut socket).await;
    assert!(payload.to_string().contains("CUSTOM_PERSONA_EVIDENCE"));
    assert!(team.0.store.get("persona").unwrap().worktree.is_none());
    respond(&mut socket, json!({"content":"checked"})).await;
    tokio::time::timeout(
        Duration::from_secs(8),
        team.command("", json!({"action":"wait"})),
    )
    .await
    .unwrap()
    .unwrap();
}
