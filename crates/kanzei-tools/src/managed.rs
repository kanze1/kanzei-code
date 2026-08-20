//! 托管文档围栏(D-173/R-139 前台版、D-174 后台版共用)。
//!
//! 托管目录只能由专用工具写入。"能不能绕过"绝不能靠猜命令文本——
//! `[System.IO.File]::WriteAllText`、重定向、python/node 一行流、`git checkout`
//! 单文件都能避开任何字符串匹配,所以本模块一律走**结果侧**判定:
//! 动作之前拍下托管目录的镜像,动作之后再比一次,改了就隔离留证 + 整体回滚。
//!
//! 前台(bash 同步命令)与后台(D-174 受管后台任务)共用同一套快照/回滚口径,
//! 仓库里不存在第二套"不能写入"的语义。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 托管目录:write/edit 对它们硬 deny,shell 也不许绕过去改(D-173)。
pub(crate) const MANAGED_ROOTS: &[&str] = &[".kanzei/project", ".kanzei/memory"];
/// 单文件镜像上限:超过就只记指纹,能检测但无法回滚(会如实说明)。
pub(crate) const MANAGED_SNAPSHOT_FILE_LIMIT: u64 = 4 * 1024 * 1024;
/// 镜像文件数上限,防止有人往托管目录塞进一整棵大树把每次 bash 拖垮。
pub(crate) const MANAGED_SNAPSHOT_MAX_FILES: usize = 2000;

/// 托管目录的镜像。`None` 内容 = 文件超过镜像上限,只能检测不能回滚。
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ManagedSnapshot {
    files: BTreeMap<String, Option<Vec<u8>>>,
    /// 目录规模超限时放弃镜像:此时既不检测也不回滚,并在输出里如实说明。
    truncated: bool,
}

impl ManagedSnapshot {
    pub(crate) fn capture(project_root: &Path) -> Self {
        let mut snapshot = ManagedSnapshot::default();
        for root in MANAGED_ROOTS {
            collect_files(&project_root.join(root), project_root, &mut snapshot);
            if snapshot.truncated {
                break;
            }
        }
        snapshot
    }

    pub(crate) fn is_complete(&self) -> bool {
        !self.truncated && self.files.values().all(Option::is_some)
    }

    /// 把 `current` 里 `paths` 指定的这些路径吸收进本基线(其余路径保持原样)。
    ///
    /// D-258:吸收粒度从「前缀内全部路径」收窄为「窗口期间实际变化的路径」。
    /// 整前缀吸收会让后台进程在窗口内偷写的文件(专用工具没碰、窗口前后却有变化)
    /// 也被固化进基线;按路径吸收只认「打开/关闭两次镜像之间的差异」,后台进程
    /// 必须和专用工具写同一批文件才能蒙混——写别的文件会在窗口关闭后留痕,
    /// 被守卫下一轮对账判成越界回滚。
    ///
    /// `paths` 里在 `current` 中不存在的路径视为被删除(窗口内被删的托管文件同样
    /// 要吸收,否则守卫会把"专用工具的归档移动"当成越界删除再给写回去)。
    pub(crate) fn absorb_paths(&mut self, current: &ManagedSnapshot, paths: &[&str]) {
        for &path in paths {
            match current.files.get(path) {
                Some(content) => {
                    self.files.insert(path.to_string(), content.clone());
                }
                None => {
                    self.files.remove(path);
                }
            }
        }
    }
}

/// 两次镜像之间的托管变更集。
#[derive(Debug, Default, Clone)]
pub(crate) struct ManagedChange {
    pub(crate) modified: Vec<String>,
    pub(crate) created: Vec<String>,
    pub(crate) deleted: Vec<String>,
}

impl ManagedChange {
    pub(crate) fn is_empty(&self) -> bool {
        self.modified.is_empty() && self.created.is_empty() && self.deleted.is_empty()
    }

    /// 全部被触碰的路径(修改 + 新建 + 删除)。
    pub(crate) fn touched(&self) -> Vec<&String> {
        self.modified
            .iter()
            .chain(self.created.iter())
            .chain(self.deleted.iter())
            .collect()
    }

    /// 按谓词切成两半:`keep` 为真的进左边,其余进右边。
    /// 后台守卫用它把"窗口正开着、有合法解释"的路径与真正的越界分流。
    pub(crate) fn partition(&self, keep: impl Fn(&str) -> bool) -> (ManagedChange, ManagedChange) {
        let split = |paths: &[String]| -> (Vec<String>, Vec<String>) {
            paths.iter().cloned().partition(|path| keep(path))
        };
        let (kept_modified, rest_modified) = split(&self.modified);
        let (kept_created, rest_created) = split(&self.created);
        let (kept_deleted, rest_deleted) = split(&self.deleted);
        (
            ManagedChange {
                modified: kept_modified,
                created: kept_created,
                deleted: kept_deleted,
            },
            ManagedChange {
                modified: rest_modified,
                created: rest_created,
                deleted: rest_deleted,
            },
        )
    }
}

