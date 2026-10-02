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
//! - `cdp`:进程内 CDP 调用 / 订阅 / 子 frame 导航闸 / 只对面板开 DevTools;
//! - `console`:控制台环形缓冲与 CDP 事件解析;
//! - `host`:主窗口的焦点归还与移动通知(开 unstable 后 wry 不再替主 webview 做这两件事);
//! - `commands`:前端 IPC 命令;`agent`:browser 工具的面板后端。
//!
//! 主窗口加了子 webview 以后 `get_webview_window("main")` 返回 None(B0 实测):全仓一律用
//! `get_webview("main")` / `get_window("main")`,本模块测试用 grep 守住。

pub(crate) mod agent;
mod cdp;
pub(crate) mod commands;
pub(crate) mod console;
mod host;
mod pane;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Url;

use console::{CdpSignal, EntryDraft};
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
/// 在纯 DPI 变化时不触发)、主窗口关闭时收起面板;再给主窗口补上焦点归还与移动通知
/// (`host`:开 tauri "unstable" 后主 webview 按 WindowChild 建,wry 不再挂父窗口子类化,
/// 对所有用户生效,与是否打开过面板无关)。
pub(crate) fn install(app: &tauri::AppHandle, main_window: &tauri::WebviewWindow) {
    let _ = APP.set(app.clone());
    host::attach_main(main_window);
    let handle = app.clone();
    main_window.on_window_event(move |event| match event {
        tauri::WindowEvent::ScaleFactorChanged { .. } => pane::reapply_bounds(&handle),
        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
            pane::close(&handle)
        }
        _ => {}
    });
}

/// 主 webview 的页面加载事件(main.rs 在主窗口 builder 上挂 `on_page_load`)。
///
/// 主界面开始(重新)加载——F5 / Ctrl+R(wry 的浏览器加速键默认开着)、界面自己 reload——时收掉面板:
/// Rust 侧的子 webview 还活着、还可见,新起的前端却从「没有面板」起步(`alive` 为 false),原生面板会
/// 停在旧矩形上盖住对话列。这是前端启动时无条件 `preview_close` 的双保险(preview_pane.md「前端」§2)。
///
/// 第一次启动的那次加载同样会来:那时 slot 是空的,`pane::close` 只递增关闭代次就返回,不发事件、
/// 不碰任何窗口(代次只影响「创建途中被关」的比对,而那时还没有任何创建)。
/// 关闭放到异步任务里做,与 preview_close 命令同一条路径:回调本身跑在主 webview 的 ContentLoading
/// COM 回调里,不在那里同步关另一个 webview。
pub(crate) fn on_main_page_load(
    app: &tauri::AppHandle,
    label: &str,
    event: tauri::webview::PageLoadEvent,
) {
    if !main_load_closes_pane(label, event) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { pane::close(&app) });
}

/// 哪些页面加载事件要收面板:只有主 webview 的 `Started`(ContentLoading:新文档开始加载;
/// pushState / 锚点跳转这类同文档导航不触发)。面板自己的加载、主界面的 `Finished` 都不算。
pub(crate) fn main_load_closes_pane(label: &str, event: tauri::webview::PageLoadEvent) -> bool {
    label == MAIN_LABEL && event == tauri::webview::PageLoadEvent::Started
}

/// 面板状态(`.manage`)。面板句柄可克隆,锁只在取/换句柄时持有,CDP 往返不持锁。
#[derive(Default)]
pub(crate) struct PreviewState {
    pub(crate) slot: Mutex<Option<Pane>>,
    /// 创建面板串行化:两个并发 preview_open 不能各建一个子 webview。
    pub(crate) creating: tokio::sync::Mutex<()>,
    /// 代理动作串行化:同一面板上 open→截图 这种序列不能被另一次调用插队
    /// (父线与复用父线身份的子代理共用这把锁)。
    pub(crate) agent: tokio::sync::Mutex<()>,
    /// 关闭次数。创建要好几百毫秒,期间用户点了关闭时 slot 还是空的;建好后比对它,
    /// 变了就把刚建好的面板当场关掉,不写进 slot、不显示。
    pub(crate) close_epoch: std::sync::atomic::AtomicU64,
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
    /// 最近一次被隐藏的时刻(ms)。前端的「遮挡冻结」(菜单 / 模态压到面板上)也是
    /// set_visible(false),路由据此区分「刚被遮住」与「早就收起」(见 [`should_wait_visible`])。
    #[serde(skip)]
    pub(crate) hidden_at: u64,
    /// 主 frame 错误态的判定状态。
    #[serde(skip)]
    pub(crate) nav: NavTracker,
}

/// 文档失败暂存的条数上限(iframe 多的页面一次加载会有好几条)。
const FAILURE_MEMORY: usize = 16;

