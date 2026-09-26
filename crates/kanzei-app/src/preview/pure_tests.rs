//! 预览面板纯函数与源码守卫的单测(UI2-0926 #8)。

use std::path::{Path, PathBuf};

use super::*;

fn fixture(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kz-preview-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("site")).unwrap();
    std::fs::create_dir_all(root.join("中文 目录")).unwrap();
    std::fs::write(root.join("site/index.html"), "<p>x</p>").unwrap();
    std::fs::write(root.join("中文 目录/页 面.html"), "<p>中</p>").unwrap();
    root
}

fn url_of(target: Result<Target, String>) -> String {
    match target {
        Ok(Target::Url(url)) => url,
        other => panic!("期望 URL,实得 {other:?}"),
    }
}

#[test]
fn normalize_target_端口与本机地址补全() {
    let roots: Vec<PathBuf> = Vec::new();
    for (input, expected) in [
        ("5173", "http://localhost:5173/"),
        (" 5173 ", "http://localhost:5173/"),
        (":3000/x", "http://localhost:3000/x"),
        (":3000", "http://localhost:3000/"),
        ("localhost:8080", "http://localhost:8080/"),
        ("localhost", "http://localhost/"),
        ("127.0.0.1:4173", "http://127.0.0.1:4173/"),
        ("[::1]:5000/a", "http://[::1]:5000/a"),
        ("https://example.com/a?b=1", "https://example.com/a?b=1"),
        ("http://localhost:5173/app", "http://localhost:5173/app"),
        ("example.com/docs", "https://example.com/docs"),
        ("about:blank", "about:blank"),
    ] {
        assert_eq!(url_of(normalize_target(input, &roots)), expected, "{input}");
    }
    for bad in [
        "",
        "0",
        "70000",
        ":abc",
        "ftp://example.com/",
        "javascript:alert(1)",
        "http://tauri.localhost/",
        "http://ipc.localhost/cmd",
        "tauri://localhost",
        "hello world",
    ] {
        assert!(normalize_target(bad, &roots).is_err(), "{bad:?} 必须被拒");
    }
}

