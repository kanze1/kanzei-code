//! kzapp — kanzei Tauri 桌面端。
//! 前端为静态页面(ui/),经 command + event 通信:
//! run_prompt → kz:* 流式事件;kz:ask 权限弹窗 → answer_ask;stop_run 中止;
//! projects_* 多项目管理(~/.kanzei/app.json);settings_* 全局配置表单。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// 悬挂中的权限询问:除通道外携带上下文,支持"总是允许"落盘。
struct PendingAsk {
    sender: oneshot::Sender<kanzei_core::AskReply>,
    action: String,
    resource: String,
    project_root: PathBuf,
}

#[derive(Default)]
struct AppState {
    asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    ask_seq: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    current_run: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    /// 会话内多轮连续:窗口存活期间的完整消息历史(M2 落盘前的内存态方案)。
    conversation: Arc<Mutex<Vec<kanzei_llm::Message>>>,
    /// 历史所属项目;切换项目自动清空。
    conversation_project: Arc<Mutex<Option<String>>>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
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
            settings_open,
            app_info,
            models_list,
            docs_update,
            docs_open,
            summarize_chat,
            git_status,
            conventions_init,
            conversation_clear
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
    dirs::home_dir()
        .unwrap_or_default()
        .join(".kanzei")
        .join("app.json")
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
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(prefs).unwrap_or_default(),
    );
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
    if prefs
        .current
        .as_deref()
        .map(|c| !Path::new(c).is_dir())
        .unwrap_or(true)
    {
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
    let canonical = dir
        .canonicalize()
        .map(strip_verbatim)
        .unwrap_or(path.clone());
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
                    "fields": e.fields,
                    // 展开面板需要:合法的下一步状态(硬门禁同款规则)。
                    "nextStatuses": kind.statuses.iter()
                        .filter(|s| {
                            **s != e.status
                                && DocStore::open(&root, kind).transition_allowed(&e.status, s).is_ok()
                        })
                        .collect::<Vec<_>>(),
                })
            })
            .collect()
    };
    let conventions_path = root.join(CONVENTIONS_REL);
    let conventions = match std::fs::read_to_string(&conventions_path) {
        Ok(text) => json!({
            "exists": true,
            "headings": text.lines()
                .filter(|l| l.starts_with('#'))
                .map(|l| l.trim_start_matches('#').trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>(),
        }),
        Err(_) => json!({ "exists": false, "headings": [] }),
    };
    json!({
        "conventions": conventions,
        "root": root.display().to_string(),
        "requirements": load(&REQUIREMENTS),
        "defects": load(&DEFECTS),
    })
}

// ---------- 设置(全局 kanzei.toml 表单) ----------

fn global_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".kanzei")
        .join("kanzei.toml")
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
                "contextLimit": p.context_limit,
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
    #[serde(default)]
    context_limit: Option<u64>,
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
                api_key_env: p
                    .api_key_env
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                auth: p.auth.filter(|s| !s.is_empty()),
                context_limit: p.context_limit,
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
    hidden_command("cmd")
        .args(["/c", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 侧边栏直接改状态/关闭(走同一套 TrackerTool 硬门禁,不绕过状态机)。
#[tauri::command]
async fn docs_update(
    project_dir: String,
    kind: String,
    action: String,
    id: String,
    status: Option<String>,
) -> Result<String, String> {
    use kanzei_harness::Tool as _;
    use kanzei_tools::docstore::{DEFECTS as D, FINDINGS as F, REQUIREMENTS as R, SOURCES as S};
    use kanzei_tools::tracker::TrackerTool;
    let tool = match kind.as_str() {
        "req" => TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &R,
            requires_refs: None,
        },
        "defect" => TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &D,
            requires_refs: None,
        },
        "source" => TrackerTool {
            tool_name: "source",
            noun: "source",
            kind: &S,
            requires_refs: None,
        },
        "finding" => TrackerTool {
            tool_name: "finding",
            noun: "finding",
            kind: &F,
            requires_refs: Some(&S),
        },
        other => return Err(format!("unknown kind `{other}`")),
    };
    let mut input = json!({ "action": action, "id": id });
    if let Some(status) = status {
        input["status"] = json!(status);
    }
    let ctx = kanzei_harness::ToolCtx::new(PathBuf::from(&project_dir));
    let output = tool.execute(input, &ctx).await;
    if output.is_error {
        Err(output.content)
    } else {
        Ok(output.content)
    }
}

