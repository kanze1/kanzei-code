//! 浏览器工具(R-269):playwright-core 辅进程 headless 自检通道。
//!
//! Rust 侧经 stdio 起 `scripts/browser-helper.mjs`(Node 辅进程),playwright-core
//! 以 channel 模式自 launch 本机 Edge/Chrome headless(不下载浏览器二进制)。
//! 自 launch 实例不碰 WebView2,天然绕开 D-319(WebView2 DevTools 端口不监听)。
//!
//! 协议:JSON-RPC over stdio,单行 JSON 请求/响应,id 配对。Node 侧是权威数据源
//! (browser/page 实例),Rust 侧是客户端 + 生命周期管理。
//!
//! # 生命周期(不变式:不留僵尸 headless)
//! - 辅进程单例:同一 project 的多次调用复用同一个 Node 进程与 browser;
//! - 空闲超时回收:每次调用后刷新 last_used,后台线程在空闲超过预算时发
//!   `shutdown` 并等进程退出(reaper 常驻,shutdown 后继续监控);
//! - 工具关闭即收尾:Drop 时 kill + wait(兜底),保证不留进程。
//!
//! # 缺依赖诊断
//! 无 Node / 无 Edge/Chrome / playwright-core 未装:明确报错并给出修复指引,
//! 不静默降级。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kanzei_harness::ToolOutput;
use serde::Deserialize;

/// R-269 浏览器工具:playwright-core 辅进程 headless 自检通道。
///
/// 能力(按批次):批1 open(URL/本地文件)+ screenshot(含移动 viewport,图片经
/// ToolOutput.images 回模型);批2 dom/console;批3 click/type。
pub(crate) struct BrowserTool;

#[async_trait::async_trait]
impl kanzei_harness::Tool for BrowserTool {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn description(&self) -> String {
        "Open a URL or local HTML file in a headless browser (playwright-core channel mode, \
         uses your installed Edge/Chrome — never WebView2, so it sidesteps D-319) and return \
         a screenshot through the image channel. Params: url or path; optional viewport \
         (mobile-375x667 | mobile-390x844 | mobile-412x915 | mobile-360x800) for mobile UI \
         self-checks; optional channel (msedge default | chrome). The screenshot is delivered \
         as an image the model can actually see. Stateful flow: call open with url/path once, \
         then call dom/console/click/type without url/path to keep the current page state. \
         Supplying url/path to an action explicitly reloads that target before the action."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        input_schema()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        let target = input["path"]
            .as_str()
            .map(|p| format!("path:{p}"))
            .or_else(|| input["url"].as_str().map(|u| format!("url:{u}")))
            .unwrap_or_else(|| "*".into());
        vec![target]
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
        let _ = ctx; // 辅进程按单例管理,不依赖 cwd/project_root 分区(仓库级浏览器)。
        let parsed: BrowserInput = match serde_json::from_value(input) {
            Ok(v) => v,
            Err(e) => {
                return ToolOutput::error(format!(
                    "browser 参数解析失败: {e}。需要 url 或 path;可选 viewport/channel"
                ))
            }
        };
        start_idle_reaper();
        execute_browser(parsed).await
    }
}

/// 辅进程空闲回收预算:超过这个时间没有调用就 shutdown。
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// RPC 调用超时(含 browser launch 首次冷启动)。
const RPC_TIMEOUT: Duration = Duration::from_secs(60);
/// 单次截图体积上限(与 R-249 截图口径一致,防超大 base64 打爆上下文)。
const MAX_SCREENSHOT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
pub(crate) struct BrowserInput {
    /// open: 目标 URL 或本地文件路径。
    url: Option<String>,
    /// open: 本地文件路径(file:// 自动转换)。
    path: Option<String>,
    /// 动作:open(默认,打开并截图)| dom(读可读结构)| console(读页面错误)
    /// | click(点元素)| type(填输入框)。
    #[serde(default = "default_action")]
    action: String,
    /// dom / click / type: CSS selector。
    selector: Option<String>,
    /// type: 要填入的文本。
    text: Option<String>,
    /// screenshot / open: 移动 viewport 预设(mobile-375x667 等)。
    viewport: Option<String>,
    /// 浏览器 channel:msedge(默认) | chrome。
    #[serde(default = "default_channel")]
    channel: String,
}

