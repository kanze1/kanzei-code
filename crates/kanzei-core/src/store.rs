//! 项目级 SQLite 会话事件存储。
//!
//! 存储层只负责持久化事实，不负责 runner 的执行策略。事件序列按 session
//! 独立递增；输入先进入 inbox，只有 runner 在安全边界提升后才成为可见消息。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const SCHEMA_VERSION: i64 = 1;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
        let connection = Connection::open(path)?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let store = Self {
            connection: Connection::open_in_memory()?,
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
    pub fn cancel_input(&self, session_id: &str, input_id: &str) -> Result<bool, StoreError> {
        let changed = self.connection.execute(
            "UPDATE session_inputs SET status = 'cancelled'
             WHERE session_id = ?1 AND input_id = ?2 AND status = 'pending'",
            params![session_id, input_id],
        )?;
        Ok(changed > 0)
    }

    /// 取消会话中全部尚未提升的输入，供停止运行时清理 queue。
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
             INSERT INTO schema_meta(key, value) VALUES ('schema_version', '1')
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
}
