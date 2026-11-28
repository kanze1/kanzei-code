//! Read-only workbench summaries. Lifecycle recovery is installed once at application startup.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_core::SessionStore;
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::prefs::{load_prefs, AppPrefs};
use crate::processes::naming::{load_naming, order_key, process_kind, process_ordinal};
use crate::{AppState, MutexPoisonExt};

const MAX_LINES: usize = 16;
const MAX_CACHED_PROJECTS: usize = 128;
const MAX_CACHE_BYTES: usize = 4 * 1024 * 1024;
type SummaryCache = BTreeMap<String, (Value, usize)>;
static LAST_GOOD: LazyLock<Mutex<SummaryCache>> = LazyLock::new(Mutex::default);

fn remember_summary(cache: &mut SummaryCache, path: &str, summary: &Value) {
    let bytes = summary.to_string().len();
    if bytes > MAX_CACHE_BYTES {
        return;
    }
    cache.remove(path);
    while cache.len() >= MAX_CACHED_PROJECTS
        || cache.values().map(|(_, size)| *size).sum::<usize>() + bytes > MAX_CACHE_BYTES
    {
        let oldest = cache
            .iter()
            .min_by_key(|(_, (value, _))| value["observed_at"].as_u64().unwrap_or_default())
            .map(|(path, _)| path.clone());
        if let Some(oldest) = oldest {
            cache.remove(&oldest);
        } else {
            break;
        }
    }
    cache.insert(path.to_string(), (summary.clone(), bytes));
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// Registered project identity is the selected folder. Never inherit a parent's state database
// merely because an empty child folder does not yet have a .kanzei directory.
fn project_root(path: &str) -> PathBuf {
    crate::normalized_project_root(Path::new(path))
}

fn identity(prefs: &AppPrefs, path: &str) -> Value {
    json!({"path": path, "project_id": path,
        "name": prefs.names.get(path).cloned().unwrap_or_else(|| crate::projects::base_name_for_snapshot(path)),
        "current": prefs.current.as_deref() == Some(path)})
}

fn branch_at(path: &str) -> Option<String> {
    let dot_git = Path::new(path).join(".git");
    let git = if dot_git.is_file() {
        let text = std::fs::read_to_string(dot_git).ok()?;
        let git = PathBuf::from(text.strip_prefix("gitdir:")?.trim());
        if git.is_absolute() {
            git
        } else {
            Path::new(path).join(git)
        }
    } else {
        dot_git
    };
    std::fs::read_to_string(git.join("HEAD"))
        .ok()?
        .trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_string)
}

/// 线路排序键:运行中的在前,其后主对话最前、`pN` 按数字序。
fn line_order_key(line: &Value) -> (bool, (u8, u64, &str)) {
    (
        line["running"] != true,
        order_key(line["id"].as_str().unwrap_or("")),
    )
}

fn lines(
    state: &AppState,
    root: &Path,
    store: Option<&SessionStore>,
) -> Result<Vec<Value>, String> {
    let mut all = BTreeMap::new();
    if let Some(store) = store {
        for item in store
            .list_processes(&root.display().to_string())
            .map_err(|e| e.to_string())?
        {
            let available = item
                .worktree_path
                .as_deref()
                .is_none_or(|p| Path::new(p).is_dir());
            let session_id = crate::process_session_id(root, Some(&item.process_id));
            let kind = process_kind(&item.process_id, item.profile.as_deref());
            let ordinal = process_ordinal(&item.process_id);
            let naming = load_naming(store, &session_id, kind);
            all.insert(item.process_id.clone(), json!({
                "id": item.process_id, "label": naming.display(kind),
                "title": naming.title(), "title_custom": naming.custom.is_some(),
                "kind": kind.as_str(), "ordinal": ordinal, "updated_at": naming.updated_at,
                "session_id": session_id,
                "running": false, "stage": if available { "空闲" } else { "工作树不可用" },
                "branch": item.worktree_path.as_deref().and_then(branch_at), "worktree_path": item.worktree_path,
                "profile": item.profile, "available": available, "current_item_id": null,
            }));
        }
    }
    let handles = state
        .processes
        .lock_or_recover()
        .values()
        .filter(|p| p.origin_project.0 == root)
        .cloned()
        .collect::<Vec<_>>();
    for handle in handles {
        let naming = store
            .map(|store| {
                let session_id = crate::process_session_id(root, Some(&handle.id));
                load_naming(store, &session_id, crate::state::process_kind_of(&handle))
            })
            .unwrap_or_default();
        let info = crate::state::process_info_with(state, &handle, &naming);
        all.insert(
            info.id.clone(),
            json!({"id": info.id, "label": info.label,
            "title": info.title, "title_custom": info.title_custom,
            "kind": info.kind, "ordinal": info.ordinal, "updated_at": info.updated_at,
            "session_id": info.session_id, "running": info.running, "stage": info.stage,
            "branch": info.branch, "worktree_path": info.worktree_path, "profile": info.profile,
            "available": info.worktree_path.as_deref().is_none_or(|p| Path::new(p).is_dir()),
            "current_item_id": null}),
        );
    }
    let mut lines = all.into_values().collect::<Vec<_>>();
    // 运行中的在前;同组内主对话最前、pN 按数字序(p2 在 p10 之前)。
    lines.sort_by(|a, b| line_order_key(a).cmp(&line_order_key(b)));
    Ok(lines)
}

