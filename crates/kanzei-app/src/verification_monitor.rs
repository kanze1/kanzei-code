//! Reawaken a work line after a background verification job reaches a terminal result.
//!
//! The verifier is a separate child process. This monitor joins its job file to the
//! owning Work Unit, maps `claimed_by` (branch identity) back to the app process, and
//! admits one stable, explicitly system-labeled queue input per job.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::Manager;

use crate::{AppState, ProcessInfo};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROJECT_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn terminal_job_status(status: &str) -> bool {
    matches!(
        status,
        "passed" | "failed" | "stale" | "timed_out" | "interrupted" | "output_limit"
    )
}

fn should_enqueue_terminal_result(
    previous: Option<&str>,
    current: &str,
    job_updated_at: u64,
    monitor_started_at: u64,
) -> bool {
    if !terminal_job_status(current) {
        return false;
    }
    previous.is_some_and(|previous| !terminal_job_status(previous))
        || job_updated_at >= monitor_started_at
}

fn owning_process<'a>(
    unit: &kanzei_core::WorkProjection,
    processes: &'a [ProcessInfo],
    session_id: Option<&str>,
    store: &kanzei_core::SessionStore,
) -> Option<&'a ProcessInfo> {
    if let Some(session_id) = session_id {
        return processes.iter().find(|p| p.session_id == session_id);
    }
    // Legacy jobs have no recipient. Recover only an unambiguous matching owner.
    let mut candidates = processes
        .iter()
        .filter(|process| match unit.claimed_by.as_deref() {
            Some(branch) => process.branch.as_deref() == Some(branch),
            None => {
                process.worktree_path.is_none()
                    && store
                        .latest_work_claim(&process.session_id, None)
                        .ok()
                        .flatten()
                        .as_deref()
                        == Some(unit.requirement_id.as_str())
            }
        });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn wake_input_id(job_id: &str) -> String {
    format!("verification-wake-{job_id}")
}

fn has_pending_wake_input(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    input_id: &str,
) -> Result<bool, String> {
    store
        .list_pending_inputs(session_id)
        .map(|inputs| inputs.iter().any(|input| input.input_id == input_id))
        .map_err(|error| error.to_string())
}

fn admit_wake_input(
    store: &kanzei_core::SessionStore,
    project_root: &Path,
    session_id: &str,
    job_id: &str,
    unit_id: &str,
    status: &str,
) -> Result<bool, String> {
    let input_id = wake_input_id(job_id);
    store
        .create_session(session_id, &project_root.display().to_string(), None)
        .map_err(|error| error.to_string())?;
    if let Some(existing) = store
        .input_status(&input_id)
        .map_err(|error| error.to_string())?
    {
        if existing != "pending" {
            return Ok(false);
        }
        let belongs_to_session = store
            .list_pending_inputs(session_id)
            .map_err(|error| error.to_string())?
            .iter()
            .any(|input| input.input_id == input_id);
        return belongs_to_session
            .then_some(true)
            .ok_or_else(|| "verification wake input belongs to a different session".into());
    }

    let prompt = format!(
        "Kanzei 系统通知（非用户指令）：后台验证任务 {job_id} 对工作单元 {unit_id} 已结束，结果为 {status}。请读取当前工作单元状态和验证日志，再按现有项目规则继续；不得把失败或过期结果当成通过，也不得超出已有授权。"
    );
    let input = store
        .admit_input_with_source(
            session_id,
            &input_id,
            &prompt,
            kanzei_core::Delivery::Queue,
            Some("verification_monitor"),
        )
        .map_err(|error| error.to_string())?;
    if input.session_id != session_id || input.prompt != prompt {
        return Err("verification wake input identity collision".into());
    }
    Ok(true)
}

