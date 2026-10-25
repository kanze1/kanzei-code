//! 当前工作事实随每次模型请求刷新，只补充适用的状态操作，不规定实现方法。
use super::WorkItemSummary;
use kanzei_harness::{Component, HarnessDraft, ProfileKind, ResolveCtx};
use kanzei_memory::docstore::{DocKind, Entry};

/// Recorded test outcomes are context, not proof that the current version passed.
pub(super) fn recent_test_evidence(
    root: &std::path::Path,
    entry_id: &str,
) -> Vec<serde_json::Value> {
    let mut records = crate::test_record::records_for_entry(root, entry_id);
    records.sort_by_key(|record| {
        record["id"]
            .as_str()
            .unwrap_or_default()
            .strip_prefix("T-")
            .and_then(|id| id.parse::<u64>().ok())
            .unwrap_or(0)
    });
    records.into_iter().rev().take(3).map(|record| {
        let summary = record["fields"].as_array().and_then(|fields| {
            fields.iter().find(|field| field["key"] == "摘要")
                .and_then(|field| field["value"].as_str())
        }).unwrap_or_default();
        serde_json::json!({
            "id": record["id"], "status": record["status"],
            "title": record["title"].as_str().unwrap_or_default().chars().take(120).collect::<String>(),
            "summary": summary.chars().take(260).collect::<String>(),
        })
    }).collect()
}

pub(super) fn recent_completed(
    documents: [(&[Entry], &[Entry], &DocKind); 2],
) -> Vec<WorkItemSummary> {
    documents
        .into_iter()
        .flat_map(|(active, archive, kind)| {
            active
                .iter()
                .chain(archive.iter())
                .filter(|entry| kind.terminal.contains(&entry.status.as_str()))
                .rev()
                .take(4)
                .map(move |entry| WorkItemSummary {
                    id: entry.id.clone(),
                    kind: if kind.prefix == "R" {
                        "requirement"
                    } else {
                        "defect"
                    }
                    .into(),
                    title: entry.title.clone(),
                    lifecycle_status: entry.status.clone(),
                    block_reasons: Vec::new(),
                })
        })
        .collect()
}

pub struct WorkControlContext(pub kanzei_harness::auto_run::WorkPriority);

impl Component for WorkControlContext {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile == ProfileKind::DevAuto
            && !kanzei_harness::is_general_conversation_root(&ctx.project_root)
        {
            let priority = self.0;
            draft.context.insert(
                "development/control",
                kanzei_harness::context::refreshing_source("development/control", move |ctx| {
                    Some(super::resolved_control_prompt(
                        &ctx.cwd,
                        &ctx.project_root,
                        priority,
                    ))
                }),
            );
        }
        Ok(())
    }
}

/// 只解释当前状态对应的操作；方向复核、目标收口与普通执行互不叠加。
/// 实现、调查与拆解方式由 Agent 根据用户目标和实际缺口决定。
pub(super) fn conditional_guidance(state: &super::ResolvedControlState) -> String {
    let mut rules = Vec::new();
    if state.resume_reconcile.is_some() {
        rules.push(
            "resume_reconcile 说明进展记录缺少或未匹配当前来源；结合实际代码恢复工作。".to_owned(),
        );
    }
    if state.resume_worktree.is_some() {
        rules.push("resume_worktree 是本步采样的工作树快照，后续变化按需核实。".to_owned());
    }
    if let Some(item) = &state.selected {
        let context = item.work_unit_context.as_ref();
        if let Some(revision) = context.and_then(|context| context.revision_action) {
            rules.push(match revision {
                super::WorkRevisionAction::Yield => "当前方向待复核：安全停止旧方向的工作及相关任务，保存检查点后用 work revision_yield 交还节点。",
                super::WorkRevisionAction::Resolve => "当前处理方向复核：用 work impact 读取受影响节点和版本，再通过 apply_revision 调整契约与依赖。保留已完成历史，需要重验或替换的结果使用后继节点。",
            }.into());
        } else if let Some(action) = item.outcome_action {
            rules.push(match action {
                super::OutcomeAction::Plan => "当前目标尚无工作节点，用 work create_unit 记录可验收的工作，按实际需要拆解。",
                super::OutcomeAction::Accept => "当前目标的节点已结束，核对原始目标和实际交付；满足后用 req close 收口，有遗漏则补充工作节点。节点被替代不代表目标已交付。",
            }.into());
        } else {
            let step = context.map(|context| context.readiness.step);
            if step.is_none_or(|step| step == kanzei_core::WorkNodeStep::Execute) {
                match state.decision {
                    super::WorkDecision::Start => rules.push("当前 selected 为默认选择，开始执行前用 work claim 登记。需要改选时，说明 reason 并遵守依赖、归属和 WIP 约束。".into()),
                    super::WorkDecision::Resume => rules.push("当前 selected 已在进行，可继续推进；工作内容或方向变化时按事实更新状态。".into()),
                    _ => {},
                }
            }
            match step {
                Some(kanzei_core::WorkNodeStep::Plan) => rules.push("当前节点尚未就绪，可用 work refine 完善验收，或按实际缺口用 work decompose 拆分；更新契约时使用最新 source_sequence。".into()),
                Some(kanzei_core::WorkNodeStep::Integrate) => rules.push("当前进行父级组合验收，结合子结果核对原始目标并记录本节点证据；发现缺口时补充工作。".into()),
                _ => {},
            }
            if matches!(
                step,
                Some(kanzei_core::WorkNodeStep::Execute | kanzei_core::WorkNodeStep::Integrate)
            ) {
                rules.push("工作方法与拆解粒度由你按目标和实际复杂度决定。交接可用 work checkpoint；交付通过 work verify、evidence、complete 记录实际验收结果。".into());
            }
        }
    }
    if rules.is_empty() {
        String::new()
    } else {
        format!(
            "<current-work-policy>\n{}\n</current-work-policy>",
            rules.join("\n")
        )
    }
}

pub fn resolved_control_prompt(
    cwd: &std::path::Path,
    project_root: &std::path::Path,
    priority: kanzei_harness::auto_run::WorkPriority,
) -> String {
    let state = super::resolve_work_selection(cwd, project_root, priority);
    let tests = state
        .as_ref()
        .ok()
        .and_then(|state| state.selected.as_ref())
        .map(|selected| recent_test_evidence(project_root, &selected.id))
        .unwrap_or_default();
    let mut prompt = resolved_control_prompt_of(state);
    if !tests.is_empty() {
        prompt.push_str(&format!(
            "\n<current-test-evidence>\n{}\n</current-test-evidence>\n\
             以上是历史测试记录；是否适用于本次交付取决于源码版本和验证范围。\n",
            serde_json::to_string(&tests).expect("test summaries serialize")
        ));
    }
    prompt
}

/// 把**已算好**的裁决渲染成注入块。
///
/// 调用方可复用同一份选中工作，避免重复采样后出现事实或目标不一致。
pub fn resolved_control_prompt_of(state: Result<super::ResolvedControlState, String>) -> String {
    let guidance = state
        .as_ref()
        .ok()
        .map(conditional_guidance)
        .unwrap_or_default();
    let state = state
        .map(super::output::structured_control_output)
        .map(|state| serde_json::to_string_pretty(&state).expect("control state serializes"))
        .unwrap_or_else(|error| {
            serde_json::json!({"decision": "error", "reason": error}).to_string()
        });
    format!("\n\n<resolved-control-state>\n{state}\n</resolved-control-state>\n{guidance}")
}
