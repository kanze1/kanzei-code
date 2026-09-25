//! Preserve completed evidence before revising a full experiment matrix.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullRound {
    pub round: usize,
    pub artifact_root: String,
    pub reason: String,
    pub plan: FullPlan,
    pub results: Vec<FullResult>,
    pub mvp: Mvp,
    pub compute: Option<compute::PreparedCompute>,
    pub analysis: String,
    pub verdict: String,
    pub paper: Option<paper::PaperRecord>,
    /// Paths are relative to artifact_root; values bind the preserved bytes.
    pub artifacts: BTreeMap<String, String>,
}

pub(super) fn reopen(
    root: &Path,
    state: &mut Workflow,
    reason: &str,
    pivot: bool,
) -> Result<(), String> {
    if !matches!(
        state.stage,
        Stage::WritePaper | Stage::ReviewPaper | Stage::CompilePaper | Stage::Completed
    ) {
        return Err("请先提交完整实验分析，再补充实验".into());
    }
    let plan = state.full_plan.clone().ok_or("缺少完整实验方案")?;
    if state.full_results.len() != plan.experiments.len() {
        return Err("本轮完整实验尚未收齐".into());
    }
    let analysis = state.analysis.clone().ok_or("缺少完整实验分析")?;
    let mvp = state.mvp.clone().ok_or("缺少 MVP")?;
    let dir = topic_dir(root, &state.topic)?;
    let mut files = BTreeMap::new();
    for file in [
        &plan.protocol,
        &mvp.protocol,
        &analysis,
        "results.json",
        "sources.md",
    ] {
        artifact(root, &state.topic, file)?;
        files.insert(
            file.to_string(),
            std::fs::read(dir.join(file)).map_err(|e| e.to_string())?,
        );
    }
    if let Some(paper) = &state.paper {
        for (path, bytes) in paper::inputs(root, state)? {
            let relative = path
                .strip_prefix(&dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(relative, bytes);
        }
        for file in paper.pdf.iter().chain(paper.manifest.iter()) {
            artifact(root, &state.topic, file)?;
            files.insert(
                file.clone(),
                std::fs::read(dir.join(file)).map_err(|e| e.to_string())?,
            );
        }
    }
    let artifact_root = format!(
        "rounds/full-{:03}-r{}",
        state.full_rounds.len() + 1,
        state.revision
    );
    let mut hashes = BTreeMap::new();
    for (relative, bytes) in files {
        let path = dir.join(&artifact_root).join(&relative);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        if !path
            .parent()
            .unwrap()
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&dir)
        {
            return Err("历史实验快照路径越界".into());
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => file.write_all(&bytes).map_err(|e| e.to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if std::fs::read(&path).map_err(|e| e.to_string())? != bytes {
                    return Err(format!(
                        "历史快照已存在且内容不同，不能覆盖: {}",
                        path.display()
                    ));
                }
            }
            Err(error) => return Err(error.to_string()),
        }
        hashes.insert(relative, format!("{:x}", Sha256::digest(&bytes)));
    }
    state.full_rounds.push(FullRound {
        round: state.full_rounds.len() + 1,
        artifact_root,
        reason: reason.into(),
        plan,
        results: state.full_results.clone(),
        mvp,
        compute: state.compute.clone(),
        analysis,
        verdict: state.verdict.clone().ok_or("缺少分析结论")?,
        paper: state.paper.take(),
        artifacts: hashes,
    });
    state.full_plan = None;
    state.full_results.clear();
    state.analysis = None;
    state.verdict = None;
    state.stage = if pivot {
        Stage::ChooseDirection
    } else {
        Stage::PlanFull
    };
    if pivot {
        state.selected_direction = None;
        state.mvp = None;
        state.baseline_result = None;
        state.result_ids.clear();
        state.interpretation = None;
    }
    Ok(())
}

pub(super) fn validate_reuse(
    root: &Path,
    state: &Workflow,
    plan: &FullPlan,
    input: &Value,
) -> Result<Vec<FullResult>, String> {
    let results: Vec<FullResult> = match input.get("reuse_results") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string())?,
        None => Vec::new(),
    };
    let mut experiments = HashSet::new();
    let mut ids = HashSet::new();
    for result in &results {
        if !experiments.insert(&result.experiment_id) || !ids.insert(&result.result_id) {
            return Err("复用的实验项或结果编号重复".into());
        }
        let spec = plan
            .experiments
            .iter()
            .find(|spec| spec.id == result.experiment_id)
            .ok_or("复用项不在新实验矩阵中")?;
        let round = state
            .full_rounds
            .iter()
            .rev()
            .find(|round| {
                round.results.iter().any(|old| {
                    old.experiment_id == result.experiment_id && old.result_id == result.result_id
                })
            })
            .ok_or("只能显式复用已归档完整实验中的成功结果")?;
        if !round.plan.experiments.iter().any(|old| old == spec)
            || state.mvp.as_ref().is_none_or(|mvp| {
                mvp.metric != round.mvp.metric || mvp.exploration_id != round.mvp.exploration_id
            })
        {
            return Err("实验定义或指标已变化，请新建运行，不能复用旧结果".into());
        }
        let run = run_fact(root, state, &result.result_id)?;
        let params: Value = serde_json::from_str(&run.params_text).map_err(|e| e.to_string())?;
        if run.status != "succeeded"
            || !has_metric(state, &run)
            || params["experiment_id"] != spec.id
            || params["role"] != spec.role
            || params["seed"].as_u64() != Some(spec.seed)
        {
            return Err("复用结果必须成功且与实验定义及指标一致".into());
        }
    }
    Ok(results)
}
