//! 存储域(R-257 B3):DocStore 结构定义与核心读写(open/lock/try_lock/load/save/
//! next_id/archive_file)。归档(archive.rs)、校验(validation.rs)以扩展 impl 块
//! 定义在各自域文件。自 docstore.rs 原样迁出,零行为变更。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::model::{DocKind, Entry};
use super::parse::{parse_document, DocumentTemplate};
use super::render::{render, render_with_template};

pub struct DocStore {
    pub kind: &'static DocKind,
    pub path: PathBuf,
    pub(crate) preserved: Arc<Mutex<Option<DocumentTemplate>>>,
    pub(crate) preserved_archive: Arc<Mutex<Option<DocumentTemplate>>>,
}
impl DocStore {
    pub fn open(project_root: &Path, kind: &'static DocKind) -> Self {
        DocStore {
            kind,
            path: project_root.join(kind.rel_path),
            preserved: Arc::new(Mutex::new(None)),
            preserved_archive: Arc::new(Mutex::new(None)),
        }
    }

    /// 取本 kind 的跨进程写锁(R-138)。
    ///
    /// **一个 kind 一把锁**,键取自活动文档路径,同时罩住活动文件、归档文件与
    /// 编号账本——这三者本来就是一笔账(`next_id` 要同时扫它们),分开锁等于没锁。
    ///
    /// **读路径一律不加锁**:原子写之后读者只会看到旧全量或新全量,不存在截断态;
    /// 让读者排队只会把"文档面板刷新"变成"等 agent 写完"。
    ///
    /// **防死锁不变量:持锁期间永不获取第二把锁。** 写事务只锁自己的 kind,
    /// `check_refs` 之类跨 kind 的查询走不加锁的读路径——结构上不可能循环等待。
    /// 谁要在持锁时再去锁另一个 kind,先把这条不变量改掉并说明新的加锁序。
    pub fn lock(&self) -> std::io::Result<crate::atomic_file::FileLock> {
        crate::atomic_file::lock_exclusive(&self.path)
    }

    /// 限时取锁:拿不到返回 `Ok(None)`。给"做不成也无所谓"的幂等写用。
    pub fn try_lock(
        &self,
        budget: std::time::Duration,
    ) -> std::io::Result<Option<crate::atomic_file::FileLock>> {
        crate::atomic_file::try_lock_exclusive(&self.path, budget)
    }

    pub fn load(&self) -> std::io::Result<Vec<Entry>> {
        // D-338:load 与 save 同一把锁互斥。save 是 tmp+rename 原子替换,但
        // Windows 上 rename 覆盖目标与读者 open 目标之间有竞态窗口——读者在
        // 替换瞬间 open 会 NotFound,load 对 NotFound 宽容返回 Ok(vec![]) =
        // 「读到 0 条」的假空快照(D-338 压测 20 轮 1 次失败,条目数 0)。
        // 持同一把 FileLock 后,读者在 save 持锁期间等待,rename 完成后才读,
        // 永远看到完整快照(3 或 30),不再有中间态窗口。FileLock 同线程重入
        // 安全(depth 计数),内部持锁路径调 load 不会自锁死。
        let _lock = self.lock()?;
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                let parsed = parse_document(self.kind, &text);
                *self.preserved.lock().unwrap() = Some(parsed.template.clone());
                Ok(parsed.entries)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, entries: &[Entry]) -> std::io::Result<()> {
        // 自保护:直接调 save 的入口(测试、restore、外部工具)也拿得到互斥。
        // 已在外层事务里持锁的调用方(archive_terminal / tracker 写动作)走
        // 同线程重入,不会自锁死。
        let _lock = self.lock()?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let template = self.preserved.lock().unwrap().clone();
        let text = template
            .as_ref()
            .map(|template| render_with_template(self.kind, entries, template))
            .unwrap_or_else(|| render(self.kind, entries));
        // R-138:tmp+rename 原子替换。裸 std::fs::write 是先截断再写,并发读者
        // 会看到零长度/半截文件,而 load() 对空文件宽容返回 Ok(vec![])——
        // 「成功但空」的快照就是这么穿到前端的(D-249 第①层)。
        crate::atomic_file::write_atomic(&self.path, &text)
    }

    /// ID 分配扫活跃 + 归档 + 废弃账本:归档移走或主动废弃过的编号都绝不复用。
    pub fn next_id(&self, entries: &[Entry]) -> String {
        let archived = self.load_archive().unwrap_or_default();
        let max = entries
            .iter()
            .chain(archived.iter())
            .filter_map(|e| {
                e.id.strip_prefix(self.kind.prefix)?
                    .strip_prefix('-')?
                    .parse::<u32>()
                    .ok()
            })
            .chain(self.voided_ids().keys().copied())
            .max()
            .unwrap_or(0);
        format!("{}-{:03}", self.kind.prefix, max + 1)
    }

    /// 归档文件:同目录 `<name>-archive.md`(如 requirements-archive.md)。
    pub fn archive_file(&self) -> PathBuf {
        match self.path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => self.path.with_file_name(format!("{stem}-archive.md")),
            None => self.path.with_extension("archive.md"),
        }
    }
}
