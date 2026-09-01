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
    // R-242 批7:清空追加 conversation.reset 开启新 segment(保留原始历史,不是
    // 数据删除)。段边界统一由 conversation.reset 承担;不再写 conversation.updated
    // 空快照(它是旧式清空标记,且验收⑦停止新增该事件)。
    store
        .append_event(
            &session_id,
            "conversation.reset",
            &json!({ "cleared": true }),
        )
        .map_err(|e| e.to_string())?;
    runtime_for(&state, &session_id)
        .conversation
        .lock()
        .unwrap()
        .insert(session_id.clone(), Vec::new());
    reset_auto_run_state(&state, &session_id);
    Ok(())
}

/// 2026-08-20 现场:鞭挞控制器按 session_id 存(state.auto_runs),不随
/// conversation.reset 走。新对话只清了投影历史,失败重试计数(failed_rounds)
/// 原样留着——卡在失败重试循环里点新对话,鞭挞立刻带着旧计数发起下一轮,
/// 复用的还是导致上次失败的运行时状态,新对话形同虚设。这里同步走
/// auto_state_reset 同款 reset():只清轮数/失败计数,不碰 enabled/paused/
/// max_rounds 这些用户显式设置的开关。
pub(crate) fn reset_auto_run_state(state: &AppState, session_id: &str) {
    state
        .auto_runs
        .lock()
        .unwrap()
        .entry(session_id.to_string())
        .or_default()
        .state
        .reset();
}

/// R-242 批7:段边界 = conversation.reset 事件(升序 sequence)。
///
/// legacy 与投影模式共用同一段边界(清空语义统一);旧式 conversation.updated
/// 空快照不再作为清空标记。
fn segment_boundaries(
    store: &kanzei_core::SessionStore,
    session_id: &str,
) -> Result<Vec<i64>, String> {
    Ok(store
        .list_events_by_type(session_id, 0, "conversation.reset")
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|event| event.sequence)
        .collect())
}

/// R-242 批7:最新 segment 的投影 surface(新段 prior 为空的真源)。
///
/// 段边界 = 最新 conversation.reset;reset 之后的事实投影为新段消息(旧段事实
/// 保留在事件日志,仅不进入新段 surface)。会话无任何 typed facts 时回退 legacy
/// 快照——mobile 线程等未被 typed writer 接管的会话,真源仍是 conversation.updated
/// (批6 投影切换的回归防护)。重复 reset 幂等:连续边界间无事实 → 空段,不报错。
pub(crate) fn project_latest_segment(
    store: &kanzei_core::SessionStore,
    session_id: &str,
) -> Result<Vec<kanzei_llm::Message>, String> {
    let facts = store
        .list_session_facts(session_id)
        .map_err(|e| e.to_string())?;
    let boundary = segment_boundaries(store, session_id)?.pop();
    let segment_start = boundary.unwrap_or(0);
    let filtered: Vec<_> = facts
        .into_iter()
        .filter(|(event, _)| event.sequence > segment_start)
        .collect();
    let compacted_surface = store
        .latest_completed_compaction_surface(session_id, segment_start)
        .map_err(|e| e.to_string())?;
    if filtered.is_empty() {
        if let Some((_, surface)) = compacted_surface {
            return Ok(surface);
        }
        // legacy/mobile 会话没有 typed facts 时也必须尊重 reset;否则新对话会
        // 回退到 reset 之前最后一条 conversation.updated,把旧历史重新喂给 runner。
        return recover_latest_legacy_segment_raw(store, session_id, boundary)
            .map_err(|e| e.to_string());
    }
    let projection = match compacted_surface {
        Some((sequence, surface)) => kanzei_core::project_session_facts_with_surface(
            &filtered,
            Some(sequence),
            Some(surface),
        ),
        None => kanzei_core::project_session_facts(&filtered),
    };
    Ok(projection.surface_messages)
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
    // R-242 批6/7:事件投影真源。sequence=None(最新历史)且 gate 开启时,从
    // 事件日志投影最新 segment surface。sequence=Some 既可能是 legacy
    // conversation.updated 快照序号,也可能是 conversation_list 投影模式返回的
    // typed fact 段末序号;两种序号必须走各自的恢复路径。
    if sequence.is_none() && crate::projection_gate::read_path_uses_projection("conversation_get") {
        return project_latest_segment(&store, &session_id);
    }
    if sequence.is_none() {
        recover_latest_legacy_segment_raw(
            &store,
            &session_id,
            segment_boundaries(&store, &session_id)?.pop(),
        )
        .map_err(|e| e.to_string())
    } else {
        let Some(sequence) = sequence else {
            unreachable!("sequence 已在分支条件中确认")
        };
        if store
            .event_by_sequence_and_type(&session_id, sequence, "conversation.updated")
            .map_err(|e| e.to_string())?
            .is_none()
        {
            if let Some(event) = store
                .event_by_sequence(&session_id, sequence)
                .map_err(|e| e.to_string())?
            {
                // 投影历史列表的 sequence 指向 typed fact 段末,不能再按
                // conversation.updated 的精确序号读取,否则合法历史会被当成空数组。
                if kanzei_core::store::decode_session_fact(&event)
                    .map_err(|e| e.to_string())?
                    .is_some()
                {
                    return project_segment_at_sequence(&store, &session_id, sequence);
                }
            }
        }
        recover_messages_raw(&store, &session_id, Some(sequence)).map_err(|e| e.to_string())
    }
}

