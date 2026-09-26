//! 网页预览面板(UI2-0926 #8「网页渲染的工具有吗？对标GPT的」,docs/design/preview_pane.md)。
//!
//! 用户可见、可操作的应用内网页面板:Tauri 子 webview(`Window::add_child`,需 tauri
//! "unstable")+ WebView2 进程内 CDP(webview2-com,不开调试端口)。代理的 `browser` 工具在
//! 面板开着、又正显示这条线时驱动它(见 [`route`]),否则走无头。
//!
//! 模块分工:
//! - 本文件:状态([`PreviewState`] / [`PaneMeta`])与纯函数([`normalize_target`]、
//!   [`nav_allowed`]、[`route`]、[`fit_device`]、[`error_kind`]),全部有单测;
//! - `pane`:子 webview 生命周期(B0 结论:focused(false)、共用主 webview 的 WebView2 环境、
//!   spawn_blocking 里 add_child + 活性回环、隐藏时不截图、设备模式 = 边界缩放 + set_zoom);
//! - `cdp`:进程内 CDP 调用 / 订阅 / 只对面板开 DevTools;
//! - `console`:控制台环形缓冲与 CDP 事件解析;
//! - `commands`:前端 IPC 命令;`agent`:browser 工具的面板后端。
//!
//! 主窗口加了子 webview 以后 `get_webview_window("main")` 返回 None(B0 实测):全仓一律用
//! `get_webview("main")` / `get_window("main")`,本模块测试用 grep 守住。

pub(crate) mod agent;
mod cdp;
pub(crate) mod commands;
pub(crate) mod console;
mod pane;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::Url;

pub(crate) use pane::Pane;
#[cfg(test)]
pub(crate) use pane::{capture_payload, pick_payload};

/// 子 webview 的 label 前缀。每次创建用 `preview-<代次>`:关掉的面板在 tauri 的
/// webview 表里注销是异步的,同名立刻重建会撞「label 已存在」。
/// DevTools 只允许对这类 label 打开(见 cdp::open_devtools)。
pub(crate) const PREVIEW_LABEL: &str = "preview";

/// 是不是预览面板的 label(`preview-<数字>`)。
pub(crate) fn is_preview_label(label: &str) -> bool {
    label
        .strip_prefix(PREVIEW_LABEL)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}
/// 主窗口 / 主 webview 的 label(tauri.conf.json 未显式命名,取默认)。
pub(crate) const MAIN_LABEL: &str = "main";

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 应用句柄:CDP 回调、代理工具等没有 AppHandle 的地方经它发事件、取状态。
pub(crate) fn app() -> Option<&'static tauri::AppHandle> {
    APP.get()
}

/// 启动时装配:记下应用句柄,挂主窗口事件——DPI 变化重放边界(DOM 的 ResizeObserver
/// 在纯 DPI 变化时不触发)、主窗口关闭时收起面板。
pub(crate) fn install(app: &tauri::AppHandle, main_window: &tauri::WebviewWindow) {
    let _ = APP.set(app.clone());
    let handle = app.clone();
    main_window.on_window_event(move |event| match event {
        tauri::WindowEvent::ScaleFactorChanged { .. } => pane::reapply_bounds(&handle),
        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
            pane::close(&handle)
        }
        _ => {}
    });
}

/// 面板状态(`.manage`)。面板句柄可克隆,锁只在取/换句柄时持有,CDP 往返不持锁。
#[derive(Default)]
pub(crate) struct PreviewState {
    pub(crate) slot: Mutex<Option<Pane>>,
    /// 创建面板串行化:两个并发 preview_open 不能各建一个子 webview。
    pub(crate) creating: tokio::sync::Mutex<()>,
    /// 代理动作串行化:同一面板上 open→截图 这种序列不能被另一次调用插队。
    pub(crate) agent: tokio::sync::Mutex<()>,
}

/// 设备预设。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DevicePreset {
    #[default]
    Fill,
    Phone,
    Tablet,
    Desktop,
}

