//! UI 对单个托管后台终端的明确停止请求，不取消整个对话。
#[tauri::command]
pub(crate) async fn run_tool_process_stop(
    project_dir: String,
    process_id: String,
) -> Result<bool, String> {
    let root = crate::normalized_project_root(std::path::Path::new(&project_dir));
    kanzei_tools::background::stop_for_project(&root, &process_id).await
}
