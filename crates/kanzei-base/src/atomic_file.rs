//! 整文件替换的共享原语:tmp+rename 原子写(R-138)。
//!
//! 仓里只能有**一套**原子写/文件锁实现。docstore、test_record、memory、files、
//! auth/store 以及后续任何"整读整写单个 markdown/json"的写入口都从这里取,不要
//! 各写各的——两套原语意味着两套失败语义,并发排查时没人说得清哪一份才是真的。
//!
//! 为什么必须原子:`std::fs::write` 是**先截断再写**。写到一半时另一个线程/
//! 进程读到的是零长度或半截文件,而 docstore 的 `load()` 对空文件宽容返回
//! `Ok(vec![])`——一次「成功但空」的快照就这样穿到前端(D-249 的第①层)。
//!
//! 本模块原寄居 kanzei-llm(D-261:llm 是依赖图最底层,当时只有放这里才能让
//! 最底层共用同一套)。R-208 拆出 kanzei-base 零依赖 crate 承接:llm/tools 从
//! 这里取,harness 也能直接依赖(否则 R-181 的跨进程 lease 契约在 harness、
//! 原语在 llm,照单实施会撞依赖方向墙)。纯搬迁零行为变更。

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

/// CAS 变体:内容指纹匹配才替换(D-261 并轨 architecture.rs 的 replace_recoverably)。
///
/// 写临时文件后、rename 前校验**目标当前内容**的指纹仍等于 `expected_hash`,
/// 不匹配则**放弃替换**并返回错误(目标原样未动)——调用方拿到错误后应重新
/// get,而不是把并发期间的改动盖掉。返回 `Result<(), String>` 与旧
/// `replace_recoverably` 保持一致,conventions/architecture 调用点无需改签名。
///
/// 成功路径与 [`write_atomic`] 相同:同目录临时文件 + 原子 rename,Windows 上
/// 可原子覆盖已有目标,不需要 backup 三步走。失败路径保留临时文件供排查。
pub fn write_atomic_cas(
    path: &Path,
    content: &str,
    expected_hash: &str,
    hash_of: impl Fn(&str) -> String,
) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| format!("{} 没有父目录,无法在同目录建临时文件", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let tmp = temp_sibling(path, parent);

    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)
        .map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
    if let Err(error) = file
        .write_all(content.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cannot persist {}: {error}", tmp.display()));
    }
    drop(file);

    let live = std::fs::read_to_string(path).unwrap_or_default();
    let live_hash = hash_of(&live);
    if live_hash != expected_hash {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "stale expected_hash `{expected_hash}` during final write; the file is now `{live_hash}`. Nothing was replaced. Re-run `get`."
        ));
    }

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
    let reason = last.map(|e| e.to_string()).unwrap_or_default();
    Err(format!(
        "原子替换 {} 失败: {reason}。新内容已完整写在 {},原文件未被破坏——\
         重试本次操作,或确认没有进程占用目标后把临时文件改名回去。",
        path.display(),
        tmp.display(),
    ))
}

/// 替换失败时的错误文本:点名目标、临时文件与恢复动作。
///
/// 为什么**不删**临时文件(与 kanzei-llm/src/auth/store.rs 的旧取舍相反):凭证
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
    mode: LockMode,
    /// `!Send` 标记:锁必须在获取它的线程上释放,也不允许跨 await 存活。
    _not_send: std::marker::PhantomData<*const ()>,
}

