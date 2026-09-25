use super::*;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExperimentSpec {
    pub id: String,
    pub role: String,
    pub seed: u64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FullPlan {
    pub protocol: String,
    pub experiments: Vec<ExperimentSpec>,
    #[serde(default)]
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FullResult {
    pub experiment_id: String,
    pub result_id: String,
}

/// Check provenance both before execution and when binding a persisted result.
pub(super) fn validate_run_environment(
    state: &Workflow,
    run: &kanzei_core::ResearchRunRecord,
) -> Result<(), String> {
    if state.version < 2 {
        return Ok(());
    }
    let compute = state
        .compute
        .as_ref()
        .ok_or("先 prepare_compute，再启动实验")?;
    let execution: Value = serde_json::from_str(&run.execution_json).map_err(|e| e.to_string())?;
    if execution["kind"] != compute.kind
        || execution["workdir"].as_str() != Some(&compute.workdir)
        || execution["host"].as_str() != compute.host.as_deref()
        || execution["environment_id"].as_str() != compute.spec.environment_id.as_deref()
    {
        return Err(
            "实验执行位置与已准备环境不一致，请使用 compute 中的 kind/host/workdir/environment_id"
                .into(),
        );
    }
    if run.started_at < state.started_at || run.started_at < compute.prepared_at {
        return Err("实验早于本次流程或环境准备，请使用当前准备环境下的运行结果".into());
    }
    Ok(())
}

pub(super) fn validate_run(
    state: &Workflow,
    run: &kanzei_core::ResearchRunRecord,
    previous_runs: &[kanzei_core::ResearchRunRecord],
) -> Result<(), String> {
    validate_run_environment(state, run)?;
    if state.version < 2 {
        return Ok(());
    }
    if state.stage == Stage::RunFull {
        let params: Value = serde_json::from_str(&run.params_text)
            .map_err(|_| "完整实验 params_text 须为包含 experiment_id/role/seed 的 JSON")?;
        let spec = state
            .full_plan
            .as_ref()
            .and_then(|p| {
                p.experiments
                    .iter()
                    .find(|e| params["experiment_id"] == e.id)
            })
            .ok_or("实验未在完整实验矩阵登记")?;
        if params["role"] != spec.role || params["seed"].as_u64() != Some(spec.seed) {
            return Err("实验 role/seed 与协议不一致".into());
        }
        if state
            .full_results
            .iter()
            .any(|r| r.experiment_id == spec.id)
        {
            return Err("该实验项已经有成功结果，请回读，不要重复启动".into());
        }
        if let Some(result) = previous_runs
            .iter()
            .filter_map(|run| recoverable_full_result(state, run))
            .find(|result| result.experiment_id == spec.id)
        {
            return Err(format!(
                "实验项 {} 已有成功结果 {}，请先 record_full 登记，不要重复启动",
                spec.id, result.result_id
            ));
        }
    }
    Ok(())
}

fn recoverable_full_result(
    state: &Workflow,
    run: &kanzei_core::ResearchRunRecord,
) -> Option<FullResult> {
    let plan = state.full_plan.as_ref()?;
    if state.stage != Stage::RunFull
        || run.topic != state.topic
        || run.exploration_id != state.mvp.as_ref()?.exploration_id
        || run.status != "succeeded"
        || run.finished_at.is_none()
        || run.started_at < plan.created_at
        || !has_metric(state, run)
        || validate_run_environment(state, run).is_err()
    {
        return None;
    }
    let params: Value = serde_json::from_str(&run.params_text).ok()?;
    let spec = plan.experiments.iter().find(|spec| {
        params["experiment_id"] == spec.id
            && params["role"] == spec.role
            && params["seed"].as_u64() == Some(spec.seed)
    })?;
    if state
        .full_results
        .iter()
        .any(|result| result.experiment_id == spec.id || result.result_id == run.result_id)
    {
        return None;
    }
    Some(FullResult {
        experiment_id: spec.id.clone(),
        result_id: run.result_id.clone(),
    })
}

/// Rebuild recovery hints from existing run facts; never create another source of truth.
pub(super) fn recovery(root: &Path, state: &Workflow) -> Result<Value, String> {
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
        .map_err(|e| e.to_string())?;
    let runs = store
        .list_research_runs(&state.topic)
        .map_err(|e| e.to_string())?;
    let used = runs
        .iter()
        .filter(|run| run.started_at >= state.started_at)
        .count();
    let active: Vec<_> = runs.iter()
        .filter(|run| matches!(run.status.as_str(), "running" | "queued"))
        .map(|run| json!({"result_id":run.result_id,"exploration_id":run.exploration_id,"status":run.status}))
        .collect();
    let pending: Vec<_> = runs
        .iter()
        .filter_map(|run| recoverable_full_result(state, run))
        .collect();
    Ok(json!({
        "experiment_budget":{"limit":state.max_mvp_runs,"used":used,"remaining":(state.max_mvp_runs as usize).saturating_sub(used)},
        "active_runs":active,"pending_full_results":pending
    }))
}

pub(super) fn advance(
    root: &Path,
    state: &mut Workflow,
    action: &str,
    input: &Value,
) -> Result<(), String> {
    match action {
        "define_full" => {
            expect(state, Stage::PlanFull)?;
            if state.compute.is_none() {
                return Err("先 prepare_compute 验证实验环境".into());
            }
            let mut plan: FullPlan =
                serde_json::from_value(input["full_plan"].clone()).map_err(|e| e.to_string())?;
            artifact(root, &state.topic, &plan.protocol)?;
            if !(6..=100).contains(&plan.experiments.len()) {
                return Err("完整实验应包含 6–100 项，覆盖配对重复、消融和鲁棒性".into());
            }
            let mut ids = HashSet::new();
            let mut paired_seeds = HashSet::new();
            for e in &plan.experiments {
                if e.id.is_empty()
                    || !ids.insert(&e.id)
                    || e.description.trim().is_empty()
                    || !matches!(
                        e.role.as_str(),
                        "baseline" | "main" | "ablation" | "robustness"
                    )
                {
                    return Err(
                        "实验项需要唯一 id、description 和 baseline/main/ablation/robustness role"
                            .into(),
                    );
                }
                if matches!(e.role.as_str(), "baseline" | "main")
                    && !paired_seeds.insert((&e.role, e.seed))
                {
                    return Err("baseline/main 每个随机种子只能各出现一次，避免重复计入".into());
                }
            }
            for role in ["baseline", "main", "ablation", "robustness"] {
                if !plan.experiments.iter().any(|e| e.role == role) {
                    return Err(format!("实验矩阵缺少 {role}"));
                }
            }
            let seeds = |role: &str| {
                plan.experiments
                    .iter()
                    .filter(|e| e.role == role)
                    .map(|e| e.seed)
                    .collect::<HashSet<_>>()
            };
            if seeds("baseline").len() < 2 || seeds("baseline") != seeds("main") {
                return Err("baseline/main 需要相同的至少两个随机种子".into());
            }
            let reused = rounds::validate_reuse(root, state, &plan, input)?;
            let new_runs = plan.experiments.len() - reused.len();
            if new_runs == 0 {
                return Err("补充实验方案至少需要一个新实验项".into());
            }
            let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))
                .map_err(|e| e.to_string())?;
            let used = store
                .list_research_runs(&state.topic)
                .map_err(|e| e.to_string())?
                .iter()
                .filter(|r| r.started_at >= state.started_at)
                .count();
            if used + new_runs > state.max_mvp_runs as usize {
                return Err("剩余实验次数不足，请在概览调整预算后再提交完整实验方案".into());
            }
            plan.created_at = now_ms();
            state.full_plan = Some(plan);
            state.full_results = reused;
            state.stage = Stage::RunFull;
        }
        "record_full" => {
            expect(state, Stage::RunFull)?;
            let results: Vec<FullResult> =
                serde_json::from_value(input["full_results"].clone()).map_err(|e| e.to_string())?;
            if results.is_empty() {
                return Err("至少提交一条完整实验结果".into());
            }
            for result in results {
                let run = run_fact(root, state, &result.result_id)?;
                let plan = state.full_plan.as_ref().ok_or("缺少完整实验方案")?;
                let spec = plan
                    .experiments
                    .iter()
                    .find(|e| e.id == result.experiment_id)
                    .ok_or("结果未在实验矩阵中登记")?;
                let params: Value =
                    serde_json::from_str(&run.params_text).map_err(|e| e.to_string())?;
                if run.status != "succeeded"
                    || !has_metric(state, &run)
                    || run.started_at < plan.created_at
                    || params["experiment_id"] != spec.id
                    || params["role"] != spec.role
                    || params["seed"].as_u64() != Some(spec.seed)
                {
                    return Err("结果必须是本次矩阵对应项的成功运行，并带协议约定指标；失败项请新建运行重试".into());
                }
                if state.full_results.iter().any(|r| {
                    r.experiment_id == result.experiment_id || r.result_id == result.result_id
                }) {
                    return Err("实验项或 result_id 已登记，不能重复计入".into());
                }
                state.full_results.push(result);
            }
            if state.full_results.len() == state.full_plan.as_ref().unwrap().experiments.len() {
                let summary = results_summary(root, state)?;
                kanzei_base::atomic_file::write_atomic(
                    &topic_dir(root, &state.topic)?.join("results.json"),
                    &serde_json::to_string_pretty(&summary).unwrap(),
                )
                .map_err(|e| e.to_string())?;
                state.stage = Stage::Analyze;
            }
        }
        "submit_analysis" => {
            expect(state, Stage::Analyze)?;
            let file = required(input, "artifact")?;
            artifact(root, &state.topic, &file)?;
            let verdict = required(input, "verdict")?;
            if !matches!(verdict.as_str(), "supported" | "rejected" | "inconclusive") {
                return Err("无效结论".into());
            }
            let next = input["next"].as_str().unwrap_or("complete");
            if !matches!(next, "complete" | "iterate" | "pivot") {
                return Err("next 须为 complete/iterate/pivot".into());
            }
            let reason = if next != "complete" {
                Some(required(input, "reason")?)
            } else {
                None
            };
            if file != "report.md" {
                let dir = topic_dir(root, &state.topic)?;
                let report = std::fs::read_to_string(dir.join(&file)).map_err(|e| e.to_string())?;
                kanzei_base::atomic_file::write_atomic(&dir.join("report.md"), &report)
                    .map_err(|e| e.to_string())?;
            }
            state.analysis = Some(file);
            state.verdict = Some(verdict);
            state.stage = Stage::WritePaper;
            if let Some(reason) = reason {
                rounds::reopen(root, state, &reason, next == "pivot")?;
            }
        }
        "submit_paper" => paper::submit(root, state, input)?,
        "review_paper" => paper::review(root, state)?,
        _ => return Err("未知或非 agent 操作".into()),
    }
    Ok(())
}

