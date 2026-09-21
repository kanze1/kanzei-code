//! R-363: topic-level AUTO research checkpoints; experiments retain their existing facts.
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::research_plan::{
    load_plan, save_plan, PlanBudget, PlanNode, PlanNodeStatus, PlanStatus, ResearchPlan,
};

mod tool;
pub use tool::{ResearchWorkflowContext, ResearchWorkflowTool};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Survey,
    Map,
    ChooseDirection,
    DesignMvp,
    Prepare,
    RunMvp,
    Interpret,
    Completed,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Survey => "文献调研",
            Self::Map => "研究地图",
            Self::ChooseDirection => "等待选题",
            Self::DesignMvp => "MVP 方案",
            Self::Prepare => "实验准备",
            Self::RunMvp => "MVP 实验",
            Self::Interpret => "结果解读",
            Self::Completed => "MVP 已完成",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Direction {
    pub id: String,
    pub title: String,
    pub question: String,
    pub rationale: String,
    pub uncertainty: String,
    pub cost: String,
    pub validation: String,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Mvp {
    pub question: String,
    pub hypothesis: String,
    pub baseline: String,
    /// Exact metric name emitted by the experiment callback (for example accuracy).
    pub metric: String,
    pub success: String,
    pub failure: String,
    pub exploration_id: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub version: u32,
    pub topic: String,
    pub revision: u64,
    pub stage: Stage,
    pub paused: bool,
    pub waiting_reason: Option<String>,
    pub started_at: i64,
    pub max_mvp_runs: u32,
    pub budget: PlanBudget,
    pub survey: Option<String>,
    pub map: Option<String>,
    pub directions: Vec<Direction>,
    pub selected_direction: Option<String>,
    pub mvp: Option<Mvp>,
    pub baseline_result: Option<String>,
    pub result_ids: Vec<String>,
    pub interpretation: Option<String>,
    pub verdict: Option<String>,
    pub history: Vec<Value>,
}

impl Workflow {
    pub fn runnable(&self) -> bool {
        !self.paused
            && self.waiting_reason.is_none()
            && !matches!(self.stage, Stage::ChooseDirection | Stage::Completed)
    }

    pub fn guidance(&self) -> String {
        let next = match self.stage {
            Stage::Survey => "先 research_loop start/resume，检索与阅读核心一手资料，登记 source/finding，写综述后 research_workflow survey_complete。",
            Stage::Map => "综合已有证据，写研究地图，候选方向逐项填写价值、未知、成本、最小验证与 source_ids，调用 publish_map 后等待用户选择。",
            Stage::ChooseDirection => "等待用户在课题概览选择方向；不得自行选择或启动实验。",
            Stage::DesignMvp => "围绕 selected_direction 定义可证伪问题、基线、指标和成功/失败判据，先写协议和 exploration Markdown，再调用 define_mvp。",
            Stage::Prepare => "编写课题内实验代码并准备依赖和数据，复用现有环境，使用 research_runner 跑基线。成功后 environment_ready 绑定基线 result_id 和准备记录。缺资源用 request_input 写明缺口。",
            Stage::RunMvp => "读取基线和协议，用 research_runner 执行 MVP，保留日志、环境和指标。完成后 record_mvp 绑定实际 result_ids；先回读已存在的运行，不能因恢复而重复启动。",
            Stage::Interpret => "检查结果和失败原因，写明观察、支持范围、其他解释和下一步。interpret 的 verdict 为 supported/rejected/inconclusive，next 为 complete/iterate/pivot；complete 只表示本次 MVP 研究收敛，不宣称论文或完整实验完成。",
            Stage::Completed => "本次 MVP 研究已收敛。向用户交付结论、证据和后续完整实验建议。",
        };
        format!("AUTO research 当前阶段：{}，revision={}。{}\n{}\n恢复前先调用 research_workflow get；每次状态变更传当前 revision。不要操作 dev backlog。工作流数据：{}",
            self.stage.label(), self.revision,
            if self.runnable() { "按已授权范围继续。" } else { "当前应停止自动推进，等待用户。" },
            next, serde_json::to_string(&json!({"topic": self.topic, "selected_direction": self.selected_direction,
                "directions": self.directions, "mvp": self.mvp, "baseline_result": self.baseline_result,
                "result_ids": self.result_ids, "waiting_reason": self.waiting_reason, "max_mvp_runs": self.max_mvp_runs})).unwrap_or_default())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn topic_dir(root: &Path, topic: &str) -> Result<PathBuf, String> {
    crate::docstore::DocStore::validate_topic(topic).map_err(|e| e.to_string())?;
    let research = root
        .join(".kanzei/research")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let dir = research
        .join(topic)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !dir.is_dir() || !dir.starts_with(&research) {
        return Err("课题目录越界".into());
    }
    Ok(dir)
}

fn path(root: &Path, topic: &str) -> Result<PathBuf, String> {
    Ok(topic_dir(root, topic)?.join("workflow.json"))
}

pub fn load(root: &Path, topic: &str) -> Result<Option<Workflow>, String> {
    let path = path(root, topic)?;
    let _lock = kanzei_base::atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let state: Workflow =
                serde_json::from_str(&text).map_err(|e| format!("研究流程损坏: {e}"))?;
            if state.version != 1 || state.topic != topic {
                return Err("研究流程版本或课题不匹配".into());
            }
            Ok(Some(state))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn save(root: &Path, state: &Workflow) -> Result<(), String> {
    kanzei_base::atomic_file::write_atomic(
        &path(root, &state.topic)?,
        &serde_json::to_string_pretty(state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn start(
    root: &Path,
    topic: &str,
    budget: PlanBudget,
    max_mvp_runs: u32,
) -> Result<Workflow, String> {
    let file = path(root, topic)?;
    let _lock = kanzei_base::atomic_file::lock_exclusive(&file).map_err(|e| e.to_string())?;
    if load(root, topic)?.is_some() {
        return Err("AUTO research 已启动，请恢复已有流程".into());
    }
    if !(2..=100).contains(&max_mvp_runs) {
        return Err("实验次数预算须为 2–100（包含基线与失败运行）".into());
    }
    if budget.max_rounds == 0 || budget.max_tokens == 0 || budget.max_concurrency == 0 {
        return Err("检索预算必须大于零".into());
    }
    if let Some(plan) = load_plan(root, topic)? {
        if !matches!(
            plan.status,
            PlanStatus::Approved | PlanStatus::Running | PlanStatus::Completed
        ) {
            return Err("当前课题已有未批准计划，请先在研究计划页批准".into());
        }
    } else {
        save_plan(root, &ResearchPlan {
            version: 1, topic: topic.into(), title: format!("{topic} 文献调研与研究地图"),
            status: PlanStatus::Approved, open_questions: vec![], budget: budget.clone(), revision: 1,
            nodes: vec![PlanNode { id: "survey".into(), title: "调研与研究地图".into(),
                objective: "阅读核心一手资料，比较已有路线，给出带出处、成本与验证方式的候选方向，等待用户选择".into(),
                status: PlanNodeStatus::Ready, depends_on: vec![], children: vec![] }],
        })?;
    }
    let state = Workflow {
        version: 1,
        topic: topic.into(),
        revision: 1,
        stage: Stage::Survey,
        paused: false,
        waiting_reason: None,
        started_at: now_ms(),
        max_mvp_runs,
        budget,
        survey: None,
        map: None,
        directions: vec![],
        selected_direction: None,
        mvp: None,
        baseline_result: None,
        result_ids: vec![],
        interpretation: None,
        verdict: None,
        history: vec![json!({"action": "start", "actor": "user", "at": now_ms()})],
    };
    save(root, &state)?;
    Ok(state)
}

fn update(
    root: &Path,
    topic: &str,
    revision: u64,
    action: &str,
    actor: &str,
    change: impl FnOnce(&mut Workflow) -> Result<(), String>,
) -> Result<Workflow, String> {
    let file = path(root, topic)?;
    let _lock = kanzei_base::atomic_file::lock_exclusive(&file).map_err(|e| e.to_string())?;
    let mut state = load(root, topic)?.ok_or("尚未启动 AUTO research")?;
    if state.revision != revision {
        return Err("研究流程已更新，请刷新后重试".into());
    }
    let previous = json!({"stage": state.stage, "selected_direction": state.selected_direction,
        "mvp": state.mvp, "baseline_result": state.baseline_result, "result_ids": state.result_ids,
        "interpretation": state.interpretation, "verdict": state.verdict});
    change(&mut state)?;
    state.revision += 1;
    state.history.push(
        json!({"action": action, "actor": actor, "at": now_ms(), "previous": previous,
        "stage": state.stage, "revision": state.revision}),
    );
    save(root, &state)?;
    Ok(state)
}

/// Only user-facing commands call this; the model tool deliberately omits these actions.
pub fn user_action(
    root: &Path,
    topic: &str,
    revision: u64,
    action: &str,
    direction: Option<&str>,
) -> Result<Workflow, String> {
    update(root, topic, revision, action, "user", |state| {
        match action {
            "select" => {
                if state.stage != Stage::ChooseDirection {
                    return Err("当前不在选题阶段".into());
                }
                let id = direction.ok_or("请选择方向")?;
                if !state.directions.iter().any(|d| d.id == id) {
                    return Err("候选方向不存在".into());
                }
                state.selected_direction = Some(id.into());
                state.stage = Stage::DesignMvp;
                state.waiting_reason = None;
                state.paused = false;
            }
            "pause" => state.paused = true,
            "resume" => {
                if matches!(state.stage, Stage::ChooseDirection | Stage::Completed) {
                    return Err("请先选题；已完成流程不能恢复".into());
                }
                state.paused = false;
                state.waiting_reason = None;
            }
            _ => return Err("未知用户操作".into()),
        }
        Ok(())
    })
}

pub fn set_run_budget(
    root: &Path,
    topic: &str,
    revision: u64,
    max_mvp_runs: u32,
) -> Result<Workflow, String> {
    update(root, topic, revision, "budget", "user", |state| {
        if !(2..=100).contains(&max_mvp_runs) {
            return Err("实验次数预算须为 2–100".into());
        }
        state.max_mvp_runs = max_mvp_runs;
        Ok(())
    })
}

fn required(input: &Value, key: &str) -> Result<String, String> {
    input[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("缺少非空字段 {key}"))
}

fn artifact(root: &Path, topic: &str, relative: &str) -> Result<(), String> {
    let rel = Path::new(relative);
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("工件路径必须相对于当前课题，且不能包含 ..".into());
    }
    let dir = topic_dir(root, topic)?;
    let file = dir
        .join(rel)
        .canonicalize()
        .map_err(|_| format!("工件不存在: {relative}"))?;
    if !file.starts_with(&dir)
        || !file.is_file()
        || file.metadata().map_err(|e| e.to_string())?.len() == 0
    {
        return Err(format!("工件为空或越界: {relative}"));
    }
    Ok(())
}

fn expect(state: &Workflow, stage: Stage) -> Result<(), String> {
    if state.stage != stage {
        return Err(format!(
            "当前阶段是 {}，不能跳到 {}",
            state.stage.label(),
            stage.label()
        ));
    }
    Ok(())
}

fn run_fact(
    root: &Path,
    state: &Workflow,
    id: &str,
) -> Result<kanzei_core::ResearchRunRecord, String> {
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
        .map_err(|e| e.to_string())?;
    let run = store
        .get_research_run(id)
        .map_err(|e| e.to_string())?
        .ok_or("实验运行不存在")?;
    let mvp = state.mvp.as_ref().ok_or("尚无 MVP 方案")?;
    if run.topic != state.topic || run.exploration_id != mvp.exploration_id {
        return Err("实验不属于当前课题和 MVP 探索".into());
    }
    if !matches!(run.status.as_str(), "succeeded" | "failed" | "cancelled")
        || run.finished_at.is_none()
    {
        return Err("实验尚未结束，请回读运行状态".into());
    }
    Ok(run)
}

fn has_metric(state: &Workflow, run: &kanzei_core::ResearchRunRecord) -> bool {
    let Some(mvp) = &state.mvp else {
        return false;
    };
    serde_json::from_str::<Value>(&run.metrics_last_json)
        .ok()
        .and_then(|metrics| metrics[&mvp.metric].as_f64())
        .is_some_and(f64::is_finite)
}

pub fn advance(
    root: &Path,
    topic: &str,
    revision: u64,
    action: &str,
    input: &Value,
) -> Result<Workflow, String> {
    update(root, topic, revision, action, "agent", |state| {
        if !state.runnable() {
            return Err("当前流程已暂停、等待用户或已完成".into());
        }
        match action {
            "survey_complete" => {
                expect(state, Stage::Survey)?;
                let file = required(input, "artifact")?;
                artifact(root, topic, &file)?;
                state.survey = Some(file);
                state.stage = Stage::Map;
            }
            "publish_map" => {
                expect(state, Stage::Map)?;
                let file = required(input, "artifact")?;
                artifact(root, topic, &file)?;
                let directions: Vec<Direction> =
                    serde_json::from_value(input["directions"].clone())
                        .map_err(|e| e.to_string())?;
                if directions.is_empty() {
                    return Err("研究地图至少需要一个候选方向".into());
                }
                let sources =
                    crate::docstore::DocStore::open_topic(root, &crate::docstore::SOURCES, topic)
                        .and_then(|store| store.load())
                        .map_err(|e| e.to_string())?;
                let mut ids = std::collections::HashSet::new();
                for d in &directions {
                    for value in [
                        &d.id,
                        &d.title,
                        &d.question,
                        &d.rationale,
                        &d.uncertainty,
                        &d.cost,
                        &d.validation,
                    ] {
                        if value.trim().is_empty() {
                            return Err("候选方向的所有字段必须非空".into());
                        }
                    }
                    if !ids.insert(&d.id) {
                        return Err("候选方向 id 重复".into());
                    }
                    if d.source_ids.is_empty()
                        || d.source_ids
                            .iter()
                            .any(|id| !sources.iter().any(|s| &s.id == id))
                    {
                        return Err(format!("方向 {} 必须引用当前课题已登记来源", d.id));
                    }
                }
                state.map = Some(file);
                state.directions = directions;
                state.stage = Stage::ChooseDirection;
            }
            "define_mvp" => {
                expect(state, Stage::DesignMvp)?;
                let mvp: Mvp =
                    serde_json::from_value(input["mvp"].clone()).map_err(|e| e.to_string())?;
                for value in [
                    &mvp.question,
                    &mvp.hypothesis,
                    &mvp.baseline,
                    &mvp.metric,
                    &mvp.success,
                    &mvp.failure,
                ] {
                    if value.trim().is_empty() {
                        return Err("MVP 必须包含问题、假设、基线、指标及成功/失败判据".into());
                    }
                }
                artifact(root, topic, &mvp.protocol)?;
                let model =
                    kanzei_core::load_research_topic(root, topic).map_err(|e| e.to_string())?;
                if !model
                    .explorations
                    .iter()
                    .any(|e| e.frontmatter.id == mvp.exploration_id)
                {
                    return Err("请先创建有效的 exploration Markdown".into());
                }
                state.mvp = Some(mvp);
                state.baseline_result = None;
                state.result_ids.clear();
                state.stage = Stage::Prepare;
            }
            "environment_ready" => {
                expect(state, Stage::Prepare)?;
                artifact(root, topic, &required(input, "artifact")?)?;
                let id = required(input, "baseline_result")?;
                let baseline = run_fact(root, state, &id)?;
                if baseline.status != "succeeded" {
                    return Err("基线必须成功运行".into());
                }
                if !has_metric(state, &baseline) {
                    return Err("基线缺少 MVP 约定的指标回调，请核对 metric 名称".into());
                }
                state.baseline_result = Some(id);
                state.stage = Stage::RunMvp;
            }
            "record_mvp" => {
                expect(state, Stage::RunMvp)?;
                let ids: Vec<String> = serde_json::from_value(input["result_ids"].clone())
                    .map_err(|e| e.to_string())?;
                if ids.is_empty() {
                    return Err("至少绑定一条实际 MVP 实验结果".into());
                }
                for id in &ids {
                    if state.baseline_result.as_ref() == Some(id) {
                        return Err("基线不能同时作为 MVP 结果".into());
                    }
                    run_fact(root, state, id)?;
                }
                state.result_ids = ids;
                state.stage = Stage::Interpret;
            }
            "interpret" => {
                expect(state, Stage::Interpret)?;
                let file = required(input, "artifact")?;
                artifact(root, topic, &file)?;
                let verdict = required(input, "verdict")?;
                if !matches!(verdict.as_str(), "supported" | "rejected" | "inconclusive") {
                    return Err("verdict 须为 supported/rejected/inconclusive".into());
                }
                for id in &state.result_ids {
                    let run = run_fact(root, state, id)?;
                    if (run.status != "succeeded" || !has_metric(state, &run))
                        && verdict != "inconclusive"
                    {
                        return Err(
                            "失败或缺指标的运行只能支持不确定结论，不能据此支持或否定假设".into(),
                        );
                    }
                }
                state.interpretation = Some(file);
                state.verdict = Some(verdict);
                state.stage = match required(input, "next")?.as_str() {
                    "complete" => Stage::Completed,
                    "iterate" => Stage::DesignMvp,
                    "pivot" => Stage::ChooseDirection,
                    _ => return Err("next 须为 complete/iterate/pivot".into()),
                };
            }
            "request_input" => state.waiting_reason = Some(required(input, "reason")?),
            _ => return Err("未知或非 agent 操作".into()),
        }
        Ok(())
    })
}

/// The runner calls this before creating any process. Missing workflow preserves manual research.
pub fn check_run(root: &Path, topic: &str, exploration_id: &str) -> Result<(), String> {
    crate::docstore::DocStore::validate_topic(topic).map_err(|e| e.to_string())?;
    if !root
        .join(".kanzei/research")
        .join(topic)
        .join("workflow.json")
        .exists()
    {
        return Ok(());
    }
    let Some(state) = load(root, topic)? else {
        return Ok(());
    };
    if !state.runnable() || !matches!(state.stage, Stage::Prepare | Stage::RunMvp) {
        return Err("当前研究阶段不能启动实验".into());
    }
    if state.mvp.as_ref().map(|m| m.exploration_id.as_str()) != Some(exploration_id) {
        return Err("实验必须属于当前 MVP 探索".into());
    }
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
        .map_err(|e| e.to_string())?;
    let runs = store.list_research_runs(topic).map_err(|e| e.to_string())?;
    if runs
        .iter()
        .filter(|r| r.started_at >= state.started_at)
        .count()
        >= state.max_mvp_runs as usize
    {
        return Err("已达到本课题实验次数预算，请解读已有结果或等待用户调整".into());
    }
    if runs
        .iter()
        .any(|r| matches!(r.status.as_str(), "running" | "queued"))
    {
        return Err("当前课题已有运行，请先回读或取消，不能重复启动".into());
    }
    Ok(())
}

/// Serialize admission with workflow edits and other sessions before the process is spawned.
pub fn validate_run_id(
    root: &Path,
    topic: &str,
    exploration: &str,
    result: &str,
) -> Result<(), String> {
    if !root
        .join(".kanzei/research")
        .join(topic)
        .join("workflow.json")
        .exists()
    {
        return Ok(());
    }
    let suffix = result
        .strip_prefix(&format!("{exploration}-"))
        .ok_or("实验编号必须为 E-<n>-<nn>，例如 E-001-01")?;
    if suffix.is_empty() || !suffix.bytes().all(|c| c.is_ascii_digit()) {
        return Err("实验编号后缀必须为数字，不能包含路径".into());
    }
    Ok(())
}

/// Admission and persistence share the canonical workflow lock.
pub fn record_run_start(root: &Path, run: &kanzei_core::ResearchRunRecord) -> Result<(), String> {
    // Use the same canonical spelling as load/update: Windows extended-path aliases
    // otherwise acquire a second non-reentrant lock for the same physical file.
    let file = path(root, &run.topic)?;
    let guard = if file.exists() {
        Some(kanzei_base::atomic_file::lock_exclusive(&file).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
        .map_err(|e| e.to_string())?;
    if guard.is_some() {
        check_run(root, &run.topic, &run.exploration_id)?;
        if store
            .get_research_run(&run.result_id)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err("实验 result_id 已存在，请回读原记录；新实验使用新编号".into());
        }
    }
    store.upsert_research_run(run).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
