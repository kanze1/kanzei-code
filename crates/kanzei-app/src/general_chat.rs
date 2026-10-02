//! Projectless conversations own a global store; this root is never a project preference.
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::State;

pub(crate) fn storage_root() -> Result<PathBuf, String> {
    kanzei_harness::kanzei_home()
        .map(|home| home.join("conversations/general"))
        .ok_or_else(|| "无法确定无项目对话的存储目录".into())
}

pub(crate) fn is_general_root(root: &Path) -> bool {
    kanzei_harness::is_general_conversation_root(root)
}

pub(crate) fn enforce(process: &crate::ProcessHandle) {
    let mut profile = process.profile.lock().unwrap();
    if profile.as_deref() != Some("dev") {
        *profile = Some("dev".into());
        process.subagents_enabled.store(true, Ordering::SeqCst);
    }
    *process.research_topic.lock().unwrap() = None;
    process
        .phase_pipeline_enabled
        .store(false, Ordering::SeqCst);
    process
        .tracker_writes_enabled
        .store(false, Ordering::SeqCst);
}

pub(crate) fn open(state: &crate::AppState) -> Result<String, String> {
    let root = storage_root()?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let root = crate::normalized_project_root(&root);
    crate::ensure_default_process(state, &root);
    crate::processes::registry::restore_processes_from_store_once(state, &root)?;
    let handles: Vec<_> = state
        .processes
        .lock()
        .unwrap()
        .values()
        .filter(|p| p.origin_project.0 == root)
        .cloned()
        .collect();
    for process in handles {
        if process.profile.lock().unwrap().as_deref() != Some("dev")
            || process.research_topic.lock().unwrap().is_some()
            || process.phase_pipeline_enabled.load(Ordering::SeqCst)
            || process.tracker_writes_enabled.load(Ordering::SeqCst)
        {
            enforce(&process);
            crate::processes::registry::persist_process(&root, &process)?;
        }
    }
    Ok(root.display().to_string())
}

#[tauri::command]
pub(crate) fn general_chat_location() -> Result<Option<String>, String> {
    let root = storage_root()?;
    Ok(root
        .is_dir()
        .then(|| crate::normalized_project_root(&root).display().to_string()))
}

#[tauri::command]
pub(crate) fn general_chat_open(state: State<'_, crate::AppState>) -> Result<String, String> {
    open(&state)
}

struct LinkGuard(std::sync::Arc<crate::SessionRuntime>);
impl Drop for LinkGuard {
    fn drop(&mut self) {
        self.0.compacting.store(false, Ordering::SeqCst);
    }
}

