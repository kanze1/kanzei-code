//! 项目级 SQLite 会话事件存储。
//!
//! 存储层只负责持久化事实，不负责 runner 的执行策略。事件序列按 session
//! 独立递增；输入先进入 inbox，只有 runner 在安全边界提升后才成为可见消息。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// v3:episodes 表(R-106,轮次情景摘要;回滚 = DROP TABLE episodes 并把版本改回 2)。
// v5:session_inputs 补 running/completed/failed 终态 + finished_at;
//     episodes 补 provider/model/run_id/input_id/duration_ms(D-173 可观测性)。
const SCHEMA_VERSION: i64 = 5;

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
    /// 库比本二进制新。文案必须给出出路:光说"不兼容"会让人以为库坏了而去删库,
    /// 而删库丢的是全部会话历史——正确动作是把这个二进制升到同一版本。
    #[error(
        "数据库 schema 版本 {found} 高于本程序支持的 {supported}:这个 .kanzei/state.db \
         是更新版本的 kanzei 创建的。请把当前这个程序升到同一版本(桌面端用设置页「检查更新」;\
         CLI 用 cargo install --path crates/kanzei --force),不要删库——降级会丢掉全部会话历史。\
         升级前的库已自动备份为 state.db.v<n>.bak。"
    )]
    UnsupportedSchema { found: i64, supported: i64 },
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

/// 一轮的情景摘要。参数从 9 个涨到 14 个之后位置参数已经不可读了(错位一个
/// 就把 provider 写进 model 也编译得过),所以收成具名结构。
#[derive(Debug, Clone, Default)]
pub struct EpisodeRecord<'a> {
    pub session_id: &'a str,
    pub prompt_head: &'a str,
    pub outcome: &'a str,
    pub steps: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tools_json: &'a str,
    pub context_json: &'a str,
    pub metrics_json: &'a str,
    /// 这一轮实际使用的 provider/model —— 事后复盘不能靠"当前配置"反推。
    pub provider: &'a str,
    pub model: &'a str,
    pub run_id: &'a str,
    pub input_id: &'a str,
    pub duration_ms: u64,
}