#[test]
fn normalize_target_本地路径含空格与中文() {
    let root = fixture("paths");
    let roots = vec![root.clone()];
    assert_eq!(
        normalize_target("site/index.html", &roots).unwrap(),
        Target::Local(root.join("site/index.html"))
    );
    assert_eq!(
        normalize_target("/site/index.html", &roots).unwrap(),
        Target::Local(root.join("site/index.html")),
        "以 / 开头视为项目内路径"
    );
    let cjk = root.join("中文 目录").join("页 面.html");
    assert_eq!(
        normalize_target(&cjk.display().to_string(), &[]).unwrap(),
        Target::Local(cjk.clone()),
        "绝对路径不需要项目上下文"
    );
    let file_url = format!("file:///{}", cjk.display().to_string().replace('\\', "/"));
    assert_eq!(
        normalize_target(&file_url, &[]).unwrap(),
        Target::Local(cjk.clone())
    );
    assert!(normalize_target("site/missing.html", &roots)
        .unwrap_err()
        .contains("找不到"));
    assert!(normalize_target("./nope.html", &[])
        .unwrap_err()
        .contains("项目上下文"));
    // 换成静态服务 URL:中文与空格被百分号编码。
    let url = resolve_target(&cjk.display().to_string(), &roots).unwrap();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert!(
        url.contains("%E4%B8%AD%E6%96%87%20%E7%9B%AE%E5%BD%95"),
        "{url}"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn nav_allowed_拒绝应用内部源与危险scheme() {
    let check = |raw: &str| nav_allowed(&Url::parse(raw).unwrap());
    for blocked in [
        "http://tauri.localhost/index.html",
        "https://tauri.localhost/",
        "http://ipc.localhost/spike_secret",
        "http://asset.localhost/x",
        "http://kzproto.localhost/",
        "http://TAURI.LOCALHOST/",
        "http://tauri.localhost./",
        "file:///C:/Windows/win.ini",
        "javascript:alert(1)",
        "about:srcdoc",
        "data:image/svg+xml,<svg/>",
        "chrome://settings",
        "ftp://example.com/",
    ] {
        assert!(!check(blocked), "{blocked} 必须被拒");
    }
    for allowed in [
        "http://localhost:5173/",
        "http://127.0.0.1:4173/t/0123456789abcdef/r/abc/index.html",
        "http://[::1]:8080/",
        "https://example.com/",
        "about:blank",
        "data:text/html,<p>x</p>",
    ] {
        assert!(check(allowed), "{allowed} 应放行");
    }
}

/// 复核修复:子 frame 同样要过 *.localhost 规则(wry 的应用协议对子 frame 请求同样生效,
/// tauri 的 is_local_url 又不看端口);页面内部常用的 about: / data: / blob: 不误伤。
#[test]
fn frame_nav_allowed_子框架同样拒绝应用内部源() {
    let check = |raw: &str| frame_nav_allowed(&Url::parse(raw).unwrap());
    for blocked in [
        "http://tauri.localhost/index.html",
        "http://tauri.localhost:3000/",
        "https://ipc.localhost/cmd",
        "http://asset.localhost/x",
        "http://TAURI.LOCALHOST./",
        "blob:http://tauri.localhost/0b1c",
        "file:///C:/Windows/win.ini",
        "chrome://settings",
        "tauri://localhost/",
        "ftp://example.com/",
    ] {
        assert!(!check(blocked), "{blocked} 必须被拒");
    }
    for allowed in [
        "https://www.youtube.com/embed/x",
        "http://localhost:5173/frame.html",
        "http://127.0.0.1:4173/t/abc/r/def/a.html",
        "about:blank",
        "about:srcdoc",
        "data:text/html,<p>x</p>",
        "data:image/svg+xml,<svg/>",
        "blob:http://localhost:5173/0b1c",
        "javascript:void(0)",
    ] {
        assert!(check(allowed), "{allowed} 应放行");
    }
    // 顶层闸比子 frame 严:about:srcdoc 只该出现在 iframe 里。
    assert!(!nav_allowed(&Url::parse("about:srcdoc").unwrap()));
}

fn meta(visible: bool, bound: Option<&str>, alive: bool) -> PaneMeta {
    PaneMeta {
        visible,
        alive,
        bound_process_id: bound.map(str::to_string),
        ..PaneMeta::default()
    }
}

/// B0:隐藏面板截图永不返回——路由绝不能选隐藏的面板。
#[test]
fn route_只有可见且绑定本线的面板才走面板() {
    let line = Some("p|a");
    assert_eq!(
        route(Some(&meta(true, Some("p|a"), true)), line),
        Backend::Pane
    );
    assert_eq!(
        route(Some(&meta(false, Some("p|a"), true)), line),
        Backend::Headless,
        "隐藏面板"
    );
    assert_eq!(
        route(Some(&meta(true, Some("p|b"), true)), line),
        Backend::Headless,
        "别的线"
    );
    assert_eq!(
        route(Some(&meta(true, None, true)), line),
        Backend::Headless,
        "未绑定"
    );
    assert_eq!(
        route(Some(&meta(true, Some("p|a"), false)), line),
        Backend::Headless,
        "没活过来的面板"
    );
    assert_eq!(route(None, line), Backend::Headless, "没有面板");
    assert_eq!(
        route(Some(&meta(true, Some("p|a"), true)), None),
        Backend::Headless,
        "调用方没有线身份"
    );
    // 应用未装配(单测进程)时恒为无头。
    assert_eq!(route_hint_for(Some("p|a")), Backend::Headless);
}

/// 复核修复:显示时按上报的线覆盖绑定(null 即解绑),隐藏时保留绑定并记下隐藏时刻。
#[test]
fn 可见性上报_显示时覆盖绑定_null即解绑_隐藏时保留() {
    let mut meta = meta(false, Some("p|a"), true);
    meta.set_visibility(true, Some("p|a".into()), 1_000);
    assert_eq!(route(Some(&meta), Some("p|a")), Backend::Pane);

    // 用户切到一个没有线的上下文:面板仍显示,但不再绑在上一条线上。
    meta.set_visibility(true, None, 2_000);
    assert_eq!(meta.bound_process_id, None);
    assert_eq!(
        route(Some(&meta), Some("p|a")),
        Backend::Headless,
        "旧线的代理不能驱动用户此刻在别的上下文里看到的面板"
    );

    meta.set_visibility(true, Some("p|b".into()), 3_000);
    meta.set_visibility(false, None, 4_000);
    assert_eq!(meta.bound_process_id.as_deref(), Some("p|b"), "隐藏不解绑");
    assert_eq!(meta.hidden_at, 4_000);
    meta.set_visibility(false, None, 9_000);
    assert_eq!(meta.hidden_at, 4_000, "已经隐藏的再报隐藏,不刷新隐藏时刻");
}

/// 复核修复:前端的遮挡冻结也是 set_visible(false)。刚被遮住的面板值得等一等(权限预估也按面板算),
/// 早就收起的、别的线的、错误页收起的不等。
#[test]
fn 刚被遮住的面板值得等_早就收起的不等() {
    let line = Some("p|a");
    let mut meta = meta(true, Some("p|a"), true);
    meta.set_visibility(false, None, 100_000);
    assert!(should_wait_visible(&meta, line, 100_500));
    assert_eq!(
        route(Some(&meta), line),
        Backend::Headless,
        "路由本身仍只认可见"
    );
    assert_eq!(route_hint(Some(&meta), line, 100_500), Backend::Pane);
    assert!(
        !should_wait_visible(&meta, line, 100_000 + OCCLUSION_GRACE_MS + 1),
        "隐藏超过宽限:用户收起了面板,不白等"
    );
    assert_eq!(
        route_hint(Some(&meta), line, 100_000 + OCCLUSION_GRACE_MS + 1),
        Backend::Headless
    );
    assert!(!should_wait_visible(&meta, Some("p|b"), 100_500), "别的线");
    assert!(!should_wait_visible(&meta, None, 100_500), "没有线身份");
    let mut errored = meta.clone();
    errored.error = Some(describe_error("net::ERR_CONNECTION_REFUSED", "u"));
    assert!(
        !should_wait_visible(&errored, line, 100_500),
        "错误页收起的等不来"
    );
    let mut dead = meta.clone();
    dead.alive = false;
    assert!(!should_wait_visible(&dead, line, 100_500));
    let visible = self::meta(true, Some("p|a"), true);
    assert!(
        !should_wait_visible(&visible, line, 100_500),
        "可见的不用等"
    );
    assert_eq!(route_hint(Some(&visible), line, 0), Backend::Pane);
    assert_eq!(route_hint(None, line, 0), Backend::Headless);
}

fn feed(meta: &mut PaneMeta, events: &[(&str, serde_json::Value)]) -> Vec<MetaEffect> {
    events
        .iter()
        .filter_map(|(name, params)| console::parse_event(name, params))
        .map(|signal| meta.apply(&signal))
        .collect()
}

fn main_commit(loader: &str, url: &str) -> (&'static str, serde_json::Value) {
    (
        "Page.frameNavigated",
        serde_json::json!({"frame": {"id": "MAIN", "loaderId": loader, "url": url}, "type": "Navigation"}),
    )
}

fn failed(request: &str, error: &str) -> (&'static str, serde_json::Value) {
    (
        "Network.loadingFailed",
        serde_json::json!({"requestId": request, "type": "Document", "errorText": error, "canceled": false}),
    )
}

fn error_page(
    id: &str,
    parent: Option<&str>,
    loader: &str,
    unreachable: &str,
) -> (&'static str, serde_json::Value) {
    (
        "Page.frameNavigated",
        serde_json::json!({"frame": {"id": id, "parentId": parent, "loaderId": loader,
            "url": "chrome-error://chromewebdata/", "unreachableUrl": unreachable}, "type": "Navigation"}),
    )
}

/// 复核修复(major):主帧正常、iframe 指向死端口 / 带 X-Frame-Options 时,CDP 同样发
/// loadingFailed(Document)。样例是 Edge 实测的事件序列(scratchpad probe):不能进入错误态,
/// 也不能让代理 open 的 wait_load 提前以失败结束。
#[test]
fn iframe加载失败不进入页面错误态() {
    let mut meta = meta(true, Some("p|a"), true);
    meta.begin_navigation();
    let effects = feed(
        &mut meta,
        &[
            main_commit("L0", "http://127.0.0.1:4296/"),
            failed("R1", "net::ERR_UNSAFE_PORT"),
            failed("R2", "net::ERR_BLOCKED_BY_RESPONSE"),
            failed("R3", "net::ERR_BLOCKED_BY_CLIENT"),
            error_page("F1", Some("MAIN"), "R1", "http://127.0.0.1:1/dead"),
            error_page("F2", Some("MAIN"), "R2", "http://127.0.0.1:4296/xfo"),
            ("Page.loadEventFired", serde_json::json!({"timestamp": 1.0})),
        ],
    );
    assert_eq!(meta.error, None, "iframe 的失败不是页面失败");
    assert_eq!(meta.url, "http://127.0.0.1:4296/");
    assert!(!meta.loading);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, MetaEffect::Failed(_))),
        "{effects:?}"
    );
    assert_eq!(effects[0], MetaEffect::Committed);
}

