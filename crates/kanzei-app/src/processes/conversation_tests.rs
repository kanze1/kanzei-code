use super::{lifecycle::*, registry::*};
use crate::{process_session_id, AppState};

struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kz-conversation-model-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn all_conversations_share_creation_close_delete_and_restart() {
    let temp = Temp::new();
    let root = crate::normalized_project_root(temp.path());
    let project = root.display().to_string();
    let state = AppState::default();
    assert!(list_processes(&state, &project).unwrap().is_empty());
    let first = create_process(&state, &project, None, Some("dev".into()), None, None, None)
        .await
        .unwrap();
    let second = create_process(&state, &project, None, Some("dev".into()), None, None, None)
        .await
        .unwrap();
    assert_eq!(first.kind, second.kind);
    assert_eq!(first.kind, "conversation");
    assert_eq!(list_processes(&state, &project).unwrap().len(), 2);
    for info in [&first, &second] {
        assert!(info.id.starts_with('p'));
        assert!(serde_json::to_value(info)
            .unwrap()
            .get("authority")
            .is_none());
        rename_process(&state, &project, &info.id, "保留的标题").unwrap();
        let process = resolve_conversation(&state, &root, Some(&info.id)).unwrap();
        close_process(&state, &process).await.unwrap();
    }
    assert!(list_processes(&state, &project).unwrap().is_empty());
    let restarted = AppState::default();
    assert!(list_processes(&restarted, &project).unwrap().is_empty());
    assert_eq!(closed_processes(&project).unwrap().len(), 2);
    for info in [&first, &second] {
        purge_process(&restarted, &project, &info.id, &|_| Ok(()))
            .await
            .unwrap();
        assert!(rename_process(&restarted, &project, &info.id, "stale").is_err());
        assert!(resolve_conversation(&restarted, &root, Some(&info.id)).is_err());
        assert!(
            crate::conversation::clear_conversation(&restarted, &project, Some(&info.id)).is_err()
        );
        assert!(list_pending_inputs(project.clone(), Some(info.id.clone()))
            .unwrap()
            .is_empty());
        assert!(!cancel_input(project.clone(), "stale".into(), Some(info.id.clone())).unwrap());
    }
    assert!(closed_processes(&project).unwrap().is_empty());
    let reopened = AppState::default();
    assert!(list_processes(&reopened, &project).unwrap().is_empty());
    let next = ensure_conversation(&reopened, &root).unwrap();
    assert_ne!(next.id, first.id);
    assert_ne!(next.id, second.id);
}

#[tokio::test]
async fn legacy_first_conversation_is_adopted_without_moving_history_and_never_resurrected() {
    let temp = Temp::new();
    let root = crate::normalized_project_root(temp.path());
    let project = root.display().to_string();
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    let legacy = crate::state::legacy_process_id(&root);
    let session = process_session_id(&root, Some(&legacy));
    store
        .create_session(&session, &project, Some("旧话题"))
        .unwrap();
    store
        .append_event(
            &session,
            "conversation.updated",
            &serde_json::json!({"messages":[]}),
        )
        .unwrap();
    let before = store.list_events(&session, 0).unwrap();
    let state = AppState::default();
    let restored = list_processes(&state, &project).unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, legacy);
    assert_eq!(restored[0].session_id, session);
    assert_eq!(restored[0].kind, "conversation");
    assert_eq!(restored[0].title.as_deref(), Some("旧话题"));
    assert_eq!(store.list_events(&session, 0).unwrap().len(), before.len());
    let handle = resolve_conversation(&state, &root, Some(&legacy)).unwrap();
    close_process(&state, &handle).await.unwrap();
    assert_eq!(closed_processes(&project).unwrap().len(), 1);
    assert!(list_processes(&AppState::default(), &project)
        .unwrap()
        .is_empty());
    purge_process(&state, &project, &legacy, &|_| Ok(()))
        .await
        .unwrap();
    assert!(store.get_session(&session).unwrap().is_none());
    assert!(list_processes(&AppState::default(), &project)
        .unwrap()
        .is_empty());
    assert!(rename_process(&state, &project, &legacy, "stale").is_err());
    assert!(crate::conversation::clear_conversation(&state, &project, Some(&legacy)).is_err());
    assert!(crate::conversation::clear_conversation(&state, &project, None).is_err());
    assert!(list_pending_inputs(project.clone(), Some(legacy.clone()))
        .unwrap()
        .is_empty());
    assert!(!cancel_input(project.clone(), "stale".into(), Some(legacy)).unwrap());
    assert!(store.get_session(&session).unwrap().is_none());
}
