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
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub(crate) conversation_goals: HashMap<String, String>,
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
    /// 「用…打开」的自配工具(设置→打开方式)。Rust 要据此启动外部程序,所以是带类型的字段,
    /// 不放进 `ui_layout`;内置工具(VS Code / Windows 终端 / PowerShell / Git Bash)不存这里,
    /// 每次按需探测。见 `commands/os_open.rs`。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) open_tools: Vec<OpenToolConfig>,
}

/// 一个自配的「打开方式」。`args` 里的 `{path}` 在启动时换成目标目录;没有 `{path}` 时保存会补上。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OpenToolConfig {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) command: String,
    #[serde(default)]
    pub(crate) args: Vec<String>,
}

fn prefs_path() -> PathBuf {
    kanzei_harness::kanzei_home()
        .unwrap_or_default()
        .join("app.json")
}
pub(crate) fn load_prefs() -> AppPrefs {
    load_prefs_for_write().unwrap_or_default()
}

/// 写入必须基于成功读取的状态；仅首次无文件允许默认初始化。
/// 只读展示仍由 load_prefs 保持既有 best-effort 回落，不授权覆盖损坏原文。
pub(crate) fn load_prefs_for_write() -> Result<AppPrefs, String> {
    let path = prefs_path();
    let mut prefs: AppPrefs = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("解析偏好文件 {} 失败: {error}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppPrefs::default(),
        Err(error) => {
            return Err(format!("读取偏好文件 {} 失败: {error}", path.display()));
        }
    };
    simplify_pref_keys(&mut prefs);
    Ok(prefs)
}

/// UI2-0926 #13:按进程 id / 项目路径作键的偏好,键里的 `\\?\` 前缀去掉(与 schema v25 同一条
/// path_form 规则)。进程 id 形如 `d|\\?\C:\…` —— 身份根改成 simplify 形态之后,不迁移的话
/// 每条线的鞭挞开关与停机设置都会在升级后「丢失」。两种写法并存时保留**去前缀**的那条、丢掉
/// 带前缀的:只有本版本才写去前缀的键,所以它一定是较新的写入;带前缀的只可能是升级前的存档,
/// 或别处(本地 localStorage 的旧映射)回灌的陈值——让它赢,用户升级后的设置(比如关掉的鞭挞)
/// 会在下次启动时被旧值改回去(复核 minor)。
pub(crate) fn simplify_pref_keys(prefs: &mut AppPrefs) {
    fn simplify_key(key: &str) -> Option<String> {
        let (prefix, path) = match key.split_once('|') {
            Some((prefix, path)) => (Some(prefix), path),
            None => (None, key),
        };
        let simplified = kanzei_tools::path_form::simplify_str(path);
        if simplified == path {
            return None;
        }
        Some(match prefix {
            Some(prefix) => format!("{prefix}|{simplified}"),
            None => simplified.into_owned(),
        })
    }
    fn migrate<V>(map: &mut HashMap<String, V>) {
        let renames: Vec<(String, String)> = map
            .keys()
            .filter_map(|key| simplify_key(key).map(|to| (key.clone(), to)))
            .collect();
        for (from, to) in renames {
            if let Some(value) = map.remove(&from) {
                map.entry(to).or_insert(value);
            }
        }
    }
    migrate(&mut prefs.process_auto_state);
    migrate(&mut prefs.work_priority);
}

/// 两个项目路径是否指同一目录:忽略 `\\?\` 前缀、斜杠方向、末尾分隔符与大小写(Windows)。
pub(crate) fn same_project_path(a: &str, b: &str) -> bool {
    fn norm(path: &str) -> String {
        kanzei_tools::path_form::simplify_str(path)
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    }
    norm(a) == norm(b)
}

/// 偏好键(`p2|<项目路径>`、`d|<项目路径>` 或裸项目路径)是否属于这个项目。
fn key_in_project(key: &str, project: &str) -> bool {
    let path = key.split_once('|').map_or(key, |(_, rest)| rest);
    same_project_path(path, project)
}

/// 同一个进程 id 的不同写法(带不带 `\\?\` 前缀)视为同一个。
fn same_process_id(a: &str, b: &str) -> bool {
    match (a.split_once('|'), b.split_once('|')) {
        (Some((prefix_a, rest_a)), Some((prefix_b, rest_b))) => {
            prefix_a == prefix_b && same_project_path(rest_a, rest_b)
        }
        _ => a == b,
    }
}

