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
    // UI2-0926 #10 对话背景(星座背景):开关、图案、密度、不透明度与「我的图片」导出的点集。
    // 只存点集,从不存原图;序列化上限 BACKDROP_MAX_BYTES。见 docs/design/ui_chat_backdrop.md。
    #[serde(default)]
    pub(crate) backdrop: Option<Value>,
    #[serde(default)]
    pub(crate) work_priority: HashMap<String, String>,
    #[serde(default)]
    pub(crate) auto_max: Option<u32>,
    #[serde(default)]
    pub(crate) continue_prompt: Option<String>,
    #[serde(default)]
    pub(crate) process_auto_state: HashMap<String, Value>,
    #[serde(default)]
    pub(crate) workspace_state: HashMap<String, Value>,
    // UI2-0926 #14/#4:界面布局偏好(后台任务侧栏开关、可调框几何、分隔条宽高)。
    // 形如 { side_panel: {auto_open, auto_close}, frames: {id: {...}}, splits: {id: px} }。
    // 前端只发变化的键,后端按「分区 → 键」两级合并(见 merge_ui_layout),值为 null 即删除。
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub(crate) ui_layout: Value,
    /// 记忆页视图:"list" | "graph"(记忆图谱)。单独一个字段,与其它 UI 布局偏好互不牵连。
    #[serde(default)]
    pub(crate) memory_view: Option<String>,
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

/// 布局偏好的写入上限:几何与开关只有几十个键,超过即视为异常负载,整次写入丢弃。
const UI_LAYOUT_MAX_BYTES: usize = 64 * 1024;

/// 界面布局偏好的合并写入(D-404 通道的通用字段)。
///
/// 两级合并:`patch` 的第一级是分区(side_panel / frames / splits),第二级是分区里的键;
/// 第二级的值整体替换(一个框的几何 {l,t,w…} 必须整条换掉,逐字段合并会留下 l 与 r 并存的非法组合),
/// 值为 null 即删除该键,分区为 null 即删除整个分区。patch 不是对象时忽略(不抹掉已存偏好)。
pub(crate) fn merge_ui_layout(target: &mut Value, patch: Value) {
    let Value::Object(sections) = patch else {
        return;
    };
    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }
    let Some(root) = target.as_object_mut() else {
        return;
    };
    for (section, value) in sections {
        match value {
            Value::Null => {
                root.remove(&section);
            }
            Value::Object(keys) => {
                let slot = root
                    .entry(section)
                    .or_insert_with(|| Value::Object(serde_json::Map::new()));
                if !slot.is_object() {
                    *slot = Value::Object(serde_json::Map::new());
                }
                if let Some(entries) = slot.as_object_mut() {
                    for (key, entry) in keys {
                        if entry.is_null() {
                            entries.remove(&key);
                        } else {
                            entries.insert(key, entry);
                        }
                    }
                }
            }
            other => {
                root.insert(section, other);
            }
        }
    }
}

fn apply_ui_layout(prefs: &mut AppPrefs, patch: Option<Value>) {
    let Some(patch) = patch else {
        return;
    };
    if serde_json::to_string(&patch).map_or(true, |text| text.len() > UI_LAYOUT_MAX_BYTES) {
        return;
    }
    merge_ui_layout(&mut prefs.ui_layout, patch);
}

/// 对话背景偏好的序列化上限。点集 64 颗星约 2KB,200 星 / 400 边的上限也远低于它;
/// 超限说明前端误把原图或像素塞了进来,整条拒绝、原值不变。
pub(crate) const BACKDROP_MAX_BYTES: usize = 64 * 1024;

fn apply_backdrop(prefs: &mut AppPrefs, backdrop: Option<Value>) -> Result<(), String> {
    let Some(value) = backdrop else {
        return Ok(());
    };
    let size = serde_json::to_string(&value).map_or(usize::MAX, |text| text.len());
    if size > BACKDROP_MAX_BYTES {
        return Err(format!(
            "对话背景偏好过大({size} 字节,上限 {BACKDROP_MAX_BYTES}):只保存星座点集,不保存图片"
        ));
    }
    prefs.backdrop = Some(value);
    Ok(())
}

/// 记忆页视图偏好:只认 list / graph,其它值忽略(不把脏值写进 app.json)。
fn apply_memory_view(prefs: &mut AppPrefs, memory_view: Option<String>) {
    if let Some(view) = memory_view.filter(|v| v == "list" || v == "graph") {
        prefs.memory_view = Some(view);
    }
}

