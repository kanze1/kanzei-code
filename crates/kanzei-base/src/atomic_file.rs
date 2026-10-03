//! 同目录临时文件 + rename：读取方只会看见完整的旧版本或新版本。
//!
//! 原子替换只负责文件可见性。读改写事务由 FileLock 串行化；CAS 在内部持锁。
//! 文档与写日志仍是两次持久化，不是一个跨文件事务。

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod lock;
pub use lock::{
    lock_exclusive, lock_path_for, lock_shared, try_lock_exclusive, try_lock_shared, FileLock,
    DEFAULT_LOCK_BUDGET,
};

/// 写入并同步同目录临时文件，然后一次 rename 替换目标。
/// 替换失败立即报错，完整候选文件保留在错误给出的路径。
pub fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_bytes_guarded(path, bytes, || Ok(()))
}

/// 临时内容落盘后、替换前执行检查。检查失败不改目标，删除候选文件。
/// 调用者负责读改写事务的锁；检查本身不能阻止外部编辑器在 rename 前写入。
pub fn write_atomic_bytes_guarded(
    path: &Path,
    bytes: &[u8],
    mut check: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} 没有父目录", path.display()),
            )
        })?;
    std::fs::create_dir_all(parent)?;
    let tmp = temp_sibling(path, parent)?;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        std::fs::remove_file(&tmp)?;
        return Err(error);
    }
    drop(file);
    if let Err(error) = check() {
        std::fs::remove_file(&tmp)?;
        return Err(error);
    }
    std::fs::rename(&tmp, path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "原子替换 {} 失败: {error}。新内容保留在 {}，原文件未被破坏。",
                path.display(),
                tmp.display()
            ),
        )
    })
}

pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    write_atomic_bytes(path, text.as_bytes())
}

/// 在同一 FileLock 内校验内容指纹并替换。已有外层排他锁时按同线程重入处理。
///
/// 这保证遵守 FileLock 协议的 writer 不会在检查与替换之间插入写入；不是 OS
/// 条件交换，不能对绕过协议的外部编辑器作同样保证。尚未创建的文件按空内容校验。
pub fn write_atomic_cas(
    path: &Path,
    content: &str,
    expected_hash: &str,
    hash_of: impl Fn(&str) -> String,
) -> Result<(), String> {
    let _lock = lock_exclusive(path).map_err(|error| error.to_string())?;
    write_atomic_bytes_guarded(path, content.as_bytes(), || {
        let live = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        let actual = hash_of(&live);
        if actual != expected_hash {
            return Err(io::Error::other(format!(
                "stale expected_hash `{expected_hash}` during final write; the file is now `{actual}`. Nothing was replaced. Re-run `get`."
            )));
        }
        Ok(())
    }).map_err(|error| error.to_string())
}

fn temp_sibling(path: &Path, parent: &Path) -> io::Result<PathBuf> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "目标没有文件名"))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mut temporary = std::ffi::OsString::from(".");
    temporary.push(name);
    temporary.push(format!(
        ".{}.{nanos}.{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    Ok(parent.join(temporary))
}

#[cfg(test)]
mod tests;
