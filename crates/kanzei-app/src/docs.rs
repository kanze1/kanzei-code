//! Project document and tracker commands.

use std::path::{Path, PathBuf};

use serde_json::json;
use tauri::State;

use kanzei_tools::docstore::{DocStore, DEFECTS, FINDINGS, GOALS, REQUIREMENTS, SOURCES};

pub(crate) const CONVENTIONS_REL: &str = ".kanzei/project/conventions.md";

use crate::{normalized_project_root, state::hidden_command};

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
        "# 开发规范\n\n## 代码风格\n- \n\n## 提交规范\n- \n\n## 测试要求\n- \n\n## 禁止事项\n- \n",
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
pub fn test_run_record(
    project_dir: String,
    title: String,
    status: String,
    command: Option<String>,
    summary: Option<String>,
) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    kanzei_tools::test_record::append_test_run(
        &root,
        &title,
        &status,
        command.as_deref(),
        summary.as_deref(),
    )
}

#[tauri::command]
pub fn docs_snapshot(project_dir: String) -> serde_json::Value {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    for kind in [&REQUIREMENTS, &DEFECTS, &GOALS] {
        let _ = DocStore::open(&root, kind).archive_terminal();
    }
    // 一次快照只读取一次提交历史。按条目多次起 Git 会把“即时刷新”反过来变成卡顿源。
    let batch_ids = [&REQUIREMENTS, &DEFECTS]
        .into_iter()
        .flat_map(|kind| DocStore::open(&root, kind).load().unwrap_or_default())
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let derived_batch_done =
        kanzei_tools::git_batches::completed_batches_for_entries(&root, batch_ids).ok();
    let archived = |kind: &'static kanzei_tools::docstore::DocKind| -> usize {
        DocStore::open(&root, kind)
            .load_archive()
            .map_or(0, |a| a.len())
    };
    let archived_entries =
        |kind: &'static kanzei_tools::docstore::DocKind| -> Vec<serde_json::Value> {
            DocStore::open(&root, kind)
                .load_archive()
                .unwrap_or_default()
                .into_iter()
                .map(|e| {
                    json!({
                        "id": e.id, "title": e.title, "status": e.status, "severity": e.severity,
                        "fields": e.fields, "closed": true,
                    })
                })
                .collect()
        };
    let load = |kind: &'static kanzei_tools::docstore::DocKind| -> Vec<serde_json::Value> {
        let store = DocStore::open(&root, kind);
        let entries = store.load().unwrap_or_default();
        // 反向依赖图(R-111):跨 req/defect 构建「谁依赖我」。放在快照入口算一次,
        // 两个 kind 共用,避免每个 kind 重复读盘。仅对 req/defect 输出(其它 kind 无依赖语义)。
        let (dependents_deps, dependents) = if kind.rel_path == REQUIREMENTS.rel_path
            || kind.rel_path == DEFECTS.rel_path
        {
            kanzei_tools::tracker::dependents_map(
                &kanzei_harness::ToolCtx::new(root.clone()),
                kind,
                &entries,
            )
            .unwrap_or_default()
        } else {
            Default::default()
        };
        let scheduled: Vec<(kanzei_tools::docstore::Entry, Vec<String>)> =
            if kind.rel_path == REQUIREMENTS.rel_path || kind.rel_path == DEFECTS.rel_path {
                kanzei_tools::tracker::schedule_for_display(
                    &kanzei_harness::ToolCtx::new(root.clone()),
                    kind,
                    &entries,
                )
                .map(|items| {
                    items
                        .into_iter()
                        .map(|item| (item.entry, item.block_reasons))
                        .collect()
                })
                .unwrap_or_else(|_| {
                    entries
                        .iter()
                        .cloned()
                        .map(|entry| (entry, Vec::new()))
                        .collect()
                })
            } else {
                entries
                    .iter()
                    .cloned()
                    .map(|entry| (entry, Vec::new()))
                    .collect()
            };
        scheduled.into_iter().map(|(e, block_reasons)| {
        // 提交标题是批次完成时产生的真源；字段只保留为 Git 不可用时的回退与收口校验。
        let derived_done = derived_batch_done
            .as_ref()
            .and_then(|counts| counts.get(&e.id))
            .copied();
        let (batch_done, batch_total) =
            kanzei_tools::docstore::batch_progress_with_derived_done(&e, derived_done);
        json!({
            "id": e.id, "title": e.title, "status": e.status, "severity": e.severity,
            "priority": e.fields.iter().find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority")).map(|(_, value)| value),
            "complexity": e.fields.iter().find(|(key, _)| key == "复杂度" || key.eq_ignore_ascii_case("complexity")).map(|(_, value)| value),
            // 批次进度:格数与已填格由后端算,前端只渲染；Git 可用时已填格来自提交历史。
            "batches": { "done": batch_done, "total": batch_total },
            "closed": kind.terminal.contains(&e.status.as_str()), "blocked": !block_reasons.is_empty(),
            "block_reasons": block_reasons, "fields": e.fields,
            // 正向依赖(dependencies)与反向链接(dependents,R-111):均来自「依赖:」字段解析。
            "dependencies": dependents_deps.get(&e.id).cloned().unwrap_or_default(),
            "dependents": dependents.get(&e.id).cloned().unwrap_or_default(),
            "nextStatuses": kind.statuses.iter().filter(|s| **s != e.status && DocStore::open(&root, kind).transition_allowed(&e.status, s).is_ok()).collect::<Vec<_>>(),
        })}).collect()
    };
    let conventions_path = root.join(CONVENTIONS_REL);
    let conventions = match std::fs::read_to_string(&conventions_path) {
        Ok(text) => {
            json!({ "exists": true, "headings": text.lines().filter(|l| l.starts_with('#')).map(|l| l.trim_start_matches('#').trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>() })
        }
        Err(_) => json!({ "exists": false, "headings": [] }),
    };
    json!({
        "conventions": conventions, "root": root.display().to_string(),
        "requirements": load(&REQUIREMENTS), "defects": load(&DEFECTS), "goals": load(&GOALS),
        "sources": load(&SOURCES), "findings": load(&FINDINGS),
        "archived": { "req": archived(&REQUIREMENTS), "defect": archived(&DEFECTS), "goal": archived(&GOALS), "source": archived(&SOURCES), "finding": archived(&FINDINGS) },
        "archived_entries": { "req": archived_entries(&REQUIREMENTS), "defect": archived_entries(&DEFECTS), "goal": archived_entries(&GOALS), "source": archived_entries(&SOURCES), "finding": archived_entries(&FINDINGS) },
    })
}

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
    let ctx = kanzei_harness::ToolCtx::new(PathBuf::from(&project_dir));
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
pub fn docs_read_custom(project_dir: String, rel_path: String) -> Result<serde_json::Value, String> {
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
    let index = std::fs::read_to_string(&index_path)
        .map_err(|e| format!("架构索引读取失败: {e}"))?;
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
    }))
}