/// 主帧死地址:loadingFailed(Document) → ContentLoading(错误页也触发)→ 主帧错误页提交。
/// 错误码按 loaderId 取回,ContentLoading 不能把错误抹掉(两种次序都要成立)。
#[test]
fn 主帧死地址报服务没在跑_contentloading不清错误() {
    let dead = "http://127.0.0.1:5173/";
    // 次序一:失败 → ContentLoading → 错误页提交。
    let mut meta = meta(true, Some("p|a"), true);
    meta.begin_navigation();
    feed(&mut meta, &[failed("RQ", "net::ERR_CONNECTION_REFUSED")]);
    assert_eq!(meta.error, None, "只暂存,不直接进错误态");
    meta.content_loading(dead);
    let effects = feed(&mut meta, &[error_page("MAIN", None, "RQ", dead)]);
    let error = meta.error.clone().expect("主帧错误页必须进入错误态");
    assert_eq!(error.kind, ErrorKind::ConnectionRefused, "{error:?}");
    assert!(error.text.contains("服务没在跑"));
    assert_eq!(meta.url, dead);
    match &effects[..] {
        [MetaEffect::Failed(entry)] => {
            assert_eq!(entry.level, "network");
            assert!(entry.text.contains("ERR_CONNECTION_REFUSED"), "{entry:?}");
        }
        other => panic!("期望 Failed,实得 {other:?}"),
    }

    // 次序二:失败 → 错误页提交 → ContentLoading 最后才到。
    let mut meta = self::meta(true, Some("p|a"), true);
    meta.begin_navigation();
    feed(
        &mut meta,
        &[
            failed("RQ", "net::ERR_CONNECTION_REFUSED"),
            error_page("MAIN", None, "RQ", dead),
        ],
    );
    meta.content_loading(dead);
    assert_eq!(
        meta.error.as_ref().map(|e| e.kind),
        Some(ErrorKind::ConnectionRefused),
        "ContentLoading 不碰错误态"
    );

    // 罕见次序:错误页先到(先按 ERR_FAILED 记),随后的失败按 loaderId 补上具体错误码。
    let mut meta = self::meta(true, Some("p|a"), true);
    feed(&mut meta, &[error_page("MAIN", None, "RQ", dead)]);
    assert_eq!(meta.error.as_ref().map(|e| e.kind), Some(ErrorKind::Other));
    let effects = feed(&mut meta, &[failed("RQ", "net::ERR_CONNECTION_REFUSED")]);
    assert_eq!(effects, vec![MetaEffect::Changed]);
    assert_eq!(
        meta.error.as_ref().map(|e| e.kind),
        Some(ErrorKind::ConnectionRefused)
    );
    // 别的请求(比如随后 iframe 的失败)不会改写它。
    feed(&mut meta, &[failed("OTHER", "net::ERR_BLOCKED_BY_CLIENT")]);
    assert_eq!(
        meta.error.as_ref().map(|e| e.kind),
        Some(ErrorKind::ConnectionRefused)
    );

    // 错误态只在下一次导航开始时复位;成功提交同样清掉。
    meta.begin_navigation();
    assert_eq!(meta.error, None);
    assert!(meta.loading);
    feed(&mut meta, &[error_page("MAIN", None, "R9", dead)]);
    assert!(meta.error.is_some());
    feed(&mut meta, &[main_commit("R10", "http://127.0.0.1:5173/ok")]);
    assert_eq!(meta.error, None);
}

