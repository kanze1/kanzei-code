//! 编辑类工具的文件前像登记与内容寻址存储(R-366 B1)。
//!
//! 每个 run/path 只记录第一次触碰时的原貌；本模块只采集与索引，不执行回退。
//! 前像采不到时要占住一条哨兵行(pre_exists=1、pre_blob=NULL、pre_bytes=-1),让同 run
//! 后续触碰按首触语义直接返回——宁可「不可还原」,也不能把本 run 写出的中间态补采成
//! 前像(审计 19)。哨兵实际覆盖到哪里(写路径见 kanzei-tools write.rs 的
//! `file_checkpointed_write`):
//! - 原文读到了、blob 写不进:采集当场先插哨兵行再报错;
//! - 前像这一步整体没成(采集报错,或写前 state.db 打开失败):落盘后记后像时,本
//!   run/path 还没有行就插哨兵行;写前 open 失败的,落盘后再 open 一次,只为写这条行;
//! - 边界:落盘后那次 open 或后像写入也失败时不留任何行,同 run 后续触碰仍可能把
//!   中间态采成前像(审计 19(3) 的进程内兜底未做)。

use std::path::{Path, PathBuf};

use rusqlite::params;
use sha2::{Digest, Sha256};

use super::{now_ms, project_state_path, SessionStore, StoreError};
use kanzei_base::atomic_file::write_atomic_bytes;

pub const FILE_CHECKPOINT_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// 前像未知的哨兵值:与超上限行(pre_bytes > 上限)同样 pre_blob=NULL,靠 -1 区分原因。
pub const FILE_CHECKPOINT_UNKNOWN_PRE_BYTES: i64 = -1;

#[derive(Clone, Copy, Debug)]
pub struct FileCheckpointTarget<'a> {
    pub project_root: &'a Path,
    pub run_id: &'a str,
    pub process_id: Option<&'a str>,
    /// 代码树根,见 [`file_checkpoint_tree_root`];rel_path 由它与 abs_path 现算,不收调用方的原始入参。
    pub tree_root: &'a Path,
    pub abs_path: &'a Path,
}

pub fn file_checkpoint_blob_path(project_root: &Path, sha256: &str) -> PathBuf {
    project_root
        .join(".kanzei")
        .join("artifacts")
        .join("checkpoints")
        .join(sha256)
}

/// 稳定识别同一文件：规范化父目录、拼回文件名，再统一分隔符；Windows 不区分大小写。
pub fn file_checkpoint_path_key(abs_path: &Path) -> String {
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

/// 路径比较形态,仿 kanzei-harness `project_root::dir_key`(那边是 pub(crate),跨 crate
/// 够不着):剥 `\\?\` / `\\?\UNC\` 前缀、Windows 上统一分隔符并小写、折叠 `.` / `..`
/// (复用权限侧同一份 normalize_resource)、去尾分隔符;结果以 `/` 分隔。
///
/// 只活在相等/前缀判断里,不存储。裸 `Path ==` 在 Windows 上除盘符外逐字节比较,
/// `C:\Users\Kanzei\proj`、`c:\users\kanzei\proj`、`\\?\C:\Users\Kanzei\proj` 三者两两不等。
fn path_compare_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let stripped = raw
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| raw.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| raw.to_string());
    // 分隔符与大小写只在 Windows 上等价;其它平台上 `A` 与 `a` 是两个目录。
    #[cfg(windows)]
    let unified = stripped.replace('/', "\\").to_lowercase();
    #[cfg(not(windows))]
    let unified = stripped;
    let key = kanzei_harness::permission::normalize_resource(&unified);
    key.trim_end_matches(['\\', '/']).to_string()
}

