//! Memory commands and inbox consolidation.

use std::path::PathBuf;

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
    if !all_hits.is_empty() {
        kanzei_tools::memory::record_memory_search_telemetry(
            &root,
            &query,
            &all_hits,
            false,
            "lexical",
            &kanzei_tools::memory::RetrievalTiming::default(),
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
    let report = consolidate_memory_inbox(project_dir, None)
        .await
        .map_err(|error| error.to_string())?;
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
