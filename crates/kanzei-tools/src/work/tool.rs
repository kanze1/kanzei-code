//! `work` 工具的输入契约与写操作。
//!
//! 调度投影保留在父模块；本模块集中处理工具 schema、Work Unit 事件写入与旧队列
//! claim，避免控制面解析与命令副作用继续堆叠在同一个巨石文件。

use super::*;
use async_trait::async_trait;
use input::WorkInput;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use serde::Serialize;

mod input;

mod revision;

const UNIT_ACTIONS: &[&str] = &[
    "impact",
    "begin_revision",
    "apply_revision",
    "revision_yield",
    "graph",
    "decompose",
    "refine",
    "create_unit",
    "get_unit",
    "list_units",
    "checkpoint",
    "block",
    "unblock",
    "verify",
    "evidence",
    "complete",
    "supersede",
];

fn required<'a>(value: &'a Option<String>, name: &str) -> Result<&'a str, String> {
    value
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("`{name}` is required"))
}

fn pretty(value: impl Serialize) -> ToolOutput {
    match serde_json::to_string_pretty(&value) {
        Ok(content) => ToolOutput::ok(content),
        Err(error) => ToolOutput::error(format!("cannot serialize work unit output: {error}")),
    }
}

fn open_work_store(project_root: &std::path::Path) -> Result<SessionStore, String> {
    SessionStore::open(&kanzei_core::project_state_path(project_root))
        .map_err(|error| format!("cannot open work unit store: {error}"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlanActor {
    Agent,
    User,
}

fn outcome_plan_blocker(outcome: Option<&Entry>) -> Option<&'static str> {
    (!outcome.is_some_and(|entry| {
        uses_work_units(entry) && matches!(entry.status.as_str(), "draft" | "todo" | "doing")
    }))
    .then_some("父需求已暂停、结束或不在活动队列")
}

fn plan_owner_blocker(
    unit: &kanzei_core::WorkProjection,
    actor: PlanActor,
    owner: Option<String>,
) -> Option<&'static str> {
    (matches!(
        unit.status,
        WorkUnitStatus::Active | WorkUnitStatus::Verifying
    ) && (actor == PlanActor::User || unit.claimed_by != owner))
        .then_some("节点已有执行者，请先由执行者结束当前工作或释放节点，再修改计划")
}

fn execute_work_unit_action(
    input: &WorkInput,
    ctx: &ToolCtx,
    actor: PlanActor,
) -> Option<ToolOutput> {
    let is_unit_claim =
        input.action == "claim" && input.id.as_deref().is_some_and(|id| id.contains("/W"));
    if !is_unit_claim && !UNIT_ACTIONS.contains(&input.action.as_str()) {
        return None;
    }
    if !matches!(
        input.action.as_str(),
        "graph" | "list_units" | "create_unit"
    ) && input.id.as_deref().is_some_and(|id| !id.contains("/W"))
    {
        let parent = input.id.as_deref().unwrap();
        return Some(ToolOutput::needs_correction("WORK_UNIT_ID_REQUIRED",
            format!("此动作需要 Work Unit 的 unit_id（例如 {parent}/W1），不能使用父需求 {parent}。先 work list_units(requirement_id={parent})，使用返回的 unit_id；req update 只记录需求进展，不代表 Work Unit 完成。")));
    }

    if matches!(input.action.as_str(), "list_units" | "graph") {
        let store = match open_work_store(&ctx.project_root) {
            Ok(store) => store,
            Err(error) => return Some(ToolOutput::error(error)),
        };
        return Some(
            match store.list_work_units(input.requirement_id.as_deref()) {
                Ok(units) if input.action == "graph" => {
                    let all = match store.list_work_units(None) {
                        Ok(all) => all,
                        Err(error) => return Some(ToolOutput::error(error.to_string())),
                    };
                    pretty(
                        json!({"nodes": units.iter().map(|unit| json!({"unit": unit, "readiness": unit.readiness(&all), "planning_blockers": unit.planning_blockers(&all)})).collect::<Vec<_>>()}),
                    )
                }
                Ok(units) => pretty(units),
                Err(error) => ToolOutput::error(format!("cannot list work units: {error}")),
            },
        );
    }
    if matches!(input.action.as_str(), "get_unit" | "impact") {
        let id = match required(&input.id, "id") {
            Ok(id) => id,
            Err(error) => return Some(ToolOutput::error(error)),
        };
        let store = match open_work_store(&ctx.project_root) {
            Ok(store) => store,
            Err(error) => return Some(ToolOutput::error(error)),
        };
        if input.action == "impact" {
            return Some(match store.work_revision_impact(id) {
                Ok(impact) => pretty(impact),
                Err(error) => ToolOutput::error(error.to_string()),
            });
        }
        return Some(
            match (store.get_work_unit(id), store.list_work_events(id)) {
                (Ok(Some(unit)), Ok(events)) => {
                    pretty(json!({"contract": unit.spec(), "unit": unit, "events": events}))
                }
                (Ok(None), _) => ToolOutput::error(format!("unknown work unit `{id}`")),
                (Err(error), _) | (_, Err(error)) => {
                    ToolOutput::error(format!("cannot read work unit: {error}"))
                }
            },
        );
    }

    let work_lock_path = ctx.project_root.join(".kanzei/project/work-selection");
    let _work_lock = match crate::atomic_file::lock_exclusive(&work_lock_path) {
        Ok(lock) => lock,
        Err(error) => {
            return Some(ToolOutput::error(format!(
                "cannot lock work selection: {error}"
            )))
        }
    };
    let store = match open_work_store(&ctx.project_root) {
        Ok(store) => store,
        Err(error) => return Some(ToolOutput::error(error)),
    };

    if input.action == "create_unit" {
        let requirement_id = match required(&input.requirement_id, "requirement_id") {
            Ok(id) => id,
            Err(error) => return Some(ToolOutput::error(error)),
        };
        let objective = match required(&input.objective, "objective") {
            Ok(value) => value,
            Err(error) => return Some(ToolOutput::error(error)),
        };
        let req_store = DocStore::open(&ctx.project_root, &REQUIREMENTS);
        let requirements = match req_store.load() {
            Ok(entries) => entries,
            Err(error) => {
                return Some(ToolOutput::error(format!(
                    "cannot read requirements: {error}"
                )))
            }
        };
        let Some(outcome) = requirements.iter().find(|entry| entry.id == requirement_id) else {
            return Some(ToolOutput::error(format!(
                "unknown active requirement `{requirement_id}`"
            )));
        };
        if !uses_work_units(outcome) {
            return Some(ToolOutput::error(format!(
                "{requirement_id} 未设置 `执行模型: work_units_v1`，不能创建 Work Unit"
            )));
        }
        if let Some(reason) = outcome_plan_blocker(Some(outcome)) {
            return Some(ToolOutput::needs_correction(
                "WORK_OUTCOME_NOT_PLANNABLE",
                reason,
            ));
        }
        let existing = match store.list_work_units(Some(requirement_id)) {
            Ok(units) => units,
            Err(error) => {
                return Some(ToolOutput::error(format!(
                    "cannot inspect work units: {error}"
                )))
            }
        };
        let next = existing
            .iter()
            .filter_map(|unit| unit.unit_id.rsplit_once("/W")?.1.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let observation = repo_observation(&ctx.cwd);
        let spec = kanzei_core::WorkUnitSpec {
            node: input.node.clone(),
            unit_id: format!("{requirement_id}/W{next}"),
            requirement_id: requirement_id.into(),
            objective: objective.trim().into(),
            scope: input.scope.clone(),
            dependencies: input.dependencies.clone(),
            acceptance: input.acceptance.clone(),
            verification: input.verification.clone(),
            base_revision: input
                .base_revision
                .clone()
                .unwrap_or(observation.observed_head),
        };
        return Some(match store.create_work_unit(spec) {
            Ok(unit) => pretty(unit),
            Err(error) => ToolOutput::error(format!("cannot create work unit: {error}")),
        });
    }

    let id = match required(&input.id, "id") {
        Ok(id) => id,
        Err(error) => return Some(ToolOutput::error(error)),
    };
    let current = match store.get_work_unit(id) {
        Ok(Some(unit)) => unit,
        Ok(None) => return Some(ToolOutput::error(format!("unknown work unit `{id}`"))),
        Err(error) => return Some(ToolOutput::error(format!("cannot read work unit: {error}"))),
    };

    if matches!(
        input.action.as_str(),
        "begin_revision" | "apply_revision" | "revision_yield"
    ) {
        return Some(revision::execute(input, ctx, actor, &store, &current));
    }

    if matches!(input.action.as_str(), "decompose" | "refine") {
        let req_store = DocStore::open(&ctx.project_root, &REQUIREMENTS);
        let _doc_lock = match req_store.lock() {
            Ok(lock) => lock,
            Err(error) => return Some(ToolOutput::error(error.to_string())),
        };
        let requirements = match req_store.load() {
            Ok(entries) => entries,
            Err(error) => return Some(ToolOutput::error(error.to_string())),
        };
        if let Some(reason) = outcome_plan_blocker(
            requirements
                .iter()
                .find(|entry| entry.id == current.requirement_id),
        ) {
            return Some(ToolOutput::needs_correction(
                "WORK_OUTCOME_NOT_PLANNABLE",
                reason,
            ));
        }
        if let Some(reason) =
            plan_owner_blocker(&current, actor, line_identity(&ctx.cwd, &ctx.project_root))
        {
            return Some(ToolOutput::needs_correction("WORK_OWNER_MISMATCH", reason));
        }
        return Some(execute_plan_change(input, &store, &current));
    }

    let references = if input.action == "evidence" {
        input.evidence_refs.iter().collect::<Vec<_>>()
    } else if input.action == "complete" {
        current
            .evidence
            .iter()
            .flat_map(|evidence| &evidence.evidence_refs)
            .collect()
    } else {
        Vec::new()
    };
    for reference in references {
        if let Err(error) = super::evidence::validate_reference(&ctx.project_root, id, reference) {
            return Some(ToolOutput::needs_correction(
                "WORK_EVIDENCE_UNAVAILABLE",
                error,
            ));
        }
    }

    if is_unit_claim {
        if current.review_pending() {
            return Some(ToolOutput::needs_correction("WORK_REVIEW_PENDING", "方向变化待复核；执行者先停止旧工作并 revision_yield，规划者使用 apply_revision，不要 claim"));
        }
        if current.status == WorkUnitStatus::Ready {
            if let Err(error) = super::execution::check_revision(ctx, &current.base_revision) {
                return Some(ToolOutput::needs_correction(
                    "EXECUTION_REVISION_MISMATCH",
                    error,
                ));
            }
        }
        let me = line_identity(&ctx.cwd, &ctx.project_root);
        let state = match resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority) {
            Ok(state) => state,
            Err(error) => return Some(ToolOutput::error(error)),
        };
        match state.decision {
            WorkDecision::WipViolation => {
                return Some(ToolOutput::error(format!(
                    "{}；先收口现有 WIP，不能 claim {id}",
                    state.reason
                )))
            }
            WorkDecision::Resume
                if state
                    .selected
                    .as_ref()
                    .is_some_and(|selected| selected.id != id) =>
            {
                return Some(ToolOutput::error(format!(
                    "已有可执行 WIP {}，必须 Resume；不能 claim 第二个 Work Unit",
                    state.selected.as_ref().unwrap().id
                )))
            }
            WorkDecision::Resume | WorkDecision::Start => {}
            WorkDecision::Blocked | WorkDecision::Empty => {
                let foreign_takeover = state.foreign_wip.iter().any(|unit| unit.id == id)
                    && input
                        .reason
                        .as_deref()
                        .is_some_and(|reason| !reason.trim().is_empty());
                if !foreign_takeover {
                    return Some(ToolOutput::error(format!(
                        "当前裁决是 {:?}: {}",
                        state.decision, state.reason
                    )));
                }
            }
        }
        if state
            .blocked_items
            .iter()
            .any(|candidate| candidate.id == id)
        {
            return Some(ToolOutput::error(format!("{id} 当前被阻塞，不能 claim")));
        }
        let is_default = state.selected.as_ref().is_some_and(|item| item.id == id);
        let takeover = matches!(
            current.status,
            WorkUnitStatus::Active | WorkUnitStatus::Verifying
        ) && current.claimed_by.as_deref() != me.as_deref();
        if takeover
            && input
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            return Some(ToolOutput::error(
                "接管其他线的 Work Unit 必须提供非空 reason",
            ));
        }
        if !is_default
            && !takeover
            && input
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            return Some(ToolOutput::error(
                "claim 偏离引擎默认选择时必须提供非空 reason",
            ));
        }
        let resuming_current = matches!(
            current.status,
            WorkUnitStatus::Active | WorkUnitStatus::Verifying
        ) && !takeover;

        // 父 Requirement 的 doing 只表达 Outcome 已启动，不再承载 WIP/取得线/进展。
        // 文件先写、事件后写；事件失败时在同一文档锁内恢复原快照。
        let req_store = DocStore::open(&ctx.project_root, &REQUIREMENTS);
        let _req_lock = match req_store.lock() {
            Ok(lock) => lock,
            Err(error) => {
                return Some(ToolOutput::error(format!(
                    "cannot lock requirements: {error}"
                )))
            }
        };
        let mut requirements = match req_store.load() {
            Ok(entries) => entries,
            Err(error) => {
                return Some(ToolOutput::error(format!(
                    "cannot read requirements: {error}"
                )))
            }
        };
        let before = requirements.clone();
        let Some(outcome) = requirements
            .iter_mut()
            .find(|entry| entry.id == current.requirement_id)
        else {
            return Some(ToolOutput::error(format!(
                "Work Unit 的父需求 {} 不在活动队列",
                current.requirement_id
            )));
        };
        if !uses_work_units(outcome) {
            return Some(ToolOutput::error("父需求未启用 work_units_v1"));
        }
        let execution = match super::execution::preflight(ctx, outcome) {
            Ok(execution) => execution,
            Err(error) => {
                return Some(ToolOutput::needs_correction(
                    "EXECUTION_INPUTS_NOT_READY",
                    error,
                ))
            }
        };
        if let Err(error) = crate::work::log::append(
            &ctx.project_root,
            "execution_bound",
            Some(id),
            ctx,
            execution,
        ) {
            tracing::warn!(%error, "execution binding observation could not be recorded");
        }
        if let Err(error) = crate::prior_art::check_start(&ctx.project_root, outcome) {
            return Some(ToolOutput::needs_correction("PRIOR_ART_REQUIRED", error));
        }
        if resuming_current {
            return Some(pretty(current));
        }
        // R-313:work_units_v1 的父 Outcome 也不能绕过进入 doing 前的发现/确认门禁。
        if outcome.status != "doing" {
            if let Err(error) = crate::tracker::TrackerTool::check_requirement_start(outcome) {
                return Some(ToolOutput::error(format!("{} {error}", outcome.id)));
            }
            if let Err(error) = req_store.transition_allowed(&outcome.status, "doing") {
                return Some(ToolOutput::error(error));
            }
            outcome.status = "doing".into();
            outcome.sync_status_fields();
            if let Err(error) = req_store.save(&requirements) {
                return Some(ToolOutput::error(format!(
                    "cannot activate outcome: {error}"
                )));
            }
        }
        let fact = if takeover {
            kanzei_core::WorkFact::Reassigned {
                claimed_by: me,
                reason: input.reason.clone().unwrap_or_default(),
            }
        } else {
            kanzei_core::WorkFact::Claimed { claimed_by: me }
        };
        return Some(match store.append_work_fact(id, fact) {
            Ok(unit) => pretty(unit),
            Err(error) => {
                let rollback = req_store.save(&before);
                ToolOutput::error(format!(
                    "cannot claim work unit: {error}; outcome rollback: {}",
                    rollback
                        .map(|_| "ok".to_string())
                        .unwrap_or_else(|rollback_error| rollback_error.to_string())
                ))
            }
        });
    }

    let fact = match input.action.as_str() {
        "checkpoint" => {
            let summary = match required(&input.summary, "summary") {
                Ok(value) => value,
                Err(error) => return Some(ToolOutput::error(error)),
            };
            let next_action = match required(&input.next_action, "next_action") {
                Ok(value) => value,
                Err(error) => return Some(ToolOutput::error(error)),
            };
            let observation = repo_observation(&ctx.cwd);
            kanzei_core::WorkFact::Checkpointed {
                checkpoint: kanzei_core::WorkCheckpoint {
                    summary: summary.into(),
                    next_action: next_action.into(),
                    decisions: input.decisions.clone(),
                    retrieval_refs: input.retrieval_refs.clone(),
                    observed_head: input
                        .observed_head
                        .clone()
                        .unwrap_or(observation.observed_head),
                    observed_worktree_hash: input
                        .observed_worktree_hash
                        .clone()
                        .unwrap_or(observation.observed_worktree_hash),
                },
            }
        }
        "block" => kanzei_core::WorkFact::Blocked {
            reason: match required(&input.reason, "reason") {
                Ok(value) => value.into(),
                Err(error) => return Some(ToolOutput::error(error)),
            },
        },
        "unblock" => kanzei_core::WorkFact::Unblocked {
            reason: match required(&input.reason, "reason") {
                Ok(value) => value.into(),
                Err(error) => return Some(ToolOutput::error(error)),
            },
        },
        "verify" => kanzei_core::WorkFact::VerificationStarted,
        "evidence" => kanzei_core::WorkFact::EvidenceAdded {
            evidence: kanzei_core::WorkEvidence {
                criterion: match required(&input.criterion, "criterion") {
                    Ok(value) => value.into(),
                    Err(error) => return Some(ToolOutput::error(error)),
                },
                evidence_refs: input.evidence_refs.clone(),
            },
        },
        "complete" => kanzei_core::WorkFact::Completed,
        "supersede" => kanzei_core::WorkFact::Superseded {
            reason: match required(&input.reason, "reason") {
                Ok(value) => value.into(),
                Err(error) => return Some(ToolOutput::error(error)),
            },
        },
        _ => return Some(ToolOutput::error("unknown work unit action")),
    };
    Some(match store.append_work_fact(id, fact) {
        Ok(unit) => pretty(unit),
        Err(error) => ToolOutput::error(format!("cannot append work fact: {error}")),
    })
}

