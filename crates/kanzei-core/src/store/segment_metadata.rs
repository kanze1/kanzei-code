//! Rebuildable history-list metadata. Events remain the only content authority.
use super::{SessionStore, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::Value;

impl SessionStore {
    /// Changes committed by another connection invalidate a projection being
    /// built here, including deletion/update that need not change MAX(sequence).
    pub fn metadata_snapshot_version(&self) -> Result<i64, StoreError> {
        Ok(self
            .connection
            .query_row("PRAGMA data_version", [], |row| row.get(0))?)
    }

    pub fn cached_segment_metadata(
        &self,
        session: &str,
        range: (i64, i64),
        projection: bool,
    ) -> Result<Option<Value>, StoreError> {
        let text: Option<String> = self.connection.query_row(
            "SELECT metadata_json FROM conversation_segment_metadata WHERE session_id=?1 AND start_sequence=?2 AND end_sequence=?3 AND projection=?4",
            params![session,range.0,range.1,projection], |row| row.get(0),
        ).optional()?;
        text.map(|text| serde_json::from_str(&text).map_err(StoreError::from))
            .transpose()
    }

    /// Returns false when a concurrent commit changed the source while the
    /// caller projected it. Such a value must not become the reusable cache.
    pub fn cache_segment_metadata(
        &self,
        session: &str,
        range: (i64, i64),
        projection: bool,
        version: i64,
        metadata: &Value,
    ) -> Result<bool, StoreError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let current: i64 = tx.query_row("PRAGMA data_version", [], |row| row.get(0))?;
        if current != version {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO conversation_segment_metadata(session_id,start_sequence,end_sequence,projection,metadata_json) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(session_id,start_sequence,end_sequence,projection) DO UPDATE SET metadata_json=excluded.metadata_json",
            params![session,range.0,range.1,projection,serde_json::to_string(metadata)?],
        )?;
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_concurrent_source_commit_cannot_publish_old_metadata() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-segment-cache-{}-{}",
            std::process::id(),
            super::super::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.db");
        let reader = SessionStore::open(&path).unwrap();
        reader.create_session("session", "/fixture", None).unwrap();
        let writer = SessionStore::open(&path).unwrap();
        let version = reader.metadata_snapshot_version().unwrap();
        writer
            .append_event("session", "conversation.reset", &json!({}))
            .unwrap();
        assert!(!reader
            .cache_segment_metadata(
                "session",
                (0, i64::MAX),
                true,
                version,
                &json!({"title":"stale"})
            )
            .unwrap());
        assert!(reader
            .cached_segment_metadata("session", (0, i64::MAX), true)
            .unwrap()
            .is_none());
        let version = reader.metadata_snapshot_version().unwrap();
        assert!(reader
            .cache_segment_metadata("session", (0, i64::MAX), true, version, &Value::Null)
            .unwrap());
        drop(writer);
        drop(reader);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn event_changes_invalidate_only_their_segment_and_empty_metadata_is_cached() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("session", "/fixture", None).unwrap();
        let first = store
            .append_event("session", "conversation.updated", &json!({"messages":[]}))
            .unwrap();
        let boundary = store
            .append_event("session", "conversation.reset", &json!({}))
            .unwrap();
        let version = store.metadata_snapshot_version().unwrap();
        store
            .cache_segment_metadata(
                "session",
                (0, boundary.sequence),
                true,
                version,
                &json!({"title":"old"}),
            )
            .unwrap();
        store
            .cache_segment_metadata(
                "session",
                (boundary.sequence, i64::MAX),
                true,
                version,
                &Value::Null,
            )
            .unwrap();
        assert_eq!(
            store
                .cached_segment_metadata("session", (boundary.sequence, i64::MAX), true)
                .unwrap(),
            Some(Value::Null)
        );
        store
            .append_event("session", "session.turn_started", &json!({}))
            .unwrap();
        assert!(store
            .cached_segment_metadata("session", (boundary.sequence, i64::MAX), true)
            .unwrap()
            .is_none());
        assert!(store
            .cached_segment_metadata("session", (0, boundary.sequence), true)
            .unwrap()
            .is_some());
        store
            .connection
            .execute(
                "DELETE FROM session_events WHERE event_id=?1",
                [&first.event_id],
            )
            .unwrap();
        assert!(store
            .cached_segment_metadata("session", (0, boundary.sequence), true)
            .unwrap()
            .is_none());
    }
}
