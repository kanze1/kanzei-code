//! Process and worktree commands.
//!
//! # 字段口径(F4 定死,先读这一段再改本文件)
//!
//! `ProcessHandle.project_dir` 与 `ProcessHandle.origin_project` **恒为主根**
//! (`normalized_project_root` 的规范化形态);一条线的执行工作树**只**由
//! `worktree_path` 承担。这不是风格偏好,是四处反推主根的代码逼出来的硬约束:
//!
//! - 本文件 `create_process` 的 `p{n}` 计数按 `project_dir` 分桶——改存 worktree
//!   后每棵树各自从 p1 开始,编号立刻撞车;
//! - 本文件 `process_update` 与 `process_close` 都用 `project_dir` 反推 root 去
//!   开 `state.db`——改存 worktree 会把库落进工作树,线一关就连同工作树一起没了;
//! - `state.rs` 的 `process_info` 用 `project_dir` 算 `session_id`——改存 worktree
//!   等于给同一条线换身份串,会话历史集体失联(D-176 红线)。
//!
//! 正因为 `project_dir` 恒主根,`p{n}` 在一个项目内已经唯一,`session_id` 才不
//! 需要再加 worktree 后缀(§0 定案 2 的前提)。`store/schema.rs` 的 processes 表
//! 注释同批更正为这个口径。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use kanzei_harness::orchestration::{ProjectExecutionCoordinator, WriterLease, WriterLeaseRequest};
use kanzei_tools::docstore::{DEFECTS, REQUIREMENTS};
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
    restore_processes_from_store_once(&state, &root)?;
    // 外部 `git worktree remove`、旧版「放弃工作树」都会留下已绑进程但目录消失的
    // 记录。列表刷新是用户可见的恢复点，必须先收掉这些死线，不能让它们继续出现在
    // 页签里，直到发送时才以不存在的 cwd 失败。
    prune_missing_worktree_processes(&state, &root)?;
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
                worktree_field(root, Path::new(path), "branch")
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
                origin_project: record.origin_project.clone(),
                project_dir: record.project_dir.clone(),
                worktree_path: record.worktree_path.clone(),
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
            origin_project: process.origin_project.clone(),
            project_dir: process.project_dir.clone(),
            worktree_path: process.worktree_path.clone(),
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

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri IPC 参数保持独立可选字段,避免前端契约套一层临时对象。
pub async fn process_create(
    state: State<'_, AppState>,
    project_dir: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    // 「勘察复核」开关(阶段流水线总闸)。缺省 = 关,见 `ProcessHandle` 的字段注释。
    phase_pipeline: Option<bool>,
    // 仅分支线有意义:允许该线更新主根中的唯一 tracker 文档。缺省 = 关。
    tracker_writes: Option<bool>,
    // 给定则同时建一棵工作树并绑到这条线上;缺省(Tauri 对未传的 Option 参数解析为
    // None)保持今天的行为,worktree_path 恒 None。
    worktree_name: Option<String>,
) -> Result<ProcessInfo, String> {
    create_process_with_tracker(
        &state,
        &project_dir,
        model,
        profile,
        reasoning,
        phase_pipeline,
        tracker_writes,
        worktree_name,
    )
    .await
}