/// 用系统默认程序打开文档原文(requirements.md / defects.md)。
#[tauri::command]
fn docs_open(project_dir: String, kind: String) -> Result<(), String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let rel = match kind.as_str() {
        "req" => kanzei_tools::docstore::REQUIREMENTS.rel_path,
        "defect" => kanzei_tools::docstore::DEFECTS.rel_path,
        "conventions" => CONVENTIONS_REL,
        other => return Err(format!("unknown kind `{other}`")),
    };
    let path = root.join(rel);
    if !path.is_file() {
        return Err(format!("文档还不存在:{}", path.display()));
    }
    hidden_command("cmd")
        .args(["/c", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// git 概览:分支 + 未提交改动数(状态栏显示)。
#[tauri::command]
async fn git_status(project_dir: String) -> Result<serde_json::Value, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    tokio::task::spawn_blocking(move || {
        let run = |args: &[&str]| -> Option<String> {
            let out = hidden_command("git")
                .args(args)
                .current_dir(&root)
                .output()
                .ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"]);
        let changes = run(&["status", "--porcelain"])
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);
        let last = run(&["log", "-1", "--format=%h %s"]);
        json!({ "branch": branch, "changes": changes, "last": last })
    })
    .await
    .map_err(|e| e.to_string())
}

const CONVENTIONS_REL: &str = ".kanzei/project/conventions.md";

/// 桌面端调用外部程序时禁止创建控制台窗口(Windows GUI 应用不应闪出黑框)。
fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

/// 开发规范模板(不存在时一键创建;用户手写维护,agent 只读注入)。
#[tauri::command]
fn conventions_init(project_dir: String) -> Result<String, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let path = root.join(CONVENTIONS_REL);
    if path.is_file() {
        return Ok(path.display().to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &path,
        "# 开发规范\n\n## 代码风格\n- \n\n## 提交规范\n- \n\n## 测试要求\n- \n\n## 禁止事项\n- \n",
    )
    .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// 对话总结:fast 模型生成纪要并存档到 .kanzei/summaries/。
/// fast 模型跑一段总结(手动「总结」与 R-021 自动压缩共用)。
async fn fast_summarize(cwd: &Path, transcript: &str) -> Result<String, String> {
    use futures::StreamExt;
    let config = KanzeiConfig::load(cwd).map_err(|e| e.to_string())?;
    let resolved = config.resolve_model("fast").map_err(|e| e.to_string())?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let request = kanzei_llm::LlmRequest {
        model: resolved.model.clone(),
        system: vec![
            "把下面的人机协作对话记录总结成简洁的中文纪要:做了什么、改了哪些文件、\
             结论、遗留问题/下一步。markdown 列表,300 字以内。"
                .into(),
        ],
        messages: vec![kanzei_llm::Message::user_text(transcript)],
        tools: vec![],
        max_tokens: 2048,
        temperature: None,
    };
    let mut stream = client
        .stream(&route, &request)
        .await
        .map_err(|e| e.to_string())?;
    let mut summary = String::new();
    while let Some(event) = stream.next().await {
        if let kanzei_llm::LlmEvent::TextDelta { text, .. } = event.map_err(|e| e.to_string())? {
            summary.push_str(&text);
        }
    }
    if summary.trim().is_empty() {
        return Err("模型没有产出总结(fast 模型是否在运行?)".into());
    }
    Ok(summary)
}

/// 压缩用的对话文本化(工具结果截断,总量有界)。
fn render_transcript(messages: &[kanzei_llm::Message]) -> String {
    let mut out = String::new();
    'outer: for message in messages {
        for part in &message.parts {
            match part {
                kanzei_llm::Part::Text { text } => {
                    out.push_str(match message.role {
                        kanzei_llm::Role::User => "[用户] ",
                        kanzei_llm::Role::Assistant => "[助手] ",
                    });
                    out.push_str(text);
                    out.push('\n');
                }
                kanzei_llm::Part::ToolCall { name, input, .. } => {
                    out.push_str(&format!("[工具调用] {name} {input}\n"));
                }
                kanzei_llm::Part::ToolResult { content, .. } => {
                    let snippet: String = content.chars().take(1500).collect();
                    out.push_str(&format!("[工具结果] {snippet}\n"));
                }
                _ => {}
            }
            if out.len() > 100_000 {
                break 'outer;
            }
        }
    }
    out
}

#[tauri::command]
async fn summarize_chat(
    project_dir: String,
    transcript: String,
) -> Result<serde_json::Value, String> {
    let cwd = PathBuf::from(&project_dir);
    let summary = fast_summarize(&cwd, &transcript).await?;
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let dir = root.join(".kanzei").join("summaries");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("summary-{secs}.md"));
    std::fs::write(&path, &summary).map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "path": path.display().to_string() }))
}

