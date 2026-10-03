//! User-triggered compaction uses the same core algorithm and durable surface as automatic compaction.
use crate::{normalized_project_root, process_session_id, runtime_for, AppState};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
};
use tauri::State;

struct CompactGuard(Arc<crate::state::SessionRuntime>);
impl Drop for CompactGuard {
    fn drop(&mut self) {
        self.0.compacting.store(false, Ordering::SeqCst);
    }
}

#[tauri::command]
pub(crate) async fn conversation_compact(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
    focus: Option<String>,
) -> Result<Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    {
        let _lifecycle = runtime.lifecycle.lock().unwrap();
        if runtime.running.load(Ordering::SeqCst) || runtime.compacting.swap(true, Ordering::SeqCst)
        {
            return Err("对话正在运行或压缩，请结束后再压缩".into());
        }
    }
    let _guard = CompactGuard(runtime.clone());
    let state_path = kanzei_core::project_state_path(&root);
    let source_sequence;
    let mut messages = {
        let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
        if !store
            .list_pending_inputs(&session_id)
            .map_err(|e| e.to_string())?
            .is_empty()
        {
            return Err("对话还有排队输入，处理完后再压缩".into());
        }
        source_sequence = store
            .list_events(&session_id, 0)
            .map_err(|e| e.to_string())?
            .last()
            .map(|e| e.sequence)
            .unwrap_or(0);
        crate::conversation::project_latest_segment(&store, &session_id)
            .map_err(|e| e.to_string())?
    };
    let before = kanzei_core::estimate_conversation_tokens(&messages);
    if messages.len() < 3 {
        return Ok(json!({"changed":false,"message":"当前内容很少，无需压缩"}));
    }
    let config = kanzei_harness::KanzeiConfig::load_at_root(&root).map_err(|e| e.to_string())?;
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
    let selected = process.model.lock().unwrap().clone();
    let resolved = config
        .resolve_model(selected.as_deref().unwrap_or("primary"))
        .map_err(|e| e.to_string())?;
    let proxy = crate::run::assembly::resolve_proxy(&config);
    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = kanzei_llm::LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let mut model = kanzei_tools::run::build_digest_model(&config, &proxy, &resolved, &route).await;
    model.archive_root = Some(root.clone());
    model.focus = focus.filter(|value| !value.trim().is_empty());
    let _publication = kanzei_core::store::artifact_liveness::acquire_publication(&root)
        .await
        .map_err(|e| e.to_string())?;
    let mut traces = Vec::new();
    let dropped = kanzei_core::compact_conversation_with_model(
        &client,
        Some(&model),
        &mut messages,
        before.max(4000),
        &mut traces,
        config.limits.recent_verbatim_ratio(),
    )
    .await;
    if dropped == 0 {
        return Ok(json!({"changed":false,"message":"当前没有完整、可压缩的历史区间"}));
    }
    let after = kanzei_core::estimate_conversation_tokens(&messages);
    // A concurrent cache append must not land between the checked commit and publish.
    let mut conversation = runtime.conversation.lock().unwrap();
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    store.append_compaction_transaction_checked(&session_id, &format!("manual:{}", crate::run::now_ms()),
        &json!({"manual":true,"focus":model.focus,"before":before,"after":after,"dropped":dropped}),
        &json!(messages),Some(source_sequence)).map_err(|e| e.to_string())?;
    conversation.insert(session_id, messages);
    Ok(
        json!({"changed":true,"before":before,"after":after,"message":format!("已压缩：{before} → {after} token") }),
    )
}
