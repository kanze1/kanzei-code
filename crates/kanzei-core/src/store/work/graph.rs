//! Recursive ownership and dependency invariants over the existing work event store.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkNodeKind {
    Goal,
    #[default]
    Requirement,
    Implementation,
    Defect,
    Research,
    Design,
    Documentation,
    Verification,
}

/// A project vocabulary entry, not an authority or a tool grant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkNodeType {
    pub name: String,
    pub description: String,
    pub deliverable: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkNodeContract {
    #[serde(default)]
    pub kind: WorkNodeKind,
    /// Reuse an existing project definition before declaring a new subtype.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_type: Option<WorkNodeType>,
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Exact criteria from the immediate parent which this result supports.
    #[serde(default)]
    pub covers: Vec<String>,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkNodeStep {
    Plan,
    Execute,
    Integrate,
    Wait,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkNodeReadiness {
    pub step: WorkNodeStep,
    pub blockers: Vec<String>,
    pub uncovered: Vec<String>,
}

pub(super) fn validate_node(node: &WorkNodeContract) -> Result<(), StoreError> {
    if let Some(kind) = &node.custom_type {
        for (label, value) in [
            ("type name", &kind.name),
            ("type description", &kind.description),
            ("type deliverable", &kind.deliverable),
        ] {
            validate_bounded_text(value, label)?;
            if value.trim() != value {
                return Err(invalid("扩展类型字段不能含首尾空白"));
            }
        }
        if kind.name.chars().count() > 40 {
            return Err(invalid("扩展类型名称最多 40 个字符"));
        }
    }
    if let Some(parent) = &node.parent_id {
        validate_bounded_text(parent, "parent_id")?;
        if node.covers.is_empty() {
            return Err(invalid("子节点必须声明 covers，说明支撑父目标的哪条验收"));
        }
    } else if !node.covers.is_empty() {
        return Err(invalid("顶层节点不能声明不存在的父级验收"));
    }
    if node.explanation.chars().count() > MAX_CHECKPOINT_SUMMARY_CHARS {
        return Err(invalid("节点解释超过上下文预算"));
    }
    for (name, values) in [
        ("covers", &node.covers),
        ("questions", &node.questions),
        ("source_refs", &node.source_refs),
    ] {
        if values.len() > MAX_WORK_LIST_ITEMS {
            return Err(invalid(format!("{name} 超过节点预算")));
        }
        let mut seen = BTreeSet::new();
        for value in values {
            validate_bounded_text(value, name)?;
            if !seen.insert(value) {
                return Err(invalid(format!("{name} 包含重复内容")));
            }
        }
    }
    Ok(())
}

impl WorkProjection {
    /// Planning may resolve missing research/dependencies, but cannot mutate a
    /// suspended branch or race an executor. Callers additionally enforce ownership.
    pub fn planning_blockers(&self, all: &[WorkProjection]) -> Vec<String> {
        let mut blockers = Vec::new();
        if !matches!(self.status, WorkUnitStatus::Ready | WorkUnitStatus::Active) {
            blockers.push(format!("节点状态 {} 不允许修改计划", self.status.as_str()));
        }
        if self
            .background_verification
            .as_ref()
            .is_some_and(|job| job.pending)
        {
            blockers.push("后台验证尚未结束".into());
        }
        let mut current = self;
        let mut seen = BTreeSet::new();
        while let Some(id) = &current.node.parent_id {
            if !seen.insert(id) {
                blockers.push("节点归属存在循环".into());
                break;
            }
            let Some(parent) = all.iter().find(|node| node.unit_id == *id) else {
                blockers.push(format!("父节点不存在: {id}"));
                break;
            };
            if parent.status.is_terminal() || parent.status == WorkUnitStatus::Blocked {
                blockers.push(format!("祖先 {id} 已暂停或结束"));
            }
            current = parent;
        }
        let mut descendants = vec![self.unit_id.as_str()];
        let mut visited = BTreeSet::new();
        while let Some(id) = descendants.pop() {
            if !visited.insert(id) {
                continue;
            }
            for child in all
                .iter()
                .filter(|node| node.node.parent_id.as_deref() == Some(id))
            {
                if matches!(
                    child.status,
                    WorkUnitStatus::Active | WorkUnitStatus::Verifying
                ) {
                    blockers.push(format!("子节点正在执行或验证: {}", child.unit_id));
                }
                descendants.push(&child.unit_id);
            }
        }
        blockers
    }

    pub fn spec(&self) -> WorkUnitSpec {
        WorkUnitSpec {
            unit_id: self.unit_id.clone(),
            requirement_id: self.requirement_id.clone(),
            objective: self.objective.clone(),
            scope: self.scope.clone(),
            dependencies: self.dependencies.clone(),
            acceptance: self.acceptance.clone(),
            verification: self.verification.clone(),
            base_revision: self.base_revision.clone(),
            node: self.node.clone(),
        }
    }

    /// Ownership waits and dependencies have one domain definition for tools and UI.
    pub fn readiness(&self, all: &[WorkProjection]) -> WorkNodeReadiness {
        let mut result = WorkNodeReadiness {
            step: WorkNodeStep::Execute,
            blockers: Vec::new(),
            uncovered: Vec::new(),
        };
        if self.status.is_terminal() {
            result.step = WorkNodeStep::Finished;
            return result;
        }
        let children = all
            .iter()
            .filter(|n| n.node.parent_id.as_deref() == Some(&self.unit_id))
            .collect::<Vec<_>>();
        for child in &children {
            if !child.status.is_terminal() {
                result
                    .blockers
                    .push(format!("子节点未结束: {}", child.unit_id));
            }
        }
        if !children.is_empty() {
            result.uncovered = self
                .acceptance
                .iter()
                .filter(|criterion| {
                    !children.iter().any(|child| {
                        child.status == WorkUnitStatus::Done
                            && child.node.covers.contains(criterion)
                    })
                })
                .cloned()
                .collect();
        }
        let mut current = Some(self);
        let mut visited = BTreeSet::new();
        while let Some(node) = current {
            if !visited.insert(&node.unit_id) {
                result.blockers.push("节点归属存在循环".into());
                break;
            }
            if node.status == WorkUnitStatus::Blocked {
                result.blockers.push(format!(
                    "{} 已阻塞: {}",
                    node.unit_id,
                    node.blocked_reason.as_deref().unwrap_or("未提供原因")
                ));
            }
            if node.unit_id != self.unit_id && node.status.is_terminal() {
                result
                    .blockers
                    .push(format!("祖先 {} 已结束", node.unit_id));
            }
            for dependency in &node.dependencies {
                if !all
                    .iter()
                    .any(|d| d.unit_id == *dependency && d.status == WorkUnitStatus::Done)
                {
                    result
                        .blockers
                        .push(format!("{} 的依赖未完成: {dependency}", node.unit_id));
                }
            }
            current = match &node.node.parent_id {
                Some(parent) => match all.iter().find(|n| n.unit_id == *parent) {
                    Some(parent) => Some(parent),
                    None => {
                        result.blockers.push(format!("父节点不存在: {parent}"));
                        None
                    }
                },
                None => None,
            };
        }
        result.step = if !result.blockers.is_empty() {
            WorkNodeStep::Wait
        } else if self.acceptance.is_empty()
            || !self.node.questions.is_empty()
            || !result.uncovered.is_empty()
        {
            WorkNodeStep::Plan
        } else if children.is_empty() {
            WorkNodeStep::Execute
        } else {
            WorkNodeStep::Integrate
        };
        result
    }
}

pub(super) fn read_graph(tx: &Transaction<'_>) -> Result<Vec<WorkProjection>, StoreError> {
    let mut statement = tx.prepare("SELECT projection_json FROM work_surfaces ORDER BY unit_id")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|json| serde_json::from_str(&json).map_err(StoreError::from))
        .collect()
}

pub(super) fn validate_parent(
    node: &WorkProjection,
    all: &[WorkProjection],
) -> Result<(), StoreError> {
    if let Some(kind) = &node.node.custom_type {
        if all
            .iter()
            .filter(|other| other.unit_id != node.unit_id)
            .any(|other| {
                other.node.custom_type.as_ref().is_some_and(|existing| {
                    existing.name.to_lowercase() == kind.name.to_lowercase()
                        && (existing != kind || other.node.kind != node.node.kind)
                })
            })
        {
            return Err(invalid(
                "项目中已有同名扩展类型，请复用其基础类型、定义和产物",
            ));
        }
    }
    if let Some(id) = &node.node.parent_id {
        let parent = all
            .iter()
            .find(|p| p.unit_id == *id)
            .ok_or_else(|| invalid(format!("父节点不存在: {id}")))?;
        if parent.requirement_id != node.requirement_id {
            return Err(invalid("父子节点必须归属同一 Outcome；跨 Outcome 使用依赖"));
        }
        if node
            .node
            .covers
            .iter()
            .any(|criterion| !parent.acceptance.contains(criterion))
        {
            return Err(invalid("covers 必须引用父节点当前的验收条款"));
        }
    }
    Ok(())
}

fn validate_graph(all: &[WorkProjection]) -> Result<(), StoreError> {
    let mut edges: BTreeMap<&str, Vec<&str>> = all
        .iter()
        .map(|n| {
            (
                n.unit_id.as_str(),
                n.dependencies.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    for node in all {
        if let Some(parent) = &node.node.parent_id {
            edges
                .get_mut(parent.as_str())
                .ok_or_else(|| invalid(format!("父节点不存在: {parent}")))?
                .push(&node.unit_id);
        }
        let mut parent_id = node.node.parent_id.as_deref();
        let mut visited = BTreeSet::new();
        while let Some(id) = parent_id {
            if !visited.insert(id) {
                return Err(invalid(format!("节点归属存在循环: {id}")));
            }
            let parent = all
                .iter()
                .find(|p| p.unit_id == id)
                .ok_or_else(|| invalid(format!("父节点不存在: {id}")))?;
            edges
                .get_mut(node.unit_id.as_str())
                .expect("node is indexed")
                .extend(parent.dependencies.iter().map(String::as_str));
            parent_id = parent.node.parent_id.as_deref();
        }
    }
    fn visit<'a>(
        id: &'a str,
        edges: &BTreeMap<&'a str, Vec<&'a str>>,
        active: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<(), StoreError> {
        if done.contains(id) {
            return Ok(());
        }
        if !active.insert(id) {
            return Err(invalid(format!("节点依赖与父级收口构成循环: {id}")));
        }
        for dependency in edges
            .get(id)
            .ok_or_else(|| invalid(format!("依赖节点不存在: {id}")))?
        {
            visit(dependency, edges, active, done)?;
        }
        active.remove(id);
        done.insert(id);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for id in edges.keys() {
        visit(id, &edges, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

pub(super) fn validate_action(
    node: &WorkProjection,
    all: &[WorkProjection],
    fact: &WorkFact,
) -> Result<(), StoreError> {
    let readiness = node.readiness(all);
    if matches!(
        fact,
        WorkFact::Claimed { .. }
            | WorkFact::VerificationStarted
            | WorkFact::VerificationQueued { .. }
            | WorkFact::Completed
    ) && node.acceptance.is_empty()
    {
        return Err(invalid("先用 refine 明确完成条件，再执行或验收节点"));
    }
    if matches!(
        fact,
        WorkFact::Claimed { .. }
            | WorkFact::VerificationStarted
            | WorkFact::VerificationQueued { .. }
            | WorkFact::Completed
    ) && !readiness.blockers.is_empty()
    {
        return Err(invalid(readiness.blockers.join("；")));
    }
    if matches!(
        fact,
        WorkFact::VerificationStarted | WorkFact::VerificationQueued { .. } | WorkFact::Completed
    ) && readiness.step == WorkNodeStep::Plan
    {
        return Err(invalid(format!(
            "节点仍需规划：未决问题 {:?}，未覆盖父级验收 {:?}",
            node.node.questions, readiness.uncovered
        )));
    }
    if matches!(fact, WorkFact::Superseded { .. })
        && all
            .iter()
            .any(|n| n.node.parent_id.as_deref() == Some(&node.unit_id) && !n.status.is_terminal())
    {
        return Err(invalid("父节点仍有未结束子节点，不能整体废弃"));
    }
    Ok(())
}

fn observed<'a>(
    all: &'a [WorkProjection],
    id: &str,
    expected: i64,
) -> Result<&'a WorkProjection, StoreError> {
    let node = all
        .iter()
        .find(|node| node.unit_id == id)
        .ok_or_else(|| invalid(format!("未知节点: {id}")))?;
    if node.source_sequence != expected {
        return Err(invalid(format!(
            "节点版本已变化: expected={expected}, actual={}；重读后修订",
            node.source_sequence
        )));
    }
    Ok(node)
}

impl SessionStore {
    /// All children and the parent's released claim commit together or not at all.
    pub fn decompose_work_unit(
        &self,
        id: &str,
        expected_sequence: i64,
        reason: &str,
        children: Vec<WorkUnitSpec>,
    ) -> Result<Vec<WorkProjection>, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let mut graph = read_graph(&tx)?;
        let parent = observed(&graph, id, expected_sequence)?.clone();
        if parent.acceptance.is_empty() {
            return Err(invalid(
                "先用 refine 明确父目标的完成条件，再拆分覆盖这些条件的子工作",
            ));
        }
        let blockers = parent.planning_blockers(&graph);
        if !blockers.is_empty() {
            return Err(invalid(blockers.join("；")));
        }
        if children.is_empty() || children.len() > MAX_WORK_LIST_ITEMS {
            return Err(invalid("拆分必须包含 1 到节点预算上限数量的子节点"));
        }
        let mut created = Vec::new();
        for spec in children {
            validate_spec(&spec)?;
            if spec.node.parent_id.as_deref() != Some(id)
                || spec.requirement_id != parent.requirement_id
            {
                return Err(invalid("拆分子节点的 parent_id 和 Outcome 必须匹配父节点"));
            }
            if graph.iter().any(|n| n.unit_id == spec.unit_id) {
                return Err(invalid(format!("节点已存在: {}", spec.unit_id)));
            }
            let event = append_event_tx(
                &tx,
                &spec.unit_id,
                &spec.requirement_id,
                &WorkFact::Created { spec: spec.clone() },
            )?;
            let child = apply_fact(None, &event)?;
            validate_parent(&child, &graph)?;
            upsert_surface_tx(&tx, &child)?;
            graph.push(child.clone());
            created.push(child);
        }
        validate_graph(&graph)?;
        let fact = WorkFact::Decomposed {
            children: created.iter().map(|n| n.unit_id.clone()).collect(),
            reason: reason.into(),
        };
        let event = append_event_tx(&tx, id, &parent.requirement_id, &fact)?;
        let parent = apply_fact(Some(parent), &event)?;
        upsert_surface_tx(&tx, &parent)?;
        tx.commit()?;
        Ok(created)
    }

    pub fn refine_work_unit(
        &self,
        spec: WorkUnitSpec,
        expected_sequence: i64,
        reason: &str,
    ) -> Result<WorkProjection, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let mut graph = read_graph(&tx)?;
        let current = observed(&graph, &spec.unit_id, expected_sequence)?.clone();
        let blockers = current.planning_blockers(&graph);
        if !blockers.is_empty() {
            return Err(invalid(blockers.join("；")));
        }
        if graph
            .iter()
            .any(|n| n.node.parent_id.as_deref() == Some(&spec.unit_id) && !n.status.is_terminal())
        {
            return Err(invalid(
                "子工作未结束，不能改变其父契约；先完成或显式处置子工作",
            ));
        }
        let event = append_event_tx(
            &tx,
            &spec.unit_id,
            &spec.requirement_id,
            &WorkFact::Refined {
                spec: spec.clone(),
                reason: reason.into(),
            },
        )?;
        let updated = apply_fact(Some(current), &event)?;
        validate_parent(&updated, &graph)?;
        *graph
            .iter_mut()
            .find(|n| n.unit_id == updated.unit_id)
            .expect("observed node exists") = updated.clone();
        validate_graph(&graph)?;
        upsert_surface_tx(&tx, &updated)?;
        tx.commit()?;
        Ok(updated)
    }
}

#[cfg(test)]
mod tests;
