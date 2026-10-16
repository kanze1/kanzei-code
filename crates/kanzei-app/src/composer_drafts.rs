//! Composer recovery has one durable owner for native and management targets.
//! Attachment bytes are stored once by content hash; draft records hold references.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn directory() -> Result<PathBuf, String> {
    kanzei_harness::kanzei_home()
        .map(|home| home.join("composer-drafts"))
        .ok_or_else(|| "无法确定草稿目录".into())
}
fn file_for(root: &Path, key: &str) -> Result<PathBuf, String> {
    let target: Vec<String> =
        serde_json::from_str(key).map_err(|e| format!("草稿目标无效: {e}"))?;
    if target.len() != 6 {
        return Err("草稿目标必须包含完整身份".into());
    }
    Ok(root.join(format!(
        "{}.json",
        kanzei_tools::content_hash(key.as_bytes())
    )))
}
fn save_at(root: &Path, key: &str, draft: Option<Value>) -> Result<(), String> {
    let file = file_for(root, key)?;
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let _guard = kanzei_tools::atomic_file::lock_exclusive(&root.join("index"))
        .map_err(|e| e.to_string())?;
    let assets = root
        .join("attachments")
        .join(kanzei_tools::content_hash(key.as_bytes()));
    let Some(mut draft) = draft else {
        match std::fs::remove_file(file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        };
        return match std::fs::remove_dir_all(assets) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    };
    if !draft.is_object() || draft.get("text").is_some_and(|v| !v.is_string()) {
        return Err("草稿格式无效".into());
    }
    let mut referenced = std::collections::BTreeSet::new();
    if let Some(attachments) = draft.get_mut("attachments") {
        let attachments = attachments.as_array_mut().ok_or("附件列表格式无效")?;
        std::fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
        for attachment in attachments {
            let data = attachment
                .get("data")
                .and_then(Value::as_str)
                .ok_or("附件内容缺失")?;
            let hash = kanzei_tools::content_hash(data.as_bytes());
            let asset = assets.join(&hash);
            if !asset.exists() {
                kanzei_tools::atomic_file::write_atomic(&asset, data).map_err(|e| e.to_string())?;
            }
            referenced.insert(hash.clone());
            let object = attachment.as_object_mut().ok_or("附件格式无效")?;
            object.remove("data");
            object.insert("draftAsset".into(), Value::String(hash));
        }
    }
    let text =
        serde_json::to_string(&json!({"key": key, "draft": draft})).map_err(|e| e.to_string())?;
    kanzei_tools::atomic_file::write_atomic(&file, &text).map_err(|e| e.to_string())?;
    // Only this target owns this asset directory; remove references replaced by
    // the committed record without scanning or touching another conversation.
    if assets.exists() {
        for entry in std::fs::read_dir(&assets).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !referenced.contains(&entry.file_name().to_string_lossy().into_owned()) {
                std::fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}
fn load_at(root: &Path) -> Result<Vec<Value>, String> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let _guard = kanzei_tools::atomic_file::lock_exclusive(&root.join("index"))
        .map_err(|e| e.to_string())?;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut rows = vec![];
    for entry in entries {
        let file = entry.map_err(|e| e.to_string())?.path();
        if file.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let mut row: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        let key = row
            .get("key")
            .and_then(Value::as_str)
            .ok_or("草稿目标缺失")?;
        if file_for(root, key)? != file {
            return Err("草稿目标与文件不匹配".into());
        }
        let assets = root
            .join("attachments")
            .join(kanzei_tools::content_hash(key.as_bytes()));
        if !row["draft"].is_object() {
            return Err("草稿格式无效".into());
        }
        if let Some(attachments) = row["draft"].get_mut("attachments") {
            for attachment in attachments.as_array_mut().ok_or("附件列表格式无效")? {
                let hash = attachment
                    .get("draftAsset")
                    .and_then(Value::as_str)
                    .ok_or("附件引用缺失")?;
                if hash.contains(['/', '\\']) || hash == "." || hash == ".." {
                    return Err("附件引用无效".into());
                }
                let data = std::fs::read_to_string(assets.join(hash))
                    .map_err(|e| format!("草稿附件读取失败: {e}"))?;
                if kanzei_tools::content_hash(data.as_bytes()) != hash {
                    return Err("草稿附件校验失败".into());
                }
                let object = attachment.as_object_mut().ok_or("附件格式无效")?;
                object.remove("draftAsset");
                object.insert("data".into(), Value::String(data));
            }
        }
        rows.push(row);
    }
    Ok(rows)
}
#[tauri::command]
pub async fn composer_draft_save(key: String, draft: Option<Value>) -> Result<(), String> {
    let root = directory()?;
    tauri::async_runtime::spawn_blocking(move || save_at(&root, &key, draft))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn composer_drafts_load() -> Result<Vec<Value>, String> {
    let root = directory()?;
    tauri::async_runtime::spawn_blocking(move || load_at(&root))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_preserves_text_attachments_and_target_isolation() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("kz-composer-draft-{}-{stamp}", std::process::id()));
        let key = r#"["c:/project","p1","","main","",""]"#;
        let second = r#"["c:/project","p2","","main","",""]"#;
        let draft = json!({"text":"中文草稿", "attachments":[{"file_name":"a.png","media_type":"image/png","data":"YWJj"}]});
        save_at(&root, key, Some(draft.clone())).unwrap();
        save_at(
            &root,
            second,
            Some(json!({"text":"另一对话","attachments":[]})),
        )
        .unwrap();
        let rows = load_at(&root).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter().find(|row| row["key"] == key).unwrap()["draft"],
            draft
        );
        let stored = std::fs::read_to_string(file_for(&root, key).unwrap()).unwrap();
        assert!(!stored.contains("YWJj"));
        assert!(stored.contains("draftAsset"));
        let assets = root
            .join("attachments")
            .join(kanzei_tools::content_hash(key.as_bytes()));
        assert_eq!(std::fs::read_dir(&assets).unwrap().count(), 1);
        save_at(
            &root,
            key,
            Some(json!({"text":"without attachment", "attachments":[]})),
        )
        .unwrap();
        assert_eq!(std::fs::read_dir(&assets).unwrap().count(), 0);
        save_at(&root, key, Some(draft)).unwrap();
        save_at(&root, key, None).unwrap();
        assert_eq!(load_at(&root).unwrap().len(), 1);
        assert!(
            !assets.exists(),
            "deleting a draft releases its attachment ownership"
        );
        std::fs::write(file_for(&root, second).unwrap(), "broken").unwrap();
        assert!(load_at(&root).is_err());
        assert!(save_at(&root, "[]", None).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_attachment_and_failed_directory_are_explicit_errors() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kz-composer-negative-{}-{stamp}",
            std::process::id()
        ));
        let key = r#"["c:/project","p1","","main","",""]"#;
        save_at(
            &root,
            key,
            Some(json!({"text":"retain me", "attachments":[{"file_name":"a","data":"YWJj"}]})),
        )
        .unwrap();
        let asset = root
            .join("attachments")
            .join(kanzei_tools::content_hash(key.as_bytes()))
            .join(kanzei_tools::content_hash(b"YWJj"));
        std::fs::remove_file(asset).unwrap();
        assert!(load_at(&root).unwrap_err().contains("附件读取失败"));
        let blocked = root.join("blocked");
        std::fs::write(&blocked, "not a directory").unwrap();
        assert!(save_at(&blocked, key, Some(json!({"text":"keep in UI"}))).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