pub(crate) fn project_summary(
    state: &AppState,
    prefs: &AppPrefs,
    path: &str,
) -> Result<Value, String> {
    let root = project_root(path);
    if !root.is_dir() {
        return Err("项目目录暂时不可访问".into());
    }
    let state_path = kanzei_core::project_state_path(&root);
    let store = if state_path.exists() {
        let store = SessionStore::open_read_only(&state_path).map_err(|e| e.to_string())?;
        store.check_workspace_schema().map_err(|e| e.to_string())?;
        Some(store)
    } else {
        None
    };
    let journal = kanzei_core::project_session_id(&root);
    let mut all_lines = lines(state, &root, store.as_ref())?;
    let running_lines = all_lines
        .iter()
        .filter(|line| line["running"] == true)
        .count();
    let lines_total = all_lines.len();
    all_lines.truncate(MAX_LINES);
    let facts = store
        .as_ref()
        .map(|store| store.workspace_summary(&journal))
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| json!({"pending":0,"recent_progress":null}));
    let mut project = identity(prefs, path);
    let map = project.as_object_mut().unwrap();
    map.extend(
        json!({
            "status": if running_lines > 0 { "running" } else { "idle" },
            "updated_at": facts["recent_progress"]["at"],
            "pending_count": facts["pending"],
            "lines": all_lines,
            "lines_total": lines_total,
            "running_lines": running_lines,
            "recent_progress": facts["recent_progress"],
            "error": null
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    project["content_revision"] = json!(kanzei_core::stable_json_hash(&project));
    project["freshness"] = json!("fresh");
    project["observed_at"] = json!(now_ms());
    Ok(project)
}

pub(crate) fn overview(state: &AppState, prefs: &AppPrefs) -> Value {
    let summaries: Vec<(String, Result<Value, String>)> = if prefs.projects.len() <= 1 {
        prefs
            .projects
            .iter()
            .map(|path| (path.clone(), project_summary(state, prefs, path)))
            .collect()
    } else {
        std::thread::scope(|scope| {
            let handles: Vec<_> = prefs
                .projects
                .iter()
                .map(|path| {
                    scope.spawn(move || (path.clone(), project_summary(state, prefs, path)))
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| (String::new(), Err("项目摘要读取线程异常".into())))
                })
                .collect()
        })
    };
    let projects = summaries
        .into_iter()
        .map(|(path, result)| match result {
            Ok(project) => {
                let mut cache = LAST_GOOD.lock_or_recover();
                remember_summary(&mut cache, &path, &project);
                project
            }
            Err(error) => {
                let cached = LAST_GOOD
                    .lock_or_recover()
                    .get(&path)
                    .map(|(value, _)| value.clone());
                let stale = cached.is_some();
                let mut project = cached.unwrap_or_else(|| identity(prefs, &path));
                project
                    .as_object_mut()
                    .unwrap()
                    .extend(identity(prefs, &path).as_object().unwrap().clone());
                project["freshness"] = json!(if stale { "stale" } else { "unavailable" });
                project["error"] = json!(error);
                project
            }
        })
        .collect::<Vec<_>>();
    json!({"current": prefs.current, "projects": projects, "observed_at": now_ms()})
}

#[tauri::command(async)]
pub(crate) fn workspace_overview(state: tauri::State<'_, AppState>) -> Result<Value, String> {
    Ok(overview(&state, &load_prefs()))
}

/// Startup owns restoration. The retry loop is independent of the visible page and installed
/// exactly once; read queries never restore registrations.
pub(crate) fn install_recovery(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut first_pass = true;
        loop {
            let handle = app.clone();
            let result = tokio::task::spawn_blocking(move || {
                let state = handle.state::<AppState>();
                let mut changed = false;
                for path in load_prefs().projects {
                    let root = project_root(&path);
                    match recover_project(&state, &root) {
                        Ok(recovered) => changed |= recovered,
                        Err(error) => tracing::warn!(project = %path, %error, "workspace lifecycle recovery will retry"),
                    }
                }
                changed
            })
            .await;
            match result {
                Ok(changed) => {
                    if first_pass || changed {
                        let _ = app.emit("kz:workspace-invalidated", json!({"source":"lifecycle"}));
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "workspace recovery task failed");
                }
            }
            first_pass = false;
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}

fn recover_project(state: &AppState, root: &Path) -> Result<bool, String> {
    if !root.is_dir() || !kanzei_core::project_state_path(root).is_file() {
        return Ok(false);
    }
    let key = root.display().to_string();
    if state.restored_projects.lock_or_recover().contains(&key) {
        return Ok(false);
    }
    crate::processes::restore_processes_from_store_once(state, root)?;
    Ok(true)
}

#[cfg(test)]
mod tests;