/// `process_create` 的非 Tauri 内核。
///
/// 拆出来是为了能测:`State<'_, AppState>` 在单元测试里构造不出来,而本批要验的
/// 事(真实绑定 / 一树一线 / 失败整体回滚 / 并发不互相破坏)全在这段逻辑里。
///
/// # 并发下的正确性靠什么(K2' 返工的根因:上一版靠错了东西)
///
/// 上一版把「预检 → 建树 → 绑定落库」罩进项目**写租约**,以为竞态就此消失。**没有。**
/// `MemoryCoordinator` 是 `AppState` 里的进程内内存对象(设计基线 §6.2 明写:`kz` CLI、
/// 自举循环、第二个 kzapp 实例都看不见它),所以那条破坏一字未减,只是从「线程之间」
/// 搬到了「进程之间」:两个并发建同名树的调用者,输的一方的回滚照旧
/// `worktree remove --force` + 删分支,掉的是**赢家刚建好的**树和分支。上一版为此写下的
/// 免责理由(「跨进程那一层由 git 自己兜底」)是错的 —— **git 的失败正是触发破坏的那一步**。
///
/// 现在正确性不靠任何锁,靠 git 自己的原子性:`git branch <name> <base>` 的 ref 创建是
/// CAS(已存在即失败),把它当作**认领**并让它先行,见
/// [`create_worktree_with_receipt`]。认领失败 ⇒ 本次调用什么都没建出来 ⇒ 零回滚。
/// 这条不变量跨进程成立,不依赖协调器。
///
/// # 写租约为什么还留着
///
/// 它对**同进程内**的仲裁与审计仍有价值(`worktree_create`/`merge`/`discard` 三条命令
/// 已在用同一个入口,建线不进来就会与它们乱序),但**不再是正确性的依靠**。租约获取带
/// 上界([`WRITE_LEASE_TIMEOUT`]),超时报明确错误 —— 否则一条卡死的 writer 能让建线
/// 永久 pending,而 app 里够不到取消入口。
///
/// # 锁边界(为什么 git 的耗时调用全在内存锁外)
///
/// `git worktree add` 是一次全量检出,`status` + `diff` 在大仓上也要几百毫秒到数秒。
/// 它们若压在 `state.processes` 的 guard 里,这段时间 `process_list` /
/// `process_update` / `process_close` / `run_prompt` 全部卡在同一把锁上——界面对所有线
/// 冻结。所以内存锁只在两个**极短**的临界区里取:查重一次、编号+落库+插表一次;
/// 步骤 ③ 的全部 git 调用在锁外。(落库那次临界区里有一次 SQLite 打开+写入,毫秒级;
/// 放在锁内是为了让「编号 ⇒ 落库 ⇒ 插表」保持原子,否则两条不带 worktree 的建线
/// ——它们不取租约——会算出同一个 `p{n}`。)
///
/// # 不带 worktree 的建线为什么不取租约
///
/// 它一个字节都不写项目工作区,只往 `state.db` 加一行。让它去排项目写租约,等于把
/// 「新开一条线」这个按钮挂在正在跑的 writer 后面等,UX 上不可接受,收益是零。
#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn create_process(
    state: &AppState,
    project_dir: &str,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    phase_pipeline: Option<bool>,
    worktree_name: Option<String>,
) -> Result<ProcessInfo, String> {
    create_process_with_tracker(
        state,
        project_dir,
        model,
        profile,
        reasoning,
        phase_pipeline,
        None,
        worktree_name,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn create_process_with_tracker(
    state: &AppState,
    project_dir: &str,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    phase_pipeline: Option<bool>,
    tracker_writes: Option<bool>,
    worktree_name: Option<String>,
) -> Result<ProcessInfo, String> {
    let root = normalized_project_root(Path::new(project_dir));
    ensure_default_process(state, &root);
    // 恒主根:见本文件头的「字段口径」。worktree 路径只进 worktree_path。
    let project = root.display().to_string();
    let worktree_name = worktree_name.filter(|value| !value.trim().is_empty());

    // ① 要建树才取写租约,与 worktree_create 同一个仲裁入口。
    let _lease = match worktree_name.as_deref() {
        Some(_) => {
            Some(acquire_project_write_lease(state, &root, "process create worktree").await?)
        }
        None => None,
    };

    // ② 一树一线查重(建树之前:被拒时磁盘上一棵树都不许多出来)。
    //    查的是**内存表 ∪ state.db**:同一个函数里的编号分配一直是查库的,查重只扫内存
    //    表就自相矛盾——重启后内存表是空的,于是同名建线绕过查重、一路撞到
    //    `create_worktree` 的目录预检,给出的文案会教用户 `worktree remove --force`
    //    一棵**仍被库里某条线绑着、且可能带未提交改动**的活树,还完全不点名那条线。
    let planned = match worktree_name.as_deref() {
        Some(name) => {
            let (target, _) = worktree_target(&root, name)?;
            let key = worktree_key(&target);
            if let Some(bound) = bound_thread_for_worktree(state, &root, &project, &key)? {
                return Err(bound_error(&target, &bound));
            }
            Some((target, key))
        }
        None => None,
    };

    // ③ 建树。耗时的 git 调用全在内存锁之外,原子性由 ① 的写租约兜着。
    //    失败直接返回:create_worktree 自己已经把残留收干净(收不掉的会在错误里点名)。
    let created = match worktree_name.as_deref() {
        Some(name) => Some(create_worktree_with_receipt(&root, name)?),
        None => None,
    };
    let (worktree_path, branch, receipt) = match created {
        Some((info, receipt)) => (Some(info.path), Some(info.branch), Some(receipt)),
        None => (None, None, None),
    };

    // ④ 编号 + 落库 + 插内存表(一个临界区内完成)。任一步失败就整体回滚,
    //    绝不留半绑定态——磁盘上有树、库里没线是最坏结局:界面上看不见它,
    //    也就没有任何入口能把它收掉。
    match register_process(
        state,
        &root,
        &project,
        worktree_path,
        branch,
        planned
            .as_ref()
            .map(|(target, key)| (target.as_path(), key.as_str())),
        ThreadSettings {
            model,
            profile,
            reasoning,
            phase_pipeline,
            tracker_writes,
        },
    ) {
        Ok(info) => Ok(info),
        Err(error) => Err(match receipt {
            Some(receipt) => with_residue(error, rollback_worktree(&root, &receipt)),
            None => error,
        }),
    }
}

/// 一树一线被拒时的文案:必须点名是哪条线绑着它,否则用户无从下手。
fn bound_error(target: &Path, bound_id: &str) -> String {
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
fn bound_thread_for_worktree(
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
                    .as_deref()
                    .is_some_and(|path| worktree_key(Path::new(path)) == key)
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
fn stored_bound_thread(stored: &[kanzei_core::StoredProcess], key: &str) -> Option<String> {
    stored
        .iter()
        .find(|record| {
            record
                .worktree_path
                .as_deref()
                .is_some_and(|path| worktree_key(Path::new(path)) == key)
        })
        .map(|record| record.process_id.clone())
}

/// `p{n}` 编号:`p<数字>|<项目>` 里的那个数字。其它形态(默认线 `d|…`)返回 None。
fn process_index(id: &str) -> Option<u32> {
    id.split('|').next()?.strip_prefix('p')?.parse::<u32>().ok()
}

/// 建线时一次带进来的线级设置(打包只为把参数个数压下来,没有别的语义)。
struct ThreadSettings {
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    phase_pipeline: Option<bool>,
    tracker_writes: Option<bool>,
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
fn register_process(
    state: &AppState,
    root: &Path,
    project: &str,
    worktree_path: Option<String>,
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
                .as_deref()
                .is_some_and(|path| worktree_key(Path::new(path)) == key)
        }) {
            return Err(bound_error(target, &bound.id));
        }
    }

    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let stored = store
        .list_processes(project)
        .map_err(|e| format!("读取进程注册失败: {e}"))?;
    // 二次查重的库那一半:与上面的内存那一半同为「内存 ∪ 库」,口径必须一致,
    // 否则重启后的绑定会从这道闸底下漏过去(一棵树两条线)。
    if let Some((target, key)) = planned {
        if let Some(bound) = stored_bound_thread(&stored, key) {
            return Err(bound_error(target, &bound));
        }
    }
    let next = next_process_index(&processes, &stored, project);

    let process = ProcessHandle {
        id: format!("p{next}|{project}"),
        origin_project: project.to_string(),
        project_dir: project.to_string(),
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
            origin_project: process.origin_project.clone(),
            project_dir: process.project_dir.clone(),
            worktree_path: process.worktree_path.clone(),
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
    project: &str,
) -> u32 {
    let from_memory = processes
        .values()
        .filter(|process| process.project_dir == project)
        .filter_map(|process| process_index(&process.id));
    let from_store = stored
        .iter()
        .filter_map(|record| process_index(&record.process_id));
    from_memory.chain(from_store).max().unwrap_or(0) + 1
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn process_update(
    state: State<'_, AppState>,
    process_id: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    // 项目级手填模型候选(provider:model 列表)。R-178 批3:前端「＋ 手填模型…」
    // 写这条通道,不再以 localStorage 为真源。
    manual_models: Option<Vec<String>>,
    // 「勘察复核」开关(阶段流水线总闸),见 `ProcessHandle` 的字段注释。
    phase_pipeline: Option<bool>,
    tracker_writes: Option<bool>,
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
    if let Some(manual_models) = manual_models {
        *process.manual_models.lock().unwrap() = manual_models;
    }
    if let Some(phase_pipeline) = phase_pipeline {
        process
            .phase_pipeline_enabled
            .store(phase_pipeline, Ordering::SeqCst);
    }
    if let Some(tracker_writes) = tracker_writes {
        process
            .tracker_writes_enabled
            .store(tracker_writes, Ordering::SeqCst);
    }
    // R-178 D3:任何字段变更同步落库(含默认进程——它是「主对话」的模型/开关状态,
    // 重启后要用库值回填)。
    let root = normalized_project_root(Path::new(&process.project_dir));
    persist_process(&root, &process)?;
    mark_project_restored(&state, &root);
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
        *runtime.stage.lock().unwrap() = "空闲".into();
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
        process
            .tracker_writes_enabled
            .store(false, Ordering::SeqCst);
        // R-178 D3:复位后的空状态也要落库,否则重启后库里的旧值又回填回来。
        persist_process(&root, &process)?;
    } else {
        // 先处置工作树,再删绑定行。顺序是硬的:绑定行一删,这棵树在库里就再也查不到,
        // 处置逻辑连它绑给谁都说不出来。
        let disposal = process
            .worktree_path
            .as_deref()
            .map(|worktree| reclaim_worktree_on_close(&root, Path::new(worktree)));
        unregister_parallel_process(&state, &root, &process_id)?;
        if let Some(Err(kept)) = disposal {
            // 留下来的树此刻已经无主(绑定行删了)。它至少得在**审计流里可发现**,
            // 否则磁盘上有树、库里没线、界面上没入口,三缺一地彻底失联。
            let state_path = kanzei_core::project_state_path(&root);
            let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
            let _ = store.create_session(&session_id, &root.display().to_string(), None);
            let _ = store.append_event(
                &session_id,
                "worktree.orphaned",
                &json!({ "process_id": process_id, "detail": kept }),
            );
        }
    }
    Ok(())
}

/// 注销一条非默认进程及其持久化登记。工作树已被成功摘除、启动恢复发现目录消失、
/// 或用户显式关线时都复用这一出口，避免只删目录却留下会话继续拿它当 cwd。
pub(crate) fn unregister_parallel_process(
    state: &AppState,
    root: &Path,
    process_id: &str,
) -> Result<(), String> {
    if process_id.starts_with("d|") {
        return Err("默认进程不能注销".into());
    }
    // 先删持久登记；失败时保留内存绑定，避免半截状态把仍可恢复的线藏掉。
    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    store
        .delete_process(process_id)
        .map_err(|e| format!("删除进程注册失败: {e}"))?;

    let session_id = process_session_id(root, Some(process_id));
    if let Some(runtime) = state.runtimes.lock().unwrap().get(&session_id).cloned() {
        if let Some(handle) = runtime.current_run.lock().unwrap().take() {
            handle.abort();
        }
        runtime.asks.lock().unwrap().clear();
        runtime.running.store(false, Ordering::SeqCst);
        *runtime.stage.lock().unwrap() = "空闲".into();
    }
    state.auto_runs.lock().unwrap().remove(&session_id);
    state.processes.lock().unwrap().remove(process_id);
    Ok(())
}

/// 高频列表刷新也要修复本运行期里被外部删除的树。恢复阶段只处理 state.db；这里
/// 处理已在内存中的绑定，保证用户点一次刷新就能从旧版遗留状态恢复。
fn prune_missing_worktree_processes(state: &AppState, root: &Path) -> Result<(), String> {
    let project = root.display().to_string();
    let stale_ids = state
        .processes
        .lock()
        .unwrap()
        .values()
        .filter(|process| {
            process.origin_project == project
                && process
                    .worktree_path
                    .as_deref()
                    .is_some_and(|path| !Path::new(path).is_dir())
        })
        .map(|process| process.id.clone())
        .collect::<Vec<_>>();
    for process_id in stale_ids {
        unregister_parallel_process(state, root, &process_id)?;
    }
    Ok(())
}

/// 关线时对绑定工作树的处置(K2' 重要3)。
///
/// 老版关线**一个 git 命令都不发**:库里的绑定行删了,树和分支留在磁盘上无人认领,
/// app 内再没有任何入口能收 —— 之后同名建线就撞进「工作树已存在」/「分支已存在」。
///
/// # 语义定死:只自动回收「证明得了一文不值」的那一棵
///
/// 两条同时成立才删:
/// 1. 工作区**干净**(`status --porcelain` 空,含未跟踪文件);
/// 2. 分支已经是主 HEAD 的**祖先**(活已经合并进去了)。
///
/// 两条成立 ⇒ 这棵树和这条分支里没有任何独有内容,删掉零损失;这也正是「收活之后关线」
/// 这条主流程的形态,用户按下关闭就该干净收场。任何一条不成立就**原样留着** ——
/// 里面可能是几个小时还没提交的活,自动删掉是丢工作。
///
/// `Err(说明)` = 没删,说明里带着路径、分支与可执行的回收命令(调用方落进审计流)。
fn reclaim_worktree_on_close(root: &Path, worktree: &Path) -> Result<(), String> {
    let branch = worktree_field(root, worktree, "branch")
        .map_err(|error| format!("工作树 {} 的分支查不出来: {error}", worktree.display()))?;
    let (files, _) = worktree_status(root, worktree)
        .map_err(|error| format!("工作树 {} 的状态查不出来: {error}", worktree.display()))?;
    let recycle_hint = format!(
        "回收命令:`git -C \"{}\" worktree remove --force \"{}\"` 与 \
         `git -C \"{}\" branch -D {branch}`",
        git_arg_path(root),
        git_arg_path(worktree),
        git_arg_path(root),
    );
    if !files.is_empty() {
        return Err(format!(
            "工作树 {} 有 {} 处未提交改动,关线时保留未删(分支 {branch})。{recycle_hint}",
            worktree.display(),
            files.len(),
        ));
    }
    let merged = worktree_command(root, &["merge-base", "--is-ancestor", &branch, "HEAD"])
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !merged {
        return Err(format!(
            "分支 {branch} 还没并进主线(工作树 {} 干净但提交没合过来),关线时保留未删。\
             {recycle_hint}",
            worktree.display(),
        ));
    }
    // 干净且已合并:先摘树(不加 --force —— 上面刚验过干净,真要 force 才删得掉说明
    // 判断和现实对不上,那就该保留),再按 sha 做 CAS 删分支。
    let sha = rev_parse(root, &format!("refs/heads/{branch}"));
    let removed = worktree_command(root, &["worktree", "remove", &git_arg_path(worktree)])
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !removed {
        return Err(format!(
            "工作树 {} 干净且已合并,但 git 没能摘掉它(可能有程序占着里面的文件),\
             关线时保留未删。{recycle_hint}",
            worktree.display(),
        ));
    }
    match sha {
        Some(sha) => {
            let refname = format!("refs/heads/{branch}");
            let _ = worktree_command(root, &["update-ref", "-d", &refname, &sha]);
            if branch_exists(root, &branch) {
                return Err(format!(
                    "工作树 {} 已摘掉,但分支 {branch} 没删成(它可能已经不在关线时那个 \
                     sha 上)。若确认可丢弃:`git -C \"{}\" branch -D {branch}`",
                    worktree.display(),
                    git_arg_path(root),
                ));
            }
            Ok(())
        }
        None => Err(format!(
            "工作树 {} 已摘掉,但分支 {branch} 的 sha 解析不出来,没敢删它。\
             若确认可丢弃:`git -C \"{}\" branch -D {branch}`",
            worktree.display(),
            git_arg_path(root),
        )),
    }
}

fn worktree_command(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    hidden_command("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git 执行失败: {e}"))
}

/// 把路径变成可以交给 git 当**命令行参数**的形态(剥掉 Windows 扩展长度前缀)。
///
/// `normalized_project_root` 会 `canonicalize`,Windows 上产出 `\\?\C:\...`。
/// 2026-08-11 实测:该形态作为进程的 `current_dir` 完全可用(Win32 API 认),但
/// **作为 git 的参数一律失败** —— `git worktree add -b b \\?\C:\x\wt HEAD` 报
/// `could not create leading directories of '//?/C:/x/wt/.git': Invalid argument`。
/// 所以凡是把路径交给 git 当参数的地方都得先过这一层,否则「建线」在 Windows 上
/// 一次都成功不了(R-177 验收①)。反过来 `current_dir` 不必剥,保持原样即可。
fn git_arg_path(path: &Path) -> String {
    let raw = path.display().to_string();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string()
}

/// 一树一线查重用的路径键。
///
/// 查重必须发生在**建树之前**(D4 定案),那时目标目录还不存在、`canonicalize`
/// 必然失败,所以这里是「能规范化就规范化,不能就退回字面量归一」:统一分隔符、
/// 剥扩展长度前缀、去尾分隔符,Windows 上再小写。已绑定的线存的是已存在的真实
/// 路径(canonicalize 成功),目标路径存的是尚不存在的字面量(canonicalize 失败),
/// 两条路径经过同一套归一后仍然可比——这是查重能对上的原因。
fn worktree_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let raw = resolved.display().to_string().replace('\\', "/");
    let stripped = match raw.strip_prefix("//?/UNC/") {
        Some(rest) => format!("//{rest}"),
        None => raw.strip_prefix("//?/").unwrap_or(&raw).to_string(),
    };
    let trimmed = stripped.trim_end_matches('/');
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_string()
    }
}

