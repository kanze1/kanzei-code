//! 预览面板的前端命令(IPC 契约见 docs/design/preview_pane.md §6)。全部 async:
//! Windows 上在同步命令里建 webview 会死锁,add_child 又会在调用线程上同步等 UI 线程。

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use tauri::AppHandle;

use super::{pane, ColorScheme, DevicePreset, Rect};

/// 图片类命令的体积上限。
pub(crate) const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
/// 工具截图目录(与 kanzei-core runner/tool_images.rs 同一路径)。
pub(crate) const TOOL_IMAGES_REL: &str = ".kanzei/artifacts/tool-images";
const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];

#[tauri::command]
pub(crate) async fn preview_open(
    app: AppHandle,
    target: String,
    process_id: Option<String>,
    bounds: Rect,
) -> Result<Value, String> {
    let roots = super::roots_for_process(&app, process_id.as_deref());
    let url = super::resolve_target(&target, &roots)?;
    pane::open(&app, &url, process_id, bounds).await
}

#[tauri::command]
pub(crate) async fn preview_set_bounds(
    app: AppHandle,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    pane::set_bounds(&app, Rect { x, y, w, h });
    Ok(())
}

#[tauri::command]
pub(crate) async fn preview_set_visible(
    app: AppHandle,
    visible: bool,
    process_id: Option<String>,
) -> Result<Value, String> {
    Ok(pane::set_visible(&app, visible, process_id))
}

#[tauri::command]
pub(crate) async fn preview_nav(app: AppHandle, action: String) -> Result<(), String> {
    pane::nav(&app, &action).await
}

#[tauri::command]
pub(crate) async fn preview_close(app: AppHandle) -> Result<(), String> {
    pane::close(&app);
    Ok(())
}

#[tauri::command]
pub(crate) async fn preview_capture(
    app: AppHandle,
    full_page: Option<bool>,
    clip: Option<Rect>,
) -> Result<Value, String> {
    let pane = pane::current(&app).ok_or("预览面板没有打开")?;
    let (png, width, height) = pane::capture(&pane, full_page.unwrap_or(false), clip).await?;
    Ok(pane::capture_payload(png, width, height))
}

#[tauri::command]
pub(crate) async fn preview_console(
    app: AppHandle,
    since_seq: Option<u64>,
) -> Result<Value, String> {
    let entries = pane::current(&app)
        .map(|pane| pane.shared.console().since(since_seq.unwrap_or(0)))
        .unwrap_or_default();
    Ok(super::console::console_payload(&entries))
}

#[tauri::command]
pub(crate) async fn preview_console_clear(app: AppHandle) -> Result<(), String> {
    if let Some(pane) = pane::current(&app) {
        pane.shared.console().clear();
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn preview_device(
    app: AppHandle,
    preset: Option<String>,
    scheme: Option<String>,
) -> Result<Value, String> {
    let preset = match preset.as_deref() {
        Some(raw) => Some(
            DevicePreset::parse(raw)
                .ok_or_else(|| format!("未知设备 {raw}(fill|phone|tablet|desktop)"))?,
        ),
        None => None,
    };
    let scheme = match scheme.as_deref() {
        Some(raw) => Some(
            ColorScheme::parse(raw).ok_or_else(|| format!("未知配色 {raw}(auto|light|dark)"))?,
        ),
        None => None,
    };
    pane::set_device(&app, preset, scheme).await
}

#[tauri::command]
pub(crate) async fn preview_pick(app: AppHandle, on: bool) -> Result<(), String> {
    pane::set_pick(&app, on).await
}

#[tauri::command]
pub(crate) async fn preview_snippet(html: String) -> Result<Value, String> {
    let url = kanzei_tools::preview_server::global()?.add_snippet(&html)?;
    Ok(json!({ "url": url }))
}

#[tauri::command]
pub(crate) async fn preview_dev_urls(project_dir: String) -> Result<Value, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    Ok(dev_urls_payload(&kanzei_tools::dev_urls::dev_server_urls(
        &root,
    )))
}

pub(crate) fn dev_urls_payload(urls: &[kanzei_tools::dev_urls::DevUrl]) -> Value {
    json!({ "urls": urls })
}

#[tauri::command]
pub(crate) async fn preview_clear_site_data(app: AppHandle) -> Result<(), String> {
    pane::clear_site_data(&app).await.map(|_| ())
}

#[tauri::command]
pub(crate) async fn preview_open_devtools(app: AppHandle) -> Result<(), String> {
    pane::open_devtools(&app).await
}

/// 在系统默认浏览器里打开面板当前地址(只放 http/https)。
#[tauri::command]
pub(crate) async fn preview_open_external(app: AppHandle) -> Result<(), String> {
    let pane = pane::current(&app).ok_or("预览面板没有打开")?;
    let url = pane.shared.meta().url.clone();
    let parsed = tauri::Url::parse(&url).map_err(|_| format!("当前地址不是网页: {url}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("只能在系统浏览器里打开 http(s) 地址: {url}"));
    }
    // url.dll 直接把整个参数当 URL 交给默认浏览器,不经 cmd 的 & | 解析。
    std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(parsed.as_str())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("打开系统浏览器失败: {e}"))
}

/// 对话里 `[tool-image]` 标记指向的截图。只收 `.kanzei/artifacts/tool-images/<sha256>.<ext>`。
#[tauri::command]
pub(crate) async fn tool_image(project_dir: String, rel: String) -> Result<Value, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    let path = tool_image_path(&root, &rel)?;
    image_payload(&path)
}

