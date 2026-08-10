//! Process and worktree commands.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use serde_json::json;
use tauri::State;

use crate::state::hidden_command;
use crate::{
    ensure_default_process, normalized_project_root, process_info, process_session_id, AppState,
    ProcessHandle, ProcessInfo, WorktreeInfo,
};

#[tauri::command]
pub fn list_pending_inputs(
    project_dir: String,
    process_id: Option<String>,
) -> Result<Vec<kanzei_core::AdmittedInput>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let state_path = kanzei_core::project_state_path(&root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let session_id = process_session_id(&root, process_id.as_deref());
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    store
        .list_pending_inputs(&session_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_input(
    project_dir: String,
    input_id: String,
    process_id: Option<String>,
) -> Result<bool, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let state_path = kanzei_core::project_state_path(&root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let session_id = process_session_id(&root, process_id.as_deref());
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    let cancelled = store
        .cancel_input(&session_id, &input_id)
        .map_err(|error| error.to_string())?;
    if cancelled {
        store
            .append_event(
                &session_id,
                "prompt.cancelled",
                &json!({ "input_id": input_id }),
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(cancelled)
}

#[tauri::command]
pub fn process_list(
    state: State<'_, AppState>,
    project_dir: String,
) -> Result<Vec<ProcessInfo>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let default = ensure_default_process(&state, &root);
    // R-178 D3:启动/切换项目时从 state.db 恢复本项目的线/进程注册
    // (页签不丢 + 线级模型/profile/reasoning/勘察复核开关回填)。
    restore_processes_from_store(&state, &root)?;
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

/// R-178 D3:把 state.db 里该主项目的线/进程注册合入内存进程表。
///
/// 已存在的进程(id 相同)只回填持久字段(model/profile/reasoning/勘察复核),
/// 不重建——内存里的 SessionRuntime 等运行时状态属于当前会话,不能被覆盖;
/// 不存在的(重启后的线页签)新建 ProcessHandle 恢复存在性。库是字段的权威,
/// 因为 process_create/update/close 每次都同步落库。
pub(crate) fn restore_processes_from_store(state: &AppState, root: &Path) -> Result<(), String> {
    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let origin = root.display().to_string();
    let stored = store
        .list_processes(&origin)
        .map_err(|e| format!("读取进程注册失败: {e}"))?;
    let mut processes = state.processes.lock().unwrap();
    for record in stored {
        let handle = processes
            .entry(record.process_id.clone())
            .or_insert_with(|| ProcessHandle {
                id: record.process_id.clone(),
                origin_project: record.origin_project.clone(),
                project_dir: record.project_dir.clone(),
                worktree_path: record.worktree_path.clone(),
                model: Arc::new(Mutex::new(None)),
                profile: Arc::new(Mutex::new(None)),
                reasoning: Arc::new(Mutex::new(None)),
                phase_pipeline_enabled: Arc::new(AtomicBool::new(false)),
            });
        // 库值回填:process_update 每次落库,库是持久字段的权威。
        *handle.model.lock().unwrap() = record.model;
        *handle.profile.lock().unwrap() = record.profile;
        *handle.reasoning.lock().unwrap() = record.reasoning;
        handle
            .phase_pipeline_enabled
            .store(record.phase_pipeline, Ordering::SeqCst);
    }
    Ok(())
}

/// 把一条 ProcessHandle 的持久字段写回 state.db(process_create/update 共用)。
pub(crate) fn persist_process(root: &Path, process: &ProcessHandle) -> Result<(), String> {
    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    store
        .upsert_process(&kanzei_core::StoredProcess {
            process_id: process.id.clone(),
            origin_project: process.origin_project.clone(),
            project_dir: process.project_dir.clone(),
            worktree_path: process.worktree_path.clone(),
            model: process.model.lock().unwrap().clone(),
            profile: process.profile.lock().unwrap().clone(),
            reasoning: process.reasoning.lock().unwrap().clone(),
            phase_pipeline: process.phase_pipeline_enabled.load(Ordering::SeqCst),
            updated_at: crate::run::now_ms(),
        })
        .map_err(|e| format!("进程状态落库失败: {e}"))
}

#[tauri::command]
pub fn process_create(
    state: State<'_, AppState>,
    project_dir: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    // 「勘察复核」开关(阶段流水线总闸)。缺省 = 关,见 `ProcessHandle` 的字段注释。
    phase_pipeline: Option<bool>,
) -> Result<ProcessInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    ensure_default_process(&state, &root);
    let project = root.display().to_string();
    let mut processes = state.processes.lock().unwrap();
    let next = processes
        .values()
        .filter(|process| process.project_dir == project && process.id.starts_with("p"))
        .filter_map(|process| {
            process
                .id
                .split('|')
                .next()?
                .strip_prefix('p')?
                .parse::<u32>()
                .ok()
        })
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
        reasoning: Arc::new(Mutex::new(
            reasoning.filter(|value| !value.trim().is_empty()),
        )),
        phase_pipeline_enabled: Arc::new(AtomicBool::new(phase_pipeline.unwrap_or(false))),
    };
    let info = process_info(&state, &process);
    processes.insert(process.id.clone(), process.clone());
    drop(processes);
    // R-178 D3:非默认线创建即落库,重启后页签与线级状态可恢复。
    persist_process(&root, &process)?;
    Ok(info)
}

#[tauri::command]
pub fn process_update(
    state: State<'_, AppState>,
    process_id: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    // 「勘察复核」开关(阶段流水线总闸),见 `ProcessHandle` 的字段注释。
    phase_pipeline: Option<bool>,
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
    if let Some(phase_pipeline) = phase_pipeline {
        process
            .phase_pipeline_enabled
            .store(phase_pipeline, Ordering::SeqCst);
    }
    // R-178 D3:任何字段变更同步落库(含默认进程——它是「主对话」的模型/开关状态,
    // 重启后要用库值回填)。
    let root = normalized_project_root(Path::new(&process.project_dir));
    persist_process(&root, &process)?;
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
    // 自主推进控制器与进程生命周期同源；关闭后不能让下次同 ID 会话继承旧轮数。
    state.auto_runs.lock().unwrap().remove(&session_id);
    if process_id.starts_with("d|") {
        *process.model.lock().unwrap() = None;
        *process.profile.lock().unwrap() = None;
        // 默认进程不销毁,只复位;复位值必须与 ensure_default_process 的默认一致(关)。
        process
            .phase_pipeline_enabled
            .store(false, Ordering::SeqCst);
        // R-178 D3:复位后的空状态也要落库,否则重启后库里的旧值又回填回来。
        persist_process(&root, &process)?;
    } else {
        state.processes.lock().unwrap().remove(&process_id);
        // R-178 D3:线关闭即从库删除,页签不再恢复。
        let state_path = kanzei_core::project_state_path(&root);
        let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
        store
            .delete_process(&process_id)
            .map_err(|e| format!("删除进程注册失败: {e}"))?;
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
    let worktree =
        std::fs::canonicalize(worktree_path).map_err(|e| format!("工作树不存在或无法解析: {e}"))?;
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
pub async fn worktree_create(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    name: String,
) -> Result<WorktreeInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    // R-171 批4:worktree 创建是项目级写操作(新分支+工作树),接入写仲裁。
    let _lease = state
        .coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            project_root: root.clone(),
            run_id: format!("worktree_create_{}", crate::run::now_ms()),
            process_id: "worktree".into(),
            reason: "worktree create".into(),
        })
        .await
        .map_err(|e| format!("无法获取项目写租约: {e}"))?;
    let safe_name: String = name
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
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
    let output = worktree_command(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &worktree.display().to_string(),
            "HEAD",
        ],
    )?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(WorktreeInfo {
        path: worktree.display().to_string(),
        branch,
        files: Vec::new(),
        clean: true,
        diff: String::new(),
    })
}

