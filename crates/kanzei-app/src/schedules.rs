use kanzei_tools::schedules::{self as shared, ScheduleDef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::Emitter;

async fn host(root: &Path, def: &ScheduleDef, enabled: bool) -> Result<(), String> {
    match def.host.as_str() {
        "app" => Ok(()),
        "system" => shared::hosts::system(root, def, enabled).await,
        _ => shared::hosts::server(root, def, enabled).await,
    }
}
fn read_definition(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn check_definition_revision(
    previous: Option<&str>,
    expected_hash: Option<&str>,
) -> Result<(), String> {
    let current_hash = previous.map(|text| kanzei_tools::content_hash(text.as_bytes()));
    if expected_hash != current_hash.as_deref() {
        return Err("任务定义已被修改，请刷新后再保存".into());
    }
    Ok(())
}
fn replace_definition(
    path: &Path,
    expected: Option<&str>,
    text: Option<&str>,
) -> Result<(), String> {
    let _lock =
        kanzei_tools::atomic_file::try_lock_exclusive(path, std::time::Duration::from_secs(2))
            .map_err(|e| e.to_string())?
            .ok_or("任务定义正被修改，请稍后重试")?;
    let mut guard = || {
        if read_definition(path)?.as_deref() != expected {
            return Err(std::io::Error::other("任务定义已变化，请刷新后重试"));
        }
        Ok(())
    };
    if let Some(text) = text {
        kanzei_tools::atomic_file::write_atomic_bytes_guarded(path, text.as_bytes(), &mut guard)
    } else {
        guard().and_then(|_| std::fs::remove_file(path))
    }
    .map_err(|e| e.to_string())
}
fn schedule_roots() -> Vec<PathBuf> {
    let mut roots: Vec<_> = crate::prefs::load_prefs()
        .projects
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if let Ok(root) = crate::general_chat::storage_root() {
        if root.join(".kanzei/schedules").is_dir() && !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

fn scheduler_enabled(service: bool, embedded: bool, disabled: bool, e2e: bool) -> bool {
    (service || embedded) && !disabled && !e2e
}
pub(crate) async fn scheduler(app: tauri::AppHandle) {
    if !scheduler_enabled(
        crate::runtime_service::is_service(),
        crate::runtime_service::embedded(),
        std::env::var("KANZEI_SCHEDULER").is_ok_and(|value| value == "off"),
        std::env::var("KANZEI_E2E_CDP").is_ok(),
    ) {
        return;
    }
    let pulling = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        interval.tick().await;
        for root in schedule_roots() {
            if !root.join(".kanzei/schedules").is_dir() {
                continue;
            }
            let (definitions, _) = shared::load(&root);
            for def in definitions {
                if def.host.starts_with("server:") {
                    if def.enabled {
                        let key = format!("{}:{}", root.display(), def.name);
                        if !pulling.lock().unwrap().insert(key.clone()) {
                            continue;
                        }
                        let pending = pulling.clone();
                        let root = root.clone();
                        tokio::spawn(async move {
                            if let Err(error) = shared::hosts::pull(&root, &def).await {
                                tracing::warn!(%error,"schedule server pull failed");
                            }
                            pending.lock().unwrap().remove(&key);
                        });
                    }
                    continue;
                }
                if def.host != "app" {
                    continue;
                }
                let now = chrono_now();
                match shared::due(&root, &def, now) {
                    Ok(Some(slot)) => spawn(app.clone(), root.clone(), def, slot, "timer".into()),
                    Err(error) => tracing::warn!(%error,"schedule scan failed"),
                    _ => {}
                }
            }
        }
    }
}
fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
fn spawn(app: tauri::AppHandle, root: PathBuf, def: ScheduleDef, slot: i64, trigger: String) {
    tokio::spawn(async move {
        let name = def.name.clone();
        let notify = def.writeback.iter().any(|channel| channel == "notify");
        let result = shared::execute(root.clone(), def, slot, trigger).await;
        let payload = match result {
            Ok(Some(outcome)) => {
                json!({"project":root,"name":name,"phase":"finished","notify":notify,"result":outcome})
            }
            Ok(None) => return,
            Err(error) => {
                json!({"project":root,"name":name,"phase":"failed","notify":true,"error":error})
            }
        };
        let _ = app.emit("kz:schedule-run", payload);
    });
}
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn schedule_action(
    app: tauri::AppHandle,
    project_dir: String,
    action: String,
    name: Option<String>,
    definition: Option<ScheduleDef>,
    expected_hash: Option<String>,
    enabled: Option<bool>,
) -> Result<Value, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    if !root.is_dir() {
        return Err("项目目录不存在".into());
    }
    if action == "list" {
        return list_tasks(&root, chrono_now(), || {});
    }
    let name = name
        .or_else(|| definition.as_ref().map(|def| def.name.clone()))
        .ok_or("缺少任务名称")?;
    let path = shared::path(&root, &name)?;
    if action == "history" {
        return shared::history(&root, Some(&name), 100).map(|history| json!(history));
    }
    static EDIT_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    let _edit = EDIT_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let previous = read_definition(&path).map_err(|e| e.to_string())?;
    let old = previous
        .as_deref()
        .map(|text| shared::parse(text, &name))
        .transpose()
        .map_err(|diag| format!("{}:{} {}", diag.file, diag.line, diag.message))?;
    if action == "run" {
        let def = old.ok_or("任务不存在")?;
        if def.host.starts_with("server:") {
            shared::hosts::run_server_now(&root, &def).await?;
        } else {
            spawn(app, root, def, chrono_now(), "manual".into());
        }
        return Ok(json!({"queued":true}));
    }
    if !matches!(action.as_str(), "save" | "toggle" | "delete") {
        return Err("未知定时任务操作".into());
    }
    // The definition editor uses the same stale-write principle as ordinary files.
    check_definition_revision(previous.as_deref(), expected_hash.as_deref())?;
    let mut new = match action.as_str() {
        "save" => definition.ok_or("缺少任务定义")?,
        _ => old.clone().ok_or("任务不存在")?,
    };
    if new.name != name {
        return Err("任务名称与文件身份不一致".into());
    }
    if action == "toggle" {
        new.enabled = enabled.ok_or("缺少启用状态")?;
    }
    let text = if action == "toggle" {
        shared::set_enabled(previous.as_deref().unwrap(), new.enabled)
    } else {
        shared::render(&new)
    };
    shared::parse(&text, &name)
        .map_err(|diag| format!("{}:{} {}", diag.file, diag.line, diag.message))?;
    if action == "delete" {
        replace_definition(&path, previous.as_deref(), None)?;
        if let Some(old) = &old {
            if let Err(error) = host(&root, old, false).await {
                let restored = replace_definition(&path, None, previous.as_deref());
                return Err(format!(
                    "主机取消登记失败：{error}；任务定义恢复：{}",
                    restored.map(|_| "成功".into()).unwrap_or_else(|e| e)
                ));
            }
        }
        shared::record(
            &root,
            "schedule.disarmed",
            &json!({"name":name,"at_ms":chrono_now()}),
        )?;
        return Ok(json!({"deleted":true}));
    }
    replace_definition(&path, previous.as_deref(), Some(&text))?;
    let registration = async {
        if let Some(old) = &old {
            if old.host != new.host {
                host(&root, old, false).await?;
            }
        }
        host(&root, &new, new.enabled).await
    }
    .await;
    if let Err(error) = registration {
        let rollback = if previous.is_some() {
            previous.clone().unwrap()
        } else {
            shared::set_enabled(&text, false)
        };
        let restored = replace_definition(&path, Some(&text), Some(&rollback));
        if let Err(restore_error) = restored {
            return Err(format!(
                "主机注册失败：{error}；定义恢复失败：{restore_error}；请刷新并检查主机登记"
            ));
        }
        let mut host_errors = Vec::new();
        if old.as_ref().is_none_or(|old| old.host != new.host) {
            if let Err(e) = host(&root, &new, false).await {
                host_errors.push(e);
            }
        }
        if let Some(old) = &old {
            if let Err(e) = host(&root, old, old.enabled).await {
                host_errors.push(e);
            }
        }
        return Err(format!(
            "主机注册失败：{error}；定义已恢复{}",
            if host_errors.is_empty() {
                "，主机登记已恢复".into()
            } else {
                format!("；主机恢复失败：{}", host_errors.join("；"))
            }
        ));
    }
    shared::record(
        &root,
        if new.enabled {
            "schedule.armed"
        } else {
            "schedule.disarmed"
        },
        &json!({"name":name,"at_ms":chrono_now()}),
    )?;
    Ok(json!({"saved":true}))
}

fn list_tasks(root: &Path, now: i64, after_load: impl FnOnce()) -> Result<Value, String> {
    let (definitions, diagnostics) = shared::load_with_revisions(root);
    after_load();
    let mut tasks = vec![];
    for (def, revision) in definitions {
        let history = shared::history(root, Some(&def.name), 20)?;
        let next = shared::next_slot(&def, now)?;
        tasks.push(json!({"definition":def,"revision":revision,"next_ms":next,"history":history}));
    }
    Ok(json!({"tasks":tasks,"diagnostics":diagnostics}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_runtime_owners_start_the_timer() {
        assert!(
            !scheduler_enabled(false, false, false, false),
            "desktop clients must not claim execution"
        );
        assert!(scheduler_enabled(true, false, false, false));
        assert!(scheduler_enabled(false, true, false, false));
        assert!(!scheduler_enabled(true, false, true, false));
        assert!(!scheduler_enabled(false, true, false, true));
    }
    #[test]
    fn list_revision_and_definition_stay_on_the_same_read_during_an_external_edit() {
        let root = std::env::temp_dir().join(format!(
            "kz-schedule-list-{}-{}",
            std::process::id(),
            chrono_now()
        ));
        let path = shared::path(&root, "test").unwrap();
        let before = "---\nname: test\nenabled: true\nwhen: 每 15 分钟\nsteps:\n  - prompt: original\n---\nOriginal body\n";
        let external = before
            .replace("original", "external")
            .replace("Original body", "External body");
        replace_definition(&path, None, Some(before)).unwrap();
        let listed = list_tasks(&root, chrono_now(), || {
            std::fs::write(&path, &external).unwrap()
        })
        .unwrap();
        let task = &listed["tasks"][0];
        let definition: ScheduleDef = serde_json::from_value(task["definition"].clone()).unwrap();
        let rendered = shared::render(&definition);
        let current = read_definition(&path).unwrap();
        let saved = check_definition_revision(current.as_deref(), task["revision"].as_str())
            .and_then(|_| replace_definition(&path, current.as_deref(), Some(&rendered)));
        assert!(
            saved.is_err(),
            "a listed source/revision pair must reject the old save after an external edit: save={saved:?}, durable={:?}",
            read_definition(&path).unwrap()
        );
        assert_eq!(definition, shared::parse(before, "test").unwrap());
        assert_eq!(
            task["revision"],
            kanzei_tools::content_hash(before.as_bytes()),
            "displayed source A must never carry later version B's CAS fingerprint"
        );
        assert_ne!(
            task["revision"],
            kanzei_tools::content_hash(external.as_bytes())
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), external);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_deleted_definition_rejects_its_old_editor_but_allows_an_explicit_new_task() {
        let root = std::env::temp_dir().join(format!(
            "kz-schedule-deleted-{}-{}",
            std::process::id(),
            chrono_now()
        ));
        let path = shared::path(&root, "task").unwrap();
        let previous = "---\nname: task\nenabled: true\nwhen: 每 15 分钟\nsteps:\n  - prompt: inspect\n---\nOld body\n";
        let stale_edit = shared::render(&shared::parse(previous, "task").unwrap());
        let revision = kanzei_tools::content_hash(previous.as_bytes());
        replace_definition(&path, None, Some(previous)).unwrap();
        check_definition_revision(read_definition(&path).unwrap().as_deref(), Some(&revision))
            .unwrap();
        replace_definition(&path, Some(previous), None).unwrap();
        let current = read_definition(&path).unwrap();
        let saved = check_definition_revision(current.as_deref(), Some(&revision))
            .and_then(|_| replace_definition(&path, current.as_deref(), Some(&stale_edit)));
        assert!(
            saved.is_err(),
            "a missing definition must not turn a stale edit into creation: save={saved:?}, recreated={}",
            path.exists()
        );
        assert!(!path.exists());
        check_definition_revision(current.as_deref(), None).unwrap();
        let new = previous.replace("Old body", "New body");
        shared::parse(&new, "task").unwrap();
        replace_definition(&path, None, Some(&new)).unwrap();
        assert_eq!(
            read_definition(&path).unwrap().as_deref(),
            Some(new.as_str())
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn timer_scans_general_tasks_without_registering_a_project() {
        let home = std::env::temp_dir().join(format!(
            "kz-general-scheduler-{}-{}",
            std::process::id(),
            chrono_now()
        ));
        crate::settings::with_kanzei_home(&home, || {
            let root = crate::general_chat::storage_root().unwrap();
            std::fs::create_dir_all(root.join(".kanzei/schedules")).unwrap();
            assert!(schedule_roots().contains(&root));
            assert!(crate::prefs::load_prefs().projects.is_empty());
            assert!(!home.join("app.json").exists());
        });
        std::fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn definition_changes_and_rollback_never_replace_an_external_edit() {
        let root = std::env::temp_dir().join(format!(
            "kz-schedule-editor-{}-{}",
            std::process::id(),
            chrono_now()
        ));
        let path = root.join("task.md");
        replace_definition(&path, None, Some("original")).unwrap();
        replace_definition(&path, Some("original"), Some("new")).unwrap();
        std::fs::write(&path, "external edit").unwrap();
        assert!(replace_definition(&path, Some("new"), Some("original")).is_err());
        assert!(replace_definition(&path, Some("new"), None).is_err());
        assert_eq!(
            read_definition(&path).unwrap().as_deref(),
            Some("external edit")
        );
        replace_definition(&path, Some("external edit"), None).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