/// 交付卡片的图片缩略图:沿用 open_delivered_path 的根内校验。
#[tauri::command]
pub(crate) async fn delivered_image(project_dir: String, path: String) -> Result<Value, String> {
    let target = crate::commands::run::resolve_delivered_path(&project_dir, &path)?;
    let ext = target
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("不是可预览的图片: {}", target.display()));
    }
    image_payload(&target)
}

pub(crate) fn tool_image_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim().replace('\\', "/");
    let name = rel
        .strip_prefix(TOOL_IMAGES_REL)
        .and_then(|rest| rest.strip_prefix('/'))
        .ok_or_else(|| format!("只能读取 {TOOL_IMAGES_REL}/ 下的截图: {rel}"))?;
    let (stem, ext) = name
        .rsplit_once('.')
        .ok_or_else(|| format!("截图文件名不合法: {name}"))?;
    let valid = stem.len() == 64
        && stem.bytes().all(|b| b.is_ascii_hexdigit())
        && matches!(ext, "png" | "jpg" | "webp" | "gif");
    if !valid {
        return Err(format!("截图文件名不合法: {name}"));
    }
    let dir = root
        .join(TOOL_IMAGES_REL)
        .canonicalize()
        .map_err(|e| format!("截图目录不存在: {e}"))?;
    let path = dir
        .join(name)
        .canonicalize()
        .map_err(|e| format!("截图不存在: {e}"))?;
    if !path.starts_with(&dir) {
        return Err(format!("拒绝读取截图目录之外的文件: {}", path.display()));
    }
    Ok(path)
}

fn image_payload(path: &Path) -> Result<Value, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读不到图片: {e}"))?;
    if !meta.is_file() {
        return Err(format!("不是文件: {}", path.display()));
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "图片过大({} 字节 > {} 上限)",
            meta.len(),
            MAX_IMAGE_BYTES
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读不到图片: {e}"))?;
    Ok(json!({ "png": base64::engine::general_purpose::STANDARD.encode(bytes) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-preview-cmd-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(TOOL_IMAGES_REL)).unwrap();
        root
    }

    #[test]
    fn 工具截图路径只收截图目录下的sha文件名() {
        let root = fixture("tool-image");
        let name = format!("{}.png", "a".repeat(64));
        std::fs::write(root.join(TOOL_IMAGES_REL).join(&name), b"png").unwrap();
        std::fs::write(root.join("secret.txt"), b"secret").unwrap();
        // 目录里但文件名不合规的:只能靠文件名规则拦,不能指望路径包含判定。
        std::fs::write(root.join(TOOL_IMAGES_REL).join("notes.txt"), b"x").unwrap();
        std::fs::write(root.join(TOOL_IMAGES_REL).join("short.png"), b"x").unwrap();
        for bad_name in ["notes.txt", "short.png"] {
            assert!(
                tool_image_path(&root, &format!("{TOOL_IMAGES_REL}/{bad_name}")).is_err(),
                "{bad_name} 必须被文件名规则拒绝"
            );
        }
        let ok = tool_image_path(&root, &format!("{TOOL_IMAGES_REL}/{name}")).unwrap();
        assert!(ok.ends_with(&name));
        let backslash = tool_image_path(&root, &format!(".kanzei\\artifacts\\tool-images\\{name}"));
        assert!(backslash.is_ok(), "Windows 分隔符同样可用");
        for bad in [
            "secret.txt".to_string(),
            format!("{TOOL_IMAGES_REL}/../../secret.txt"),
            format!("{TOOL_IMAGES_REL}/{}.png", "a".repeat(63)),
            format!("{TOOL_IMAGES_REL}/{}.txt", "a".repeat(64)),
            format!("{TOOL_IMAGES_REL}/sub/{name}"),
        ] {
            assert!(tool_image_path(&root, &bad).is_err(), "{bad} 必须被拒");
        }
        let big = root.join("big.png");
        std::fs::write(&big, vec![0u8; (MAX_IMAGE_BYTES + 1) as usize]).unwrap();
        assert!(image_payload(&big).unwrap_err().contains("过大"));
        std::fs::remove_dir_all(&root).ok();
    }
}
