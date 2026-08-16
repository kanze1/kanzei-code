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
//!   `shutdown` 并等进程退出;
//! - 工具关闭即收尾:Drop 时发 shutdown + kill(兜底),保证不留进程。
//!
//! # 缺依赖诊断
//! 无 Node / 无 Edge/Chrome / playwright-core 未装:明确报错并给出修复指引,
//! 不静默降级。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
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
         as an image the model can actually see."
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
    /// screenshot / open: 移动 viewport 预设(mobile-375x667 等)。
    viewport: Option<String>,
    /// 浏览器 channel:msedge(默认) | chrome。
    #[serde(default = "default_channel")]
    channel: String,
}

fn default_channel() -> String {
    "msedge".into()
}

/// 辅进程句柄:子进程 + 写请求的 stdin + 读响应的 stdout。
struct HelperProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
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

        // 逐行读响应,直到 id 配对。
        let deadline = Instant::now() + RPC_TIMEOUT;
        loop {
            if Instant::now() > deadline {
                return Err("RPC 超时:辅进程未在预算内响应".into());
            }
            let mut buf = String::new();
            let read = self
                .stdout
                .read_line(&mut buf)
                .map_err(|e| format!("读辅进程响应失败: {e}"))?;
            if read == 0 {
                return Err("辅进程已退出(可能浏览器启动失败或 Node 缺失)".into());
            }
            let parsed: serde_json::Value =
                serde_json::from_str(buf.trim()).map_err(|e| format!("响应不是 JSON: {e}"))?;
            if parsed["id"].as_u64() != Some(id) {
                continue; // 其他请求的响应(不应发生,单请求串行)
            }
            if let Some(err) = parsed["error"].as_str() {
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
                break;
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
    if let Ok(explicit) = std::env::var("KANZEI_NODE") {
        if !explicit.is_empty() {
            return Some(explicit);
        }
    }
    // PATH 探测 node.exe / node.cmd(pwsh 下 .cmd 也要试)。
    for name in ["node", "node.exe"] {
        if let Ok(path) = which(name) {
            return Some(path);
        }
    }
    None
}

fn which(name: &str) -> Result<String, String> {
    let path = std::env::var("PATH").unwrap_or_default();
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
        let stdout = BufReader::new(child.stdout.take().ok_or("辅进程 stdout 不可用")?);
        *guard = Some(HelperProcess {
            child,
            stdin,
            stdout,
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
    let url = match resolve_target(&input) {
        Ok(u) => u,
        Err(e) => return ToolOutput::error(e),
    };
    let viewport = parse_viewport(input.viewport.as_deref());

    match with_helper(|helper| {
        // open:打开目标。
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
        Err(e) => ToolOutput::error(format!("浏览器工具失败: {e}")),
    }
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
            "url": { "type": "string", "description": "目标 http(s) URL" },
            "path": { "type": "string", "description": "本地 HTML 文件路径(file:// 自动转换)" },
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
        })
        .unwrap_err();
        assert!(err.contains("url 或 path"), "{err}");

        // 非法 url 被拒。
        let err = resolve_target(&BrowserInput {
            url: Some("ftp://x".into()),
            path: None,
            viewport: None,
            channel: "msedge".into(),
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
        })
        .unwrap();
        assert!(url.starts_with("file:///"), "本地文件应转 file://: {url}");
        assert!(url.contains("browser_tool.rs"), "{url}");
    }

    /// 缺 Node 诊断:构造一个不可能存在的 Node 路径,工具必须给明确指引而非静默。
    #[test]
    fn 缺node诊断明确() {
        // 通过临时替换 PATH 模拟无 node——不易在测试里改全局 PATH,这里验证
        // find_node 在空 PATH 下返回 None(→ with_helper 报缺 Node)。
        let empty_path = std::env::var_os("PATH").unwrap_or_default();
        // 保存后临时置空,测完恢复。
        std::env::remove_var("KANZEI_NODE");
        std::env::set_var("PATH", "");
        let found = find_node();
        std::env::set_var("PATH", empty_path);
        assert!(
            found.is_none(),
            "空 PATH 下 find_node 应返回 None,实得 {found:?}"
        );
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
}
