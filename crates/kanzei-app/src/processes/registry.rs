//! 进程注册与持久化(R-254 批1,纯搬迁自 processes.rs)。
//!
//! 独立理由:进程注册是「线怎么被创建、编号、落库、恢复」的变更理由——`p{n}` 编号、
//! 一树一线查重、state.db 恢复回填、持久字段权威,与「线怎么跑/怎么关」(lifecycle)、
//! 「工作树怎么建/合并/收割」(workspace)、「门禁怎么跑」(gate)互不相关。加一条
//! 注册规则不必读懂合并策略(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):编号必须看库不能只看内存表(重启后内存表为空,只看内存会
//! 撞出重复 p{n} 并把旧行的 worktree_path 改写,见 register_process 的注释);
//! 锁边界:git 子进程在全局进程表锁之外执行(restore 的分支名恢复);查重必须
//! 「内存 ∪ 库」两边都查。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::{process_info, AppState, ProcessHandle, ProcessInfo, ProjectRoot, WorktreeRoot};
use kanzei_tools::worktree as wt;

/// R-178 D3:把 state.db 里该主项目的线/进程注册合入内存进程表。
///
/// 已存在的进程(id 相同)只回填持久字段(model/profile/reasoning/勘察复核/tracker 写入),
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
    // 重启恢复时同样清理旧版留下的死线。这里只以目录不存在为准：目录仍在但未合并/
    // 有改动的线不能因为恢复而被擅自删除，保留给用户继续收活。
    let stale_ids = stored
        .iter()
        .filter(|record| {
            record
                .worktree_path
                .as_deref()
                .is_some_and(|path| !Path::new(path).is_dir())
        })
        .map(|record| record.process_id.clone())
        .collect::<Vec<_>>();
    for process_id in &stale_ids {
        store
            .delete_process(process_id)
            .map_err(|e| format!("清理失效隔离线 {process_id} 失败: {e}"))?;
    }
    // git 子进程可能在大仓上耗时,必须在全局进程表锁之外完成。否则一次 process_list
    // 恢复分支名会把 process_update/close/run_prompt 全部冻住。
    let restored = stored
        .into_iter()
        .filter(|record| !stale_ids.iter().any(|id| id == &record.process_id))
        .map(|record| {
            let branch = record.worktree_path.as_deref().and_then(|path| {
                wt::worktree_current_branch(Path::new(path))
                    .ok()
                    .map(|branch| branch.trim_start_matches("refs/heads/").to_string())
            });
            (record, branch)
        })
        .collect::<Vec<_>>();
    let mut processes = state.processes.lock().unwrap();
    for (record, restored_branch) in restored {
        let handle = processes
            .entry(record.process_id.clone())
            .or_insert_with(|| ProcessHandle {
                id: record.process_id.clone(),
                // D-367:库值是 String,恢复时转类型化主根/工作树根。
                origin_project: ProjectRoot(PathBuf::from(&record.origin_project)),
                project_dir: ProjectRoot(PathBuf::from(&record.project_dir)),
                worktree_path: record
                    .worktree_path
                    .as_ref()
                    .map(|path| WorktreeRoot(PathBuf::from(path))),
                branch: restored_branch.clone(),
                model: Arc::new(Mutex::new(None)),
                profile: Arc::new(Mutex::new(None)),
                reasoning: Arc::new(Mutex::new(None)),
                manual_models: Arc::new(Mutex::new(Vec::new())),
                phase_pipeline_enabled: Arc::new(AtomicBool::new(false)),
                tracker_writes_enabled: Arc::new(AtomicBool::new(false)),
            });
        handle.branch = restored_branch;
        // 库值回填:process_update 每次落库,库是持久字段的权威。
        *handle.model.lock().unwrap() = record.model;
        *handle.profile.lock().unwrap() = record.profile;
        *handle.reasoning.lock().unwrap() = record.reasoning;
        *handle.manual_models.lock().unwrap() = record.manual_models;
        handle
            .phase_pipeline_enabled
            .store(record.phase_pipeline, Ordering::SeqCst);
        handle
            .tracker_writes_enabled
            .store(record.tracker_writes_enabled, Ordering::SeqCst);
    }
    Ok(())
}

/// 项目首次进入时恢复持久进程注册；同一运行期后续刷新只读内存运行态。
///
/// `restore_processes_from_store` 保留为重启/测试使用的明确恢复原语，不能直接挂在
/// 高频的 `process_list` 或 `collaboration_snapshot` 路径上。
pub(crate) fn restore_processes_from_store_once(
    state: &AppState,
    root: &Path,
) -> Result<(), String> {
    let project = root.display().to_string();
    let mut restored = state.restored_projects.lock().unwrap();
    if restored.contains(&project) {
        return Ok(());
    }
    restore_processes_from_store(state, root)?;
    restored.insert(project);
    Ok(())
}

