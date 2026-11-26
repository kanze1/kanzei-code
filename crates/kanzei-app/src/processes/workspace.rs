//! 工作树生命周期、合并、收割与写租约(R-254 批1,纯搬迁自 processes.rs)。
//!
//! 独立理由:工作树是「树怎么建、怎么列、怎么合并、怎么放弃、关线时怎么处置、交付
//! 怎么回写主根 tracker」的变更理由——与进程注册(lifecycle/registry)互不相关:改
//! 一条合并策略不必读懂进程编号;改一处收割回写不必读懂关线顺序(照 files_view.rs
//! 模式)。项目级写租约也归本域:合并/放弃改写既有工作区,是工作树操作的一部分。
//!
//! 危险点(搬迁纪律):合并/放弃是项目级写操作,必须接写仲裁
//! (`acquire_project_write_lease`,超时 120s 防永久 pending);`with_idle_bound_process`
//! 在绑定线路 lifecycle 临界区内检查,消除 check-then-act 窗口;`reclaim_worktree_on_close`
//! 只自动回收「工作区干净 + 分支已是主 HEAD 祖先」的树,任何一条不成立就原样留并给
//! 回收命令;`\\?\` 前缀参数 git 收不下,放弃前必须剥掉(`git_arg_path`)。

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use kanzei_harness::orchestration::{ProjectExecutionCoordinator, WriterLease, WriterLeaseRequest};

use crate::{normalized_project_root, process_session_id, AppState, WorktreeInfo};
use kanzei_tools::worktree as wt;

use super::lifecycle::unregister_parallel_process;
use super::registry::bound_thread_for_worktree;

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
pub(crate) fn reclaim_worktree_on_close(root: &Path, worktree: &Path) -> Result<(), String> {
    let branch = wt::worktree_current_branch(worktree)
        .map_err(|error| format!("工作树 {} 的分支查不出来: {error}", worktree.display()))?;
    let (files, _) = wt::worktree_status(root, worktree)
        .map_err(|error| format!("工作树 {} 的状态查不出来: {error}", worktree.display()))?;
    let recycle_hint = format!(
        "回收命令:`git -C \"{}\" worktree remove --force \"{}\"` 与 \
         `git -C \"{}\" branch -D {branch}`",
        wt::git_arg_path(root),
        wt::git_arg_path(worktree),
        wt::git_arg_path(root),
    );
    if !files.is_empty() {
        return Err(format!(
            "工作树 {} 有 {} 处未提交改动,关线时保留未删(分支 {branch})。{recycle_hint}",
            worktree.display(),
            files.len(),
        ));
    }
    let merged = wt::worktree_command(root, &["merge-base", "--is-ancestor", &branch, "HEAD"])
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
    let sha = wt::rev_parse(root, &format!("refs/heads/{branch}"));
    let removed = wt::worktree_command(root, &["worktree", "remove", &wt::git_arg_path(worktree)])
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
            let _ = wt::worktree_command(root, &["update-ref", "-d", &refname, &sha]);
            if wt::branch_exists(root, &branch) {
                return Err(format!(
                    "工作树 {} 已摘掉,但分支 {branch} 没删成(它可能已经不在关线时那个 \
                     sha 上)。若确认可丢弃:`git -C \"{}\" branch -D {branch}`",
                    worktree.display(),
                    wt::git_arg_path(root),
                ));
            }
            Ok(())
        }
        None => Err(format!(
            "工作树 {} 已摘掉,但分支 {branch} 的 sha 解析不出来,没敢删它。\
             若确认可丢弃:`git -C \"{}\" branch -D {branch}`",
            worktree.display(),
            wt::git_arg_path(root),
        )),
    }
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
/// R-171 批4 的口径:合并/放弃会改写既有工作区,必须进源码写仲裁。单纯创建
/// worktree 已拆到 `AppState.worktree_ops`，不再与运行中的代码 writer 互斥。
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

