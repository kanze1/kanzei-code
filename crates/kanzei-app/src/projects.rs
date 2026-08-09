//! Project registry commands and per-project isolation checks.

use std::path::{Path, PathBuf};
use serde_json::json;
use crate::prefs::{load_prefs, save_prefs, AppPrefs};

fn base_name(path: &str) -> String { Path::new(path).file_name().and_then(|name| name.to_str()).filter(|name| !name.is_empty()).unwrap_or(path).to_owned() }
fn strip_verbatim(p: PathBuf) -> String { let s = p.display().to_string(); s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s) }

#[tauri::command]
pub fn projects_get() -> AppPrefs { let mut prefs = load_prefs(); prefs.projects.retain(|p| Path::new(p).is_dir()); prefs.names.retain(|path, _| prefs.projects.contains(path)); if prefs.projects.is_empty() { if let Ok(cwd) = std::env::current_dir() { prefs.projects.push(cwd.display().to_string()); } } if prefs.current.as_deref().map(|c| !Path::new(c).is_dir()).unwrap_or(true) { prefs.current = prefs.projects.first().cloned(); } save_prefs(&prefs); prefs }

#[tauri::command]
pub fn projects_init(path: String, name: Option<String>) -> Result<AppPrefs, String> { let dir = PathBuf::from(&path); std::fs::create_dir_all(&dir).map_err(|e| format!("创建项目目录失败: {e}"))?; std::fs::create_dir_all(dir.join(".kanzei")).map_err(|e| format!("创建项目配置目录失败: {e}"))?; let canonical = dir.canonicalize().map(strip_verbatim).unwrap_or(path.clone()); let mut prefs = load_prefs(); if !prefs.projects.contains(&canonical) { prefs.projects.push(canonical.clone()); } let display_name = name.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned).unwrap_or_else(|| base_name(&canonical)); prefs.names.insert(canonical.clone(), display_name); prefs.current = Some(canonical); save_prefs(&prefs); Ok(projects_get()) }

#[tauri::command]
pub fn projects_rename(path: String, name: String) -> Result<AppPrefs, String> { let name = name.trim(); if name.is_empty() { return Err("项目名称不能为空".into()); } let mut prefs = load_prefs(); if !prefs.projects.iter().any(|project| project == &path) { return Err("项目不在项目列表中".into()); } prefs.names.insert(path, name.to_owned()); save_prefs(&prefs); Ok(projects_get()) }

#[tauri::command]
pub fn projects_add(path: String) -> Result<AppPrefs, String> { let dir = PathBuf::from(&path); if !dir.is_dir() { return Err(format!("目录不存在: {path}")); } std::fs::create_dir_all(dir.join(".kanzei")).map_err(|e| format!("创建项目配置目录失败: {e}"))?; let canonical = dir.canonicalize().map(strip_verbatim).unwrap_or(path.clone()); let mut prefs = load_prefs(); if !prefs.projects.contains(&canonical) { prefs.projects.push(canonical.clone()); } prefs.current = Some(canonical); save_prefs(&prefs); Ok(projects_get()) }

fn root_has_data(root: &Path) -> bool { let k = root.join(".kanzei"); ["project", "memory"].iter().any(|sub| k.join(sub).read_dir().map(|mut d| d.next().is_some()).unwrap_or(false)) || k.join("state.db").is_file() }
pub(crate) fn ensure_project_isolated(dir: &Path) -> bool { if dir.join(".kanzei").is_dir() { return false; } let Some(resolved) = kanzei_harness::config::discover_project_root(dir) else { return false; }; if std::fs::canonicalize(&resolved).ok() == std::fs::canonicalize(dir).ok() || root_has_data(&resolved) { return false; } std::fs::create_dir_all(dir.join(".kanzei")).is_ok() }

#[tauri::command]
pub fn project_root_info(project_dir: String) -> serde_json::Value { let selected = PathBuf::from(&project_dir); let repaired = ensure_project_isolated(&selected); let resolved = kanzei_harness::config::discover_project_root(&selected).unwrap_or_else(|| selected.clone()); let same = std::fs::canonicalize(&selected).ok() == std::fs::canonicalize(&resolved).ok(); json!({"selected": selected.display().to_string(), "resolved": resolved.display().to_string(), "shared": !same, "autoRepaired": repaired}) }

#[tauri::command]
pub fn projects_isolation_report() -> serde_json::Value { let prefs = load_prefs(); let mut shared = Vec::new(); let mut repaired = Vec::new(); for path in &prefs.projects { let dir = PathBuf::from(path); if !dir.is_dir() { continue; } if ensure_project_isolated(&dir) { repaired.push(path.clone()); continue; } let resolved = kanzei_harness::config::discover_project_root(&dir).unwrap_or_else(|| dir.clone()); if std::fs::canonicalize(&resolved).ok() != std::fs::canonicalize(&dir).ok() { shared.push(json!({"project": path, "resolved": resolved.display().to_string()})); } } json!({"shared": shared, "autoRepaired": repaired}) }

#[tauri::command]
pub fn project_detach(project_dir: String) -> Result<(), String> { let dir = PathBuf::from(&project_dir); if !dir.is_dir() { return Err(format!("目录不存在: {project_dir}")); } std::fs::create_dir_all(dir.join(".kanzei").join("project")).map_err(|e| format!("创建项目空间失败: {e}"))?; let resolved = kanzei_harness::config::discover_project_root(&dir).unwrap_or_else(|| dir.clone()); if std::fs::canonicalize(&resolved).ok() != std::fs::canonicalize(&dir).ok() { return Err(format!("已创建 {}/.kanzei,但项目根仍解析为 {} —— 请检查目录权限", dir.display(), resolved.display())); } Ok(()) }

#[tauri::command]
pub async fn projects_pick() -> Result<Option<AppPrefs>, String> { let picked = rfd::AsyncFileDialog::new().pick_folder().await; match picked { Some(handle) => projects_add(handle.path().display().to_string()).map(Some), None => Ok(None) } }

fn collect_project_files(root: &Path, dir: &Path, query: &str, results: &mut Vec<String>) {
    if results.len() >= 50 { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if results.len() >= 50 { break; }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if matches!(name.as_str(), ".git" | ".kanzei" | "target" | "node_modules") { continue; }
            collect_project_files(root, &path, query, results);
        } else if path.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            if query.is_empty() || relative.to_ascii_lowercase().contains(&query.to_ascii_lowercase()) {
                results.push(relative);
            }
        }
    }
}

#[tauri::command]
pub fn project_files(project_dir: String, query: String) -> Result<Vec<String>, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    if !root.is_dir() { return Err(format!("项目目录不存在: {}", root.display())); }
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

#[tauri::command]
pub fn projects_remove(path: String) -> AppPrefs { let mut prefs = load_prefs(); prefs.projects.retain(|p| p != &path); prefs.names.remove(&path); if prefs.current.as_deref() == Some(path.as_str()) { prefs.current = prefs.projects.first().cloned(); } save_prefs(&prefs); projects_get() }
#[tauri::command]
pub fn projects_select(path: String) -> AppPrefs { let mut prefs = load_prefs(); if prefs.projects.contains(&path) { ensure_project_isolated(Path::new(&path)); prefs.current = Some(path); } save_prefs(&prefs); prefs }

pub(crate) fn base_name_for_snapshot(path: &str) -> String { base_name(path) }