/// Copy the conversation into a new project discussion, retaining its global history.
pub(crate) async fn link_to_project(
    state: &crate::AppState,
    process_id: &str,
    target: &Path,
) -> Result<crate::ProcessInfo, String> {
    let root = crate::normalized_project_root(&storage_root()?);
    let target = crate::normalized_project_root(target);
    if !target.is_dir() {
        return Err("项目目录当前不可用，请恢复后再关联".into());
    }
    if is_general_root(&target)
        || !crate::projects::projects_get()
            .projects
            .iter()
            .any(|p| crate::normalized_project_root(Path::new(p)) == target)
    {
        return Err("请选择已登记的项目".into());
    }
    let process = state
        .processes
        .lock()
        .unwrap()
        .get(process_id)
        .cloned()
        .ok_or("对话不存在")?;
    if process.origin_project.0 != root {
        return Err("这不是无项目对话".into());
    }
    let session = crate::process_session_id(&root, Some(process_id));
    let runtime = crate::runtime_for(state, &session);
    {
        let _lock = runtime.lifecycle.lock().unwrap();
        if runtime.running.load(Ordering::SeqCst) || runtime.compacting.swap(true, Ordering::SeqCst)
        {
            return Err("对话正在运行或整理，请结束后再关联".into());
        }
    }
    let _guard = LinkGuard(runtime);
    let source = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    if !source
        .list_pending_inputs(&session)
        .map_err(|e| e.to_string())?
        .is_empty()
    {
        return Err("对话还有排队输入，请处理后再关联".into());
    }
    // Import the same current segment the user sees, including typed facts and
    // completed compaction; the legacy-only helper can be empty after restart.
    let messages = kanzei_core::filter_message_history(
        &crate::conversation::project_latest_segment(&source, &session)?,
    );
    let model = process.model.lock().unwrap().clone();
    let reasoning = process.reasoning.lock().unwrap().clone();
    let created = crate::processes::lifecycle::create_process_with_tracker(
        state,
        &target.display().to_string(),
        model,
        Some("readonly".into()),
        reasoning,
        Some(false),
        Some(false),
        Some(false),
        None,
        None,
        None,
    )
    .await?;
    let owner = crate::process_session_id(&target, Some(&created.id));
    let copy = (|| -> Result<(), String> {
        let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&target))
            .map_err(|e| e.to_string())?;
        store
            .create_session(&owner, &target.display().to_string(), None)
            .map_err(|e| e.to_string())?;
        let hash = kanzei_core::store::stable_json_hash(&messages);
        let seed = kanzei_core::SessionFactEnvelope::new(
            format!("general-link:{}", created.id),
            None,
            kanzei_core::SessionFact::LegacySeeded {
                source_event_id: format!("general:{session}"),
                source_sequence: 0,
                source_hash: hash,
                messages: messages.clone(),
            },
        );
        store
            .append_session_facts_checked(
                &owner,
                &mut kanzei_core::SessionInvariant::default(),
                &[seed],
            )
            .map_err(|e| e.to_string())?;
        // Both read paths remain supported while projection rollout is optional.
        store
            .append_event(
                &owner,
                "conversation.updated",
                &serde_json::json!({"messages":messages}),
            )
            .map_err(|e| e.to_string())?;
        store
            .append_event(
                &owner,
                "general.linked",
                &serde_json::json!({"source_session":session,"source_root":root}),
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(error) = copy {
        let orphan = state.processes.lock().unwrap().get(&created.id).cloned();
        if let Some(orphan) = orphan {
            let _ = crate::processes::lifecycle::close_process(state, &orphan).await;
        }
        return Err(error);
    }
    crate::runtime_for(state, &owner)
        .conversation
        .lock()
        .unwrap()
        .insert(owner, messages);
    Ok(created)
}

#[tauri::command]
pub(crate) async fn general_chat_link(
    state: State<'_, crate::AppState>,
    process_id: String,
    project_dir: String,
) -> Result<crate::ProcessInfo, String> {
    link_to_project(&state, &process_id, Path::new(&project_dir)).await
}

/// Apply the shared projectless scope after app-specific components.
pub(crate) struct GeneralChatBoundary;
impl kanzei_harness::Component for GeneralChatBoundary {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        ctx: &kanzei_harness::ResolveCtx,
    ) -> anyhow::Result<()> {
        kanzei_harness::Component::contribute(&kanzei_tools::GeneralChatProfile, draft, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn global_store_restores_events_without_registering_a_project() {
        let home = std::env::temp_dir().join(format!(
            "kz-general-{}-{}",
            std::process::id(),
            crate::run::now_ms()
        ));
        crate::settings::with_kanzei_home(&home, || {
            std::fs::create_dir_all(home.join("skills")).unwrap();
            std::fs::create_dir_all(home.join("agents")).unwrap();
            std::fs::write(home.join("skills/general-fixture.md"), "---\nname: general-fixture\ndescription: Generate an artifact\n---\nUse tools to create files.").unwrap();
            std::fs::write(home.join("agents/persona-fixture.md"), "---\nname: persona-fixture\nmode: subagent\nprofile: all\n---\nCUSTOM_GENERAL_PERSONA").unwrap();
            let state = crate::AppState::default();
            let root = PathBuf::from(open(&state).unwrap());
            assert!(is_general_root(&root));
            let p = crate::ensure_default_process(&state, &root);
            assert_eq!(p.profile.lock().unwrap().as_deref(), Some("dev"));
            assert!(p.subagents_enabled.load(Ordering::SeqCst));
            p.subagents_enabled.store(false, Ordering::SeqCst);
            enforce(&p);
            assert!(!p.subagents_enabled.load(Ordering::SeqCst));
            let workspace = kanzei_harness::general_conversation_workspace(&root, "chat-one");
            assert_ne!(
                workspace,
                kanzei_harness::general_conversation_workspace(&root, "chat-two")
            );
            let tool_ctx = kanzei_harness::ToolCtx::new(workspace, root.clone());
            assert!(!tool_ctx.project_workflow);
            assert!(!tool_ctx.clone().project_workflow);
            let session = crate::process_session_id(&root, Some(&p.id));
            let store =
                kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
            store
                .create_session(&session, &root.display().to_string(), None)
                .unwrap();
            store
                .append_event(
                    &session,
                    "general.probe",
                    &serde_json::json!({"text":"saved"}),
                )
                .unwrap();
            drop(store);
            let restarted = crate::AppState::default();
            assert_eq!(open(&restarted).unwrap(), root.display().to_string());
            assert!(restarted.processes.lock().unwrap().contains_key(&p.id));
            let restored_store =
                kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
            assert_eq!(
                restored_store
                    .latest_event(&session, "general.probe")
                    .unwrap()
                    .unwrap()
                    .payload["text"],
                "saved"
            );
            drop(restored_store);
            assert!(!home.join("app.json").exists());
            assert!(!root.join(".kanzei/project/requirements.md").exists());
            let mut config = kanzei_harness::config::KanzeiConfig::default();
            config.permissions.rules.push(kanzei_harness::rule(
                "write",
                "*/protected.txt",
                kanzei_harness::Effect::Deny,
            ));
            let ctx = kanzei_harness::ResolveCtx {
                profile: kanzei_harness::ProfileKind::Dev,
                cwd: root.clone(),
                project_root: root,
                config: std::sync::Arc::new(config),
            };
            let snapshot = crate::run::assembly::build_run_harness(false, None)
                .resolve(&ctx)
                .unwrap();
            let tools: Vec<_> = snapshot
                .materialize_tools()
                .iter()
                .map(|tool| tool.name())
                .collect();
            assert!(tools.contains(&"webfetch"));
            for name in [
                "read",
                "write",
                "edit",
                "bash",
                "process",
                "browser",
                "latex",
                "plot",
                "memory_search",
                "tool_search",
            ] {
                assert!(tools.contains(&name), "missing general tool {name}");
            }
            for name in [
                "req",
                "work",
                "defect",
                "test_record",
                "architecture",
                "conventions",
                "prior_art",
            ] {
                assert!(!tools.contains(&name), "project tool leaked: {name}");
            }
            assert!(snapshot.agents().len() > 1);
            assert!(snapshot
                .select_agent(Some("general"))
                .unwrap()
                .system
                .contains("create the actual file"));
            assert!(snapshot.select_agent(Some("dev")).is_ok());
            assert!(snapshot.skills().get("general-fixture").is_some());
            assert_eq!(
                snapshot.agents().get("persona-fixture").unwrap().system,
                "CUSTOM_GENERAL_PERSONA"
            );
            assert_eq!(
                snapshot.permissions().evaluate("write", "/protected.txt"),
                kanzei_harness::Effect::Deny
            );
            assert_eq!(
                snapshot.permissions().evaluate("write", "/ordinary.txt"),
                kanzei_harness::Effect::Ask
            );
            assert_eq!(
                snapshot
                    .permissions()
                    .evaluate("write", "/.kanzei/project/requirements.md"),
                kanzei_harness::Effect::Ask
            );
            // A writable child keeps task steering and private memory context.
            let mut draft = kanzei_harness::HarnessDraft::default();
            draft.context.insert(
                "team/inbox",
                kanzei_harness::source("team/inbox", |_| Some("STEERING".into())),
            );
            kanzei_harness::Component::contribute(
                &kanzei_tools::GeneralChatProfile,
                &mut draft,
                &ctx,
            )
            .unwrap();
            assert!(draft.context.get("team/inbox").is_some());
            assert!(!is_general_root(&home.join("real-project")));
        });
        std::fs::remove_dir_all(home).unwrap();
    }
}
