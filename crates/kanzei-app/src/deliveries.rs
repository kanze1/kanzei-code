//! File delivery is independent of work-unit completion and conversation surfaces.
use kanzei_core::{project_state_path, SessionStore};
use kanzei_harness::ToolCtx;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

fn shown(path: &Path) -> String {
    let raw = path.to_string_lossy();
    // The old live display converted a verbatim Windows path to //?/C:/... .
    // Normalize separators before simplifying; retain verbatim form for long paths.
    let native = if cfg!(windows) {
        raw.replace('/', "\\")
    } else {
        raw.into_owned()
    };
    let simple = kanzei_tools::path_form::simplify(Path::new(&native))
        .display()
        .to_string();
    if simple.starts_with(r"\\?\") {
        simple
    } else {
        simple.replace('\\', "/")
    }
}
fn modified_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

/// Batch evidence is deliberately separate from download receipts and work-unit acceptance.
#[tauri::command]
pub(crate) fn batch_evidence(project_dir: String) -> Result<Vec<Value>, String> {
    let root = Path::new(&project_dir)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let mut rows = batch_log_rows(&root)?;
    let db = project_state_path(&root);
    if db.exists() {
        let store = SessionStore::open_read_only(&db).map_err(|e| e.to_string())?;
        for event in store.batch_tool_events().map_err(|e| e.to_string())? {
            let input = &event.payload["input"];
            let mut row = json!({
                "id":event.event_id, "created_at":event.created_at,
                "run_id":event.payload["run_id"], "session_id":event.session_id,
                "entry_id":input["id"], "source":"tool_fact", "paths":[],
                "test_record_ids":[], "evidence_refs":[],
            });
            if event.payload["name"] == "work" {
                let (Some(scope), Some(target), Some(criterion)) = (
                    input["handoff_scope"].as_str(),
                    input["handoff_target"].as_str(),
                    input["criterion"].as_str(),
                ) else {
                    continue;
                };
                if target.trim().is_empty()
                    || criterion.trim().is_empty()
                    || input["evidence_refs"].as_array().is_none_or(Vec::is_empty)
                {
                    continue;
                }
                row["kind"] = json!("handoff");
                row["status"] = json!("declared");
                row["scope"] = json!(scope);
                row["target"] = json!(target);
                row["criterion"] = json!(criterion);
                row["summary"] = input["summary"].clone();
                row["evidence_refs"] = input["evidence_refs"].clone();
            } else {
                let content = event.payload["content"].as_str().unwrap_or("");
                let Some(commit) = committed_hash(content) else {
                    continue;
                };
                row["kind"] = json!("git_checkpoint");
                row["status"] = json!("recorded");
                row["commit"] = json!(commit);
                row["summary"] = input["message"].clone();
                row["evidence_refs"] = json!([commit]);
            }
            if !rows
                .iter()
                .any(|existing| same_batch_record(existing, &row))
            {
                rows.push(row);
            }
        }
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row["created_at"].as_u64().unwrap_or(0)));
    rows.truncate(200);
    Ok(rows)
}

fn batch_log_rows(root: &Path) -> Result<Vec<Value>, String> {
    use std::io::BufRead;
    let file = match std::fs::File::open(root.join(".kanzei/artifacts/work-log.jsonl")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(format!("读取批次证据失败: {error}")),
    };
    let mut rows = std::collections::VecDeque::new();
    for (line, raw) in std::io::BufReader::new(file).lines().enumerate() {
        let raw = raw.map_err(|error| format!("读取批次证据失败: {error}"))?;
        let Ok(record) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let mut row = json!({
            "id":format!("work-log:{}", line + 1), "source":"work_log",
            "source_ref":format!(".kanzei/artifacts/work-log.jsonl:{}", line + 1),
            "created_at":record["at_ms"], "run_id":record["run_id"],
            "entry_id":record["id"], "line":record["line"],
            "paths":[], "test_record_ids":[], "evidence_refs":[],
        });
        match record["event"].as_str() {
            Some("handoff")
                if record["criterion"]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty())
                    && record["evidence_refs"]
                        .as_array()
                        .is_some_and(|refs| !refs.is_empty()) =>
            {
                row["kind"] = json!("handoff");
                row["status"] = json!("declared");
                for key in ["scope", "target", "criterion", "summary", "evidence_refs"] {
                    row[key] = record[key].clone();
                }
            }
            Some("deliver")
                if record["source"] == "engine"
                    && record["commit"].as_str().is_some_and(is_commit_hash) =>
            {
                row["kind"] = json!("git_checkpoint");
                row["status"] = json!("recorded");
                row["commit"] = record["commit"].clone();
                for key in ["paths", "test_record_ids"] {
                    if record[key].is_array() {
                        row[key] = record[key].clone();
                    }
                }
                row["evidence_refs"] = json!([record["commit"]]);
            }
            _ => continue,
        }
        rows.push_back(row);
        if rows.len() > 200 {
            rows.pop_front();
        }
    }
    Ok(rows.into_iter().collect())
}