impl DevicePreset {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "fill" => DevicePreset::Fill,
            "phone" => DevicePreset::Phone,
            "tablet" => DevicePreset::Tablet,
            "desktop" => DevicePreset::Desktop,
            _ => return None,
        })
    }

    /// 设备的 CSS 尺寸;Fill 没有固定尺寸。
    pub(crate) fn size(self) -> Option<(f64, f64)> {
        match self {
            DevicePreset::Fill => None,
            DevicePreset::Phone => Some((390.0, 844.0)),
            DevicePreset::Tablet => Some((768.0, 1024.0)),
            DevicePreset::Desktop => Some((1280.0, 800.0)),
        }
    }

    /// 手机/平板模拟触屏。
    pub(crate) fn touch(self) -> bool {
        matches!(self, DevicePreset::Phone | DevicePreset::Tablet)
    }

    /// browser 工具的 viewport 预设映射成面板设备。
    pub(crate) fn from_viewport(viewport: &str) -> Option<Self> {
        if viewport.starts_with("mobile-") {
            Some(DevicePreset::Phone)
        } else if viewport.starts_with("tablet-") {
            Some(DevicePreset::Tablet)
        } else if viewport.starts_with("desktop-") {
            Some(DevicePreset::Desktop)
        } else {
            None
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            DevicePreset::Fill => "fill(自适应面板)",
            DevicePreset::Phone => "phone-390x844",
            DevicePreset::Tablet => "tablet-768x1024",
            DevicePreset::Desktop => "desktop-1280x800",
        }
    }
}

/// 深浅色模拟。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ColorScheme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl ColorScheme {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "auto" => ColorScheme::Auto,
            "light" => ColorScheme::Light,
            "dark" => ColorScheme::Dark,
            _ => return None,
        })
    }
}

/// 矩形(CSS px = Tauri 逻辑像素;应用没用 zoom)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorKind {
    ConnectionRefused,
    UnsafePort,
    Blocked,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PaneError {
    pub(crate) kind: ErrorKind,
    pub(crate) text: String,
}

/// 面板元数据。序列化形态就是 `kz:preview-state` 的载荷(IPC 契约)。
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaneMeta {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) loading: bool,
    pub(crate) can_back: bool,
    pub(crate) can_forward: bool,
    pub(crate) visible: bool,
    pub(crate) bound_process_id: Option<String>,
    pub(crate) device: DevicePreset,
    pub(crate) scheme: ColorScheme,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<PaneError>,
    /// 子 webview 已通过活性回环(B0:add_child 的 Ok 不可信)。
    #[serde(skip)]
    pub(crate) alive: bool,
    /// 前端 #preview-host 的矩形;设备模式在它里面居中缩放。
    #[serde(skip)]
    pub(crate) host: Rect,
    /// 批注(元素选择)模式是否开着。
    #[serde(skip)]
    pub(crate) pick: bool,
}

/// `kz:preview-state` 载荷。
pub(crate) fn state_payload(meta: &PaneMeta) -> serde_json::Value {
    serde_json::to_value(meta).unwrap_or_else(|_| serde_json::json!({}))
}

/// 代理动作走哪个后端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Pane,
    Headless,
}

/// 路由:面板存在且活着、**正在显示**、绑定了线且就是本次调用的线 → 面板;其余一律无头。
///
/// 「正在显示」是硬条件:B0 实测隐藏面板上的 Page.captureScreenshot 永不返回。
/// 子代理复用父 ctx,但 ask 对子代理恒为 Deny,默认本就用不到 browser。
pub(crate) fn route(meta: Option<&PaneMeta>, process_id: Option<&str>) -> Backend {
    match (meta, process_id) {
        (Some(meta), Some(process_id))
            if meta.alive
                && meta.visible
                && meta.bound_process_id.as_deref() == Some(process_id) =>
        {
            Backend::Pane
        }
        _ => Backend::Headless,
    }
}

/// 读当前面板状态做路由(没有面板 / 应用未装配时恒为无头)。
pub(crate) fn route_for(process_id: Option<&str>) -> Backend {
    let meta = current_meta();
    route(meta.as_ref(), process_id)
}

/// 当前面板的元数据快照。
pub(crate) fn current_meta() -> Option<PaneMeta> {
    use tauri::Manager;
    let state = app()?.try_state::<PreviewState>()?;
    let pane = state.slot.lock().ok()?.clone()?;
    let meta = pane.shared.meta.lock().ok()?.clone();
    Some(meta)
}

/// WebView2 ZoomFactor 的下限;更小的面板里设备画面按下限缩放、再裁到面板内。
pub(crate) const MIN_ZOOM: f64 = 0.25;

