//! R-277 批2：检索-阅读-反思环的可恢复状态机。
//!
//! 该工具只向主上下文回传压缩证据(summary + relevance + source refs)，不回传网页原文。
//! 网络搜索/阅读仍由 research agent 的受限工具完成，环负责预算、轮次、缺口和来源绑定。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::docstore::{DocStore, FINDINGS, SOURCES};
use crate::research_plan::{load_plan, PlanStatus};
use crate::tracker::ResearchTrackerTool;

const LOOP_FILE: &str = "loop.json";
const MAX_SUMMARY_CHARS: usize = 4_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompressedEvidence {
    pub round: u32,
    pub relevance: f32,
    pub summary: String,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchLoopState {
    pub version: u32,
    pub topic: String,
    pub status: String,
    pub phase: String,
    pub round: u32,
    pub max_rounds: u32,
    pub max_tokens: u32,
    pub max_concurrency: u32,
    pub tokens_used: u32,
    #[serde(default)]
    pub evidence: Vec<CompressedEvidence>,
    #[serde(default)]
    pub gaps: Vec<String>,
    #[serde(default)]
    pub findings: Vec<String>,
    #[serde(default)]
    pub active_tasks: Vec<String>,
    #[serde(default)]
    pub next_task_id: u32,
}

fn state_path(root: &Path, topic: &str) -> Result<PathBuf, String> {
    DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    Ok(root.join(".kanzei/research").join(topic).join(LOOP_FILE))
}

fn load_state(root: &Path, topic: &str) -> Result<Option<ResearchLoopState>, String> {
    let path = state_path(root, topic)?;
    if !path.is_file() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("读取检索环状态失败: {error}"))?;
    let mut state: ResearchLoopState =
        serde_json::from_str(&text).map_err(|error| format!("检索环状态 JSON 无效: {error}"))?;
    if crate::research_workflow::load(root, topic)?.is_some() {
        let budget = crate::research_control::budget(root, topic)?;
        state.max_rounds = budget.max_rounds;
        state.max_tokens = budget.max_tokens;
        state.max_concurrency = budget.max_concurrency;
    }
    Ok(Some(state))
}

/// D-571:research 档的 websearch/webfetch 必须属于 begin_search 已登记的活动任务。
/// 联网工具只做这一个真源校验，不复制 loop 的轮次/并发状态机。
pub(crate) fn validate_external_task(
    root: &Path,
    topic: &str,
    task_id: &str,
) -> Result<(), String> {
    crate::research_control::require_survey(root, topic)?;
    let state = load_state(root, topic)?
        .ok_or_else(|| format!("topic `{topic}` 尚未启动检索环；先 research_loop start"))?;
    if state.status != "running" || state.phase != "search" {
        return Err(format!(
            "topic `{topic}` 当前为 {} / {}，不允许直接联网检索",
            state.status, state.phase
        ));
    }
    if !state.active_tasks.iter().any(|active| active == task_id) {
        return Err(format!(
            "RESEARCH_LOOP_TASK_REQUIRED: task_id `{task_id}` 不是 topic `{topic}` 的活动检索任务；每次 websearch/webfetch 前必须先调用 research_loop begin_search，不能绕过轮次与并发预算。"
        ));
    }
    Ok(())
}

fn save_state(root: &Path, state: &ResearchLoopState) -> Result<(), String> {
    let path = state_path(root, &state.topic)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建检索环目录失败: {error}"))?;
    }
    let _lock = kanzei_base::atomic_file::lock_exclusive(&path)
        .map_err(|error| format!("锁定检索环状态失败: {error}"))?;
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| format!("序列化检索环状态失败: {error}"))?;
    kanzei_base::atomic_file::write_atomic(&path, &text)
        .map_err(|error| format!("保存检索环状态失败: {error}"))?;
    Ok(())
}

fn state_summary(state: &ResearchLoopState) -> Value {
    json!({
        "topic": state.topic,
        "status": state.status,
        "phase": state.phase,
        "round": state.round,
        "budget": {
            "max_rounds": state.max_rounds,
            "max_tokens": state.max_tokens,
            "max_concurrency": state.max_concurrency,
            "tokens_used": state.tokens_used,
        },
        "evidence": state.evidence,
        "gaps": state.gaps,
        "findings": state.findings,
        "active_tasks": state.active_tasks,
    })
}

pub struct ResearchLoopTool;