/// 从历史列表返回的 typed fact 段末序号恢复该段的可见消息。
///
/// 投影历史的 sequence 是段内最后一条 typed fact,而不是
/// `conversation.updated` 快照序号。恢复时必须同时尊重最近的 reset 边界,
/// 否则打开旧历史会把后续对话或前一段内容混进来。
fn project_segment_at_sequence(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    end_sequence: i64,
) -> Result<Vec<kanzei_llm::Message>, String> {
    let segment_start = segment_boundaries(store, session_id)?
        .into_iter()
        .rev()
        .find(|sequence| *sequence < end_sequence)
        .unwrap_or(0);
    let facts = store
        .list_session_facts(session_id)
        .map_err(|e| e.to_string())?;
    let segment: Vec<_> = facts
        .into_iter()
        .filter(|(event, _)| event.sequence > segment_start && event.sequence <= end_sequence)
        .collect();
    Ok(kanzei_core::project_session_facts(&segment).surface_messages)
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
        .list_latest_segment_facts(&session_id)
        .map_err(|e| e.to_string())?;
    let compaction_diagnostics = store
        .incomplete_compaction_diagnostics(&session_id)
        .map_err(|e| e.to_string())?;
    let boundary = segment_boundaries(&store, &session_id)?.pop();
    let projection = kanzei_core::project_session_facts(&facts);
    let legacy = recover_latest_legacy_segment_raw(&store, &session_id, boundary)
        .map_err(|e| e.to_string())?;
    let comparison = kanzei_core::compare_shadow(&projection, &legacy);
    Ok(json!({
        "session_id": session_id,
        "comparison": comparison,
        "compaction_diagnostics": compaction_diagnostics,
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
    // R-242 批7:段边界统一来自 conversation.reset(清空/新会话标记)。
    let segment_start = store
        .list_events_by_type(&session_id, 0, "conversation.reset")
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|event| event.sequence <= limit)
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
    // R-242 批7:段边界统一来自 conversation.reset(清空/新会话标记),legacy 与
    // 投影共用;gate 开启时按段投影 typed facts,关闭时按段内最新 legacy 快照。
    let boundaries = segment_boundaries(&store, &session_id)?;
    if crate::projection_gate::read_path_uses_projection("conversation_list") {
        return conversation_list_projected(&store, &session_id, &boundaries);
    }
    conversation_list_legacy(&store, &session_id, &boundaries)
}

/// R-242 批7:投影模式的段列表。每段 = (前一边界, 本边界] 的 typed facts 投影
/// surface;空段(连续 reset 间无事实)跳过;旧段保留在列表(可审计)。
fn conversation_list_projected(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    boundaries: &[i64],
) -> Result<Vec<serde_json::Value>, String> {
    let facts = store
        .list_session_facts(session_id)
        .map_err(|e| e.to_string())?;
    if facts.is_empty() {
        // 无 typed facts 的会话(mobile 线程等未被 typed writer 接管)回退
        // legacy 快照段——与 project_latest_segment 的空回退同一防护语义。
        return conversation_list_legacy(store, session_id, boundaries);
    }
    let mut segments = Vec::new();
    let mut start = 0i64;
    for &boundary in boundaries {
        if let Some(segment) = project_segment(&facts, start, boundary) {
            segments.push(segment);
        }
        start = boundary;
    }
    if let Some(segment) = project_segment(&facts, start, i64::MAX) {
        segments.push(segment);
    }
    Ok(segments)
}

/// 单段投影元数据:段内 facts 的 surface 消息(标题=首条 user 文本,数量=surface 长)。
fn project_segment(
    facts: &[(kanzei_core::StoredEvent, kanzei_core::SessionFactEnvelope)],
    start: i64,
    end: i64,
) -> Option<serde_json::Value> {
    let segment: Vec<_> = facts
        .iter()
        .filter(|(event, _)| event.sequence > start && event.sequence <= end)
        .cloned()
        .collect();
    if segment.is_empty() {
        return None;
    }
    let projection = kanzei_core::project_session_facts(&segment);
    let surface = &projection.surface_messages;
    if surface.is_empty() {
        return None;
    }
    let title = surface
        .iter()
        .find(|message| message.role == kanzei_llm::Role::User)
        .and_then(|message| {
            message.parts.iter().find_map(|part| match part {
                kanzei_llm::Part::Text { text } => Some(text.clone()),
                _ => None,
            })
        })
        .unwrap_or_else(|| "新对话".into());
    let last_seq = segment.last().map(|(event, _)| event.sequence).unwrap_or(0);
    let created_at = segment
        .first()
        .map(|(event, _)| event.created_at)
        .unwrap_or(0);
    Some(json!({
        "sequence": last_seq,
        "created_at": created_at,
        "title": title.chars().take(48).collect::<String>(),
        "message_count": surface.len(),
    }))
}

/// legacy 模式的段列表:段边界 = conversation.reset;段内容 = 该段内最新
/// conversation.updated 快照(旧式轮末全量)。空段跳过。
fn conversation_list_legacy(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    boundaries: &[i64],
) -> Result<Vec<serde_json::Value>, String> {
    let snapshots = store
        .list_events_by_type(session_id, 0, "conversation.updated")
        .map_err(|e| e.to_string())?;
    let mut segments = Vec::new();
    let mut start = 0i64;
    let ends: Vec<i64> = boundaries
        .iter()
        .copied()
        .chain(std::iter::once(i64::MAX))
        .collect();
    for &end in &ends {
        if let Some(event) = snapshots
            .iter()
            .filter(|event| event.sequence > start && event.sequence <= end)
            .rfind(|event| {
                event.payload["messages"]
                    .as_array()
                    .is_some_and(|messages| !messages.is_empty())
            })
        {
            let messages = event.payload["messages"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let title = messages
                .iter()
                .find(|item| item["role"] == "user")
                .and_then(|item| item["parts"].as_array())
                .and_then(|parts| parts.iter().find(|part| part["type"] == "text"))
                .and_then(|part| part["text"].as_str())
                .unwrap_or("新对话");
            segments.push(json!({
                "sequence": event.sequence,
                "created_at": event.created_at,
                "title": title.chars().take(48).collect::<String>(),
                "message_count": messages.len(),
            }));
        }
        start = end;
    }
    Ok(segments)
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
    // D-421 修复:投影模式下列表返回的是投影段(sequence = 段内最后 typed fact 的
    // sequence),只删 conversation.updated 快照会「删不掉」(类型不匹配删 0 条)。
    // 按 sequence 指向的事件类型分派:快照(legacy 列表)→ 删单条快照;typed fact
    // (投影列表)→ 删该段(段边界 = conversation.reset,见 segment_boundaries)。
    let mut deleted = 0usize;
    for sequence in &sequences {
        let Some(event) = store
            .event_by_sequence(&session_id, *sequence)
            .map_err(|e| e.to_string())?
        else {
            continue;
        };
        if event.event_type == "conversation.updated" {
            deleted += store
                .delete_events_by_sequence(&session_id, "conversation.updated", &[*sequence])
                .map_err(|e| e.to_string())?;
        } else {
            let boundaries = segment_boundaries(&store, &session_id)?;
            let start = boundaries
                .iter()
                .rfind(|boundary| **boundary < *sequence)
                .copied()
                .unwrap_or(0);
            // 段的终点是**下一个 reset 边界**(末段为 +∞),不是列表回报的
            // sequence——那只是段内最后一条 typed fact。轮末 legacy 快照
            // conversation.updated 写在所有 typed fact 之后(persist_round_outcome),
            // sequence 更大;按 fact 收口就把它留在库里,两处后果都是真缺陷:
            // ①facts 清空后 conversation_list_projected 回退 legacy,那条幸存快照
            //   又冒出来成为一条「历史对话」——删除要点两次;
            // ②project_latest_segment 同样回退 legacy,把整段旧历史读回 runner
            //   prior——用户以为删掉了,下一轮其实还带着。
            let end = boundaries
                .iter()
                .find(|boundary| **boundary >= *sequence)
                .copied()
                .unwrap_or(i64::MAX);
            deleted += store
                .delete_conversation_segment(&session_id, start, end)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(deleted)
}

/// R-245 B6:删除弹窗选择“安全整理”后的真实 storage cleanup 消费方。
#[tauri::command]
pub(crate) fn conversation_cleanup(project_dir: String) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let state_path = kanzei_core::project_state_path(&root);
    let store = kanzei_core::SessionStore::open_for_explicit_cleanup(&state_path)
        .map_err(|e| e.to_string())?;
    let result = store.cleanup_storage(&root).map_err(|e| e.to_string())?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

fn recover_latest_legacy_segment_raw(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    boundary: Option<i64>,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    let event = store
        .list_events_by_type(session_id, 0, "conversation.updated")?
        .into_iter()
        .filter(|event| boundary.is_none_or(|start| event.sequence > start))
        .rev()
        .find(|event| {
            event.payload["messages"]
                .as_array()
                .is_some_and(|messages| !messages.is_empty())
        });
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

pub(crate) fn recover_messages(
    store: &kanzei_core::SessionStore,
    session_id: &str,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    let boundary = segment_boundaries(store, session_id)
        .map_err(anyhow::Error::msg)?
        .pop();
    if boundary.is_none() {
        return recover_messages_at(store, session_id, None);
    }
    Ok(kanzei_core::filter_message_history(
        &recover_latest_legacy_segment_raw(store, session_id, boundary)?,
    ))
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
    if !persisted.is_empty() {
        // 持久事实是 coordinator 按当前 projection/legacy gate 选择出的权威来源。
        // 内存 map 只是进程内缓存；若它非空就拒绝刷新会把旧快照继续喂给 provider。
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
    fn latest_segment_recovers_completed_compaction_surface() {
        let root = test_project_root("compaction-surface");
        let canonical = normalized_project_root(&root);
        let session_id = process_session_id(&canonical, None);
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
        store
            .create_session(&session_id, &canonical.display().to_string(), None)
            .unwrap();
        store
            .append_event(
                &session_id,
                "conversation.updated",
                &json!({"messages": [kanzei_llm::Message::user_text("原始 transcript")]}),
            )
            .unwrap();
        let surface = vec![kanzei_llm::Message::user_text("恢复后的 surface")];
        store
            .append_compaction_transaction(
                &session_id,
                "cmp-restart",
                &json!({"digest":"恢复"}),
                &serde_json::to_value(&surface).unwrap(),
            )
            .unwrap();
        let recovered = project_latest_segment(&store, &session_id).unwrap();
        assert_eq!(recovered, surface);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
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