/// 把任意字符串压成能当目录名/分支名用的形态。
fn sanitize_component(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

/// 工作树的目标路径与分支名——只算不落盘。
///
/// 与 `create_worktree` 分开,是因为一树一线查重要在建树**之前**拿到目标路径
/// (D4 定案:目标树已被绑定则拒绝,此时一棵树都不许多出来)。
///
/// # 路径里必须带项目名
///
/// 老版是 `root.parent().join(".kanzei-worktree-<name>")` —— 不含项目名。同一个父目录
/// 下放两个项目(`~/code/a`、`~/code/b`,这是常见布局)时,两边各建一棵叫 `dev` 的
/// 工作树会**落到同一条路径** `~/code/.kanzei-worktree-dev`:后建的那边看见目录已存在
/// 直接失败,而错误说的是「工作树已存在」,用户在自己项目里根本找不到它;更糟的是
/// 回滚会去动另一个项目的目录。加上项目名之后,同一父目录下的项目目录名必然互不相同,
/// 冲突面就此消失。
///
/// # 项目名与工作树名之间的分隔符不能是 `-`
///
/// [`sanitize_component`] 把一切非 `[A-Za-z0-9_-]` 压成 `-`,所以两个分量里都可能出现
/// `-`。用 `-` 连接就还原不出边界:「项目 `a` + 名字 `b-c`」与「项目 `a-b` + 名字 `c`」
/// 都落成 `.kanzei-worktree-a-b-c`,两个不同项目的工作树撞进同一条路径——正是上面刚
/// 修掉的那类冲突换个形态回来。用 `.` 连接就没有歧义:`sanitize_component` 永远产不出
/// `.`(它是被压掉的那一类),所以第一个 `.` 必然是分隔符本身。
pub(crate) fn worktree_target(root: &Path, name: &str) -> Result<(PathBuf, String), String> {
    let safe_name = sanitize_component(name);
    if safe_name.is_empty() {
        return Err("工作树名称不能为空".into());
    }
    let parent = root.parent().unwrap_or(root);
    let project_tag = root
        .file_name()
        .map(|value| sanitize_component(&value.to_string_lossy()))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "project".into());
    Ok((
        parent.join(format!(".kanzei-worktree-{project_tag}.{safe_name}")),
        // 分支名不必带项目名:分支活在本仓库里,跨项目不共享命名空间。
        format!("kanzei/thread-{safe_name}"),
    ))
}

