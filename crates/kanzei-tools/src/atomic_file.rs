//! 整文件替换的共享原语:tmp+rename 原子写(R-138)。
//!
//! 仓里只能有**一套**原子写/文件锁实现。docstore、test_record 以及后续任何
//! "整读整写单个 markdown"的写入口都从这里取,不要各写各的——两套原语意味着
//! 两套失败语义,并发排查时没人说得清哪一份才是真的。
//!
//! 为什么必须原子:`std::fs::write` 是**先截断再写**。写到一半时另一个线程/
//! 进程读到的是零长度或半截文件,而 docstore 的 `load()` 对空文件宽容返回
//! `Ok(vec![])`——一次「成功但空」的快照就这样穿到前端(D-249 的第①层)。

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

/// rename 的重试次数与退避基数。Windows 上目标被杀软/索引器/编辑器短暂持有时
/// `MoveFileExW` 会失败,退避几十毫秒后基本都能过;超过这个预算说明是真占用,
/// 该把现场留给人看,而不是继续傻等。
const RENAME_ATTEMPTS: u32 = 6;
const RENAME_BACKOFF_MS: u64 = 20;

/// 同目录临时文件 + 原子替换。
///
/// 临时文件必须与目标**同目录**:跨卷 rename 不是原子操作,某些实现直接失败。
/// 名字带 pid + 纳秒,两个 kanzei 进程(kzapp / kz CLI / 自举循环)或同进程的
/// 两个线程不会互踩临时文件。
///
/// Windows 上 `std::fs::rename` 走 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`,
/// **同卷内可以原子覆盖已有目标**——不需要"先备份再改名"的三步走(architecture.rs
/// 里那句"Windows 不能原子覆盖已有目标"的注释是错的,别照抄)。
///
/// 失败时**保留临时文件**,理由见 [`cleanup_hint`]。
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} 没有父目录,无法在同目录建临时文件", path.display()),
            )
        })?;
    std::fs::create_dir_all(parent)?;
    let tmp = temp_sibling(path, parent);

    // create_new:名字撞上了宁可报错,也绝不覆盖别人在飞的临时文件。
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)?;
    if let Err(error) = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        // 这一步失败时临时文件里是半截内容,没有保留价值,删掉不丢任何东西。
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    drop(file);

    let mut last: Option<std::io::Error> = None;
    for attempt in 0..RENAME_ATTEMPTS {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last = Some(error);
                std::thread::sleep(Duration::from_millis(
                    RENAME_BACKOFF_MS * (attempt as u64 + 1),
                ));
            }
        }
    }
    Err(std::io::Error::other(cleanup_hint(path, &tmp, last)))
}

/// 替换失败时的错误文本:点名目标、临时文件与恢复动作。
///
/// 为什么**不删**临时文件(与 kanzei-llm/src/auth/store.rs 的取舍相反):凭证
/// 文件删了还能重新登录,而 tracker 文档的新内容是**内存里唯一的一份**——工具
/// 一返回就没了,删掉等于把用户/agent 这次编辑直接丢掉。原文件在任何一条失败
/// 路径上都未被触碰,所以"保留现场"是纯增益(R-138 验收④)。
fn cleanup_hint(path: &Path, tmp: &Path, last: Option<std::io::Error>) -> String {
    let reason = last.map(|e| e.to_string()).unwrap_or_default();
    format!(
        "原子替换 {} 失败: {reason}。新内容已完整写在 {},原文件未被破坏——\
         重试本次操作,或确认没有进程占用目标后把临时文件改名回去。",
        path.display(),
        tmp.display(),
    )
}

// ---------------------------------------------------------------------------
// 跨进程写锁(R-138 验收②)
// ---------------------------------------------------------------------------

/// 拿不到锁时的默认等待预算。写事务本身是毫秒级的,等到 3 秒还没轮到,
/// 说明对面不是"正在写"而是卡住了/崩了,该把确定的失败还给调用方。
pub const DEFAULT_LOCK_BUDGET: Duration = Duration::from_millis(3000);
/// 轮询间隔。独占句柄没有"等待"原语,只能轮询;5ms 对毫秒级事务足够细。
const LOCK_POLL_MS: u64 = 5;
/// 非 Windows 的锁文件陈旧判据(Windows 靠 OS 关句柄,不需要这条)。
#[cfg(not(windows))]
const LOCK_STALE_AFTER: Duration = Duration::from_secs(30);

