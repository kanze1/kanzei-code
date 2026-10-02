//! Shared control for AUTO and the legacy survey adapters.
#[cfg(test)]
mod tests;
use crate::research_plan::{load_plan, PlanBudget};

pub fn workflow(
    root: &Path,
    topic: &str,
) -> Result<Option<crate::research_workflow::Workflow>, String> {
    crate::docstore::DocStore::validate_topic(topic).map_err(|e| e.to_string())?;
    if !root
        .join(".kanzei/research")
        .join(topic)
        .join("workflow.json")
        .is_file()
    {
        return Ok(None);
    }
    crate::research_workflow::load(root, topic)
}
use std::path::Path;

pub(crate) fn budget(root: &Path, topic: &str) -> Result<PlanBudget, String> {
    if let Some(workflow) = workflow(root, topic)? {
        return Ok(workflow.budget);
    }
    let dir = crate::research_workflow::topic_dir(root, topic)?;
    let file = dir.join("budget.json");
    let budget = if file.is_file() {
        serde_json::from_str(&std::fs::read_to_string(file).map_err(|e| e.to_string())?)
            .map_err(|e| format!("预算 JSON 无效: {e}"))?
    } else {
        load_plan(root, topic)?.ok_or("尚未创建研究计划")?.budget
    };
    validate_budget(&budget)?;
    Ok(budget)
}

pub(crate) fn validate_budget(budget: &PlanBudget) -> Result<(), String> {
    if budget.max_rounds == 0 || budget.max_tokens == 0 || budget.max_concurrency == 0 {
        return Err("检索预算必须大于零".into());
    }
    Ok(())
}

pub(crate) fn require_survey(root: &Path, topic: &str) -> Result<(), String> {
    if let Some(workflow) = workflow(root, topic)? {
        if workflow.stage != crate::research_workflow::Stage::Survey || !workflow.runnable() {
            return Err(format!(
                "调研由 research_workflow 控制；当前 {}，paused={}；请从主流程恢复",
                workflow.stage.label(),
                workflow.paused
            ));
        }
    }
    Ok(())
}