#[tauri::command]
pub fn worktree_diff(project_dir: String, worktree_path: String) -> Result<WorktreeInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let branch = worktree_field(&root, &worktree, "branch")?;
    let output = worktree_command(
        &root,
        &[
            "-C",
            &worktree.display().to_string(),
            "status",
            "--porcelain",
        ],
    )?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let diff_output = worktree_command(
        &root,
        &[
            "-C",
            &worktree.display().to_string(),
            "diff",
            "--no-ext-diff",
            "--binary",
        ],
    )?;
    if !diff_output.status.success() {
        return Err(String::from_utf8_lossy(&diff_output.stderr)
            .trim()
            .to_string());
    }
    let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
    Ok(WorktreeInfo {
        path: worktree.display().to_string(),
        branch,
        clean: files.is_empty(),
        files,
        diff,
    })
}

#[tauri::command]
pub async fn worktree_merge(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    worktree_path: String,
) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    // R-171 批4:worktree 合并是项目级写操作,接入写仲裁。
    let _lease = state
        .coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            project_root: root.clone(),
            run_id: format!("worktree_merge_{}", crate::run::now_ms()),
            process_id: "worktree".into(),
            reason: "worktree merge".into(),
        })
        .await
        .map_err(|e| format!("无法获取项目写租约: {e}"))?;
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let branch = worktree_field(&root, &worktree, "branch")?;
    let check = worktree_command(&root, &["merge-tree", "--write-tree", "HEAD", &branch])?;
    if !check.status.success() {
        return Err(format!(
            "合并前冲突检测失败,双方改动已保留:\n{}",
            String::from_utf8_lossy(&check.stdout)
        ));
    }
    let output = worktree_command(&root, &["merge", "--no-ff", &branch])?;
    if !output.status.success() {
        return Err(format!(
            "合并未完成,请在主项目中解决并保留工作树:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(format!(
        "已合并工作树分支 {branch};工作树仍保留,可检查后显式放弃"
    ))
}

#[tauri::command]
pub async fn worktree_discard(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    worktree_path: String,
) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    // R-171 批4:worktree 放弃(remove 工作树)是项目级写操作,接入写仲裁。
    let _lease = state
        .coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            project_root: root.clone(),
            run_id: format!("worktree_discard_{}", crate::run::now_ms()),
            process_id: "worktree".into(),
            reason: "worktree discard".into(),
        })
        .await
        .map_err(|e| format!("无法获取项目写租约: {e}"))?;
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let output = worktree_command(
        &root,
        &["worktree", "remove", &worktree.display().to_string()],
    )?;
    if !output.status.success() {
        return Err(format!(
            "工作树未放弃: 工作树可能仍有未提交改动,已保留以便恢复:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(format!(
        "已放弃工作树 {} 的工作目录;分支仍保留",
        worktree.display()
    ))
}
