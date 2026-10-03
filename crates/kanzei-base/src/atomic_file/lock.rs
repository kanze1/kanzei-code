//! OS 文件锁负责进程间互斥；每路径的槽只负责线程重入和共享计数。
//! 非阻塞获取在槽内完成，等待时释放槽，不需要 acquiring 中间态。

use std::collections::HashMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

pub const DEFAULT_LOCK_BUDGET: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// 锁只在获取它的线程上释放；读改写事务不得跨 await 或耗时外部调用。
pub struct FileLock {
    slot: Arc<Slot>,
    mode: Mode,
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

#[derive(Default)]
struct Slot {
    state: Mutex<State>,
    released: Condvar,
}

/// 保持既有同目录 `<stem>.lock` 命名；锁文件始终留存，不能靠删文件解锁。
pub fn lock_path_for(target: &Path) -> PathBuf {
    target.with_extension("lock")
}

/// 解析目标链接；目标尚未创建时解析父目录。只有 NotFound 表示合法的首次创建。
fn open_lock(target: &Path) -> io::Result<(PathBuf, File)> {
    let target = std::path::absolute(target)?;
    let resolved = match target.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = target
                .parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "锁目标没有父目录"))?;
            std::fs::create_dir_all(parent)?;
            let name = target
                .file_name()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "锁目标没有文件名"))?;
            parent.canonicalize()?.join(name)
        }
        Err(error) => return Err(error),
    };
    let path = lock_path_for(&resolved);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    Ok((path.canonicalize()?, file))
}

fn slot_for(path: PathBuf) -> Arc<Slot> {
    static SLOTS: OnceLock<Mutex<HashMap<PathBuf, Arc<Slot>>>> = OnceLock::new();
    SLOTS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap()
        .entry(path)
        .or_default()
        .clone()
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
    let deadline = Instant::now() + budget;
    let (path, file) = open_lock(target)?;
    let slot = slot_for(path);
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
            return Ok(None);
        }
        state = slot
            .released
            .wait_timeout(state, remaining.min(POLL_INTERVAL))
            .unwrap()
            .0;
    };
    drop(state);
    Ok(Some(FileLock {
        slot,
        mode: acquired,
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
    }
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
