//! 当前裁决随每次模型请求刷新，不追加进历史。
use super::WorkItemSummary;
use kanzei_harness::{Component, HarnessDraft, ProfileKind, ResolveCtx};
use kanzei_memory::docstore::{DocKind, Entry};

/// Facts are supplied by the engine; the model only writes semantic milestone handoffs.
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
        if ctx.profile == ProfileKind::Dev
            && !kanzei_harness::is_general_conversation_root(&ctx.project_root)
        {
            let priority = self.0;
            draft.context.insert(
                "dev/control",
                kanzei_harness::context::refreshing_source("dev/control", move |ctx| {
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

/// 仅当前选中工作的适用规则；状态已在同一次裁决中取得，不额外查询队列/git。
pub(super) fn conditional_guidance(state: &super::ResolvedControlState) -> String {
    let mut rules = Vec::new();
    if state.resume_reconcile.is_some() {
        rules.push(
            "resume_reconcile 提示进展可能陈旧：先核对已落地代码和提交，再修正进展并继续。"
                .to_owned(),
        );
    }
    if state.resume_worktree.is_some() {
        rules.push(
            "resume_worktree 已提供本步工作树事实；直接读相关文件，不重复查询同一份 git 状态。"
                .to_owned(),
        );
    }
    if let Some(item) = &state.selected {
        match item.outcome_action {
            Some(super::OutcomeAction::Plan) => rules.push("当前 Outcome 尚无工作节点：保留用户原始目标，使用 work create_unit 创建首个 goal/规划节点，明确验收并把未知写入 node.questions。随后按实际缺口递归拆解；这不是等待外部解锁的阻塞。".into()),
            Some(super::OutcomeAction::Accept) => rules.push("当前 Outcome 的节点均已结束：核对原始委托和 Outcome 验收证据，满足后 req close；若发现遗漏，创建新的工作节点继续。全部 superseded 不能当作交付。".into()),
            None => {},
        }
        let field = |name: &str| {
            item.fields
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.value.as_str())
                .unwrap_or("")
        };
        if item.kind == "work_unit" {
            rules.push("当前是 work_unit：交接用 work checkpoint；完成走 verify → 按原验收逐条 evidence → complete，进展不复制回父 Outcome。".into());
            if let Some(context) = &item.work_unit_context {
                match context.revision_action {
                    Some(super::WorkRevisionAction::Yield) => rules.push("方向已变化：停止旧契约的实现，确认自己启动的任务均已停止或结束，保存检查点，然后 work revision_yield(id,revision_id,reason) 交还节点；不要 claim、继续实现或完成旧验收。".into()),
                    Some(super::WorkRevisionAction::Resolve) => rules.push("当前工作是方向复核，无需 claim：读取 impact 和受影响节点，按新方向逐项判断 retain/revise/reverify/replace/retire。保留仍有效的结果；完成历史不改写，需要重验时建立后继节点并修正依赖。用最新 expected_versions 与完整 revision_decisions/additions 调用 apply_revision；版本冲突先重读，不覆盖未知变化。".into()),
                    None => {},
                }
                match context.readiness.step {
                    kanzei_core::WorkNodeStep::Plan => rules.push("当前节点需要规划：若 acceptance 为空，先用 refine 明确完成条件；不要 claim 或编造证据。解释目标与缺口，用 decompose 把未决问题转为可验收的调研/设计/实验节点；子结果返回后用 refine 更新完整契约并消除已解决问题。两者均须读取最新 source_sequence。不能直接宣称验证完成。".into()),
                    kanzei_core::WorkNodeStep::Integrate => rules.push("子节点已结束，当前执行父级组合验收。逐项核对原始目标与子结果，补齐自身 evidence；子节点完成不能替代父级验收。发现缺口则继续拆分。".into()),
                    kanzei_core::WorkNodeStep::Execute => rules.push("先确认此节点能在一次有界工作中独立验收；若仍含知识缺口或过大范围，用 decompose 继续拆分。只展开当前需要的分支，沿 node.parent_id 保留目标归属，依赖使用 dependencies。".into()),
                    kanzei_core::WorkNodeStep::Wait | kanzei_core::WorkNodeStep::Finished => {},
                }
            }
        } else {
            if !field("批次").is_empty() {
                rules.push("批次规则：总数按实际工作选定且不超过 10。每批结束一次更新 批次 k/N 和语义进展；提交主题带 <ID> B<k>。完成时计数必须对应真实完成数，超大范围拆为后续条目，不虚报填满。".into());
            }
            if matches!(field("复杂度"), "中" | "大")
                && field("进展").is_empty()
                && field("设计冻结").is_empty()
            {
                rules.push("首次实现前做一次 Design freeze：不变式、权威数据源、预期修改文件、最小验证四项，写入设计冻结字段。已有约定沿用，只有新证据推翻时才修订。".into());
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
             These are recorded outcomes, refreshed automatically with the worktree facts. \
             Do not copy them into progress after every test. Their applicability still depends \
             on the source version and test scope; preserve that distinction at acceptance.\n",
            serde_json::to_string(&tests).unwrap_or_default()
        ));
    }
    prompt
}

/// 把**已算好**的裁决渲染成注入块。
///
/// 拆出来是为了让一轮之内只算一次:`resolve_work_decision` 内部有 4 次 git 调用
/// (含 `git diff --binary HEAD`),而同一份裁决既要进 system prompt,也要作为
/// 任务上下文灌给勘察/复核角色。算两次除了浪费,还会出现主代理与角色看到不同
/// 条目的可能——尤其复核发生在实现段之后,重算会选到下一条。
pub fn resolved_control_prompt_of(state: Result<super::ResolvedControlState, String>) -> String {
    let guidance = state
        .as_ref()
        .ok()
        .map(conditional_guidance)
        .unwrap_or_default();
    let state = state
        .map(super::output::structured_control_output)
        .map(|state| serde_json::to_string_pretty(&state).unwrap_or_else(|_| "{}".into()))
        .unwrap_or_else(|error| {
            serde_json::json!({"decision": "error", "reason": error}).to_string()
        });
    format!("\n\n<resolved-control-state>\n{state}\n</resolved-control-state>\n{guidance}")
}
