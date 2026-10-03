//! Project rules: first creation, non-destructive proposals, and user-owned CAS saves.
use super::CONVENTIONS_REL;
use crate::normalized_text_hash;
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
    let hash = normalized_text_hash(&content);
    let proposal = match std::fs::read_to_string(root.join(PROPOSAL_REL)) {
        Ok(text) => Some(serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    // Legacy proposals contain unversioned DefaultHasher fingerprints and no base text.
    // Their old base cannot be reconstructed safely. Keep the proposal and its identity
    // unchanged; null asks the UI to request a comparison without claiming a user edit.
    let proposal_base_matches_current = proposal
        .as_ref()
        .and_then(|proposal| proposal["base_hash"].as_str())
        .filter(|base| base.starts_with("fnv-"))
        .map(|base| base == hash);
    Ok(
        json!({ "exists": path.is_file(), "path": path, "hash": hash, "content": content,
            "proposal": proposal, "proposal_base_matches_current": proposal_base_matches_current }),
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
    Ok(normalized_text_hash(content))
}

/// Proposed replacements are not injected as active rules until the user saves them.
pub fn propose(root: &Path, content: &str, expected: &str) -> Result<String, String> {
    validate(content)?;
    let path = root.join(CONVENTIONS_REL);
    let _lock = crate::atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    let current = read_current(root)?;
    if normalized_text_hash(&current) != expected {
        return Err("stale expected_hash：规范已修改，请 get 后基于最新内容生成建议".into());
    }
    let proposal_path = root.join(PROPOSAL_REL);
    if proposal_path.exists() {
        return Err("已有待审建议稿，请先在规范页保存或放弃该建议；不会覆盖待审内容".into());
    }
    let hash = normalized_text_hash(content);
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
    if normalized_text_hash(&read_current(root)?) != expected {
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
    crate::atomic_file::write_atomic_cas(&path, content, expected, normalized_text_hash)?;
    if let Some(hash) = proposal_hash {
        discard_locked(root, hash)
            .map_err(|e| format!("规范已保存，但清理建议稿失败，请重新载入：{e}"))?;
    }
    Ok(normalized_text_hash(content))
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

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kz-conv-draft-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn legacy_proposal(root: &Path) -> Value {
        // Saved legacy identities are opaque; never recompute them with DefaultHasher.
        let proposal = json!({
            "base_hash": "0123456789abcdef",
            "hash": "03434cc9ff86a7fd",
            "content": "# suggested\nkeep the user's rules\n"
        });
        std::fs::write(root.join(PROPOSAL_REL), proposal.to_string()).unwrap();
        proposal
    }

    #[test]
    fn proposal_base_comparison_ignores_only_line_endings() {
        let root = temp_root("line-endings");
        let original = "# rules\r\nuser rule\r\n";
        let hash = create(&root, original).unwrap();
        assert_eq!(hash, crate::content_hash(b"# rules\nuser rule\n"));
        propose(&root, "# suggested\nnew rule\n", &hash).unwrap();
        assert_eq!(
            snapshot(&root).unwrap()["proposal_base_matches_current"],
            true
        );

        std::fs::write(root.join(CONVENTIONS_REL), original.replace("\r\n", "\n")).unwrap();
        assert_eq!(
            snapshot(&root).unwrap()["proposal_base_matches_current"],
            true
        );
        std::fs::write(root.join(CONVENTIONS_REL), "# rules\nchanged rule\n").unwrap();
        assert_eq!(
            snapshot(&root).unwrap()["proposal_base_matches_current"],
            false
        );
        assert!(save_user(&root, "stale replacement", &hash, None).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_proposal_remains_readable_and_save_preserves_concurrent_edits() {
        let root = temp_root("legacy-save");
        create(&root, "# rules\nuser rule\n").unwrap();
        let legacy = legacy_proposal(&root);
        let before = std::fs::read(root.join(PROPOSAL_REL)).unwrap();
        let opened = snapshot(&root).unwrap();
        assert!(opened["proposal_base_matches_current"].is_null());
        assert_eq!(opened["proposal"], legacy);
        assert_eq!(std::fs::read(root.join(PROPOSAL_REL)).unwrap(), before);

        let concurrent = "# rules\nuser edited after opening\n";
        std::fs::write(root.join(CONVENTIONS_REL), concurrent).unwrap();
        assert!(save_user(
            &root,
            legacy["content"].as_str().unwrap(),
            opened["hash"].as_str().unwrap(),
            legacy["hash"].as_str(),
        )
        .is_err());
        assert_eq!(read_current(&root).unwrap(), concurrent);
        assert_eq!(std::fs::read(root.join(PROPOSAL_REL)).unwrap(), before);

        // After reloading and reviewing the current rules, an old proposal can still be saved.
        let reloaded = snapshot(&root).unwrap();
        let merged = "# rules\nuser edited after opening\nreviewed addition\n";
        save_user(
            &root,
            merged,
            reloaded["hash"].as_str().unwrap(),
            legacy["hash"].as_str(),
        )
        .unwrap();
        assert_eq!(read_current(&root).unwrap(), merged);
        assert!(!root.join(PROPOSAL_REL).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_proposal_identity_protects_save_and_discard() {
        let root = temp_root("legacy-identity");
        let current_hash = create(&root, "# rules\nkeep me\n").unwrap();
        let legacy = legacy_proposal(&root);
        let mut replacement = legacy.clone();
        replacement["hash"] = json!("fedcba9876543210");
        replacement["content"] = json!("# different proposal\n");
        std::fs::write(root.join(PROPOSAL_REL), replacement.to_string()).unwrap();
        assert!(save_user(
            &root,
            "unreviewed replacement",
            &current_hash,
            legacy["hash"].as_str(),
        )
        .is_err());
        assert_eq!(read_current(&root).unwrap(), "# rules\nkeep me\n");
        assert!(discard(&root, legacy["hash"].as_str().unwrap()).is_err());
        assert!(root.join(PROPOSAL_REL).exists());
        discard(&root, replacement["hash"].as_str().unwrap()).unwrap();
        assert!(!root.join(PROPOSAL_REL).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

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