#[test]
fn 失败暂存有上限() {
    let mut meta = meta(true, None, true);
    for index in 0..40 {
        feed(
            &mut meta,
            &[failed(&format!("R{index}"), "net::ERR_BLOCKED_BY_CLIENT")],
        );
    }
    // 早被挤出去的那条取不回具体码,退回 ERR_FAILED。
    feed(&mut meta, &[error_page("MAIN", None, "R0", "http://x/")]);
    assert_eq!(meta.error.as_ref().map(|e| e.kind), Some(ErrorKind::Other));
    feed(&mut meta, &[error_page("MAIN", None, "R39", "http://x/")]);
    assert_eq!(
        meta.error.as_ref().map(|e| e.kind),
        Some(ErrorKind::Blocked)
    );
}

/// 复核修复:SPA 的 pushState 只来 Page.navigatedWithinDocument,不更新就会让地址栏、
/// 代理结果与「在系统浏览器打开」一直用旧地址。只认主 frame。
#[test]
fn 同文档导航只认主帧并更新地址() {
    let mut meta = meta(true, None, true);
    feed(&mut meta, &[main_commit("L0", "http://localhost:5173/")]);
    assert_eq!(meta.nav.main_frame_id.as_deref(), Some("MAIN"));
    let same = |frame: &str, url: &str| {
        (
            "Page.navigatedWithinDocument",
            serde_json::json!({"frameId": frame, "url": url, "navigationType": "historyApi"}),
        )
    };
    let effects = feed(&mut meta, &[same("MAIN", "http://localhost:5173/users/7")]);
    assert_eq!(effects, vec![MetaEffect::SameDocument]);
    assert_eq!(meta.url, "http://localhost:5173/users/7");
    let effects = feed(&mut meta, &[same("IFRAME", "http://ads.example/x#y")]);
    assert_eq!(effects, vec![MetaEffect::None], "iframe 的 pushState 不算");
    assert_eq!(meta.url, "http://localhost:5173/users/7");
    let effects = feed(&mut meta, &[same("MAIN", "http://localhost:5173/users/7")]);
    assert_eq!(effects, vec![MetaEffect::None], "地址没变不发状态");
    // 还不知道主 frame id 时一律不认(不猜)。
    let mut fresh = self::meta(true, None, true);
    assert_eq!(
        feed(&mut fresh, &[same("MAIN", "http://localhost:5173/a")]),
        vec![MetaEffect::None]
    );
}