fn default_channel() -> String {
    "msedge".into()
}

fn default_action() -> String {
    "open".into()
}

/// 辅进程句柄:子进程 + 写请求的 stdin + reader 线程推入的响应行。
/// D-400:stdout 由独立 reader 线程持续读(挂死时 recv_timeout 兜底,
/// 此前 read_line 阻塞使 60s 超时失效)。
struct HelperProcess {
    child: Child,
    stdin: ChildStdin,
    rx: std::sync::mpsc::Receiver<String>,
    next_id: u64,
}

impl Drop for HelperProcess {
    fn drop(&mut self) {
        // D-400:模块头注释声称「Drop 收尾」但此前无实现——补上 kill + wait 兜底,
        // 不留僵尸 headless(注释已与实现对齐)。
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl HelperProcess {
    fn rpc(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let req = serde_json::json!({ "id": id, "method": method, "params": params });
        let mut line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .map_err(|e| format!("写入辅进程失败: {e}"))?;
        self.stdin.flush().map_err(|e| format!("flush 失败: {e}"))?;

        // 逐行读响应,直到 id 配对。reader 线程持续读 stdout 推入 channel,
        // recv_timeout 兜底挂死辅进程(D-400:此前 read_line 阻塞使超时失效)。
        let deadline = Instant::now() + RPC_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("RPC 超时:辅进程未在预算内响应".into());
            }
            let line = match self.rx.recv_timeout(remaining) {
                Ok(line) => line,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err("RPC 超时:辅进程未在预算内响应".into());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("辅进程已退出(可能浏览器启动失败或 Node 缺失)".into());
                }
            };
            let parsed: serde_json::Value =
                serde_json::from_str(line.trim()).map_err(|e| format!("响应不是 JSON: {e}"))?;
            if parsed["id"].as_u64() != Some(id) {
                continue; // 其他请求的响应(不应发生,单请求串行)
            }
            // D-400:辅进程把所有错误(含 catch)写进 result.error(嵌套于 result),
            // 顶层 error 与嵌套 result.error 统查——click/type/open 失败必须透传为工具错误,
            // 不得报成功(此前只查顶层 parsed["error"],永远查不到,交互断言全面假绿)。
            if let Some(err) = parsed["error"]
                .as_str()
                .or_else(|| parsed["result"]["error"].as_str())
            {
                return Err(err.to_string());
            }
            return Ok(parsed["result"].clone());
        }
    }
}

/// 全局辅进程注册表(单例,按 project_root 键控)。
struct HelperRegistry {
    process: Mutex<Option<HelperProcess>>,
    last_used: AtomicU64,
    shutting_down: AtomicBool,
}

impl HelperRegistry {
    fn new() -> Self {
        HelperRegistry {
            process: Mutex::new(None),
            last_used: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
        }
    }

    fn touch(&self) {
        self.last_used.store(now_ms() as u64, Ordering::SeqCst);
    }
}

// 用 SystemTime 记 last_used(ms)。
fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

/// 空闲回收线程:每次调用后刷新 last_used;后台线程在空闲超过 IDLE_TIMEOUT 时
/// 发 shutdown 并等进程退出,然后清空注册表——不留僵尸 headless。
pub(crate) fn start_idle_reaper() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(IDLE_TIMEOUT / 2);
            let reg = registry();
            if reg.shutting_down.load(Ordering::SeqCst) {
                // D-400:shutdown 期间跳过即可,不 break——Once 只执行一次,
                // break 后 reaper 永久死亡,后续空闲进程不再被回收。
                continue;
            }
            let last = reg.last_used.load(Ordering::SeqCst) as u128;
            if last > 0 && now_ms().saturating_sub(last) > IDLE_TIMEOUT.as_millis() {
                shutdown_helper();
            }
        });
    });
}