/// 工作树的真实工作区状态:未提交文件清单 + diff。
///
/// `create_worktree` 与 `worktree_diff` 共用同一次探测。建线以前返回的是硬编码
/// 乐观值(空 files / clean=true / 空 diff),收活流程会把「线还有活没提交」当成
/// 干净合并——那是丢工作,不是显示问题。
pub(crate) fn worktree_status(
    root: &Path,
    worktree: &Path,
) -> Result<(Vec<String>, String), String> {
    let target = git_arg_path(worktree);
    let output = worktree_command(root, &["-C", &target, "status", "--porcelain"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let diff_output =
        worktree_command(root, &["-C", &target, "diff", "--no-ext-diff", "--binary"])?;
    if !diff_output.status.success() {
        return Err(String::from_utf8_lossy(&diff_output.stderr)
            .trim()
            .to_string());
    }
    Ok((
        files,
        String::from_utf8_lossy(&diff_output.stdout).to_string(),
    ))
}

/// 目录残留挡住这个名字时的文案(D-004 口径:必须自带可执行的解法)。
///
/// 这条预检是「名字被目录占死」唯一的出口。走到这里时**一树一线查重已经放行**
/// (`create_process` 那一侧查的是内存表 ∪ state.db),也就是说没有任何一条线绑着它 ——
/// 它是无主残留:上一条线关闭时留下的、或上次回滚没收干净的。里面**可能有未提交改动**,
/// 所以文案先给「看一眼」,再给「删掉」,最后才给分支那一半。
fn residual_worktree_error(root: &Path, worktree: &Path, branch: &str) -> String {
    format!(
        "工作树已存在: {};app 的线清单里没有线绑着它(无主残留:上一条线关闭时留下,\
         或上次回滚没收干净),里面可能还有未提交改动。\
         先看一眼 `git -C \"{}\" status --porcelain`;确认可丢弃后执行 \
         `git -C \"{}\" worktree remove --force \"{}\"`,该命令报「不是工作树」就直接\
         删掉这个目录;同名分支 {branch} 若也要一并回收,再执行 \
         `git -C \"{}\" branch -D {branch}`,然后重试",
        worktree.display(),
        git_arg_path(worktree),
        git_arg_path(root),
        git_arg_path(worktree),
        git_arg_path(root),
    )
}

/// 认领(`git branch`)失败时的文案。
///
/// 最常见的成因是**分支残留**:「放弃工作树」只删目录、分支照旧留着(`worktree_discard`
/// 的返回值自己写着「分支仍保留」),于是同名再建撞上它。老版把 git 的原文
/// `fatal: a branch named '…' already exists` 直接抛给用户 —— 不带任何解法,而 app 里
/// **根本没有删分支的入口**,这个名字就此变成死结。按 D-004 口径,这里必须点名
/// 「是一条已存在的**分支**占着这个名字」(不是目录残留)并给出可执行动作。
///
/// 判「是不是撞名」用的是 `branch_exists` 复查,不是匹配 git 的错误文本 —— git 的输出
/// 会被本地化,匹配字符串在中文 git 上直接失效。
fn branch_claim_error(root: &Path, branch: &str, worktree: &Path, git_error: &str) -> String {
    if branch_exists(root, branch) {
        format!(
            "这个工作树名字被一条已存在的分支占着: {branch}(不是目录残留:{} 并不存在)。\
             本次调用没有创建任何东西,磁盘与 git 状态一个字节都没动。\
             app 里没有删分支的入口,请先确认它上面没有要保留的提交:\
             `git -C \"{}\" log --oneline HEAD..{branch}`;确认可丢弃后执行 \
             `git -C \"{}\" branch -D {branch}` 再重试,想保留就换一个工作树名字",
            worktree.display(),
            git_arg_path(root),
            git_arg_path(root),
        )
    } else {
        format!("认领分支 {branch} 失败,本次建线没有创建任何东西(磁盘与 git 状态未变): {git_error}")
    }
}

/// 本地分支是否已经存在。用 `rev-parse --verify` 走全名 `refs/heads/<branch>`,
/// 不用 `branch --list`(那是 glob 匹配)。
fn branch_exists(root: &Path, branch: &str) -> bool {
    let refname = format!("refs/heads/{branch}");
    worktree_command(root, &["rev-parse", "--verify", "--quiet", &refname])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 回滚凭据:本次调用**确实建出来了什么**,以及删它的判据。
///
/// # 核心不变量(K2' 定死,改本文件先读这一段)
///
/// **只回滚本次调用亲手创建出来的东西;认领(`git branch`)失败即证明没有任何东西是
/// 我创建的,因此一个字节都不许删。**
///
/// 这条不变量在类型层面就成立,不靠任何调用方自觉:凭据**只在认领成功之后构造**
/// ([`create_worktree_with_receipt`] 里,目录预检与 `git branch` 两道闸都过了才有它),
/// 所以「有凭据」⇔「这棵树的目录与这条分支都是本次调用建出来的」。认领失败与目录残留
/// 两条路径**根本产不出凭据**,回滚代码对它们不可达 —— 这就是「零回滚」的机械形态。
/// 它不依赖锁,跨进程成立。
///
/// 分支那一侧还有第二道判据:只有停在**建出来时那个 sha** 上才允许删(线上已经有提交
/// 就删不动)。见 [`Self::branch_sha`]。
pub(crate) struct WorktreeReceipt {
    pub(crate) worktree: PathBuf,
    pub(crate) branch: String,
    /// `Some(sha)` = 认领成功那一刻这条分支停在 `sha`;回滚用
    /// `git update-ref -d <ref> <sha>` 做**原子比较后删除**——ref 已经不在这个 sha 上
    /// (线上已经有提交 / 别人重建了它)就删不动,这正是要的。
    ///
    /// `None` = sha 解析不出来(git 出错等),无法做 CAS;回滚一律不碰,也不当残留报。
    pub(crate) branch_sha: Option<String>,
}

/// 40/64 位十六进制且非全零才算一个可用作 CAS 判据的对象名。
fn is_object_name(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value.chars().all(|ch| ch.is_ascii_hexdigit())
        && value.chars().any(|ch| ch != '0')
}

/// 解析一个 ref 的对象名;解析不出来(不存在 / git 出错 / 形态不对)返回 None。
///
/// 全零对象名必须挡掉:`git update-ref -d <ref> 000…0` 不是「比较后删除」,
/// 它的语义是另一回事,拿它当判据等于把 CAS 退化成无条件删除。
fn rev_parse(root: &Path, refname: &str) -> Option<String> {
    let output = worktree_command(root, &["rev-parse", "--verify", "--quiet", refname]).ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    is_object_name(&value).then_some(value)
}

/// git 是否仍把这条路径登记为本仓库的工作树。
fn worktree_is_registered(root: &Path, worktree: &Path) -> bool {
    let key = worktree_key(worktree);
    worktree_command(root, &["worktree", "list", "--porcelain"])
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.strip_prefix("worktree "))
                .any(|path| worktree_key(Path::new(path.trim())) == key)
        })
        .unwrap_or(false)
}