#[tauri::command]
pub fn ui_prefs_get() -> serde_json::Value {
    let p = load_prefs();
    json!({
        "theme": p.theme,
        "backdrop": p.backdrop,
        "work_priority": p.work_priority,
        "auto_max": p.auto_max,
        "continue_prompt": p.continue_prompt,
        "process_auto_state": p.process_auto_state,
        "workspace_state": p.workspace_state,
        "ui_layout": if p.ui_layout.is_object() { p.ui_layout } else { json!({}) },
        "memory_view": p.memory_view,
    })
}

// UI 偏好通道的请求与持久化对象都使用 snake_case。
// 每个参数对应 ui_prefs 通道里一个独立字段(IPC 形状即参数名),收成结构体会改动前端调用约定。
#[allow(clippy::too_many_arguments)]
#[tauri::command(rename_all = "snake_case")]
pub fn ui_prefs_set(
    theme: Option<String>,
    backdrop: Option<Value>,
    work_priority: Option<HashMap<String, String>>,
    auto_max: Option<u32>,
    continue_prompt: Option<String>,
    process_auto_state: Option<HashMap<String, Value>>,
    workspace_state: Option<HashMap<String, Value>>,
    ui_layout: Option<Value>,
    memory_view: Option<String>,
) -> Result<(), String> {
    let mut prefs = load_prefs();
    apply_backdrop(&mut prefs, backdrop)?;
    apply_ui_prefs(
        &mut prefs,
        theme,
        work_priority,
        auto_max,
        continue_prompt,
        process_auto_state,
    );
    if let Some(workspace_state) = workspace_state {
        prefs.workspace_state = workspace_state;
    }
    apply_ui_layout(&mut prefs, ui_layout);
    apply_memory_view(&mut prefs, memory_view);
    save_prefs(&prefs);
    Ok(())
}

#[cfg(test)]
mod backdrop_tests {
    use super::*;

    #[test]
    fn backdrop_往返写入后读回() {
        let mut p = AppPrefs::default();
        let value = json!({
            "enabled": true, "preset": "custom", "density": 1.2, "opacity": 0.8,
            "custom": {"v": 1, "kind": "image", "points": [[0.1, 0.2, 2.4], [0.5, 0.5, 1.8], [0.9, 0.7, 3.1]], "edges": [[0, 1, 0], [1, 2, 0]], "hub": -1}
        });
        apply_backdrop(&mut p, Some(value.clone())).expect("小于上限应写入");
        let restored: AppPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(restored.backdrop, Some(value));
        apply_backdrop(&mut p, None).expect("None 不变更");
        assert_eq!(p.backdrop.as_ref().unwrap()["preset"], "custom");
    }

    #[test]
    fn backdrop_旧app_json无该字段_回落none() {
        let old = r#"{"projects":["p1"],"theme":"dark"}"#;
        let p: AppPrefs = serde_json::from_str(old).expect("旧格式应兼容");
        assert!(p.backdrop.is_none());
        assert_eq!(p.theme.as_deref(), Some("dark"));
    }

