//! kzapp — kanzei Tauri 桌面端。
//! 前端为静态页面(ui/),经 command + event 通信:
//! run_prompt → kz:* 流式事件;kz:ask 权限弹窗 → answer_ask;stop_run 中止;
//! projects_* 多项目管理(~/.kanzei/app.json);settings_* 全局配置表单。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{Emitter, State, Window};
use tokio::sync::oneshot;

use kanzei_core::{run_once_with_parts, AskFuture, RunEvent, RunnerConfig};

mod files_view;
mod agent_container;
mod fast_model;
mod update;
mod memory;
mod state;
mod prefs;
mod projects;
mod processes;
mod mobile;
mod docs;
mod settings;
mod conversation;
mod harness_ext;

pub(crate) use settings::{LimitsPayload, ProviderPayload, SettingsPayload};
pub(crate) use settings::{
    global_config_path, settings_read_document, settings_save_at_path,
    settings_write_document, settings_apply_limits, settings_apply_providers, settings_apply_scalar_fields,
};

pub(crate) use update::{
    build_stamp, clear_stale_installer, image_replaced, image_stamp, installer_path,
    pending_path, release_is_newer, update_helper_path, update_log_at, validate_installer,
    wait_for_parent_exit, installed_cli_is_older,
};

use kanzei_harness::{
    ConfigComponent, Harness, KanzeiConfig, MarkdownComponent, ProfileKind, ResolveCtx, ToolCtx,
};
use kanzei_llm::{LlmClient, ProxyConfig};
use kanzei_tools::docstore::{DocStore, DEFECTS, FINDINGS, GOALS, REQUIREMENTS, SOURCES};
use kanzei_tools::tracker::schedule_for_display;
use kanzei_tools::{BaseComponent, DevProfile, ResearchProfile};

/// 运行轨迹的时间戳(Unix 毫秒)。
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

pub(crate) use state::{
    normalized_project_root, pending_ask_payload, process_info, process_session_id,
    prompt_attachment_parts, runtime_for, stop_runtime_and_finalize, take_pending_ask, ui_probe,
    ui_probe_result, with_session_id,
    AppState, LiveRun, MobileService, MobileServiceInfo, PendingAsk, ProcessHandle, ProcessInfo,
    PromptAttachment, SessionRuntime, WorktreeInfo, UI_PROBE_EMIT, UI_PROBES, UI_PROBE_SEQ,
    flush_live_run, ensure_default_process,
};

#[cfg(test)]
mod state_tests;

#[cfg(test)]
mod permission_tests;

#[cfg(test)]
mod conversation_tests;

#[cfg(test)]
mod process_tests;

#[cfg(test)]
mod update_tests_update;

fn main() {
    if update::startup_update() { return; }
    // 安装器只装得了 kzapp,CLI 得由这里搬到位——两者共用一个库,版本必须同步(D-175)。
    update::sync_bundled_cli();
    // 窗口创建之前自清孤儿 webview(D-171):上一个实例被强杀留下的
    // msedgewebview2 会锁住数据目录,不清的话本次启动必黑屏。
    update::cleanup_orphan_webviews();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    tauri::Builder::default()
        .manage(AppState::default())
        // UI 探针的出口:装一次,之后工具侧只认 UI_PROBE_EMIT。
        .setup(|app| {
            let handle = app.handle().clone();
            let _ = UI_PROBE_EMIT.set(Box::new(move |payload| {
                let _ = handle.emit("kz:ui-probe", payload);
            }));
            // 窗口从 tauri.conf.json 自动创建改为这里手动创建(R-101 E2 harness):
            // 配置里 `"create": false`,由 from_config 按同一份配置建窗口,生产路径
            // 行为不变;仅当环境变量 KANZEI_E2E_CDP 非空时注入 --remote-debugging-port
            // 打开 WebView2 DevTools 协议,供 E2 脚本通过 CDP 驱动真实 UI。
            let window_config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .ok_or_else(|| "tauri.conf.json 未配置任何窗口".to_string())?;
            let mut builder = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?;
            if let Ok(port) = std::env::var("KANZEI_E2E_CDP") {
                if !port.trim().is_empty() {
                    builder = builder.additional_browser_args(&format!("--remote-debugging-port={}", port.trim()));
                }
            }
            builder.build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ui_probe_result,
            files_view::files_snapshot,
            files_view::file_preview,
            files_view::files_annotate,
            projects::projects_get,
            projects::projects_add,
            projects::projects_init,
            projects::projects_rename,
            projects::projects_pick,
            projects::projects_remove,
            projects::projects_select,
            workspace_snapshot,
            docs::docs_snapshot,
            run_prompt,
            stop_run,
            answer_ask,
            pending_asks_get,
            settings::settings_get,
            settings::settings_save,
            settings::settings_open,
            export_pick_dir,
            export_project_data,
            settings::permission_rules_get,
            settings::permission_rule_delete,
            settings::provider_test,
            update::update_check_command,
            update::update_install_command,
            quick_req,
            defect_review,
            memory::memory_overview,
            memory::memory_entries,
            memory::memory_recalls,
            memory::memory_entry_delete,
            memory::memory_note_candidates,
            memory::memory_note_discard,
            run_metrics,
            projects::project_root_info,
            projects::project_detach,
            projects::projects_isolation_report,
            fast_model::fast_model_status,
            fast_model::fast_model_setup,
            memory::memory_entry_save,
            memory::memory_search_page,
            memory::memory_context_bill,
            memory::memory_consolidate,
            memory::memory_focus_get,
            memory::memory_focus_set,
            app_info,
            models_list,
            docs::docs_update,
            docs::docs_open,
            summarize_chat,
            git_status,
            conventions_init,
            conversation::conversation_clear,
            conversation::conversation_delete,
            docs::docs_read,
            conversation::conversation_get,
            conversation::conversation_trace_get,
            conversation::conversation_list,
            list_pending_inputs,
            cancel_input,
            project_files,
            processes::process_list,
            processes::process_create,
            processes::process_update,
            processes::process_close,
            processes::worktree_create,
            processes::worktree_diff,
            processes::worktree_merge,
            processes::worktree_discard,
            test_runs_snapshot,
            test_run_record,
            mobile::mobile_service_start,
            mobile::mobile_service_stop,
            agent_container::agent_container_create,
            agent_container::agent_container_upgrade,
            agent_container::agent_container_rollback
        ])
        .run(tauri::generate_context!())
        .expect("error while running kanzei app");
}

#[tauri::command]
fn list_pending_inputs(
    project_dir: String,
    process_id: Option<String>,
) -> Result<Vec<kanzei_core::AdmittedInput>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let state_path = kanzei_core::project_state_path(&root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let session_id = process_session_id(&root, process_id.as_deref());
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    store
        .list_pending_inputs(&session_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_input(
    project_dir: String,
    input_id: String,
    process_id: Option<String>,
) -> Result<bool, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let state_path = kanzei_core::project_state_path(&root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let session_id = process_session_id(&root, process_id.as_deref());
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    let cancelled = store
        .cancel_input(&session_id, &input_id)
        .map_err(|error| error.to_string())?;
    if cancelled {
        store
            .append_event(
                &session_id,
                "prompt.cancelled",
                &json!({ "input_id": input_id }),
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(cancelled)
}

#[tauri::command]
fn workspace_snapshot() -> Result<serde_json::Value, String> {
    let prefs = projects::projects_get();
    let mut projects = Vec::new();
    for path in &prefs.projects {
        // 与运行侧同源的项目根,否则工作区卡片的状态/历史与实际运行会话对不上(D-058)。
        let root = normalized_project_root(Path::new(path));
        let session_id = kanzei_core::project_session_id(&root);
        let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
            .map_err(|e| e.to_string())?;
        let session = store.create_session(&session_id, &root.display().to_string(), None)
            .map_err(|e| e.to_string())?;
        let conversations = conversation_list(path.clone(), None).unwrap_or_default();
        let pending = list_pending_inputs(path.clone(), None).unwrap_or_default();
        let recent = conversation_trace_get(path.clone(), None, None).unwrap_or_default();
        projects.push(json!({
            "path": path,
            "name": prefs.names.get(path).cloned().unwrap_or_else(|| projects::base_name_for_snapshot(path)),
            "current": prefs.current.as_deref() == Some(path.as_str()),
            "status": session.status,
            "updated_at": session.updated_at,
            "pending_count": pending.len(),
            "conversation": conversations.first(),
            "recent_activity": recent.into_iter().rev().take(8).collect::<Vec<_>>(),
        }));
    }
    Ok(json!({ "current": prefs.current, "projects": projects }))
}
// ---------- 项目文档 ----------

pub(crate) fn docs_snapshot(project_dir: String) -> serde_json::Value {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    // 自动归档:终态条目移入 *-archive.md,侧边栏与 agent 上下文只剩进行中的。
    for kind in [&REQUIREMENTS, &DEFECTS, &GOALS] {
        let _ = DocStore::open(&root, kind).archive_terminal();
    }
    let archived = |kind: &'static kanzei_tools::docstore::DocKind| -> usize {
        DocStore::open(&root, kind)
            .load_archive()
            .map_or(0, |a| a.len())
    };
    let archived_entries = |kind: &'static kanzei_tools::docstore::DocKind| -> Vec<serde_json::Value> {
        DocStore::open(&root, kind)
            .load_archive()
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "title": e.title,
                    "status": e.status,
                    "severity": e.severity,
                    "fields": e.fields,
                    "closed": true,
                })
            })
            .collect()
    };
    let load = |kind: &'static kanzei_tools::docstore::DocKind| -> Vec<serde_json::Value> {
        let store = DocStore::open(&root, kind);
        let entries = store.load().unwrap_or_default();
        let scheduled: Vec<(kanzei_tools::docstore::Entry, Vec<String>)> =
            if kind.rel_path == REQUIREMENTS.rel_path || kind.rel_path == DEFECTS.rel_path {
                schedule_for_display(&ToolCtx::new(root.clone()), kind, &entries)
                    .map(|items| {
                        items
                            .into_iter()
                            .map(|item| (item.entry, item.block_reasons))
                            .collect()
                    })
                    .unwrap_or_else(|_| entries.iter().cloned().map(|entry| (entry, Vec::new())).collect())
            } else {
                entries.iter().cloned().map(|entry| (entry, Vec::new())).collect()
            };
        scheduled
            .into_iter()
            .map(|(e, block_reasons)| {
                json!({
                    "id": e.id,
                    "title": e.title,
                    "status": e.status,
                    "severity": e.severity,
                    "priority": e.fields.iter()
                        .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
                        .map(|(_, value)| value),
                    // R-051:复杂度(小/中/大),缺失前端显示"未评估"。
                    "complexity": e.fields.iter()
                        .find(|(key, _)| key == "复杂度" || key.eq_ignore_ascii_case("complexity"))
                        .map(|(_, value)| value),
                    "closed": kind.terminal.contains(&e.status.as_str()),
                    "blocked": !block_reasons.is_empty(),
                    "block_reasons": block_reasons,
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
        "goals": load(&GOALS),
        "sources": load(&SOURCES),
        "findings": load(&FINDINGS),
        "archived": {
            "req": archived(&REQUIREMENTS),
            "defect": archived(&DEFECTS),
            "goal": archived(&GOALS),
            "source": archived(&SOURCES),
            "finding": archived(&FINDINGS),
        },
        "archived_entries": {
            "req": archived_entries(&REQUIREMENTS),
            "defect": archived_entries(&DEFECTS),
            "goal": archived_entries(&GOALS),
            "source": archived_entries(&SOURCES),
            "finding": archived_entries(&FINDINGS),
        },
    })
}

#[tauri::command]
fn test_runs_snapshot(project_dir: String) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    kanzei_tools::test_record::test_runs_snapshot(&root)
}