fn is_commit_hash(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn committed_hash(content: &str) -> Option<&str> {
    // GitTool's show output follows this success marker. Its preceding parenthesized
    // staged hash is an index fingerprint, not the resulting commit.
    let (_, after) = content.split_once("committed verified staged set (")?;
    after.lines().skip(1).find_map(|line| {
        let (hash, _) = line.split_once(' ')?;
        is_commit_hash(hash).then_some(hash)
    })
}

fn same_batch_record(a: &Value, b: &Value) -> bool {
    if a["kind"] != b["kind"] || a["run_id"] != b["run_id"] {
        return false;
    }
    if a["kind"] == "handoff" {
        // A later declaration may carry new evidence even with the same criterion.
        // Only collapse the ledger and typed-event projections of the same declaration.
        a["source"] != b["source"]
            && ["scope", "target", "criterion", "evidence_refs"]
                .iter()
                .all(|key| a[key] == b[key])
    } else {
        match (a["commit"].as_str(), b["commit"].as_str()) {
            (Some(a), Some(b)) => a.starts_with(b) || b.starts_with(a),
            _ => false,
        }
    }
}

pub(crate) fn record(
    ctx: &ToolCtx,
    mut display: Value,
    meta: &std::fs::Metadata,
) -> Result<Value, String> {
    display["path"] = json!(shown(Path::new(display["path"].as_str().unwrap_or(""))));
    let session = ctx
        .session_id
        .clone()
        .unwrap_or_else(|| kanzei_core::project_session_id(&ctx.project_root));
    let store =
        SessionStore::open(&project_state_path(&ctx.project_root)).map_err(|e| e.to_string())?;
    store
        .create_session(&session, &shown(&ctx.project_root), None)
        .map_err(|e| e.to_string())?;
    display["project_dir"] = json!(shown(&ctx.project_root));
    display["worktree_root"] = json!(shown(&ctx.cwd));
    display["session_id"] = json!(session);
    display["run_id"] = json!(ctx.run_id);
    display["modified_ms"] = json!(modified_ms(meta));
    let event = store
        .append_event(&session, "file.delivered", &display)
        .map_err(|e| e.to_string())?;
    display["id"] = json!(event.event_id);
    display["created_at"] = json!(event.created_at);
    display["status"] = json!("available");
    Ok(display)
}

#[tauri::command]
pub(crate) fn delivered_files(project_dir: String) -> Result<Vec<Value>, String> {
    let root = Path::new(&project_dir)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let db = project_state_path(&root);
    if !db.exists() {
        return Ok(vec![]);
    }
    let store = SessionStore::open_read_only(&db).map_err(|e| e.to_string())?;
    project_receipts(&root, &store)
}
fn project_receipts(root: &Path, store: &SessionStore) -> Result<Vec<Value>, String> {
    let events = store.delivery_events().map_err(|e| e.to_string())?;
    let mut managed = HashMap::new();
    for event in events
        .iter()
        .filter(|e| e.event_type == "file.delivery_managed")
    {
        if let Some(id) = event.payload["id"].as_str() {
            managed.entry(id.to_string()).or_insert(&event.payload);
        }
    }
    let processes = store
        .list_processes(&root.display().to_string())
        .map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut recorded = HashSet::new();
    // Prefer durable receipts with their original cwd and file version over legacy projections.
    for event in events.iter().filter(|e| e.event_type == "file.delivered") {
        let mut row = event.payload.clone();
        row["id"] = json!(event.event_id);
        row["created_at"] = json!(event.created_at);
        row["session_id"] = json!(event.session_id);
        recorded.insert((
            row["run_id"].as_str().unwrap_or("").to_string(),
            row["path"]
                .as_str()
                .unwrap_or("")
                .replace('\\', "/")
                .to_lowercase(),
        ));
        rows.push(row);
    }
    for event in events.iter().filter(|e| e.event_type.ends_with(".legacy")) {
        let input = &event.payload["input"];
        let Some(raw) = input["path"].as_str() else {
            continue;
        };
        let candidate = Path::new(raw);
        let line_id = event.session_id.split_once('#').map(|(_, id)| id);
        let process = line_id.and_then(|id| {
            processes
                .iter()
                .find(|p| p.process_id.split('|').next() == Some(id))
        });
        let legacy_root = process
            .map(|p| PathBuf::from(p.worktree_path.as_deref().unwrap_or(&p.project_dir)))
            .unwrap_or_else(|| root.to_path_buf());
        let path = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            legacy_root.join(candidate)
        };
        let path = shown(&path);
        let run = event.payload["run_id"].as_str().unwrap_or("");
        if recorded.contains(&(run.to_string(), path.to_lowercase())) {
            continue;
        }
        let content = event.payload["content"].as_str().unwrap_or("");
        let bytes = content
            .split(" bytes)")
            .next()
            .and_then(|s| s.rsplit_once('('))
            .and_then(|(_, n)| n.parse::<u64>().ok());
        rows.push(json!({"id":event.event_id,"kind":"file","path":path,
            "name":candidate.file_name().map(|s|s.to_string_lossy()),"bytes":bytes,"caption":input["caption"],
            "project_dir":shown(root),"worktree_root":shown(&legacy_root),"scope_known":line_id.is_none() || process.is_some(),"session_id":event.session_id,
            "created_at":event.created_at,"run_id":run,"legacy":true}));
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row["created_at"].as_i64().unwrap_or(0)));
    let mut unique = HashSet::new();
    rows.retain(|row| {
        unique.insert((
            row["session_id"].as_str().unwrap_or("").to_string(),
            row["path"].as_str().unwrap_or("").to_lowercase(),
        ))
    });
    for row in &mut rows {
        if let Some(saved) = row["id"].as_str().and_then(|id| managed.get(id)) {
            row["archived"] = saved["archived"].clone();
            row["removed"] = saved["removed"].clone();
            row["trashed_path"] = saved["trashed_path"].clone();
        }
        if row["scope_known"] == false {
            row["status"] = json!("unavailable");
            continue;
        }
        let scope = row["worktree_root"].as_str().unwrap_or("");
        let path = row["path"].as_str().unwrap_or("");
        let target = crate::commands::run::resolve_delivered_path(scope, path);
        let status = match target.and_then(|p| std::fs::metadata(p).map_err(|e| e.to_string())) {
            Ok(meta) => {
                let bytes_changed = row["bytes"].as_u64().is_some_and(|b| b != meta.len());
                let time_changed = row["modified_ms"]
                    .as_u64()
                    .is_some_and(|t| Some(t) != modified_ms(&meta))
                    || row["legacy"] == true
                        && modified_ms(&meta).is_some_and(|t| {
                            t > row["created_at"].as_u64().unwrap_or(0).saturating_add(1000)
                        });
                row["current_bytes"] = json!(meta.len());
                if bytes_changed || time_changed {
                    "changed"
                } else {
                    "available"
                }
            }
            Err(_) => "unavailable",
        };
        row["status"] = json!(status);
        if row["trashed_path"].as_str().is_some() {
            row["status"] = json!("trashed");
        }
    }
    Ok(rows)
}