// ---------- 运行 ----------

/// reply: "deny" | "once" | "always"。always 先把泛化规则写进项目配置再放行。
#[tauri::command]
fn answer_ask(window: Window, state: State<'_, AppState>, id: u64, reply: String) {
    let Some(pending) = state.asks.lock().unwrap().remove(&id) else {
        return;
    };
    let decision = match reply.as_str() {
        "always" => {
            let pattern =
                kanzei_harness::config::generalize_resource(&pending.action, &pending.resource);
            match kanzei_harness::config::append_allow_rule(
                &pending.project_root,
                &pending.action,
                &pattern,
            ) {
                Ok(path) => {
                    let _ = window.emit("kz:status", json!({
                        "stage": "权限",
                        "detail": format!("已记住:{} {pattern} → {}", pending.action, path.display()),
                    }));
                }
                Err(e) => {
                    let _ = window.emit(
                        "kz:status",
                        json!({
                            "stage": "权限",
                            "detail": format!("规则保存失败:{e}(本次仍放行)"),
                        }),
                    );
                }
            }
            kanzei_core::AskReply::AlwaysAllow
        }
        "once" => kanzei_core::AskReply::AllowOnce,
        _ => kanzei_core::AskReply::Deny,
    };
    let _ = pending.sender.send(decision);
}

