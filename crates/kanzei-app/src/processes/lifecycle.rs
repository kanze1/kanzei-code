//! 进程生命周期与 IPC 命令(R-254 批1,纯搬迁自 processes.rs)。
//!
//! 独立理由:进程生命周期是「线怎么建、怎么改、怎么关」的变更理由——`process_create`
//! 族(建线:预检→建树→注册→回滚)、`process_update`(线级字段变更)、`process_close`
//! 族(关线:停止→注销→处置工作树)、`process_list`(列表恢复)。它与注册编号
//! (registry)、工作树操作(workspace)、门禁(gate)互不相关:改一条关线顺序不必读懂
//! 门禁步骤表(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):关线顺序必须是「停止/注销 → 回收 owner 后台进程 → 处置工作树」,
//! 旧顺序先 git remove 再注销会让运行中的进程在脚下删目录;默认线(`d|`)不销毁只
//! 复位;注销是运行会话终点,统一出口先落飞轨迹再清 ask 再收敛输入(cancelled)。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use kanzei_harness::{Tool, ToolCtx};
use serde_json::json;
use tauri::State;

use crate::{
    ensure_default_process, halt_runtime_immediately, normalized_project_root, process_info,
    process_session_id, AppState, ProcessHandle, ProcessInfo, WorktreeRoot,
};
use kanzei_tools::worktree as wt;

use super::registry::{
    bound_error, bound_thread_for_worktree, mark_project_restored, persist_process,
    register_process, restore_processes_from_store_once, ThreadSettings,
};
use super::workspace::reclaim_worktree_on_close;

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
        .filter(|process| process.origin_project.0 == root)
        .map(|process| process_info(&state, process))
        .collect::<Vec<_>>();
    if !result.iter().any(|item| item.id == default.id) {
        result.push(process_info(&state, &default));
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
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
    // 进程级「子代理」开关。缺省 = 开,保持既有 task 能力。
    subagents_enabled: Option<bool>,
    // 仅分支线有意义:允许该线更新主根中的唯一 tracker 文档。缺省 = 关。
    tracker_writes: Option<bool>,
    // 给定则同时建一棵工作树并绑到这条线上;缺省(Tauri 对未传的 Option 参数解析为
    // None)保持今天的行为,worktree_path 恒 None。
    worktree_name: Option<String>,
    // R-247:并行视图选中的 R/D 条目。由桌面主进程在建树后以新分支身份执行真实
    // work claim；不因此放开该分支线的通用 tracker 写权限。
    work_item_id: Option<String>,
) -> Result<ProcessInfo, String> {
    create_process_with_tracker(
        &state,
        &project_dir,
        model,
        profile,
        reasoning,
        phase_pipeline,
        subagents_enabled,
        tracker_writes,
        worktree_name,
        work_item_id,
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
/// # 为什么建线不再排源码写租约
///
/// 源码写租约覆盖的是某条线对代码的修改周期；创建独立 worktree 只新增 Git ref、
/// worktree 登记与目录，不修改现有线的代码。把二者放进同一租约会导致主线运行时
/// 新建线路最长等待 120 秒，直接失去并行入口。现在建线/建树走
/// `AppState.worktree_ops` 的独立串行闸；合并/放弃仍保留源码写租约。
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
        Some(true),
        None,
        worktree_name,
        None,
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
    subagents_enabled: Option<bool>,
    tracker_writes: Option<bool>,
    worktree_name: Option<String>,
    work_item_id: Option<String>,
) -> Result<ProcessInfo, String> {
    let root = normalized_project_root(Path::new(project_dir));
    ensure_default_process(state, &root);
    // 恒主根:见本文件头的「字段口径」。worktree 路径只进 worktree_path。
    let project = root.display().to_string();
    let worktree_name = worktree_name.filter(|value| !value.trim().is_empty());
    let work_item_id = work_item_id.filter(|value| !value.trim().is_empty());
    if work_item_id.is_some() && worktree_name.is_none() {
        return Err("条目绑定只适用于带独立工作树的并行线".into());
    }

    // ① 建树只排 Git 工作树元数据闸，不排主线源码写租约。guard 持有到绑定落库结束，
    //    让「建 ref/目录 → 注册线路」在同一应用内保持原子顺序。
    let _worktree_guard = match worktree_name.as_deref() {
        Some(_) => Some(state.worktree_ops.lock().await),
        None => None,
    };

    // ② 一树一线查重(建树之前:被拒时磁盘上一棵树都不许多出来)。
    //    查的是**内存表 ∪ state.db**:同一个函数里的编号分配一直是查库的,查重只扫内存
    //    表就自相矛盾——重启后内存表是空的,于是同名建线绕过查重、一路撞到
    //    `create_worktree` 的目录预检,给出的文案会教用户 `worktree remove --force`
    //    一棵**仍被库里某条线绑着、且可能带未提交改动**的活树,还完全不点名那条线。
    let planned = match worktree_name.as_deref() {
        Some(name) => {
            let (target, _) = wt::worktree_target(&root, name)?;
            let key = wt::worktree_key(&target);
            if let Some(bound) = bound_thread_for_worktree(state, &root, &project, &key)? {
                return Err(bound_error(&target, &bound));
            }
            Some((target, key))
        }
        None => None,
    };

    // ③ 建树。耗时的 git 调用全在内存锁之外,同进程顺序由 ① 的元数据闸兜着；
    //    跨进程正确性仍由 create_worktree_with_receipt 的 git ref CAS 兜着。
    //    失败直接返回:create_worktree 自己已经把残留收干净(收不掉的会在错误里点名)。
    let created = match worktree_name.as_deref() {
        Some(name) => Some(wt::create_worktree_with_receipt(&root, name)?),
        None => None,
    };
    let (worktree_path, branch, receipt) = match created {
        Some((info, receipt)) => (
            Some(WorktreeRoot(PathBuf::from(info.path))),
            Some(info.branch),
            Some(receipt),
        ),
        None => (None, None, None),
    };

    // ④ 编号 + 落库 + 插内存表(一个临界区内完成)。任一步失败就整体回滚,
    //    绝不留半绑定态——磁盘上有树、库里没线是最坏结局:界面上看不见它,
    //    也就没有任何入口能把它收掉。
    let registered = register_process(
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
            subagents_enabled,
            tracker_writes,
        },
    );
    let info = match registered {
        Ok(info) => info,
        Err(error) => {
            return Err(match receipt.as_ref() {
                Some(receipt) => wt::with_residue(error, wt::rollback_worktree(&root, receipt)),
                None => error,
            });
        }
    };
    if let Some(work_item_id) = work_item_id.as_deref() {
        if let Err(error) = claim_work_item_for_process(&root, &info, work_item_id).await {
            let cleanup = unregister_parallel_process(state, &root, &info.id)
                .map(|_| ())
                .map_err(|cleanup| format!("注销半绑定线路失败: {cleanup}"));
            let error = wt::with_residue(error, cleanup);
            return Err(match receipt.as_ref() {
                Some(receipt) => wt::with_residue(error, wt::rollback_worktree(&root, receipt)),
                None => error,
            });
        }
    }
    Ok(info)
}

