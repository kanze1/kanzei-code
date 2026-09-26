//! 浏览器工具(R-269;UI2-0926 #8 扩展):一个工具名,两个后端。
//!
//! - **无头后端**(本 crate,CLI 与桌面兜底):Rust 经 stdio 起 `scripts/browser-helper.mjs`
//!   (Node 辅进程),playwright-core 以 channel 模式自 launch 本机 Edge/Chrome headless,不碰
//!   WebView2,天然绕开 D-319。机制见 [`headless`]。
//! - **面板后端**(kanzei-app `preview::agent`):桌面端「网页预览」面板开着、又正显示这条线时,
//!   同名动作经进程内 CDP 驱动用户眼前的面板。走哪边由引擎路由(`preview::route`),模型不选。
//!
//! 两个后端共用这里的输入结构、schema、目标解析(本地路径与 HTML 片段一律走
//! [`crate::preview_server`] 的 127.0.0.1 静态服务)、权限资源与**全部输出格式**——
//! 结果首行写明 `backend: pane（用户可见）` 或 `backend: headless`,其余逐字同形。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kanzei_harness::ToolOutput;
use serde::Deserialize;
use sha2::Digest;

mod headless;
pub use headless::execute_headless;

/// DOM walker 唯一源:无头侧 helper `import`,面板侧经 [`dom_walker_expression`] 包进 CDP 表达式。
const DOM_WALKER_SOURCE: &str = include_str!("../../../scripts/browser-dom-walker.mjs");

/// 单次截图体积上限(与 R-249 截图口径一致,防超大 base64 打爆上下文)。
pub const MAX_SCREENSHOT_BYTES: usize = 4 * 1024 * 1024;
/// eval 结果回喂上限(字符)。
pub const MAX_EVAL_CHARS: usize = 8_000;
/// wait 的等待上限。
pub const MAX_WAIT_MS: u64 = 10_000;
/// console all=true 时回喂的条数上限。
pub const CONSOLE_ALL_LIMIT: usize = 200;

/// 结果走了哪个后端。首行必须写明——弱模型分不清两个后端时,至少知道用户看没看见。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Pane,
    Headless,
}

impl Backend {
    pub fn line(self) -> &'static str {
        match self {
            Backend::Pane => "backend: pane（用户可见）",
            Backend::Headless => "backend: headless",
        }
    }

    fn slot(self) -> usize {
        match self {
            Backend::Pane => 0,
            Backend::Headless => 1,
        }
    }
}

/// 桌面端走无头时追加的一行:用户看不到无头画面,卡片上有「在预览中打开」。
pub const HEADLESS_PANE_HINT: &str = "提示: 用户看不到无头浏览器的画面;需要用户一起看时,请用户点卡片上的「在预览中打开」把它显示在网页预览面板里。";

/// R-269 浏览器工具(无头后端)。CLI 与桌面兜底都注册它;桌面端由 kanzei-app 的
/// DesktopBrowserTool 以同名覆盖、按面板状态路由。
pub(crate) struct BrowserTool;

#[async_trait::async_trait]
impl kanzei_harness::Tool for BrowserTool {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn description(&self) -> String {
        description()
    }

    fn input_schema(&self) -> serde_json::Value {
        input_schema()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        resources_for(input, None, current_url(Backend::Headless).as_deref())
    }

    fn resources_with_ctx(
        &self,
        input: &serde_json::Value,
        ctx: &kanzei_harness::ToolCtx,
    ) -> Vec<String> {
        resources_for(
            input,
            Some(&ctx.cwd),
            current_url(Backend::Headless).as_deref(),
        )
    }

    fn concurrency(
        &self,
        _input: &serde_json::Value,
        ctx: &kanzei_harness::ToolCtx,
    ) -> kanzei_harness::ToolConcurrency {
        // 辅进程单例共享 browser:同一 tree 内串行(避免并发 RPC 交错)。
        kanzei_harness::ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &kanzei_harness::ToolCtx) -> ToolOutput {
        match parse_browser_input(input) {
            Ok(parsed) => execute_headless(parsed, ctx, false).await,
            Err(output) => *output,
        }
    }
}

/// 两个后端共用的工具说明。
pub fn description() -> String {
    "Drive a browser page and see it. Actions: open (navigate to url/path/html, return a \
     screenshot), screenshot (current page, no reload; full_page or selector), dom, console \
     (errors/warnings; all=true for every level), click, type, press (key), scroll, wait \
     (selector/text/ms ≤10000), eval (JS expression → JSON). Targets: url (http/https), path \
     (local HTML, served from a 127.0.0.1 static server so ES modules work) or html (inline \
     snippet). Stateful: open once, then omit url/path/html to keep the current page. Optional \
     viewport (mobile/tablet/desktop presets) and color_scheme (light|dark). In the desktop \
     app, when the user has the web preview pane open on this conversation, actions drive that \
     visible pane; otherwise a headless Edge/Chrome. Result line 1 names the backend."
        .into()
}

