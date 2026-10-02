//! 旧版记忆迁移(R-255 第一刀,纯搬迁自 store.rs)。
//!
//! 独立理由:legacy 迁移是「R-098 的 .kanzei/project/memory.md(tracker M-条目)→
//! 一条一文件」的一次性历史搬迁,与运行期准入/检索/收件箱正交:迁完即不再触发,
//! 独立成域后,store 的日常读写路径不必背负历史格式知识(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):幂等——legacy 文件不存在即跳过;迁移后原文件改写为指路牌,
//! 二次运行不会重复落条目;旧 stale 条目降为 deprecated,其余 active。

use std::path::Path;

use super::store::MemoryStore;
use super::{today, MemoryEntry};

impl MemoryStore {
    /// legacy 迁移:R-098 的 .kanzei/project/memory.md(tracker M-条目)→ 一条一文件。
    /// 幂等:legacy 文件不存在即跳过;迁移后原文件改写为指路牌。
    pub(crate) fn migrate_legacy(&self, project_root: &Path) {
        let legacy = project_root
            .join(".kanzei")
            .join("project")
            .join("memory.md");
        if !legacy.is_file() {
            return;
        }
        let Ok(text) = std::fs::read_to_string(&legacy) else {
            return;
        };
        let entries = crate::docstore::parse(&crate::docstore::MEMORY, &text);
        if entries.is_empty() {
            return;
        }
        let now = today();
        for legacy_entry in &entries {
            if legacy_entry.id.is_empty() {
                continue;
            }
            let body: String = legacy_entry
                .fields
                .iter()
                .map(|(k, v)| format!("- {k}: {v}\n"))
                .collect();
            let entry = MemoryEntry {
                id: legacy_entry.id.clone(),
                scope: self.scope.label().into(),
                category: "fact".into(),
                title: legacy_entry.title.clone(),
                description: format!("{}(迁移自 memory.md)", legacy_entry.title),
                status: if legacy_entry.status == "stale" {
                    "deprecated"
                } else {
                    "active"
                }
                .into(),
                created: now.clone(),
                updated: now.clone(),
                source: "migration".into(),
                extras: Vec::new(),
                body,
            };
            let _ = self.write_entry(&entry, None);
        }
        let _ = self.refresh_derived();
        let _ = crate::atomic_file::write_atomic(
            &legacy,
            "# Memory\n\n(已迁移至 .kanzei/memory/,由 memory_search 检索;本文件不再使用。)\n",
        );
    }
}