/// 设备模式的落位:`(子 webview 边界, ZoomFactor)`。
///
/// B0 结论:边界 = 设备 × 缩放(在 host 里居中)+ `Webview::set_zoom(缩放)`,不用
/// setDeviceMetricsOverride 的 scale(实测画面被裁)。Fill 直接铺满 host、缩放 1。
pub(crate) fn fit_device(host: Rect, preset: DevicePreset) -> (Rect, f64) {
    let Some((width, height)) = preset.size() else {
        return (host, 1.0);
    };
    if host.w <= 0.0 || host.h <= 0.0 {
        return (
            Rect {
                x: host.x,
                y: host.y,
                w: 0.0,
                h: 0.0,
            },
            1.0,
        );
    }
    let zoom = (host.w / width).min(host.h / height).clamp(MIN_ZOOM, 1.0);
    let w = (width * zoom).min(host.w);
    let h = (height * zoom).min(host.h);
    (
        Rect {
            x: host.x + (host.w - w) / 2.0,
            y: host.y + (host.h - h) / 2.0,
            w,
            h,
        },
        zoom,
    )
}

/// 导航闸(on_navigation / 代理导航 / 地址栏共用)。
///
/// B0 对照组:不设闸的子 webview 一旦跳到 `http://tauri.localhost` 就拿到全部 IPC
/// (spike_secret 被执行)。所以:
/// - http(s):任何 `*.localhost` 子域一律拒绝(tauri / ipc / asset,以及其它自注册协议),
///   `localhost` 本身放行;尾点(`tauri.localhost.`)先剥再判;
/// - about:blank、data:text/html 放行;file:、javascript: 与其它 scheme 一律拒绝。
pub(crate) fn nav_allowed(url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => match url.host_str() {
            // IP 字面量(含 `[::1]`)不可能以 .localhost 结尾;域名先剥尾点再判。
            Some(host) => {
                let host = host.trim_end_matches('.').to_ascii_lowercase();
                !host.is_empty() && !host.ends_with(".localhost")
            }
            None => false,
        },
        "about" => url.as_str() == "about:blank",
        "data" => url.path().to_ascii_lowercase().starts_with("text/html"),
        _ => false,
    }
}

/// 地址栏输入的解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Url(String),
    Local(PathBuf),
}

/// 地址栏 / preview_open 的目标规范化。
///
/// - `5173` → `http://localhost:5173/`;`:3000/x` → `http://localhost:3000/x`;
/// - `localhost:8080`、`127.0.0.1:4173`、`[::1]:5000/a` 补 `http://`;
/// - 带 scheme 的按 URL 解析并过 [`nav_allowed`](file:// 转成本地文件);
/// - 盘符 / UNC 绝对路径、`./ ../ /` 开头或在某个根里存在的相对路径 → 本地文件
///   (由调用方换成静态服务 URL);
/// - 其余像域名的补 `https://`,再不像就报错。
pub(crate) fn normalize_target(input: &str, roots: &[PathBuf]) -> Result<Target, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("地址为空".into());
    }
    if raw.bytes().all(|b| b.is_ascii_digit()) {
        let port = parse_port(raw)?;
        return Ok(Target::Url(format!("http://localhost:{port}/")));
    }
    if let Some(rest) = raw.strip_prefix(':') {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let tail = &rest[digits.len()..];
        if !digits.is_empty() && (tail.is_empty() || tail.starts_with('/') || tail.starts_with('?'))
        {
            let port = parse_port(&digits)?;
            let path = if tail.is_empty() { "/" } else { tail };
            return checked_url(&format!("http://localhost:{port}{path}"));
        }
        return Err(format!("看不懂的地址: {raw}"));
    }
    if is_windows_absolute(raw) {
        return local(PathBuf::from(raw));
    }
    let lower = raw.to_ascii_lowercase();
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        if let Some(rest) = lower.strip_prefix(host) {
            if rest.is_empty() || rest.starts_with(':') || rest.starts_with('/') {
                return checked_url(&format!("http://{raw}"));
            }
        }
    }
    const SCHEMES: [&str; 8] = [
        "about:",
        "data:",
        "javascript:",
        "vbscript:",
        "file:",
        "mailto:",
        "blob:",
        "tauri:",
    ];
    if raw.contains("://") || SCHEMES.iter().any(|scheme| lower.starts_with(scheme)) {
        let url = Url::parse(raw).map_err(|e| format!("地址无法解析: {e}"))?;
        return match url.scheme() {
            "file" => url
                .to_file_path()
                .map_err(|_| format!("file 地址不是本地路径: {raw}"))
                .and_then(local),
            "http" | "https" | "about" | "data" => checked_url(url.as_str()),
            other => Err(format!(
                "不支持的地址类型 {other}:(只支持 http、https、本地文件)"
            )),
        };
    }
    let relative = raw.trim_start_matches(['/', '\\']);
    let explicit_path = raw.starts_with("./")
        || raw.starts_with("../")
        || raw.starts_with(".\\")
        || raw.starts_with("..\\")
        || raw.starts_with('/')
        || raw.starts_with('\\');
    for root in roots {
        let candidate = root.join(relative);
        if candidate.exists() {
            return local(candidate);
        }
    }
    let looks_like_file = [".html", ".htm", ".svg", ".xhtml"]
        .iter()
        .any(|ext| lower.ends_with(ext));
    if explicit_path || looks_like_file || raw.contains('\\') {
        return Err(if roots.is_empty() {
            format!("找不到本地文件 {raw}(相对路径需要项目上下文,请给绝对路径)")
        } else {
            format!("找不到本地文件 {raw}")
        });
    }
    let host = raw.split(['/', '?', '#']).next().unwrap_or("");
    let domain_like = host.contains('.')
        && !raw.contains(char::is_whitespace)
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    if domain_like {
        return checked_url(&format!("https://{raw}"));
    }
    Err(format!("看不懂的地址: {raw}"))
}

