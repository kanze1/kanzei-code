//! Process and worktree commands.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::State;

use crate::{
    ensure_default_process, hidden_command, normalized_project_root, process_info, process_session_id,
    AppState, ProcessHandle, ProcessInfo, WorktreeInfo,
};

#[tauri::command]
pub fn process_list(state: State<'_, AppState>, project_dir: String) -> Result<Vec<ProcessInfo>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let default = ensure_default_process(&state, &root);
    let processes = state.processes.lock().unwrap();
    let mut result = processes
        .values()
        .filter(|process| process.origin_project == root.display().to_string())
        .map(|process| process_info(&state, process))
        .collect::<Vec<_>>();
    if !result.iter().any(|item| item.id == default.id) {
        result.push(process_info(&state, &default));
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

#[tauri::command]
pub fn process_create(
    state: State<'_, AppState>,
    project_dir: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    subagent: Option<bool>,
) -> Result<ProcessInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    ensure_default_process(&state, &root);
    let project = root.display().to_string();
    let mut processes = state.processes.lock().unwrap();
    let next = processes
        .values()
        .filter(|process| process.project_dir == project && process.id.starts_with("p"))
        .filter_map(|process| process.id.split('|').next()?.strip_prefix('p')?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    let process = ProcessHandle {
        id: format!("p{next}|{project}"),
        origin_project: project.clone(),
        project_dir: project,
        worktree_path: None,
        model: Arc::new(Mutex::new(model.filter(|value| !value.trim().is_empty()))),
        profile: Arc::new(Mutex::new(profile.filter(|value| !value.trim().is_empty()))),
        reasoning: Arc::new(Mutex::new(reasoning.filter(|value| !value.trim().is_empty()))),
        subagent_enabled: Arc::new(AtomicBool::new(subagent.unwrap_or(true))),
    };
    let info = process_info(&state, &process);
    processes.insert(process.id.clone(), process);
    Ok(info)
}

#[tauri::command]
pub fn process_update(
    state: State<'_, AppState>,
    process_id: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    subagent: Option<bool>,
) -> Result<ProcessInfo, String> {
    let process = state
        .processes
        .lock()
        .unwrap()
        .get(&process_id)
        .cloned()
        .ok_or_else(|| format!("进程不存在: {process_id}"))?;
    if let Some(model) = model {
        *process.model.lock().unwrap() = Some(model).filter(|value| !value.trim().is_empty());
    }
    if let Some(profile) = profile {
        *process.profile.lock().unwrap() = Some(profile).filter(|value| !value.trim().is_empty());
    }
    if let Some(reasoning) = reasoning {
        // 空串 = 清除本进程覆盖,回落配置默认档。
        *process.reasoning.lock().unwrap() =
            Some(reasoning).filter(|value| !value.trim().is_empty());
    }
    if let Some(subagent) = subagent {
        process.subagent_enabled.store(subagent, Ordering::SeqCst);
    }
    Ok(process_info(&state, &process))
}

#[tauri::command]
pub fn process_close(state: State<'_, AppState>, process_id: String) -> Result<(), String> {
    let process = state
        .processes
        .lock()
        .unwrap()
        .get(&process_id)
        .cloned()
        .ok_or_else(|| format!("进程不存在: {process_id}"))?;
    let root = PathBuf::from(&process.project_dir);
    let session_id = process_session_id(&root, Some(&process_id));
    if let Some(runtime) = state.runtimes.lock().unwrap().get(&session_id).cloned() {
        if let Some(handle) = runtime.current_run.lock().unwrap().take() {
            handle.abort();
        }
        runtime.asks.lock().unwrap().clear();
        runtime.running.store(false, Ordering::SeqCst);
    }
    if process_id.starts_with("d|") {
        *process.model.lock().unwrap() = None;
        *process.profile.lock().unwrap() = None;
        process.subagent_enabled.store(true, Ordering::SeqCst);
    } else {
        state.processes.lock().unwrap().remove(&process_id);
    }
    Ok(())
}

fn worktree_command(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    hidden_command("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git 执行失败: {e}"))
}

fn worktree_field(root: &Path, worktree: &Path, field: &str) -> Result<String, String> {
    let output = worktree_command(worktree, &["branch", "--show-current"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        return Err(format!("工作树没有可合并分支: {}", worktree.display()));
    }
    if field == "branch" {
        Ok(branch)
    } else {
        let _ = root;
        Ok(branch)
    }
}

fn validate_worktree_path(root: &Path, worktree_path: &str) -> Result<PathBuf, String> {
    let worktree = std::fs::canonicalize(worktree_path)
        .map_err(|e| format!("工作树不存在或无法解析: {e}"))?;
    let parent = root
        .parent()
        .unwrap_or(root)
        .canonicalize()
        .unwrap_or_else(|_| root.parent().unwrap_or(root).to_path_buf());
    if !worktree.starts_with(&parent) || worktree == root {
        return Err("工作树必须位于项目同级目录,不能指向项目本身或外部路径".into());
    }
    Ok(worktree)
}

#[tauri::command]
pub fn worktree_create(project_dir: String, name: String) -> Result<WorktreeInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let safe_name: String = name
        .trim()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') { ch } else { '-' })
        .collect();
    if safe_name.is_empty() {
        return Err("工作树名称不能为空".into());
    }
    let parent = root.parent().unwrap_or(&root);
    let worktree = parent.join(format!(".kanzei-worktree-{safe_name}"));
    if worktree.exists() {
        return Err(format!("工作树已存在: {}", worktree.display()));
    }
    let branch = format!("kanzei/thread-{safe_name}");
    let output = worktree_command(&root, &[
        "worktree", "add", "-b", &branch, &worktree.display().to_string(), "HEAD",
    ])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(WorktreeInfo { path: worktree.display().to_string(), branch, files: Vec::new(), clean: true, diff: String::new() })
}

#[tauri::command]
pub fn worktree_diff(project_dir: String, worktree_path: String) -> Result<WorktreeInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let branch = worktree_field(&root, &worktree, "branch")?;
    let output = worktree_command(&root, &["-C", &worktree.display().to_string(), "status", "--porcelain"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let diff_output = worktree_command(&root, &["-C", &worktree.display().to_string(), "diff", "--no-ext-diff", "--binary"])?;
    if !diff_output.status.success() {
        return Err(String::from_utf8_lossy(&diff_output.stderr).trim().to_string());
    }
    let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
    Ok(WorktreeInfo { path: worktree.display().to_string(), branch, clean: files.is_empty(), files, diff })
}

#[tauri::command]
pub fn worktree_merge(project_dir: String, worktree_path: String) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let branch = worktree_field(&root, &worktree, "branch")?;
    let check = worktree_command(&root, &["merge-tree", "--write-tree", "HEAD", &branch])?;
    if !check.status.success() {
        return Err(format!("合并前冲突检测失败,双方改动已保留:\n{}", String::from_utf8_lossy(&check.stdout)));
    }
    let output = worktree_command(&root, &["merge", "--no-ff", &branch])?;
    if !output.status.success() {
        return Err(format!("合并未完成,请在主项目中解决并保留工作树:\n{}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(format!("已合并工作树分支 {branch};工作树仍保留,可检查后显式放弃"))
}

#[tauri::command]
pub fn worktree_discard(project_dir: String, worktree_path: String) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let output = worktree_command(&root, &["worktree", "remove", &worktree.display().to_string()])?;
    if !output.status.success() {
        return Err(format!("工作树未放弃: 工作树可能仍有未提交改动,已保留以便恢复:\n{}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(format!("已放弃工作树 {} 的工作目录;分支仍保留", worktree.display()))
}
