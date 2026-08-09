//! 更新命令与版本检查。

use std::path::Path;
use std::process::Command;

use serde_json::json;

use kanzei_harness::KanzeiConfig;
use kanzei_llm::ProxyConfig;

pub(crate) fn release_is_newer(current_info: &str, tag: &str, published_at: Option<&str>) -> bool {
    let current_hash = current_info.split_whitespace().next().unwrap_or("dev");
    if current_hash == "dev" || tag.is_empty() || tag.contains(current_hash) { return false; }
    let Some((local_stamp, date_only)) = build_stamp(current_info) else { return false };
    let Some(release_stamp) = published_at.and_then(timestamp_digits) else { return false };
    if date_only { release_stamp[..8] > local_stamp[..8] } else { release_stamp > local_stamp }
}

pub(crate) fn build_stamp(info: &str) -> Option<(String, bool)> {
    let token = info.split_whitespace().nth(1)?;
    let digits = timestamp_digits(token)?;
    Some((digits, token.chars().filter(|c| c.is_ascii_digit()).count() < 14))
}

pub(crate) fn timestamp_digits(value: &str) -> Option<String> {
    let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 { return None; }
    if digits.len() >= 14 { Some(digits[..14].to_string()) } else { Some(format!("{digits:0<14}")) }
}

#[tauri::command(rename = "update_check")]
pub(crate) async fn update_check_command() -> Result<serde_json::Value, String> {
    let current = option_env!("KANZEI_BUILD_INFO").unwrap_or("dev");
    let current_hash = current.split_whitespace().next().unwrap_or("dev").to_string();
    let config = KanzeiConfig::load(Path::new(".")).unwrap_or_default();
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let client = kanzei_llm::proxy::build_http_client(&proxy).map_err(|e| e.to_string())?;
    let resp = client
        .get("https://api.github.com/repos/kanze1/kanzei-code/releases/latest")
        .header("user-agent", "kanzei-app")
        .timeout(std::time::Duration::from_secs(15))
        .send().await.map_err(|e| format!("请求失败:{e}"))?;
    if resp.status().as_u16() == 404 {
        return Ok(json!({ "current": current_hash, "status": "none", "message": "还没有发布过安装包(用 scripts/package.ps1 -Publish 发布第一版)" }));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let tag = body["tag_name"].as_str().unwrap_or("").to_string();
    let url = body["assets"].as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(".exe"))))
        .and_then(|a| a["browser_download_url"].as_str()).unwrap_or("").to_string();
    let published_at = body["published_at"].as_str().or_else(|| body["created_at"].as_str());
    let newer = release_is_newer(current, &tag, published_at);
    Ok(json!({ "current": current_hash, "latest": tag, "newer": newer, "url": url, "status": if newer { "update" } else { "latest" } }))
}

#[tauri::command(rename = "update_install")]
pub(crate) async fn update_install_command(app: tauri::AppHandle, url: String) -> Result<String, String> {
    if !url.starts_with("https://github.com/kanze1/kanzei-code/") { return Err("仅允许本仓库 release 资源".into()); }
    let config = KanzeiConfig::load(Path::new(".")).unwrap_or_default();
    let proxy = match config.proxy.as_deref() { Some("off") => ProxyConfig::Disabled, Some("env") | None => ProxyConfig::Env, Some(p) => ProxyConfig::Explicit(p.to_string()) };
    let client = kanzei_llm::proxy::build_http_client(&proxy).map_err(|e| e.to_string())?;
    let bytes = client.get(&url).header("user-agent", "kanzei-app").timeout(std::time::Duration::from_secs(300)).send().await
        .map_err(|e| format!("下载失败:{e}"))?.error_for_status().map_err(|e| format!("下载失败:{e}"))?.bytes().await.map_err(|e| e.to_string())?;
    super::validate_installer(&bytes)?;
    let notes = super::clear_stale_installer();
    let path = super::installer_path();
    std::fs::write(&path, &bytes).map_err(|e| format!("写入安装包失败:{e}(检查 %TEMP% 是否可写或被杀软占用)"))?;
    let exe = std::env::current_exe().map_err(|e| format!("无法定位自身路径:{e}"))?;
    let helper = super::update_helper_path();
    let _ = std::fs::remove_file(&helper);
    std::fs::copy(&exe, &helper).map_err(|e| format!("准备更新交接程序失败:{e}。可手动运行 {} 完成安装。", path.display()))?;
    super::update_log(&format!("交接:helper={} 安装包={}", helper.display(), path.display()));
    Command::new(&helper).arg("--kz-install-helper").arg(&path).arg(&exe).arg(std::process::id().to_string()).spawn()
        .map_err(|e| format!("启动更新交接失败:{e}。可手动运行 {} 完成安装。", path.display()))?;
    let mb = bytes.len() / 1_048_576;
    let prefix = if notes.is_empty() { String::new() } else { format!("{};", notes.join(";")) };
    app.exit(0);
    Ok(format!("{prefix}已下载 {mb} MB,正在退出并静默安装,装完会自动重新打开"))
}

pub(crate) fn startup_update() -> bool { super::startup_update() }
pub(crate) fn sync_bundled_cli() { super::sync_bundled_cli() }
pub(crate) fn cleanup_orphan_webviews() { super::cleanup_orphan_webviews() }
