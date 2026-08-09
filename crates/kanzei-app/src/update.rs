//! 更新命令的模块入口。
//!
//! 批2先固定启动调用与 command 的模块边界；实现剪切将在本批完成时移入本文件，
//! 这些转发保持现有行为与测试路径不变。

#[tauri::command(rename = "update_check")]
pub(crate) async fn update_check_command() -> Result<serde_json::Value, String> {
    super::update_check_impl().await
}

#[tauri::command(rename = "update_install")]
pub(crate) async fn update_install_command(
    app: tauri::AppHandle,
    url: String,
) -> Result<String, String> {
    super::update_install_impl(app, url).await
}

pub(crate) fn startup_update() -> bool {
    super::startup_update()
}

pub(crate) fn sync_bundled_cli() {
    super::sync_bundled_cli()
}

pub(crate) fn cleanup_orphan_webviews() {
    super::cleanup_orphan_webviews()
}