pub(crate) fn start(app: tauri::AppHandle, window: tauri::Window) {
    tauri::async_runtime::spawn(async move {
        let monitor_started_at = now_ms();
        let mut observed_statuses = HashMap::<(PathBuf, String), String>::new();
        let mut project_paths = crate::prefs::load_prefs().projects;
        let mut projects_refreshed = Instant::now();

        loop {
            if projects_refreshed.elapsed() >= PROJECT_REFRESH_INTERVAL {
                project_paths = crate::prefs::load_prefs().projects;
                projects_refreshed = Instant::now();
            }

            for project_dir in &project_paths {
                let root = crate::normalized_project_root(Path::new(project_dir));
                if !root.is_dir() {
                    continue;
                }
                if let Err(error) = kanzei_tools::verification::recover(&root) {
                    tracing::warn!(project = %root.display(), %error, "verification recovery failed");
                    continue;
                }
                let state = app.state::<AppState>();
                let processes = match crate::processes::process_list(state, project_dir.clone()) {
                    Ok(processes) => processes,
                    Err(error) => {
                        tracing::warn!(project = %root.display(), %error, "verification wake process lookup failed");
                        continue;
                    }
                };
                let jobs = match kanzei_tools::verification::list_jobs(&root) {
                    Ok(jobs) => jobs,
                    Err(error) => {
                        tracing::warn!(project = %root.display(), %error, "verification job listing failed");
                        continue;
                    }
                };
                let store = match kanzei_core::SessionStore::open(&kanzei_core::project_state_path(
                    &root,
                )) {
                    Ok(store) => store,
                    Err(error) => {
                        tracing::warn!(project = %root.display(), %error, "verification wake store open failed");
                        continue;
                    }
                };

                for job in jobs {
                    let key = (root.clone(), job.id.clone());
                    let previous = observed_statuses.insert(key, job.status.clone());
                    let should_enqueue = should_enqueue_terminal_result(
                        previous.as_deref(),
                        &job.status,
                        job.updated_at,
                        monitor_started_at,
                    );
                    if !terminal_job_status(&job.status)
                        || job.status == "superseded"
                        || job.status == "cancelled"
                    {
                        continue;
                    }
                    let unit = match store.get_work_unit(&job.unit_id) {
                        Ok(Some(unit)) => unit,
                        _ => continue,
                    };
                    if unit.status == kanzei_core::WorkUnitStatus::Superseded
                        || !unit
                            .background_verification
                            .as_ref()
                            .is_some_and(|verification| {
                                verification.job_id == job.id && !verification.pending
                            })
                    {
                        continue;
                    }
                    let Some(process) =
                        owning_process(&unit, &processes, job.session_id.as_deref(), &store)
                    else {
                        continue;
                    };
                    let input_id = wake_input_id(&job.id);
                    let already_pending = match has_pending_wake_input(
                        &store,
                        &process.session_id,
                        &input_id,
                    ) {
                        Ok(pending) => pending,
                        Err(error) => {
                            tracing::warn!(project = %root.display(), job = %job.id, %error, "verification wake queue check failed");
                            continue;
                        }
                    };
                    if should_enqueue && !already_pending {
                        if let Err(error) = admit_wake_input(
                            &store,
                            &root,
                            &process.session_id,
                            &job.id,
                            &job.unit_id,
                            &job.status,
                        ) {
                            tracing::warn!(project = %root.display(), job = %job.id, %error, "verification wake admission failed");
                            continue;
                        }
                    }
                    let pending = match has_pending_wake_input(
                        &store,
                        &process.session_id,
                        &input_id,
                    ) {
                        Ok(pending) => pending,
                        Err(error) => {
                            tracing::warn!(project = %root.display(), job = %job.id, %error, "verification wake queue check failed");
                            continue;
                        }
                    };
                    if !pending || process.running {
                        continue;
                    }
                    let state = app.state::<AppState>();
                    if let Err(error) = crate::commands::run::schedule_run(
                        window.clone(),
                        &state,
                        project_dir.clone(),
                        Some(process.id.clone()),
                        crate::commands::run::Submission::Saved {
                            session_id: process.session_id.clone(),
                            input_id: input_id.clone(),
                        },
                        crate::commands::run::RunOptions {
                            autonomous: true,
                            execution_batch: true,
                            ..Default::default()
                        },
                    ) {
                        tracing::warn!(project = %root.display(), job = %job.id, line = %process.id, %error, "verification wake dispatch failed");
                    }
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_core::SessionStore;

    #[tokio::test]
    async fn verification_returns_to_its_conversation_without_first_peer_fallback() {
        let root = std::env::temp_dir().join(format!(
            "kz-verification-owner-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = crate::normalized_project_root(&root);
        let project = root.display().to_string();
        let state = AppState::default();
        let mut peers = Vec::new();
        for _ in 0..2 {
            peers.push(
                crate::processes::create_process(
                    &state,
                    &project,
                    None,
                    Some("dev".into()),
                    None,
                    None,
                )
                .await
                .unwrap(),
            );
        }
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store
            .create_work_unit(kanzei_core::WorkUnitSpec {
                unit_id: "R-001/W1".into(),
                requirement_id: "R-001".into(),
                objective: "recipient isolation".into(),
                scope: vec![],
                dependencies: vec![],
                acceptance: vec!["exact session".into()],
                verification: vec![],
                base_revision: "fixture".into(),
            })
            .unwrap();
        let unit = store.get_work_unit("R-001/W1").unwrap().unwrap();
        assert_eq!(
            owning_process(&unit, &peers, Some(&peers[1].session_id), &store)
                .unwrap()
                .id,
            peers[1].id
        );
        assert!(owning_process(&unit, &peers[..1], Some(&peers[1].session_id), &store).is_none());
        assert!(owning_process(&unit, &peers, None, &store).is_none());
        let mut claimed = unit;
        claimed.claimed_by = Some("shared-branch".into());
        peers[0].branch = claimed.claimed_by.clone();
        assert_eq!(
            owning_process(&claimed, &peers, None, &store).unwrap().id,
            peers[0].id
        );
        peers[1].branch = claimed.claimed_by.clone();
        assert!(owning_process(&claimed, &peers, None, &store).is_none());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_results_are_woken_once_and_retried_until_admitted() {
        assert!(should_enqueue_terminal_result(
            Some("running"),
            "passed",
            99,
            100
        ));
        assert!(should_enqueue_terminal_result(
            Some("queued"),
            "failed",
            99,
            100
        ));
        assert!(should_enqueue_terminal_result(None, "passed", 100, 100));
        assert!(!should_enqueue_terminal_result(None, "passed", 99, 100));
        assert!(should_enqueue_terminal_result(
            Some("passed"),
            "passed",
            101,
            100
        ));
        assert!(!should_enqueue_terminal_result(
            Some("passed"),
            "passed",
            99,
            100
        ));
        assert!(!should_enqueue_terminal_result(
            Some("running"),
            "cancelled",
            101,
            100
        ));
        assert!(!should_enqueue_terminal_result(
            Some("running"),
            "superseded",
            101,
            100
        ));
        assert!(!should_enqueue_terminal_result(
            Some("queued"),
            "running",
            101,
            100
        ));
    }

    #[test]
    fn wake_input_is_durable_idempotent_and_session_scoped() {
        let root = std::env::temp_dir().join(format!(
            "kz-verification-wake-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let session_id = kanzei_core::project_session_id(&root);
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store
            .create_session(&session_id, &root.display().to_string(), None)
            .unwrap();

        assert!(
            admit_wake_input(&store, &root, &session_id, "v-123", "R-379/W1", "passed").unwrap()
        );
        assert!(
            admit_wake_input(&store, &root, &session_id, "v-123", "R-379/W1", "passed").unwrap()
        );
        assert_eq!(store.list_pending_inputs(&session_id).unwrap().len(), 1);
        let other_session = format!("{session_id}-other");
        store
            .create_session(&other_session, &root.display().to_string(), None)
            .unwrap();
        assert!(
            admit_wake_input(&store, &root, &other_session, "v-123", "R-379/W1", "passed",)
                .is_err()
        );

        assert_eq!(
            store
                .input_status("verification-wake-v-123")
                .unwrap()
                .as_deref(),
            Some("pending")
        );
        assert_eq!(
            store
                .list_events_by_type(&session_id, 0, "prompt.admitted")
                .unwrap()
                .len(),
            1
        );

        drop(store);
        std::fs::remove_dir_all(root).ok();
    }
}
