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
            phase_pipeline: process.phase_pipeline_enabled.load(Ordering::SeqCst),
            updated_at: crate::run::now_ms(),
        })
        .map_err(|e| format!("进程状态落库失败: {e}"))
}

#[tauri::command]
pub async fn process_create(
    state: State<'_, AppState>,
    project_dir: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    // 「勘察复核」开关(阶段流水线总闸)。缺省 = 关,见 `ProcessHandle` 的字段注释。
    phase_pipeline: Option<bool>,
    // 给定则同时建一棵工作树并绑到这条线上;缺省(Tauri 对未传的 Option 参数解析为
    // None)保持今天的行为,worktree_path 恒 None。
    worktree_name: Option<String>,
) -> Result<ProcessInfo, String> {
    create_process(
        &state,
        &project_dir,
        model,
        profile,
        reasoning,
        phase_pipeline,
        worktree_name,
    )
    .await
}

/// `process_create` 的非 Tauri 内核。
///
/// 拆出来是为了能测:`State<'_, AppState>` 在单元测试里构造不出来,而本批要验的
/// 事(真实绑定 / 一树一线 / 失败整体回滚 / 走写仲裁)全在这段逻辑里。
///
/// # 为什么这里必须取项目写租约(K2 返工的根因)
///
/// 建工作树 = 建目录 + 建分支 + 改仓库的 worktree 清单,**是项目级写操作**。同文件的
/// `worktree_create` / `worktree_merge` / `worktree_discard` 三个入口早就接进了写仲裁
/// (R-171 批4 的口径),唯独建线这条新入口绕过去了,于是它与 `worktree_create` 之间
/// 只剩下**两把互不相干的锁**:一边是协调器的写租约,一边是 `state.processes` 的
/// Mutex。两条路可以真并发,而 `create_worktree` 里「这条分支是不是我建的」是**调用前
/// 采样出来的布尔量**——采样与 `add` 之间别人把树和分支建好了,自己的 `add` 因此失败,
/// 清理块就会拿着一个过期的判断去 `worktree remove --force` + 删分支,**删掉的是赢家
/// 刚建好的树和分支**。并发正是本项目的用例(整条第一梯队就是为任务级并行做的),
/// 所以这不是理论风险。
///
/// 修法是让「预检 → 建树 → 建分支 → 绑定落库」整段罩在**同一个项目写租约**下:
/// 与 `worktree_create` 同一个仲裁入口([`acquire_project_write_lease`]),同一项目内
/// 天然串行。竞态窗口与仲裁旁路同时消失。回滚侧另有一道 SHA 双保险,见
/// [`WorktreeReceipt`]。
///
/// # 锁边界的代价(为什么建树挪到了锁外)
///
/// 老版把 `git worktree add`(一次全量检出)+ `git status` + `git diff` 全压在
/// `state.processes` 的 guard 里,理由是「查重必须原子」。代价是这段几百毫秒到数秒的
/// 阻塞期间,`process_list` / `process_update` / `process_close` / `run_prompt`
/// 全部卡在同一把锁上——整个界面对所有线冻结。现在原子性由写租约保证,内存锁只在
/// 两个**极短**的临界区里取:查重一次、编号+落库+插表一次。git 的耗时调用全在锁外。
/// (落库那次临界区里有一次 SQLite 打开+写入,毫秒级;放在锁内是为了让「编号 ⇒ 落库
/// ⇒ 插表」保持原子,否则两条不带 worktree 的建线——它们不取写租约——会算出同一个
/// `p{n}`。)
///
/// # 不带 worktree 的建线为什么不取租约
///
/// 它一个字节都不写项目工作区,只往 `state.db` 加一行。让它去排项目写租约,等于把
/// 「新开一条线」这个按钮挂在正在跑的 writer 后面等,UX 上不可接受,收益是零。
///
/// 跨**进程**(两个 app 实例同时开着同一个项目)不在协调器覆盖范围内;那一层由
/// git 自己兜底(`worktree add` 撞已存在的目标会失败),本轮不做进程间互斥。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn create_process(
    state: &AppState,
    project_dir: &str,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    phase_pipeline: Option<bool>,
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
    let planned = match worktree_name.as_deref() {
        Some(name) => {
            let (target, _) = worktree_target(&root, name)?;
            let key = worktree_key(&target);
            if let Some(bound) = bound_process_id(state, &key) {
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
    let (worktree_path, receipt) = match created {
        Some((info, receipt)) => (Some(info.path), Some(receipt)),
        None => (None, None),
    };

    // ④ 编号 + 落库 + 插内存表(一个临界区内完成)。任一步失败就整体回滚,
    //    绝不留半绑定态——磁盘上有树、库里没线是最坏结局:界面上看不见它,
    //    也就没有任何入口能把它收掉。
    match register_process(
        state,
        &root,
        &project,
        worktree_path,
        planned
            .as_ref()
            .map(|(target, key)| (target.as_path(), key.as_str())),
        ThreadSettings {
            model,
            profile,
            reasoning,
            phase_pipeline,
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
        "工作树 {} 已绑定到线 {bound_id};一棵工作树同时只能有一条线",
        target.display()
    )
}

/// 内存进程表里是否已经有线绑着这棵树;有就给出那条线的 id。
fn bound_process_id(state: &AppState, key: &str) -> Option<String> {
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
    planned: Option<(&Path, &str)>,
    settings: ThreadSettings,
) -> Result<ProcessInfo, String> {
    let ThreadSettings {
        model,
        profile,
        reasoning,
        phase_pipeline,
    } = settings;
    let mut processes = state.processes.lock().unwrap();

    // 二次查重:建树期间内存锁是放开的,`restore_processes_from_store` 可能把库里的
    // 绑定合了进来。同项目的并发建线被写租约挡在外面,这一次查的是那条路。
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
    let next = next_process_index(&processes, &stored, project);

    let process = ProcessHandle {
        id: format!("p{next}|{project}"),
        origin_project: project.to_string(),
        project_dir: project.to_string(),
        worktree_path,
        model: Arc::new(Mutex::new(model.filter(|value| !value.trim().is_empty()))),
        profile: Arc::new(Mutex::new(profile.filter(|value| !value.trim().is_empty()))),
        reasoning: Arc::new(Mutex::new(
            reasoning.filter(|value| !value.trim().is_empty()),
        )),
        phase_pipeline_enabled: Arc::new(AtomicBool::new(phase_pipeline.unwrap_or(false))),
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
            phase_pipeline: process.phase_pipeline_enabled.load(Ordering::SeqCst),
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
        parent.join(format!(".kanzei-worktree-{project_tag}-{safe_name}")),
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
/// 老版把「删哪棵树、删哪条分支」现算:目标路径由名字推、分支由名字推、
/// 「这条分支是不是我建的」是一个调用前采样的布尔量。三样都不是**本次建出来的那个
/// 东西的身份**,而只是「同名的那个东西」——并发下同名的东西可能是别人刚建好的。
/// 凭据把身份钉死:分支只有停在**建出来时那个 sha** 上才允许删。
pub(crate) struct WorktreeReceipt {
    pub(crate) worktree: PathBuf,
    pub(crate) branch: String,
    /// `Some(sha)` = 这条分支是本次调用建出来的,且当时停在 `sha`;回滚用
    /// `git update-ref -d <ref> <sha>` 做**原子比较后删除**——ref 已经不在这个 sha 上
    /// (别人重建了它 / 线上已经有提交)就删不动,这正是要的。
    ///
    /// `None` = 调用前就存在(用户的东西)或无法确认;回滚一律不碰,也不当残留报。
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
/// 一次就是自己等自己。
///
/// # `git worktree add` 之后的每一步都必须能回滚
///
/// 这个函数有两条失败路径,两条都会在磁盘与 git 里留下**界面上没有任何入口能收掉**
/// 的残留,所以两条都得自己收干净:
///
/// 1. **`add` 自己失败**:2026-08-11 实测 git 的四种 add 失败模式(目标是非空目录 /
///    目标是文件 / 前置目录建不出 / 路径超长)**统统把新分支留在原地** —— `git
///    worktree add -b` 是先建分支再挂树的。不删分支,下一次同名建线就撞
///    `a branch named '…' already exists`,**第二次起永久失败**。
/// 2. **`add` 成功、后面的工作区探测失败**:此时树已经挂上、分支已经落地,用 `?`
///    直接抛错就留下一棵孤儿树 + 一条孤儿分支。`create_process` 的整体回滚**救不了
///    它** —— 那一层只回滚它自己那一步(落库),`create_worktree` 返回 Err 时它认为
///    这里已经什么都没建出来。
///
/// # 分支只删自己建的那一条,而且只在它没动过的时候删
///
/// 回滚要问的是反方向的问题:**不是「我该删哪条分支」,而是「这条分支是不是我建的」**。
/// 两道判据缺一不可:
/// - `add` 若因为同名分支**已经存在**而失败,那条分支是用户的东西,删掉就是丢数据 ——
///   所以 add 之前先采样它存不存在,只有「调用前不存在」才进入下一道;
/// - 采样是**过去时**,并发下它可能已经过期(别人在这中间把同名分支建了出来)。
///   所以再记下**本次建出来时那条分支的 sha**,删的时候交给
///   `git update-ref -d <ref> <sha>` 原子比较:ref 不在这个 sha 上就删不动。
pub(crate) fn create_worktree(root: &Path, name: &str) -> Result<WorktreeInfo, String> {
    create_worktree_with_receipt(root, name).map(|(info, _)| info)
}

/// [`create_worktree`] 加一张回滚凭据:建线要拿它在落库失败时回滚。
pub(crate) fn create_worktree_with_receipt(
    root: &Path,
    name: &str,
) -> Result<(WorktreeInfo, WorktreeReceipt), String> {
    let (worktree, branch) = worktree_target(root, name)?;
    if worktree.exists() {
        // D-004 口径:这条预检是「名字被占死」唯一的出口,必须自带可执行的解法,
        // 否则一次失败的清理就能让这个名字永远建不出来而用户无从下手。
        return Err(format!(
            "工作树已存在: {};若它是上次回滚失败留下的残留,先执行 \
             `git -C \"{}\" worktree remove --force \"{}\"`,该命令报「不是工作树」\
             就直接删掉这个目录,然后重试",
            worktree.display(),
            git_arg_path(root),
            git_arg_path(&worktree),
        ));
    }
    let branch_was_there = branch_exists(root, &branch);
    // 建树基点:add 失败时新建的分支必然停在这里(命令里显式给的就是 HEAD),
    // 拿它当回滚删分支的 CAS 判据。
    let base_sha = rev_parse(root, "HEAD");
    let output = worktree_command(
        root,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &git_arg_path(&worktree),
            "HEAD",
        ],
    )?;
    if !output.status.success() {
        let receipt = WorktreeReceipt {
            worktree,
            branch,
            branch_sha: if branch_was_there { None } else { base_sha },
        };
        let failure = String::from_utf8_lossy(&output.stderr).trim().to_string();
        // 分支若是本次 add 建出来的,必须一并收掉(见上文第 1 条)。
        return Err(with_residue(failure, discard_worktree(root, &receipt)));
    }
    // add 已经成功:从这里往下的任何失败都得整体回滚,不能用 `?` 抛出去(见上文第 2 条)。
    // `-b` 撞名必失败 ⇒ add 成功就说明这条分支是本次建出来的,记下它此刻的 sha。
    let receipt = WorktreeReceipt {
        branch_sha: rev_parse(root, &format!("refs/heads/{branch}")),
        worktree,
        branch,
    };
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

/// 项目级写操作的**唯一**仲裁入口。
///
/// R-171 批4 的口径:worktree 的创建/合并/放弃都是项目级写操作,得进写仲裁。
/// K2 补上第四个调用方 `create_process`(建线)—— 它以前绕过这里,拿 `state.processes`
/// 的 Mutex 当互斥,与本函数的租约互不可见,于是「建线」与「建工作树」可以真并发,
/// 输的一方会拿过期的判断删掉赢的一方刚建好的树和分支。
///
/// `run_id` 必须**全局唯一**:协调器按 run_id 认领与释放租约,重号会让一个持有者的
/// 释放动作落到另一个持有者头上。毫秒时间戳在并发下会撞,所以再缀一个进程内自增号。
async fn acquire_project_write_lease(
    state: &AppState,
    root: &Path,
    reason: &str,
) -> Result<WriterLease, String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let run_id = format!(
        "worktree_{}_{}",
        crate::run::now_ms(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    );
    state
        .coordinator
        .acquire_writer_lease(WriterLeaseRequest {
            project_root: root.to_path_buf(),
            run_id,
            process_id: "worktree".into(),
            reason: reason.into(),
        })
        .await
        .map_err(|e| format!("无法获取项目写租约: {e}"))
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
    let _lease = acquire_project_write_lease(&state, &root, "worktree merge").await?;
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
    let _lease = acquire_project_write_lease(&state, &root, "worktree discard").await?;
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    // git 收不下 `\\?\` 前缀的参数(见 `git_arg_path`),而 validate 出来的正是
    // canonicalize 的产物——不剥这一层,放弃工作树在 Windows 上永远失败。
    let output = worktree_command(&root, &["worktree", "remove", &git_arg_path(&worktree)])?;
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

// R-177 验收⑦:processes.rs 在 F4 之前零测试(既无 mod tests 也无 #[test])。
#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
