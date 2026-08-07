//! 项目级 SQLite 会话事件存储。
//!
//! 存储层只负责持久化事实，不负责 runner 的执行策略。事件序列按 session
//! 独立递增；输入先进入 inbox，只有 runner 在安全边界提升后才成为可见消息。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SCHEMA_VERSION: i64 = 2;

pub fn project_state_path(project_root: &Path) -> PathBuf {
    project_root.join(".kanzei").join("state.db")
}

pub fn project_session_id(project_root: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    project_root
        .to_string_lossy()
        .to_lowercase()
        .hash(&mut hasher);
    format!("ses_project_{:016x}", hasher.finish())
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("数据库 schema 版本不兼容: {0}")]
    UnsupportedSchema(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub session_id: String,
    pub project_root: String,
    pub title: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredEvent {
    pub event_id: String,
    pub session_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    Steer,
    Queue,
}

impl Delivery {
    fn as_str(self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdmittedInput {
    pub input_id: String,
    pub session_id: String,
    pub prompt: String,
    pub delivery: Delivery,
    pub created_at: i64,
}

pub struct SessionStore {
    connection: Connection,
}

impl SessionStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        }
        let mut connection = Connection::open(path)?;
        // 同一个 state.db 会被大量短命连接并发读写(每个 Tauri 命令、每次运行、移动端线程各开一条)。
        // 默认 rollback journal + busy_timeout=0 会让并发写直接 SQLITE_BUSY,
        // 表现为运行成功却报失败、事件丢失、入队被拒(D-064)。
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.set_transaction_behavior(TransactionBehavior::Immediate);
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut connection = Connection::open_in_memory()?;
        connection.set_transaction_behavior(TransactionBehavior::Immediate);
        let store = Self {
            connection,
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn create_session(
        &self,
        session_id: &str,
        project_root: &str,
        title: Option<&str>,
    ) -> Result<Session, StoreError> {
        let now = now_ms();
        self.connection.execute(
            "INSERT INTO sessions(session_id, project_root, title, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'idle', ?4, ?4)
             ON CONFLICT(session_id) DO NOTHING",
            params![session_id, project_root, title, now],
        )?;
        self.get_session(session_id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows.into())
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>, StoreError> {
        self.connection
            .query_row(
                "SELECT session_id, project_root, title, status, created_at, updated_at
                 FROM sessions WHERE session_id = ?1",
                params![session_id],
                |row| {
                    Ok(Session {
                        session_id: row.get(0)?,
                        project_root: row.get(1)?,
                        title: row.get(2)?,
                        status: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// 更新会话生命周期状态，并同步更新时间。
    ///
    /// 状态值由 runner 约定为 `idle`、`running`、`failed`；存储层不限制
    /// 未来新增的状态，以便迁移时保持向后兼容。
    pub fn set_status(&self, session_id: &str, status: &str) -> Result<(), StoreError> {
        let changed = self.connection.execute(
            "UPDATE sessions SET status = ?1, updated_at = ?2 WHERE session_id = ?3",
            params![status, now_ms(), session_id],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows.into());
        }
        Ok(())
    }

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

    pub fn admit_input(
        &self,
        session_id: &str,
        input_id: &str,
        prompt: &str,
        delivery: Delivery,
    ) -> Result<AdmittedInput, StoreError> {
        let now = now_ms();
        self.connection.execute(
            "INSERT INTO session_inputs(input_id, session_id, prompt, delivery, status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5)
             ON CONFLICT(input_id) DO NOTHING",
            params![input_id, session_id, prompt, delivery.as_str(), now],
        )?;
        self.connection
            .query_row(
                "SELECT input_id, session_id, prompt, delivery, created_at
                 FROM session_inputs WHERE input_id = ?1",
                params![input_id],
                input_from_row,
            )
            .map_err(Into::into)
    }

    pub fn has_pending(&self, session_id: &str, delivery: Delivery) -> Result<bool, StoreError> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM session_inputs WHERE session_id = ?1 AND delivery = ?2 AND status = 'pending'",
            params![session_id, delivery.as_str()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// 取消尚未提升的输入。已提升或已取消的输入不会被回收，避免篡改调度事实。
    /// 返回尚未提升的输入,按 admission 顺序展示给前端队列面板。
    pub fn list_pending_inputs(&self, session_id: &str) -> Result<Vec<AdmittedInput>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT input_id, session_id, prompt, delivery, created_at
             FROM session_inputs
             WHERE session_id = ?1 AND status = 'pending'
             ORDER BY created_at, rowid",
        )?;
        let rows = statement.query_map(params![session_id], input_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn cancel_input(&self, session_id: &str, input_id: &str) -> Result<bool, StoreError> {
        let changed = self.connection.execute(
            "UPDATE session_inputs SET status = 'cancelled'
             WHERE session_id = ?1 AND input_id = ?2 AND status = 'pending'",
            params![session_id, input_id],
        )?;
        Ok(changed > 0)
    }

    /// 取消会话中全部尚未提升的输入，供停止运行时清理 queue。
    /// 停止运行时取消尚未完成的输入，包括已提升但尚未完成执行的输入。
    pub fn cancel_unfinished_inputs(&self, session_id: &str) -> Result<usize, StoreError> {
        let changed = self.connection.execute(
            "UPDATE session_inputs SET status = 'cancelled'
             WHERE session_id = ?1 AND status IN ('pending', 'promoted')",
            params![session_id],
        )?;
        Ok(changed)
    }
    pub fn cancel_pending_inputs(&self, session_id: &str) -> Result<usize, StoreError> {
        let changed = self.connection.execute(
            "UPDATE session_inputs SET status = 'cancelled'
             WHERE session_id = ?1 AND status = 'pending'",
            params![session_id],
        )?;
        Ok(changed)
    }

    pub fn promote_steers(&self, session_id: &str) -> Result<Vec<AdmittedInput>, StoreError> {
        self.promote_where(session_id, "steer", false)
    }

    pub fn promote_next_queue(
        &self,
        session_id: &str,
    ) -> Result<Option<AdmittedInput>, StoreError> {
        Ok(self
            .promote_where(session_id, "queue", true)?
            .into_iter()
            .next())
    }

    fn promote_next_steer(
        &self,
        session_id: &str,
    ) -> Result<Option<AdmittedInput>, StoreError> {
        Ok(self
            .promote_where(session_id, "steer", true)?
            .into_iter()
            .next())
    }

    /// 优先提升一条 steer；没有 steer 时再提升一条 queue，供运行边界 drain 使用。
    pub fn promote_next_input(
        &self,
        session_id: &str,
    ) -> Result<Option<AdmittedInput>, StoreError> {
        if let Some(input) = self.promote_next_steer(session_id)? {
            return Ok(Some(input));
        }
        self.promote_next_queue(session_id)
    }

    fn promote_where(
        &self,
        session_id: &str,
        delivery: &str,
        one: bool,
    ) -> Result<Vec<AdmittedInput>, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let limit = if one { " LIMIT 1" } else { "" };
        let sql = format!(
            "SELECT input_id, session_id, prompt, delivery, created_at
             FROM session_inputs WHERE session_id = ?1 AND delivery = ?2 AND status = 'pending'
             ORDER BY created_at, rowid{limit}"
        );
        let inputs = {
            let mut statement = tx.prepare(&sql)?;
            let rows = statement.query_map(params![session_id, delivery], input_from_row)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for input in &inputs {
            tx.execute(
                "UPDATE session_inputs SET status = 'promoted', promoted_at = ?1
                 WHERE input_id = ?2 AND status = 'pending'",
                params![now_ms(), input.input_id],
            )?;
        }
        tx.commit()?;
        Ok(inputs)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        self.connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS schema_meta (
                 key TEXT PRIMARY KEY NOT NULL,
                 value TEXT NOT NULL
             );",
        )?;
        let current: Option<i64> = self
            .connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| value.parse().unwrap_or_default());
        if let Some(version) = current {
            if version > SCHEMA_VERSION {
                return Err(StoreError::UnsupportedSchema(version));
            }
            if version == SCHEMA_VERSION {
                return Ok(());
            }
        }
        let tx = self.connection.unchecked_transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                 session_id TEXT PRIMARY KEY NOT NULL,
                 project_root TEXT NOT NULL,
                 title TEXT,
                 status TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS session_events (
                 event_id TEXT PRIMARY KEY NOT NULL,
                 session_id TEXT NOT NULL REFERENCES sessions(session_id),
                 sequence INTEGER NOT NULL,
                 event_type TEXT NOT NULL,
                 payload_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 UNIQUE(session_id, sequence)
             );
             CREATE INDEX IF NOT EXISTS session_events_session_sequence
                 ON session_events(session_id, sequence);
             CREATE TABLE IF NOT EXISTS session_inputs (
                 input_id TEXT PRIMARY KEY NOT NULL,
                 session_id TEXT NOT NULL REFERENCES sessions(session_id),
                 prompt TEXT NOT NULL,
                 delivery TEXT NOT NULL CHECK(delivery IN ('steer', 'queue')),
                 status TEXT NOT NULL CHECK(status IN ('pending', 'promoted', 'cancelled')),
                 created_at INTEGER NOT NULL,
                 promoted_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS session_inputs_pending
                 ON session_inputs(session_id, delivery, status, created_at);
             CREATE TABLE IF NOT EXISTS agent_notifications (
                 event_id TEXT PRIMARY KEY NOT NULL,
                 thread_id TEXT NOT NULL,
                 sequence INTEGER NOT NULL,
                 payload_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 UNIQUE(thread_id, sequence)
             );
             CREATE INDEX IF NOT EXISTS agent_notifications_thread_sequence
                 ON agent_notifications(thread_id, sequence);
             CREATE TABLE IF NOT EXISTS delivery_cursors (
                 device_id TEXT NOT NULL,
                 thread_id TEXT NOT NULL,
                 cursor INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 PRIMARY KEY(device_id, thread_id)
             );
             INSERT INTO schema_meta(key, value) VALUES ('schema_version', '2')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn append_event_tx(
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

fn input_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdmittedInput> {
    let delivery: String = row.get(3)?;
    let delivery = match delivery.as_str() {
        "steer" => Delivery::Steer,
        "queue" => Delivery::Queue,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(AdmittedInput {
        input_id: row.get(0)?,
        session_id: row.get(1)?,
        prompt: row.get(2)?,
        delivery,
        created_at: row.get(4)?,
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间必须晚于 Unix epoch")
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SessionStore {
        let store = SessionStore::open_in_memory().unwrap();
        store
            .create_session("ses_test", "C:/project", None)
            .unwrap();
        store
    }

    #[test]
    fn 会话状态更新并刷新时间() {
        let store = store();
        let before = store.get_session("ses_test").unwrap().unwrap();
        store.set_status("ses_test", "running").unwrap();
        let after = store.get_session("ses_test").unwrap().unwrap();
        assert_eq!(after.status, "running");
        assert!(after.updated_at >= before.updated_at);
        assert!(matches!(
            store.set_status("missing", "running"),
            Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
        ));
    }

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
    fn steer_合并且_queue_保持_fifo() {
        let store = store();
        store
            .admit_input("ses_test", "i1", "s1", Delivery::Steer)
            .unwrap();
        store
            .admit_input("ses_test", "i2", "q1", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "i3", "s2", Delivery::Steer)
            .unwrap();
        let steers = store.promote_steers("ses_test").unwrap();
        assert_eq!(
            steers.iter().map(|x| x.prompt.as_str()).collect::<Vec<_>>(),
            ["s1", "s2"]
        );
        assert_eq!(
            store
                .promote_next_queue("ses_test")
                .unwrap()
                .unwrap()
                .prompt,
            "q1"
        );
        assert!(!store.has_pending("ses_test", Delivery::Steer).unwrap());
    }

    #[test]
    fn drain_优先提升_steer_再取_queue() {
        let store = store();
        store
            .admit_input("ses_test", "q1", "队列", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "s1", "插入", Delivery::Steer)
            .unwrap();
        assert_eq!(
            store
                .promote_next_input("ses_test")
                .unwrap()
                .unwrap()
                .delivery,
            Delivery::Steer
        );
        assert_eq!(
            store
                .promote_next_input("ses_test")
                .unwrap()
                .unwrap()
                .delivery,
            Delivery::Queue
        );
    }

    #[test]
    fn drain_依次提升全部_steer_再取_queue() {
        let store = store();
        store
            .admit_input("ses_test", "s1", "插入一", Delivery::Steer)
            .unwrap();
        store
            .admit_input("ses_test", "s2", "插入二", Delivery::Steer)
            .unwrap();
        store
            .admit_input("ses_test", "q1", "队列", Delivery::Queue)
            .unwrap();

        let prompts = (0..3)
            .map(|_| store.promote_next_input("ses_test").unwrap().unwrap().prompt)
            .collect::<Vec<_>>();
        assert_eq!(prompts, ["插入一", "插入二", "队列"]);
        assert!(!store.has_pending("ses_test", Delivery::Steer).unwrap());
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
    }

    #[test]
    fn 重复_admission_是幂等的() {
        let store = store();
        let first = store
            .admit_input("ses_test", "same", "prompt", Delivery::Queue)
            .unwrap();
        let second = store
            .admit_input("ses_test", "same", "other", Delivery::Steer)
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn 只能取消尚未提升的输入() {
        let store = store();
        store
            .admit_input("ses_test", "pending", "待取消", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "promoted", "已提升", Delivery::Queue)
            .unwrap();
        assert!(store.cancel_input("ses_test", "pending").unwrap());
        assert!(!store.cancel_input("ses_test", "pending").unwrap());
        assert!(!store.cancel_input("ses_test", "missing").unwrap());
        assert!(store.has_pending("ses_test", Delivery::Queue).unwrap());

        store.promote_next_queue("ses_test").unwrap();
        assert!(!store.cancel_input("ses_test", "promoted").unwrap());
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
    }

    #[test]
    fn 停止时取消_pending_和已_promoted_输入() {
        let store = store();
        store
            .admit_input("ses_test", "pending", "待执行", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "promoted", "已提升未完成", Delivery::Queue)
            .unwrap();
        let promoted = store.promote_next_input("ses_test").unwrap().unwrap();
        assert_eq!(promoted.input_id, "pending");
        assert_eq!(store.cancel_unfinished_inputs("ses_test").unwrap(), 2);
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
        assert!(!store.cancel_input("ses_test", "promoted").unwrap());
    }

    #[test]
    fn 停止运行时只取消本会话的_pending_输入() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        store
            .admit_input("ses_test", "q1", "当前会话", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_other", "q2", "其他会话", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses_test", "s1", "已提升", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses_test").unwrap();

        assert_eq!(store.cancel_pending_inputs("ses_test").unwrap(), 1);
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
        assert!(store.has_pending("ses_other", Delivery::Queue).unwrap());
        assert!(!store.cancel_input("ses_test", "s1").unwrap());
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
    fn r050_poc_停止一个会话不影响另一个会话队列() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        store.admit_input("ses_test", "a-pending", "A", Delivery::Queue).unwrap();
        store.admit_input("ses_other", "b-pending", "B", Delivery::Queue).unwrap();
        store.admit_input("ses_other", "b-steer", "B steer", Delivery::Steer).unwrap();

        assert_eq!(store.cancel_pending_inputs("ses_test").unwrap(), 1);
        assert!(!store.has_pending("ses_test", Delivery::Queue).unwrap());
        assert!(store.has_pending("ses_other", Delivery::Queue).unwrap());
        assert_eq!(store.promote_next_input("ses_other").unwrap().unwrap().prompt, "B steer");
        assert_eq!(store.promote_next_input("ses_other").unwrap().unwrap().prompt, "B");
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
    }    #[test]
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