fn execute_plan_change(
    input: &WorkInput,
    store: &SessionStore,
    current: &kanzei_core::WorkProjection,
) -> ToolOutput {
    let Some(expected) = input.expected_sequence else {
        return ToolOutput::needs_correction(
            "WORK_VERSION_REQUIRED",
            "重读 work get_unit，并提供 expected_sequence；不能覆盖未知版本的计划",
        );
    };
    let reason = match required(&input.reason, "reason") {
        Ok(reason) => reason,
        Err(error) => return ToolOutput::error(error),
    };
    if input.action == "refine" {
        let Some(spec) = input.refinement.clone() else {
            return ToolOutput::error("refine requires the complete refinement contract");
        };
        if spec.unit_id != current.unit_id {
            return ToolOutput::error("refinement.unit_id must match id");
        }
        return match store.refine_work_unit(spec, expected, reason) {
            Ok(unit) => pretty(unit),
            Err(error) => ToolOutput::needs_correction("WORK_PLAN_REJECTED", error.to_string()),
        };
    }
    let existing = match store.list_work_units(Some(&current.requirement_id)) {
        Ok(units) => units,
        Err(error) => return ToolOutput::error(error.to_string()),
    };
    let mut sequence = existing
        .iter()
        .filter_map(|unit| unit.unit_id.rsplit_once("/W")?.1.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let mut keys = BTreeMap::new();
    for child in &input.children {
        if child.key.trim().is_empty() || keys.contains_key(&child.key) {
            return ToolOutput::error("decompose children need unique nonblank keys");
        }
        let Some(next) = sequence.checked_add(1) else {
            return ToolOutput::error("work unit ID range exhausted");
        };
        sequence = next;
        keys.insert(
            child.key.clone(),
            format!("{}/W{sequence}", current.requirement_id),
        );
    }
    let mut specs = Vec::new();
    for child in &input.children {
        let mut node = child.node.clone();
        if node
            .parent_id
            .as_ref()
            .is_some_and(|id| *id != current.unit_id)
        {
            return ToolOutput::error("child parent_id must match the decomposed node");
        }
        node.parent_id = Some(current.unit_id.clone());
        let mut dependencies = Vec::new();
        for dependency in &child.dependencies {
            dependencies.push(if let Some(key) = dependency.strip_prefix('@') {
                match keys.get(key) {
                    Some(id) => id.clone(),
                    None => {
                        return ToolOutput::error(format!("unknown child dependency key: {key}"))
                    }
                }
            } else {
                dependency.clone()
            });
        }
        specs.push(kanzei_core::WorkUnitSpec {
            unit_id: keys[&child.key].clone(),
            requirement_id: current.requirement_id.clone(),
            objective: child.objective.clone(),
            scope: child.scope.clone(),
            dependencies,
            acceptance: child.acceptance.clone(),
            verification: child.verification.clone(),
            base_revision: current.base_revision.clone(),
            node,
        });
    }
    match store.decompose_work_unit(&current.unit_id, expected, reason, specs) {
        Ok(children) => pretty(json!({"children": children, "ids": keys})),
        Err(error) => ToolOutput::needs_correction("WORK_PLAN_REJECTED", error.to_string()),
    }
}

pub struct WorkTool;

impl WorkTool {
    /// UI hints and actual commits use the same domain/ownership/outcome rules.
    pub fn user_plan_blockers(
        unit: &kanzei_core::WorkProjection,
        all: &[kanzei_core::WorkProjection],
        outcome: Option<&Entry>,
    ) -> Vec<String> {
        let mut blockers = unit.planning_blockers(all);
        blockers.extend(outcome_plan_blocker(outcome).map(str::to_owned));
        blockers.extend(plan_owner_blocker(unit, PlanActor::User, None).map(str::to_owned));
        blockers
    }

    /// Explicit user planning shares the agent transaction and cannot claim work,
    /// forge ownership, broaden tool grants or complete a node.
    pub fn decompose_from_user(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: WorkInput = match crate::parse_input(self, input) {
            Ok(input) => input,
            Err(output) => return output,
        };
        if input.action != "decompose" {
            return ToolOutput::error("用户裂变入口只接受 decompose");
        }
        execute_work_unit_action(&input, ctx, PlanActor::User).expect("decompose is a unit action")
    }
}

#[async_trait]
impl Tool for WorkTool {
    fn name(&self) -> &'static str {
        "work"
    }

    fn description(&self) -> String {
        "Read queue decisions and maintain work units, plans, revisions, verification evidence and handoffs. \
         next returns the current decision; graph exposes readiness and planning gaps. Planning needs no execution claim. \
         Lifecycle actions use exact unit IDs and declared acceptance; parents require their own integration evidence. \
         Decomposition uses node.covers and sibling @key dependencies. Revisions use observed versions and owner yields before applying affected decisions. \
         Claiming another conversation's work requires a takeover reason. Priority comes from the run. \
         Verification jobs are submitted through bash.verification. Handoff records completed scope and evidence; it does not itself schedule more work.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(WorkInput)).unwrap();
        schema["properties"]["action"]["enum"] = json!([
            "impact",
            "begin_revision",
            "apply_revision",
            "revision_yield",
            "graph",
            "decompose",
            "refine",
            "next",
            "verification_jobs",
            "verification_cancel",
            "reconcile",
            "claim",
            "create_unit",
            "get_unit",
            "list_units",
            "checkpoint",
            "block",
            "unblock",
            "verify",
            "evidence",
            "complete",
            "supersede",
            "handoff"
        ]);
        schema
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        let action = input
            .get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        // handoff 是控制面声明，写入过程记录，不修改业务条目。
        let read_only = matches!(
            action,
            "next"
                | "reconcile"
                | "get_unit"
                | "list_units"
                | "graph"
                | "impact"
                | "handoff"
                | "verification_jobs"
        );
        vec![format!(
            "{}:{action}",
            if read_only { "read" } else { "write" }
        )]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let mut input: WorkInput = match crate::parse_input(self, input) {
            Ok(input) => input,
            Err(output) => return output,
        };
        if input.action == "verification_jobs" {
            return match crate::verification::recover(&ctx.project_root)
                .and_then(|_| crate::verification::list_jobs(&ctx.project_root))
            {
                Ok(jobs) => pretty(jobs),
                Err(error) => ToolOutput::error(error.to_string()),
            };
        }
        if input.action == "verification_cancel" {
            return match required(&input.id, "id")
                .map_err(anyhow::Error::msg)
                .and_then(|id| crate::verification::cancel(&ctx.project_root, id))
            {
                Ok(()) => ToolOutput::ok("verification cancellation requested"),
                Err(error) => ToolOutput::error(error.to_string()),
            };
        }
        // 完成声明写入 work-log;事件层保留范围、目标和证据,轮末再核对当前委托。
        // 只有 ToolEnd 成功才采纳,不扫可能已被压缩的消息历史。
        //
        // 为什么挂在 work 而不是新开一个工具:D-662 已把「托管专用工具膨胀」判成缺陷,
        // 而 work 本就是取活与工作编排的抽象面,循环控制归它语义自洽,也不扩大模型的
        // 工具选择面。
        if input.action == "handoff" {
            let summary = input
                .summary
                .as_deref()
                .or(input.reason.as_deref())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .unwrap_or("no summary given");
            let criterion = input
                .criterion
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty());
            if criterion.is_none() {
                return ToolOutput::needs_correction(
                    "HANDOFF_COMPLETION_CRITERION_REQUIRED",
                    "请提供 criterion，说明本次声明范围内已满足的完成条件；局部交付使用 work_item 或 batch。",
                );
            }
            if input.evidence_refs.is_empty()
                || input
                    .evidence_refs
                    .iter()
                    .any(|item| item.trim().is_empty())
            {
                return ToolOutput::needs_correction(
                    "HANDOFF_EVIDENCE_REQUIRED",
                    "handoff 只能在有可复核证据时成功；请提供至少一个 evidence_refs（file:line、测试记录或提交号）。",
                );
            }
            let criterion = criterion.unwrap();
            let (Some(scope), Some(target)) = (
                input.handoff_scope,
                input
                    .handoff_target
                    .as_deref()
                    .filter(|target| !target.trim().is_empty()),
            ) else {
                return ToolOutput::needs_correction(
                    "HANDOFF_SCOPE_REQUIRED",
                    "请提供 handoff_scope 和 handoff_target：work_item 对应工作项，batch 对应阶段，request/goal 对应本轮完成契约中的请求标识或目标原文。",
                );
            };
            let declaration = kanzei_harness::handoff::HandoffDeclaration {
                scope,
                target: target.into(),
                criterion: criterion.into(),
                evidence_refs: input.evidence_refs.clone(),
                summary: Some(summary.into()),
            };
            // 声明属于本轮过程事实,不是条目状态;保留原文供用户复核。
            if let Err(error) = crate::work::log::append(
                &ctx.project_root,
                "handoff",
                input.id.as_deref(),
                ctx,
                serde_json::to_value(&declaration).unwrap(),
            ) {
                return ToolOutput::failed("HANDOFF_RECORD_WRITE_FAILED", error.to_string());
            }
            return ToolOutput::ok(
                json!({
                    "status": "recorded", "handoff": declaration,
                    "completion": "pending_controller_check",
                    "notice": kanzei_harness::handoff::INTERNAL_RECORD_NOTICE,
                })
                .to_string(),
            );
        }
        if input.action == "claim" {
            if let Some(id) = input
                .id
                .as_deref()
                .filter(|id| id.starts_with("R-") && !id.contains("/W"))
            {
                let outcomes = match DocStore::open(&ctx.project_root, &REQUIREMENTS).load() {
                    Ok(outcomes) => outcomes,
                    Err(error) => return ToolOutput::error(error.to_string()),
                };
                if outcomes
                    .iter()
                    .any(|outcome| outcome.id == id && uses_work_units(outcome))
                {
                    let state = match resolve_work_for_outcome(
                        &ctx.cwd,
                        &ctx.project_root,
                        ctx.work_priority,
                        id,
                    ) {
                        Ok(state) => state,
                        Err(error) => return ToolOutput::error(error),
                    };
                    let Some(selected) = state.selected else {
                        return ToolOutput::blocked_by_workflow(
                            "WORK_OUTCOME_NOT_READY",
                            state.reason,
                        );
                    };
                    if selected.id == id {
                        return pretty(
                            json!({"claimed":id,"outcome_action":selected.outcome_action,"instruction":"规划使用 work create_unit；收口核对 Outcome 验收后 req close。执行占用属于具体节点。"}),
                        );
                    }
                    if selected
                        .work_unit_context
                        .as_ref()
                        .is_none_or(|context| context.unit.requirement_id != id)
                    {
                        return ToolOutput::blocked_by_workflow(
                            "WORK_CURRENT_TASK_ACTIVE",
                            format!("先完成或阻塞当前任务 {}，再领取目标 {id}", selected.id),
                        );
                    }
                    input.id = Some(selected.id);
                }
            }
        }
        if let Some(output) = execute_work_unit_action(&input, ctx, PlanActor::Agent) {
            return output;
        }
        if input.action == "reconcile" {
            return match resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority) {
                Ok(state) => ToolOutput::ok(
                    serde_json::to_string_pretty(&super::output::reconciliation_output(
                        &state.reconciliation,
                        &ctx.project_root,
                    ))
                    .unwrap(),
                ),
                Err(error) => ToolOutput::error(error),
            };
        }
        if input.action == "next" {
            let state = if input.detail {
                resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority)
            } else {
                resolve_work_selection(&ctx.cwd, &ctx.project_root, ctx.work_priority)
            };
            return match state {
                Ok(state) => ToolOutput::ok(
                    serde_json::to_string_pretty(&if input.detail {
                        serde_json::to_value(state).unwrap()
                    } else {
                        super::output::structured_control_output(state)
                    })
                    .unwrap(),
                ),
                Err(error) => ToolOutput::error(error),
            };
        }
        if input.action != "claim" {
            return ToolOutput::error("unknown action; see work tool schema");
        }
        let Some(id) = input.id.as_deref() else {
            return ToolOutput::error("`id` is required for claim");
        };

        let work_lock_path = ctx.project_root.join(".kanzei/project/work-selection");
        let _work_lock = match crate::atomic_file::lock_exclusive(&work_lock_path) {
            Ok(lock) => lock,
            Err(error) => return ToolOutput::error(format!("cannot lock work selection: {error}")),
        };
        let state = match resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority) {
            Ok(state) => state,
            Err(error) => return ToolOutput::error(error),
        };
        match state.decision {
            WorkDecision::WipViolation => {
                return ToolOutput::error(format!(
                    "{}；当前 WIP: {}",
                    state.reason,
                    state
                        .executable_wip
                        .iter()
                        .map(|item| item.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            WorkDecision::Resume if state.selected.as_ref().is_some_and(|item| item.id != id) => {
                return ToolOutput::error(format!(
                    "已有可执行 WIP {}，必须 Resume；claim override 不能再开第二个 WIP",
                    state.selected.as_ref().unwrap().id
                ));
            }
            WorkDecision::Blocked | WorkDecision::Empty => {
                // D-354:Empty/Blocked 里仍可能有他线持有的条目——带非空 reason 的
                // 接管(线停机/用户改派)要能走通,不能被"无可取条目"一票否决。
                let foreign_takeover = state.foreign_wip.iter().any(|item| item.id == id)
                    && input
                        .reason
                        .as_deref()
                        .is_some_and(|reason| !reason.trim().is_empty());
                if !foreign_takeover {
                    return ToolOutput::error(format!(
                        "当前裁决是 {:?}: {}",
                        state.decision, state.reason
                    ));
                }
            }
            WorkDecision::Resume | WorkDecision::Start => {}
        }
        // D-354:他线持有的条目不能顺手 claim——报错要指明「被谁持有」,而不是
        // 笼统的"偏离默认选择"。接管(线死了/用户改派)走 override 通道:带非空
        // reason,接管成功会改写「取得线」并把依据留在审计字段。
        if state.foreign_wip.iter().any(|item| item.id == id)
            && input
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            return ToolOutput::error(format!(
                "{id} 正被其他线持有(见 foreign_wip),不能重复 claim;\
                 确要接管须提供非空 reason(留取活覆盖审计,接管会改写取得线)"
            ));
        }
        let is_default = state.selected.as_ref().is_some_and(|item| item.id == id);
        if !is_default
            && input
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            return ToolOutput::error(
                "claim 偏离引擎默认选择时必须提供非空 reason，供取活覆盖审计",
            );
        }

        let (kind, wip_status) = if id.starts_with("R-") {
            (&REQUIREMENTS, "doing")
        } else if id.starts_with("D-") {
            (&DEFECTS, "fixing")
        } else {
            return ToolOutput::error("claim id 必须是 R-xxx 或 D-xxx");
        };
        let store = DocStore::open(&ctx.project_root, kind);
        let _doc_lock = match store.lock() {
            Ok(lock) => lock,
            Err(error) => return ToolOutput::error(format!("cannot lock tracker: {error}")),
        };
        let mut entries = match store.load() {
            Ok(entries) => entries,
            Err(error) => return ToolOutput::error(format!("cannot read tracker: {error}")),
        };
        let Some(position) = entries.iter().position(|entry| entry.id == id) else {
            return ToolOutput::error(format!("unknown id `{id}`"));
        };
        if kind.terminal.contains(&entries[position].status.as_str()) {
            return ToolOutput::error(format!("{id} 已是终态，不能 claim"));
        }
        let me = line_identity(&ctx.cwd, &ctx.project_root);
        if !is_default {
            let refreshed = resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority)
                .ok()
                .and_then(|control| control.blocked_items.into_iter().find(|item| item.id == id));
            if let Some(blocked) = refreshed {
                return ToolOutput::error(format!(
                    "{id} 当前被阻塞，不能覆盖 claim: {}",
                    blocked.block_reasons.join("；")
                ));
            }
        }
        let execution = match super::execution::preflight(ctx, &entries[position]) {
            Ok(execution) => execution,
            Err(error) => return ToolOutput::needs_correction("EXECUTION_INPUTS_NOT_READY", error),
        };
        if kind.prefix == "R" {
            if let Err(error) = crate::prior_art::check_start(&ctx.project_root, &entries[position])
            {
                return ToolOutput::needs_correction("PRIOR_ART_REQUIRED", error);
            }
        }
        // R-313:旧 work claim 是进入 doing/fixing 的真实消费者；需求 claim 不能绕过
        // Discovery Record、核心语义确认和限定词一致性门禁。
        if kind.prefix == "R"
            && entries[position].status != wip_status
            && entries[position].status != "doing"
        {
            if let Err(error) =
                crate::tracker::TrackerTool::check_requirement_start(&entries[position])
            {
                return ToolOutput::error(format!("{id} {error}"));
            }
        }
        if entries[position].status != wip_status {
            if let Err(error) = store.transition_allowed(&entries[position].status, wip_status) {
                return ToolOutput::error(error);
            }
            entries[position].status = wip_status.into();
            entries[position].sync_status_fields();
        }
        // 取活依据是**调度器每轮重算的机制产物**,不是条目自身的状态。
        // 原来把它写进条目字段,而 `structured_entry` 会把条目的全部 fields 序列化进
        // 控制状态——于是这行必然过期的快照跟着条目进每一次 `work next` 与文档快照,
        // 只写不读却永久占上下文(全仓 grep,唯一"读者"就是覆盖它的这段自己)。
        // 现在落 append-only 过程事实,与 incident 同一去处。
        if let Err(error) = crate::work::log::append(
            &ctx.project_root,
            "claim",
            Some(id),
            ctx,
            serde_json::json!({
                "source": if is_default { "engine" } else { "override" },
                "execution": execution,
                "reason": if is_default {
                    state.reason.clone()
                } else {
                    input.reason.as_deref().unwrap_or_default().trim().to_string()
                },
                "decision": format!("{:?}", state.decision),
            }),
        ) {
            tracing::warn!(%error, "claim observation could not be recorded");
        }
        // D-354:落「取得线」事实(设计 parallel_lines_ui §1.2:被取得是事实不是推断)。
        // 默认线不写字段(无字段 = 默认线),接管时清掉他线残留。
        match &me {
            Some(line) => {
                match entries[position]
                    .fields
                    .iter_mut()
                    .find(|(key, _)| key == "取得线")
                {
                    Some((_, value)) => *value = line.clone(),
                    None => entries[position]
                        .fields
                        .push(("取得线".into(), line.clone())),
                }
            }
            None => entries[position].fields.retain(|(key, _)| key != "取得线"),
        }
        if let Err(error) = store.save(&entries) {
            return ToolOutput::error(format!("cannot save claim: {error}"));
        }
        ToolOutput::ok(
            json!({
                "claimed": id,
                "execution": execution,
                "lifecycle_status": wip_status,
                "override": !is_default,
                "line": me,
                "work_priority": priority_name(ctx.work_priority),
                // 认领收据提供已登记关联；不让 runner 解析自由叙述来猜依赖。
                "context": { "related_items": entries[position].refs() },
            })
            .to_string(),
        )
    }
}