/// D-382:锁模式。加共享档的理由不是"更快",是**保住并行**。
///
/// 原先只有排他一档,于是三类互不冲突的动作被迫排成一队:
///   - bash 围栏(D-364)要在命令窗口内挡住托管文档的写者,于是持全部 8 份文档的
///     排他锁,**持有到命令结束**(默认 120s、上限 600s);
///   - 另一条并行线的 bash 围栏要的是同一件事,两者之间本无冲突,却互斥;
///   - 桌面端只是**读**文档(D-338 为避开 rename/open 竞态也取了排他锁)。
///
/// 实测后果:一条线跑 `cargo check`,另一条线的 bash 按 500ms 预算取锁必然失败
/// (持锁 600s vs 抢锁 500ms = 1200:1),文档面板按 3s 预算也拿不到——两条线互为
/// 对方的阻塞源,不会自愈。
///
/// 共享档让"读者与围栏"彼此共存,只把**真正的写者**挡在外面,两条不变式都不松:
/// D-364 要的"窗口内没有写者"仍然成立(写者要排他),D-338 要的"读者看不到 rename
/// 中间态"也仍然成立(写者持排他时读者等)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LockMode {
    Shared,
    Exclusive,
}

#[derive(Default)]
struct SlotState {
    owner: Option<ThreadId>,
    depth: usize,
    /// 共享持有者:线程 → 重入计数。非空 = 本进程有人持共享档。
    shared: HashMap<ThreadId, usize>,
    /// 有线程已占住槽位、正在开 OS 句柄。此刻 `handle` 还是 `None`,但锁**不是**
    /// 空闲的——后来者必须等它落定,不能把"句柄还没开"误判成"没人持锁"。
    acquiring: bool,
    /// OS 句柄。排他档是 `share_mode(0)`;共享档由第一个共享持有者开、最后一个
    /// 释放者关。置 `None` 即释放跨进程锁。
    handle: Option<std::fs::File>,
}

