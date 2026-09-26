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
    kanzei_tools::path_form::simplify(&p).display().to_string()
}

/// 项目空间首次落 `.kanzei/` 时同步创建先行调研骨架。返回 true 只表示本次是
/// 首次初始化；重复选择/添加项目绝不覆盖已经填写的 prior-art 工件。
///
/// UI2-0926 #13:首次初始化同时写 `.kanzei/.gitignore`(state.db*、*.lock、.write-log/、
/// artifacts/ 等运行时文件不进版本库;project/*.md 照常入库)。已有 `.kanzei` 的老项目不补写——
/// 它们多半在自己的根 .gitignore 里管着(kanzei 仓库就是),凭空多出一个未跟踪文件只会添乱。
fn initialize_kanzei_space(dir: &Path) -> Result<bool, String> {
    let first_init = !dir.join(".kanzei").exists();
    std::fs::create_dir_all(dir.join(".kanzei"))
        .map_err(|e| format!("创建项目配置目录失败: {e}"))?;
    if first_init {
        kanzei_tools::prior_art::start_project_init(dir)?;
        kanzei_tools::project_state::ensure_kanzei_gitignore(dir)
            .map_err(|e| format!("写 .kanzei/.gitignore 失败: {e}"))?;
    }
    Ok(first_init)
}

/// 新项目名:能直接当 Windows 目录名用。
fn validate_project_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("项目名称不能为空".into());
    }
    if name.chars().count() > 100 {
        return Err("项目名称太长(最多 100 个字)".into());
    }
    if let Some(bad) = name.chars().find(|c| {
        matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return Err(format!("项目名称不能包含「{bad}」"));
    }
    if name.ends_with('.') || name == ".." {
        return Err("项目名称不能以「.」结尾".into());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|word| stem == *word)
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        return Err(format!("「{name}」是 Windows 保留名,换一个名字"));
    }
    Ok(name.to_string())
}

/// 目录里除 `.kanzei` 外有没有别的东西。
fn has_content_besides_kanzei(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| entry.file_name() != ".kanzei")
        })
        .unwrap_or(false)
}

fn register_project(dir: &Path, display_name: Option<&str>) -> AppPrefs {
    let canonical = dir
        .canonicalize()
        .map(strip_verbatim)
        .unwrap_or_else(|_| dir.display().to_string());
    let mut prefs = load_prefs();
    if !prefs.projects.contains(&canonical) {
        prefs.projects.push(canonical.clone());
    }
    if let Some(name) = display_name.map(str::trim).filter(|v| !v.is_empty()) {
        prefs.names.insert(canonical.clone(), name.to_owned());
    }
    prefs.current = Some(canonical);
    save_prefs(&prefs);
    projects_get()
}

/// UI2-0926 #13 新建项目(对话框):在 `parent` 下建 `name` 目录 + `.kanzei`(含运行时忽略规则),
/// `git_init` 时建独立仓库;本机配了 git 身份就再做一次首提交(并行线要 HEAD)。
/// `description` 原样带回,前端放进输入框当第一条消息的草稿(不自动发送)。
/// 新建项目的磁盘部分(不碰 app.json,可单测):返回 (目录, git 结果, git 失败原因)。
pub(crate) fn create_project_dir(
    parent: &str,
    name: &str,
    git_init: bool,
) -> Result<
    (
        PathBuf,
        Option<kanzei_tools::project_state::GitInitOutcome>,
        Option<String>,
    ),
    String,