#[tauri::command]
fn test_run_record(
    project_dir: String,
    title: String,
    status: String,
    command: Option<String>,
    summary: Option<String>,
) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    kanzei_tools::test_record::append_test_run(
        &root,
        &title,
        &status,
        command.as_deref(),
        summary.as_deref(),
    )
}

fn collect_project_files(root: &Path, dir: &Path, query: &str, results: &mut Vec<String>) {
    if results.len() >= 50 { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if results.len() >= 50 { break; }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if matches!(name.as_str(), ".git" | ".kanzei" | "target" | "node_modules") { continue; }
            collect_project_files(root, &path, query, results);
        } else if path.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            if query.is_empty() || relative.to_ascii_lowercase().contains(&query.to_ascii_lowercase()) {
                results.push(relative);
            }
        }
    }
}

#[tauri::command]
fn project_files(project_dir: String, query: String) -> Result<Vec<String>, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    if !root.is_dir() { return Err(format!("项目目录不存在: {}", root.display())); }
    let mut results = Vec::new();
    collect_project_files(&root, &root, query.trim(), &mut results);
    Ok(results)
}
// ---------- 设置(全局 kanzei.toml 表单) ----------

pub(crate) fn settings_save_at_path_impl(payload: SettingsPayload, path: &Path) -> Result<(), String> {
    // 以现有配置文本为底,只改设置页管理的键:注释、排版、未知字段原样保留(D-082)。
    // 文件存在但解析失败必须报错——静默回退默认值再覆写等于销毁用户配置。
    let mut doc = crate::settings_read_document(path)?;

    settings_apply_scalar_fields(&mut doc, &payload)?;

    settings_apply_limits(&mut doc, &payload)?;

    settings_apply_providers(&mut doc, &payload)?;

    crate::settings_write_document(doc, path)
}

#[derive(Debug, Deserialize)]
struct ExportOptions {
    project_dir: String,
    output_dir: String,
    include_memory: bool,
    include_requirements: bool,
    include_defects: bool,
    include_config: bool,
}

fn copy_export_file(root: &Path, destination: &Path, relative: &str, files: &mut Vec<String>) -> Result<(), String> {
    let source = root.join(relative);
    if !source.is_file() {
        return Ok(());
    }
    let target = destination.join(relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败: {e}"))?;
    }
    std::fs::copy(&source, &target).map_err(|e| format!("导出 {} 失败: {e}", source.display()))?;
    files.push(relative.replace('\\', "/"));
    Ok(())
}

fn copy_export_tree(source: &Path, destination: &Path, relative: &str, files: &mut Vec<String>) -> Result<(), String> {
    if !source.is_dir() {
        return Ok(());
    }
    for item in std::fs::read_dir(source).map_err(|e| format!("读取导出目录失败: {e}"))? {
        let item = item.map_err(|e| format!("读取导出条目失败: {e}"))?;
        let child_relative = Path::new(relative).join(item.file_name());
        let child_source = item.path();
        if child_source.is_dir() {
            copy_export_tree(&child_source, destination, &child_relative.display().to_string(), files)?;
        } else if child_source.is_file() {
            let relative_text = child_relative.display().to_string();
            let target = destination.join(&child_relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败: {e}"))?;
            }
            std::fs::copy(&child_source, &target)
                .map_err(|e| format!("导出 {} 失败: {e}", child_source.display()))?;
            files.push(relative_text.replace('\\', "/"));
        }
    }
    Ok(())
}

#[tauri::command]
async fn export_pick_dir() -> Result<Option<String>, String> {
    Ok(rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|handle| handle.path().display().to_string()))
}

#[tauri::command]
fn export_project_data(options: ExportOptions) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&options.project_dir));
    let output_base = PathBuf::from(options.output_dir.trim());
    if output_base.as_os_str().is_empty() {
        return Err("请先选择导出目录".into());
    }
    std::fs::create_dir_all(&output_base).map_err(|e| format!("创建导出目录失败: {e}"))?;
    let root_canonical = root.canonicalize().map_err(|e| format!("项目目录无法解析: {e}"))?;
    let output_canonical = output_base
        .canonicalize()
        .map_err(|e| format!("导出目录无法解析: {e}"))?;
    if output_canonical.starts_with(&root_canonical) {
        return Err("导出目录不能位于项目目录内".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let destination = output_canonical.join(format!("kanzei-export-{stamp}"));
    std::fs::create_dir_all(&destination).map_err(|e| format!("创建导出包目录失败: {e}"))?;
    let mut files = Vec::new();
    if options.include_memory {
        copy_export_tree(&root.join(".kanzei/memory"), &destination, ".kanzei/memory", &mut files)?;
    }
    if options.include_requirements {
        for relative in [
            ".kanzei/project/requirements.md",
            ".kanzei/project/requirements-archive.md",
        ] {
            copy_export_file(&root, &destination, relative, &mut files)?;
        }
    }
    if options.include_defects {
        for relative in [".kanzei/project/defects.md", ".kanzei/project/defects-archive.md"] {
            copy_export_file(&root, &destination, relative, &mut files)?;
        }
    }
    if options.include_config {
        copy_export_file(&root, &destination, ".kanzei/kanzei.toml", &mut files)?;
    }
    if files.is_empty() {
        let _ = std::fs::remove_dir_all(&destination);
        return Err("没有可导出的工作资料".into());
    }
    files.sort();
    Ok(json!({ "path": destination.display().to_string(), "files": files }))
}

/// R-126:让 agent 能自查真实运行中的界面。改完前端只跑 node --check 检不出
/// 渲染问题——D-164(编辑表单渲染成一片无标题输入框)、D-165(同一字段渲染两遍)
/// 都是语法完全正确却明显不对的例子,只有看真实渲染结果才发现得了。
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UiProbeInput {
    /// CSS 选择器,如 `#req-list .doc-item`。dom / style 需要,console 忽略。
    #[serde(default)]
    selector: Option<String>,
}

struct UiDomTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiDomTool {
    fn name(&self) -> &'static str {
        "ui_dom"
    }
    fn description(&self) -> String {
        "读取当前运行中窗口里匹配选择器的 DOM 子树(标签、class、可见文本、层级)。\
         改完前端用它确认渲染结果——node --check 只查语法,查不出渲染成什么样。只读。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap()
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        let input: UiProbeInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(e) => return kanzei_harness::ToolOutput::error(format!("invalid input: {e}")),
        };
        let Some(selector) = input.selector.as_deref().filter(|s| !s.trim().is_empty()) else {
            return kanzei_harness::ToolOutput::error("需要 selector");
        };
        match ui_probe("dom", selector).await {
            Ok(value) => kanzei_harness::ToolOutput::ok(
                value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()),
            ),
            Err(e) => kanzei_harness::ToolOutput::error(e),
        }
    }
}

struct UiConsoleTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiConsoleTool {
    fn name(&self) -> &'static str {
        "ui_console"
    }
    fn description(&self) -> String {
        "读取当前窗口自加载以来累积的 console 错误与警告(含未捕获异常)。\
         前端改动后必查:ReferenceError 一类问题不会让页面白屏,只会让某一块悄悄失效。只读。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap()
    }
    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        match ui_probe("console", "").await {
            Ok(value) => kanzei_harness::ToolOutput::ok(
                value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()),
            ),
            Err(e) => kanzei_harness::ToolOutput::error(e),
        }
    }
}

struct UiStyleTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiStyleTool {
    fn name(&self) -> &'static str {
        "ui_style"
    }
    fn description(&self) -> String {
        "读取匹配元素的计算样式与盒模型(display/位置/尺寸/关键布局属性)。\
         用来判断「为什么它没显示出来」「为什么挤成一团」,比猜 CSS 快得多。只读。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap()
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        let input: UiProbeInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(e) => return kanzei_harness::ToolOutput::error(format!("invalid input: {e}")),
        };
        let Some(selector) = input.selector.as_deref().filter(|s| !s.trim().is_empty()) else {
            return kanzei_harness::ToolOutput::error("需要 selector");
        };
        match ui_probe("style", selector).await {
            Ok(value) => kanzei_harness::ToolOutput::ok(
                value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()),
            ),
            Err(e) => kanzei_harness::ToolOutput::error(e),
        }
    }
}

