//! OS 文件锁负责进程间互斥；每路径的槽只负责线程重入和共享计数。
//! 非阻塞获取在槽内完成，等待时释放槽，不需要 acquiring 中间态。
//! 协调文件放在用户缓存的 kanzei/file-locks 下，绝不占用或创建工作区文件。
//! 它们保留稳定身份；锁占用由 OS 释放，内存槽在最后一个持有者/等待者退出时回收。

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

mod directory;

pub const DEFAULT_LOCK_BUDGET: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// 锁只在获取它的线程上释放；读改写事务不得跨 await 或耗时外部调用。
pub struct FileLock {
    slot: Arc<Slot>,
    mode: Mode,
    path: PathBuf,
    acquired_at: Instant,
    _not_send: std::marker::PhantomData<*const ()>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Shared,
    Exclusive,
}

#[derive(Default)]
enum State {
    #[default]
    Unlocked,
    Shared {
        readers: HashMap<ThreadId, usize>,
        _file: File,
    },
    Exclusive {
        owner: ThreadId,
        depth: usize,
        _file: File,
    },
}

struct Slot {
    path: PathBuf,
    state: Mutex<State>,
    released: Condvar,
}

/// Stable coordination identity for the complete physical target path, including
/// its extension. Resolving this path never creates anything in the target tree.
/// All instances in the OS account share a cache independent of process profiles.
pub fn lock_path_for(target: &Path) -> io::Result<PathBuf> {
    let absolute = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()?.join(target)
    };
    let resolved = crate::path_form::resolve_target(&absolute)?;
    let mut digest = Sha256::new();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        // Preserve unpaired UTF-16 surrogates while folding case, so missing
        // leaves keep the same identity after creation with a different case.
        let identity = crate::path_form::strip_verbatim_os(resolved.as_os_str());
        for character in char::decode_utf16(identity.encode_wide()) {
            match character {
                Ok(character) => {
                    for lowercase in character.to_lowercase() {
                        for unit in lowercase.encode_utf16(&mut [0; 2]) {
                            digest.update(unit.to_le_bytes());
                        }
                    }
                }
                Err(error) => digest.update(error.unpaired_surrogate().to_le_bytes()),
            }
        }
    }
    #[cfg(not(windows))]
    digest.update(resolved.as_os_str().as_encoded_bytes());
    Ok(directory::path()?.join(format!("{:x}.lock", digest.finalize())))
}

fn open_lock(target: &Path) -> io::Result<(PathBuf, File)> {
    let path = lock_path_for(target)?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    Ok((path, file))
}

fn slots() -> &'static Mutex<HashMap<PathBuf, Weak<Slot>>> {
    static SLOTS: OnceLock<Mutex<HashMap<PathBuf, Weak<Slot>>>> = OnceLock::new();
    SLOTS.get_or_init(Mutex::default)
}

fn slot_for(path: PathBuf) -> Arc<Slot> {
    let mut slots = slots().lock().unwrap();
    if let Some(slot) = slots.get(&path).and_then(Weak::upgrade) {
        return slot;
    }
    let slot = Arc::new(Slot {
        path: path.clone(),
        state: Mutex::default(),
        released: Condvar::new(),
    });
    slots.insert(path, Arc::downgrade(&slot));
    slot
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut slots = slots().lock().unwrap();
        // Another thread may already have replaced our expired weak entry.
        if slots
            .get(&self.path)
            .is_some_and(|slot| std::ptr::eq(slot.as_ptr(), self))
        {
            slots.remove(&self.path);
        }
    }
}

pub fn lock_exclusive(target: &Path) -> io::Result<FileLock> {
    lock(target, Mode::Exclusive)
}

pub fn lock_shared(target: &Path) -> io::Result<FileLock> {
    lock(target, Mode::Shared)
}

fn lock(target: &Path, mode: Mode) -> io::Result<FileLock> {
    try_lock(target, mode, DEFAULT_LOCK_BUDGET)?.ok_or_else(|| {
        let label = match mode {
            Mode::Shared => "共享读锁",
            Mode::Exclusive => "独占写锁",
        };
        io::Error::new(
            io::ErrorKind::WouldBlock,
            format!(
                "等待 {DEFAULT_LOCK_BUDGET:?} 仍拿不到 {} 的{label}",
                target.display()
            ),
        )
    })
}