fn parse_port(raw: &str) -> Result<u16, String> {
    raw.parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| format!("端口不合法: {raw}"))
}

fn is_windows_absolute(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/'))
        || raw.starts_with("\\\\")
}

fn local(path: PathBuf) -> Result<Target, String> {
    if path.exists() {
        Ok(Target::Local(path))
    } else {
        Err(format!("找不到本地文件 {}", path.display()))
    }
}

fn checked_url(raw: &str) -> Result<Target, String> {
    let url = Url::parse(raw).map_err(|e| format!("地址无法解析: {e}"))?;
    if !nav_allowed(&url) {
        return Err(format!(
            "拒绝打开 {url}:应用内部地址(*.localhost 子域)、file:、javascript: 等一律不在面板里打开"
        ));
    }
    Ok(Target::Url(url.to_string()))
}

/// 规范化后换成可导航的 URL:本地文件走 127.0.0.1 静态服务(与 browser 工具同一个服务)。
pub(crate) fn resolve_target(input: &str, roots: &[PathBuf]) -> Result<String, String> {
    match normalize_target(input, roots)? {
        Target::Url(url) => Ok(url),
        Target::Local(path) => kanzei_tools::preview_server::global()?.url_for_path(&path, roots),
    }
}

/// 导航失败文本的分类(`kz:preview-state` 的 error.kind)。
pub(crate) fn error_kind(error_text: &str) -> ErrorKind {
    if error_text.contains("ERR_CONNECTION_REFUSED") {
        ErrorKind::ConnectionRefused
    } else if error_text.contains("ERR_UNSAFE_PORT") {
        ErrorKind::UnsafePort
    } else if error_text.contains("ERR_BLOCKED") {
        ErrorKind::Blocked
    } else {
        ErrorKind::Other
    }
}

/// 面向用户的失败说明(带原始错误码,代理侧的 browser_error 也靠它给处理建议)。
pub(crate) fn describe_error(error_text: &str, url: &str) -> PaneError {
    let kind = error_kind(error_text);
    let text = match kind {
        ErrorKind::ConnectionRefused => {
            format!("服务没在跑?{url} 拒绝连接({error_text})")
        }
        ErrorKind::UnsafePort => {
            format!("浏览器禁止访问这个端口,请换一个端口({error_text})")
        }
        ErrorKind::Blocked => format!("请求被拦截({error_text})"),
        ErrorKind::Other => format!("页面加载失败({error_text})"),
    };
    PaneError { kind, text }
}

/// 进程(对话线)对应的根:工作树线先给工作树、再给主根;默认线 `d|<root>`。
pub(crate) fn roots_for_process(app: &tauri::AppHandle, process_id: Option<&str>) -> Vec<PathBuf> {
    use tauri::Manager;
    let Some(process_id) = process_id else {
        return Vec::new();
    };
    if let Some(state) = app.try_state::<crate::AppState>() {
        use crate::MutexPoisonExt;
        if let Some(process) = state.processes.lock_or_recover().get(process_id) {
            let mut roots = Vec::new();
            if let Some(worktree) = &process.worktree_path {
                roots.push(worktree.0.clone());
            }
            roots.push(process.project_dir.0.clone());
            return roots;
        }
    }
    process_id
        .strip_prefix("d|")
        .map(|root| vec![Path::new(root).to_path_buf()])
        .unwrap_or_default()
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod pure_tests;
