//! Bounded read model for a multi-project workbench. No recovery, event replay or writes.

use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use super::{SessionStore, StoreError};

impl SessionStore {
    /// A workbench read must not silently migrate an older database or interpret a newer
    /// schema. Startup recovery owns migration and retries; this check performs no writes.
    pub fn check_workspace_schema(&self) -> Result<(), StoreError> {
        let has_meta: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_meta')",
            [],
            |row| row.get(0),
        )?;
        let version: Option<i64> = if has_meta {
            self.connection
                .query_row(
                    "SELECT value FROM schema_meta WHERE key='schema_version'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .and_then(|value| value.parse().ok())
        } else {
            None
        };
        match version {
            Some(found) if found == super::SCHEMA_VERSION => Ok(()),
            Some(found) if found > super::SCHEMA_VERSION => Err(StoreError::UnsupportedSchema {
                found,
                supported: super::SCHEMA_VERSION,
            }),
            Some(found) => Err(StoreError::InvalidInput(format!(
                "状态库需要从 schema {found} 升级至 {}；应用启动恢复会处理，工作台查询保持只读",
                super::SCHEMA_VERSION
            ))),
            None => Err(StoreError::InvalidInput(
                "状态库缺少 schema 版本，等待应用恢复；工作台查询保持只读".into(),
            )),
        }
    }

