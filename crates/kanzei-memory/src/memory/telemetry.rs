//! 记忆效果画像(R-255 第一刀,纯搬迁自 store.rs)。
//!
//! 独立理由:命中统计是「记忆被用得多不多、值不值」的独立变更理由——`record_hits`
//! 写命中计数、`hit_profile` 给 UI 画像、`hits_map` 给统计展示。它与准入、检索排序
//! 正交:改画像口径不必读懂准入策略;hits 是观测(效果画像),不参与排序(R-150
//! 自增强退役),所以可跳可丢(D-368:围栏窗口内直接跳过记录)。
//!
//! 危险点(搬迁纪律):检索的观测副作用(命中计数)可跳可丢——bash 围栏窗口(持有
//! 记忆树锁)内直接跳过记录,绝不让一次只读检索把 index.db 改出窗口、被围栏当
//! 越界回滚(那会让 bash 误报 [managed-files],命中计数也照样丢,两边都亏)。

use rusqlite::params;

use super::store::{now_ms, MemoryStore};
use super::Novelty;

impl MemoryStore {
    /// R-165 批2:检索命中的观测副作用——记录一次真实召回(hits 计数)。
    /// 可跳可丢(D-368),见模块头。
    pub fn record_hits(&self, ids: &[String]) {
        let Ok(Some(_tree_lock)) = crate::atomic_file::try_lock_exclusive(
            &self.root,
            std::time::Duration::from_millis(50),
        ) else {
            return;
        };
        let Ok(conn) = self.open_db() else { return };
        let now = now_ms();
        for id in ids {
            let _ = conn.execute(
                "INSERT INTO memory_hits(id, hits, last_hit_at) VALUES (?1, 1, ?2)
                 ON CONFLICT(id) DO UPDATE SET hits = hits + 1, last_hit_at = ?2",
                params![id, now],
            );
        }
    }

    /// 命中画像(UI 展示):id → (hits, last_hit_at)。库缺失返回空表(派生物语义)。
    pub fn hit_profile(&self) -> std::collections::BTreeMap<String, (u64, i64)> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(conn) = self.open_db() else { return out };
        let Ok(mut statement) = conn.prepare("SELECT id, hits, last_hit_at FROM memory_hits")
        else {
            return out;
        };
        if let Ok(rows) = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        }) {
            for (id, hits, last) in rows.flatten() {
                out.insert(id, (hits.max(0) as u64, last));
            }
        }
        out
    }

    /// 命中统计(UI 展示):id → hits。库缺失返回空表(派生物语义)。
    pub fn hits_map(&self) -> std::collections::BTreeMap<String, u64> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(conn) = self.open_db() else {
            return out;
        };
        let Ok(mut statement) = conn.prepare("SELECT id, hits FROM memory_hits") else {
            return out;
        };
        if let Ok(rows) =
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        {
            for row in rows.flatten() {
                out.insert(row.0, row.1.max(0) as u64);
            }
        }
        out
    }

    /// 落一条 novelty 三档分流计数遥测(验收④)。
    pub fn record_novelty(&self, verdict: &Novelty, fingerprint: &str, note_head: &str) {
        let Ok(conn) = self.open_db() else { return };
        let _ = conn.execute(
            "INSERT INTO novelty_events(at, verdict, fingerprint, note_head) VALUES (?1, ?2, ?3, ?4)",
            params![now_ms(), verdict.as_str(), fingerprint, note_head.chars().take(80).collect::<String>()],
        );
    }

    /// R-165 批2 recurrence 三段晋升的跨轮计数(验收②):
    /// 同指纹跨轮复发计数,第 2 次才 candidate、第 3 次且带修复成功才 promote。
    /// 返回第几次出现(1-based)。持久化在 index.db,manager 消化失败笔记时查。
    pub fn bump_recurrence(&self, fingerprint: &str) -> u32 {
        let Ok(conn) = self.open_db() else { return 1 };
        let now = now_ms();
        let _ = conn.execute(
            "INSERT INTO recurrence_counts(fingerprint, count, last_at) VALUES (?1, 1, ?2)
             ON CONFLICT(fingerprint) DO UPDATE SET count = count + 1, last_at = ?2",
            params![fingerprint, now],
        );
        conn.query_row(
            "SELECT count FROM recurrence_counts WHERE fingerprint = ?1",
            params![fingerprint],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as u32)
        .unwrap_or(1)
    }

    /// 查询某指纹当前的复发次数(只读,不递增)。
    pub fn recurrence_count(&self, fingerprint: &str) -> u32 {
        let Ok(conn) = self.open_db() else { return 0 };
        conn.query_row(
            "SELECT count FROM recurrence_counts WHERE fingerprint = ?1",
            params![fingerprint],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as u32)
        .unwrap_or(0)
    }
}