pub(crate) fn mark_project_restored(state: &AppState, root: &Path) {
    state
        .restored_projects
        .lock()
        .unwrap()
        .insert(root.display().to_string());
}

/// 把一条 ProcessHandle 的持久字段写回 state.db(process_update/process_close 用)。
///
/// 建线**不走这里**:它要的是「只插新行,撞 id 就失败」,见 `register_process`。
pub(crate) fn persist_process(root: &Path, process: &ProcessHandle) -> Result<(), String> {
    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    store
        .upsert_process(&kanzei_core::StoredProcess {
            process_id: process.id.clone(),
            origin_project: process.origin_project.0.display().to_string(),
            project_dir: process.project_dir.0.display().to_string(),
            worktree_path: process
                .worktree_path
                .as_ref()
                .map(|worktree| worktree.0.display().to_string()),
            model: process.model.lock().unwrap().clone(),
            profile: process.profile.lock().unwrap().clone(),
            reasoning: process.reasoning.lock().unwrap().clone(),
            manual_models: process.manual_models.lock().unwrap().clone(),
            phase_pipeline: process.phase_pipeline_enabled.load(Ordering::SeqCst),
            tracker_writes_enabled: process.tracker_writes_enabled.load(Ordering::SeqCst),
            updated_at: crate::run::now_ms(),
        })
        .map_err(|e| format!("进程状态落库失败: {e}"))
}

/// 一树一线被拒时的文案:必须点名是哪条线绑着它,否则用户无从下手。
pub(crate) fn bound_error(target: &Path, bound_id: &str) -> String {
    format!(
        "工作树 {} 已绑定到线 {bound_id};一棵工作树同时只能有一条线。\
         要复用这棵树就先关掉线 {bound_id},或换一个工作树名字",
        target.display()
    )
}

/// 一树一线查重:**内存进程表 ∪ state.db**,命中就给出占着它的那条线的 id。
///
/// 两边都要查。内存表是当前会话的权威(刚建出来还没被别人回读),库是重启后的权威
/// (内存表此刻是空的)。只查一边都会漏,而漏掉的后果是同名建线撞进 git 的原始报错。
pub(crate) fn bound_thread_for_worktree(
    state: &AppState,
    root: &Path,
    project: &str,
    key: &str,
) -> Result<Option<String>, String> {
    // guard 只在这个块里活着——下面要开 SQLite,绝不能带着全局进程表锁去做 IO。
    let in_memory = {
        state
            .processes
            .lock()
            .unwrap()
            .values()
            .find(|process| {
                process
                    .worktree_path
                    .as_ref()
                    .is_some_and(|worktree| wt::worktree_key(worktree.as_path()) == key)
            })
            .map(|process| process.id.clone())
    };
    if in_memory.is_some() {
        return Ok(in_memory);
    }
    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let stored = store
        .list_processes(project)
        .map_err(|e| format!("读取进程注册失败: {e}"))?;
    Ok(stored_bound_thread(&stored, key))
}

/// 库里是否已经有线绑着这棵树(按 [`worktree_key`] 归一后比较)。
pub(crate) fn stored_bound_thread(stored: &[kanzei_core::StoredProcess], key: &str) -> Option<String> {
    stored
        .iter()
        .find(|record| {
            record
                .worktree_path
                .as_deref()
                .is_some_and(|path| wt::worktree_key(Path::new(path)) == key)
        })
        .map(|record| record.process_id.clone())
}

/// `p{n}` 编号:`p<数字>|<项目>` 里的那个数字。其它形态(默认线 `d|…`)返回 None。
fn process_index(id: &str) -> Option<u64> {
    id.split('|').next()?.strip_prefix('p')?.parse::<u64>().ok()
}

/// 建线时一次带进来的线级设置(打包只为把参数个数压下来,没有别的语义)。
pub(crate) struct ThreadSettings {
    pub(crate) model: Option<String>,
    pub(crate) profile: Option<String>,
    pub(crate) reasoning: Option<String>,
    pub(crate) phase_pipeline: Option<bool>,
    pub(crate) tracker_writes: Option<bool>,
}