#[async_trait]
impl Tool for ResearchLoopTool {
    fn name(&self) -> &'static str {
        "research_loop"
    }

    fn description(&self) -> String {
        "研究调研适配器：AUTO 课题服从 research_workflow 的 Survey 阶段、暂停与预算；旧课题 start 需 approved 计划。add_evidence 接收压缩证据，reflect 收敛检索，add_finding 绑定来源；resume 读取调研工件及主流程恢复信息。".into()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["start", "begin_search", "add_evidence", "reflect", "add_finding", "resume"] },
                "topic": { "type": "string", "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$" },
                "summary": { "type": "string", "maxLength": MAX_SUMMARY_CHARS },
                "relevance": { "type": "number", "minimum": 0, "maximum": 1 },
                "source_ids": { "type": "array", "items": { "type": "string" } },
                "gaps": { "type": "array", "items": { "type": "string" } },
                "task_id": { "type": "string" },
                "title": { "type": "string" },
                "conclusion": { "type": "string" },
                "evidence_level": { "type": "string", "enum": ["V0", "V1", "V2", "V3"] },
                "evidence_depth": { "type": "string", "enum": ["摘要级", "正文级", "不适用"] }
            },
            "required": ["action", "topic"]
        })
    }

    fn resources(&self, input: &Value) -> Vec<String> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        vec![format!(
            "{}:{action}",
            if action == "resume" { "read" } else { "write" }
        )]
    }

    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput {
        let action = input.get("action").and_then(Value::as_str).unwrap_or("");
        let Some(topic) = input.get("topic").and_then(Value::as_str) else {
            return ToolOutput::needs_correction("MISSING_TOPIC", "research_loop 必须提供 topic");
        };
        if let Err(error) = state_path(&ctx.project_root, topic) {
            return ToolOutput::needs_correction("INVALID_TOPIC", error);
        }
        if action != "resume" {
            if let Err(error) = crate::research_control::require_survey(&ctx.project_root, topic) {
                return ToolOutput::needs_correction("WORKFLOW_CONTROL", error);
            }
        }
        match action {
            "start" => {
                let workflow = if ctx
                    .project_root
                    .join(".kanzei/research")
                    .join(topic)
                    .join("workflow.json")
                    .exists()
                {
                    match crate::research_workflow::load(&ctx.project_root, topic) {
                        Ok(state) => state,
                        Err(error) => return ToolOutput::error(error),
                    }
                } else {
                    None
                };
                let plan = match load_plan(&ctx.project_root, topic) {
                    Ok(Some(plan)) => plan,
                    Ok(None) => {
                        return ToolOutput::error(format!("topic `{topic}` 尚未创建研究计划"))
                    }
                    Err(error) => return ToolOutput::error(error),
                };
                if plan.status != PlanStatus::Approved && workflow.is_none() {
                    return ToolOutput::needs_confirmation(
                        "PLAN_NOT_APPROVED",
                        format!("研究计划当前状态 {:?}，必须先由用户批准", plan.status),
                    );
                }
                match load_state(&ctx.project_root, topic) {
                    Ok(Some(state)) => return ToolOutput::ok(state_summary(&state).to_string()),
                    Ok(None) => {}
                    Err(error) => return ToolOutput::error(error),
                }
                let budget = match crate::research_control::budget(&ctx.project_root, topic) {
                    Ok(budget) => budget,
                    Err(error) => return ToolOutput::error(error),
                };
                let state = ResearchLoopState {
                    version: 1,
                    topic: topic.into(),
                    status: "running".into(),
                    phase: "search".into(),
                    round: 0,
                    max_rounds: budget.max_rounds,
                    max_tokens: budget.max_tokens,
                    max_concurrency: budget.max_concurrency,
                    tokens_used: 0,
                    evidence: Vec::new(),
                    gaps: Vec::new(),
                    findings: Vec::new(),
                    active_tasks: Vec::new(),
                    next_task_id: 0,
                };
                match save_state(&ctx.project_root, &state) {
                    Ok(()) => ToolOutput::ok(state_summary(&state).to_string()),
                    Err(error) => ToolOutput::error(error),
                }
            }
            "resume" => match load_state(&ctx.project_root, topic) {
                Ok(Some(state)) => {
                    let mut summary = state_summary(&state);
                    match crate::research_control::workflow(&ctx.project_root, topic) {
                        Ok(Some(workflow)) => {
                            summary["controller"] = json!("research_workflow");
                            summary["workflow_stage"] = json!(workflow.stage);
                            summary["paused"] = json!(workflow.paused);
                            summary["next_action"] = json!(workflow.guidance());
                        }
                        Ok(None) => {}
                        Err(error) => return ToolOutput::error(error),
                    }
                    ToolOutput::ok(summary.to_string())
                }
                Ok(None) => ToolOutput::error(format!("topic `{topic}` 尚未启动检索环")),
                Err(error) => ToolOutput::error(error),
            },
            "begin_search" => {
                let mut state = match load_state(&ctx.project_root, topic) {
                    Ok(Some(state)) => state,
                    Ok(None) => {
                        return ToolOutput::error(format!("topic `{topic}` 尚未启动检索环"))
                    }
                    Err(error) => return ToolOutput::error(error),
                };
                if state.status != "running" || state.phase != "search" {
                    return ToolOutput::error(format!(
                        "当前检索环状态 {} / {} 不接受新检索任务",
                        state.status, state.phase
                    ));
                }
                if state.active_tasks.len() as u32 >= state.max_concurrency {
                    return ToolOutput::needs_correction(
                        "CONCURRENCY_LIMIT",
                        format!(
                            "当前已有 {} 个活动检索任务，预算上限为 {}；先用 add_evidence 完成任务",
                            state.active_tasks.len(),
                            state.max_concurrency
                        ),
                    );
                }
                let task_id = format!("r{}-t{}", state.round, state.next_task_id);
                state.next_task_id += 1;
                state.active_tasks.push(task_id.clone());
                match save_state(&ctx.project_root, &state) {
                    Ok(()) => ToolOutput::ok(
                        json!({ "task_id": task_id, "round": state.round, "max_concurrency": state.max_concurrency, "active_tasks": state.active_tasks }).to_string(),
                    ),
                    Err(error) => ToolOutput::error(error),
                }
            }
            "add_evidence" => {
                let mut state = match load_state(&ctx.project_root, topic) {
                    Ok(Some(state)) => state,
                    Ok(None) => {
                        return ToolOutput::error(format!("topic `{topic}` 尚未启动检索环"))
                    }
                    Err(error) => return ToolOutput::error(error),
                };
                if state.status != "running" || state.phase != "search" {
                    return ToolOutput::error(format!(
                        "当前检索环状态 {} / {} 不接受证据",
                        state.status, state.phase
                    ));
                }
                let Some(task_id) = input.get("task_id").and_then(Value::as_str) else {
                    return ToolOutput::needs_correction(
                        "MISSING_TASK_ID",
                        "add_evidence 必须携带 begin_search 返回的 task_id",
                    );
                };
                if !state.active_tasks.iter().any(|active| active == task_id) {
                    return ToolOutput::needs_correction(
                        "UNKNOWN_TASK_ID",
                        "task_id 不属于当前 topic 的活动检索任务",
                    );
                }
                let summary = input
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let relevance = input
                    .get("relevance")
                    .and_then(Value::as_f64)
                    .unwrap_or(-1.0);
                let source_ids = input
                    .get("source_ids")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if summary.is_empty()
                    || summary.chars().count() > MAX_SUMMARY_CHARS
                    || !(0.0..=1.0).contains(&relevance)
                    || source_ids.is_empty()
                {
                    return ToolOutput::needs_correction("INVALID_COMPRESSED_EVIDENCE", "add_evidence 必须提供不超过 4000 字的 summary、0..1 relevance 和至少一个 source_ids；不得传原始网页字段");
                }
                state.active_tasks.retain(|active| active != task_id);
                let estimated_tokens = summary.chars().count().div_ceil(4) as u32;
                if state.tokens_used.saturating_add(estimated_tokens) > state.max_tokens {
                    state.status = "budget_exhausted".into();
                    state.phase = "synthesize".into();
                    let _ = save_state(&ctx.project_root, &state);
                    return ToolOutput::ok(state_summary(&state).to_string());
                }
                state.tokens_used += estimated_tokens;
                state.evidence.push(CompressedEvidence {
                    round: state.round,
                    relevance: relevance as f32,
                    summary: summary.into(),
                    source_ids,
                });
                match save_state(&ctx.project_root, &state) {
                    Ok(()) => ToolOutput::ok(state_summary(&state).to_string()),
                    Err(error) => ToolOutput::error(error),
                }
            }
            "reflect" => {
                let mut state = match load_state(&ctx.project_root, topic) {
                    Ok(Some(state)) => state,
                    Ok(None) => {
                        return ToolOutput::error(format!("topic `{topic}` 尚未启动检索环"))
                    }
                    Err(error) => return ToolOutput::error(error),
                };
                if !state.active_tasks.is_empty() {
                    return ToolOutput::needs_correction(
                        "ACTIVE_SEARCH_TASKS",
                        "仍有活动检索任务；先用 add_evidence 回收其压缩摘要",
                    );
                }
                let gaps = input
                    .get("gaps")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                state.gaps = gaps;
                if state.gaps.is_empty()
                    || state.round + 1 >= state.max_rounds
                    || state.status == "budget_exhausted"
                {
                    state.phase = "synthesize".into();
                    if state.status == "running" {
                        state.status = "ready_to_write".into();
                    }
                } else {
                    state.round += 1;
                    state.phase = "search".into();
                }
                match save_state(&ctx.project_root, &state) {
                    Ok(()) => ToolOutput::ok(state_summary(&state).to_string()),
                    Err(error) => ToolOutput::error(error),
                }
            }
            "add_finding" => {
                let mut state = match load_state(&ctx.project_root, topic) {
                    Ok(Some(state)) => state,
                    Ok(None) => {
                        return ToolOutput::error(format!("topic `{topic}` 尚未启动检索环"))
                    }
                    Err(error) => return ToolOutput::error(error),
                };
                if !matches!(state.status.as_str(), "ready_to_write" | "budget_exhausted") {
                    return ToolOutput::error("只有反思收敛或预算耗尽后才能写入 finding");
                }
                let title = input
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let conclusion = input
                    .get("conclusion")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let evidence_level = input
                    .get("evidence_level")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let evidence_depth = input
                    .get("evidence_depth")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let refs = input
                    .get("source_ids")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if title.is_empty()
                    || conclusion.is_empty()
                    || evidence_level.is_empty()
                    || evidence_depth.is_empty()
                    || refs.is_empty()
                {
                    return ToolOutput::needs_correction("FINDING_REQUIRES_SOURCE", "finding 必须提供 title、conclusion、evidence_level、evidence_depth 和 source_ids");
                }
                let source_tool = ResearchTrackerTool::new("source", "source", &SOURCES, None);
                for source_id in &refs {
                    let source = source_tool
                        .execute(json!({"action":"get","id":source_id,"topic":topic}), ctx)
                        .await;
                    if source.is_error {
                        return ToolOutput::needs_correction(
                            "SOURCE_NOT_FOUND",
                            format!("引用来源 {source_id} 无法读取: {}", source.content),
                        );
                    }
                    let entry: Value = match serde_json::from_str(&source.content) {
                        Ok(value) => value,
                        Err(error) => {
                            return ToolOutput::error(format!("invalid source entry: {error}"))
                        }
                    };
                    let source_url = entry
                        .get("fields")
                        .and_then(Value::as_array)
                        .and_then(|fields| {
                            fields.iter().find(|field| {
                                field["name"].as_str().is_some_and(|name| {
                                    name.eq_ignore_ascii_case("url") || name == "网址"
                                })
                            })
                        })
                        .and_then(|field| field["value"].as_str());
                    let Some(source_url) = source_url else {
                        return ToolOutput::needs_correction(
                            "SOURCE_URL_REQUIRED",
                            format!("引用来源 {source_id} 没有 URL 字段;需先登记可核验 URL 并经 webfetch 存证"),
                        );
                    };
                    match crate::web_refs::was_fetched(ctx, source_url) {
                        Ok(true) => {}
                        Ok(false) => {
                            return ToolOutput::needs_correction(
                                "SOURCE_NOT_WEBFETCHED",
                                format!("引用来源 {source_id} 尚未通过 webfetch 存证;请先对该 URL 调用 webfetch,再添加 finding"),
                            );
                        }
                        Err(error) => {
                            return ToolOutput::error(format!(
                                "cannot verify webfetch provenance: {error}"
                            ));
                        }
                    }
                }
                let source_tool = ResearchTrackerTool::new("source", "source", &SOURCES, None);
                for source_id in &refs {
                    let source = source_tool
                        .execute(json!({"action":"get","id":source_id,"topic":topic}), ctx)
                        .await;
                    if source.is_error {
                        return ToolOutput::needs_correction(
                            "SOURCE_NOT_FOUND",
                            format!("引用来源 {source_id} 无法读取: {}", source.content),
                        );
                    }
                    let entry: Value = match serde_json::from_str(&source.content) {
                        Ok(value) => value,
                        Err(error) => {
                            return ToolOutput::error(format!("invalid source entry: {error}"));
                        }
                    };
                    let source_url = entry
                        .get("fields")
                        .and_then(Value::as_array)
                        .and_then(|fields| {
                            fields.iter().find(|field| {
                                field["name"].as_str().is_some_and(|name| {
                                    name.eq_ignore_ascii_case("url") || name == "网址"
                                })
                            })
                        })
                        .and_then(|field| field["value"].as_str());
                    let Some(source_url) = source_url else {
                        return ToolOutput::needs_correction(
                            "SOURCE_URL_REQUIRED",
                            format!("引用来源 {source_id} 没有 URL 字段;需先登记 URL 并经 webfetch 存证"),
                        );
                    };
                    match crate::web_refs::was_fetched(ctx, source_url) {
                        Ok(true) => {}
                        Ok(false) => {
                            return ToolOutput::needs_correction(
                                "SOURCE_NOT_WEBFETCHED",
                                format!("引用来源 {source_id} 尚未通过 webfetch 存证;请先对该 URL 调用 webfetch,再添加 finding"),
                            );
                        }
                        Err(error) => {
                            return ToolOutput::error(format!(
                                "cannot verify webfetch provenance: {error}"
                            ));
                        }
                    }
                }
                let tracker =
                    ResearchTrackerTool::new("finding", "finding", &FINDINGS, Some(&SOURCES));
                let output = tracker.execute(json!({ "action": "add", "title": title, "topic": topic, "refs": refs, "fields": { "论断": conclusion, "等级": evidence_level, "证据深度": evidence_depth } }), ctx).await;
                if output.is_error {
                    return output;
                }
                state.findings.push(title.into());
                match save_state(&ctx.project_root, &state) {
                    Ok(()) => ToolOutput::ok(
                        json!({ "finding": output.content, "loop": state_summary(&state) })
                            .to_string(),
                    ),
                    Err(error) => ToolOutput::error(error),
                }
            }
            _ => ToolOutput::needs_correction(
                "INVALID_ACTION",
                "action 只能是 start/add_evidence/reflect/add_finding/resume",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research_plan::{save_plan, PlanBudget, PlanNode, PlanNodeStatus, ResearchPlan};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kz-research-loop-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn plan(topic: &str, status: PlanStatus) -> ResearchPlan {
        ResearchPlan {
            version: 1,
            topic: topic.into(),
            title: "测试计划".into(),
            status,
            open_questions: Vec::new(),
            nodes: vec![PlanNode {
                id: "one".into(),
                title: "一项".into(),
                objective: "验证".into(),
                status: PlanNodeStatus::Pending,
                depends_on: Vec::new(),
                children: Vec::new(),
            }],
            budget: PlanBudget {
                max_rounds: 2,
                max_tokens: 100,
                max_concurrency: 1,
            },
            revision: 1,
        }
    }

    #[tokio::test]
    async fn start_requires_approved_plan_and_persists_loop() {
        let project = root("approval");
        let ctx = ToolCtx::new(project.clone(), project.clone());
        save_plan(&project, &plan("loop-smoke", PlanStatus::AwaitingApproval)).unwrap();
        let tool = ResearchLoopTool;
        let blocked = tool
            .execute(json!({ "action": "start", "topic": "loop-smoke" }), &ctx)
            .await;
        assert_eq!(blocked.code, Some("PLAN_NOT_APPROVED"));
        save_plan(&project, &plan("loop-smoke", PlanStatus::Approved)).unwrap();
        let started = tool
            .execute(json!({ "action": "start", "topic": "loop-smoke" }), &ctx)
            .await;
        assert!(!started.is_error, "{}", started.content);
        assert!(project
            .join(".kanzei/research/loop-smoke/loop.json")
            .is_file());
        let invalid = tool
            .execute(
                json!({ "action": "add_evidence", "topic": "loop-smoke", "summary": "原始网页全文不应进入此字段" }),
                &ctx,
            )
            .await;
        assert_eq!(invalid.code, Some("MISSING_TASK_ID"));
        std::fs::remove_dir_all(project).ok();
    }

    #[tokio::test]
    async fn concurrency_gate_reflection_and_source_binding_are_mechanical() {
        let project = root("loop");
        let ctx = ToolCtx::new(project.clone(), project.clone());
        save_plan(&project, &plan("loop-smoke", PlanStatus::Approved)).unwrap();
        let tool = ResearchLoopTool;
        tool.execute(json!({ "action": "start", "topic": "loop-smoke" }), &ctx)
            .await;
        let first = tool
            .execute(
                json!({ "action": "begin_search", "topic": "loop-smoke" }),
                &ctx,
            )
            .await;
        assert!(!first.is_error, "{}", first.content);
        let task_id = serde_json::from_str::<Value>(&first.content).unwrap()["task_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(validate_external_task(&project, "loop-smoke", &task_id).is_ok());
        assert!(validate_external_task(&project, "loop-smoke", "r0-forged")
            .unwrap_err()
            .contains("RESEARCH_LOOP_TASK_REQUIRED"));
        let second = tool
            .execute(
                json!({ "action": "begin_search", "topic": "loop-smoke" }),
                &ctx,
            )
            .await;
        assert_eq!(second.code, Some("CONCURRENCY_LIMIT"));
        let evidence = tool
            .execute(
                json!({ "action": "add_evidence", "topic": "loop-smoke", "task_id": task_id, "summary": "正文支撑的压缩摘要", "relevance": 0.9, "source_ids": ["S-999"] }),
                &ctx,
            )
            .await;
        assert!(!evidence.is_error, "{}", evidence.content);
        assert!(validate_external_task(&project, "loop-smoke", &task_id).is_err());
        let reflected = tool
            .execute(
                json!({ "action": "reflect", "topic": "loop-smoke", "gaps": [] }),
                &ctx,
            )
            .await;
        assert!(!reflected.is_error, "{}", reflected.content);
        let source_tool = ResearchTrackerTool::new("source", "source", &SOURCES, None);
        let source_url = "https://example.org/not-yet-read";
        let source_output = source_tool
            .execute(
                json!({
                    "action":"add",
                    "title":"Unfetched test source",
                    "topic":"loop-smoke",
                    "fields":{"URL":source_url}
                }),
                &ctx,
            )
            .await;
        assert!(!source_output.is_error, "{}", source_output.content);
        let source_id = source_output
            .content
            .split_whitespace()
            .nth(1)
            .expect("source add returns S-ID")
            .to_string();
        crate::web_refs::remember(&ctx, source_url, "Unfetched test source").unwrap();
        let finding = tool
            .execute(
                json!({ "action": "add_finding", "topic": "loop-smoke", "title": "blocked before webfetch", "conclusion": "未经 webfetch 的来源不能落盘", "evidence_level": "V2", "evidence_depth": "正文级", "source_ids": [&source_id] }),
                &ctx,
            )
            .await;
        assert_eq!(finding.code, Some("SOURCE_NOT_WEBFETCHED"));
        crate::web_refs::mark_fetched(&ctx, source_url, "Unfetched test source").unwrap();
        let accepted = tool
            .execute(
                json!({ "action": "add_finding", "topic": "loop-smoke", "title": "verified after webfetch", "conclusion": "经 webfetch 存证的来源可绑定 finding", "evidence_level": "V2", "evidence_depth": "正文级", "source_ids": [&source_id] }),
                &ctx,
            )
            .await;
        assert!(!accepted.is_error, "{}", accepted.content);
        let stored = serde_json::from_str::<Value>(
            &std::fs::read_to_string(project.join(".kanzei/research/loop-smoke/loop.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(stored["findings"].as_array().unwrap().len(), 1);

        std::fs::remove_dir_all(project).ok();
    }

    #[test]
    fn network_tools_require_running_loop_task() {
        let project = root("network-gate");
        let state = ResearchLoopState {
            version: 1,
            topic: "loop-smoke".into(),
            status: "running".into(),
            phase: "search".into(),
            round: 0,
            max_rounds: 2,
            max_tokens: 100,
            max_concurrency: 1,
            tokens_used: 0,
            evidence: Vec::new(),
            gaps: Vec::new(),
            findings: Vec::new(),
            active_tasks: vec!["r0-t0".into()],
            next_task_id: 1,
        };
        save_state(&project, &state).unwrap();

        let not_started = validate_external_task(&project, "other-topic", "r0-t0").unwrap_err();
        assert!(not_started.contains("尚未启动检索环"), "{not_started}");

        let unknown_task = validate_external_task(&project, "loop-smoke", "r0-t9").unwrap_err();
        assert!(
            unknown_task.contains("RESEARCH_LOOP_TASK_REQUIRED"),
            "{unknown_task}"
        );

        validate_external_task(&project, "loop-smoke", "r0-t0").unwrap();
        std::fs::remove_dir_all(project).ok();
    }
}
