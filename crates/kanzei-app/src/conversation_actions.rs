//! Desktop rewind/fork controls. The model has no tool for either action.
use crate::{normalized_project_root, process_session_id, runtime_for, AppState};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
};
use tauri::State;

struct MutationGuard(Arc<crate::state::SessionRuntime>);
impl Drop for MutationGuard {
    fn drop(&mut self) {
        self.0.compacting.store(false, Ordering::SeqCst);
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn conversation_action(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
    action: String,
    text: String,
    occurrence_from_end: Option<usize>,
    expected_hash: Option<String>,
    force: Option<bool>,
) -> Result<Value, String> {
    if !matches!(
        action.as_str(),
        "preview" | "conversation" | "code" | "both" | "fork"
    ) {
        return Err("未知对话操作".into());
    }
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    {
        let _lock = runtime.lifecycle.lock().unwrap();
        if runtime.running.load(Ordering::SeqCst) || runtime.compacting.swap(true, Ordering::SeqCst)
        {
            return Err("对话正在运行或整理，请结束后再操作".into());
        }
    }
    let _guard = MutationGuard(runtime.clone());
    let process = match process_id.as_deref() {
        Some(id) => state
            .processes
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("对话不存在或已被删除")?,
        None => return Err("请选择一段对话".into()),
    };
    if normalized_project_root(&process.origin_project.0) != root {
        return Err("对话不属于当前项目".into());
    }
    let code_root = if crate::general_chat::is_general_root(&root) {
        kanzei_harness::general_conversation_workspace(&root, &session_id)
    } else {
        process
            .worktree_path
            .as_ref()
            .map(|path| path.0.clone())
            .unwrap_or_else(|| root.clone())
    };
    let state_path = kanzei_core::project_state_path(&root);
    let _owner = kanzei_core::store::session_execution::try_acquire(&state_path, &session_id)
        .map_err(|e| format!("对话正在执行，不能修改历史：{e}"))?;
    let plan = {
        let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
        if !store
            .list_pending_inputs(&session_id)
            .map_err(|e| e.to_string())?
            .is_empty()
        {
            return Err("对话还有排队输入，请处理后再操作".into());
        }
        store
            .rewind_plan(&session_id, &text, occurrence_from_end.unwrap_or(0))
            .map_err(|e| e.to_string())?
    };
    if action == "preview" {
        return Ok(
            json!({"sourceHash":plan.source_hash,"files":plan.files,"unhandled":plan.unhandled,
            "prompt":plan.prompt,"worktree":process.worktree_path.is_some(),"keptMessages":plan.surface.len()}),
        );
    }
    if expected_hash.as_deref() != Some(plan.source_hash.as_str()) {
        return Err("预览后对话已变化，请重新预览".into());
    }
    if action == "fork" {
        let model = process.model.lock().unwrap().clone();
        let profile = process.profile.lock().unwrap().clone();
        let reasoning = process.reasoning.lock().unwrap().clone();
        let topic = process.research_topic.lock().unwrap().clone();
        let subagent_mode = *process.subagent_mode.lock().unwrap();
        let new = crate::processes::lifecycle::create_process_with_tracker(
            &state,
            &project_dir,
            model,
            profile,
            reasoning,
            Some(subagent_mode),
            Some(process.tracker_writes_enabled.load(Ordering::SeqCst)),
            None,
            None,
            topic,
        )
        .await?;
        let owner = process_session_id(&root, Some(&new.id));
        let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
        store
            .create_session(&owner, &root.display().to_string(), None)
            .map_err(|e| e.to_string())?;
        let stamp = crate::run::now_ms();
        let seed = kanzei_core::SessionFactEnvelope::new(
            format!("fork:{stamp}"),
            None,
            kanzei_core::SessionFact::LegacySeeded {
                source_event_id: format!("fork:{session_id}:{}", plan.target_sequence),
                source_sequence: plan.target_sequence,
                source_hash: kanzei_core::store::stable_json_hash(&plan.surface),
                messages: plan.surface.clone(),
            },
        );
        store
            .append_session_facts_checked(
                &owner,
                &mut kanzei_core::SessionInvariant::default(),
                &[seed],
            )
            .map_err(|e| e.to_string())?;
        // A fork must restore with either the legacy or projected read path.
        store
            .append_event(
                &owner,
                "conversation.updated",
                &json!({"messages":plan.surface}),
            )
            .map_err(|e| e.to_string())?;
        runtime_for(&state, &owner)
            .conversation
            .lock()
            .unwrap()
            .insert(owner, plan.surface);
        return Ok(json!({"processId":new.id,"prompt":plan.prompt,"forked":true}));
    }
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let result = store
        .apply_rewind(
            &root,
            &code_root,
            &session_id,
            &plan,
            action != "code",
            action != "conversation",
            force.unwrap_or(false),
        )
        .map_err(|e| e.to_string())?;
    if action != "code" {
        runtime
            .conversation
            .lock()
            .unwrap()
            .insert(session_id.clone(), plan.surface);
    }
    runtime.read_ledger.clear();
    crate::conversation::reset_auto_run_state(&state, &session_id);
    Ok(result)
}
