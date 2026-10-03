use super::*;
use kanzei_harness::{KanzeiConfig, ProfileKind};
use kanzei_llm::{ProxyConfig, Route};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

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
        ToolCtx::new(root.clone(), root).with_session_id(owner.into()),
        runtime,
        LlmClient::new(&ProxyConfig::Disabled).unwrap(),
        None,
    )
    .unwrap()
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
    let team = team(root.clone(), &url, "writer-owner");
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
async fn dependent_verifier_reads_actual_candidate_files() {
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
        json!({"agent":"verify","prompt":"Read candidate evidence","depends_on":["writer"]}),
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
async fn custom_persona_is_visible_and_preserves_system_and_budget() {
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
