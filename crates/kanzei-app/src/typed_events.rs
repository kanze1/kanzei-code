//! R-241：桌面 runner 对 core `TypedSessionWriter` 的薄适配与定向测试。

pub(crate) use kanzei_core::{
    prepare_typed_session as prepare_session, SessionTurnTerminal as TerminalFact,
    TypedSessionWriter as TypedEventWriter,
};

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_core::{project_session_facts, SessionStore};
    use kanzei_llm::{Message, Part};
    use serde_json::json;
    use std::path::PathBuf;

    fn fixture() -> (PathBuf, SessionStore) {
        let root = std::env::temp_dir().join(format!(
            "kz_typed_writer_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.db");
        let store = SessionStore::open(&path).unwrap();
        store.create_session("ses", "C:/project", None).unwrap();
        (path, store)
    }

    #[test]
    fn writer_batches_draft_commits_tool_results_and_terminal() {
        let (path, store) = fixture();
        let mut writer = TypedEventWriter::new(&path, "ses", "turn");
        writer.user_message("input", Message::user_text("q"));
        writer.turn_started(1, 3);
        writer.push_text(&"x".repeat(2 * 1024));
        let assistant = Message::assistant(vec![
            Part::Text {
                text: "x".repeat(2 * 1024),
            },
            Part::ToolCall {
                id: "c".into(),
                name: "read".into(),
                input: json!({"path":"a"}),
            },
        ]);
        writer.assistant_committed(1, assistant);
        writer.tool_results_committed(
            1,
            Message::tool_results(vec![Part::ToolResult {
                call_id: "c".into(),
                content: "ok".into(),
                is_error: false,
            }]),
        );
        writer.finish(TerminalFact::Completed);
        assert!(writer.errors().is_empty(), "{:?}", writer.errors());
        let projection = project_session_facts(&store.list_session_facts("ses").unwrap());
        assert_eq!(projection.surface_messages.len(), 3);
        assert!(projection.interrupted_assistants.is_empty());
    }

    #[test]
    fn stream_restart_marks_old_draft_superseded() {
        let (path, store) = fixture();
        let mut writer = TypedEventWriter::new(&path, "ses", "turn");
        writer.turn_started(1, 0);
        writer.push_text("old partial");
        writer.stream_restarted();
        writer.push_text("new partial");
        writer.finish(TerminalFact::Stopped);
        assert!(writer.errors().is_empty(), "{:?}", writer.errors());
        let projection = project_session_facts(&store.list_session_facts("ses").unwrap());
        assert_eq!(projection.interrupted_assistants.len(), 2);
        assert!(projection.interrupted_assistants[0].superseded);
        assert!(!projection.interrupted_assistants[1].superseded);
        assert_eq!(projection.transcript_messages.len(), 1);
    }
}