/// R-247 的权限边界：建线绑定是主进程编排动作，不要求用户先给新线打开
/// `tracker_writes`。这里仍复用 WorkTool 的 WIP、阻塞、接管与跨进程锁语义，
/// 没有第二套“看起来像 claim”的字段直写。
async fn claim_work_item_for_process(
    root: &Path,
    process: &ProcessInfo,
    work_item_id: &str,
) -> Result<(), String> {
    let cwd = process
        .worktree_path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "条目绑定缺少并行线工作树".to_string())?;
    let output = kanzei_tools::WorkTool
        .execute(
            json!({
                "action": "claim",
                "id": work_item_id,
                "reason": "parallel-line-create:用户从并行视图选择条目开线"
            }),
            &ToolCtx::new(cwd, root.to_path_buf()),
        )
        .await;
    if output.is_error {
        Err(format!("新线路未能绑定 {work_item_id}: {}", output.content))
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) async fn create_process_with_work_item(
    state: &AppState,
    project_dir: &str,
    worktree_name: String,
    work_item_id: String,
) -> Result<ProcessInfo, String> {
    create_process_with_tracker(
        state,
        project_dir,
        None,
        None,
        None,
        Some(false),
        Some(true),
        Some(false),
        Some(worktree_name),
        Some(work_item_id),
    )
    .await
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
    // 进程级「子代理」开关；关闭后 task 不进入工具面。
    subagents_enabled: Option<bool>,
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
    if let Some(subagents_enabled) = subagents_enabled {
        process
            .subagents_enabled
            .store(subagents_enabled, Ordering::SeqCst);
    }
    if let Some(tracker_writes) = tracker_writes {
        process
            .tracker_writes_enabled
            .store(tracker_writes, Ordering::SeqCst);
    }
    // R-178 D3:任何字段变更同步落库(含默认进程——它是「主对话」的模型/开关状态,
    // 重启后要用库值回填)。D-367:project_dir 恒主根,直接取类型化路径。
    let root = &process.project_dir.0;
    persist_process(root, &process)?;
    mark_project_restored(&state, root);
    Ok(process_info(&state, &process))
}

