//! kzapp — kanzei Tauri 桌面端。
//! 前端为 Vite + TypeScript + React 18 工程(web/，构建产物 dist-web/)，经 command + event 通信:
//! run_prompt → kz:* 流式事件;kz:ask 权限弹窗 → answer_ask;stop_run 中止;
//! projects_* 多项目管理(~/.kanzei/app.json);settings_* 全局配置表单。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Emitter;

mod agent_directory;
mod app_events;
mod app_lifecycle;
mod async_mailbox;
mod attachments;
mod auto_run;
mod commands;
mod composer_drafts;
mod context_view;
mod conversation;
mod conversation_actions;
mod experience_events;
mod fast_model;
/// UI2-0926 #6:上下文抽屉轻编辑(路径解析、只读策略、BOM/换行、file_stat/file_write)。
mod files_edit;
mod files_view;
mod general_chat;
mod harness_ext;
/// Rust↔JS 返回值形状契约；命令与参数从 command_registry.json 和 Rust 签名生成。
/// 纯判据模块,不参与产品构建。
#[cfg(test)]
mod ipc_contract;
mod manual_compact;
mod memory;
mod memory_chat;
/// UI-0926 #3:模型配置的展示真源(model_effective)与项目级模型覆盖的逐键编辑。
mod model_config;
mod native_dispatch;
mod orchestration_trace;
mod prefs;
mod processes;
mod projection_gate;
mod projects;
mod repo_wiki;
mod run;
mod session_supervisor;
mod settings;
mod skills;
mod state;
mod terminal_monitor;
mod typed_events;
mod update;
mod workspace;

#[cfg(test)]
pub(crate) use projects::{export_project_data, ExportOptions};

pub(crate) use settings::global_config_path;

#[cfg(test)]
pub(crate) use settings::{ProviderPayload, SettingsPayload};

#[cfg(test)]
pub(crate) use update::{
    installed_cli_is_older, pending_path, release_is_newer, release_verdict, update_helper_path,
    update_log_at, validate_installer, wait_for_parent_exit, ReleaseVerdict,
};

#[cfg(test)]
pub(crate) use state::ensure_default_process;
pub(crate) use state::{
    flush_live_run, flush_live_trace, normalized_project_root, pending_ask_payload, process_info,
    process_session_id, prompt_attachment_parts, record_live_trace, record_live_trace_at_path,
    record_unpersisted_artifact, request_runtime_stop, runtime_for, take_pending_ask, ui_probe,
    with_session_id, AppState, LiveRun, MutexPoisonExt, PendingAsk, ProcessHandle, ProcessInfo,
    ProjectRoot, PromptAttachment, SessionRuntime, WorktreeInfo, WorktreeRoot, UI_PROBE_EMIT,
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
    if update::startup_update() {
        return;
    }
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
        .manage(memory_chat::MemoryChatState::default())
        // UI 探针的出口:装一次,之后工具侧只认 UI_PROBE_EMIT。
        .setup(move |app| {
            crate::session_supervisor::install(app.handle());
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
            let mut builder =
                tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?;
            if let Ok(port) = std::env::var("KANZEI_E2E_CDP") {
                if !port.trim().is_empty() {
                    // D-289:Chromium M111+ 的 CDP 要求显式 origin 白名单,否则
                    // playwright-core connectOverCDP(非 DevTools 客户端)握手被拒。
                    // 仅 E2 注入时携带,生产路径不带这两个参数。
                    builder = builder.additional_browser_args(&format!(
                        "--remote-debugging-port={} --remote-allow-origins=*",
                        port.trim()
                    ));
                }
            }
            // UX-012:关掉 Tauri 默认的拖放处理，确保前端 HTML5 拖放文件到输入框可用。
            builder = builder.disable_drag_drop_handler();
            let main_window = builder.build()?;
            let shutdown_handle = app.handle().clone();
            main_window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    app_lifecycle::cleanup_on_window_destroyed(&shutdown_handle);
                }
            });
            workspace::install_recovery(app.handle());

            // R-190 启动即保活:fast 指向本地 Ollama 且 CLI 已装但服务未运行 → 自动拉起。
            // 未安装 / 外部 provider / 已运行 → 零动作;失败不阻塞启动。
            tauri::async_runtime::spawn(async move {
                let _ = fast_model::fast_model_ensure_running().await;
            });
            Ok(())
        })
        .invoke_handler(native_dispatch::handler())
        .run(tauri::generate_context!())
        .expect("error while running kanzei app");
}
