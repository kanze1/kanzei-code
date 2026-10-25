//! 结构化文档存储与源文版本快照。读取保留 Markdown 模板，写入使用锁与原子替换。
//! 归档与状态校验由各自的 DocStore 扩展实现负责。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::model::{DocKind, Entry};
use super::parse::{parse_document, DocumentTemplate};
use super::render::{render, render_with_template};

fn validate_topic(topic: &str) -> std::io::Result<&str> {
    let valid = !topic.is_empty()
        && topic.len() <= 80
        && !topic.starts_with('-')
        && !topic.ends_with('-')
        && !topic.contains("--")
        && topic
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(topic)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("topic 必须是 1-80 个小写字母/数字组成的 kebab-case: `{topic}`"),
        ))
    }
}

/// 一次受锁读取的条目与完整源文件版本，不额外回读磁盘计算版本。
#[derive(Debug)]
pub struct DocumentSnapshot {
    pub entries: Vec<Entry>,
    pub revision: String,
}

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

    /// B2:研究来源/发现按课题隔离到 `.kanzei/research/<topic>/`。
    /// topic 只接受小写 kebab-case，避免 `..`、绝对路径和跨目录写入。
    pub fn open_topic(
        project_root: &Path,
        kind: &'static DocKind,
        topic: &str,
    ) -> std::io::Result<Self> {
        let topic = validate_topic(topic)?;
        if !matches!(kind.prefix, "S" | "F") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "topic store 只适用于 source/finding",
            ));
        }
        let filename = Path::new(kind.rel_path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "无效的 topic 文档名")
            })?;
        Ok(DocStore {
            kind,
            path: project_root
                .join(".kanzei/research")
                .join(topic)
                .join(filename),
            preserved: Arc::new(Mutex::new(None)),
            preserved_archive: Arc::new(Mutex::new(None)),
        })
    }

    pub fn validate_topic(topic: &str) -> std::io::Result<&str> {
        validate_topic(topic)
    }

    /// 取本 kind 的跨进程写锁(R-138)。
    ///
    /// **一个 kind 一把锁**,键取自活动文档路径,同时罩住活动文件、归档文件与
    /// 编号账本——这三者本来就是一笔账(`next_id` 要同时扫它们),分开锁等于没锁。
    ///
    /// 读路径取共享锁：读者之间可并行，写者持排他锁时读者等待。
    /// 同目标内的 load 走锁实现的重入路径，不重复阻塞自己。
    ///
    /// 跨 kind 引用校验也会获取目标共享锁；调用方须保持单向依赖的取锁顺序，
    /// 不可把“共享读取”误当成不取锁或不会等待。
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
        match self.load_snapshot()? {
            Some(snapshot) => Ok(snapshot.entries),
            None => Ok(Vec::new()),
        }
    }

    /// 条目与完整源文版本来自同一次受锁读取；文件缺失与读取失败分别表达。
    pub fn load_snapshot(&self) -> std::io::Result<Option<DocumentSnapshot>> {
        // 读者共享、保存者排他，避免 Windows 原子替换期间读到假缺失。
        // 锁保护源文读取和模板解析；同一线程内的嵌套读取由锁实现处理重入。
        let _lock = crate::atomic_file::lock_shared(&self.path)?;
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                let parsed = parse_document(self.kind, &text);
                *self.preserved.lock().unwrap() = Some(parsed.template.clone());
                Ok(Some(DocumentSnapshot {
                    entries: parsed.entries,
                    revision: kanzei_base::content_hash(text.as_bytes()),
                }))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
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
    pub fn next_id(&self, entries: &[Entry]) -> std::io::Result<String> {
        let archived = self.load_archive()?;
        let voided = self.voided_ids()?;
        let max = entries
            .iter()
            .chain(archived.iter())
            .filter_map(|e| {
                e.id.strip_prefix(self.kind.prefix)?
                    .strip_prefix('-')?
                    .parse::<u32>()
                    .ok()
            })
            .chain(voided.keys().copied())
            .max()
            .unwrap_or(0);
        Ok(format!("{}-{:03}", self.kind.prefix, max + 1))
    }

    /// 归档文件:同目录 `<name>-archive.md`(如 requirements-archive.md)。
    pub fn archive_file(&self) -> PathBuf {
        match self.path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => self.path.with_file_name(format!("{stem}-archive.md")),
            None => self.path.with_extension("archive.md"),
        }
    }
}
