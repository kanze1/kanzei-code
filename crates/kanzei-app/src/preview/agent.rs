//! browser 工具的面板后端:同名动作经进程内 CDP 驱动用户眼前的预览面板。
//!
//! 输入解析、参数校验、目标解析(静态服务)与**全部输出格式**都用 kanzei-tools 的
//! `browser` 共用面,首行是 `backend: pane（用户可见）`,其余与无头后端同形。
//! B0 已验证:CDP 点击 / insertText / 按键都不抢系统焦点;返回值 returnByValue。

use std::time::Duration;

use kanzei_harness::{ToolCtx, ToolOutput};
use kanzei_tools::browser::{
    self as shared, Backend, BrowserAction, BrowserInput, ConsoleItem, MAX_WAIT_MS,
};
use serde_json::{json, Value};

use super::{cdp, pane, ColorScheme, DevicePreset, Pane};

/// 等主文档 load 的上限。
const LOAD_TIMEOUT: Duration = Duration::from_secs(15);
const BACKEND: Backend = Backend::Pane;

/// 在面板上执行一次 browser 动作。面板在路由之后被关掉、或隐藏超过 [`super::VISIBLE_WAIT`]
/// (不只是被菜单暂时遮住)时返回 None,由调用方回落无头。
pub(crate) async fn execute(input: &BrowserInput, ctx: &ToolCtx) -> Option<ToolOutput> {
    use tauri::Manager;
    let app = super::app()?;
    let state = app.try_state::<super::PreviewState>()?;
    let _serial = state.agent.lock().await;
    let pane = pane::current(app)?;
    if !pane::wait_visible(&pane, super::VISIBLE_WAIT).await {
        return None;
    }
    Some(run(app, &pane, input, ctx).await)
}

async fn run(
    app: &tauri::AppHandle,
    pane: &Pane,
    input: &BrowserInput,
    ctx: &ToolCtx,
) -> ToolOutput {
    let action = match BrowserAction::parse(&input.action) {
        Ok(action) => action,
        Err(error) => return shared::browser_error(BACKEND, &error),
    };
    if let Err(error) = shared::validate(input, action) {
        return shared::browser_error(BACKEND, &error);
    }
    let target = match shared::resolve_nav_target(input, &ctx.cwd, &ctx.project_root) {
        Ok(target) => target,
        Err(error) => return shared::browser_error(BACKEND, &error),
    };
    let mut notes: Vec<String> = target.iter().filter_map(|t| t.note.clone()).collect();
    // viewport / color_scheme 映射成面板的设备与深浅色(用户看得见这个变化)。
    let device = input
        .viewport
        .as_deref()
        .and_then(DevicePreset::from_viewport);
    let scheme = input.color_scheme.as_deref().and_then(ColorScheme::parse);
    if device.is_some() || scheme.is_some() {
        if let Err(error) = pane::set_device(app, device, scheme).await {
            return shared::browser_error(BACKEND, &error);
        }
    }
    if let Some(target) = &target {
        let load_before = pane
            .shared
            .load_seq
            .load(std::sync::atomic::Ordering::SeqCst);
        let fail_before = pane
            .shared
            .fail_seq
            .load(std::sync::atomic::Ordering::SeqCst);
        pane.shared.meta().begin_navigation();
        if let Err(error) = pane::navigate(pane, &target.url) {
            return shared::browser_error(BACKEND, &error);
        }
        if !pane::wait_load(pane, load_before, fail_before, LOAD_TIMEOUT).await {
            notes.push(format!(
                "页面 {} 秒内没有触发 load 事件,按当前画面继续",
                LOAD_TIMEOUT.as_secs()
            ));
        }
        if let Some(error) = pane.shared.meta().error.clone() {
            return shared::browser_error(BACKEND, &error.text);
        }
    }
    match act(pane, input, action, &notes).await {
        Ok(output) => output,
        Err(error) => shared::browser_error(BACKEND, &error),
    }
}

fn page_url(pane: &Pane) -> String {
    let url = pane.shared.meta().url.clone();
    if url.is_empty() {
        "(current page)".into()
    } else {
        url
    }
}

fn viewport_text(pane: &Pane) -> String {
    pane.shared.meta().device.label().to_string()
}

