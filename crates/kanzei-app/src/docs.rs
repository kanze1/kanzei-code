//! Project document and tracker commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kanzei_tools::docstore::{
    DocStore, DECISIONS, DEFECTS, FINDINGS, IDEAS, REQUIREMENTS, SOURCES,
};
use serde_json::json;

pub(crate) const CONVENTIONS_REL: &str = ".kanzei/project/conventions.md";

mod research_snapshot;
use research_snapshot::research_topics;

use crate::{normalized_project_root, state::hidden_command};
use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use kanzei_tools::worktree as wt;

/// git 概览:分支 + 未提交改动数(状态栏显示)。
#[tauri::command]
pub async fn git_status(
    project_dir: String,
    worktree_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let project_root = normalized_project_root(Path::new(&project_dir));
    // project_dir 恒为项目主根;线路的 Git 状态必须显式绑定到它自己的工作树。
    // 复用统一校验,拒绝把任意目录当成线路工作树传进来。
    let root = match worktree_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        Some(path) => wt::validate_worktree_path(&project_root, path)?,
        None => project_root,
    };
    tokio::task::spawn_blocking(move || {
        // UI2-0926 #13:先看代码树自己是不是仓库。不是的话给出「无 Git / 位于上级仓库」的事实,
        // 绝不把上级仓库的分支与改动当成本项目的显示出来(原先 rev-parse 在上级仓库里照样成功)。
        // 只读 .git/HEAD/refs(git_state_of),不跑整套项目探测——bash 每跑完一条就作废探测缓存。
        match kanzei_tools::project_state::git_state_of(&root) {
            kanzei_tools::project_state::GitState::Repo { .. } => {}
            kanzei_tools::project_state::GitState::None => {
                return json!({
                    "repo": "none", "branch": null, "changes": 0, "last": null,
                    "additions": 0, "deletions": 0, "files": [],
                });
            }
            kanzei_tools::project_state::GitState::Parent { toplevel } => {
                return json!({
                    "repo": "parent", "toplevel": toplevel, "branch": null, "changes": 0,
                    "last": null, "additions": 0, "deletions": 0, "files": [],
                });
            }
        }
        let run = |args: &[&str]| -> Option<String> {
            let out = hidden_command("git")
                .args(args)
                .current_dir(&root)
                .output()
                .ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"]);
        let porcelain = run(&["status", "--porcelain"]).unwrap_or_default();
        let changes = porcelain.lines().filter(|l| !l.trim().is_empty()).count();
        let last = run(&["log", "-1", "--format=%h %s"]);
        // 逐文件增删:输入框上方的「本轮改动」条要按文件展开,不能只给一个改动数。
        // 基线取 HEAD(未提交的全部改动)——用户问的是「我这一轮把工作树改成了什么样」。
        // 二进制文件 numstat 给 `-`,记 0 并标出来,别把它算成 0 行改动骗人。
        let mut files: Vec<serde_json::Value> = Vec::new();
        let mut additions = 0u64;
        let mut deletions = 0u64;
        for line in run(&["diff", "--numstat", "HEAD"])
            .unwrap_or_default()
            .lines()
        {
            let mut parts = line.split('\t');
            let (add, del, path) = (parts.next(), parts.next(), parts.next());
            let (Some(add), Some(del), Some(path)) = (add, del, path) else {
                continue;
            };
            let binary = add == "-" || del == "-";
            let add: u64 = add.parse().unwrap_or(0);
            let del: u64 = del.parse().unwrap_or(0);
            additions += add;
            deletions += del;
            files.push(
                json!({ "path": path, "additions": add, "deletions": del, "binary": binary }),
            );
        }
        // 未跟踪文件不进 diff --numstat,但它们确实是本轮的产物,漏掉会让统计偏小。
        for line in porcelain.lines() {
            let Some(path) = line.strip_prefix("?? ") else {
                continue;
            };
            files.push(
                json!({ "path": path.trim(), "additions": 0, "deletions": 0, "untracked": true }),
            );
        }
        json!({
            "repo": "own",
            "branch": branch, "changes": changes, "last": last,
            "additions": additions, "deletions": deletions, "files": files,
        })
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn conventions_read(project_dir: String) -> Result<serde_json::Value, String> {
    kanzei_tools::conventions::drafts::snapshot(&conventions_root(&project_dir))
}

#[tauri::command]
pub fn conventions_save(
    project_dir: String,
    content: String,
    expected_hash: String,
    proposal_hash: Option<String>,
) -> Result<String, String> {
    kanzei_tools::conventions::drafts::save_user(
        &conventions_root(&project_dir),
        &content,
        &expected_hash,
        proposal_hash.as_deref(),
    )
}

#[tauri::command]
pub fn conventions_discard(project_dir: String, expected_hash: String) -> Result<(), String> {
    kanzei_tools::conventions::drafts::discard(&conventions_root(&project_dir), &expected_hash)
}

fn conventions_root(project_dir: &str) -> PathBuf {
    kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| PathBuf::from(project_dir))
}

/// 兼容旧客户端的新建入口；当前界面通过 Agent 从项目生成。
#[tauri::command]
pub fn conventions_init(project_dir: String) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let path = root.join(CONVENTIONS_REL);
    if path.is_file() {
        return Ok(path.display().to_string());
    }
    kanzei_tools::conventions::drafts::create(
        &root,
        "# 项目约束与规范\n\n待根据项目清单、README、测试和发布配置生成。\n",
    )?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn test_runs_snapshot(project_dir: String) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    kanzei_tools::test_record::test_runs_snapshot(&root)
}

#[tauri::command]
pub async fn test_run_record(
    state: tauri::State<'_, crate::AppState>,
    project_dir: String,
    title: String,
    status: String,
    command: Option<String>,
    summary: Option<String>,
    refs: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    // R-171 批4:test_record 是独立写入口(写 tests.md),接入项目级写仲裁——
    // 与 writer run 竞争同一租约,不能绕过协调器(D-227 并发覆盖的机械门禁)。
    let _lease = state
        .coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            write_scope: root.clone(),
            run_id: format!("test_record_{}", crate::run::now_ms()),
            process_id: "test_record".into(),
            reason: "test record write".into(),
        })
        .await
        .map_err(|e| format!("无法获取项目写租约: {e}"))?;
    kanzei_tools::test_record::append_test_run(
        &root,
        &title,
        &status,
        command.as_deref(),
        summary.as_deref(),
        refs.as_deref(),
    )
}

