//! 用户偏好条目(R-255 第三刀,纯搬迁自 store.rs)。
//!
//! 独立理由:偏好条目是「用户随时会改的定调」(开发重心这类)的独立变更理由——
//! `find_preference` 按标题前缀找 active 偏好、`upsert_preference` 命中即改否则新增。
//! 它与准入(add)、生命周期(promote)、检索(search)正交:定调会被反复调整,必须
//! 复用同一条目,每次切换都新增会把索引撑爆且历史无从对照(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):upsert 走用户手改路径,不经 memory-manager(A-005:用户编辑
//! 本就不受写读分离约束);force=true 已跳过语义闸,Uncertain 保守取候选首条。

use super::store::{AddOutcome, MemoryStore};
use super::{today, MemoryEntry};

impl MemoryStore {
    /// 按标题前缀找一条 active 偏好条目(开发重心这类"随时会改的定调")。
    pub fn find_preference(&self, title_prefix: &str) -> Option<MemoryEntry> {
        self.load_all().into_iter().map(|(_, e)| e).find(|e| {
            e.category == "preference" && e.status == "active" && e.title.starts_with(title_prefix)
        })
    }

    /// 用户直写的偏好 upsert:标题前缀命中就改,否则新增。
    /// 定调会被反复调整,必须复用同一条目——每次切换都新增会把索引撑爆且历史无从对照。
    /// (用户手改路径,不经 memory-manager;A-005:用户编辑本就不受写读分离约束。)
    pub fn upsert_preference(
        &self,
        title_prefix: &str,
        title: &str,
        description: &str,
        body: &str,
    ) -> anyhow::Result<MemoryEntry> {
        if let Some(existing) = self.find_preference(title_prefix) {
            let entries = self.load_all();
            let (path, mut entry) = entries
                .into_iter()
                .find(|(_, e)| e.id == existing.id)
                .expect("found above");
            entry.title = title.trim().to_string();
            entry.description = description.trim().to_string();
            entry.body = body.trim().to_string();
            entry.updated = today();
            self.write_entry(&entry, Some(&path))?;
            self.refresh_derived()?;
            return Ok(entry);
        }
        match self.add(
            "preference",
            title,
            description,
            body,
            "user",
            &[],
            None,
            true,
        )? {
            AddOutcome::Added(entry)
            | AddOutcome::Duplicate(entry)
            | AddOutcome::SubjectConflict(entry) => Ok(entry),
            // force=true 已跳过语义闸,Uncertain 不应出现;保守处理为取候选首条。
            AddOutcome::Uncertain(mut candidates) => Ok(candidates
                .pop()
                .ok_or_else(|| anyhow::anyhow!("force add failed: no entry"))?),
        }
    }
}
