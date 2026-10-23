//! Direction changes use the same project/doc locks and outcome authority as planning.
use super::*;

pub(super) fn execute(
    input: &WorkInput,
    ctx: &ToolCtx,
    actor: PlanActor,
    store: &SessionStore,
    current: &WorkProjection,
) -> ToolOutput {
    match execute_checked(input, ctx, actor, store, current) {
        Ok(output) => output,
        Err(error) => ToolOutput::needs_correction("WORK_REVISION_REJECTED", error),
    }
}

fn execute_checked(
    input: &WorkInput,
    ctx: &ToolCtx,
    actor: PlanActor,
    store: &SessionStore,
    current: &WorkProjection,
) -> Result<ToolOutput, String> {
    if input.action == "revision_yield" {
        if actor != PlanActor::Agent
            || current.claimed_by != line_identity(&ctx.cwd, &ctx.project_root)
        {
            return Err("仅当前执行线可在停止旧工作后交还节点；不能代替其他执行者确认停工".into());
        }
        let unit = store
            .append_work_fact(
                &current.unit_id,
                kanzei_core::WorkFact::RevisionYielded {
                    revision_id: required(&input.revision_id, "revision_id")?.into(),
                    reason: required(&input.reason, "reason")?.into(),
                },
            )
            .map_err(|error| error.to_string())?;
        return Ok(pretty(unit));
    }
    let expected = input
        .expected_versions
        .as_ref()
        .ok_or("先读取 impact 并提供完整 expected_versions")?;
    let all = store
        .list_work_units(None)
        .map_err(|error| error.to_string())?;
    let affected = if input.action == "begin_revision" {
        store
            .work_revision_impact(&current.unit_id)
            .map_err(|error| error.to_string())?
            .affected
            .into_iter()
            .map(|node| node.unit_id)
            .collect::<BTreeSet<_>>()
    } else {
        let revision_id = required(&input.revision_id, "revision_id")?;
        all.iter()
            .filter(|node| {
                node.review_pending()
                    && node.review.as_ref().is_some_and(|review| {
                        review.request.revision_id == revision_id
                            && review.request.anchor_id == current.unit_id
                    })
            })
            .map(|node| node.unit_id.clone())
            .collect()
    };
    let docs = DocStore::open(&ctx.project_root, &REQUIREMENTS);
    let _doc_lock = docs.lock().map_err(|error| error.to_string())?;
    let outcomes = docs.load().map_err(|error| error.to_string())?;
    for node in all.iter().filter(|node| affected.contains(&node.unit_id)) {
        if let Some(reason) = outcome_plan_blocker(
            outcomes
                .iter()
                .find(|entry| entry.id == node.requirement_id),
        ) {
            return Err(format!("{}: {reason}", node.requirement_id));
        }
    }
    if input.action == "begin_revision" {
        let request = store
            .begin_work_revision(
                &current.unit_id,
                expected,
                required(&input.direction, "direction")?,
                required(&input.reason, "reason")?,
            )
            .map_err(|error| error.to_string())?;
        Ok(pretty(request))
    } else {
        let units = store
            .apply_work_revision(
                &current.unit_id,
                required(&input.revision_id, "revision_id")?,
                expected,
                input.revision_decisions.clone(),
                input.additions.clone(),
            )
            .map_err(|error| error.to_string())?;
        Ok(pretty(units))
    }
}