/// 比对两次镜像。无差异返回 None。
pub(crate) fn diff(before: &ManagedSnapshot, after: &ManagedSnapshot) -> Option<ManagedChange> {
    let mut change = ManagedChange::default();
    for (path, content) in &after.files {
        match before.files.get(path) {
            None => change.created.push(path.clone()),
            Some(old) if old != content => change.modified.push(path.clone()),
            Some(_) => {}
        }
    }
    for path in before.files.keys() {
        if !after.files.contains_key(path) {
            change.deleted.push(path.clone());
        }
    }
    (!change.is_empty()).then_some(change)
}

/// 隔离留证 + 回滚到 `before`。返回(留证目录, 实际回滚成功的文件数)。
///
/// 先隔离再回滚:哪怕这次改动其实来自用户手改,内容也一份不丢,可原样取回。
pub(crate) fn quarantine_and_restore(
    project_root: &Path,
    before: &ManagedSnapshot,
    change: &ManagedChange,
    tag: &str,
) -> (std::path::PathBuf, usize) {
    let quarantine = project_root.join(".kanzei/quarantine").join(format!(
        "{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default()
    ));
    let mut restored = 0usize;
    for path in change.modified.iter().chain(change.created.iter()) {
        let absolute = project_root.join(path);
        let saved = quarantine.join(path);
        if let Some(parent) = saved.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(&absolute, &saved);
        match before.files.get(path) {
            // 动作前存在:写回原内容(内容超限没镜像的只能保持现状,由调用方点名)。
            Some(Some(original)) => {
                // D-399:该路径若有合法写日志(同路径「先合法写后越界写」),回滚到
                // 最后一次合法日志内容,而不是窗口开点(窗口开点会丢掉合法写)。
                let restore_content = crate::write_log::last_content(project_root, path)
                    .unwrap_or_else(|| original.clone());
                if std::fs::write(&absolute, restore_content).is_ok() {
                    restored += 1;
                }
            }
            Some(None) => {}
            // 动作前不存在:删掉新建的。
            None => {
                if std::fs::remove_file(&absolute).is_ok() {
                    restored += 1;
                }
            }
        }
    }
    for path in &change.deleted {
        if let Some(Some(original)) = before.files.get(path) {
            let absolute = project_root.join(path);
            // D-399:被删文件若有合法写日志,回滚到合法终态(同先合法写后越界删)。
            let restore_content = crate::write_log::last_content(project_root, path)
                .unwrap_or_else(|| original.clone());
            if let Some(parent) = absolute.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&absolute, restore_content).is_ok() {
                restored += 1;
            }
        }
    }
    (quarantine, restored)
}

/// 托管文档的**活动文件**锁清单(D-364)。
///
/// 每类文档一把锁、键取自活动文件——docstore「一个 kind 一把锁,键取自活动文档
/// 路径,同时罩住活动文件、归档文件与编号账本」、test_record 同源。所以这里只锁
/// 活动文件,归档/`-archive.md`/账本的写者走的都是同一把锁。
///
/// 对 tracker/tests/conventions/architecture 四族,本清单已完整覆盖;source/finding
/// 零写入方(见 docs.rs D-296 注释),不入列。`.kanzei/memory/` 下的**动态条目文件**
/// 创建前无法预锁,由 D-368 的 memory 树锁(见 [`memory_tree_lock_path`])整树罩住。
fn known_active_doc_paths(project_root: &Path) -> Vec<PathBuf> {
    use crate::architecture::ARCHITECTURE_REL;
    use crate::conventions::CONVENTIONS_REL;
    use crate::docstore::{DECISIONS, DEFECTS, IDEAS, MEMORY, REQUIREMENTS};
    use crate::test_record::TEST_RUNS_REL;
    [
        REQUIREMENTS.rel_path,
        DEFECTS.rel_path,
        IDEAS.rel_path,
        DECISIONS.rel_path,
        MEMORY.rel_path,
        TEST_RUNS_REL,
        CONVENTIONS_REL,
        ARCHITECTURE_REL,
    ]
    .into_iter()
    .map(|rel| project_root.join(rel))
    .collect()
}

/// D-368:`.kanzei/memory/` 动态条目树的互斥锁目标(memory 写入口持同一把锁,
/// 见 kanzei-memory store.rs `MemoryStore::tree_lock`)。
///
/// 为什么锁**整树**而不是逐个文件:动态条目文件(M-xxx.md、inbox.md、voided-ids.md
/// 等)在创建前无法预锁,逐个枚举必然漏;锁目标 = memory 根目录本身,锁文件 =
/// 同目录 `<root>.lock`(`.kanzei/memory.lock`,由 [`crate::atomic_file::lock_path_for`]
/// 派生)。锁文件落在 `.kanzei/`(不是任何托管根,不进镜像),且被 `.kanzei/**/*.lock`
/// 忽略——围栏快照对它无感,前后两张镜像都不会因锁文件而误报。
fn memory_tree_lock_path(project_root: &Path) -> PathBuf {
    project_root.join(".kanzei").join("memory")
}

