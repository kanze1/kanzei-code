//! kzapp — kanzei Tauri 桌面端。
//! 前端为静态页面(ui/),经 command + event 通信:
//! run_prompt → kz:* 流式事件;kz:ask 权限弹窗 → answer_ask;stop_run 中止;
//! projects_* 多项目管理(~/.kanzei/app.json);settings_* 全局配置表单。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Emitter;

pub(crate) use kanzei_core::{run_once_with_parts, AskFuture};
pub(crate) use kanzei_harness::ConfigComponent;

mod agent_directory;
mod agent_team;
mod app_events;
mod async_mailbox;
mod attachments;
mod auto_run;
mod browser_selection;
mod collaboration;
mod commands;
mod composer_drafts;
mod context_view;
mod conversation;
mod conversation_actions;
mod decisions;
mod deliveries;
mod desktop_bridge;
mod docs;
mod durable_questions;
mod experience_events;
mod fast_model;
/// UX-089:文件页未保存草稿的落盘(file_draft_save / file_draft_clear / file_drafts_load)。
mod files_draft;
/// UI2-0926 #6:文件页编辑(路径解析、只读策略、BOM/换行、file_stat/file_write)。
mod files_edit;
mod files_view;
mod general_chat;
mod harness_ext;
/// Rust↔JS 返回值形状契约；命令与参数从 command_registry.json 和 Rust 签名生成。
/// 历史上部分命令手工拼 JSON,
/// 而前端冒烟断言的是前端自己写的 fixture,后端改名两侧全绿、界面碎)。
/// 纯判据模块,不参与产品构建。
#[cfg(test)]
mod ipc_contract;
mod manual_compact;
mod memory;
mod memory_chat;
mod mobile;
mod mobile_notify;
/// UI-0926 #3:模型配置的展示真源(model_effective)与项目级模型覆盖的逐键编辑。
mod model_config;
mod native_control;
mod native_desktop;
mod native_dispatch;
mod orchestration_trace;
mod prefs;
/// UI2-0926 #8:网页预览面板(子 webview + 进程内 CDP)与 browser 工具的面板后端。
mod preview;
mod processes;
mod projection_gate;
mod projects;
mod research_latex;
mod research_library;
mod research_topics;
mod run;
mod runtime_continuation;
mod runtime_service;
mod schedules;
mod screenshot;
mod session_supervisor;
mod settings;
mod skills;
mod softwire;
mod state;
mod subagents;
mod terminal_monitor;
mod tracker_capture;
mod typed_events;
mod update;
mod verification_monitor;
mod voice;
mod voice_service;
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
    if let Some(code) =
        kanzei_tools::verification::worker_entry(&std::env::args().skip(1).collect::<Vec<_>>())
    {
        std::process::exit(code);
    }
    let service = runtime_service::is_service();
    let _service_lock = if service {
        match runtime_service::service_lock() {
            Ok(Some(lock)) => Some(lock),
            _ => return,
        }
    } else {
        None
    };
    if !service && update::startup_update() {
        return;
    }
    // 安装器只装得了 kzapp,CLI 得由这里搬到位——两者共用一个库,版本必须同步(D-175)。
    if !service {
        update::sync_bundled_cli();
    }
    // 窗口创建之前自清孤儿 webview(D-171):上一个实例被强杀留下的
    // msedgewebview2 会锁住数据目录,不清的话本次启动必黑屏。
    if !service {
        update::cleanup_orphan_webviews();
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    tauri::Builder::default()
        .manage(AppState::default())
        .manage(voice::VoiceState::default())
        .manage(memory_chat::MemoryChatState::default())
        .manage(preview::PreviewState::default())
        // UI 探针的出口:装一次,之后工具侧只认 UI_PROBE_EMIT。
        .setup(move |app| {
            crate::session_supervisor::install(app.handle());
            let handle = app.handle().clone();
            let _ = UI_PROBE_EMIT.set(Box::new(move |payload| {
                let _ = handle.emit("kz:ui-probe", payload);
            }));
            // D-387:手机消息注入桌面后的 UI 通知出口——kz:mobile-message 事件驱动
            // 前端刷新会话列表(消费方闭环:手机发→桌面可见)。
            let msg_handle = app.handle().clone();
            let _ = crate::state::MOBILE_MESSAGE_EMIT.set(Box::new(move |session_id, text| {
                let _ = msg_handle.emit(
                    "kz:mobile-message",
                    serde_json::json!({ "session_id": session_id, "text": text }),
                );
            }));
            if service {
                // The execution owner has no graphical surface. Native command
                // dispatch, application events and managed state work without
                // constructing a Window or starting WebView2.
                runtime_service::install_service(app.handle())?;
                tauri::async_runtime::spawn(schedules::scheduler(app.handle().clone()));
                workspace::install_recovery(app.handle());
                durable_questions::recover_outbox(app.handle().clone());
                verification_monitor::start(app.handle().clone(), app.handle().clone());
                tauri::async_runtime::spawn(async move {
                    let _ = fast_model::fast_model_ensure_running().await;
                });
                return Ok(());
            }
            // 窗口从 tauri.conf.json 自动创建改为这里手动创建(R-101 E2 harness):
            // 配置里 `"create": false`,由 from_config 按同一份配置建窗口,生产路径
            // 行为不变;仅当环境变量 KANZEI_E2E_CDP 非空时注入 --remote-debugging-port
            // 打开 WebView2 DevTools 协议,供 E2 脚本通过 CDP 驱动真实 UI。
            runtime_service::install_client(app.handle());
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
            // UX-012:关掉 Tauri 默认的拖放处理。Windows 上它会在 WebView2 窗口上注册自己的 IDropTarget,
            // 页面里的 HTML5 拖放(把文件拖进输入框当附件、需求/文档列表的拖拽排序)一律收不到事件;
            // 前端没有用 tauri://drag-drop 的地方(网页预览子 webview 同样关着)。关掉之后浏览器默认行为是
            // 「文件落在没人处理的地方 = 导航到该文件」,由前端 17-files.js 的全局 dragover/drop 兜底拦下。
            builder = builder.disable_drag_drop_handler();
            // UI2-0926 #8:主界面开始(重新)加载(F5 / Ctrl+R)时收掉网页预览面板,
            // 与前端启动时的 preview_close 互为双保险;第一次启动的加载无害(还没有面板)。
            let page_app = app.handle().clone();
            builder = builder.on_page_load(move |window, payload| {
                preview::on_main_page_load(&page_app, window.label(), payload.event())
            });
            let main_window = builder.build()?;
            native_control::install(app.handle(), &main_window);
            tauri::async_runtime::spawn(schedules::scheduler(app.handle().clone()));
            // UI2-0926 #8:网页预览面板的装配(应用句柄 + DPI 变化重放边界 + 关窗收面板)。
            preview::install(app.handle(), &main_window);
            if runtime_service::embedded() {
                workspace::install_recovery(app.handle());
            }

            verification_monitor::start(app.handle().clone(), app.handle().clone());
            // R-249 批2:窗口句柄装进静态,截图工具据此抓真实画面。取不到句柄
            // (非 Windows / 平台不支持)时不装——工具会如实报「窗口未就绪」,
            // 而不是拿一个假句柄去抓出别人的窗口。
            #[cfg(windows)]
            if let Ok(hwnd) = main_window.hwnd() {
                let _ = state::UI_WINDOW_HANDLE.set(hwnd.0 as isize);
            }
            #[cfg(not(windows))]
            let _ = &main_window;
            // R-190 启动即保活:fast 指向本地 Ollama 且 CLI 已装但服务未运行 → 自动拉起。
            // 未安装 / 外部 provider / 已运行 → 零动作;失败不阻塞启动(状态由常驻探测如实反映)。
            tauri::async_runtime::spawn(async move {
                let _ = fast_model::fast_model_ensure_running().await;
            });
            Ok(())
        })
        .invoke_handler(runtime_service::route(native_dispatch::handler()))
        .run(tauri::generate_context!())
        .expect("error while running kanzei app");
}
