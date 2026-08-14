//! Project document and tracker commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kanzei_tools::docstore::{DocStore, DEFECTS, FINDINGS, GOALS, REQUIREMENTS, SOURCES};
use serde_json::json;

pub(crate) const CONVENTIONS_REL: &str = ".kanzei/project/conventions.md";

use crate::{normalized_project_root, state::hidden_command};
use kanzei_harness::orchestration::ProjectExecutionCoordinator;

/// git 概览:分支 + 未提交改动数(状态栏显示)。
#[tauri::command]
pub async fn git_status(project_dir: String) -> Result<serde_json::Value, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    tokio::task::spawn_blocking(move || {
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
        let changes = run(&["status", "--porcelain"])
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);
        let last = run(&["log", "-1", "--format=%h %s"]);
        json!({ "branch": branch, "changes": changes, "last": last })
    })
    .await
    .map_err(|e| e.to_string())
}

/// 开发规范模板(不存在时一键创建;用户手写维护,agent 只读注入)。
#[tauri::command]
pub fn conventions_init(project_dir: String) -> Result<String, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let path = root.join(CONVENTIONS_REL);
    if path.is_file() {
        return Ok(path.display().to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &path,
        "# 开发规范(项目特有部分)\n\n\
         本文件只放**项目特有**规则(分支策略、架构契约、构建发布等)。\n\
         **通用**开发规则——取活与阻塞口径、关闭边界、验收证据、标签与依赖字段、\n\
         批次与验证节奏、代码修改原则、命名风格、测试与文档纪律、任务级并行——\n\
         由 kanzei 引擎内置注入(R-191,单源 kanzei-harness),所有项目默认一致,\n\
         不要抄到这里:在项目文件里复制通用规则只会漂移。\n\n\
         ## 分支与提交流程\n\
         - \n\n\
         ## 架构与契约\n\
         - \n\n\
         ## 构建与发布\n\
         - \n\n\
         ## 测试要求(项目特有补充)\n\
         - \n\n\
         ## 禁止事项(项目特有补充)\n\
         - \n",
    )
    .map_err(|e| e.to_string())?;
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

/// R-130:批量初始化测试→条目映射。扫描 tests.md 旧记录,从标题回填「关联」字段。
/// 与 test_run_record 同为 tests.md 写入口,接入项目级写仲裁(R-171 批4 模式),
/// 不能绕过协调器直接写文件(D-227 并发覆盖的机械门禁)。
#[tauri::command]
pub async fn test_runs_init_refs(
    state: tauri::State<'_, crate::AppState>,
    project_dir: String,
) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let _lease = state
        .coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            write_scope: root.clone(),
            run_id: format!("test_init_refs_{}", crate::run::now_ms()),
            process_id: "test_record".into(),
            reason: "test refs backfill".into(),
        })
        .await
        .map_err(|e| format!("无法获取项目写租约: {e}"))?;
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
#[tauri::command]
pub fn docs_snapshot(project_dir: String) -> Result<serde_json::Value, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    // 终态条目顺手归档:幂等、且只在"真有条目刚进终态"时才写盘。它与本次快照的
    // 读、以及 agent 那边的 tracker 写完全可能同时在飞(D-249 第④层),所以走
    // 限时锁——拿不到就跳过,下次刷新再归档,绝不让文档面板为了一次归档卡住。
    // 预算故意给得很短:UI 刷新的响应性优先级高于"这一轮就把归档做掉"。
    let mut warnings: Vec<String> = Vec::new();
    for kind in [&REQUIREMENTS, &DEFECTS, &GOALS] {
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
    let kinds = [&REQUIREMENTS, &DEFECTS, &GOALS, &SOURCES, &FINDINGS];
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
                json!({
                    "id": e.id, "title": e.title, "status": e.status, "severity": e.severity,
                    "priority": e.fields.iter().find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority")).map(|(_, value)| value),
                    "complexity": e.fields.iter().find(|(key, _)| key == "复杂度" || key.eq_ignore_ascii_case("complexity")).map(|(_, value)| value),
                    "batches": { "done": batch_done, "total": batch_total },
                    "closed": kind.terminal.contains(&e.status.as_str()), "blocked": !block_reasons.is_empty(),
                    "block_reasons": block_reasons, "claimed_by": claimed_by, "fields": e.fields,
                    "dependencies": dependents_deps.get(&e.id).cloned().unwrap_or_default(),
                    "dependents": dependents.get(&e.id).cloned().unwrap_or_default(),
                    "nextStatuses": kind.statuses.iter().filter(|s| **s != e.status && DocStore::open(&root, kind).transition_allowed(&e.status, s).is_ok()).collect::<Vec<_>>(),
                })
            }).collect())
        };
    let conventions_path = root.join(CONVENTIONS_REL);
    let conventions = match std::fs::read_to_string(&conventions_path) {
        Ok(text) => {
            json!({ "exists": true, "headings": text.lines().filter(|l| l.starts_with('#')).map(|l| l.trim_start_matches('#').trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>() })
        }
        Err(_) => json!({ "exists": false, "headings": [] }),
    };
    Ok(json!({
        "conventions": conventions, "root": root.display().to_string(),
        // warnings 是新增字段:前端忽略未知键,所以不需要改 .js。它承载"读成功了,
        // 但顺手做的那次写没做成"这种半程状态——以前这类信息被 `let _ =` 吃掉。
        "warnings": warnings,
        "requirements": load(&REQUIREMENTS)?, "defects": load(&DEFECTS)?, "goals": load(&GOALS)?,
        "sources": load(&SOURCES)?, "findings": load(&FINDINGS)?,
        "archived": { "req": archived_entries(&REQUIREMENTS).len(), "defect": archived_entries(&DEFECTS).len(), "goal": archived_entries(&GOALS).len(), "source": archived_entries(&SOURCES).len(), "finding": archived_entries(&FINDINGS).len() },
    }))
}

