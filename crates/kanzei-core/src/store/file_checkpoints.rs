//! 编辑类工具的文件前像登记与内容寻址存储(R-366 B1)。
//!
//! 每个 run/path 只记录第一次触碰时的原貌；本模块只采集与索引，不执行回退。

use std::path::{Path, PathBuf};

use rusqlite::params;
use sha2::{Digest, Sha256};

use super::{now_ms, project_state_path, SessionStore, StoreError};
use kanzei_base::atomic_file::write_atomic_bytes;

pub const FILE_CHECKPOINT_MAX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct FileCheckpointTarget<'a> {
    pub project_root: &'a Path,
    pub run_id: &'a str,
    pub process_id: Option<&'a str>,
    pub tree_root: &'a Path,
    pub abs_path: &'a Path,
    pub rel_path: &'a str,
}

pub fn checkpoint_blob_path(project_root: &Path, sha256: &str) -> PathBuf {
    project_root
        .join(".kanzei")
        .join("artifacts")
        .join("checkpoints")
        .join(sha256)
}

/// 稳定识别同一文件：规范化父目录、拼回文件名，再统一分隔符；Windows 不区分大小写。
pub fn checkpoint_path_key(abs_path: &Path) -> String {
    let parent = abs_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let file_name = abs_path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let mut key = canonical_parent
        .join(file_name.as_ref())
        .to_string_lossy()
        .replace('\\', "/");
    #[cfg(windows)]
    {
        key = key.to_lowercase();
    }
    key
}

