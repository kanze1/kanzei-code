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
}
