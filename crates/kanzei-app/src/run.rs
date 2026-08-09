//! 运行主链路命令与运行画像。

use std::path::PathBuf;

use serde_json::json;

#[tauri::command]
pub(crate) fn run_metrics(project_dir: String, limit: Option<usize>) -> Result<serde_json::Value, String> {
    let root = PathBuf::from(&project_dir);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).map_err(|e| e.to_string())?;
    let session_id = kanzei_core::project_session_id(&root);
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let rows = store.recent_episodes(&session_id, limit).map_err(|e| e.to_string())?;
    let rounds: Vec<serde_json::Value> = rows.into_iter().map(|(at, prompt, outcome, steps, input, output, tools, context, metrics)| {
        let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap_or(json!({}));
        let metrics_value = parse(&metrics);
        json!({
            "at": at,
            "prompt": prompt,
            "outcome": outcome,
            "steps": steps,
            "inputTokens": input,
            "outputTokens": output,
            "tools": parse(&tools),
            "context": parse(&context),
            "metrics": metrics_value,
            "measured": metrics.trim() != "{}" && !metrics.trim().is_empty(),
        })
    }).collect();
    Ok(json!({ "rounds": rounds }))
}