/// 顺手做的幂等维护写(UX-014):只在写权空闲时拿租约,拿不到就返回 `None` 让调用方跳过。
///
/// 此前回填关联也走阻塞式 `acquire_writer_lease`:agent 一轮可能跑几十分钟,期间租约被
/// 持有,回填就一直排队,前端又等它返回才去取快照——测试记录页签空白、不更新。
/// 回填只是补旧记录的「关联」字段,下次刷新再补无妨;读快照从来不需要租约。
/// 先看有没有 writer(有就不排队),没有再带一个很短的预算去抢;抢不到要撤掉排队登记,
/// 不然空等的申请会留在队列里,等 writer 交权时白白接过去再丢弃。
async fn writer_lease_if_idle<C: ProjectExecutionCoordinator + ?Sized>(
    coordinator: &C,
    request: kanzei_harness::orchestration::WriterLeaseRequest,
    budget: std::time::Duration,
) -> Result<Option<kanzei_harness::orchestration::WriterLease>, String> {
    if coordinator.snapshot(&request.write_scope).writer.is_some() {
        return Ok(None);
    }
    let run_id = request.run_id.clone();
    match tokio::time::timeout(budget, coordinator.acquire_writer_lease(request)).await {
        Ok(Ok(lease)) => Ok(Some(lease)),
        Ok(Err(e)) => Err(format!("无法获取项目写租约: {e}")),
        Err(_) => {
            coordinator.cancel_waiter(&run_id);
            Ok(None)
        }
    }
}

/// R-130:批量初始化测试→条目映射。扫描 tests.md 旧记录,从标题回填「关联」字段。
/// 与 test_run_record 同为 tests.md 写入口,接入项目级写仲裁(R-171 批4 模式),
/// 不能绕过协调器直接写文件(D-227 并发覆盖的机械门禁)。
///
/// UX-014:这是**可跳过**的维护写——写权被占着(agent 在跑)就返回 `skipped: true`,
/// 不排队等待;前端读快照不依赖它。
#[tauri::command]
pub async fn test_runs_init_refs(
    state: tauri::State<'_, crate::AppState>,
    project_dir: String,
) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let Some(_lease) = writer_lease_if_idle(
        &*state.coordinator,
        kanzei_harness::orchestration::WriterLeaseRequest {
            write_scope: root.clone(),
            run_id: format!("test_init_refs_{}", crate::run::now_ms()),
            process_id: "test_record".into(),
            reason: "test refs backfill".into(),
        },
        std::time::Duration::from_millis(300),
    )
    .await?
    else {
        return Ok(json!({ "backfilled": 0, "skipped": true }));
    };
    kanzei_tools::test_record::initialize_refs(&root)
}

