//! Memory commands and inbox consolidation.

use std::path::PathBuf;
use std::sync::Arc;

use kanzei_core::{run_once_with_parts, AskFuture, RunEvent, RunnerConfig};
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};
use serde_json::json;

fn memory_stores_for(project_dir: &str) -> Vec<kanzei_tools::memory::MemoryStore> {
    let cwd = PathBuf::from(project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let mut stores = vec![kanzei_tools::memory::MemoryStore::project(&root)];
    stores.extend(kanzei_tools::memory::MemoryStore::global());
    stores
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
pub(crate) fn memory_entries(
    project_dir: String,
    scope: String,
    category: Option<String>,
) -> Result<serde_json::Value, String> {
    for store in memory_stores_for(&project_dir) {
        if store.scope.label() == scope {
            let profile = store.hit_profile();
            // R-150:召回/采纳率并入条目——recall_profile 提供 (recalled, fetched),
            // 前端据此显示采纳率与零采纳标记(零采纳候选的 UI 消费)。
            let recall = store.recall_profile();
            let list: Vec<serde_json::Value> = store.load_all().into_iter().filter(|(_, e)| category.as_deref().is_none_or(|c| e.category == c)).map(|(path, e)| { let (hits, last_hit_at) = profile.get(&e.id).copied().unwrap_or((0, 0)); let (recalled, fetched) = recall.get(&e.id).copied().unwrap_or((0, 0)); json!({"id": e.id, "category": e.category, "title": e.title, "description": e.description, "status": e.status, "updated": e.updated, "source": e.source, "refs": e.refs(), "hits": hits, "lastHitAt": last_hit_at, "recalled": recalled, "fetched": fetched, "path": path.display().to_string(), "body": e.body}) }).collect();
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
pub(crate) fn memory_recalls(project_dir: String, limit: Option<usize>) -> serde_json::Value {
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let mut rounds: Vec<kanzei_tools::memory::RecallRound> = Vec::new();
    for store in memory_stores_for(&project_dir) {
        rounds.extend(store.recalls(limit));
    }
    rounds.sort_by_key(|round| std::cmp::Reverse(round.at));
    rounds.truncate(limit);
    let total = rounds.len();
    let with_fetch = rounds
        .iter()
        .filter(|r| r.hits.iter().any(|h| h.fetched))
        .count();
    json!({"rounds": rounds, "rounds_total": total, "rounds_with_fetch": with_fetch})
}

/// R-150 空闲整理清单:零采纳候选、复发候选与 stale 积压(内容①)。
/// 零采纳 = 召回≥3 但从未拉正文;复发 = 最近回放里同一标题反复出现(暂以
/// 召回次数高且采纳为 0 的近似——精确复发标记走 fingerprint,这里给 UI 候选)。
/// stale 积压(D-217) = archive/ 里已归档条目数(引擎 archive_dead 搬运后的
/// 遗忘总量,供「待复查」提示——归档文件保留墓碑正文可回看)。
/// 只列候选不处置——处置走既有墓碑机制(memory_entry_save 降级 / delete),不静默删。
#[tauri::command]
pub(crate) fn memory_value_flags(project_dir: String) -> serde_json::Value {
    let mut zero_adopt = Vec::new();
    let mut recurring = Vec::new();
    let mut stale_archived = 0usize;
    for store in memory_stores_for(&project_dir) {
        let entries = store.load_all();
        let profile = store.recall_profile();
        // 零采纳候选:召回≥3 且从未采纳(active 条目的 UI 消费,只读)。
        for (_, e) in entries.iter().filter(|(_, e)| e.status == "active") {
            if let Some(&(recalled, fetched)) = profile.get(&e.id) {
                if recalled >= 3 && fetched == 0 {
                    zero_adopt.push(json!({"scope": store.scope.label(), "id": e.id, "title": e.title, "recalled": recalled, "fetched": 0}));
                }
            }
        }
        // 复发候选:召回轮次多、采纳为 0 的条目是「语义显著但决策无关」的头号嫌疑;
        // 与 R-149 决策权重口径一致(召回≥3 才起权),同一清单给空闲整理用。
        // 这里只汇总 active 且 recalled>=3 的条目(零采纳子集之外再加 fetched>0 的),
        // 复发信号暂用 recalled 频次近似,精确 fingerprint 复发见 R-150 文档。
        for (_, e) in entries.iter().filter(|(_, e)| e.status == "active") {
            if let Some(&(recalled, fetched)) = profile.get(&e.id) {
                if recalled >= 3 {
                    recurring.push(json!({"scope": store.scope.label(), "id": e.id, "title": e.title, "recalled": recalled, "fetched": fetched}));
                }
            }
        }
        stale_archived += store.archived_count();
    }
    json!({"zeroAdopt": zero_adopt, "recurring": recurring, "staleArchived": stale_archived})
}

/// R-132 一键整理:对零采纳候选(召回≥3 采纳=0 且 active)批量降级为 stale。
/// 走既有墓碑机制(降级不删除、可逆——UI 详情可改回 active),不静默删。
/// 返回降级清单与跳过清单,供前端给出结果反馈。
#[tauri::command]
pub(crate) fn memory_cleanup_demote(project_dir: String) -> Result<serde_json::Value, String> {
    let mut demoted = Vec::new();
    let mut skipped = Vec::new();
    for store in memory_stores_for(&project_dir) {
        let entries = store.load_all();
        let profile = store.recall_profile();
        for (_, e) in entries.iter().filter(|(_, e)| e.status == "active") {
            let Some(&(recalled, fetched)) = profile.get(&e.id) else {
                continue;
            };
            if recalled >= 3 && fetched == 0 {
                match store.update(&e.id, None, None, None, Some("stale"), None, false) {
                    Ok(updated) => demoted.push(json!({"id": e.id, "title": updated.title})),
                    Err(err) => skipped.push(json!({"id": e.id, "reason": err.to_string()})),
                }
            }
        }
    }
    Ok(json!({"demoted": demoted, "skipped": skipped}))
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
    let mut all_hits: Vec<kanzei_tools::memory::SearchHit> = Vec::new();
    for store in memory_stores_for(&project_dir) {
        if let Ok(found) = store.search(&query, None, None, 8) {
            all_hits.extend(found);
        }
    }
    all_hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_hits.truncate(8);
    let out: Vec<serde_json::Value> = all_hits
        .iter()
        .map(|h| {
            json!({"id": h.entry.id, "scope": h.entry.scope, "category": h.entry.category, "title": h.entry.title, "description": h.entry.description, "status": h.entry.status, "snippet": h.snippet, "hits": h.hits})
        })
        .collect();
    if !all_hits.is_empty() {
        kanzei_tools::memory::record_memory_search_telemetry(&root, &query, &all_hits, false);
    }
    json!(out)
}

const FOCUS_TITLE_PREFIX: &str = "开发重心";

#[tauri::command]
pub(crate) fn memory_focus_get(project_dir: String) -> serde_json::Value {
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let store = kanzei_tools::memory::MemoryStore::project(&root);
    match store.find_preference(FOCUS_TITLE_PREFIX) {
        Some(entry) => {
            json!({"id": entry.id, "title": entry.title, "body": entry.body, "updated": entry.updated})
        }
        None => serde_json::Value::Null,
    }
}

#[tauri::command]
pub(crate) fn memory_focus_set(
    project_dir: String,
    title: String,
    body: String,
) -> Result<serde_json::Value, String> {
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let store = kanzei_tools::memory::MemoryStore::project(&root);
    let entry = store
        .upsert_preference(
            FOCUS_TITLE_PREFIX,
            title.trim(),
            "取活/排优先级时必读:当前项目该先做什么",
            body.trim(),
        )
        .map_err(|e| e.to_string())?;
    Ok(json!({"id": entry.id, "title": entry.title, "body": entry.body}))
}

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
    consolidate_memory_inbox(project_dir.clone()).await;
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let store = kanzei_tools::memory::MemoryStore::project(&root);
    Ok(json!({"pending": store.pending_notes()}))
}

pub(crate) async fn consolidate_memory_inbox(project_dir: String) {
    let cwd = PathBuf::from(&project_dir);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let store = kanzei_tools::memory::MemoryStore::project(&project_root);
    if store.pending_notes() == 0 {
        return;
    }
    let inbox = store.read_inbox();
    let Ok(config) = KanzeiConfig::load(&cwd) else {
        return;
    };
    let config = Arc::new(config);
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let mut harness = Harness::default();
    harness.add(kanzei_tools::memory::MemoryManagerComponent);
    let Ok(snapshot) = harness.resolve(&rctx) else {
        return;
    };
    let agent = kanzei_tools::memory::manager_agent();
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let Ok(client) = LlmClient::new(&proxy) else {
        return;
    };
    let tool_ctx = ToolCtx {
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        ..Default::default()
    };
    let prompt = format!("Consolidate these inbox notes into durable memory entries:\n\n{inbox}");
    for role in ["primary", "fast"] {
        let Ok(resolved) = config.resolve_model(role) else {
            continue;
        };
        let Ok(route) = kanzei_core::build_route(&resolved, &proxy).await else {
            continue;
        };
        let runner_config = RunnerConfig {
            model: resolved.model.clone(),
            max_tokens: 4096,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
            ask_policy: kanzei_core::AskPolicy::NonInteractive,
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
            Box::pin(async move {
                match request {
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        let _ = run_once_with_parts(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &tool_ctx,
            &prompt,
            None,
            &[],
            None,
            None,
            &mut on_event,
            &mut ask,
        )
        .await;
        if store.pending_notes() == 0 {
            return;
        }
    }
}