/// 从 `ui_layout` 的一个分区里摘掉命中的条目:键命中直接删;值是数组就滤掉命中的元素,
/// 值是对象就删命中的子键;数组/对象被摘空时整个键也删。返回是否有改动。
fn scrub_layout_section(
    section: &mut serde_json::Map<String, Value>,
    matches: &dyn Fn(&str) -> bool,
) -> bool {
    let mut changed = false;
    let keys = section.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        if matches(&key) {
            section.remove(&key);
            changed = true;
            continue;
        }
        let Some(value) = section.get_mut(&key) else {
            continue;
        };
        let emptied = match value {
            Value::Array(items) => {
                let before = items.len();
                items.retain(|item| !item.as_str().is_some_and(matches));
                changed |= items.len() != before;
                before > 0 && items.is_empty()
            }
            Value::Object(inner) => {
                let before = inner.len();
                inner.retain(|name, _| !matches(name));
                changed |= inner.len() != before;
                before > 0 && inner.is_empty()
            }
            _ => false,
        };
        if emptied {
            section.remove(&key);
        }
    }
    changed
}

fn scrub_layout(layout: &mut Value, sections: &[&str], matches: &dyn Fn(&str) -> bool) -> bool {
    let Some(root) = layout.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for name in sections {
        if let Some(section) = root.get_mut(*name).and_then(Value::as_object_mut) {
            changed |= scrub_layout_section(section, matches);
        }
    }
    changed
}

/// 删除对话时清掉 app.json 里挂在该进程 id 上的界面状态:鞭挞设置、会话置顶、手动排序。
/// 返回是否有改动(没有就不必写盘)。
pub(crate) fn purge_process_prefs(prefs: &mut AppPrefs, process_id: &str) -> bool {
    let before = prefs.process_auto_state.len();
    prefs
        .process_auto_state
        .retain(|key, _| !same_process_id(key, process_id));
    let mut changed = prefs.process_auto_state.len() != before;
    changed |= scrub_layout(
        &mut prefs.ui_layout,
        &["session_pins", "session_order"],
        &|id| same_process_id(id, process_id),
    );
    changed
}

/// 移除项目时清掉 app.json 里它留下的全部残留:显示名、工作区状态、任务优先级、各线鞭挞设置,
/// 以及侧栏的置顶/排序/展开态。磁盘上的项目文件一概不动。
pub(crate) fn purge_project_prefs(prefs: &mut AppPrefs, project: &str) {
    prefs
        .names
        .retain(|path, _| !same_project_path(path, project));
    prefs
        .workspace_state
        .retain(|path, _| !same_project_path(path, project));
    prefs
        .work_priority
        .retain(|path, _| !same_project_path(path, project));
    prefs
        .process_auto_state
        .retain(|key, _| !key_in_project(key, project));
    scrub_layout(
        &mut prefs.ui_layout,
        &[
            "session_pins",
            "session_order",
            "sidebar_open",
            "project_pins",
        ],
        &|key| key_in_project(key, project),
    );
}

