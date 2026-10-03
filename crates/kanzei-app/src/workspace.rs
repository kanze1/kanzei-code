//! Read-only workbench summaries. Lifecycle recovery is installed once at application startup.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_core::SessionStore;
use kanzei_tools::docstore::{self, DocKind, Entry, DEFECTS, REQUIREMENTS};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use crate::prefs::{load_prefs, AppPrefs};
use crate::processes::naming::{load_naming, order_key, process_kind, process_ordinal};
use crate::{AppState, MutexPoisonExt};

const MAX_LINES: usize = 16;
const MAX_ITEMS: usize = 2;
const MAX_TRACKER_BYTES: u64 = 2 * 1024 * 1024;
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

fn load_entries(root: &Path, kind: &DocKind) -> Result<Vec<Entry>, String> {
    let path = root.join(kind.rel_path);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("读取 {} 失败: {error}", kind.rel_path)),
    };
    let mut text = String::new();
    file.take(MAX_TRACKER_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| format!("读取 {} 失败: {error}", kind.rel_path))?;
    if text.len() as u64 > MAX_TRACKER_BYTES {
        return Err(format!(
            "{} 超出工作台摘要读取上限，请进入项目查看",
            kind.rel_path
        ));
    }
    Ok(docstore::parse(kind, &text))
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
                "id": item.process_id, "label": naming.display(kind, ordinal),
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
    for line in lines.iter_mut().take(MAX_LINES) {
        if let Some(store) = store {
            let session_id = line["session_id"].as_str().unwrap_or("");
            let runtime = state.runtimes.lock_or_recover().get(session_id).cloned();
            if line["running"] == true {
                // Historical claims remain useful ownership facts, but another run may be
                // doing unrelated work. Only this running turn can supply a running item.
                if let Some(runtime) = runtime {
                    let generation = runtime.run_generation.load(Ordering::SeqCst);
                    let run_id = {
                        let live = runtime.live.lock_or_recover();
                        (!live.flushed)
                            .then(|| live.run_id.clone())
                            .filter(|id| !id.is_empty())
                    };
                    if let Some(run_id) = run_id {
                        let claim = store
                            .latest_work_claim(session_id, Some(&run_id))
                            .map_err(|e| e.to_string())?;
                        if runtime.running.load(Ordering::SeqCst)
                            && runtime.run_generation.load(Ordering::SeqCst) == generation
                            && runtime.live.lock_or_recover().run_id == run_id
                        {
                            line["current_item_id"] = json!(claim);
                        }
                    }
                }
            } else {
                line["current_item_id"] = json!(store
                    .latest_work_claim(session_id, None)
                    .map_err(|e| e.to_string())?);
            }
        }
    }
    Ok(lines)
}

