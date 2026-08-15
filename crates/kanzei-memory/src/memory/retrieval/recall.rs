//! 召回观测(R-255 第三刀,从 store.rs 迁出)。
//!
//! 把「这次检索召回了什么、注入多少、有没有被取用」落进 memory_recalls,供
//! recall_profile(画像)与 UI 复盘。它是检索的观测副作用,与排序决策无关。

use rusqlite::params;

use crate::memory::store::{now_ms, MemoryStore, SearchHit};
use crate::memory::{RecallHit, RecallRound};

impl MemoryStore {
    /// 记录一次召回轮次(观测,R-125):prompt 头、命中条目快照、注入字节数。
    /// 返回 recall_id(时间戳+scope 前缀);库不可用时仍返回 id(观测可丢)。
    pub fn record_recall(&self, prompt: &str, hits: &[SearchHit], injected_bytes: usize) -> String {
        let at = now_ms();
        let recall_id = format!("{at}-{}", self.scope.prefix());
        let Ok(conn) = self.open_db() else {
            return recall_id;
        };
        let head: String = prompt.chars().take(160).collect();
        for hit in hits {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO memory_recalls
                 (recall_id, at, prompt_head, injected_bytes, entry_id, title, scope, category, score, snippet, fetched)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                     COALESCE((SELECT fetched FROM memory_recalls WHERE recall_id = ?1 AND entry_id = ?5), 0))",
                params![
                    recall_id,
                    at,
                    head,
                    injected_bytes as i64,
                    hit.entry.id,
                    hit.entry.title,
                    hit.entry.scope,
                    hit.entry.category,
                    hit.score,
                    hit.snippet,
                ],
            );
        }
        recall_id
    }

    /// 标记某条记忆的正文在召回之后确实被拉取过 = 这次召回起了作用。
    /// 只回填最近一次召回:更早的那次已经有自己的结论,不能被后来的行为追认。
    pub fn mark_recall_fetched(&self, entry_id: &str) {
        let Ok(conn) = self.open_db() else { return };
        let _ = conn.execute(
            "UPDATE memory_recalls SET fetched = 1
             WHERE entry_id = ?1 AND recall_id = (
                 SELECT recall_id FROM memory_recalls WHERE entry_id = ?1 ORDER BY at DESC LIMIT 1)",
            params![entry_id],
        );
    }

    /// 最近若干次召回,按轮次聚合(新的在前)。
    pub fn recalls(&self, limit: usize) -> Vec<RecallRound> {
        let Ok(conn) = self.open_db() else {
            return Vec::new();
        };
        let Ok(mut statement) = conn.prepare(
            "SELECT recall_id, at, prompt_head, injected_bytes, entry_id, title, scope, category, score, snippet, fetched
             FROM memory_recalls ORDER BY at DESC, entry_id ASC",
        ) else {
            return Vec::new();
        };
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                RecallHit {
                    id: row.get(4)?,
                    title: row.get(5)?,
                    scope: row.get(6)?,
                    category: row.get(7)?,
                    score: row.get(8)?,
                    snippet: row.get(9)?,
                    fetched: row.get::<_, i64>(10)? != 0,
                },
            ))
        });
        let Ok(rows) = rows else { return Vec::new() };
        let mut rounds: Vec<RecallRound> = Vec::new();
        for row in rows.flatten() {
            let (recall_id, at, prompt_head, injected, hit) = row;
            match rounds.iter_mut().find(|r| r.recall_id == recall_id) {
                Some(round) => round.hits.push(hit),
                None => {
                    if rounds.len() >= limit {
                        break;
                    }
                    rounds.push(RecallRound {
                        recall_id,
                        at,
                        prompt_head,
                        injected_bytes: injected as usize,
                        hits: vec![hit],
                    });
                }
            }
        }
        rounds
    }
}