/// D-296:归档只在用户展开历史入口时通过此命令加载,普通快照不把历史正文塞进 IPC。
#[tauri::command]
pub fn docs_archive_entries(
    project_dir: String,
    kind: String,
) -> Result<Vec<serde_json::Value>, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let doc_kind = match kind.as_str() {
        "req" => &REQUIREMENTS,
        "defect" => &DEFECTS,
        "goal" => &GOALS,
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
    order: Option<Vec<String>>,
) -> Result<String, String> {
    use kanzei_harness::Tool as _;
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
        "goal" => TrackerTool {
            tool_name: "goal",
            noun: "goal",
            kind: &GOALS,
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
    // R-141:Tauri command 入口,发现式取根合法且只做这一次。
    let ctx = kanzei_harness::ToolCtx::discovering(PathBuf::from(&project_dir));
    let output = tool.execute(input, &ctx).await;
    if output.is_error {
        Err(output.content)
    } else {
        Ok(output.content)
    }
}

fn docs_path(project_dir: &str, kind: &str) -> Result<PathBuf, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| PathBuf::from(project_dir));
    let path = match kind {
        "req" => root.join(REQUIREMENTS.rel_path),
        "defect" => root.join(DEFECTS.rel_path),
        "goal" => root.join(GOALS.rel_path),
        "conventions" => root.join(CONVENTIONS_REL),
        "architecture" => root.join(".kanzei/project/architecture/README.md"),
        "req-archive" => DocStore::open(&root, &REQUIREMENTS).archive_file(),
        "defect-archive" => DocStore::open(&root, &DEFECTS).archive_file(),
        "goal-archive" => DocStore::open(&root, &GOALS).archive_file(),
        "source" => root.join(SOURCES.rel_path),
        "finding" => root.join(FINDINGS.rel_path),
        "report" => root.join(".kanzei/research/report.md"),
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
pub fn docs_open(project_dir: String, kind: String) -> Result<(), String> {
    let path = docs_path(&project_dir, &kind)?;
    hidden_command("cmd")
        .args(["/c", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn docs_read(project_dir: String, kind: String) -> Result<serde_json::Value, String> {
    let path = docs_path(&project_dir, &kind)?;
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))?;
    Ok(json!({
        "path": path.display().to_string(),
        "name": path.file_name().and_then(|n| n.to_str()).unwrap_or(&kind),
        "content": content,
    }))
}

/// 读取项目内任意相对路径的 Markdown(R-122 架构浏览用):只读 docs/ 前缀文件,
/// 防止把命令变成任意文件读取通道。返回与 docs_read 同构。
#[tauri::command]
pub fn docs_read_custom(
    project_dir: String,
    rel_path: String,
) -> Result<serde_json::Value, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
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
#[tauri::command]
pub fn architecture_snapshot(project_dir: String) -> Result<serde_json::Value, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let index_path = root.join(".kanzei/project/architecture/README.md");
    let index =
        std::fs::read_to_string(&index_path).map_err(|e| format!("架构索引读取失败: {e}"))?;
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
        "design_docs": docs,
        // R-188:代码生成的架构图数据——workspace crate 依赖边 + 设计文档节点。
        // 纯代码从 Cargo.toml 抽取,非文生图/预置图;前端据此渲染 mermaid。
        "graph": build_workspace_graph(&root),
    }))
}

/// R-188 验收①:从 workspace 真实数据源(Cargo.toml members + 各 crate 的
/// `kanzei-*` 依赖)抽取 crate 依赖边,供前端生成架构图。返回 (crate, 依赖)
/// 二元组列表,边去重排序。解析不到任何 crate 时返回空(前端降级文字树)。
pub(crate) fn build_workspace_graph(root: &std::path::Path) -> Vec<(String, String)> {
    let Ok(workspace_toml) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return Vec::new();
    };
    // members = ["crates/kanzei-harness", ...]
    let members: Vec<String> = workspace_toml
        .lines()
        .skip_while(|l| l.trim() != "[workspace]")
        .skip_while(|l| !l.contains("members"))
        .take_while(|l| l.trim() != "]")
        .filter_map(|l| {
            l.trim()
                .trim_matches(',')
                .trim_matches('"')
                .strip_prefix("crates/")
                .map(str::to_string)
        })
        .collect();
    let mut edges: Vec<(String, String)> = Vec::new();
    for member in &members {
        let path = root.join("crates").join(member).join("Cargo.toml");
        let Ok(toml) = std::fs::read_to_string(&path) else {
            continue;
        };
        // 抓 `kanzei-xxx.workspace = true` 与 `kanzei-xxx = { path = ... }`
        // 两种形态的 kanzei-* 依赖(内部 crate 依赖)。
        for line in toml.lines() {
            let trimmed = line.trim();
            if let Some(dep) = trimmed
                .strip_prefix("kanzei-")
                .and_then(|rest| rest.split(['.', '=', ' ']).next())
            {
                let dep_name = format!("kanzei-{dep}");
                if members.contains(&dep_name) && dep_name != *member {
                    edges.push((member.clone(), dep_name));
                }
            }
        }
    }
    edges.sort();
    edges.dedup();
    edges
}