/// 工具输入。字段与 [`input_schema`] 对齐;面板后端直接读这些字段。
#[derive(Deserialize, Debug, Clone)]
pub struct BrowserInput {
    /// 目标 http(s) URL(也接受 file:// 与 about:blank)。
    pub url: Option<String>,
    /// 本地 HTML 文件路径(相对代码树或绝对),经静态服务打开。
    pub path: Option<String>,
    /// 内联 HTML 片段,经静态服务 /s/ 打开。
    pub html: Option<String>,
    #[serde(default = "default_action")]
    pub action: String,
    pub selector: Option<String>,
    pub text: Option<String>,
    pub key: Option<String>,
    pub expression: Option<String>,
    pub dy: Option<f64>,
    pub ms: Option<u64>,
    #[serde(default)]
    pub full_page: bool,
    #[serde(default)]
    pub all: bool,
    pub viewport: Option<String>,
    pub color_scheme: Option<String>,
    #[serde(default = "default_channel")]
    pub channel: String,
}

impl Default for BrowserInput {
    fn default() -> Self {
        BrowserInput {
            url: None,
            path: None,
            html: None,
            action: default_action(),
            selector: None,
            text: None,
            key: None,
            expression: None,
            dy: None,
            ms: None,
            full_page: false,
            all: false,
            viewport: None,
            color_scheme: None,
            channel: default_channel(),
        }
    }
}

fn default_channel() -> String {
    "msedge".into()
}

fn default_action() -> String {
    "open".into()
}