/// 工作树的 git 管理目录(`<worktree>/.git` 是一个写着 `gitdir: …` 的文件)。
///
/// 拿它是为了**定点**摘掉登记项,替掉老版的 `git worktree prune`——见
/// [`discard_worktree`] 里的说明。
fn worktree_admin_dir(worktree: &Path) -> Option<PathBuf> {
    let marker = std::fs::read_to_string(worktree.join(".git")).ok()?;
    let path = PathBuf::from(marker.trim().strip_prefix("gitdir:")?.trim());
    path.is_dir().then_some(path)
}

/// 建工作树(非 Tauri 内核):`worktree_create` 命令与 `create_process` 建线共用。
///
/// **它本身不取写租约**:两个调用方都在外层取(`worktree_create` →
/// [`create_worktree_arbitrated`],建线 → `create_process`),租约不可重入,在这里再取
/// 一次就是自己等自己。而正确性本来也不由租约提供,见下。
///
/// # 并发安全靠 git 的 ref CAS,不靠锁(K2' 返工)
///
/// 老版是 `git worktree add -b <branch> <path> HEAD`:**建分支与建树是同一条命令**,
/// 于是「这条分支是不是我建的」只能靠**调用前采样**的布尔量去猜。采样是过去时——并发
/// 下别人在采样与 add 之间把树和分支建好了,自己的 add 因此失败,清理块就拿着过期判断
/// 去 `worktree remove --force` + 删分支,**删的是赢家刚建好的东西**。上一版拿写租约挡
/// 这个窗口,但协调器是进程内对象,换成两个 OS 进程(kz CLI / 自举循环 / 第二个 kzapp)
/// 破坏原样复现。
///
/// 现在把「认领」与「建树」拆开,并让认领先行:
///
/// 1. `git branch <branch> HEAD` —— **ref 创建是原子的(CAS),已存在即失败**。
///    成功 ⇒ 这条分支是我建的,这个结论**跨进程**成立,由 git 保证;
///    失败 ⇒ 别人(或用户)拥有这个名字 ⇒ **报错返回,零回滚**。
/// 2. `git worktree add <path> <branch>` —— 挂到一条已经归我的分支上。
/// 3. 失败回滚:因为第 1 步成功过,我确知分支归我 ⇒ 按 sha 做 CAS 删除;目录也只可能是
///    本次建出来的(第 0 步的 `worktree.exists()` 预检已排除事前存在)。
///
/// 核心不变量与它的类型层面形态见 [`WorktreeReceipt`]。
///
/// # `git worktree add` 之后的每一步都必须能回滚
///
/// 认领成功之后有两条失败路径,两条都会在磁盘与 git 里留下**界面上没有任何入口能收掉**
/// 的残留,所以两条都得自己收干净:
///
/// 1. **`add` 自己失败**:分支已经落地(第 1 步建的)。不删它,下一次同名建线就撞
///    `a branch named '…' already exists`,**第二次起永久失败**。
/// 2. **`add` 成功、后面的工作区探测失败**:树已经挂上、分支已经落地,用 `?` 直接抛错
///    就留下一棵孤儿树 + 一条孤儿分支。`create_process` 的整体回滚**救不了它** ——
///    那一层只回滚它自己那一步(落库),`create_worktree` 返回 Err 时它认为这里已经
///    什么都没建出来。
pub(crate) fn create_worktree(root: &Path, name: &str) -> Result<WorktreeInfo, String> {
    create_worktree_with_receipt(root, name).map(|(info, _)| info)
}

/// [`create_worktree`] 加一张回滚凭据:建线要拿它在落库失败时回滚。
pub(crate) fn create_worktree_with_receipt(
    root: &Path,
    name: &str,
) -> Result<(WorktreeInfo, WorktreeReceipt), String> {
    let (worktree, branch) = worktree_target(root, name)?;
    // ⓪ 目录残留预检。在认领之前,所以走到这里一定**零回滚**(还没有凭据)。
    if worktree.exists() {
        return Err(residual_worktree_error(root, &worktree, &branch));
    }
    // ① 原子认领:ref 创建是 CAS,已存在即失败。成功即拥有,跨进程成立。
    let claim = worktree_command(root, &["branch", &branch, "HEAD"])?;
    if !claim.status.success() {
        // 零回滚:第 1 步失败即证明本次调用没有创建出任何东西,一个字节都不许删。
        let failure = String::from_utf8_lossy(&claim.stderr).trim().to_string();
        return Err(branch_claim_error(root, &branch, &worktree, &failure));
    }
    // 认领成功 ⇒ 分支归我 ⇒ 从这里起才有凭据(见 WorktreeReceipt 的不变量)。
    let receipt = WorktreeReceipt {
        branch_sha: rev_parse(root, &format!("refs/heads/{branch}")),
        worktree,
        branch,
    };
    // ② 挂树到已经归我的那条分支上(不再用 `-b`:认领已经在第 1 步做完了)。
    let output = worktree_command(
        root,
        &[
            "worktree",
            "add",
            &git_arg_path(&receipt.worktree),
            &receipt.branch,
        ],
    )?;
    if !output.status.success() {
        let failure = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(with_residue(failure, discard_worktree(root, &receipt)));
    }
    let (files, diff) = match worktree_status(root, &receipt.worktree) {
        Ok(probed) => probed,
        Err(error) => return Err(with_residue(error, rollback_worktree(root, &receipt))),
    };
    Ok((
        WorktreeInfo {
            path: git_arg_path(&receipt.worktree),
            branch: receipt.branch.clone(),
            clean: files.is_empty(),
            files,
            diff,
            // 刚建出来还没绑线;绑定发生在 process_create 里,清单由 worktree_list 合并。
            bound_process: None,
        },
        receipt,
    ))
}

