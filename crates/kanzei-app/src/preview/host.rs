//! 主窗口的焦点归还与移动通知(UI2-0926 #8 复核修复)。
//!
//! 开了 tauri "unstable" 之后,主 webview 从启动起就按 `WebviewKind::WindowChild` 建
//! (tauri-runtime-wry 2.11.4 lib.rs:4684),而 wry 只对**非** child 的 webview 给父窗口挂子类化
//! (wry 0.55.1 webview2/mod.rs:539 `if !is_child`)。于是丢了两件事,对所有用户生效、
//! 与是否打开过预览面板无关:
//!
//! 1. `WM_SETFOCUS → controller.MoveFocus`:Alt+Tab、点任务栏切回 kanzei 后,键盘焦点停在顶层
//!    窗口上,输入框与快捷键收不到按键,要先点一下页面;tao 的 WM_SETFOCUS 只发事件、不转交焦点,
//!    tauri 在第一次 add_child 之前连这个事件都不往外发(lib.rs:525 起的注释);
//! 2. `WM_MOVE / WM_MOVING → NotifyParentWindowPositionChanged`:移动窗口后,原生 `<select>`、
//!    右键菜单等 WebView2 弹层可能按旧位置弹出。
//!
//! 这里在主窗口上补挂**同样的**子类化(与 wry 对非 child webview 做的一致,只少了
//! WM_ENTERSIZEMOVE 时抢焦点那一条),焦点还给上次拿到焦点的 webview:默认主界面;用户点进过、
//! 仍然可见的预览面板优先。另加一道保险:只在主窗口就是前台窗口时才转交焦点——B0 实测
//! MoveFocus(PROGRAMMATIC) 会把窗口拉到前台,后台时绝不能调。
//!
//! 全部状态都在 UI 线程上(thread_local),子类化回调与 GotFocus 回调也都在 UI 线程上跑;
//! 唯一例外是主窗口句柄(原子量),供「kanzei 是不是前台程序」的判定在任何线程上读。

/// 「kanzei 就是前台程序」:前台窗口正是主窗口(句柄非空)。预览面板与主界面都是主窗口里的
/// 子窗口,前台窗口取的是顶层窗口;面板的 DevTools 窗口、别的程序在前台时都不算。
pub(crate) fn foreground_is_main(foreground: isize, main: isize) -> bool {
    main != 0 && foreground == main
}

