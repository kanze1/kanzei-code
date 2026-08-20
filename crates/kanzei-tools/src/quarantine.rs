//! `.kanzei/quarantine/` 取证清理内核(D-566)。
//!
//! 只自动处理带已知类型和毫秒时间戳的直接子目录；未知命名的目录一律保留并计入
//! `preserved_dirs`，避免把无法判定的真实越界证据误删。默认只 dry-run，实际删除必须
//! 由调用方显式传入 `apply = true`。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const KNOWN_KINDS: &[&str] = &["shell", "shell-with-log", "cross-tree", "bg"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineEntry {
    pub path: PathBuf,
    pub kind: Option<String>,
    pub created_at_ms: Option<u128>,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupReport {
    pub dry_run: bool,
    pub scanned_dirs: usize,
    pub eligible_dirs: usize,
    pub preserved_dirs: usize,
    pub removed_dirs: usize,
    pub eligible_bytes: u64,
    pub freed_bytes: u64,
    pub preserved_paths: Vec<PathBuf>,
}

fn quarantine_root(project_root: &Path) -> PathBuf {
    project_root.join(".kanzei/quarantine")
}

fn classify(name: &str) -> Option<(String, u128)> {
    let (kind, timestamp) = name.rsplit_once('-')?;
    if !KNOWN_KINDS.contains(&kind) {
        return None;
    }
    Some((kind.to_string(), timestamp.parse().ok()?))
}

fn directory_bytes(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let child = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&child) else {
                return 0;
            };
            if metadata.is_dir() {
                directory_bytes(&child)
            } else if metadata.is_file() {
                metadata.len()
            } else {
                0
            }
        })
        .sum()
}

pub fn inspect(project_root: &Path) -> std::io::Result<Vec<QuarantineEntry>> {
    let root = quarantine_root(project_root);
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let (kind, created_at_ms) = classify(&name)
            .map(|(kind, timestamp)| (Some(kind), Some(timestamp)))
            .unwrap_or((None, None));
        entries.push(QuarantineEntry {
            bytes: directory_bytes(&path),
            path,
            kind,
            created_at_ms,
        });
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

/// 清理已知类型的旧取证目录。`kind` 与 `before_ms` 至少提供一个才能实际删除；
/// dry-run 可以不带筛选器，用于先盘点全部存量。未知命名目录永远只报告、不删除。
pub fn cleanup(
    project_root: &Path,
    kind: Option<&str>,
    before_ms: Option<u128>,
    apply: bool,
) -> std::io::Result<CleanupReport> {
    if apply && kind.is_none() && before_ms.is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "实际清理至少需要 --type 或 --older-than-days 筛选条件",
        ));
    }
    if let Some(kind) = kind {
        if !KNOWN_KINDS.contains(&kind) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("未知 quarantine 类型: {kind}"),
            ));
        }
    }
    let entries = inspect(project_root)?;
    let mut report = CleanupReport {
        dry_run: !apply,
        scanned_dirs: entries.len(),
        eligible_dirs: 0,
        preserved_dirs: 0,
        removed_dirs: 0,
        eligible_bytes: 0,
        freed_bytes: 0,
        preserved_paths: Vec::new(),
    };
    for entry in entries {
        let Some(entry_kind) = entry.kind.as_deref() else {
            report.preserved_dirs += 1;
            report.preserved_paths.push(entry.path);
            continue;
        };
        let matches_kind = kind.is_none_or(|wanted| wanted == entry_kind);
        let matches_date = before_ms
            .is_none_or(|cutoff| entry.created_at_ms.is_some_and(|created| created <= cutoff));
        if !matches_kind || !matches_date {
            report.preserved_dirs += 1;
            report.preserved_paths.push(entry.path);
            continue;
        }
        report.eligible_dirs += 1;
        report.eligible_bytes = report.eligible_bytes.saturating_add(entry.bytes);
        if apply {
            fs::remove_dir_all(&entry.path)?;
            report.removed_dirs += 1;
            report.freed_bytes = report.freed_bytes.saturating_add(entry.bytes);
        }
    }
    Ok(report)
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("kz-quarantine-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".kanzei/quarantine")).unwrap();
        root
    }

    #[test]
    fn dry_run统计已知类型并保留未知证据() {
        let root = temp_project("dry-run");
        fs::create_dir_all(root.join(".kanzei/quarantine/cross-tree-100/changed")).unwrap();
        fs::write(
            root.join(".kanzei/quarantine/cross-tree-100/changed/file.txt"),
            "evidence",
        )
        .unwrap();
        fs::create_dir_all(root.join(".kanzei/quarantine/user-kept")).unwrap();
        let report = cleanup(&root, None, None, false).unwrap();
        assert!(report.dry_run);
        assert_eq!(report.scanned_dirs, 2);
        assert_eq!(report.eligible_dirs, 1);
        assert_eq!(report.removed_dirs, 0);
        assert_eq!(report.preserved_dirs, 1);
        assert!(root.join(".kanzei/quarantine/user-kept").is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn 实际清理必须带筛选并报告释放字节数() {
        let root = temp_project("apply");
        fs::create_dir_all(root.join(".kanzei/quarantine/bg-100")).unwrap();
        fs::write(root.join(".kanzei/quarantine/bg-100/evidence"), "12345").unwrap();
        let report = cleanup(&root, Some("bg"), None, true).unwrap();
        assert_eq!(report.removed_dirs, 1);
        assert_eq!(report.freed_bytes, 5);
        assert!(!root.join(".kanzei/quarantine/bg-100").exists());
        assert!(cleanup(&root, None, None, true).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
