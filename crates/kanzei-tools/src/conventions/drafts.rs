//! Project rules: first creation, non-destructive proposals, and user-owned CAS saves.
use super::{content_hash, CONVENTIONS_REL};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

pub const PROPOSAL_REL: &str = ".kanzei/project/conventions.proposal.json";

fn read_current(root: &Path) -> Result<String, String> {
    match std::fs::read_to_string(root.join(CONVENTIONS_REL)) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn snapshot(root: &Path) -> Result<Value, String> {
    let path = root.join(CONVENTIONS_REL);
    let content = read_current(root)?;
    let proposal = match std::fs::read_to_string(root.join(PROPOSAL_REL)) {
        Ok(text) => Some(serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    Ok(
        json!({ "exists": path.is_file(), "path": path, "hash": content_hash(&content), "content": content, "proposal": proposal }),
    )
}

fn validate(content: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err("规范内容不能为空".into());
    }
    if content.len() > 256 * 1024 {
        return Err("规范超过 256 KiB，请精简项目约束".into());
    }
    Ok(())
}

/// Exclusive creation also refuses an existing empty file; user edits never get overwritten.
pub fn create(root: &Path, content: &str) -> Result<String, String> {
    validate(content)?;
    let path = root.join(CONVENTIONS_REL);
    let _lock = crate::atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("不能覆盖已有规范；已有文件请 get 后 patch 或 propose：{e}"))?;
    file.write_all(content.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(content_hash(content))
}

/// Proposed replacements are not injected as active rules until the user saves them.
pub fn propose(root: &Path, content: &str, expected: &str) -> Result<String, String> {
    validate(content)?;
    let path = root.join(CONVENTIONS_REL);
    let _lock = crate::atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    let current = read_current(root)?;
    if content_hash(&current) != expected {
        return Err("stale expected_hash：规范已修改，请 get 后基于最新内容生成建议".into());
    }
    let proposal_path = root.join(PROPOSAL_REL);
    if proposal_path.exists() {
        return Err("已有待审建议稿，请先在规范页保存或放弃该建议；不会覆盖待审内容".into());
    }
    let hash = content_hash(content);
    let proposal = json!({"base_hash": expected, "hash": hash, "content": content});
    crate::atomic_file::write_atomic(&proposal_path, &proposal.to_string())
        .map_err(|e| e.to_string())?;
    Ok(hash)
}

/// Only the explicit frontend user action exposes whole-document save.
pub fn save_user(
    root: &Path,
    content: &str,
    expected: &str,
    proposal_hash: Option<&str>,
) -> Result<String, String> {
    validate(content)?;
    let path = root.join(CONVENTIONS_REL);
    let _lock = crate::atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    if content_hash(&read_current(root)?) != expected {
        return Err("规范已被其他操作修改。你的编辑仍保留，请复制内容并重新载入后合并。".into());
    }
    if let Some(hash) = proposal_hash {
        let proposal: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(PROPOSAL_REL)).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if proposal["hash"].as_str() != Some(hash) {
            return Err("建议稿已变化，请重新载入；规范尚未写入".into());
        }
    }
    crate::atomic_file::write_atomic_cas(&path, content, expected, content_hash)?;
    if let Some(hash) = proposal_hash {
        discard_locked(root, hash)
            .map_err(|e| format!("规范已保存，但清理建议稿失败，请重新载入：{e}"))?;
    }
    Ok(content_hash(content))
}

fn discard_locked(root: &Path, expected: &str) -> Result<(), String> {
    let path = root.join(PROPOSAL_REL);
    if !path.exists() {
        return Ok(());
    }
    let proposal: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if proposal["hash"].as_str() != Some(expected) {
        return Err("建议稿已变化，请重新载入；当前规范保存结果不受影响".into());
    }
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

pub fn discard(root: &Path, expected: &str) -> Result<(), String> {
    let _lock = crate::atomic_file::lock_exclusive(&root.join(CONVENTIONS_REL))
        .map_err(|e| e.to_string())?;
    discard_locked(root, expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn create_propose_edit_and_conflict_preserve_user_rules() {
        let root = std::env::temp_dir().join(format!("kz-conv-draft-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let first = create(&root, "# rules\nuser rule").unwrap();
        assert!(create(&root, "replacement").is_err());
        let draft = propose(&root, "# suggested\nnew rule", &first).unwrap();
        assert!(propose(&root, "another", &first).is_err());
        assert_eq!(snapshot(&root).unwrap()["content"], "# rules\nuser rule");
        assert!(save_user(&root, "invalid replacement", &first, Some("wrong-proposal")).is_err());
        assert_eq!(snapshot(&root).unwrap()["content"], "# rules\nuser rule");
        let edited = save_user(&root, "# rules\nuser edited", &first, None).unwrap();
        assert!(save_user(&root, "old draft", &first, Some(&draft)).is_err());
        assert_eq!(snapshot(&root).unwrap()["content"], "# rules\nuser edited");
        assert!(propose(&root, "stale", &first).is_err());
        save_user(
            &root,
            "# merged\nuser edited\nnew rule",
            &edited,
            Some(&draft),
        )
        .unwrap();
        assert!(snapshot(&root).unwrap()["proposal"].is_null());
        std::fs::remove_dir_all(root).unwrap();
    }
}
