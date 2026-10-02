//! browser 工具的无头后端(R-269):playwright-core 辅进程。
//!
//! 协议:JSON-RPC over stdio,单行 JSON 请求/响应,id 配对。Node 侧是权威数据源
//! (browser/page 实例),Rust 侧是客户端 + 生命周期管理。
//!
//! # 生命周期(不变式:不留僵尸 headless)
//! - 辅进程单例:多次调用复用同一个 Node 进程与 browser;
//! - 空闲超时回收:每次调用后刷新 last_used,后台线程在空闲超过预算时发
//!   `shutdown` 并等进程退出(reaper 常驻,shutdown 后继续监控);
//! - 工具关闭即收尾:Drop 时 kill + wait(兜底),保证不留进程。
//!
//! # 辅助脚本位置(UI2-0926 #8)
//! 依次找:环境变量 `KANZEI_BROWSER_HELPER` → `<exe 所在目录>/scripts/browser-helper.mjs`
//! (安装包的 bundle resource)→ 编译期仓库路径。缺失时报错写全三处。
//!
//! # 缺依赖诊断
//! 无 Node / 无 Edge/Chrome / playwright-core 未装:明确报错并给出修复指引,不静默降级。

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kanzei_harness::ToolOutput;

use super::{
    browser_error, out_click, out_console, out_dom, out_eval, out_open, out_press, out_screenshot,
    out_scroll, out_type, out_wait, parse_viewport, resolve_nav_target, screenshot_scope,
    scroll_scope, set_current_url, validate, viewport_label, wait_scope, Backend, BrowserAction,
    BrowserInput, ConsoleItem, HEADLESS_PANE_HINT, MAX_WAIT_MS,
};

/// 辅进程空闲回收预算:超过这个时间没有调用就 shutdown。
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// RPC 调用超时(含 browser launch 首次冷启动)。
const RPC_TIMEOUT: Duration = Duration::from_secs(60);

/// 无头后端入口(CLI 的 browser 与桌面端的路由兜底共用)。
///
/// `pane_hint`:桌面端传 true,在 open/screenshot 结果里提示用户可以在预览面板里看。
pub async fn execute_headless(
    input: BrowserInput,
    ctx: &kanzei_harness::ToolCtx,
    pane_hint: bool,
) -> ToolOutput {
    let backend = Backend::Headless;
    let action = match BrowserAction::parse(&input.action) {
        Ok(action) => action,
        Err(error) => return browser_error(backend, &error),
    };
    if let Err(error) = validate(&input, action) {
        return browser_error(backend, &error);
    }
    let target = match resolve_nav_target(&input, &ctx.cwd, &ctx.project_root) {
        Ok(target) => target,
        Err(error) => return browser_error(backend, &error),
    };
    start_idle_reaper();
    let mut notes: Vec<String> = target.iter().filter_map(|t| t.note.clone()).collect();
    if pane_hint {
        notes.push(HEADLESS_PANE_HINT.to_string());
    }
    let result = with_helper(|helper| run_action(helper, &input, action, target, &notes));
    match result {
        Ok((output, url)) => {
            if let Some(url) = url {
                set_current_url(backend, Some(url));
            }
            output
        }
        Err(error) => browser_error(backend, &error),
    }
}