/// 项目文档快照。
///
/// **D-249:读失败绝不降级成空列表。** 这里的每一次 `load()` 都要么给出真实条目,
/// 要么把错误抛给前端——`unwrap_or_default()` 会把「读不到」伪装成「一条都没有」,
/// 而它长得像成功,所以下游没有任何一环会重试或报警:计数归零、列表闪空、筛选
/// 回落全从这条通道来。抛错之后前端两处 catch(refreshDocs / refreshDocsSoon)
/// 都不会重绘,**上一份快照原样留在屏幕上**,这正是我们要的降级方式。
///
/// 唯一的例外是开头那次归档:它是**写**,写不成不该让读挂掉,失败收进 `warnings`。
#[tauri::command(async)]
pub fn docs_snapshot(project_dir: String) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    // 终态条目顺手归档:幂等、且只在"真有条目刚进终态"时才写盘。它与本次快照的
    // 读、以及 agent 那边的 tracker 写完全可能同时在飞(D-249 第④层),所以走
    // 限时锁——拿不到就跳过,下次刷新再归档,绝不让文档面板为了一次归档卡住。
    // 预算故意给得很短:UI 刷新的响应性优先级高于"这一轮就把归档做掉"。
    let mut warnings: Vec<String> = Vec::new();
    for kind in [&REQUIREMENTS, &DEFECTS, &IDEAS] {
        let store = DocStore::open(&root, kind);
        match store.try_lock(std::time::Duration::from_millis(200)) {
            // 拿到锁才归档;archive_terminal 内部同线程重入,不会自锁死。
            Ok(Some(_lock)) => {
                if let Err(e) = store.archive_terminal() {
                    // 原先是 `let _ =`:归档失败连一行日志都没有。写失败可以不
                    // 拖垮读,但不能无声无息。
                    tracing::warn!(target: "kanzei::docs", path = %store.path.display(), error = %e, "归档终态条目失败");
                    warnings.push(format!("{} 归档失败: {e}", kind.rel_path));
                }
            }
            Ok(None) => {
                tracing::debug!(target: "kanzei::docs", path = %store.path.display(), "归档跳过:写锁被占用")
            }
            Err(e) => {
                tracing::warn!(target: "kanzei::docs", path = %store.path.display(), error = %e, "取归档写锁失败");
                warnings.push(format!("{} 取写锁失败: {e}", kind.rel_path));
            }
        }
    }
    let read_failed = |kind: &kanzei_tools::docstore::DocKind, e: std::io::Error| {
        format!("读取 {} 失败: {e}", kind.rel_path)
    };
    // D-296:一次快照建立单份 active/archive 读缓存。后续批次、计数、依赖、调度与
    // IPC 组装都只消费这份缓存,不再让同一个 md 文件被不同闭包重复解析。
    // source/finding 这两条线零写入方(dev 档提示词里没有 source/finding 工具)、
    // 零消费者,绝大多数项目里文件根本不存在。可 DocStore::open + load 会为了取锁
    // create_dir_all 父目录并造一个 .lock 文件——每次快照都在给一条没人用的线造垃圾,
    // 还让前端拿到两个恒空数组去渲染两块永远的「(空)」。文件不存在就不入列;
    // 下游全部走 active_entries/archived_entries 的 &[] 回落,一行都不用改。
    let mut kinds: Vec<&'static kanzei_tools::docstore::DocKind> =
        vec![&REQUIREMENTS, &DEFECTS, &IDEAS];
    for optional in [&SOURCES, &FINDINGS] {
        let store = DocStore::open(&root, optional);
        if store.path.is_file() || store.archive_file().is_file() {
            kinds.push(optional);
        }
    }
    let mut active: BTreeMap<&'static str, Vec<kanzei_tools::docstore::Entry>> = BTreeMap::new();
    let mut archived_docs: BTreeMap<&'static str, Vec<kanzei_tools::docstore::Entry>> =
        BTreeMap::new();
    for kind in kinds {
        let store = DocStore::open(&root, kind);
        active.insert(
            kind.rel_path,
            store.load().map_err(|e| read_failed(kind, e))?,
        );
        archived_docs.insert(
            kind.rel_path,
            store.load_archive().map_err(|e| read_failed(kind, e))?,
        );
    }
    let active_entries = |kind: &'static kanzei_tools::docstore::DocKind| {
        active.get(kind.rel_path).map(Vec::as_slice).unwrap_or(&[])
    };
    let archived_entries = |kind: &'static kanzei_tools::docstore::DocKind| {
        archived_docs
            .get(kind.rel_path)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    };
    // 一次快照只读取一次提交历史。按条目多次起 Git 会把“即时刷新”反过来变成卡顿源。
    let batch_ids: Vec<String> = [&REQUIREMENTS, &DEFECTS]
        .into_iter()
        .flat_map(|kind| active_entries(kind).iter().map(|entry| entry.id.clone()))
        .collect();
    let derived_batch_done =
        kanzei_tools::git_batches::completed_batches_for_entries(&root, batch_ids).ok();
    let states = kanzei_tools::tracker::dependency_states_from_documents(
        (
            active_entries(&REQUIREMENTS),
            archived_entries(&REQUIREMENTS),
        ),
        (active_entries(&DEFECTS), archived_entries(&DEFECTS)),
    );
    let (dependents_deps, dependents) = kanzei_tools::tracker::dependents_map_with_states(&states);
    let state_path = kanzei_core::project_state_path(&root);
    let work_units = if state_path.is_file() {
        kanzei_core::SessionStore::open(&state_path)
            .map_err(|error| format!("读取 Work Unit 状态库失败: {error}"))?
            .list_work_units(None)
            .map_err(|error| format!("读取 Work Unit 投影失败: {error}"))?
    } else {
        Vec::new()
    };
    let mut work_units_by_requirement: BTreeMap<String, Vec<kanzei_core::WorkProjection>> =
        BTreeMap::new();
    for unit in &work_units {
        work_units_by_requirement
            .entry(unit.requirement_id.clone())
            .or_default()
            .push(unit.clone());
    }
    let load =
        |kind: &'static kanzei_tools::docstore::DocKind| -> Result<Vec<serde_json::Value>, String> {
            let entries = active_entries(kind);
            let scheduled: Vec<(kanzei_tools::docstore::Entry, Vec<String>)> =
                if kind.rel_path == REQUIREMENTS.rel_path || kind.rel_path == DEFECTS.rel_path {
                    kanzei_tools::tracker::schedule_for_display_with_states(entries, &states)
                        .into_iter()
                        .map(|item| (item.entry, item.block_reasons))
                        .collect()
                } else {
                    entries
                        .iter()
                        .cloned()
                        .map(|entry| (entry, Vec::new()))
                        .collect()
                };
            Ok(scheduled.into_iter().map(|(e, block_reasons)| {
                // 提交标题是批次完成时产生的真源；字段只保留为 Git 不可用时的回退与收口校验。
                let derived_done = derived_batch_done
                    .as_ref()
                    .and_then(|counts| counts.get(&e.id))
                    .copied();
                let (batch_done, batch_total) =
                    kanzei_tools::docstore::batch_progress_with_derived_done(&e, derived_done);
                // R-247:backlog 的「被取得」直接读 tracker 字段。None 对 doing/fixing
                // 的含义由 D-354 定义为默认线持有；前端不得再解析 prompt 猜条目。
                let claimed_by = e
                    .fields
                    .iter()
                    .find(|(key, _)| key == "取得线")
                    .map(|(_, value)| value.clone());
                let execution_model = e
                    .fields
                    .iter()
                    .find(|(key, _)| key == "执行模型" || key.eq_ignore_ascii_case("execution_model"))
                    .map(|(_, value)| value.clone());
                let prior_art = if kind.prefix == "R" {
                    kanzei_tools::prior_art::entry_status(&root, &e)
                } else {
                    None
                };
                json!({
                    "id": e.id, "title": e.title, "status": e.status, "severity": e.severity,
                    "priority": e.fields.iter().find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority")).map(|(_, value)| value),
                    "complexity": e.fields.iter().find(|(key, _)| key == "复杂度" || key.eq_ignore_ascii_case("complexity")).map(|(_, value)| value),
                    "batches": { "done": batch_done, "total": batch_total },
                    "closed": kind.terminal.contains(&e.status.as_str()), "blocked": !block_reasons.is_empty(),
                    "block_reasons": block_reasons, "claimed_by": claimed_by, "fields": e.fields,
                    "execution_model": execution_model,
                    "prior_art": prior_art,
                    "requirement": (kind.prefix == "R").then(|| kanzei_tools::docstore::requirement::requirement_view(&e)),
                    "work_units": work_units_by_requirement.get(&e.id).cloned().unwrap_or_default(),
                    "dependencies": dependents_deps.get(&e.id).cloned().unwrap_or_default(),
                    "dependents": dependents.get(&e.id).cloned().unwrap_or_default(),
                    "nextStatuses": kind.statuses.iter().filter(|s| **s != e.status && DocStore::open(&root, kind).transition_allowed(&e.status, s).is_ok()).collect::<Vec<_>>(),
                })
            }).collect())
        };
    let conventions_path = root.join(CONVENTIONS_REL);
    let mut conventions = match std::fs::read_to_string(&conventions_path) {
        Ok(text) => {
            json!({ "exists": true, "headings": text.lines().filter(|l| l.starts_with('#')).map(|l| l.trim_start_matches('#').trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>() })
        }
        Err(_) => json!({ "exists": false, "headings": [] }),
    };
    conventions["has_proposal"] = json!(root
        .join(kanzei_tools::conventions::drafts::PROPOSAL_REL)
        .is_file());
    Ok(json!({
        "conventions": conventions, "root": root.display().to_string(),
        // warnings 是新增字段:前端忽略未知键,所以不需要改 .js。它承载"读成功了,
        // 但顺手做的那次写没做成"这种半程状态——以前这类信息被 `let _ =` 吃掉。
        "warnings": warnings,
        "incident_metrics": kanzei_tools::incident::metrics(&root),
        "work_units": work_units,
        "requirements": load(&REQUIREMENTS)?, "defects": load(&DEFECTS)?, "ideas": load(&IDEAS)?,
        "sources": load(&SOURCES)?, "findings": load(&FINDINGS)?,
        "research_topics": research_topics(&root)?,
        "archived": { "req": archived_entries(&REQUIREMENTS).len(), "defect": archived_entries(&DEFECTS).len(), "idea": archived_entries(&IDEAS).len(), "source": archived_entries(&SOURCES).len(), "finding": archived_entries(&FINDINGS).len() },
    }))
}