/// 主 frame 错误态的判定(随 PaneMeta 一起加锁)。
///
/// Network.loadingFailed(Document) **不分帧**:外站的广告 / 跟踪 iframe 被 WebView2 跟踪防护拦下
/// (ERR_BLOCKED_BY_CLIENT)、iframe 目标带 X-Frame-Options(ERR_BLOCKED_BY_RESPONSE)、iframe
/// 指向死端口,都会来一条。所以失败只**暂存**(requestId → errorText),等主 frame 真的提交了
/// 错误页(frameNavigated 无 parentId、带 unreachableUrl)才进入错误态;错误页的 loaderId 就是
/// 那次失败请求的 requestId(Edge 实测,主帧与子帧同一规律),据此取具体错误码。
#[derive(Clone, Debug, Default)]
pub(crate) struct NavTracker {
    /// 主 frame 的 CDP frame id(创建时 Page.getFrameTree 取,主 frame 每次提交时刷新)。
    pub(crate) main_frame_id: Option<String>,
    failures: VecDeque<(String, String)>,
    /// 当前错误页的 loaderId:错误页比 loadingFailed 先到时,后到的失败据此补上具体错误码。
    error_loader: Option<String>,
}

impl NavTracker {
    fn remember_failure(&mut self, request_id: &str, error_text: &str) {
        self.failures
            .push_back((request_id.to_string(), error_text.to_string()));
        while self.failures.len() > FAILURE_MEMORY {
            self.failures.pop_front();
        }
    }

    fn take_failure(&mut self, request_id: &str) -> Option<String> {
        let index = self.failures.iter().position(|(id, _)| id == request_id)?;
        self.failures.remove(index).map(|(_, text)| text)
    }
}

/// CDP 信号落到面板元数据之后,调用方要做的事。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MetaEffect {
    /// 与界面无关(比如 iframe 的失败,只是暂存)。
    None,
    /// 元数据变了:发 kz:preview-state。
    Changed,
    /// 主 frame 提交了新文档:清控制台、发状态、重放设备模拟、刷新前进后退。
    Committed,
    /// 主 frame 提交了错误页:清控制台后记这一条、fail_seq + 1、发状态。
    Failed(EntryDraft),
    /// 主 frame 同文档导航(SPA 路由):发状态、刷新前进后退。
    SameDocument,
}

impl PaneMeta {
    /// 顶层导航开始(on_navigation 放行、preview_open、代理导航):错误态只在这里复位。
    pub(crate) fn begin_navigation(&mut self) {
        self.error = None;
        self.nav.error_loader = None;
        self.loading = true;
    }

    /// wry 的 `PageLoadEvent::Started`(= ContentLoading)。WebView2 对 chrome-error 错误页
    /// 同样触发它(B0 的死地址有一条 started),所以这里**不碰** error——否则
    /// 「服务没在跑?」会被自己的错误页抹掉。
    pub(crate) fn content_loading(&mut self, url: &str) {
        self.loading = true;
        self.url = url.to_string();
    }

    /// 可见性上报(preview_open 视同显示)。显示时按上报的线**覆盖**绑定(null 即解绑:
    /// 用户此刻看的上下文没有线,就不该让别的线的代理驱动这块面板);隐藏时保留绑定,
    /// 并记下隐藏时刻。
    pub(crate) fn set_visibility(&mut self, visible: bool, process_id: Option<String>, now: u64) {
        if visible {
            self.bound_process_id = process_id;
        } else if self.visible {
            self.hidden_at = now;
        }
        self.visible = visible;
    }