fn run_action(
    helper: &mut HelperProcess,
    input: &BrowserInput,
    action: BrowserAction,
    target: Option<super::NavTarget>,
    notes: &[String],
) -> Result<(ToolOutput, Option<String>), String> {
    let backend = Backend::Headless;
    let viewport = parse_viewport(input.viewport.as_deref())
        .map(|(width, height)| serde_json::json!({ "width": width, "height": height }));
    let viewport_text = viewport_label(input.viewport.as_deref());
    let mut opened: Option<(String, String)> = None;
    if let Some(target) = &target {
        let result = helper.rpc(
            "open",
            serde_json::json!({ "url": target.url, "channel": input.channel, "viewport": viewport }),
        )?;
        opened = Some((
            result["title"].as_str().unwrap_or("").to_string(),
            result["url"].as_str().unwrap_or(&target.url).to_string(),
        ));
    }
    if let Some(scheme) = &input.color_scheme {
        helper.rpc("emulateMedia", serde_json::json!({ "colorScheme": scheme }))?;
    }
    let page_url = |value: &serde_json::Value| -> String {
        value["url"]
            .as_str()
            .map(str::to_string)
            .or_else(|| opened.as_ref().map(|(_, url)| url.clone()))
            .unwrap_or_else(|| "(current page)".into())
    };
    let selector = input.selector.clone().unwrap_or_default();
    Ok(match action {
        BrowserAction::Open => {
            let shot = helper.rpc("screenshot", serde_json::json!({ "viewport": viewport }))?;
            let (title, url) = opened.clone().unwrap_or_default();
            let png = png_field(&shot)?;
            (
                out_open(backend, &title, &url, &viewport_text, notes, png),
                Some(url),
            )
        }
        BrowserAction::Screenshot => {
            let shot = helper.rpc(
                "screenshot",
                serde_json::json!({
                    "viewport": viewport,
                    "fullPage": input.full_page,
                    "selector": input.selector,
                }),
            )?;
            let url = page_url(&shot);
            let scope = screenshot_scope(input);
            let png = png_field(&shot)?;
            (
                out_screenshot(backend, &url, &viewport_text, &scope, notes, png),
                Some(url),
            )
        }
        BrowserAction::Dom => {
            let dom = helper.rpc("dom", serde_json::json!({ "selector": selector }))?;
            let url = page_url(&dom);
            let structure = dom["dom"].as_str().unwrap_or("");
            (out_dom(backend, &url, &selector, structure), Some(url))
        }
        BrowserAction::Console => {
            let console = helper.rpc("console", serde_json::json!({ "all": input.all }))?;
            let url = page_url(&console);
            let entries: Vec<ConsoleItem> = console["errors"]
                .as_array()
                .map(|items| items.iter().map(console_item).collect())
                .unwrap_or_default();
            (out_console(backend, &url, &entries, input.all), Some(url))
        }
        BrowserAction::Click => {
            let result = helper.rpc("click", serde_json::json!({ "selector": selector }))?;
            let url = page_url(&result);
            (out_click(backend, &selector, &url), Some(url))
        }
        BrowserAction::Type => {
            let text = input.text.clone().unwrap_or_default();
            let result = helper.rpc(
                "type",
                serde_json::json!({ "selector": selector, "text": text }),
            )?;
            let url = page_url(&result);
            (
                out_type(backend, &selector, text.chars().count(), &url),
                Some(url),
            )
        }
        BrowserAction::Press => {
            let key = input.key.clone().unwrap_or_default();
            let result = helper.rpc("press", serde_json::json!({ "key": key }))?;
            let url = page_url(&result);
            (out_press(backend, &key, &url), Some(url))
        }
        BrowserAction::Scroll => {
            let result = helper.rpc(
                "scroll",
                serde_json::json!({ "selector": input.selector, "dy": input.dy }),
            )?;
            let url = page_url(&result);
            let what = scroll_scope(input);
            (
                out_scroll(backend, &what, result["scrollY"].as_f64(), &url),
                Some(url),
            )
        }
        BrowserAction::Wait => {
            let result = helper.rpc(
                "wait",
                serde_json::json!({
                    "selector": input.selector,
                    "text": input.text,
                    "ms": input.ms.map(|ms| ms.min(MAX_WAIT_MS)),
                }),
            )?;
            let url = page_url(&result);
            let what = wait_scope(input);
            let elapsed = result["elapsedMs"].as_u64().unwrap_or(0);
            (out_wait(backend, &what, elapsed, &url), Some(url))
        }
        BrowserAction::Eval => {
            let result = helper.rpc(
                "eval",
                serde_json::json!({ "expression": input.expression }),
            )?;
            let url = page_url(&result);
            let json = result["json"].as_str().unwrap_or("null");
            (out_eval(backend, json, &url), Some(url))
        }
    })
}

fn png_field(value: &serde_json::Value) -> Result<String, String> {
    value["png"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "截图响应缺 png 字段".to_string())
}

fn console_item(value: &serde_json::Value) -> ConsoleItem {
    ConsoleItem {
        level: value["type"].as_str().unwrap_or("?").to_string(),
        text: value["text"].as_str().unwrap_or("?").to_string(),
        url: value["url"].as_str().map(str::to_string),
        line: value["line"].as_u64(),
        col: value["column"].as_u64(),
    }
}