/// 关闭辅进程(发 shutdown + 兜底 kill)。幂等。
pub(crate) fn shutdown_helper() {
    let reg = registry();
    reg.shutting_down.store(true, Ordering::SeqCst);
    let mut guard = match reg.process.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if let Some(helper) = guard.as_mut() {
        let _ = helper.rpc("shutdown", serde_json::json!({}));
        let _ = helper.child.kill();
        let _ = helper.child.wait();
    }
    *guard = None;
    reg.shutting_down.store(false, Ordering::SeqCst);
}

static REGISTRY: std::sync::OnceLock<HelperRegistry> = std::sync::OnceLock::new();

fn registry() -> &'static HelperRegistry {
    REGISTRY.get_or_init(HelperRegistry::new)
}

/// 找 Node 可执行文件:环境变量或 PATH。
fn find_node() -> Option<String> {
    find_node_in(
        std::env::var("KANZEI_NODE").ok(),
        &std::env::var("PATH").unwrap_or_default(),
    )
}

/// 纯函数内核:显式路径(KANZEI_NODE)优先,否则按给定 PATH 探测。
/// D-584:测试模拟"无 node"必须走这条注入缝,不得清进程级 PATH——
/// cargo test 同进程多线程,清 PATH 会让并行测试按名拉起 git/node 时误报 not found。
fn find_node_in(explicit: Option<String>, path: &str) -> Option<String> {
    if let Some(explicit) = explicit {
        if !explicit.is_empty() {
            return Some(explicit);
        }
    }
    // PATH 探测 node.exe / node.cmd(pwsh 下 .cmd 也要试)。
    for name in ["node", "node.exe"] {
        if let Ok(found) = which_in(path, name) {
            return Some(found);
        }
    }
    None
}

fn which_in(path: &str, name: &str) -> Result<String, String> {
    for dir in path.split(';') {
        if dir.is_empty() {
            continue;
        }
        let candidate = std::path::Path::new(dir).join(name);
        if candidate.is_file() {
            return Ok(candidate.display().to_string());
        }
    }
    Err(format!("{name} 不在 PATH"))
}

/// 获取(必要时启动)辅进程,返回可变句柄。上锁期间做 RPC,保证单请求串行。
fn with_helper<T>(f: impl FnOnce(&mut HelperProcess) -> Result<T, String>) -> Result<T, String> {
    let reg = registry();
    let mut guard = reg.process.lock().map_err(|_| "辅进程锁中毒".to_string())?;
    let node = find_node().ok_or_else(|| {
        "未找到 Node.js:浏览器工具需要 Node 运行 playwright-core 辅进程。\
         请安装 Node.js 并确保 `node` 在 PATH 中,或设置 KANZEI_NODE 指向 node 可执行文件。"
            .to_string()
    })?;

    // 已有进程:检查是否还活着(读 0 字节 = 退出)。
    if guard.is_none() {
        let helper_script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("scripts/browser-helper.mjs"))
            .ok_or_else(|| "无法定位 scripts/browser-helper.mjs".to_string())?;
        if !helper_script.is_file() {
            return Err(format!(
                "辅进程脚本缺失: {}。仓库结构异常或未拉取 scripts/。",
                helper_script.display()
            ));
        }
        let mut child = std::process::Command::new(&node)
            .arg(&helper_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("启动 Node 辅进程失败: {e}"))?;
        let stdin = child.stdin.take().ok_or("辅进程 stdin 不可用")?;
        // D-400:reader 线程持续读 stdout 推入 channel(挂死兜底;stdout 所有权移入线程)。
        let stdout = child.stdout.take().ok_or("辅进程 stdout 不可用")?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break; // rpc 侧已丢弃(进程被回收)。
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        *guard = Some(HelperProcess {
            child,
            stdin,
            rx,
            next_id: 0,
        });
    }
    reg.touch();
    let result = f(guard.as_mut().expect("刚确保 Some"));
    reg.touch();
    result
}