> {
    let name = validate_project_name(name)?;
    let parent_dir = PathBuf::from(parent.trim());
    if !parent_dir.is_dir() {
        return Err(format!("位置不存在: {}", parent_dir.display()));
    }
    let dir = parent_dir.join(&name);
    if dir.exists() && (!dir.is_dir() || has_content_besides_kanzei(&dir)) {
        return Err(format!(
            "「{}」已存在且不是空目录;换个名字,或用「打开文件夹…」打开它",
            dir.display()
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建项目目录失败: {e}"))?;
    initialize_kanzei_space(&dir)?;
    // 已有的空 `.kanzei`(比如上次创建到一半)同样补上忽略规则。
    kanzei_tools::project_state::ensure_kanzei_gitignore(&dir)
        .map_err(|e| format!("写 .kanzei/.gitignore 失败: {e}"))?;
    if !git_init {
        return Ok((dir, None, None));
    }
    match kanzei_tools::project_state::git_init(&dir, true) {
        Ok(outcome) => Ok((dir, Some(outcome), None)),
        // git 不在 PATH 上等:项目照样建好,界面给出警告。
        Err(error) => Ok((dir, None, Some(error))),
    }
}

#[tauri::command]
pub fn projects_create(
    parent: String,
    name: String,
    git_init: bool,
    description: Option<String>,
) -> Result<serde_json::Value, String> {
    let (dir, git, git_error) = create_project_dir(&parent, &name, git_init)?;
    let prefs = register_project(&dir, Some(name.trim()));
    let root = crate::normalized_project_root(&dir);
    Ok(json!({
        "prefs": prefs,
        "path": root.display().to_string(),
        "facts": kanzei_tools::project_state::probe(&root),
        "git": git,
        "gitError": git_error,
        "description": description.map(|text| text.trim().to_string()).filter(|text| !text.is_empty()),
    }))
}

/// UI2-0926 #13:给已有的非 Git 项目建独立仓库(横幅/芯片的「初始化 Git」)。只 init + 补
/// `.kanzei/.gitignore`,不自动提交——已有文件里可能有不该进库的东西,首提交交给用户或 agent。
/// 项目位于上级仓库内时也在项目根建嵌套仓库(前端先确认过)。
#[tauri::command]
pub fn project_git_init(project_dir: String) -> Result<serde_json::Value, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    if !root.is_dir() {
        return Err(format!("项目目录不存在: {}", root.display()));
    }
    let outcome = kanzei_tools::project_state::git_init(&root, false)?;
    Ok(json!({
        "git": outcome,
        "facts": kanzei_tools::project_state::probe(&root),
    }))
}

/// UI2-0926 #13:项目状态事实(与 agent 上下文里的 `<project-state>` 同源)。
#[tauri::command]
pub fn project_facts(project_dir: String) -> Result<serde_json::Value, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    if !root.is_dir() {
        return Err(format!("项目目录不存在: {}", root.display()));
    }
    serde_json::to_value(kanzei_tools::project_state::probe_cached(&root))
        .map_err(|e| e.to_string())
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
    initialize_kanzei_space(&dir)?;
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
    initialize_kanzei_space(&dir)?;
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
    initialize_kanzei_space(dir).is_ok()
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
    initialize_kanzei_space(&dir)?;
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

#[cfg(test)]
mod prior_art_init_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    // ── 分区:工作目录管理(UI2-0926 #13)──

    fn temp_parent(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-project-create-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn 新建项目_建目录_忽略规则_git_默认建库() {
        let parent = temp_parent("git");
        let (dir, git, error) =
            create_project_dir(&parent.display().to_string(), "MD文件保存", true).unwrap();
        assert_eq!(dir, parent.join("MD文件保存"));
        assert!(dir.join(".kanzei").is_dir());
        let ignore = std::fs::read_to_string(dir.join(".kanzei/.gitignore")).unwrap();
        assert!(
            ignore.contains("state.db-*") && ignore.contains("*.lock"),
            "{ignore}"
        );
        assert!(ignore.contains("artifacts/"), "{ignore}");
        if error.is_none() {
            let git = git.expect("git 结果");
            assert!(git.created);
            assert!(dir.join(".git").exists());
            // 本机有 git 身份就有首提交(并行线要 HEAD),否则明确标出缺身份。
            assert!(git.committed || git.identity_missing, "{git:?}");
        }
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn 新建项目_不勾_git_只建目录_非空目标拒绝() {
        let parent = temp_parent("plain");
        let (dir, git, _) =
            create_project_dir(&parent.display().to_string(), "demo", false).unwrap();
        assert!(git.is_none());
        assert!(!dir.join(".git").exists());
        std::fs::create_dir_all(parent.join("taken")).unwrap();
        std::fs::write(parent.join("taken").join("x.txt"), "x").unwrap();
        let err = create_project_dir(&parent.display().to_string(), "taken", false).unwrap_err();
        assert!(err.contains("打开文件夹"), "{err}");
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn 项目名校验() {
        assert!(validate_project_name("  ").is_err());
        assert!(validate_project_name("a/b").is_err());
        assert!(validate_project_name("a:b").is_err());
        assert!(validate_project_name("con").is_err());
        assert!(validate_project_name("COM3.txt").is_err());
        assert!(validate_project_name("dots.").is_err());
        assert_eq!(
            validate_project_name(" 手机 Markdown ").unwrap(),
            "手机 Markdown"
        );
        assert_eq!(validate_project_name("console").unwrap(), "console");
    }

    #[test]
    fn 首次初始化创建prior_art骨架且重复进入不覆盖() {
        let root = std::env::temp_dir().join(format!(
            "kz-project-prior-art-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        assert!(initialize_kanzei_space(&root).unwrap());
        let path = root.join(".kanzei/research/project-init/prior-art.md");
        assert!(path.is_file());
        std::fs::write(&path, "用户已填写").unwrap();
        assert!(!initialize_kanzei_space(&root).unwrap());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "用户已填写");
    }
}

/// 工作区 = **跨项目的运行现场**:一眼看出哪个项目在跑、跑的是哪条线、卡在哪。
/// 原来这里只给「项目 + 当前对话 + 最近活动」——那些侧栏和文档页里全都有,
/// 等于把别处的信息又摆了一遍。真正只有这里能回答的是「另外那个项目现在怎么样」,
/// 所以补 lines:每条线的运行态、阶段、正在用的工具、归属分支。
#[tauri::command(async)]
pub(crate) fn workspace_snapshot(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<serde_json::Value, String> {
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
        // 线级现场。process_list 已经做了「恢复注册 + 剪掉死线」,直接复用它的结论,
        // 不在这里另写一套枚举——两套口径迟早会对不上。
        let lines = crate::processes::process_list(state.clone(), path.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|info| {
                json!({
                    "id": info.id,
                    "label": info.label,
                    "running": info.running,
                    "stage": info.stage,
                    "branch": info.branch,
                    "worktree_path": info.worktree_path,
                    "profile": info.profile,
                })
            })
            .collect::<Vec<_>>();
        let running_lines = lines
            .iter()
            .filter(|line| line["running"].as_bool() == Some(true))
            .count();
        projects.push(json!({
            "path": path,
            "name": prefs.names.get(path).cloned().unwrap_or_else(|| base_name_for_snapshot(path)),
            "current": prefs.current.as_deref() == Some(path.as_str()),
            "status": session.status,
            "updated_at": session.updated_at,
            "pending_count": pending.len(),
            "conversation": conversations.first(),
            "recent_activity": recent.into_iter().rev().take(8).collect::<Vec<_>>(),
            "lines": lines,
            "running_lines": running_lines,
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
            ..Default::default()
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
