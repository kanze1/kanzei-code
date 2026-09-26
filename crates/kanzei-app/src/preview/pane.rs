//! 预览面板子 webview 的生命周期。B0 规则(docs/design/preview_pane.md §9)在这里落地:
//!
//! 1. builder `.focused(false)`:默认 true 会让 wry MoveFocus(PROGRAMMATIC),每次开面板都从
//!    用户正在用的程序那里抢走系统焦点;
//! 2. `.with_environment(主 webview 的 ICoreWebView2Environment)`:不共用环境时(KANZEI_E2E_CDP
//!    给主 webview 加了浏览器参数)创建会失败,而 add_child 照样返回 Ok,只在日志留一行;
//! 3. add_child 在 spawn_blocking 里调(它在调用线程上同步等 UI 线程),之后用 with_webview
//!    回环确认活着——Ok 与 `window.webviews()` 都不可信;
//! 4. 路由只在面板可见时走面板,截图 5 秒超时、迟到的完成回调作废(隐藏面板截图永不返回);
//! 5. 设备模式 = 边界(设备 × 缩放,居中)+ `set_zoom(缩放)` + 触屏模拟,离开设备模式把缩放
//!    设回 1(ZoomFactor 跨导航、跨源保持);
//! 6. 「服务没在跑?」只认**主 frame** 提交的 chrome-error 错误页(frameNavigated 带
//!    unreachableUrl),具体错误码按 loaderId 从暂存的 Network.loadingFailed(Document)里取
//!    (iframe 的失败同样会来,不能直接当页面失败,见 PaneMeta::apply);on_page_load 在这种情况下
//!    照样报 Started / Finished,错误态只在导航开始时复位;
//! 7. DPI 变化(WindowEvent::ScaleFactorChanged)重放边界。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewBuilder,
    WebviewUrl,
};

use super::console::{parse_event, CdpSignal, ConsoleRing, EntryDraft, SUBSCRIBED_EVENTS};
use super::{
    cdp, fit_device, host, nav_allowed, now_ms, state_payload, ColorScheme, DevicePreset,
    MetaEffect, PaneMeta, PreviewState, Rect, MAIN_LABEL, PREVIEW_LABEL, VISIBLE_WAIT,
};

/// 截图超时(B0:可见时 12–25 ms;隐藏时永不返回)。
pub(crate) const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
/// 整页截图的高度上限(CSS px),防超长页面撑爆内存。
const FULL_PAGE_MAX_HEIGHT: f64 = 16_384.0;
/// 控制台增量推送间隔。
const CONSOLE_FLUSH: Duration = Duration::from_millis(250);

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// 面板句柄(可克隆;状态在 `shared` 里)。
#[derive(Clone)]
pub(crate) struct Pane {
    pub(crate) webview: Webview,
    pub(crate) shared: Arc<Shared>,
}

pub(crate) struct Shared {
    pub(crate) generation: u64,
    pub(crate) meta: Mutex<PaneMeta>,
    pub(crate) console: Mutex<ConsoleRing>,
    /// Page.loadEventFired 计数(代理 open 等它)。
    pub(crate) load_seq: AtomicU64,
    /// 主文档失败计数(代理 open 据此提前结束等待)。
    pub(crate) fail_seq: AtomicU64,
    pub(crate) closed: AtomicBool,
    zoom: Mutex<f64>,
}