    /// 把一个 CDP 信号落到元数据上(纯函数,事件次序的单测都在 pure_tests)。
    pub(crate) fn apply(&mut self, signal: &CdpSignal) -> MetaEffect {
        match signal {
            CdpSignal::DocumentFailed {
                request_id,
                error_text,
            } => {
                self.nav.remember_failure(request_id, error_text);
                if !request_id.is_empty() && self.nav.error_loader.as_deref() == Some(request_id) {
                    // 错误页先到(罕见):补上具体错误码。
                    self.error = Some(describe_error(error_text, &self.url));
                    return MetaEffect::Changed;
                }
                MetaEffect::None
            }
            CdpSignal::MainFrameNavigated {
                frame_id,
                loader_id,
                url,
                unreachable_url,
            } => {
                if !frame_id.is_empty() {
                    self.nav.main_frame_id = Some(frame_id.clone());
                }
                match unreachable_url {
                    Some(unreachable) => {
                        let error_text = self
                            .nav
                            .take_failure(loader_id)
                            .unwrap_or_else(|| "net::ERR_FAILED".to_string());
                        self.url = unreachable.clone();
                        self.loading = false;
                        self.error = Some(describe_error(&error_text, unreachable));
                        self.nav.error_loader = Some(loader_id.clone());
                        MetaEffect::Failed(EntryDraft {
                            level: "network".into(),
                            text: format!("页面加载失败: {error_text}"),
                            url: unreachable.clone(),
                            ..EntryDraft::default()
                        })
                    }
                    None => {
                        self.url = url.clone();
                        self.error = None;
                        self.nav.error_loader = None;
                        MetaEffect::Committed
                    }
                }
            }
            CdpSignal::SameDocumentNavigated { frame_id, url } => {
                if self.nav.main_frame_id.as_deref() != Some(frame_id.as_str()) || self.url == *url
                {
                    return MetaEffect::None;
                }
                self.url = url.clone();
                MetaEffect::SameDocument
            }
            CdpSignal::LoadEventFired => {
                self.loading = false;
                MetaEffect::Changed
            }
            CdpSignal::Entry(_) | CdpSignal::InspectNode { .. } => MetaEffect::None,
        }
    }
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
/// 子代理复用父线的 ctx(含 process_id),所以同样会路由到父线绑定的面板;
/// 与父线的调用经 `PreviewState.agent` 串行,不会交错。
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

/// 遮挡冻结的宽限:隐藏不到这么久的面板视为「被菜单 / 模态暂时遮住」,代理调用先等它露出来。
pub(crate) const OCCLUSION_GRACE_MS: u64 = 10_000;
/// 等面板重新露出来的上限。
pub(crate) const VISIBLE_WAIT: Duration = Duration::from_millis(1500);

/// 面板绑定本线、活着、只是**刚被**隐藏(前端遮挡冻结也走 set_visible(false))→ 值得等一等,
/// 免得 open→click→截图 这样的序列中途换到无头(另一张页面、另一份状态)。
/// 早就收起的面板(用户切到别的视图)不等,直接无头,不给每次调用白加延迟。
pub(crate) fn should_wait_visible(meta: &PaneMeta, process_id: Option<&str>, now: u64) -> bool {
    process_id.is_some()
        && meta.alive
        && !meta.visible
        // 错误页时前端主动收起面板,等不来。
        && meta.error.is_none()
        && meta.bound_process_id.as_deref() == process_id
        && now.saturating_sub(meta.hidden_at) <= OCCLUSION_GRACE_MS
}

/// 权限判定用的后端预估:与 [`route_waiting`] 的结论一致(刚被遮住的面板按面板算)。
pub(crate) fn route_hint(meta: Option<&PaneMeta>, process_id: Option<&str>, now: u64) -> Backend {
    match meta {
        Some(meta) if should_wait_visible(meta, process_id, now) => Backend::Pane,
        _ => route(meta, process_id),
    }
}

/// 权限判定用:[`route_hint`] 的当前值(没有面板 / 应用未装配时恒为无头)。
pub(crate) fn route_hint_for(process_id: Option<&str>) -> Backend {
    let meta = current_meta();
    route_hint(meta.as_ref(), process_id, now_ms())
}

/// 真正执行前的路由:面板刚被遮住时最多等 [`VISIBLE_WAIT`] 让它露出来,再定后端。
pub(crate) async fn route_waiting(process_id: Option<&str>) -> Backend {
    let deadline = tokio::time::Instant::now() + VISIBLE_WAIT;
    loop {
        let meta = current_meta();
        if route(meta.as_ref(), process_id) == Backend::Pane {
            return Backend::Pane;
        }
        let worth_waiting = meta
            .as_ref()
            .is_some_and(|meta| should_wait_visible(meta, process_id, now_ms()));
        if !worth_waiting || tokio::time::Instant::now() >= deadline {
            return Backend::Headless;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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
        "http" | "https" => web_host_allowed(url),
        "about" => url.as_str() == "about:blank",
        "data" => url.path().to_ascii_lowercase().starts_with("text/html"),
        _ => false,
    }
}

/// http(s) 的 host 过闸:任何 `*.localhost` 子域一律拒绝(不看端口——tauri 的 is_local_url
/// 也不看端口,`http://tauri.localhost:3000` 的内容来自本机 3000 端口却被当成本地源)。
fn web_host_allowed(url: &Url) -> bool {
    match url.host_str() {
        // IP 字面量(含 `[::1]`)不可能以 .localhost 结尾;域名先剥尾点再判。
        Some(host) => {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            !host.is_empty() && !host.ends_with(".localhost")
        }
        None => false,
    }
}

/// 子 frame(iframe)的导航闸:`cdp::guard_frame_navigation` 挂在 FrameNavigationStarting 上。
///
/// on_navigation(NavigationStarting)只管顶层导航;而 wry 注册 custom protocol 用的是
/// SOURCE_KINDS_ALL,子 frame 请求 `http://tauri.localhost/…` 同样会被应用协议接管,远程页面里
/// 又注入了带 invoke key 的 `__TAURI_INTERNALS__`——所以子 frame 也要过同一条 host 规则。
/// 与顶层相比放宽的只有页面内部常见、又拿不到新源的几种:`about:`(blank / srcdoc)、`data:`、
/// `javascript:`(都继承或不透明于父页面的源)、`blob:`(内层源同样过 host 规则)。
pub(crate) fn frame_nav_allowed(url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => web_host_allowed(url),
        "about" | "data" | "javascript" => true,
        "blob" => Url::parse(url.path()).ok().is_some_and(|inner| {
            matches!(inner.scheme(), "http" | "https") && web_host_allowed(&inner)
        }),
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