/// 回收一条建到一半的线:摘工作树 + 按凭据删分支。
///
/// `Err(残留说明)` = 有东西没收掉,里面点名了残留路径和可执行的清理动作。
pub(crate) fn rollback_worktree(root: &Path, receipt: &WorktreeReceipt) -> Result<(), String> {
    discard_worktree(root, receipt)
}

/// 把失败原因与回滚残留拼成一条错误。
///
/// D-004 口径:回滚收不干净是用户**必须**知道的事(不清理掉,这个工作树名字就一直
/// 用不了),不能像老版那样 `let _ =` 吞掉。
fn with_residue(error: String, rollback: Result<(), String>) -> String {
    match rollback {
        Ok(()) => error,
        Err(residue) => format!("{error}\n{residue}"),
    }
}

/// 回滚的实现:先 remove 工作树(分支正被它 checkout,不先摘就删不掉),再按凭据删分支。
///
/// 目录删得下手的依据是 [`WorktreeReceipt`] 的不变量:凭据只在认领成功之后构造,而认领
/// 之前那道 `worktree.exists()` 预检已经排除了「目录事前就在」,所以凭据里的目录必然是
/// 本次调用建出来的。分支那一侧另有 sha CAS 兜底。
///
/// # 为什么不再调 `git worktree prune`
///
/// 老版在 remove 失败的兜底路径上调 `git worktree prune`。prune 是**全仓**操作:
/// 2026-08-11 实测,它会把仓里所有「目录当前不可达」的工作树登记项一并摘掉 ——
/// 包括跟本次目标毫无关系的、用户只是把目录临时挪走或放在未挂载盘上的那些。
/// 而 add 失败路径**必然**走到这个兜底(目标压根没登记成功,`worktree remove` 一定
/// 失败),于是每一次建线失败都在静默地改写全仓的工作树清单。
///
/// 替代品是定点摘除:删目录之前先从 `<worktree>/.git` 读出这棵树自己的管理目录,
/// 目录删掉之后只删那一个。实测 git 在 add 失败时会自己清掉半成品登记项,所以这条
/// 路多数时候无事可做——但它至少不会碰别人。
fn discard_worktree(root: &Path, receipt: &WorktreeReceipt) -> Result<(), String> {
    let target = git_arg_path(&receipt.worktree);
    let removed = worktree_command(root, &["worktree", "remove", "--force", &target])
        .map(|output| output.status.success())
        .unwrap_or(false);
    let mut dir_error = None;
    if !removed {
        // 先记下管理目录(读的是还没被删的 `<worktree>/.git`),删完目录再定点摘。
        let admin = worktree_admin_dir(&receipt.worktree);
        if let Err(error) = std::fs::remove_dir_all(&receipt.worktree) {
            if receipt.worktree.exists() {
                dir_error = Some(error.to_string());
            }
        }
        if !receipt.worktree.exists() {
            if let Some(admin) = admin {
                let _ = std::fs::remove_dir_all(admin);
            }
        }
    }

    let mut branch_residue = false;
    if let Some(sha) = receipt.branch_sha.as_deref() {
        if branch_exists(root, &receipt.branch) {
            // 目录还在、且 git 仍把它登记为工作树时,这条分支正被它 checkout:
            // 此时删 ref 只会把残留工作树的 HEAD 打断(实测会变成全零),留着更安全。
            let still_checked_out =
                receipt.worktree.exists() && worktree_is_registered(root, &receipt.worktree);
            if !still_checked_out {
                let refname = format!("refs/heads/{}", receipt.branch);
                let _ = worktree_command(root, &["update-ref", "-d", &refname, sha]);
            }
            branch_residue = branch_exists(root, &receipt.branch);
        }
    }

    if dir_error.is_none() && !branch_residue {
        return Ok(());
    }
    let mut lines =
        vec!["回滚未收干净,下面的残留要手动清理,否则同名建线会一直报「工作树已存在」:".to_string()];
    if let Some(error) = dir_error {
        lines.push(format!(
            "  · 工作树目录仍在: {}(删除失败: {error})",
            receipt.worktree.display()
        ));
        lines.push(format!(
            "    等占用它的程序退出后执行 `git -C \"{}\" worktree remove --force \"{}\"`;\
             该命令报「不是工作树」就直接删掉这个目录",
            git_arg_path(root),
            target
        ));
    }
    if branch_residue {
        lines.push(format!("  · 分支仍在: {}", receipt.branch));
        lines.push(format!(
            "    确认上面没有要保留的提交后执行 `git -C \"{}\" branch -D {}`",
            git_arg_path(root),
            receipt.branch
        ));
    }
    Err(lines.join("\n"))
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

/// 校验一条 worktree 路径:**必须是 git 自己认得的工作树**,且不是主根。
///
/// R-177 内容③/验收④:判据从「位于项目同级目录之下」改成「出现在
/// `git worktree list --porcelain` 里」。两个方向都更对:
/// - **更严**——兄弟目录里的另一个 git 仓、或者随便一个同级目录,以前都能通过
///   路径前缀检查混进来,现在过不了;
/// - **更全**——手工 `git worktree add` 到别处的树以前一律被拒,现在只要 git
///   认得就能合并/放弃(验收④要求这类树也能被发现)。
fn validate_worktree_path(root: &Path, worktree_path: &str) -> Result<PathBuf, String> {
    let worktree =
        std::fs::canonicalize(worktree_path).map_err(|e| format!("工作树不存在或无法解析: {e}"))?;
    if worktree_key(&worktree) == worktree_key(root) {
        return Err("不能对项目主树本身执行工作树操作".into());
    }
    let known = git_worktrees(root)?;
    if !known
        .iter()
        .any(|entry| worktree_key(&entry.path) == worktree_key(&worktree))
    {
        return Err(format!(
            "git 不认得这棵工作树: {}。只有 `git worktree list` 列出的树才能操作。",
            worktree.display()
        ));
    }
    Ok(worktree)
}

/// 本项目在 git 眼里的全部工作树(**含主树自己**)。
fn git_worktrees(root: &Path) -> Result<Vec<kanzei_tools::WorktreeEntry>, String> {
    let output = worktree_command(root, &["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(kanzei_tools::parse_worktree_list(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// 项目写租约的获取上界。
///
/// 协调器的排队是**无限期**的:没有上界时,一条卡死/挂起的 writer 会把建线、建树、
/// 合并、放弃全部变成永久 pending,而取消入口(`cancel_waiter`)在 app 界面上够不到
/// —— 用户能做的只有杀进程。所以宁可超时报错让人重试,也不留永久挂起。
/// 120s 是「大仓上一次正常写操作的量级」与「人还愿意等」之间的取值。
pub(crate) const WRITE_LEASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// 项目级写操作的**唯一**仲裁入口。
///
/// R-171 批4 的口径:worktree 的创建/合并/放弃都是项目级写操作,得进写仲裁。
/// K2 补上第四个调用方 `create_process`(建线),让四条路在同一进程内保持有序。
///
/// **它不是并发正确性的依靠**(K2' 更正):协调器是进程内内存对象,`kz` CLI /
/// 自举循环 / 第二个 kzapp 实例都看不见它。同名建树的正确性由 git 的 ref CAS 保证,
/// 见 [`create_worktree_with_receipt`]。这里的租约只管同进程内的顺序与审计。
///
/// `run_id` 必须**全局唯一**:协调器按 run_id 认领与释放租约,重号会让一个持有者的
/// 释放动作落到另一个持有者头上。毫秒时间戳在并发下会撞,所以再缀一个进程内自增号。
async fn acquire_project_write_lease(
    state: &AppState,
    root: &Path,
    reason: &str,
) -> Result<WriterLease, String> {
    acquire_project_write_lease_within(state, root, reason, WRITE_LEASE_TIMEOUT).await
}

/// [`acquire_project_write_lease`] 的带上界形态(超时可注入,测试用)。
///
/// # 超时为什么不能直接 `tokio::time::timeout` 包一层
///
/// `timeout` 到点会**丢掉里面的 future**,连带丢掉排队用的 oneshot 接收端。而协调器
/// 交接租约时走的是 `let _ = tx.send(Ok(lease))`:接收端已经没了 ⇒ send 把租约原样退回
/// ⇒ 租约在**协调器仍持有 projects 锁**的那一行被 drop ⇒ drop 回调又去锁同一把
/// `std::sync::Mutex` ⇒ 自锁死。所以这里用 `select!` + `pin!`:超时分支里 future 仍然
/// 活着(接收端没被丢),先 `cancel_waiter` 把自己从队列里摘掉,再 await 同一个 future
/// 收敛 —— 拿到的要么是取消错误(报超时),要么是刚好抢到的租约(那就照常返回,
/// 没必要为了报错而把到手的租约扔掉)。
pub(crate) async fn acquire_project_write_lease_within(
    state: &AppState,
    root: &Path,
    reason: &str,
    limit: std::time::Duration,
) -> Result<WriterLease, String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let run_id = format!(
        "worktree_{}_{}",
        crate::run::now_ms(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    );
    let acquire = state.coordinator.acquire_writer_lease(WriterLeaseRequest {
        write_scope: root.to_path_buf(),
        run_id: run_id.clone(),
        process_id: "worktree".into(),
        reason: reason.into(),
    });
    tokio::pin!(acquire);
    tokio::select! {
        result = &mut acquire => result.map_err(|e| format!("无法获取项目写租约: {e}")),
        _ = tokio::time::sleep(limit) => {
            state.coordinator.cancel_waiter(&run_id);
            match acquire.await {
                Ok(lease) => Ok(lease),
                Err(_) => Err(write_lease_timeout_error(state, root, reason, limit)),
            }
        }
    }
}

/// 超时文案:必须说清等的是谁、等了多久、下一步能做什么。
fn write_lease_timeout_error(
    state: &AppState,
    root: &Path,
    reason: &str,
    limit: std::time::Duration,
) -> String {
    let snapshot = state.coordinator.snapshot(root);
    let holder = snapshot
        .writer_run_id
        .as_deref()
        .map(|run| format!("当前写者 run_id={run}"))
        .unwrap_or_else(|| "此刻查不到写者(它可能刚刚释放)".to_string());
    format!(
        "等待项目写租约超时({}s):{}({holder})。\
         项目 {} 上有别的写操作长时间没结束——先让那条线跑完或停掉它,再重试本次操作。\
         本次操作没有创建任何东西",
        limit.as_secs(),
        reason,
        root.display(),
    )
}

/// `worktree_create` 的非 Tauri 内核:取写租约 + 建树。
///
/// 拆出来是为了能测——并发测试要让「建线」和「建工作树」真的同时打进同一个仲裁。
pub(crate) async fn create_worktree_arbitrated(
    state: &AppState,
    root: &Path,
    name: &str,
) -> Result<WorktreeInfo, String> {
    let _lease = acquire_project_write_lease(state, root, "worktree create").await?;
    create_worktree(root, name)
}

#[tauri::command]
pub async fn worktree_create(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    name: String,
) -> Result<WorktreeInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    create_worktree_arbitrated(&state, &root, &name).await
}

/// 收活五格之③(门禁)的返回:每个门禁步骤的名称与成败摘要。
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct GateStep {
    pub(crate) name: String,
    pub(crate) ok: bool,
    pub(crate) summary: String,
}

/// 一个门禁步骤的规格:程序 + 参数(在线的树里执行)。
struct GateStepSpec {
    name: &'static str,
    program: &'static str,
    args: Vec<String>,
}

/// 收活门禁的步骤表(设计文档 §5:fmt / clippy / test / 前端冒烟)。
///
/// 只在对应文件存在时才纳入——非 Rust 仓库不装样子跑 cargo,没有前端冒烟脚本的
/// 线不装样子跑 node;「门禁要么真的能验,要么不列」。
fn gate_steps(worktree: &Path) -> Vec<GateStepSpec> {
    let mut steps = Vec::new();
    if worktree.join("Cargo.toml").is_file() {
        steps.push(GateStepSpec {
            name: "fmt",
            program: "cargo",
            args: vec!["fmt".into(), "--all".into(), "--".into(), "--check".into()],
        });
        steps.push(GateStepSpec {
            name: "clippy",
            program: "cargo",
            args: vec![
                "clippy".into(),
                "--workspace".into(),
                "--all-targets".into(),
                "--quiet".into(),
            ],
        });
        steps.push(GateStepSpec {
            name: "test",
            program: "cargo",
            args: vec!["test".into(), "--workspace".into()],
        });
    }
    if worktree.join("scripts/ui-runtime-smoke.mjs").is_file() {
        steps.push(GateStepSpec {
            name: "ui-smoke",
            program: "node",
            args: vec!["scripts/ui-runtime-smoke.mjs".into()],
        });
    }
    steps
}

/// 执行一个门禁步骤:隐藏控制台窗口异步跑,收集成败与输出摘要。
async fn run_gate_step(cwd: &Path, spec: &GateStepSpec) -> GateStep {
    let mut command = tokio::process::Command::new(spec.program);
    command.args(&spec.args).current_dir(cwd);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().await;
    let (ok, body) = match output {
        Ok(out) => {
            let ok = out.status.success();
            let text = if ok {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stderr.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    stderr.trim().to_string()
                }
            };
            (ok, text)
        }
        Err(error) => (false, format!("无法执行 {}: {error}", spec.program)),
    };
    GateStep {
        name: spec.name.into(),
        ok,
        // 摘要截断,避免一次全量 test 的输出把前端面板撑爆。
        summary: {
            let head: String = body.chars().take(1200).collect();
            if body.chars().count() > 1200 {
                format!("{head}\n…(输出过长已截断)")
            } else {
                head
            }
        },
    }
}

/// 收活门禁(设计文档 §5 步骤③):在线的树里依次跑 fmt/clippy/test/前端冒烟,
/// 任何一步失败都不阻断后续(收活要求看到全貌),整体成败由调用方按步骤聚合。
pub(crate) async fn run_worktree_gate(worktree: &Path) -> Vec<GateStep> {
    let mut results = Vec::new();
    for spec in gate_steps(worktree) {
        results.push(run_gate_step(worktree, &spec).await);
    }
    results
}

#[tauri::command]
pub async fn worktree_gate(
    project_dir: String,
    worktree_path: String,
) -> Result<Vec<GateStep>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    Ok(run_worktree_gate(&worktree).await)
}

/// R-184 批5(收活格5):合并成功后,把线的交付回写**主根** tracker。
///
/// 设计文档 §5 ⑤:回写必须走 tracker 工具落主根一份,标记带取得者代号;线全程
/// 不碰 `.kanzei/**`(两个 worktree 相隔 10 秒各登记一条缺陷都拿到 D-267 的教训)。
/// 这里就是桌面端那个落点:claims 里的条目 ID 决定写哪个 docstore,`append_progress`
/// 走与 `TrackerTool::execute` 相同的跨进程锁与完整性门禁,只追加「进展」不改状态
/// (该不该 done/open 仍由取活判定负责)。
///
/// claim 不是条目 ID(自由文本)时拒绝回写——宁可让用户看到"无法自动回写",
/// 也不能猜一个 ID 写错条目。acceptance 检查在关闭时由主代理用自己的 tracker
/// 工具做,不在这里越权。
fn parse_harvest_claim(claim: &str) -> Result<(&str, &str), String> {
    let Some((prefix, id)) = claim.split_once('-') else {
        return Err(format!(
            "认领 `{claim}` 不是条目 ID(应为 R-xxx / D-xxx),无法自动回写;请用主代理的 tracker 工具手动登记收活。"
        ));
    };
    if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!(
            "认领 `{claim}` 不是严格的 R-xxx / D-xxx 条目 ID,无法自动回写;请用主代理的 tracker 工具手动登记收活。"
        ));
    }
    match prefix {
        "R" | "D" => Ok((prefix, id)),
        _ => Err(format!(
            "认领 `{claim}` 的条目类型不受收活回写支持(R/D 之外);请用主代理的 tracker 工具手动登记。"
        )),
    }
}