pub(super) fn results_summary(root: &Path, state: &Workflow) -> Result<Value, String> {
    let metric = &state.mvp.as_ref().ok_or("缺少 MVP")?.metric;
    let mut rows = vec![];
    for result in &state.full_results {
        let run = run_fact(root, state, &result.result_id)?;
        let metrics: Value =
            serde_json::from_str(&run.metrics_last_json).map_err(|e| e.to_string())?;
        let spec = state
            .full_plan
            .as_ref()
            .unwrap()
            .experiments
            .iter()
            .find(|e| e.id == result.experiment_id)
            .unwrap();
        let reused_from = state
            .full_rounds
            .iter()
            .find(|round| round.results.iter().any(|r| r.result_id == run.result_id))
            .map(|round| round.round);
        rows.push(json!({"experiment_id":spec.id,"role":spec.role,"seed":spec.seed,"result_id":run.result_id,"metrics":metrics,"params":run.params_text,"environment":run.environment_snapshot_ref,"code_ref":run.code_ref_json,"reused_from_round":reused_from}));
    }
    let mut groups = serde_json::Map::new();
    for role in ["baseline", "main", "ablation", "robustness"] {
        let values: Vec<_> = rows
            .iter()
            .filter(|r| r["role"] == role)
            .filter_map(|r| r["metrics"][metric].as_f64())
            .collect();
        let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
        let sd = if values.len() > 1 {
            Some(
                (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                    / (values.len() - 1) as f64)
                    .sqrt(),
            )
        } else {
            None
        };
        groups.insert(
            role.into(),
            json!({"n":values.len(),"mean":mean,"sample_sd":sd}),
        );
    }
    Ok(
        json!({"topic":state.topic,"round":state.full_rounds.len()+1,"metric":metric,"rows":rows,"groups":groups,
        "previous_rounds":state.full_rounds.iter().map(|round| json!({"round":round.round,"results_file":format!("{}/results.json",round.artifact_root),"analysis":format!("{}/{}",round.artifact_root,round.analysis)})).collect::<Vec<_>>()}),
    )
}