/// 浏览器工具的 execute 入口(供 Tool trait 调用)。
pub(crate) async fn execute_browser(input: BrowserInput) -> ToolOutput {
    match input.action.as_str() {
        "dom" => execute_dom(&input).await,
        "console" => execute_console(&input).await,
        "click" => execute_click(&input).await,
        "type" => execute_type(&input).await,
        _ => execute_open(&input).await,
    }
}

/// open(默认):打开目标并截图,图片经 ToolOutput.images 回模型。
async fn execute_open(input: &BrowserInput) -> ToolOutput {
    let url = match resolve_target(input) {
        Ok(u) => u,
        Err(e) => return ToolOutput::error(e),
    };
    let viewport = parse_viewport(input.viewport.as_deref());

    match with_helper(|helper| {
        let open_result = helper.rpc(
            "open",
            serde_json::json!({
                "url": url,
                "channel": input.channel,
                "viewport": viewport,
            }),
        )?;
        // 同一批调用里连带截图(open 即出图,自检通道的主消费形态)。
        let screenshot = helper.rpc("screenshot", serde_json::json!({ "viewport": viewport }))?;
        let title = open_result["title"].as_str().unwrap_or("").to_string();
        let page_url = open_result["url"].as_str().unwrap_or(&url).to_string();
        let png_b64 = screenshot["png"]
            .as_str()
            .ok_or("截图响应缺 png 字段")?
            .to_string();
        Ok((title, page_url, png_b64))
    }) {
        Ok((title, page_url, png_b64)) => {
            if png_b64.len() > MAX_SCREENSHOT_BYTES {
                return ToolOutput::error(format!(
                    "截图过大({} base64 字节 > {} 上限),未回喂模型",
                    png_b64.len(),
                    MAX_SCREENSHOT_BYTES
                ));
            }
            let mut output = ToolOutput::ok(format!(
                "浏览器已打开并截图:\ntitle: {title}\nurl: {page_url}\nviewport: {}",
                viewport_label(input.viewport.as_deref())
            ));
            output = output.with_images(vec![kanzei_harness::ToolImage {
                media_type: "image/png".into(),
                data: png_b64,
            }]);
            output
        }
        Err(e) => browser_error(e),
    }
}

/// dom:读页面可读结构(可选 selector,默认 body 整树)。
async fn execute_dom(input: &BrowserInput) -> ToolOutput {
    let target = match resolve_optional_target(input) {
        Ok(target) => target,
        Err(e) => return ToolOutput::error(e),
    };
    let viewport = parse_viewport(input.viewport.as_deref());
    let selector = input.selector.clone().unwrap_or_default();

    match with_helper(|helper| {
        let opened = open_if_target(helper, target.as_deref(), input, viewport.as_ref())?;
        let dom = helper.rpc("dom", serde_json::json!({ "selector": selector }))?;
        let structure = dom["dom"].as_str().unwrap_or("").to_string();
        let page_url = dom["url"]
            .as_str()
            .or(opened.as_deref())
            .unwrap_or("(current page)")
            .to_string();
        Ok((page_url, structure))
    }) {
        Ok((url, structure)) => {
            if structure.is_empty() {
                return ToolOutput::error(format!("dom 读取为空(selector: {selector:?})"));
            }
            ToolOutput::ok(format!("页面 DOM 结构(url: {url}):\n{structure}"))
        }
        Err(e) => browser_error(e),
    }
}