async fn act(
    pane: &Pane,
    input: &BrowserInput,
    action: BrowserAction,
    notes: &[String],
) -> Result<ToolOutput, String> {
    let webview = &pane.webview;
    let selector = input.selector.clone().unwrap_or_default();
    let needs_page = !matches!(action, BrowserAction::Open);
    if needs_page {
        let url = pane.shared.meta().url.clone();
        if url.is_empty() || url == "about:blank" {
            return Err("预览面板尚未打开页面(no browser: call open first)".into());
        }
    }
    Ok(match action {
        BrowserAction::Open => {
            let (png, _, _) = pane::capture(pane, false, None).await?;
            let title = pane.shared.meta().title.clone();
            shared::out_open(
                BACKEND,
                &title,
                &page_url(pane),
                &viewport_text(pane),
                notes,
                png,
            )
        }
        BrowserAction::Screenshot => {
            let clip = match input.selector.as_deref() {
                Some(selector) => Some(element_clip(webview, selector).await?),
                None => None,
            };
            let mut notes = notes.to_vec();
            let mut full_page = input.full_page && clip.is_none();
            if full_page && pane::is_zoomed(pane.shared.zoom()) {
                // 设备模式下整页几何不可靠(见 pane::capture):退回可视区,并在结果里说清楚。
                full_page = false;
                notes.push(
                    "设备模式(手机 / 平板 / 桌面尺寸缩放显示)下整页截图暂不支持,本次只截了面板里的可视区;需要整页请滚动后分段截图,或请用户把预览面板的设备切回「自适应」"
                        .into(),
                );
            }
            let (png, _, _) = pane::capture(pane, full_page, clip).await?;
            shared::out_screenshot(
                BACKEND,
                &page_url(pane),
                &viewport_text(pane),
                &shared::screenshot_scope(input),
                &notes,
                png,
            )
        }
        BrowserAction::Dom => {
            let expression = shared::dom_walker_expression(input.selector.as_deref());
            let value = cdp::evaluate(webview, &expression, false, cdp::CALL_TIMEOUT).await?;
            shared::out_dom(
                BACKEND,
                &page_url(pane),
                &selector,
                value.as_str().unwrap_or(""),
            )
        }
        BrowserAction::Console => {
            let entries: Vec<ConsoleItem> = pane
                .shared
                .console()
                .since(0)
                .into_iter()
                .map(|entry| ConsoleItem {
                    level: entry.level,
                    text: entry.text,
                    url: (!entry.url.is_empty()).then_some(entry.url),
                    line: entry.line,
                    col: entry.col,
                })
                .collect();
            shared::out_console(BACKEND, &page_url(pane), &entries, input.all)
        }
        BrowserAction::Click => {
            let point = element_center(webview, &selector).await?;
            click_at(webview, point).await?;
            settle().await;
            shared::out_click(BACKEND, &selector, &page_url(pane))
        }
        BrowserAction::Type => {
            let text = input.text.clone().unwrap_or_default();
            let focused = cdp::evaluate(
                webview,
                &format!("({FOCUS_AND_CLEAR})({})", json!(selector)),
                false,
                cdp::CALL_TIMEOUT,
            )
            .await?;
            if focused != Value::Bool(true) {
                return Err(format!("没有找到可输入的元素 {selector:?}"));
            }
            if !text.is_empty() {
                cdp::call(
                    webview,
                    "Input.insertText",
                    json!({ "text": text }),
                    cdp::CALL_TIMEOUT,
                )
                .await?;
            }
            shared::out_type(BACKEND, &selector, text.chars().count(), &page_url(pane))
        }
        BrowserAction::Press => {
            let key = input.key.clone().unwrap_or_default();
            for event in key_events(&key)? {
                cdp::call(webview, "Input.dispatchKeyEvent", event, cdp::CALL_TIMEOUT).await?;
            }
            settle().await;
            shared::out_press(BACKEND, &key, &page_url(pane))
        }
        BrowserAction::Scroll => {
            let expression = match input.selector.as_deref() {
                Some(selector) => format!(
                    "(() => {{ const el = document.querySelector({}); if (!el) return null; el.scrollIntoView({{block: 'center', inline: 'nearest'}}); return window.scrollY; }})()",
                    json!(selector)
                ),
                None => format!(
                    "(() => {{ window.scrollBy(0, {}); return window.scrollY; }})()",
                    input.dy.unwrap_or(600.0)
                ),
            };
            let scroll_y = cdp::evaluate(webview, &expression, false, cdp::CALL_TIMEOUT).await?;
            if scroll_y.is_null() {
                return Err(format!("没有找到元素 {selector:?}"));
            }
            shared::out_scroll(
                BACKEND,
                &shared::scroll_scope(input),
                scroll_y.as_f64(),
                &page_url(pane),
            )
        }
        BrowserAction::Wait => {
            let elapsed = wait(webview, input).await?;
            shared::out_wait(
                BACKEND,
                &shared::wait_scope(input),
                elapsed,
                &page_url(pane),
            )
        }
        BrowserAction::Eval => {
            let expression = input.expression.clone().unwrap_or_default();
            let value = cdp::evaluate(webview, &expression, true, cdp::CALL_TIMEOUT).await?;
            let json = serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
            shared::out_eval(BACKEND, &json, &page_url(pane))
        }
    })
}

