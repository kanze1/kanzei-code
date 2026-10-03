//! Real CLI processes share one temporary project/SQLite store and a local HTTP
//! provider. Barriers keep the first process alive while the second enters.

use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use kanzei_core::{Delivery, SessionFact, SessionStore};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};

use super::common::TestHome;

const DEADLINE: Duration = Duration::from_secs(20);

fn project(home: &TestHome, address: std::net::SocketAddr) -> std::path::PathBuf {
    let root = home.root.join("project");
    std::fs::create_dir_all(root.join(".kanzei")).unwrap();
    std::fs::write(
        root.join(".kanzei/kanzei.toml"),
        format!(
            "[models]\nprimary = \"mock:test-model\"\n\n[providers.mock]\n\
             protocol = \"openai\"\nbase_url = \"http://{address}/v1\"\n"
        ),
    )
    .unwrap();
    root
}

fn spawn(home: &TestHome, root: &Path, prompt: &str, new: bool) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kz"));
    command.args(["run", "--no-subagents"]);
    if new {
        command.arg("--new");
    }
    command.arg(prompt).current_dir(root);
    home.apply(&mut command);
    command
        .env("KANZEI_MODEL", "mock:test-model")
        .env("KANZEI_AGENT", "dev-pair")
        .env("KANZEI_PROFILE", "dev")
        .env("KANZEI_PROXY", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

async fn output(child: Child) -> Output {
    tokio::time::timeout(DEADLINE, child.wait_with_output())
        .await
        .expect("CLI must settle")
        .unwrap()
}

async fn request(listener: &TcpListener) -> (TcpStream, Value) {
    let (mut socket, _) = tokio::time::timeout(DEADLINE, listener.accept())
        .await
        .expect("CLI must reach local provider")
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    let header_end = loop {
        let count = socket.read(&mut chunk).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let length = String::from_utf8_lossy(&bytes[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    while bytes.len() < header_end + length {
        let count = socket.read(&mut chunk).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
    (socket, body)
}

fn event(text: &str, finish: Option<&str>) -> String {
    format!(
        "data: {}\n\n",
        json!({"choices":[{"index":0,"delta":{"content":text},"finish_reason":finish}]})
    )
}

async fn response(socket: &mut TcpStream, body: &str) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await.unwrap();
    socket.write_all(body.as_bytes()).await.unwrap();
}

async fn complete(socket: &mut TcpStream) {
    response(socket, &(event("done", Some("stop")) + "data: [DONE]\n\n")).await;
}

fn store(root: &Path) -> (SessionStore, String) {
    (
        SessionStore::open(&kanzei_core::project_state_path(root)).unwrap(),
        kanzei_core::project_session_id(root),
    )
}

fn check_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// Also serve an unexpected old-code request so a negative control proves the
// actual second POST/DB mutation instead of hanging at its local HTTP socket.
async fn settle_or_serve(child: Child, listener: &TcpListener) -> (Output, Option<Value>) {
    let wait = output(child);
    tokio::pin!(wait);
    tokio::select! {
        output = &mut wait => (output, None),
        (mut socket, body) = request(listener) => {
            complete(&mut socket).await;
            (wait.await, Some(body))
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_cli_cannot_recover_or_reset_a_live_first_cli() {
    let home = TestHome::new("session-owner-live");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = project(&home, listener.local_addr().unwrap());
    let first = spawn(&home, &root, "first owner", false);
    let (mut first_socket, _) = request(&listener).await;
    let (db, session) = store(&root);
    let first_turn = db
        .list_session_facts(&session)
        .unwrap()
        .iter()
        .find(|(_, fact)| matches!(fact.fact, SessionFact::TurnStarted { .. }))
        .unwrap()
        .1
        .turn_id
        .clone();
    let second = spawn(&home, &root, "second owner", true);
    let (second_output, second_post) = settle_or_serve(second, &listener).await;
    let while_live = db.list_session_facts(&session).unwrap();
    let resets = db
        .list_events_by_type(&session, 0, "conversation.reset")
        .unwrap();
    complete(&mut first_socket).await;
    let first_output = output(first).await;
    // All processes/handles settle before assertions or TestHome cleanup.
    eprintln!(
        "live CLI evidence: second_post={} resets={} live_first_failed={} first_success={} second_success={}",
        second_post.is_some(),
        resets.len(),
        while_live.iter().filter(|(_, fact)| {
            fact.turn_id == first_turn && matches!(fact.fact, SessionFact::TurnFailed { .. })
        }).count(),
        first_output.status.success(),
        second_output.status.success(),
    );
    assert!(
        second_post.is_none(),
        "a second live CLI must not issue a provider POST"
    );
    assert!(!second_output.status.success());
    assert!(String::from_utf8_lossy(&second_output.stderr).contains("当前对话正在运行"));
    assert!(
        resets.is_empty(),
        "busy --new must not reset the first owner's source"
    );
    assert!(
        !while_live.iter().any(|(_, fact)| {
            fact.turn_id == first_turn && matches!(fact.fact, SessionFact::TurnFailed { .. })
        }),
        "a held provider socket is not a crashed execution"
    );
    check_success(&first_output);
    assert!(
        db.list_pending_inputs(&session).unwrap().is_empty(),
        "busy input was not admitted"
    );
    let retry = spawn(&home, &root, "second owner", false);
    let (mut retry_socket, _) = request(&listener).await;
    complete(&mut retry_socket).await;
    check_success(&output(retry).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crashed_cli_releases_owner_and_recovers_draft_and_claim_once() {
    let home = TestHome::new("session-owner-crash");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = project(&home, listener.local_addr().unwrap());
    let mut first = spawn(&home, &root, "crash first owner", false);
    let (mut first_socket, _) = request(&listener).await;
    let draft = event(&"x".repeat(2048), None);
    let ending = event("", Some("stop")) + "data: [DONE]\n\n";
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        draft.len() + ending.len()
    );
    first_socket.write_all(head.as_bytes()).await.unwrap();
    first_socket.write_all(draft.as_bytes()).await.unwrap();
    let (db, session) = store(&root);
    let old = tokio::time::timeout(DEADLINE, async {
        loop {
            let facts = db.list_session_facts(&session).unwrap();
            if let Some((_, fact)) = facts
                .iter()
                .find(|(_, fact)| matches!(fact.fact, SessionFact::AssistantDraftAppended { .. }))
            {
                break fact.turn_id.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("first CLI must durably publish its draft before crash");
    let input = db
        .list_session_facts(&session)
        .unwrap()
        .into_iter()
        .find_map(|(_, fact)| {
            if fact.turn_id == old {
                if let SessionFact::UserMessageCommitted { input_id, .. } = fact.fact {
                    return Some(input_id);
                }
            }
            None
        })
        .unwrap();
    first.kill().await.unwrap();
    first.wait().await.unwrap();
    drop(first_socket);
    let next = spawn(&home, &root, "after crash", false);
    let (mut next_socket, _) = request(&listener).await;
    complete(&mut next_socket).await;
    check_success(&output(next).await);
    let facts = db.list_session_facts(&session).unwrap();
    assert_eq!(
        facts
            .iter()
            .filter(|(_, fact)| {
                fact.turn_id == old && matches!(fact.fact, SessionFact::TurnFailed { .. })
            })
            .count(),
        1
    );
    assert_eq!(
        facts
            .iter()
            .filter(|(_, fact)| {
                fact.turn_id == old
                    && matches!(fact.fact, SessionFact::AssistantMessageInterrupted { .. })
            })
            .count(),
        1
    );
    assert_eq!(db.input_status(&input).unwrap().as_deref(), Some("failed"));
    let again = spawn(&home, &root, "after recovery", false);
    let (mut again_socket, _) = request(&listener).await;
    complete(&mut again_socket).await;
    check_success(&output(again).await);
    assert_eq!(
        db.list_session_facts(&session)
            .unwrap()
            .iter()
            .filter(|(_, fact)| {
                fact.turn_id == old && matches!(fact.fact, SessionFact::TurnFailed { .. })
            })
            .count(),
        1,
        "recovery must not append a second terminal on a later run"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_uses_fifo_head_prompt_and_identity_leaving_argv_input_pending() {
    let home = TestHome::new("session-owner-fifo");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = project(&home, listener.local_addr().unwrap());
    let (db, session) = store(&root);
    db.create_session(&session, root.to_str().unwrap(), None)
        .unwrap();
    db.admit_input(&session, "head-A", "old FIFO message A", Delivery::Queue)
        .unwrap();
    let child = spawn(&home, &root, "new argv message B", false);
    let (mut socket, request) = request(&listener).await;
    complete(&mut socket).await;
    check_success(&output(child).await);
    let pending = db.list_pending_inputs(&session).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].prompt, "new argv message B");
    assert_ne!(pending[0].input_id, "head-A");
    assert_eq!(
        db.input_status("head-A").unwrap().as_deref(),
        Some("completed")
    );
    let user = db
        .list_session_facts(&session)
        .unwrap()
        .into_iter()
        .find_map(|(_, fact)| match fact.fact {
            SessionFact::UserMessageCommitted { input_id, message } => Some((input_id, message)),
            _ => None,
        })
        .unwrap();
    assert_eq!(user.0, "head-A");
    assert_eq!(user.1, kanzei_llm::Message::user_text("old FIFO message A"));
    let body = request.to_string();
    assert!(body.contains("old FIFO message A"));
    assert!(
        !body.contains("new argv message B"),
        "new argv cannot run under an older input ID"
    );
    assert_eq!(
        db.list_episodes(&session, 1).unwrap()[0].1,
        "old FIFO message A"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_cli_run_finishes_its_claim_before_releasing_owner() {
    let home = TestHome::new("session-owner-failure");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = project(&home, listener.local_addr().unwrap());
    let child = spawn(&home, &root, "failed provider", false);
    let (mut socket, _) = request(&listener).await;
    response(&mut socket, &event("partial response", None)).await;
    drop(socket); // Clean HTTP EOF without an authoritative completion event.
    let outcome = output(child).await;
    assert!(!outcome.status.success());
    let (db, session) = store(&root);
    let input = db
        .list_session_facts(&session)
        .unwrap()
        .into_iter()
        .find_map(|(_, fact)| match fact.fact {
            SessionFact::UserMessageCommitted { input_id, .. } => Some(input_id),
            _ => None,
        })
        .unwrap();
    assert_eq!(db.input_status(&input).unwrap().as_deref(), Some("failed"));
    assert!(db
        .list_session_facts(&session)
        .unwrap()
        .iter()
        .any(|(_, fact)| { matches!(fact.fact, SessionFact::TurnFailed { .. }) }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_does_not_fake_execution_of_a_queued_desktop_work_payload() {
    let home = TestHome::new("session-owner-work-payload");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = project(&home, listener.local_addr().unwrap());
    let (db, session) = store(&root);
    db.create_session(&session, root.to_str().unwrap(), None)
        .unwrap();
    db.admit_work_input(&session, "desktop-work", "work A", "R-001/W1")
        .unwrap();
    let child = spawn(&home, &root, "plain argv B", false);
    let (outcome, post) = settle_or_serve(child, &listener).await;
    assert!(!outcome.status.success());
    assert!(String::from_utf8_lossy(&outcome.stderr).contains("需要桌面执行上下文"));
    assert!(post.is_none());
    assert_eq!(
        db.input_status("desktop-work").unwrap().as_deref(),
        Some("failed")
    );
    assert_eq!(
        db.list_pending_inputs(&session).unwrap()[0].prompt,
        "plain argv B"
    );
    assert!(db.list_session_facts(&session).unwrap().is_empty());
}
