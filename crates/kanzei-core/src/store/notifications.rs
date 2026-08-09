//! 通知域(R-155 S3):移动端通知的落库、线程序号与投递 cursor。
//! 方法都在 impl SessionStore 上,connection 字段经 pub(crate) 访问。
//! 已在事务内不得再调自开 tx 的方法(见 mod.rs unchecked_transaction 注)。

use rusqlite::{params, OptionalExtension};

use super::{now_ms, SessionStore, StoreError};

impl SessionStore {
        pub fn append_notification(
            &self,
            notification: &crate::notification::AgentNotification,
        ) -> Result<(), StoreError> {
            self.connection.execute(
                "INSERT INTO agent_notifications
                 (event_id, thread_id, sequence, payload_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(event_id) DO NOTHING",
                params![
                    &notification.event_id,
                    &notification.thread_id,
                    notification.sequence as i64,
                    serde_json::to_string(notification)?,
                    notification.created_at,
                ],
            )?;
            Ok(())
        }
    
        /// 在同一写事务内分配线程序号并插入通知，避免调用方组合读写造成竞态。
        pub fn append_notification_atomic(
            &self,
            thread_id: &str,
            status: &str,
            summary: &str,
            requires_action: bool,
        ) -> Result<crate::notification::AgentNotification, StoreError> {
            let tx = self.connection.unchecked_transaction()?;
            let sequence: i64 = tx.query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1
                 FROM agent_notifications WHERE thread_id = ?1",
                params![thread_id],
                |row| row.get(0),
            )?;
            let notification = crate::notification::AgentNotification {
                event_id: format!("mobile_{thread_id}_{sequence}"),
                thread_id: thread_id.to_string(),
                agent_id: "primary".into(),
                kind: "agent_status_changed".into(),
                status: status.to_string(),
                summary: summary.to_string(),
                requires_action,
                sequence: sequence.max(1) as u64,
                created_at: now_ms(),
            };
            tx.execute(
                "INSERT INTO agent_notifications
                 (event_id, thread_id, sequence, payload_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    &notification.event_id,
                    &notification.thread_id,
                    notification.sequence as i64,
                    serde_json::to_string(&notification)?,
                    notification.created_at,
                ],
            )?;
            tx.commit()?;
            Ok(notification)
        }
        pub fn next_notification_sequence(&self, thread_id: &str) -> Result<u64, StoreError> {
            let sequence: i64 = self.connection.query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM agent_notifications WHERE thread_id = ?1",
                params![thread_id],
                |row| row.get(0),
            )?;
            Ok(sequence.max(1) as u64)
        }
    
        pub fn replay_notifications(
            &self,
            thread_id: &str,
            after_sequence: u64,
            limit: usize,
        ) -> Result<Vec<crate::notification::AgentNotification>, StoreError> {
            let mut statement = self.connection.prepare(
                "SELECT payload_json FROM agent_notifications
                 WHERE thread_id = ?1 AND sequence > ?2 ORDER BY sequence LIMIT ?3",
            )?;
            let rows = statement.query_map(
                params![thread_id, after_sequence as i64, limit as i64],
                |row| {
                    let payload: String = row.get(0)?;
                    serde_json::from_str(&payload).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    
        pub fn delivery_cursor(&self, device_id: &str, thread_id: &str) -> Result<u64, StoreError> {
            let cursor = self
                .connection
                .query_row(
                    "SELECT cursor FROM delivery_cursors WHERE device_id = ?1 AND thread_id = ?2",
                    params![device_id, thread_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .map(|cursor| cursor.max(0) as u64)
                .unwrap_or(0);
            Ok(cursor)
        }
    
        pub fn set_delivery_cursor(
            &self,
            device_id: &str,
            thread_id: &str,
            cursor: u64,
        ) -> Result<(), StoreError> {
            self.connection.execute(
                "INSERT INTO delivery_cursors(device_id, thread_id, cursor, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(device_id, thread_id) DO UPDATE SET cursor = excluded.cursor, updated_at = excluded.updated_at",
                params![device_id, thread_id, cursor as i64, now_ms()],
            )?;
            Ok(())
        }
    }

