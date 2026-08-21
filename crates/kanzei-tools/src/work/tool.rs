//! `work` 工具的输入契约与写操作。
//!
//! 调度投影保留在父模块；本模块集中处理工具 schema、Work Unit 事件写入与旧队列
//! claim，避免控制面解析与命令副作用继续堆叠在同一个巨石文件。

use super::*;
use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
struct WorkInput {
    /// next/claim 保留旧工作流；其余动作用于 work_units_v1。
    action: String,
    /// claim/get_unit/checkpoint/block/unblock/verify/evidence/complete/supersede 必填。
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    requirement_id: Option<String>,
    #[serde(default)]
    objective: Option<String>,
    #[serde(default)]
    scope: Vec<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    acceptance: Vec<String>,
    #[serde(default)]
    verification: Vec<String>,
    #[serde(default)]
    base_revision: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    next_action: Option<String>,
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    retrieval_refs: Vec<String>,
    #[serde(default)]
    observed_head: Option<String>,
    #[serde(default)]
    observed_worktree_hash: Option<String>,
    #[serde(default)]
    criterion: Option<String>,
    #[serde(default)]
    evidence_refs: Vec<String>,
    /// 偏离默认选择、接管、阻塞/解阻塞、废弃时写入审计。
    #[serde(default)]
    reason: Option<String>,
}

const UNIT_ACTIONS: &[&str] = &[
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

fn execute_work_unit_action(input: &WorkInput, ctx: &ToolCtx) -> Option<ToolOutput> {
    let is_unit_claim =
        input.action == "claim" && input.id.as_deref().is_some_and(|id| id.contains("/W"));
    if !is_unit_claim && !UNIT_ACTIONS.contains(&input.action.as_str()) {
        return None;
    }

    if input.action == "list_units" {
        let store = match open_work_store(&ctx.project_root) {
            Ok(store) => store,
            Err(error) => return Some(ToolOutput::error(error)),
        };
        return Some(
            match store.list_work_units(input.requirement_id.as_deref()) {
                Ok(units) => pretty(units),
                Err(error) => ToolOutput::error(format!("cannot list work units: {error}")),
            },
        );
    }
    if input.action == "get_unit" {
        let id = match required(&input.id, "id") {
            Ok(id) => id,
            Err(error) => return Some(ToolOutput::error(error)),
        };
        let store = match open_work_store(&ctx.project_root) {
            Ok(store) => store,
            Err(error) => return Some(ToolOutput::error(error)),
        };
        return Some(
            match (store.get_work_unit(id), store.list_work_events(id)) {
                (Ok(Some(unit)), Ok(events)) => pretty(json!({"unit": unit, "events": events})),
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
        if input.acceptance.is_empty() {
            return Some(ToolOutput::error(
                "`acceptance` 至少需要一条可逐项登记证据的验收标准",
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

    if is_unit_claim {
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
        if matches!(
            current.status,
            WorkUnitStatus::Active | WorkUnitStatus::Verifying
        ) && !takeover
        {
            return Some(pretty(current));
        }

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
        // R-313:work_units_v1 的父 Outcome 也不能绕过进入 doing 前的发现/确认门禁。
        if outcome.status != "doing" {
            if let Err(error) = crate::tracker::TrackerTool::check_requirement_start(outcome) {
                return Some(ToolOutput::error(format!("{} {error}", outcome.id)));
            }
            if let Err(error) = req_store.transition_allowed(&outcome.status, "doing") {
                return Some(ToolOutput::error(error));
            }
            outcome.status = "doing".into();
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

pub struct WorkTool;

#[async_trait]
impl Tool for WorkTool {
    fn name(&self) -> &'static str {
        "work"
    }

    fn description(&self) -> String {
        "Resolve the authoritative work decision. Legacy requirements/defects keep next/claim; \
         requirements opting into `work_units_v1` use create_unit, claim, checkpoint, block, \
         unblock, verify, evidence, complete and supersede over append-only events. \
         WIP discipline is per line: items held by other lines appear as foreign_wip (read-only \
         background) and are never selected for this line; claiming one requires an explicit \
         takeover reason. Queue priority comes from the run and cannot be overridden by tool input. \n         Use action `handoff` to declare the task finished and hand control back: the engine \n         stops the run and will NOT push you to keep going. Call it when the work is genuinely \n         done, when you need the user to decide, or when there is nothing useful left to do — \n         do not invent work to fill a round."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(WorkInput)).unwrap();
        schema["properties"]["action"]["enum"] = json!([
            "next",
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
        // R-322:handoff 是控制面声明,不碰磁盘,按只读归类(权限上无副作用可管)。
        let read_only = matches!(action, "next" | "get_unit" | "list_units" | "handoff");
        vec![format!(
            "{}:{action}",
            if read_only { "read" } else { "write" }
        )]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: WorkInput = match crate::parse_input(self, input) {
            Ok(input) => input,
            Err(output) => return output,
        };
        // R-322(#7):模型的停机权。本动作**不写任何东西**——它只把「我认为做完了」
        // 变成一条引擎能机械观察到的事实(轮末 MetricsSink 按 ToolStart/ToolEnd 收口,
        // 与 D-654 的 close 计数同一套路;不扫消息历史,因为轮中压缩会让切片错位)。
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
            return ToolOutput::ok(format!(
                "handoff acknowledged: {summary}
control returned to the user;                  the engine will not push this run further."
            ));
        }
        if let Some(output) = execute_work_unit_action(&input, ctx) {
            return output;
        }
        if input.action == "next" {
            return match resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority) {
                Ok(state) => ToolOutput::ok(
                    serde_json::to_string_pretty(&compact_for_context(state)).unwrap(),
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
        }
        let audit = if is_default {
            format!("engine:{}", state.reason)
        } else {
            format!(
                "override:{}",
                input.reason.as_deref().unwrap_or_default().trim()
            )
        };
        match entries[position]
            .fields
            .iter_mut()
            .find(|(key, _)| key == "取活依据")
        {
            Some((_, value)) => *value = audit,
            None => entries[position].fields.push(("取活依据".into(), audit)),
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
                "lifecycle_status": wip_status,
                "override": !is_default,
                "line": me,
                "work_priority": priority_name(ctx.work_priority),
            })
            .to_string(),
        )
    }
}