impl Shared {
    pub(crate) fn meta(&self) -> MutexGuard<'_, PaneMeta> {
        self.meta
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub(crate) fn console(&self) -> MutexGuard<'_, ConsoleRing> {
        self.console
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// 当前 WebView2 ZoomFactor(设备模式下 < 1)。
    pub(crate) fn zoom(&self) -> f64 {
        *self
            .zoom
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

fn state(app: &AppHandle) -> Option<tauri::State<'_, PreviewState>> {
    app.try_state::<PreviewState>()
}

/// 当前面板(已关闭的不算)。
pub(crate) fn current(app: &AppHandle) -> Option<Pane> {
    let state = state(app)?;
    let pane = state.slot.lock().ok()?.clone()?;
    (!pane.shared.closed.load(Ordering::SeqCst)).then_some(pane)
}

fn current_generation(app: &AppHandle, generation: u64) -> Option<Pane> {
    current(app).filter(|pane| pane.shared.generation == generation)
}

pub(crate) fn emit_state(app: &AppHandle, shared: &Shared) {
    let payload = state_payload(&shared.meta());
    let _ = app.emit("kz:preview-state", payload);
}

fn emit_closed(app: &AppHandle) {
    let payload = state_payload(&PaneMeta::default());
    let _ = app.emit("kz:preview-state", payload);
}

/// 没有面板时 kz:preview-state 的样子。
pub(crate) fn closed_payload() -> Value {
    state_payload(&PaneMeta::default())
}

/// 取现有面板,没有就建一个(串行化,两次并发打开只建一个)。
async fn ensure(app: &AppHandle, host: Rect) -> Result<Pane, String> {
    if let Some(pane) = current(app) {
        return Ok(pane);
    }
    let state = state(app).ok_or("预览状态未装配")?;
    let _creating = state.creating.lock().await;
    if let Some(pane) = current(app) {
        return Ok(pane);
    }
    let epoch = state.close_epoch.load(Ordering::SeqCst);
    let pane = create(app, host).await?;
    // 比对与写入在同一把 slot 锁里:close() 先递增代次再取 slot,两边怎么交错都不会漏关。
    let closed = {
        let mut slot = state
            .slot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let closed = closed_during_creation(epoch, state.close_epoch.load(Ordering::SeqCst));
        if !closed {
            *slot = Some(pane.clone());
        }
        closed
    };
    if closed {
        discard(app, &pane);
        return Err("预览面板在创建过程中被关闭了".into());
    }
    Ok(pane)
}

/// 创建期间有没有人关过面板(关过就不能把新面板写进 slot、更不能显示)。
pub(crate) fn closed_during_creation(epoch_before: u64, epoch_now: u64) -> bool {
    epoch_before != epoch_now
}

/// 丢弃一块没进 slot 的面板:释放 UI 线程上的 CDP 接收器与焦点登记,再关子 webview。
fn discard(app: &AppHandle, pane: &Pane) {
    pane.shared.closed.store(true, Ordering::SeqCst);
    release(app, pane.shared.generation);
    let _ = pane.webview.close();
}

/// 释放某代面板在 UI 线程上登记的东西(CDP 事件接收器、焦点归还用的 controller)。
fn release(app: &AppHandle, generation: u64) {
    let _ = app.run_on_main_thread(move || {
        cdp::release_on_ui_thread(generation);
        host::release_panel_on_ui_thread(generation);
    });
}

async fn create(app: &AppHandle, host: Rect) -> Result<Pane, String> {
    // B0:add_child 之后 get_webview_window("main") 就是 None;一律按 window / webview 分别取。
    let window = app.get_window(MAIN_LABEL).ok_or("主窗口不存在")?;
    let main = app.get_webview(MAIN_LABEL).ok_or("主 webview 不存在")?;
    for (label, stale) in app.webviews() {
        // 上一次创建失败留下的空壳(webviews() 里有它,但它从未真正建成)。
        if super::is_preview_label(&label) {
            let _ = stale.close();
        }
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let shared = Arc::new(Shared {
        generation,
        meta: Mutex::new(PaneMeta {
            host,
            ..PaneMeta::default()
        }),
        console: Mutex::new(ConsoleRing::default()),
        load_seq: AtomicU64::new(0),
        fail_seq: AtomicU64::new(0),
        closed: AtomicBool::new(false),
        zoom: Mutex::new(1.0),
    });
    let label = format!("{PREVIEW_LABEL}-{generation}");
    let builder = builder(app, &label, Arc::downgrade(&shared))?;
    #[cfg(windows)]
    let builder = builder.with_environment(cdp::main_environment(&main).await?.0);
    #[cfg(not(windows))]
    let _ = &main;
    let (x, y, w, h) = (host.x, host.y, host.w.max(1.0), host.h.max(1.0));
    let joined = tokio::time::timeout(
        Duration::from_secs(10),
        tauri::async_runtime::spawn_blocking(move || {
            window.add_child(builder, LogicalPosition::new(x, y), LogicalSize::new(w, h))
        }),
    )
    .await;
    let webview = match joined {
        Err(_) => return Err("创建预览面板超时(10 秒)".into()),
        Ok(Err(error)) => return Err(format!("创建预览面板的线程异常: {error}")),
        Ok(Ok(Err(error))) => return Err(format!("创建预览面板失败: {error}")),
        Ok(Ok(Ok(webview))) => webview,
    };
    let pane = Pane { webview, shared };
    // 初始化任何一步失败:UI 线程上已登记的接收器 / 处理器随代次一起释放,再关掉空壳。
    if let Err(error) = initialize(app, &pane).await {
        discard(app, &pane);
        return Err(error);
    }
    pane.shared.meta().alive = true;
    spawn_console_flusher(app.clone(), Arc::downgrade(&pane.shared));
    Ok(pane)
}

/// 建成之后、第一次导航之前:活性回环兼订阅 → 子 frame 导航闸 → *.enable → 记主 frame id →
/// 焦点登记(B0 的顺序:先订阅、再 enable、再导航)。
async fn initialize(app: &AppHandle, pane: &Pane) -> Result<(), String> {
    let generation = pane.shared.generation;
    // 活性回环兼订阅:闭包被丢弃 / 超时 = 子 webview 实际没建成。
    let handler: cdp::EventHandler = {
        let app = app.clone();
        let weak = Arc::downgrade(&pane.shared);
        Arc::new(move |name, params| handle_event(&app, &weak, name, params))
    };
    cdp::subscribe(&pane.webview, generation, SUBSCRIBED_EVENTS, handler)
        .await
        .map_err(|error| {
            format!("预览面板没有建成({error})。常见原因:WebView2 环境与主界面不一致。")
        })?;
    let on_blocked: cdp::BlockedHandler = {
        let weak = Arc::downgrade(&pane.shared);
        Arc::new(move |url| {
            if let Some(shared) = weak.upgrade() {
                shared
                    .console()
                    .push(blocked_entry(&url, "子框架"), now_ms());
            }
        })
    };
    cdp::guard_frame_navigation(&pane.webview, on_blocked)
        .await
        .map_err(|error| format!("预览面板初始化失败(子框架导航闸): {error}"))?;
    for method in [
        "Runtime.enable",
        "Log.enable",
        "Page.enable",
        "Network.enable",
    ] {
        cdp::call(&pane.webview, method, json!({}), cdp::CALL_TIMEOUT)
            .await
            .map_err(|error| format!("预览面板初始化失败({method}): {error}"))?;
    }
    // 同文档导航(pushState)只认主 frame:先记下它的 id,之后每次主 frame 提交时刷新。
    if let Ok(tree) = cdp::call(
        &pane.webview,
        "Page.getFrameTree",
        json!({}),
        cdp::CALL_TIMEOUT,
    )
    .await
    {
        if let Some(id) = tree["frameTree"]["frame"]["id"].as_str() {
            pane.shared.meta().nav.main_frame_id = Some(id.to_string());
        }
    }
    host::register_panel(&pane.webview, generation);
    Ok(())
}

/// 被导航闸拦下的地址记一条控制台警告;**不**改页面级错误态(当前页还好好的,
/// 比如页面里的 mailto: / blob: 链接、`*.localhost` 子域的开发地址)。
fn blocked_entry(url: &str, scope: &str) -> EntryDraft {
    EntryDraft {
        level: "warning".into(),
        text: format!(
            "已拦截{scope}导航:{url}(应用内部地址 *.localhost、file:、javascript: 等不在预览面板里打开)"
        ),
        url: url.to_string(),
        ..EntryDraft::default()
    }
}

fn builder(
    app: &AppHandle,
    label: &str,
    weak: Weak<Shared>,
) -> Result<WebviewBuilder<tauri::Wry>, String> {
    let blank = Url::parse("about:blank").map_err(|e| e.to_string())?;
    let nav_app = app.clone();
    let nav_weak = weak.clone();
    let load_app = app.clone();
    let load_weak = weak.clone();
    let title_app = app.clone();
    let title_weak = weak;
    let window_app = app.clone();
    Ok(WebviewBuilder::new(label, WebviewUrl::External(blank))
        .focused(false)
        .disable_drag_drop_handler()
        .on_navigation(move |url| {
            let allowed = nav_allowed(url);
            if let Some(shared) = nav_weak.upgrade() {
                if allowed {
                    // 顶层导航开始(含后退 / 前进 / 刷新):上一页的错误态在这里、也只在这里复位。
                    shared.meta().begin_navigation();
                    emit_state(&nav_app, &shared);
                } else {
                    shared
                        .console()
                        .push(blocked_entry(url.as_str(), ""), now_ms());
                }
            }
            allowed
        })
        .on_new_window(move |url, _features| {
            // window.open / target=_blank:不开新窗口,放行的地址在本面板里打开。
            if nav_allowed(&url) {
                if let Some(pane) = current(&window_app) {
                    tauri::async_runtime::spawn(async move {
                        let _ = pane.webview.navigate(url);
                    });
                }
            }
            NewWindowResponse::Deny
        })
        .on_page_load(move |_webview, payload| {
            let Some(shared) = load_weak.upgrade() else {
                return;
            };
            match payload.event() {
                PageLoadEvent::Started => {
                    shared.meta().content_loading(payload.url().as_str());
                }
                PageLoadEvent::Finished => {
                    shared.meta().loading = false;
                    spawn_history_refresh(load_app.clone(), shared.generation);
                }
            }
            emit_state(&load_app, &shared);
        })
        .on_document_title_changed(move |_webview, title| {
            if let Some(shared) = title_weak.upgrade() {
                shared.meta().title = title;
                emit_state(&title_app, &shared);
            }
        }))
}

/// CDP 事件回调(UI 线程):只做记账与发事件,耗时动作一律 spawn。
fn handle_event(app: &AppHandle, weak: &Weak<Shared>, name: &'static str, params: Value) {
    let Some(shared) = weak.upgrade() else {
        return;
    };
    let Some(signal) = parse_event(name, &params) else {
        return;
    };
    match signal {
        CdpSignal::Entry(draft) => {
            shared.console().push(draft, now_ms());
        }
        CdpSignal::InspectNode { backend_node_id } => {
            let app = app.clone();
            let generation = shared.generation;
            tauri::async_runtime::spawn(async move {
                if let Err(error) = finish_pick(&app, generation, backend_node_id).await {
                    tracing::warn!(%error, "网页批注取元素失败");
                }
            });
        }
        signal => {
            if signal == CdpSignal::LoadEventFired {
                shared.load_seq.fetch_add(1, Ordering::SeqCst);
            }
            let effect = shared.meta().apply(&signal);
            match effect {
                MetaEffect::None => {}
                MetaEffect::Changed => emit_state(app, &shared),
                MetaEffect::Committed => {
                    shared.console().on_main_frame_navigated(false);
                    emit_state(app, &shared);
                    spawn_reapply_emulation(app.clone(), shared.generation);
                    spawn_history_refresh(app.clone(), shared.generation);
                }
                MetaEffect::Failed(entry) => {
                    {
                        let mut console = shared.console();
                        console.on_main_frame_navigated(false);
                        console.push(entry, now_ms());
                    }
                    shared.fail_seq.fetch_add(1, Ordering::SeqCst);
                    emit_state(app, &shared);
                    spawn_history_refresh(app.clone(), shared.generation);
                }
                MetaEffect::SameDocument => {
                    emit_state(app, &shared);
                    spawn_history_refresh(app.clone(), shared.generation);
                }
            }
            if signal == CdpSignal::LoadEventFired {
                spawn_history_refresh(app.clone(), shared.generation);
            }
        }
    }
}

fn spawn_console_flusher(app: AppHandle, weak: Weak<Shared>) {
    tauri::async_runtime::spawn(async move {
        let mut last = 0u64;
        loop {
            tokio::time::sleep(CONSOLE_FLUSH).await;
            let Some(shared) = weak.upgrade() else {
                break;
            };
            if shared.closed.load(Ordering::SeqCst) {
                break;
            }
            let entries = shared.console().since(last);
            if let Some(tail) = entries.last() {
                last = tail.seq;
                let _ = app.emit(
                    "kz:preview-console",
                    super::console::console_payload(&entries),
                );
            }
        }
    });
}

fn spawn_history_refresh(app: AppHandle, generation: u64) {
    tauri::async_runtime::spawn(async move {
        let Some(pane) = current_generation(&app, generation) else {
            return;
        };
        if let Ok((back, forward)) = cdp::history(&pane.webview).await {
            let changed = {
                let mut meta = pane.shared.meta();
                let changed = meta.can_back != back || meta.can_forward != forward;
                meta.can_back = back;
                meta.can_forward = forward;
                changed
            };
            if changed {
                emit_state(&app, &pane.shared);
            }
        }
    });
}

fn spawn_reapply_emulation(app: AppHandle, generation: u64) {
    tauri::async_runtime::spawn(async move {
        let Some(pane) = current_generation(&app, generation) else {
            return;
        };
        let (device, scheme) = {
            let meta = pane.shared.meta();
            (meta.device, meta.scheme)
        };
        if device.touch() || scheme != ColorScheme::Auto {
            let _ = apply_emulation(&pane, device, scheme).await;
        }
    });
}

async fn apply_emulation(
    pane: &Pane,
    device: DevicePreset,
    scheme: ColorScheme,
) -> Result<(), String> {
    cdp::call(
        &pane.webview,
        "Emulation.setTouchEmulationEnabled",
        json!({ "enabled": device.touch(), "maxTouchPoints": 5 }),
        cdp::CALL_TIMEOUT,
    )
    .await?;
    let features = match scheme {
        ColorScheme::Auto => json!([]),
        ColorScheme::Light => json!([{ "name": "prefers-color-scheme", "value": "light" }]),
        ColorScheme::Dark => json!([{ "name": "prefers-color-scheme", "value": "dark" }]),
    };
    cdp::call(
        &pane.webview,
        "Emulation.setEmulatedMedia",
        json!({ "features": features }),
        cdp::CALL_TIMEOUT,
    )
    .await?;
    Ok(())
}

/// 按 host 与设备重放边界与缩放。
fn apply_bounds(pane: &Pane) {
    let (host, device) = {
        let meta = pane.shared.meta();
        (meta.host, meta.device)
    };
    let (rect, zoom) = fit_device(host, device);
    let _ = pane.webview.set_bounds(tauri::Rect {
        position: LogicalPosition::new(rect.x, rect.y).into(),
        size: LogicalSize::new(rect.w.max(1.0), rect.h.max(1.0)).into(),
    });
    let mut current_zoom = pane
        .shared
        .zoom
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if (*current_zoom - zoom).abs() > f64::EPSILON && pane.webview.set_zoom(zoom).is_ok() {
        *current_zoom = zoom;
    }
}

/// DPI 变化时重放边界(WindowEvent::ScaleFactorChanged)。
pub(crate) fn reapply_bounds(app: &AppHandle) {
    if let Some(pane) = current(app) {
        apply_bounds(&pane);
    }
}

/// 导航(过闸)。
pub(crate) fn navigate(pane: &Pane, url: &str) -> Result<(), String> {
    let parsed = Url::parse(url).map_err(|e| format!("地址无法解析: {e}"))?;
    if !nav_allowed(&parsed) {
        return Err(format!("拒绝在面板里打开 {url}"));
    }
    pane.webview
        .navigate(parsed)
        .map_err(|e| format!("导航失败: {e}"))
}

/// preview_open:建(或复用)面板、定位、显示、绑定线、导航。
pub(crate) async fn open(
    app: &AppHandle,
    url: &str,
    process_id: Option<String>,
    host: Rect,
) -> Result<Value, String> {
    let pane = ensure(app, host).await?;
    {
        let mut meta = pane.shared.meta();
        meta.host = host;
        // 打开即显示:绑定按这次上报的线覆盖(null = 不绑任何线)。
        meta.set_visibility(true, process_id, now_ms());
        meta.begin_navigation();
        meta.url = url.to_string();
    }
    apply_bounds(&pane);
    let _ = pane.webview.show();
    navigate(&pane, url)?;
    emit_state(app, &pane.shared);
    let meta = pane.shared.meta().clone();
    Ok(state_payload(&meta))
}

pub(crate) fn set_bounds(app: &AppHandle, host: Rect) {
    if let Some(pane) = current(app) {
        pane.shared.meta().host = host;
        apply_bounds(&pane);
    }
}

pub(crate) fn set_visible(app: &AppHandle, visible: bool, process_id: Option<String>) -> Value {
    let Some(pane) = current(app) else {
        return closed_payload();
    };
    pane.shared
        .meta()
        .set_visibility(visible, process_id, now_ms());
    if visible {
        apply_bounds(&pane);
        let _ = pane.webview.show();
    } else {
        let _ = pane.webview.hide();
    }
    emit_state(app, &pane.shared);
    let meta = pane.shared.meta().clone();
    state_payload(&meta)
}

pub(crate) async fn nav(app: &AppHandle, action: &str) -> Result<(), String> {
    let pane = current(app).ok_or("预览面板没有打开")?;
    match action {
        "reload" => pane
            .webview
            .reload()
            .map_err(|e| format!("刷新失败: {e}"))?,
        "back" => cdp::go(&pane.webview, "back").await?,
        "forward" => cdp::go(&pane.webview, "forward").await?,
        "stop" => cdp::go(&pane.webview, "stop").await?,
        other => return Err(format!("未知导航动作 {other}(back|forward|reload|stop)")),
    }
    spawn_history_refresh(app.clone(), pane.shared.generation);
    Ok(())
}

/// 关闭面板:释放 CDP 订阅与焦点登记、关子 webview、清路由状态。幂等。
/// 面板还在创建中(slot 还是空的)时同样生效:递增关闭代次,ensure 建好后比对到就当场丢弃。
pub(crate) fn close(app: &AppHandle) {
    let Some(state) = state(app) else {
        return;
    };
    state.close_epoch.fetch_add(1, Ordering::SeqCst);
    let pane = state
        .slot
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .take();
    let Some(pane) = pane else {
        return;
    };
    discard(app, &pane);
    emit_closed(app);
}

/// PNG 宽高(IHDR)。
pub(crate) fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

/// 等面板露出来(前端遮挡冻结通常几百毫秒);超时返回 false。
pub(crate) async fn wait_visible(pane: &Pane, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if pane.shared.meta().visible {
            return true;
        }
        if pane.shared.closed.load(Ordering::SeqCst) || tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// ZoomFactor 不是 1(设备模式把画面缩进了面板)。
pub(crate) fn is_zoomed(zoom: f64) -> bool {
    (zoom - 1.0).abs() > 1e-6
}

/// captureScreenshot 的 clip:CDP 的单位是 DIP,而元素矩形是 CSS px;
/// ZoomFactor = z 时 1 CSS px = z DIP,所以四个量都乘 z(设备模式下不换算会裁偏)。
pub(crate) fn clip_params(clip: Rect, zoom: f64) -> Value {
    json!({
        "x": clip.x.max(0.0) * zoom,
        "y": clip.y.max(0.0) * zoom,
        "width": clip.w.max(1.0) * zoom,
        "height": clip.h.max(1.0) * zoom,
        "scale": 1
    })
}

/// 截图(可见时才截;刚被遮住时最多等 [`VISIBLE_WAIT`])。返回 `(base64 PNG, 宽, 高)`。
/// `clip` 是文档坐标的 CSS px;设备模式(ZoomFactor ≠ 1)下不做整页截图。
pub(crate) async fn capture(
    pane: &Pane,
    full_page: bool,
    clip: Option<Rect>,
) -> Result<(String, u32, u32), String> {
    if !wait_visible(pane, VISIBLE_WAIT).await {
        return Err(
            "预览面板当前是隐藏的(被菜单 / 弹窗遮住,或已收起),无法截图(隐藏时截图不会返回)".into(),
        );
    }
    let zoom = pane.shared.zoom();
    let mut params = json!({ "format": "png" });
    if full_page && is_zoomed(zoom) {
        return Err(format!(
            "设备模式(缩放 {:.0}%)下不支持整页截图:缩放下的整页几何没有验证过;请切回「自适应」再截,或只截可视区",
            zoom * 100.0
        ));
    }
    if full_page {
        let metrics = cdp::call(
            &pane.webview,
            "Page.getLayoutMetrics",
            json!({}),
            cdp::CALL_TIMEOUT,
        )
        .await?;
        let size = &metrics["cssContentSize"];
        let width = size["width"].as_f64().unwrap_or(0.0).max(1.0);
        let height = size["height"]
            .as_f64()
            .unwrap_or(0.0)
            .clamp(1.0, FULL_PAGE_MAX_HEIGHT);
        params["captureBeyondViewport"] = json!(true);
        params["clip"] = json!({ "x": 0, "y": 0, "width": width, "height": height, "scale": 1 });
    } else if let Some(clip) = clip {
        params["clip"] = clip_params(clip, zoom);
    }
    let result = cdp::call(
        &pane.webview,
        "Page.captureScreenshot",
        params,
        CAPTURE_TIMEOUT,
    )
    .await?;
    let data = result["data"]
        .as_str()
        .ok_or("截图响应缺 data 字段")?
        .to_string();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("截图不是合法 base64: {e}"))?;
    let (width, height) = png_size(&bytes).ok_or("截图不是 PNG")?;
    Ok((data, width, height))
}

/// `preview_capture` 的载荷。
pub(crate) fn capture_payload(png: String, width: u32, height: u32) -> Value {
    json!({ "png": png, "width": width, "height": height })
}

pub(crate) async fn set_device(
    app: &AppHandle,
    preset: Option<DevicePreset>,
    scheme: Option<ColorScheme>,
) -> Result<Value, String> {
    let pane = current(app).ok_or("预览面板没有打开")?;
    let (device, scheme) = {
        let mut meta = pane.shared.meta();
        if let Some(preset) = preset {
            meta.device = preset;
        }
        if let Some(scheme) = scheme {
            meta.scheme = scheme;
        }
        (meta.device, meta.scheme)
    };
    apply_bounds(&pane);
    apply_emulation(&pane, device, scheme).await?;
    emit_state(app, &pane.shared);
    let meta = pane.shared.meta().clone();
    Ok(state_payload(&meta))
}

/// Overlay 高亮配色(DevTools 元素选择器的惯用配色;画在页面里,不属于 kanzei 界面配色)。
fn highlight_config() -> Value {
    json!({
        "showInfo": true,
        "contentColor": { "r": 111, "g": 168, "b": 220, "a": 0.55 },
        "paddingColor": { "r": 147, "g": 196, "b": 125, "a": 0.45 },
        "borderColor": { "r": 255, "g": 229, "b": 153, "a": 0.8 },
        "marginColor": { "r": 246, "g": 178, "b": 107, "a": 0.45 }
    })
}

pub(crate) async fn set_pick(app: &AppHandle, on: bool) -> Result<(), String> {
    let pane = current(app).ok_or("预览面板没有打开")?;
    if on {
        cdp::call(&pane.webview, "DOM.enable", json!({}), cdp::CALL_TIMEOUT).await?;
        cdp::call(
            &pane.webview,
            "Overlay.enable",
            json!({}),
            cdp::CALL_TIMEOUT,
        )
        .await?;
        cdp::call(
            &pane.webview,
            "Overlay.setInspectMode",
            json!({ "mode": "searchForNode", "highlightConfig": highlight_config() }),
            cdp::CALL_TIMEOUT,
        )
        .await?;
    } else {
        cdp::call(
            &pane.webview,
            "Overlay.setInspectMode",
            json!({ "mode": "none", "highlightConfig": {} }),
            cdp::CALL_TIMEOUT,
        )
        .await?;
    }
    pane.shared.meta().pick = on;
    Ok(())
}

/// 被点中元素的选择器 / 文字 / 位置(Runtime.callFunctionOn,this = 元素)。
const PICK_FUNCTION: &str = r##"function () {
  const el = this;
  const esc = (s) => (window.CSS && CSS.escape ? CSS.escape(s) : String(s).replace(/[^a-zA-Z0-9_-]/g, (c) => "\\" + c));
  const parts = [];
  let node = el;
  while (node && node.nodeType === 1 && parts.length < 6) {
    if (node.id) { parts.unshift("#" + esc(node.id)); break; }
    let part = node.tagName.toLowerCase();
    const parent = node.parentElement;
    if (parent) {
      const same = Array.from(parent.children).filter((c) => c.tagName === node.tagName);
      if (same.length > 1) part += ":nth-of-type(" + (same.indexOf(node) + 1) + ")";
    }
    parts.unshift(part);
    if (node.tagName === "BODY" || node.tagName === "HTML") break;
    node = parent;
  }
  const r = el.getBoundingClientRect();
  const text = (el.innerText || el.textContent || "").replace(/\s+/g, " ").trim().slice(0, 200);
  return { selector: parts.join(" > "), text, tag: el.tagName.toLowerCase(),
           rect: { x: r.x, y: r.y, w: r.width, h: r.height }, scrollX: window.scrollX, scrollY: window.scrollY };
}"##;

/// `kz:preview-pick` 的载荷。
pub(crate) fn pick_payload(info: &Value, png: String) -> Value {
    json!({
        "selector": info["selector"].as_str().unwrap_or(""),
        "text": info["text"].as_str().unwrap_or(""),
        "tag": info["tag"].as_str().unwrap_or(""),
        "rect": {
            "x": info["rect"]["x"].as_f64().unwrap_or(0.0),
            "y": info["rect"]["y"].as_f64().unwrap_or(0.0),
            "w": info["rect"]["w"].as_f64().unwrap_or(0.0),
            "h": info["rect"]["h"].as_f64().unwrap_or(0.0),
        },
        "png": png,
    })
}

async fn finish_pick(app: &AppHandle, generation: u64, backend_node_id: i64) -> Result<(), String> {
    let pane = current_generation(app, generation).ok_or("面板已关闭")?;
    let resolved = cdp::call(
        &pane.webview,
        "DOM.resolveNode",
        json!({ "backendNodeId": backend_node_id }),
        cdp::CALL_TIMEOUT,
    )
    .await?;
    let object_id = resolved["object"]["objectId"]
        .as_str()
        .ok_or("选中的节点没有 objectId")?
        .to_string();
    let described = cdp::call(
        &pane.webview,
        "Runtime.callFunctionOn",
        json!({ "functionDeclaration": PICK_FUNCTION, "objectId": object_id, "returnByValue": true }),
        cdp::CALL_TIMEOUT,
    )
    .await?;
    let info = described["result"]["value"].clone();
    // 选中一次就退出选择模式(与 DevTools 的元素选择器一致)。
    let _ = set_pick(app, false).await;
    let margin = 8.0;
    let clip = Rect {
        x: info["rect"]["x"].as_f64().unwrap_or(0.0) + info["scrollX"].as_f64().unwrap_or(0.0)
            - margin,
        y: info["rect"]["y"].as_f64().unwrap_or(0.0) + info["scrollY"].as_f64().unwrap_or(0.0)
            - margin,
        w: info["rect"]["w"].as_f64().unwrap_or(0.0) + margin * 2.0,
        h: info["rect"]["h"].as_f64().unwrap_or(0.0) + margin * 2.0,
    };
    let png = match capture(&pane, false, Some(clip)).await {
        Ok((png, _, _)) => png,
        Err(error) => {
            tracing::warn!(%error, "网页批注裁图失败,只回传元素信息");
            String::new()
        }
    };
    let _ = app.emit("kz:preview-pick", pick_payload(&info, png));
    // 点选完成时系统焦点还在子 webview 里(用户刚在页面上点了一下),前端 addPickAttachment 里的
    // promptBox.focus() 接不到键盘输入:把焦点还给主界面,补一句修改意见就能直接打字。
    return_focus_to_main(app);
    Ok(())
}

/// 把键盘焦点还给主界面:`get_webview("main")` 的 `set_focus`(wry 里是 MoveFocus(PROGRAMMATIC),
/// 会恢复主文档里原来聚焦的元素)。**只在 kanzei 就是前台程序时**做——B0 实测 MoveFocus 会把
/// 窗口拉到前台,用户已经切去别的程序时不能把 kanzei 拽回来;判定与转交都在 UI 线程上连着做,
/// 中间不给前台切换留空当。不用 get_webview_window:第一次 add_child 之后它是 None(§8)。
pub(crate) fn return_focus_to_main(app: &AppHandle) {
    let Some(main) = app.get_webview(MAIN_LABEL) else {
        return;
    };
    let _ = app.run_on_main_thread(move || {
        if host::main_is_foreground() {
            let _ = main.set_focus();
        }
    });
}

/// 「清除本站数据」:只清当前源。**禁止**用清全部浏览数据的那个 API——面板与 kanzei 主界面共用
/// 同一个 WebView2 profile,那会连 kanzei 自己的 localStorage 偏好一起清掉(见 pure_tests 的守卫)。
pub(crate) async fn clear_site_data(app: &AppHandle) -> Result<String, String> {
    let pane = current(app).ok_or("预览面板没有打开")?;
    let url = pane.shared.meta().url.clone();
    let parsed = Url::parse(&url).map_err(|_| format!("当前地址不是网页: {url}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("当前地址不是 http(s) 网页: {url}"));
    }
    let origin = parsed.origin().ascii_serialization();
    cdp::call(
        &pane.webview,
        "Storage.clearDataForOrigin",
        json!({ "origin": origin, "storageTypes": "all" }),
        cdp::CALL_TIMEOUT,
    )
    .await?;
    Ok(origin)
}

pub(crate) async fn open_devtools(app: &AppHandle) -> Result<(), String> {
    let pane = current(app).ok_or("预览面板没有打开")?;
    let typed = cdp::PaneWebview::new(&pane.webview).ok_or("DevTools 只能对预览面板打开")?;
    cdp::open_devtools(&typed).await
}

/// 等主文档加载完(load 事件)或失败;超时不算错(同文档跳转没有 load 事件),返回 false。
pub(crate) async fn wait_load(
    pane: &Pane,
    load_before: u64,
    fail_before: u64,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if pane.shared.load_seq.load(Ordering::SeqCst) > load_before
            || pane.shared.fail_seq.load(Ordering::SeqCst) > fail_before
            || pane.shared.closed.load(Ordering::SeqCst)
        {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png尺寸取自ihdr() {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        bytes.extend(900u32.to_be_bytes());
        bytes.extend(630u32.to_be_bytes());
        assert_eq!(png_size(&bytes), Some((900, 630)));
        assert_eq!(png_size(b"not a png at all, definitely"), None);
    }

    #[test]
    fn 批注载荷字段齐全() {
        let payload = pick_payload(
            &json!({"selector": "#go", "text": "Go", "tag": "button",
                    "rect": {"x": 1.0, "y": 2.0, "w": 3.0, "h": 4.0}}),
            "iVBOR".into(),
        );
        for key in ["selector", "text", "tag", "rect", "png"] {
            assert!(payload.get(key).is_some(), "缺 {key}");
        }
        assert_eq!(payload["rect"]["h"], 4.0);
    }

    /// 复核修复:captureScreenshot 的 clip 单位是 DIP,设备模式(ZoomFactor < 1)下要按缩放换算,
    /// 否则元素截图与批注裁图会裁偏。
    #[test]
    fn 截图clip按缩放换成dip_缩放为1时原样() {
        let clip = Rect {
            x: 100.0,
            y: 40.0,
            w: 200.0,
            h: 50.0,
        };
        let same = clip_params(clip, 1.0);
        assert_eq!(
            (same["x"].as_f64(), same["width"].as_f64()),
            (Some(100.0), Some(200.0))
        );
        let zoomed = clip_params(clip, 0.5);
        assert_eq!(zoomed["x"].as_f64(), Some(50.0));
        assert_eq!(zoomed["y"].as_f64(), Some(20.0));
        assert_eq!(zoomed["width"].as_f64(), Some(100.0));
        assert_eq!(zoomed["height"].as_f64(), Some(25.0));
        assert_eq!(zoomed["scale"], 1);
        let negative = clip_params(
            Rect {
                x: -8.0,
                y: -8.0,
                w: 0.0,
                h: 0.0,
            },
            0.5,
        );
        assert_eq!(negative["x"].as_f64(), Some(0.0), "负坐标夹到 0");
        assert_eq!(negative["width"].as_f64(), Some(0.5), "至少 1 CSS px");
        assert!(is_zoomed(0.5) && !is_zoomed(1.0));
    }

    /// 复核修复:创建期间用户点了关闭(slot 还是空的),建好的面板不能再写进 slot。
    #[test]
    fn 创建期间被关闭时丢弃新面板() {
        assert!(!closed_during_creation(3, 3));
        assert!(closed_during_creation(3, 4));
    }
}
