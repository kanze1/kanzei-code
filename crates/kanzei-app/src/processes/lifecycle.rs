//! 进程生命周期与 IPC 命令(R-254 批1,纯搬迁自 processes.rs)。
//!
//! 独立理由:进程生命周期是「线怎么建、怎么改、怎么关」的变更理由——`process_create`
//! 族(建线:预检→建树→注册→回滚)、`process_update`(线级字段变更)、`process_close`
//! 族(关线:停止→注销→处置工作树)、`process_list`(列表恢复)。它与注册编号
//! (registry)、工作树操作(workspace)、门禁(gate)互不相关:改一条关线顺序不必读懂
//! 门禁步骤表(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):关线顺序必须是「停止/注销 → 回收 owner 后台进程 → 处置工作树」,
//! 旧顺序先 git remove 再注销会让运行中的进程在脚下删目录。所有对话统一注销，
//! 注销是运行会话终点,统一出口先落飞轨迹再清 ask 再收敛输入(cancelled)。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use kanzei_harness::{Tool, ToolCtx};
use serde_json::json;
use tauri::State;

#[cfg(test)]
use crate::state::legacy_process_id;
use crate::state::{process_info_with, process_kind_of};
use crate::{
    halt_runtime_immediately, normalized_project_root, process_info, process_session_id, AppState,
    ProcessHandle, ProcessInfo, WorktreeRoot,
};
use kanzei_tools::worktree as wt;

use super::naming::{
    clean_user_title, load_naming, open_naming_store, order_key, process_ordinal, ProcessKind,
};
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
    list_processes(&state, &project_dir)
}

/// `process_list` 的非 Tauri 内核(`State` 在单元测试里构造不出来)。
pub(crate) fn list_processes(
    state: &AppState,
    project_dir: &str,
) -> Result<Vec<ProcessInfo>, String> {
    let root = normalized_project_root(Path::new(project_dir));
    // R-178 D3:启动/切换项目时从 state.db 恢复本项目的线/进程注册
    // (页签不丢 + 线级模型/profile/reasoning/勘察复核开关回填)。
    restore_processes_from_store_once(state, &root)?;
    // 外部 `git worktree remove`、旧版「放弃工作树」都会留下已绑进程但目录消失的
    // 记录。列表刷新是用户可见的恢复点，必须先收掉这些死线，不能让它们继续出现在
    // 页签里，直到发送时才以不存在的 cwd 失败。
    prune_missing_worktree_processes(state, &root)?;
    let handles = state
        .processes
        .lock()
        .unwrap()
        .values()
        .filter(|process| process.origin_project.0 == root)
        .cloned()
        .collect::<Vec<_>>();
    // 命名事实(用户命名/首条消息/最近活动)一次开库给全部线用;库读不到就回落到类型 + 序号。
    let store = open_naming_store(&root);
    let general = crate::general_chat::is_general_root(&root);
    let mut result = handles
        .iter()
        .map(|process| {
            let naming = store
                .as_ref()
                .map(|store| {
                    let session_id = process_session_id(&root, Some(&process.id));
                    // 无项目对话没有项目主控角色，第一段也应按首条消息自动命名。
                    let kind = if general {
                        ProcessKind::Discussion
                    } else {
                        process_kind_of(process)
                    };
                    load_naming(store, &session_id, kind)
                })
                .unwrap_or_default();
            process_info_with(state, process, &naming)
        })
        .collect::<Vec<_>>();
    // 数字序:p2 在 p10 之前(原先按 id 字符串排,p10 会挤到 p2 前面)。
    result.sort_by(|a, b| order_key(&a.id).cmp(&order_key(&b.id)));
    Ok(result)
}

/// 对话的存续不依赖执行进程。已注销的身份仍能管理自己的历史记录，
/// 但必须同时属于本项目的退役账本且存在会话，不能凭客户端传来的 id 造记录。
fn owned_conversation(
    state: &AppState,
    root: &Path,
    process_id: &str,
) -> Result<Option<ProcessHandle>, String> {
    restore_processes_from_store_once(state, root)?;
    if let Some(process) = state.processes.lock().unwrap().get(process_id).cloned() {
        if process.origin_project.0 != root {
            return Err("这个对话不属于当前项目".into());
        }
        return Ok(Some(process));
    }
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
        .map_err(|e| e.to_string())?;
    let retired = store
        .list_retired_process_ids(&root.display().to_string())
        .map_err(|e| e.to_string())?;
    let session_id = process_session_id(root, Some(process_id));
    if retired.iter().any(|id| id == process_id)
        && store
            .get_session(&session_id)
            .map_err(|e| e.to_string())?
            .is_some()
    {
        Ok(None)
    } else {
        Err("对话不存在或已被删除".into())
    }
}

/// 重命名一段对话(UX-009)。写 `sessions.title`(state.db),空白 = 清除命名、回到自动名。
/// 旧格式对话也能改名;运行中也能改(改名不碰运行态,也不刷新「最近活动」)。
#[tauri::command]
pub fn process_rename(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: String,
    title: String,
) -> Result<(), String> {
    rename_process(&state, &project_dir, &process_id, &title)
}

