//! 记忆漏斗遥测(R-161)。事实写入 state.db，CLI 与桌面端共享此接口。

use rusqlite::params;

use super::{now_ms, SessionStore, StoreError};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FunnelCounts {
    pub available: u64,
    pub retrieved: u64,
    pub injected: u64,
    pub action_changed: u64,
    pub outcome_improved: u64,
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

    /// 把 created_at >= since_ms 且尚未关联 episode 的 recall_events 回填到该 episode。
    /// 开跑预检索(R-106)发生在 episode 落库之前,写入时没有 episode_id;
    /// 轮末 append_episode 后用本轮开始时间戳回填,recall_events 才能 join episodes(验收①)。
    /// 返回实际回填行数;没有待回填行时静默返回 0。
    pub fn link_recall_events_to_episode(
        &self,
        episode_id: i64,
        since_ms: i64,
    ) -> Result<usize, StoreError> {
        let n = self.connection.execute(
            "UPDATE recall_events SET episode_id = ?1
             WHERE episode_id IS NULL AND created_at >= ?2",
            params![episode_id, since_ms],
        )?;
        Ok(n)
    }

    /// 机械口径：available 为 active 记忆数，其余阶段按 state.db 证据去重计数。
    pub fn funnel_counts(&self) -> Result<FunnelCounts, StoreError> {
        let available =
            self.connection
                .query_row("SELECT COUNT(*) FROM memory_sources", [], |row| {
                    row.get::<_, i64>(0)
                })?;
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
        let action_changed = self.connection.query_row(
            "SELECT COUNT(DISTINCT memory_id) FROM memory_eval WHERE arm = 'action_changed' AND success = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        let outcome_improved = self.connection.query_row(
            "SELECT COUNT(DISTINCT memory_id) FROM memory_eval WHERE arm = 'outcome_improved' AND success = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(FunnelCounts {
            available: available as u64,
            retrieved: retrieved as u64,
            injected: injected as u64,
            action_changed: action_changed as u64,
            outcome_improved: outcome_improved as u64,
        })
    }

    /// R-162 事件召回明细查询(可观测性 + 测试断言):
    /// 返回 recall_events 里 trigger_type='event_recall' 的行
    /// (recall_id, trigger_payload, policy_action, query),按 created_at 升序。
    pub fn event_recall_log(
        &self,
    ) -> Result<Vec<(String, String, String, String)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT recall_id, trigger_payload, policy_action, query
             FROM recall_events WHERE trigger_type = 'event_recall'
             ORDER BY created_at",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
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
            store.funnel_counts().unwrap(),
            FunnelCounts {
                available: 1,
                retrieved: 1,
                injected: 1,
                action_changed: 1,
                outcome_improved: 1
            }
        );
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
        let linked = store
            .link_recall_events_to_episode(episode, 0)
            .unwrap();
        assert_eq!(linked, 1, "开跑预检索的 recall_event 必须回填到本轮 episode");
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
        let linked2 = store
            .link_recall_events_to_episode(second, 100)
            .unwrap();
        assert_eq!(linked2, 0, "时间窗外的旧事件不得被误回填");
    }
}
