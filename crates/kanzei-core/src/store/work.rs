//! 长程任务的执行单元事件与当前投影。
//!
//! Requirement 继续是用户可编辑的 Outcome 真源；本模块只保存一次可装载、可执行、
//! 可验证的 Work Unit。`work_events` 仅追加，`work_surfaces` 是可从事件完全重建的缓存，
//! 模型上下文和 UI 都消费投影，审计与恢复读取原始事件。

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::{now_ms, SessionStore, StoreError};

pub const WORK_PROJECTION_FORMAT_VERSION: u32 = 1;
pub const MAX_WORK_OBJECTIVE_CHARS: usize = 1_000;
pub const MAX_WORK_LIST_ITEMS: usize = 32;
pub const MAX_WORK_ITEM_CHARS: usize = 2_000;
pub const MAX_CHECKPOINT_SUMMARY_CHARS: usize = 4_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkUnitStatus {
    Ready,
    Active,
    Blocked,
    Verifying,
    Done,
    Superseded,
}

impl WorkUnitStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Active => "active",
            Self::Blocked => "blocked",
            Self::Verifying => "verifying",
            Self::Done => "done",
            Self::Superseded => "superseded",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Superseded)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkUnitSpec {
    pub unit_id: String,
    pub requirement_id: String,
    pub objective: String,
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub verification: Vec<String>,
    pub base_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkCheckpoint {
    pub summary: String,
    pub next_action: String,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub retrieval_refs: Vec<String>,
    pub observed_head: String,
    pub observed_worktree_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkEvidence {
    pub criterion: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkFact {
    Created {
        spec: WorkUnitSpec,
    },
    Claimed {
        claimed_by: Option<String>,
    },
    Reassigned {
        claimed_by: Option<String>,
        reason: String,
    },
    Checkpointed {
        checkpoint: WorkCheckpoint,
    },
    Blocked {
        reason: String,
    },
    Unblocked {
        reason: String,
    },
    VerificationStarted,
    EvidenceAdded {
        evidence: WorkEvidence,
    },
    Completed,
    Superseded {
        reason: String,
    },
}

impl WorkFact {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Created { .. } => "work.created",
            Self::Claimed { .. } => "work.claimed",
            Self::Reassigned { .. } => "work.reassigned",
            Self::Checkpointed { .. } => "work.checkpointed",
            Self::Blocked { .. } => "work.blocked",
            Self::Unblocked { .. } => "work.unblocked",
            Self::VerificationStarted => "work.verification_started",
            Self::EvidenceAdded { .. } => "work.evidence_added",
            Self::Completed => "work.completed",
            Self::Superseded { .. } => "work.superseded",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredWorkEvent {
    pub event_id: String,
    pub unit_id: String,
    pub requirement_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub fact: WorkFact,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkProjection {
    pub format_version: u32,
    pub unit_id: String,
    pub requirement_id: String,
    pub objective: String,
    pub scope: Vec<String>,
    pub dependencies: Vec<String>,
    pub acceptance: Vec<String>,
    pub verification: Vec<String>,
    pub base_revision: String,
    pub status: WorkUnitStatus,
    pub claimed_by: Option<String>,
    pub blocked_reason: Option<String>,
    pub last_checkpoint: Option<WorkCheckpoint>,
    pub evidence: Vec<WorkEvidence>,
    pub terminal_reason: Option<String>,
    pub source_sequence: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl WorkProjection {
    pub fn dependencies_satisfied(&self, all: &[WorkProjection]) -> bool {
        self.dependencies.iter().all(|dependency| {
            all.iter()
                .find(|candidate| candidate.unit_id == *dependency)
                .is_some_and(|candidate| candidate.status == WorkUnitStatus::Done)
        })
    }
}

fn invalid(message: impl Into<String>) -> StoreError {
    StoreError::InvalidInput(message.into())
}

fn validate_spec(spec: &WorkUnitSpec) -> Result<(), StoreError> {
    if spec.requirement_id.trim().is_empty() || !spec.requirement_id.starts_with("R-") {
        return Err(invalid("work unit requirement_id 必须是 R-xxx"));
    }
    let expected_prefix = format!("{}/W", spec.requirement_id);
    if !spec.unit_id.starts_with(&expected_prefix)
        || spec.unit_id[expected_prefix.len()..]
            .parse::<u32>()
            .is_err()
    {
        return Err(invalid(format!(
            "work unit id 必须使用 {}/W<n>，实际为 {}",
            spec.requirement_id, spec.unit_id
        )));
    }
    if spec.objective.trim().is_empty() {
        return Err(invalid("work unit objective 不能为空"));
    }
    if spec.objective.chars().count() > MAX_WORK_OBJECTIVE_CHARS {
        return Err(invalid(format!(
            "work unit objective 超过 {MAX_WORK_OBJECTIVE_CHARS} 字符预算"
        )));
    }
    if spec.acceptance.is_empty() || spec.acceptance.iter().any(|item| item.trim().is_empty()) {
        return Err(invalid("work unit 至少需要一条非空 acceptance"));
    }
    if spec.base_revision.trim().is_empty() {
        return Err(invalid("work unit base_revision 不能为空"));
    }
    if spec.base_revision.chars().count() > MAX_WORK_ITEM_CHARS {
        return Err(invalid("work unit base_revision 超过上下文预算"));
    }
    if spec.dependencies.iter().any(|id| id == &spec.unit_id) {
        return Err(invalid("work unit 不能依赖自身"));
    }
    for (name, values) in [
        ("scope", &spec.scope),
        ("dependencies", &spec.dependencies),
        ("acceptance", &spec.acceptance),
        ("verification", &spec.verification),
    ] {
        if values.len() > MAX_WORK_LIST_ITEMS {
            return Err(invalid(format!(
                "work unit {name} 超过 {MAX_WORK_LIST_ITEMS} 项预算"
            )));
        }
        if values
            .iter()
            .any(|value| value.chars().count() > MAX_WORK_ITEM_CHARS)
        {
            return Err(invalid(format!(
                "work unit {name} 单项超过 {MAX_WORK_ITEM_CHARS} 字符预算"
            )));
        }
    }
    Ok(())
}

fn validate_checkpoint(checkpoint: &WorkCheckpoint) -> Result<(), StoreError> {
    if checkpoint.summary.trim().is_empty() || checkpoint.next_action.trim().is_empty() {
        return Err(invalid("checkpoint summary 与 next_action 不能为空"));
    }
    if checkpoint.summary.chars().count() > MAX_CHECKPOINT_SUMMARY_CHARS
        || checkpoint.next_action.chars().count() > MAX_WORK_ITEM_CHARS
        || checkpoint.observed_head.chars().count() > MAX_WORK_ITEM_CHARS
        || checkpoint.observed_worktree_hash.chars().count() > MAX_WORK_ITEM_CHARS
    {
        return Err(invalid("checkpoint 超过上下文预算"));
    }
    for (name, values) in [
        ("decisions", &checkpoint.decisions),
        ("retrieval_refs", &checkpoint.retrieval_refs),
    ] {
        if values.len() > MAX_WORK_LIST_ITEMS
            || values
                .iter()
                .any(|value| value.chars().count() > MAX_WORK_ITEM_CHARS)
        {
            return Err(invalid(format!("checkpoint {name} 超过上下文预算")));
        }
    }
    Ok(())
}

fn validate_bounded_text(value: &str, name: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{name} 不能为空")));
    }
    if value.chars().count() > MAX_WORK_ITEM_CHARS {
        return Err(invalid(format!("{name} 超过上下文预算")));
    }
    Ok(())
}

fn apply_fact(
    current: Option<WorkProjection>,
    event: &StoredWorkEvent,
) -> Result<WorkProjection, StoreError> {
    match (&current, &event.fact) {
        (None, WorkFact::Created { spec }) => {
            validate_spec(spec)?;
            if spec.unit_id != event.unit_id || spec.requirement_id != event.requirement_id {
                return Err(invalid("work.created 的 spec 与事件身份不一致"));
            }
            return Ok(WorkProjection {
                format_version: WORK_PROJECTION_FORMAT_VERSION,
                unit_id: spec.unit_id.clone(),
                requirement_id: spec.requirement_id.clone(),
                objective: spec.objective.trim().to_string(),
                scope: spec.scope.clone(),
                dependencies: spec.dependencies.clone(),
                acceptance: spec.acceptance.clone(),
                verification: spec.verification.clone(),
                base_revision: spec.base_revision.clone(),
                status: WorkUnitStatus::Ready,
                claimed_by: None,
                blocked_reason: None,
                last_checkpoint: None,
                evidence: Vec::new(),
                terminal_reason: None,
                source_sequence: event.sequence,
                created_at: event.created_at,
                updated_at: event.created_at,
            });
        }
        (None, _) => return Err(invalid("work unit 第一条事件必须是 work.created")),
        (Some(_), WorkFact::Created { .. }) => {
            return Err(invalid("work.created 只能是第一条事件"))
        }
        _ => {}
    }

    let mut projection = current.expect("上面的 match 已排除 None");
    if projection.status.is_terminal() {
        return Err(invalid(format!(
            "{} 已是终态 {}，不能再追加 {}",
            projection.unit_id,
            projection.status.as_str(),
            event.event_type
        )));
    }
    match &event.fact {
        WorkFact::Claimed { claimed_by } => {
            if projection.status != WorkUnitStatus::Ready {
                return Err(invalid("只有 ready work unit 可以 claim"));
            }
            if claimed_by
                .as_ref()
                .is_some_and(|owner| owner.chars().count() > MAX_WORK_ITEM_CHARS)
            {
                return Err(invalid("claimed_by 超过上下文预算"));
            }
            projection.status = WorkUnitStatus::Active;
            projection.claimed_by = claimed_by.clone();
        }
        WorkFact::Reassigned { claimed_by, reason } => {
            if !matches!(
                projection.status,
                WorkUnitStatus::Active | WorkUnitStatus::Verifying
            ) {
                return Err(invalid("只有 active/verifying work unit 可以 reassign"));
            }
            validate_bounded_text(reason, "reassign reason")?;
            if claimed_by
                .as_ref()
                .is_some_and(|owner| owner.chars().count() > MAX_WORK_ITEM_CHARS)
            {
                return Err(invalid("claimed_by 超过上下文预算"));
            }
            projection.claimed_by = claimed_by.clone();
        }
        WorkFact::Checkpointed { checkpoint } => {
            if !matches!(
                projection.status,
                WorkUnitStatus::Active | WorkUnitStatus::Blocked | WorkUnitStatus::Verifying
            ) {
                return Err(invalid(
                    "只有 active/blocked/verifying work unit 可以 checkpoint",
                ));
            }
            validate_checkpoint(checkpoint)?;
            projection.last_checkpoint = Some(checkpoint.clone());
        }
        WorkFact::Blocked { reason } => {
            validate_bounded_text(reason, "block reason")?;
            projection.status = WorkUnitStatus::Blocked;
            projection.blocked_reason = Some(reason.trim().to_string());
        }
        WorkFact::Unblocked { reason } => {
            if projection.status != WorkUnitStatus::Blocked {
                return Err(invalid("只有 blocked work unit 可以 unblock"));
            }
            validate_bounded_text(reason, "unblock reason")?;
            projection.status = WorkUnitStatus::Ready;
            projection.blocked_reason = None;
            projection.claimed_by = None;
        }
        WorkFact::VerificationStarted => {
            if projection.status != WorkUnitStatus::Active {
                return Err(invalid("只有 active work unit 可以进入 verifying"));
            }
            projection.status = WorkUnitStatus::Verifying;
        }
        WorkFact::EvidenceAdded { evidence } => {
            if !matches!(
                projection.status,
                WorkUnitStatus::Active | WorkUnitStatus::Verifying
            ) {
                return Err(invalid("只有 active/verifying work unit 可以登记 evidence"));
            }
            if evidence.criterion.trim().is_empty() || evidence.evidence_refs.is_empty() {
                return Err(invalid("evidence 必须包含 criterion 与至少一个引用"));
            }
            if !projection.acceptance.contains(&evidence.criterion) {
                return Err(invalid(format!(
                    "evidence criterion 不属于本单元 acceptance: {}",
                    evidence.criterion
                )));
            }
            if evidence.evidence_refs.len() > MAX_WORK_LIST_ITEMS
                || evidence
                    .evidence_refs
                    .iter()
                    .any(|reference| reference.chars().count() > MAX_WORK_ITEM_CHARS)
            {
                return Err(invalid("evidence_refs 超过上下文预算"));
            }
            if let Some(existing) = projection
                .evidence
                .iter_mut()
                .find(|item| item.criterion == evidence.criterion)
            {
                for reference in &evidence.evidence_refs {
                    if !existing.evidence_refs.contains(reference) {
                        existing.evidence_refs.push(reference.clone());
                    }
                }
                if existing.evidence_refs.len() > MAX_WORK_LIST_ITEMS {
                    return Err(invalid("同一 acceptance 的 evidence_refs 超过上下文预算"));
                }
            } else {
                projection.evidence.push(evidence.clone());
            }
        }
        WorkFact::Completed => {
            if projection.status != WorkUnitStatus::Verifying {
                return Err(invalid("work unit 必须先进入 verifying 才能 complete"));
            }
            let uncovered = projection
                .acceptance
                .iter()
                .filter(|criterion| {
                    !projection
                        .evidence
                        .iter()
                        .any(|evidence| evidence.criterion == **criterion)
                })
                .cloned()
                .collect::<Vec<_>>();
            if !uncovered.is_empty() {
                return Err(invalid(format!(
                    "work unit 仍有未覆盖 acceptance: {}",
                    uncovered.join(" | ")
                )));
            }
            projection.status = WorkUnitStatus::Done;
            projection.claimed_by = None;
        }
        WorkFact::Superseded { reason } => {
            validate_bounded_text(reason, "superseded reason")?;
            projection.status = WorkUnitStatus::Superseded;
            projection.terminal_reason = Some(reason.trim().to_string());
            projection.claimed_by = None;
        }
        WorkFact::Created { .. } => unreachable!(),
    }
    projection.source_sequence = event.sequence;
    projection.updated_at = event.created_at;
    Ok(projection)
}

pub fn project_work_events(events: &[StoredWorkEvent]) -> Result<WorkProjection, StoreError> {
    let mut projection = None;
    for event in events {
        projection = Some(apply_fact(projection, event)?);
    }
    projection.ok_or_else(|| invalid("work unit 没有事件"))
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredWorkEvent> {
    let payload_json: String = row.get(5)?;
    let fact = serde_json::from_str(&payload_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(StoredWorkEvent {
        event_id: row.get(0)?,
        unit_id: row.get(1)?,
        requirement_id: row.get(2)?,
        sequence: row.get(3)?,
        event_type: row.get(4)?,
        fact,
        created_at: row.get(6)?,
    })
}

fn upsert_surface_tx(tx: &Transaction<'_>, projection: &WorkProjection) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO work_surfaces
            (unit_id, requirement_id, status, source_sequence, projection_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(unit_id) DO UPDATE SET
            requirement_id = excluded.requirement_id,
            status = excluded.status,
            source_sequence = excluded.source_sequence,
            projection_json = excluded.projection_json,
            updated_at = excluded.updated_at",
        params![
            projection.unit_id,
            projection.requirement_id,
            projection.status.as_str(),
            projection.source_sequence,
            serde_json::to_string(projection)?,
            projection.updated_at,
        ],
    )?;
    Ok(())
}

fn append_event_tx(
    tx: &Transaction<'_>,
    unit_id: &str,
    requirement_id: &str,
    fact: &WorkFact,
) -> Result<StoredWorkEvent, StoreError> {
    let sequence: i64 = tx.query_row(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM work_events WHERE unit_id = ?1",
        params![unit_id],
        |row| row.get(0),
    )?;
    let created_at = now_ms();
    let safe_unit_id = unit_id.replace(['/', '\\'], "_");
    let event_id = format!("work_evt_{safe_unit_id}_{sequence}");
    tx.execute(
        "INSERT INTO work_events
            (event_id, unit_id, requirement_id, sequence, event_type, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event_id,
            unit_id,
            requirement_id,
            sequence,
            fact.event_type(),
            serde_json::to_string(fact)?,
            created_at,
        ],
    )?;
    Ok(StoredWorkEvent {
        event_id,
        unit_id: unit_id.to_string(),
        requirement_id: requirement_id.to_string(),
        sequence,
        event_type: fact.event_type().to_string(),
        fact: fact.clone(),
        created_at,
    })
}

impl SessionStore {
    pub fn create_work_unit(&self, spec: WorkUnitSpec) -> Result<WorkProjection, StoreError> {
        validate_spec(&spec)?;
        let tx = self.connection.unchecked_transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM work_surfaces WHERE unit_id = ?1)",
            params![spec.unit_id],
            |row| row.get(0),
        )?;
        if exists {
            return Err(invalid(format!("work unit {} 已存在", spec.unit_id)));
        }
        for dependency in &spec.dependencies {
            let dependency_exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM work_surfaces WHERE unit_id = ?1)",
                params![dependency],
                |row| row.get(0),
            )?;
            if !dependency_exists {
                return Err(invalid(format!(
                    "work unit dependency 不存在: {dependency}"
                )));
            }
        }
        let fact = WorkFact::Created { spec: spec.clone() };
        let event = append_event_tx(&tx, &spec.unit_id, &spec.requirement_id, &fact)?;
        let projection = apply_fact(None, &event)?;
        upsert_surface_tx(&tx, &projection)?;
        tx.commit()?;
        Ok(projection)
    }

    pub fn append_work_fact(
        &self,
        unit_id: &str,
        fact: WorkFact,
    ) -> Result<WorkProjection, StoreError> {
        if matches!(fact, WorkFact::Created { .. }) {
            return Err(invalid("请使用 create_work_unit 创建 work unit"));
        }
        let tx = self.connection.unchecked_transaction()?;
        let projection_json: Option<String> = tx
            .query_row(
                "SELECT projection_json FROM work_surfaces WHERE unit_id = ?1",
                params![unit_id],
                |row| row.get(0),
            )
            .optional()?;
        let current: WorkProjection = serde_json::from_str(
            &projection_json.ok_or_else(|| invalid(format!("未知 work unit: {unit_id}")))?,
        )?;
        let event = append_event_tx(&tx, unit_id, &current.requirement_id, &fact)?;
        let next = apply_fact(Some(current), &event)?;
        upsert_surface_tx(&tx, &next)?;
        tx.commit()?;
        Ok(next)
    }

    pub fn get_work_unit(&self, unit_id: &str) -> Result<Option<WorkProjection>, StoreError> {
        let projection_json: Option<String> = self
            .connection
            .query_row(
                "SELECT projection_json FROM work_surfaces WHERE unit_id = ?1",
                params![unit_id],
                |row| row.get(0),
            )
            .optional()?;
        projection_json
            .map(|json| serde_json::from_str(&json).map_err(StoreError::from))
            .transpose()
    }

    pub fn list_work_units(
        &self,
        requirement_id: Option<&str>,
    ) -> Result<Vec<WorkProjection>, StoreError> {
        let json_rows = if let Some(requirement_id) = requirement_id {
            let mut statement = self.connection.prepare(
                "SELECT projection_json FROM work_surfaces
                 WHERE requirement_id = ?1 ORDER BY unit_id",
            )?;
            let rows = statement
                .query_map(params![requirement_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        } else {
            let mut statement = self.connection.prepare(
                "SELECT projection_json FROM work_surfaces ORDER BY requirement_id, unit_id",
            )?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        json_rows
            .into_iter()
            .map(|json| serde_json::from_str(&json).map_err(StoreError::from))
            .collect()
    }

    pub fn list_work_events(&self, unit_id: &str) -> Result<Vec<StoredWorkEvent>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT event_id, unit_id, requirement_id, sequence, event_type, payload_json, created_at
             FROM work_events WHERE unit_id = ?1 ORDER BY sequence",
        )?;
        let events = statement
            .query_map(params![unit_id], event_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)?;
        Ok(events)
    }

    pub fn rebuild_work_surface(&self, unit_id: &str) -> Result<WorkProjection, StoreError> {
        let events = self.list_work_events(unit_id)?;
        let projection = project_work_events(&events)?;
        let tx = self.connection.unchecked_transaction()?;
        upsert_surface_tx(&tx, &projection)?;
        tx.commit()?;
        Ok(projection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, dependencies: Vec<String>) -> WorkUnitSpec {
        WorkUnitSpec {
            unit_id: id.into(),
            requirement_id: "R-900".into(),
            objective: format!("完成 {id}"),
            scope: vec!["crates/example".into()],
            dependencies,
            acceptance: vec!["criterion-1".into()],
            verification: vec!["cargo test -p example".into()],
            base_revision: "0123456789abcdef".into(),
        }
    }

    #[test]
    fn work_events_replay_matches_surface_and_enforces_acceptance() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_work_unit(spec("R-900/W1", vec![])).unwrap();
        store
            .append_work_fact(
                "R-900/W1",
                WorkFact::Claimed {
                    claimed_by: Some("line-a".into()),
                },
            )
            .unwrap();
        store
            .append_work_fact("R-900/W1", WorkFact::VerificationStarted)
            .unwrap();
        let error = store
            .append_work_fact("R-900/W1", WorkFact::Completed)
            .unwrap_err();
        assert!(error.to_string().contains("未覆盖 acceptance"));
        store
            .append_work_fact(
                "R-900/W1",
                WorkFact::EvidenceAdded {
                    evidence: WorkEvidence {
                        criterion: "criterion-1".into(),
                        evidence_refs: vec!["T-1".into()],
                    },
                },
            )
            .unwrap();
        let completed = store
            .append_work_fact("R-900/W1", WorkFact::Completed)
            .unwrap();
        assert_eq!(completed.status, WorkUnitStatus::Done);

        let replayed = project_work_events(&store.list_work_events("R-900/W1").unwrap()).unwrap();
        assert_eq!(completed, replayed);
        let rebuilt = store.rebuild_work_surface("R-900/W1").unwrap();
        assert_eq!(completed, rebuilt);
    }

    #[test]
    fn dependencies_are_first_class_and_must_exist() {
        let store = SessionStore::open_in_memory().unwrap();
        let missing = store
            .create_work_unit(spec("R-900/W2", vec!["R-900/W1".into()]))
            .unwrap_err();
        assert!(missing.to_string().contains("dependency 不存在"));
        store.create_work_unit(spec("R-900/W1", vec![])).unwrap();
        let second = store
            .create_work_unit(spec("R-900/W2", vec!["R-900/W1".into()]))
            .unwrap();
        let all = store.list_work_units(Some("R-900")).unwrap();
        assert!(!second.dependencies_satisfied(&all));
    }

    #[test]
    fn checkpoint_replaces_surface_without_losing_audit_events() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_work_unit(spec("R-900/W1", vec![])).unwrap();
        store
            .append_work_fact("R-900/W1", WorkFact::Claimed { claimed_by: None })
            .unwrap();
        for index in 1..=2 {
            store
                .append_work_fact(
                    "R-900/W1",
                    WorkFact::Checkpointed {
                        checkpoint: WorkCheckpoint {
                            summary: format!("summary-{index}"),
                            next_action: format!("next-{index}"),
                            decisions: vec![],
                            retrieval_refs: vec![],
                            observed_head: format!("head-{index}"),
                            observed_worktree_hash: format!("tree-{index}"),
                        },
                    },
                )
                .unwrap();
        }
        let projection = store.get_work_unit("R-900/W1").unwrap().unwrap();
        assert_eq!(projection.last_checkpoint.unwrap().summary, "summary-2");
        assert_eq!(store.list_work_events("R-900/W1").unwrap().len(), 4);
    }

    #[test]
    fn context_budget_and_reassignment_are_enforced() {
        let store = SessionStore::open_in_memory().unwrap();
        let mut oversized = spec("R-900/W1", vec![]);
        oversized.objective = "x".repeat(MAX_WORK_OBJECTIVE_CHARS + 1);
        assert!(store.create_work_unit(oversized).is_err());

        store.create_work_unit(spec("R-900/W1", vec![])).unwrap();
        store
            .append_work_fact(
                "R-900/W1",
                WorkFact::Claimed {
                    claimed_by: Some("line-a".into()),
                },
            )
            .unwrap();
        let reassigned = store
            .append_work_fact(
                "R-900/W1",
                WorkFact::Reassigned {
                    claimed_by: Some("line-b".into()),
                    reason: "用户改派".into(),
                },
            )
            .unwrap();
        assert_eq!(reassigned.claimed_by.as_deref(), Some("line-b"));
        assert_eq!(store.list_work_events("R-900/W1").unwrap().len(), 3);

        let unknown_criterion = store
            .append_work_fact(
                "R-900/W1",
                WorkFact::EvidenceAdded {
                    evidence: WorkEvidence {
                        criterion: "not-in-acceptance".into(),
                        evidence_refs: vec!["ref".into()],
                    },
                },
            )
            .unwrap_err();
        assert!(unknown_criterion.to_string().contains("不属于"));
    }
}