pub(crate) fn rename_process(
    state: &AppState,
    project_dir: &str,
    process_id: &str,
    title: &str,
) -> Result<(), String> {
    let title = clean_user_title(title)?;
    let root = normalized_project_root(Path::new(project_dir));
    owned_conversation(state, &root, process_id)?;
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let session_id = process_session_id(&root, Some(process_id));
    store
        .set_session_title(&session_id, &root.display().to_string(), title.as_deref())
        .map_err(|e| format!("保存对话名称失败: {e}"))
}

/// 删除一段对话及其线路登记(真删,不可恢复)。
///
/// - 所有对话使用相同的关闭与删除规则；运行中先停止再删除。
/// - 先走 [`close_process`]:停止 → 注销 → 回收后台进程 → 按**关闭线路语义**处置工作树——只回收
///   「干净且已合并」的树,否则原样留着,绝不静默丢活。
/// - 工作树被留下时对话记录也保留(树和它的上下文要能对上),只把线路从列表移除并说明;
///   其余情况对话记录(事件/输入/情景/通知)随会话一并删除,同时清掉 app.json 里该 id 的
///   鞭挞设置、置顶与手动排序。
///
/// `forget_prefs` 清理 app.json 里挂在该 id 上的界面状态(生产用 [`forget_process_prefs`];
/// 注入点是为了单测不去碰真实的 `~/.kanzei/app.json`)。
pub(crate) async fn purge_process(
    state: &AppState,
    project_dir: &str,
    process_id: &str,
    forget_prefs: &(dyn Fn(&str) -> Result<(), String> + Sync),
) -> Result<String, String> {
    let root = normalized_project_root(Path::new(project_dir));
    // 删除是幂等操作：旧窗口/菜单持有的退役身份可以再次确认删除，不能因此复活记录。
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let session_id = process_session_id(&root, Some(process_id));
    let already_deleted = store
        .list_retired_process_ids(&root.display().to_string())
        .map_err(|e| e.to_string())?
        .iter()
        .any(|id| id == process_id)
        && store
            .get_session(&session_id)
            .map_err(|e| e.to_string())?
            .is_none();
    drop(store);
    if already_deleted {
        forget_prefs(process_id)?;
        return Ok("对话已删除".into());
    }
    let process = owned_conversation(state, &root, process_id)?;
    let session_id = process_session_id(&root, Some(process_id));
    let running = state
        .runtimes
        .lock()
        .unwrap()
        .get(&session_id)
        .is_some_and(|runtime| runtime.running.load(Ordering::SeqCst));
    if running {
        return Err("对话正在运行,先停止再删除".into());
    }
    // 关闭只结束执行，历史仍是一段可管理的对话。删除历史不再创建/关闭执行进程，
    // 也不触碰已经留下的工作树；保留退役身份，防止旧编号与偏好被复用。
    let Some(process) = process else {
        purge_session_data(&root, &session_id)?;
        forget_prefs(process_id)?;
        return Ok("对话已删除".into());
    };
    let closed = close_process(state, &process).await?;
    forget_prefs(process_id)?;
    if let Some(kept) = process
        .worktree_path
        .as_ref()
        .filter(|worktree| worktree.0.is_dir())
    {
        // `closed` 形如「已关闭；<保留原因…>」,只带「；」之后的处置说明
        // (保留原因与回收命令)。
        let detail = closed.split_once('；').map_or("", |(_, rest)| rest);
        return Ok(format!(
            "对话已从列表移除,但独立任务的工作树仍保留在 {}(有未合并或未提交的内容),\
             对话记录一并保留以便对照。{detail}",
            kept.0.display()
        ));
    }
    let deleted = purge_session_data(&root, &session_id)
        .map_err(|e| format!("对话已从列表移除,但清除它的记录失败: {e}"))?;
    Ok(if deleted {
        "对话已删除".into()
    } else {
        "对话已删除(它还没有任何记录)".into()
    })
}

/// 清掉 app.json 里挂在该对话 id 上的界面状态。没有改动就不写盘。
fn forget_process_prefs(process_id: &str) -> Result<(), String> {
    let _guard = crate::prefs::write_guard()?;
    let mut prefs = crate::prefs::load_prefs_for_write()?;
    if crate::prefs::purge_process_prefs(&mut prefs, process_id) {
        crate::prefs::save_prefs(&prefs)?;
    }
    Ok(())
}