/// 跨进程 + 跨线程的独占写锁。
///
/// **定位(设计基线 parallel_read_serial_write_orchestration.md §208):** 这把锁
/// **不是**运行级 writer 租约的替代品,不要拿它去做第二套调度。它保护的是租约
/// 看不见的入口——`kz` CLI 直通 tracker、自举循环、以及同一项目被两个 kzapp
/// 打开的场景。租约管"哪个 run 能写",这把锁管"两个 OS 进程别同时改同一个文件"。
///
/// **纪律:毫秒级持有。** 只包住「读—改—写」这一段,绝不跨 `.await`、绝不跨
/// LLM 调用。为此 `FileLock` 被刻意做成 `!Send`——谁想把它拿过 await 点,
/// 编译器会先拦下来。
///
/// 两层实现:
/// - **进程内**:按锁路径索引的可重入互斥(同线程重入计数,别的线程排队)。
///   独占句柄在同一进程内也会自我冲突,没有这一层,`archive_terminal` 内部
///   调 `save` 就会自锁死。
/// - **进程间**:Windows 用 `share_mode(0)` 开独占句柄,第二个进程 open 直接
///   拿到 ERROR_SHARING_VIOLATION;句柄随进程退出由 OS 关闭,**崩溃不留死锁**。
pub struct FileLock {
    key: String,
    /// `!Send` 标记:锁必须在获取它的线程上释放,也不允许跨 await 存活。
    _not_send: std::marker::PhantomData<*const ()>,
}

#[derive(Default)]
struct SlotState {
    owner: Option<ThreadId>,
    depth: usize,
    /// 独占句柄。`depth > 0` 且已完成获取时为 `Some`;置 `None` 即释放跨进程锁。
    handle: Option<std::fs::File>,
}

struct Registry {
    slots: Mutex<HashMap<String, SlotState>>,
    released: Condvar,
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Registry {
        slots: Mutex::new(HashMap::new()),
        released: Condvar::new(),
    })
}

/// 目标文件对应的锁文件:**同目录** `<stem>.lock`。
///
/// 同目录有两个理由:锁与被锁的东西一起搬(worktree/副本天然各锁各的),
/// 以及不需要额外知道项目根。锁文件是运行时产物,已进 .gitignore。
pub fn lock_path_for(target: &Path) -> PathBuf {
    match target.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => target.with_file_name(format!("{stem}.lock")),
        None => target.with_extension("lock"),
    }
}

/// 取独占锁,按默认预算等待;超时返回 `WouldBlock` 错误(带可读文本)。
pub fn lock_exclusive(target: &Path) -> std::io::Result<FileLock> {
    match try_lock_exclusive(target, DEFAULT_LOCK_BUDGET)? {
        Some(lock) => Ok(lock),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "等待 {:?} 仍拿不到 {} 的写锁:另一个 kanzei 进程正在写这个文件。\
                 稍后重试;若确认没有别的进程在跑,删掉锁文件即可。",
                DEFAULT_LOCK_BUDGET,
                target.display()
            ),
        )),
    }
}

/// 取独占锁,最多等 `budget`;等不到返回 `Ok(None)`(不是错误)。
///
/// 给"顺手做一下、做不成也无所谓"的路径用——典型是 UI 只读快照里那次幂等归档:
/// 拿不到锁就跳过,下次刷新再说,绝不能让文档面板为了一次归档卡住。
pub fn try_lock_exclusive(target: &Path, budget: Duration) -> std::io::Result<Option<FileLock>> {
    let path = lock_path_for(target);
    let key = lock_key(&path);
    let registry = registry();
    let me = std::thread::current().id();
    let deadline = Instant::now() + budget;

    let mut slots = registry.slots.lock().unwrap();
    loop {
        let state = slots.entry(key.clone()).or_default();
        if state.depth == 0 {
            // 先把槽位占住再放开注册表锁:OS 句柄可能要轮询几十毫秒,
            // 拿着全局锁去 sleep 会连累其它文件的加锁路径。
            state.owner = Some(me);
            state.depth = 1;
            drop(slots);
            let outcome = open_exclusive_until(&path, deadline);
            let mut slots = registry.slots.lock().unwrap();
            let state = slots.entry(key.clone()).or_default();
            return match outcome {
                Ok(Some(file)) => {
                    state.handle = Some(file);
                    drop(slots);
                    Ok(Some(FileLock {
                        key,
                        _not_send: std::marker::PhantomData,
                    }))
                }
                other => {
                    // 没拿到就得把槽位还回去,否则这个 key 在本进程里永久假占用。
                    state.owner = None;
                    state.depth = 0;
                    drop(slots);
                    registry.released.notify_all();
                    other.map(|_| None)
                }
            };
        }
        if state.owner == Some(me) {
            // 同线程重入:archive_terminal → save、tracker 事务 → void_id 都走这里。
            state.depth += 1;
            return Ok(Some(FileLock {
                key,
                _not_send: std::marker::PhantomData,
            }));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        let (guard, timeout) = registry
            .released
            .wait_timeout(slots, deadline - now)
            .unwrap();
        slots = guard;
        if timeout.timed_out() && Instant::now() >= deadline {
            return Ok(None);
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let registry = registry();
        let mut slots = registry.slots.lock().unwrap();
        if let Some(state) = slots.get_mut(&self.key) {
            state.depth = state.depth.saturating_sub(1);
            if state.depth == 0 {
                state.owner = None;
                // 关句柄 = 释放跨进程锁。必须在通知等待者之前发生,否则被唤醒的
                // 线程会立刻撞上还没关掉的句柄,白转一圈。
                // 锁文件本身留在盘上:删它会与另一个进程正在进行的 open 赛跑,
                // 而一个零字节的 .lock 文件没有任何成本(已进 .gitignore)。
                state.handle = None;
            }
        }
        drop(slots);
        registry.released.notify_all();
    }
}

/// 轮询式获取独占句柄,直到 `deadline`。`Ok(None)` = 一直被别的进程占着。
fn open_exclusive_until(path: &Path, deadline: Instant) -> std::io::Result<Option<std::fs::File>> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    loop {
        match open_exclusive(path) {
            Ok(file) => return Ok(Some(file)),
            Err(error) if is_contended(&error) => {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(LOCK_POLL_MS));
            }
            Err(error) => return Err(error),
        }
    }
}

