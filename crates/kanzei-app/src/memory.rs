//! Memory commands and inbox consolidation.

use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::json;

fn memory_stores_for(project_dir: &str) -> Vec<kanzei_tools::memory::MemoryStore> {
    let cwd = PathBuf::from(project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let mut stores = vec![kanzei_tools::memory::MemoryStore::project(&root)];
    stores.extend(kanzei_tools::memory::MemoryStore::global());
    stores
}

fn append_memory_fact(
    project_dir: &str,
    event_type: &str,
    entity_id: Option<String>,
    payload: serde_json::Value,
) -> Result<(), String> {
    let cwd = PathBuf::from(project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let state_path = kanzei_core::project_state_path(&root);
    let session_id = kanzei_core::project_session_id(&root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|error| error.to_string())?;
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|error| error.to_string())?;
    let event = kanzei_core::experience_events::ExperienceEvent::new_scoped(
        event_type,
        kanzei_core::experience_events::ExperienceEventClass::Fact,
        session_id,
        Some(root.display().to_string()),
        None,
        None,
        entity_id,
        payload,
        now_ms(),
    )
    .map_err(|error| error.to_string())?;
    event
        .append_fact_if_new(&store)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn promotion_gap_count(
    entries: &[(PathBuf, kanzei_tools::memory::MemoryEntry)],
    source_backed: &HashSet<String>,
) -> usize {
    entries
        .iter()
        .filter(|(_, entry)| {
            let legacy_exempt = entry.status == "active"
                && entry.source.trim() == "user"
                && entry.refs().is_empty();
            matches!(entry.status.as_str(), "candidate" | "shadow" | "active")
                && !legacy_exempt
                && !source_backed.contains(&entry.id)
        })
        .count()
}

#[tauri::command]
pub(crate) fn memory_overview(project_dir: String) -> serde_json::Value {
    let mut scopes = Vec::new();
    for store in memory_stores_for(&project_dir) {
        let entries = store.load_all();
        let hits = store.hits_map();
        let mut categories = serde_json::Map::new();
        for cat in kanzei_tools::memory::CATEGORIES {
            let of_cat: Vec<_> = entries.iter().filter(|(_, e)| e.category == *cat).collect();
            let active = of_cat.iter().filter(|(_, e)| e.status == "active").count();
            let bytes: usize = of_cat
                .iter()
                .map(|(_, e)| e.body.len() + e.title.len() + e.description.len())
                .sum();
            let last = of_cat
                .iter()
                .map(|(_, e)| e.updated.clone())
                .max()
                .unwrap_or_default();
            categories.insert(cat.to_string(), json!({"active": active, "stale": of_cat.len() - active, "bytes": bytes, "last": last}));
        }
        scopes.push(json!({"scope": store.scope.label(), "root": store.root.display().to_string(), "total": entries.len(), "hitsTotal": hits.values().sum::<u64>(), "categories": categories, "inboxPending": store.pending_notes(), "integrity": store.integrity_issues()}));
    }
    json!({"scopes": scopes})
}

#[tauri::command]
pub(crate) fn memory_control_plane(project_dir: String) -> serde_json::Value {
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let store = kanzei_tools::memory::MemoryStore::project(&root);
    let entries = store.load_all();
    let pending = store.pending_notes();
    let oldest_waiting = store
        .read_inbox()
        .lines()
        .find_map(|line| line.strip_prefix("## note ").map(str::to_string));
    let checkpoint = store.read_inbox_checkpoint();
    let state = kanzei_core::project_state_path(&root);
    let session_id = kanzei_core::project_session_id(&root);
    let session = kanzei_core::SessionStore::open(&state).ok();
    let source_backed = session
        .as_ref()
        .and_then(|session| session.memory_ids_with_sources().ok())
        .unwrap_or_default();
    let promotion_gaps = promotion_gap_count(&entries, &source_backed);
    let recall = store.usage_counts();
    let recalled = recall.values().map(|counts| counts.recalled).sum::<u64>();
    let injected = recall.values().map(|counts| counts.injected).sum::<u64>();
    let read = recall.values().map(|counts| counts.read).sum::<u64>();
    let read_observed = recall
        .values()
        .map(|counts| counts.read_observed)
        .sum::<u64>();
    let recall_links = session
        .as_ref()
        .and_then(|session| session.recall_link_stats().ok())
        .unwrap_or_default();
    let effects = kanzei_core::SessionStore::open(&state)
        .ok()
        .and_then(|session| session.memory_effects().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|effect| {
            json!({
                "memory_id": effect.memory_id,
                "effect_mean": effect.effect_mean,
                "effect_ci": effect.effect_ci,
                "eval_n": effect.eval_n,
                "last_eval": effect.last_eval,
            })
        })
        .collect::<Vec<_>>();
    let experience_facts = session
        .as_ref()
        .and_then(|store| kanzei_core::experience_events::replay_facts(store, &session_id, 0).ok())
        .unwrap_or_default();
    json!({
        "backlog": pending,
        "oldest_waiting": oldest_waiting,
        "batch": checkpoint,
        "promotion_gaps": promotion_gaps,
        "recall": {
            "recalled": recalled,
            "injected": injected,
            "read": read,
            "read_observed": read_observed,
            "events_total": recall_links.total,
            "events_linked": recall_links.linked,
            "events_orphaned": recall_links.orphaned,
        },
        "effects": effects,
        "experience_facts": experience_facts,
    })
}

#[tauri::command]
pub(crate) fn memory_entries(
    project_dir: String,
    scope: String,
    category: Option<String>,
) -> Result<serde_json::Value, String> {
    for store in memory_stores_for(&project_dir) {
        if store.scope.label() == scope {
            let profile = store.hit_profile();
            let usage = store.usage_counts();
            let list: Vec<serde_json::Value> = store.load_all().into_iter()
                .filter(|(_, entry)| category.as_deref().is_none_or(|value| entry.category == value))
                .map(|(path, entry)| {
                    let (hits, last_hit_at) = profile.get(&entry.id).copied().unwrap_or((0, 0));
                    let counts = usage.get(&entry.id).cloned().unwrap_or_default();
                    json!({"id": entry.id, "scope": store.scope.label(), "category": entry.category,
                        "title": entry.title, "description": entry.description, "status": entry.status,
                        "updated": entry.updated, "source": entry.source, "refs": entry.refs(),
                        "hits": hits, "last_hit_at": last_hit_at, "recalled": counts.recalled,
                        "injected": counts.injected, "read": counts.read, "read_observed": counts.read_observed,
                        "path": path.display().to_string(), "body": entry.body})
                }).collect();
            return Ok(json!(list));
        }
    }
    Err(format!("未知记忆域: {scope}"))
}

#[tauri::command]
pub(crate) fn memory_note_candidates(project_dir: String) -> serde_json::Value {
    let mut out = Vec::new();
    for store in memory_stores_for(&project_dir) {
        for (hint, summary, detail) in store.pending_note_list() {
            let fingerprint = summary
                .rfind('[')
                .and_then(|i| {
                    summary[i..]
                        .find(']')
                        .map(|j| summary[i..i + j + 1].to_string())
                })
                .unwrap_or_default();
            out.push(json!({"scope": store.scope.label(), "hint": hint, "summary": summary, "detail": detail, "fingerprint": fingerprint}));
        }
    }
    json!(out)
}

#[tauri::command]
pub(crate) fn memory_note_discard(
    project_dir: String,
    scope: String,
    fingerprint: String,
) -> Result<bool, String> {
    for store in memory_stores_for(&project_dir) {
        if store.scope.label() == scope {
            return store.discard_note(&fingerprint).map_err(|e| e.to_string());
        }
    }
    Err(format!("未知记忆域: {scope}"))
}

#[tauri::command]
pub(crate) fn memory_recalls(
    project_dir: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let root = kanzei_harness::config::discover_project_root(&PathBuf::from(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let state = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|error| error.to_string())?;
    let observations = state
        .memory_recall_observations(limit)
        .map_err(|error| error.to_string())?;
    let entries = kanzei_tools::memory::MemoryStore::project(&root)
        .load_all()
        .into_iter()
        .map(|(_, entry)| (entry.id.clone(), entry))
        .collect::<std::collections::HashMap<_, _>>();
    let rounds = observations
        .into_iter()
        .map(|observation| {
            let hits = observation
                .retrieved_ids
                .iter()
                .map(|id| {
                    let entry = entries.get(id);
                    let injected = observation.injected_ids.contains(id);
                    let read = if injected {
                        observation.read_ids.as_ref().map(|ids| ids.contains(id))
                    } else {
                        None
                    };
                    json!({"id": id, "title": entry.map(|entry| entry.title.as_str()).unwrap_or(id),
                "scope": "project", "category": entry.map(|entry| entry.category.as_str()),
                "injected": injected, "read": read})
                })
                .collect::<Vec<_>>();
            let mut round =
                serde_json::to_value(&observation).expect("serializable memory observation");
            round["hits"] = json!(hits);
            round
        })
        .collect::<Vec<_>>();
    Ok(json!({"rounds_total": rounds.len(), "rounds": rounds}))
}

/// 使用观测只用于人工复查；高频召回和未读取均不证明无效或失败复发。
#[tauri::command]
pub(crate) fn memory_value_flags(project_dir: String) -> serde_json::Value {
    let mut zero_read = Vec::new();
    let mut frequent = Vec::new();
    let mut stale_archived = 0usize;
    for store in memory_stores_for(&project_dir) {
        let usage = store.usage_counts();
        for (_, entry) in store
            .load_all()
            .iter()
            .filter(|(_, entry)| entry.status == "active")
        {
            let Some(counts) = usage.get(&entry.id) else {
                continue;
            };
            let item = json!({"scope": store.scope.label(), "id": entry.id, "title": entry.title,
                "recalled": counts.recalled, "injected": counts.injected,
                "read": counts.read, "read_observed": counts.read_observed});
            if counts.read_observed >= 3 && counts.read == 0 {
                zero_read.push(item);
            } else if counts.recalled >= 3 {
                frequent.push(item);
            }
        }
        stale_archived += store.archived_count();
    }
    json!({"zero_read": zero_read, "frequent": frequent, "stale_archived": stale_archived})
}

#[tauri::command]
pub(crate) fn memory_entry_save(
    project_dir: String,
    scope: String,
    id: String,
    title: Option<String>,
    description: Option<String>,
    body: Option<String>,
    status: Option<String>,
) -> Result<(), String> {
    for store in memory_stores_for(&project_dir) {
        if store.scope.label() == scope {
            store
                .update(
                    &id,
                    title.as_deref(),
                    description.as_deref(),
                    body.as_deref(),
                    status.as_deref(),
                    None,
                    false, // A-005:UI 用户直写豁免主题一致性,用户有权写任何内容
                )
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err(format!("未知记忆域: {scope}"))
}

#[tauri::command]
pub(crate) fn memory_entry_delete(
    project_dir: String,
    scope: String,
    id: String,
) -> Result<(), String> {
    for store in memory_stores_for(&project_dir) {
        if store.scope.label() == scope {
            let Some((path, _)) = store.load_all().into_iter().find(|(_, e)| e.id == id) else {
                return Err(format!("记忆 {id} 不存在(可能已被删除)"));
            };
            std::fs::remove_file(&path)
                .map_err(|e| format!("删除 {} 失败: {e}", path.display()))?;
            store.refresh_derived().map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err(format!("未知记忆域: {scope}"))
}

#[tauri::command]
pub(crate) fn memory_search_page(project_dir: String, query: String) -> serde_json::Value {
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    // R-161 桌面端同源:与 memory_search 工具/CLI 开跑预检索走同一漏斗口径,
    // 命中即记 RETRIEVED(桌面搜索页只展示、不进 LLM 上下文,故 injected=false)。
    // D-366:检索走统一门面(index 是 ranking 唯一实现处),不再直调 store.search。
    let index = kanzei_tools::memory::SqliteMemoryIndex::new(&root);
    let all_hits: Vec<kanzei_tools::memory::SearchHit> = index.search_entries(
        &kanzei_tools::memory::IndexQuery::text(&query),
        None,
        None,
        8,
    );
    let out: Vec<serde_json::Value> = all_hits
        .iter()
        .map(|h| {
            json!({"id": h.entry.id, "scope": h.entry.scope, "category": h.entry.category, "title": h.entry.title, "description": h.entry.description, "status": h.entry.status, "snippet": h.snippet, "hits": h.hits})
        })
        .collect();
    {
        kanzei_tools::memory::record_memory_search_telemetry(
            &root,
            &query,
            &all_hits,
            false,
            "lexical",
            &kanzei_tools::memory::RetrievalTiming::default(),
            None,
            "user_search",
        );
    }
    json!(out)
}

// 「开发重心」的 memory_focus_get / memory_focus_set 已移除。
//
// 它们把取活序开关镜像成一条 preference 记忆,而 preference 会以 STANDING
// DIRECTIVES 的抬头全文常驻注入,与引擎 <resolved-control-state> 里那句
// "do not re-arbitrate queue priority from tracker prose" 正面对撞——同一个决策
// 两套机制、两个权威。实测让同一条规则复活三代(M-002 → M-063 → M-070):
// 每次退役后开关一切,upsert_preference 就再生一条。
//
// 取活序现在单源:前端开关 → localStorage → run.rs normalize_work_priority
// → WorkPriority → resolve_work_decision。详见 ui/08-compose.js 的说明。
//
// MemoryStore 的 find_preference/upsert_preference 保留:preference 类别与
// STANDING DIRECTIVES 注入机制本身没有问题(问题只在拿它承载引擎已经权威裁决的
// 那个决策),将来写真正的用户偏好仍要用这对原语。

#[tauri::command]
pub(crate) fn memory_context_bill(project_dir: String) -> serde_json::Value {
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let state = kanzei_core::project_state_path(&root);
    let session = kanzei_core::project_session_id(&root);
    let Ok(store) = kanzei_core::SessionStore::open(&state) else {
        return json!({"bill": [], "episodes": []});
    };
    let bill = store
        .latest_episode_context(&session)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!([]));
    let episodes: Vec<serde_json::Value> = store.list_episodes(&session, 8).unwrap_or_default().into_iter().map(|(at, prompt, outcome, steps, tools)| json!({"at": at, "prompt": prompt, "outcome": outcome, "steps": steps, "tools": serde_json::from_str::<serde_json::Value>(&tools).unwrap_or(json!({}))})).collect();
    json!({"bill": bill, "episodes": episodes})
}

#[tauri::command]
pub(crate) async fn memory_consolidate(project_dir: String) -> Result<serde_json::Value, String> {
    // 手动触发(设置页按钮)不在轮末序列里,没有"当轮 episode"可代填。
    let report = consolidate_memory_inbox(project_dir.clone(), None)
        .await
        .map_err(|error| error.to_string())?;
    let event_type = if report.has_failures() {
        "memory_consolidation_stopped"
    } else {
        "memory_consolidation_completed"
    };
    append_memory_fact(
        &project_dir,
        event_type,
        Some("memory_consolidation".into()),
        serde_json::to_value(&report).map_err(|error| error.to_string())?,
    )?;
    Ok(json!({"pending": report.pending_after, "report": report}))
}

pub(crate) async fn consolidate_memory_inbox(
    project_dir: String,
    current_episode_id: Option<i64>,
) -> anyhow::Result<kanzei_tools::memory_consolidation::ConsolidationReport> {
    kanzei_tools::memory_consolidation::consolidate_memory_for_project(
        &project_dir,
        current_episode_id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::promotion_gap_count;
    use std::collections::HashSet;
    use std::path::PathBuf;

    #[test]
    fn recalls_ipc_reads_current_state_and_keeps_unknown_distinct() {
        let root = std::env::temp_dir().join(format!(
            "kz-recalls-ipc-{}-{}",
            std::process::id(),
            super::now_ms()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        let state =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        let event = kanzei_core::RecallEvent {
            recall_id: "old",
            episode_id: None,
            step_id: None,
            trigger_type: "memory_search",
            trigger_payload: "{}",
            policy_action: "lexical",
            query: "query",
            candidate_ids: "[\"M-1\"]",
            retrieved_ids: "[\"M-1\"]",
            injected_ids: "[\"M-1\"]",
            lexical_ms: 0,
            embed_ms: 0,
            vector_ms: 0,
            total_ms: 0,
        };
        state.record_recall_event(&event).unwrap();
        state
            .record_recall_event_for_run(
                &kanzei_core::RecallEvent {
                    recall_id: "new",
                    ..event
                },
                Some("run-a"),
            )
            .unwrap();
        let result = super::memory_recalls(root.display().to_string(), Some(20)).unwrap();
        assert_eq!(result["rounds_total"], 2);
        let rounds = result["rounds"].as_array().unwrap();
        assert!(
            rounds.iter().find(|row| row["recall_id"] == "old").unwrap()["hits"][0]["read"]
                .is_null()
        );
        assert_eq!(
            rounds.iter().find(|row| row["recall_id"] == "new").unwrap()["hits"][0]["read"],
            false
        );
        state.record_memory_read("run-a", "M-1").unwrap();
        let result = super::memory_recalls(root.display().to_string(), Some(20)).unwrap();
        assert_eq!(result["rounds"][0]["hits"][0]["read"], true);
        state
            .record_recall_event(&kanzei_core::RecallEvent {
                recall_id: "corrupt",
                retrieved_ids: "not-json",
                ..event
            })
            .unwrap();
        assert!(
            super::memory_recalls(root.display().to_string(), Some(20)).is_err(),
            "损坏观测必须报告错误，不能伪装成空历史"
        );
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn entry(
        id: &str,
        status: &str,
        source: &str,
        refs: &[(&str, &str)],
    ) -> kanzei_tools::memory::MemoryEntry {
        kanzei_tools::memory::MemoryEntry {
            id: id.into(),
            scope: "project".into(),
            category: "fact".into(),
            title: id.into(),
            description: "test hook".into(),
            status: status.into(),
            created: "2026-01-01".into(),
            updated: "2026-01-01".into(),
            source: source.into(),
            extras: refs
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect(),
            body: "test".into(),
        }
    }

    #[test]
    fn promotion_gaps_uses_db_provenance_and_keeps_legacy_active_exempt() {
        let entries = vec![
            (PathBuf::new(), entry("M-legacy", "active", "user", &[])),
            (
                PathBuf::new(),
                entry("M-linked", "candidate", "manager", &[]),
            ),
            (
                PathBuf::new(),
                entry("M-unlinked", "active", "user", &[("refs", "R-1")]),
            ),
        ];
        let mut source_backed = HashSet::from(["M-linked".to_string()]);
        assert_eq!(promotion_gap_count(&entries, &source_backed), 1);
        source_backed.insert("M-unlinked".into());
        assert_eq!(promotion_gap_count(&entries, &source_backed), 0);
    }
}
