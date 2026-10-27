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

#[test]
fn composer_cancellation_preserves_background_inputs_and_their_origins() {
    use kanzei_base::MessageOrigin;
    use kanzei_core::Delivery;

    let temp = Temp::new();
    let root = crate::normalized_project_root(temp.path());
    let project = root.display().to_string();
    let process = "callback-owner";
    let session = process_session_id(&root, Some(process));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
    store.create_session(&session, &project, None).unwrap();
    for origin in [
        MessageOrigin::Notification,
        MessageOrigin::Engine,
        MessageOrigin::Agent,
        MessageOrigin::Tool,
        MessageOrigin::Summary,
        MessageOrigin::Unspecified,
    ] {
        store
            .admit_input_with_source(
                &session,
                origin.as_str(),
                "same transport text",
                Delivery::Steer,
                Some(origin.as_str()),
                &[],
            )
            .unwrap();
        assert!(cancel_input(
            project.clone(),
            origin.as_str().into(),
            Some(process.into())
        )
        .is_err());
        assert_eq!(
            store.input_status(origin.as_str()).unwrap().as_deref(),
            Some("pending")
        );
    }
    for id in ["human-followup", "human-question-answer"] {
        store
            .admit_input(&session, id, "same transport text", Delivery::Queue)
            .unwrap();
        assert!(!cancel_input(project.clone(), id.into(), Some("other-owner".into())).unwrap());
        assert!(cancel_input(project.clone(), id.into(), Some(process.into())).unwrap());
        assert!(!cancel_input(project.clone(), id.into(), Some(process.into())).unwrap());
    }
    // Callbacks remain in the scheduler and retain authorship when consumed.
    let pending = list_pending_inputs(project, Some(process.into())).unwrap();
    assert_eq!(pending.len(), 6);
    for input in pending {
        let promoted = store.promote_next_input(&session).unwrap().unwrap();
        assert_eq!(promoted.input_id, input.input_id);
        assert_eq!(promoted.origin, input.origin);
    }
    assert!(store.list_pending_inputs(&session).unwrap().is_empty());
}

#[tokio::test]
async fn all_conversations_share_creation_close_delete_and_restart() {
    let temp = Temp::new();
    let root = crate::normalized_project_root(temp.path());
    let project = root.display().to_string();
    let state = AppState::default();
    assert!(list_processes(&state, &project).unwrap().is_empty());
    let first = create_process(&state, &project, None, Some("dev-auto".into()), None, None)
        .await
        .unwrap();
    let second = create_process(&state, &project, None, Some("dev-auto".into()), None, None)
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