/// R-277：研究工作台读取并审批计划；审批只接受 agent 已请求审批的状态。
#[tauri::command]
pub fn research_plan_get(project_dir: String, topic: String) -> Result<serde_json::Value, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let workflow = kanzei_tools::research_control::workflow(&root, &topic)?;
    match kanzei_tools::research_plan::load_plan(&root, &topic)? {
        Some(mut plan) => {
            if let Some(workflow) = &workflow {
                plan.budget = workflow.budget.clone();
            }
            Ok(
                json!({ "exists": true, "plan": plan, "controller": if workflow.is_some() { "workflow" } else { "plan" } }),
            )
        }
        None => Ok(json!({ "exists": false, "topic": topic })),
    }
}

#[tauri::command]
pub fn research_plan_approve(
    project_dir: String,
    topic: String,
) -> Result<serde_json::Value, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    if kanzei_tools::research_control::workflow(&root, &topic)?.is_some() {
        return Err("计划由 AUTO 主流程管理，请从研究流程恢复或选题".into());
    }
    let plan = kanzei_tools::research_plan::approve_plan(&root, &topic)?;
    Ok(json!({ "exists": true, "plan": plan }))
}

/// D-296:归档只在用户展开历史入口时通过此命令加载,普通快照不把历史正文塞进 IPC。
#[tauri::command]
pub fn docs_archive_entries(
    project_dir: String,
    kind: String,
) -> Result<Vec<serde_json::Value>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let doc_kind = match kind.as_str() {
        "req" => &REQUIREMENTS,
        "defect" => &DEFECTS,
        "idea" => &IDEAS,
        "source" => &SOURCES,
        "finding" => &FINDINGS,
        other => return Err(format!("未知归档类型:{other}")),
    };
    DocStore::open(&root, doc_kind)
        .load_archive()
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| {
                    json!({
                        "id": entry.id, "title": entry.title, "status": entry.status,
                        "severity": entry.severity, "fields": entry.fields, "closed": true,
                        "requirement": (doc_kind.prefix == "R").then(|| kanzei_tools::docstore::requirement::requirement_view(&entry)),
                    })
                })
                .collect()
        })
        .map_err(|e| format!("读取 {} 失败: {e}", doc_kind.rel_path))
}