#[test]
fn fit_device_铺满_缩放居中_不超过1且有下限() {
    let host = Rect {
        x: 100.0,
        y: 50.0,
        w: 360.0,
        h: 700.0,
    };
    assert_eq!(fit_device(host, DevicePreset::Fill), (host, 1.0));

    let (rect, zoom) = fit_device(host, DevicePreset::Phone);
    assert!(zoom < 1.0, "390×844 放进 360×700 必须缩小");
    assert!((zoom - 700.0 / 844.0).abs() < 1e-9);
    assert!((rect.w - 390.0 * zoom).abs() < 1e-9 && (rect.h - 700.0).abs() < 1e-9);
    assert!(
        (rect.x - (100.0 + (360.0 - rect.w) / 2.0)).abs() < 1e-9,
        "水平居中"
    );
    assert!(rect.x >= host.x && rect.x + rect.w <= host.x + host.w + 1e-9);

    let big = Rect {
        x: 0.0,
        y: 0.0,
        w: 2000.0,
        h: 2000.0,
    };
    let (rect, zoom) = fit_device(big, DevicePreset::Tablet);
    assert_eq!(zoom, 1.0, "缩放不超过 1");
    assert_eq!((rect.w, rect.h), (768.0, 1024.0));
    assert_eq!((rect.x, rect.y), (616.0, 488.0));

    let tiny = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 150.0,
    };
    let (rect, zoom) = fit_device(tiny, DevicePreset::Desktop);
    assert_eq!(zoom, MIN_ZOOM, "WebView2 ZoomFactor 有下限");
    assert!(rect.w <= tiny.w && rect.h <= tiny.h, "裁到面板内");

    let empty = Rect::default();
    assert_eq!(fit_device(empty, DevicePreset::Phone).0.w, 0.0);
}

#[test]
fn 导航失败分类与说明() {
    assert_eq!(
        error_kind("net::ERR_CONNECTION_REFUSED"),
        ErrorKind::ConnectionRefused
    );
    assert_eq!(error_kind("net::ERR_UNSAFE_PORT"), ErrorKind::UnsafePort);
    assert_eq!(error_kind("net::ERR_BLOCKED_BY_CLIENT"), ErrorKind::Blocked);
    assert_eq!(error_kind("net::ERR_NAME_NOT_RESOLVED"), ErrorKind::Other);
    let described = describe_error("net::ERR_CONNECTION_REFUSED", "http://127.0.0.1:1/");
    assert!(described.text.contains("服务没在跑"));
    assert!(
        described.text.contains("ERR_CONNECTION_REFUSED"),
        "保留原始错误码,代理侧的处理建议靠它"
    );
}

#[test]
fn 状态载荷是驼峰且没有错误时省略error() {
    let mut meta = meta(true, Some("d|C:/p"), true);
    meta.url = "http://localhost:5173/".into();
    let payload = state_payload(&meta);
    for key in [
        "url",
        "title",
        "loading",
        "canBack",
        "canForward",
        "visible",
        "boundProcessId",
        "device",
        "scheme",
    ] {
        assert!(payload.get(key).is_some(), "缺 {key}: {payload}");
    }
    assert!(payload.get("error").is_none());
    assert!(payload.get("alive").is_none() && payload.get("host").is_none());
    assert_eq!(payload["device"], "fill");
    assert_eq!(payload["scheme"], "auto");
    meta.error = Some(describe_error("net::ERR_UNSAFE_PORT", "u"));
    assert_eq!(state_payload(&meta)["error"]["kind"], "unsafe_port");
}

