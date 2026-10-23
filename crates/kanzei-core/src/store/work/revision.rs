//! Versioned direction review and atomic graph replanning over the existing event store.
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub type WorkGraphVersion = BTreeMap<String, i64>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkRevisionRequest {
    pub revision_id: String,
    pub anchor_id: String,
    pub direction: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkRevisionDisposition {
    Retain,
    Revise,
    Reverify,
    Replace,
    Retire,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkRevisionDecision {
    pub unit_id: String,
    pub disposition: WorkRevisionDisposition,
    pub reason: String,
    #[serde(default)]
    pub contract: Option<WorkUnitSpec>,
    /// Explicitly re-attest coverage without rewriting a completed result's acceptance.
    #[serde(default)]
    pub covers: Option<Vec<String>>,
    #[serde(default)]
    pub replacements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkRevisionResolution {
    pub decision: WorkRevisionDecision,
    pub basis_sequence: i64,
    /// Recorded on the anchor so all new work can be traced to this revision.
    pub created_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkReview {
    pub request: WorkRevisionRequest,
    pub basis_sequence: i64,
    pub resolution: Option<WorkRevisionResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "relation", content = "unit_id", rename_all = "snake_case")]
pub enum WorkImpactCause {
    Direction,
    ChildOf(String),
    DependsOn(String),
    Integrates(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkImpactNode {
    pub unit_id: String,
    pub causes: Vec<WorkImpactCause>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkImpact {
    pub anchor_id: String,
    /// Complete read set detects concurrent changes, including newly created dependencies.
    pub versions: WorkGraphVersion,
    pub affected: Vec<WorkImpactNode>,
}

impl WorkProjection {
    pub fn review_pending(&self) -> bool {
        self.review
            .as_ref()
            .is_some_and(|review| review.resolution.is_none())
    }

    pub fn is_current(&self) -> bool {
        self.status != WorkUnitStatus::Superseded
            && !self
                .review
                .as_ref()
                .and_then(|review| review.resolution.as_ref())
                .is_some_and(|resolution| {
                    matches!(
                        resolution.decision.disposition,
                        WorkRevisionDisposition::Reverify
                            | WorkRevisionDisposition::Replace
                            | WorkRevisionDisposition::Retire
                    )
                })
    }

    pub fn result_is_current(&self) -> bool {
        self.status == WorkUnitStatus::Done && self.is_current() && !self.review_pending()
    }
}

fn versions(all: &[WorkProjection]) -> WorkGraphVersion {
    all.iter()
        .map(|node| (node.unit_id.clone(), node.source_sequence))
        .collect()
}

fn check_version(all: &[WorkProjection], expected: &WorkGraphVersion) -> Result<(), StoreError> {
    if &versions(all) != expected {
        return Err(invalid(
            "工作图已变化；重读 impact 后重新评估，不能覆盖未知版本",
        ));
    }
    Ok(())
}

fn impact(all: &[WorkProjection], anchor: &str) -> Result<WorkImpact, StoreError> {
    if !all
        .iter()
        .any(|node| node.unit_id == anchor && node.is_current())
    {
        return Err(invalid("方向变更节点不存在或已被替代"));
    }
    let mut causes: BTreeMap<String, Vec<WorkImpactCause>> = BTreeMap::new();
    let mut queue = VecDeque::from([(anchor.to_owned(), true, WorkImpactCause::Direction)]);
    // Integration-only ancestors invalidate consumers, but do not fan out to unrelated siblings.
    let mut visited: BTreeMap<String, bool> = BTreeMap::new();
    while let Some((id, descend, cause)) = queue.pop_front() {
        let reasons = causes.entry(id.clone()).or_default();
        if !reasons.contains(&cause) {
            reasons.push(cause);
        }
        if visited
            .get(&id)
            .is_some_and(|previous| *previous || !descend)
        {
            continue;
        }
        visited.insert(id.clone(), descend);
        let node = all
            .iter()
            .find(|node| node.unit_id == id)
            .ok_or_else(|| invalid("影响路径引用了不存在的节点"))?;
        for other in all.iter().filter(|other| other.is_current()) {
            if descend && other.node.parent_id.as_deref() == Some(&id) {
                queue.push_back((
                    other.unit_id.clone(),
                    true,
                    WorkImpactCause::ChildOf(id.clone()),
                ));
            }
            if other.dependencies.contains(&id) {
                queue.push_back((
                    other.unit_id.clone(),
                    true,
                    WorkImpactCause::DependsOn(id.clone()),
                ));
            }
            if node.node.parent_id.as_deref() == Some(&other.unit_id) {
                queue.push_back((
                    other.unit_id.clone(),
                    false,
                    WorkImpactCause::Integrates(id.clone()),
                ));
            }
        }
    }
    Ok(WorkImpact {
        anchor_id: anchor.into(),
        versions: versions(all),
        affected: causes
            .into_iter()
            .map(|(unit_id, causes)| WorkImpactNode { unit_id, causes })
            .collect(),
    })
}

pub(super) fn apply_review_requested(
    node: &mut WorkProjection,
    request: &WorkRevisionRequest,
) -> Result<(), StoreError> {
    for (label, value) in [
        ("revision_id", &request.revision_id),
        ("anchor_id", &request.anchor_id),
        ("direction", &request.direction),
        ("revision reason", &request.reason),
    ] {
        validate_bounded_text(value, label)?;
    }
    if node.review_pending() || !node.is_current() {
        return Err(invalid("节点已有待复核方向或已被替代"));
    }
    node.review = Some(WorkReview {
        request: request.clone(),
        basis_sequence: node.source_sequence,
        resolution: None,
    });
    Ok(())
}

pub(super) fn apply_review_resolution(
    node: &mut WorkProjection,
    revision_id: &str,
    resolution: &WorkRevisionResolution,
) -> Result<(), StoreError> {
    let review = node
        .review
        .as_ref()
        .ok_or_else(|| invalid("节点没有方向复核请求"))?;
    if review.request.revision_id != revision_id
        || review.resolution.is_some()
        || resolution.basis_sequence != node.source_sequence
    {
        return Err(invalid("方向复核版本已变化"));
    }
    if matches!(
        node.status,
        WorkUnitStatus::Active | WorkUnitStatus::Verifying
    ) || node
        .background_verification
        .as_ref()
        .is_some_and(|job| job.pending)
    {
        return Err(invalid("执行者或验证任务尚未交还节点，不能应用新方向"));
    }
    let decision = &resolution.decision;
    if decision.unit_id != node.unit_id {
        return Err(invalid("复核处置身份不匹配"));
    }
    validate_bounded_text(&decision.reason, "review decision reason")?;
    if decision.disposition != WorkRevisionDisposition::Revise && decision.contract.is_some() {
        return Err(invalid("只有 revise 可以替换契约"));
    }
    if decision.disposition != WorkRevisionDisposition::Retain && decision.covers.is_some() {
        return Err(invalid(
            "只有 retain 可以重新声明父级覆盖；revise 使用完整契约",
        ));
    }
    match decision.disposition {
        WorkRevisionDisposition::Retain => {
            if let Some(covers) = &decision.covers {
                node.node.covers = covers.clone();
                graph::validate_node(&node.node)?;
            }
        }
        WorkRevisionDisposition::Revise => {
            if node.status.is_terminal() {
                return Err(invalid(
                    "历史完成契约不能改写；使用 reverify 或 replace 创建后继工作",
                ));
            }
            let spec = decision
                .contract
                .as_ref()
                .ok_or_else(|| invalid("revise 必须提供完整契约"))?;
            replace_contract(node, spec)?;
        }
        WorkRevisionDisposition::Reverify | WorkRevisionDisposition::Replace => {
            if decision.replacements.is_empty() {
                return Err(invalid("替代或重验必须指定后继节点"));
            }
            if decision.disposition == WorkRevisionDisposition::Reverify
                && node.status != WorkUnitStatus::Done
            {
                return Err(invalid(
                    "reverify 用于已完成结果；未完成工作使用 revise 或 replace",
                ));
            }
            if !node.status.is_terminal() {
                node.status = WorkUnitStatus::Superseded;
                node.terminal_reason = Some(decision.reason.clone());
                node.claimed_by = None;
            }
        }
        WorkRevisionDisposition::Retire => {
            if !node.status.is_terminal() {
                node.status = WorkUnitStatus::Superseded;
                node.terminal_reason = Some(decision.reason.clone());
                node.claimed_by = None;
            }
        }
    }
    if !matches!(
        decision.disposition,
        WorkRevisionDisposition::Reverify | WorkRevisionDisposition::Replace
    ) && !decision.replacements.is_empty()
    {
        return Err(invalid("只有 replace/reverify 可以声明后继节点"));
    }
    node.review.as_mut().expect("review checked").resolution = Some(resolution.clone());
    Ok(())
}

impl SessionStore {
    pub fn work_revision_impact(&self, anchor: &str) -> Result<WorkImpact, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        impact(&graph::read_graph(&tx)?, anchor)
    }

    /// Register intent and freeze every candidate atomically. Existing executors must yield.
    pub fn begin_work_revision(
        &self,
        anchor: &str,
        expected: &WorkGraphVersion,
        direction: &str,
        reason: &str,
    ) -> Result<WorkRevisionRequest, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let all = graph::read_graph(&tx)?;
        check_version(&all, expected)?;
        let affected = impact(&all, anchor)?;
        let current = all
            .iter()
            .find(|node| node.unit_id == anchor)
            .expect("impact checked anchor");
        let sequence = current
            .source_sequence
            .checked_add(1)
            .ok_or_else(|| invalid("节点版本号已耗尽"))?;
        let request = WorkRevisionRequest {
            revision_id: format!("{anchor}@{sequence}"),
            anchor_id: anchor.into(),
            direction: direction.into(),
            reason: reason.into(),
        };
        for candidate in affected.affected {
            let node = all
                .iter()
                .find(|node| node.unit_id == candidate.unit_id)
                .expect("impact node exists");
            let event = append_event_tx(
                &tx,
                &node.unit_id,
                &node.requirement_id,
                &WorkFact::ReviewRequested {
                    request: request.clone(),
                },
            )?;
            let updated = apply_fact(Some(node.clone()), &event)?;
            upsert_surface_tx(&tx, &updated)?;
        }
        tx.commit()?;
        Ok(request)
    }

    /// One observed graph, one disposition per pending node, one atomic final graph.
    pub fn apply_work_revision(
        &self,
        anchor: &str,
        revision_id: &str,
        expected: &WorkGraphVersion,
        decisions: Vec<WorkRevisionDecision>,
        additions: Vec<WorkUnitSpec>,
    ) -> Result<Vec<WorkProjection>, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let mut all = graph::read_graph(&tx)?;
        check_version(&all, expected)?;
        let pending: BTreeSet<String> = all
            .iter()
            .filter(|node| {
                node.review_pending()
                    && node.review.as_ref().is_some_and(|review| {
                        review.request.revision_id == revision_id
                            && review.request.anchor_id == anchor
                    })
            })
            .map(|node| node.unit_id.clone())
            .collect();
        if !pending.contains(anchor) {
            return Err(invalid("找不到此目标的待复核方向"));
        }
        let ids: BTreeSet<_> = decisions
            .iter()
            .map(|decision| decision.unit_id.clone())
            .collect();
        if decisions.len() != ids.len() || ids != pending {
            return Err(invalid(
                "必须逐一处置全部受影响节点，不能遗漏、重复或混入其他节点",
            ));
        }
        let outcomes: BTreeSet<_> = all
            .iter()
            .filter(|node| pending.contains(&node.unit_id))
            .map(|node| node.requirement_id.clone())
            .collect();
        let created_ids: Vec<_> = additions.iter().map(|spec| spec.unit_id.clone()).collect();
        for decision in decisions {
            let index = all
                .iter()
                .position(|node| node.unit_id == decision.unit_id)
                .expect("decision set checked");
            let node = all[index].clone();
            let resolution = WorkRevisionResolution {
                decision,
                basis_sequence: node.source_sequence,
                created_ids: if node.unit_id == anchor {
                    created_ids.clone()
                } else {
                    vec![]
                },
            };
            let event = append_event_tx(
                &tx,
                &node.unit_id,
                &node.requirement_id,
                &WorkFact::ReviewResolved {
                    revision_id: revision_id.into(),
                    resolution,
                },
            )?;
            all[index] = apply_fact(Some(node), &event)?;
        }
        for spec in additions {
            validate_spec(&spec)?;
            if !outcomes.contains(&spec.requirement_id)
                || all.iter().any(|node| node.unit_id == spec.unit_id)
            {
                return Err(invalid("新增节点必须属于受影响 Outcome 且使用新的编号"));
            }
            let event = append_event_tx(
                &tx,
                &spec.unit_id,
                &spec.requirement_id,
                &WorkFact::Created { spec: spec.clone() },
            )?;
            all.push(apply_fact(None, &event)?);
        }
        for node in all.iter().filter(|node| node.is_current()) {
            graph::validate_parent(node, &all)?;
            if pending.contains(&node.unit_id) || created_ids.contains(&node.unit_id) {
                graph::validate_contract_links(node, &all)?;
            }
            if !node.status.is_terminal() {
                for id in node.dependencies.iter().chain(node.node.parent_id.iter()) {
                    if !all
                        .iter()
                        .any(|other| other.unit_id == *id && other.is_current())
                    {
                        return Err(invalid(format!(
                            "{} 仍引用已替代或撤销的节点 {id}，请修正关系",
                            node.unit_id
                        )));
                    }
                }
            }
        }
        for node in all.iter().filter(|node| pending.contains(&node.unit_id)) {
            let decision = &node
                .review
                .as_ref()
                .expect("review exists")
                .resolution
                .as_ref()
                .expect("resolved")
                .decision;
            let unique: BTreeSet<_> = decision.replacements.iter().collect();
            if unique.len() != decision.replacements.len() {
                return Err(invalid("后继节点不能重复"));
            }
            for id in &decision.replacements {
                if id == &node.unit_id
                    || !all.iter().any(|other| {
                        other.unit_id == *id
                            && other.is_current()
                            && !other.review_pending()
                            && (decision.disposition != WorkRevisionDisposition::Reverify
                                || !other.status.is_terminal())
                    })
                {
                    return Err(invalid("后继节点必须存在、适用于当前方向且不是自身"));
                }
            }
        }
        graph::validate_graph(&all)?;
        let result: Vec<_> = all
            .into_iter()
            .filter(|node| pending.contains(&node.unit_id) || created_ids.contains(&node.unit_id))
            .collect();
        for node in &result {
            upsert_surface_tx(&tx, node)?;
        }
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
