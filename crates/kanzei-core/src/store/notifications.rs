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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::store;

    #[test]
    fn 通知和移动端_cursor_跨重建可回放() {
        let store = store();
        let event = crate::AgentNotification {
            event_id: "evt_mobile_1".into(),
            thread_id: "thread_a".into(),
            agent_id: "subagent".into(),
            kind: "agent_status_changed".into(),
            status: "succeeded".into(),
            summary: "完成".into(),
            requires_action: false,
            sequence: 1,
            created_at: 123,
        };
        store.append_notification(&event).unwrap();
        assert_eq!(store.replay_notifications("thread_a", 0, 10).unwrap(), vec![event]);
        assert_eq!(store.delivery_cursor("device_a", "thread_a").unwrap(), 0);
        store.set_delivery_cursor("device_a", "thread_a", 1).unwrap();
        assert_eq!(store.delivery_cursor("device_a", "thread_a").unwrap(), 1);
        assert!(store.replay_notifications("thread_a", 1, 10).unwrap().is_empty());
    }

    #[test]
    fn 并发原子追加通知的_sequence_连续且不丢失() {
        use std::sync::{Arc, Barrier};

        let path = std::env::temp_dir().join(format!(
            "kz-notification-concurrency-{}-{}.db",
            std::process::id(),
            now_ms()
        ));
        let stores = (0..4)
            .map(|_| SessionStore::open(&path).unwrap())
            .collect::<Vec<_>>();
        let barrier = Arc::new(Barrier::new(stores.len()));
        let handles = stores
            .into_iter()
            .enumerate()
            .map(|(worker, store)| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    (0..20)
                        .map(|index| {
                            store
                                .append_notification_atomic(
                                    "thread_concurrent",
                                    "succeeded",
                                    &format!("worker={worker},index={index}"),
                                    false,
                                )
                                .unwrap()
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let notifications = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        let mut sequences = notifications
            .iter()
            .map(|notification| notification.sequence)
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=80).collect::<Vec<_>>());
        assert_eq!(
            SessionStore::open(&path)
                .unwrap()
                .replay_notifications("thread_concurrent", 0, 100)
                .unwrap()
                .len(),
            80
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn 通知的_sequence_冲突不会被静默忽略() {
        let store = store();
        let first = crate::AgentNotification {
            event_id: "notification_first".into(),
            thread_id: "thread_conflict".into(),
            agent_id: "primary".into(),
            kind: "agent_status_changed".into(),
            status: "succeeded".into(),
            summary: "first".into(),
            requires_action: false,
            sequence: 1,
            created_at: now_ms(),
        };
        let mut second = first.clone();
        second.event_id = "notification_second".into();
        store.append_notification(&first).unwrap();
        assert!(store.append_notification(&second).is_err());
        assert_eq!(
            store
                .replay_notifications("thread_conflict", 0, 10)
                .unwrap()
                .len(),
            1
        );
    }
}

