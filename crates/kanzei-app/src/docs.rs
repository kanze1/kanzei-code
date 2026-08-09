//! Project document and tracker commands.

use std::path::{Path, PathBuf};

use serde_json::json;
use tauri::State;

use kanzei_tools::docstore::{DocStore, DEFECTS, FINDINGS, GOALS, REQUIREMENTS, SOURCES};

use crate::{hidden_command, CONVENTIONS_REL};

#[tauri::command]
pub fn docs_snapshot(project_dir: String) -> serde_json::Value {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    for kind in [&REQUIREMENTS, &DEFECTS, &GOALS] {
        let _ = DocStore::open(&root, kind).archive_terminal();
    }
    let archived = |kind: &'static kanzei_tools::docstore::DocKind| -> usize {
        DocStore::open(&root, kind).load_archive().map_or(0, |a| a.len())
    };
    let archived_entries = |kind: &'static kanzei_tools::docstore::DocKind| -> Vec<serde_json::Value> {
        DocStore::open(&root, kind).load_archive().unwrap_or_default().into_iter().map(|e| json!({
            "id": e.id, "title": e.title, "status": e.status, "severity": e.severity,
            "fields": e.fields, "closed": true,
        })).collect()
    };
    let load = |kind: &'static kanzei_tools::docstore::DocKind| -> Vec<serde_json::Value> {
        let store = DocStore::open(&root, kind);
        let entries = store.load().unwrap_or_default();
        let scheduled: Vec<(kanzei_tools::docstore::Entry, Vec<String>)> =
            if kind.rel_path == REQUIREMENTS.rel_path || kind.rel_path == DEFECTS.rel_path {
                kanzei_tools::tracker::schedule_for_display(&kanzei_harness::ToolCtx::new(root.clone()), kind, &entries)
                    .map(|items| items.into_iter().map(|item| (item.entry, item.block_reasons)).collect())
                    .unwrap_or_else(|_| entries.iter().cloned().map(|entry| (entry, Vec::new())).collect())
            } else { entries.iter().cloned().map(|entry| (entry, Vec::new())).collect() };
        scheduled.into_iter().map(|(e, block_reasons)| json!({
            "id": e.id, "title": e.title, "status": e.status, "severity": e.severity,
            "priority": e.fields.iter().find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority")).map(|(_, value)| value),
            "complexity": e.fields.iter().find(|(key, _)| key == "复杂度" || key.eq_ignore_ascii_case("complexity")).map(|(_, value)| value),
            "closed": kind.terminal.contains(&e.status.as_str()), "blocked": !block_reasons.is_empty(),
            "block_reasons": block_reasons, "fields": e.fields,
            "nextStatuses": kind.statuses.iter().filter(|s| **s != e.status && DocStore::open(&root, kind).transition_allowed(&e.status, s).is_ok()).collect::<Vec<_>>(),
        })).collect()
    };
    let conventions_path = root.join(CONVENTIONS_REL);
    let conventions = match std::fs::read_to_string(&conventions_path) {
        Ok(text) => json!({ "exists": true, "headings": text.lines().filter(|l| l.starts_with('#')).map(|l| l.trim_start_matches('#').trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>() }),
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
        "req" => TrackerTool { tool_name: "req", noun: "requirement", kind: &REQUIREMENTS, requires_refs: None },
        "defect" => TrackerTool { tool_name: "defect", noun: "defect", kind: &DEFECTS, requires_refs: None },
        "source" => TrackerTool { tool_name: "source", noun: "source", kind: &SOURCES, requires_refs: None },
        "finding" => TrackerTool { tool_name: "finding", noun: "finding", kind: &FINDINGS, requires_refs: Some(&SOURCES) },
        "goal" => TrackerTool { tool_name: "goal", noun: "goal", kind: &GOALS, requires_refs: None },
        other => return Err(format!("unknown kind `{other}`")),
    };
    let mut input = json!({ "action": action, "id": id });
    if let Some(order) = order.filter(|o| !o.is_empty()) { input["order"] = json!(order); }
    if let Some(status) = status { input["status"] = json!(status); }
    if let Some(title) = title.filter(|t| !t.trim().is_empty()) { input["title"] = json!(title); }
    if let Some(priority) = priority.filter(|p| !p.trim().is_empty()) { input["priority"] = json!(priority); }
    if let Some(fields) = fields.filter(|f| f.is_object()) { input["fields"] = fields; }
    let ctx = kanzei_harness::ToolCtx::new(PathBuf::from(&project_dir));
    let output = tool.execute(input, &ctx).await;
    if output.is_error { Err(output.content) } else { Ok(output.content) }
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
    if !path.is_file() { return Err(format!("文档还不存在:{}", path.display())); }
    Ok(path)
}

#[tauri::command]
pub fn docs_open(project_dir: String, kind: String) -> Result<(), String> {
    let path = docs_path(&project_dir, &kind)?;
    hidden_command("cmd").args(["/c", "start", "", &path.display().to_string()]).spawn().map_err(|e| e.to_string())?;
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