/// 代码树根(审计 17):从 cwd 向上找最近的含 `.git` 项(worktree 里是文件)的目录,
/// 以 project_root 封顶——走到项目根仍没有 `.git` 就停在项目根,不越过它往上找
/// (免得撞上 HOME 的 dotfiles 仓库);cwd 不在项目根之下且一路都没有 `.git` 时退回 cwd。
/// 不认 `.kanzei` 标记。项目根的判定与写法无关(见 [`path_compare_key`]):显式
/// `--project-root` 的大小写、分隔符或 `\\?\` 形态与 cwd 不同,封顶照样生效。
///
/// 口径边界:CLI 在项目子目录里跑时被提升到同一棵树的树根;桌面端 cwd 含 `.git` 或
/// 就是主根时,结果与 code_root_for 一致;打开的目录下既无 `.git` 也无 `.kanzei`、
/// 主根是它的某个祖先时,这里会上提到主根,而不是 code_root_for(project_dir)。
pub fn file_checkpoint_tree_root(cwd: &Path, project_root: &Path) -> PathBuf {
    let root_key = (!project_root.as_os_str().is_empty()).then(|| path_compare_key(project_root));
    for dir in cwd.ancestors() {
        if dir.as_os_str().is_empty() {
            break;
        }
        if dir.join(".git").exists() || root_key.as_deref() == Some(path_compare_key(dir).as_str())
        {
            return dir.to_path_buf();
        }
    }
    cwd.to_path_buf()
}

/// 相对代码树根的路径,统一 '/' 分隔;落在树外(或恰为树根本身)时原样记 abs_path
/// 字符串(与 abs_path 列相同,即树外标记)。
///
/// 前缀比较走 [`path_compare_key`]:先词法比,对不上(链接、8.3 短名等)再按规范化后
/// 的父目录比一次。Windows 上结果统一小写,与 [`file_checkpoint_path_key`] 口径一致——
/// 写工具的 normalize_resource 会把入参转小写,相对入参得到「原大小写 cwd + 小写尾部」、
/// 绝对入参得到整串小写,不统一的话同一文件会随入参形式记成两种 rel_path。
/// `..` 按词法折叠:折回树内时照常算相对路径,折到树外时记 abs_path。
pub fn file_checkpoint_rel_path(tree_root: &Path, abs_path: &Path) -> String {
    fn relative_under(root_key: &str, path_key: &str) -> Option<String> {
        let rest = path_key.strip_prefix(root_key)?.strip_prefix('/')?;
        (!rest.is_empty() && rest.split('/').all(|seg| !matches!(seg, "" | "." | "..")))
            .then(|| rest.to_string())
    }

    if tree_root.as_os_str().is_empty() {
        return abs_path.to_string_lossy().into_owned();
    }
    relative_under(&path_compare_key(tree_root), &path_compare_key(abs_path))
        .or_else(|| {
            let root = std::fs::canonicalize(tree_root).ok()?;
            let parent = abs_path.parent().filter(|p| !p.as_os_str().is_empty())?;
            let canonical = std::fs::canonicalize(parent)
                .ok()?
                .join(abs_path.file_name()?);
            relative_under(&path_compare_key(&root), &path_compare_key(&canonical))
        })
        .unwrap_or_else(|| abs_path.to_string_lossy().into_owned())
}

/// 薄包装:自己 open 一次再转调。写路径用 [`capture_file_preimage_in`] 与后像共用连接。
pub fn capture_file_preimage(target: &FileCheckpointTarget<'_>) -> Result<(), StoreError> {
    let store = SessionStore::open(&project_state_path(target.project_root))?;
    capture_file_preimage_in(&store, target)
}