#[allow(clippy::too_many_arguments)] // Tauri command 参数名是前端 IPC 契约，不能合并为不兼容对象。
#[tauri::command]
pub async fn docs_update(
    project_dir: String,
    kind: String,
    action: String,
    id: String,
    status: Option<String>,
    title: Option<String>,
    priority: Option<String>,
    fields: Option<serde_json::Value>,
    requirement: Option<serde_json::Value>,
    evidence: Option<serde_json::Value>,
    order: Option<Vec<String>>,
    topic: Option<String>,
    reason: Option<String>,
) -> Result<String, String> {
    use kanzei_tools::tracker::TrackerTool;
    let tool = match kind.as_str() {
        "req" => TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        },
        "defect" => TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        },
        "source" => TrackerTool {
            tool_name: "source",
            noun: "source",
            kind: &SOURCES,
            requires_refs: None,
        },
        "finding" => TrackerTool {
            tool_name: "finding",
            noun: "finding",
            kind: &FINDINGS,
            requires_refs: Some(&SOURCES),
        },
        "idea" => TrackerTool {
            tool_name: "idea",
            noun: "idea",
            kind: &IDEAS,
            requires_refs: None,
        },
        other => return Err(format!("unknown kind `{other}`")),
    };
    let mut input = json!({ "action": action, "id": id });
    if let Some(order) = order.filter(|o| !o.is_empty()) {
        input["order"] = json!(order);
    }
    if let Some(status) = status {
        input["status"] = json!(status);
    }
    if let Some(title) = title.filter(|t| !t.trim().is_empty()) {
        input["title"] = json!(title);
    }
    if let Some(priority) = priority.filter(|p| !p.trim().is_empty()) {
        input["priority"] = json!(priority);
    }
    if let Some(fields) = fields.filter(|f| f.is_object()) {
        input["fields"] = fields;
    }
    if let Some(spec) = requirement {
        input["requirement"] = spec;
    }
    if let Some(items) = evidence {
        input["evidence"] = items;
    }
    if let Some(topic) = topic.filter(|topic| !topic.trim().is_empty()) {
        input["topic"] = json!(topic);
    }
    if let Some(reason) = reason.filter(|reason| !reason.trim().is_empty()) {
        input["reason"] = json!(reason);
    }
    // R-141:Tauri command 入口,发现式取根合法且只做这一次。
    let ctx = kanzei_harness::ToolCtx::discovering(PathBuf::from(&project_dir));
    let output = tool.execute_user_action(input, &ctx).await;
    if output.is_error {
        Err(output.content)
    } else {
        Ok(output.content)
    }
}

#[cfg(test)]
mod user_acceptance_tests {
    use super::*;

