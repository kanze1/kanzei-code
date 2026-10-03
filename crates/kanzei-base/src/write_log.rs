//! 写后凭据：调用者持文档/目录写锁，先写文档，再记录日志，最后释放写锁。
//!
//! 项目日志锁串行分配递增编号；时间只用于窗口筛选，编号决定先后。
//! 历史清理必须保留每个路径的最新事件和最后可恢复状态。读取错误直接返回，
//! 调用者不能把损坏的凭据当成“没有合法写入”后继续自动回滚。

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

mod codec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoggedContent {
    /// 保存正文，空 Vec 表示合法空文件。
    Stored(Vec<u8>),
    /// 只保存指纹；可以归因，但不能用它恢复正文。
    FingerprintOnly,
    /// 文件已被合法删除，与空文件不同。
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteLogEntry {
    pub at_ms: u128,
    /// 相对项目根的路径，使用 `/` 分隔。
    pub path: String,
    pub fingerprint: String,
    pub content: LoggedContent,
    pub run_id: Option<String>,
    pub process_id: Option<String>,
}

impl WriteLogEntry {
    /// None 表示文件不存在；Some(&[]) 表示空文件。
    pub fn matches_content(&self, current: Option<&[u8]>) -> bool {
        match (&self.content, current) {
            (LoggedContent::Deleted, None) => true,
            (LoggedContent::Deleted, Some(_)) | (_, None) => false,
            (_, Some(bytes)) => self.fingerprint == crate::content_hash(bytes),
        }
    }
}

struct Record {
    order: u128,
    file: PathBuf,
    entry: WriteLogEntry,
}

const WRITE_LOG_MAX_FILES: usize = 500;

fn log_root(project_root: &Path) -> PathBuf {
    project_root.join(".kanzei/.write-log")
}

/// 先写文档再记录，二者不是跨文件原子事务。失败必须向调用者暴露。
pub fn record(project_root: &Path, entry: &WriteLogEntry) -> io::Result<PathBuf> {
    let root = log_root(project_root);
    std::fs::create_dir_all(&root)?;
    let _lock = crate::atomic_file::lock_exclusive(&root.join("journal"))?;
    let mut records = read_records(&root)?;
    let previous = records.last().map_or(0, |record| record.order);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let order = now.max(
        previous
            .checked_add(1)
            .ok_or_else(|| invalid("写日志编号溢出"))?,
    );
    let file = root.join(format!(
        "{order:039}-{}.log",
        crate::content_hash(entry.path.as_bytes())
    ));
    crate::atomic_file::write_atomic(&file, &codec::encode(entry))?;
    records.push(Record {
        order,
        file: file.clone(),
        entry: entry.clone(),
    });
    prune(&records, WRITE_LOG_MAX_FILES)?;
    Ok(file)
}

fn read_records(root: &Path) -> io::Result<Vec<Record>> {
    let mut records = Vec::new();
    let mut identities = HashSet::new();
    for file in std::fs::read_dir(root)? {
        let file = file?.path();
        if file.extension().is_none_or(|extension| extension != "log") {
            continue;
        }
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid(format!("无效写日志文件名：{}", file.display())))?;
        let order = name
            .split('-')
            .next()
            .and_then(|order| order.parse().ok())
            .ok_or_else(|| invalid(format!("无效写日志编号：{}", file.display())))?;
        let entry = codec::decode(&std::fs::read_to_string(&file)?)
            .map_err(|error| invalid(format!("写日志 {}：{error}", file.display())))?;
        if !identities.insert((order, entry.path.clone())) {
            return Err(invalid(format!(
                "旧日志无法确定 {} 在编号 {order} 的写入顺序",
                entry.path
            )));
        }
        records.push(Record { order, file, entry });
    }
    records.sort_by_key(|record| record.order);
    Ok(records)
}

/// 返回窗口内事件，按持久化编号升序。目录尚未创建表示没有日志，其余错误返回 Err。
pub fn entries_after(project_root: &Path, at_ms: u128) -> io::Result<Vec<WriteLogEntry>> {
    let root = log_root(project_root);
    if !root.try_exists()? {
        return Ok(Vec::new());
    }
    let _lock = crate::atomic_file::lock_shared(&root.join("journal"))?;
    Ok(read_records(&root)?
        .into_iter()
        .map(|record| record.entry)
        .filter(|entry| entry.at_ms >= at_ms)
        .collect())
}

fn prune(records: &[Record], limit: usize) -> io::Result<()> {
    let mut latest = HashSet::new();
    let mut checkpoint = HashSet::new();
    let mut protected = HashSet::new();
    for record in records.iter().rev() {
        let newest = latest.insert(record.entry.path.as_str());
        let recoverable = !matches!(record.entry.content, LoggedContent::FingerprintOnly)
            && checkpoint.insert(record.entry.path.as_str());
        if newest || recoverable {
            protected.insert(&record.file);
        }
    }
    let excess = records.len().saturating_sub(limit.max(protected.len()));
    for record in records
        .iter()
        .filter(|record| !protected.contains(&record.file))
        .take(excess)
    {
        std::fs::remove_file(&record.file)?;
    }
    Ok(())
}

/// 调用者需要按路径查最后状态时复用同一排序，不按毫秒重新排序。
pub fn latest_by_path(entries: &[WriteLogEntry]) -> HashMap<&str, &WriteLogEntry> {
    entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect()
}

fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
