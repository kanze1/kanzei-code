//! kzapp — kanzei Tauri 桌面端。
//! 前端为静态页面(ui/),经 command + event 通信:
//! run_prompt → kz:* 流式事件;kz:ask 权限弹窗 → answer_ask;stop_run 中止;
//! projects_* 多项目管理(~/.kanzei/app.json);settings_* 全局配置表单。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Emitter;

pub(crate) use kanzei_core::{run_once_with_parts, AskFuture};
pub(crate) use kanzei_harness::ConfigComponent;

mod agent_container;
mod conversation;
mod docs;
mod fast_model;
mod files_view;
mod harness_ext;
mod memory;
mod mobile;
mod prefs;
mod processes;
mod projects;
mod run;
mod settings;
mod state;
mod subagents;
mod update;

pub(crate) use projects::{export_project_data, ExportOptions};

pub(crate) use settings::{global_config_path, settings_save_at_path};
pub(crate) use settings::{LimitsPayload, ProviderPayload, SettingsPayload};

pub(crate) use update::{
    build_stamp, clear_stale_installer, installed_cli_is_older, installer_path, pending_path,
    release_is_newer, update_helper_path, update_log_at, validate_installer, wait_for_parent_exit,
};

pub(crate) use state::{
    ensure_default_process, flush_live_run, normalized_project_root, pending_ask_payload,
    process_info, process_session_id, prompt_attachment_parts, runtime_for,
    stop_runtime_and_finalize, take_pending_ask, ui_probe, ui_probe_result, with_session_id,
    AppState, LiveRun, MobileService, MobileServiceInfo, PendingAsk, ProcessHandle, ProcessInfo,
    PromptAttachment, SessionRuntime, WorktreeInfo, UI_PROBES, UI_PROBE_EMIT, UI_PROBE_SEQ,
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
                    builder = builder.additional_browser_args(&format!(
                        "--remote-debugging-port={}",
                        port.trim()
                    ));
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
            projects::workspace_snapshot,
            docs::docs_snapshot,
            run::run_prompt,
            run::stop_run,
            run::answer_ask,
            run::pending_asks_get,
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
            subagents::defect_review,
            memory::memory_overview,
            memory::memory_entries,
            memory::memory_recalls,
            memory::memory_entry_delete,
            memory::memory_note_candidates,
            memory::memory_note_discard,
            run::run_metrics,
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
            run::app_info,
            run::models_list,
            docs::docs_update,
            docs::docs_open,
            run::summarize_chat,
            docs::git_status,
            docs::conventions_init,
            conversation::conversation_clear,
            conversation::conversation_delete,
            docs::docs_read,
            docs::docs_read_custom,
            docs::architecture_snapshot,
            conversation::conversation_get,
            conversation::conversation_trace_get,
            conversation::conversation_list,
            processes::list_pending_inputs,
            processes::cancel_input,
            projects::project_files,
            processes::process_list,
            processes::process_create,
            processes::process_update,
            processes::process_close,
            processes::worktree_create,
            processes::worktree_diff,
            processes::worktree_merge,
            processes::worktree_discard,
            docs::test_runs_snapshot,
            docs::test_run_record,
            mobile::mobile_service_start,
            mobile::mobile_service_stop,
            agent_container::agent_container_create,
            agent_container::agent_container_upgrade,
            agent_container::agent_container_rollback
        ])
        .run(tauri::generate_context!())
        .expect("error while running kanzei app");
}