fn item_summary(
    path: &str,
    kind: &str,
    entry: &Entry,
    derived: Option<u32>,
    lines: &[Value],
    claims: &[(String, Option<String>)],
) -> Value {
    let claimed_by = entry
        .fields
        .iter()
        .find(|(key, _)| key == "取得线")
        .map(|(_, value)| value.trim())
        .filter(|v| !v.is_empty());
    let mut owners = Vec::new();
    for line in lines {
        let branch = line["branch"].as_str();
        let shared_tree = line["worktree_path"].is_null();
        let receipt_matches = line["current_item_id"] == entry.id;
        let unit_claim = claims.iter().any(|(id, owner)| {
            id == &entry.id
                && match owner.as_deref() {
                    Some(owner) => branch == Some(owner),
                    None => shared_tree && receipt_matches,
                }
        });
        let explicit_owner = claimed_by.is_some() && claimed_by == branch;
        let current_owner = if claims.iter().any(|(id, _)| id == &entry.id) {
            unit_claim
        } else if claimed_by.is_some() {
            explicit_owner
        } else {
            shared_tree
        };
        // A later reassignment may invalidate a successful receipt in this very turn.
        let exact_claim = receipt_matches && current_owner;
        // Historical doing alone cannot claim the default line or make an item "running".
        if exact_claim || unit_claim || explicit_owner {
            owners.push(json!({"id": line["id"], "label": line["label"],
                "running": line["running"] == true && exact_claim}));
        }
    }
    let (done, total) = docstore::batch_progress_with_derived_done(entry, derived);
    let declared = docstore::declared_batch_progress(entry).is_some();
    let source = if !declared {
        "none"
    } else if derived.is_some() {
        "git"
    } else {
        "markdown"
    };
    json!({"project_id": path, "kind": kind, "id": entry.id,
        "title": entry.title.chars().take(200).collect::<String>(), "status": entry.status,
        "priority": entry.fields.iter().find(|(k, _)| k == "优先级" || k.eq_ignore_ascii_case("priority")).map(|(_, v)| v),
        "batches": {"done": if declared { Some(done) } else { None }, "total": if declared { Some(total) } else { None }, "source": source},
        "claimed_by": claimed_by, "running": owners.iter().any(|line| line["running"] == true), "owner_lines": owners})
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
        if root.join(".kanzei/verification").is_dir()
            && std::fs::read_dir(root.join(".kanzei/verification"))
                .map_err(|e| e.to_string())?
                .any(|entry| {
                    entry
                        .map(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                        .unwrap_or(true)
                })
        {
            return Err("状态库缺失但仍有验证记录，暂时无法确认项目进度".into());
        }
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
    let facts = store.as_ref().map(|store| store.workspace_summary(&journal)).transpose()
        .map_err(|e| e.to_string())?.unwrap_or_else(|| json!({"active":0,"verifying":0,"ready_to_try":0,"decisions":0,"missing_facts":0,"pending":0,"recent_progress":null}));
    let claims = store
        .as_ref()
        .map(SessionStore::workspace_work_claims)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let mut entries = Vec::new();
    for (key, kind) in [("req", &REQUIREMENTS), ("defect", &DEFECTS)] {
        entries.extend(
            load_entries(&root, kind)?
                .into_iter()
                .filter(|e| e.status == kind.statuses[1])
                .map(|e| (key, e)),
        );
    }
    let batch_ids = entries
        .iter()
        .map(|(_, entry)| entry.id.clone())
        .collect::<Vec<_>>();
    let derived = if batch_ids.is_empty() {
        None
    } else {
        kanzei_tools::git_batches::completed_batches_for_entries(&root, batch_ids).ok()
    };
    let mut items = entries
        .iter()
        .map(|(kind, entry)| {
            item_summary(
                path,
                kind,
                entry,
                derived
                    .as_ref()
                    .and_then(|counts| counts.get(&entry.id))
                    .copied(),
                &all_lines,
                &claims,
            )
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| {
        (
            item["running"] != true,
            item["owner_lines"].as_array().is_none_or(Vec::is_empty),
            item["id"].as_str().unwrap_or("").to_string(),
        )
    });
    let total = items.len();
    items.truncate(MAX_ITEMS);
    let mut project = identity(prefs, path);
    let map = project.as_object_mut().unwrap();
    map.extend(json!({"status": if running_lines > 0 { "running" } else { "idle" },
        "updated_at": facts["recent_progress"]["at"], "pending_count": facts["pending"],
        "lines": all_lines, "lines_total": lines_total, "running_lines": running_lines,
        "current_items": items, "current_items_total": total,
        "counts": {"in_progress": total, "verifying": facts["verifying"], "ready_to_try": facts["ready_to_try"], "decisions": facts["decisions"], "missing_facts": facts["missing_facts"]},
        "recent_progress": facts["recent_progress"], "error": null}).as_object().unwrap().clone());
    project["content_revision"] = json!(kanzei_core::store::stable_json_hash(&project));
    project["freshness"] = json!("fresh");
    project["observed_at"] = json!(now_ms());
    Ok(project)
}

pub(crate) fn overview(state: &AppState, prefs: &AppPrefs) -> Value {
    let projects = prefs
        .projects
        .iter()
        .map(|path| match project_summary(state, prefs, path) {
            Ok(project) => {
                let mut cache = LAST_GOOD.lock_or_recover();
                remember_summary(&mut cache, path, &project);
                project
            }
            Err(error) => {
                let cached = LAST_GOOD
                    .lock_or_recover()
                    .get(path)
                    .map(|(value, _)| value.clone());
                let stale = cached.is_some();
                let mut project = cached.unwrap_or_else(|| identity(prefs, path));
                project
                    .as_object_mut()
                    .unwrap()
                    .extend(identity(prefs, path).as_object().unwrap().clone());
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

// The main route retains its existing projection; this pure projection supports the published tests.
#[cfg(test)]
pub(crate) fn details(state: &AppState, prefs: &AppPrefs, project_dir: Option<&str>) -> Value {
    let projects = prefs.projects.iter().filter(|path| project_dir.is_none_or(|selected| selected == path.as_str())).map(|path| {
        let result = (|| -> Result<Value, String> {
            let mut project = project_summary(state, prefs, path)?;
            let root = project_root(path);
            let journal = kanzei_core::project_session_id(&root);
            let state_path = kanzei_core::project_state_path(&root);
            project.as_object_mut().unwrap().extend(json!({"conversation":null,"recent_activity":[],"decisions":[],"work_units":[],"verification_jobs":[],"work_acceptances":[],"rework":{}}).as_object().unwrap().clone());
            if !state_path.exists() { return Ok(project); }
            let store = SessionStore::open_read_only(&state_path).map_err(|e| e.to_string())?;
            store.check_workspace_schema().map_err(|e| e.to_string())?;
            let decisions = store.list_decisions(&journal).map_err(|e| e.to_string())?;
            let rework = decisions.iter().filter_map(|d| d.review.as_ref()).filter_map(|r| r.rework_input_id.as_ref())
                .map(|id| store.input_status(id).map(|status| (id.clone(), status))).collect::<Result<BTreeMap<_,_>,_>>().map_err(|e| e.to_string())?;
            project["decisions"] = json!(decisions);
            project["work_units"] = json!(store.list_work_units(None).map_err(|e| e.to_string())?);
            project["work_acceptances"] = json!(store.list_work_acceptances(&journal).map_err(|e| e.to_string())?);
            project["rework"] = json!(rework);
            project["verification_jobs"] = json!(kanzei_tools::verification::list_jobs(&root).map_err(|e| e.to_string())?.into_iter().take(100).map(|job| json!({
                "id":job.id,"unit_id":job.unit_id,"status":job.status,"source_fingerprint":job.source_fingerprint,
                "snapshot":job.snapshot,"log_path":job.log_path,"exit_code":job.exit_code,
                "updated_at":job.updated_at,"error":job.error,"environment":job.environment})).collect::<Vec<_>>());
            // Detail revisions cover the actual reviewable data, separately from freshness.
            project["content_revision"] = json!(kanzei_core::store::stable_json_hash(&json!({
                "decisions":project["decisions"],"work_units":project["work_units"],
                "verification_jobs":project["verification_jobs"],"work_acceptances":project["work_acceptances"],"rework":project["rework"]})));
            Ok(project)
        })();
        result.unwrap_or_else(|error| { let mut project = identity(prefs, path); project["error"] = json!(error); project["freshness"] = json!("unavailable"); project })
    }).collect::<Vec<_>>();
    json!({"current":prefs.current,"projects":projects,"observed_at":now_ms()})
}

/// Startup owns restoration. The retry loop is independent of the visible page and installed
/// exactly once; read queries never restore registrations or recover background verification.
pub(crate) fn install_recovery(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let handle = app.clone();
            let result = tokio::task::spawn_blocking(move || {
                let state = handle.state::<AppState>();
                for path in load_prefs().projects {
                    let root = project_root(&path);
                    let recovered = recover_project(&state, &root);
                    if let Err(error) = recovered { tracing::warn!(project = %path, %error, "workspace lifecycle recovery will retry"); }
                }
            }).await;
            if let Err(error) = result {
                tracing::warn!(%error, "workspace recovery task failed");
            }
            let _ = app.emit("kz:workspace-invalidated", json!({"source":"lifecycle"}));
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}

fn recover_project(state: &AppState, root: &Path) -> Result<(), String> {
    if !root.is_dir() || !kanzei_core::project_state_path(root).is_file() {
        return Ok(());
    }
    crate::processes::restore_processes_from_store_once(state, root)?;
    kanzei_tools::verification::recover(root).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