#[tauri::command]
pub async fn worktree_harvest_writeback(
    project_dir: String,
    worktree_path: String,
    claim: String,
    agent_code: String,
    branch: String,
) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    // 收活对象必须是 git 认得的真实工作树(与 merge/gate 同一条路径校验),防越界。
    let _worktree = validate_worktree_path(&root, &worktree_path)?;
    let (prefix, id) = parse_harvest_claim(&claim)?;
    let kind = match prefix {
        "R" => &REQUIREMENTS,
        "D" => &DEFECTS,
        _ => unreachable!("parse_harvest_claim only returns R/D"),
    };
    let note =
        format!("由 {agent_code} 线交付并合并(branch {branch})。收活回写来自 {worktree_path}。");
    let updated = kanzei_tools::tracker::append_progress(&root, kind, id, &note)?;
    let progress = updated
        .fields
        .iter()
        .find(|(k, _)| k == "进展")
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    Ok(format!("已回写 {id} 收活记录。当前进展:\n{progress}"))
}

/// 线清单:真源是 `git worktree list --porcelain`(R-177 内容③ / 验收④)。
///
/// 改前清单存在前端 `localStorage["kz-worktrees:*"]` 里,于是三件事都做不到:
/// 手工 `git worktree add` 出来的树看不见、换机器/清缓存后清单归零、而磁盘上的
/// 树还在。现在每次都问 git,前端不再持有任何清单状态。
///
/// 主树自己那条剔除;bare 与 prunable 的也剔除(前者不是工作树,后者的目录已经
/// 不在了,列出来只会让「合并/放弃」按钮点了就报错)。绑定关系从进程表合并进来,
/// 让界面能显示这棵树被哪条线占着。
#[tauri::command]
pub fn worktree_list(
    state: tauri::State<'_, AppState>,
    project_dir: String,
) -> Result<Vec<WorktreeInfo>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let root_key = worktree_key(&root);
    let bound: std::collections::BTreeMap<String, String> = state
        .processes
        .lock()
        .unwrap()
        .values()
        .filter_map(|process| {
            let path = process.worktree_path.as_ref()?;
            Some((worktree_key(Path::new(path)), process.id.clone()))
        })
        .collect();
    let mut out = Vec::new();
    for entry in git_worktrees(&root)? {
        let key = worktree_key(&entry.path);
        if key == root_key || entry.bare || entry.prunable {
            continue;
        }
        // 探测失败不整条丢:树在清单里但状态取不到,用户更需要看见它并知道为什么。
        let (files, diff) = worktree_status(&root, &entry.path)
            .unwrap_or_else(|error| (vec![format!("(状态不可读: {error})")], String::new()));
        out.push(WorktreeInfo {
            path: git_arg_path(&entry.path),
            branch: entry
                .branch
                .clone()
                .unwrap_or_else(|| "(游离 HEAD)".to_string()),
            clean: files.is_empty(),
            files,
            diff,
            bound_process: bound.get(&key).cloned(),
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn worktree_diff(project_dir: String, worktree_path: String) -> Result<WorktreeInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let branch = worktree_field(&root, &worktree, "branch")?;
    let (files, diff) = worktree_status(&root, &worktree)?;
    Ok(WorktreeInfo {
        path: git_arg_path(&worktree),
        branch,
        clean: files.is_empty(),
        files,
        diff,
        bound_process: None,
    })
}

/// 合并命令的可测试内核。写租约由 Tauri 命令在调用前获取；这里保留从路径校验、
/// merge-tree 预检到 `--no-ff` 合并的完整可观察语义。
fn merge_worktree(root: &Path, worktree_path: &str) -> Result<String, String> {
    let worktree = validate_worktree_path(root, worktree_path)?;
    let branch = worktree_field(root, &worktree, "branch")?;
    let check = worktree_command(root, &["merge-tree", "--write-tree", "HEAD", &branch])?;
    if !check.status.success() {
        return Err(format!(
            "合并前冲突检测失败,双方改动已保留:\n{}",
            String::from_utf8_lossy(&check.stdout)
        ));
    }
    let output = worktree_command(root, &["merge", "--no-ff", &branch])?;
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
pub async fn worktree_merge(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    worktree_path: String,
) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    // R-171 批4:worktree 合并是项目级写操作,接入写仲裁。
    let _lease = acquire_project_write_lease(&state, &root, "worktree merge").await?;
    merge_worktree(&root, &worktree_path)
}

/// 放弃命令的可测试内核。未提交改动时 git 必须拒绝并保留现场；写租约仍由
/// Tauri 命令承担，不把协调器行为混进结果测试。
fn discard_worktree_checked(root: &Path, worktree_path: &str) -> Result<String, String> {
    let worktree = validate_worktree_path(root, worktree_path)?;
    // git 收不下 `\\?\` 前缀的参数(见 `git_arg_path`),而 validate 出来的正是
    // canonicalize 的产物——不剥这一层,放弃工作树在 Windows 上永远失败。
    let output = worktree_command(root, &["worktree", "remove", &git_arg_path(&worktree)])?;
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

/// 放弃一棵工作树必须和它绑定的进程一起收口。先确定绑定身份，再让 git 删除目录；
/// 删除成功后注销进程与 state.db 记录。顺序倒过来会让失败的 git remove 直接丢失
/// 用户仍可恢复的线，遗漏注销则会复现「目录不存在但对话仍向它发送」的问题。
fn discard_worktree_and_unregister(
    state: &AppState,
    root: &Path,
    worktree_path: &str,
) -> Result<String, String> {
    let worktree = validate_worktree_path(root, worktree_path)?;
    let project = root.display().to_string();
    let bound_process_id =
        bound_thread_for_worktree(state, root, &project, &worktree_key(&worktree))?;
    let result = discard_worktree_checked(root, &git_arg_path(&worktree))?;
    if let Some(process_id) = bound_process_id {
        unregister_parallel_process(state, root, &process_id)?;
        Ok(format!("{result};已关闭绑定线路 {process_id}"))
    } else {
        Ok(result)
    }
}

#[tauri::command]
pub async fn worktree_discard(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    worktree_path: String,
) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    // R-171 批4:worktree 放弃(remove 工作树)是项目级写操作,接入写仲裁。
    let _lease = acquire_project_write_lease(&state, &root, "worktree discard").await?;
    discard_worktree_and_unregister(&state, &root, &worktree_path)
}

// R-177 验收⑦:processes.rs 在 F4 之前零测试(既无 mod tests 也无 #[test])。
#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