/// 别的进程占着锁的错误形态。Windows 上共享冲突有三个码,漏一个就会把"正常
/// 争用"当成硬错误抛给用户。
fn is_contended(error: &std::io::Error) -> bool {
    if matches!(
        error.kind(),
        std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
    ) {
        return true;
    }
    // ERROR_ACCESS_DENIED(5) / ERROR_SHARING_VIOLATION(32) / ERROR_LOCK_VIOLATION(33)
    matches!(error.raw_os_error(), Some(5) | Some(32) | Some(33))
}

/// Windows:`share_mode(0)` = 不允许任何共享,第二个 open 直接失败。
/// 句柄由 OS 在进程退出时关闭,所以进程崩了不会留下死锁——这正是选独占句柄
/// 而不是"锁文件存在即上锁"的原因。
#[cfg(windows)]
fn open_exclusive(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(path)
}

/// 非 Windows:std 没有 flock 绑定,用 `O_EXCL` 锁文件顶上。
/// 代价是进程崩溃会留下陈旧锁文件,所以补一条按 mtime 的摘除规则。
/// 本仓主跑 Windows,这条分支的定位是"可编译且语义合理",不是主战场。
#[cfg(not(windows))]
fn open_exclusive(path: &Path) -> std::io::Result<std::fs::File> {
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
    {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let stale = std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .map(|at| {
                    at.elapsed()
                        .map(|age| age > LOCK_STALE_AFTER)
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if stale {
                let _ = std::fs::remove_file(path);
            }
            Err(error)
        }
        Err(error) => Err(error),
    }
}

/// 注册表键。Windows 上路径大小写不敏感、分隔符两可,不规范化就会出现
/// `C:\a\x.lock` 与 `c:/a/x.lock` 各占一把锁的假互斥(config.rs 的 dir_key
/// 踩过同一个坑)。
fn lock_key(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        raw.to_lowercase()
    } else {
        raw
    }
}