/// 收口读取托管文档前的取锁预算。
///
/// 这里必须与专用写入口的默认等待预算一致:冷 CI 首次建立 memory 派生索引时,
/// 一次合法写事务可能超过旧的 500ms。收口若更早放弃,会把仍在进行的合法写误报为
/// “无法归因”,让无越界写的 bash 假失败。3s 仍是有界等待,写者卡死时不会无限阻塞。
const LOCK_ACQUIRE_BUDGET: std::time::Duration = crate::atomic_file::DEFAULT_LOCK_BUDGET;

/// bash 围栏收口读取期间持有的托管文档共享锁。
///
/// R-268 后命令窗口内允许专用写者自由落盘并记写日志;命令结束时才短暂取这组锁,
/// 确保 after 快照不会读到写事务中间态。随后按写日志吸收合法变化、回滚越界变化。
///
/// `.kanzei/memory/` 下的动态条目文件(M-xxx.md、inbox.md 等)创建前无法逐个锁,
/// 围栏额外取一把 **memory 树共享锁**(锁目标 = memory 根目录,锁文件 =
/// `.kanzei/memory.lock`),与 memory 写入口的排他树锁互斥。
///
/// 为什么用独立线程持有:`FileLock` 被刻意做成 `!Send`,而收口函数需要把整组锁作为
/// 句柄返回给调用线程完成快照和对账。锁留在**获取它的线程**上直到 release 信号,
/// 主线程只持有 Send 句柄;释放时先发信号再 join,保证锁在线程退出前释放干净。
pub(crate) struct ManagedLocks {
    release: std::sync::mpsc::Sender<()>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl ManagedLocks {
    /// 非托管目录的空锁组:不持任何锁,Drop 无副作用(发信号到无人接收的 channel)。
    fn noop() -> Self {
        let (release, _rx) = std::sync::mpsc::channel();
        ManagedLocks {
            release,
            join: None,
        }
    }
}

impl Drop for ManagedLocks {
    fn drop(&mut self) {
        let _ = self.release.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// 获取全部已知托管文档的共享锁。任一文件在预算内取不到(另一个进程正写入)
/// 即整体失败:围栏拿不全锁就不能保证收口快照一致,宁可拒绝 bash 也不能带病回滚。
///
/// **非托管目录零副作用(D-364 B3)**:根下没有 `.kanzei` 就没有托管文档可保护,
/// 直接返回空锁组——`try_lock_exclusive` 会 `create_dir_all` 父目录,在一个不是
/// 项目的目录里执行 bash 时,光取锁就把 `.kanzei/project/` 连同一堆 `.lock` 造了
/// 出来,那个 `.kanzei` 反过来成了「这里是项目根」的标记,污染目录发现(实测:
/// `%TEMP%` 被 bash 测试当 project_root 跑一次后,`Temp\.kanzei` 让所有临时目录
/// 向上解析成同一个项目根,进程恢复隔离测试全挂)。
pub(crate) fn acquire_managed_locks(project_root: &Path) -> Result<ManagedLocks, String> {
    if !managed_scope_exists(project_root) {
        return Ok(ManagedLocks::noop());
    }
    let paths = known_active_doc_paths(project_root)
        .into_iter()
        .chain(std::iter::once(memory_tree_lock_path(project_root)))
        .collect::<Vec<_>>();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let join = std::thread::spawn(move || {
        let mut locks: Vec<crate::atomic_file::FileLock> = Vec::new();
        let outcome = (|| -> Result<(), String> {
            for path in &paths {
                // D-382:围栏取**共享档**。它要的是「窗口内没有写者」,不是「没有
                // 别的围栏」——两条并行线的围栏诉求完全相同,互斥纯属误伤。改共享
                // 之后 D-364 的不变式一字不动:写者要排他,照样被挡在窗口外。
                let lock = crate::atomic_file::try_lock_shared(path, LOCK_ACQUIRE_BUDGET)
                    .map_err(|e| format!("cannot lock managed path {}: {e}", path.display()))?
                    .ok_or_else(|| {
                        format!(
                            "cannot lock managed path {} within {:?}: another process is writing \
                             it. Wait for it to finish and retry the command.",
                            path.display(),
                            LOCK_ACQUIRE_BUDGET
                        )
                    })?;
                locks.push(lock);
            }
            Ok(())
        })();
        let _ = result_tx.send(outcome);
        // 成败都等 release:成功时等命令结束;失败时主线程会立刻发,让锁随本线程退出释放。
        let _ = release_rx.recv();
        drop(locks);
    });
    match result_rx.recv() {
        Ok(Ok(())) => Ok(ManagedLocks {
            release: release_tx,
            join: Some(join),
        }),
        Ok(Err(e)) => {
            let _ = release_tx.send(());
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            let _ = join.join();
            Err("managed-lock acquisition thread panicked".into())
        }
    }
}

/// 项目是否处于 Harness 托管之下(存在 `.kanzei` 目录)。
pub(crate) fn managed_scope_exists(project_root: &Path) -> bool {
    project_root.join(".kanzei").is_dir()
}

fn collect_files(dir: &Path, project_root: &Path, snapshot: &mut ManagedSnapshot) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if snapshot.files.len() >= MANAGED_SNAPSHOT_MAX_FILES {
            snapshot.truncated = true;
            return;
        }
        let path = entry.path();
        // D-364:跳过锁文件(`<stem>.lock`,跨进程写锁的运行时产物,已进 .gitignore)。
        // 锁文件被 `share_mode(0)` 独占句柄持有,`metadata()` 读取会被拒绝——快照把它
        // 算进去会因「读不到属性」记成超限文件,整棵托管树判为不可回滚(D-364 实测)。
        // 它也不是文档内容,本就不该进镜像。
        if path.extension().and_then(|e| e.to_str()) == Some("lock") {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => collect_files(&path, project_root, snapshot),
            Ok(kind) if kind.is_file() => {
                let Ok(relative) = path.strip_prefix(project_root) else {
                    continue;
                };
                let key = relative.display().to_string().replace('\\', "/");
                let size = entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
                let content = (size <= MANAGED_SNAPSHOT_FILE_LIMIT)
                    .then(|| std::fs::read(&path).ok())
                    .flatten();
                snapshot.files.insert(key, content);
            }
            _ => {}
        }
    }
}

/// 比对执行前后的托管目录镜像。有改动就隔离留证 + 整体回滚,并生成回喂模型的报告。
///
/// R-268 之后前台 bash 改用 [`enforce_managed_files_with_writer_log`](带写日志对账);
/// 本函数保留为无日志场景的退化形态与既有测试的锚点(D-174 语义不动)。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn enforce_managed_files(
    project_root: &Path,
    before: ManagedSnapshot,
) -> Option<String> {
    let after = ManagedSnapshot::capture(project_root);
    if after == before {
        return None;
    }
    let after_incomplete = !after.is_complete();
    let change = diff(&before, &after)?;
    let listed = change
        .touched()
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let (quarantine, restored) = quarantine_and_restore(project_root, &before, &change, "shell");

    let incomplete = if after_incomplete {
        format!(
            "\nWARNING: the post-command snapshot exceeded its safety bound. Known changes were \
             rolled back, but the managed tree must be inspected manually before any further shell \
             command (limit: {MANAGED_SNAPSHOT_MAX_FILES} files / {MANAGED_SNAPSHOT_FILE_LIMIT} bytes per file)."
        )
    } else {
        String::new()
    };
    Some(format!(
        "[managed-files] BLOCKED AND ROLLED BACK. This command modified files under {} — those \
         paths are policy-managed and the shell is not a write channel for them, no matter which \
         mechanism is used (redirect, Set-Content/Out-File, [System.IO.File]::WriteAllText, \
         python/node one-liner, git checkout of a single file).\n\
         touched: {listed}\n\
         restored {restored} file(s) to their pre-command contents; your versions were kept at \
         {} so nothing is lost.\n\
         Redo the change through the dedicated tool (`req`/`defect`/`idea`/`decision` for tracker \
         entries, `architecture` for the architecture index, `memory_note` for memory). If no \
         tool covers what you need, that is an unimplemented capability: record it and tell the \
         user — do not look for another shell route.{incomplete}",
        MANAGED_ROOTS.join(" and "),
        quarantine.display(),
    ))
}