/// 前端自查与定位工具集(R-126)。全部只读,受既有权限契约约束。
struct FrontendToolsComponent;
impl kanzei_harness::Component for FrontendToolsComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        _ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
        draft.tools.insert("ui_dom", Arc::new(UiDomTool));
        draft.tools.insert("ui_console", Arc::new(UiConsoleTool));
        draft.tools.insert("ui_style", Arc::new(UiStyleTool));
        draft
            .tools
            .insert("frontend_locate", Arc::new(kanzei_tools::frontend::FrontendLocateTool));
        draft
            .tools
            .insert("frontend_check", Arc::new(kanzei_tools::frontend::FrontendCheckTool));
        // 只读工具无需逐次确认;它们不写任何东西,权限风险为零。
        for name in ["ui_dom", "ui_console", "ui_style", "frontend_locate", "frontend_check"] {
            draft
                .permissions
                .push(kanzei_harness::rule(name, "*", kanzei_harness::Effect::Allow));
        }
        Ok(())
    }
}

/// R-053 快速记录:只挂单个 tracker 工具的最小组件(独立迷你 run 专用)。
struct QuickCaptureComponent {
    capture: &'static str, // "req" | "defect"
}
impl kanzei_harness::Component for QuickCaptureComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        _ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
        let tool = if self.capture == "defect" {
            kanzei_tools::tracker::TrackerTool {
                tool_name: "defect",
                noun: "defect",
                kind: &DEFECTS,
                requires_refs: None,
            }
        } else {
            kanzei_tools::tracker::TrackerTool {
                tool_name: "req",
                noun: "requirement",
                kind: &REQUIREMENTS,
                requires_refs: None,
            }
        };
        let name = tool.tool_name;
        draft.tools.insert(name, Arc::new(tool));
        draft
            .permissions
            .push(kanzei_harness::rule(name, "*", kanzei_harness::Effect::Allow));
        Ok(())
    }
}

const DEFECT_REVIEW_SYSTEM: &str = "You are a read-only defect review agent. You only have read, glob, and grep. \
Read .kanzei/project/defects.md first, then verify every active defect against relevant code, tests, and design documents. \
Reply in Chinese Markdown with: 1. summary and active defect count; 2. categories; 3. likely duplicates with IDs; \
4. impact of each defect; 5. suggested priority with reasons; 6. verifiable evidence using exact file paths, functions, \
and line numbers; 7. concrete next steps. Do not modify files, run commands, update trackers, or claim unverified facts.";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DefectReviewResult {
    empty: bool,
    report: String,
    defect_count: usize,
}

fn defect_review_snapshot(
    rctx: &ResolveCtx,
) -> anyhow::Result<Arc<kanzei_harness::HarnessSnapshot>> {
    let mut harness = Harness::default();
    harness
        .add(kanzei_tools::SubagentBase)
        .add(ConfigComponent);
    harness.resolve(rctx)
}

fn defect_review_report(summary: &kanzei_core::RunSummary) -> Result<String, String> {
    let report = summary.text.trim();
    if report.is_empty() {
        Err("审查模型没有返回报告".into())
    } else {
        Ok(report.to_string())
    }
}

