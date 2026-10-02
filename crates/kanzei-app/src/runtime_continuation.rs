//! A detached run cannot depend on a browser timer to start its next round.
use crate::{AppState, MutexPoisonExt};
use serde_json::Value;
use std::sync::atomic::Ordering;
use tauri::Manager;
pub(crate) fn arm(app: &tauri::AppHandle, payload: &Value) {
    let action = &payload["autoAction"];
    if !matches!(
        action["type"].as_str(),
        Some("Continue" | "Nudge" | "VerifyRound" | "GoalPending" | "RetryAfterFailure")
    ) {
        return;
    }
    let Some(owner) = payload["sessionId"].as_str() else {
        return;
    };
    let state = app.state::<AppState>();
    let Some(runtime) = state.runtimes.lock_or_recover().get(owner).cloned() else {
        return;
    };
    let Some(process) = state
        .processes
        .lock_or_recover()
        .values()
        .find(|p| crate::process_session_id(&p.origin_project.0, Some(&p.id)) == owner)
        .cloned()
    else {
        return;
    };
    let generation = runtime.run_generation.load(Ordering::SeqCst);
    let async_generation = runtime.async_generation.load(Ordering::SeqCst);
    let mut options = runtime.callback_options.lock_or_recover().clone();
    options.autonomous = true;
    options.execution_batch = true;
    let prompt = action["prompt"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| crate::prefs::load_prefs().continue_prompt)
        .unwrap_or_else(|| {
            "继续推进当前任务。先核对当前需求、阻塞与交付事实，再执行下一批独立工作。".into()
        });
    let delay = action["delayMs"]
        .as_u64()
        .unwrap_or(2000)
        .clamp(500, 60_000);
    let owner = owner.to_owned();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        // The done event can precede final resource cleanup. Wait for that same
        // generation to release its lease instead of losing the next round.
        while runtime.running.load(Ordering::SeqCst) {
            if runtime.run_generation.load(Ordering::SeqCst) != generation
                || runtime.async_generation.load(Ordering::SeqCst) != async_generation
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        let Some(window) = app.get_window("main") else {
            return;
        };
        let state = app.state::<AppState>();
        if let Err(error) = crate::commands::run::schedule_run(
            window,
            &state,
            process.origin_project.0.display().to_string(),
            Some(process.id.clone()),
            crate::commands::run::Submission::Automatic {
                prompt,
                generation,
                async_generation,
            },
            options,
        ) {
            tracing::warn!(%error,session=%owner,"background continuation did not start");
        }
    });
}
