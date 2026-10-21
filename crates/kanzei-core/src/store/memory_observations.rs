//! 记忆使用事实：召回、注入、同轮正文读取与可核对的工具恢复。

use std::collections::BTreeMap;

use kanzei_llm::Message;
#[cfg(test)]
use kanzei_llm::Part;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{SessionStore, StoreError};
use crate::runner::recovery::verified_recoveries;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFeedbackKind {
    Adopted,
    Corrected,
    Cleared,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryUsageCounts {
    /// Distinct retrieval passes that considered this memory as a candidate.
    pub opportunities: u64,
    pub recalled: u64,
    pub injected: u64,
    pub read: u64,
    /// 有读取观测能力的注入次数；历史 NULL 不进入这个分母。
    pub read_observed: u64,
    pub last_at: i64,
    /// Explicit user reports, separate from retrieval and verified tool outcomes.
    pub adopted: u64,
    pub corrected: u64,
    pub last_feedback: Option<MemoryFeedbackKind>,
    pub feedback_revision: Option<String>,
    pub last_feedback_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRecallObservation {
    pub recall_id: String,
    pub at: i64,
    pub episode_id: Option<i64>,
    pub run_id: Option<String>,
    pub session_id: Option<String>,
    pub prompt_head: Option<String>,
    pub trigger_type: String,
    pub policy_action: String,
    pub query: String,
    pub retrieved_ids: Vec<String>,
    pub injected_ids: Vec<String>,
    pub read_ids: Option<Vec<String>>,
    pub total_ms: u64,
}

impl SessionStore {
    pub fn record_memory_feedback(
        &self,
        session_id: &str,
        memory_id: &str,
        revision: &str,
        kind: MemoryFeedbackKind,
    ) -> Result<bool, StoreError> {
        if memory_id.is_empty() || revision.is_empty() {
            return Err(StoreError::InvalidInput(
                "memory feedback requires an identity and source revision".into(),
            ));
        }
        let kind = serde_json::to_value(kind)?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let latest: Option<(String, String)> = tx.query_row(
            "SELECT json_extract(payload_json,'$.revision'), json_extract(payload_json,'$.kind') FROM session_events WHERE session_id=?1 AND event_type='memory.feedback' AND json_extract(payload_json,'$.memory_id')=?2 ORDER BY sequence DESC LIMIT 1",
            params![session_id,memory_id], |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        let changed = latest
            .as_ref()
            .is_none_or(|(previous_revision, previous_kind)| {
                previous_revision != revision || Some(previous_kind.as_str()) != kind.as_str()
            });
        if changed {
            super::events::append_event_tx(
                &tx,
                session_id,
                "memory.feedback",
                &serde_json::json!({"memory_id":memory_id,"revision":revision,"kind":kind,"actor":"user"}),
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub fn memory_recall_observations(
        &self,
        limit: usize,
    ) -> Result<Vec<MemoryRecallObservation>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT r.recall_id, r.created_at, r.episode_id, r.run_id, e.session_id,
                    e.prompt_head, r.trigger_type, r.policy_action, r.query,
                    r.retrieved_ids, r.injected_ids, r.read_ids, r.total_ms
             FROM recall_events r LEFT JOIN episodes e ON e.episode_id = r.episode_id
             ORDER BY r.created_at DESC, r.rowid DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit.clamp(1, 200) as i64], |row| {
            let read_ids: Option<String> = row.get(11)?;
            let parse_ids = |column| -> rusqlite::Result<Vec<String>> {
                let json: String = row.get(column)?;
                serde_json::from_str(&json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        column,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            };
            Ok(MemoryRecallObservation {
                recall_id: row.get(0)?,
                at: row.get(1)?,
                episode_id: row.get(2)?,
                run_id: row.get(3)?,
                session_id: row.get(4)?,
                prompt_head: row.get(5)?,
                trigger_type: row.get(6)?,
                policy_action: row.get(7)?,
                query: row.get(8)?,
                retrieved_ids: parse_ids(9)?,
                injected_ids: parse_ids(10)?,
                read_ids: read_ids
                    .map(|json| serde_json::from_str(&json))
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?,
                total_ms: row.get::<_, i64>(12)?.max(0) as u64,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    /// 只回填同一运行内最近一次真正注入的记忆。UI 搜索、旧记录与别的运行不受影响。
    pub fn record_memory_read(&self, run_id: &str, memory_id: &str) -> Result<usize, StoreError> {
        if run_id.trim().is_empty() {
            return Ok(0);
        }
        Ok(self.connection.execute(
            "UPDATE recall_events SET read_ids = json_insert(read_ids, '$[#]', ?2)
             WHERE recall_id = (
                 SELECT recall_id FROM recall_events r
                 WHERE run_id = ?1 AND read_ids IS NOT NULL
                   AND EXISTS(SELECT 1 FROM json_each(r.injected_ids) WHERE value = ?2)
                 ORDER BY created_at DESC, rowid DESC LIMIT 1
             ) AND NOT EXISTS(SELECT 1 FROM json_each(read_ids) WHERE value = ?2)",
            params![run_id, memory_id],
        )?)
    }

    pub fn memory_usage_counts(&self) -> Result<BTreeMap<String, MemoryUsageCounts>, StoreError> {
        let mut statement = self.connection.prepare(
            "WITH eligible AS (
                SELECT r.recall_id, ids.value AS memory_id FROM recall_events r, json_each(r.candidate_ids) ids WHERE r.trigger_type != 'user_search'
                UNION SELECT r.recall_id, ids.value FROM recall_events r, json_each(r.retrieved_ids) ids WHERE r.trigger_type != 'user_search'
            ) SELECT e.memory_id, COUNT(DISTINCT r.recall_id),
                COUNT(DISTINCT CASE WHEN EXISTS(SELECT 1 FROM json_each(r.retrieved_ids) WHERE value = e.memory_id) THEN r.recall_id END),
                COUNT(DISTINCT CASE WHEN EXISTS(SELECT 1 FROM json_each(r.injected_ids) WHERE value = e.memory_id) THEN r.recall_id END),
                COUNT(DISTINCT CASE WHEN EXISTS(SELECT 1 FROM json_each(r.read_ids) WHERE value = e.memory_id) THEN r.recall_id END),
                COUNT(DISTINCT CASE WHEN r.read_ids IS NOT NULL AND EXISTS(SELECT 1 FROM json_each(r.injected_ids) WHERE value = e.memory_id) THEN r.recall_id END),
                MAX(r.created_at)
             FROM eligible e JOIN recall_events r ON r.recall_id=e.recall_id
             GROUP BY e.memory_id ORDER BY e.memory_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                MemoryUsageCounts {
                    opportunities: row.get::<_, i64>(1)?.max(0) as u64,
                    recalled: row.get::<_, i64>(2)?.max(0) as u64,
                    injected: row.get::<_, i64>(3)?.max(0) as u64,
                    read: row.get::<_, i64>(4)?.max(0) as u64,
                    read_observed: row.get::<_, i64>(5)?.max(0) as u64,
                    last_at: row.get(6)?,
                    ..Default::default()
                },
            ))
        })?;
        let mut counts = rows.collect::<Result<BTreeMap<_, _>, _>>()?;
        let mut statement = self.connection.prepare("SELECT json_extract(payload_json,'$.memory_id'),json_extract(payload_json,'$.revision'),json_extract(payload_json,'$.kind'),created_at FROM session_events WHERE event_type='memory.feedback' ORDER BY created_at,rowid")?;
        let feedback = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        for row in feedback {
            let (id, revision, kind, at) = row?;
            let kind: MemoryFeedbackKind = serde_json::from_value(serde_json::Value::String(kind))?;
            let entry = counts.entry(id).or_default();
            entry.adopted += u64::from(kind == MemoryFeedbackKind::Adopted);
            entry.corrected += u64::from(kind == MemoryFeedbackKind::Corrected);
            entry.last_feedback = Some(kind);
            entry.feedback_revision = Some(revision);
            entry.last_feedback_at = at;
        }
        Ok(counts)
    }

    /// 从本轮真实工具配对计算恢复，模型不能通过 memory_promote 的参数伪造此证据。
    pub fn record_episode_recoveries(
        &self,
        episode_id: i64,
        messages: &[Message],
    ) -> Result<(), StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM memory_recoveries WHERE episode_id = ?1",
            [episode_id],
        )?;
        for recovery in verified_recoveries(messages) {
            tx.execute(
                "INSERT OR IGNORE INTO memory_recoveries
                 (episode_id, fingerprint, failed_call_id, recovered_call_id, tool, target, contract_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    episode_id,
                    recovery.fingerprint,
                    recovery.failed_call_id,
                    recovery.recovered_call_id,
                    recovery.tool,
                    recovery.target,
                    crate::runner::recovery::RECOVERY_CONTRACT_VERSION,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn has_memory_recovery(
        &self,
        episode_id: i64,
        fingerprint: &str,
    ) -> Result<bool, StoreError> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_recoveries WHERE episode_id = ?1 AND fingerprint = ?2 AND contract_version = ?3)",
            params![episode_id, crate::normalize_fp_marker(fingerprint), crate::runner::recovery::RECOVERY_CONTRACT_VERSION], |row| row.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EpisodeRecord, RecallEvent};

    #[test]
    fn candidate_opportunities_and_explicit_feedback_do_not_invent_adoption_from_injection() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("reviews", "fixture", None).unwrap();
        let mut recall = event("candidate-only");
        recall.retrieved_ids = "[\"M-1\"]";
        recall.injected_ids = "[\"M-1\"]";
        store.record_recall_event(&recall).unwrap();
        let counts = store.memory_usage_counts().unwrap();
        assert_eq!(counts["M-2"].opportunities, 1);
        assert_eq!(counts["M-2"].recalled, 0);
        assert_eq!(counts["M-1"].injected, 1);
        assert_eq!(counts["M-1"].adopted, 0);
        assert!(store
            .record_memory_feedback("reviews", "M-2", "revision-a", MemoryFeedbackKind::Adopted)
            .unwrap());
        assert!(!store
            .record_memory_feedback("reviews", "M-2", "revision-a", MemoryFeedbackKind::Adopted)
            .unwrap());
        assert!(store
            .record_memory_feedback(
                "reviews",
                "M-2",
                "revision-a",
                MemoryFeedbackKind::Corrected
            )
            .unwrap());
        assert!(store
            .record_memory_feedback("reviews", "M-2", "revision-b", MemoryFeedbackKind::Cleared)
            .unwrap());
        let counts = store.memory_usage_counts().unwrap();
        assert_eq!(counts["M-2"].adopted, 1);
        assert_eq!(counts["M-2"].corrected, 1);
        assert_eq!(
            counts["M-2"].last_feedback,
            Some(MemoryFeedbackKind::Cleared)
        );
        assert_eq!(
            counts["M-2"].feedback_revision.as_deref(),
            Some("revision-b")
        );
        assert_eq!(counts["M-2"].injected, 0);
        assert_eq!(store.list_events("reviews", 0).unwrap().len(), 3);
    }

    fn event(id: &str) -> RecallEvent<'_> {
        RecallEvent {
            recall_id: id,
            episode_id: None,
            step_id: None,
            trigger_type: "memory_search",
            trigger_payload: "{}",
            policy_action: "lexical",
            query: "repair",
            candidate_ids: "[\"M-1\",\"M-2\"]",
            retrieved_ids: "[\"M-1\",\"M-2\"]",
            injected_ids: "[\"M-1\"]",
            lexical_ms: 0,
            embed_ms: 0,
            vector_ms: 0,
            total_ms: 0,
        }
    }

    #[test]
    fn read_and_episode_links_are_isolated_by_run() {
        let store = SessionStore::open_in_memory().unwrap();
        store.record_recall_event(&event("legacy")).unwrap();
        store
            .record_recall_event_for_run(&event("run-a-hit"), Some("run-a"))
            .unwrap();
        store
            .record_recall_event_for_run(&event("run-b-hit"), Some("run-b"))
            .unwrap();
        assert_eq!(store.record_memory_read("run-a", "M-2").unwrap(), 0);
        assert_eq!(store.record_memory_read("wrong-run", "M-1").unwrap(), 0);
        assert_eq!(store.record_memory_read("run-a", "M-1").unwrap(), 1);
        assert_eq!(store.record_memory_read("run-a", "M-1").unwrap(), 0);
        let episode = store
            .append_episode(&EpisodeRecord {
                session_id: "session-a",
                run_id: "run-a",
                prompt_head: "repair task",
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            store
                .link_recall_events_to_episode(episode, i64::MAX)
                .unwrap(),
            1
        );
        let rows = store.memory_recall_observations(20).unwrap();
        let a = rows
            .iter()
            .find(|row| row.recall_id == "run-a-hit")
            .unwrap();
        assert_eq!(a.read_ids.as_deref(), Some(["M-1".to_string()].as_slice()));
        assert_eq!(a.episode_id, Some(episode));
        let b = rows
            .iter()
            .find(|row| row.recall_id == "run-b-hit")
            .unwrap();
        assert_eq!(b.read_ids, Some(vec![]));
        assert_eq!(b.episode_id, None);
        let legacy = rows.iter().find(|row| row.recall_id == "legacy").unwrap();
        assert!(
            legacy.run_id.is_none() && legacy.read_ids.is_none() && legacy.episode_id.is_none()
        );
        let usage = store.memory_usage_counts().unwrap();
        assert_eq!(
            (
                usage["M-1"].recalled,
                usage["M-1"].injected,
                usage["M-1"].read,
                usage["M-1"].read_observed
            ),
            (3, 3, 1, 2)
        );
        assert_eq!((usage["M-2"].injected, usage["M-2"].read_observed), (0, 0));
    }

    #[test]
    fn empty_searches_and_user_searches_remain_visible_without_inflating_usage() {
        let store = SessionStore::open_in_memory().unwrap();
        store
            .record_recall_event(&RecallEvent {
                trigger_type: "user_search",
                ..event("ui")
            })
            .unwrap();
        store
            .record_recall_event_for_run(
                &RecallEvent {
                    candidate_ids: "[]",
                    retrieved_ids: "[]",
                    injected_ids: "[]",
                    policy_action: "miss",
                    ..event("miss")
                },
                Some("run"),
            )
            .unwrap();
        assert_eq!(store.memory_recall_observations(20).unwrap().len(), 2);
        assert!(store.memory_usage_counts().unwrap().is_empty());
    }

    #[test]
    fn v22_migration_preserves_legacy_unknown_and_is_idempotent() {
        let store = SessionStore::open_in_memory().unwrap();
        store.record_recall_event(&event("legacy")).unwrap();
        store
            .connection
            .execute_batch(
                "DROP INDEX recall_events_run_created;
             ALTER TABLE recall_events DROP COLUMN run_id;
             ALTER TABLE recall_events DROP COLUMN read_ids;
             DROP TABLE memory_recoveries;
             UPDATE schema_meta SET value = '22' WHERE key = 'schema_version';",
            )
            .unwrap();
        store.migrate().unwrap();
        store.migrate().unwrap();
        let rows = store.memory_recall_observations(20).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].run_id.is_none() && rows[0].read_ids.is_none());
        store
            .record_recall_event_for_run(&event("new"), Some("run-new"))
            .unwrap();
        assert_eq!(store.record_memory_read("run-new", "M-1").unwrap(), 1);
    }

    fn tool(id: &str, name: &str, target: &str, error: bool) -> Vec<Message> {
        vec![
            Message::assistant(vec![Part::ToolCall {
                id: id.into(),
                name: name.into(),
                input: if name == "bash" {
                    serde_json::json!({"command": target})
                } else {
                    serde_json::json!({"path": target})
                },
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: id.into(),
                is_error: error,
                content: if error { "old_string not found" } else { "ok" }.into(),
            }]),
        ]
    }

    #[test]
    fn recovery_requires_matching_tool_target_and_surviving_success() {
        let mut messages = tool("failure", "edit", "a/x.rs", true);
        messages.extend(tool("read", "read", "a/x.rs", false));
        messages.extend(tool("other", "edit", "b/x.rs", false));
        assert!(verified_recoveries(&messages).is_empty());
        messages.extend(tool("fixed", "edit", "a/x.rs", false));
        let recovered = verified_recoveries(&messages);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].failed_call_id, "failure");
        assert_eq!(recovered[0].recovered_call_id, "fixed");
        let store = SessionStore::open_in_memory().unwrap();
        let episode = store
            .append_episode(&EpisodeRecord {
                session_id: "s",
                run_id: "r",
                ..Default::default()
            })
            .unwrap();
        store.record_episode_recoveries(episode, &messages).unwrap();
        store.record_episode_recoveries(episode, &messages).unwrap();
        assert!(store
            .has_memory_recovery(episode, "[fp:edit|old_string not found]")
            .unwrap());
        assert!(!store
            .has_memory_recovery(episode, "[fp:edit|permission denied]")
            .unwrap());
        messages.extend(tool("again", "edit", "a/x.rs", true));
        assert!(verified_recoveries(&messages).is_empty());
        store.record_episode_recoveries(episode, &messages).unwrap();
        assert!(!store
            .has_memory_recovery(episode, "[fp:edit|old_string not found]")
            .unwrap());
        let mut commands = tool("failed-test", "bash", "cargo test", true);
        commands.extend(tool("status", "bash", "git status", false));
        assert!(verified_recoveries(&commands).is_empty());
    }

    #[test]
    fn noop_is_not_recovery() {
        let mut messages = tool("fail", "edit", "x", true);
        let mut noop = tool("noop", "edit", "x", false);
        if let Part::ToolResult { content, .. } = &mut noop[1].parts[0] {
            *content = "[tool_outcome=noop code=EDIT_IDENTICAL_INPUT] unchanged".into();
        }
        messages.extend(noop);
        assert!(verified_recoveries(&messages).is_empty());
    }

    #[test]
    fn v28_recovery_rows_remain_auditable_but_cannot_verify_a_new_promotion() {
        let store = SessionStore::open_in_memory().unwrap();
        let episode = store
            .append_episode(&EpisodeRecord {
                session_id: "legacy",
                run_id: "legacy",
                ..Default::default()
            })
            .unwrap();
        store
            .connection
            .execute_batch(
                "ALTER TABLE memory_recoveries DROP COLUMN contract_version;
            UPDATE schema_meta SET value='28' WHERE key='schema_version';",
            )
            .unwrap();
        store.connection.execute("INSERT INTO memory_recoveries (episode_id,fingerprint,failed_call_id,recovered_call_id,tool,target)
            VALUES (?1,'[fp:edit|old_string not found]','old-failed','old-recovered','edit','a/x.rs')", [episode]).unwrap();
        store.migrate().unwrap();
        store.migrate().unwrap();
        let count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM memory_recoveries WHERE contract_version=0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert!(!store
            .has_memory_recovery(episode, "[fp:edit|old_string not found]")
            .unwrap());
        let mut messages = tool("failed", "edit", "a/x.rs", true);
        messages.extend(tool("fixed", "edit", "a/x.rs", false));
        store.record_episode_recoveries(episode, &messages).unwrap();
        assert!(store
            .has_memory_recovery(episode, "[fp:edit|old_string not found]")
            .unwrap());
    }

    #[test]
    fn summary_and_promotion_share_recovery_contract_for_audited_false_positives() {
        let cases = [
            (
                "read",
                serde_json::json!({"path":"docs/architectures.json"}),
                "permission denied by ruleset",
                "glob",
                serde_json::json!({"pattern":"**/architectures.json"}),
                "(no files match)",
                false,
            ),
            (
                "work",
                serde_json::json!({"action":"complete","id":"R-034"}),
                "unknown work unit R-034",
                "req",
                serde_json::json!({"action":"update","id":"R-034"}),
                "updated: R-034 [doing]",
                false,
            ),
            (
                "edit",
                serde_json::json!({"path":"a/x.rs"}),
                "old_string not found",
                "read",
                serde_json::json!({"path":"b/x.rs"}),
                "contents",
                false,
            ),
            (
                "edit",
                serde_json::json!({"path":"a/x.rs"}),
                "old_string not found",
                "edit",
                serde_json::json!({"path":"a/x.rs"}),
                "[tool_outcome=noop code=EDIT_IDENTICAL_INPUT] unchanged",
                false,
            ),
            (
                "read",
                serde_json::json!({"path":"x.rs","offset":1}),
                "permission denied by ruleset",
                "read",
                serde_json::json!({"path":"x.rs","offset":50}),
                "other section",
                false,
            ),
            (
                "read",
                serde_json::json!({"path":"x.rs"}),
                "permission denied by ruleset",
                "read",
                serde_json::json!({"path":"x.rs"}),
                "source contents",
                true,
            ),
        ];
        for (name, input, error, next_name, next_input, output, expected) in cases {
            let messages = vec![
                Message::assistant(vec![Part::ToolCall {
                    id: "failed".into(),
                    name: name.into(),
                    input,
                }]),
                Message::tool_results(vec![Part::ToolResult {
                    call_id: "failed".into(),
                    content: error.into(),
                    is_error: true,
                }]),
                Message::assistant(vec![Part::ToolCall {
                    id: "next".into(),
                    name: next_name.into(),
                    input: next_input,
                }]),
                Message::tool_results(vec![Part::ToolResult {
                    call_id: "next".into(),
                    content: output.into(),
                    is_error: false,
                }]),
            ];
            let recovery = verified_recoveries(&messages);
            assert_eq!(!recovery.is_empty(), expected, "{name} -> {next_name}");
            let summaries = crate::summarize_failures(&messages);
            assert_eq!(
                summaries.iter().any(|signal| signal.recovered_by.is_some()),
                expected,
                "{name} -> {next_name}"
            );
        }
    }
}