/// 辅进程句柄:子进程 + 写请求的 stdin + reader 线程推入的响应行。
/// D-400:stdout 由独立 reader 线程持续读(挂死时 recv_timeout 兜底,
/// 此前 read_line 阻塞使 60s 超时失效)。
pub(crate) struct HelperProcess {
    child: Child,
    stdin: ChildStdin,
    rx: std::sync::mpsc::Receiver<String>,
    next_id: u64,
}

impl Drop for HelperProcess {
    fn drop(&mut self) {
        // D-400:Drop 收尾 kill + wait 兜底,不留僵尸 headless。
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl HelperProcess {
    pub(crate) fn rpc(
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
        // recv_timeout 兜底挂死辅进程(D-400)。
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
            // 顶层 error 与嵌套 result.error 统查——失败必须透传为工具错误。
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

/// 全局辅进程注册表(单例)。
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

/// 空闲回收线程:空闲超过 IDLE_TIMEOUT 时发 shutdown 并等进程退出——不留僵尸 headless。
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
    set_current_url(Backend::Headless, None);
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
/// D-584:测试模拟"无 node"必须走这条注入缝,不得清进程级 PATH。
fn find_node_in(explicit: Option<String>, path: &str) -> Option<String> {
    if let Some(explicit) = explicit {
        if !explicit.is_empty() {
            return Some(explicit);
        }
    }
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
        let candidate = Path::new(dir).join(name);
        if candidate.is_file() {
            return Ok(candidate.display().to_string());
        }
    }
    Err(format!("{name} 不在 PATH"))
}

/// 编译期仓库根(`crates/kanzei-tools/../..`)。
fn repo_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

/// 辅助脚本候选,按优先级:环境变量 → 安装目录 bundle resource → 编译期仓库路径。
fn helper_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(explicit) = std::env::var("KANZEI_BROWSER_HELPER") {
        if !explicit.trim().is_empty() {
            candidates.push(PathBuf::from(explicit));
        }
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        candidates.push(dir.join("scripts").join("browser-helper.mjs"));
    }
    if let Some(root) = repo_root() {
        candidates.push(root.join("scripts").join("browser-helper.mjs"));
    }
    candidates
}

/// 纯判定:第一个存在的候选;都不在时报错写全每一处。
fn pick_helper(candidates: &[PathBuf]) -> Result<PathBuf, String> {
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .ok_or_else(|| {
            let tried = candidates
                .iter()
                .map(|c| format!("  - {}", c.display()))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "找不到浏览器辅助脚本 browser-helper.mjs。已按顺序查找(KANZEI_BROWSER_HELPER → 安装目录 scripts/ → 构建时仓库):\n{tried}"
            )
        })
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

    if guard.is_none() {
        let helper_script = pick_helper(&helper_candidates())?;
        let mut command = std::process::Command::new(&node);
        command
            .arg(&helper_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        // 安装目录下的脚本旁边没有 node_modules:把构建时仓库根交给 helper 兜底解析 playwright-core。
        if std::env::var_os("KANZEI_PLAYWRIGHT_ROOT").is_none() {
            if let Some(root) = repo_root().filter(|root| {
                root.join("node_modules")
                    .join("playwright-core")
                    .join("package.json")
                    .is_file()
            }) {
                command.env("KANZEI_PLAYWRIGHT_ROOT", root);
            }
        }
        crate::hide_console(&mut command);
        let mut child = command
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实 Edge 的用例共用一个辅进程单例,必须互斥。
    fn browser_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-browser-headless-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ctx(dir: &Path) -> kanzei_harness::ToolCtx {
        kanzei_harness::ToolCtx::new(dir.to_path_buf(), dir.to_path_buf())
    }

    fn act(action: &str) -> BrowserInput {
        BrowserInput {
            action: action.into(),
            ..BrowserInput::default()
        }
    }

    /// 缺 Node 诊断:无 KANZEI_NODE 且 PATH 为空时 find_node 必须返回 None。
    /// D-584:走注入缝,不改进程级 PATH。
    #[test]
    fn 缺node诊断明确() {
        let found = find_node_in(None, "");
        assert!(
            found.is_none(),
            "空 PATH 下 find_node 应返回 None,实得 {found:?}"
        );
        assert!(find_node_in(Some(String::new()), "").is_none());
    }

    /// 辅助脚本三级查找:取第一个存在的;都不在时报错列全。
    #[test]
    fn 辅助脚本按顺序查找且缺失时写全三处() {
        let dir = temp_dir("helper");
        let exe_scripts = dir.join("exe/scripts/browser-helper.mjs");
        let repo = dir.join("repo/scripts/browser-helper.mjs");
        std::fs::create_dir_all(repo.parent().unwrap()).unwrap();
        std::fs::write(&repo, "// repo").unwrap();
        let candidates = vec![
            dir.join("env/browser-helper.mjs"),
            exe_scripts.clone(),
            repo.clone(),
        ];
        assert_eq!(pick_helper(&candidates).unwrap(), repo);
        std::fs::create_dir_all(exe_scripts.parent().unwrap()).unwrap();
        std::fs::write(&exe_scripts, "// bundled").unwrap();
        assert_eq!(
            pick_helper(&candidates).unwrap(),
            exe_scripts,
            "安装目录优先于仓库"
        );
        let err = pick_helper(&candidates[..1]).unwrap_err();
        assert!(
            err.contains("KANZEI_BROWSER_HELPER") && err.contains("env"),
            "{err}"
        );
        // 真实候选里一定含编译期仓库路径,且那里的脚本存在(本仓测试时)。
        assert!(helper_candidates()
            .iter()
            .any(|c| c.ends_with("scripts/browser-helper.mjs") && c.is_file()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 无目标的非open动作允许省略目标() {
        let input = BrowserInput {
            action: "click".into(),
            selector: Some("#submit".into()),
            ..BrowserInput::default()
        };
        assert!(validate(&input, BrowserAction::Click).is_ok());
        assert_eq!(
            resolve_nav_target(&input, Path::new("."), Path::new(".")).unwrap(),
            None
        );
    }

    /// D-718:真实 Edge 走完整包装层。旧实现会在 type/click/dom 前强制要求
    /// url/path 并重新导航，输入值在点击前已经丢失；该测试必须覆盖省略目标的连续动作。
    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn 连续open_type_click_dom复用当前页面状态() {
        let _guard = browser_lock().lock().await;
        let html = r#"<!doctype html><html><head><title>stateful-browser</title></head><body>
<input id="value"><button id="commit" onclick="document.getElementById('result').textContent=document.getElementById('value').value">commit</button>
<div id="result">empty</div><script>console.error('stateful diagnostic')</script></body></html>"#;
        let dir = temp_dir("stateful");
        std::fs::write(dir.join("kz-browser-stateful.html"), html).unwrap();
        let ctx = ctx(&dir);

        shutdown_helper();
        let before_open = execute_headless(
            BrowserInput {
                selector: Some("#result".into()),
                ..act("dom")
            },
            &ctx,
            false,
        )
        .await;
        let opened = execute_headless(
            BrowserInput {
                path: Some("kz-browser-stateful.html".into()),
                ..act("open")
            },
            &ctx,
            false,
        )
        .await;
        let typed = execute_headless(
            BrowserInput {
                selector: Some("#value".into()),
                text: Some("kept-state".into()),
                ..act("type")
            },
            &ctx,
            false,
        )
        .await;
        let clicked = execute_headless(
            BrowserInput {
                selector: Some("#commit".into()),
                ..act("click")
            },
            &ctx,
            false,
        )
        .await;
        let dom = execute_headless(
            BrowserInput {
                selector: Some("#result".into()),
                ..act("dom")
            },
            &ctx,
            false,
        )
        .await;
        let console = execute_headless(act("console"), &ctx, false).await;
        shutdown_helper();
        std::fs::remove_dir_all(&dir).ok();

        assert!(before_open.is_error, "未 open 不得假成功");
        assert!(before_open.content.contains("先调用 browser open"));
        assert!(!opened.is_error, "{}", opened.content);
        assert!(opened.content.starts_with("backend: headless\n"));
        assert!(
            opened.content.contains("http://127.0.0.1:"),
            "path 走静态服务"
        );
        assert!(!typed.is_error, "{}", typed.content);
        assert!(!clicked.is_error, "{}", clicked.content);
        assert!(!dom.is_error, "{}", dom.content);
        assert!(dom.content.contains("kept-state"), "{}", dom.content);
        assert!(console.is_error, "console.error 必须作为工具错误返回");
        assert!(
            console.content.contains("stateful diagnostic"),
            "{}",
            console.content
        );
        assert!(
            console.content.contains("kz-browser-stateful"),
            "{}",
            console.content
        );
    }

    /// UI2-0926 #8:screenshot 不重新导航(输入值还在)→ press Enter 提交表单 → wait 文字
    /// → eval 回 JSON;静态服务下 ES module 页面能渲染,file:// 对照白屏。
    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn 截图不重导航_按键提交_等待文字_eval_静态服务下es模块可渲染() {
        let _guard = browser_lock().lock().await;
        let dir = temp_dir("actions");
        std::fs::write(
            dir.join("index.html"),
            r#"<!doctype html><html><head><title>actions</title></head><body>
<form id="f" onsubmit="event.preventDefault(); document.getElementById('out').textContent = 'submitted:' + document.getElementById('q').value;">
<input id="q" name="q"></form><div id="out">idle</div><div id="mod">module-pending</div>
<script type="module">import { n } from './mod.mjs'; document.getElementById('mod').textContent = 'module-ok-' + n;</script>
</body></html>"#,
        )
        .unwrap();
        std::fs::write(dir.join("mod.mjs"), "export const n = 7;\n").unwrap();
        let ctx = ctx(&dir);
        let run = |input: BrowserInput| execute_headless(input, &ctx, true);

        shutdown_helper();
        let opened = run(BrowserInput {
            path: Some("index.html".into()),
            ..act("open")
        })
        .await;
        let typed = run(BrowserInput {
            selector: Some("#q".into()),
            text: Some("kept-state".into()),
            ..act("type")
        })
        .await;
        let shot = run(act("screenshot")).await;
        let dom = run(BrowserInput {
            selector: Some("#q".into()),
            ..act("dom")
        })
        .await;
        let pressed = run(BrowserInput {
            key: Some("Enter".into()),
            ..act("press")
        })
        .await;
        let waited = run(BrowserInput {
            text: Some("submitted:kept-state".into()),
            ms: Some(5000),
            ..act("wait")
        })
        .await;
        let evaluated = run(BrowserInput {
            expression: Some("({ a: 1, title: document.title })".into()),
            ..act("eval")
        })
        .await;
        let module = run(BrowserInput {
            selector: Some("#mod".into()),
            ..act("dom")
        })
        .await;
        // 对照:同一页面走 file:// 时 module 被 CORS 拦下,停在 pending。
        let file_url = format!(
            "file:///{}",
            crate::path_form::strip_verbatim(
                &std::fs::canonicalize(dir.join("index.html"))
                    .unwrap()
                    .to_string_lossy()
            )
            .replace('\\', "/")
        );
        let control = with_helper(|helper| {
            helper.rpc(
                "open",
                serde_json::json!({ "url": file_url, "channel": "msedge" }),
            )?;
            std::thread::sleep(Duration::from_millis(500));
            helper.rpc("dom", serde_json::json!({ "selector": "#mod" }))
        });
        shutdown_helper();
        std::fs::remove_dir_all(&dir).ok();

        assert!(!opened.is_error, "{}", opened.content);
        assert!(
            opened.content.contains(HEADLESS_PANE_HINT),
            "桌面端要提示可在面板里看"
        );
        assert!(!typed.is_error, "{}", typed.content);
        assert!(!shot.is_error, "{}", shot.content);
        assert_eq!(shot.images.len(), 1, "screenshot 必须带图");
        assert!(shot.content.contains("未重新导航"));
        assert!(
            dom.content.contains("kept-state"),
            "截图不得重新导航: {}",
            dom.content
        );
        assert!(!pressed.is_error, "{}", pressed.content);
        assert!(!waited.is_error, "{}", waited.content);
        assert!(
            evaluated.content.contains(r#""a":1"#),
            "{}",
            evaluated.content
        );
        assert!(
            evaluated.content.contains("actions"),
            "{}",
            evaluated.content
        );
        assert!(module.content.contains("module-ok-7"), "{}", module.content);
        let control = control.expect("file:// 对照页能打开");
        assert!(
            !control["dom"].as_str().unwrap_or("").contains("module-ok"),
            "file:// 下 module 应失败(对照组): {control}"
        );
    }

    /// D-400:rpc 统查 result.error(嵌套)——辅进程把所有错误(含 catch)写进
    /// result.error,失败必须透传为工具错误,不得报成功。
    #[test]
    fn rpc_嵌套result_error透传为工具错误() {
        let Some(node) = find_node() else {
            eprintln!("跳过:本机无 node");
            return;
        };
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
        let dir = temp_dir("fake-helper");
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
        drop(helper);
        std::fs::remove_dir_all(&dir).ok();
    }
}