#[test]
fn 设备与配色解析() {
    assert_eq!(DevicePreset::parse("phone"), Some(DevicePreset::Phone));
    assert_eq!(DevicePreset::parse("watch"), None);
    assert_eq!(
        DevicePreset::from_viewport("mobile-375x667"),
        Some(DevicePreset::Phone)
    );
    assert_eq!(
        DevicePreset::from_viewport("tablet-768x1024"),
        Some(DevicePreset::Tablet)
    );
    assert!(DevicePreset::Phone.touch() && !DevicePreset::Desktop.touch());
    assert_eq!(ColorScheme::parse("dark"), Some(ColorScheme::Dark));
    assert_eq!(ColorScheme::parse("sepia"), None);
}

// ── 源码守卫:B0 的几条「只能靠不调用来保证」的约束 ──

fn app_sources() -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                out.push((path, text));
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    assert!(out.len() > 20, "源码扫描范围异常: {}", out.len());
    out
}

/// 在**代码行**里找(整行注释不算:文档里要能写出这些名字来说明为什么不用)。
fn hits(needle: &str) -> Vec<PathBuf> {
    app_sources()
        .into_iter()
        .filter(|(_, text)| {
            text.lines()
                .any(|line| !line.trim_start().starts_with("//") && line.contains(needle))
        })
        .map(|(path, _)| path)
        .collect()
}

/// B0:OpenDevToolsWindow 在 AreDevToolsEnabled=false 时照样能开,而且**主界面也能被打开**。
/// 唯一防线是只在 cdp::open_devtools(只收 PaneWebview)里调用它。
#[test]
fn open_devtools只在面板句柄上调用_主webview永不() {
    let needle = concat!("OpenDevTools", "Window(");
    let files = hits(needle);
    assert_eq!(files.len(), 1, "只允许一处调用: {files:?}");
    assert!(files[0].ends_with("preview/cdp.rs"), "{files:?}");
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert_eq!(text.matches(needle).count(), 1, "cdp.rs 里也只允许一处");
    let call_at = text.find(needle).unwrap();
    let function_at = text[..call_at]
        .rfind("async fn ")
        .expect("调用必须在函数里");
    assert!(
        text[function_at..call_at].starts_with("async fn open_devtools(pane: &super::PaneWebview)"),
        "OpenDevToolsWindow 只能出现在只收 PaneWebview 的 open_devtools 里"
    );
    assert!(
        text[function_at..call_at].contains("devtools_target_allowed"),
        "调用前还要按 label 复核"
    );
    assert!(cdp::devtools_target_allowed("preview-3"));
    assert!(!cdp::devtools_target_allowed("preview-"));
    assert!(!cdp::devtools_target_allowed("preview-x"));
    assert!(!cdp::devtools_target_allowed("previewer-1"));
    assert!(!cdp::devtools_target_allowed(MAIN_LABEL));
    assert!(!cdp::devtools_target_allowed("other"));
}

/// B0:不开 AreDevToolsEnabled(F12 / Inspect 保持关闭);也不开 tauri 的 devtools feature。
#[test]
fn 不打开devtools开关也不用tauri的devtools() {
    assert!(hits(concat!("SetAreDevTools", "Enabled")).is_empty());
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let tauri_line = manifest
        .lines()
        .find(|line| line.starts_with("tauri = "))
        .unwrap();
    assert!(!tauri_line.contains("devtools"), "{tauri_line}");
    assert!(tauri_line.contains("\"unstable\""), "{tauri_line}");
}

/// 面板与主界面共用 WebView2 profile:清全部浏览数据会清掉 kanzei 自己的偏好。
#[test]
fn 禁止清全部浏览数据_只清当前源() {
    assert!(hits(concat!("clear_all_", "browsing_data")).is_empty());
    let pane =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/preview/pane.rs"))
            .unwrap();
    assert!(pane.contains("Storage.clearDataForOrigin"));
}

/// B0:第一次 add_child 之后 get_webview_window("main") 返回 None;用了它的代码会静默失效。
#[test]
fn 全仓不用按窗口取webview的旧接口() {
    let files = hits(concat!("get_webview_", "window("));
    assert!(files.is_empty(), "改用 get_webview / get_window: {files:?}");
}