/// Hold this across the complete read/modify/write transaction, including RPC
/// callers running concurrently in the detached runtime.
pub(crate) fn write_guard() -> Result<kanzei_tools::atomic_file::FileLock, String> {
    kanzei_tools::atomic_file::lock_exclusive(&prefs_path()).map_err(|e| e.to_string())
}
pub(crate) fn save_prefs(prefs: &AppPrefs) -> Result<(), String> {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    kanzei_tools::atomic_file::write_atomic(
        &path,
        &serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
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
        prefs.work_priority.extend(v);
    }
    if let Some(v) = auto_max {
        prefs.auto_max = Some(v);
    }
    if let Some(v) = continue_prompt {
        prefs.continue_prompt = Some(v);
    }
    if let Some(v) = process_auto_state {
        for (id, patch) in v {
            match patch {
                Value::Null => {
                    prefs.process_auto_state.remove(&id);
                }
                Value::Object(fields) => {
                    let current = prefs
                        .process_auto_state
                        .entry(id)
                        .or_insert_with(|| json!({}));
                    if let Some(object) = current.as_object_mut() {
                        object.extend(fields);
                    } else {
                        *current = Value::Object(fields);
                    }
                }
                value => {
                    prefs.process_auto_state.insert(id, value);
                }
            }
        }
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

/// 导航写端只发送触及字段；同一 write_guard 内合并，保留其它窗口的项目/课题。
/// workspace 的 null 是存储值(例如 process_id 清空)，与 ui_layout 的删除语义不同。
fn merge_workspace_state(state: &mut HashMap<String, Value>, patch: HashMap<String, Value>) {
    fn merge_fields(current: &mut Value, patch: Value) {
        if let Value::Object(fields) = patch {
            if !current.is_object() {
                *current = json!({});
            }
            current.as_object_mut().unwrap().extend(fields);
        } else {
            *current = patch;
        }
    }
    for (project, update) in patch {
        let current = state.entry(project).or_insert_with(|| json!({}));
        let Value::Object(update) = update else {
            *current = update;
            continue;
        };
        if !current.is_object() {
            *current = json!({});
        }
        let bucket = current.as_object_mut().unwrap();
        for (section, value) in update {
            let target = bucket.entry(section.clone()).or_insert(Value::Null);
            if section == "dev" || section == "research" {
                merge_fields(target, value);
            } else if section == "topic_states" && value.is_object() {
                if !target.is_object() {
                    *target = json!({});
                }
                let topics = target.as_object_mut().unwrap();
                for (topic, fields) in value.as_object().unwrap() {
                    merge_fields(
                        topics.entry(topic.clone()).or_insert(Value::Null),
                        fields.clone(),
                    );
                }
            } else {
                *target = value;
            }
        }
    }
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
    let _guard = write_guard()?;
    let mut prefs = load_prefs_for_write()?;
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
        merge_workspace_state(&mut prefs.workspace_state, workspace_state);
    }
    apply_ui_layout(&mut prefs, ui_layout);
    apply_memory_view(&mut prefs, memory_view);
    save_prefs(&prefs)
}

#[cfg(test)]
pub(crate) mod failure_tests {
    use super::*;
    use std::path::Path;

    pub(crate) fn with_home(tag: &str, test: impl FnOnce(&Path)) {
        let home = std::env::temp_dir().join(format!(
            "kz-prefs-failure-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::settings::with_kanzei_home(&home, || test(&home))
        }));
        std::fs::remove_dir_all(home).unwrap();
        result.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    }

    pub(crate) fn fixture(home: &Path) -> Value {
        let a = home.join("registered-A");
        let b = home.join("registered-B");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let a = a.display().to_string();
        let b = b.display().to_string();
        json!({
            "projects": [a, b], "current": a, "names": {&a: "Kept A", &b: "Kept B"},
            "theme": "dark", "work_priority": {&a: "high"},
            "process_auto_state": {format!("p1|{a}"): {"enabled": true}, format!("p2|{a}"): {"enabled": false}},
            "open_tools": [{"id": "kept", "label": "Kept tool", "command": "fixture-unused.exe", "args": ["{path}"]}]
        })
    }

    pub(crate) fn damaged_inputs(good: &Value) -> Vec<(&'static str, Vec<u8>)> {
        let normal = serde_json::to_vec(good).unwrap();
        let mut utf8 = normal.clone();
        utf8.push(0xff);
        let mut json = normal;
        json.pop();
        let mut typed = good.clone();
        typed["auto_max"] = json!("not-a-u32");
        vec![
            ("utf8", utf8),
            ("json", json),
            ("typed", serde_json::to_vec(&typed).unwrap()),
        ]
    }

    pub(crate) fn assert_rejected_unchanged(
        home: &Path,
        bytes: &[u8],
        write: impl FnOnce() -> Result<(), String>,
    ) {
        let path = home.join("app.json");
        std::fs::write(&path, bytes).unwrap();
        let result = write();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "failed original read must not publish default preferences"
        );
        let error = result.expect_err(
            "a failed read must be reported rather than accepted as a successful save/noop",
        );
        assert!(error.contains("app.json"), "{error}");
        assert!(
            error.contains("读取偏好文件") || error.contains("解析偏好文件"),
            "{error}"
        );
    }

    fn save_theme() -> Result<(), String> {
        ui_prefs_set(
            Some("light".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    fn rejected_theme(case: &str) {
        with_home(case, |home| {
            let good = fixture(home);
            let bytes = damaged_inputs(&good)
                .into_iter()
                .find(|(name, _)| *name == case)
                .unwrap()
                .1;
            assert_rejected_unchanged(home, &bytes, save_theme);
            assert!(
                load_prefs().projects.is_empty(),
                "readonly best-effort contract remains unchanged"
            );
        });
    }

    #[test]
    fn ui_setter_keeps_invalid_utf8_original_bytes() {
        rejected_theme("utf8");
    }

    #[test]
    fn ui_setter_keeps_invalid_json_original_bytes() {
        rejected_theme("json");
    }

    #[test]
    fn ui_setter_keeps_invalid_typed_field_original_bytes() {
        rejected_theme("typed");
    }

    #[test]
    fn ui_setter_initializes_not_found_and_preserves_normal_unrelated_fields() {
        with_home("controls", |home| {
            assert!(!home.join("app.json").exists());
            save_theme().unwrap();
            assert_eq!(
                load_prefs_for_write().unwrap().theme.as_deref(),
                Some("light")
            );
            let good = fixture(home);
            std::fs::write(home.join("app.json"), serde_json::to_vec(&good).unwrap()).unwrap();
            save_theme().unwrap();
            let after: Value =
                serde_json::from_slice(&std::fs::read(home.join("app.json")).unwrap()).unwrap();
            assert_eq!(after["theme"], "light");
            for field in [
                "projects",
                "names",
                "open_tools",
                "work_priority",
                "process_auto_state",
            ] {
                assert_eq!(after[field], good[field], "{field}");
            }
        });
    }

    #[test]
    fn ui_setter_refuses_io_read_failure_before_attempting_replacement() {
        with_home("io", |home| {
            std::fs::create_dir(home.join("app.json")).unwrap();
            let error = save_theme().unwrap_err();
            assert!(error.contains("读取偏好文件"), "{error}");
            assert!(home.join("app.json").is_dir());
        });
    }
}

#[cfg(test)]
mod run_control_delta_tests {
    use super::*;
    use crate::prefs::failure_tests::with_home;

    fn save_controls(priority: Option<Value>, auto: Option<Value>) {
        ui_prefs_set(
            None,
            None,
            priority.map(|value| serde_json::from_value(value).unwrap()),
            None,
            None,
            auto.map(|value| serde_json::from_value(value).unwrap()),
            None,
            None,
            None,
        )
        .unwrap();
    }

    #[test]
    fn real_setter_preserves_independent_project_priority_deltas() {
        with_home("a7-priority", |_| {
            save_controls(
                Some(json!({"A": "defect-first", "B": "defect-first"})),
                Some(json!({"p|A": {"enabled": true, "mode": "dev-pair"}})),
            );
            let a = ui_prefs_get();
            let b = ui_prefs_get();
            assert_eq!(a, b, "two windows start from the same snapshot");
            save_controls(Some(json!({"A": "requirement-first"})), None);
            save_controls(Some(json!({"B": "requirement-first"})), None);
            save_controls(Some(json!({})), Some(json!({})));
            let restored = ui_prefs_get();
            assert_eq!(restored["work_priority"]["A"], "requirement-first");
            assert_eq!(restored["work_priority"]["B"], "requirement-first");
            assert_eq!(restored["process_auto_state"], a["process_auto_state"]);
            let raw: Value = serde_json::from_slice(&std::fs::read(prefs_path()).unwrap()).unwrap();
            assert_eq!(raw["work_priority"], restored["work_priority"]);
        });
    }

    #[test]
    fn real_setter_preserves_process_fields_and_only_explicit_retirement_deletes() {
        with_home("a7-process", |_| {
            save_controls(
                Some(json!({"A": "requirement-first"})),
                Some(json!({
                    "p|A": {"enabled": true, "paused": false, "stopAfterRound": false, "maxRounds": 7, "mode": "dev-pair"},
                    "p|B": {"enabled": false, "maxRounds": 9},
                    "p|C": {"enabled": true, "mode": "dev-auto"}
                })),
            );
            let a = ui_prefs_get();
            let b = ui_prefs_get();
            assert_eq!(a, b);
            save_controls(None, Some(json!({"p|A": {"paused": true}})));
            save_controls(
                None,
                Some(json!({"p|A": {"enabled": false}, "p|B": {"enabled": true}})),
            );
            save_controls(None, Some(json!({"p|B": null})));
            save_controls(None, Some(json!({})));
            let restored = ui_prefs_get();
            assert_eq!(restored["process_auto_state"]["p|A"]["paused"], true);
            assert_eq!(restored["process_auto_state"]["p|A"]["enabled"], false);
            assert_eq!(
                restored["process_auto_state"]["p|A"]["stopAfterRound"],
                false
            );
            assert_eq!(restored["process_auto_state"]["p|A"]["maxRounds"], 7);
            assert_eq!(restored["process_auto_state"]["p|A"]["mode"], "dev-pair");
            assert!(!restored["process_auto_state"]
                .as_object()
                .unwrap()
                .contains_key("p|B"));
            assert_eq!(
                restored["process_auto_state"]["p|C"],
                a["process_auto_state"]["p|C"]
            );
            assert_eq!(restored["work_priority"], a["work_priority"]);
            let raw: Value = serde_json::from_slice(&std::fs::read(prefs_path()).unwrap()).unwrap();
            assert_eq!(raw["process_auto_state"], restored["process_auto_state"]);
        });
    }

    #[test]
    fn real_concurrent_setters_merge_distinct_fields_under_the_existing_write_guard() {
        with_home("a7-concurrent", |_| {
            save_controls(
                None,
                Some(json!({"p|A": {"enabled": true, "paused": false, "mode": "dev-pair"}})),
            );
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            std::thread::scope(|scope| {
                for patch in [
                    json!({"p|A": {"paused": true}}),
                    json!({"p|A": {"enabled": false}}),
                ] {
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        save_controls(None, Some(patch));
                    });
                }
            });
            let restored = ui_prefs_get();
            assert_eq!(restored["process_auto_state"]["p|A"]["enabled"], false);
            assert_eq!(restored["process_auto_state"]["p|A"]["paused"], true);
            assert_eq!(restored["process_auto_state"]["p|A"]["mode"], "dev-pair");
        });
    }
}

