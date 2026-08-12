//! Project registry commands and per-project isolation checks.

use crate::normalized_project_root;
use crate::prefs::{load_prefs, save_prefs, AppPrefs};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn base_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_owned()
}
fn strip_verbatim(p: PathBuf) -> String {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

#[tauri::command]
pub fn projects_get() -> AppPrefs {
    let prefs = normalize_prefs(load_prefs(), |path| Path::new(path).is_dir());
    save_prefs(&prefs);
    prefs
}

fn normalize_prefs(mut prefs: AppPrefs, mut project_exists: impl FnMut(&str) -> bool) -> AppPrefs {
    prefs.projects.retain(|path| project_exists(path));
    prefs.names.retain(|path, _| prefs.projects.contains(path));
    if !prefs
        .current
        .as_ref()
        .is_some_and(|current| prefs.projects.contains(current))
    {
        prefs.current = prefs.projects.first().cloned();
    }
    prefs
}

#[tauri::command]
pub fn projects_init(path: String, name: Option<String>) -> Result<AppPrefs, String> {
    let dir = PathBuf::from(&path);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建项目目录失败: {e}"))?;
    std::fs::create_dir_all(dir.join(".kanzei"))
        .map_err(|e| format!("创建项目配置目录失败: {e}"))?;
    let canonical = dir
        .canonicalize()
        .map(strip_verbatim)
        .unwrap_or(path.clone());
    let mut prefs = load_prefs();
    if !prefs.projects.contains(&canonical) {
        prefs.projects.push(canonical.clone());
    }
    let display_name = name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| base_name(&canonical));
    prefs.names.insert(canonical.clone(), display_name);
    prefs.current = Some(canonical);
    save_prefs(&prefs);
    Ok(projects_get())
}

#[tauri::command]
pub fn projects_rename(path: String, name: String) -> Result<AppPrefs, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("项目名称不能为空".into());
    }
    let mut prefs = load_prefs();
    if !prefs.projects.iter().any(|project| project == &path) {
        return Err("项目不在项目列表中".into());
    }
    prefs.names.insert(path, name.to_owned());
    save_prefs(&prefs);
    Ok(projects_get())
}

#[tauri::command]
pub fn projects_add(path: String) -> Result<AppPrefs, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("目录不存在: {path}"));
    }
    std::fs::create_dir_all(dir.join(".kanzei"))
        .map_err(|e| format!("创建项目配置目录失败: {e}"))?;
    let canonical = dir
        .canonicalize()
        .map(strip_verbatim)
        .unwrap_or(path.clone());
    let mut prefs = load_prefs();
    if !prefs.projects.contains(&canonical) {
        prefs.projects.push(canonical.clone());
    }
    prefs.current = Some(canonical);
    save_prefs(&prefs);
    Ok(projects_get())
}

fn root_has_data(root: &Path) -> bool {
    let k = root.join(".kanzei");
    ["project", "memory"].iter().any(|sub| {
        k.join(sub)
            .read_dir()
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
    }) || k.join("state.db").is_file()
}
pub(crate) fn ensure_project_isolated(dir: &Path) -> bool {
    if dir.join(".kanzei").is_dir() {
        return false;
    }
    let Some(resolved) = kanzei_harness::config::discover_project_root(dir) else {
        return false;
    };
    if std::fs::canonicalize(&resolved).ok() == std::fs::canonicalize(dir).ok()
        || root_has_data(&resolved)
    {
        return false;
    }
    std::fs::create_dir_all(dir.join(".kanzei")).is_ok()
}

#[tauri::command]
pub fn project_root_info(project_dir: String) -> serde_json::Value {
    let selected = PathBuf::from(&project_dir);
    let repaired = ensure_project_isolated(&selected);
    let resolved = kanzei_harness::config::discover_project_root(&selected)
        .unwrap_or_else(|| selected.clone());
    let same = std::fs::canonicalize(&selected).ok() == std::fs::canonicalize(&resolved).ok();
    json!({"selected": selected.display().to_string(), "resolved": resolved.display().to_string(), "shared": !same, "autoRepaired": repaired})
}

#[tauri::command]
pub fn projects_isolation_report() -> serde_json::Value {
    let prefs = load_prefs();
    let mut shared = Vec::new();
    let mut repaired = Vec::new();
    for path in &prefs.projects {
        let dir = PathBuf::from(path);
        if !dir.is_dir() {
            continue;
        }
        if ensure_project_isolated(&dir) {
            repaired.push(path.clone());
            continue;
        }
        let resolved =
            kanzei_harness::config::discover_project_root(&dir).unwrap_or_else(|| dir.clone());
        if std::fs::canonicalize(&resolved).ok() != std::fs::canonicalize(&dir).ok() {
            shared.push(json!({"project": path, "resolved": resolved.display().to_string()}));
        }
    }
    json!({"shared": shared, "autoRepaired": repaired})
}

