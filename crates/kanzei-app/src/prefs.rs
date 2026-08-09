//! Desktop project preferences persisted in ~/.kanzei/app.json.

use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AppPrefs {
    #[serde(default)] pub(crate) projects: Vec<String>,
    #[serde(default)] pub(crate) current: Option<String>,
    #[serde(default)] pub(crate) names: HashMap<String, String>,
}

fn prefs_path() -> PathBuf { kanzei_harness::kanzei_home().unwrap_or_default().join("app.json") }
pub(crate) fn load_prefs() -> AppPrefs { std::fs::read_to_string(prefs_path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default() }
pub(crate) fn save_prefs(prefs: &AppPrefs) { let path = prefs_path(); if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); } let _ = std::fs::write(&path, serde_json::to_string_pretty(prefs).unwrap_or_default()); }
