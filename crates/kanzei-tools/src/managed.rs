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
                if std::fs::write(&absolute, original).is_ok() {
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
            if let Some(parent) = absolute.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&absolute, original).is_ok() {
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
    use crate::docstore::{DECISIONS, DEFECTS, GOALS, MEMORY, REQUIREMENTS};
    use crate::test_record::TEST_RUNS_REL;
    [
        REQUIREMENTS.rel_path,
        DEFECTS.rel_path,
        GOALS.rel_path,
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

/// 单个文档的取锁预算。正常情况对面是毫秒级事务,几百毫秒足够;异常(对方卡死)
/// 快速失败,不让一次 bash 被锁获取拖垮。
const LOCK_ACQUIRE_BUDGET: std::time::Duration = std::time::Duration::from_millis(500);

/// D-364:bash 围栏在命令执行期间持有的托管文档写锁。
///
/// 围栏的归因判据是「命令前后托管树快照一致」——命令窗口内**任何**变化都被判成
/// bash 越界并整体回滚。但窗口内的变化可能来自**另一个进程的合法写入**(自举轮跑
/// bash 时外部 `kz req add`):旧实现没有区分手段,把别人的合法写入一起回滚掉了,
/// 于是出现「added 成功但条目整体消失」(D-364)。修复 = 把并发写者挡在窗口之外:
/// 命令执行期间持有全部已知托管文档的 FileLock,写者在窗口内等锁、命令结束才落盘、
/// 因此不被围栏误回滚;超过预算则写者明确报错,绝不假成功。
///
/// D-368 同族残余:`.kanzei/memory/` 下的动态条目文件(M-xxx.md、inbox.md 等)创建前
/// 无法逐个预锁,围栏额外持一把 **memory 树锁**(锁目标 = memory 根目录,锁文件 =
/// `.kanzei/memory.lock`),memory 写入口(write_entry/refresh_derived/inbox/账本)持
/// 同一把锁——窗口内并发 memory_add 同样被挡到窗口外落盘,不被误回滚。
///
/// 为什么用独立线程持有:`FileLock` 被刻意做成 `!Send`,禁止跨 await 存活(毫秒级
/// 持有纪律);而 bash 命令执行是秒级的 async 窗口,锁必须跨过整个窗口。折中 = 锁
/// 在**获取它的线程**上持有到 release 信号到达,主线程只持有一个 Send 的句柄——
/// 「锁不离线程」的 !Send 纪律与「跨命令窗口持锁」两件事同时成立。释放时先发信号
/// 再 join,保证锁在线程退出前释放干净。
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

/// 获取全部已知托管文档的活动文件锁。任一文件在预算内取不到(另一个进程正持锁)
/// 即整体失败:围栏拿不全锁就不能保证归因正确,宁可拒绝 bash 也不能带病回滚。
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
                let lock = crate::atomic_file::try_lock_exclusive(path, LOCK_ACQUIRE_BUDGET)
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
         Redo the change through the dedicated tool (`req`/`defect`/`goal`/`decision` for tracker \
         entries, `architecture` for the architecture index, `memory_note` for memory). If no \
         tool covers what you need, that is an unimplemented capability: record it and tell the \
         user — do not look for another shell route.{incomplete}",
        MANAGED_ROOTS.join(" and "),
        quarantine.display(),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;

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

    /// D-364 的核心回归:围栏持锁期间——
    /// ① 另一进程(用另一线程模拟)的合法并发写者被锁挡在窗口外;
    /// ② 命令自身不走锁的越界写照旧被围栏检出并整体回滚(围栏本职不丢);
    /// ③ 释放锁后并发写者拿到锁,命令结束后落盘、不被误回滚。
    #[test]
    fn 持锁挡并发写者_越界写仍回滚_释放后写者成功() {
        let root = temp_project("locks");
        let req = root.join(".kanzei/project/requirements.md");
        std::fs::write(&req, "# Requirements\n\n## R-001 既存条目 [todo]\n").unwrap();

        // 真实顺序与 bash_body 一致:先持锁、再拍 before 快照(锁文件同现两张镜像,
        // 否则围栏会把锁文件当成命令新建的文件误删)。
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
            ".kanzei/project/goals.md",
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

    /// D-368 核心回归:围栏持锁窗口内,并发 memory 写者(另一线程模拟另一进程的
    /// `memory_add`)被 memory 树锁挡在窗外;命令自身不走锁的越界写照旧被围栏检出
    /// 并整体回滚(围栏本职不丢);释放后写者拿到树锁、落盘不被误回滚。
    #[test]
    fn 围栏持memory树锁挡并发写者_越界写仍回滚_释放后写者成功() {
        let root = temp_project("mem-lock");
        std::fs::create_dir_all(root.join(".kanzei/memory")).unwrap();
        let mem_root = root.join(".kanzei/memory");

        // 真实顺序与 bash_body 一致:先持锁(含 D-368 memory 树锁)、再拍 before 快照。
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
}