/// console:读页面累积的 console 错误/警告。
async fn execute_console(input: &BrowserInput) -> ToolOutput {
    let target = match resolve_optional_target(input) {
        Ok(target) => target,
        Err(e) => return ToolOutput::error(e),
    };
    let viewport = parse_viewport(input.viewport.as_deref());

    match with_helper(|helper| {
        let opened = open_if_target(helper, target.as_deref(), input, viewport.as_ref())?;
        let console = helper.rpc("console", serde_json::json!({}))?;
        let errors = console["errors"].as_array().cloned().unwrap_or_default();
        let page_url = console["url"]
            .as_str()
            .or(opened.as_deref())
            .unwrap_or("(current page)")
            .to_string();
        Ok((page_url, errors))
    }) {
        Ok((url, errors)) => {
            if errors.is_empty() {
                return ToolOutput::ok(format!("页面 console 无错误/警告(url: {url})"));
            }
            let lines = errors
                .iter()
                .map(|e| {
                    let ty = e["type"].as_str().unwrap_or("?");
                    let text = e["text"].as_str().unwrap_or("?");
                    format!("[{ty}] {text}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            ToolOutput::error(format!(
                "页面 console 错误/警告 {} 条(url: {url}):\n{lines}",
                errors.len()
            ))
        }
        Err(e) => browser_error(e),
    }
}

/// click:点击页面元素(selector 必填)。
async fn execute_click(input: &BrowserInput) -> ToolOutput {
    let target = match resolve_optional_target(input) {
        Ok(target) => target,
        Err(e) => return ToolOutput::error(e),
    };
    let selector = match input.selector.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return ToolOutput::error("click 需要 selector 参数".to_string()),
    };
    let viewport = parse_viewport(input.viewport.as_deref());

    match with_helper(|helper| {
        let opened = open_if_target(helper, target.as_deref(), input, viewport.as_ref())?;
        let result = helper.rpc("click", serde_json::json!({ "selector": selector }))?;
        let page_url = result["url"]
            .as_str()
            .or(opened.as_deref())
            .unwrap_or("(current page)")
            .to_string();
        Ok(page_url)
    }) {
        Ok(page_url) => ToolOutput::ok(format!("已点击 {selector:?};当前 url: {page_url}")),
        Err(e) => browser_error(e),
    }
}

/// type:向输入框填入文本(selector + text 必填)。
async fn execute_type(input: &BrowserInput) -> ToolOutput {
    let target = match resolve_optional_target(input) {
        Ok(target) => target,
        Err(e) => return ToolOutput::error(e),
    };
    let selector = match input.selector.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return ToolOutput::error("type 需要 selector 参数".to_string()),
    };
    let text = match input.text.as_deref() {
        Some(t) => t.to_string(),
        None => return ToolOutput::error("type 需要 text 参数".to_string()),
    };
    let viewport = parse_viewport(input.viewport.as_deref());

    match with_helper(|helper| {
        let opened = open_if_target(helper, target.as_deref(), input, viewport.as_ref())?;
        let result = helper.rpc(
            "type",
            serde_json::json!({ "selector": selector, "text": text }),
        )?;
        let page_url = result["url"]
            .as_str()
            .or(opened.as_deref())
            .unwrap_or("(current page)")
            .to_string();
        Ok(page_url)
    }) {
        Ok(page_url) => ToolOutput::ok(format!(
            "已向 {selector:?} 填入文本({} 字符);当前 url: {page_url}",
            text.chars().count(),
        )),
        Err(e) => browser_error(e),
    }
}

/// url/path 是可选导航目标。open 必须提供目标；其它动作省略目标时复用当前页，
/// 这是表单连续交互不丢状态的关键语义。
fn resolve_optional_target(input: &BrowserInput) -> Result<Option<String>, String> {
    if input.path.is_none() && input.url.is_none() {
        return Ok(None);
    }
    resolve_target(input).map(Some)
}

fn open_if_target(
    helper: &mut HelperProcess,
    target: Option<&str>,
    input: &BrowserInput,
    viewport: Option<&serde_json::Value>,
) -> Result<Option<String>, String> {
    let Some(url) = target else {
        return Ok(None);
    };
    let opened = helper.rpc(
        "open",
        serde_json::json!({
            "url": url,
            "channel": input.channel,
            "viewport": viewport,
        }),
    )?;
    Ok(Some(opened["url"].as_str().unwrap_or(url).to_string()))
}