fn temp_sibling(path: &Path, parent: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("kanzei-atomic");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    parent.join(format!(".{name}.{}.{nanos}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 临时目录(标记: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-atomic-{标记}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn 原子写替换已有目标且不留临时文件() {
        let dir = 临时目录("replace");
        let path = dir.join("doc.md");
        write_atomic(&path, "第一版").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "第一版");
        // Windows 上覆盖已有目标同样走 MOVEFILE_REPLACE_EXISTING,不需要先删。
        write_atomic(&path, "第二版内容更长").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "第二版内容更长");

        let 残留: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(残留.is_empty(), "成功路径不该留临时文件");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 临时文件与目标同目录() {
        // 跨卷 rename 会失败,这条不变量塌了原子写就整体失效。
        let dir = 临时目录("sibling");
        let path = dir.join("doc.md");
        let tmp = temp_sibling(&path, &dir);
        assert_eq!(tmp.parent(), path.parent());
        assert!(tmp
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(&std::process::id().to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 父目录不存在时自动创建() {
        let dir = 临时目录("mkdir");
        let path = dir.join("a").join("b").join("doc.md");
        write_atomic(&path, "内容").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "内容");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 跨进程互斥的**机械证据**:同一个锁路径上第二个独占句柄必须拿不到。
    /// 没有这一条,上面那些并发用例证明的都只是进程内互斥。
    #[cfg(windows)]
    #[test]
    fn 独占句柄第二次打开必然失败() {
        let dir = 临时目录("exclusive");
        let 锁 = lock_path_for(&dir.join("doc.md"));
        assert_eq!(锁.file_name().unwrap(), "doc.lock");
        assert_eq!(锁.parent(), Some(dir.as_path()), "锁文件必须与目标同目录");

        let 第一个 = open_exclusive(&锁).unwrap();
        let 第二个 = open_exclusive(&锁).unwrap_err();
        assert!(
            is_contended(&第二个),
            "第二个句柄的失败必须被识别成争用而不是硬错误: {第二个:?} raw={:?}",
            第二个.raw_os_error()
        );
        drop(第一个);
        // 关掉句柄即释放——句柄由 OS 托管,所以进程崩溃也不会留下死锁。
        open_exclusive(&锁).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 锁同线程可重入而其它线程排队() {
        let dir = 临时目录("reentrant");
        let target = dir.join("doc.md");

        let 外层 = try_lock_exclusive(&target, DEFAULT_LOCK_BUDGET)
            .unwrap()
            .unwrap();
        // archive_terminal → save、tracker 事务 → void_id 都是这个形态:
        // 不可重入的话内层直接自锁死。
        let 内层 = try_lock_exclusive(&target, DEFAULT_LOCK_BUDGET).unwrap();
        assert!(内层.is_some(), "同线程重入必须放行");
        drop(内层);

        // 外层还持有:别的线程在预算内拿不到。
        let 旁人 = {
            let target = target.clone();
            std::thread::spawn(move || {
                try_lock_exclusive(&target, Duration::from_millis(80))
                    .unwrap()
                    .is_none()
            })
        };
        assert!(旁人.join().unwrap(), "重入计数没归零时别的线程不该拿到锁");

        drop(外层);
        let 旁人 = {
            let target = target.clone();
            std::thread::spawn(move || {
                try_lock_exclusive(&target, DEFAULT_LOCK_BUDGET)
                    .unwrap()
                    .is_some()
            })
        };
        assert!(旁人.join().unwrap(), "释放后必须能被别的线程拿到");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 限时取锁拿不到时返回空而不是错误() {
        // docs_snapshot 那次幂等归档靠这条语义:拿不到就跳过,绝不让面板卡住。
        let dir = 临时目录("try-lock");
        let target = dir.join("doc.md");
        let 持有 = try_lock_exclusive(&target, DEFAULT_LOCK_BUDGET)
            .unwrap()
            .unwrap();
        // FileLock 是 !Send,拿不出线程边界——这本身就是"绝不跨 await/跨线程持有"
        // 那条纪律的机械保证,所以这里在线程内就把结论收敛成布尔。
        let 旁人 = {
            let target = target.clone();
            std::thread::spawn(move || {
                try_lock_exclusive(&target, Duration::from_millis(50)).map(|lock| lock.is_none())
            })
        };
        let 结果 = 旁人.join().unwrap();
        assert!(结果.is_ok(), "争用不是错误");
        assert!(结果.unwrap(), "等不到就返回 None");
        drop(持有);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-138 验收④:替换失败要保留现场可重试——原文件逐字不动,新内容留在临时文件里。
    #[cfg(windows)]
    #[test]
    fn 替换失败时保留临时文件且原文件不被破坏() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = 临时目录("locked");
        let path = dir.join("doc.md");
        write_atomic(&path, "原始内容").unwrap();

        // share_mode(0) = 独占,rename 无法替换被这样打开的目标(模拟杀软/编辑器占用)。
        let 占用 = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();

        let error = write_atomic(&path, "新内容").unwrap_err().to_string();
        assert!(error.contains("原子替换"), "错误要点名操作: {error}");
        assert!(error.contains(".tmp"), "错误要点名临时文件位置: {error}");
        assert!(
            error.contains("原文件未被破坏"),
            "要告诉调用方现场是干净的: {error}"
        );

        drop(占用);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "原始内容",
            "失败路径绝不能碰原文件"
        );
        let 留证: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert_eq!(留证.len(), 1, "新内容必须留在临时文件里: {留证:?}");
        assert_eq!(
            std::fs::read_to_string(dir.join(&留证[0])).unwrap(),
            "新内容"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
