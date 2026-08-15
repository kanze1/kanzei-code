//! kzapp — kanzei Tauri 桌面端。
//! 前端为静态页面(ui/),经 command + event 通信:
//! run_prompt → kz:* 流式事件;kz:ask 权限弹窗 → answer_ask;stop_run 中止;
//! projects_* 多项目管理(~/.kanzei/app.json);settings_* 全局配置表单。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Emitter;

pub(crate) use kanzei_core::{run_once_with_parts, AskFuture};
pub(crate) use kanzei_harness::ConfigComponent;

mod agent_container;
mod auto_run;
mod collaboration;
mod commands;
mod conversation;
mod docs;
mod fast_model;
mod files_view;
mod harness_ext;
mod memory;
mod mobile;
mod orchestration_trace;
mod phase_pipeline;
mod prefs;
mod processes;
mod projects;
mod run;
mod screenshot;
mod settings;
mod state;
mod subagents;
mod typed_events;
mod update;

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

pub(crate) use state::{
    ensure_default_process, flush_live_run, flush_live_trace, halt_runtime_immediately,
    normalized_project_root, pending_ask_payload, process_info, process_session_id,
    prompt_attachment_parts, record_live_trace, record_live_trace_at_path, runtime_for,
    stop_runtime_and_finalize, take_pending_ask, ui_probe, ui_probe_result, with_session_id,
    AppState, LiveRun, MobileService, MobileServiceInfo, PendingAsk, ProcessHandle, ProcessInfo,
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

#[cfg(test)]
mod phase_pipeline_tests;

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
            let main_window = builder.build()?;
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
            projects::workspace_snapshot,
            docs::docs_snapshot,
            docs::docs_archive_entries,
            run::run_prompt,
            run::stop_run,
            run::stop_task,
            run::answer_ask,
            run::pending_asks_get,
            auto_run::auto_state_update,
            auto_run::auto_state_reset,
            settings::settings_get,
            settings::settings_save,
            settings::settings_open,
            projects::export_pick_dir,
            projects::export_project_data,
            settings::permission_rules_get,
            settings::permission_rule_delete,
            settings::provider_test,
            update::update_check_command,
            update::update_install_command,
            subagents::quick_req,
            subagents::idea_split,
            subagents::defect_review,
            memory::memory_overview,
            memory::memory_entries,
            memory::memory_recalls,
            memory::memory_value_flags,
            memory::memory_cleanup_demote,
            memory::memory_entry_delete,
            memory::memory_note_candidates,
            memory::memory_note_discard,
            run::run_metrics,
            run::run_metrics_by_category,
            projects::project_root_info,
            projects::project_detach,
            projects::projects_isolation_report,
            fast_model::fast_model_status,
            fast_model::fast_model_setup,
            memory::memory_entry_save,
            memory::memory_search_page,
            memory::memory_context_bill,
            memory::memory_consolidate,
            run::app_info,
            commands::models::models_list,
            docs::docs_update,
            docs::docs_open,
            commands::summarize::summarize_chat,
            docs::git_status,
            docs::conventions_init,
            conversation::conversation_clear,
            conversation::conversation_delete,
            docs::docs_read,
            docs::docs_read_custom,
            docs::architecture_snapshot,
            conversation::conversation_get,
            conversation::conversation_shadow_get,
            conversation::conversation_trace_get,
            conversation::conversation_list,
            processes::list_pending_inputs,
            processes::cancel_input,
            projects::project_files,
            processes::process_list,
            processes::process_create,
            processes::process_update,
            processes::process_close,
            collaboration::collaboration_snapshot,
            processes::worktree_create,
            processes::worktree_list,
            processes::worktree_diff,
            processes::worktree_merge,
            processes::worktree_merge_preview,
            processes::worktree_discard,
            processes::worktree_gate,
            processes::worktree_post_merge_gate,
            processes::worktree_harvest_candidates,
            processes::worktree_harvest_writeback,
            docs::test_runs_snapshot,
            docs::test_run_record,
            docs::test_runs_init_refs,
            mobile::mobile_service_start,
            mobile::mobile_service_stop,
            agent_container::agent_container_create,
            agent_container::agent_container_upgrade,
            agent_container::agent_container_rollback
        ])
        .run(tauri::generate_context!())
        .expect("error while running kanzei app");
}