/// R-092:独立只读缺陷审查。它不进入主 conversation/queue，也不持有任何写工具；
/// fast 失败后回退 primary，结果直接返回前端的 Markdown 查看器。
#[tauri::command]
async fn defect_review(project_dir: String) -> Result<DefectReviewResult, String> {
    let cwd = PathBuf::from(&project_dir);
    let config = Arc::new(KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let defects = DocStore::open(&project_root, &DEFECTS)
        .load()
        .map_err(|e| e.to_string())?;
    if defects.is_empty() {
        return Ok(DefectReviewResult {
            empty: true,
            report: "当前没有活动缺陷。".into(),
            defect_count: 0,
        });
    }

    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let snapshot = defect_review_snapshot(&rctx).map_err(|e| e.to_string())?;
    let mut agent = kanzei_tools::explore_agent();
    agent.name = "defect-review".into();
    agent.system = DEFECT_REVIEW_SYSTEM.into();
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(value) => ProxyConfig::Explicit(value.to_string()),
    };
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let tool_ctx = ToolCtx {
        cwd,
        project_root,
    };
    let prompt = format!(
        "审查当前项目 defects.md 中的 {} 条活动缺陷。逐条核对真实代码、测试和调用方，输出约定的 Markdown 报告。",
        defects.len()
    );
    let mut last_error = "没有可用的 fast 或 primary 模型".to_string();
    for role in ["fast", "primary"] {
        let resolved = match config.resolve_model(role) {
            Ok(value) => value,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let route = match kanzei_core::build_route(&resolved, &proxy).await {
            Ok(value) => value,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let runner_config = RunnerConfig {
            max_tokens: config.limits.max_tokens(),
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
            model: resolved.model,
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
            Box::pin(async move {
                match request {
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        match run_once_with_parts(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &tool_ctx,
            &prompt,
            &[],
            None,
            None,
            &mut on_event,
            &mut ask,
        )
        .await
        {
            Ok(summary) => match defect_review_report(&summary) {
                Ok(report) => {
                    return Ok(DefectReviewResult {
                        empty: false,
                        report,
                        defect_count: defects.len(),
                    });
                }
                Err(error) => last_error = error,
            },
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(format!("缺陷自动审查失败:{last_error}"))
}

/// R-053:自然语言描述 → 独立子代理结构化落库。与主对话完全并行,
/// 不碰 conversation/queue/lifecycle;fast 落库失败自动升级 primary 重试一次。
#[tauri::command]
async fn quick_req(
    project_dir: String,
    description: String,
    kind: Option<String>,
) -> Result<String, String> {
    let description = description.trim().to_string();
    if description.is_empty() {
        return Err("描述不能为空".into());
    }
    let capture: &'static str = match kind.as_deref() {
        Some("defect") => "defect",
        _ => "req",
    };
    let cwd = PathBuf::from(&project_dir);
    let config = Arc::new(KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let mut harness = Harness::default();
    harness.add(harness_ext::QuickCaptureComponent { capture });
    let snapshot = harness.resolve(&rctx).map_err(|e| e.to_string())?;
    let system = if capture == "defect" {
        // D-205:复现字段禁止编造。旧文案只说 "if inferable",推断不出时模型的默认
        // 行为就是硬挤一个伪复现(实例 D-204:「复现: 查看 SOP 时」),伪复现看起来
        // 像真的,下游拿着它开工只会猜错或空转,还没人知道该回去问用户。
        "You capture ONE defect from the user's natural-language description. Call the \
         `defect` tool exactly once with action \"add\": a concise title (<=40 chars, \
         Chinese preferred, keep qualifier words like 用户/桌面端/CLI from the original), \
         severity high|medium|low, fields = {\"复现\": concrete reproduction steps ONLY \
         if the description actually contains them — NEVER invent or pad one; when not \
         reproducible from the text, write \"待澄清: \" followed by the specific \
         questions the user must answer, \"原始描述\": the user's original text \
         verbatim}. Then reply with only the new id."
    } else {
        "You capture ONE requirement from the user's natural-language description. Call \
         the `req` tool exactly once with action \"add\": a concise title (<=40 chars, \
         Chinese preferred), fields = {\"priority\": suggested P0-P3, \"复杂度\": 小|中|大, \
         \"验收\": one draft acceptance line, \"归属\": \"kanzei\", \"原始描述\": the \
         user's original text verbatim}. Then reply with only the new id."
    };
    let agent = kanzei_harness::AgentDef {
        name: "quickcapture".into(),
        profile: kanzei_harness::ProfileScope::Dev,
        model: "fast".into(),
        mode: kanzei_harness::AgentMode::Subagent,
        steps: 4,
        system: system.into(),
    };
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let tool_ctx = ToolCtx {
        cwd: cwd.clone(),
        project_root: project_root.clone(),
    };
    let doc_kind = if capture == "defect" { &DEFECTS } else { &REQUIREMENTS };
    let store = DocStore::open(&project_root, doc_kind);
    let before: std::collections::HashSet<String> = store
        .load()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|e| e.id.clone())
        .collect();
    let prompt = format!("描述(原文):\n{description}");
    for role in ["fast", "primary"] {
        let Ok(resolved) = config.resolve_model(role) else {
            continue;
        };
        let Ok(route) = kanzei_core::build_route(&resolved, &proxy).await else {
            continue;
        };
        let runner_config = RunnerConfig {
            model: resolved.model.clone(),
            max_tokens: 2048,
            // 快记是机械结构化,不开思考。
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
            Box::pin(async move {
                match request {
                    // 快照里只有 req 工具,放行是安全的;问题一律取消(无人应答)。
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        let _ = run_once_with_parts(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &tool_ctx,
            &prompt,
            &[],
            None,
            None,
            &mut on_event,
            &mut ask,
        )
        .await;
        // 成功判据不信模型嘴,只看库:落了新条目才算数。
        let after = store.load().map_err(|e| e.to_string())?;
        if let Some(new_entry) = after.iter().find(|e| !before.contains(&e.id)) {
            return Ok(format!("{} {}", new_entry.id, new_entry.title));
        }
    }
    Err("子代理未能落库(fast/primary 均失败),请重试或在对话里直接说".into())
}

/// R-099 + R-127:最近若干轮的运行画像,供调试面板按轮次展示与跨轮对比。/// R-099 + R-127:最近若干轮的运行画像,供调试面板按轮次展示与跨轮对比。
/// 与冗余度量共用 `summarize_metrics` 的同一份口径,不另算一套。
#[tauri::command]
fn run_metrics(project_dir: String, limit: Option<usize>) -> Result<serde_json::Value, String> {
    let root = PathBuf::from(&project_dir);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let session_id = kanzei_core::project_session_id(&root);
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let rows = store
        .recent_episodes(&session_id, limit)
        .map_err(|e| e.to_string())?;
    let rounds: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(at, prompt, outcome, steps, input, output, tools, context, metrics)| {
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
                // 空对象代表那一轮早于度量落地,前端要能与"全零"区分开。
                "measured": metrics.trim() != "{}" && !metrics.trim().is_empty(),
            })
        })
        .collect();
    Ok(json!({ "rounds": rounds }))
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
pub(crate) fn hidden_command(program: &str) -> Command {
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
        // 纪要总结不需要思考预算。
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: config.service_tier_for(&resolved),
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

fn persist_always_allow(
    project_root: &Path,
    action: &str,
    resource: &str,
) -> Result<(kanzei_core::AskReply, PathBuf), String> {
    let pattern = kanzei_harness::config::generalize_resource(action, resource);
    let path = kanzei_harness::config::append_allow_rule(project_root, action, &pattern)
        .map_err(|error| error.to_string())?;
    Ok((kanzei_core::AskReply::AlwaysAllow, path))
}

/// reply: "deny" | "once" | "always"。always 先把泛化规则写进项目配置再放行。
#[tauri::command]
fn answer_ask(window: Window, state: State<'_, AppState>, id: u64, reply: String) {
    let Some(pending) = take_pending_ask(&state, id) else {
        return;
    };
    if matches!(pending.request, kanzei_core::AskRequest::Question { .. }) {
        let response = if reply.trim().is_empty() || reply == "cancel" {
            kanzei_core::AskResponse::Cancelled
        } else {
            kanzei_core::AskResponse::Answer(reply)
        };
        let _ = pending.sender.send(response);
        return;
    }
    let decision = match reply.as_str() {
        "always" => {
            let pattern =
                kanzei_harness::config::generalize_resource(&pending.action, &pending.resource);
            match persist_always_allow(
                &pending.project_root,
                &pending.action,
                &pending.resource,
            ) {
                Ok((reply, path)) => {
                    let _ = window.emit("kz:status", with_session_id(json!({
                        "stage": "权限",
                        "detail": format!("已记住:{} {pattern} → {}", pending.action, path.display()),
                    }), &pending.session_id));
                    reply
                }
                Err(error) => {
                    let _ = window.emit(
                        "kz:status",
                        with_session_id(
                            json!({
                                "stage": "权限",
                                "detail": format!("规则保存失败:{error};本次拒绝"),
                            }),
                            &pending.session_id,
                        ),
                    );
                    kanzei_core::AskReply::Deny
                }
            }
        }
        "once" => kanzei_core::AskReply::AllowOnce,
        _ => kanzei_core::AskReply::Deny,
    };
    let _ = pending.sender.send(kanzei_core::AskResponse::Permission(decision));
}

#[tauri::command]
fn pending_asks_get(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    let asks = runtime.asks.lock().unwrap();
    Ok(asks
        .iter()
        .map(|(id, pending)| pending_ask_payload(*id, pending))
        .collect())
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
        } else if p.auth.as_deref() == Some("claude") {
            // 实际可用型号(2026-08):Opus 5 / Sonnet 5 / Haiku 4.5。
            for m in [
                "claude-opus-5",
                "claude-sonnet-5",
                "claude-haiku-4-5-20251001",
            ] {
                items.push(json!({"id": format!("{name}:{m}"), "label": format!("{name}:{m}")}));
            }
        } else if p.protocol == "openai" || p.protocol == "openai-responses" {
            // D-167:任何 OpenAI 兼容端点(DeepSeek/OpenRouter/Kimi/自建 vLLM…)都走标准
            // GET {base_url}/models。早期这里只硬编码了 codex/claude/ollama 三种,别的
            // provider 加进配置后一个模型都列不出来,等于配了也用不了。
            // Ollama 例外:它的 /v1/models 不全,原生 /api/tags 才是真源。
            if p.base_url.contains("11434") {
                push_ollama_models(&mut items, name, &p.base_url).await;
                continue;
            }
            let key = p
                .api_key
                .clone()
                .filter(|k| !k.trim().is_empty())
                .or_else(|| p.api_key_env.as_deref().and_then(|e| std::env::var(e).ok()));
            let url = format!("{}/models", p.base_url.trim_end_matches('/'));
            let proxy = match config.proxy.as_deref() {
                Some("off") => ProxyConfig::Disabled,
                Some("env") | None => ProxyConfig::Env,
                Some(custom) => ProxyConfig::Explicit(custom.to_string()),
            };
            let Ok(client) = kanzei_llm::proxy::build_http_client(&proxy) else {
                continue;
            };
            let mut request = client.get(&url).timeout(std::time::Duration::from_secs(6));
            if let Some(k) = &key {
                request = request.bearer_auth(k);
            }
            // 探测失败不算错误:端点可能没实现 /models,或 key 还没配好。
            // 手填入口(前端的「＋ 手填模型…」)始终可用,所以这里静默跳过即可。
            if let Ok(resp) = request.send().await {
                if let Ok(v) = resp.json::<serde_json::Value>().await {
                    for m in v["data"].as_array().unwrap_or(&Vec::new()) {
                        if let Some(id) = m["id"].as_str() {
                            items.push(json!({
                                "id": format!("{name}:{id}"),
                                "label": format!("{name}:{id}"),
                            }));
                        }
                    }
                }
            }
        } else if p.base_url.contains("11434") {
            push_ollama_models(&mut items, name, &p.base_url).await;
        }
    }
    Ok(json!(items))
}

/// Ollama 的模型清单走原生 /api/tags:它的 /v1/models 不完整。
/// 本机服务不走代理——挂了代理反而连不上 127.0.0.1。
async fn push_ollama_models(items: &mut Vec<serde_json::Value>, name: &str, base_url: &str) {
    let tags_url = format!("{}/api/tags", base_url.trim_end_matches("/v1"));
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    else {
        return;
    };
    let Ok(resp) = client.get(&tags_url).send().await else {
        return;
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return;
    };
    for m in v["models"].as_array().unwrap_or(&Vec::new()) {
        if let Some(n) = m["name"].as_str() {
            items.push(json!({
                "id": format!("{name}:{n}"),
                "label": format!("{name}:{n}"),
            }));
        }
    }
}

#[tauri::command]
fn app_info() -> serde_json::Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "build": option_env!("KANZEI_BUILD_INFO").unwrap_or("dev"),
    })
}

#[tauri::command]
fn stop_run(
    window: Window,
    state: State<'_, AppState>,
    project_dir: Option<String>,
    process_id: Option<String>,
) {
    let target_project = project_dir.as_ref().map(PathBuf::from).map(|cwd| {
        normalized_project_root(&cwd)
    });
    let target_session = target_project
        .as_ref()
        .map(|root| process_session_id(root, process_id.as_deref()));
    let runtimes: Vec<Arc<SessionRuntime>> = state
        .runtimes
        .lock()
        .unwrap()
        .iter()
        .filter(|(session_id, runtime)| {
            target_session.as_ref().map_or(true, |target| target == *session_id)
                && runtime.running.load(Ordering::SeqCst)
        })
        .map(|(_, runtime)| runtime.clone())
        .collect();
    if runtimes.is_empty() {
        let _ = window.emit(
            "kz:error",
            with_session_id(
                json!({ "message": "目标项目当前没有可停止的运行" }),
                target_session.as_deref().unwrap_or(""),
            ),
        );
        return;
    }
    let mut cancelled = None;
    for runtime in runtimes {
        let result = target_project.clone().map(|root| {
            let session_id = target_session
                .clone()
                .unwrap_or_else(|| kanzei_core::project_session_id(&root));
            let state_path = kanzei_core::project_state_path(&root);
            kanzei_core::SessionStore::open(&state_path)
                .and_then(|store| stop_runtime_and_finalize(&runtime, &store, &session_id))
        });
        cancelled = result;
    }
    match cancelled.transpose() {
        Ok(Some(count)) => {
            let _ = window.emit(
                "kz:stopped",
                with_session_id(
                    json!({ "cancelled_queue": count }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
        }
        Ok(None) => {
            let _ = window.emit(
                "kz:stopped",
                with_session_id(
                    json!({ "cancelled_queue": 0 }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
        }
        Err(error) => {
            let _ = window.emit(
                "kz:error",
                with_session_id(
                    json!({ "message": format!("停止时清理排队输入失败: {error}") }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
            let _ = window.emit(
                "kz:stopped",
                with_session_id(
                    json!({ "cancelled_queue": 0 }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
        }
    }

    // 后台进程不随 abort 结束:不回收会留下孤儿 dev server 占端口(R-097)。
    if let Some(root) = target_project {
        let window = window.clone();
        let session = target_session.clone().unwrap_or_default();
        tauri::async_runtime::spawn(async move {
            let killed = kanzei_tools::kill_background_processes(&root).await;
            if killed > 0 {
                let _ = window.emit(
                    "kz:status",
                    with_session_id(
                        json!({ "stage": "停止", "detail": format!("已回收 {killed} 个后台进程") }),
                        &session,
                    ),
                );
            }
        });
    }
}

fn parse_delivery(value: Option<&str>) -> anyhow::Result<kanzei_core::Delivery> {
    match value.unwrap_or("queue") {
        "steer" => Ok(kanzei_core::Delivery::Steer),
        "queue" => Ok(kanzei_core::Delivery::Queue),
        other => Err(anyhow::anyhow!("未知输入交付模式: {other}")),
    }
}

fn report_persistence_failure(
    window: &Window,
    session_id: &str,
    operation: &str,
    error: impl std::fmt::Display,
) {
    let message = format!("运行结果已保留，但{operation}失败: {error}");
    tracing::warn!("{message}");
    let _ = window.emit(
        "kz:error",
        with_session_id(json!({ "message": message }), session_id),
    );
}
fn append_run_notification(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    status: &str,
    summary: impl Into<String>,
    requires_action: bool,
) -> anyhow::Result<()> {
    store.append_notification_atomic(
        session_id,
        status,
        &summary.into(),
        requires_action,
    )?;
    Ok(())
}
fn admit_input(
    project_dir: &str,
    session_id: &str,
    prompt: &str,
    delivery: kanzei_core::Delivery,
) -> anyhow::Result<kanzei_core::AdmittedInput> {
    let project_root = normalized_project_root(Path::new(project_dir));
    let state_path = kanzei_core::project_state_path(&project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(&session_id, &project_root.display().to_string(), None)?;
    let input_id = format!(
        "input_{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let input = store.admit_input(&session_id, &input_id, prompt, delivery)?;
    store.append_event(
        &session_id,
        "prompt.admitted",
        &json!({ "input_id": input_id, "delivery": if matches!(delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
    )?;
    Ok(input)
}

fn promote_next_input(project_dir: &str, session_id: &str) -> anyhow::Result<Option<kanzei_core::AdmittedInput>> {
    let project_root = normalized_project_root(Path::new(project_dir));
    let state_path = kanzei_core::project_state_path(&project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    let Some(input) = store.promote_next_input(&session_id)? else {
        return Ok(None);
    };
    store.append_event(
        &session_id,
        "prompt.promoted",
        &json!({ "input_id": input.input_id, "delivery": if matches!(input.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
    )?;
    Ok(Some(input))
}

#[tauri::command]
async fn run_prompt(
    window: Window,
    state: State<'_, AppState>,
    prompt: String,
    project_dir: String,
    profile: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    work_priority: Option<String>,
    delivery: Option<String>,
    attachments: Option<Vec<PromptAttachment>>,
    process_id: Option<String>,
) -> Result<(), String> {
    let delivery = parse_delivery(delivery.as_deref()).map_err(|e| e.to_string())?;
    let project_root = normalized_project_root(Path::new(&project_dir));
    let process = if let Some(process_id) = process_id.as_deref() {
        let process = state
            .processes
            .lock()
            .unwrap()
            .get(process_id)
            .cloned()
            .ok_or_else(|| format!("进程不存在: {process_id}"))?;
        if process.project_dir != project_root.display().to_string() {
            return Err("进程不属于当前项目".into());
        }
        process
    } else {
        ensure_default_process(&state, &project_root)
    };
    let session_id = process_session_id(&project_root, Some(&process.id));
    let profile = profile.or_else(|| process.profile.lock().unwrap().clone());
    let model = model.or_else(|| process.model.lock().unwrap().clone());
    let reasoning = process.reasoning.lock().unwrap().clone();
    let subagent_enabled = process.subagent_enabled.load(Ordering::SeqCst);
    let runtime = runtime_for(&state, &session_id);
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    {
        if runtime.running.load(Ordering::SeqCst) {
            if attachments.as_ref().is_some_and(|items| !items.is_empty()) {
                return Err("当前任务运行中不能排队附件，请等待本轮完成后再发送".into());
            }
            let queued = admit_input(&project_dir, &session_id, &prompt, delivery).map_err(|e| e.to_string())?;
            let _ = window.emit(
                "kz:status",
                with_session_id(
                    json!({ "stage": "排队", "detail": format!("已排队，前方输入将依次执行（{}）", queued.input_id) }),
                    &session_id,
                ),
            );
            return Ok(());
        }
        runtime.running.store(true, Ordering::SeqCst);
    }
    let asks = runtime.asks.clone();
    let ask_seq = state.ask_seq.clone();
    let running = runtime.running.clone();
    let lifecycle = runtime.lifecycle.clone();
    let conversation = runtime.conversation.clone();
    let live_run = runtime.live.clone();

    let runtime_for_task = runtime.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut next_input = None;
        let mut next_prompt = prompt;
        let mut next_attachments = attachments;
        // 会话是否因失败而停:只影响 kz:idle 的 reason,前端据此区分"跑完了"与"崩了"。
        let mut idle_reason = "completed";
        loop {
            let result = run_task(
                &window,
                asks.clone(),
                ask_seq.clone(),
                next_prompt,
                next_attachments.take(),
                project_dir.clone(),
                session_id.clone(),
                subagent_enabled,
                profile.clone(),
                agent.clone(),
                model.clone(),
                work_priority.clone(),
                reasoning.clone(),
                conversation.clone(),
                live_run.clone(),
                delivery,
                next_input.take(),
            )
            .await;
            if let Err(e) = &result {
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
                let _ = window.emit(
                    "kz:error",
                    with_session_id(json!({ "message": format!("{message}{hint}") }), &session_id),
                );
            }
            if result.is_err() {
                let _lifecycle = lifecycle.lock().unwrap();
                running.store(false, Ordering::SeqCst);
                idle_reason = "failed";
                break;
            }
            // 必须写回外层 next_input:此处若新建绑定,promote 出来的输入会被丢弃,
            // 下一轮 run_task 收到 None 后按新输入重新 admit,导致队列顺序反转并重复入库。
            next_input = {
                let _lifecycle = lifecycle.lock().unwrap();
                match promote_next_input(&project_dir, &session_id) {
                    Ok(input) => {
                        if input.is_none() {
                            running.store(false, Ordering::SeqCst);
                        }
                        input
                    }
                    Err(error) => {
                        let _ = window.emit(
                            "kz:error",
                            with_session_id(json!({ "message": error.to_string() }), &session_id),
                        );
                        running.store(false, Ordering::SeqCst);
                        idle_reason = "failed";
                        None
                    }
                }
            };
            let Some(input) = next_input.clone() else {
                break;
            };
            next_prompt = input.prompt.clone();
            let _ = window.emit(
                "kz:status",
                with_session_id(
                    json!({ "stage": "排队", "detail": format!("开始执行排队输入（{}）", input.input_id) }),
                    &session_id,
                ),
            );
        }
        // 会话级终态(R-086)。kz:done 是**一轮**的终点:排队输入会让上面这个 loop
        // 接着跑下一轮,期间 runtime.running 一直是 true。前端若拿 kz:done 判空闲,
        // 多轮运行会在第一轮后就显示成已结束。只有 loop 真正退出后发的 kz:idle
        // 才代表"这个会话停了",UI 的会话状态机只认它收敛终态。
        // 被 stop_run abort 时这里不会执行——那条路径自己发 kz:stopped。
        let _ = window.emit(
            "kz:idle",
            with_session_id(json!({ "reason": idle_reason }), &session_id),
        );
        // 自然完成/失败时释放会话容器中的句柄;stop_run abort 后则由 stop 路径取走。
        runtime_for_task.current_run.lock().unwrap().take();
    });
    *runtime.current_run.lock().unwrap() = Some(handle);
    // spawn 可能在句柄安装前快速结束,避免把已结束句柄重新放回容器。
    if !runtime.running.load(Ordering::SeqCst) {
        runtime.current_run.lock().unwrap().take();
    }
    Ok(())
}

async fn run_task(
    window: &Window,
    asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    ask_seq: Arc<AtomicU64>,
    prompt: String,
    attachments: Option<Vec<PromptAttachment>>,
    project_dir: String,
    session_id: String,
    subagent_enabled: bool,
    profile: Option<String>,
    agent_name: Option<String>,
    model_override: Option<String>,
    work_priority: Option<String>,
    reasoning_override: Option<String>,
    conversation: Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    live_run: Arc<Mutex<LiveRun>>,
    delivery: kanzei_core::Delivery,
    promoted_input: Option<kanzei_core::AdmittedInput>,
) -> anyhow::Result<()> {
    // 阶段汇报:让前端每一步都有着落(用户反馈:要详细指示)。
    let stage = |name: &str, detail: String| {
        let _ = window.emit(
            "kz:status",
            with_session_id(json!({ "stage": name, "detail": detail }), &session_id),
        );
    };

    let cwd = PathBuf::from(&project_dir);
    anyhow::ensure!(cwd.is_dir(), "工作目录不存在: {project_dir}");

    stage("配置", format!("加载 {}", cwd.display()));
    let (config, config_warnings) = KanzeiConfig::load_with_warnings(&cwd)?;
    let config = Arc::new(config);
    for warning in &config_warnings {
        stage("配置", warning.clone());
    }
    for warning in config.bash_permission_warnings() {
        stage("权限", warning);
    }
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
        // 前端自查与定位工具只在桌面端有意义(需要真实运行中的窗口);
        // 顺序在 profile 之后、Config 之前,用户配置仍可覆盖。
        .add(harness_ext::FrontendToolsComponent)
        .add(MarkdownComponent)
        .add(ConfigComponent);
    let snapshot = harness.resolve(&rctx)?;
    let mut agent = snapshot.select_agent(agent_name.as_deref())?.clone();
    let work_priority = match work_priority.as_deref() {
        Some("requirement-first") => "requirement-first",
        _ => "defect-first",
    };
    if profile == ProfileKind::Dev {
        // 前端自查段跟着 FrontendToolsComponent 走:注册了这 5 个工具的装配线才追加。
        // 写死在 dev 基础提示词里的话,CLI 侧会被指向 5 个根本不存在的工具。
        agent.system.push('\n');
        agent.system.push('\n');
        agent.system.push_str(kanzei_tools::frontend_inspection_guidance());
        let (first, second) = if work_priority == "requirement-first" {
            ("requirements.md", "defects.md")
        } else {
            ("defects.md", "requirements.md")
        };
        agent.system.push_str(&format!(
            "\n\nWork selection mode for this run: {work_priority}. Scan {first} from top to bottom first; only after it has no workable item scan {second}. This run's selected mode overrides the default queue order in the surrounding project context."
        ));
    }
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
        max_tokens: config.limits.max_tokens(),
        // 每进程选择优先,未选则用 kanzei.toml 的 [models] reasoning 默认档。
        reasoning: reasoning_override
            .as_deref()
            .or(config.models.reasoning.as_deref())
            .map(kanzei_llm::ReasoningEffort::parse)
            .unwrap_or_default(),
        service_tier: config.service_tier_for(&resolved),
        // 轮内主动压缩的预算基准(D-176)。轮末那次压缩保留作兜底,但长轮/自动续跑
        // 根本轮不到它,真正起作用的是这条。
        context_limit: resolved.provider.context_limit,
        limits: config.limits.clone(),
    };
    let ctx = ToolCtx { cwd, project_root };

    let state_path = kanzei_core::project_state_path(&ctx.project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(&session_id, &ctx.project_root.display().to_string(), None)?;
    let is_new_input = promoted_input.is_none();
    let promoted = if let Some(input) = promoted_input {
        input
    } else {
        let input_id = format!(
            "input_{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        store.admit_input(&session_id, &input_id, &prompt, delivery)?;
        store.append_event(
            &session_id,
            "prompt.admitted",
            &json!({ "input_id": input_id, "delivery": if matches!(delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
        )?;
        store
            .promote_next_input(&session_id)?
            .ok_or_else(|| anyhow::anyhow!("无法提升已提交的桌面端输入"))?
    };
    if is_new_input {
        store.append_event(
            &session_id,
            "prompt.promoted",
            &json!({ "input_id": promoted.input_id, "delivery": if matches!(promoted.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
        )?;
    }
    let prompt = promoted.prompt;
    // promoted → running,并记住本轮身份与墙钟(D-173)。少了 running/completed 这段
    // 生命周期,跑完的输入永远停在 promoted,以后任何一次停止都会把它追认成 cancelled。
    let promoted_input_id = promoted.input_id.clone();
    store.start_input(&promoted_input_id)?;
    let run_id = format!(
        "run_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );
    let run_started = std::time::Instant::now();
    store.set_status(&session_id, "running")?;
    append_run_notification(&store, &session_id, "running", "任务已开始", false)?;
    store.append_event(
        &session_id,
        "session.status_changed",
        &json!({ "status": "running" }),
    )?;
    let _ = window.emit(
        "kz:meta",
        with_session_id(json!({
            "profile": format!("{profile:?}").to_lowercase(),
            "agent": agent.name,
            "model": format!("{}:{}", resolved.provider_name, resolved.model),
            "contextLimit": resolved.provider.context_limit,
        }), &session_id),
    );

    let event_window = window.clone();
    let session_id_for_events = session_id.clone();
    let emit_event = move |name: &str, payload: serde_json::Value| {
        event_window.emit(name, with_session_id(payload, &session_id_for_events))
    };
    // 轨迹与统计写进 runtime 的 live 画像,停止路径才够得着(D-179)。
    let live = live_run.clone();
    live.lock().unwrap().begin(
        &run_id,
        &promoted_input_id,
        &prompt,
        &resolved.provider_name,
        &resolved.model,
    );
    let trace_log = live.clone();
    // D-173 可观测性:主代理的工具调用原先只实时发给 UI,一条也不落库——
    // 于是"时间花在模型、shell 还是等用户""用户点了几次权限"事后统统无从查证,
    // 只能从最终对话快照反推。这里按 id 记开始时刻,收尾时连耗时一起写进 run.trace。
    let tool_started: Arc<Mutex<HashMap<String, std::time::Instant>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut on_event = move |event: RunEvent| {
        let elapsed_ms = |id: &str| -> Option<u128> {
            tool_started
                .lock()
                .unwrap()
                .remove(id)
                .map(|at| at.elapsed().as_millis())
        };
        let _ = match event {
            RunEvent::TurnStart { step, max_steps } => {
                {
                    let mut live = trace_log.lock().unwrap();
                    live.steps = live.steps.max(step);
                    live.trace.push(json!({
                        "kind": "turn.started", "step": step, "at": now_ms(),
                    }));
                }
                emit_event("kz:turn", json!({ "step": step, "maxSteps": max_steps }))
            }
            RunEvent::Text(text) => emit_event("kz:text", json!({ "text": text })),
            RunEvent::Reasoning(text) => emit_event("kz:reasoning", json!({ "text": text })),
            RunEvent::ToolStart {
                id,
                name,
                summary,
                input,
            } => {
                tool_started
                    .lock()
                    .unwrap()
                    .insert(id.clone(), std::time::Instant::now());
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "tool.started", "id": id, "name": name,
                    "summary": summary, "at": now_ms(),
                }));
                emit_event(
                    "kz:tool-start",
                    json!({ "id": id, "name": name, "summary": summary, "input": input }),
                )
            }
            RunEvent::ToolEnd {
                id,
                name,
                ok,
                preview,
                display,
            } => {
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "tool.completed", "id": id, "name": name, "ok": ok,
                    "durationMs": elapsed_ms(&id), "at": now_ms(),
                    // 失败原因要留档,成功的预览不必——轨迹不是第二份对话记录。
                    "error": (!ok).then(|| preview.chars().take(400).collect::<String>()),
                }));
                emit_event(
                    "kz:tool-end",
                    json!({ "id": id, "name": name, "ok": ok, "preview": preview, "display": display }),
                )
            }
            // 轮内主动压缩:UI 要看得见"什么时候让的路、让掉了多少",
            // 否则历史突然变短只会被当成 bug(D-176)。
            RunEvent::ContextCompacted {
                before_tokens,
                after_tokens,
                budget_tokens,
                limit_tokens,
                dropped_messages,
            } => {
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "context.compacted", "before": before_tokens, "after": after_tokens,
                    "budget": budget_tokens, "limit": limit_tokens,
                    "dropped": dropped_messages, "at": now_ms(),
                }));
                emit_event(
                    "kz:status",
                    json!({
                        "stage": "压缩",
                        "detail": format!(
                            "上下文约 {}k 已达 {}k 预算线(上限 {}k),就地压缩为 {}k,裁掉 {dropped_messages} 条历史",
                            before_tokens / 1000, budget_tokens / 1000,
                            limit_tokens / 1000, after_tokens / 1000
                        ),
                    }),
                )
            }
            RunEvent::PermissionResolved {
                tool_call_id,
                action,
                resource,
                decision,
                source,
            } => {
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "permission.resolved", "id": tool_call_id, "action": action,
                    "resource": resource, "decision": decision, "source": source, "at": now_ms(),
                }));
                Ok(())
            }
            // 子代理实时状态:挂到对应 task 块的进度行,并附带可展开的子工具轨迹。
            RunEvent::TaskProgress { id, text, trace } => {
                let payload = json!({
                    "id": id,
                    "text": text,
                    "trace": trace.map(|item| json!({
                        "child_id": item.child_id,
                        "phase": item.phase,
                        "name": item.name,
                        "summary": item.summary,
                        "ok": item.ok,
                        "preview": item.preview,
                        "display": item.display,
                    })),
                });
                trace_log.lock().unwrap().trace.push(payload.clone());
                emit_event("kz:task-progress", payload)
            },
            RunEvent::Retry { attempt, max, delay_ms } => emit_event(
                "kz:status",
                json!({ "stage": "重试", "detail": format!("网络请求暂时失败,第 {attempt}/{max} 次重试,等待 {delay_ms}ms") }),
            ),
            // 本步工具尚未执行,重放零副作用;前端需丢弃本步已渲染的残缺输出。
            RunEvent::StreamRestart { attempt, max, delay_ms } => emit_event(
                "kz:stream-restart",
                json!({
                    "attempt": attempt,
                    "max": max,
                    "delayMs": delay_ms,
                    "detail": format!("连接中断,重新请求本轮 {attempt}/{max},等待 {delay_ms}ms"),
                }),
            ),
            // 每步累计:停止时 episode 才有真实 token 数,而不是写个 0 冒充。
            RunEvent::StepEnd { usage, .. } => {
                {
                    let mut live = trace_log.lock().unwrap();
                    live.input_tokens += usage.input;
                    live.output_tokens += usage.output;
                }
                emit_event(
                    "kz:step",
                    json!({
                        "input": usage.input, "output": usage.output,
                        "cacheRead": usage.cache_read, "cacheWrite": usage.cache_write,
                    }),
                )
            }
        };
    };

    let ask_window = window.clone();
    let ask_root = ctx.project_root.clone();
    let ask_session_id = session_id.clone();
    let mut ask = move |request: kanzei_core::AskRequest| -> AskFuture {
        let (sender, receiver) = oneshot::channel();
        let id = ask_seq.fetch_add(1, Ordering::SeqCst);
        let (action, resource, payload) = match &request {
            kanzei_core::AskRequest::Permission { action, resource } => (
                action.clone(),
                resource.clone(),
                json!({ "kind": "permission", "id": id, "action": action, "resource": resource, "remember": kanzei_harness::config::generalize_resource(action, resource) }),
            ),
            kanzei_core::AskRequest::Question { question, options, default } => (
                "question".into(),
                question.clone(),
                json!({ "kind": "question", "id": id, "question": question, "options": options, "default": default }),
            ),
        };
        let payload = with_session_id(payload, &ask_session_id);
        asks.lock().unwrap().insert(
            id,
            PendingAsk { sender, request, action, resource, project_root: ask_root.clone(), session_id: ask_session_id.clone() },
        );
        let _ = ask_window.emit("kz:ask", payload);
        Box::pin(async move { receiver.await.unwrap_or(kanzei_core::AskResponse::Cancelled) })
    };

    // 会话连续:同项目续上内存历史；应用重启后从事件日志恢复最近一次完整消息投影。
    let persisted = conversation::recover_messages(&store, &session_id)?;
    let prior = conversation::conversation_prior(&conversation, &session_id, persisted);
    if !prior.is_empty() {
        stage("会话", format!("延续对话({} 条历史消息)", prior.len()));
    }

    // task 子代理运行时:独立只读快照;fast 角色缺席时两个档位都退回主模型。
    let subagent_rt = if subagent_enabled {
        let mut sub_harness = Harness::default();
        sub_harness
            .add(kanzei_tools::SubagentBase)
            .add(ConfigComponent);
        let sub_snapshot = sub_harness.resolve(&rctx)?;
        let fast = match config.resolve_model("fast") {
            Ok(r) => (kanzei_core::build_route(&r, &proxy).await)
                .ok()
                .map(|fr| (fr, r.model.clone(), config.service_tier_for(&r))),
            Err(_) => None,
        };
        let primary_tier = config.service_tier_for(&resolved);
        let fast_tier = fast.as_ref().map(|(_, _, tier)| tier.clone()).unwrap_or_else(|| primary_tier.clone());
        Some(kanzei_core::SubagentRuntime {
            snapshot: sub_snapshot,
            agent: kanzei_tools::explore_agent(),
            fast: fast
                .map(|(r, m, _)| (r, m))
                .unwrap_or_else(|| (route.clone(), resolved.model.clone())),
            primary: (route.clone(), resolved.model.clone()),
            fast_service_tier: fast_tier,
            primary_service_tier: primary_tier,
            max_tokens: config.limits.subagent_max_tokens(),
            // 纯兜底(用户定调:不设短限),防子代理失控挂死整轮。
            timeout_secs: config.limits.subagent_timeout_secs(),
            limits: config.limits.clone(),
        })
    } else {
        None
    };

    let initial_parts = prompt_attachment_parts(attachments.unwrap_or_default())?;
    if !initial_parts.is_empty() {
        let image_count = initial_parts
            .iter()
            .filter(|part| matches!(part, kanzei_llm::Part::Image { .. }))
            .count();
        let document_count = initial_parts
            .iter()
            .filter(|part| matches!(part, kanzei_llm::Part::Document { .. }))
            .count();
        stage(
            "附件",
            format!(
                "已接收 {} 个附件，转换为 {} 个图片、{} 个文档输入，准备发送给 agent",
                initial_parts.len(),
                image_count,
                document_count
            ),
        );
    }

    // 开跑预检索(R-106):prompt 命中既有记忆时前置索引提示块;历史存用户原文。
    let run_prompt = match kanzei_tools::memory::prompt_hints(&ctx.project_root, &prompt) {
        Some(hints) => format!("{hints}\n\n{prompt}"),
        None => prompt.clone(),
    };
    let run_result = run_once_with_parts(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        &run_prompt,
        &prior,
        (!initial_parts.is_empty()).then_some(initial_parts.as_slice()),
        subagent_rt.as_ref(),
        &mut on_event,
        &mut ask,
    )
    .await;
    let store = match kanzei_core::SessionStore::open(&state_path) {
        Ok(store) => Some(store),
        Err(error) => {
            report_persistence_failure(window, &session_id, "打开会话数据库", error);
            None
        }
    };
    if let Some(store) = store.as_ref() {
        match &run_result {
            Ok(summary) => {
                if let Err(error) = store.set_status(&session_id, "idle") {
                    report_persistence_failure(window, &session_id, "写入 idle 状态", error);
                }
                if let Err(error) = store.append_event(
                    &session_id,
                    "session.status_changed",
                    &json!({ "status": "idle" }),
                ) {
                    report_persistence_failure(window, &session_id, "写入完成状态事件", error);
                }
                if let Err(error) = store.append_event(
                    &session_id,
                    "run.completed",
                    &json!({
                        "steps": summary.steps,
                        "halted_by_user": summary.halted_by_user,
                        "input": summary.usage.input,
                        "output": summary.usage.output,
                        // 上下文账单(R-106):各注入源字符数,UI 与度量共用。
                        "context": summary.context_report,
                    }),
                ) {
                    report_persistence_failure(window, &session_id, "写入完成事件", error);
                }
                // 本轮切片:summary.messages = prior + 本轮;统计与失败提炼都只看本轮,
                // 否则历史失败反复上报、工具计数累计全历史(R-099 基线失真)。
                let this_run = &summary.messages[prior.len().min(summary.messages.len())..];
                // 轮末失败提炼与机械投递(R-105):不依赖模型自觉调用 memory_note。
                let signals = kanzei_core::summarize_failures(this_run);
                if !signals.is_empty() {
                    let memory = kanzei_tools::memory::MemoryStore::project(&ctx.project_root);
                    kanzei_tools::memory::harvest_failures(&memory, &signals);
                }
                // SOP 提炼(R-124):只在本轮确实完成了一个完整条目时触发,闸门在
                // completed_entry 里用代码强制。SOP 是用户的常用模板,所以只产候选,
                // 落到 global 候选箱等用户一键采纳——agent 不能自己决定入库。
                // 根因→fact(R-105):同一次收口把根因原料投项目 inbox,由 manager
                // 提炼成 fact——SOP 判 NOOP 时根因仍有记忆价值。
                if let Some(done) = kanzei_core::completed_entry(this_run) {
                    if let Some(global) = kanzei_tools::memory::MemoryStore::global() {
                        kanzei_tools::memory::harvest_sop(&global, &done, &prompt);
                    }
                    kanzei_tools::memory::harvest_entry_fact(
                        &kanzei_tools::memory::MemoryStore::project(&ctx.project_root),
                        &done,
                        &prompt,
                        &signals,
                    );
                }
                // episode 落库(R-106):机械轨迹画像。失败不阻塞收尾。
                let _ = store.append_episode(&kanzei_core::EpisodeRecord {
                    session_id: &session_id,
                    prompt_head: &prompt,
                    outcome: if summary.halted_by_user { "halted" } else { "completed" },
                    steps: summary.steps,
                    input_tokens: summary.usage.input,
                    output_tokens: summary.usage.output,
                    tools_json: &serde_json::to_string(&kanzei_core::summarize_tools(this_run))
                        .unwrap_or_default(),
                    context_json: &serde_json::to_string(&summary.context_report)
                        .unwrap_or_default(),
                    // R-099 调用画像:与冗余治理共用同一份口径,别处不再各算各的。
                    metrics_json: &serde_json::to_string(&kanzei_core::summarize_metrics(this_run))
                        .unwrap_or_default(),
                    // D-173:轮次归属与墙钟。缺了它们,复盘只能从"当前配置"反推模型,
                    // 而配置随时会变——最基本的事实都无法证伪。
                    provider: &resolved.provider_name,
                    model: &resolved.model,
                    run_id: &run_id,
                    input_id: &promoted_input_id,
                    duration_ms: run_started.elapsed().as_millis() as u64,
                    // R-106:上下文溢出压缩丢弃的轨迹段沉淀为 episode 的一部分,
                    // 让溢出路径不再无声丢弃轨迹,复盘时可通过 episodes.overflow_json 查回。
                    overflow_json: &serde_json::to_string(&summary.overflow_traces)
                        .unwrap_or_default(),
                });
                let _ = store.finish_input(&promoted_input_id, true);
                // 富 episode(带工具画像/上下文账单)已写,标记防重:停止路径的
                // flush_live_run 不该再补一条信息量更少的(D-179)。
                live.lock().unwrap().flushed = true;
                if let Err(error) = append_run_notification(
                    store,
                    &session_id,
                    "succeeded",
                    "任务完成",
                    false,
                ) {
                    report_persistence_failure(window, &session_id, "写入完成通知", error);
                }
                // 轮末记忆整理(R-105):独立任务消化 inbox 草稿,不阻塞完成事件。
                tauri::async_runtime::spawn(memory::consolidate_memory_inbox(project_dir.clone()));
            }
            Err(error) => {
                if let Err(persistence_error) = store.set_status(&session_id, "failed") {
                    report_persistence_failure(
                        window,
                        &session_id,
                        "写入失败状态",
                        persistence_error,
                    );
                }
                if let Err(persistence_error) = store.append_event(
                    &session_id,
                    "session.status_changed",
                    &json!({ "status": "failed" }),
                ) {
                    report_persistence_failure(
                        window,
                        &session_id,
                        "写入失败状态事件",
                        persistence_error,
                    );
                }
                if let Err(persistence_error) = store.append_event(
                    &session_id,
                    "run.failed",
                    &json!({ "error": error.to_string() }),
                ) {
                    report_persistence_failure(window, &session_id, "写入失败事件", persistence_error);
                }
                // 失败轮次原先在 `let summary = run_result?;` 处提前返回,轨迹与
                // episode 一并丢失——和被停止的轮次是同一个洞(D-179)。
                flush_live_run(store, &session_id, &live, "failed");
                let _ = store.finish_input(&promoted_input_id, false);
                if let Err(persistence_error) = append_run_notification(
                    store,
                    &session_id,
                    "failed",
                    error.to_string(),
                    false,
                ) {
                    report_persistence_failure(window, &session_id, "写入失败通知", persistence_error);
                }
            }
        }
    }
    let summary = run_result?;

    let history_len = summary.messages.len();
    // R-076:本轮工具画像随 kz:done 带给前端,鞭挞据此判定「实质进展」——
    // 只算本轮切片,不含 prior,否则历史工具调用让每一轮都看着像有动作。
    let this_run_tools =
        kanzei_core::summarize_tools(&summary.messages[prior.len().min(summary.messages.len())..]);
    conversation
        .lock()
        .unwrap()
        .insert(session_id.clone(), summary.messages);

    // R-021 自动压缩:历史估算超过上下文上限 70% 时,fast 模型出纪要并替换历史。
    // 估算用 len/4(与压缩预检同源的粗粒度);失败保留原历史,绝不丢上下文。
    if let Some(limit) = resolved.provider.context_limit {
        let estimate = {
            let conversations = conversation.lock().unwrap();
            let conv = conversations.get(&session_id).cloned().unwrap_or_default();
            serde_json::to_string(&conv)
                .map(|s| s.len() as u64 / 4)
                .unwrap_or(0)
        };
        if estimate > limit * 7 / 10 {
            stage(
                "压缩",
                format!(
                    "历史约 {}k token,超过 {}k 的 70%,自动压缩中…",
                    estimate / 1000,
                    limit / 1000
                ),
            );
            let transcript = {
                let conversations = conversation.lock().unwrap();
                let conv = conversations.get(&session_id).cloned().unwrap_or_default();
                render_transcript(&conv)
            };
            match fast_summarize(&ctx.cwd, &transcript).await {
                Ok(digest) => {
                    conversation.lock().unwrap().insert(
                        session_id.clone(),
                        vec![kanzei_llm::Message::user_text(format!(
                            "(系统:此前对话已自动压缩为以下纪要,基于它继续)\n{digest}"
                        ))],
                    );
                    let _ = window.emit(
                        "kz:compacted",
                        with_session_id(json!({ "summary": digest }), &session_id),
                    );
                }
                Err(e) => stage("压缩", format!("压缩失败:{e}(保留原历史)")),
            }
        }
    }

    let messages = conversation
        .lock()
        .unwrap()
        .get(&session_id)
        .cloned()
        .unwrap_or_default();
    let trace = live.lock().unwrap().trace.clone();
    if let Some(store) = store.as_ref() {
        if !trace.is_empty() {
            if let Err(error) = store.append_event(&session_id, "run.trace", &json!({ "events": trace })) {
                report_persistence_failure(window, &session_id, "写入运行轨迹", error);
            }
        }
        if let Err(error) = store.append_event(
            &session_id,
            "conversation.updated",
            &json!({ "messages": messages }),
        ) {
            report_persistence_failure(window, &session_id, "写入对话历史", error);
        }
    }
    let _ = window.emit(
        "kz:done",
        with_session_id(json!({
            "steps": summary.steps,
            "halted": summary.halted_by_user,
            "history": history_len,
            "input": summary.usage.input,
            "output": summary.usage.output,
            "cacheRead": summary.usage.cache_read,
            "cacheWrite": summary.usage.cache_write,
            "tools": this_run_tools,
        }), &session_id),
    );
    Ok(())
}


#[cfg(test)]
mod install_verify_tests {
    use super::image_replaced;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// D-199:"退出码成功"不等于"文件换了"。这条判据是唯一能把
    /// 静默没装上与真更新分开的东西,它必须对每一种"没换"都判 false。
    #[test]
    fn 未替换的镜像一律不算更新成功() {
        let t = UNIX_EPOCH + Duration::from_secs(1_786_212_410);
        let stamp = Some((t, 22_449_664_u64));

        // 实测形态:两次「检查更新」都 exit=0,前后 mtime 与大小一模一样。
        assert!(!image_replaced(stamp, stamp), "前后完全相同必须判为未替换");
        // 任一侧读不到:宁可多报可疑,也不能说成成功。
        assert!(!image_replaced(None, stamp));
        assert!(!image_replaced(stamp, None));
        assert!(!image_replaced(None, None));
    }

    #[test]
    fn 时间或大小任一变化都算替换成功() {
        let t = UNIX_EPOCH + Duration::from_secs(1_786_212_410);
        let stamp = Some((t, 22_449_664_u64));
        // 新构建通常两者都变;但只变一个也是真的换了,不能漏判成失败——
        // 漏判会让用户看到"更新未生效"却其实已经生效,比不报更让人不敢信。
        assert!(image_replaced(stamp, Some((t + Duration::from_secs(1), 22_449_664))));
        assert!(image_replaced(stamp, Some((t, 22_449_665))));
    }

    /// 真实文件上跑一遍:touch 之后指纹必须变。纯比较函数测不到
    /// `image_stamp` 取的字段对不对,而取错字段的话上面两条全绿也没用。
    #[test]
    fn image_stamp_跟得上真实文件改动() {
        let path = std::env::temp_dir().join(format!(
            "kz-d199-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::write(&path, b"old").unwrap();
        let before = super::image_stamp(&path);
        assert!(before.is_some());
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&path, b"replaced-with-longer-content").unwrap();
        let after = super::image_stamp(&path);
        assert!(image_replaced(before, after), "{before:?} -> {after:?}");
        std::fs::remove_file(&path).unwrap();
    }
}

#[cfg(test)]
mod assembly_tests {
    use super::*;

    /// D-195 的桌面这一半:追加前端自查段的装配线,必须真的注册了那段点名的每个工具。
    /// CLI 那一半在 kanzei-tools::profiles 的同名测试里(它守的是"别把这段写回基础
    /// 提示词")。两条合起来才是机制——D-190 只把文字挪了个地方,组件注册(run_task
    /// 里的 FrontendToolsComponent)与提示词追加(紧邻 work-priority 那几行)仍是两处
    /// 各写各的,没有任何东西保证同进同退。谁摘掉组件而留下追加,这里立刻红。
    #[test]
    fn 桌面装配线必须注册前端自查段点名的每个工具() {
        let root = PathBuf::from("C:/kanzei-d195-app-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        // run_task 的装配线,但不加 MarkdownComponent:它读真实 ~/.kanzei,
        // 会让这条测试的结果取决于跑测试的机器上放了什么。
        let mut harness = Harness::default();
        harness
            .add(BaseComponent)
            .add(DevProfile)
            .add(ResearchProfile)
            .add(harness_ext::FrontendToolsComponent)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let tools: Vec<String> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();

        let mentioned =
            kanzei_tools::prompt_tool_mentions(kanzei_tools::frontend_inspection_guidance());
        // 提取不出名字说明提取规则坏了,不是装配对了——那种绿是假的。
        assert_eq!(
            mentioned.len(),
            5,
            "前端自查段应点名 5 个工具,实际提取到 {mentioned:?}"
        );
        for tool in mentioned {
            assert!(
                tools.contains(&tool),
                "桌面装配线追加了点名 `{tool}` 的提示词,却没注册它;已注册: {tools:?}"
            );
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn 运行上限只写填了的键_留空的键从配置里移除() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-limits-{}.toml",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let payload = |limits: LimitsPayload| SettingsPayload {
            primary: String::new(),
            fast: String::new(),
            proxy: "env".into(),
            reasoning: None,
            codex_fast_mode: false,
            profile_default: None,
            profile: None,
            limits,
            providers: vec![],
        };
        settings_save_at_path(
            payload(LimitsPayload {
                max_tokens: Some(16384),
                subagent_timeout_secs: Some(300),
                ..Default::default()
            }),
            &path,
        )
        .unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.limits.max_tokens, Some(16384));
        assert_eq!(saved.limits.subagent_timeout_secs, Some(300));
        assert_eq!(saved.limits.max_tasks_per_turn, None, "没填的键不该被写进文件");
        assert_eq!(saved.limits.max_tasks_per_turn(), 8, "没填就走内置默认");

        // 清空表单 = 回到内置默认:必须把键删掉,不能留一个写死的旧数字,
        // 否则今后改了默认值,用户文件里那份陈旧数字会静默压住新默认。
        settings_save_at_path(payload(LimitsPayload::default()), &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("max_tokens"), "清空后仍留着死键:\n{text}");
        let saved: KanzeiConfig = toml::from_str(&text).unwrap();
        assert_eq!(saved.limits.max_tokens(), 8192);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn settings_save_preserves_handwritten_permission_rules() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-settings-{}.toml",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::write(
            &path,
            "[permissions]
[[permissions.rules]]
action = \"bash\"
resource = \"{\\\"command\\\":\\\"git status\\\",\\\"workdir\\\":\\\".\\\"}\"
effect = \"allow\"
",
        ).unwrap();
        settings_save_at_path(SettingsPayload {
            primary: "anthropic:claude-sonnet-5".into(),
            fast: String::new(),
            proxy: "env".into(),
            reasoning: None,
            codex_fast_mode: false,
            profile_default: None,
            profile: None,
            limits: Default::default(),
            providers: vec![],
        }, &path).unwrap();
        let config: KanzeiConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config.permissions.rules.len(), 1);
        assert_eq!(config.permissions.rules[0].action, "bash");
        assert_eq!(config.models.primary.as_deref(), Some("anthropic:claude-sonnet-5"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settings_save_preserves_comments_and_unknown_fields() {
        // D-082 完整不变量:保存不得破坏注释、排版与 schema 未知的字段。
        let path = std::env::temp_dir().join(format!(
            "kanzei-settings-preserve-{}.toml",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::write(
            &path,
            "# 顶部注释:手写配置\nproxy = \"http://127.0.0.1:7890\"\n\n[models]\nprimary = \"anthropic:claude-sonnet-5\" # 主模型\n\n[future_section]\nnew_field = \"来自新版本\"\n\n[[permissions.rules]]\naction = \"read\"\nresource = \"*/.env\"\neffect = \"deny\"\n",
        )
        .unwrap();
        settings_save_at_path(
            SettingsPayload {
                primary: "anthropic:claude-opus-5".into(),
                fast: "ollama:qwen3.5:4b".into(),
                proxy: "env".into(),
                reasoning: Some("high".into()),
                codex_fast_mode: true,
                profile_default: Some("dev".into()),
                profile: None,
                limits: Default::default(),
                providers: vec![],
            },
            &path,
        )
        .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        for expected in [
            "# 顶部注释:手写配置",
            "# 主模型",
            "[future_section]",
            "new_field = \"来自新版本\"",
        ] {
            assert!(saved.contains(expected), "missing preserved text: {expected}\n---\n{saved}");
        }
        // proxy 回落默认:已存在的键写显式 "env" 而不是删除(删除会连带删掉挂在键上的注释)。
        assert!(saved.contains("proxy = \"env\""), "proxy should reset to env:\n{saved}");
        let config: KanzeiConfig = toml::from_str(&saved).unwrap();
        assert_eq!(config.models.primary.as_deref(), Some("anthropic:claude-opus-5"));
        assert_eq!(config.models.reasoning.as_deref(), Some("high"));
        assert_eq!(config.models.codex_fast_mode, Some(true));
        assert_eq!(config.permissions.rules.len(), 1);
        assert_eq!(config.permissions.rules[0].resource, "*/.env");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settings_save_refuses_to_overwrite_unparseable_config() {
        // 解析失败绝不允许"回退默认值再覆写"——那等于销毁用户配置(D-082 的事故路径)。
        let path = std::env::temp_dir().join(format!(
            "kanzei-settings-broken-{}.toml",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let broken = "[models\nprimary = 不是合法 toml";
        std::fs::write(&path, broken).unwrap();
        let result = settings_save_at_path(
            SettingsPayload {
                primary: "anthropic:claude-sonnet-5".into(),
                fast: String::new(),
                proxy: "env".into(),
                reasoning: None,
                codex_fast_mode: false,
                profile_default: None,
                profile: None,
                limits: Default::default(),
                providers: vec![],
            },
            &path,
        );
        assert!(result.is_err(), "saving over a broken config must fail");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken, "file must be untouched");
        let _ = std::fs::remove_file(path);
    }
}