/// 真删库里这段对话的全部记录。返回是否真有记录被删(没发过消息的对话没有会话行)。
///
/// 调用前已确认运行时空闲:库里残留的 `running` 状态与未结束输入是上次异常退出留下的陈值,
/// 不能再拦住删除,先收敛成 idle/cancelled。
fn purge_session_data(root: &Path, session_id: &str) -> Result<bool, String> {
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
        .map_err(|e| e.to_string())?;
    if store
        .get_session(session_id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Ok(false);
    }
    store
        .cancel_unfinished_inputs(session_id)
        .map_err(|e| e.to_string())?;
    store
        .set_status(session_id, "idle")
        .map_err(|e| e.to_string())?;
    store
        .purge_session(session_id, root)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn process_purge(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: String,
) -> Result<String, String> {
    purge_process(&state, &project_dir, &process_id, &forget_process_prefs).await
}

/// 已关闭(注销)的线路清单(UX-035),**只读**。
///
/// 关闭线路只注销身份(processes → retired_processes),对话记录一条不删,但界面原先从此再也看不到它。
/// 这里把「身份已注销、但库里还留着有内容的会话」的线路列出来,前端在侧栏/历史弹层的「已关闭」折叠分组里
/// 只读查看(用既有的 `conversation_get`,会话 id 是进程 id 的纯函数,注销后照样取得到)。
///
/// 不列:被删除的记录、从未发过消息也没命名的空对话。旧版第一条对话同样可关闭。
/// 退役账本不记 profile,所以类型无从判断——名字只给 `title`(用户命名 ‖ 首条消息前 48 字),
/// 取不到就由前端按 `ordinal` 补「已关闭的对话 N」。字段是 snake_case,与 `process_list` 一致。
#[tauri::command]
pub fn process_closed_list(project_dir: String) -> Result<Vec<serde_json::Value>, String> {
    closed_processes(&project_dir)
}

pub(crate) fn closed_processes(project_dir: &str) -> Result<Vec<serde_json::Value>, String> {
    let root = normalized_project_root(Path::new(project_dir));
    let state_path = kanzei_core::project_state_path(&root);
    if !state_path.is_file() {
        return Ok(Vec::new());
    }
    let store = kanzei_core::SessionStore::open_read_only(&state_path)
        .map_err(|e| format!("读取历史对话失败: {e}"))?;
    let retired = store
        .list_retired_processes(&root.display().to_string())
        .map_err(|e| format!("读取已关闭独立任务失败: {e}"))?;
    let mut closed = Vec::new();
    for (id, closed_at) in retired {
        let session_id = process_session_id(&root, Some(&id));
        let Some(session) = store.get_session(&session_id).map_err(|e| e.to_string())? else {
            continue;
        };
        let naming = load_naming(&store, &session_id, ProcessKind::Conversation);
        if naming.title().is_none() && session.updated_at <= session.created_at {
            continue;
        }
        closed.push(json!({
            "id": id,
            "session_id": session_id,
            "ordinal": process_ordinal(&id),
            "title": naming.title(),
            "title_custom": naming.custom.is_some(),
            "closed_at": closed_at,
            "updated_at": session.updated_at,
        }));
    }
    Ok(closed)
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
    research_topic: Option<String>,
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
        research_topic,
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
    research_topic: Option<String>,
) -> Result<ProcessInfo, String> {
    let root = normalized_project_root(Path::new(project_dir));
    let general = crate::general_chat::is_general_root(&root);
    if general && (worktree_name.is_some() || work_item_id.is_some() || research_topic.is_some()) {
        return Err("无项目对话不能绑定工作树、需求或研究课题".into());
    }
    let profile = if general { Some("dev".into()) } else { profile };
    let phase_pipeline = if general { Some(false) } else { phase_pipeline };
    let subagents_enabled = if general {
        Some(subagents_enabled.unwrap_or(true))
    } else {
        subagents_enabled
    };
    let tracker_writes = if general { Some(false) } else { tracker_writes };
    crate::research_topics::validate_run_topic(
        &root,
        profile.as_deref(),
        research_topic.as_deref(),
        research_topic.as_deref(),
    )?;
    if research_topic.is_some() && worktree_name.is_some() {
        return Err("研究对话不能创建开发工作树".into());
    }
    // 恒主根:见本文件头的「字段口径」。worktree 路径只进 worktree_path。
    let project = root.display().to_string();
    let worktree_name = worktree_name.filter(|value| !value.trim().is_empty());
    let work_item_id = work_item_id.filter(|value| !value.trim().is_empty());
    if work_item_id.is_some() && worktree_name.is_none() {
        return Err("条目绑定只适用于带独立工作树的独立任务".into());
    }
    // UI2-0926 #13:工作树线需要一个**有提交的独立仓库**。原先要一路跑到 `git worktree add`
    // 才失败(无 Git / 上级仓库 / 没有 HEAD 三种都是),报出来的是 git 的原话;这里先说清原因。
    if worktree_name.is_some() {
        match kanzei_tools::project_state::git_state_of(&root) {
            kanzei_tools::project_state::GitState::Repo {
                has_commits: true, ..
            } => {}
            kanzei_tools::project_state::GitState::Repo { .. } => {
                return Err(
                    "独立任务需要仓库里至少有一次提交(工作树从 HEAD 分出);先提交一次再新建".into(),
                )
            }
            kanzei_tools::project_state::GitState::None => {
                return Err("独立任务需要 Git:本项目还不是 Git 仓库,先在项目头「初始化 Git」".into())
            }
            kanzei_tools::project_state::GitState::Parent { toplevel } => {
                return Err(format!(
                    "独立任务需要本项目自己的 Git 仓库:它只是位于上级仓库 {toplevel} 内,先「初始化 Git」建独立仓库"
                ))
            }
        }
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
            research_topic,
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
                .map_err(|cleanup| format!("注销半绑定独立任务失败: {cleanup}"));
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
        .ok_or_else(|| "条目绑定缺少独立任务的工作树".to_string())?;
    let output = kanzei_tools::WorkTool
        .execute(
            json!({
                "action": "claim",
                "id": work_item_id,
                "reason": "parallel-line-create:用户新建独立任务时绑定了这个条目"
            }),
            &ToolCtx::new(cwd, root.to_path_buf()),
        )
        .await;
    if output.is_error {
        Err(format!(
            "新独立任务未能绑定 {work_item_id}: {}",
            output.content
        ))
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
        None,
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
        .ok_or_else(|| "对话不存在或已被关闭".to_string())?;
    if process.research_topic.lock().unwrap().is_some()
        && profile.as_deref().is_some_and(|value| value != "research")
    {
        return Err("已绑定课题的研究对话不能切换为开发任务".into());
    }
    let general = crate::general_chat::is_general_root(&process.origin_project.0);
    if general
        && (profile.as_deref().is_some_and(|p| p != "dev")
            || phase_pipeline == Some(true)
            || tracker_writes == Some(true))
    {
        return Err("无项目对话不启用项目阶段或条目流程".into());
    }
    persist_settings_update(&process, |process| {
        if let Some(model) = model {
            *process.model.lock().unwrap() = Some(model).filter(|value| !value.trim().is_empty());
        }
        if let Some(profile) = profile {
            *process.profile.lock().unwrap() =
                Some(profile).filter(|value| !value.trim().is_empty());
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
    })?;
    // R-178 D3:任何对话的字段变更都同步落库(模型和开关状态,
    // 重启后要用库值回填)。D-367:project_dir 恒主根,直接取类型化路径。
    let root = &process.project_dir.0;
    mark_project_restored(&state, root);
    Ok(process_info(&state, &process))
}

/// Keep the published settings intact until their durable replacement succeeds.
/// Holding the existing settings locks also serializes concurrent updates.
fn persist_settings_update(
    process: &ProcessHandle,
    update: impl FnOnce(&ProcessHandle),
) -> Result<(), String> {
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    let mut model = process.model.lock().unwrap();
    let mut profile = process.profile.lock().unwrap();
    let mut reasoning = process.reasoning.lock().unwrap();
    let mut manual_models = process.manual_models.lock().unwrap();
    let mut candidate = process.clone();
    candidate.model = Arc::new(Mutex::new(model.clone()));
    candidate.profile = Arc::new(Mutex::new(profile.clone()));
    candidate.reasoning = Arc::new(Mutex::new(reasoning.clone()));
    candidate.manual_models = Arc::new(Mutex::new(manual_models.clone()));
    candidate.phase_pipeline_enabled = Arc::new(AtomicBool::new(
        process.phase_pipeline_enabled.load(Ordering::SeqCst),
    ));
    candidate.subagents_enabled = Arc::new(AtomicBool::new(
        process.subagents_enabled.load(Ordering::SeqCst),
    ));
    candidate.tracker_writes_enabled = Arc::new(AtomicBool::new(
        process.tracker_writes_enabled.load(Ordering::SeqCst),
    ));
    update(&candidate);
    persist_process(&candidate.project_dir.0, &candidate)?;
    *model = candidate.model.lock().unwrap().clone();
    *profile = candidate.profile.lock().unwrap().clone();
    *reasoning = candidate.reasoning.lock().unwrap().clone();
    *manual_models = candidate.manual_models.lock().unwrap().clone();
    process.phase_pipeline_enabled.store(
        candidate.phase_pipeline_enabled.load(Ordering::SeqCst),
        Ordering::SeqCst,
    );
    process.subagents_enabled.store(
        candidate.subagents_enabled.load(Ordering::SeqCst),
        Ordering::SeqCst,
    );
    process.tracker_writes_enabled.store(
        candidate.tracker_writes_enabled.load(Ordering::SeqCst),
        Ordering::SeqCst,
    );
    Ok(())
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
        .ok_or_else(|| "对话不存在或已被关闭".to_string())?;
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
    kanzei_harness::pending_question::cancel_owner(root, &session_id, None)?;
    if let Some(team) = kanzei_tools::team::find(root, &session_id) {
        team.stop_all();
    }
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
    let background = (killed > 0).then(|| format!("；已回收 {killed} 个后台终端"));
    let dropped = (cancelled > 0).then(|| format!("；已取消 {cancelled} 条排队输入"));
    let release = if released.is_empty() {
        String::new()
    } else {
        format!("；已释放需求绑定 {}", released.join(", "))
    };
    let dropped = dropped.unwrap_or_default();
    match disposal {
        Some(Ok(())) => Ok(format!(
            "已关闭,并回收已合并的干净工作树{}{dropped}{release}",
            background.unwrap_or_default(),
        )),
        Some(Err(kept)) => Ok(format!(
            "已关闭；{kept}{}{dropped}{release}",
            background.unwrap_or_default(),
        )),
        None => Ok(format!(
            "已关闭{}{dropped}{release}",
            background.unwrap_or_default(),
        )),
    }
}

/// 注销一条对话的进程及其持久化登记。工作树已被成功摘除、启动恢复发现目录消失、
/// 或用户显式关线时都复用这一出口，避免只删目录却留下会话继续拿它当 cwd。
pub(crate) fn unregister_parallel_process(
    state: &AppState,
    root: &Path,
    process_id: &str,
) -> Result<Vec<String>, String> {
    let state_path = kanzei_core::project_state_path(root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let session_id = process_session_id(root, Some(process_id));
    kanzei_harness::pending_question::cancel_owner(root, &session_id, None)?;
    if let Some(team) = kanzei_tools::team::find(root, &session_id) {
        team.stop_all();
    }
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
                .map_err(|e| format!("注销独立任务时收尾对话失败: {e}"))?;
        } else {
            // runtime 容器会在首次历史读取/ask 恢复时提前存在；空闲容器没有
            // promoted 输入可 finalize，直接清待答队列即可。
            let _lifecycle = runtime.lifecycle.lock().unwrap();
            runtime.retire_async();
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
        .map_err(|e| format!("删除对话登记失败: {e}"))?;
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

#[cfg(test)]
mod prefs_failure_tests {
    use super::*;
    use crate::prefs::failure_tests::{
        assert_rejected_unchanged, damaged_inputs, fixture, with_home,
    };

    #[test]
    fn cleanup_writer_reports_failed_read_without_publishing_defaults() {
        with_home("process-cleanup", |home| {
            let good = fixture(home);
            let process = format!("p1|{}", good["projects"][0].as_str().unwrap());
            for (_, bytes) in damaged_inputs(&good) {
                assert_rejected_unchanged(home, &bytes, || forget_process_prefs(&process));
            }
        });
    }

    #[test]
    fn cleanup_writer_preserves_normal_fields_and_not_found_noop() {
        with_home("cleanup-controls", |home| {
            forget_process_prefs("missing").unwrap();
            assert!(!home.join("app.json").exists());
            let good = fixture(home);
            let a = good["projects"][0].as_str().unwrap();
            let target = format!("p1|{a}");
            let other = format!("p2|{a}");
            std::fs::write(home.join("app.json"), serde_json::to_vec(&good).unwrap()).unwrap();
            forget_process_prefs(&target).unwrap();
            let bytes = std::fs::read(home.join("app.json")).unwrap();
            let after: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert!(after["process_auto_state"].get(&target).is_none());
            assert_eq!(
                after["process_auto_state"][&other],
                good["process_auto_state"][&other]
            );
            for field in ["projects", "names", "theme", "open_tools"] {
                assert_eq!(after[field], good[field], "{field}");
            }
            forget_process_prefs(&target).unwrap();
            assert_eq!(
                std::fs::read(home.join("app.json")).unwrap(),
                bytes,
                "unchanged cleanup remains a no-write noop"
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn settings_write_failure_preserves_runtime_and_success_publishes() {
        let root = temp_project("settings-failure");
        let state = AppState::default();
        let info = discussion(&state, &root.display().to_string()).await;
        let process = owned_process(&state, &root, &info.id).unwrap();
        let old_model = process.model.lock().unwrap().clone();
        let old_subagents = process.subagents_enabled.load(Ordering::SeqCst);
        let db = rusqlite::Connection::open(kanzei_core::project_state_path(&root)).unwrap();
        db.execute_batch("CREATE TRIGGER reject_settings BEFORE UPDATE ON processes BEGIN SELECT RAISE(FAIL, 'audit settings write failure'); END;").unwrap();
        let change = |candidate: &ProcessHandle| {
            *candidate.model.lock().unwrap() = Some("changed:model".into());
            candidate
                .subagents_enabled
                .store(!old_subagents, Ordering::SeqCst);
        };
        let error = persist_settings_update(&process, change).unwrap_err();
        assert!(error.contains("audit settings write failure"), "{error}");
        assert_eq!(*process.model.lock().unwrap(), old_model);
        assert_eq!(
            process.subagents_enabled.load(Ordering::SeqCst),
            old_subagents
        );
        db.execute_batch("DROP TRIGGER reject_settings").unwrap();
        persist_settings_update(&process, change).unwrap();
        assert_eq!(
            process.model.lock().unwrap().as_deref(),
            Some("changed:model")
        );
        assert_eq!(
            process.subagents_enabled.load(Ordering::SeqCst),
            !old_subagents
        );
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        let stored = store.get_process(&process.id).unwrap().unwrap();
        assert_eq!(stored.model.as_deref(), Some("changed:model"));
        assert_eq!(stored.subagents_enabled, !old_subagents);
    }

    fn owned_process(state: &AppState, root: &Path, id: &str) -> Result<ProcessHandle, String> {
        super::super::registry::resolve_conversation(state, root, Some(id))
    }

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-ux009-{tag}-{}-{}",
            std::process::id(),
            crate::run::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        normalized_project_root(&dir)
    }

    async fn discussion(state: &AppState, project: &str) -> ProcessInfo {
        create_process(
            state,
            project,
            None,
            Some("readonly".into()),
            None,
            None,
            None,
        )
        .await
        .unwrap()
    }

    fn info_of(state: &AppState, project: &str, id: &str) -> ProcessInfo {
        list_processes(state, project)
            .unwrap()
            .into_iter()
            .find(|item| item.id == id)
            .unwrap_or_else(|| panic!("列表里没有 {id}"))
    }

    /// 界面不再出现 `pN` / 「默认」:展示名 = 用户命名 ‖ 首条消息前 48 字 ‖ 类型 + 序号。
    #[tokio::test]
    async fn 对话命名_改名清除_自动标题_数字序() {
        let root = temp_project("naming");
        let project = root.display().to_string();
        let state = AppState::default();
        let mut created = Vec::new();
        for _ in 0..11 {
            created.push(discussion(&state, &project).await);
        }
        let first = &created[0];
        assert_eq!(first.kind, "discussion");
        assert_eq!(first.label, "讨论 1");
        assert_eq!((first.ordinal, first.title.as_deref()), (Some(1), None));
        assert!(!first.title_custom);

        // 数字序:p2 在 p10 之前。
        let list = list_processes(&state, &project).unwrap();
        let ordinals: Vec<_> = list.iter().map(|item| item.ordinal).collect();
        let expected: Vec<_> = (1..=11).map(Some).collect();
        assert_eq!(ordinals, expected);
        assert_eq!(
            (list[0].kind.as_str(), list[0].label.as_str()),
            ("discussion", "讨论 1")
        );

        // 用户命名(整理空白)→ 优先于一切;空白 = 清除,回到类型 + 序号。
        let second = created[1].id.clone();
        rename_process(&state, &project, &second, "  方案  对照 ").unwrap();
        let renamed = info_of(&state, &project, &second);
        assert_eq!(renamed.label, "方案 对照");
        assert_eq!(renamed.title.as_deref(), Some("方案 对照"));
        assert!(renamed.title_custom);
        assert!(renamed.updated_at.is_some(), "改名顺手建出了会话行");
        rename_process(&state, &project, &second, "   ").unwrap();
        let cleared = info_of(&state, &project, &second);
        assert_eq!(cleared.label, "讨论 2");
        assert!(!cleared.title_custom && cleared.title.is_none());

        // 自动标题:首条消息第一行的前 48 字;用户命名压过它,清掉命名后回到它。
        let third = created[2].id.clone();
        let session_id = process_session_id(&root, Some(&third));
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store.create_session(&session_id, &project, None).unwrap();
        store
            .admit_input(
                &session_id,
                "in-1",
                "梳理登录需求\n再补一行细节",
                kanzei_core::Delivery::Queue,
            )
            .unwrap();
        let auto = info_of(&state, &project, &third);
        assert_eq!(auto.label, "梳理登录需求");
        assert_eq!(auto.title.as_deref(), Some("梳理登录需求"));
        assert!(!auto.title_custom, "自动标题不是用户命名");
        rename_process(&state, &project, &third, "登录页").unwrap();
        assert_eq!(info_of(&state, &project, &third).label, "登录页");
        rename_process(&state, &project, &third, "").unwrap();
        assert_eq!(info_of(&state, &project, &third).label, "梳理登录需求");

        // 旧格式对话也能改名,但不会因为首条消息被改名。
        let main_id = crate::ensure_default_process(&state, &root).id;
        rename_process(&state, &project, &main_id, "总控").unwrap();
        let main = info_of(&state, &project, &main_id);
        assert_eq!(
            (main.label.as_str(), main.kind.as_str()),
            ("总控", "conversation")
        );

        // 不存在的对话、过长的名字都拒绝。
        let error = rename_process(&state, &project, "p99|nowhere", "x").unwrap_err();
        assert!(error.contains("不存在"), "{error}");
        let error = rename_process(&state, &project, &second, &"长".repeat(61)).unwrap_err();
        assert!(error.contains("太长"), "{error}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 删除对话 = 真删记录 + 注销登记;运行中与不存在身份拒绝,且拒绝时什么都不动。
    #[tokio::test]
    async fn 删除对话_真删记录与登记_运行中与不存在身份拒绝() {
        let root = temp_project("purge");
        let project = root.display().to_string();
        let state = AppState::default();
        let target = discussion(&state, &project).await;
        let bystander = discussion(&state, &project).await;
        let session_id = process_session_id(&root, Some(&target.id));
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store.create_session(&session_id, &project, None).unwrap();
        store
            .admit_input(
                &session_id,
                "in-1",
                "要被删掉的话",
                kanzei_core::Delivery::Queue,
            )
            .unwrap();
        store
            .append_event(&session_id, "note", &json!({"text": "x"}))
            .unwrap();

        let forgotten = Mutex::new(Vec::<String>::new());
        let forget = |id: &str| -> Result<(), String> {
            forgotten.lock().unwrap().push(id.to_string());
            Ok(())
        };

        let error = purge_process(&state, &project, &legacy_process_id(&root), &forget)
            .await
            .unwrap_err();
        assert!(error.contains("不存在"), "{error}");

        let runtime = crate::runtime_for(&state, &session_id);
        runtime.running.store(true, Ordering::SeqCst);
        let error = purge_process(&state, &project, &target.id, &forget)
            .await
            .unwrap_err();
        assert!(error.contains("正在运行"), "{error}");
        assert!(state.processes.lock().unwrap().contains_key(&target.id));
        assert!(store.get_session(&session_id).unwrap().is_some());
        assert!(forgotten.lock().unwrap().is_empty(), "拒绝时不清偏好");
        runtime.running.store(false, Ordering::SeqCst);

        let message = purge_process(&state, &project, &target.id, &forget)
            .await
            .unwrap();
        assert_eq!(message, "对话已删除");
        assert!(!state.processes.lock().unwrap().contains_key(&target.id));
        assert!(store.get_session(&session_id).unwrap().is_none());
        assert_eq!(
            forgotten.lock().unwrap().as_slice(),
            std::slice::from_ref(&target.id)
        );
        // 注销进退役账本:编号不会被新线复用,旧置顶/排序键不会误认新对话。
        assert!(store
            .list_retired_process_ids(&project)
            .unwrap()
            .contains(&target.id));
        // 旁观的那条不受影响,列表里只剩它。
        let ids: Vec<_> = list_processes(&state, &project)
            .unwrap()
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(ids.as_slice(), std::slice::from_ref(&bystander.id));

        assert_eq!(
            purge_process(&state, &project, &target.id, &forget)
                .await
                .unwrap(),
            "对话已删除"
        );
        assert!(crate::conversation::conversation_get(
            project.clone(),
            None,
            Some(target.id.clone())
        )
        .unwrap()
        .is_empty());
        assert!(
            crate::conversation::conversation_list(project.clone(), Some(target.id.clone()))
                .unwrap()
                .is_empty()
        );
        assert!(crate::conversation::conversation_trace_get(
            project.clone(),
            None,
            Some(target.id.clone())
        )
        .unwrap()
        .is_empty());
        assert!(
            store.get_session(&session_id).unwrap().is_none(),
            "读取不能复活已删除对话"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// UX-035:关闭线路只注销身份、对话记录还在——「已关闭」清单把有内容的列出来(最近关闭的在前),
    /// 真删的、从没用过的空线都不列;清单的 id 能直接喂给只读的 `conversation_get`。
    #[tokio::test]
    async fn 已关闭线路清单_有内容的才列_真删与空线不列() {
        let root = temp_project("closed-list");
        let project = root.display().to_string();
        let state = AppState::default();
        let talked = discussion(&state, &project).await;
        let named = discussion(&state, &project).await;
        let empty = discussion(&state, &project).await;
        let purged = discussion(&state, &project).await;
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        for (info, text) in [
            (&talked, "梳理登录需求\n再补一行"),
            (&purged, "会被删掉的话"),
        ] {
            let session_id = process_session_id(&root, Some(&info.id));
            store.create_session(&session_id, &project, None).unwrap();
            let input_id = format!("in-{}", info.ordinal.unwrap_or(0));
            store
                .admit_input(&session_id, &input_id, text, kanzei_core::Delivery::Queue)
                .unwrap();
            // 跑完的输入才是真对话:关闭只回收没有结局的输入,已完成的首条消息仍是自动标题的来源。
            store.promote_next_queue(&session_id).unwrap();
            store.finish_input(&input_id, true).unwrap();
        }
        rename_process(&state, &project, &named.id, "方案对照").unwrap();
        assert!(
            closed_processes(&project).unwrap().is_empty(),
            "还没关闭,清单为空"
        );

        for info in [&talked, &named, &empty] {
            let handle = owned_process(&state, &root, &info.id).unwrap();
            close_process(&state, &handle).await.unwrap();
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        purge_process(&state, &project, &purged.id, &|_| Ok(()))
            .await
            .unwrap();

        let closed = closed_processes(&project).unwrap();
        let ids: Vec<_> = closed
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        // 最近关闭的在前;空线与被真删的不在清单里。
        assert_eq!(ids, [named.id.as_str(), talked.id.as_str()]);
        assert_eq!(closed[0]["title"], "方案对照");
        assert_eq!(closed[0]["title_custom"], true);
        assert_eq!(closed[1]["title"], "梳理登录需求");
        assert_eq!(
            closed[1]["title_custom"], false,
            "首条消息自动标题不算用户命名"
        );
        assert_eq!(closed[1]["ordinal"], 1);
        assert_eq!(
            closed[1]["session_id"],
            process_session_id(&root, Some(&talked.id))
        );
        assert!(
            closed[0]["closed_at"].as_i64().unwrap() >= closed[1]["closed_at"].as_i64().unwrap()
        );
        // 它们的对话仍能被只读取回(会话 id 是进程 id 的纯函数,注销后照样取得到)。
        assert!(store
            .get_session(closed[1]["session_id"].as_str().unwrap())
            .unwrap()
            .is_some());
        // 没有 state.db 的目录:空清单,不报错。
        let nowhere = temp_project("closed-list-none");
        std::fs::remove_dir_all(nowhere.join(".kanzei")).ok();
        assert!(closed_processes(&nowhere.display().to_string())
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(&nowhere).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    /// `process_closed_list` 的 IPC 形状(前端「已关闭」分组直接读这些键)。
    #[tokio::test]
    async fn process_closed_list_形状与ipc契约一致() {
        let root = temp_project("closed-contract");
        let project = root.display().to_string();
        let state = AppState::default();
        let info = discussion(&state, &project).await;
        rename_process(&state, &project, &info.id, "契约样本").unwrap();
        let handle = owned_process(&state, &root, &info.id).unwrap();
        close_process(&state, &handle).await.unwrap();
        let closed = closed_processes(&project).unwrap();
        assert_eq!(closed.len(), 1);
        crate::ipc_contract::tests::check_contract(
            "process_closed_list",
            crate::ipc_contract::shape(&serde_json::Value::Array(closed)),
            "process_closed_list 的 IPC 形状变了",
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn closed_conversation_can_rename_and_delete_after_restart() {
        let root = temp_project("closed-manage");
        let other_root = temp_project("closed-other");
        let project = root.display().to_string();
        let state = AppState::default();
        let target = discussion(&state, &project).await;
        let bystander = discussion(&state, &project).await;
        rename_process(&state, &project, &target.id, "旧对话").unwrap();
        rename_process(&state, &project, &bystander.id, "保留的对话").unwrap();
        close_process(&state, &owned_process(&state, &root, &target.id).unwrap())
            .await
            .unwrap();
        let restarted = AppState::default();
        rename_process(&restarted, &project, &target.id, "重新命名的历史").unwrap();
        assert_eq!(
            closed_processes(&project).unwrap()[0]["title"],
            "重新命名的历史"
        );
        assert!(!restarted.processes.lock().unwrap().contains_key(&target.id));
        // 同一个 id 不能拿到另一个项目中改名/删除，也不能对已删除身份重新造一条空记录。
        assert!(rename_process(
            &restarted,
            &other_root.display().to_string(),
            &target.id,
            "错误项目"
        )
        .is_err());
        assert!(purge_process(
            &restarted,
            &other_root.display().to_string(),
            &target.id,
            &|_| Ok(())
        )
        .await
        .is_err());
        assert_eq!(
            purge_process(&restarted, &project, &target.id, &|_| Ok(()))
                .await
                .unwrap(),
            "对话已删除"
        );
        assert!(closed_processes(&project).unwrap().is_empty());
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        assert!(store.get_session(&target.session_id).unwrap().is_none());
        assert!(store.get_session(&bystander.session_id).unwrap().is_some());
        assert!(store
            .list_retired_process_ids(&project)
            .unwrap()
            .contains(&target.id));
        assert!(rename_process(&restarted, &project, &target.id, "不能复活").is_err());
        assert!(!list_processes(&AppState::default(), &project)
            .unwrap()
            .iter()
            .any(|p| p.id == target.id));
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&other_root).ok();
    }

    /// 带工作树的独立任务走关闭线路语义:树没回收(这里目录不是 git 工作树 = 查不出分支)
    /// 就原样留下,对话记录也留下,只把线路从列表移除并说明——不静默丢活。
    #[tokio::test]
    async fn 删除带工作树的独立任务_树被留下时记录也留下() {
        let root = temp_project("purge-wt");
        let project = root.display().to_string();
        let worktree = root.join("wt-line");
        std::fs::create_dir_all(&worktree).unwrap();
        let state = AppState::default();
        let id = format!("p7|{project}");
        state.processes.lock().unwrap().insert(
            id.clone(),
            ProcessHandle {
                id: id.clone(),
                origin_project: crate::ProjectRoot(root.clone()),
                project_dir: crate::ProjectRoot(root.clone()),
                worktree_path: Some(WorktreeRoot(worktree.clone())),
                branch: None,
                model: Arc::new(Mutex::new(None)),
                profile: Arc::new(Mutex::new(None)),
                research_topic: Arc::new(Mutex::new(None)),
                reasoning: Arc::new(Mutex::new(None)),
                manual_models: Arc::new(Mutex::new(Vec::new())),
                phase_pipeline_enabled: Arc::new(AtomicBool::new(false)),
                subagents_enabled: Arc::new(AtomicBool::new(true)),
                tracker_writes_enabled: Arc::new(AtomicBool::new(false)),
            },
        );
        let session_id = process_session_id(&root, Some(&id));
        persist_process(&root, &owned_process(&state, &root, &id).unwrap()).unwrap();
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store.create_session(&session_id, &project, None).unwrap();

        let message = purge_process(&state, &project, &id, &|_| Ok(()))
            .await
            .unwrap();
        assert!(message.contains("工作树仍保留"), "{message}");
        assert!(worktree.is_dir(), "工作树原样留着");
        assert!(
            store.get_session(&session_id).unwrap().is_some(),
            "树还在,对话记录不删"
        );
        assert!(!state.processes.lock().unwrap().contains_key(&id));
        // 用户之后明确删除保留下来的历史记录：只删对话，遗留工作树仍在。
        purge_process(&state, &project, &id, &|_| Ok(()))
            .await
            .unwrap();
        assert!(store.get_session(&session_id).unwrap().is_none());
        assert!(worktree.is_dir(), "删除历史不会连带删除遗留工作树");
        std::fs::remove_dir_all(&root).ok();
    }
}
