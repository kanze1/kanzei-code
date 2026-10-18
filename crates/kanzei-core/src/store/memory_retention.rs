//! Versioned availability is the denominator for cold-memory decisions.
use super::{SessionStore, StoreError};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableMemory {
    pub id: String,
    pub revision: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MemoryRetentionCounts {
    pub opportunities: u64,
    pub first_opportunity_at: i64,
    pub last_opportunity_at: i64,
    pub retrieved_runs: u64,
    pub last_retrieved_at: Option<i64>,
}

impl SessionStore {
    pub fn record_memory_availability(
        &self,
        session: &str,
        run_id: &str,
        entries: &[AvailableMemory],
    ) -> Result<bool, StoreError> {
        if run_id.is_empty()
            || entries
                .iter()
                .any(|entry| entry.id.is_empty() || entry.revision.is_empty())
        {
            return Err(StoreError::InvalidInput(
                "memory availability requires run and versioned identities".into(),
            ));
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM session_events WHERE session_id=?1 AND event_type='memory.availability' AND json_extract(payload_json,'$.run_id')=?2)",params![session, run_id], |row| row.get(0))?;
        if !exists {
            super::events::append_event_tx(
                &tx,
                session,
                "memory.availability",
                &serde_json::json!({"run_id":run_id,"entries":entries}),
            )?;
        }
        tx.commit()?;
        Ok(!exists)
    }

    pub fn memory_retention_counts(
        &self,
    ) -> Result<BTreeMap<(String, String), MemoryRetentionCounts>, StoreError> {
        let mut statement = self.connection.prepare(
            "WITH available AS (
                SELECT json_extract(e.value,'$.id') AS id,json_extract(e.value,'$.revision') AS revision,
                    json_extract(s.payload_json,'$.run_id') AS run_id,s.created_at AS at
                FROM session_events s,json_each(s.payload_json,'$.entries') e WHERE s.event_type='memory.availability'
            ) SELECT a.id,a.revision,COUNT(DISTINCT a.run_id),MIN(a.at),MAX(a.at),
                COUNT(DISTINCT CASE WHEN EXISTS(SELECT 1 FROM json_each(r.retrieved_ids) WHERE value=a.id) THEN a.run_id END),
                MAX(CASE WHEN EXISTS(SELECT 1 FROM json_each(r.retrieved_ids) WHERE value=a.id) THEN r.created_at END)
              FROM available a LEFT JOIN recall_events r ON r.run_id=a.run_id AND r.trigger_type!='user_search'
              GROUP BY a.id,a.revision"
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                (row.get(0)?, row.get(1)?),
                MemoryRetentionCounts {
                    opportunities: row.get::<_, i64>(2)?.max(0) as u64,
                    first_opportunity_at: row.get(3)?,
                    last_opportunity_at: row.get(4)?,
                    retrieved_runs: row.get::<_, i64>(5)?.max(0) as u64,
                    last_retrieved_at: row.get(6)?,
                },
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn availability_deduplicates_runs_and_does_not_mix_content_versions() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("s", ".", None).unwrap();
        let entry = |revision: &str| AvailableMemory {
            id: "M-1".into(),
            revision: revision.into(),
        };
        assert!(store
            .record_memory_availability("s", "run-1", &[entry("v1")])
            .unwrap());
        assert!(!store
            .record_memory_availability("s", "run-1", &[entry("v1")])
            .unwrap());
        store
            .record_memory_availability("s", "run-2", &[entry("v2")])
            .unwrap();
        let counts = store.memory_retention_counts().unwrap();
        assert_eq!(counts[&("M-1".into(), "v1".into())].opportunities, 1);
        assert_eq!(counts[&("M-1".into(), "v2".into())].opportunities, 1);
        assert_eq!(counts[&("M-1".into(), "v1".into())].retrieved_runs, 0);
        let recall = crate::RecallEvent {
            recall_id: "used-v1",
            episode_id: None,
            step_id: None,
            trigger_type: "memory_search",
            trigger_payload: "{}",
            policy_action: "lexical",
            query: "fact",
            candidate_ids: "[\"M-1\"]",
            retrieved_ids: "[\"M-1\"]",
            injected_ids: "[]",
            lexical_ms: 0,
            embed_ms: 0,
            vector_ms: 0,
            total_ms: 0,
        };
        store
            .record_recall_event_for_run(&recall, Some("run-1"))
            .unwrap();
        let counts = store.memory_retention_counts().unwrap();
        assert_eq!(counts[&("M-1".into(), "v1".into())].retrieved_runs, 1);
        assert!(counts[&("M-1".into(), "v1".into())]
            .last_retrieved_at
            .is_some());
        assert_eq!(counts[&("M-1".into(), "v2".into())].retrieved_runs, 0);
    }
}