#[tauri::command]
pub(crate) fn delivery_manage(
    project_dir: String,
    id: String,
    action: String,
) -> Result<Vec<Value>, String> {
    let root = Path::new(&project_dir)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let store = SessionStore::open(&project_state_path(&root)).map_err(|e| e.to_string())?;
    let rows = project_receipts(&root, &store)?;
    let row = rows
        .iter()
        .find(|row| row["id"] == id)
        .ok_or("交付记录已变化，请刷新")?;
    let session = row["session_id"].as_str().ok_or("交付记录没有所属对话")?;
    let mut saved = json!({"id":id, "archived":row["archived"] == true, "removed":row["removed"] == true, "trashed_path":row["trashed_path"]});
    // File operations use the durable receipt's scope and version, never a UI path.
    let _owner = if matches!(action.as_str(), "trash" | "restore_file") {
        Some(
            kanzei_core::store::session_execution::try_acquire(&project_state_path(&root), session)
                .map_err(|e| format!("原对话正在运行，请停止后再整理文件：{e}"))?,
        )
    } else {
        None
    };
    match action.as_str() {
        "archive" => saved["archived"] = json!(true),
        "restore" => {
            saved["archived"] = json!(false);
            saved["removed"] = json!(false);
        }
        "remove" => saved["removed"] = json!(true),
        "trash" => {
            if row["status"] != "available" {
                return Err("文件已变化或不可用，请刷新后再操作".into());
            }
            let scope = row["worktree_root"]
                .as_str()
                .ok_or("交付记录缺少工作目录")?;
            let path = row["path"].as_str().ok_or("交付记录缺少文件路径")?;
            let target = crate::commands::run::resolve_delivered_path(scope, path)?;
            let meta = std::fs::metadata(&target).map_err(|e| e.to_string())?;
            if row["bytes"]
                .as_u64()
                .is_some_and(|bytes| bytes != meta.len())
                || row["modified_ms"]
                    .as_u64()
                    .is_some_and(|time| Some(time) != modified_ms(&meta))
            {
                return Err("文件版本已变化，未删除文件".into());
            }
            let trash = root.join(".kanzei/delivery-trash");
            std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
            let trash = trash.canonicalize().map_err(|e| e.to_string())?;
            if !trash.starts_with(&root) {
                return Err("交付回收区不在项目内".into());
            }
            let destination = trash.join(format!(
                "{}-{}",
                std::time::SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_nanos(),
                target.file_name().unwrap_or_default().to_string_lossy()
            ));
            if destination.exists() {
                return Err("交付回收区文件名冲突，请重试".into());
            }
            std::fs::rename(&target, &destination).map_err(|e| e.to_string())?;
            saved["trashed_path"] = json!(shown(&destination));
            saved["archived"] = json!(true);
        }
        "restore_file" => {
            let trashed = row["trashed_path"].as_str().ok_or("文件不在交付回收区")?;
            let trash_root = root
                .join(".kanzei/delivery-trash")
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let source = Path::new(trashed)
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !trash_root.starts_with(&root)
                || !source.starts_with(&trash_root)
                || !source.is_file()
            {
                return Err("交付回收文件不可用".into());
            }
            let original = Path::new(row["path"].as_str().ok_or("原路径不可用")?);
            let parent = original
                .parent()
                .ok_or("原目录不可用")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            let scope = Path::new(row["worktree_root"].as_str().ok_or("原工作目录不可用")?)
                .canonicalize()
                .map_err(|e| e.to_string())?;
            if !parent.starts_with(&scope) || original.exists() {
                return Err("原目录已变化或已有同名文件，未覆盖".into());
            }
            std::fs::rename(
                &source,
                parent.join(original.file_name().ok_or("原文件名不可用")?),
            )
            .map_err(|e| e.to_string())?;
            saved["trashed_path"] = Value::Null;
            saved["archived"] = json!(false);
            saved["removed"] = json!(false);
        }
        _ => return Err("未知交付整理操作".into()),
    }
    if let Err(error) = store.append_event(session, "file.delivery_managed", &saved) {
        // Roll back a file move if its durable receipt could not be recorded.
        if action == "trash" {
            if let (Some(source), Some(original)) =
                (saved["trashed_path"].as_str(), row["path"].as_str())
            {
                let _ = std::fs::rename(source, original);
            }
        } else if action == "restore_file" {
            if let (Some(original), Some(destination)) =
                (row["path"].as_str(), row["trashed_path"].as_str())
            {
                let _ = std::fs::rename(original, destination);
            }
        }
        return Err(error.to_string());
    }
    project_receipts(&root, &store)
}