/// 关线/复位前清空该会话尚未消费的排队输入。admitted 而未 promote 的输入若不
/// 处置,主线复位后会在下一次开跑时被静默续跑,并行线关闭后则无声丢失——两种
/// 结局用户都看不见。逐条落 prompt.cancelled,取消数量计入关闭消息。
fn cancel_pending_inputs_on_close(
    store: &kanzei_core::SessionStore,
    session_id: &str,
) -> Result<usize, String> {
    let pending = store
        .list_pending_inputs(session_id)
        .map_err(|e| e.to_string())?;
    let mut cancelled = 0usize;
    for input in &pending {
        if store
            .cancel_input(session_id, &input.input_id)
            .map_err(|e| e.to_string())?
        {
            store
                .append_event(
                    session_id,
                    "prompt.cancelled",
                    &json!({ "input_id": input.input_id, "reason": "line_closed" }),
                )
                .map_err(|e| e.to_string())?;
            cancelled += 1;
        }
    }
    Ok(cancelled)
}

#[tauri::command]
pub async fn process_close(
    state: State<'_, AppState>,
    process_id: String,
) -> Result<String, String> {
    let process = state
        .processes
        .lock()
        .unwrap()
        .get(&process_id)
        .cloned()
        .ok_or_else(|| format!("进程不存在: {process_id}"))?;
    close_process(&state, &process).await
}

pub(crate) async fn close_process(
    state: &AppState,
    process: &ProcessHandle,
) -> Result<String, String> {
    let process_id = process.id.clone();
    // D-367:project_dir 恒主根(ProjectRoot),直接取路径。
    let root = &process.project_dir.0;
    let session_id = process_session_id(root, Some(&process_id));
    if process_id.starts_with("d|") {
        let state_path = kanzei_core::project_state_path(root);
        let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
        if let Some(runtime) = state.runtimes.lock().unwrap().get(&session_id).cloned() {
            if runtime.running.load(Ordering::SeqCst) {
                halt_runtime_immediately(&runtime, &store, &session_id)
                    .map_err(|e| format!("关闭主线路时收口会话失败: {e}"))?;
            } else {
                runtime.asks.lock().unwrap().clear();
            }
        }
        // 自主推进控制器与进程生命周期同源；关闭后不能继承旧轮数。
        state.auto_runs.lock().unwrap().remove(&session_id);
        *process.model.lock().unwrap() = None;
        *process.profile.lock().unwrap() = None;
        // 默认进程不销毁,只复位;复位值必须与 ensure_default_process 的默认一致(关)。
        process
            .phase_pipeline_enabled
            .store(false, Ordering::SeqCst);
        process.subagents_enabled.store(true, Ordering::SeqCst);
        process
            .tracker_writes_enabled
            .store(false, Ordering::SeqCst);
        // R-178 D3:复位后的空状态也要落库,否则重启后库里的旧值又回填回来。
        persist_process(root, process)?;
        let cancelled = cancel_pending_inputs_on_close(&store, &session_id)?;
        Ok(if cancelled > 0 {
            format!("主线路已停止并复位；已取消 {cancelled} 条排队输入")
        } else {
            "主线路已停止并复位".into()
        })
    } else {
        // 关闭顺序必须是「停止/注销 → 回收 owner 后台进程 → 处置工作树」。旧顺序先跑
        // git worktree remove，再进 unregister 停运行；运行中的进程仍把该树当 cwd 时，
        // 可能在它脚下删目录。process 已在上面克隆，注销后仍保有处置所需路径。
        let released = unregister_parallel_process(state, root, &process_id)?;
        let killed = kanzei_tools::kill_background_processes_for_process(root, &process_id).await;
        // 注销之后运行时已停,不会再有 promote 与取消赛跑;此时清排队输入最稳。
        let state_path = kanzei_core::project_state_path(root);
        let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
        let _ = store.create_session(&session_id, &root.display().to_string(), None);
        let cancelled = cancel_pending_inputs_on_close(&store, &session_id)?;
        let disposal = process
            .worktree_path
            .as_ref()
            .map(|worktree| reclaim_worktree_on_close(root, worktree.as_path()));
        if let Some(Err(kept)) = disposal.as_ref() {
            // 留下来的树此刻已经无主(绑定行删了)。它至少得在**审计流里可发现**,
            // 否则磁盘上有树、库里没线、界面上没入口,三缺一地彻底失联。
            let _ = store.append_event(
                &session_id,
                "worktree.orphaned",
                &json!({ "process_id": process_id, "detail": kept }),
            );
        }
        let background = (killed > 0).then(|| format!("；已回收 {killed} 个后台进程"));
        let dropped = (cancelled > 0).then(|| format!("；已取消 {cancelled} 条排队输入"));
        let release = if released.is_empty() {
            String::new()
        } else {
            format!("；已释放取活绑定 {}", released.join(", "))
        };
        let dropped = dropped.unwrap_or_default();
        match disposal {
            Some(Ok(())) => Ok(format!(
                "已关闭线路 {process_id} 并回收已合并的干净工作树{}{dropped}{release}",
                background.unwrap_or_default(),
            )),
            Some(Err(kept)) => Ok(format!(
                "已关闭线路 {process_id}；{kept}{}{dropped}{release}",
                background.unwrap_or_default(),
            )),
            None => Ok(format!(
                "已关闭线路 {process_id}{}{dropped}{release}",
                background.unwrap_or_default(),
            )),
        }
    }
}