fn browser_error(error: String) -> ToolOutput {
    let hint = if error.contains("no browser: call open first") {
        "先调用 browser open 并提供 url/path；后续 click/type/dom/console 可省略目标以复用当前页面。"
    } else if error.contains("ERR_CONNECTION_REFUSED")
        || error.contains("ERR_HTTP_RESPONSE_CODE_FAILURE")
        || error.contains("net::ERR_")
    {
        "先用 process list/output/wait 确认前端服务仍在运行，再按启动日志中的实际 Local URL 与端口访问；不要假设固定为 5173 或 4173。"
    } else {
        "可先用 browser console 和 process output 获取页面与服务端诊断。"
    };
    ToolOutput::error(format!("浏览器工具失败: {error}\n处理建议: {hint}"))
}

/// 解析目标:path(本地文件)转 file:// URL;url 原样。
fn resolve_target(input: &BrowserInput) -> Result<String, String> {
    if let Some(path) = &input.path {
        let abs = std::fs::canonicalize(path)
            .map_err(|e| format!("本地文件不存在或无法解析: {path} ({e})"))?;
        // canonicalize 在 Windows 返回 `\\?\C:\...`(扩展长度前缀),file:// URL
        // 不能带它——剥离后转 `/` 分隔。
        let raw = abs.display().to_string();
        let stripped = raw
            .strip_prefix(r"\\?\UNC\")
            .map(|rest| format!(r"\\{rest}"))
            .unwrap_or_else(|| raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string());
        return Ok(format!("file:///{}", stripped.replace('\\', "/")));
    }
    if let Some(url) = &input.url {
        if !(url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("file://"))
        {
            return Err(format!("url 必须 http(s):// 或 file://,实得: {url}"));
        }
        return Ok(url.clone());
    }
    Err("browser 需要 url 或 path 参数".into())
}

/// 内置移动 viewport 预设;未知名返回 None(用默认 1280x720)。
fn parse_viewport(name: Option<&str>) -> Option<serde_json::Value> {
    let (w, h) = match name {
        Some("mobile-375x667") => (375, 667), // iPhone SE 2 代
        Some("mobile-390x844") => (390, 844), // iPhone 12/13/14
        Some("mobile-412x915") => (412, 915), // Android Pixel 系
        Some("mobile-360x800") => (360, 800), // 小屏 Android
        _ => return None,
    };
    Some(serde_json::json!({ "width": w, "height": h }))
}

fn viewport_label(name: Option<&str>) -> String {
    match name {
        Some(n) => n.to_string(),
        None => "desktop-1280x720".into(),
    }
}

/// 工具输入 schema(与 Tool::input_schema 对接,手写 JSON Schema 避免 schemars
/// 依赖;字段与 BrowserInput 对齐)。
pub(crate) fn input_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "url": { "type": "string", "description": "目标 http(s) URL；open 必填，其他动作省略时复用当前页面，提供时先重新导航" },
            "path": { "type": "string", "description": "本地 HTML 文件路径(file:// 自动转换)；open 必填，其他动作省略时复用当前页面" },
            "action": {
                "type": "string",
                "enum": ["open", "dom", "console", "click", "type"],
                "description": "动作:open(默认,打开并截图回模型)| dom(读可读 DOM 结构,可选 selector)| console(读当前导航后的 console 错误/警告)| click(点击 selector 元素)| type(向 selector 输入框填入 text)。连续动作省略 url/path 即复用当前页"
            },
            "selector": { "type": "string", "description": "dom/click/type 用:CSS selector" },
            "text": { "type": "string", "description": "type 用:要填入输入框的文本" },
            "viewport": {
                "type": "string",
                "enum": ["mobile-375x667", "mobile-390x844", "mobile-412x915", "mobile-360x800"],
                "description": "移动 viewport 预设(默认桌面 1280x720)"
            },
            "channel": {
                "type": "string",
                "enum": ["msedge", "chrome"],
                "description": "浏览器 channel(默认 msedge)"
            }
        },
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 工具 schema 与输入结构对齐:url/path 至少其一、viewport/channel 枚举合法。
    #[test]
    fn schema_含url与path与viewport枚举() {
        let schema = input_schema();
        let props = &schema["properties"];
        assert!(props["url"].is_object(), "schema 必须有 url");
        assert!(props["path"].is_object(), "schema 必须有 path");
        let viewport_enum = props["viewport"]["enum"].as_array().unwrap();
        assert!(viewport_enum.iter().any(|v| v == "mobile-375x667"));
        assert!(viewport_enum.iter().any(|v| v == "mobile-412x915"));
        let channel_enum = props["channel"]["enum"].as_array().unwrap();
        assert!(channel_enum.iter().any(|v| v == "msedge"));
        assert!(channel_enum.iter().any(|v| v == "chrome"));
    }

    /// 目标解析:path 转 file:// URL,非法 url 被拒,缺参数报错。
    #[test]
    fn resolve_target_本地文件与url与缺参() {
        // 缺参报错。
        let err = resolve_target(&BrowserInput {
            url: None,
            path: None,
            viewport: None,
            channel: "msedge".into(),
            action: "open".into(),
            selector: None,
            text: None,
        })
        .unwrap_err();
        assert!(err.contains("url 或 path"), "{err}");

        // 非法 url 被拒。
        let err = resolve_target(&BrowserInput {
            url: Some("ftp://x".into()),
            path: None,
            viewport: None,
            channel: "msedge".into(),
            action: "open".into(),
            selector: None,
            text: None,
        })
        .unwrap_err();
        assert!(err.contains("http(s)"), "{err}");

        // path 转 file://(用本 crate 的 src/browser_tool.rs 做存在性验证)。
        let this_file =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/browser_tool.rs");
        let abs = std::fs::canonicalize(&this_file)
            .unwrap_or_else(|e| panic!("canonicalize {}: {e}", this_file.display()));
        let url = resolve_target(&BrowserInput {
            url: None,
            path: Some(abs.display().to_string()),
            viewport: None,
            channel: "msedge".into(),
            action: "open".into(),
            selector: None,
            text: None,
        })
        .unwrap();
        assert!(url.starts_with("file:///"), "本地文件应转 file://: {url}");
        assert!(url.contains("browser_tool.rs"), "{url}");
    }

    /// 缺 Node 诊断:无 KANZEI_NODE 且 PATH 为空时 find_node 必须返回 None
    /// (→ with_helper 报缺 Node)。D-584:走注入缝,不改进程级 PATH。
    #[test]
    fn 缺node诊断明确() {
        let found = find_node_in(None, "");
        assert!(
            found.is_none(),
            "空 PATH 下 find_node 应返回 None,实得 {found:?}"
        );
        // KANZEI_NODE 显式为空串时同样落回 PATH 探测。
        assert!(find_node_in(Some(String::new()), "").is_none());
    }

    /// 移动 viewport 预设解析:命中返回宽高,未知名返回 None(默认桌面)。
    #[test]
    fn viewport预设解析() {
        assert_eq!(
            parse_viewport(Some("mobile-390x844")),
            Some(serde_json::json!({ "width": 390, "height": 844 }))
        );
        assert_eq!(parse_viewport(Some("unknown")), None);
        assert_eq!(parse_viewport(None), None);
    }

    #[test]
    fn 非open动作允许省略目标且网络错误给前端服务诊断() {
        let input = BrowserInput {
            url: None,
            path: None,
            viewport: None,
            channel: "msedge".into(),
            action: "click".into(),
            selector: Some("#submit".into()),
            text: None,
        };
        assert_eq!(resolve_optional_target(&input).unwrap(), None);

        let no_open = browser_error("no browser: call open first".into());
        assert!(no_open.is_error);
        assert!(no_open.content.contains("先调用 browser open"));

        let refused = browser_error("page.goto: net::ERR_CONNECTION_REFUSED".into());
        assert!(refused.is_error);
        assert!(refused.content.contains("process list/output/wait"));
        assert!(refused.content.contains("Local URL"));
    }

    /// D-718:真实 Edge 走完整包装层。旧实现会在 type/click/dom 前强制要求
    /// url/path 并重新导航，输入值在点击前已经丢失；该测试必须覆盖省略目标的连续动作。
    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn 连续open_type_click_dom复用当前页面状态() {
        let html = r#"<!doctype html><html><head><title>stateful-browser</title></head><body>
<input id="value"><button id="commit" onclick="document.getElementById('result').textContent=document.getElementById('value').value">commit</button>
<div id="result">empty</div></body></html>"#;
        let path = std::env::temp_dir().join(format!(
            "kz-browser-stateful-{}-{}.html",
            std::process::id(),
            now_ms()
        ));
        std::fs::write(&path, html).unwrap();

        shutdown_helper();
        let before_open = execute_browser(BrowserInput {
            url: None,
            path: None,
            viewport: None,
            channel: "msedge".into(),
            action: "dom".into(),
            selector: Some("#result".into()),
            text: None,
        })
        .await;
        let opened = execute_browser(BrowserInput {
            url: None,
            path: Some(path.display().to_string()),
            viewport: None,
            channel: "msedge".into(),
            action: "open".into(),
            selector: None,
            text: None,
        })
        .await;
        let typed = execute_browser(BrowserInput {
            url: None,
            path: None,
            viewport: None,
            channel: "msedge".into(),
            action: "type".into(),
            selector: Some("#value".into()),
            text: Some("kept-state".into()),
        })
        .await;
        let clicked = execute_browser(BrowserInput {
            url: None,
            path: None,
            viewport: None,
            channel: "msedge".into(),
            action: "click".into(),
            selector: Some("#commit".into()),
            text: None,
        })
        .await;
        let dom = execute_browser(BrowserInput {
            url: None,
            path: None,
            viewport: None,
            channel: "msedge".into(),
            action: "dom".into(),
            selector: Some("#result".into()),
            text: None,
        })
        .await;
        shutdown_helper();
        std::fs::remove_file(&path).ok();

        assert!(before_open.is_error, "未 open 不得假成功");
        assert!(before_open.content.contains("先调用 browser open"));
        assert!(!opened.is_error, "{}", opened.content);
        assert!(!typed.is_error, "{}", typed.content);
        assert!(!clicked.is_error, "{}", clicked.content);
        assert!(!dom.is_error, "{}", dom.content);
        assert!(dom.content.contains("kept-state"), "{}", dom.content);
    }

    /// D-400:rpc 统查 result.error(嵌套)——辅进程把所有错误(含 catch)写进
    /// result.error,click/type/open 失败必须透传为工具错误,不得报成功
    /// (此前只查顶层 parsed["error"],永远查不到,交互断言全面假绿)。
    #[test]
    fn rpc_嵌套result_error透传为工具错误() {
        let Some(node) = find_node() else {
            eprintln!("跳过:本机无 node");
            return;
        };
        // 假 helper:读一行请求,回带嵌套 error 的响应(模拟 helper.mjs 的 catch 路径)。
        let script = r#"
            process.stdin.setEncoding("utf8");
            process.stdin.on("data", (chunk) => {
              for (const line of chunk.split("\n")) {
                const t = line.trim();
                if (!t) continue;
                const req = JSON.parse(t);
                process.stdout.write(JSON.stringify({ id: req.id, result: { error: "click failed: element not found" } }) + "\n");
              }
            });
        "#;
        let dir = std::env::temp_dir().join(format!("kz-browser-helper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("fake-helper.mjs");
        std::fs::write(&script_path, script).unwrap();
        let mut child = std::process::Command::new(&node)
            .arg(&script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("node 假 helper 启动");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if tx.send(line.unwrap_or_default()).is_err() {
                    break;
                }
            }
        });
        let mut helper = HelperProcess {
            child,
            stdin,
            rx,
            next_id: 0,
        };
        let err = helper
            .rpc("click", serde_json::json!({ "selector": "#x" }))
            .unwrap_err();
        assert!(
            err.contains("click failed"),
            "嵌套 result.error 必须透传为工具错误: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