#[cfg(test)]
mod workspace_state_tests {
    use super::*;

    fn with_home(tag: &str, run: impl FnOnce()) {
        let home = std::env::temp_dir().join(format!(
            "kz-workspace-prefs-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::settings::with_kanzei_home(&home, run)
        }));
        std::fs::remove_dir_all(home).unwrap();
        result.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    }

    fn save_workspace(patch: Value) {
        ui_prefs_set(
            None,
            None,
            None,
            None,
            None,
            None,
            Some(serde_json::from_value(patch).unwrap()),
            None,
            None,
        )
        .unwrap();
    }

    #[test]
    fn independent_windows_preserve_projects_through_real_prefs_set_and_reload() {
        with_home("projects", || {
            ui_prefs_set(
                Some("dark".into()),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(json!({"splits": {"sidebar": 320}})),
                None,
            )
            .unwrap();
            save_workspace(json!({
                "project-A": {"dev": {"view": "chat", "process_id": "d|A"}},
                "project-B": {"dev": {"view": "chat", "process_id": "d|B"}}
            }));
            // Two windows read the same snapshot, then send only their own changes.
            let a = ui_prefs_get();
            let b = ui_prefs_get();
            assert_eq!(a["workspace_state"], b["workspace_state"]);
            save_workspace(json!({"project-A": {"dev": {"view": "lines"}}}));
            save_workspace(json!({"project-B": {"dev": {"view": "arch"}}}));
            let restored = ui_prefs_get();
            assert_eq!(
                restored["workspace_state"]["project-A"]["dev"]["view"],
                "lines"
            );
            assert_eq!(
                restored["workspace_state"]["project-B"]["dev"]["view"],
                "arch"
            );
            assert_eq!(
                restored["workspace_state"]["project-A"]["dev"]["process_id"],
                "d|A"
            );
            assert_eq!(
                restored["workspace_state"]["project-B"]["dev"]["process_id"],
                "d|B"
            );
            assert_eq!(restored["theme"], "dark");
            assert_eq!(restored["ui_layout"]["splits"]["sidebar"], 320);
            let raw: Value =
                serde_json::from_str(&std::fs::read_to_string(prefs_path()).unwrap()).unwrap();
            assert_eq!(raw["workspace_state"], restored["workspace_state"]);
        });
    }

    #[test]
    fn independent_windows_preserve_topics_fields_and_null_through_real_prefs_set() {
        with_home("topics", || {
            save_workspace(json!({"@research-library": {
                "space": "dev", "research": {"category": "research", "process_id": "p|old"},
                "topic_states": {"origin": {"page": "overview"}}
            }}));
            let a = ui_prefs_get();
            let b = ui_prefs_get();
            assert_eq!(a["workspace_state"], b["workspace_state"]);
            save_workspace(json!({"@research-library": {
                "research": {"topic_id": "alpha", "page": "writing", "process_id": null},
                "topic_states": {"alpha": {"page": "writing", "process_id": null}}
            }}));
            save_workspace(json!({"@research-library": {
                "research": {"topic_id": "beta", "page": "reading", "process_id": null},
                "topic_states": {"beta": {"page": "reading", "process_id": null}}
            }}));
            save_workspace(
                json!({"@research-library": {"topic_states": {"alpha": {"view": "chat"}}}}),
            );
            let restored = ui_prefs_get();
            let library = &restored["workspace_state"]["@research-library"];
            assert_eq!(library["topic_states"]["origin"]["page"], "overview");
            assert_eq!(library["topic_states"]["alpha"]["page"], "writing");
            assert_eq!(library["topic_states"]["alpha"]["view"], "chat");
            assert_eq!(library["topic_states"]["beta"]["page"], "reading");
            assert_eq!(library["research"]["category"], "research");
            assert_eq!(library["research"]["process_id"], Value::Null);
            assert_eq!(library["topic_states"]["alpha"]["process_id"], Value::Null);
            assert!(library["research"]
                .as_object()
                .unwrap()
                .contains_key("process_id"));
            assert!(library["topic_states"]["alpha"]
                .as_object()
                .unwrap()
                .contains_key("process_id"));
            assert_eq!(library["space"], "dev");
            let raw: Value =
                serde_json::from_str(&std::fs::read_to_string(prefs_path()).unwrap()).unwrap();
            assert_eq!(raw["workspace_state"], restored["workspace_state"]);
        });
    }
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
    fn 偏好键去掉_verbatim_前缀_并存时保留去前缀的值() {
        let mut prefs = AppPrefs::default();
        prefs
            .process_auto_state
            .insert(r"d|\\?\C:\proj".into(), json!({ "enabled": true }));
        prefs.process_auto_state.insert(
            r"p2|\\?\C:\proj".into(),
            json!({ "enabled": true, "paused": true }),
        );
        prefs
            .process_auto_state
            .insert(r"p2|C:\proj".into(), json!({ "enabled": false }));
        prefs
            .work_priority
            .insert(r"\\?\C:\proj".into(), "requirement-first".into());
        let long = format!(r"d|\\?\C:\{}", "x".repeat(300));
        prefs.process_auto_state.insert(long.clone(), json!({}));
        simplify_pref_keys(&mut prefs);
        assert_eq!(
            prefs.process_auto_state[r"d|C:\proj"]["enabled"],
            json!(true)
        );
        // 并存:去前缀的键是本版本写的(较新),带前缀的是旧存档/回灌的陈值,丢掉。
        assert_eq!(
            prefs.process_auto_state[r"p2|C:\proj"],
            json!({ "enabled": false })
        );
        assert!(!prefs.process_auto_state.contains_key(r"p2|\\?\C:\proj"));
        assert!(
            prefs.process_auto_state.contains_key(&long),
            "超长路径保持原样"
        );
        assert_eq!(prefs.work_priority[r"C:\proj"], "requirement-first");
    }

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
    fn 项目路径比较忽略前缀斜杠方向末尾分隔符与大小写() {
        assert!(same_project_path(r"\\?\C:\Proj\a", r"c:/proj/a/"));
        assert!(same_project_path(r"C:\proj", r"C:\proj\"));
        assert!(!same_project_path(r"C:\proj", r"C:\proj2"));
        assert!(!same_project_path(r"C:\proj", r"D:\proj"));
    }

    #[test]
    fn 删除对话清掉鞭挞设置与置顶排序_不误伤别的对话() {
        let mut prefs = AppPrefs::default();
        prefs
            .process_auto_state
            .insert(r"p2|C:\proj".into(), json!({ "enabled": true }));
        prefs
            .process_auto_state
            .insert(r"p3|C:\proj".into(), json!({ "enabled": true }));
        prefs.ui_layout = json!({
            "session_pins": { r"p2|C:\proj": true, r"p3|C:\proj": true },
            "session_order": {
                r"C:\proj": [r"p3|C:\proj", r"p2|C:\proj"],
                r"C:\other": [r"p2|C:\other"],
            },
            "sidebar_open": { r"C:\proj": true },
        });
        // 带 `\\?\` 前缀的 id 写法同样命中。
        assert!(purge_process_prefs(&mut prefs, r"p2|\\?\C:\proj"));
        assert!(!prefs.process_auto_state.contains_key(r"p2|C:\proj"));
        assert!(prefs.process_auto_state.contains_key(r"p3|C:\proj"));
        assert_eq!(
            prefs.ui_layout["session_pins"],
            json!({ r"p3|C:\proj": true })
        );
        assert_eq!(
            prefs.ui_layout["session_order"][r"C:\proj"],
            json!([r"p3|C:\proj"])
        );
        assert_eq!(
            prefs.ui_layout["session_order"][r"C:\other"],
            json!([r"p2|C:\other"]),
            "别的项目里同号的对话不受影响"
        );
        assert_eq!(prefs.ui_layout["sidebar_open"][r"C:\proj"], json!(true));
        // 再删一次没有任何改动,调用方据此省掉写盘。
        assert!(!purge_process_prefs(&mut prefs, r"p2|C:\proj"));
    }

    #[test]
    fn 移除项目清掉全部残留_别的项目原样保留() {
        let mut prefs = AppPrefs::default();
        for path in [r"C:\a", r"C:\b"] {
            prefs.names.insert(path.into(), format!("名 {path}"));
            prefs
                .workspace_state
                .insert(path.into(), json!({ "space": "dev" }));
            prefs
                .work_priority
                .insert(path.into(), "requirement-first".into());
            for prefix in ["d", "p2"] {
                prefs
                    .process_auto_state
                    .insert(format!("{prefix}|{path}"), json!({ "enabled": true }));
            }
        }
        prefs.ui_layout = json!({
            "session_pins": { r"p2|C:\a": true, r"p2|C:\b": true },
            "session_order": { r"C:\a": [r"p2|C:\a"], r"C:\b": [r"p2|C:\b"] },
            "sidebar_open": { r"C:\a": true, r"C:\b": true },
            "splits": { "sidebar": 300 },
        });
        purge_project_prefs(&mut prefs, r"c:\A");
        assert_eq!(prefs.names.keys().collect::<Vec<_>>(), [r"C:\b"]);
        assert_eq!(prefs.workspace_state.keys().collect::<Vec<_>>(), [r"C:\b"]);
        assert_eq!(prefs.work_priority.keys().collect::<Vec<_>>(), [r"C:\b"]);
        let mut left = prefs.process_auto_state.keys().cloned().collect::<Vec<_>>();
        left.sort();
        assert_eq!(left, [r"d|C:\b", r"p2|C:\b"]);
        assert_eq!(prefs.ui_layout["session_pins"], json!({ r"p2|C:\b": true }));
        assert_eq!(
            prefs.ui_layout["session_order"],
            json!({ r"C:\b": [r"p2|C:\b"] })
        );
        assert_eq!(prefs.ui_layout["sidebar_open"], json!({ r"C:\b": true }));
        assert_eq!(
            prefs.ui_layout["splits"]["sidebar"],
            json!(300),
            "无关分区不动"
        );
    }

    #[test]
    fn open_tools_旧文件无字段回落空_空列表不写出_往返保真() {
        let old: AppPrefs = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert!(old.open_tools.is_empty());
        assert!(!serde_json::to_string(&old).unwrap().contains("open_tools"));
        let prefs: AppPrefs = serde_json::from_str(
            r#"{"open_tools":[{"id":"zed","label":"Zed","command":"zed","args":["{path}"]},
                              {"id":"x","label":"X","command":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(prefs.open_tools[0].args, ["{path}"]);
        assert!(
            prefs.open_tools[1].args.is_empty(),
            "缺 args 回落空,由保存时补 {{path}}"
        );
        let restored: AppPrefs =
            serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
        assert_eq!(restored.open_tools, prefs.open_tools);
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
