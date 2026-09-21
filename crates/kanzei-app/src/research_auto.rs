//! AUTO research uses topic checkpoints, with the existing loop's stop/retry primitives.
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use kanzei_harness::auto_run::{AutoRunAction, AutoRunCtx, WorkPriority};
use kanzei_tools::research_workflow::{self as workflow, Stage, Workflow};
use serde_json::{json, Value};

fn root(project_dir: &str) -> PathBuf {
    kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| project_dir.into())
}

#[tauri::command]
pub fn research_workflow_get(
    project_dir: String,
    topic: String,
) -> Result<Option<Workflow>, String> {
    workflow::load(&root(&project_dir), &topic)
}

#[tauri::command]
pub fn research_workflow_start(
    project_dir: String,
    topic: String,
    budget: kanzei_tools::research_plan::PlanBudget,
    max_mvp_runs: u32,
) -> Result<Workflow, String> {
    workflow::start(&root(&project_dir), &topic, budget, max_mvp_runs)
}

#[tauri::command]
pub fn research_workflow_update(
    project_dir: String,
    topic: String,
    revision: u64,
    action: String,
    direction: Option<String>,
    max_mvp_runs: Option<u32>,
) -> Result<Workflow, String> {
    if action == "budget" {
        return workflow::set_run_budget(
            &root(&project_dir),
            &topic,
            revision,
            max_mvp_runs.ok_or("缺少实验次数预算")?,
        );
    }
    workflow::user_action(
        &root(&project_dir),
        &topic,
        revision,
        &action,
        direction.as_deref(),
    )
}

pub(crate) fn progress_signature(root: &Path, topic: Option<&str>) -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let Some(topic) = topic else {
        return "unbound-research".into();
    };
    topic.hash(&mut hash);
    if let Ok(dir) = workflow::topic_dir(root, topic) {
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|p| matches!(p.extension().and_then(|s| s.to_str()), Some("json" | "md")))
            .collect();
        paths.sort();
        for path in paths {
            path.hash(&mut hash);
            std::fs::read(path).unwrap_or_default().hash(&mut hash);
        }
    }
    format!("research-{hash:016x}", hash = hash.finish())
}

pub(crate) fn decide(
    ctrl: &mut crate::auto_run::AutoRunController,
    mut ctx: AutoRunCtx<'_>,
    root: &Path,
    topic: Option<&str>,
) -> Value {
    if !ctrl.enabled || ctx.halted {
        return json!({"type":"NoContinue"});
    }
    let loaded = topic
        .ok_or_else(|| "请先绑定研究课题".to_string())
        .and_then(|t| workflow::load(root, t));
    let state = match loaded {
        Ok(Some(state)) => state,
        other => {
            ctrl.state.reset();
            return json!({"type":"Stop", "reason":"ResearchWaiting", "message": match other {
                Err(error) => error, _ => "请在课题概览启动 AUTO research".into()
            }});
        }
    };
    if !state.runnable() {
        ctrl.state.reset();
        let reason = if state.stage == Stage::Completed {
            "ResearchCompleted"
        } else {
            "ResearchWaiting"
        };
        let message = if state.paused {
            "研究已暂停".into()
        } else if let Some(reason) = &state.waiting_reason {
            reason.clone()
        } else {
            state.stage.label().into()
        };
        return json!({"type":"Stop", "reason":reason, "message":message});
    }
    ctx.auto_allowed = true;
    ctx.intensity = kanzei_harness::HarnessIntensity::Paired;
    ctx.goal_active = true;
    ctx.model_declared_done = false;
    ctx.verify_every_n = 0;
    let action = crate::auto_run::decide_auto_run(ctrl, ctx);
    let mut payload = crate::auto_run::serialize_action(action, WorkPriority::DefectFirst);
    if matches!(action, AutoRunAction::Continue | AutoRunAction::GoalPending) {
        payload["type"] = json!("Continue");
        payload["prompt"] = json!(state.guidance());
    }
    payload["rounds"] = json!(ctrl.state.rounds);
    payload
}

#[cfg(test)]
mod tests;