    /// Count current business facts in SQL; never materialize decision text or transcripts.
    pub fn workspace_summary(&self, journal: &str) -> Result<Value, StoreError> {
        self.check_workspace_schema()?;
        let latest =
            "WITH decisions AS (SELECT payload_json FROM session_events WHERE session_id=?1
            AND sequence IN (SELECT MAX(sequence) FROM session_events WHERE session_id=?1
                AND event_type='decision.updated' GROUP BY json_extract(payload_json,'$.id')))";
        let (decisions, missing_facts): (u64, u64) = self.connection.query_row(
            &format!("{latest} SELECT
                COALESCE(SUM(json_extract(payload_json,'$.status')='decided' AND json_extract(payload_json,'$.review') IS NULL),0),
                COALESCE(SUM(json_extract(payload_json,'$.status')='needs_input'),0) FROM decisions"),
            [journal], |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let (active, verifying, ready_to_try): (u64, u64, u64) = self.connection.query_row(
            &format!("{latest} SELECT
                COALESCE(SUM(status='active'),0), COALESCE(SUM(status='verifying'),0),
                COALESCE(SUM(status='done' AND NOT EXISTS
                    (SELECT 1 FROM decisions d WHERE json_extract(d.payload_json,'$.work_unit_id')=w.unit_id
                        AND json_extract(d.payload_json,'$.review.action')='correct')
                    AND NOT EXISTS (SELECT 1 FROM session_events a WHERE a.session_id=?1
                        AND a.event_type='work.user_reviewed'
                        AND json_extract(a.payload_json,'$.unit_id')=w.unit_id
                        AND json_extract(a.payload_json,'$.source_sequence')=w.source_sequence)),0)
                FROM work_surfaces w"),
            [journal], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let pending: u64 = self.connection.query_row(
            "SELECT COUNT(*) FROM session_inputs WHERE status='pending'",
            [],
            |row| row.get(0),
        )?;
        // These are business transitions, not tool heartbeats. A single bounded row is enough.
        let recent = self
            .connection
            .query_row(
                "SELECT requirement_id, event_type, created_at FROM work_events
                WHERE event_type IN ('work.completed','work.checkpointed','work.blocked',
                    'work.verification_started','work.verification_concluded','work.evidence_added')
                ORDER BY created_at DESC, rowid DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?
            .map(|(id, kind, at)| {
                let action = match kind.as_str() {
                    "work.completed" => "机器验证完成",
                    "work.checkpointed" => "已记录工作进展",
                    "work.blocked" => "工作遇到阻塞",
                    "work.verification_started" => "开始机器验证",
                    "work.verification_concluded" => "机器验证已返回",
                    _ => "补充了验收证据",
                };
                json!({"label": format!("{id} · {action}"), "at": at, "kind": kind})
            });
        Ok(
            json!({"active": active, "verifying": verifying, "ready_to_try": ready_to_try,
            "decisions": decisions, "missing_facts": missing_facts, "pending": pending,
            "recent_progress": recent}),
        )
    }

    /// Small ownership projection, excluding objective, checkpoints and evidence bodies.
    pub fn workspace_work_claims(&self) -> Result<Vec<(String, Option<String>)>, StoreError> {
        let mut query = self.connection.prepare(
            "SELECT DISTINCT requirement_id, json_extract(projection_json,'$.claimed_by')
                FROM work_surfaces WHERE status IN ('active','verifying')
                ORDER BY updated_at DESC LIMIT 128",
        )?;
        let rows = query
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into);
        rows
    }

    /// Read the latest successful authoritative work claim, paired by turn and call identity.
    /// No prompt parsing, no transcript loading, no fallback from a failed claim to its ID.
    pub fn latest_work_claim(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
    ) -> Result<Option<String>, StoreError> {
        let mut calls = self.connection.prepare(
            "SELECT sequence, payload_json, event_type FROM session_events WHERE session_id=?1
                AND ((event_type='session.tool_called'
                AND json_extract(payload_json,'$.fact.name')='work'
                AND json_extract(payload_json,'$.fact.input.action')='claim'
                AND (?2 IS NULL OR json_extract(payload_json,'$.turn_id')=?2))
                OR (event_type='work.ui_claimed'
                AND (?2 IS NULL OR json_extract(payload_json,'$.run_id')=?2)))
                ORDER BY sequence DESC LIMIT 32",
        )?;
        let rows = calls.query_map(params![session_id, turn_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (sequence, raw, event_type) = row?;
            let call: Value = serde_json::from_str(&raw)?;
            if event_type == "work.ui_claimed" {
                if call["receipt"]["claimed"] == call["item_id"]
                    && matches!(
                        call["receipt"]["lifecycle_status"].as_str(),
                        Some("doing" | "fixing")
                    )
                {
                    return Ok(call["item_id"].as_str().map(str::to_string));
                }
                continue;
            }
            let Some(id) = call["fact"]["input"]["id"].as_str() else {
                continue;
            };
            let result: Option<String> = self
                .connection
                .query_row(
                    "SELECT payload_json FROM session_events WHERE session_id=?1
                    AND event_type='session.tool_result_committed' AND sequence>?2
                    AND json_extract(payload_json,'$.turn_id')=?3
                    AND json_extract(payload_json,'$.fact.call_id')=?4
                    ORDER BY sequence LIMIT 1",
                    params![
                        session_id,
                        sequence,
                        call["turn_id"].as_str(),
                        call["fact"]["call_id"].as_str()
                    ],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(raw) = result else { continue };
            let result: Value = serde_json::from_str(&raw)?;
            if result["fact"]["is_error"] != false {
                continue;
            }
            let Some(receipt) = result["fact"]["content"]
                .as_str()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
            else {
                continue;
            };
            if receipt["claimed"] == id
                && matches!(
                    receipt["lifecycle_status"].as_str(),
                    Some("doing" | "fixing")
                )
            {
                return Ok(Some(id.to_string()));
            }
            if receipt["unit_id"] == id
                && matches!(receipt["status"].as_str(), Some("active" | "verifying"))
            {
                return Ok(receipt["requirement_id"].as_str().map(str::to_string));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WorkEvidence, WorkFact, WorkUnitSpec};

    #[test]
    fn workspace_schema_check_reports_old_or_new_versions_without_migrating() {
        let store = SessionStore::open_in_memory().unwrap();
        for version in [
            super::super::SCHEMA_VERSION - 1,
            super::super::SCHEMA_VERSION + 1,
        ] {
            store
                .connection
                .execute(
                    "UPDATE schema_meta SET value=?1 WHERE key='schema_version'",
                    [version.to_string()],
                )
                .unwrap();
            assert!(store.check_workspace_schema().is_err());
            let still: String = store
                .connection
                .query_row(
                    "SELECT value FROM schema_meta WHERE key='schema_version'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(still, version.to_string());
        }
    }

    #[test]
    fn workspace_counters_keep_machine_completion_and_user_acceptance_separate() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("journal", "project", None).unwrap();
        store
            .create_work_unit(WorkUnitSpec {
                unit_id: "R-001/W1".into(),
                requirement_id: "R-001".into(),
                objective: "Reader".into(),
                scope: vec!["src".into()],
                dependencies: vec![],
                acceptance: vec!["read".into()],
                verification: vec!["test".into()],
                base_revision: "base".into(),
            })
            .unwrap();
        store
            .append_work_fact("R-001/W1", WorkFact::Claimed { claimed_by: None })
            .unwrap();
        assert_eq!(
            store.workspace_work_claims().unwrap(),
            vec![("R-001".into(), None)]
        );
        assert_eq!(store.workspace_summary("journal").unwrap()["active"], 1);
        store
            .append_work_fact("R-001/W1", WorkFact::VerificationStarted)
            .unwrap();
        assert_eq!(store.workspace_summary("journal").unwrap()["verifying"], 1);
        store
            .append_work_fact(
                "R-001/W1",
                WorkFact::EvidenceAdded {
                    evidence: WorkEvidence {
                        criterion: "read".into(),
                        evidence_refs: vec!["tests passed".into()],
                    },
                },
            )
            .unwrap();
        let unit = store
            .append_work_fact("R-001/W1", WorkFact::Completed)
            .unwrap();
        let before = store.workspace_summary("journal").unwrap();
        assert_eq!(before["ready_to_try"], 1);
        assert_eq!(before["recent_progress"]["kind"], "work.completed");
        store
            .accept_work_delivery("journal", &unit.unit_id, unit.source_sequence)
            .unwrap();
        assert_eq!(
            store.workspace_summary("journal").unwrap()["ready_to_try"],
            0
        );
        // Tool activity is not a business milestone and must not replace the last meaningful row.
        store
            .append_event(
                "journal",
                "run.trace",
                &json!({"kind":"tool","name":"read"}),
            )
            .unwrap();
        assert_eq!(
            store.workspace_summary("journal").unwrap()["recent_progress"],
            before["recent_progress"]
        );
    }

    #[test]
    fn workspace_decision_counts_use_latest_revision_without_loading_body() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("journal", "project", None).unwrap();
        for event in [
            json!({"id":"one","status":"decided","review":null}),
            json!({"id":"two","status":"needs_input","review":null}),
            json!({"id":"one","status":"decided","review":{"action":"accept"}}),
        ] {
            store
                .append_event("journal", "decision.updated", &event)
                .unwrap();
        }
        let summary = store.workspace_summary("journal").unwrap();
        assert_eq!(summary["decisions"], 0);
        assert_eq!(summary["missing_facts"], 1);
        assert!(summary.get("question").is_none());
    }
}