pub fn try_lock_exclusive(target: &Path, budget: Duration) -> io::Result<Option<FileLock>> {
    try_lock(target, Mode::Exclusive, budget)
}

pub fn try_lock_shared(target: &Path, budget: Duration) -> io::Result<Option<FileLock>> {
    try_lock(target, Mode::Shared, budget)
}

fn try_lock(target: &Path, requested: Mode, budget: Duration) -> io::Result<Option<FileLock>> {
    let started = Instant::now();
    let (path, file) = open_lock(target)?;
    acquire_opened(target, path, file, requested, budget, started)
}

fn acquire_opened(
    target: &Path,
    path: PathBuf,
    file: File,
    requested: Mode,
    budget: Duration,
    started: Instant,
) -> io::Result<Option<FileLock>> {
    let deadline = started + budget;
    let slot = slot_for(path.clone());
    let me = std::thread::current().id();
    let mut state = slot.state.lock().unwrap();
    let acquired = loop {
        match &mut *state {
            State::Exclusive { owner, depth, .. } if *owner == me => {
                *depth += 1;
                break Mode::Exclusive;
            }
            State::Shared { readers, .. } if requested == Mode::Shared => {
                *readers.entry(me).or_default() += 1;
                break Mode::Shared;
            }
            State::Shared { readers, .. } if readers.contains_key(&me) => {
                return Err(io::Error::new(
                    io::ErrorKind::Deadlock,
                    format!(
                        "{} 已被本线程以共享档持有；先释放共享锁，再取排他锁",
                        target.display()
                    ),
                ));
            }
            State::Unlocked => {
                let result = match requested {
                    Mode::Shared => file.try_lock_shared(),
                    Mode::Exclusive => file.try_lock(),
                };
                match result {
                    Ok(()) => {
                        *state = match requested {
                            Mode::Shared => State::Shared {
                                readers: HashMap::from([(me, 1)]),
                                _file: file,
                            },
                            Mode::Exclusive => State::Exclusive {
                                owner: me,
                                depth: 1,
                                _file: file,
                            },
                        };
                        break requested;
                    }
                    Err(TryLockError::WouldBlock) => {}
                    Err(TryLockError::Error(error)) => return Err(error),
                }
            }
            _ => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            tracing::debug!(target: "kanzei_file_lock", path=%path.display(), mode=?requested, wait_ms=started.elapsed().as_secs_f64()*1000.0, "file lock wait expired");
            return Ok(None);
        }
        state = slot
            .released
            .wait_timeout(state, remaining.min(POLL_INTERVAL))
            .unwrap()
            .0;
    };
    drop(state);
    let acquired_at = Instant::now();
    tracing::debug!(target: "kanzei_file_lock", path=%path.display(), mode=?acquired, wait_ms=started.elapsed().as_secs_f64()*1000.0, "file lock acquired");
    Ok(Some(FileLock {
        slot,
        mode: acquired,
        path,
        acquired_at,
        _not_send: std::marker::PhantomData,
    }))
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let mut state = self.slot.state.lock().unwrap();
        let empty = match (&mut *state, self.mode) {
            (State::Exclusive { owner, depth, .. }, Mode::Exclusive) => {
                assert_eq!(*owner, std::thread::current().id());
                *depth -= 1;
                *depth == 0
            }
            (State::Shared { readers, .. }, Mode::Shared) => {
                let me = std::thread::current().id();
                let count = readers.get_mut(&me).expect("共享持有者必须存在");
                *count -= 1;
                if *count == 0 {
                    readers.remove(&me);
                }
                readers.is_empty()
            }
            _ => unreachable!("FileLock 与持有状态不一致"),
        };
        if empty {
            *state = State::Unlocked;
        }
        drop(state);
        self.slot.released.notify_all();
        tracing::debug!(target: "kanzei_file_lock", path=%self.path.display(), mode=?self.mode, released=empty, hold_ms=self.acquired_at.elapsed().as_secs_f64()*1000.0, "file lock guard released");
    }
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
