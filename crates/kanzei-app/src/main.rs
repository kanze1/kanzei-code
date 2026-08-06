//! kzapp — kanzei Tauri 桌面端。
//! 前端为静态页面(ui/),经 command + event 通信:
//! run_prompt → kz:* 流式事件;kz:ask 权限弹窗 → answer_ask;stop_run 中止;
//! projects_* 多项目管理(~/.kanzei/app.json);settings_* 全局配置表单。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{Emitter, State, Window};
use tokio::sync::oneshot;

use kanzei_core::{run_once, AskFuture, RunEvent, RunnerConfig};
use kanzei_harness::{
    ConfigComponent, Harness, KanzeiConfig, MarkdownComponent, ProfileKind, ResolveCtx, ToolCtx,
};
use kanzei_llm::{LlmClient, ProxyConfig};
use kanzei_tools::docstore::{DocStore, DEFECTS, REQUIREMENTS};
use kanzei_tools::{BaseComponent, DevProfile, ResearchProfile};

#[derive(Default)]
struct AppState {
    asks: Arc<Mutex<HashMap<u64, oneshot::Sender<bool>>>>,
    ask_seq: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    current_run: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            projects_get,
            projects_add,
            projects_pick,
            projects_remove,
            projects_select,
            docs_snapshot,
            run_prompt,
            stop_run,
            answer_ask,
            settings_get,
            settings_save,
            settings_open
        ])
        .run(tauri::generate_context!())
        .expect("error while running kanzei app");
}

// ---------- 多项目管理(~/.kanzei/app.json) ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AppPrefs {
    #[serde(default)]
    projects: Vec<String>,
    #[serde(default)]
    current: Option<String>,
}

fn prefs_path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".kanzei").join("app.json")
}

fn load_prefs() -> AppPrefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_prefs(prefs: &AppPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(prefs).unwrap_or_default());
}

#[tauri::command]
fn projects_get() -> AppPrefs {
    let mut prefs = load_prefs();
    prefs.projects.retain(|p| Path::new(p).is_dir());
    if prefs.projects.is_empty() {
        if let Ok(cwd) = std::env::current_dir() {
            prefs.projects.push(cwd.display().to_string());
        }
    }
    if prefs.current.as_deref().map(|c| !Path::new(c).is_dir()).unwrap_or(true) {
        prefs.current = prefs.projects.first().cloned();
    }
    save_prefs(&prefs);
    prefs
}

#[tauri::command]
fn projects_add(path: String) -> Result<AppPrefs, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("目录不存在: {path}"));
    }
    let canonical = dir.canonicalize().map(strip_verbatim).unwrap_or(path.clone());
    let mut prefs = load_prefs();
    if !prefs.projects.contains(&canonical) {
        prefs.projects.push(canonical.clone());
    }
    prefs.current = Some(canonical);
    save_prefs(&prefs);
    Ok(projects_get())
}

#[tauri::command]
async fn projects_pick() -> Result<Option<AppPrefs>, String> {
    let picked = rfd::AsyncFileDialog::new().pick_folder().await;
    match picked {
        Some(handle) => projects_add(handle.path().display().to_string()).map(Some),
        None => Ok(None),
    }
}

#[tauri::command]
fn projects_remove(path: String) -> AppPrefs {
    let mut prefs = load_prefs();
    prefs.projects.retain(|p| p != &path);
    if prefs.current.as_deref() == Some(path.as_str()) {
        prefs.current = prefs.projects.first().cloned();
    }
    save_prefs(&prefs);
    projects_get()
}

#[tauri::command]
fn projects_select(path: String) -> AppPrefs {
    let mut prefs = load_prefs();
    if prefs.projects.contains(&path) {
        prefs.current = Some(path);
    }
    save_prefs(&prefs);
    prefs
}

/// Windows canonicalize 会带 \\?\ 前缀,展示前剥掉。
fn strip_verbatim(p: PathBuf) -> String {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

// ---------- 项目文档 ----------

#[tauri::command]
fn docs_snapshot(project_dir: String) -> serde_json::Value {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let load = |kind: &'static kanzei_tools::docstore::DocKind| -> Vec<serde_json::Value> {
        DocStore::open(&root, kind)
            .load()
            .unwrap_or_default()
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "title": e.title,
                    "status": e.status,
                    "severity": e.severity,
                    "closed": kind.terminal.contains(&e.status.as_str()),
                })
            })
            .collect()
    };
    json!({
        "root": root.display().to_string(),
        "requirements": load(&REQUIREMENTS),
        "defects": load(&DEFECTS),
    })
}

