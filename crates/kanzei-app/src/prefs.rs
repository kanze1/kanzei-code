//! Desktop project preferences persisted in ~/.kanzei/app.json.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AppPrefs {
    #[serde(default)]
    pub(crate) projects: Vec<String>,
    #[serde(default)]
    pub(crate) current: Option<String>,
    #[serde(default)]
    pub(crate) names: HashMap<String, String>,
    // D-404:本机 WebView2 localStorage leveldb 数据文件缺失(2026-08-16 实证:
    // EBWebView\Default\Local Storage\leveldb 无 .ldb/.log,仅 MANIFEST 残留),
    // localStorage 偏好重启即丢。关键 UI 偏好(主题/鞭挞)改存 app.json,
    // localStorage 仅作旧值兼容(前端先读本地旧值,后端值回来后覆盖)。
    #[serde(default)]
    pub(crate) theme: Option<String>,
    #[serde(default)]
    pub(crate) work_priority: HashMap<String, String>,
    #[serde(default)]
    pub(crate) auto_max: Option<u32>,
    #[serde(default)]
    pub(crate) continue_prompt: Option<String>,
    #[serde(default)]
    pub(crate) process_auto_state: HashMap<String, Value>,
}

fn prefs_path() -> PathBuf {
    kanzei_harness::kanzei_home()
        .unwrap_or_default()
        .join("app.json")
}
pub(crate) fn load_prefs() -> AppPrefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}
pub(crate) fn save_prefs(prefs: &AppPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(prefs).unwrap_or_default(),
    );
}

// ---------- D-404:关键 UI 偏好后端持久化 ----------
// WebView2 localStorage 在本机不落盘(数据文件缺失),主题/鞭挞等偏好迁到这里。
// None 参数 = 该字段不变(前端每次只带变化的字段)。

fn apply_ui_prefs(
    prefs: &mut AppPrefs,
    theme: Option<String>,
    work_priority: Option<HashMap<String, String>>,
    auto_max: Option<u32>,
    continue_prompt: Option<String>,
    process_auto_state: Option<HashMap<String, Value>>,
) {
    if let Some(v) = theme {
        prefs.theme = Some(v);
    }
    if let Some(v) = work_priority {
        prefs.work_priority = v;
    }
    if let Some(v) = auto_max {
        prefs.auto_max = Some(v);
    }
    if let Some(v) = continue_prompt {
        prefs.continue_prompt = Some(v);
    }
    if let Some(v) = process_auto_state {
        prefs.process_auto_state = v;
    }
}

#[tauri::command]
pub fn ui_prefs_get() -> serde_json::Value {
    let p = load_prefs();
    json!({
        "theme": p.theme,
        "work_priority": p.work_priority,
        "auto_max": p.auto_max,
        "continue_prompt": p.continue_prompt,
        "process_auto_state": p.process_auto_state,
    })
}

#[tauri::command]
pub fn ui_prefs_set(
    theme: Option<String>,
    work_priority: Option<HashMap<String, String>>,
    auto_max: Option<u32>,
    continue_prompt: Option<String>,
    process_auto_state: Option<HashMap<String, Value>>,
) -> Result<(), String> {
    let mut prefs = load_prefs();
    apply_ui_prefs(
        &mut prefs,
        theme,
        work_priority,
        auto_max,
        continue_prompt,
        process_auto_state,
    );
    save_prefs(&prefs);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_prefs_往返_新字段写入后读回() {
        let mut p = AppPrefs::default();
        let mut wp = HashMap::new();
        wp.insert("proj-a".into(), "requirement-first".into());
        let mut pa = HashMap::new();
        pa.insert("proc-1".into(), json!({ "enabled": true, "maxRounds": 5 }));
        apply_ui_prefs(
            &mut p,
            Some("light".into()),
            Some(wp),
            Some(7),
            Some("继续".into()),
            Some(pa),
        );
        assert_eq!(p.theme.as_deref(), Some("light"));
        assert_eq!(
            p.work_priority.get("proj-a").map(String::as_str),
            Some("requirement-first")
        );
        assert_eq!(p.auto_max, Some(7));
        assert_eq!(p.continue_prompt.as_deref(), Some("继续"));
        assert_eq!(p.process_auto_state["proc-1"]["enabled"], json!(true));
    }

    #[test]
    fn ui_prefs_旧app_json无新字段_反序列化回落默认() {
        let old = r#"{"projects":["p1"],"current":"p1","names":{"p1":"P"}}"#;
        let p: AppPrefs = serde_json::from_str(old).expect("旧格式应兼容");
        assert_eq!(p.projects, vec!["p1"]);
        assert!(p.theme.is_none());
        assert!(p.work_priority.is_empty());
        assert!(p.auto_max.is_none());
        assert!(p.process_auto_state.is_empty());
    }

    #[test]
    fn ui_prefs_none参数不变更既有字段() {
        let mut p = AppPrefs {
            theme: Some("dark".into()),
            ..Default::default()
        };
        apply_ui_prefs(&mut p, None, None, None, None, None);
        assert_eq!(p.theme.as_deref(), Some("dark"));
    }
}
