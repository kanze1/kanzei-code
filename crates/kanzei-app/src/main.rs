//! kzapp — kanzei Tauri 桌面端(最小可用壳)。
//! 前端为静态页面(ui/),经 Tauri command + event 与 runner 通信:
//! run_prompt 发起 → kz:text/kz:tool-* 流式事件 → kz:ask 权限弹窗 → answer_ask 回填。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tauri::{Emitter, State, Window};
use tokio::sync::oneshot;

use kanzei_core::{run_once, AskFuture, RunEvent, RunnerConfig};
use kanzei_harness::{
    ConfigComponent, Harness, KanzeiConfig, MarkdownComponent, ProfileKind, ResolveCtx, ToolCtx,
};
use kanzei_llm::{LlmClient, ProxyConfig, Route};
use kanzei_tools::docstore::{DocStore, DEFECTS, REQUIREMENTS};
use kanzei_tools::{BaseComponent, DevProfile, ResearchProfile};

#[derive(Default)]
struct AppState {
    asks: Arc<Mutex<HashMap<u64, oneshot::Sender<bool>>>>,
    ask_seq: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
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
            default_project_dir,
            docs_snapshot,
            run_prompt,
            answer_ask
        ])
        .run(tauri::generate_context!())
        .expect("error while running kanzei app");
}

#[tauri::command]
fn default_project_dir() -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .display()
        .to_string()
}

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

#[tauri::command]
fn answer_ask(state: State<'_, AppState>, id: u64, allow: bool) {
    if let Some(sender) = state.asks.lock().unwrap().remove(&id) {
        let _ = sender.send(allow);
    }
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

    tauri::async_runtime::spawn(async move {
        let result = run_task(&window, asks, ask_seq, prompt, project_dir, profile).await;
        if let Err(e) = result {
            let _ = window.emit("kz:error", json!({ "message": e.to_string() }));
        }
        running.store(false, Ordering::SeqCst);
    });
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
    let profile: ProfileKind = match profile.as_deref() {
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
    let api_key = resolved
        .provider
        .api_key_env
        .as_deref()
        .and_then(|name| std::env::var(name).ok());
    let route = match resolved.provider.protocol.as_str() {
        "anthropic" => {
            let key = api_key.ok_or_else(|| {
                anyhow::anyhow!(
                    "provider `{}` 需要环境变量 {}",
                    resolved.provider_name,
                    resolved.provider.api_key_env.as_deref().unwrap_or("<api_key_env>")
                )
            })?;
            Route::anthropic_at(&resolved.provider.base_url, &key)
        }
        "openai" => Route::openai_at(&resolved.provider.base_url, api_key.as_deref()),
        other => anyhow::bail!("unknown protocol `{other}`"),
    };
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
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