/// B0 的建面板规则写死在 builder 上:不抢焦点、共用主环境、spawn_blocking 里 add_child。
/// 复核修复追加:子 frame 导航闸(FrameNavigationStarting)在建面板时挂上,判定用 frame_nav_allowed。
#[test]
fn 建面板遵守b0规则() {
    let src = |rel: &str| {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
            .unwrap()
            .replace("\r\n", "\n")
    };
    let pane = src("src/preview/pane.rs");
    for required in [
        ".focused(false)",
        ".with_environment(",
        "spawn_blocking",
        ".on_navigation(",
        "NewWindowResponse::Deny",
        "CAPTURE_TIMEOUT",
        "cdp::guard_frame_navigation(",
    ] {
        assert!(pane.contains(required), "pane.rs 缺 {required}");
    }
    assert!(
        !pane.contains(".additional_browser_args("),
        "子 webview 不能自带浏览器参数"
    );
    assert!(!pane.contains(".data_directory("));
    assert!(!pane.contains(".incognito("));
    let cdp = src("src/preview/cdp.rs");
    let guard_at = cdp
        .find("fn guard_frame_navigation(")
        .expect("cdp.rs 缺子 frame 导航闸");
    let body = &cdp[guard_at..];
    let body = &body[..body.find("\n    }\n").unwrap_or(body.len())];
    for required in [
        "add_FrameNavigationStarting(",
        "frame_nav_allowed(",
        "SetCancel(true)",
    ] {
        assert!(body.contains(required), "子 frame 导航闸缺 {required}");
    }
}

/// 复核修复(major):开 unstable 后主 webview 是 WindowChild,wry 不再给父窗口挂子类化。
/// host.rs 必须补回 WM_SETFOCUS → MoveFocus(且只在前台时)与 WM_MOVE → NotifyParentWindowPositionChanged,
/// 并且在启动装配里挂上、面板建成时登记、关面板时释放。
#[test]
fn 主窗口补回焦点归还与移动通知() {
    let src = |rel: &str| {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
            .unwrap()
            .replace("\r\n", "\n")
    };
    let host = src("src/preview/host.rs");
    let proc_at = host
        .find("unsafe extern \"system\" fn host_proc(")
        .expect("缺子类化回调");
    let proc_body = &host[proc_at..];
    for required in [
        "WM_SETFOCUS => restore_focus(hwnd)",
        "WM_MOVE | WM_MOVING => notify_moved()",
        "DefSubclassProc(",
    ] {
        assert!(proc_body.contains(required), "host_proc 缺 {required}");
    }
    for required in [
        "SetWindowSubclass(",
        "MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)",
        "NotifyParentWindowPositionChanged()",
        "GetForegroundWindow()",
        "add_GotFocus(",
    ] {
        assert!(host.contains(required), "host.rs 缺 {required}");
    }
    let restore_at = host.find("fn restore_focus(").unwrap();
    let restore = &host[restore_at
        ..host[restore_at..]
            .find("\n}\n")
            .map_or(host.len(), |end| restore_at + end)];
    let foreground_at = restore
        .find("GetForegroundWindow()")
        .expect("转交焦点前必须查前台");
    let move_at = restore
        .find("MoveFocus(")
        .expect("restore_focus 必须 MoveFocus");
    assert!(
        foreground_at < move_at,
        "先查前台再转交焦点(后台调 MoveFocus 会抢前台)"
    );
    let module = src("src/preview/mod.rs");
    let install_at = module.find("pub(crate) fn install(").unwrap();
    assert!(
        module[install_at..].contains("host::attach_main(main_window)"),
        "启动装配里必须挂上主窗口子类化"
    );
    let pane = src("src/preview/pane.rs");
    assert!(pane.contains("host::register_panel(&pane.webview, generation)"));
    assert!(pane.contains("host::release_panel_on_ui_thread(generation)"));
}

// ── 集成收尾:前端请后端补的两条(preview_pane.md「前端」§10 第 2、3 条) ──

/// 顶层函数的函数体(从签名到第一个行首的 `}`)。
fn top_level_fn<'a>(src: &'a str, signature: &str) -> &'a str {
    let at = src
        .find(signature)
        .unwrap_or_else(|| panic!("缺 {signature}"));
    let body = &src[at..];
    &body[..body.find("\n}\n").map_or(body.len(), |end| end + 3)]
}

fn app_src(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap()
        .replace("\r\n", "\n")
}