    #[test]
    fn backdrop_超过上限被拒绝且原值不变() {
        let mut p = AppPrefs {
            backdrop: Some(json!({"preset": "orion"})),
            ..Default::default()
        };
        let huge = json!({"preset": "custom", "pixels": "x".repeat(BACKDROP_MAX_BYTES)});
        let err = apply_backdrop(&mut p, Some(huge)).expect_err("超限必须拒绝");
        assert!(err.contains("上限"), "{err}");
        assert_eq!(p.backdrop, Some(json!({"preset": "orion"})));
    }
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
        assert!(p.workspace_state.is_empty());
    }

    #[test]
    fn workspace_preferences_preserve_each_project_and_space() {
        let mut p = AppPrefs::default();
        p.workspace_state.insert("project-a".into(), json!({"space":"research","dev":{"process_id":"dev-a","view":"chat"},"research":{"process_id":"research-a","topic":"topic-a","page":"writing","view":"research"}}));
        p.workspace_state
            .insert("project-b".into(), json!({"space":"dev"}));
        let restored: AppPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(restored.workspace_state, p.workspace_state);
        assert_eq!(
            restored.workspace_state["project-a"]["research"]["topic"],
            "topic-a"
        );
    }

    #[test]
    fn ui_layout_两级合并_键整体替换_null删除() {
        let mut p = AppPrefs::default();
        apply_ui_layout(
            &mut p,
            Some(json!({
                "side_panel": { "auto_open": true, "auto_close": true },
                "frames": { "viewer": { "v": 1, "l": 90, "t": 50, "w": 880 } },
                "splits": { "sidebar": 320, "tasks": 440 },
            })),
        );
        // 只发变化的键:同分区其它键保留;框几何整条替换(不能残留 l 与 r 并存)。
        apply_ui_layout(
            &mut p,
            Some(json!({
                "side_panel": { "auto_open": false },
                "frames": { "viewer": { "v": 1, "r": 40, "b": 30 } },
                "splits": { "tasks": null },
            })),
        );
        assert_eq!(p.ui_layout["side_panel"]["auto_open"], json!(false));
        assert_eq!(p.ui_layout["side_panel"]["auto_close"], json!(true));
        assert_eq!(
            p.ui_layout["frames"]["viewer"],
            json!({ "v": 1, "r": 40, "b": 30 })
        );
        assert!(p.ui_layout["frames"]["viewer"].get("l").is_none());
        assert_eq!(p.ui_layout["splits"]["sidebar"], json!(320));
        assert!(p.ui_layout["splits"].get("tasks").is_none());
        // 分区为 null 删除整个分区;非对象 patch 忽略,不抹掉已存偏好。
        apply_ui_layout(&mut p, Some(json!({ "frames": null })));
        assert!(p.ui_layout.get("frames").is_none());
        apply_ui_layout(&mut p, Some(json!("oops")));
        apply_ui_layout(&mut p, None);
        assert_eq!(p.ui_layout["splits"]["sidebar"], json!(320));
    }

    #[test]
    fn ui_layout_超大负载整次丢弃() {
        let mut p = AppPrefs::default();
        apply_ui_layout(&mut p, Some(json!({ "splits": { "sidebar": 300 } })));
        let huge = "x".repeat(UI_LAYOUT_MAX_BYTES + 1);
        apply_ui_layout(&mut p, Some(json!({ "frames": { "viewer": huge } })));
        assert!(p.ui_layout.get("frames").is_none());
        assert_eq!(p.ui_layout["splits"]["sidebar"], json!(300));
    }

    #[test]
    fn ui_layout_往返_旧app_json无字段回落空() {
        let old = r#"{"projects":["p1"],"theme":"dark"}"#;
        let mut p: AppPrefs = serde_json::from_str(old).expect("旧格式应兼容");
        assert!(p.ui_layout.is_null());
        // 未写过布局偏好时不往 app.json 里写 "ui_layout": null。
        assert!(!serde_json::to_string(&p).unwrap().contains("ui_layout"));
        apply_ui_layout(
            &mut p,
            Some(json!({ "frames": { "ask": { "v": 1, "r": 22, "b": 18, "w": 640 } } })),
        );
        let restored: AppPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(restored.ui_layout["frames"]["ask"]["w"], json!(640));
        assert_eq!(restored.theme.as_deref(), Some("dark"));
        // 旧 app.json 里若是非对象(手改坏了),下一次合并写入从空对象开始。
        let mut broken = AppPrefs {
            ui_layout: json!([1, 2]),
            ..Default::default()
        };
        merge_ui_layout(&mut broken.ui_layout, json!({ "splits": { "log": 240 } }));
        assert_eq!(broken.ui_layout, json!({ "splits": { "log": 240 } }));
    }

    #[test]
    fn memory_view_偏好只收_list_graph_旧文件回落默认() {
        let mut p = AppPrefs::default();
        apply_memory_view(&mut p, Some("graph".into()));
        assert_eq!(p.memory_view.as_deref(), Some("graph"));
        apply_memory_view(&mut p, Some("canvas".into()));
        assert_eq!(p.memory_view.as_deref(), Some("graph"), "非法值忽略");
        apply_memory_view(&mut p, None);
        assert_eq!(p.memory_view.as_deref(), Some("graph"), "None 不变");
        let restored: AppPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(restored.memory_view.as_deref(), Some("graph"));
        let old: AppPrefs = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert!(old.memory_view.is_none());
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
