//! Task 生命周期事实(R-338 B1)：只追加到 `session_events`，不改写旧事实。
//!
//! task 的身份与关闭事实必须显式产生；本模块不从 prompt、时间间隔或 session
//! 边界推断 task。`task_event_id` 是调用方可重放的幂等键，原始存储事件仍由
//! `SessionStore::append_event_tx` 统一分配 session 内 sequence。

use std::collections::{BTreeMap, BTreeSet};

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    InProgress,
    Completed,
    Failed,
    Cancelled,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRoundProjection {
    pub episode_id: i64,
    pub session_id: String,
    pub created_at: i64,
    pub input_id: String,
    pub run_id: String,
    pub outcome: String,
    pub steps: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskProjection {
    pub task_id: String,
    pub title: Option<String>,
    pub status: TaskStatus,
    pub outcome: Option<TaskOutcome>,
    pub started_at: i64,
    pub closed_at: Option<i64>,
    pub closed_by: Option<String>,
    pub reason: Option<String>,
    pub session_ids: Vec<String>,
    pub input_ids: Vec<String>,
    pub episode_ids: Vec<i64>,
    pub rounds: Vec<TaskRoundProjection>,
    pub round_count: u64,
    pub input_count: u64,
    pub steps_sum: u64,
    pub input_tokens_sum: u64,
    pub output_tokens_sum: u64,
    pub duration_ms_sum: u64,
    pub last_activity_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskTrend {
    pub closed_task_count: u64,
    pub completed_task_count: u64,
    pub failed_task_count: u64,
    pub cancelled_task_count: u64,
    pub abandoned_task_count: u64,
    pub round_count: u64,
    pub steps_sum: u64,
    pub input_tokens_sum: u64,
    pub output_tokens_sum: u64,
    pub duration_ms_sum: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMetricsProjection {
    pub completed_tasks: Vec<TaskProjection>,
    pub in_progress_tasks: Vec<TaskProjection>,
    pub trend: TaskTrend,
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

    /// 从 task 事实重建单个 projection；不会写入或缓存派生结果。
    pub fn project_task(&self, task_id: &str) -> Result<TaskProjection, StoreError> {
        validate_identifier("task_id", task_id)?;
        let events = self.list_task_events(task_id)?;
        let started = events
            .iter()
            .filter(|event| event.event_type == TASK_STARTED_EVENT_TYPE)
            .min_by_key(|event| {
                payload_i64(&event.payload, "started_at").unwrap_or(event.created_at)
            })
            .ok_or_else(|| {
                StoreError::InvalidInput(format!("task 缺少 task.started 事实: {task_id}"))
            })?;
        let closed = events
            .iter()
            .filter(|event| event.event_type == TASK_CLOSED_EVENT_TYPE)
            .min_by_key(|event| {
                payload_i64(&event.payload, "closed_at").unwrap_or(event.created_at)
            });

        let mut session_ids = BTreeSet::new();
        let mut input_ids = BTreeSet::new();
        for event in &events {
            session_ids.insert(event.session_id.clone());
            if let Some(session_id) = payload_string(&event.payload, "session_id") {
                session_ids.insert(session_id);
            }
            if let Some(input_id) = payload_string(&event.payload, "input_id") {
                input_ids.insert(input_id);
            }
        }
        let rounds = self.load_task_rounds(&input_ids, &events)?;
        let mut episode_ids = BTreeSet::new();
        for round in &rounds {
            episode_ids.insert(round.episode_id);
            session_ids.insert(round.session_id.clone());
            if !round.input_id.is_empty() {
                input_ids.insert(round.input_id.clone());
            }
        }

        let outcome = closed
            .map(|event| {
                payload_string(&event.payload, "outcome")
                    .ok_or_else(|| StoreError::InvalidInput("task.closed 缺少 outcome".into()))
                    .and_then(|value| parse_task_outcome(&value))
            })
            .transpose()?;
        let status = match outcome {
            Some(TaskOutcome::Completed) => TaskStatus::Completed,
            Some(TaskOutcome::Failed) => TaskStatus::Failed,
            Some(TaskOutcome::Cancelled) => TaskStatus::Cancelled,
            Some(TaskOutcome::Abandoned) => TaskStatus::Abandoned,
            None => TaskStatus::InProgress,
        };
        let started_at = payload_i64(&started.payload, "started_at").unwrap_or(started.created_at);
        let closed_at = closed
            .map(|event| payload_i64(&event.payload, "closed_at").unwrap_or(event.created_at));
        let last_activity_at = events
            .iter()
            .map(|event| event.created_at)
            .chain(rounds.iter().map(|round| round.created_at))
            .max()
            .unwrap_or(started_at);
        let input_count = input_ids.len() as u64;
        let projection = TaskProjection {
            task_id: task_id.to_string(),
            title: payload_string(&started.payload, "title"),
            status,
            outcome,
            started_at,
            closed_at,
            closed_by: closed.and_then(|event| payload_string(&event.payload, "closed_by")),
            reason: closed.and_then(|event| payload_string(&event.payload, "reason")),
            session_ids: session_ids.into_iter().collect(),
            input_ids: input_ids.into_iter().collect(),
            episode_ids: episode_ids.into_iter().collect(),
            round_count: rounds.len() as u64,
            input_count,
            steps_sum: rounds.iter().map(|round| u64::from(round.steps)).sum(),
            input_tokens_sum: rounds.iter().map(|round| round.input_tokens).sum(),
            output_tokens_sum: rounds.iter().map(|round| round.output_tokens).sum(),
            duration_ms_sum: rounds.iter().map(|round| round.duration_ms).sum(),
            rounds,
            last_activity_at,
        };
        Ok(projection)
    }

    /// 重建全库 task projection；task_id 去重由 SQL DISTINCT 完成。
    pub fn list_task_projections(&self) -> Result<Vec<TaskProjection>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT json_extract(payload_json, '$.task_id')
                 FROM session_events
                 WHERE event_type IN (?1, ?2, ?3)
                 ORDER BY json_extract(payload_json, '$.task_id')",
        )?;
        let rows = statement.query_map(
            params![
                TASK_STARTED_EVENT_TYPE,
                TASK_MEMBERSHIP_ADDED_EVENT_TYPE,
                TASK_CLOSED_EVENT_TYPE
            ],
            |row| row.get::<_, String>(0),
        )?;
        rows.map(|row| row.map_err(StoreError::from))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|task_id| self.project_task(&task_id))
            .collect()
    }

    /// 按关闭事实分流，trend 只消费已关闭 task；未关闭 task 保持独立列表。
    pub fn task_metrics(&self) -> Result<TaskMetricsProjection, StoreError> {
        let mut result = TaskMetricsProjection {
            completed_tasks: Vec::new(),
            in_progress_tasks: Vec::new(),
            trend: TaskTrend::default(),
        };
        for task in self.list_task_projections()? {
            if task.closed_at.is_some() {
                result.trend.add(&task);
                result.completed_tasks.push(task);
            } else {
                result.in_progress_tasks.push(task);
            }
        }
        Ok(result)
    }

    fn load_task_rounds(
        &self,
        input_ids: &BTreeSet<String>,
        events: &[StoredEvent],
    ) -> Result<Vec<TaskRoundProjection>, StoreError> {
        let mut episode_ids = BTreeSet::new();
        for event in events {
            if event.event_type == TASK_MEMBERSHIP_ADDED_EVENT_TYPE {
                if let Some(episode_id) = event.payload.get("episode_id").and_then(Value::as_i64) {
                    episode_ids.insert(episode_id);
                }
            }
        }
        let mut rounds = BTreeMap::new();
        for episode_id in episode_ids {
            if let Some(round) = self
                .connection
                .query_row(
                    "SELECT episode_id, session_id, created_at, input_id, run_id, outcome,
                            steps, input_tokens, output_tokens, duration_ms
                         FROM episodes WHERE episode_id = ?1",
                    params![episode_id],
                    task_round_from_row,
                )
                .optional()?
            {
                rounds.insert(round.episode_id, round);
            }
        }
        for input_id in input_ids {
            let mut statement = self.connection.prepare(
                "SELECT episode_id, session_id, created_at, input_id, run_id, outcome,
                        steps, input_tokens, output_tokens, duration_ms
                     FROM episodes WHERE input_id = ?1 ORDER BY created_at, episode_id",
            )?;
            let rows = statement.query_map(params![input_id], task_round_from_row)?;
            for row in rows {
                let round = row?;
                rounds.insert(round.episode_id, round);
            }
        }
        Ok(rounds.into_values().collect())
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

impl TaskTrend {
    fn add(&mut self, task: &TaskProjection) {
        self.closed_task_count += 1;
        match task.outcome {
            Some(TaskOutcome::Completed) => self.completed_task_count += 1,
            Some(TaskOutcome::Failed) => self.failed_task_count += 1,
            Some(TaskOutcome::Cancelled) => self.cancelled_task_count += 1,
            Some(TaskOutcome::Abandoned) => self.abandoned_task_count += 1,
            None => return,
        }
        self.round_count += task.round_count;
        self.steps_sum += task.steps_sum;
        self.input_tokens_sum += task.input_tokens_sum;
        self.output_tokens_sum += task.output_tokens_sum;
        self.duration_ms_sum += task.duration_ms_sum;
    }
}

fn task_round_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRoundProjection> {
    Ok(TaskRoundProjection {
        episode_id: row.get(0)?,
        session_id: row.get(1)?,
        created_at: row.get(2)?,
        input_id: row.get(3)?,
        run_id: row.get(4)?,
        outcome: row.get(5)?,
        steps: row.get::<_, i64>(6)? as u32,
        input_tokens: row.get::<_, i64>(7)? as u64,
        output_tokens: row.get::<_, i64>(8)? as u64,
        duration_ms: row.get::<_, i64>(9)? as u64,
    })
}

fn payload_string(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn payload_i64(payload: &Value, key: &str) -> Option<i64> {
    payload.get(key).and_then(Value::as_i64)
}

fn parse_task_outcome(value: &str) -> Result<TaskOutcome, StoreError> {
    match value {
        "completed" => Ok(TaskOutcome::Completed),
        "failed" => Ok(TaskOutcome::Failed),
        "cancelled" => Ok(TaskOutcome::Cancelled),
        "abandoned" => Ok(TaskOutcome::Abandoned),
        other => Err(StoreError::InvalidInput(format!(
            "task.closed outcome 不支持: {other}"
        ))),
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
    fn task_projection_rebuilds_rounds_and_splits_closed_tasks() {
        let store = store();
        let episode_id = store
            .append_episode(&crate::store::EpisodeRecord {
                session_id: "ses_test",
                prompt_head: "运行画像 task projection",
                outcome: "completed",
                steps: 7,
                input_tokens: 100,
                output_tokens: 20,
                tools_json: "{}",
                context_json: "[]",
                metrics_json: "{}",
                provider: "provider",
                model: "model",
                run_id: "run-task-1",
                input_id: "input-task-1",
                duration_ms: 900,
                overflow_json: "[]",
            })
            .unwrap();
        store
            .append_task_started(
                "ses_test",
                "task-closed",
                Some("已关闭任务"),
                Some("input-task-1"),
            )
            .unwrap();
        store
            .append_task_membership_added(
                "ses_test",
                "task-closed",
                "membership-1",
                Some("input-task-1"),
                Some(episode_id),
            )
            .unwrap();
        store
            .append_task_closed(
                "ses_test",
                "task-closed",
                TaskOutcome::Completed,
                "agent",
                None,
            )
            .unwrap();
        store
            .append_task_started("ses_test", "task-open", Some("进行中任务"), None)
            .unwrap();

        let closed = store.project_task("task-closed").unwrap();
        assert_eq!(closed.status, TaskStatus::Completed);
        assert_eq!(closed.outcome, Some(TaskOutcome::Completed));
        assert_eq!(closed.round_count, 1);
        assert_eq!(closed.input_count, 1);
        assert_eq!(closed.steps_sum, 7);
        assert_eq!(closed.input_tokens_sum, 100);
        assert_eq!(closed.output_tokens_sum, 20);
        assert_eq!(closed.duration_ms_sum, 900);
        assert_eq!(closed.rounds[0].episode_id, episode_id);

        let metrics = store.task_metrics().unwrap();
        assert_eq!(metrics.completed_tasks.len(), 1);
        assert_eq!(metrics.in_progress_tasks.len(), 1);
        assert_eq!(metrics.trend.closed_task_count, 1);
        assert_eq!(metrics.trend.completed_task_count, 1);
        assert_eq!(metrics.trend.round_count, 1);
        assert_eq!(metrics.trend.input_tokens_sum, 100);
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