/// 解析工具输入;失败时给出带格式说明的纠错输出。
pub fn parse_browser_input(input: serde_json::Value) -> Result<BrowserInput, Box<ToolOutput>> {
    serde_json::from_value(input).map_err(|e| {
        Box::new(ToolOutput::error(format!(
            "browser 参数解析失败: {e}。open 需要 url、path 或 html 之一;其余动作可省略目标复用当前页面"
        )))
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserAction {
    Open,
    Screenshot,
    Dom,
    Console,
    Click,
    Type,
    Press,
    Scroll,
    Wait,
    Eval,
}

impl BrowserAction {
    /// 动作名解析。弱模型常写的近义词直接映射,其余报错并列出合法值——
    /// 旧实现把未知动作一律当 open,会悄悄重新导航、丢掉页面状态。
    pub fn parse(raw: &str) -> Result<Self, String> {
        Ok(match raw.trim().to_ascii_lowercase().as_str() {
            "" | "open" | "navigate" | "goto" | "visit" => BrowserAction::Open,
            "screenshot" | "capture" | "snapshot" => BrowserAction::Screenshot,
            "dom" => BrowserAction::Dom,
            "console" | "logs" => BrowserAction::Console,
            "click" => BrowserAction::Click,
            "type" | "fill" => BrowserAction::Type,
            "press" | "key" | "keypress" => BrowserAction::Press,
            "scroll" => BrowserAction::Scroll,
            "wait" => BrowserAction::Wait,
            "eval" | "evaluate" | "js" => BrowserAction::Eval,
            other => {
                return Err(format!(
                    "未知 browser 动作 {other:?}。可用:open | screenshot | dom | console | click | type | press | scroll | wait | eval"
                ))
            }
        })
    }
}

/// 动作参数的机械校验(两个后端执行前都先过这一道,错误文本一致)。
pub fn validate(input: &BrowserInput, action: BrowserAction) -> Result<(), String> {
    let targets = [&input.url, &input.path, &input.html]
        .iter()
        .filter(|value| value.is_some())
        .count();
    if targets > 1 {
        return Err("url、path、html 只能给一个".into());
    }
    let non_empty = |value: &Option<String>| value.as_deref().is_some_and(|v| !v.trim().is_empty());
    match action {
        BrowserAction::Open if targets == 0 => {
            Err("open 需要 url、path 或 html 之一;其余动作省略目标即复用当前页面".into())
        }
        BrowserAction::Click if !non_empty(&input.selector) => {
            Err("click 需要 selector 参数".into())
        }
        BrowserAction::Type if !non_empty(&input.selector) => Err("type 需要 selector 参数".into()),
        BrowserAction::Type if input.text.is_none() => Err("type 需要 text 参数".into()),
        BrowserAction::Press if !non_empty(&input.key) => {
            Err("press 需要 key 参数(如 Enter、Tab、Escape、Control+A)".into())
        }
        BrowserAction::Eval if !non_empty(&input.expression) => {
            Err("eval 需要 expression 参数".into())
        }
        BrowserAction::Wait
            if !non_empty(&input.selector) && !non_empty(&input.text) && input.ms.is_none() =>
        {
            Err("wait 需要 selector、text 或 ms 之一(ms ≤ 10000)".into())
        }
        _ => Ok(()),
    }
}

/// 解析后的导航目标。`note` 是需要如实告诉模型的降级说明(静态服务不可用回落 file:// 等)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavTarget {
    pub url: String,
    pub note: Option<String>,
}

/// 把 url/path/html 换成可导航的 URL。本地路径与 HTML 片段走 127.0.0.1 静态服务
/// (file:// 下 ES module 与 fetch 相对资源会失败);相对路径以代码树 `cwd` 为基准。
pub fn resolve_nav_target(
    input: &BrowserInput,
    cwd: &Path,
    project_root: &Path,
) -> Result<Option<NavTarget>, String> {
    if let Some(html) = &input.html {
        let server = crate::preview_server::global()
            .map_err(|e| format!("html 片段需要预览静态服务: {e}"))?;
        return Ok(Some(NavTarget {
            url: server.add_snippet(html)?,
            note: None,
        }));
    }
    if let Some(path) = &input.path {
        return local_target(&absolute(cwd, Path::new(path)), cwd, project_root).map(Some);
    }
    let Some(url) = input.url.as_deref().map(str::trim) else {
        return Ok(None);
    };
    if url.starts_with("file:") {
        let parsed = reqwest::Url::parse(url).map_err(|e| format!("file URL 无法解析: {e}"))?;
        let path = parsed
            .to_file_path()
            .map_err(|_| format!("file URL 不是本地路径: {url}"))?;
        return local_target(&path, cwd, project_root).map(Some);
    }
    if url.starts_with("http://") || url.starts_with("https://") || url == "about:blank" {
        return Ok(Some(NavTarget {
            url: url.to_string(),
            note: None,
        }));
    }
    Err(format!(
        "url 必须是 http(s)://、file:// 或 about:blank,实得: {url}"
    ))
}

fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn local_target(path: &Path, cwd: &Path, project_root: &Path) -> Result<NavTarget, String> {
    if !path.exists() {
        return Err(format!("本地文件不存在: {}", path.display()));
    }
    let roots: Vec<PathBuf> = [cwd, project_root]
        .iter()
        .filter(|root| !root.as_os_str().is_empty())
        .map(|root| root.to_path_buf())
        .collect();
    match crate::preview_server::global() {
        Ok(server) => Ok(NavTarget {
            url: server.url_for_path(path, &roots)?,
            note: None,
        }),
        Err(error) => Ok(NavTarget {
            url: file_url(path)?,
            note: Some(format!(
                "预览静态服务不可用({error}),已回落 file:// 打开;ES module 与 fetch 相对资源在 file:// 下可能失败"
            )),
        }),
    }
}

fn file_url(path: &Path) -> Result<String, String> {
    let abs = std::fs::canonicalize(path)
        .map_err(|e| format!("本地文件不存在或无法解析: {} ({e})", path.display()))?;
    let raw = crate::path_form::strip_verbatim(&abs.to_string_lossy()).into_owned();
    Ok(format!("file:///{}", raw.replace('\\', "/")))
}

/// 权限资源:`url:<host[:port]/path>` / `path:<abs>` / `html:<sha8>`;不带目标的动作取
/// 当前页(`current`),没有当前页记 `page:none`。
///
/// URL 按解析后的 host 取,不按字符串前缀——`http://localhost:80@evil.com/` 的 host 是
/// evil.com,不能被 `url:localhost:*` 放行。
pub fn resources_for(
    input: &serde_json::Value,
    cwd: Option<&Path>,
    current: Option<&str>,
) -> Vec<String> {
    if let Some(html) = input["html"].as_str() {
        let digest = sha2::Sha256::digest(html.as_bytes());
        let short: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
        return vec![format!("html:{short}")];
    }
    if let Some(path) = input["path"].as_str() {
        let abs = match cwd {
            Some(cwd) => absolute(cwd, Path::new(path)),
            None => PathBuf::from(path),
        };
        return vec![format!("path:{}", abs.display())];
    }
    if let Some(url) = input["url"].as_str() {
        return vec![url_resource(url)];
    }
    match current {
        Some(url) => vec![url_resource(url)],
        None => vec!["page:none".into()],
    }
}

/// 单个 URL 的权限资源形态。
pub fn url_resource(url: &str) -> String {
    let url = url.trim();
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return format!("url:{url}");
    };
    match parsed.scheme() {
        "http" | "https" => {
            let host = parsed.host_str().unwrap_or("");
            let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
            let path = parsed.path().trim_end_matches('/');
            format!("url:{host}{port}{path}")
        }
        "file" => match parsed.to_file_path() {
            Ok(path) => format!("path:{}", path.display()),
            Err(_) => format!("url:{url}"),
        },
        _ => format!("url:{url}"),
    }
}

static CURRENT_URLS: Mutex<[Option<String>; 2]> = Mutex::new([None, None]);

/// 记录某后端的当前页 URL(权限判定里不带目标的动作取它)。
pub fn set_current_url(backend: Backend, url: Option<String>) {
    if let Ok(mut slots) = CURRENT_URLS.lock() {
        slots[backend.slot()] = url;
    }
}

pub fn current_url(backend: Backend) -> Option<String> {
    CURRENT_URLS
        .lock()
        .ok()
        .and_then(|slots| slots[backend.slot()].clone())
}

/// 面板后端用:把 walker 源码包成一条 `Runtime.evaluate` 表达式,返回 JSON 字符串。
pub fn dom_walker_expression(selector: Option<&str>) -> String {
    let body = DOM_WALKER_SOURCE.replacen("export function domWalker", "function domWalker", 1);
    let selector = serde_json::to_string(&selector).unwrap_or_else(|_| "null".into());
    format!("(() => {{\n{body}\nreturn domWalker({selector});\n}})()")
}

/// 视口预设;未知名返回 None(无头用默认 1280x720)。
pub fn parse_viewport(name: Option<&str>) -> Option<(u32, u32)> {
    Some(match name? {
        "mobile-375x667" => (375, 667), // iPhone SE 2 代
        "mobile-390x844" => (390, 844), // iPhone 12/13/14
        "mobile-412x915" => (412, 915), // Android Pixel 系
        "mobile-360x800" => (360, 800), // 小屏 Android
        "tablet-768x1024" => (768, 1024),
        "desktop-1280x800" => (1280, 800),
        _ => return None,
    })
}

pub fn viewport_label(name: Option<&str>) -> String {
    match name {
        Some(n) => n.to_string(),
        None => "desktop-1280x720".into(),
    }
}

/// console 条目(两个后端同一形态)。行列 1 起算。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConsoleItem {
    pub level: String,
    pub text: String,
    pub url: Option<String>,
    pub line: Option<u64>,
    pub col: Option<u64>,
}