pub fn capture_file_preimage(target: &FileCheckpointTarget<'_>) -> Result<(), StoreError> {
    let store = SessionStore::open(&project_state_path(target.project_root))?;
    let path_key = checkpoint_path_key(target.abs_path);
    let captured: bool = store.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM file_checkpoints WHERE run_id = ?1 AND path_key = ?2)",
        params![target.run_id, path_key],
        |row| row.get(0),
    )?;
    if captured {
        return Ok(());
    }

    let (pre_exists, pre_blob, pre_bytes) = match std::fs::read(target.abs_path) {
        Ok(bytes) => {
            let byte_count = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
            if bytes.len() as u64 > FILE_CHECKPOINT_MAX_BYTES {
                (1_i64, None, byte_count)
            } else {
                let digest = sha256_hex(&bytes);
                let blob_path = checkpoint_blob_path(target.project_root, &digest);
                if !blob_path.is_file() {
                    write_atomic_bytes(&blob_path, &bytes)?;
                }
                (1_i64, Some(digest), byte_count)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (0_i64, None, 0_i64),
        Err(error) => return Err(StoreError::Io(error)),
    };

    let now = now_ms();
    store.connection.execute(
        "INSERT OR IGNORE INTO file_checkpoints
             (run_id, path_key, abs_path, rel_path, tree_root, process_id,
              pre_exists, pre_blob, pre_bytes, captured_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            target.run_id,
            path_key,
            target.abs_path.to_string_lossy().as_ref(),
            target.rel_path,
            target.tree_root.to_string_lossy().as_ref(),
            target.process_id,
            pre_exists,
            pre_blob,
            pre_bytes,
            now,
        ],
    )?;
    Ok(())
}

pub fn record_file_postimage(
    target: &FileCheckpointTarget<'_>,
    written: &[u8],
) -> Result<(), StoreError> {
    let store = SessionStore::open(&project_state_path(target.project_root))?;
    let changed = store.connection.execute(
        "UPDATE file_checkpoints
            SET post_hash = ?3, updated_at = ?4
          WHERE run_id = ?1 AND path_key = ?2",
        params![
            target.run_id,
            checkpoint_path_key(target.abs_path),
            sha256_hex(written),
            now_ms(),
        ],
    )?;
    if changed == 0 {
        return Err(StoreError::InvalidInput(format!(
            "file checkpoint preimage missing for run {} path {}",
            target.run_id,
            target.abs_path.display()
        )));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("writing into String is infallible");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn temp_root(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kz-file-checkpoints-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn target<'a>(
        root: &'a Path,
        run_id: &'a str,
        abs_path: &'a Path,
        rel_path: &'a str,
    ) -> FileCheckpointTarget<'a> {
        FileCheckpointTarget {
            project_root: root,
            run_id,
            process_id: Some("process-test"),
            tree_root: root,
            abs_path,
            rel_path,
        }
    }

    fn connection(root: &Path) -> Connection {
        Connection::open(project_state_path(root)).unwrap()
    }

    #[test]
    fn 同一run同一文件仅保留首次前像并记录最新后像哈希() {
        let root = temp_root("first-touch");
        let path = root.join("target.txt");
        std::fs::write(&path, b"first").unwrap();
        let target = target(&root, "run-a", &path, "target.txt");

        capture_file_preimage(&target).unwrap();
        std::fs::write(&path, b"second").unwrap();
        capture_file_preimage(&target).unwrap();
        record_file_postimage(&target, b"latest").unwrap();

        let db = connection(&root);
        let (count, exists, blob, bytes, post_hash): (
            i64,
            i64,
            Option<String>,
            i64,
            Option<String>,
        ) = db
            .query_row(
                "SELECT COUNT(*), MAX(pre_exists), MAX(pre_blob), MAX(pre_bytes), MAX(post_hash)
                   FROM file_checkpoints WHERE run_id = 'run-a'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(exists, 1);
        assert_eq!(bytes, 5);
        let blob = blob.expect("小文件应写入内容 blob");
        assert_eq!(
            std::fs::read(checkpoint_blob_path(&root, &blob)).unwrap(),
            b"first"
        );
        assert_eq!(post_hash.as_deref(), Some(sha256_hex(b"latest").as_str()));
        drop(db);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 新文件记录为原本不存在() {
        let root = temp_root("new-file");
        let path = root.join("created.txt");
        let target = target(&root, "run-new", &path, "created.txt");
        capture_file_preimage(&target).unwrap();

        let db = connection(&root);
        let (exists, blob, bytes): (i64, Option<String>, i64) = db
            .query_row(
                "SELECT pre_exists, pre_blob, pre_bytes FROM file_checkpoints WHERE run_id = ?1",
                ["run-new"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((exists, blob, bytes), (0, None, 0));
        drop(db);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 相同前像由不同路径共享单个内容blob() {
        let root = temp_root("dedupe");
        let first = root.join("first.txt");
        let second = root.join("second.txt");
        std::fs::write(&first, b"shared bytes").unwrap();
        std::fs::write(&second, b"shared bytes").unwrap();
        capture_file_preimage(&target(&root, "run-dedupe", &first, "first.txt")).unwrap();
        capture_file_preimage(&target(&root, "run-dedupe", &second, "second.txt")).unwrap();

        let db = connection(&root);
        let (rows, distinct_blobs): (i64, i64) = db
            .query_row(
                "SELECT COUNT(*), COUNT(DISTINCT pre_blob) FROM file_checkpoints WHERE run_id = ?1",
                ["run-dedupe"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((rows, distinct_blobs), (2, 1));
        let blob_dir = root.join(".kanzei/artifacts/checkpoints");
        assert_eq!(std::fs::read_dir(blob_dir).unwrap().count(), 1);
        drop(db);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 超过大小上限时记录存在但不保存blob() {
        let root = temp_root("too-large");
        let path = root.join("large.bin");
        std::fs::write(&path, vec![b'x'; FILE_CHECKPOINT_MAX_BYTES as usize + 1]).unwrap();
        capture_file_preimage(&target(&root, "run-large", &path, "large.bin")).unwrap();

        let db = connection(&root);
        let (exists, blob, bytes): (i64, Option<String>, i64) = db
            .query_row(
                "SELECT pre_exists, pre_blob, pre_bytes FROM file_checkpoints WHERE run_id = ?1",
                ["run-large"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(exists, 1);
        assert_eq!(blob, None);
        assert_eq!(bytes, FILE_CHECKPOINT_MAX_BYTES as i64 + 1);
        drop(db);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn non_not_found_io_error_is_not_misclassified_as_missing() {
        let root = temp_root("read-error");
        let path = root.join("directory");
        std::fs::create_dir_all(&path).unwrap();
        let error = capture_file_preimage(&target(&root, "run-error", &path, "directory"))
            .expect_err("目录读取必须是真错误");
        assert!(matches!(error, StoreError::Io(_)), "{error:?}");

        let db = connection(&root);
        let rows: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM file_checkpoints WHERE run_id = ?1",
                ["run-error"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0);
        drop(db);
        std::fs::remove_dir_all(root).ok();
    }
}