    #[tokio::test]
    async fn desktop_user_completion_preserves_large_requirement_and_archives_it() {
        let root = std::env::temp_dir().join(format!(
            "kz-user-acceptance-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = DocStore::open(&root, &REQUIREMENTS);
        store
            .save(&[kanzei_tools::docstore::Entry {
                id: "R-001".into(),
                title: "移动端 Markdown".into(),
                status: "doing".into(),
                severity: None,
                fields: vec![
                    ("复杂度".into(), "大".into()),
                    ("验收".into(), "①全部内容可检索；②全部附件可打开".into()),
                    ("批次".into(), "0/3".into()),
                ],
            }])
            .unwrap();
        let project = root.to_string_lossy().into_owned();
        let result = docs_update(
            project.clone(),
            "req".into(),
            "close".into(),
            "R-001".into(),
            Some("done".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");
        let completed = store.load().unwrap();
        assert_eq!(completed[0].status, "done");
        assert!(completed[0]
            .fields
            .iter()
            .any(|(key, value)| key == "用户验收" && value.contains("用户手动确认")));
        docs_snapshot(project).unwrap();
        assert!(store.load().unwrap().is_empty());
        let archived = store.load_archive().unwrap();
        assert_eq!(archived.len(), 1);
        assert!(archived[0]
            .fields
            .iter()
            .any(|(key, value)| key == "验收" && value.contains("全部附件")));
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn arxiv_id_from_url(url: &str) -> Result<String, String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.contains("://arxiv.org/") || lower.contains("://export.arxiv.org/")) {
        return Err(format!("不是受支持的 arXiv URL: {url}"));
    }
    let path = url.split('?').next().unwrap_or(url);
    let id = ["/abs/", "/html/", "/pdf/"]
        .iter()
        .find_map(|marker| path.split_once(marker).map(|(_, rest)| rest))
        .unwrap_or("")
        .trim_matches('/')
        .trim_end_matches(".pdf")
        .to_string();
    if id.is_empty()
        || id.contains("..")
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '/' | '_'))
    {
        return Err(format!("无法从 arXiv URL 提取安全文献 ID: {url}"));
    }
    Ok(id)
}

fn research_fulltext_path(
    root: &Path,
    topic: &str,
    id: &str,
    extension: &str,
) -> Result<PathBuf, String> {
    kanzei_tools::docstore::DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    let safe_id = id.replace(['/', '\\'], "_");
    let dir = root.join(".kanzei/research").join(topic).join("fulltext");
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建正文目录失败: {error}"))?;
    Ok(dir.join(format!("{safe_id}.{extension}")))
}

/// 批5：arXiv 正文通道。HTML→ar5iv→PDF，成功结果落入 topic/fulltext 并返回正文级证据。
#[tauri::command]
pub async fn research_arxiv_preview(
    project_dir: String,
    topic: String,
    url: String,
) -> Result<serde_json::Value, String> {
    let id = arxiv_id_from_url(&url)?;
    let root = normalized_project_root(Path::new(&project_dir));
    let ctx = kanzei_harness::ToolCtx::default();
    let candidates = [
        (format!("https://arxiv.org/html/{id}"), "html"),
        (format!("https://ar5iv.labs.arxiv.org/html/{id}"), "html"),
        (format!("https://arxiv.org/pdf/{id}.pdf"), "pdf"),
    ];
    let mut failures = Vec::new();
    for (candidate, kind) in candidates {
        let fetched =
            match kanzei_tools::webfetch::fetch_bytes(&candidate, &ctx, 16 * 1024 * 1024).await {
                Ok(value) => value,
                Err(error) => {
                    failures.push(format!("{candidate}: {error}"));
                    continue;
                }
            };
        if !(200..300).contains(&fetched.status) || fetched.body.is_empty() {
            failures.push(format!("{candidate}: HTTP {}", fetched.status));
            continue;
        }
        if kind == "html" {
            let raw = String::from_utf8_lossy(&fetched.body).into_owned();
            let text = kanzei_tools::webfetch::html_to_text(&raw);
            if text.trim().len() < 200 {
                failures.push(format!("{candidate}: 正文为空或过短"));
                continue;
            }
            let path = research_fulltext_path(&root, &topic, &id, "html")?;
            std::fs::write(&path, raw).map_err(|error| format!("保存 arXiv HTML 失败: {error}"))?;
            return Ok(json!({
                "title": format!("arXiv {id}"),
                "text": text,
                "depth": "正文级",
                "source_url": candidate,
                "path": path.display().to_string(),
                "fallback": failures,
            }));
        }
        if fetched.body.starts_with(b"%PDF-") {
            let path = research_fulltext_path(&root, &topic, &id, "pdf")?;
            std::fs::write(&path, &fetched.body)
                .map_err(|error| format!("保存 arXiv PDF 失败: {error}"))?;
            let text = kanzei_tools::pdf_to_text(&path)?;
            return Ok(json!({
                "title": format!("arXiv {id}"),
                "text": text,
                "depth": "正文级",
                "source_url": candidate,
                "path": path.display().to_string(),
                "fallback": failures,
            }));
        }
        failures.push(format!("{candidate}: 响应不是 PDF"));
    }
    Err(format!("arXiv 正文获取失败: {}", failures.join("; ")))
}

/// D-413:研究工作台「点开参考文献」的后端。抓取与 HTML→文本复用 webfetch 工具
/// 本体(同一套代理/超时/截断口径),不另造第二条抓取路径;返回纯文本给内置 viewer
/// 渲染——用户 2026-08-16 定调「在应用内打开,不跳出去」。
///
/// 只接受 http/https:研究来源的 URL 就是这两种;放开 file:// 等 scheme 等于给
/// 前端开一条读任意本地文件的旁路(代码域来源走的是 file_preview,不经这里)。
#[tauri::command]
pub async fn webfetch_preview(url: String) -> Result<serde_json::Value, String> {
    use kanzei_harness::Tool as _;
    let trimmed = url.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(format!("只支持 http/https 链接:{trimmed}"));
    }
    let ctx = kanzei_harness::ToolCtx::default();
    let output = kanzei_tools::webfetch::WebFetchTool
        .execute(json!({ "url": trimmed }), &ctx)
        .await;
    if output.is_error {
        return Err(output.content);
    }
    // webfetch 输出形如 `HTTP 200 · <url>\n\n<正文>`:标题行留给 viewer 标题,
    // 正文进 markdown 渲染面。切不出来时整体当正文,不假装解析成功。
    let (head, body) = output
        .content
        .split_once("\n\n")
        .unwrap_or(("", output.content.as_str()));
    Ok(json!({
        "title": if head.is_empty() { trimmed } else { head },
        "text": body,
    }))
}

fn docs_path(project_dir: &str, kind: &str, topic: Option<&str>) -> Result<PathBuf, String> {
    let root = normalized_project_root(Path::new(project_dir));
    let topic_path = |doc_kind: &'static kanzei_tools::docstore::DocKind| {
        topic
            .map(|topic| {
                kanzei_tools::docstore::DocStore::open_topic(&root, doc_kind, topic)
                    .map(|store| store.path)
                    .map_err(|error| error.to_string())
            })
            .transpose()
    };
    let path = match kind {
        "req" => root.join(REQUIREMENTS.rel_path),
        // 记忆图谱点决策节点(A-*)打开决策文档。
        "decision" => root.join(DECISIONS.rel_path),
        "defect" => root.join(DEFECTS.rel_path),
        "idea" => root.join(IDEAS.rel_path),
        "conventions" => root.join(CONVENTIONS_REL),
        "architecture" => root.join(".kanzei/project/architecture/README.md"),
        "req-archive" => DocStore::open(&root, &REQUIREMENTS).archive_file(),
        "defect-archive" => DocStore::open(&root, &DEFECTS).archive_file(),
        "idea-archive" => DocStore::open(&root, &IDEAS).archive_file(),
        "source" => topic_path(&SOURCES)?.unwrap_or_else(|| root.join(SOURCES.rel_path)),
        "finding" => topic_path(&FINDINGS)?.unwrap_or_else(|| root.join(FINDINGS.rel_path)),
        "report" => topic
            .map(|topic| {
                kanzei_tools::docstore::DocStore::validate_topic(topic)
                    .map_err(|error| error.to_string())?;
                Ok::<PathBuf, String>(root.join(".kanzei/research").join(topic).join("report.md"))
            })
            .transpose()?
            .unwrap_or_else(|| root.join(".kanzei/research/report.md")),
        "source-archive" => DocStore::open(&root, &SOURCES).archive_file(),
        "finding-archive" => DocStore::open(&root, &FINDINGS).archive_file(),
        other => return Err(format!("unknown kind `{other}`")),
    };
    if !path.is_file() {
        return Err(format!("文档还不存在:{}", path.display()));
    }
    Ok(path)
}