/// `worktree_create` 的非 Tauri 内核:取工作树元数据闸 + 建树。
///
/// 拆出来是为了能测——并发测试要让「建线」和「建工作树」真的同时打进同一个仲裁。
pub(crate) async fn create_worktree_arbitrated(
    state: &AppState,
    root: &Path,
    name: &str,
) -> Result<WorktreeInfo, String> {
    let _worktree_guard = state.worktree_ops.lock().await;
    wt::create_worktree(root, name)
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
    let root_key = wt::worktree_key(&root);
    let bound: std::collections::BTreeMap<String, String> = state
        .processes
        .lock()
        .unwrap()
        .values()
        .filter_map(|process| {
            let worktree = process.worktree_path.as_ref()?;
            Some((wt::worktree_key(worktree.as_path()), process.id.clone()))
        })
        .collect();
    let mut out = Vec::new();
    for entry in wt::git_worktrees(&root)? {
        let key = wt::worktree_key(&entry.path);
        if key == root_key || entry.bare || entry.prunable {
            continue;
        }
        // 探测失败不整条丢:树在清单里但状态取不到,用户更需要看见它并知道为什么。
        let (files, diff) = wt::worktree_status(&root, &entry.path)
            .unwrap_or_else(|error| (vec![format!("(状态不可读: {error})")], String::new()));
        out.push(WorktreeInfo {
            path: wt::git_arg_path(&entry.path),
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
    let worktree = wt::validate_worktree_path(&root, &worktree_path)?;
    let branch = wt::worktree_current_branch(&worktree)?;
    let (files, diff) = wt::worktree_status(&root, &worktree)?;
    Ok(WorktreeInfo {
        path: wt::git_arg_path(&worktree),
        branch,
        clean: files.is_empty(),
        files,
        diff,
        bound_process: None,
    })
}

/// R-179 验收②③:合并前的冲突预检命令——UI 在确认合并前调用,返回冲突文件
/// 列表(可读形态)。无冲突返回空列表,命令本身不执行合并。
#[tauri::command]
pub fn worktree_merge_preview(
    project_dir: String,
    worktree_path: String,
) -> Result<Vec<String>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = wt::validate_worktree_path(&root, &worktree_path)?;
    let branch = wt::worktree_current_branch(&worktree)?;
    let check = wt::worktree_command(&root, &["merge-tree", "--write-tree", "HEAD", &branch])?;
    if check.status.success() {
        return Ok(Vec::new());
    }
    let conflicts = wt::parse_merge_tree_conflicts(&check.stdout);
    Ok(conflicts)
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
    merge_worktree_and_release(&state, &root, &worktree_path)
}

/// 合并与取得线释放的可测试内核。Git 合并已经成功后，release 失败不能伪装成
/// “合并失败”诱导用户重试 merge；结果保留成功事实并带明确警告，关线仍可重试释放。
pub(crate) fn merge_worktree_and_release(
    state: &AppState,
    root: &Path,
    worktree_path: &str,
) -> Result<String, String> {
    let worktree = wt::validate_worktree_path(root, worktree_path)?;
    with_idle_bound_process(state, root, &worktree, "合并", || {
        wt::merge_worktree(root, worktree_path)
    })
}

/// 在绑定线路的 lifecycle 临界区内检查并执行工作树破坏性操作。检查通过后，
/// `run_prompt` 也无法在 Git 操作结束前启动该线，消除 check-then-act 窗口。
pub(crate) fn with_idle_bound_process<T>(
    state: &AppState,
    root: &Path,
    worktree: &Path,
    action: &str,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let project = root.display().to_string();
    let bound = bound_thread_for_worktree(state, root, &project, &wt::worktree_key(worktree))?;
    let runtime = bound.as_deref().and_then(|process_id| {
        let session_id = process_session_id(root, Some(process_id));
        state.runtimes.lock().unwrap().get(&session_id).cloned()
    });
    let _lifecycle = runtime
        .as_ref()
        .map(|runtime| runtime.lifecycle.lock().unwrap());
    if runtime.as_ref().is_some_and(|runtime| runtime.is_running()) {
        return Err(format!(
            "独立任务运行中，请先停止并等它收尾，才能{action}工作树"
        ));
    }
    operation()
}

/// 放弃命令的可测试内核。未提交改动时 git 必须拒绝并保留现场；写租约仍由
/// Tauri 命令承担，不把协调器行为混进结果测试。
pub(crate) fn discard_worktree_checked(root: &Path, worktree_path: &str) -> Result<String, String> {
    let worktree = wt::validate_worktree_path(root, worktree_path)?;
    // git 收不下 `\\?\` 前缀的参数(见 `git_arg_path`),而 validate 出来的正是
    // canonicalize 的产物——不剥这一层,放弃工作树在 Windows 上永远失败。
    let output = wt::worktree_command(root, &["worktree", "remove", &wt::git_arg_path(&worktree)])?;
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
pub(crate) fn discard_worktree_and_unregister(
    state: &AppState,
    root: &Path,
    worktree_path: &str,
) -> Result<String, String> {
    let worktree = wt::validate_worktree_path(root, worktree_path)?;
    let project = root.display().to_string();
    let bound_process_id =
        bound_thread_for_worktree(state, root, &project, &wt::worktree_key(&worktree))?;
    let result = with_idle_bound_process(state, root, &worktree, "放弃", || {
        discard_worktree_checked(root, &wt::git_arg_path(&worktree))
    })?;
    if let Some(process_id) = bound_process_id {
        unregister_parallel_process(state, root, &process_id)?;
        Ok(format!("{result};已关闭绑定的独立任务"))
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