impl ConsoleItem {
    /// 错误一类:error / warning / 未捕获异常 / 资源加载失败。
    pub fn is_problem(&self) -> bool {
        matches!(
            self.level.as_str(),
            "error" | "warning" | "pageerror" | "network" | "assert"
        )
    }
}

/// 将 console 条目格式化为面向模型的诊断文本。浏览器提供资源 URL 时必须保留，
/// 否则诸如 favicon 404 只剩一条无来源的通用错误，无法区分页面脚本与静态资源。
pub fn format_console_item(item: &ConsoleItem) -> String {
    let Some(url) = item.url.as_deref().filter(|url| !url.is_empty()) else {
        return format!("[{}] {}", item.level, item.text);
    };
    match (item.line, item.col) {
        (Some(line), Some(col)) => format!("[{}] {} ({url}:{line}:{col})", item.level, item.text),
        (Some(line), None) => format!("[{}] {} ({url}:{line})", item.level, item.text),
        _ => format!("[{}] {} ({url})", item.level, item.text),
    }
}

fn with_backend(backend: Backend, body: String) -> String {
    format!("{}\n{body}", backend.line())
}

fn with_png(output: ToolOutput, backend: Backend, png_b64: String) -> ToolOutput {
    if png_b64.len() > MAX_SCREENSHOT_BYTES {
        return ToolOutput::error(with_backend(
            backend,
            format!(
                "截图过大({} base64 字节 > {} 上限),未回喂模型;改用 selector 截元素或去掉 full_page",
                png_b64.len(),
                MAX_SCREENSHOT_BYTES
            ),
        ));
    }
    output.with_images(vec![kanzei_harness::ToolImage {
        media_type: "image/png".into(),
        data: png_b64,
    }])
}

/// open 的结果:打开并截图。
pub fn out_open(
    backend: Backend,
    title: &str,
    url: &str,
    viewport: &str,
    notes: &[String],
    png_b64: String,
) -> ToolOutput {
    let mut body = format!("浏览器已打开并截图:\ntitle: {title}\nurl: {url}\nviewport: {viewport}");
    for note in notes {
        body.push('\n');
        body.push_str(note);
    }
    with_png(
        ToolOutput::ok(with_backend(backend, body)),
        backend,
        png_b64,
    )
}