#[tauri::command]
pub fn project_detach(project_dir: String) -> Result<(), String> {
    let dir = PathBuf::from(&project_dir);
    if !dir.is_dir() {
        return Err(format!("目录不存在: {project_dir}"));
    }
    std::fs::create_dir_all(dir.join(".kanzei").join("project"))
        .map_err(|e| format!("创建项目空间失败: {e}"))?;
    let resolved =
        kanzei_harness::config::discover_project_root(&dir).unwrap_or_else(|| dir.clone());
    if std::fs::canonicalize(&resolved).ok() != std::fs::canonicalize(&dir).ok() {
        return Err(format!(
            "已创建 {}/.kanzei,但项目根仍解析为 {} —— 请检查目录权限",
            dir.display(),
            resolved.display()
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn projects_pick() -> Result<Option<AppPrefs>, String> {
    let picked = rfd::AsyncFileDialog::new().pick_folder().await;
    match picked {
        Some(handle) => projects_add(handle.path().display().to_string()).map(Some),
        None => Ok(None),
    }
}

fn collect_project_files(root: &Path, dir: &Path, query: &str, results: &mut Vec<String>) {
    if results.len() >= 50 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if results.len() >= 50 {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if matches!(
                name.as_str(),
                ".git" | ".kanzei" | "target" | "node_modules"
            ) {
                continue;
            }
            collect_project_files(root, &path, query, results);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if query.is_empty()
                || relative
                    .to_ascii_lowercase()
                    .contains(&query.to_ascii_lowercase())
            {
                results.push(relative);
            }
        }
    }
}

#[tauri::command]
pub fn project_files(project_dir: String, query: String) -> Result<Vec<String>, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    if !root.is_dir() {
        return Err(format!("项目目录不存在: {}", root.display()));
    }
    let mut results = Vec::new();
    collect_project_files(&root, &root, query.trim(), &mut results);
    Ok(results)
}

#[tauri::command]
pub async fn export_pick_dir() -> Result<Option<String>, String> {
    Ok(rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|handle| handle.path().display().to_string()))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExportOptions {
    pub(crate) project_dir: String,
    pub(crate) output_dir: String,
    pub(crate) include_memory: bool,
    pub(crate) include_requirements: bool,
    pub(crate) include_defects: bool,
    pub(crate) include_config: bool,
}

fn copy_export_file(
    root: &Path,
    destination: &Path,
    relative: &str,
    files: &mut Vec<String>,
) -> Result<(), String> {
    let source = root.join(relative);
    if !source.is_file() {
        return Ok(());
    }
    let target = destination.join(relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败: {e}"))?;
    }
    std::fs::copy(&source, &target).map_err(|e| format!("导出 {} 失败: {e}", source.display()))?;
    files.push(relative.replace('\\', "/"));
    Ok(())
}

fn copy_export_tree(
    source: &Path,
    destination: &Path,
    relative: &str,
    files: &mut Vec<String>,
) -> Result<(), String> {
    if !source.is_dir() {
        return Ok(());
    }
    for item in std::fs::read_dir(source).map_err(|e| format!("读取导出目录失败: {e}"))? {
        let item = item.map_err(|e| format!("读取导出条目失败: {e}"))?;
        let child_relative = Path::new(relative).join(item.file_name());
        let child_source = item.path();
        if child_source.is_dir() {
            copy_export_tree(
                &child_source,
                destination,
                &child_relative.display().to_string(),
                files,
            )?;
        } else if child_source.is_file() {
            let relative_text = child_relative.display().to_string();
            let target = destination.join(&child_relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败: {e}"))?;
            }
            std::fs::copy(&child_source, &target)
                .map_err(|e| format!("导出 {} 失败: {e}", child_source.display()))?;
            files.push(relative_text.replace('\\', "/"));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn export_project_data(options: ExportOptions) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&options.project_dir));
    let output_base = PathBuf::from(options.output_dir.trim());
    if output_base.as_os_str().is_empty() {
        return Err("请先选择导出目录".into());
    }
    std::fs::create_dir_all(&output_base).map_err(|e| format!("创建导出目录失败: {e}"))?;
    let root_canonical = root
        .canonicalize()
        .map_err(|e| format!("项目目录无法解析: {e}"))?;
    let output_canonical = output_base
        .canonicalize()
        .map_err(|e| format!("导出目录无法解析: {e}"))?;
    if output_canonical.starts_with(&root_canonical) {
        return Err("导出目录不能位于项目目录内".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let destination = output_canonical.join(format!("kanzei-export-{stamp}"));
    std::fs::create_dir_all(&destination).map_err(|e| format!("创建导出包目录失败: {e}"))?;
    let mut files = Vec::new();
    if options.include_memory {
        copy_export_tree(
            &root.join(".kanzei/memory"),
            &destination,
            ".kanzei/memory",
            &mut files,
        )?;
    }
    if options.include_requirements {
        for relative in [
            ".kanzei/project/requirements.md",
            ".kanzei/project/requirements-archive.md",
        ] {
            copy_export_file(&root, &destination, relative, &mut files)?;
        }
    }
    if options.include_defects {
        for relative in [
            ".kanzei/project/defects.md",
            ".kanzei/project/defects-archive.md",
        ] {
            copy_export_file(&root, &destination, relative, &mut files)?;
        }
    }
    if options.include_config {
        copy_export_file(&root, &destination, ".kanzei/kanzei.toml", &mut files)?;
    }
    if files.is_empty() {
        let _ = std::fs::remove_dir_all(&destination);
        return Err("没有可导出的工作资料".into());
    }
    files.sort();
    Ok(json!({ "path": destination.display().to_string(), "files": files }))
}

#[tauri::command]
pub fn projects_remove(path: String) -> AppPrefs {
    let mut prefs = load_prefs();
    prefs.projects.retain(|p| p != &path);
    prefs.names.remove(&path);
    if prefs.current.as_deref() == Some(path.as_str()) {
        prefs.current = prefs.projects.first().cloned();
    }
    save_prefs(&prefs);
    projects_get()
}
#[tauri::command]
pub fn projects_select(path: String) -> AppPrefs {
    let mut prefs = load_prefs();
    if prefs.projects.contains(&path) {
        ensure_project_isolated(Path::new(&path));
        prefs.current = Some(path);
    }
    save_prefs(&prefs);
    prefs
}

pub(crate) fn base_name_for_snapshot(path: &str) -> String {
    base_name(path)
}

#[tauri::command]
pub(crate) fn workspace_snapshot() -> Result<serde_json::Value, String> {
    let prefs = projects_get();
    let mut projects = Vec::new();
    for path in &prefs.projects {
        let root = normalized_project_root(Path::new(path));
        let session_id = kanzei_core::project_session_id(&root);
        let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
            .map_err(|e| e.to_string())?;
        let session = store
            .create_session(&session_id, &root.display().to_string(), None)
            .map_err(|e| e.to_string())?;
        let conversations =
            crate::conversation::conversation_list(path.clone(), None).unwrap_or_default();
        let pending = crate::processes::list_pending_inputs(path.clone(), None).unwrap_or_default();
        let recent = crate::conversation::conversation_trace_get(path.clone(), None, None)
            .unwrap_or_default();
        projects.push(json!({
            "path": path,
            "name": prefs.names.get(path).cloned().unwrap_or_else(|| base_name_for_snapshot(path)),
            "current": prefs.current.as_deref() == Some(path.as_str()),
            "status": session.status,
            "updated_at": session.updated_at,
            "pending_count": pending.len(),
            "conversation": conversations.first(),
            "recent_activity": recent.into_iter().rev().take(8).collect::<Vec<_>>(),
        }));
    }
    Ok(json!({ "current": prefs.current, "projects": projects }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn empty_project_preferences_stay_empty() {
        let prefs = normalize_prefs(AppPrefs::default(), |_| true);

        assert!(prefs.projects.is_empty());
        assert_eq!(prefs.current, None);
        assert!(prefs.names.is_empty());
    }

    #[test]
    fn invalid_projects_are_pruned_without_inventing_a_replacement() {
        let prefs = AppPrefs {
            projects: vec!["missing".into(), "kept".into()],
            current: Some("missing".into()),
            names: HashMap::from([
                ("missing".into(), "旧项目".into()),
                ("kept".into(), "保留项目".into()),
            ]),
        };

        let prefs = normalize_prefs(prefs, |path| path == "kept");

        assert_eq!(prefs.projects, ["kept"]);
        assert_eq!(prefs.current.as_deref(), Some("kept"));
        assert_eq!(prefs.names.len(), 1);
        assert_eq!(
            prefs.names.get("kept").map(String::as_str),
            Some("保留项目")
        );
    }
}
