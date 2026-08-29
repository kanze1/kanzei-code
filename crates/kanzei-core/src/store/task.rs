//! Task 生命周期事实(R-338 B1)：只追加到 `session_events`，不改写旧事实。
//!
//! task 的身份与关闭事实必须显式产生；本模块不从 prompt、时间间隔或 session
//! 边界推断 task。`task_event_id` 是调用方可重放的幂等键，原始存储事件仍由
//! `SessionStore::append_event_tx` 统一分配 session 内 sequence。

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    events::append_event_tx, events::event_from_row, now_ms, SessionStore, StoreError, StoredEvent,
};

pub const TASK_STARTED_EVENT_TYPE: &str = "task.started";
pub const TASK_MEMBERSHIP_ADDED_EVENT_TYPE: &str = "task.membership_added";
pub const TASK_CLOSED_EVENT_TYPE: &str = "task.closed";
pub const TASK_EVENT_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    Completed,
    Failed,
    Cancelled,
    Abandoned,
}

impl TaskOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Abandoned => "abandoned",
        }
    }
}

impl SessionStore {
    /// 显式创建 task。重复提交同一 task 的 start 事实只返回第一次事件。
    pub fn append_task_started(
        &self,
        session_id: &str,
        task_id: &str,
        title: Option<&str>,
        input_id: Option<&str>,
    ) -> Result<StoredEvent, StoreError> {
        validate_identifier("session_id", session_id)?;
        validate_identifier("task_id", task_id)?;
        let task_event_id = format!("{task_id}:started");
        self.append_task_event(
            session_id,
            TASK_STARTED_EVENT_TYPE,
            &task_event_id,
            serde_json::json!({
                "schema_version": TASK_EVENT_SCHEMA_VERSION,
                "task_event_id": task_event_id,
                "task_id": task_id,
                "session_id": session_id,
                "title": title,
                "input_id": input_id,
                "started_at": now_ms(),
            }),
        )
    }

    /// 为 task 增加一条显式 membership。跨 session 归属只能通过本入口发生。
    pub fn append_task_membership_added(
        &self,
        session_id: &str,
        task_id: &str,
        membership_id: &str,
        input_id: Option<&str>,
        episode_id: Option<i64>,
    ) -> Result<StoredEvent, StoreError> {
        validate_identifier("session_id", session_id)?;
        validate_identifier("task_id", task_id)?;
        validate_identifier("membership_id", membership_id)?;
        if input_id.is_none() && episode_id.is_none() {
            return Err(StoreError::InvalidInput(
                "task membership 至少需要 input_id 或 episode_id".into(),
            ));
        }
        let task_event_id = format!("{task_id}:membership:{membership_id}");
        self.append_task_event(
            session_id,
            TASK_MEMBERSHIP_ADDED_EVENT_TYPE,
            &task_event_id,
            serde_json::json!({
                "schema_version": TASK_EVENT_SCHEMA_VERSION,
                "task_event_id": task_event_id,
                "task_id": task_id,
                "membership_id": membership_id,
                "session_id": session_id,
                "input_id": input_id,
                "episode_id": episode_id,
                "attached_at": now_ms(),
            }),
        )
    }

    /// 显式关闭 task。重复提交同一 task 的 close 事实只返回第一次事件。
    pub fn append_task_closed(
        &self,
        session_id: &str,
        task_id: &str,
        outcome: TaskOutcome,
        closed_by: &str,
        reason: Option<&str>,
    ) -> Result<StoredEvent, StoreError> {
        validate_identifier("session_id", session_id)?;
        validate_identifier("task_id", task_id)?;
        validate_identifier("closed_by", closed_by)?;
        let task_event_id = format!("{task_id}:closed");
        self.append_task_event(
            session_id,
            TASK_CLOSED_EVENT_TYPE,
            &task_event_id,
            serde_json::json!({
                "schema_version": TASK_EVENT_SCHEMA_VERSION,
                "task_event_id": task_event_id,
                "task_id": task_id,
                "outcome": outcome.as_str(),
                "closed_by": closed_by,
                "reason": reason,
                "closed_at": now_ms(),
            }),
        )
    }

