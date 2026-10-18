//! Nonactivating surfaces and a minimal Escape-only low-level hook.
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::Notify;
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::VK_ESCAPE, WindowsAndMessaging::*},
};

const BAR: &str = "native-control-bar";
const HIGHLIGHT: &str = "native-control-highlight";
static BAR_READY: AtomicBool = AtomicBool::new(false);
static HIGHLIGHT_READY: AtomicBool = AtomicBool::new(false);
static READY: Notify = Notify::const_new();
static ARMED: AtomicBool = AtomicBool::new(false);
static ESC_DOWN: AtomicBool = AtomicBool::new(false);
static STOP_PENDING: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn escape_hook(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0 {
        let key = &*(data as *const KBDLLHOOKSTRUCT);
        if key.vkCode == u32::from(VK_ESCAPE) && key.dwExtraInfo != super::INPUT_TAG {
            if message == WM_KEYUP as usize || message == WM_SYSKEYUP as usize {
                if ESC_DOWN.swap(false, Ordering::SeqCst) {
                    return 1;
                }
            } else if message == WM_KEYDOWN as usize || message == WM_SYSKEYDOWN as usize {
                if ESC_DOWN.load(Ordering::SeqCst) {
                    return 1;
                }
                if ARMED.swap(false, Ordering::SeqCst) {
                    STOP_PENDING.store(true, Ordering::SeqCst);
                    ESC_DOWN.store(true, Ordering::SeqCst);
                    // Never perform IPC, UI work or wait for cancellation in a hook.
                    tauri::async_runtime::spawn(super::escape());
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, message, data)
}
pub(super) fn install(app: &tauri::AppHandle) -> Result<(), String> {
    for (label, surface, width, height) in [
        (BAR, "bar", 480.0, 188.0),
        (HIGHLIGHT, "highlight", 32.0, 32.0),
    ] {
        let window = WebviewWindowBuilder::new(
            app,
            label,
            WebviewUrl::App(format!("native-control.html?surface={surface}").into()),
        )
        .title(if label == BAR {
            "Kanzei · 原生控制"
        } else {
            "Kanzei · 操作位置"
        })
        .inner_size(width, height)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .visible(false)
        .on_page_load(move |_, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                if label == BAR {
                    BAR_READY.store(false, Ordering::SeqCst);
                } else {
                    HIGHLIGHT_READY.store(false, Ordering::SeqCst);
                }
                if ARMED.swap(false, Ordering::SeqCst) {
                    STOP_PENDING.store(true, Ordering::SeqCst);
                    tauri::async_runtime::spawn(super::escape());
                }
            }
        })
        .build()
        .map_err(|error| error.to_string())?;
        if label == HIGHLIGHT {
            window
                .set_ignore_cursor_events(true)
                .map_err(|error| error.to_string())?;
        }
        window.on_window_event(|event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                tauri::async_runtime::spawn(super::escape());
            }
        });
    }
    let (sending, receiving) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("native-escape".into())
        .spawn(move || unsafe {
            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(escape_hook),
                GetModuleHandleW(std::ptr::null()),
                0,
            );
            if hook.is_null() {
                let _ = sending.send(Err(format!(
                    "全局 Esc 急停不可用：{}",
                    std::io::Error::last_os_error()
                )));
                return;
            }
            HOOK_THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
            if sending.send(Ok(())).is_err() {
                UnhookWindowsHookEx(hook);
                return;
            }
            let mut message = std::mem::zeroed();
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            UnhookWindowsHookEx(hook);
            HOOK_THREAD.store(0, Ordering::SeqCst);
            if ARMED.swap(false, Ordering::SeqCst) {
                STOP_PENDING.store(true, Ordering::SeqCst);
                tauri::async_runtime::spawn(super::escape());
            }
        })
        .map_err(|error| error.to_string())?;
    receiving.recv().map_err(|error| error.to_string())?
}
pub(super) async fn ready() -> Result<(), String> {
    if HOOK_THREAD.load(Ordering::SeqCst) == 0 {
        return Err("全局 Esc 急停未就绪，未操作目标窗口".into());
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let notified = READY.notified();
            if BAR_READY.load(Ordering::SeqCst) && HIGHLIGHT_READY.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    })
    .await
    .map_err(|_| "控制提示尚未就绪，未操作目标窗口".to_string())
}
pub(super) fn surface_ready(label: &str) -> Result<(), String> {
    match label {
        BAR => BAR_READY.store(true, Ordering::SeqCst),
        HIGHLIGHT => HIGHLIGHT_READY.store(true, Ordering::SeqCst),
        _ => return Err("只有原生控制提示窗口可以确认显示就绪".into()),
    }
    READY.notify_waiters();
    Ok(())
}
pub(super) fn shutdown() {
    ARMED.store(false, Ordering::SeqCst);
    let thread = HOOK_THREAD.load(Ordering::SeqCst);
    if thread != 0 {
        unsafe {
            PostThreadMessageW(thread, WM_QUIT, 0, 0);
        }
    }
}
pub(super) fn stop_pending() -> bool {
    STOP_PENDING.load(Ordering::SeqCst)
}
pub(super) fn new_lease() {
    STOP_PENDING.store(false, Ordering::SeqCst);
}
pub(super) fn present(app: &tauri::AppHandle, state: &Value) -> Result<(), String> {
    let running = state["phase"] == "running";
    ARMED.store(running && !stop_pending(), Ordering::SeqCst);
    let bar = app.get_window(BAR).ok_or("原生控制条不可用")?;
    let highlight = app.get_window(HIGHLIGHT).ok_or("原生位置提示不可用")?;
    let action = state["trace"].as_array().and_then(|trace| trace.last());
    let bounds = action
        .and_then(|action| action.get("bounds"))
        .filter(|bounds| bounds.is_object());
    let x = bounds
        .and_then(|bounds| bounds["x"].as_f64())
        .unwrap_or(0.0);
    let y = bounds
        .and_then(|bounds| bounds["y"].as_f64())
        .unwrap_or(0.0);
    let monitor = app
        .monitor_from_point(x, y)
        .map_err(|error| error.to_string())?
        .ok_or("目标显示器不可用")?;
    let scale = monitor.scale_factor();
    let mut work: MONITORINFO = unsafe { std::mem::zeroed() };
    work.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    let monitor_handle = unsafe {
        MonitorFromPoint(
            POINT {
                x: x as i32,
                y: y as i32,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    if unsafe { GetMonitorInfoW(monitor_handle, &mut work) } == 0 {
        return Err("无法读取显示器工作区".into());
    }
    let width = (480.0 * scale) as u32;
    let height = (188.0 * scale) as u32;
    bar.set_size(PhysicalSize::new(width, height))
        .map_err(|error| error.to_string())?;
    let margin = (16.0 * scale) as i32;
    let candidates = [
        (
            work.rcWork.right - width as i32 - margin,
            work.rcWork.bottom - height as i32 - margin,
        ),
        (
            work.rcWork.left + margin,
            work.rcWork.bottom - height as i32 - margin,
        ),
        (
            work.rcWork.right - width as i32 - margin,
            work.rcWork.top + margin,
        ),
        (work.rcWork.left + margin, work.rcWork.top + margin),
    ];
    // Never place the stop bar over the pointer location it is describing.
    let pointer = action
        .filter(|action| matches!(action["action"].as_str(), Some("click" | "invoke")))
        .and_then(|action| {
            let bounds = bounds?;
            let point = &action["point"];
            Some((
                x + point["x"]
                    .as_f64()
                    .unwrap_or(bounds["width"].as_f64()? / 2.0),
                y + point["y"]
                    .as_f64()
                    .unwrap_or(bounds["height"].as_f64()? / 2.0),
            ))
        });
    let (left, top) = candidates
        .into_iter()
        .find(|(left, top)| {
            pointer.is_none_or(|(x, y)| {
                x < *left as f64
                    || x >= (*left + width as i32) as f64
                    || y < *top as f64
                    || y >= (*top + height as i32) as f64
            })
        })
        .ok_or("控制条无法避开操作位置")?;
    bar.set_position(PhysicalPosition::new(left, top))
        .map_err(|error| error.to_string())?;
    if state["phase"] == "idle" {
        bar.hide().map_err(|error| error.to_string())?;
    } else {
        bar.show().map_err(|error| error.to_string())?;
    }
    if running {
        if let Some(bounds) = bounds {
            let width = bounds["width"].as_f64().ok_or("位置提示缺少宽度")?;
            let height = bounds["height"].as_f64().ok_or("位置提示缺少高度")?;
            if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
                highlight
                    .set_position(PhysicalPosition::new(x as i32, y as i32))
                    .map_err(|error| error.to_string())?;
                highlight
                    .set_size(PhysicalSize::new(width as u32, height as u32))
                    .map_err(|error| error.to_string())?;
                highlight.show().map_err(|error| error.to_string())?;
                return Ok(());
            }
        }
    }
    highlight.hide().map_err(|error| error.to_string())
}