pub fn capture_file_preimage_in(
    store: &SessionStore,
    target: &FileCheckpointTarget<'_>,
) -> Result<(), StoreError> {
    let path_key = file_checkpoint_path_key(target.abs_path);
    let captured: bool = store.connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM file_checkpoints WHERE run_id = ?1 AND path_key = ?2)",
        params![target.run_id, path_key],
        |row| row.get(0),
    )?;
    if captured {
        return Ok(());
    }

    // 先看元数据:超上限的大文件不整读进内存(审计 18)。
    let (pre_exists, pre_blob, pre_bytes) = match std::fs::metadata(target.abs_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (0_i64, None, 0_i64),
        Err(error) => return Err(StoreError::Io(error)),
        Ok(metadata) if metadata.len() > FILE_CHECKPOINT_MAX_BYTES => (
            1_i64,
            None,
            i64::try_from(metadata.len()).unwrap_or(i64::MAX),
        ),
        Ok(_) => {
            let bytes = std::fs::read(target.abs_path)?;
            let byte_count = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
            if bytes.len() as u64 > FILE_CHECKPOINT_MAX_BYTES {
                (1_i64, None, byte_count)
            } else {
                let digest = sha256_hex(&bytes);
                let blob_path = file_checkpoint_blob_path(target.project_root, &digest);
                if !blob_path.is_file() {
                    if let Err(error) = write_atomic_bytes(&blob_path, &bytes) {
                        // 审计 19:原文读到了却存不下——先占哨兵行再报错,后续触碰不得补采。
                        let _ = insert_preimage_row(
                            store,
                            target,
                            &path_key,
                            1,
                            None,
                            FILE_CHECKPOINT_UNKNOWN_PRE_BYTES,
                        );
                        return Err(StoreError::Io(error));
                    }
                }
                (1_i64, Some(digest), byte_count)
            }
        }
    };

    insert_preimage_row(
        store,
        target,
        &path_key,
        pre_exists,
        pre_blob.as_deref(),
        pre_bytes,
    )
}