/// screenshot 的结果:当前页截图,不重新导航。
pub fn out_screenshot(
    backend: Backend,
    url: &str,
    viewport: &str,
    scope: &str,
    notes: &[String],
    png_b64: String,
) -> ToolOutput {
    let mut body = format!("已截图(未重新导航,{scope}):\nurl: {url}\nviewport: {viewport}");
    for note in notes {
        body.push('\n');
        body.push_str(note);
    }
    with_png(
        ToolOutput::ok(with_backend(backend, body)),
        backend,
        png_b64,
    )
}

pub fn out_dom(backend: Backend, url: &str, selector: &str, structure: &str) -> ToolOutput {
    if structure.is_empty() {
        return ToolOutput::error(with_backend(
            backend,
            format!("dom 读取为空(selector: {selector:?})"),
        ));
    }
    ToolOutput::ok(with_backend(
        backend,
        format!("页面 DOM 结构(url: {url}):\n{structure}"),
    ))
}

/// console:默认只报错误/警告(有即判工具错误);all=true 回最近 200 条全部级别。
pub fn out_console(backend: Backend, url: &str, entries: &[ConsoleItem], all: bool) -> ToolOutput {
    let shown: Vec<&ConsoleItem> = if all {
        let skip = entries.len().saturating_sub(CONSOLE_ALL_LIMIT);
        entries.iter().skip(skip).collect()
    } else {
        entries.iter().filter(|item| item.is_problem()).collect()
    };
    if shown.is_empty() {
        let text = if all {
            format!("页面 console 为空(url: {url})")
        } else {
            format!("页面 console 无错误/警告(url: {url})")
        };
        return ToolOutput::ok(with_backend(backend, text));
    }
    let lines = shown
        .iter()
        .map(|item| format_console_item(item))
        .collect::<Vec<_>>()
        .join("\n");
    let problems = shown.iter().filter(|item| item.is_problem()).count();
    let header = if all {
        format!(
            "页面 console 最近 {} 条(其中错误/警告 {problems} 条,url: {url}):",
            shown.len()
        )
    } else {
        format!("页面 console 错误/警告 {} 条(url: {url}):", shown.len())
    };
    let text = with_backend(backend, format!("{header}\n{lines}"));
    if problems > 0 {
        ToolOutput::error(text)
    } else {
        ToolOutput::ok(text)
    }
}

pub fn out_click(backend: Backend, selector: &str, url: &str) -> ToolOutput {
    ToolOutput::ok(with_backend(
        backend,
        format!("已点击 {selector:?};当前 url: {url}"),
    ))
}

pub fn out_type(backend: Backend, selector: &str, chars: usize, url: &str) -> ToolOutput {
    ToolOutput::ok(with_backend(
        backend,
        format!("已向 {selector:?} 填入文本({chars} 字符);当前 url: {url}"),
    ))
}

pub fn out_press(backend: Backend, key: &str, url: &str) -> ToolOutput {
    ToolOutput::ok(with_backend(
        backend,
        format!("已按键 {key};当前 url: {url}"),
    ))
}

pub fn out_scroll(backend: Backend, what: &str, scroll_y: Option<f64>, url: &str) -> ToolOutput {
    let position = scroll_y
        .map(|y| format!(";scrollY={}", y.round()))
        .unwrap_or_default();
    ToolOutput::ok(with_backend(
        backend,
        format!("已滚动({what}){position};当前 url: {url}"),
    ))
}

pub fn out_wait(backend: Backend, what: &str, elapsed_ms: u64, url: &str) -> ToolOutput {
    ToolOutput::ok(with_backend(
        backend,
        format!("等待完成({what},{elapsed_ms} ms);当前 url: {url}"),
    ))
}

pub fn out_eval(backend: Backend, json: &str, url: &str) -> ToolOutput {
    let mut shown: String = json.chars().take(MAX_EVAL_CHARS).collect();
    if shown.len() < json.len() {
        shown.push_str(&format!(
            "\n…(已截断,原长 {} 字符;请缩小表达式的返回值)",
            json.chars().count()
        ));
    }
    ToolOutput::ok(with_backend(
        backend,
        format!("eval 结果(JSON,url: {url}):\n{shown}"),
    ))
}

/// 截图范围说明(两个后端同一措辞)。
pub fn screenshot_scope(input: &BrowserInput) -> String {
    match (&input.selector, input.full_page) {
        (Some(selector), _) => format!("元素 {selector:?}"),
        (None, true) => "整页".into(),
        (None, false) => "当前视口".into(),
    }
}

pub fn scroll_scope(input: &BrowserInput) -> String {
    match &input.selector {
        Some(selector) => format!("滚到 {selector:?}"),
        None => format!("dy={}", input.dy.unwrap_or(600.0)),
    }
}