// ---------- 设置(全局 kanzei.toml 表单) ----------

fn global_config_path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".kanzei").join("kanzei.toml")
}

#[tauri::command]
fn settings_get() -> serde_json::Value {
    let path = global_config_path();
    let mut config: KanzeiConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    config.fill_defaults();
    let providers: Vec<serde_json::Value> = config
        .providers
        .iter()
        .map(|(name, p)| {
            let key_present = p
                .api_key_env
                .as_deref()
                .map(|env| std::env::var(env).map(|v| !v.is_empty()).unwrap_or(false));
            json!({
                "name": name,
                "protocol": p.protocol,
                "baseUrl": p.base_url,
                "apiKeyEnv": p.api_key_env,
                "keyPresent": key_present,
                "auth": p.auth,
            })
        })
        .collect();
    json!({
        "path": path.display().to_string(),
        "primary": config.models.primary,
        "fast": config.models.fast,
        "proxy": config.proxy.unwrap_or_else(|| "env".into()),
        "profileDefault": config.profile.default.unwrap_or_else(|| "dev".into()),
        "providers": providers,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPayload {
    primary: String,
    fast: String,
    proxy: String,
    profile_default: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    providers: Vec<ProviderPayload>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderPayload {
    name: String,
    protocol: String,
    base_url: String,
    api_key_env: Option<String>,
    /// 特殊认证透传(codex);表单只读展示,不丢字段。
    #[serde(default)]
    auth: Option<String>,
}

#[tauri::command]
fn settings_save(payload: SettingsPayload) -> Result<(), String> {
    let mut config = KanzeiConfig::default();
    config.models.primary = Some(payload.primary.trim().to_string()).filter(|s| !s.is_empty());
    config.models.fast = Some(payload.fast.trim().to_string()).filter(|s| !s.is_empty());
    config.proxy = match payload.proxy.trim() {
        "" | "env" => None,
        other => Some(other.to_string()),
    };
    config.profile.default = payload
        .profile_default
        .or(payload.profile)
        .filter(|p| p == "dev" || p == "research");
    for p in payload.providers {
        let name = p.name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        config.providers.insert(
            name,
            kanzei_harness::config::ProviderConfig {
                protocol: p.protocol.trim().to_string(),
                base_url: p.base_url.trim().trim_end_matches('/').to_string(),
                api_key_env: p.api_key_env.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
                auth: p.auth.filter(|s| !s.is_empty()),
            },
        );
    }
    let text = toml::to_string_pretty(&config).map_err(|e| e.to_string())?;
    let path = global_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

#[tauri::command]
fn settings_open() -> Result<(), String> {
    let path = global_config_path();
    if !path.is_file() {
        settings_save(SettingsPayload {
            primary: String::new(),
            fast: String::new(),
            proxy: "env".into(),
            profile_default: None,
            profile: None,
            providers: vec![],
        })?;
    }
    std::process::Command::new("cmd")
        .args(["/c", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- 运行 ----------

#[tauri::command]
fn answer_ask(state: State<'_, AppState>, id: u64, allow: bool) {
    if let Some(sender) = state.asks.lock().unwrap().remove(&id) {
        let _ = sender.send(allow);
    }
}

#[tauri::command]
fn stop_run(window: Window, state: State<'_, AppState>) {
    if let Some(handle) = state.current_run.lock().unwrap().take() {
        handle.abort();
    }
    // 挂起的权限询问一并作废(否则 runner 已死、弹窗还悬着)。
    state.asks.lock().unwrap().clear();
    state.running.store(false, Ordering::SeqCst);
    let _ = window.emit("kz:stopped", json!({}));
}

#[tauri::command]
async fn run_prompt(
    window: Window,
    state: State<'_, AppState>,
    prompt: String,
    project_dir: String,
    profile: Option<String>,
) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("已有任务在运行".into());
    }
    let asks = state.asks.clone();
    let ask_seq = state.ask_seq.clone();
    let running = state.running.clone();
    let current_run = state.current_run.clone();

    let handle = tauri::async_runtime::spawn(async move {
        let result = run_task(&window, asks, ask_seq, prompt, project_dir, profile).await;
        if let Err(e) = result {
            let message = e.to_string();
            let lower = message.to_lowercase();
            let hint = if ["timed out", "timeout", "connect", "dns", "connection"]
                .iter()
                .any(|k| lower.contains(k))
            {
                "\n提示:疑似网络不通。若需代理,在设置页把代理设为「指定地址」(如 http://127.0.0.1:12000)后重试;本地模型(ollama)不受代理影响。"
            } else {
                ""
            };
            let _ = window.emit("kz:error", json!({ "message": format!("{message}{hint}") }));
        }
        running.store(false, Ordering::SeqCst);
    });
    *state.current_run.lock().unwrap() = Some(handle);
    Ok(())
}

async fn run_task(
    window: &Window,
    asks: Arc<Mutex<HashMap<u64, oneshot::Sender<bool>>>>,
    ask_seq: Arc<AtomicU64>,
    prompt: String,
    project_dir: String,
    profile: Option<String>,
) -> anyhow::Result<()> {
    let cwd = PathBuf::from(&project_dir);
    anyhow::ensure!(cwd.is_dir(), "工作目录不存在: {project_dir}");

    let config = Arc::new(KanzeiConfig::load(&cwd)?);
    let profile: ProfileKind = match profile.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => p.parse().map_err(|e: String| anyhow::anyhow!(e))?,
        None => config.default_profile(),
    };
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let rctx = ResolveCtx {
        profile,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };

    let mut harness = Harness::default();
    harness
        .add(BaseComponent)
        .add(DevProfile)
        .add(ResearchProfile)
        .add(MarkdownComponent)
        .add(ConfigComponent);
    let snapshot = harness.resolve(&rctx)?;
    let agent = snapshot.select_agent(None)?.clone();

    let resolved = config.resolve_model(&agent.model)?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy).await?;
    let client = LlmClient::new(&proxy)?;
    let runner_config = RunnerConfig { model: resolved.model.clone(), max_tokens: 8192 };
    let ctx = ToolCtx { cwd, project_root };

    let _ = window.emit(
        "kz:meta",
        json!({
            "profile": format!("{profile:?}").to_lowercase(),
            "agent": agent.name,
            "model": format!("{}:{}", resolved.provider_name, resolved.model),
        }),
    );

    let event_window = window.clone();
    let mut on_event = move |event: RunEvent| {
        let _ = match event {
            RunEvent::Text(text) => event_window.emit("kz:text", json!({ "text": text })),
            RunEvent::Reasoning(text) => event_window.emit("kz:reasoning", json!({ "text": text })),
            RunEvent::ToolStart { name, summary } => {
                event_window.emit("kz:tool-start", json!({ "name": name, "summary": summary }))
            }
            RunEvent::ToolEnd { name, ok, preview } => event_window.emit(
                "kz:tool-end",
                json!({ "name": name, "ok": ok, "preview": preview }),
            ),
            RunEvent::StepEnd { usage, .. } => event_window.emit(
                "kz:step",
                json!({
                    "input": usage.input, "output": usage.output,
                    "cacheRead": usage.cache_read, "cacheWrite": usage.cache_write,
                }),
            ),
        };
    };

    let ask_window = window.clone();
    let mut ask = move |action: String, resource: String| -> AskFuture {
        let (sender, receiver) = oneshot::channel();
        let id = ask_seq.fetch_add(1, Ordering::SeqCst);
        asks.lock().unwrap().insert(id, sender);
        let _ = ask_window.emit(
            "kz:ask",
            json!({ "id": id, "action": action, "resource": resource }),
        );
        Box::pin(async move { receiver.await.unwrap_or(false) })
    };

    let summary = run_once(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        &prompt,
        &mut on_event,
        &mut ask,
    )
    .await?;

    let _ = window.emit(
        "kz:done",
        json!({
            "steps": summary.steps,
            "halted": summary.halted_by_user,
            "input": summary.usage.input,
            "output": summary.usage.output,
            "cacheRead": summary.usage.cache_read,
            "cacheWrite": summary.usage.cache_write,
        }),
    );
    Ok(())
}
