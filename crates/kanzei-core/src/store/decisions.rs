//! 自主决策与用户复核。复用 session_events，不另建一套任务或记忆数据库。
//! 原始决定不被用户纠正覆盖；纠正、原会话输入入队在同一 SQLite 事务提交。

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::events::append_event_tx;
use super::{now_ms, SessionStore, StoreError};

pub const DECISION_EVENT: &str = "decision.updated";
pub const WORK_REVIEW_EVENT: &str = "work.user_reviewed";
pub const PREFERENCE_APPLIED_EVENT: &str = "preference_applied";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkAcceptance {
    pub unit_id: String,
    pub source_sequence: i64,
    pub accepted_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionResolution {
    pub answer: String,
    /// 简短、可供用户检查的理由，不记录隐藏思维过程。
    pub rationale: String,
    pub impact: String,
    #[serde(default)]
    pub preference_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Deciding,
    Decided,
    NeedsInput,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceScope {
    Once,
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    Accept,
    Correct,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionReview {
    pub request_id: String,
    pub action: ReviewAction,
    pub feedback: String,
    pub scope: PreferenceScope,
    pub reviewed_at: i64,
    pub rework_input_id: Option<String>,
    pub preference_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionRecord {
    pub id: String,
    pub revision: i64,
    pub project: String,
    pub session_id: String,
    pub process_id: Option<String>,
    pub run_id: String,
    pub call_id: String,
    pub question: String,
    pub options: Vec<Value>,
    pub work_unit_id: Option<String>,
    pub status: DecisionStatus,
    pub resolution: Option<DecisionResolution>,
    pub missing_fact: Option<String>,
    pub review: Option<DecisionReview>,
    pub created_at: i64,
}

pub struct AgentDecision<'a> {
    pub project: &'a str,
    pub session_id: &'a str,
    pub process_id: Option<&'a str>,
    pub run_id: &'a str,
    pub call_id: &'a str,
    pub question: &'a str,
    pub options: Vec<Value>,
    pub work_unit_id: Option<String>,
    pub resolution: Option<DecisionResolution>,
    pub missing_fact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDecision {
    pub request_id: String,
    pub expected_revision: i64,
    pub action: ReviewAction,
    #[serde(default)]
    pub feedback: String,
    pub scope: PreferenceScope,
}

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidInput(message.into())
}

fn bounded(text: &str, name: &str, max: usize) -> Result<(), StoreError> {
    if text.trim().is_empty() || text.chars().count() > max {
        return Err(invalid(&format!("{name} must contain 1..{max} characters")));
    }
    Ok(())
}

/// 包含 run 与原问题：同一问题重试不增加记录，不同轮/项目不合并。
pub fn decision_id(run_id: &str, session_id: &str, question: &str) -> String {
    let mut hash = Sha256::new();
    for part in [run_id, session_id, question.trim()] {
        hash.update(part.len().to_le_bytes());
        hash.update(part.as_bytes());
    }
    format!("dec-{:x}", hash.finalize())
}

fn read_tx(
    tx: &Transaction<'_>,
    journal: &str,
    id: &str,
) -> Result<Option<DecisionRecord>, StoreError> {
    let json: Option<String> = tx
        .query_row(
            "SELECT payload_json FROM session_events WHERE session_id=?1 AND event_type=?2
         AND json_extract(payload_json, '$.id')=?3 ORDER BY sequence DESC LIMIT 1",
            params![journal, DECISION_EVENT, id],
            |row| row.get(0),
        )
        .optional()?;
    json.map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}

fn append(
    tx: &Transaction<'_>,
    journal: &str,
    record: &mut DecisionRecord,
) -> Result<(), StoreError> {
    record.revision = tx.query_row(
        "SELECT COALESCE(MAX(sequence),0)+1 FROM session_events WHERE session_id=?1",
        [journal],
        |row| row.get(0),
    )?;
    append_event_tx(tx, journal, DECISION_EVENT, &serde_json::to_value(record)?)?;
    Ok(())
}

impl SessionStore {
    pub fn list_work_acceptances(&self, journal: &str) -> Result<Vec<WorkAcceptance>, StoreError> {
        self.list_events_by_type(journal, 0, WORK_REVIEW_EVENT)?
            .into_iter()
            .map(|e| serde_json::from_value(e.payload).map_err(Into::into))
            .collect()
    }

    /// 人工验收只附加审阅事实，永远不更改机器完成状态和取活队列。
    pub fn accept_work_delivery(
        &self,
        journal: &str,
        unit_id: &str,
        source_sequence: i64,
    ) -> Result<WorkAcceptance, StoreError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let json: String = tx.query_row(
            "SELECT projection_json FROM work_surfaces WHERE unit_id=?1",
            [unit_id],
            |r| r.get(0),
        )?;
        let unit: super::WorkProjection = serde_json::from_str(&json)?;
        if unit.status != super::WorkUnitStatus::Done || unit.source_sequence != source_sequence {
            return Err(invalid(
                "delivery changed or machine verification is incomplete; refresh first",
            ));
        }
        if self.list_decisions(journal)?.iter().any(|d| {
            d.work_unit_id.as_deref() == Some(unit_id)
                && d.review
                    .as_ref()
                    .is_some_and(|r| r.action == ReviewAction::Correct)
        }) {
            return Err(invalid(
                "delivery has corrected decisions; review the new work unit after revalidation",
            ));
        }
        if let Some(existing) = self
            .list_work_acceptances(journal)?
            .into_iter()
            .find(|a| a.unit_id == unit_id && a.source_sequence == source_sequence)
        {
            return Ok(existing);
        }
        let acceptance = WorkAcceptance {
            unit_id: unit_id.into(),
            source_sequence,
            accepted_at: now_ms(),
        };
        append_event_tx(
            &tx,
            journal,
            WORK_REVIEW_EVENT,
            &serde_json::to_value(&acceptance)?,
        )?;
        tx.commit()?;
        Ok(acceptance)
    }

    pub fn list_decisions(&self, journal: &str) -> Result<Vec<DecisionRecord>, StoreError> {
        // 在 SQL 层只取每个决定的最新状态；不读取 conversation/run.trace。
        let mut stmt = self.connection.prepare(
            "SELECT payload_json FROM session_events WHERE session_id=?1 AND sequence IN
             (SELECT MAX(sequence) FROM session_events WHERE session_id=?1 AND event_type=?2
              GROUP BY json_extract(payload_json, '$.id')) ORDER BY sequence DESC",
        )?;
        let rows = stmt.query_map(params![journal, DECISION_EVENT], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|s| Ok(serde_json::from_str(&s?)?)).collect()
    }

    pub fn record_agent_decision(
        &self,
        journal: &str,
        mut input: AgentDecision<'_>,
    ) -> Result<DecisionRecord, StoreError> {
        bounded(input.question, "question", 4000)?;
        bounded(input.run_id, "run_id", 512)?;
        bounded(input.session_id, "session_id", 1024)?;
        if input.options.len() > 20 || serde_json::to_vec(&input.options)?.len() > 16000 {
            return Err(invalid("too many/large decision options"));
        }
        if input.resolution.is_some() && input.missing_fact.is_some() {
            return Err(invalid("supply decision OR missing_fact"));
        }
        if let Some(choice) = &input.resolution {
            bounded(&choice.answer, "decision.answer", 4000)?;
            bounded(&choice.rationale, "decision.rationale", 4000)?;
            bounded(&choice.impact, "decision.impact", 4000)?;
            if choice.preference_refs.len() > 20
                || choice
                    .preference_refs
                    .iter()
                    .any(|reference| reference.trim().is_empty() || reference.len() > 512)
            {
                return Err(invalid("too many/invalid preference_refs"));
            }
        }
        if let Some(fact) = &input.missing_fact {
            bounded(fact, "missing_fact", 4000)?;
        }
        if let Some(unit) = &input.work_unit_id {
            if self.get_work_unit(unit)?.is_none() {
                return Err(invalid("unknown work_unit_id"));
            }
        }
        let preference_refs = if let Some(choice) = input.resolution.as_mut() {
            let mut unique = Vec::with_capacity(choice.preference_refs.len());
            for reference in std::mem::take(&mut choice.preference_refs) {
                let reference = reference.trim().to_owned();
                if !unique.contains(&reference) {
                    unique.push(reference);
                }
            }
            choice.preference_refs = unique.clone();
            unique
        } else {
            Vec::new()
        };
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let id = decision_id(input.run_id, input.session_id, input.question);
        let existing = read_tx(&tx, journal, &id)?;
        if let Some(record) = &existing {
            // 已作决定与用户复核不可被重放的工具调用覆盖。
            if record.status != DecisionStatus::Deciding
                || (input.resolution.is_none() && input.missing_fact.is_none())
            {
                return Ok(record.clone());
            }
        }
        let mut record = existing.unwrap_or_else(|| DecisionRecord {
            id,
            revision: 0,
            project: input.project.into(),
            session_id: input.session_id.into(),
            process_id: input.process_id.map(Into::into),
            run_id: input.run_id.into(),
            call_id: input.call_id.into(),
            question: input.question.trim().into(),
            options: input.options,
            work_unit_id: input.work_unit_id,
            status: DecisionStatus::Deciding,
            resolution: None,
            missing_fact: None,
            review: None,
            created_at: now_ms(),
        });
        record.status = if input.resolution.is_some() {
            DecisionStatus::Decided
        } else if input.missing_fact.is_some() {
            DecisionStatus::NeedsInput
        } else {
            DecisionStatus::Deciding
        };
        record.resolution = input.resolution;
        record.missing_fact = input.missing_fact;
        append(&tx, journal, &mut record)?;
        let mut recorded_refs = std::collections::BTreeSet::new();
        for preference_ref in preference_refs {
            if !recorded_refs.insert(preference_ref.clone()) {
                continue;
            }
            append_event_tx(
                &tx,
                journal,
                PREFERENCE_APPLIED_EVENT,
                &serde_json::json!({
                    "decision_id": record.id,
                    "preference_ref": preference_ref,
                    "decision_revision": record.revision,
                    "run_id": record.run_id,
                    "session_id": record.session_id,
                    "asserted_by": "agent",
                    "recorded_at": now_ms(),
                }),
            )?;
        }
        tx.commit()?;
        Ok(record)
    }

    pub fn review_decision(
        &self,
        journal: &str,
        id: &str,
        input: &ReviewDecision,
    ) -> Result<DecisionRecord, StoreError> {
        bounded(&input.request_id, "request_id", 128)?;
        if input.action == ReviewAction::Correct {
            bounded(&input.feedback, "feedback", 8000)?;
        }
        if input.action == ReviewAction::Accept
            && (input.scope != PreferenceScope::Once || !input.feedback.is_empty())
        {
            return Err(invalid(
                "accept does not create preferences; use correct with explicit feedback and scope",
            ));
        }
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let mut record = read_tx(&tx, journal, id)?.ok_or_else(|| invalid("decision not found"))?;
        if let Some(review) = &record.review {
            if review.request_id == input.request_id {
                if review.action != input.action
                    || review.feedback != input.feedback.trim()
                    || review.scope != input.scope
                {
                    return Err(invalid(
                        "review request_id was reused with different content",
                    ));
                }
                return Ok(record);
            }
        }
        if record.revision != input.expected_revision {
            return Err(invalid("decision changed; refresh before reviewing"));
        }
        if record.status == DecisionStatus::Deciding {
            return Err(invalid("agent has not decided yet"));
        }
        if record.status == DecisionStatus::NeedsInput && input.action == ReviewAction::Accept {
            return Err(invalid("missing facts require a reply"));
        }
        let rework_input_id = if input.action == ReviewAction::Correct {
            let queue_id = decision_id(id, &record.session_id, &input.request_id);
            let prompt = format!(
                "用户复核决策 {} 并要求修改。\n原问题：{}\n原决定：{}\n用户纠正：{}\n适用范围：{:?}\n相关 Work Unit：{}\n在原任务上落实修改并重新验证受影响的内容；旧验证不能当作修改后的证据。若关联工作单元已经完成，为返工创建新的 Work Unit 并引用原单元，保留原完成记录。更新检查点并保留本决策引用。",
                record.id, record.question, record.resolution.as_ref().map(|r| r.answer.as_str()).unwrap_or("缺少外部事实"),
                input.feedback.trim(), input.scope, record.work_unit_id.as_deref().unwrap_or("未绑定"),
            );
            tx.execute(
                "INSERT INTO session_inputs(input_id,session_id,prompt,delivery,status,created_at)
                 VALUES(?1,?2,?3,'queue','pending',?4)",
                params![queue_id, record.session_id, prompt, now_ms()],
            )?;
            append_event_tx(
                &tx,
                &record.session_id,
                "prompt.admitted",
                &serde_json::json!({
                    "input_id": queue_id, "delivery": "queue", "decision_id": record.id,
                }),
            )?;
            Some(queue_id)
        } else {
            None
        };
        record.review = Some(DecisionReview {
            request_id: input.request_id.clone(),
            action: input.action,
            feedback: input.feedback.trim().into(),
            scope: input.scope,
            reviewed_at: now_ms(),
            rework_input_id,
            preference_id: None,
        });
        append(&tx, journal, &mut record)?;
        tx.commit()?;
        Ok(record)
    }

    /// 文件记忆与 SQLite 之间的可重试确认。未确认时 UI 明示“偏好待保存”。
    pub fn link_decision_preference(
        &self,
        journal: &str,
        id: &str,
        request_id: &str,
        preference_id: &str,
    ) -> Result<DecisionRecord, StoreError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let mut record = read_tx(&tx, journal, id)?.ok_or_else(|| invalid("decision not found"))?;
        let review = record
            .review
            .as_mut()
            .ok_or_else(|| invalid("decision not reviewed"))?;
        if review.request_id != request_id
            || review.action != ReviewAction::Correct
            || review.scope == PreferenceScope::Once
        {
            return Err(invalid(
                "preference does not match the current explicit review",
            ));
        }
        if review.preference_id.as_deref() == Some(preference_id) {
            return Ok(record);
        }
        review.preference_id = Some(preference_id.into());
        append(&tx, journal, &mut record)?;
        tx.commit()?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(store: &SessionStore, run: &str, session: &str, resolution: bool) -> DecisionRecord {
        store.create_session(session, "C:/project", None).unwrap();
        store
            .record_agent_decision(
                "ses_test",
                AgentDecision {
                    project: "C:/project",
                    session_id: session,
                    process_id: Some("p2"),
                    run_id: run,
                    call_id: "call-1",
                    question: "分页还是滚动？",
                    options: vec![],
                    work_unit_id: None,
                    missing_fact: None,
                    resolution: resolution.then(|| DecisionResolution {
                        answer: "分页".into(),
                        rationale: "保持位置".into(),
                        impact: "列表每页 50 项".into(),
                        preference_refs: vec![],
                    }),
                },
            )
            .unwrap()
    }
    #[test]
    fn autonomous_decisions_replay_resolve_and_preserve_original() {
        let store = super::super::testutil::store();
        let pending = record(&store, "run-1", "ses_test#p2", false);
        assert_eq!(pending.status, DecisionStatus::Deciding);
        let decided = record(&store, "run-1", "ses_test#p2", true);
        assert_eq!(decided.id, pending.id);
        assert!(decided.revision > pending.revision);
        assert_eq!(record(&store, "run-1", "ses_test#p2", false), decided);
        assert_ne!(record(&store, "run-2", "ses_test#p2", true).id, decided.id);
        assert_eq!(store.list_decisions("ses_test").unwrap().len(), 2);
    }
    #[test]
    fn explicit_preference_refs_emit_deduplicated_replayable_application_facts() {
        let store = super::super::testutil::store();
        let pending = record(&store, "run-pref", "ses_test#p2", false);
        let input = AgentDecision {
            project: "C:/project",
            session_id: "ses_test#p2",
            process_id: Some("p2"),
            run_id: "run-pref",
            call_id: "call-resolve",
            question: "分页还是滚动？",
            options: vec![],
            work_unit_id: None,
            missing_fact: None,
            resolution: Some(DecisionResolution {
                answer: "分页".into(),
                rationale: "沿用已确认的体验偏好".into(),
                impact: "维持列表交互一致".into(),
                preference_refs: vec!["M-301".into(), "M-302".into(), "M-301".into()],
            }),
        };
        let decided = store.record_agent_decision("ses_test", input).unwrap();
        assert_eq!(decided.id, pending.id);
        assert_eq!(
            decided.resolution.as_ref().unwrap().preference_refs,
            vec!["M-301", "M-302"],
            "决策引用也规范化去重，避免 UI 与应用事实两套口径"
        );
        let events = store
            .list_events_by_type("ses_test", 0, PREFERENCE_APPLIED_EVENT)
            .unwrap();
        assert_eq!(events.len(), 2, "重复引用只记录一次");
        assert_eq!(events[0].payload["decision_id"], decided.id);
        assert_eq!(events[0].payload["preference_ref"], "M-301");
        assert_eq!(events[0].payload["decision_revision"], decided.revision);
        assert_eq!(events[0].payload["asserted_by"], "agent");
        assert_eq!(events[1].payload["preference_ref"], "M-302");

        let replayed = store
            .record_agent_decision(
                "ses_test",
                AgentDecision {
                    project: "C:/project",
                    session_id: "ses_test#p2",
                    process_id: Some("p2"),
                    run_id: "run-pref",
                    call_id: "replayed-call",
                    question: "分页还是滚动？",
                    options: vec![],
                    work_unit_id: None,
                    missing_fact: None,
                    resolution: Some(DecisionResolution {
                        answer: "分页".into(),
                        rationale: "沿用已确认的体验偏好".into(),
                        impact: "维持列表交互一致".into(),
                        preference_refs: vec!["M-301".into(), "M-302".into()],
                    }),
                },
            )
            .unwrap();
        assert_eq!(replayed, decided);
        assert_eq!(
            store
                .list_events_by_type("ses_test", 0, PREFERENCE_APPLIED_EVENT)
                .unwrap(),
            events,
            "重放已决定的问题不能重复记账"
        );

        let invalid = store.record_agent_decision(
            "ses_test",
            AgentDecision {
                project: "C:/project",
                session_id: "ses_test#p2",
                process_id: Some("p2"),
                run_id: "run-pref-invalid",
                call_id: "call-invalid-ref",
                question: "引用空偏好是否有效？",
                options: vec![],
                work_unit_id: None,
                missing_fact: None,
                resolution: Some(DecisionResolution {
                    answer: "无效".into(),
                    rationale: "测试".into(),
                    impact: "拒绝空引用".into(),
                    preference_refs: vec!["   ".into()],
                }),
            },
        );
        assert!(invalid.is_err());
        assert_eq!(store.list_decisions("ses_test").unwrap().len(), 1);
        assert_eq!(
            store
                .list_events_by_type("ses_test", 0, PREFERENCE_APPLIED_EVENT)
                .unwrap(),
            events
        );
    }

    #[test]
    fn correction_is_atomic_scoped_and_retry_safe() {
        let store = super::super::testutil::store();
        let original = record(&store, "run-1", "ses_test#p2", true);
        let input = ReviewDecision {
            request_id: "review-1".into(),
            expected_revision: original.revision,
            action: ReviewAction::Correct,
            feedback: "使用虚拟滚动".into(),
            scope: PreferenceScope::Project,
        };
        let corrected = store
            .review_decision("ses_test", &original.id, &input)
            .unwrap();
        assert_eq!(corrected.resolution, original.resolution);
        assert_eq!(
            store
                .review_decision("ses_test", &original.id, &input)
                .unwrap(),
            corrected
        );
        assert_eq!(store.list_pending_inputs("ses_test#p2").unwrap().len(), 1);
        assert!(store.list_pending_inputs("ses_test").unwrap().is_empty());
        assert!(store
            .review_decision(
                "ses_test",
                &original.id,
                &ReviewDecision {
                    request_id: "stale".into(),
                    ..input
                }
            )
            .is_err());
        let linked = store
            .link_decision_preference("ses_test", &original.id, "review-1", "M-1")
            .unwrap();
        assert_eq!(linked.review.unwrap().preference_id.as_deref(), Some("M-1"));
    }
    #[test]
    fn acceptance_never_implies_global_preference() {
        let store = super::super::testutil::store();
        let original = record(&store, "run-1", "ses_test", true);
        let input = ReviewDecision {
            request_id: "review-1".into(),
            expected_revision: original.revision,
            action: ReviewAction::Accept,
            feedback: String::new(),
            scope: PreferenceScope::Global,
        };
        assert!(store
            .review_decision("ses_test", &original.id, &input)
            .is_err());
        let accepted = store
            .review_decision(
                "ses_test",
                &original.id,
                &ReviewDecision {
                    scope: PreferenceScope::Once,
                    ..input
                },
            )
            .unwrap();
        assert!(accepted.review.unwrap().preference_id.is_none());
        assert!(store.list_pending_inputs("ses_test").unwrap().is_empty());
    }

    #[test]
    fn human_acceptance_does_not_block_machine_completion_and_is_revision_bound() {
        use super::super::{WorkEvidence, WorkFact, WorkUnitSpec, WorkUnitStatus};
        let store = super::super::testutil::store();
        let unit = "R-900/W1";
        store
            .create_work_unit(WorkUnitSpec {
                unit_id: unit.into(),
                requirement_id: "R-900".into(),
                objective: "deliver".into(),
                scope: vec!["src".into()],
                dependencies: vec![],
                acceptance: vec!["works".into()],
                verification: vec!["targeted test".into()],
                base_revision: "abc123".into(),
            })
            .unwrap();
        assert!(store.accept_work_delivery("ses_test", unit, 1).is_err());
        store
            .append_work_fact(unit, WorkFact::Claimed { claimed_by: None })
            .unwrap();
        store
            .append_work_fact(unit, WorkFact::VerificationStarted)
            .unwrap();
        store
            .append_work_fact(
                unit,
                WorkFact::EvidenceAdded {
                    evidence: WorkEvidence {
                        criterion: "works".into(),
                        evidence_refs: vec!["T-1".into()],
                    },
                },
            )
            .unwrap();
        let completed = store.append_work_fact(unit, WorkFact::Completed).unwrap();
        assert_eq!(completed.status, WorkUnitStatus::Done);
        assert!(store.list_work_acceptances("ses_test").unwrap().is_empty());
        assert!(store.accept_work_delivery("ses_test", unit, 1).is_err());
        let accepted = store
            .accept_work_delivery("ses_test", unit, completed.source_sequence)
            .unwrap();
        assert_eq!(
            accepted,
            store
                .accept_work_delivery("ses_test", unit, completed.source_sequence)
                .unwrap()
        );
        assert_eq!(store.list_work_acceptances("ses_test").unwrap().len(), 1);
        assert_eq!(store.get_work_unit(unit).unwrap().unwrap(), completed);
    }
}