pub fn wait_scope(input: &BrowserInput) -> String {
    match (&input.selector, &input.text) {
        (Some(selector), _) => format!("出现 {selector:?}"),
        (None, Some(text)) => format!("出现文字 {text:?}"),
        (None, None) => format!("{} ms", input.ms.unwrap_or(0).min(MAX_WAIT_MS)),
    }
}

/// 失败输出:带后端首行与处理建议。
pub fn browser_error(backend: Backend, error: &str) -> ToolOutput {
    let hint = if error.contains("no browser: call open first") || error.contains("尚未打开") {
        "先调用 browser open 并提供 url/path/html；后续 click/type/dom/console 可省略目标以复用当前页面。"
    } else if error.contains("ERR_CONNECTION_REFUSED")
        || error.contains("ERR_HTTP_RESPONSE_CODE_FAILURE")
        || error.contains("net::ERR_")
    {
        "先用 process list/output/wait 确认前端服务仍在运行，再按启动日志中的实际 Local URL 与端口访问；不要假设固定为 5173 或 4173。"
    } else {
        "可先用 browser console 和 process output 获取页面与服务端诊断。"
    };
    ToolOutput::error(with_backend(
        backend,
        format!("浏览器工具失败: {error}\n处理建议: {hint}"),
    ))
}