/// 注销一条非默认进程及其持久化登记。工作树已被成功摘除、启动恢复发现目录消失、
/// 或用户显式关线时都复用这一出口，避免只删目录却留下会话继续拿它当 cwd。
pub(crate) fn unregister_parallel_process(
    state: &AppState,
    root: &Path,
    process_id: &str,
) -> Result<Vec<String>, String> {
    if process_id.starts_with("d|") {
        return Err("默认进程不能注销".into());
    }
    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let session_id = process_session_id(root, Some(process_id));
    let branch = state
        .processes
        .lock()
        .unwrap()
        .get(process_id)
        .and_then(|process| process.branch.clone());
    let runtime = state.runtimes.lock().unwrap().get(&session_id).cloned();
    if let Some(runtime) = runtime {
        if runtime.running.load(Ordering::SeqCst) {
            // 注销是运行会话的终点，不能只 abort future。统一出口会先落在飞轨迹、
            // 清 ask，再把 promoted/running/pending 输入收敛为 cancelled。
            halt_runtime_immediately(&runtime, &store, &session_id)
                .map_err(|e| format!("注销线路时收口会话失败: {e}"))?;
        } else {
            // runtime 容器会在首次历史读取/ask 恢复时提前存在；空闲容器没有
            // promoted 输入可 finalize，直接清待答队列即可。
            runtime.asks.lock().unwrap().clear();
        }
    }
    // 运行收口后、身份退役前释放 tracker 持有。释放失败时保留一条已停止的线路供
    // 用户重试，不能制造「线已消失、条目仍被幽灵分支持有」的半截状态。
    let released = match branch.as_deref() {
        Some(branch) => {
            kanzei_tools::release_line_claims(root, branch, "parallel-line-unregister")?
        }
        None => Vec::new(),
    };
    // finalize + release 成功后才退役身份；若持久化失败，保留已停止的内存线路。
    store
        .delete_process(process_id)
        .map_err(|e| format!("删除进程注册失败: {e}"))?;
    state.runtimes.lock().unwrap().remove(&session_id);
    state.auto_runs.lock().unwrap().remove(&session_id);
    state.processes.lock().unwrap().remove(process_id);
    Ok(released)
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
            process.origin_project.0.display().to_string() == project
                && process
                    .worktree_path
                    .as_ref()
                    .is_some_and(|worktree| !worktree.0.is_dir())
        })
        .map(|process| process.id.clone())
        .collect::<Vec<_>>();
    for process_id in stale_ids {
        unregister_parallel_process(state, root, &process_id)?;
    }
    Ok(())
}
