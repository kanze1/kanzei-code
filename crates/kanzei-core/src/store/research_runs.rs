//! Research run facts and replayable callback events(R-344 B2)。

use rusqlite::{params, OptionalExtension};

use super::{now_ms, ResearchRunEvent, ResearchRunRecord, SessionStore, StoreError};

impl SessionStore {
    pub fn upsert_research_run(&self, run: &ResearchRunRecord) -> Result<(), StoreError> {
        if run.result_id.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "research result_id 不能为空".into(),
            ));
        }
        self.connection.execute(
            "INSERT INTO research_runs(
                 result_id, exploration_id, topic, status, execution_json, policy, lease_id,
                 max_duration_ms, cleanup, started_at, finished_at, exit_code, cancel_reason,
                 params_text, code_ref_json, environment_snapshot_ref, artifacts_json,
                 metrics_last_json, callback_stats_json, heartbeat_at, terminal_log_path
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19, ?20, ?21
             ) ON CONFLICT(result_id) DO UPDATE SET
                 exploration_id = excluded.exploration_id,
                 topic = excluded.topic,
                 status = excluded.status,
                 execution_json = excluded.execution_json,
                 policy = excluded.policy,
                 lease_id = excluded.lease_id,
                 max_duration_ms = excluded.max_duration_ms,
                 cleanup = excluded.cleanup,
                 started_at = excluded.started_at,
                 finished_at = excluded.finished_at,
                 exit_code = excluded.exit_code,
                 cancel_reason = excluded.cancel_reason,
                 params_text = excluded.params_text,
                 code_ref_json = excluded.code_ref_json,
                 environment_snapshot_ref = excluded.environment_snapshot_ref,
                 artifacts_json = excluded.artifacts_json,
                 metrics_last_json = excluded.metrics_last_json,
                 callback_stats_json = excluded.callback_stats_json,
                 heartbeat_at = excluded.heartbeat_at,
                 terminal_log_path = excluded.terminal_log_path",
            params![
                run.result_id,
                run.exploration_id,
                run.topic,
                run.status,
                run.execution_json,
                run.policy,
                run.lease_id,
                run.max_duration_ms,
                run.cleanup,
                run.started_at,
                run.finished_at,
                run.exit_code,
                run.cancel_reason,
                run.params_text,
                run.code_ref_json,
                run.environment_snapshot_ref,
                run.artifacts_json,
                run.metrics_last_json,
                run.callback_stats_json,
                run.heartbeat_at,
                run.terminal_log_path,
            ],
        )?;
        Ok(())
    }

    pub fn get_research_run(
        &self,
        result_id: &str,
    ) -> Result<Option<ResearchRunRecord>, StoreError> {
        self.connection
            .query_row(
                "SELECT result_id, exploration_id, topic, status, execution_json, policy,
                        lease_id, max_duration_ms, cleanup, started_at, finished_at, exit_code,
                        cancel_reason, params_text, code_ref_json, environment_snapshot_ref,
                        artifacts_json, metrics_last_json, callback_stats_json, heartbeat_at,
                        terminal_log_path
                 FROM research_runs WHERE result_id = ?1",
                params![result_id],
                |row| {
                    Ok(ResearchRunRecord {
                        result_id: row.get(0)?,
                        exploration_id: row.get(1)?,
                        topic: row.get(2)?,
                        status: row.get(3)?,
                        execution_json: row.get(4)?,
                        policy: row.get(5)?,
                        lease_id: row.get(6)?,
                        max_duration_ms: row.get(7)?,
                        cleanup: row.get(8)?,
                        started_at: row.get(9)?,
                        finished_at: row.get(10)?,
                        exit_code: row.get(11)?,
                        cancel_reason: row.get(12)?,
                        params_text: row.get(13)?,
                        code_ref_json: row.get(14)?,
                        environment_snapshot_ref: row.get(15)?,
                        artifacts_json: row.get(16)?,
                        metrics_last_json: row.get(17)?,
                        callback_stats_json: row.get(18)?,
                        heartbeat_at: row.get(19)?,
                        terminal_log_path: row.get(20)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn append_research_run_event(
        &self,
        result_id: &str,
        event_type: &str,
        payload_json: &str,
    ) -> Result<ResearchRunEvent, StoreError> {
        if result_id.trim().is_empty() || event_type.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "research run event 的 result_id/event_type 不能为空".into(),
            ));
        }
        let sequence: i64 = self.connection.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM research_run_events WHERE result_id = ?1",
            params![result_id],
            |row| row.get(0),
        )?;
        let created_at = now_ms();
        self.connection.execute(
            "INSERT INTO research_run_events
                 (result_id, sequence, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![result_id, sequence, event_type, payload_json, created_at],
        )?;
        Ok(ResearchRunEvent {
            result_id: result_id.to_string(),
            sequence,
            event_type: event_type.to_string(),
            payload_json: payload_json.to_string(),
            created_at,
        })
    }

    pub fn list_research_run_events(
        &self,
        result_id: &str,
        after_sequence: i64,
    ) -> Result<Vec<ResearchRunEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT result_id, sequence, event_type, payload_json, created_at
             FROM research_run_events
             WHERE result_id = ?1 AND sequence > ?2
             ORDER BY sequence",
        )?;
        let rows = statement.query_map(params![result_id, after_sequence], |row| {
            Ok(ResearchRunEvent {
                result_id: row.get(0)?,
                sequence: row.get(1)?,
                event_type: row.get(2)?,
                payload_json: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::SessionStore;

    fn run() -> ResearchRunRecord {
        ResearchRunRecord {
            result_id: "E-001-01".into(),
            exploration_id: "E-001".into(),
            topic: "nas-search".into(),
            status: "running".into(),
            execution_json: r#"{"kind":"local","command":"python train.py"}"#.into(),
            policy: "relaxed".into(),
            lease_id: "".into(),
            max_duration_ms: 60_000,
            cleanup: "retain".into(),
            started_at: 10,
            finished_at: None,
            exit_code: None,
            cancel_reason: None,
            params_text: "lr=0.1\nseed=3".into(),
            code_ref_json: r#"{"git":"abc123","dirty":false}"#.into(),
            environment_snapshot_ref: "environment.json".into(),
            artifacts_json: "[]".into(),
            metrics_last_json: "{}".into(),
            callback_stats_json: r#"{"parsed":0,"malformed":0,"truncated":0}"#.into(),
            heartbeat_at: Some(10),
            terminal_log_path: "stdout.log".into(),
        }
    }

    #[test]
    fn research_run_facts_and_events_survive_store_reopen() {
        let store = SessionStore::open_in_memory().unwrap();
        store.upsert_research_run(&run()).unwrap();
        store
            .append_research_run_event("E-001-01", "run_started", "{}")
            .unwrap();
        store
            .append_research_run_event("E-001-01", "metric", r#"{"name":"acc","value":0.9}"#)
            .unwrap();
        let restored = store.get_research_run("E-001-01").unwrap().unwrap();
        assert_eq!(restored.params_text, "lr=0.1\nseed=3");
        assert_eq!(
            store.list_research_run_events("E-001-01", 0).unwrap().len(),
            2
        );
        assert_eq!(
            store.list_research_run_events("E-001-01", 1).unwrap()[0].event_type,
            "metric"
        );
    }
}
