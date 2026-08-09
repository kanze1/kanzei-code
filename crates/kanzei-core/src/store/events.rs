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