/// 可选模型清单:角色(primary/fast)+ codex 三型号 + ollama 已装模型(动态查询)。
#[tauri::command]
async fn models_list(project_dir: Option<String>) -> Result<serde_json::Value, String> {
    let cwd = project_dir
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .ok_or("no working dir")?;
    let config = KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?;

    let mut items: Vec<serde_json::Value> = Vec::new();
    for role in ["primary", "fast"] {
        if let Ok(r) = config.resolve_model(role) {
            items.push(json!({
                "id": role,
                "label": format!("{role} → {}:{}", r.provider_name, r.model),
            }));
        }
    }
    for (name, p) in &config.providers {
        if p.auth.as_deref() == Some("codex") {
            // ChatGPT 订阅当前仅这三个型号(2026-08)。
            for m in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
                items.push(json!({"id": format!("{name}:{m}"), "label": format!("{name}:{m}")}));
            }
        } else if p.base_url.contains("11434") {
            let tags_url = format!("{}/api/tags", p.base_url.trim_end_matches("/v1"));
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .map_err(|e| e.to_string())?;
            if let Ok(resp) = client.get(&tags_url).send().await {
                if let Ok(v) = resp.json::<serde_json::Value>().await {
                    if let Some(models) = v["models"].as_array() {
                        for m in models {
                            if let Some(n) = m["name"].as_str() {
                                items.push(json!({
                                    "id": format!("{name}:{n}"),
                                    "label": format!("{name}:{n}"),
                                }));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(json!(items))
}

/// 开新对话:清空会话内多轮历史。
#[tauri::command]
fn conversation_clear(state: State<'_, AppState>) {
    state.conversation.lock().unwrap().clear();
    *state.conversation_project.lock().unwrap() = None;
}

#[tauri::command]
fn app_info() -> serde_json::Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "build": option_env!("KANZEI_BUILD_INFO").unwrap_or("dev"),
    })
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
    model: Option<String>,
) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("已有任务在运行".into());
    }
    let asks = state.asks.clone();
    let ask_seq = state.ask_seq.clone();
    let running = state.running.clone();
    let current_run = state.current_run.clone();
    let conversation = state.conversation.clone();
    let conversation_project = state.conversation_project.clone();

    let handle = tauri::async_runtime::spawn(async move {
        let result = run_task(
            &window,
            asks,
            ask_seq,
            prompt,
            project_dir,
            profile,
            model,
            conversation,
            conversation_project,
        )
        .await;
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
    asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    ask_seq: Arc<AtomicU64>,
    prompt: String,
    project_dir: String,
    profile: Option<String>,
    model_override: Option<String>,
    conversation: Arc<Mutex<Vec<kanzei_llm::Message>>>,
    conversation_project: Arc<Mutex<Option<String>>>,
) -> anyhow::Result<()> {
    // 阶段汇报:让前端每一步都有着落(用户反馈:要详细指示)。
    let stage = |name: &str, detail: String| {
        let _ = window.emit("kz:status", json!({ "stage": name, "detail": detail }));
    };

    let cwd = PathBuf::from(&project_dir);
    anyhow::ensure!(cwd.is_dir(), "工作目录不存在: {project_dir}");

    stage("配置", format!("加载 {}", cwd.display()));
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
    stage(
        "装配",
        format!(
            "harness 就绪:agent {} · {} 个工具",
            agent.name,
            snapshot.materialize_tools().len()
        ),
    );

    // 界面模型下拉直选优先于 agent 定义。
    let model_ref = model_override
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| agent.model.clone());
    let resolved = config.resolve_model(&model_ref)?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    stage(
        "鉴权",
        format!(
            "{}:{}{}",
            resolved.provider_name,
            resolved.model,
            if resolved.provider.auth.is_some() {
                "(订阅登录态,可能刷新令牌)"
            } else {
                ""
            }
        ),
    );
    let route = kanzei_core::build_route(&resolved, &proxy).await?;
    stage("请求", "已发起,等待模型响应…".into());
    let client = LlmClient::new(&proxy)?;
    let runner_config = RunnerConfig {
        model: resolved.model.clone(),
        max_tokens: 8192,
    };
    let ctx = ToolCtx { cwd, project_root };

    let session_id = kanzei_core::project_session_id(&ctx.project_root);
    let state_path = kanzei_core::project_state_path(&ctx.project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(&session_id, &ctx.project_root.display().to_string(), None)?;
    let input_id = format!(
        "input_{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    store.admit_input(
        &session_id,
        &input_id,
        &prompt,
        kanzei_core::Delivery::Queue,
    )?;
    store.append_event(
        &session_id,
        "prompt.admitted",
        &json!({ "input_id": input_id, "delivery": "queue" }),
    )?;
    let promoted = store
        .promote_next_queue(&session_id)?
        .ok_or_else(|| anyhow::anyhow!("无法提升已提交的桌面端输入"))?;
    store.append_event(
        &session_id,
        "prompt.promoted",
        &json!({ "input_id": promoted.input_id, "delivery": "queue" }),
    )?;
    store.set_status(&session_id, "running")?;
    store.append_event(
        &session_id,
        "session.status_changed",
        &json!({ "status": "running" }),
    )?;
    drop(store);

    let _ = window.emit(
        "kz:meta",
        json!({
            "profile": format!("{profile:?}").to_lowercase(),
            "agent": agent.name,
            "model": format!("{}:{}", resolved.provider_name, resolved.model),
            "contextLimit": resolved.provider.context_limit,
        }),
    );

    let event_window = window.clone();
    let mut on_event = move |event: RunEvent| {
        let _ = match event {
            RunEvent::TurnStart { step, max_steps } => {
                event_window.emit("kz:turn", json!({ "step": step, "maxSteps": max_steps }))
            }
            RunEvent::Text(text) => event_window.emit("kz:text", json!({ "text": text })),
            RunEvent::Reasoning(text) => event_window.emit("kz:reasoning", json!({ "text": text })),
            RunEvent::ToolStart { name, summary } => {
                event_window.emit("kz:tool-start", json!({ "name": name, "summary": summary }))
            }
            RunEvent::ToolEnd {
                name,
                ok,
                preview,
                display,
            } => event_window.emit(
                "kz:tool-end",
                json!({ "name": name, "ok": ok, "preview": preview, "display": display }),
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
    let ask_root = ctx.project_root.clone();
    let mut ask = move |action: String, resource: String| -> AskFuture {
        let (sender, receiver) = oneshot::channel();
        let id = ask_seq.fetch_add(1, Ordering::SeqCst);
        let remember = kanzei_harness::config::generalize_resource(&action, &resource);
        asks.lock().unwrap().insert(
            id,
            PendingAsk {
                sender,
                action: action.clone(),
                resource: resource.clone(),
                project_root: ask_root.clone(),
            },
        );
        let _ = ask_window.emit(
            "kz:ask",
            json!({ "id": id, "action": action, "resource": resource, "remember": remember }),
        );
        Box::pin(async move { receiver.await.unwrap_or(kanzei_core::AskReply::Deny) })
    };

    // 会话连续:同项目续上历史,换项目自动开新对话。
    let prior: Vec<kanzei_llm::Message> = {
        let mut proj = conversation_project.lock().unwrap();
        let mut conv = conversation.lock().unwrap();
        if proj.as_deref() != Some(project_dir.as_str()) {
            *proj = Some(project_dir.clone());
            conv.clear();
        }
        conv.clone()
    };
    if !prior.is_empty() {
        stage("会话", format!("延续对话({} 条历史消息)", prior.len()));
    }

    let run_result = run_once(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        &prompt,
        &prior,
        &mut on_event,
        &mut ask,
    )
    .await;
    let store = kanzei_core::SessionStore::open(&state_path)?;
    match &run_result {
        Ok(summary) => {
            store.set_status(&session_id, "idle")?;
            store.append_event(
                &session_id,
                "session.status_changed",
                &json!({ "status": "idle" }),
            )?;
            store.append_event(
                &session_id,
                "run.completed",
                &json!({
                    "steps": summary.steps,
                    "halted_by_user": summary.halted_by_user,
                    "input": summary.usage.input,
                    "output": summary.usage.output,
                }),
            )?;
        }
        Err(error) => {
            store.set_status(&session_id, "failed")?;
            store.append_event(
                &session_id,
                "session.status_changed",
                &json!({ "status": "failed" }),
            )?;
            store.append_event(
                &session_id,
                "run.failed",
                &json!({ "error": error.to_string() }),
            )?;
        }
    }
    let summary = run_result?;

    let history_len = summary.messages.len();
    *conversation.lock().unwrap() = summary.messages;

    // R-021 自动压缩:历史估算超过上下文上限 70% 时,fast 模型出纪要并替换历史。
    // 估算用 len/4(与压缩预检同源的粗粒度);失败保留原历史,绝不丢上下文。
    if let Some(limit) = resolved.provider.context_limit {
        let estimate = {
            let conv = conversation.lock().unwrap();
            serde_json::to_string(&*conv)
                .map(|s| s.len() as u64 / 4)
                .unwrap_or(0)
        };
        if estimate > limit * 7 / 10 {
            stage(
                "压缩",
                format!("历史约 {}k token,超过 {}k 的 70%,自动压缩中…", estimate / 1000, limit / 1000),
            );
            let transcript = {
                let conv = conversation.lock().unwrap();
                render_transcript(&conv)
            };
            match fast_summarize(&ctx.cwd, &transcript).await {
                Ok(digest) => {
                    *conversation.lock().unwrap() = vec![kanzei_llm::Message::user_text(format!(
                        "(系统:此前对话已自动压缩为以下纪要,基于它继续)\n{digest}"
                    ))];
                    let _ = window.emit("kz:compacted", json!({ "summary": digest }));
                }
                Err(e) => stage("压缩", format!("压缩失败:{e}(保留原历史)")),
            }
        }
    }

    let _ = window.emit(
        "kz:done",
        json!({
            "steps": summary.steps,
            "halted": summary.halted_by_user,
            "history": history_len,
            "input": summary.usage.input,
            "output": summary.usage.output,
            "cacheRead": summary.usage.cache_read,
            "cacheWrite": summary.usage.cache_write,
        }),
    );
    Ok(())
}
