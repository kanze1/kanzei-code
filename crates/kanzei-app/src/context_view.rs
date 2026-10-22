//! Read projections of immutable request manifests, scoped to one conversation.
use crate::{normalized_project_root, process_session_id, AppState};
use serde_json::{json, Value};
use std::path::Path;
use tauri::State;

fn owner(
    state: &AppState,
    project_dir: &str,
    process_id: Option<&str>,
) -> Result<(std::path::PathBuf, String), String> {
    let root = normalized_project_root(Path::new(project_dir));
    if let Some(id) = process_id {
        let processes = state.processes.lock().unwrap();
        let process = processes.get(id).ok_or("对话不存在或已被删除")?;
        if normalized_project_root(&process.origin_project.0) != root {
            return Err("对话不属于当前项目".into());
        }
    }
    let session = process_session_id(&root, process_id);
    Ok((root, session))
}

fn snapshot(root: &Path, session: &str, sequence: Option<i64>) -> Result<Value, String> {
    let path = kanzei_core::project_state_path(root);
    if !path.is_file() {
        return Ok(json!({"sessionId":session,"requests":[],"selected":null,"web":[]}));
    }
    let store = kanzei_core::SessionStore::open_read_only(&path).map_err(|e| e.to_string())?;
    let requests = store
        .context_requests(session, None)
        .map_err(|e| e.to_string())?;
    let selected = match sequence {
        Some(sequence) => store
            .context_requests(session, Some(sequence))
            .map_err(|e| e.to_string())?
            .into_iter()
            .next(),
        None => requests.first().cloned(),
    };
    let floor = store
        .conversation_floor(session)
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    let web = store
        .list_named_events(session, None, Some("web.acquired"), 500)
        .map_err(|e| e.to_string())?;
    let selected_sequence = selected.as_ref().map(|request| request.sequence);
    Ok(json!({
        "sessionId":session,
        "requests":requests.iter().map(|event|json!({"sequence":event.sequence,"at":event.created_at,"turnId":event.payload["turn_id"],"step":event.payload["step"],"model":event.payload["manifest"]["model"]})).collect::<Vec<_>>(),
        "selected":selected.map(|event|json!({"sequence":event.sequence,"at":event.created_at,"turnId":event.payload["turn_id"],"step":event.payload["step"],"manifest":event.payload["manifest"]})),
        "web":web.into_iter().filter(|event|event.sequence>floor&&selected_sequence.is_none_or(|sequence|event.sequence<sequence)).map(|event|event.payload).collect::<Vec<_>>(),
    }))
}

#[tauri::command]
pub(crate) fn context_snapshot(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
    sequence: Option<i64>,
) -> Result<Value, String> {
    let (root, session) = owner(&state, &project_dir, process_id.as_deref())?;
    snapshot(&root, &session, sequence)
}

#[tauri::command]
pub(crate) fn context_content(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
    sequence: i64,
    sha256: String,
) -> Result<String, String> {
    let (root, session) = owner(&state, &project_dir, process_id.as_deref())?;
    let store = kanzei_core::SessionStore::open_read_only(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .context_block(&session, sequence, &sha256)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "该请求的原文记录不可用".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_conversation_has_no_invented_manifest_and_read_does_not_create_state() {
        let root = tempfile::tempdir().unwrap();
        let view = snapshot(root.path(), "session", None).unwrap();
        assert!(view["selected"].is_null());
        assert_eq!(view["requests"], json!([]));
        assert!(!kanzei_core::project_state_path(root.path()).exists());
    }

    #[test]
    fn native_writer_receipts_project_into_the_selected_conversation_and_request() {
        use kanzei_core::context_manifest::{ContextCapture, ContextSnapshot};
        let root = tempfile::tempdir().unwrap();
        let path = kanzei_core::project_state_path(root.path());
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store.create_session("selected", "project", None).unwrap();
        store.create_session("other", "project", None).unwrap();
        let text = "当时的规则原文";
        let request = kanzei_llm::LlmRequest {
            model: "fixture".into(),
            system: vec![text.into()],
            messages: vec![kanzei_llm::Message::user_text("inspect context")],
            tools: vec![],
            hosted_tools: vec![],
            max_tokens: 1024,
            temperature: None,
            reasoning: Default::default(),
            service_tier: None,
        };
        let route = kanzei_llm::Route::openai_at("http://127.0.0.1/v1", None);
        let captured = ContextSnapshot::capture(
            &request,
            &route,
            ContextCapture {
                system_sources: &[vec![("agent/system".into(), text.len())]],
                estimated_tokens: 10,
                ..Default::default()
            },
        );
        let sha256 = captured.manifest.blocks[0].sha256.clone();
        let mut writer = kanzei_core::TypedSessionWriter::new(&path, "selected", "run");
        assert!(writer.user_message("input", kanzei_llm::Message::user_text("inspect context")));
        writer.turn_started(1, 0);
        let receipt = writer.record_context_request(captured).unwrap();
        store
            .append_event(
                "selected",
                "web.acquired",
                &json!({"acquisition":{"id":"future-version"}}),
            )
            .unwrap();
        let view = snapshot(root.path(), "selected", Some(receipt.sequence)).unwrap();
        assert_eq!(
            view["web"],
            json!([]),
            "later acquisitions cannot be attached to an older request"
        );
        assert_eq!(view["selected"]["sequence"], receipt.sequence);
        assert_eq!(view["selected"]["manifest"]["blocks"][0]["sha256"], sha256);
        assert_eq!(
            store
                .context_block("selected", receipt.sequence, &sha256)
                .unwrap()
                .as_deref(),
            Some(text)
        );
        assert!(snapshot(root.path(), "other", None).unwrap()["selected"].is_null());
        drop(writer);
        store
            .append_event("selected", "conversation.reset", &json!({}))
            .unwrap();
        assert!(
            snapshot(root.path(), "selected", Some(receipt.sequence)).unwrap()["selected"]
                .is_null()
        );
    }
}