#[cfg(windows)]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicIsize, Ordering};

    use webview2_com::FocusChangedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Controller, ICoreWebView2FocusChangedEventHandler,
        COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
    };
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, WM_MOVE, WM_MOVING, WM_NCDESTROY, WM_SETFOCUS,
    };

    /// 子类化 id(`kzpv`)。
    const SUBCLASS_ID: usize = 0x6b7a_7076;

    /// 主窗口句柄(attach_main 记下,窗口销毁时清零)。
    static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

    thread_local! {
        static MAIN: RefCell<Option<ICoreWebView2Controller>> = const { RefCell::new(None) };
        static PANEL: RefCell<Option<(u64, ICoreWebView2Controller)>> = const { RefCell::new(None) };
        /// 上次拿到焦点的是不是预览面板(两边的 GotFocus 维护)。
        static PANEL_FOCUSED: Cell<bool> = const { Cell::new(false) };
    }

    fn on_got_focus(panel: bool) -> ICoreWebView2FocusChangedEventHandler {
        FocusChangedEventHandler::create(Box::new(move |_, _| {
            PANEL_FOCUSED.with(|flag| flag.set(panel));
            Ok(())
        }))
    }

    /// 启动时调用:把主 webview 的 controller 与主窗口句柄交给 UI 线程挂子类化。
    pub(crate) fn attach_main(main_window: &tauri::WebviewWindow) {
        let hwnd = match main_window.hwnd() {
            Ok(hwnd) => hwnd.0 as isize,
            Err(error) => {
                tracing::warn!(%error, "主窗口句柄取不到,焦点归还与移动通知未挂上");
                return;
            }
        };
        MAIN_HWND.store(hwnd, Ordering::SeqCst);
        let queued = main_window.with_webview(move |platform| {
            let controller = platform.controller();
            let mut token = 0i64;
            // SAFETY:UI 线程上的 COM 调用;controller 来自活着的主 webview。
            if let Err(error) = unsafe { controller.add_GotFocus(&on_got_focus(false), &mut token) }
            {
                tracing::warn!(%error, "主 webview 的 GotFocus 没挂上(焦点仍会默认还给主界面)");
            }
            MAIN.with(|slot| *slot.borrow_mut() = Some(controller));
            // SAFETY:hwnd 是本线程(UI 线程)创建的主窗口;回调是 'static 的 extern fn。
            let ok = unsafe { SetWindowSubclass(hwnd as HWND, Some(host_proc), SUBCLASS_ID, 0) };
            if ok == 0 {
                tracing::warn!("主窗口子类化失败:切回窗口后可能要先点一下页面才能打字");
            }
        });
        if let Err(error) = queued {
            tracing::warn!(%error, "主 webview 不可用,焦点归还与移动通知未挂上");
        }
    }

    /// 面板建成后登记它的 controller(UI 线程执行)。
    pub(crate) fn register_panel(webview: &tauri::Webview, generation: u64) {
        let queued = webview.with_webview(move |platform| {
            let controller = platform.controller();
            let mut token = 0i64;
            // SAFETY:UI 线程上的 COM 调用。
            let _ = unsafe { controller.add_GotFocus(&on_got_focus(true), &mut token) };
            PANEL.with(|slot| *slot.borrow_mut() = Some((generation, controller)));
        });
        if let Err(error) = queued {
            tracing::warn!(%error, "预览面板的焦点登记没有排上 UI 线程");
        }
    }

    /// 关面板时释放(须在 UI 线程上执行;只释放同一代)。
    pub(crate) fn release_panel_on_ui_thread(generation: u64) {
        PANEL.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.as_ref().is_some_and(|(owner, _)| *owner == generation) {
                *slot = None;
                PANEL_FOCUSED.with(|flag| flag.set(false));
            }
        });
    }

    /// kanzei 的主窗口此刻是不是前台窗口(任何线程可调;attach 之前恒为 false)。
    pub(crate) fn main_is_foreground() -> bool {
        // SAFETY:无参 Win32 调用。
        let foreground = unsafe { GetForegroundWindow() } as isize;
        super::foreground_is_main(foreground, MAIN_HWND.load(Ordering::SeqCst))
    }

    /// 焦点该还给谁:面板拿过焦点且仍可见 → 面板;否则主界面。
    fn focus_target() -> Option<ICoreWebView2Controller> {
        if PANEL_FOCUSED.with(Cell::get) {
            let panel = PANEL.with(|slot| slot.borrow().as_ref().map(|(_, c)| c.clone()));
            if let Some(panel) = panel {
                let mut visible = windows::core::BOOL(0);
                // SAFETY:UI 线程上的 COM 调用,出参指向本栈上的 BOOL。
                if unsafe { panel.IsVisible(&mut visible) }.is_ok() && visible.as_bool() {
                    return Some(panel);
                }
            }
        }
        MAIN.with(|slot| slot.borrow().clone())
    }

    fn restore_focus(hwnd: HWND) {
        // SAFETY:无参 Win32 调用。
        if unsafe { GetForegroundWindow() } != hwnd {
            return;
        }
        if let Some(target) = focus_target() {
            // SAFETY:UI 线程上的 COM 调用(先克隆出来再调,回调里不持 RefCell 借用)。
            let _ = unsafe { target.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
        }
    }

    fn notify_moved() {
        let main = MAIN.with(|slot| slot.borrow().clone());
        let panel = PANEL.with(|slot| slot.borrow().as_ref().map(|(_, c)| c.clone()));
        for controller in main.iter().chain(panel.iter()) {
            // SAFETY:UI 线程上的 COM 调用。
            let _ = unsafe { controller.NotifyParentWindowPositionChanged() };
        }
    }

    unsafe extern "system" fn host_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            WM_SETFOCUS => restore_focus(hwnd),
            WM_MOVE | WM_MOVING => notify_moved(),
            WM_NCDESTROY => {
                // SAFETY:窗口销毁前摘掉自己的子类化(Win32 惯例)。
                unsafe { RemoveWindowSubclass(hwnd, Some(host_proc), SUBCLASS_ID) };
                MAIN.with(|slot| slot.borrow_mut().take());
                PANEL.with(|slot| slot.borrow_mut().take());
                MAIN_HWND.store(0, Ordering::SeqCst);
            }
            _ => {}
        }
        // SAFETY:交还默认处理(与 wry 的父窗口子类化同一顺序:先处理、再交给 tao)。
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}

#[cfg(windows)]
pub(crate) use imp::{attach_main, main_is_foreground, register_panel, release_panel_on_ui_thread};

#[cfg(not(windows))]
mod imp_stub {
    pub(crate) fn attach_main(_main_window: &tauri::WebviewWindow) {}
    pub(crate) fn register_panel(_webview: &tauri::Webview, _generation: u64) {}
    pub(crate) fn release_panel_on_ui_thread(_generation: u64) {}
    /// 判定不了前台就当「不是」:宁可不还焦点,也不抢别的程序的焦点。
    pub(crate) fn main_is_foreground() -> bool {
        false
    }
}

#[cfg(not(windows))]
pub(crate) use imp_stub::{
    attach_main, main_is_foreground, register_panel, release_panel_on_ui_thread,
};