pub struct SessionStore {
    connection: Connection,
    /// 落盘路径;内存库为 None。迁移前的备份要用它。
    path: Option<PathBuf>,
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
        let store = Self {
            connection,
            path: Some(path.to_path_buf()),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let mut connection = Connection::open_in_memory()?;
        connection.set_transaction_behavior(TransactionBehavior::Immediate);
        let store = Self {
            connection,
            path: None,
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

    /// Ctrl+C/停止的统一收尾(D-085):恢复 idle、记录 stopped_by_user、
    /// 取消未完成输入,三步在同一事务内原子提交——中断路径不能再留下
    /// 永久 running 的幽灵会话,也不能只做一半。返回被取消的输入数。
    pub fn finalize_interrupt(&self, session_id: &str) -> Result<usize, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE sessions SET status = 'idle', updated_at = ?1 WHERE session_id = ?2",
            params![now_ms(), session_id],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows.into());
        }
        append_event_tx(
            &tx,
            session_id,
            "session.status_changed",
            &serde_json::json!({ "status": "idle", "reason": "stopped_by_user" }),
        )?;
        // 只回收**还没有结局**的输入。completed/failed 是终态,任何一次停止都
        // 不得回头改写它们——否则历史上早已跑完的输入会被追认为 cancelled。
        let cancelled = tx.execute(
            "UPDATE session_inputs SET status = 'cancelled', finished_at = ?1
             WHERE session_id = ?2 AND status IN ('pending', 'promoted', 'running')",
            params![now_ms(), session_id],
        )?;
        tx.commit()?;
        Ok(cancelled)
    }

    /// 取消会话中尚无结局的输入，供停止运行时清理 queue。
    pub fn cancel_unfinished_inputs(&self, session_id: &str) -> Result<usize, StoreError> {
        let changed = self.connection.execute(
            "UPDATE session_inputs SET status = 'cancelled', finished_at = ?1
             WHERE session_id = ?2 AND status IN ('pending', 'promoted', 'running')",
            params![now_ms(), session_id],
        )?;
        Ok(changed)
    }

    /// promoted → running:输入真的开始执行了。
    pub fn start_input(&self, input_id: &str) -> Result<bool, StoreError> {
        let changed = self.connection.execute(
            "UPDATE session_inputs SET status = 'running'
             WHERE input_id = ?1 AND status = 'promoted'",
            params![input_id],
        )?;
        Ok(changed > 0)
    }

    /// running → completed | failed:给输入一个结局,此后任何停止都不再改写它。
    pub fn finish_input(&self, input_id: &str, ok: bool) -> Result<bool, StoreError> {
        let changed = self.connection.execute(
            "UPDATE session_inputs SET status = ?1, finished_at = ?2
             WHERE input_id = ?3 AND status IN ('promoted', 'running')",
            params![if ok { "completed" } else { "failed" }, now_ms(), input_id],
        )?;
        Ok(changed > 0)
    }

    /// 输入的当前状态(审计与测试用)。
    pub fn input_status(&self, input_id: &str) -> Result<Option<String>, StoreError> {
        self.connection
            .query_row(
                "SELECT status FROM session_inputs WHERE input_id = ?1",
                params![input_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
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

    /// 迁移前的整库备份:`state.db.v<旧版本>.bak`。
    ///
    /// 用 `VACUUM INTO` 而不是复制文件——库跑在 WAL 模式下,单独拷 .db 会漏掉
    /// -wal 里尚未 checkpoint 的事务,拷出来的是个残缺快照。VACUUM INTO 由
    /// SQLite 自己生成一致副本。内存库无需备份。
    fn backup_before_upgrade(&self, from_version: i64) -> Result<(), StoreError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let backup = path.with_extension(format!("db.v{from_version}.bak"));
        // 上一次升级尝试留下的同名备份直接覆盖:它描述的是同一个旧版本。
        // VACUUM INTO 要求目标不存在,所以必须先删。
        let _ = std::fs::remove_file(&backup);
        self.connection
            .execute("VACUUM INTO ?1", params![backup.to_string_lossy()])?;
        Ok(())
    }

    /// 已存在的迁移前备份路径(供调用方提示用户如何回退)。
    pub fn backup_path(&self, from_version: i64) -> Option<PathBuf> {
        let backup = self
            .path
            .as_ref()?
            .with_extension(format!("db.v{from_version}.bak"));
        backup.is_file().then_some(backup)
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
                return Err(StoreError::UnsupportedSchema {
                    found: version,
                    supported: SCHEMA_VERSION,
                });
            }
            if version == SCHEMA_VERSION {
                return Ok(());
            }
            // 升级前先留一份旧版本的完整副本。迁移是单向的:一旦升上去,旧二进制
            // 就再也打不开这个库(上面那条 UnsupportedSchema),而桌面端与 CLI 是
            // 两个独立安装通道、可能一新一旧,回退也就无路可走。备份是那条退路。
            self.backup_before_upgrade(version)?;
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
             -- v5 状态机:pending → promoted → running → completed | failed | cancelled。
             -- 少了 running/completed/failed 时,跑完的输入会永远停在 promoted,于是
             -- 用户之后任何一次停止都会把历史上已经成功完成的输入一并改成
             -- cancelled(finalize_interrupt 按 promoted 一刀切),审计语义被彻底破坏。
             CREATE TABLE IF NOT EXISTS session_inputs (
                 input_id TEXT PRIMARY KEY NOT NULL,
                 session_id TEXT NOT NULL REFERENCES sessions(session_id),
                 prompt TEXT NOT NULL,
                 delivery TEXT NOT NULL CHECK(delivery IN ('steer', 'queue')),
                 status TEXT NOT NULL CHECK(status IN
                     ('pending', 'promoted', 'running', 'completed', 'failed', 'cancelled')),
                 created_at INTEGER NOT NULL,
                 promoted_at INTEGER,
                 finished_at INTEGER
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
             CREATE TABLE IF NOT EXISTS episodes (
                 episode_id INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 prompt_head TEXT NOT NULL,
                 outcome TEXT NOT NULL,
                 steps INTEGER NOT NULL,
                 input_tokens INTEGER NOT NULL,
                 output_tokens INTEGER NOT NULL,
                 tools_json TEXT NOT NULL,
                 context_json TEXT NOT NULL,
                 -- v4(R-099):调用画像。旧库靠下面的 ALTER 补列;默认空对象代表
                 -- 那一轮还没开始度量,与度量出来全是零必须区分得开。
                 metrics_json TEXT NOT NULL DEFAULT '{}',
                 -- v5:轮次归属。没有这几列时,连本轮实际模型都只能
                 -- 从当前配置反推,而配置随时会变——事后复盘因此无法证伪。
                 provider TEXT NOT NULL DEFAULT '',
                 model TEXT NOT NULL DEFAULT '',
                 run_id TEXT NOT NULL DEFAULT '',
                 input_id TEXT NOT NULL DEFAULT '',
                 duration_ms INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS episodes_session_created
                 ON episodes(session_id, created_at);
             INSERT INTO schema_meta(key, value) VALUES ('schema_version', '5')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
        )?;
        // 已存在的旧库:上面的 CREATE IF NOT EXISTS 不会改动既有表,逐列补。
        // 列已存在时报错,忽略即可——这是幂等迁移的常规写法。
        for column in [
            "metrics_json TEXT NOT NULL DEFAULT '{}'",
            "provider TEXT NOT NULL DEFAULT ''",
            "model TEXT NOT NULL DEFAULT ''",
            "run_id TEXT NOT NULL DEFAULT ''",
            "input_id TEXT NOT NULL DEFAULT ''",
            "duration_ms INTEGER NOT NULL DEFAULT 0",
        ] {
            let _ = tx.execute(&format!("ALTER TABLE episodes ADD COLUMN {column}"), []);
        }
        // session_inputs 的 status CHECK 写死在建表语句里,ALTER 改不了,只能重建。
        // 只在旧约束still生效时做,重建是幂等的:新库建出来就已经含 running。
        let legacy_check: Option<String> = tx
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'session_inputs'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if legacy_check.is_some_and(|sql| !sql.contains("running")) {
            tx.execute_batch(
                "ALTER TABLE session_inputs RENAME TO session_inputs_v4;
                 CREATE TABLE session_inputs (
                     input_id TEXT PRIMARY KEY NOT NULL,
                     session_id TEXT NOT NULL REFERENCES sessions(session_id),
                     prompt TEXT NOT NULL,
                     delivery TEXT NOT NULL CHECK(delivery IN ('steer', 'queue')),
                     status TEXT NOT NULL CHECK(status IN
                         ('pending', 'promoted', 'running', 'completed', 'failed', 'cancelled')),
                     created_at INTEGER NOT NULL,
                     promoted_at INTEGER,
                     finished_at INTEGER
                 );
                 INSERT INTO session_inputs
                     (input_id, session_id, prompt, delivery, status, created_at, promoted_at, finished_at)
                 SELECT input_id, session_id, prompt, delivery, status, created_at, promoted_at, NULL
                 FROM session_inputs_v4;
                 DROP TABLE session_inputs_v4;
                 CREATE INDEX IF NOT EXISTS session_inputs_pending
                     ON session_inputs(session_id, delivery, status, created_at);",
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 轮次情景摘要(R-106):机械生成的轨迹画像,R-099 度量与记忆系统共用。
    pub fn append_episode(&self, episode: &EpisodeRecord<'_>) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO episodes(session_id, created_at, prompt_head, outcome, steps,
                                  input_tokens, output_tokens, tools_json, context_json, metrics_json,
                                  provider, model, run_id, input_id, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                episode.session_id,
                now_ms(),
                episode.prompt_head.chars().take(200).collect::<String>(),
                episode.outcome,
                episode.steps,
                episode.input_tokens as i64,
                episode.output_tokens as i64,
                episode.tools_json,
                episode.context_json,
                episode.metrics_json,
                episode.provider,
                episode.model,
                episode.run_id,
                episode.input_id,
                episode.duration_ms as i64,
            ],
        )?;
        Ok(())
    }

    /// 最近若干轮的运行归属(D-173):provider/model/run_id/input_id/duration_ms。
    /// 与 `recent_episodes` 分开取,免得那个本已臃肿的元组再长五格。
    #[allow(clippy::type_complexity)]
    pub fn recent_episode_identities(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<(i64, String, String, String, String, u64)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT created_at, provider, model, run_id, input_id, duration_ms
             FROM episodes WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![session_id, limit as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get::<_, i64>(5)? as u64,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 最近若干轮的完整画像(R-099/R-127):按时间倒序。
    /// 返回原始 JSON 字符串,解析交给调用方——存储层不该关心画像的字段构成。
    #[allow(clippy::type_complexity)]
    pub fn recent_episodes(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<(i64, String, String, u32, u64, u64, String, String, String)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT created_at, prompt_head, outcome, steps, input_tokens, output_tokens,
                    tools_json, context_json, metrics_json
             FROM episodes WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![session_id, limit as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, i64>(3)? as u32,
                row.get::<_, i64>(4)? as u64,
                row.get::<_, i64>(5)? as u64,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        })?;
        Ok(rows.flatten().collect())
    }

    /// 最近一轮的上下文账单(context_json 原文),无 episode 时 None。
    pub fn latest_episode_context(&self, session_id: &str) -> Result<Option<String>, StoreError> {
        self.connection
            .query_row(
                "SELECT context_json FROM episodes WHERE session_id = ?1
                 ORDER BY created_at DESC LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// 最近 N 条 episode(新→旧):(created_at, prompt_head, outcome, steps, tools_json)。
    pub fn list_episodes(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<(i64, String, String, u32, String)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT created_at, prompt_head, outcome, steps, tools_json
             FROM episodes WHERE session_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![session_id, limit as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
    fn 中断收尾恢复空闲并原子取消未完成输入() {
        // D-085:Ctrl+C 收尾必须一次做全——状态复位、事件落库、输入取消。
        let store = store();
        store.set_status("ses_test", "running").unwrap();
        store
            .admit_input("ses_test", "input_promoted", "运行中的输入", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses_test").unwrap();
        store
            .admit_input("ses_test", "input_queued", "排队中的输入", Delivery::Queue)
            .unwrap();

        let cancelled = store.finalize_interrupt("ses_test").unwrap();
        assert_eq!(cancelled, 2, "promoted 与 pending 输入都要取消");
        assert_eq!(store.get_session("ses_test").unwrap().unwrap().status, "idle");
        let event = store
            .latest_event("ses_test", "session.status_changed")
            .unwrap()
            .unwrap();
        assert_eq!(event.payload["status"], "idle");
        assert_eq!(event.payload["reason"], "stopped_by_user");
        assert!(store.list_pending_inputs("ses_test").unwrap().is_empty());
        // 不存在的会话必须报错而不是静默成功。
        assert!(matches!(
            store.finalize_interrupt("missing"),
            Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
        ));
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
    fn episode_落库并按时间倒序回放() {
        let store = store();
        store
            .append_episode(&EpisodeRecord {
                session_id: "ses_test",
                prompt_head: "修复 D-068 限流分类",
                outcome: "completed",
                steps: 12,
                input_tokens: 50_000,
                output_tokens: 3_000,
                tools_json: r#"{"bash":5,"edit":3}"#,
                context_json: r#"[["agent/system",1200],["dev/memory",800]]"#,
                metrics_json: r#"{"terminal_calls":5,"edit_calls":3,"edit_misses":1}"#,
                provider: "deepseek",
                model: "deepseek-v4-flash",
                run_id: "run_a",
                input_id: "input_a",
                duration_ms: 708_000,
            })
            .unwrap();
        store
            .append_episode(&EpisodeRecord {
                session_id: "ses_test",
                prompt_head: "第二轮",
                outcome: "halted",
                steps: 3,
                tools_json: "{}",
                context_json: "[]",
                metrics_json: "{}",
                ..EpisodeRecord::default()
            })
            .unwrap();
        let episodes = store.list_episodes("ses_test", 10).unwrap();
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].1, "第二轮");
        assert_eq!(episodes[1].3, 12);
        assert!(episodes[1].4.contains("bash"));
        assert!(store.list_episodes("missing", 10).unwrap().is_empty());

        // R-099:调用画像随轮次落库,并能按时间倒序取回。空对象要与"度量为零"区分开。
        let recent = store.recent_episodes("ses_test", 10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].1, "第二轮");
        assert_eq!(recent[0].8, "{}", "未度量的轮次应保持空对象");
        assert!(recent[1].8.contains("edit_misses"), "画像未随轮次落库: {}", recent[1].8);

        // D-173:轮次归属必须落库。之前只能从"当前配置"反推这一轮跑的哪个模型,
        // 而配置随时会变——复盘时连最基本的事实都无法证伪。
        let identities = store.recent_episode_identities("ses_test", 10).unwrap();
        assert_eq!(identities.len(), 2);
        assert_eq!(identities[1].1, "deepseek");
        assert_eq!(identities[1].2, "deepseek-v4-flash");
        assert_eq!(identities[1].3, "run_a");
        assert_eq!(identities[1].4, "input_a");
        assert_eq!(identities[1].5, 708_000);
    }

    /// D-175:迁移是单向的,升级前必须留下可回退的完整副本;
    /// 遇到更新版本的库时,报错要说清该升哪个程序,而不是让人去删库。
    #[test]
    fn 升级前留下整库备份且更高版本给出可执行指引() {
        let path = std::env::temp_dir().join(format!(
            "kz-migrate-backup-{}-{}.db",
            std::process::id(),
            now_ms()
        ));
        // 造一个 v4 库:建到当前版本后把版本号改回去,再塞一条可验证的数据。
        {
            let store = SessionStore::open(&path).unwrap();
            store.create_session("ses_old", "C:/project", None).unwrap();
            store
                .append_event("ses_old", "conversation.updated", &serde_json::json!({"v": 1}))
                .unwrap();
            store
                .connection
                .execute(
                    "UPDATE schema_meta SET value = '4' WHERE key = 'schema_version'",
                    [],
                )
                .unwrap();
        }

        let store = SessionStore::open(&path).unwrap();
        let backup = store.backup_path(4).expect("升级前必须留下 v4 备份");
        assert!(backup.is_file(), "{}", backup.display());
        // 备份必须是能打开的一致副本(WAL 下直接拷 .db 会拿到残缺快照)。
        let restored = Connection::open(&backup).unwrap();
        let version: String = restored
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "4", "备份必须保留迁移前的版本号");
        let events: i64 = restored
            .query_row("SELECT COUNT(*) FROM session_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(events, 1, "备份必须含有迁移前的数据");
        drop(restored);
        // 迁移本身照常完成。
        assert!(store.get_session("ses_old").unwrap().is_some());
        drop(store);

        // 更新版本的库:旧程序必须拒绝打开,并且文案要给出出路。
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute(
                    "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                    params![SCHEMA_VERSION + 1],
                )
                .unwrap();
        }
        let error = match SessionStore::open(&path) {
            Ok(_) => panic!("更高版本的库必须拒绝打开"),
            Err(error) => error,
        };
        assert!(
            matches!(error, StoreError::UnsupportedSchema { found, supported }
                if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION)
        );
        let text = error.to_string();
        assert!(text.contains("cargo install"), "要告诉 CLI 怎么升: {text}");
        assert!(text.contains("检查更新"), "要告诉桌面端怎么升: {text}");
        assert!(text.contains("不要删库"), "必须堵死删库这条错误动作: {text}");

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let _ = std::fs::remove_file(&backup);
    }

    #[test]
    fn 已完成的输入不会被后来的停止追认为取消() {
        // D-173:少了 completed 终态时,跑完的输入永远停在 promoted,
        // 于是任何一次停止都会把历史成功输入一并改写成 cancelled。
        let store = store();
        store
            .admit_input("ses_test", "done_earlier", "上一轮已完成", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses_test").unwrap();
        assert!(store.start_input("done_earlier").unwrap());
        assert_eq!(store.input_status("done_earlier").unwrap().unwrap(), "running");
        assert!(store.finish_input("done_earlier", true).unwrap());
        assert_eq!(store.input_status("done_earlier").unwrap().unwrap(), "completed");

        store
            .admit_input("ses_test", "in_flight", "本轮被打断", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses_test").unwrap();
        store.start_input("in_flight").unwrap();
        store
            .admit_input("ses_test", "queued", "还没轮到", Delivery::Queue)
            .unwrap();

        store.set_status("ses_test", "running").unwrap();
        assert_eq!(store.finalize_interrupt("ses_test").unwrap(), 2);
        assert_eq!(
            store.input_status("done_earlier").unwrap().unwrap(),
            "completed",
            "已完成的输入必须保持 completed"
        );
        assert_eq!(store.input_status("in_flight").unwrap().unwrap(), "cancelled");
        assert_eq!(store.input_status("queued").unwrap().unwrap(), "cancelled");
        // 终态不可回退:再次 finish 不改写既有结局。
        assert!(!store.finish_input("in_flight", true).unwrap());
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