#[tauri::command]
pub fn docs_open(project_dir: String, kind: String, topic: Option<String>) -> Result<(), String> {
    let path = docs_path(&project_dir, &kind, topic.as_deref())?;
    hidden_command("cmd")
        .args(["/c", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn docs_read(
    project_dir: String,
    kind: String,
    topic: Option<String>,
) -> Result<serde_json::Value, String> {
    let path = docs_path(&project_dir, &kind, topic.as_deref())?;
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))?;
    Ok(json!({
        "path": path.display().to_string(),
        "name": path.file_name().and_then(|n| n.to_str()).unwrap_or(&kind),
        "content": content,
        "topic": topic,
    }))
}

/// 读取项目内任意相对路径的 Markdown(R-122 架构浏览用):只读 docs/ 前缀文件,
/// 防止把命令变成任意文件读取通道。返回与 docs_read 同构。
#[tauri::command]
pub fn docs_read_custom(
    project_dir: String,
    rel_path: String,
) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let normalized = rel_path.replace('\\', "/");
    if !normalized.starts_with("docs/") {
        return Err(format!("只允许读取 docs/ 下的文件,收到 `{rel_path}`"));
    }
    let path = root.join(&normalized);
    if !path.is_file() {
        return Err(format!("文档不存在:{}", path.display()));
    }
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))?;
    Ok(json!({
        "path": path.display().to_string(),
        "name": path.file_name().and_then(|n| n.to_str()).unwrap_or("md"),
        "content": content,
    }))
}

/// 架构浏览快照(R-122):返回架构索引文本 + docs/design 文档目录清单
/// (文件名、标题、字节数),供前端渲染「索引 + 设计文档树」的架构浏览视图。
/// 只读;索引维护仍走 architecture 工具,本命令只做呈现数据源。
///
/// 架构索引 README 是可选的(新项目没建过):缺失或读不了都不整页失败——架构图与设计文档树
/// 不依赖它,照常返回;`index_exists` / `index_error` 让前端说明为什么没有按章节分组。
#[tauri::command(async)]
pub fn architecture_snapshot(project_dir: String) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let index_path = root.join(".kanzei/project/architecture/README.md");
    let (index, index_exists, index_error) = match std::fs::read_to_string(&index_path) {
        Ok(text) => (text, true, None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (String::new(), false, None),
        Err(error) => (
            String::new(),
            false,
            Some(format!("架构索引读取失败: {error}")),
        ),
    };
    let design_dir = root.join("docs/design");
    let mut docs: Vec<serde_json::Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&design_dir) {
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        for name in names {
            let path = design_dir.join(&name);
            let meta = path.metadata().ok();
            let title = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| {
                    text.lines()
                        .find(|l| l.starts_with('#'))
                        .map(|l| l.trim_start_matches('#').trim().to_string())
                })
                .unwrap_or_default();
            docs.push(json!({
                "name": name,
                "title": title,
                "bytes": meta.map(|m| m.len()).unwrap_or(0),
            }));
        }
    }
    Ok(json!({
        "index_path": index_path.display().to_string(),
        "index": index,
        "index_exists": index_exists,
        "index_error": index_error,
        "design_docs": docs,
        // UI2-0926 #7(docs/design/architecture_diagrams.md):docs/architecture/*.md 的手写图
        // (标题/说明/首个 mermaid 围栏/起始行号/lint 问题)与 crate 依赖图(每次从 Cargo 清单
        // 生成,直接依赖与全部依赖两份 mermaid 源码)。前端 19-arch.js 用 04-diagram.js 渲染。
        "diagrams": kanzei_tools::arch_diagram::scan_diagrams(&root),
        "crates": crates_snapshot(&root),
        // 兼容字段(R-188 的旧依赖边二元组):新前端不再读,保留一个版本后删除。
        "graph": build_workspace_graph(&root),
    }))
}

