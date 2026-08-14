//! 对话历史命令与会话快照访问。

use std::path::Path;

use serde_json::json;
use tauri::State;

use crate::{normalized_project_root, process_session_id, runtime_for, AppState};

#[tauri::command]
pub(crate) fn conversation_clear(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
) -> Result<(), String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    store
        .append_event(
            &session_id,
            "conversation.updated",
            &json!({ "messages": [] }),
        )
        .map_err(|e| e.to_string())?;
    runtime_for(&state, &session_id)
        .conversation
        .lock()
        .unwrap()
        .insert(session_id, Vec::new());
    Ok(())
}

#[tauri::command]
pub(crate) fn conversation_get(
    project_dir: String,
    sequence: Option<i64>,
    process_id: Option<String>,
) -> Result<Vec<kanzei_llm::Message>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    recover_messages_raw(&store, &session_id, sequence).map_err(|e| e.to_string())
}

/// R-241 只读 shadow 入口：返回 typed-events 投影、现有快照比较和中断草稿诊断。
///
/// 该命令不会把投影写回运行态，也不会切换当前模型上下文的数据源。
#[tauri::command]
pub(crate) fn conversation_shadow_get(
    project_dir: String,
    process_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let facts = store
        .list_session_facts(&session_id)
        .map_err(|e| e.to_string())?;
    let projection = kanzei_core::project_session_facts(&facts);
    let legacy = recover_messages_raw(&store, &session_id, None).map_err(|e| e.to_string())?;
    let comparison = kanzei_core::compare_shadow(&projection, &legacy);
    Ok(json!({
        "session_id": session_id,
        "comparison": comparison,
        "projection": projection,
    }))
}

#[tauri::command]
pub(crate) fn conversation_trace_get(
    project_dir: String,
    sequence: Option<i64>,
    process_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    let events = store
        .list_events_by_type(&session_id, 0, "run.trace")
        .map_err(|e| e.to_string())?;
    let limit = sequence.unwrap_or(i64::MAX);
    // 段边界来自 conversation.updated 的空快照;只取该类型最小必要行,不全表解析。
    let segment_start = store
        .list_events_by_type(&session_id, 0, "conversation.updated")
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|event| event.sequence <= limit)
        .filter(|event| {
            event.payload["messages"]
                .as_array()
                .is_some_and(Vec::is_empty)
        })
        .map(|event| event.sequence)
        .next_back()
        .unwrap_or(0);
    Ok(events
        .into_iter()
        .filter(|event| event.sequence > segment_start && event.sequence <= limit)
        .map(|event| event.payload)
        .collect())
}

#[tauri::command]
pub(crate) fn conversation_list(
    project_dir: String,
    process_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    let mut segments: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut open = false;
    for event in store
        .list_events_by_type(&session_id, 0, "conversation.updated")
        .map_err(|e| e.to_string())?
        .into_iter()
    {
        let messages = event.payload["messages"].as_array();
        let count = messages.map_or(0, Vec::len);
        if count == 0 {
            open = false;
            continue;
        }
        if !open {
            segments.push(Vec::new());
            open = true;
        }
        let title = messages
            .and_then(|items| items.iter().find(|item| item["role"] == "user"))
            .and_then(|item| item["parts"].as_array())
            .and_then(|parts| parts.iter().find(|part| part["type"] == "text"))
            .and_then(|part| part["text"].as_str())
            .unwrap_or("新对话");
        segments.last_mut().unwrap().push(json!({ "sequence": event.sequence, "created_at": event.created_at, "title": title.chars().take(48).collect::<String>(), "message_count": count }));
    }
    Ok(segments.into_iter().filter(|snapshots| !snapshots.is_empty()).map(|snapshots| {
        let sequences: Vec<i64> = snapshots.iter().filter_map(|s| s["sequence"].as_i64()).collect();
        let last = snapshots.last().cloned().unwrap_or_default();
        json!({ "sequence": last["sequence"], "created_at": last["created_at"], "title": last["title"], "message_count": last["message_count"], "sequences": sequences })
    }).collect())
}

#[tauri::command]
pub(crate) fn conversation_delete(
    project_dir: String,
    sequences: Vec<i64>,
    process_id: Option<String>,
) -> Result<usize, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .delete_events_by_sequence(&session_id, "conversation.updated", &sequences)
        .map_err(|e| e.to_string())
}

pub(crate) fn recover_messages(
    store: &kanzei_core::SessionStore,
    session_id: &str,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    recover_messages_at(store, session_id, None)
}

pub(crate) fn recover_messages_raw(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    sequence: Option<i64>,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    let event = match sequence {
        Some(sequence) => {
            store.event_by_sequence_and_type(session_id, sequence, "conversation.updated")?
        }
        None => store.latest_event(session_id, "conversation.updated")?,
    };
    let Some(event) = event else {
        return Ok(Vec::new());
    };
    let messages = event
        .payload
        .get("messages")
        .cloned()
        .unwrap_or_else(|| json!([]));
    Ok(serde_json::from_value(messages)?)
}

pub(crate) fn recover_messages_at(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    sequence: Option<i64>,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    Ok(kanzei_core::filter_message_history(&recover_messages_raw(
        store, session_id, sequence,
    )?))
}

pub(crate) fn conversation_prior(
    conversation: &std::sync::Arc<
        std::sync::Mutex<std::collections::HashMap<String, Vec<kanzei_llm::Message>>>,
    >,
    session_id: &str,
    persisted: Vec<kanzei_llm::Message>,
) -> Vec<kanzei_llm::Message> {
    let mut conversations = conversation.lock().unwrap();
    let conv = conversations.entry(session_id.to_string()).or_default();
    if conv.is_empty() && !persisted.is_empty() {
        *conv = persisted;
    }
    conv.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_project_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kanzei-conversation-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn shadow_get_returns_projection_and_comparison_without_switching_source() {
        let root = test_project_root("shadow");
        let canonical = normalized_project_root(&root);
        let session_id = process_session_id(&canonical, None);
        {
            let store =
                kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&canonical))
                    .unwrap();
            store
                .create_session(&session_id, &canonical.display().to_string(), None)
                .unwrap();
            store
                .append_event(
                    &session_id,
                    "conversation.updated",
                    &json!({ "messages": [kanzei_llm::Message::user_text("已有历史")] }),
                )
                .unwrap();
            kanzei_core::prepare_typed_session(&store, &session_id).unwrap();
        }

        let report = conversation_shadow_get(canonical.display().to_string(), None).unwrap();
        assert_eq!(report["session_id"], session_id);
        assert_eq!(report["comparison"]["equal"], true);
        assert_eq!(report["projection"]["surface_messages"][0]["role"], "user");

        std::fs::remove_dir_all(root).unwrap();
    }
}