/// 点击 / 按键后给页面一点时间处理事件(导航、重渲染)。
async fn settle() {
    tokio::time::sleep(Duration::from_millis(150)).await;
}

/// 聚焦并清空输入目标;可输入返回 true。
const FOCUS_AND_CLEAR: &str = r#"(sel) => {
  const el = document.querySelector(sel);
  if (!el) return false;
  el.scrollIntoView({ block: "center", inline: "nearest" });
  el.focus();
  if ("value" in el && typeof el.select === "function") {
    el.select();
    if (el.value) {
      el.value = "";
      el.dispatchEvent(new Event("input", { bubbles: true }));
    }
    return true;
  }
  if (el.isContentEditable) {
    const range = document.createRange();
    range.selectNodeContents(el);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
    return true;
  }
  return document.activeElement === el;
}"#;

async fn element_center(webview: &tauri::Webview, selector: &str) -> Result<(f64, f64), String> {
    let expression = format!(
        "(() => {{ const el = document.querySelector({}); if (!el) return null; el.scrollIntoView({{block: 'center', inline: 'center'}}); const r = el.getBoundingClientRect(); return {{x: r.x + r.width / 2, y: r.y + r.height / 2}}; }})()",
        json!(selector)
    );
    let point = cdp::evaluate(webview, &expression, false, cdp::CALL_TIMEOUT).await?;
    match (point["x"].as_f64(), point["y"].as_f64()) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(format!("没有找到元素 {selector:?}")),
    }
}

async fn element_clip(webview: &tauri::Webview, selector: &str) -> Result<super::Rect, String> {
    let expression = format!(
        "(() => {{ const el = document.querySelector({}); if (!el) return null; el.scrollIntoView({{block: 'nearest'}}); const r = el.getBoundingClientRect(); return {{x: r.x + window.scrollX, y: r.y + window.scrollY, w: r.width, h: r.height}}; }})()",
        json!(selector)
    );
    let rect = cdp::evaluate(webview, &expression, false, cdp::CALL_TIMEOUT).await?;
    match (
        rect["x"].as_f64(),
        rect["y"].as_f64(),
        rect["w"].as_f64(),
        rect["h"].as_f64(),
    ) {
        (Some(x), Some(y), Some(w), Some(h)) if w > 0.0 && h > 0.0 => {
            Ok(super::Rect { x, y, w, h })
        }
        (Some(_), ..) => Err(format!("元素 {selector:?} 没有尺寸(可能不可见)")),
        _ => Err(format!("没有找到元素 {selector:?}")),
    }
}

async fn click_at(webview: &tauri::Webview, (x, y): (f64, f64)) -> Result<(), String> {
    for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
        cdp::call(
            webview,
            "Input.dispatchMouseEvent",
            json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 }),
            cdp::CALL_TIMEOUT,
        )
        .await?;
    }
    Ok(())
}

