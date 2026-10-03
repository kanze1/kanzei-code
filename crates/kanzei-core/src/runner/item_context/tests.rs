use super::*;
use crate::runner::testutil::{call, result};
use crate::{
    project_session_facts, project_session_facts_with_surface, SessionStore, TypedSessionWriter,
};

struct Project(std::path::PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kz-item-context-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn ctx(&self) -> ToolCtx {
        ToolCtx::new(self.0.clone(), self.0.clone())
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn append_claim(messages: &mut Vec<Message>, call_id: &str, id: &str, extra: Value) {
    messages.push(Message::assistant(vec![Part::ToolCall {
        id: call_id.into(),
        name: "work".into(),
        input: json!({ "action": "claim", "id": id }),
    }]));
    let mut receipt = json!({ "claimed": id, "lifecycle_status": "doing" });
    receipt
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    messages.push(result(call_id, &receipt.to_string(), false));
}

fn history() -> Vec<Message> {
    let mut messages = vec![Message::user_text(
        "总目标：完成手机阅读器。改完发版；只清旧观察。",
    )];
    append_claim(&mut messages, "claim-a", "R-001", json!({}));
    for index in 0..6 {
        let id = format!("read-{index}");
        messages.push(call(&id, "read", "path", &format!("src/file-{index}.rs")));
        messages.push(result(
            &id,
            &format!("OBSERVATION_{index}\n{}", "x".repeat(12_000)),
            false,
        ));
    }
    messages.push(Message::user_text("补充要求：记住选择的原因。"));
    messages
}

fn accepted(messages: &mut Vec<Message>, ctx: &ToolCtx) -> Vec<WorkContextReport> {
    let mut reports = Vec::new();
    ItemContext::default().prepare(messages, ctx, &mut |event| {
        if let RunEvent::WorkContextPrepared {
            report, accepted, ..
        } = event
        {
            reports.push(report);
            accepted.store(true, Ordering::Release);
        }
    });
    reports
}

#[test]
fn claim_success_required_and_same_item_never_cleans() {
    let project = Project::new();
    for variant in [
        "first",
        "same",
        "next",
        "failed",
        "wrong_id",
        "other_tool",
        "open_group",
    ] {
        let mut messages = history();
        match variant {
            "first" => {}
            "same" => append_claim(&mut messages, "claim-b", "R-001", json!({})),
            "next" | "other_tool" | "failed" | "wrong_id" => {
                messages.push(Message::assistant(vec![Part::ToolCall {
                    id: "claim-b".into(),
                    name: if variant == "other_tool" { "read" } else { "work" }.into(),
                    input: json!({ "action": if variant == "next" { "next" } else { "claim" }, "id": "R-002" }),
                }]));
                messages.push(result(
                    "claim-b",
                    &json!({
                        "claimed": if variant == "wrong_id" { "R-003" } else { "R-002" },
                        "lifecycle_status": "doing",
                    })
                    .to_string(),
                    variant == "failed",
                ));
            }
            "open_group" => {
                append_claim(&mut messages, "claim-b", "R-002", json!({}));
                messages.push(call("pending", "read", "path", "pending.rs"));
            }
            _ => unreachable!(),
        }
        let original = messages.clone();
        assert!(
            accepted(&mut messages, &project.ctx()).is_empty(),
            "{variant}"
        );
        assert_eq!(messages, original, "{variant}");
    }
    assert!(!project.0.join(".kanzei/artifacts").exists());
}

#[test]
fn switch_archives_observations_losslessly_preserving_instructions_decisions_and_pairs() {
    let project = Project::new();
    let mut messages = history();
    for (name, path, error) in [
        ("read", "AGENTS.md", false),
        ("read", "skills/test/SKILL.md", false),
        ("read", ".kanzei/memory/preference.md", false),
        ("read", "failed.rs", true),
        ("question", "decision", false),
        ("work", "checkpoint", false),
        ("bash", "build log", false),
        ("task", "background-task-handle", false),
    ] {
        messages.insert(
            3,
            result(
                path,
                &format!("PRESERVE_{path}{}", "p".repeat(3_000)),
                error,
            ),
        );
        messages.insert(3, call(path, name, "path", path));
    }
    append_claim(&mut messages, "claim-b", "R-002", json!({}));
    let original = messages.clone();
    let reports = accepted(&mut messages, &project.ctx());
    assert_eq!(reports.len(), 1);
    assert!(reports[0].before_tokens - reports[0].after_tokens >= MIN_SAVING_TOKENS);
    assert_eq!(messages.len(), original.len());
    assert_eq!(crate::history::filter_message_history(&messages), messages);
    for (old, new) in original.iter().zip(&messages) {
        for (before, after) in old.parts.iter().zip(&new.parts) {
            if let (
                Part::ToolResult {
                    content: a,
                    call_id,
                    ..
                },
                Part::ToolResult { content: b, .. },
            ) = (before, after)
            {
                if b.starts_with("[work_context_archived") {
                    let path = b.split("用 read 读取 ").nth(1).unwrap();
                    assert_eq!(std::fs::read_to_string(project.0.join(path)).unwrap(), *a);
                } else if call_id != "claim-b" {
                    assert_eq!(a, b);
                }
            } else {
                assert_eq!(before, after);
            }
        }
    }
    // 继续同一条、进程重启恢复均不会反复破坏前缀缓存。
    assert!(accepted(&mut messages, &project.ctx()).is_empty());
    append_claim(&mut messages, "claim-b-resume", "R-002", json!({}));
    assert!(accepted(&mut messages, &project.ctx()).is_empty());
}

#[test]
fn registered_relations_preserve_a_larger_recent_window() {
    let project = Project::new();
    let mut regular = history();
    let mut connected = regular.clone();
    append_claim(&mut regular, "claim-b", "R-002", json!({}));
    append_claim(
        &mut connected,
        "claim-b",
        "R-002",
        json!({ "context": { "related_items": ["R-001"] } }),
    );
    let regular = accepted(&mut regular, &project.ctx());
    let connected = accepted(&mut connected, &project.ctx());
    assert_eq!(regular.len(), 1);
    assert!(connected.is_empty() || connected[0].archived_results < regular[0].archived_results);
}

#[test]
fn work_units_recognize_parent_dependency_scope_and_success_status() {
    let mut messages = Vec::new();
    append_claim(
        &mut messages,
        "a",
        "R-001/W1",
        json!({ "scope": ["src/reader"] }),
    );
    append_claim(&mut messages, "b", "R-001/W2", json!({}));
    let boundary = latest_boundary(&messages).unwrap();
    assert!(related(&boundary.from, &boundary.to));
    let input = json!({ "action": "claim", "id": "R-002/W1" });
    let call = Call {
        name: "work",
        message: 0,
        input: &input,
    };
    for receipt in [
        json!({ "unit_id": "R-002/W1", "status": "active", "dependencies": ["R-001/W1"] }),
        json!({ "unit_id": "R-002/W1", "status": "verifying", "scope": ["src/reader/ui"] }),
    ] {
        let next = claim(&call, (0, 0), &receipt.to_string()).unwrap();
        assert!(related(&boundary.from, &next));
    }
    assert!(claim(
        &call,
        (0, 0),
        &json!({ "unit_id": "R-002/W1", "status": "blocked" }).to_string()
    )
    .is_none());
}

#[test]
fn low_gain_and_failed_storage_do_not_change_history() {
    let project = Project::new();
    let mut small = history();
    for message in &mut small {
        for part in &mut message.parts {
            if let Part::ToolResult {
                content, call_id, ..
            } = part
            {
                if call_id.starts_with("read-") {
                    *content = "small result".into();
                }
            }
        }
    }
    append_claim(&mut small, "claim-b", "R-002", json!({}));
    let original = small.clone();
    assert!(accepted(&mut small, &project.ctx()).is_empty());
    assert_eq!(small, original);
    assert!(!project.0.join(".kanzei").exists());

    std::fs::write(project.0.join(".kanzei"), "not a directory").unwrap();
    let mut large = history();
    append_claim(&mut large, "claim-b", "R-002", json!({}));
    let original = large.clone();
    assert!(accepted(&mut large, &project.ctx()).is_empty());
    assert_eq!(large, original);
}

#[test]
fn unacknowledged_surface_leaves_history_unchanged_and_does_not_retry_within_run() {
    let project = Project::new();
    let mut messages = history();
    append_claim(&mut messages, "claim-b", "R-002", json!({}));
    let original = messages.clone();
    let mut state = ItemContext::default();
    let mut attempts = 0;
    for _ in 0..2 {
        state.prepare(&mut messages, &project.ctx(), &mut |_| attempts += 1);
        assert_eq!(messages, original);
    }
    assert_eq!(attempts, 1);
}

#[test]
fn persisted_boundary_restores_surface_but_keeps_full_transcript_and_later_facts() {
    let project = Project::new();
    let state_path = crate::project_state_path(&project.0);
    let store = SessionStore::open(&state_path).unwrap();
    store
        .create_session("session", &project.0.display().to_string(), None)
        .unwrap();
    let mut messages = history();
    append_claim(&mut messages, "claim-b", "R-002", json!({}));
    let original = messages.clone();
    store
        .append_event(
            "session",
            "conversation.updated",
            &json!({ "messages": original }),
        )
        .unwrap();
    crate::prepare_typed_session(&store, "session").unwrap();
    let mut writer = TypedSessionWriter::new(&state_path, "session", "run-b");
    writer.turn_started(1, 0);
    ItemContext::default().prepare(&mut messages, &project.ctx(), &mut |event| {
        if let RunEvent::WorkContextPrepared {
            report,
            source,
            surface,
            accepted,
        } = event
        {
            assert_eq!(
                source, original,
                "the candidate must carry its pre-archive source"
            );
            accepted.store(
                writer.commit_work_context_surface(&report, &source, &surface),
                Ordering::Release,
            );
        }
    });
    assert_ne!(messages, original);
    let next = Message::assistant(vec![Part::Text {
        text: "继续实现新的阅读器条目".into(),
    }]);
    writer.assistant_committed(1, next.clone());
    assert!(writer.errors().is_empty());
    let facts = store.list_session_facts("session").unwrap();
    let (sequence, surface) = store
        .latest_completed_compaction_surface("session", 0)
        .unwrap()
        .unwrap();
    let restored = project_session_facts_with_surface(&facts, Some(sequence), Some(surface));
    let mut expected = messages.clone();
    expected.push(next.clone());
    assert_eq!(restored.surface_messages, expected);
    let mut transcript = original;
    transcript.push(next);
    assert_eq!(
        project_session_facts(&facts).transcript_messages,
        transcript
    );
    let mut restored = restored.surface_messages;
    assert!(accepted(&mut restored, &project.ctx()).is_empty());
}

#[test]
fn prepared_boundary_rejects_later_mobile_fact_without_losing_history_or_failing_run() {
    let project = Project::new();
    let state_path = crate::project_state_path(&project.0);
    let store = SessionStore::open(&state_path).unwrap();
    store
        .create_session("session", &project.0.display().to_string(), None)
        .unwrap();
    let mut messages = history();
    append_claim(&mut messages, "claim-b", "R-002", json!({}));
    let original = messages.clone();
    store
        .append_event(
            "session",
            "conversation.updated",
            &json!({ "messages": original }),
        )
        .unwrap();
    crate::prepare_typed_session(&store, "session").unwrap();
    let mut writer = TypedSessionWriter::new(&state_path, "session", "run-b");
    writer.turn_started(1, 0);
    let mobile = Message::user_text("手机在整理期间补充：保留这个新要求");
    let mut prepared = false;
    ItemContext::default().prepare(&mut messages, &project.ctx(), &mut |event| {
        if let RunEvent::WorkContextPrepared {
            report,
            source,
            surface,
            accepted,
        } = event
        {
            prepared = true;
            let store = SessionStore::open(&state_path).unwrap();
            assert_eq!(source, original);
            assert_ne!(
                surface, source,
                "the real producer must have archived results"
            );
            assert!(
                store
                    .artifact_cleanup_plan(&project.0)
                    .unwrap()
                    .unreferenced_artifact_files
                    > 0
            );
            // The source was cloned and its real blobs written. An independent
            // writer now commits the same typed user fact used by mobile input.
            let mut phone = TypedSessionWriter::new(&state_path, "session", "mobile");
            phone.user_message("mobile-input", mobile.clone());
            assert!(phone.errors().is_empty(), "{:?}", phone.errors());
            let saved = writer.commit_work_context_surface(&report, &source, &surface);
            accepted.store(saved, Ordering::Release);
            let facts = store.list_session_facts("session").unwrap();
            let durable = match store
                .latest_completed_compaction_surface("session", 0)
                .unwrap()
            {
                Some((sequence, committed)) => {
                    project_session_facts_with_surface(&facts, Some(sequence), Some(committed))
                }
                None => project_session_facts(&facts),
            };
            assert!(
                durable.surface_messages.contains(&mobile),
                "a completed replacement must not hide the later durable mobile input"
            );
            assert!(
                !saved,
                "the old source must not cover the new durable user fact"
            );
        }
    });
    assert!(
        prepared,
        "the real work-item switch must produce a candidate"
    );
    assert_eq!(
        messages, original,
        "a rejected candidate keeps runner history"
    );
    assert!(
        writer.errors().is_empty(),
        "rejection must not fail the running writer"
    );
    assert!(store
        .latest_completed_compaction_surface("session", 0)
        .unwrap()
        .is_none());
    let mut expected = original;
    expected.push(mobile.clone());
    let facts = store.list_session_facts("session").unwrap();
    assert_eq!(project_session_facts(&facts).surface_messages, expected);
    let answer = Message::assistant(vec![Part::Text {
        text: "继续正常执行，并保留手机补充".into(),
    }]);
    writer.assistant_committed(1, answer.clone());
    writer.finish(crate::SessionTurnTerminal::Completed);
    assert!(writer.errors().is_empty(), "{:?}", writer.errors());
    expected.push(answer);
    assert_eq!(
        project_session_facts(&store.list_session_facts("session").unwrap()).surface_messages,
        expected
    );
}
