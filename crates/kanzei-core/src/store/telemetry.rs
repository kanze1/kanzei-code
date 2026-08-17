//! 记忆漏斗遥测(R-161)。事实写入 state.db，CLI 与桌面端共享此接口。

use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;

use super::{now_ms, SessionStore, StoreError};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FunnelCounts {
    pub available: u64,
    pub retrieved: u64,
    pub injected: u64,
    pub action_changed: u64,
    pub outcome_improved: u64,
    /// 当前无在线写入方；展示层据此显示 N/A 而不是把离线缺口当作 0。
    pub outcome_improved_available: bool,
}

/// 从 recall_events 直接聚合每类触发的检索/注入覆盖率。
/// 这是运行时可计算的 operational precision/recall，不混入离线 memory_eval。
#[derive(Debug, Clone, PartialEq)]
pub struct RecallMetrics {
    pub trigger_type: String,
    pub events: u64,
    pub retrieved_events: u64,
    pub injected_events: u64,
    pub precision: f64,
    pub recall: f64,
}

/// recall_events 与 episodes 的可复算关联分母。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecallLinkStats {
    pub total: u64,
    pub linked: u64,
    pub orphaned: u64,
}

#[derive(Debug, Clone)]
pub struct RecallEvent<'a> {
    pub recall_id: &'a str,
    pub episode_id: Option<i64>,
    pub step_id: Option<i64>,
    pub trigger_type: &'a str,
    pub trigger_payload: &'a str,
    pub policy_action: &'a str,
    pub query: &'a str,
    pub candidate_ids: &'a str,
    pub retrieved_ids: &'a str,
    pub injected_ids: &'a str,
    pub lexical_ms: u64,
    pub embed_ms: u64,
    pub vector_ms: u64,
    pub total_ms: u64,
}

