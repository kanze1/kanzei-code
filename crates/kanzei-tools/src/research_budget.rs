//! Explicit retrieval budgets consumed by research tools, never model instructions.
use std::path::Path;

use crate::research_plan::{load_plan, PlanBudget};

pub(crate) fn load(root: &Path, topic: &str) -> Result<PlanBudget, String> {
    kanzei_memory::docstore::DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    let file = root
        .join(".kanzei/research")
        .join(topic)
        .join("budget.json");
    let budget = match std::fs::read_to_string(&file) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|error| format!("预算 JSON 无效: {error}"))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            load_plan(root, topic)?.ok_or("尚未创建研究计划")?.budget
        }
        Err(error) => return Err(format!("读取检索预算失败 {}: {error}", file.display())),
    };
    if budget.max_rounds == 0 || budget.max_tokens == 0 || budget.max_concurrency == 0 {
        return Err("检索预算必须大于零".into());
    }
    Ok(budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research_plan::{save_plan, PlanNode, PlanNodeStatus, PlanStatus, ResearchPlan};

    #[test]
    fn explicit_budget_overrides_plan_without_reading_retired_checkpoints() {
        let directory = kanzei_base::test_support::TestDir::new("kz-research-budget-");
        let root = directory.as_path();
        let plan = ResearchPlan {
            version: 1,
            topic: "budget-test".into(),
            title: "Budget source".into(),
            status: PlanStatus::Approved,
            open_questions: Vec::new(),
            nodes: vec![PlanNode {
                id: "survey".into(),
                title: "Survey".into(),
                objective: "Find evidence".into(),
                status: PlanNodeStatus::Pending,
                depends_on: Vec::new(),
                children: Vec::new(),
            }],
            budget: PlanBudget::default(),
            revision: 1,
        };
        save_plan(root, &plan).unwrap();
        let topic = root.join(".kanzei/research/budget-test");
        std::fs::write(topic.join("workflow.json"), "obsolete invalid checkpoint").unwrap();
        assert_eq!(
            load(root, "budget-test").unwrap().max_tokens,
            plan.budget.max_tokens
        );
        std::fs::write(
            topic.join("budget.json"),
            r#"{"max_rounds":2,"max_tokens":120,"max_concurrency":1}"#,
        )
        .unwrap();
        let budget = load(root, "budget-test").unwrap();
        assert_eq!(
            (budget.max_rounds, budget.max_tokens, budget.max_concurrency),
            (2, 120, 1)
        );
    }

    #[test]
    fn broken_or_zero_budget_is_an_error_instead_of_a_plan_fallback() {
        let directory = kanzei_base::test_support::TestDir::new("kz-research-budget-error-");
        let topic = directory.join(".kanzei/research/budget-test");
        std::fs::create_dir_all(&topic).unwrap();
        for text in [
            "invalid",
            r#"{"max_rounds":0,"max_tokens":120,"max_concurrency":1}"#,
        ] {
            std::fs::write(topic.join("budget.json"), text).unwrap();
            assert!(load(&directory, "budget-test").is_err());
        }
        std::fs::remove_file(topic.join("budget.json")).unwrap();
        std::fs::create_dir(topic.join("budget.json")).unwrap();
        assert!(load(&directory, "budget-test")
            .unwrap_err()
            .contains("读取检索预算失败"));
        assert!(load(&directory, "../outside").is_err());
    }
}