/// 主界面 F5 / Ctrl+R 重载后 Rust 侧的子 webview 还活着、还可见,新起的前端却从「没有面板」起步:
/// 主 webview 的 PageLoadEvent::Started 收面板(前端启动 preview_close 的双保险)。
/// 只认主 webview 的 Started;第一次启动的那次加载同样会来,此时 close 对空 slot 只递增代次就返回。
#[test]
fn 主界面开始重新加载时收面板_只认主webview的started() {
    use tauri::webview::PageLoadEvent;
    assert!(main_load_closes_pane(MAIN_LABEL, PageLoadEvent::Started));
    assert!(
        !main_load_closes_pane(MAIN_LABEL, PageLoadEvent::Finished),
        "Finished 不收:错误页、同一次加载的收尾都会来"
    );
    assert!(
        !main_load_closes_pane("preview-3", PageLoadEvent::Started),
        "面板自己的加载绝不能把自己关掉"
    );
    assert!(!main_load_closes_pane("other", PageLoadEvent::Started));

    // 接线:主窗口 builder 上挂 on_page_load,转给 preview::on_main_page_load(按 label 判)。
    let main = app_src("src/main.rs");
    let hook_at = main
        .find(".on_page_load(")
        .expect("main.rs 的主窗口 builder 缺 on_page_load");
    let build_at = main.find("builder.build()").unwrap();
    assert!(hook_at < build_at, "on_page_load 必须挂在 build 之前");
    assert!(main[hook_at..build_at].contains("preview::on_main_page_load("));
    let module = app_src("src/preview/mod.rs");
    let handler = top_level_fn(&module, "pub(crate) fn on_main_page_load(");
    let gate_at = handler
        .find("main_load_closes_pane(")
        .expect("先按 label 与事件判");
    let close_at = handler.find("pane::close(").expect("判中后收面板");
    assert!(gate_at < close_at);
    assert!(
        handler.contains("async_runtime::spawn"),
        "不在主 webview 的 ContentLoading 回调里同步关另一个 webview"
    );
    // 首次加载无害:没有面板时 close 在 discard / 发事件之前就返回。
    let pane = app_src("src/preview/pane.rs");
    let close = top_level_fn(&pane, "pub(crate) fn close(app: &AppHandle)");
    let empty_at = close
        .find("let Some(pane) = pane else")
        .expect("close 必须先判空");
    assert!(empty_at < close.find("discard(").unwrap());
    assert!(empty_at < close.find("emit_closed(").unwrap());
}

/// 批注点选完成时系统焦点还在子 webview 里,前端 promptBox.focus() 接不到键盘:发出 kz:preview-pick 后
/// 把焦点还给主界面(get_webview("main") 的 set_focus),且只在 kanzei 就是前台程序时(不抢别的程序的焦点)。
#[test]
fn 批注完成后焦点还给主界面_只在kanzei是前台时() {
    assert!(host::foreground_is_main(0x1234, 0x1234));
    assert!(!host::foreground_is_main(0x9999, 0x1234), "别的程序在前台");
    assert!(
        !host::foreground_is_main(0, 0),
        "主窗口句柄还没记下(attach 之前):一律不算前台"
    );
    assert!(!host::foreground_is_main(0x1234, 0));

    let pane = app_src("src/preview/pane.rs");
    let pick = top_level_fn(&pane, "async fn finish_pick(");
    let emit_at = pick
        .find("app.emit(\"kz:preview-pick\"")
        .expect("finish_pick 发 kz:preview-pick");
    let focus_at = pick
        .find("return_focus_to_main(app)")
        .expect("发出批注后必须把焦点还给主界面");
    assert!(emit_at < focus_at, "先发事件(前端聚焦输入框)再还焦点");

    let give_back = top_level_fn(&pane, "pub(crate) fn return_focus_to_main(");
    assert!(
        give_back.contains("app.get_webview(MAIN_LABEL)"),
        "用 get_webview(\"main\"):第一次 add_child 之后 get_webview_window 是 None"
    );
    assert!(give_back.contains("run_on_main_thread("));
    let foreground_at = give_back
        .find("host::main_is_foreground()")
        .expect("转交前必须查前台");
    let focus_call_at = give_back.find(".set_focus()").expect("set_focus");
    assert!(
        foreground_at < focus_call_at,
        "先查前台再 set_focus(MoveFocus 会把窗口拉到前台)"
    );
    let host_src = app_src("src/preview/host.rs");
    let check_at = host_src.find("pub(crate) fn main_is_foreground()").unwrap();
    let check = &host_src[check_at..];
    let check = &check[..check.find("\n    }\n").unwrap_or(check.len())];
    assert!(check.contains("GetForegroundWindow()"));
    assert!(check.contains("foreground_is_main("));
    assert!(
        host_src.contains("MAIN_HWND.store(hwnd, Ordering::SeqCst)"),
        "attach_main 要记下主窗口句柄"
    );
}
