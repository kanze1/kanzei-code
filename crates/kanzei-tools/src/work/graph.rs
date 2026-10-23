//! Work graph context and durable progress projection.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkContextNode {
    pub id: String,
    pub kind: kanzei_core::WorkNodeKind,
    pub objective: String,
    pub scope: Vec<String>,
    pub acceptance: Vec<String>,
    pub explanation: String,
    pub source_refs: Vec<String>,
    pub evidence: Vec<kanzei_core::WorkEvidence>,
    pub status: WorkUnitStatus,
}

fn context_node(node: &WorkProjection) -> WorkContextNode {
    WorkContextNode {
        id: node.unit_id.clone(),
        kind: node.node.kind,
        objective: node.objective.clone(),
        scope: node.scope.clone(),
        acceptance: node.acceptance.clone(),
        explanation: node.node.explanation.clone(),
        source_refs: node.node.source_refs.clone(),
        evidence: node.evidence.clone(),
        status: node.status,
    }
}

pub(super) fn work_required_results(
    node: &WorkProjection,
    all: &[WorkProjection],
) -> Vec<WorkContextNode> {
    let mut dependencies = BTreeSet::new();
    let ancestors = work_node_ancestors(node, all);
    for contract in std::iter::once(node).chain(
        all.iter()
            .filter(|unit| ancestors.iter().any(|ancestor| ancestor.id == unit.unit_id)),
    ) {
        dependencies.extend(contract.dependencies.iter());
    }
    all.iter()
        .filter(|unit| dependencies.contains(&unit.unit_id))
        .map(context_node)
        .collect()
}

pub(super) fn work_node_summary(node: &WorkProjection) -> WorkItemSummary {
    WorkItemSummary {
        id: node.unit_id.clone(),
        kind: "work_unit".into(),
        title: node.objective.clone(),
        lifecycle_status: node.status.as_str().into(),
        block_reasons: Vec::new(),
    }
}

pub(super) fn work_node_ancestors(
    node: &WorkProjection,
    all: &[WorkProjection],
) -> Vec<WorkContextNode> {
    let mut ancestors = Vec::new();
    let mut parent_id = node.node.parent_id.as_deref();
    let mut visited = BTreeSet::new();
    while let Some(id) = parent_id {
        if !visited.insert(id) {
            break;
        }
        let Some(parent) = all.iter().find(|node| node.unit_id == id) else {
            break;
        };
        ancestors.push(context_node(parent));
        parent_id = parent.node.parent_id.as_deref();
    }
    ancestors
}

/// Durable results for automatic progress detection. Ownership, checkpoint prose,
/// timestamps and verification attempts do not manufacture new progress.
pub fn progress_snapshot(project_root: &std::path::Path) -> Result<String, String> {
    let path = kanzei_core::project_state_path(project_root);
    if !path.is_file() {
        return Ok("[]".into());
    }
    let mut units = SessionStore::open(&path)
        .map_err(|error| error.to_string())?
        .list_work_units(None)
        .map_err(|error| error.to_string())?;
    units.sort_by(|a, b| a.unit_id.cmp(&b.unit_id));
    serde_json::to_string(
        &units
            .iter()
            .map(|unit| {
                (
                    unit.spec(),
                    &unit.evidence,
                    unit.status.is_terminal().then_some(unit.status),
                )
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|error| error.to_string())
}