/// 建线的收尾:编号 → 落库 → 进内存表,全程持 `state.processes` 的同一个 guard。
///
/// # 编号必须看库,不能只看内存表
///
/// 老版只从内存表算 `max(p{n}) + 1`。重启后内存表是空的(`create_process` 不调
/// `restore_processes_from_store`),而 `state.db` 里还留着上次的 p1 —— 于是新线也叫
/// p1,`upsert_process` 的 `ON CONFLICT DO UPDATE` 把旧行**连 `worktree_path` 一起**
/// 改写,旧线绑的那棵树从此在库里失联:磁盘上树还在,库里指向别处,界面上再也找不到
/// 它,也就没有入口能收掉它。所以这里取「内存表 ∪ 库」的最大值,并且用
/// `insert_new_process`(撞 id 就失败,既有行一个字段都不动)作为第二道闸。
pub(crate) fn register_process(
    state: &AppState,
    root: &Path,
    project: &str,
    worktree_path: Option<WorktreeRoot>,
    branch: Option<String>,
    planned: Option<(&Path, &str)>,
    settings: ThreadSettings,
) -> Result<ProcessInfo, String> {
    let ThreadSettings {
        model,
        profile,
        reasoning,
        phase_pipeline,
        tracker_writes,
    } = settings;
    let mut processes = state.processes.lock().unwrap();

    // 二次查重:建树期间内存锁是放开的,`restore_processes_from_store` 可能把库里的
    // 绑定合了进来。
    if let Some((target, key)) = planned {
        if let Some(bound) = processes.values().find(|process| {
            process
                .worktree_path
                .as_ref()
                .is_some_and(|worktree| wt::worktree_key(worktree.as_path()) == key)
        }) {
            return Err(bound_error(target, &bound.id));
        }
    }

    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let stored = store
        .list_processes(project)
        .map_err(|e| format!("读取进程注册失败: {e}"))?;
    let retired = store
        .list_retired_process_ids(project)
        .map_err(|e| format!("读取退役进程身份失败: {e}"))?;
    // 二次查重的库那一半:与上面的内存那一半同为「内存 ∪ 库」,口径必须一致,
    // 否则重启后的绑定会从这道闸底下漏过去(一棵树两条线)。
    if let Some((target, key)) = planned {
        if let Some(bound) = stored_bound_thread(&stored, key) {
            return Err(bound_error(target, &bound));
        }
    }
    let next = next_process_index(&processes, &stored, &retired, project);

    let process = ProcessHandle {
        id: format!("p{next}|{project}"),
        // D-367:project_dir/origin_project 恒主根(类型化);worktree 只进 worktree_path。
        origin_project: ProjectRoot(root.to_path_buf()),
        project_dir: ProjectRoot(root.to_path_buf()),
        worktree_path,
        branch,
        model: Arc::new(Mutex::new(model.filter(|value| !value.trim().is_empty()))),
        profile: Arc::new(Mutex::new(profile.filter(|value| !value.trim().is_empty()))),
        reasoning: Arc::new(Mutex::new(
            reasoning.filter(|value| !value.trim().is_empty()),
        )),
        manual_models: Arc::new(Mutex::new(Vec::new())),
        phase_pipeline_enabled: Arc::new(AtomicBool::new(phase_pipeline.unwrap_or(false))),
        tracker_writes_enabled: Arc::new(AtomicBool::new(tracker_writes.unwrap_or(false))),
    };

    // R-178 D3:非默认线创建即落库,重启后页签与线级状态可恢复。
    let fresh = store
        .insert_new_process(&kanzei_core::StoredProcess {
            process_id: process.id.clone(),
            origin_project: process.origin_project.0.display().to_string(),
            project_dir: process.project_dir.0.display().to_string(),
            worktree_path: process
                .worktree_path
                .as_ref()
                .map(|worktree| worktree.0.display().to_string()),
            model: process.model.lock().unwrap().clone(),
            profile: process.profile.lock().unwrap().clone(),
            reasoning: process.reasoning.lock().unwrap().clone(),
            manual_models: process.manual_models.lock().unwrap().clone(),
            phase_pipeline: process.phase_pipeline_enabled.load(Ordering::SeqCst),
            tracker_writes_enabled: process.tracker_writes_enabled.load(Ordering::SeqCst),
            updated_at: crate::run::now_ms(),
        })
        .map_err(|e| format!("进程状态落库失败: {e}"))?;
    if !fresh {
        return Err(format!(
            "线 {} 在 state.db 里已经存在,拒绝覆盖既有注册(它可能绑着另一棵工作树)",
            process.id
        ));
    }

    let info = process_info(state, &process);
    processes.insert(process.id.clone(), process);
    Ok(info)
}

/// 下一个 `p{n}`:内存表与库里同项目的编号一起取最大值再加一。
fn next_process_index(
    processes: &HashMap<String, ProcessHandle>,
    stored: &[kanzei_core::StoredProcess],
    retired: &[String],
    project: &str,
) -> u64 {
    let from_memory = processes
        .values()
        .filter(|process| process.project_dir.0.display().to_string() == project)
        .filter_map(|process| process_index(&process.id));
    let from_store = stored
        .iter()
        .filter_map(|record| process_index(&record.process_id));
    let from_retired = retired.iter().filter_map(|id| process_index(id));
    from_memory
        .chain(from_store)
        .chain(from_retired)
        .max()
        .unwrap_or(0)
        + 1
}
