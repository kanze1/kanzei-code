//! R-277 批1：研究计划树与澄清闸门。
//!
//! 计划是 research agent 与前端之间的持久化事实，不把结构化计划压成 prompt 字符串。
//! agent 可以创建计划、追加待澄清问题并请求用户审批；只有前端/用户侧后续显式审批，
//! 才能把状态推进到 approved，agent 本身不能伪造审批。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const PLAN_FILE: &str = "plan.json";
const DEFAULT_ROUNDS: u32 = 3;
const DEFAULT_TOKENS: u32 = 16_000;
const DEFAULT_CONCURRENCY: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Draft,
    Clarifying,
    AwaitingApproval,
    Approved,
    Running,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanNodeStatus {
    #[default]
    Pending,
    Clarifying,
    Ready,
    Running,
    Completed,
    Blocked,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlanBudget {
    #[serde(default = "default_rounds")]
    pub max_rounds: u32,
    #[serde(default = "default_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_concurrency")]
    pub max_concurrency: u32,
}

impl Default for PlanBudget {
    fn default() -> Self {
        Self {
            max_rounds: DEFAULT_ROUNDS,
            max_tokens: DEFAULT_TOKENS,
            max_concurrency: DEFAULT_CONCURRENCY,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlanNode {
    pub id: String,
    pub title: String,
    pub objective: String,
    #[serde(default)]
    pub status: PlanNodeStatus,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub children: Vec<PlanNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ResearchPlan {
    pub version: u32,
    pub topic: String,
    pub title: String,
    pub status: PlanStatus,
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<PlanNode>,
    #[serde(default)]
    pub budget: PlanBudget,
    #[serde(default)]
    pub revision: u32,
}

fn default_rounds() -> u32 {
    DEFAULT_ROUNDS
}

fn default_tokens() -> u32 {
    DEFAULT_TOKENS
}

fn default_concurrency() -> u32 {
    DEFAULT_CONCURRENCY
}

fn plan_path(root: &Path, topic: &str) -> Result<PathBuf, String> {
    kanzei_memory::docstore::DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    Ok(root.join(".kanzei/research").join(topic).join(PLAN_FILE))
}

fn validate_node(node: &PlanNode, ids: &mut HashSet<String>) -> Result<(), String> {
    if node.id.trim().is_empty() || node.title.trim().is_empty() || node.objective.trim().is_empty()
    {
        return Err("计划节点必须包含非空 id、title、objective".into());
    }
    if !ids.insert(node.id.clone()) {
        return Err(format!("计划节点 id 重复: {}", node.id));
    }
    for dependency in &node.depends_on {
        if dependency == &node.id {
            return Err(format!("计划节点不能依赖自身: {}", node.id));
        }
    }
    for child in &node.children {
        validate_node(child, ids)?;
    }
    Ok(())
}

pub fn validate_plan(plan: &ResearchPlan, topic: &str) -> Result<(), String> {
    if plan.version != 1 {
        return Err(format!("不支持的研究计划版本: {}", plan.version));
    }
    if plan.topic != topic {
        return Err(format!(
            "计划 topic `{}` 与请求 topic `{topic}` 不一致",
            plan.topic
        ));
    }
    if plan.title.trim().is_empty() || plan.nodes.is_empty() {
        return Err("研究计划必须包含 title 和至少一个节点".into());
    }
    if plan.budget.max_rounds == 0
        || plan.budget.max_tokens == 0
        || plan.budget.max_concurrency == 0
    {
        return Err("预算 max_rounds、max_tokens、max_concurrency 都必须大于 0".into());
    }
    let mut ids = HashSet::new();
    for node in &plan.nodes {
        validate_node(node, &mut ids)?;
    }
    for node in &plan.nodes {
        for dependency in &node.depends_on {
            if !ids.contains(dependency) {
                return Err(format!(
                    "计划节点 `{}` 依赖不存在的节点 `{dependency}`",
                    node.id
                ));
            }
        }
    }
    Ok(())
}

fn load_plan(root: &Path, topic: &str) -> Result<Option<ResearchPlan>, String> {
    let path = plan_path(root, topic)?;
    if !path.is_file() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("读取研究计划失败: {error}"))?;
    let plan =
        serde_json::from_str(&text).map_err(|error| format!("研究计划 JSON 无效: {error}"))?;
    Ok(Some(plan))
}

fn save_plan(root: &Path, plan: &ResearchPlan) -> Result<(), String> {
    validate_plan(plan, &plan.topic)?;
    let path = plan_path(root, &plan.topic)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("创建研究计划目录失败: {error}"))?;
    }
    let _lock = kanzei_base::atomic_file::lock_exclusive(&path)
        .map_err(|error| format!("锁定研究计划失败: {error}"))?;
    let text = serde_json::to_string_pretty(plan)
        .map_err(|error| format!("序列化研究计划失败: {error}"))?;
    kanzei_base::atomic_file::write_atomic(&path, &text)
        .map_err(|error| format!("保存研究计划失败: {error}"))?;
    Ok(())
}

pub struct ResearchPlanTool;

impl ResearchPlanTool {
    fn root(ctx: &ToolCtx) -> &Path {
        &ctx.project_root
    }
}

#[async_trait]
impl Tool for ResearchPlanTool {
    fn name(&self) -> &'static str {
        "research_plan"
    }

    fn description(&self) -> String {
        "研究计划树：get 读取、create 创建显式计划、clarify 记录待澄清问题、request_approval 请求用户审批；agent 不得自行 approve 或运行未批准计划。计划保存到 .kanzei/research/<topic>/plan.json。".into()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["get", "create", "clarify", "request_approval"] },
                "topic": { "type": "string", "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$" },
                "plan": { "type": "object" },
                "questions": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["action", "topic"]
        })
    }

    fn resources(&self, input: &Value) -> Vec<String> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let access = if action == "get" { "read" } else { "write" };
        vec![format!("{access}:{action}")]
    }

    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput {
        let action = input.get("action").and_then(Value::as_str).unwrap_or("");
        let Some(topic) = input.get("topic").and_then(Value::as_str) else {
            return ToolOutput::needs_correction("MISSING_TOPIC", "research_plan 必须提供 topic");
        };
        if let Err(error) = plan_path(Self::root(ctx), topic) {
            return ToolOutput::needs_correction("INVALID_TOPIC", error);
        }
        match action {
            "get" => match load_plan(Self::root(ctx), topic) {
                Ok(Some(plan)) => {
                    ToolOutput::ok(serde_json::to_string_pretty(&plan).unwrap_or_default())
                }
                Ok(None) => ToolOutput::ok(json!({ "exists": false, "topic": topic }).to_string()),
                Err(error) => ToolOutput::error(error),
            },
            "create" => {
                let Some(raw_plan) = input.get("plan") else {
                    return ToolOutput::needs_correction(
                        "MISSING_PLAN",
                        "create 必须提供 plan 对象",
                    );
                };
                let mut plan: ResearchPlan = match serde_json::from_value(raw_plan.clone()) {
                    Ok(plan) => plan,
                    Err(error) => {
                        return ToolOutput::needs_correction("INVALID_PLAN", error.to_string())
                    }
                };
                plan.topic = topic.to_string();
                plan.status = PlanStatus::Draft;
                plan.revision = 1;
                if let Err(error) = validate_plan(&plan, topic) {
                    return ToolOutput::needs_correction("INVALID_PLAN", error);
                }
                match load_plan(Self::root(ctx), topic) {
                    Ok(Some(_)) => ToolOutput::error(format!(
                        "topic `{topic}` 已存在研究计划；请先 get，不覆盖既有计划"
                    )),
                    Ok(None) => match save_plan(Self::root(ctx), &plan) {
                        Ok(()) => {
                            ToolOutput::ok(serde_json::to_string_pretty(&plan).unwrap_or_default())
                        }
                        Err(error) => ToolOutput::error(error),
                    },
                    Err(error) => ToolOutput::error(error),
                }
            }
            "clarify" => {
                let questions = input
                    .get("questions")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if questions.is_empty() {
                    return ToolOutput::needs_correction(
                        "MISSING_QUESTIONS",
                        "clarify 必须提供至少一个 questions 项",
                    );
                }
                match load_plan(Self::root(ctx), topic) {
                    Ok(Some(mut plan)) => {
                        plan.open_questions.extend(questions);
                        plan.open_questions.sort();
                        plan.open_questions.dedup();
                        plan.status = PlanStatus::Clarifying;
                        plan.revision += 1;
                        match save_plan(Self::root(ctx), &plan) {
                            Ok(()) => ToolOutput::ok(
                                serde_json::to_string_pretty(&plan).unwrap_or_default(),
                            ),
                            Err(error) => ToolOutput::error(error),
                        }
                    }
                    Ok(None) => ToolOutput::error(format!("topic `{topic}` 尚未创建研究计划")),
                    Err(error) => ToolOutput::error(error),
                }
            }
            "request_approval" => match load_plan(Self::root(ctx), topic) {
                Ok(Some(mut plan)) => {
                    if !plan.open_questions.is_empty() {
                        return ToolOutput::needs_confirmation(
                            "CLARIFICATION_REQUIRED",
                            format!(
                                "计划仍有待澄清问题，必须先由用户回答: {}",
                                plan.open_questions.join("；")
                            ),
                        );
                    }
                    if !matches!(plan.status, PlanStatus::Draft | PlanStatus::Clarifying) {
                        return ToolOutput::error(format!(
                            "当前计划状态 {:?} 不能请求审批",
                            plan.status
                        ));
                    }
                    plan.status = PlanStatus::AwaitingApproval;
                    plan.revision += 1;
                    match save_plan(Self::root(ctx), &plan) {
                        Ok(()) => ToolOutput::needs_confirmation(
                            "PLAN_AWAITING_APPROVAL",
                            serde_json::to_string_pretty(&plan).unwrap_or_default(),
                        ),
                        Err(error) => ToolOutput::error(error),
                    }
                }
                Ok(None) => ToolOutput::error(format!("topic `{topic}` 尚未创建研究计划")),
                Err(error) => ToolOutput::error(error),
            },
            _ => ToolOutput::needs_correction(
                "INVALID_ACTION",
                "action 只能是 get/create/clarify/request_approval",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-research-plan-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn sample_plan(topic: &str) -> ResearchPlan {
        ResearchPlan {
            version: 1,
            topic: topic.into(),
            title: "研究问题".into(),
            status: PlanStatus::Draft,
            open_questions: Vec::new(),
            nodes: vec![PlanNode {
                id: "scope".into(),
                title: "界定范围".into(),
                objective: "明确研究对象与排除项".into(),
                status: PlanNodeStatus::Pending,
                depends_on: Vec::new(),
                children: Vec::new(),
            }],
            budget: PlanBudget::default(),
            revision: 0,
        }
    }

    #[test]
    fn validates_tree_dependencies_and_rejects_duplicates() {
        let mut plan = sample_plan("plan-smoke");
        assert!(validate_plan(&plan, "plan-smoke").is_ok());
        plan.nodes.push(plan.nodes[0].clone());
        assert!(validate_plan(&plan, "plan-smoke")
            .unwrap_err()
            .contains("重复"));
    }

    #[tokio::test]
    async fn create_clarify_and_request_approval_persist_state() {
        let root = temp_root("state");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let tool = ResearchPlanTool;
        let mut plan = sample_plan("state-smoke");
        plan.status = PlanStatus::Approved;
        let created = tool
            .execute(
                json!({ "action": "create", "topic": "state-smoke", "plan": plan }),
                &ctx,
            )
            .await;
        assert!(!created.is_error, "{}", created.content);
        let clarified = tool.execute(json!({ "action": "clarify", "topic": "state-smoke", "questions": ["范围是否包含代码实现？"] }), &ctx).await;
        assert!(!clarified.is_error, "{}", clarified.content);
        let blocked = tool
            .execute(
                json!({ "action": "request_approval", "topic": "state-smoke" }),
                &ctx,
            )
            .await;
        assert_eq!(blocked.code.as_deref(), Some("CLARIFICATION_REQUIRED"));
        assert!(blocked.is_error);
        let path = root.join(".kanzei/research/state-smoke/plan.json");
        assert!(path.is_file());
        std::fs::remove_dir_all(root).ok();
    }
}