fn insert_preimage_row(
    store: &SessionStore,
    target: &FileCheckpointTarget<'_>,
    path_key: &str,
    pre_exists: i64,
    pre_blob: Option<&str>,
    pre_bytes: i64,
) -> Result<(), StoreError> {
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
            file_checkpoint_rel_path(target.tree_root, target.abs_path),
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

/// 薄包装:自己 open 一次再转调。写路径用 [`record_file_postimage_in`] 与前像共用连接。
pub fn record_file_postimage(
    target: &FileCheckpointTarget<'_>,
    written: &[u8],
) -> Result<(), StoreError> {
    let store = SessionStore::open(&project_state_path(target.project_root))?;
    record_file_postimage_in(&store, target, written)
}

/// 记后像哈希。本 run/path 还没有行(前像没采到)时插入哨兵行占位(审计 19)。
pub fn record_file_postimage_in(
    store: &SessionStore,
    target: &FileCheckpointTarget<'_>,
    written: &[u8],
) -> Result<(), StoreError> {
    let now = now_ms();
    let path_key = file_checkpoint_path_key(target.abs_path);
    let post_hash = sha256_hex(written);
    // 常见路径:前像行已在,只更新后像哈希。rel_path 只有插哨兵行时才用得上,
    // 不在这里现算(树外/别名路径要走 canonicalize)。
    let updated = store.connection.execute(
        "UPDATE file_checkpoints SET post_hash = ?3, updated_at = ?4
          WHERE run_id = ?1 AND path_key = ?2",
        params![target.run_id, path_key, post_hash, now],
    )?;
    if updated > 0 {
        return Ok(());
    }
    // 没有行:插哨兵行。仍用 upsert——UPDATE 与这里之间别的进程插了行,也不丢 post_hash。
    store.connection.execute(
        "INSERT INTO file_checkpoints
             (run_id, path_key, abs_path, rel_path, tree_root, process_id,
              pre_exists, pre_blob, pre_bytes, post_hash, captured_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, NULL, ?7, ?8, ?9, ?9)
         ON CONFLICT(run_id, path_key) DO UPDATE
            SET post_hash = excluded.post_hash, updated_at = excluded.updated_at",
        params![
            target.run_id,
            path_key,
            target.abs_path.to_string_lossy().as_ref(),
            file_checkpoint_rel_path(target.tree_root, target.abs_path),
            target.tree_root.to_string_lossy().as_ref(),
            target.process_id,
            FILE_CHECKPOINT_UNKNOWN_PRE_BYTES,
            post_hash,
            now,
        ],
    )?;
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

    fn target<'a>(root: &'a Path, run_id: &'a str, abs_path: &'a Path) -> FileCheckpointTarget<'a> {
        FileCheckpointTarget {
            project_root: root,
            run_id,
            process_id: Some("process-test"),
            tree_root: root,
            abs_path,
        }
    }

    fn connection(root: &Path) -> Connection {
        Connection::open(project_state_path(root)).unwrap()
    }

    type PreimageRow = (i64, Option<String>, i64, Option<String>);

    fn preimage_row(root: &Path, run_id: &str) -> PreimageRow {
        connection(root)
            .query_row(
                "SELECT pre_exists, pre_blob, pre_bytes, post_hash
                   FROM file_checkpoints WHERE run_id = ?1",
                [run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
    }

    #[test]
    fn 同一run同一文件仅保留首次前像并记录最新后像哈希() {
        let root = temp_root("first-touch");
        let path = root.join("target.txt");
        std::fs::write(&path, b"first").unwrap();
        let target = target(&root, "run-a", &path);

        capture_file_preimage(&target).unwrap();
        std::fs::write(&path, b"second").unwrap();
        capture_file_preimage(&target).unwrap();
        record_file_postimage(&target, b"latest").unwrap();

        let db = connection(&root);
        let (count, exists, blob, bytes, post_hash, rel_path): (
            i64,
            i64,
            Option<String>,
            i64,
            Option<String>,
            String,
        ) = db
            .query_row(
                "SELECT COUNT(*), MAX(pre_exists), MAX(pre_blob), MAX(pre_bytes), MAX(post_hash),
                        MAX(rel_path)
                   FROM file_checkpoints WHERE run_id = 'run-a'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(exists, 1);
        assert_eq!(bytes, 5);
        assert_eq!(rel_path, "target.txt");
        let blob = blob.expect("小文件应写入内容 blob");
        assert_eq!(
            std::fs::read(file_checkpoint_blob_path(&root, &blob)).unwrap(),
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
        capture_file_preimage(&target(&root, "run-new", &path)).unwrap();

        let (exists, blob, bytes, _) = preimage_row(&root, "run-new");
        assert_eq!((exists, blob, bytes), (0, None, 0));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 相同前像由不同路径共享单个内容blob() {
        let root = temp_root("dedupe");
        let first = root.join("first.txt");
        let second = root.join("second.txt");
        std::fs::write(&first, b"shared bytes").unwrap();
        std::fs::write(&second, b"shared bytes").unwrap();
        capture_file_preimage(&target(&root, "run-dedupe", &first)).unwrap();
        capture_file_preimage(&target(&root, "run-dedupe", &second)).unwrap();

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
    fn 超过大小上限时只看元数据不整读且不保存blob() {
        let root = temp_root("too-large");
        let path = root.join("large.bin");
        let size = FILE_CHECKPOINT_MAX_BYTES + 1;
        std::fs::File::create(&path).unwrap().set_len(size).unwrap();
        // 审计 18:让「整读」必然失败——capture 仍成功,说明大文件只走了 metadata。
        #[cfg(windows)]
        let lock = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .unwrap()
        };
        #[cfg(windows)]
        assert!(std::fs::read(&path).is_err(), "独占句柄下整读应失败");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
            // root 无视权限位,chmod 000 挡不住它的整读;刚建的文件属主就是有效 uid,
            // 为 0 时这条前提不成立,跳过断言(其余断言照跑)。
            if std::fs::metadata(&path).unwrap().uid() != 0 {
                assert!(std::fs::read(&path).is_err(), "chmod 000 下整读应失败");
            }
        }
        capture_file_preimage(&target(&root, "run-large", &path)).unwrap();
        #[cfg(windows)]
        drop(lock);

        let (exists, blob, bytes, _) = preimage_row(&root, "run-large");
        assert_eq!(exists, 1);
        assert_eq!(blob, None);
        assert_eq!(bytes, size as i64);
        assert!(!root.join(".kanzei/artifacts/checkpoints").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn non_not_found_io_error_is_not_misclassified_as_missing() {
        let root = temp_root("read-error");
        let path = root.join("directory");
        std::fs::create_dir_all(&path).unwrap();
        let error = capture_file_preimage(&target(&root, "run-error", &path))
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

    #[test]
    fn blob写失败先占哨兵行且后续触碰不补采中间态() {
        let root = temp_root("blob-failure");
        // blob 目录的位置被普通文件占住:state.db 照常可写,只有 blob 落不下。
        let artifacts = root.join(".kanzei/artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(artifacts.join("checkpoints"), b"not a directory").unwrap();
        let path = root.join("target.txt");
        std::fs::write(&path, b"original").unwrap();
        let target = target(&root, "run-sentinel", &path);

        let error = capture_file_preimage(&target).expect_err("blob 写不进必须报错");
        assert!(matches!(error, StoreError::Io(_)), "{error:?}");
        assert_eq!(
            preimage_row(&root, "run-sentinel"),
            (1, None, FILE_CHECKPOINT_UNKNOWN_PRE_BYTES, None)
        );

        std::fs::write(&path, b"first write").unwrap();
        record_file_postimage(&target, b"first write").unwrap();
        std::fs::write(&path, b"intermediate").unwrap();
        capture_file_preimage(&target).unwrap();
        record_file_postimage(&target, b"second write").unwrap();

        assert_eq!(
            preimage_row(&root, "run-sentinel"),
            (
                1,
                None,
                FILE_CHECKPOINT_UNKNOWN_PRE_BYTES,
                Some(sha256_hex(b"second write"))
            )
        );
        assert!(!artifacts.join("checkpoints").is_dir());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 只有后像时补哨兵行且之后不再采前像() {
        let root = temp_root("postimage-only");
        let path = root.join("target.txt");
        std::fs::write(&path, b"written by this run").unwrap();
        let target = target(&root, "run-post-only", &path);

        record_file_postimage(&target, b"written by this run").unwrap();
        capture_file_preimage(&target).unwrap();

        let (exists, blob, bytes, post_hash) = preimage_row(&root, "run-post-only");
        assert_eq!(
            (exists, blob, bytes),
            (1, None, FILE_CHECKPOINT_UNKNOWN_PRE_BYTES)
        );
        assert_eq!(post_hash, Some(sha256_hex(b"written by this run")));
        assert!(!root.join(".kanzei/artifacts/checkpoints").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 代码树根取最近git项且不越过项目根() {
        let root = temp_root("tree-root");
        let worktree = root.join("lines").join("wt");
        let nested = worktree.join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        // worktree 的 .git 是文件。
        std::fs::write(worktree.join(".git"), b"gitdir: elsewhere").unwrap();
        let elsewhere = root.join("main-project");
        assert_eq!(file_checkpoint_tree_root(&nested, &elsewhere), worktree);

        // 没有 .git 时停在项目根:CLI 在项目子目录跑也记项目根。
        let plain = temp_root("tree-root-plain");
        let sub = plain.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(file_checkpoint_tree_root(&sub, &plain), plain);
        assert_eq!(file_checkpoint_tree_root(&plain, &plain), plain);
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(plain).ok();
    }

    #[test]
    fn 项目根封顶与写法无关_不越过项目根去找上层git() {
        // 上层有 .git、项目根没有:封顶失效就会越过项目根,把上层仓库当树根。
        let outer = temp_root("tree-root-cap");
        std::fs::create_dir_all(outer.join(".git")).unwrap();
        let project = outer.join("proj");
        let sub = project.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        // 跨平台:带 `.` / `..` 段、尾分隔符的写法。
        let dotted = project.join("sub").join("..").join(".");
        assert_eq!(file_checkpoint_tree_root(&sub, &dotted), project);
        let trailing = PathBuf::from(format!(
            "{}{}",
            project.display(),
            std::path::MAIN_SEPARATOR
        ));
        assert_eq!(file_checkpoint_tree_root(&sub, &trailing), project);

        // Windows:显式 --project-root 与 cwd 大小写不同、分隔符不同或是 `\\?\` 形态。
        #[cfg(windows)]
        {
            let upper = PathBuf::from(project.to_string_lossy().to_uppercase());
            assert_ne!(upper.as_os_str(), project.as_os_str(), "夹具应改变大小写");
            assert_eq!(file_checkpoint_tree_root(&sub, &upper), project);
            let lower = PathBuf::from(project.to_string_lossy().to_lowercase());
            assert_eq!(file_checkpoint_tree_root(&sub, &lower), project);
            let slashed = PathBuf::from(project.to_string_lossy().replace('\\', "/"));
            assert_eq!(file_checkpoint_tree_root(&sub, &slashed), project);
            let verbatim = PathBuf::from(format!(r"\\?\{}", project.display()));
            assert_eq!(file_checkpoint_tree_root(&sub, &verbatim), project);
            // cwd 自己是 `\\?\` 形态、项目根是裸写法,同样停在项目根(返回 cwd 那侧的写法)。
            let verbatim_sub = PathBuf::from(format!(r"\\?\{}", sub.display()));
            assert_eq!(
                file_checkpoint_tree_root(&verbatim_sub, &upper),
                PathBuf::from(format!(r"\\?\{}", project.display()))
            );
        }
        std::fs::remove_dir_all(outer).ok();
    }

    #[test]
    fn 相对路径按树根计算且树外记绝对路径() {
        let root = temp_root("rel-path");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            file_checkpoint_rel_path(&root, &sub.join("x.txt")),
            "sub/x.txt"
        );
        let outside_dir = temp_root("rel-path-outside");
        let outside = outside_dir.join("y.txt");
        assert_eq!(
            file_checkpoint_rel_path(&root, &outside),
            outside.to_string_lossy()
        );
        let escaped = root.join("..").join("escaped.txt");
        assert_eq!(
            file_checkpoint_rel_path(&root, &escaped),
            escaped.to_string_lossy()
        );
        // `..` 折回树内时照常算相对路径。
        assert_eq!(
            file_checkpoint_rel_path(&root, &sub.join("..").join("back.txt")),
            "back.txt"
        );
        assert_eq!(
            file_checkpoint_rel_path(&root, &root),
            root.to_string_lossy(),
            "树根本身不是树内文件"
        );
        std::fs::remove_dir_all(outside_dir).ok();
        std::fs::remove_dir_all(root).ok();
    }

    /// Windows:写工具的 normalize_resource 把入参转小写——相对入参得到「原大小写 cwd +
    /// 小写尾部」,绝对入参得到整串小写(还用 `/` 分隔)。两种入参必须算出同一个 rel_path,
    /// 且与 path_key 同为小写口径;树根是 `\\?\` 形态也一样。
    #[cfg(windows)]
    #[test]
    fn 相对与绝对入参得到同一相对路径且统一小写() {
        let root = temp_root("rel-case");
        let tree = root.join("Tree");
        std::fs::create_dir_all(tree.join("Sub")).unwrap();
        let relative_input = tree.join("sub/x.txt");
        let absolute_input = PathBuf::from(kanzei_harness::permission::normalize_resource(
            &tree.join("Sub").join("X.txt").to_string_lossy(),
        ));
        assert_ne!(
            absolute_input.as_os_str(),
            relative_input.as_os_str(),
            "夹具应让两种入参的写法不同"
        );
        assert_eq!(
            file_checkpoint_rel_path(&tree, &relative_input),
            "sub/x.txt"
        );
        assert_eq!(
            file_checkpoint_rel_path(&tree, &absolute_input),
            "sub/x.txt"
        );
        let verbatim_tree = PathBuf::from(format!(r"\\?\{}", tree.display()));
        assert_eq!(
            file_checkpoint_rel_path(&verbatim_tree, &absolute_input),
            "sub/x.txt"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
