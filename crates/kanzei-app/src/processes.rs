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
}

/// `process_create` 的非 Tauri 内核。
///
/// 拆出来是为了能测:`State<'_, AppState>` 在单元测试里构造不出来,而本批要验的
/// 三件事(真实绑定 / 一树一线 / 失败整体回滚)全在这段逻辑里。
///
/// 建线的三步顺序不可调换:
/// 1. **查重先于建树**(D4 定案):目标树已被别的线绑走就直接拒并点名那条线的 id,
///    此时一棵树都不许多出来——否则「拒绝」还是会在磁盘上留下垃圾工作树。
/// 2. 建树。
/// 3. 落库;**落库失败就整体回滚**(摘工作树 + 删分支 + 从内存进程表移除),绝不
///    留半绑定态。半绑定态是最坏的结局:磁盘上有树、库里没线,用户在界面上看不见
///    它,也就没有任何入口能把它收掉。
///
/// # 查重不是 check-then-act
///
/// 第 1 步(查重)、第 2 步(建树)、以及把新 handle 插进内存表这三件事**同处
/// `state.processes` 的同一个 `MutexGuard` 之内** —— guard 在查重之前取得,直到插表
/// 完成才 `drop`。所以「查到没人绑 ⇒ 建树 ⇒ 绑上」是原子的,两个并发调用者不可能
/// 都通过查重。这也是为什么 `create_worktree`(一次阻塞 git 调用)被刻意留在锁内:
/// 把它挪到锁外会**立刻**打开竞态窗口,建出两棵同路径树或两条绑同一棵树的线。
/// 唯一另一个会写 `worktree_path` 的地方是 `restore_processes_from_store`,它取的
/// 是同一把锁,因此也不会与本函数交错。
///
/// 跨**进程**(两个 app 实例同时开着同一个项目)不在这把锁的覆盖范围内;那一层由
/// git 自己兜底(`worktree add` 撞已存在的目标会失败),本轮不做进程间互斥。
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_process(
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
    let mut processes = state.processes.lock().unwrap();

    // ① 一树一线查重(建树之前)。
    let planned = match worktree_name.as_deref() {
        Some(name) => {
            let (target, branch) = worktree_target(&root, name)?;
            let key = worktree_key(&target);
            if let Some(bound) = processes.values().find(|process| {
                process
                    .worktree_path
                    .as_deref()
                    .is_some_and(|path| worktree_key(Path::new(path)) == key)
            }) {
                return Err(format!(
                    "工作树 {} 已绑定到线 {};一棵工作树同时只能有一条线",
                    target.display(),
                    bound.id
                ));
            }
            Some((target, branch))
        }
        None => None,
    };

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

    // ② 建树。失败直接返回:此刻还没有任何进程进内存表,不需要回滚。
    let worktree_path = match worktree_name.as_deref() {
        Some(name) => Some(create_worktree(&root, name)?.path),
        None => None,
    };

    let process = ProcessHandle {
        id: format!("p{next}|{project}"),
        origin_project: project.clone(),
        project_dir: project,
        worktree_path,
        model: Arc::new(Mutex::new(model.filter(|value| !value.trim().is_empty()))),
        profile: Arc::new(Mutex::new(profile.filter(|value| !value.trim().is_empty()))),
        reasoning: Arc::new(Mutex::new(
            reasoning.filter(|value| !value.trim().is_empty()),
        )),
        phase_pipeline_enabled: Arc::new(AtomicBool::new(phase_pipeline.unwrap_or(false))),
    };
    let info = process_info(state, &process);
    processes.insert(process.id.clone(), process.clone());
    drop(processes);
    // ③ R-178 D3:非默认线创建即落库,重启后页签与线级状态可恢复。
    if let Err(error) = persist_process(&root, &process) {
        state.processes.lock().unwrap().remove(&process.id);
        if let Some((target, branch)) = planned {
            rollback_worktree(&root, &target, &branch);
        }
        return Err(error);
    }
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

/// 工作树的目标路径与分支名——只算不落盘。
///
/// 与 `create_worktree` 分开,是因为一树一线查重要在建树**之前**拿到目标路径
/// (D4 定案:目标树已被绑定则拒绝,此时一棵树都不许多出来)。
pub(crate) fn worktree_target(root: &Path, name: &str) -> Result<(PathBuf, String), String> {
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
    let parent = root.parent().unwrap_or(root);
    Ok((
        parent.join(format!(".kanzei-worktree-{safe_name}")),
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

/// 建工作树(非 Tauri 内核):`worktree_create` 命令与 `create_process` 建线共用。
///
/// # `git worktree add` 之后的每一步都必须能回滚
///
/// 这个函数有两条失败路径,两条都会在磁盘与 git 里留下**界面上没有任何入口能收掉**
/// 的残留,所以两条都得自己收干净:
///
/// 1. **`add` 自己失败**:2026-08-11 实测 git 的四种 add 失败模式(目标是非空目录 /
///    目标是文件 / 前置目录建不出 / 路径超长)**统统把新分支留在原地** —— `git
///    worktree add -b` 是先建分支再挂树的。只 `remove_dir_all` + `prune` 不删分支,
///    下一次同名建线就撞 `a branch named '…' already exists`,**第二次起永久失败**。
/// 2. **`add` 成功、后面的工作区探测失败**:此时树已经挂上、分支已经落地,用 `?`
///    直接抛错就留下一棵孤儿树 + 一条孤儿分支。`create_process` 的整体回滚**救不了
///    它** —— 那一层只回滚它自己那一步(落库),`create_worktree` 返回 Err 时它认为
///    这里已经什么都没建出来。
///
/// # 分支只删自己建的那一条
///
/// 回滚要问的是反方向的问题:**不是「我该删哪条分支」,而是「这条分支是不是我建的」**。
/// `add` 若因为同名分支**已经存在**而失败,那条分支是用户的东西,删掉就是丢数据。
/// 所以 add 之前先记下分支存不存在,只有「调用前不存在」才允许删。`add` **成功**的
/// 那条路径不需要这个判断:`-b` 撞名必失败,add 成功 ⇒ 分支就是本次建出来的。
pub(crate) fn create_worktree(root: &Path, name: &str) -> Result<WorktreeInfo, String> {
    let (worktree, branch) = worktree_target(root, name)?;
    if worktree.exists() {
        return Err(format!("工作树已存在: {}", worktree.display()));
    }
    let branch_was_there = branch_exists(root, &branch);
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
        // 分支若是本次 add 建出来的,必须一并收掉(见上文第 1 条)。
        discard_worktree(
            root,
            &worktree,
            (!branch_was_there).then_some(branch.as_str()),
        );
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    // add 已经成功:从这里往下的任何失败都得整体回滚,不能用 `?` 抛出去(见上文第 2 条)。
    let (files, diff) = match worktree_status(root, &worktree) {
        Ok(probed) => probed,
        Err(error) => {
            rollback_worktree(root, &worktree, &branch);
            return Err(error);
        }
    };
    Ok(WorktreeInfo {
        path: git_arg_path(&worktree),
        branch,
        clean: files.is_empty(),
        files,
        diff,
    })
}

/// 回收一条建到一半的线:摘工作树 + 删分支。
///
/// 只在「分支确定是本次建出来的」时候调用(`create_process` 的落库失败回滚、
/// `create_worktree` 里 add 成功之后的失败),所以这里无条件删分支。
pub(crate) fn rollback_worktree(root: &Path, worktree: &Path, branch: &str) {
    discard_worktree(root, worktree, Some(branch));
}

/// 回滚的实现:先 remove 工作树(分支正被它 checkout,不先摘就删不掉),再按需删分支。
/// 全程 best-effort —— 它跑在失败路径上,自己再报错只会盖掉真正的原因。
fn discard_worktree(root: &Path, worktree: &Path, branch: Option<&str>) {
    let target = git_arg_path(worktree);
    let removed = worktree_command(root, &["worktree", "remove", "--force", &target])
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !removed {
        let _ = std::fs::remove_dir_all(worktree);
        let _ = worktree_command(root, &["worktree", "prune"]);
    }
    if let Some(branch) = branch {
        let _ = worktree_command(root, &["branch", "-D", branch]);
    }
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
    create_worktree(&root, &name)
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