async fn wait(webview: &tauri::Webview, input: &BrowserInput) -> Result<u64, String> {
    let started = std::time::Instant::now();
    let condition = match (&input.selector, &input.text) {
        (Some(selector), _) => Some(format!(
            "Boolean(document.querySelector({}))",
            json!(selector)
        )),
        (None, Some(text)) => Some(format!(
            "Boolean(document.body) && document.body.innerText.includes({})",
            json!(text)
        )),
        (None, None) => None,
    };
    let Some(condition) = condition else {
        let ms = input.ms.unwrap_or(1000).min(MAX_WAIT_MS);
        tokio::time::sleep(Duration::from_millis(ms)).await;
        return Ok(ms);
    };
    let budget = Duration::from_millis(input.ms.unwrap_or(MAX_WAIT_MS).min(MAX_WAIT_MS));
    loop {
        let value = cdp::evaluate(webview, &condition, false, cdp::CALL_TIMEOUT).await?;
        if value == Value::Bool(true) {
            return Ok(started.elapsed().as_millis() as u64);
        }
        if started.elapsed() >= budget {
            return Err(format!(
                "等待超时({} ms 内未满足:{})",
                budget.as_millis(),
                shared::wait_scope(input)
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// 按键描述(如 `Enter`、`Control+A`)→ Input.dispatchKeyEvent 的 keyDown/keyUp 参数。
/// 有文字的键用 keyDown(带 text),其余用 rawKeyDown(与 Puppeteer 同口径)。
pub(crate) fn key_events(spec: &str) -> Result<Vec<Value>, String> {
    let parts: Vec<&str> = spec.split('+').map(str::trim).collect();
    let (key_name, modifier_names) = match parts.split_last() {
        Some((key, modifiers)) if !key.is_empty() => (*key, modifiers),
        _ => {
            // 单独一个 "+" 键。
            if spec.trim() == "+" {
                ("+", &[][..])
            } else {
                return Err(format!("看不懂的按键 {spec:?}"));
            }
        }
    };
    let mut modifiers = 0u32;
    for name in modifier_names {
        modifiers |= match name.to_ascii_lowercase().as_str() {
            "alt" => 1,
            "control" | "ctrl" => 2,
            "meta" | "cmd" | "command" => 4,
            "shift" => 8,
            other => {
                return Err(format!(
                    "未知修饰键 {other:?}(Alt / Control / Meta / Shift)"
                ))
            }
        };
    }
    let (key, code, virtual_key, text): (String, String, u32, Option<String>) = match key_name {
        "Enter" => ("Enter".into(), "Enter".into(), 13, Some("\r".into())),
        "Tab" => ("Tab".into(), "Tab".into(), 9, None),
        "Escape" | "Esc" => ("Escape".into(), "Escape".into(), 27, None),
        "Backspace" => ("Backspace".into(), "Backspace".into(), 8, None),
        "Delete" => ("Delete".into(), "Delete".into(), 46, None),
        "ArrowUp" => ("ArrowUp".into(), "ArrowUp".into(), 38, None),
        "ArrowDown" => ("ArrowDown".into(), "ArrowDown".into(), 40, None),
        "ArrowLeft" => ("ArrowLeft".into(), "ArrowLeft".into(), 37, None),
        "ArrowRight" => ("ArrowRight".into(), "ArrowRight".into(), 39, None),
        "Home" => ("Home".into(), "Home".into(), 36, None),
        "End" => ("End".into(), "End".into(), 35, None),
        "PageUp" => ("PageUp".into(), "PageUp".into(), 33, None),
        "PageDown" => ("PageDown".into(), "PageDown".into(), 34, None),
        "Space" | " " => (" ".into(), "Space".into(), 32, Some(" ".into())),
        name if name.len() > 1
            && name.starts_with('F')
            && name[1..].parse::<u32>().is_ok_and(|n| (1..=12).contains(&n)) =>
        {
            let n: u32 = name[1..].parse().unwrap_or(1);
            (name.into(), name.into(), 111 + n, None)
        }
        name if name.chars().count() == 1 => {
            let ch = name.chars().next().unwrap_or(' ');
            let (code, virtual_key) = if ch.is_ascii_alphabetic() {
                (
                    format!("Key{}", ch.to_ascii_uppercase()),
                    ch.to_ascii_uppercase() as u32,
                )
            } else if ch.is_ascii_digit() {
                (format!("Digit{ch}"), ch as u32)
            } else {
                (String::new(), 0)
            };
            (name.into(), code, virtual_key, Some(name.into()))
        }
        other => return Err(format!("不支持的按键 {other:?}(可用 Enter、Tab、Escape、Backspace、方向键、F1–F12、单个字符,加 Control+ 等修饰)")),
    };
    // 带 Control/Alt/Meta 时不产生文字输入(Ctrl+A 是快捷键,不是输入 a)。
    let text = if modifiers & (1 | 2 | 4) != 0 {
        None
    } else {
        text
    };
    let mut down = json!({
        "type": if text.is_some() { "keyDown" } else { "rawKeyDown" },
        "key": key, "code": code, "windowsVirtualKeyCode": virtual_key, "modifiers": modifiers,
    });
    if let Some(text) = &text {
        down["text"] = json!(text);
        down["unmodifiedText"] = json!(text);
    }
    let up = json!({
        "type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": virtual_key, "modifiers": modifiers,
    });
    Ok(vec![down, up])
}

#[cfg(test)]
mod tests {
    use super::key_events;

    #[test]
    fn 按键表覆盖回车_修饰键_单字符与f键() {
        let enter = key_events("Enter").unwrap();
        assert_eq!(enter[0]["type"], "keyDown");
        assert_eq!(enter[0]["text"], "\r");
        assert_eq!(enter[0]["windowsVirtualKeyCode"], 13);
        assert_eq!(enter[1]["type"], "keyUp");

        let select_all = key_events("Control+A").unwrap();
        assert_eq!(select_all[0]["type"], "rawKeyDown", "带 Ctrl 不输入文字");
        assert_eq!(select_all[0]["modifiers"], 2);
        assert_eq!(select_all[0]["code"], "KeyA");
        assert!(select_all[0].get("text").is_none());

        let shifted = key_events("Shift+Tab").unwrap();
        assert_eq!(shifted[0]["modifiers"], 8);
        assert_eq!(shifted[0]["windowsVirtualKeyCode"], 9);

        assert_eq!(key_events("F5").unwrap()[0]["windowsVirtualKeyCode"], 116);
        assert_eq!(key_events("7").unwrap()[0]["code"], "Digit7");
        assert_eq!(key_events("中").unwrap()[0]["text"], "中");
        assert!(key_events("Hyper+X").is_err());
        assert!(key_events("NoSuchKey").is_err());
    }
}
