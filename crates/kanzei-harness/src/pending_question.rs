//! Durable question + answer outbox. Stored separately from conversation projections.
use kanzei_base::atomic_file;
use serde_json::Value;
use std::path::{Path, PathBuf};
fn dir(root: &Path) -> PathBuf {
    root.join(".kanzei/runtime/questions")
}
fn path(root: &Path, id: u64) -> PathBuf {
    dir(root).join(format!("{id}.json"))
}
pub fn save(root: &Path, value: &Value) -> Result<(), String> {
    let id = value["payload"]["id"].as_u64().ok_or("问题缺少标识")?;
    std::fs::create_dir_all(dir(root)).map_err(|e| e.to_string())?;
    let path = path(root, id);
    let _lock = atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    if path.exists() {
        return Err("问题标识已存在".into());
    }
    atomic_file::write_atomic(&path, &value.to_string()).map_err(|e| e.to_string())
}
pub fn get(root: &Path, id: u64) -> Result<Value, String> {
    let bytes = std::fs::read(path(root, id)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "question_expired: 问题已不存在，请刷新「待我处理」".to_string()
        } else {
            error.to_string()
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
pub fn list(root: &Path) -> Result<Vec<Value>, String> {
    let entries = match std::fs::read_dir(dir(root)) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut values = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let value: Value =
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("问题记录损坏 {}: {e}", path.display()))?;
        if matches!(value["state"].as_str(), Some("pending" | "answered")) {
            values.push(value);
        }
    }
    Ok(values)
}
fn update(
    root: &Path,
    id: u64,
    change: impl FnOnce(&mut Value) -> Result<(), String>,
) -> Result<Value, String> {
    let path = path(root, id);
    let _lock = atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    let mut value = get(root, id)?;
    change(&mut value)?;
    atomic_file::write_atomic(&path, &value.to_string()).map_err(|e| e.to_string())?;
    Ok(value)
}
pub fn answer(root: &Path, id: u64, reply: &str, request_id: &str) -> Result<Value, String> {
    respond(root, id, reply, request_id, false)
}
pub fn respond(
    root: &Path,
    id: u64,
    reply: &str,
    request_id: &str,
    cancel: bool,
) -> Result<Value, String> {
    update(root, id, |value| {
        if value["state"] == "cancelled" {
            return Err("问题已取消".into());
        }
        if value["reply"].is_string() {
            if value["reply"] != reply
                || value["request_id"] != request_id
                || value["user_cancelled"].as_bool().unwrap_or(false) != cancel
            {
                return Err("该问题已收到另一份回复".into());
            }
            return Ok(());
        }
        value["reply"] = Value::String(reply.into());
        value["request_id"] = Value::String(request_id.into());
        value["user_cancelled"] = Value::Bool(cancel);
        value["state"] = Value::String("answered".into());
        Ok(())
    })
}
pub fn settle(root: &Path, callback: &str, state: &str) -> Result<(), String> {
    for value in list(root)? {
        if value["callback_id"] == callback {
            update(
                root,
                value["payload"]["id"].as_u64().ok_or("问题标识损坏")?,
                |value| {
                    if matches!(value["state"].as_str(), Some("pending" | "answered")) {
                        value["state"] = Value::String(state.into());
                    }
                    Ok(())
                },
            )?;
        }
    }
    Ok(())
}

/// Explicit stop retires old questions as well as live oneshot senders.
pub fn cancel_owner(root: &Path, owner: &str, child: Option<&str>) -> Result<(), String> {
    for record in list(root)? {
        if record["payload"]["sessionId"] == owner
            && child.is_none_or(|id| record["payload"]["agentId"] == id)
        {
            let id = record["payload"]["id"].as_u64().ok_or("问题标识损坏")?;
            update(root, id, |value| {
                if matches!(value["state"].as_str(), Some("pending" | "answered")) {
                    value["state"] = Value::String("cancelled".into());
                }
                Ok(())
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn missing_question_is_expired_but_corrupt_record_preserves_its_error() {
        let root = std::env::temp_dir().join(format!(
            "kz-question-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(get(&root, 7).unwrap_err().starts_with("question_expired:"));
        std::fs::create_dir_all(dir(&root)).unwrap();
        std::fs::write(path(&root, 7), "broken json").unwrap();
        assert!(!get(&root, 7).unwrap_err().contains("question_expired"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reopen_and_deduplicate_answer_outbox() {
        let root = std::env::temp_dir().join(format!(
            "kz-question-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        save(
            &root,
            &json!({"payload":{"id":1},"callback_id":"q1","state":"pending"}),
        )
        .unwrap();
        assert_eq!(list(&root).unwrap().len(), 1);
        answer(&root, 1, "选择 A", "r1").unwrap();
        answer(&root, 1, "选择 A", "r1").unwrap();
        assert!(answer(&root, 1, "选择 B", "r1").is_err());
        assert_eq!(get(&root, 1).unwrap()["reply"], "选择 A");
        settle(&root, "q1", "delivered").unwrap();
        assert!(list(&root).unwrap().is_empty());
        assert_eq!(get(&root, 1).unwrap()["state"], "delivered");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stop_cancels_only_the_original_actor_and_literal_cancel_is_an_answer() {
        let root = std::env::temp_dir().join(format!(
            "kz-question-stop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for (id, owner, child) in [(1, "a", Some("child")), (2, "a", None), (3, "b", None)] {
            save(&root,&json!({"payload":{"id":id,"sessionId":owner,"agentId":child},"callback_id":format!("q{id}"),"state":"pending"})).unwrap();
        }
        assert_eq!(
            answer(&root, 2, "cancel", "r2").unwrap()["user_cancelled"],
            false
        );
        cancel_owner(&root, "a", Some("child")).unwrap();
        assert!(answer(&root, 1, "A", "late").is_err());
        assert_eq!(list(&root).unwrap().len(), 2);
        cancel_owner(&root, "a", None).unwrap();
        settle(&root, "q2", "delivered").unwrap();
        assert_eq!(get(&root, 2).unwrap()["state"], "cancelled");
        assert_eq!(list(&root).unwrap()[0]["payload"]["sessionId"], "b");
        assert_eq!(
            respond(&root, 3, "", "cancel", true).unwrap()["user_cancelled"],
            true
        );
        assert!(answer(&root, 3, "", "cancel").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
