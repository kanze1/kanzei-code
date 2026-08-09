//! Settings command boundary.
//!
//! The implementation remains behaviorally centralized in main.rs during the
//! staged monolith migration; this module is the real Tauri command consumer.

use tauri::State;

use crate::{AppState, SettingsPayload};

#[tauri::command]
pub fn settings_get(project_dir: Option<String>) -> serde_json::Value {
    crate::settings_get(project_dir)
}

#[tauri::command]
pub fn settings_save(payload: SettingsPayload) -> Result<(), String> {
    crate::settings_save(payload)
}

#[tauri::command]
pub fn settings_open() -> Result<(), String> {
    crate::settings_open()
}

#[tauri::command]
pub fn permission_rules_get(project_dir: String) -> Result<serde_json::Value, String> {
    crate::permission_rules_get(project_dir)
}

#[tauri::command]
pub fn permission_rule_delete(project_dir: String, index: usize) -> Result<(), String> {
    crate::permission_rule_delete(project_dir, index)
}

#[tauri::command]
pub async fn provider_test(
    protocol: String,
    base_url: String,
    api_key_env: Option<String>,
    api_key: Option<String>,
    auth: Option<String>,
    proxy: Option<String>,
) -> Result<String, String> {
    crate::provider_test(protocol, base_url, api_key_env, api_key, auth, proxy).await
}

// Keep the state import out of the command API while this staged boundary is
// compiled alongside the existing implementation.
#[allow(dead_code)]
fn _state_type_marker(_: Option<State<'_, AppState>>) {}