impl SessionStore {
    pub fn record_recall_event(&self, event: &RecallEvent<'_>) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO recall_events
             (recall_id, episode_id, step_id, trigger_type, trigger_payload, policy_action,
              query, candidate_ids, retrieved_ids, injected_ids, lexical_ms, embed_ms,
              vector_ms, total_ms, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                event.recall_id,
                event.episode_id,
                event.step_id,
                event.trigger_type,
                event.trigger_payload,
                event.policy_action,
                event.query,
                event.candidate_ids,
                event.retrieved_ids,
                event.injected_ids,
                event.lexical_ms as i64,
                event.embed_ms as i64,
                event.vector_ms as i64,
                event.total_ms as i64,
                now_ms(),
            ],
        )?;
        Ok(())
    }

    pub fn record_memory_source(
        &self,
        memory_id: &str,
        episode_id: i64,
        event_start: Option<i64>,
        event_end: Option<i64>,
        source_hash: &str,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT OR IGNORE INTO memory_sources
             (memory_id, episode_id, event_start, event_end, source_hash)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![memory_id, episode_id, event_start, event_end, source_hash],
        )?;
        Ok(())
    }

    /// 返回已有机器 provenance 的记忆 ID，供控制面区分真实来源与 frontmatter 文字。
    pub fn memory_ids_with_sources(&self) -> Result<HashSet<String>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT DISTINCT memory_id FROM memory_sources")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    #[allow(clippy::too_many_arguments)] // 指标列与持久化表一一对应，改为对象会同时破坏导出调用方。
    pub fn record_memory_eval(
        &self,
        memory_id: &str,
        replay_case: &str,
        arm: &str,
        model: &str,
        prompt_version: &str,
        success: bool,
        steps: u64,
        tool_errors: u64,
        retries: u64,
        tokens: u64,
        first_divergence_step: Option<u64>,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO memory_eval
             (memory_id, replay_case, arm, model, prompt_version, success, steps,
              tool_errors, retries, tokens, first_divergence_step, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                memory_id,
                replay_case,
                arm,
                model,
                prompt_version,
                success as i64,
                steps as i64,
                tool_errors as i64,
                retries as i64,
                tokens as i64,
                first_divergence_step.map(|value| value as i64),
                now_ms(),
            ],
        )?;
        Ok(())
    }

    /// 把本轮时间窗内且尚未关联 episode 的 recall_events 回填到该 episode。
    /// 开跑预检索(R-106)发生在 episode 落库之前,写入时没有 episode_id;
    /// 轮末 append_episode 后用本轮开始时间戳回填,recall_events 才能 join episodes(验收①)。
    /// 上界使用目标 episode 的落库时间,避免把 episode 创建之后才产生的下一轮事件
    /// 误归因到上一轮。返回实际回填行数;没有待回填行时静默返回 0。
    pub fn link_recall_events_to_episode(
        &self,
        episode_id: i64,
        since_ms: i64,
    ) -> Result<usize, StoreError> {
        let n = self.connection.execute(
            "UPDATE recall_events SET episode_id = ?1
             WHERE episode_id IS NULL
               AND created_at >= ?2
               AND created_at <= (
                   SELECT created_at FROM episodes WHERE episode_id = ?1
               )",
            params![episode_id, since_ms],
        )?;
        Ok(n)
    }

    /// 机械口径：available 为 active 记忆数(由调用方从记忆库统计后传入——state.db
    /// 不知道文件真源;旧实现数 memory_sources 行数,该表在 provenance 接线前恒为
    /// 空,漏斗首段永远是 0),其余阶段按 state.db 证据去重计数。
    pub fn funnel_counts(&self, available_active: u64) -> Result<FunnelCounts, StoreError> {
        let retrieved = self.connection.query_row(
            "SELECT COUNT(DISTINCT memory_id) FROM (
                 SELECT value AS memory_id FROM recall_events, json_each(retrieved_ids)
             )",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let injected = self.connection.query_row(
            "SELECT COUNT(DISTINCT memory_id) FROM (
                 SELECT value AS memory_id FROM recall_events, json_each(injected_ids)
             )",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        // arm='action_changed' 与 arm='outcome_improved' 是两条独立证据链：
        // 前者表示行为发生变化，后者必须有单独的结果改善证据，不能由前者推导。
        let action_changed = self.connection.query_row(
            "SELECT COUNT(DISTINCT memory_id) FROM memory_eval WHERE arm = 'action_changed' AND success = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let outcome_evidence = self.connection.query_row(
            "SELECT COUNT(*) FROM memory_eval WHERE arm = 'outcome_improved'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let outcome_improved = self.connection.query_row(
            "SELECT COUNT(DISTINCT memory_id) FROM memory_eval WHERE arm = 'outcome_improved' AND success = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(FunnelCounts {
            available: available_active,
            retrieved: retrieved as u64,
            injected: injected as u64,
            action_changed: action_changed as u64,
            outcome_improved: outcome_improved as u64,
            outcome_improved_available: outcome_evidence > 0,
        })
    }

    /// 直接按 recall_events 聚合每类触发的检索/注入覆盖率。
    /// precision = injected / retrieved，recall = retrieved / all trigger events；
    /// miss 作为 retrieved=0 的分母保留，因而可从落库事实复算。
    pub fn recall_metrics(&self) -> Result<Vec<RecallMetrics>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT trigger_type,
                    COUNT(*) AS events,
                    SUM(CASE WHEN json_array_length(retrieved_ids) > 0 THEN 1 ELSE 0 END),
                    SUM(CASE WHEN json_array_length(injected_ids) > 0 THEN 1 ELSE 0 END)
             FROM recall_events
             GROUP BY trigger_type ORDER BY trigger_type",
        )?;
        let rows = statement.query_map([], |row| {
            let events = row.get::<_, i64>(1)? as u64;
            let retrieved = row.get::<_, i64>(2)? as u64;
            let injected = row.get::<_, i64>(3)? as u64;
            Ok(RecallMetrics {
                trigger_type: row.get(0)?,
                events,
                retrieved_events: retrieved,
                injected_events: injected,
                precision: if retrieved == 0 {
                    0.0
                } else {
                    injected as f64 / retrieved as f64
                },
                recall: if events == 0 {
                    0.0
                } else {
                    retrieved as f64 / events as f64
                },
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 直接从 recall_events 聚合关联分母，悬空事件保留在 total/orphaned 中，
    /// 不把无法 join episodes 的数据静默排除。
    pub fn recall_link_stats(&self) -> Result<RecallLinkStats, StoreError> {
        let (total, linked): (i64, i64) = self.connection.query_row(
            "SELECT COUNT(*), SUM(CASE WHEN episode_id IS NOT NULL THEN 1 ELSE 0 END)
             FROM recall_events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let total = total.max(0) as u64;
        let linked = linked.max(0) as u64;
        Ok(RecallLinkStats {
            total,
            linked,
            orphaned: total.saturating_sub(linked),
        })
    }

    /// 按现行 recall_events 聚合每条记忆的召回、注入次数和最后观测时间。
    /// retrieved/injected 都按 recall_id 去重，避免同一事件 JSON 数组重复元素放大信号。
    pub fn memory_recall_profile(
        &self,
    ) -> Result<std::collections::BTreeMap<String, (u64, u64, i64)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT retrieved.value,
                    COUNT(DISTINCT r.recall_id),
                    COUNT(DISTINCT CASE WHEN EXISTS (
                        SELECT 1 FROM json_each(r.injected_ids) injected
                        WHERE injected.value = retrieved.value
                    ) THEN r.recall_id END),
                    MAX(r.created_at)
             FROM recall_events r, json_each(r.retrieved_ids) retrieved
             GROUP BY retrieved.value
             ORDER BY retrieved.value",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?.max(0) as u64,
                row.get::<_, i64>(2)?.max(0) as u64,
                row.get(3)?,
            ))
        })?;
        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(id, recalled, injected, last_at)| (id, (recalled, injected, last_at)))
            .collect())
    }

    /// 返回最近一次 memory_search 的时间、原始 query 与 retrieved id 集，供重复注入抑制。
    /// 去重依据与生产遥测同在 state.db；不再读取 legacy index.db 的 memory_recalls。
    pub fn latest_memory_search(&self) -> Result<Option<(i64, String, Vec<String>)>, StoreError> {
        let row: Option<(i64, String, String)> = self
            .connection
            .query_row(
                "SELECT created_at, query, retrieved_ids FROM recall_events
                 WHERE trigger_type = 'memory_search'
                 ORDER BY created_at DESC, recall_id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((at, query, ids_json)) = row else {
            return Ok(None);
        };
        let ids = serde_json::from_str(&ids_json).unwrap_or_default();
        Ok(Some((at, query, ids)))
    }

    /// 返回 recall_events 里 trigger_type='event_recall' 的行
    /// (recall_id, trigger_payload, policy_action, query),按 created_at 升序。
    pub fn event_recall_log(&self) -> Result<Vec<(String, String, String, String)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT recall_id, trigger_payload, policy_action, query
             FROM recall_events WHERE trigger_type = 'event_recall'
             ORDER BY created_at",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::store;

    #[test]
    fn recall_link_stats_保留悬空事件作为分母() {
        let store = store();
        store
            .record_recall_event(&RecallEvent {
                recall_id: "linked-stat",
                episode_id: None,
                step_id: None,
                trigger_type: "memory_search",
                trigger_payload: "{}",
                policy_action: "lexical",
                query: "q",
                candidate_ids: "[]",
                retrieved_ids: "[]",
                injected_ids: "[]",
                lexical_ms: 0,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 0,
            })
            .unwrap();
        let episode = store
            .append_episode(&crate::store::EpisodeRecord {
                session_id: "stats",
                prompt_head: "p",
                outcome: "ok",
                tools_json: "[]",
                context_json: "{}",
                metrics_json: "{}",
                provider: "",
                model: "",
                run_id: "r",
                input_id: "i",
                overflow_json: "[]",
                ..Default::default()
            })
            .unwrap();
        store
            .record_recall_event(&RecallEvent {
                recall_id: "orphan-stat",
                episode_id: Some(episode),
                step_id: None,
                trigger_type: "event_recall",
                trigger_payload: "{}",
                policy_action: "lexical",
                query: "q",
                candidate_ids: "[]",
                retrieved_ids: "[]",
                injected_ids: "[]",
                lexical_ms: 0,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 0,
            })
            .unwrap();
        assert_eq!(
            store.recall_link_stats().unwrap(),
            RecallLinkStats {
                total: 2,
                linked: 1,
                orphaned: 1,
            }
        );
    }

    #[test]
    fn memory_recall_profile_聚合现行遥测并按事件去重() {
        let store = store();
        let first = RecallEvent {
            recall_id: "r-1",
            episode_id: None,
            step_id: None,
            trigger_type: "memory_search",
            trigger_payload: "{}",
            policy_action: "lexical",
            query: "q",
            candidate_ids: "[\"M-1\",\"M-2\"]",
            retrieved_ids: "[\"M-1\",\"M-2\"]",
            injected_ids: "[\"M-1\"]",
            lexical_ms: 1,
            embed_ms: 0,
            vector_ms: 0,
            total_ms: 1,
        };
        store.record_recall_event(&first).unwrap();
        let second = RecallEvent {
            recall_id: "r-2",
            episode_id: None,
            step_id: None,
            trigger_type: "memory_search",
            trigger_payload: "{}",
            policy_action: "lexical",
            query: "q",
            candidate_ids: "[\"M-1\"]",
            retrieved_ids: "[\"M-1\"]",
            injected_ids: "[]",
            lexical_ms: 1,
            embed_ms: 0,
            vector_ms: 0,
            total_ms: 1,
        };
        store.record_recall_event(&second).unwrap();

        let profile = store.memory_recall_profile().unwrap();
        assert_eq!(profile["M-1"].0, 2);
        assert_eq!(profile["M-1"].1, 1);
        assert_eq!(profile["M-2"].0, 1);
        assert_eq!(profile["M-2"].1, 0);
        assert!(profile["M-1"].2 > 0);
    }

    #[test]
    fn 遥测三表可写并按五段去重统计() {
        let store = store();
        let episode = store
            .append_episode(&crate::store::EpisodeRecord {
                session_id: "ses_test",
                prompt_head: "p",
                outcome: "ok",
                tools_json: "[]",
                context_json: "{}",
                metrics_json: "{}",
                provider: "",
                model: "",
                run_id: "r",
                input_id: "i",
                overflow_json: "[]",
                ..Default::default()
            })
            .unwrap();
        store
            .record_memory_source("M-1", episode, Some(1), Some(2), "hash")
            .unwrap();
        store
            .record_recall_event(&RecallEvent {
                recall_id: "recall-1",
                episode_id: Some(episode),
                step_id: Some(1),
                trigger_type: "tool_failure",
                trigger_payload: "{}",
                policy_action: "lexical",
                query: "cargo test",
                candidate_ids: "[\"M-1\"]",
                retrieved_ids: "[\"M-1\"]",
                injected_ids: "[\"M-1\"]",
                lexical_ms: 1,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 1,
            })
            .unwrap();
        store
            .record_memory_eval(
                "M-1",
                "case",
                "action_changed",
                "test",
                "v1",
                true,
                1,
                0,
                0,
                3,
                None,
            )
            .unwrap();
        store
            .record_memory_eval(
                "M-1",
                "case",
                "outcome_improved",
                "test",
                "v1",
                true,
                1,
                0,
                0,
                3,
                None,
            )
            .unwrap();
        assert_eq!(
            store.funnel_counts(1).unwrap(),
            FunnelCounts {
                available: 1,
                retrieved: 1,
                injected: 1,
                action_changed: 1,
                outcome_improved: 1,
                outcome_improved_available: true,
            }
        );
    }

    #[test]
    fn action_changed_without_outcome_evidence_remains_unavailable() {
        let store = store();
        store
            .record_memory_eval(
                "M-action",
                "case-action",
                "action_changed",
                "test",
                "v1",
                true,
                1,
                0,
                0,
                1,
                None,
            )
            .unwrap();
        let counts = store.funnel_counts(1).unwrap();
        assert_eq!(counts.action_changed, 1);
        assert_eq!(counts.outcome_improved, 0);
        assert!(!counts.outcome_improved_available);
    }

    #[test]
    fn recall_metrics_按触发类型从recall_events计算覆盖率() {
        let store = store();
        for event in [
            RecallEvent {
                recall_id: "metrics-hit",
                episode_id: None,
                step_id: None,
                trigger_type: "tool_failure",
                trigger_payload: "{}",
                policy_action: "lexical",
                query: "failure",
                candidate_ids: "[\"M-1\"]",
                retrieved_ids: "[\"M-1\"]",
                injected_ids: "[\"M-1\"]",
                lexical_ms: 1,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 1,
            },
            RecallEvent {
                recall_id: "metrics-miss",
                episode_id: None,
                step_id: None,
                trigger_type: "tool_failure",
                trigger_payload: "{}",
                policy_action: "miss",
                query: "new failure",
                candidate_ids: "[]",
                retrieved_ids: "[]",
                injected_ids: "[]",
                lexical_ms: 1,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 1,
            },
        ] {
            store.record_recall_event(&event).unwrap();
        }
        let metric = store
            .recall_metrics()
            .unwrap()
            .into_iter()
            .find(|metric| metric.trigger_type == "tool_failure")
            .unwrap();
        assert_eq!(metric.events, 2);
        assert_eq!(metric.retrieved_events, 1);
        assert_eq!(metric.injected_events, 1);
        assert_eq!(metric.precision, 1.0);
        assert_eq!(metric.recall, 0.5);
    }

    #[test]
    fn recall_events_回填episode后可join_episodes查询() {
        // R-161 验收①:recall_events 与 episodes 同库,轮末用本轮开始时间戳回填
        // episode_id,之后能 join episodes 查询(CLI/桌面端同一口径)。
        let store = store();
        // 开跑预检索先落一条 recall_event(episode 尚未创建,episode_id=NULL)。
        store
            .record_recall_event(&RecallEvent {
                recall_id: "memory-search-pre-run",
                episode_id: None,
                step_id: None,
                trigger_type: "memory_search",
                trigger_payload: "{}",
                policy_action: "lexical",
                query: "cargo test",
                candidate_ids: "[\"M-1\"]",
                retrieved_ids: "[\"M-1\"]",
                injected_ids: "[\"M-1\"]",
                lexical_ms: 1,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 1,
            })
            .unwrap();
        // 轮末 append_episode,回填时间窗设为"本轮开始"(0 = 早于一切)。
        let episode = store
            .append_episode(&crate::store::EpisodeRecord {
                session_id: "ses_join",
                prompt_head: "p",
                outcome: "ok",
                tools_json: "[]",
                context_json: "{}",
                metrics_json: "{}",
                provider: "",
                model: "",
                run_id: "r",
                input_id: "i",
                overflow_json: "[]",
                ..Default::default()
            })
            .unwrap();
        let linked = store.link_recall_events_to_episode(episode, 0).unwrap();
        assert_eq!(
            linked, 1,
            "开跑预检索的 recall_event 必须回填到本轮 episode"
        );
        // join 查询:episode 的 prompt_head 与 recall 的 query 同轮可对账。
        let row: Option<(String, String)> = store
            .connection
            .query_row(
                "SELECT e.prompt_head, r.query FROM recall_events r
                 JOIN episodes e ON e.episode_id = r.episode_id
                 WHERE r.recall_id = 'memory-search-pre-run'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        assert_eq!(row, Some(("p".to_string(), "cargo test".to_string())));
        // 时间窗外(比 since 更早)的旧事件不得被误回填。
        store
            .record_recall_event(&RecallEvent {
                recall_id: "stale-recall",
                episode_id: None,
                step_id: None,
                trigger_type: "memory_search",
                trigger_payload: "{}",
                policy_action: "lexical",
                query: "old",
                candidate_ids: "[]",
                retrieved_ids: "[]",
                injected_ids: "[]",
                lexical_ms: 0,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 0,
            })
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE recall_events SET created_at = 1 WHERE recall_id = 'stale-recall'",
                [],
            )
            .unwrap();
        let second = store
            .append_episode(&crate::store::EpisodeRecord {
                session_id: "ses_join",
                prompt_head: "p2",
                outcome: "ok",
                tools_json: "[]",
                context_json: "{}",
                metrics_json: "{}",
                provider: "",
                model: "",
                run_id: "r2",
                input_id: "i2",
                overflow_json: "[]",
                ..Default::default()
            })
            .unwrap();
        // episode 创建之后才产生的事件属于后续轮次，不能被这个回填调用吸收。
        store
            .record_recall_event(&RecallEvent {
                recall_id: "future-recall",
                episode_id: None,
                step_id: None,
                trigger_type: "memory_search",
                trigger_payload: "{}",
                policy_action: "lexical",
                query: "future",
                candidate_ids: "[]",
                retrieved_ids: "[]",
                injected_ids: "[]",
                lexical_ms: 0,
                embed_ms: 0,
                vector_ms: 0,
                total_ms: 0,
            })
            .unwrap();
        let second_created_at: i64 = store
            .connection
            .query_row(
                "SELECT created_at FROM episodes WHERE episode_id = ?1",
                [second],
                |row| row.get(0),
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE recall_events SET created_at = ?1 WHERE recall_id = 'future-recall'",
                [second_created_at + 1_000_000],
            )
            .unwrap();
        let linked_future = store.link_recall_events_to_episode(second, 100).unwrap();
        assert_eq!(linked_future, 0, "episode 之后的事件不得被回填到该 episode");
        let future_episode: Option<i64> = store
            .connection
            .query_row(
                "SELECT episode_id FROM recall_events WHERE recall_id = 'future-recall'",
                [],
                |row| row.get(0),
            )
            .ok();
        assert_eq!(future_episode, None);
        let linked2 = store.link_recall_events_to_episode(second, 100).unwrap();
        assert_eq!(linked2, 0, "时间窗外的旧事件不得被误回填");
    }
}
