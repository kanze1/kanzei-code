//! R-270 批4:PWA 静态页 serve 与静态资源根解析(由 mobile.rs 拆出)。

use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::mobile_json_response;

/// R-270 批4:PWA 静态页 serve。手机浏览器打开桥接地址加载 PWA(随桌面端发版
/// 分发,不另起服务)。
///
/// D-390:serve 根由调用方传入——发布版 = tauri resource 解包目录
/// (tauri.conf.json `bundle.resources` 配置 `mobile-pwa/**`),开发/测试 =
/// 源码 `crates/kanzei-app/mobile-pwa`。不再依赖编译期常量(安装版
/// CARGO_MANIFEST_DIR 目录不存在,serve 空目录必然 404)。
///
/// 路径安全:请求路径经 strip_prefix('/') 后直接 join 到 pwa_root(mobile-pwa 目录
/// 本身)——PWA 页面内相对引用(如 `app.js`/`style.css`)即 `/<文件名>`;
/// 任何含 `..` 或反斜杠的请求直接 404(不拼文件系统路径)。
/// 返回 true = 已 serve(200/404);false = 非静态资源路径(落回 JSON 分发)。
pub(super) fn serve_pwa(stream: &mut TcpStream, path: &str, pwa_root: &Path) -> bool {
    if !pwa_root.is_dir() {
        return false;
    }
    // 规范化请求路径:/ → index.html;/mobile-pwa/xxx → mobile-pwa/xxx。
    let relative = if path == "/" {
        "index.html"
    } else {
        match path.strip_prefix('/') {
            Some(rest) => rest,
            None => return false,
        }
    };
    // 路径穿越防护:任何含 `..` 或反斜杠的请求直接 404(不拼文件系统路径)。
    if relative.contains("..") || relative.contains('\\') {
        let _ = stream.write_all(&mobile_json_response(
            "404 Not Found",
            &json!({"error": "not_found"}),
        ));
        return true;
    }
    let file = pwa_root.join(relative);
    if !file.is_file() {
        let _ = stream.write_all(&mobile_json_response(
            "404 Not Found",
            &json!({"error": "not_found"}),
        ));
        return true;
    }
    let Ok(bytes) = std::fs::read(&file) else {
        let _ = stream.write_all(&mobile_json_response(
            "500 Internal Server Error",
            &json!({"error": "read_failed"}),
        ));
        return true;
    };
    let content_type = match file.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&bytes);
    true
}

/// D-390:解析 PWA 静态资源根。发布版 = tauri resource 解包目录(tauri.conf.json
/// `bundle.resources` 配置 `mobile-pwa/**`);开发/测试回退源码目录
/// (cargo run / cargo test 的 CARGO_MANIFEST_DIR 均有效)。
pub(super) fn resolve_pwa_root(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    if let Ok(resolved) = app
        .path()
        .resolve("mobile-pwa", tauri::path::BaseDirectory::Resource)
    {
        if resolved.is_dir() {
            return resolved;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("mobile-pwa")
}