/// 工具输入 schema(与 Tool::input_schema 对接,手写 JSON Schema;字段与 BrowserInput 对齐)。
pub fn input_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "description": "目标 http(s) URL；open 必填其一(url/path/html)，其他动作省略时复用当前页面，提供时先重新导航" },
            "path": { "type": "string", "description": "本地 HTML 文件路径(相对代码树或绝对)，经 127.0.0.1 静态服务打开(ES module 可用)" },
            "html": { "type": "string", "description": "内联 HTML 片段(≤2MB)，经静态服务打开" },
            "action": {
                "type": "string",
                "enum": ["open", "screenshot", "dom", "console", "click", "type", "press", "scroll", "wait", "eval"],
                "description": "open(默认,打开并截图)| screenshot(当前页截图不重新导航,可 full_page/selector)| dom(可读 DOM,可选 selector)| console(错误/警告;all=true 返回全部级别最近 200 条)| click | type(selector+text)| press(key)| scroll(selector 或 dy)| wait(selector/text/ms≤10000)| eval(expression→JSON)"
            },
            "selector": { "type": "string", "description": "dom/click/type/scroll/wait/screenshot 用:CSS selector" },
            "text": { "type": "string", "description": "type:要填入的文本;wait:等待页面出现的文字" },
            "key": { "type": "string", "description": "press 用:按键名,如 Enter、Tab、Escape、ArrowDown、Control+A" },
            "expression": { "type": "string", "description": "eval 用:JS 表达式(可返回 Promise),结果按 JSON 回显(≤8000 字符)" },
            "dy": { "type": "number", "description": "scroll 用:纵向滚动像素(默认 600,负数向上)" },
            "ms": { "type": "integer", "description": "wait 用:等待上限或纯等待时长(毫秒,≤10000)" },
            "full_page": { "type": "boolean", "description": "screenshot 用:截整页" },
            "all": { "type": "boolean", "description": "console 用:返回全部级别" },
            "viewport": {
                "type": "string",
                "enum": ["mobile-375x667", "mobile-390x844", "mobile-412x915", "mobile-360x800", "tablet-768x1024", "desktop-1280x800"],
                "description": "视口预设(默认桌面 1280x720;面板后端映射为面板的手机/平板/桌面设备)"
            },
            "color_scheme": { "type": "string", "enum": ["light", "dark"], "description": "模拟 prefers-color-scheme" },
            "channel": {
                "type": "string",
                "enum": ["msedge", "chrome"],
                "description": "无头后端的浏览器 channel(默认 msedge)"
            }
        },
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-browser-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 工具 schema 与输入结构对齐:新动作、平板/桌面视口、color_scheme、html 都在。
    #[test]
    fn schema_含新动作_视口_color_scheme与html() {
        let schema = input_schema();
        let props = &schema["properties"];
        for field in [
            "url",
            "path",
            "html",
            "key",
            "expression",
            "dy",
            "ms",
            "full_page",
            "all",
        ] {
            assert!(props[field].is_object(), "schema 必须有 {field}");
        }
        let actions = props["action"]["enum"].as_array().unwrap();
        for action in [
            "open",
            "screenshot",
            "dom",
            "console",
            "click",
            "type",
            "press",
            "scroll",
            "wait",
            "eval",
        ] {
            assert!(actions.iter().any(|a| a == action), "缺动作 {action}");
            assert!(BrowserAction::parse(action).is_ok(), "{action} 必须可解析");
        }
        let viewport_enum = props["viewport"]["enum"].as_array().unwrap();
        for preset in [
            "mobile-375x667",
            "mobile-412x915",
            "tablet-768x1024",
            "desktop-1280x800",
        ] {
            assert!(viewport_enum.iter().any(|v| v == preset), "缺视口 {preset}");
            assert!(
                parse_viewport(Some(preset)).is_some(),
                "{preset} 必须可解析"
            );
        }
        let schemes = props["color_scheme"]["enum"].as_array().unwrap();
        assert!(schemes.iter().any(|v| v == "dark") && schemes.iter().any(|v| v == "light"));
        let channel_enum = props["channel"]["enum"].as_array().unwrap();
        assert!(channel_enum.iter().any(|v| v == "msedge"));
        assert!(channel_enum.iter().any(|v| v == "chrome"));
    }

    #[test]
    fn 动作近义词映射_未知动作报错而不是悄悄重新导航() {
        assert_eq!(BrowserAction::parse("goto").unwrap(), BrowserAction::Open);
        assert_eq!(
            BrowserAction::parse("Evaluate").unwrap(),
            BrowserAction::Eval
        );
        assert_eq!(BrowserAction::parse("fill").unwrap(), BrowserAction::Type);
        let err = BrowserAction::parse("hover").unwrap_err();
        assert!(err.contains("screenshot") && err.contains("eval"), "{err}");
    }

    #[test]
    fn 参数校验逐动作给出可行动错误() {
        let input = |action: &str| BrowserInput {
            action: action.into(),
            ..BrowserInput::default()
        };
        let check = |i: &BrowserInput| validate(i, BrowserAction::parse(&i.action).unwrap());
        assert!(check(&input("open"))
            .unwrap_err()
            .contains("url、path 或 html"));
        assert!(check(&input("click")).unwrap_err().contains("selector"));
        assert!(check(&input("press")).unwrap_err().contains("key"));
        assert!(check(&input("eval")).unwrap_err().contains("expression"));
        assert!(check(&input("wait")).unwrap_err().contains("ms"));
        assert!(check(&input("screenshot")).is_ok(), "screenshot 可省略目标");
        let both = BrowserInput {
            url: Some("http://localhost:1/".into()),
            html: Some("<p>x</p>".into()),
            ..BrowserInput::default()
        };
        assert!(check(&both).unwrap_err().contains("只能给一个"));
    }

    /// 目标解析:path/html 走静态服务,非法 url 被拒,缺参返回 None(由 validate 报错)。
    #[test]
    fn 目标解析_本地路径与片段走静态服务() {
        let dir = temp_dir("target");
        std::fs::write(dir.join("page one.html"), "<p>ok</p>").unwrap();
        let resolve = |input: BrowserInput| resolve_nav_target(&input, &dir, &dir);

        assert_eq!(resolve(BrowserInput::default()).unwrap(), None);
        let err = resolve(BrowserInput {
            url: Some("ftp://x".into()),
            ..BrowserInput::default()
        })
        .unwrap_err();
        assert!(err.contains("http(s)"), "{err}");

        let by_path = resolve(BrowserInput {
            path: Some("page one.html".into()),
            ..BrowserInput::default()
        })
        .unwrap()
        .unwrap();
        assert!(by_path.url.starts_with("http://127.0.0.1:"), "{by_path:?}");
        assert!(by_path.url.ends_with("/page%20one.html"), "{by_path:?}");
        assert!(by_path.note.is_none());

        // file:// URL 与 path 同一条路。
        let file_url = file_url(&dir.join("page one.html")).unwrap();
        let by_file = resolve(BrowserInput {
            url: Some(file_url),
            ..BrowserInput::default()
        })
        .unwrap()
        .unwrap();
        assert_eq!(by_file.url, by_path.url);

        let snippet = resolve(BrowserInput {
            html: Some("<h1>片段</h1>".into()),
            ..BrowserInput::default()
        })
        .unwrap()
        .unwrap();
        assert!(snippet.url.contains("/s/") && snippet.url.ends_with(".html"));

        let missing = resolve(BrowserInput {
            path: Some("nope.html".into()),
            ..BrowserInput::default()
        })
        .unwrap_err();
        assert!(missing.contains("不存在"), "{missing}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 权限资源按解析后的host规范化() {
        let res = |v: serde_json::Value| resources_for(&v, None, None);
        assert_eq!(
            res(serde_json::json!({"url": "http://localhost:5173/app/"})),
            vec!["url:localhost:5173/app"]
        );
        assert_eq!(
            res(serde_json::json!({"url": "http://127.0.0.1:4173"})),
            vec!["url:127.0.0.1:4173"]
        );
        assert_eq!(
            res(serde_json::json!({"url": "http://[::1]:8080/x"})),
            vec!["url:[::1]:8080/x"]
        );
        assert_eq!(
            res(serde_json::json!({"url": "https://example.com/"})),
            vec!["url:example.com"]
        );
        // userinfo 伪装:真正的 host 是 evil.com。
        assert_eq!(
            res(serde_json::json!({"url": "http://localhost:80@evil.com/x"})),
            vec!["url:evil.com/x"]
        );
        assert!(res(serde_json::json!({"html": "<p>x</p>"}))[0].starts_with("html:"));
        assert_eq!(res(serde_json::json!({"html": "<p>x</p>"}))[0].len(), 13);
        let with_cwd = resources_for(
            &serde_json::json!({"path": "a.html"}),
            Some(Path::new("C:/proj")),
            None,
        );
        assert!(with_cwd[0].starts_with("path:C:/proj"), "{with_cwd:?}");
        // 不带目标:取当前页;没有当前页记 page:none。
        assert_eq!(
            resources_for(
                &serde_json::json!({"action": "click"}),
                None,
                Some("http://localhost:3000/login")
            ),
            vec!["url:localhost:3000/login"]
        );
        assert_eq!(
            res(serde_json::json!({"action": "click"})),
            vec!["page:none"]
        );
    }

    #[test]
    fn 当前页按后端分别记录() {
        set_current_url(Backend::Pane, Some("http://localhost:1/p".into()));
        set_current_url(Backend::Headless, Some("http://localhost:2/h".into()));
        assert_eq!(
            current_url(Backend::Pane).as_deref(),
            Some("http://localhost:1/p")
        );
        assert_eq!(
            current_url(Backend::Headless).as_deref(),
            Some("http://localhost:2/h")
        );
        set_current_url(Backend::Pane, None);
        assert_eq!(current_url(Backend::Pane), None);
    }

    #[test]
    fn walker表达式由共用源生成() {
        let expression = dom_walker_expression(Some("#app"));
        assert!(expression.contains("function domWalker(sel)"));
        assert!(!expression.contains("export function"), "export 必须去掉");
        assert!(expression.ends_with("return domWalker(\"#app\");\n})()"));
        assert!(dom_walker_expression(None).contains("return domWalker(null);"));
    }

    #[test]
    fn 输出首行标明后端且格式两端同形() {
        let pane = out_click(Backend::Pane, "#go", "http://localhost/");
        let headless = out_click(Backend::Headless, "#go", "http://localhost/");
        assert!(pane.content.starts_with("backend: pane（用户可见）\n"));
        assert!(headless.content.starts_with("backend: headless\n"));
        assert_eq!(
            pane.content.lines().nth(1),
            headless.content.lines().nth(1),
            "除首行外逐字同形"
        );
        let big = out_open(
            Backend::Pane,
            "t",
            "u",
            "v",
            &[],
            "x".repeat(MAX_SCREENSHOT_BYTES + 1),
        );
        assert!(big.is_error && big.images.is_empty());
        let eval = out_eval(Backend::Headless, &"a".repeat(MAX_EVAL_CHARS + 10), "u");
        assert!(eval.content.contains("已截断"));
    }

    #[test]
    fn console错误保留来源url与行列_all模式含普通日志() {
        let error = ConsoleItem {
            level: "error".into(),
            text: "Failed to load resource: 404".into(),
            url: Some("http://127.0.0.1:4173/favicon.ico".into()),
            line: Some(1),
            col: Some(1),
        };
        assert!(format_console_item(&error).contains("favicon.ico:1:1"));
        let log = ConsoleItem {
            level: "log".into(),
            text: "hello".into(),
            ..ConsoleItem::default()
        };
        assert_eq!(format_console_item(&log), "[log] hello");

        let only_log = out_console(Backend::Headless, "u", std::slice::from_ref(&log), false);
        assert!(!only_log.is_error && only_log.content.contains("无错误/警告"));
        let all_logs = out_console(Backend::Headless, "u", std::slice::from_ref(&log), true);
        assert!(!all_logs.is_error && all_logs.content.contains("[log] hello"));
        let mixed = out_console(Backend::Pane, "u", &[log, error], true);
        assert!(mixed.is_error, "含错误即判工具错误");
        assert!(mixed.content.contains("错误/警告 1 条"));
    }

    #[test]
    fn 失败输出给出前端服务诊断() {
        let no_open = browser_error(Backend::Headless, "no browser: call open first");
        assert!(no_open.is_error);
        assert!(no_open.content.contains("先调用 browser open"));
        let refused = browser_error(Backend::Pane, "page.goto: net::ERR_CONNECTION_REFUSED");
        assert!(refused.content.starts_with("backend: pane"));
        assert!(refused.content.contains("process list/output/wait"));
        assert!(refused.content.contains("Local URL"));
    }
}