/// R-268:带**写日志对账**的托管围栏(前台 bash 用)。
///
/// 与 [`enforce_managed_files`] 的区别:窗口内变化逐路径查写日志——日志命中且
/// 终态一致的路径视为**专用工具的合法写入**(吸收进基线,不回滚);未命中的路径
/// 照旧隔离回滚。这打破 D-364「窗口内没有写者」的锁依赖:写者不再需要跨窗口
/// 互斥,只要自己的写入留下日志,围栏收口时就能与越界写区分。
///
/// `window_start_ms` 是命令窗口起点(拍 `before` 快照的时刻):只认这个时刻之后
/// 的日志,窗口之前的历史写入不参与本次对账。
///
/// 返回:与 [`enforce_managed_files`] 同形态(有越界回滚时返回报告)。合法写入
/// 只吸收、不回滚,不产生报告。
pub(crate) fn enforce_managed_files_with_writer_log(
    project_root: &Path,
    before: ManagedSnapshot,
    window_start_ms: u128,
) -> Option<String> {
    // R-268:收口时取**有界**文件锁(统一默认预算 3s)再拍 after 快照——确保此刻没有
    // 写者在写(快照读到中间态会把合法写入误判成越界)。写者平时自由写(不再被
    // 贯穿窗口的共享档挡),只有收口这一瞬与写者互斥。D-603 证明冷 CI 的 memory
    // 派生索引初始化可超过 500ms,因此与写入口统一使用 3s;超预算仍明确报错。
    let _fence_locks = match acquire_managed_locks(project_root) {
        Ok(locks) => locks,
        Err(error) => {
            return Some(format!(
                "[managed-files] WARNING: fence close-out could not lock managed documents \
                 ({error}); cross-fence attribution was NOT enforced for this command."
            ));
        }
    };
    let after = ManagedSnapshot::capture(project_root);
    if after == before {
        return None;
    }
    let after_incomplete = !after.is_complete();
    let change = diff(&before, &after)?;
    let logs = crate::write_log::entries_after(project_root, window_start_ms);
    // 日志命中判据:窗口内对该路径有过写入,且写后指纹 == 当前快照指纹
    // (终态一致 = 专用工具的最后一次写就是我们看到的形态;此后没有人再动它)。
    let covered = |path: &str| -> bool {
        logs.iter().any(|entry| {
            entry.path == path && {
                let fingerprint = crate::content_hash(
                    after
                        .files
                        .get(path)
                        .map(|content| content.as_deref().unwrap_or_default())
                        .unwrap_or_default(),
                );
                entry.fingerprint == fingerprint
            }
        })
    };
    let (legitimate, breach) = change.partition(covered);
    if breach.is_empty() {
        // 全部变化都有合法写日志解释:吸收进基线,不算越界。
        let paths: Vec<&str> = legitimate.touched().iter().map(|s| s.as_str()).collect();
        // before 是本次窗口的基线;吸收后本函数丢弃,调用方(前台 bash)无需保留。
        let _absorbed = before;
        let _ = paths;
        return None;
    }
    let listed = breach
        .touched()
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let (quarantine, restored) =
        quarantine_and_restore(project_root, &before, &breach, "shell-with-log");

    let incomplete = if after_incomplete {
        format!(
            "\nWARNING: the post-command snapshot exceeded its safety bound. Known changes were \
             rolled back, but the managed tree must be inspected manually before any further shell \
             command (limit: {MANAGED_SNAPSHOT_MAX_FILES} files / {MANAGED_SNAPSHOT_FILE_LIMIT} bytes per file)."
        )
    } else {
        String::new()
    };
    Some(format!(
        "[managed-files] BLOCKED AND ROLLED BACK. This command modified files under {} — those \
         paths are policy-managed and the shell is not a write channel for them, no matter which \
         mechanism is used. Paths with a matching dedicated-tool write log were absorbed as \
         legitimate; the following had no such explanation and were rolled back:\n\
         touched: {listed}\n\
         restored {restored} file(s) to their pre-command contents; your versions were kept at \
         {} so nothing is lost.{incomplete}",
        MANAGED_ROOTS.join(" and "),
        quarantine.display(),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;

    /// D-603 回归:旧收口预算只有 500ms,冷 CI 的合法 memory 写事务稍慢就被误报成
    /// “cross-fence attribution was NOT enforced”。这里确定性持有树锁 800ms:
    /// 收口必须等待写者完成后取得一致快照,不能因超过旧阈值而假失败。
    #[test]
    fn 收口等待超过旧五百毫秒的合法memory写者() {
        let root = temp_project("close-out-waits-memory-writer");
        let memory_root = root.join(".kanzei/memory");
        std::fs::create_dir_all(&memory_root).unwrap();
        let before = ManagedSnapshot::capture(&root);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let _lock = crate::atomic_file::lock_exclusive(&memory_root).unwrap();
            ready_tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(800));
        });
        ready_rx.recv().unwrap();

        let started = std::time::Instant::now();
        let report = enforce_managed_files_with_writer_log(&root, before, 0);
        let elapsed = started.elapsed();

        writer.join().unwrap();
        assert!(report.is_none(), "合法写者结束后收口不应误报: {report:?}");
        assert!(
            elapsed >= std::time::Duration::from_millis(500),
            "测试必须真实跨过旧 500ms 失败阈值,实得 {elapsed:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D-382 回归:两条并行线的 bash 收口共享锁必须能同时持有。
    ///
    /// 改造前这条会红——围栏取排他锁,第二条线按 500ms 预算必然拿不到,报
    /// "bash refused before execution: cannot lock managed path"。实测现场是一条线
    /// 跑 cargo check(持锁分钟级),另一条线连着十次被拒。
    #[test]
    fn 两条线的收口共享锁可以同时持有() {
        let root = temp_project("parallel-fence");
        std::fs::write(
            root.join(".kanzei/project/requirements.md"),
            "# Requirements\n",
        )
        .unwrap();
        let 线一 = acquire_managed_locks(&root).expect("第一条线取围栏");
        let root2 = root.clone();
        let 线二 = std::thread::spawn(move || acquire_managed_locks(&root2))
            .join()
            .unwrap();
        assert!(
            线二.is_ok(),
            "第二条线的围栏被拒:并行线又被串行化了(D-382)。实得:{:?}",
            线二.err()
        );
        drop(线一);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 收口一致性不能因为改共享档而松动:共享锁在场时,**写者**必须仍被挡住。
    #[test]
    fn 收口共享锁在场时写者仍被挡住() {
        let root = temp_project("fence-blocks-writer");
        let doc = root.join(".kanzei/project/requirements.md");
        std::fs::write(&doc, "# Requirements\n").unwrap();
        let 围栏 = acquire_managed_locks(&root).expect("取围栏");
        let doc2 = doc.clone();
        let 写者拿到 = std::thread::spawn(move || {
            crate::atomic_file::try_lock_exclusive(&doc2, std::time::Duration::from_millis(150))
                .unwrap()
                .is_some()
        })
        .join()
        .unwrap();
        assert!(
            !写者拿到,
            "围栏窗口内写者拿到了排他锁:D-364 的归因前提失守,别人的合法写入会被回滚"
        );
        drop(围栏);
        // 围栏走后写者必须立刻能进(靠 Drop 里的广播唤醒,不是轮询)。
        let 之后 = std::thread::spawn(move || {
            crate::atomic_file::try_lock_exclusive(&doc, std::time::Duration::from_secs(2))
                .unwrap()
                .is_some()
        })
        .join()
        .unwrap();
        assert!(之后, "围栏释放后写者仍拿不到");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 读路径(DocStore::load 走共享档)不得被收口共享锁挡住——否则文档面板停止刷新。
    #[test]
    fn 收口共享锁在场时文档仍读得出来() {
        let root = temp_project("fence-allows-read");
        std::fs::write(
            root.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-001 一条 [doing]\n- 优先级: P1\n",
        )
        .unwrap();
        let 围栏 = acquire_managed_locks(&root).expect("取围栏");
        let root2 = root.clone();
        let 读到 = std::thread::spawn(move || {
            crate::docstore::DocStore::open(&root2, &crate::docstore::REQUIREMENTS)
                .load()
                .map(|entries| entries.len())
        })
        .join()
        .unwrap();
        match 读到 {
            Ok(count) => assert_eq!(count, 1, "围栏在场时读到的条目数不对:期望 1,实得 {count}"),
            Err(error) => {
                panic!("围栏在场时读不出文档:桌面端文档面板会停在「刷新失败」。实得 {error}")
            }
        }
        drop(围栏);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-managed-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        dir
    }

    /// D-364 锁与回滚原语回归:收口共享锁持有期间——
    /// ① 另一进程(用另一线程模拟)的合法并发写者被锁挡在窗口外;
    /// ② 命令自身不走锁的越界写照旧被围栏检出并整体回滚(围栏本职不丢);
    /// ③ 释放锁后并发写者拿到锁,命令结束后落盘、不被误回滚。
    #[test]
    fn 收口锁挡并发写者_越界写仍回滚_释放后写者成功() {
        let root = temp_project("locks");
        let req = root.join(".kanzei/project/requirements.md");
        std::fs::write(&req, "# Requirements\n\n## R-001 既存条目 [todo]\n").unwrap();

        // 单测把“锁住一致读阶段”和“按基线回滚越界”两个原语串起来验证。
        let locks = acquire_managed_locks(&root).unwrap();
        let before = ManagedSnapshot::capture(&root);

        // ① 窗口内并发写者(另一线程模拟另一个进程的 `kz req add`)拿不到锁。
        let contended = {
            let req = req.clone();
            std::thread::spawn(move || {
                crate::atomic_file::try_lock_exclusive(&req, std::time::Duration::from_millis(150))
                    .unwrap()
                    .is_none()
            })
        };
        assert!(contended.join().unwrap(), "围栏窗口内并发写者必须被锁挡住");

        // ② 命令自身越界写(不走锁,shell 绕行的形态)仍被围栏检出并回滚。
        std::fs::write(&req, "# Requirements\n\n## R-999 越界写入 [todo]\n").unwrap();
        let report = enforce_managed_files(&root, before).expect("越界写必须被围栏检出并回滚");
        assert!(
            report.contains("requirements.md"),
            "报告要点名被回滚的文件: {report}"
        );
        let after = std::fs::read_to_string(&req).unwrap();
        assert!(after.contains("R-001"), "回滚必须恢复执行前内容: {after}");
        assert!(!after.contains("R-999"), "越界写不得残留: {after}");

        // ③ 释放后并发写者能拿到锁(命令结束,外部写者随后落盘、不被误回滚)。
        drop(locks);
        let contended = {
            let req = req.clone();
            std::thread::spawn(move || crate::atomic_file::lock_exclusive(&req).is_ok())
        };
        assert!(contended.join().unwrap(), "释放后写者必须能拿到锁");

        std::fs::remove_dir_all(&root).ok();
    }

    /// 已知文档清单必须覆盖 tracker/tests/conventions/architecture 四族的**活动**文件
    /// (每族一把锁罩住归档与账本)。漏一条,那条的并发写者就仍会被围栏误回滚。
    #[test]
    fn 已知文档清单覆盖四族活动文件() {
        let root = temp_project("known");
        let paths: Vec<String> = known_active_doc_paths(&root)
            .into_iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .display()
                    .to_string()
                    .replace('\\', "/")
            })
            .collect();
        for expected in [
            ".kanzei/project/requirements.md",
            ".kanzei/project/defects.md",
            ".kanzei/project/ideas.md",
            ".kanzei/project/decisions.md",
            ".kanzei/project/memory.md",
            ".kanzei/project/tests.md",
            ".kanzei/project/conventions.md",
            ".kanzei/project/architecture/README.md",
        ] {
            assert!(
                paths.iter().any(|p| p == expected),
                "清单缺 {expected}: {paths:?}"
            );
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-368 树锁原语回归:收口共享锁持有期间,并发 memory 写者被 memory 树锁挡住;
    /// 不走锁的越界写照旧能被镜像差异检出并回滚;释放后写者可拿到树锁。
    #[test]
    fn 收口持memory树锁挡并发写者_越界写仍回滚_释放后写者成功() {
        let root = temp_project("mem-lock");
        std::fs::create_dir_all(root.join(".kanzei/memory")).unwrap();
        let mem_root = root.join(".kanzei/memory");

        // 单测把 memory 树锁与镜像回滚两个原语串起来验证。
        let locks = acquire_managed_locks(&root).unwrap();
        let before = ManagedSnapshot::capture(&root);

        // ① 窗口内并发 memory 写者(线程模拟另一进程 memory_add 的写阶段)拿不到树锁。
        let contended = {
            let mem_root = mem_root.clone();
            std::thread::spawn(move || {
                crate::atomic_file::try_lock_exclusive(
                    &mem_root,
                    std::time::Duration::from_millis(150),
                )
                .unwrap()
                .is_none()
            })
        };
        assert!(
            contended.join().unwrap(),
            "围栏窗口内并发 memory 写者必须被树锁挡住"
        );

        // ② 命令自身越界写 .kanzei/memory/(不走锁,shell 绕行的形态)仍被围栏检出并回滚。
        let rogue = mem_root.join("M-999-越界.md");
        std::fs::write(&rogue, "# M-999 越界\n").unwrap();
        let report = enforce_managed_files(&root, before).expect("越界写必须被围栏检出并回滚");
        assert!(
            report.contains("M-999-越界.md"),
            "报告要点名越界文件: {report}"
        );
        assert!(!rogue.exists(), "越界写不得残留");

        // ③ 释放后并发写者能拿到树锁(命令结束,外部写者随后落盘、不被误回滚)。
        drop(locks);
        let contended = {
            let mem_root = mem_root.clone();
            std::thread::spawn(move || crate::atomic_file::lock_exclusive(&mem_root).is_ok())
        };
        assert!(contended.join().unwrap(), "释放后写者必须能拿到树锁");

        std::fs::remove_dir_all(&root).ok();
    }

    /// R-268 批1 核心:围栏收口按写日志对账——**有写日志解释的合法写入不误回滚**。
    ///
    /// 场景:窗口开点拍 before 快照;窗口内专用工具(`req add`)写了 requirements.md
    /// 并留下写日志;围栏收口时必须吸收(不报越界),而不是把 req 的合法写入回滚掉。
    /// 这正是 D-364 锁方案要防的误伤,现在由写日志替代锁来归因。
    #[test]
    fn 围栏收口_有写日志的合法写入不误回滚() {
        let root = temp_project("r268-log-ok");
        let req = root.join(".kanzei/project/requirements.md");
        std::fs::write(&req, "# Requirements\n\n## R-001 既存 [todo]\n").unwrap();

        // 窗口开点(拍 before 之前取时间,与 bash_body 的 fence_window_start_ms 同序)。
        let window_start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let before = ManagedSnapshot::capture(&root);

        // 窗口内:专用工具 `req add` 写文档(真实顺序:先写文档,再记写日志)。
        let new_content = "# Requirements\n\n## R-001 既存 [todo]\n\n## R-268 新增 [todo]\n";
        std::fs::write(&req, new_content).unwrap();
        crate::write_log::record(
            &root,
            &crate::write_log::WriteLogEntry {
                at_ms: window_start + 1,
                path: ".kanzei/project/requirements.md".into(),
                fingerprint: crate::content_hash(new_content.as_bytes()),
                content: new_content.as_bytes().to_vec(),
                run_id: Some("run-a".into()),
                process_id: Some("proc-a".into()),
            },
        )
        .unwrap();

        // 围栏收口:有日志解释 → 不报越界。
        let report = enforce_managed_files_with_writer_log(&root, before, window_start);
        assert!(
            report.is_none(),
            "有写日志的合法写入不应被围栏回滚,实得: {report:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&req).unwrap(),
            new_content,
            "合法写入必须保留(吸收进基线,不回滚)"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-268 批1:同窗口**越界写**(无写日志解释)照旧被检出并回滚——围栏本职不丢。
    #[test]
    fn 围栏收口_无写日志的越界写照旧回滚() {
        let root = temp_project("r268-log-breach");
        let req = root.join(".kanzei/project/requirements.md");
        std::fs::write(&req, "# Requirements\n\n## R-001 既存 [todo]\n").unwrap();

        let window_start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let before = ManagedSnapshot::capture(&root);

        // 窗口内:bash 越界写(无写日志)。
        std::fs::write(&req, "# Requirements\n\n## R-999 越界写入 [todo]\n").unwrap();

        let report = enforce_managed_files_with_writer_log(&root, before, window_start)
            .expect("无日志解释的越界写必须被检出并回滚");
        assert!(
            report.contains("requirements.md"),
            "报告要点名被回滚的文件: {report}"
        );
        let after = std::fs::read_to_string(&req).unwrap();
        assert!(after.contains("R-001"), "回滚必须恢复执行前内容: {after}");
        assert!(!after.contains("R-999"), "越界写不得残留: {after}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-268 批1:混合场景——合法写(有日志)与越界写(无日志)同窗口,回滚只动越界侧。
    #[test]
    fn 围栏收口_合法写与越界写混合_只回滚越界侧() {
        let root = temp_project("r268-log-mixed");
        let req = root.join(".kanzei/project/requirements.md");
        let defects = root.join(".kanzei/project/defects.md");
        std::fs::write(&req, "# Requirements\n\n## R-001 既存 [todo]\n").unwrap();
        std::fs::write(&defects, "# Defects\n").unwrap();

        let window_start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let before = ManagedSnapshot::capture(&root);

        // 窗口内:req 合法写(有日志)+ bash 越界写 defects(无日志)。
        let req_new = "# Requirements\n\n## R-001 既存 [todo]\n\n## R-268 新增 [todo]\n";
        std::fs::write(&req, req_new).unwrap();
        crate::write_log::record(
            &root,
            &crate::write_log::WriteLogEntry {
                at_ms: window_start + 1,
                path: ".kanzei/project/requirements.md".into(),
                fingerprint: crate::content_hash(req_new.as_bytes()),
                content: req_new.as_bytes().to_vec(),
                run_id: Some("run-a".into()),
                process_id: Some("proc-a".into()),
            },
        )
        .unwrap();
        std::fs::write(&defects, "# Defects\n\n## D-999 越界 [open]\n").unwrap();

        let report = enforce_managed_files_with_writer_log(&root, before, window_start)
            .expect("存在无日志解释的越界写,必须报");
        assert!(
            report.contains("defects.md"),
            "报告要点名越界文件: {report}"
        );
        assert!(
            !report.contains("requirements.md"),
            "有日志的合法写不应出现在越界报告: {report}"
        );
        // 合法写保留、越界写回滚。
        assert_eq!(
            std::fs::read_to_string(&req).unwrap(),
            req_new,
            "合法写(有日志)必须保留"
        );
        assert_eq!(
            std::fs::read_to_string(&defects).unwrap(),
            "# Defects\n",
            "越界写必须回滚到窗口开点"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-399:同路径混合——同一文件先合法写(有日志)再越界写(无日志覆盖),
    /// 回滚到最后一次合法日志内容,而非窗口开点(合法写不丢)。
    #[test]
    fn 同路径_先合法写后越界写_回滚到日志内容() {
        let root = temp_project("r268-same-path");
        let req = root.join(".kanzei/project/requirements.md");
        std::fs::write(&req, "# Requirements\n\n## R-001 既存 [todo]\n").unwrap();
        let window_start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let before = ManagedSnapshot::capture(&root);

        // 窗口内:先合法写(记日志),再越界写(无日志,覆盖同一文件)。
        let legit = "# Requirements\n\n## R-001 既存 [todo]\n\n## R-268 合法新增 [todo]\n";
        std::fs::write(&req, legit).unwrap();
        crate::write_log::record(
            &root,
            &crate::write_log::WriteLogEntry {
                at_ms: window_start + 1,
                path: ".kanzei/project/requirements.md".into(),
                fingerprint: crate::content_hash(legit.as_bytes()),
                content: legit.as_bytes().to_vec(),
                run_id: Some("run-a".into()),
                process_id: Some("proc-a".into()),
            },
        )
        .unwrap();
        std::fs::write(&req, "# Requirements\n\n## R-999 越界覆盖 [open]\n").unwrap();

        let report = enforce_managed_files_with_writer_log(&root, before, window_start)
            .expect("越界写必须报");
        assert!(
            report.contains("requirements.md"),
            "报告要点名越界文件: {report}"
        );
        // 回滚到日志内容(合法终态),而非窗口开点。
        assert_eq!(
            std::fs::read_to_string(&req).unwrap(),
            legit,
            "同路径先合法后越界:回滚到最后一次合法日志内容(合法写不丢)"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