/// crate 依赖图快照:成员(包名/描述/分组/入口)、边(normal/dev/build + 是否可由传递得到)、
/// 两份 mermaid 源码。不是 Cargo 工作区时为 null(前端只显示手写图)。
fn crates_snapshot(root: &Path) -> serde_json::Value {
    use kanzei_tools::arch_diagram::{crates_mermaid, workspace_crates};
    let Some(ws) = workspace_crates(root) else {
        return serde_json::Value::Null;
    };
    json!({
        "members": ws.members,
        "edges": ws.edges,
        "mermaid": {
            "reduced": crates_mermaid(&ws, false),
            "full": crates_mermaid(&ws, true),
        },
        "hidden_transitive": ws.hidden_transitive(),
    })
}

/// R-188 验收①:从 workspace 真实数据源(Cargo.toml members + 各 crate 的内部依赖)
/// 抽取 crate 依赖边,供前端生成架构图。返回 (crate, 依赖) 二元组列表,边去重排序。
/// 解析不到任何 crate 时返回空(前端降级文字树)。
///
/// 记忆图谱起委托给 `kanzei_harness::areas::AreaRegistry`(区域注册表)——同一份
/// Cargo.toml 解析,仓里不再养第二份;返回形状不变。
pub(crate) fn build_workspace_graph(root: &std::path::Path) -> Vec<(String, String)> {
    kanzei_harness::areas::workspace_crate_deps(root)
}

#[cfg(test)]
mod architecture_snapshot_tests {
    use super::architecture_snapshot;

    /// UX-096:没有架构索引 README 的项目,快照不整体失败,设计文档树照常给出。
    #[test]
    fn 缺架构索引时快照不失败且设计文档树照常() {
        let root = std::env::temp_dir().join(format!(
            "kz-arch-snapshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("docs/design")).unwrap();
        std::fs::write(root.join("docs/design/alpha.md"), "# Alpha 设计\n正文\n").unwrap();

        let snap = architecture_snapshot(root.display().to_string()).unwrap();
        assert_eq!(snap["index"], "");
        assert_eq!(snap["index_exists"], false);
        assert!(
            snap["index_error"].is_null(),
            "文件不存在是常态,不算读取错误"
        );
        assert_eq!(snap["design_docs"][0]["name"], "alpha.md");
        assert_eq!(snap["design_docs"][0]["title"], "Alpha 设计");

        std::fs::create_dir_all(root.join(".kanzei/project/architecture")).unwrap();
        std::fs::write(
            root.join(".kanzei/project/architecture/README.md"),
            "## live_design\n",
        )
        .unwrap();
        let snap = architecture_snapshot(root.display().to_string()).unwrap();
        assert_eq!(snap["index_exists"], true);
        assert_eq!(snap["index"], "## live_design\n");
        std::fs::remove_dir_all(root).ok();
    }
}

#[cfg(test)]
mod research_arxiv_tests {
    use super::arxiv_id_from_url;

    #[test]
    fn arxiv_id_normalizes_supported_forms_and_rejects_other_hosts() {
        assert_eq!(
            arxiv_id_from_url("https://arxiv.org/abs/2301.12345v2").unwrap(),
            "2301.12345v2"
        );
        assert_eq!(
            arxiv_id_from_url("https://export.arxiv.org/pdf/cond-mat/0301234.pdf").unwrap(),
            "cond-mat/0301234"
        );
        assert!(arxiv_id_from_url("https://example.com/abs/2301.12345").is_err());
        assert!(arxiv_id_from_url("https://arxiv.org/abs/../../secret").is_err());
    }
}

#[cfg(test)]
mod test_refs_lease_tests {
    use super::writer_lease_if_idle;
    use kanzei_core::orchestration::MemoryCoordinator;
    use kanzei_harness::orchestration::{ProjectExecutionCoordinator, WriterLeaseRequest};
    use std::time::Duration;

    fn request(run_id: &str, root: &std::path::Path) -> WriterLeaseRequest {
        WriterLeaseRequest {
            write_scope: root.to_path_buf(),
            run_id: run_id.into(),
            process_id: format!("proc-{run_id}"),
            reason: "test".into(),
        }
    }

    #[tokio::test]
    async fn 写权空闲时回填拿到租约() {
        let dir = std::env::temp_dir().join(format!("kz-docs-lease-idle-{}", std::process::id()));
        let coord = MemoryCoordinator::new();
        let lease = writer_lease_if_idle(&coord, request("init", &dir), Duration::from_millis(50))
            .await
            .unwrap();
        assert!(lease.is_some());
        assert_eq!(coord.snapshot(&dir).writer_run_id.as_deref(), Some("init"));
    }

    /// UX-014:agent 持有写租约时,回填不排队等,直接跳过,且不在队列里留登记。
    #[tokio::test]
    async fn 写权被占时回填直接跳过且不排队() {
        let dir = std::env::temp_dir().join(format!("kz-docs-lease-busy-{}", std::process::id()));
        let coord = MemoryCoordinator::new();
        let _held = coord
            .acquire_writer_lease(request("agent-run", &dir))
            .await
            .unwrap();
        let skipped =
            writer_lease_if_idle(&coord, request("init", &dir), Duration::from_millis(50))
                .await
                .unwrap();
        assert!(skipped.is_none());
        let snap = coord.snapshot(&dir);
        assert_eq!(snap.writer_run_id.as_deref(), Some("agent-run"));
        assert!(snap.waiting_writers.is_empty());
    }
}