impl SlotState {
    /// 本进程内是否有人持锁(含正在获取)。
    fn busy(&self) -> bool {
        self.depth > 0 || !self.shared.is_empty() || self.acquiring
    }
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
        if !state.busy() {
            // 先把槽位占住再放开注册表锁:OS 句柄可能要轮询几十毫秒,
            // 拿着全局锁去 sleep 会连累其它文件的加锁路径。
            state.owner = Some(me);
            state.depth = 1;
            state.acquiring = true;
            drop(slots);
            let outcome = open_exclusive_until(&path, deadline);
            let mut slots = registry.slots.lock().unwrap();
            let state = slots.entry(key.clone()).or_default();
            state.acquiring = false;
            return match outcome {
                Ok(Some(file)) => {
                    state.handle = Some(file);
                    drop(slots);
                    Ok(Some(FileLock {
                        key,
                        mode: LockMode::Exclusive,
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
                mode: LockMode::Exclusive,
                _not_send: std::marker::PhantomData,
            }));
        }
        if state.shared.contains_key(&me) {
            // 共享升排他会自锁:本线程的共享句柄(share_mode=READ)正好挡住自己的
            // 排他 open,再等也等不到。快速失败并说清怎么改,好过等到预算耗尽只报
            // 一句"另一个进程在写"——那会把调用点的真实错误藏起来。
            return Err(std::io::Error::new(
                std::io::ErrorKind::Deadlock,
                format!(
                    "{} 已被本线程以共享档持有,不能在其上升级为排他锁:\
                     先释放共享锁再取排他,或把外层直接改成排他档。",
                    target.display()
                ),
            ));
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

/// 取共享锁,按默认预算等待;超时返回 `WouldBlock`(带可读文本)。
pub fn lock_shared(target: &Path) -> std::io::Result<FileLock> {
    match try_lock_shared(target, DEFAULT_LOCK_BUDGET)? {
        Some(lock) => Ok(lock),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "等待 {:?} 仍拿不到 {} 的读锁:有进程正持排他写锁。\
                 稍后重试;若确认没有别的进程在跑,删掉锁文件即可。",
                DEFAULT_LOCK_BUDGET,
                target.display()
            ),
        )),
    }
}

/// 取共享锁,最多等 `budget`;等不到返回 `Ok(None)`。
///
/// 共享档之间彼此相容,只与排他档互斥。用于**不改内容**的持有者:文档读取
/// (`DocStore::load`)与 bash 围栏(它要的是"窗口内没有写者",不是"没有别的围栏")。
/// 见 [`LockMode`] 的说明。
pub fn try_lock_shared(target: &Path, budget: Duration) -> std::io::Result<Option<FileLock>> {
    let path = lock_path_for(target);
    let key = lock_key(&path);
    let registry = registry();
    let me = std::thread::current().id();
    let deadline = Instant::now() + budget;

    let mut slots = registry.slots.lock().unwrap();
    loop {
        let state = slots.entry(key.clone()).or_default();
        if state.owner == Some(me) {
            // 已持排他:共享请求当排他重入处理(排他本就更强),释放时减 depth。
            state.depth += 1;
            return Ok(Some(FileLock {
                key,
                mode: LockMode::Exclusive,
                _not_send: std::marker::PhantomData,
            }));
        }
        if state.depth == 0 && !state.acquiring {
            if state.shared.is_empty() {
                // 本进程第一个共享持有者:要真去开 OS 句柄。占住槽位再放锁。
                state.shared.insert(me, 1);
                state.acquiring = true;
                drop(slots);
                let outcome = open_shared_until(&path, deadline);
                let mut slots = registry.slots.lock().unwrap();
                let state = slots.entry(key.clone()).or_default();
                state.acquiring = false;
                return match outcome {
                    Ok(Some(file)) => {
                        state.handle = Some(file);
                        drop(slots);
                        Ok(Some(FileLock {
                            key,
                            mode: LockMode::Shared,
                            _not_send: std::marker::PhantomData,
                        }))
                    }
                    other => {
                        state.shared.remove(&me);
                        drop(slots);
                        registry.released.notify_all();
                        other.map(|_| None)
                    }
                };
            }
            // 已有共享持有者:句柄现成的,直接挂号,不再碰 OS。
            *state.shared.entry(me).or_insert(0) += 1;
            return Ok(Some(FileLock {
                key,
                mode: LockMode::Shared,
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
            match self.mode {
                LockMode::Exclusive => {
                    state.depth = state.depth.saturating_sub(1);
                    if state.depth == 0 {
                        state.owner = None;
                    }
                }
                LockMode::Shared => {
                    let me = std::thread::current().id();
                    if let Some(count) = state.shared.get_mut(&me) {
                        *count -= 1;
                        if *count == 0 {
                            state.shared.remove(&me);
                        }
                    }
                }
            }
            // 关句柄 = 释放跨进程锁。必须在通知等待者之前发生,否则被唤醒的
            // 线程会立刻撞上还没关掉的句柄,白转一圈。
            // 共享档要等**最后一个**持有者走才关:句柄是全体共享持有者共用的。
            // 锁文件本身留在盘上:删它会与另一个进程正在进行的 open 赛跑,
            // 而一个零字节的 .lock 文件没有任何成本(已进 .gitignore)。
            if state.depth == 0 && state.shared.is_empty() {
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

/// 轮询式获取共享句柄,直到 `deadline`。`Ok(None)` = 一直被排他持有者占着。
fn open_shared_until(path: &Path, deadline: Instant) -> std::io::Result<Option<std::fs::File>> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    loop {
        match open_shared(path) {
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

/// Windows:只要**读**权限 + `FILE_SHARE_READ`。
///
/// 两条都不能松:
/// - 若一并请求写权限,后来的共享者会被拒——它的 share_mode 不允许已有句柄的写访问;
/// - 若 share_mode 放开写,排他者(share_mode=0)仍会被拒(它不允许任何既有句柄),
///   但语义就从"读写锁"退化成"谁先到谁赢",失去意义。
///
/// 反向也自动成立:排他者持 `share_mode(0)` 时,任何共享 open 都被 OS 拒绝。
#[cfg(windows)]
fn open_shared(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    match std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
    {
        Ok(file) => Ok(file),
        // 锁文件还没被谁建过:先建出来(建的这一下允许共享,免得两个共享者互相挤掉),
        // 再按共享档打开。排他者此刻若在场,这一步会以争用形态失败并回到轮询。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .open(path)?;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .open(path)
        }
        Err(error) => Err(error),
    }
}

/// 非 Windows:std 没有 flock 绑定,`O_EXCL` 锁文件表达不了共享档。
/// 降级为排他——**语义仍然正确**(共享档的约束比排他弱,用强的顶替不会放过任何
/// 该挡的),只是并发度退回改造前。本仓主跑 Windows,这条分支的定位与
/// [`open_exclusive`] 的注释一致:可编译且语义合理,不是主战场。
#[cfg(not(windows))]
fn open_shared(path: &Path) -> std::io::Result<std::fs::File> {
    open_exclusive(path)
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

    fn 指纹(s: &str) -> String {
        format!("hash:{}", s.len())
    }

    // ---- D-382 共享档语义 ----
    // 这一组钉的是「读者/围栏彼此共存、写者仍被完全挡住」。每条都直指改造前那个
    // 活锁:一条线的 bash 围栏持排他 600s,另一条线按 500ms 预算必然拿不到。

    #[test]
    fn 共享档彼此相容_跨线程也相容() {
        let dir = 临时目录("shared-compat");
        let target = dir.join("doc.md");
        let a = try_lock_shared(&target, Duration::from_millis(50))
            .unwrap()
            .expect("首个共享档应立刻拿到");
        // 同线程再取一次:重入计数,不自锁。
        let b = try_lock_shared(&target, Duration::from_millis(50))
            .unwrap()
            .expect("同线程共享重入应立刻拿到");
        // 另一个线程同时取共享:这正是「两条线的 bash 围栏」的形态。
        let target2 = target.clone();
        let 另一条线 = std::thread::spawn(move || {
            try_lock_shared(&target2, Duration::from_millis(200))
                .unwrap()
                .is_some()
        });
        assert!(
            另一条线.join().unwrap(),
            "另一线程取共享档失败:围栏之间又互斥了(D-382 的活锁就是这么来的)"
        );
        drop(b);
        drop(a);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 排他档挡住共享档_释放后共享立刻拿到() {
        let dir = 临时目录("excl-blocks-shared");
        let target = dir.join("doc.md");
        let 写锁 = lock_exclusive(&target).unwrap();
        let target2 = target.clone();
        let 读者 = std::thread::spawn(move || {
            // 写者在场时拿不到(写者是毫秒级的,这里给 80ms 预算足够表达"被挡住")。
            let 被挡 = try_lock_shared(&target2, Duration::from_millis(80))
                .unwrap()
                .is_none();
            // 写者走后必须能拿到——释放走的是 notify_all 广播,不是轮询。
            let 之后 = try_lock_shared(&target2, Duration::from_secs(2)).unwrap();
            (被挡, 之后.is_some())
        });
        std::thread::sleep(Duration::from_millis(200));
        drop(写锁);
        let (被挡, 之后拿到) = 读者.join().unwrap();
        assert!(
            被挡,
            "排他档在场时共享档不该拿到:D-338 的 rename 中间态会漏出来"
        );
        assert!(之后拿到, "排他档释放后共享档仍拿不到:广播唤醒没生效");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 共享档挡住排他档_最后一个释放后写者才进() {
        let dir = 临时目录("shared-blocks-excl");
        let target = dir.join("doc.md");
        let 读锁一 = try_lock_shared(&target, Duration::from_millis(50))
            .unwrap()
            .unwrap();
        let 读锁二 = try_lock_shared(&target, Duration::from_millis(50))
            .unwrap()
            .unwrap();
        let target2 = target.clone();
        let 写者 = std::thread::spawn(move || {
            let 被挡 = try_lock_exclusive(&target2, Duration::from_millis(80))
                .unwrap()
                .is_none();
            let 之后 = try_lock_exclusive(&target2, Duration::from_secs(2)).unwrap();
            (被挡, 之后.is_some())
        });
        std::thread::sleep(Duration::from_millis(200));
        drop(读锁一);
        // 只走了一个共享持有者,写者仍然必须等——句柄是全体共用的。
        std::thread::sleep(Duration::from_millis(100));
        drop(读锁二);
        let (被挡, 之后拿到) = 写者.join().unwrap();
        assert!(被挡, "有共享持有者时排他档不该拿到:D-364 的窗口就失守了");
        assert!(之后拿到, "共享档全部释放后排他档仍拿不到");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 排他持有者内部取共享算重入不自锁() {
        let dir = 临时目录("excl-reenter-shared");
        let target = dir.join("doc.md");
        let 外层 = lock_exclusive(&target).unwrap();
        // archive_terminal(持排他)内部调 load(取共享)就是这条路径。
        let 内层 = try_lock_shared(&target, Duration::from_millis(50))
            .unwrap()
            .expect("排他持有者内部取共享应按重入放行,否则自锁死");
        drop(内层);
        drop(外层);
        // 全部释放后别的线程应能立刻取排他。
        let target2 = target.clone();
        assert!(
            std::thread::spawn(move || try_lock_exclusive(&target2, Duration::from_secs(1))
                .unwrap()
                .is_some())
            .join()
            .unwrap(),
            "重入释放后锁没还干净"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 共享升排他快速失败而不是等到超时() {
        let dir = 临时目录("no-upgrade");
        let target = dir.join("doc.md");
        let _读锁 = try_lock_shared(&target, Duration::from_millis(50))
            .unwrap()
            .unwrap();
        let 起点 = Instant::now();
        let 结果 = try_lock_exclusive(&target, Duration::from_secs(5));
        let 用时 = 起点.elapsed();
        let error = match 结果 {
            Err(error) => error,
            Ok(_) => panic!("同线程共享升排他必须报错,不能装作在等"),
        };
        assert!(
            error.to_string().contains("共享档持有"),
            "错误文本要说清是升级自锁,实得:{error}"
        );
        assert!(
            用时 < Duration::from_secs(1),
            "应当快速失败,实际等了 {用时:?}——那会把真实原因藏在超时后面"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 绕过进程内注册表,直接验 OS 句柄层的读写锁语义——跨进程争用最终落在这里,
    /// 注册表那层挡不住别的 kanzei 进程。
    #[test]
    fn os句柄层共享与排他互斥() {
        let dir = 临时目录("os-share-mode");
        let lock_file = dir.join("doc.lock");
        let 共享一 = open_shared(&lock_file).expect("首个共享句柄");
        let 共享二 = open_shared(&lock_file).expect("第二个共享句柄应能共存");
        assert!(
            open_exclusive(&lock_file).is_err(),
            "有共享句柄时排他 open 必须被 OS 拒绝"
        );
        drop(共享一);
        drop(共享二);
        let 排他 = open_exclusive(&lock_file).expect("共享句柄全关后排他应可开");
        // 非 Windows 分支里 open_shared 就是 open_exclusive,自然互斥;
        // Windows 上靠 share_mode(0) 拒绝。两条路径都必须挡住。
        assert!(
            open_shared(&lock_file).is_err(),
            "有排他句柄时共享 open 必须被 OS 拒绝"
        );
        drop(排他);
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn 指纹匹配时替换_不匹配时放弃且原文件不动() {
        let dir = 临时目录("cas");
        let path = dir.join("doc.md");
        write_atomic(&path, "旧内容").unwrap();

        // 指纹匹配:替换成功。
        write_atomic_cas(&path, "新内容", &指纹("旧内容"), 指纹).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "新内容");

        // 指纹不匹配:放弃替换,原文件保持"新内容"。
        let err = write_atomic_cas(&path, "覆盖内容", &指纹("别的"), 指纹).unwrap_err();
        assert!(
            err.contains("stale expected_hash"),
            "要点名 CAS 失败: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "新内容",
            "CAS 失败路径绝不能碰原文件"
        );
        // 临时文件被清理:内容没保留价值(目标已是并发方的新版本)。
        let 残留: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(残留.is_empty(), "CAS 放弃后不该留临时文件");
        std::fs::remove_dir_all(&dir).ok();
    }
}
