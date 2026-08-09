//! 更新命令的模块入口。
//!
//! 批2逐步承接更新域；command 与版本判断已在本模块，启动交接辅助仍待本批继续剪切。

/// 仅当 release 的发布时间晚于本地构建时间时才允许提示更新。
/// `KANZEI_BUILD_INFO` 的旧格式只有 yyyy-MM-dd,对旧构建采用“必须晚一天”
/// 的保守判定；新格式使用 UTC 的 yyyyMMddHHmmss，避免开发构建被同日 release 覆盖。
pub(crate) fn release_is_newer(current_info: &str, tag: &str, published_at: Option<&str>) -> bool {
    let current_hash = current_info.split_whitespace().next().unwrap_or("dev");
    if current_hash == "dev" || tag.is_empty() || tag.contains(current_hash) {
        return false;
    }
    let Some((local_stamp, date_only)) = build_stamp(current_info) else {
        return false;
    };
    let Some(release_stamp) = published_at.and_then(timestamp_digits) else {
        return false;
    };
    if date_only {
        release_stamp[..8] > local_stamp[..8]
    } else {
        release_stamp > local_stamp
    }
}

pub(crate) fn build_stamp(info: &str) -> Option<(String, bool)> {
    let token = info.split_whitespace().nth(1)?;
    let digits = timestamp_digits(token)?;
    Some((digits, token.chars().filter(|c| c.is_ascii_digit()).count() < 14))
}

pub(crate) fn timestamp_digits(value: &str) -> Option<String> {
    let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    if digits.len() >= 14 {
        Some(digits[..14].to_string())
    } else {
        Some(format!("{digits:0<14}"))
    }
}

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