// Use the saved receipt to authorize a delivery from a sibling worktree. The UI
// cannot choose an arbitrary root, and symlinks are checked again on every click.
pub(crate) fn resolve(project: &str, path: &str) -> Result<PathBuf, String> {
    if let Ok(target) = crate::commands::run::resolve_delivered_path(project, path) {
        return Ok(target);
    }
    for row in delivered_files(project.to_string())? {
        if row["path"]
            .as_str()
            .is_some_and(|p| p.eq_ignore_ascii_case(&path.replace('\\', "/")))
        {
            return crate::commands::run::resolve_delivered_path(
                row["worktree_root"].as_str().unwrap_or(""),
                path,
            );
        }
    }
    Err("交付文件不可用，请检查文件是否已移动或删除。".into())
}

#[tauri::command]
pub(crate) async fn save_delivered_file(
    project_dir: String,
    path: String,
) -> Result<Option<String>, String> {
    let source = resolve(&project_dir, &path)?;
    let name = source
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let Some(destination) = rfd::AsyncFileDialog::new()
        .set_file_name(name)
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let destination = destination.path().to_path_buf();
    if destination.canonicalize().ok().as_ref() == Some(&source) {
        return Ok(Some(shown(&source)));
    }
    let saved = shown(&destination);
    tokio::task::spawn_blocking(move || std::fs::copy(source, destination))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(Some(saved))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delivery_management_preserves_receipts_and_recovers_files() {
        let f = Fixture::new();
        let file = f.file("organize.md");
        let receipt = f.receipt(&file);
        let id = receipt["id"].as_str().unwrap().to_string();
        let project = shown(&f.0);
        assert_eq!(
            delivery_manage(project.clone(), id.clone(), "archive".into()).unwrap()[0]["archived"],
            true
        );
        assert!(file.exists());
        assert_eq!(
            delivery_manage(project.clone(), id.clone(), "remove".into()).unwrap()[0]["removed"],
            true
        );
        assert!(file.exists());
        delivery_manage(project.clone(), id.clone(), "restore".into()).unwrap();
        let rows = delivery_manage(project.clone(), id.clone(), "trash".into()).unwrap();
        assert!(!file.exists());
        assert_eq!(rows[0]["status"], "trashed");
        let recovered =
            delivery_manage(project.clone(), id.clone(), "restore_file".into()).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"original");
        assert_eq!(recovered[0]["status"], "available");
        assert!(!recovered[0]["archived"].as_bool().unwrap());
        std::fs::write(&file, b"newer user content").unwrap();
        assert!(delivery_manage(project, id, "trash".into()).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"newer user content");
    }

    #[test]
    fn delivery_management_refuses_foreign_ids_running_owners_and_restore_overwrites() {
        let f = Fixture::new();
        let unrelated = Fixture::new();
        let file = f.file("keep.md");
        let receipt = f.receipt(&file);
        let id = receipt["id"].as_str().unwrap().to_string();
        assert!(delivery_manage(shown(&unrelated.0), id.clone(), "trash".into()).is_err());
        let guard = kanzei_core::store::session_execution::try_acquire(
            &project_state_path(&f.0),
            "delivery-test",
        )
        .unwrap();
        assert!(delivery_manage(shown(&f.0), id.clone(), "trash".into()).is_err());
        assert!(file.exists());
        drop(guard);
        delivery_manage(shown(&f.0), id.clone(), "trash".into()).unwrap();
        std::fs::write(&file, b"do not overwrite").unwrap();
        assert!(delivery_manage(shown(&f.0), id, "restore_file".into()).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"do not overwrite");
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "kz-delivery-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
        fn ctx(&self) -> ToolCtx {
            ToolCtx::new(self.0.clone(), self.0.clone()).with_session_id("delivery-test".into())
        }
        fn file(&self, name: &str) -> PathBuf {
            let p = self.0.join(name);
            std::fs::write(&p, b"original").unwrap();
            p
        }
        fn receipt(&self, path: &Path) -> Value {
            record(&self.ctx(), json!({"kind":"file","name":path.file_name().unwrap().to_string_lossy(),"path":shown(path),"bytes":8}), &std::fs::metadata(path).unwrap()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn handoff_and_engine_commit_are_visible_without_inventing_file_receipts() {
        let f = Fixture::new();
        std::fs::create_dir_all(f.0.join(".kanzei/artifacts")).unwrap();
        let records = [
            json!({"event":"claim","id":"R-001","at_ms":1}),
            json!({"event":"handoff","id":"R-001","at_ms":2,"run_id":"r","scope":"batch","target":"R-001 batch 1","criterion":"文档批次完成","summary":"三份文档","evidence_refs":["fd031b5","docs/report.md:1"]}),
            json!({"event":"deliver","id":"R-001","at_ms":3,"run_id":"r","commit":"fd031b50000000000000000000000000000000000","paths":["docs/report.md"],"test_record_ids":["T-001"],"source":"engine"}),
            json!({"event":"deliver","at_ms":4,"commit":"eeeeeee","source":"model"}),
        ];
        std::fs::write(
            f.0.join(".kanzei/artifacts/work-log.jsonl"),
            records
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\npartial malformed record",
        )
        .unwrap();
        let rows = batch_evidence(shown(&f.0)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["kind"], "git_checkpoint");
        assert_eq!(rows[0]["paths"], json!(["docs/report.md"]));
        assert_eq!(rows[1]["status"], "declared");
        assert_eq!(rows[1]["source_ref"], ".kanzei/artifacts/work-log.jsonl:2");
        assert!(delivered_files(shown(&f.0)).unwrap().is_empty());
    }

    #[test]
    fn successful_git_call_survives_compaction_and_deduplicates_engine_record() {
        let f = Fixture::new();
        let store = SessionStore::open(&project_state_path(&f.0)).unwrap();
        store.create_session("s", &shown(&f.0), None).unwrap();
        store.append_event("s", "session.tool_called", &json!({"turn_id":"r","fact":{"call_id":"c","name":"git","input":{"action":"commit","message":"文档批次"}}})).unwrap();
        store.append_event("s", "session.tool_result_committed", &json!({"turn_id":"r","fact":{"call_id":"c","is_error":false,"content":"committed verified staged set (aaaaaaaa)\nfd031b5 文档批次\n docs/report.md | 10 +"}})).unwrap();
        store
            .append_event("s", "surface_replaced", &json!({"messages":[]}))
            .unwrap();
        let rows = batch_evidence(shown(&f.0)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["commit"], "fd031b5");
        assert_eq!(rows[0]["source"], "tool_fact");
        assert!(delivered_files(shown(&f.0)).unwrap().is_empty());
        std::fs::create_dir_all(f.0.join(".kanzei/artifacts")).unwrap();
        std::fs::write(f.0.join(".kanzei/artifacts/work-log.jsonl"), json!({"event":"deliver","at_ms":1,"run_id":"r","commit":"fd031b50000000000000000000000000000000000","source":"engine","paths":["docs/report.md"]}).to_string()).unwrap();
        let rows = batch_evidence(shown(&f.0)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["source"], "work_log");
        assert_eq!(rows[0]["paths"], json!(["docs/report.md"]));
    }

    #[test]
    fn commit_parser_does_not_present_staged_fingerprint_or_prose_as_commit() {
        assert_eq!(
            committed_hash("committed verified staged set (aaaaaaaa)"),
            None
        );
        assert_eq!(committed_hash("I committed fd031b5"), None);
        assert_eq!(
            committed_hash(
                "[finalize] complete\ncommitted verified staged set (aaaaaaaa)\nfd031b5 docs"
            ),
            Some("fd031b5")
        );
    }

    #[test]
    fn delivery_survives_reopen_without_work_unit_and_tracks_missing_or_changed_file() {
        let f = Fixture::new();
        let file = f.file("app.apk");
        let receipt = f.receipt(&file);
        let rows = delivered_files(shown(&f.0)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], receipt["id"]);
        assert_eq!(rows[0]["status"], "available");
        std::fs::write(&file, b"different version").unwrap();
        assert_eq!(
            delivered_files(shown(&f.0)).unwrap()[0]["status"],
            "changed"
        );
        std::fs::remove_file(&file).unwrap();
        assert_eq!(
            delivered_files(shown(&f.0)).unwrap()[0]["status"],
            "unavailable"
        );
        assert!(resolve(&shown(&f.0), &shown(&file)).is_err());
    }
    #[test]
    fn legacy_success_recovers_path_caption_and_size_after_compaction() {
        let f = Fixture::new();
        f.file("old.apk");
        let store = SessionStore::open(&project_state_path(&f.0)).unwrap();
        store.create_session("s", &shown(&f.0), None).unwrap();
        store.append_event("s","session.tool_called", &json!({"turn_id":"r","fact":{"call_id":"c","name":"deliver","input":{"path":"old.apk","caption":"已构建 APK"}}})).unwrap();
        store.append_event("s","session.tool_result_committed", &json!({"turn_id":"r","fact":{"call_id":"c","is_error":false,"content":"[delivered] old.apk (8 bytes) — 已构建 APK"}})).unwrap();
        store
            .append_event("s", "surface_replaced", &json!({"messages":[]}))
            .unwrap();
        let rows = delivered_files(shown(&f.0)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "old.apk");
        assert_eq!(rows[0]["bytes"], 8);
        assert_eq!(rows[0]["status"], "available");
        assert_eq!(rows[0]["caption"], "已构建 APK");
    }
    #[test]
    fn sibling_worktree_is_bound_to_its_recorded_project() {
        let project = Fixture::new();
        let worktree = Fixture::new();
        let unrelated = Fixture::new();
        let file = worktree.file("report.txt");
        let ctx =
            ToolCtx::new(worktree.0.clone(), project.0.clone()).with_session_id("child".into());
        record(
            &ctx,
            json!({"kind":"file","path":shown(&file),"name":"report.txt","bytes":8}),
            &std::fs::metadata(&file).unwrap(),
        )
        .unwrap();
        assert_eq!(resolve(&shown(&project.0), &shown(&file)).unwrap(), file);
        assert!(resolve(&shown(&unrelated.0), &shown(&file)).is_err());
        assert!(delivered_files(shown(&unrelated.0)).unwrap().is_empty());
    }

    #[test]
    fn legacy_delivery_uses_registered_worktree_with_native_project_identity() {
        let project = Fixture::new();
        let worktree = Fixture::new();
        project.file("old.apk");
        let file = worktree.file("old.apk");
        let root = crate::normalized_project_root(&project.0);
        let native_root = root.display().to_string();
        let session = format!("{}#p1", kanzei_core::project_session_id(&root));
        let store = SessionStore::open(&project_state_path(&root)).unwrap();
        store.create_session(&session, &native_root, None).unwrap();
        store
            .upsert_process(&kanzei_core::StoredProcess {
                process_id: format!("p1|{native_root}"),
                origin_project: native_root.clone(),
                project_dir: native_root,
                worktree_path: Some(worktree.0.display().to_string()),
                model: None,
                profile: None,
                research_topic: None,
                reasoning: None,
                manual_models: vec![],
                phase_pipeline: false,
                subagents_enabled: false,
                tracker_writes_enabled: false,
                updated_at: 1,
            })
            .unwrap();
        store.append_event(&session, "session.tool_called", &json!({"turn_id":"r","fact":{"call_id":"c","name":"deliver","input":{"path":"old.apk"}}})).unwrap();
        store.append_event(&session, "session.tool_result_committed", &json!({"turn_id":"r","fact":{"call_id":"c","is_error":false,"content":"[delivered] old.apk (8 bytes)"}})).unwrap();
        let rows = delivered_files(shown(&root)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["path"], shown(&file));
        assert_eq!(rows[0]["worktree_root"], shown(&worktree.0));
        assert_eq!(rows[0]["status"], "available");
    }
    #[test]
    fn duplicate_deliveries_show_latest_receipt_without_losing_source_events() {
        let f = Fixture::new();
        let file = f.file("report.md");
        f.receipt(&file);
        let last = f.receipt(&file);
        let rows = delivered_files(shown(&f.0)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], last["id"]);
        let store = SessionStore::open_read_only(&project_state_path(&f.0)).unwrap();
        assert_eq!(store.delivery_events().unwrap().len(), 2);
    }
}
