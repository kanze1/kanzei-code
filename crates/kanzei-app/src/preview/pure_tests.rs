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
    assert_eq!(route_for(Some("p|a")), Backend::Headless);
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
#[test]
fn 建面板遵守b0规则() {
    let pane =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/preview/pane.rs"))
            .unwrap();
    for required in [
        ".focused(false)",
        ".with_environment(",
        "spawn_blocking",
        ".on_navigation(",
        "NewWindowResponse::Deny",
        "CAPTURE_TIMEOUT",
    ] {
        assert!(pane.contains(required), "pane.rs 缺 {required}");
    }
    assert!(
        !pane.contains(".additional_browser_args("),
        "子 webview 不能自带浏览器参数"
    );
    assert!(!pane.contains(".data_directory("));
    assert!(!pane.contains(".incognito("));
}