    /// 跨 session 回放 task 事实，供后续 projection 归约；不包含旧非 task 事件。
    pub fn list_task_events(&self, task_id: &str) -> Result<Vec<StoredEvent>, StoreError> {
        validate_identifier("task_id", task_id)?;
        let mut statement = self.connection.prepare(
            "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                 FROM session_events
                 WHERE json_extract(payload_json, '$.task_id') = ?1
                   AND event_type IN (?2, ?3, ?4)
                 ORDER BY created_at, session_id, sequence",
        )?;
        let rows = statement.query_map(
            params![
                task_id,
                TASK_STARTED_EVENT_TYPE,
                TASK_MEMBERSHIP_ADDED_EVENT_TYPE,
                TASK_CLOSED_EVENT_TYPE
            ],
            event_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn append_task_event(
        &self,
        session_id: &str,
        event_type: &str,
        task_event_id: &str,
        payload: Value,
    ) -> Result<StoredEvent, StoreError> {
        validate_identifier("task_event_id", task_event_id)?;
        let tx = self.connection.unchecked_transaction()?;
        let existing = tx
            .query_row(
                "SELECT event_id, session_id, sequence, event_type, payload_json, created_at
                     FROM session_events
                     WHERE event_type = ?1
                       AND json_extract(payload_json, '$.task_event_id') = ?2
                     ORDER BY sequence LIMIT 1",
                params![event_type, task_event_id],
                event_from_row,
            )
            .optional()?;
        if let Some(event) = existing {
            return Ok(event);
        }
        let event = append_event_tx(&tx, session_id, event_type, &payload)?;
        tx.commit()?;
        Ok(event)
    }
}

fn validate_identifier(name: &str, value: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        return Err(StoreError::InvalidInput(format!("{name} 不能为空")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::store;

    #[test]
    fn task_events_are_append_only_replayable_and_idempotent() {
        let store = store();
        let started = store
            .append_task_started("ses_test", "task-1", Some("整理运行画像"), Some("input-1"))
            .unwrap();
        let started_again = store
            .append_task_started(
                "ses_test",
                "task-1",
                Some("different title"),
                Some("input-2"),
            )
            .unwrap();
        assert_eq!(started.sequence, started_again.sequence);

        let membership = store
            .append_task_membership_added(
                "ses_test",
                "task-1",
                "membership-1",
                Some("input-1"),
                Some(7),
            )
            .unwrap();
        let membership_again = store
            .append_task_membership_added(
                "ses_test",
                "task-1",
                "membership-1",
                Some("input-2"),
                Some(8),
            )
            .unwrap();
        assert_eq!(membership.sequence, membership_again.sequence);

        let closed = store
            .append_task_closed("ses_test", "task-1", TaskOutcome::Completed, "agent", None)
            .unwrap();
        let closed_again = store
            .append_task_closed(
                "ses_test",
                "task-1",
                TaskOutcome::Failed,
                "user",
                Some("重复提交不应改写首次关闭"),
            )
            .unwrap();
        assert_eq!(closed.sequence, closed_again.sequence);

        let events = store.list_task_events("task-1").unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, TASK_STARTED_EVENT_TYPE);
        assert_eq!(events[1].event_type, TASK_MEMBERSHIP_ADDED_EVENT_TYPE);
        assert_eq!(events[2].event_type, TASK_CLOSED_EVENT_TYPE);
        assert_eq!(events[2].payload["outcome"], "completed");
        assert_eq!(events[2].payload["closed_by"], "agent");
    }

    #[test]
    fn task_membership_requires_explicit_fact_and_can_cross_session() {
        let store = store();
        store.create_session("ses_other", "C:/other", None).unwrap();
        assert!(store
            .append_task_membership_added("ses_test", "task-2", "membership-empty", None, None,)
            .is_err());

        store
            .append_task_started("ses_test", "task-2", None, None)
            .unwrap();
        store
            .append_task_membership_added(
                "ses_other",
                "task-2",
                "membership-cross-session",
                Some("input-2"),
                None,
            )
            .unwrap();
        let events = store.list_task_events("task-2").unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == TASK_STARTED_EVENT_TYPE)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == TASK_MEMBERSHIP_ADDED_EVENT_TYPE)
                .count(),
            1
        );
        assert!(events.iter().any(|event| event.session_id == "ses_test"));
        assert!(events.iter().any(|event| event.session_id == "ses_other"));
    }

    #[test]
    fn task_event_identifiers_must_not_be_empty() {
        let store = store();
        assert!(store
            .append_task_started("ses_test", "", None, None)
            .is_err());
        assert!(store
            .append_task_closed("ses_test", "task-3", TaskOutcome::Cancelled, "", None)
            .is_err());
    }
}
