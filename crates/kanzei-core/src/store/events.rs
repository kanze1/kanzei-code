//! 事件域(R-155 S4):session_events 的追加/回放/清理。
//! append_event_tx 提 pub(crate):inbox 跨域事务(steer 提升)需要它。
//! 已在事务内不得再调自开 tx 的方法(见 mod.rs unchecked_transaction 注)。

use rusqlite::{params, OptionalExtension, Transaction};

use serde_json::Value;

use super::{now_ms, SessionStore, StoredEvent, StoreError};

impl SessionStore {
        pub fn append_event(
            &self,
            session_id: &str,
            event_type: &str,
            payload: &Value,
        ) -> Result<StoredEvent, StoreError> {
            let tx = self.connection.unchecked_transaction()?;
            let event = append_event_tx(&tx, session_id, event_type, payload)?;
            tx.commit()?;
            Ok(event)
        }
    
        pub fn list_events(
            &self,
            session_id: &str,
            after_sequence: i64,
        ) -> Result<Vec<StoredEvent>, StoreError> {
            let mut statement = self.connection.prepare(
                "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                 FROM session_events WHERE session_id = ?1 AND sequence > ?2 ORDER BY sequence",
            )?;
            let rows = statement.query_map(params![session_id, after_sequence], event_from_row)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    
        /// 按 sequence 删除指定类型的事件(类型限定防止误删调度事件)。返回删除数。
        /// 用途:历史对话管理——快照删除不影响 prompt/session 生命周期事件。
        pub fn delete_events_by_sequence(
            &self,
            session_id: &str,
            event_type: &str,
            sequences: &[i64],
        ) -> Result<usize, StoreError> {
            let tx = self.connection.unchecked_transaction()?;
            let mut deleted = 0usize;
            {
                let mut statement = tx.prepare(
                    "DELETE FROM session_events
                     WHERE session_id = ?1 AND event_type = ?2 AND sequence = ?3",
                )?;
                for sequence in sequences {
                    deleted += statement.execute(params![session_id, event_type, sequence])?;
                }
            }
            tx.commit()?;
            Ok(deleted)
        }
    
        /// 清理当前会话的对话快照，保留 session、调度和权限事件。
        /// CLI 的 `kz run --new` 使用此入口开始新上下文，避免手动删除整个 state.db。
        pub fn clear_conversation(&self, session_id: &str) -> Result<usize, StoreError> {
            self.connection
                .execute(
                    "DELETE FROM session_events WHERE session_id = ?1 AND event_type = 'conversation.updated'",
                    params![session_id],
                )
                .map_err(Into::into)
        }
    
        pub fn latest_event(
            &self,
            session_id: &str,
            event_type: &str,
        ) -> Result<Option<StoredEvent>, StoreError> {
            self.connection
                .query_row(
                    "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                     FROM session_events
                     WHERE session_id = ?1 AND event_type = ?2
                     ORDER BY sequence DESC LIMIT 1",
                    params![session_id, event_type],
                    event_from_row,
                )
                .optional()
                .map_err(Into::into)
        }
    
}

/// 事件行解析。
fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
        let payload_json: String = row.get(4)?;
        Ok(StoredEvent {
            event_id: row.get(0)?,
            session_id: row.get(1)?,
            sequence: row.get(2)?,
            event_type: row.get(3)?,
            payload: serde_json::from_str(&payload_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
            created_at: row.get(5)?,
        })
    }

/// 在同一事务内追加事件并刷新会话 updated_at(S4 提 pub(crate))。
pub(crate) fn append_event_tx(
        tx: &Transaction<'_>,
        session_id: &str,
        event_type: &str,
        payload: &Value,
    ) -> Result<StoredEvent, StoreError> {
        let sequence: i64 = tx.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM session_events WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        let created_at = now_ms();
        let event_id = format!("evt_{}_{}", session_id, sequence);
        tx.execute(
            "INSERT INTO session_events(event_id, session_id, sequence, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![event_id, session_id, sequence, event_type, serde_json::to_string(payload)?, created_at],
        )?;
        tx.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE session_id = ?2",
            params![created_at, session_id],
        )?;
        Ok(StoredEvent {
            event_id,
            session_id: session_id.to_string(),
            sequence,
            event_type: event_type.to_string(),
            payload: payload.clone(),
            created_at,
        })
    }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::store;

    #[test]
    fn 事件序列按会话递增并可回放() {
        let store = store();
        let first = store
            .append_event(
                "ses_test",
                "session.created",
                &serde_json::json!({"ok": true}),
            )
            .unwrap();
        let second = store
            .append_event("ses_test", "turn.started", &serde_json::json!({"step": 1}))
            .unwrap();
        assert_eq!((first.sequence, second.sequence), (1, 2));
        assert_eq!(store.list_events("ses_test", 1).unwrap().len(), 1);
    }

    #[test]
    fn latest_event_按类型返回最新事件() {
        let store = store();
        store
            .append_event("ses_test", "conversation.updated", &serde_json::json!({"v": 1}))
            .unwrap();
        store
            .append_event("ses_test", "run.completed", &serde_json::json!({}))
            .unwrap();
        store
            .append_event("ses_test", "conversation.updated", &serde_json::json!({"v": 2}))
            .unwrap();
        let latest = store
            .latest_event("ses_test", "conversation.updated")
            .unwrap()
            .unwrap();
        assert_eq!(latest.payload["v"], 2);
        assert!(store.latest_event("ses_test", "missing").unwrap().is_none());
    }

    #[test]
    fn clear_conversation_只删除对话快照() {
        let store = store();
        store
            .append_event("ses_test", "conversation.updated", &serde_json::json!({"v": 1}))
            .unwrap();
        store
            .append_event("ses_test", "session.status_changed", &serde_json::json!({"status": "idle"}))
            .unwrap();
        assert_eq!(store.clear_conversation("ses_test").unwrap(), 1);
        assert!(store.latest_event("ses_test", "conversation.updated").unwrap().is_none());
        assert!(store.latest_event("ses_test", "session.status_changed").unwrap().is_some());
    }

    #[test]
    fn 不同会话的事件_id_保持唯一() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        let first = store
            .append_event("ses_test", "turn.started", &serde_json::json!({}))
            .unwrap();
        let second = store
            .append_event("ses_other", "turn.started", &serde_json::json!({}))
            .unwrap();
        assert_ne!(first.event_id, second.event_id);
    }

    #[test]
    fn r050_poc_不同会话事件回放互不串线() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        store
            .append_event("ses_test", "conversation.updated", &serde_json::json!({"thread": "a"}))
            .unwrap();
        store
            .append_event("ses_other", "conversation.updated", &serde_json::json!({"thread": "b"}))
            .unwrap();

        let a = store.list_events("ses_test", 0).unwrap();
        let b = store.list_events("ses_other", 0).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].payload["thread"], "a");
        assert_eq!(b[0].payload["thread"], "b");
        assert_eq!(a[0].sequence, 1);
        assert_eq!(b[0].sequence, 1);
    }

    #[test]
    fn 并发追加事件的_sequence_连续且唯一() {
        let path = std::env::temp_dir().join(format!(
            "kz-store-concurrency-{}-{}.db",
            std::process::id(),
            now_ms()
        ));
        let initializer = SessionStore::open(&path).unwrap();
        initializer
            .create_session("ses_concurrent", "C:/project", None)
            .unwrap();
        drop(initializer);

        let stores = (0..4)
            .map(|_| SessionStore::open(&path).unwrap())
            .collect::<Vec<_>>();
        let handles = stores
            .into_iter()
            .enumerate()
            .map(|(worker, store)| {
                std::thread::spawn(move || {
                    (0..20)
                        .map(|index| {
                            store
                                .append_event(
                                    "ses_concurrent",
                                    "test.concurrent",
                                    &serde_json::json!({"worker": worker, "index": index}),
                                )
                                .unwrap()
                                .sequence
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let mut sequences = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=80).collect::<Vec<_>>());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
}

