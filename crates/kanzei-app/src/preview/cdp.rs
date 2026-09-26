//! WebView2 进程内 CDP(webview2-com):不开 --remote-debugging-port,生产环境不留调试口。
//!
//! 线程模型(B0 实测:73/73 完成回调、88/88 事件回调都在 UI 线程):
//!   async 任务 --with_webview(闭包: Send)--> UI 线程执行闭包
//!   UI 线程:ICoreWebView2::CallDevToolsProtocolMethod(method, json, handler)
//!   WebView2 稍后**在 UI 线程**调 handler → oneshot::Sender::send
//!   async 任务带超时等 oneshot。UI 线程从不同步等待;超时之后才到的完成回调发给已丢弃的
//!   接收端,静默作废(隐藏面板上的截图就是这样:B0 实测永不返回,重新显示后才补回)。
//!
//! DevTools:只对面板打开([`open_devtools`] 只收 [`PaneWebview`])。B0 实测
//! `OpenDevToolsWindow` 在 AreDevToolsEnabled=false 时照样能开,**主界面也能被打开**——
//! 所以不打开那个开关(F12/Inspect 保持关闭),唯一的防线是永远不对主 webview 调用,
//! 由 [`super::pure_tests`] 的 grep 用例守住。

use std::time::Duration;

use serde_json::Value;
use tauri::Webview;

/// 面板子 webview 的类型化句柄:只有 label 是 `preview-<代次>` 才造得出来。
pub(crate) struct PaneWebview(Webview);

impl PaneWebview {
    pub(crate) fn new(webview: &Webview) -> Option<Self> {
        devtools_target_allowed(webview.label()).then(|| PaneWebview(webview.clone()))
    }
}

/// DevTools 只许开在预览面板上。
pub(crate) fn devtools_target_allowed(label: &str) -> bool {
    super::is_preview_label(label) && label != super::MAIN_LABEL
}

/// 默认的 CDP 调用超时。
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// 事件回调:`(事件名, 参数 JSON)`,在 UI 线程上调用,必须很快返回。
pub(crate) type EventHandler = std::sync::Arc<dyn Fn(&'static str, Value) + Send + Sync>;

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;
    use std::time::Duration;

    use serde_json::Value;
    use tauri::Webview;
    use tokio::sync::oneshot;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2DevToolsProtocolEventReceiver, ICoreWebView2Environment,
    };
    use webview2_com::{
        CallDevToolsProtocolMethodCompletedHandler, DevToolsProtocolEventReceivedEventHandler,
    };
    use windows::core::{HSTRING, PWSTR};

    thread_local! {
        /// 事件接收器是 COM 对象(!Send),只能活在 UI 线程;按面板代次登记,关面板时释放。
        static RECEIVERS: RefCell<Vec<(u64, ICoreWebView2DevToolsProtocolEventReceiver, i64)>> =
            const { RefCell::new(Vec::new()) };
    }

    /// 主 webview 的 WebView2 环境。`ICoreWebView2Environment` 是 !Send,Tauri 自己也对
    /// WebviewAttributes 做了 `unsafe impl Send`;这里只把它从 UI 线程搬进 builder。
    pub(crate) struct SendEnv(pub(crate) ICoreWebView2Environment);
    // SAFETY:环境对象只在 UI 线程上被使用(builder 在 UI 线程 build);这里只做一次所有权转移。
    unsafe impl Send for SendEnv {}

    /// 在 UI 线程上拿到 ICoreWebView2 执行 `f`,结果经 oneshot 带超时送回。
    pub(crate) async fn with_core<T, F>(
        webview: &Webview,
        timeout: Duration,
        f: F,
    ) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&ICoreWebView2, &ICoreWebView2Environment) -> windows::core::Result<T>
            + Send
            + 'static,
    {
        let (tx, rx) = oneshot::channel();
        webview
            .with_webview(move |platform| {
                let controller = platform.controller();
                let environment = platform.environment();
                // SAFETY:COM 调用发生在 UI 线程,controller 来自活着的 webview。
                let result = unsafe { controller.CoreWebView2() }
                    .and_then(|core| f(&core, &environment))
                    .map_err(|e| format!("{e}"));
                let _ = tx.send(result);
            })
            .map_err(|e| format!("with_webview: {e}"))?;
        match tokio::time::timeout(timeout, rx).await {
            Err(_) => Err(format!(
                "UI 线程 {} ms 内没有执行(面板可能已关闭或创建失败)",
                timeout.as_millis()
            )),
            Ok(Err(_)) => Err("面板已不存在(闭包被丢弃)".into()),
            Ok(Ok(result)) => result,
        }
    }

    pub(crate) async fn main_environment(main: &Webview) -> Result<SendEnv, String> {
        with_core(main, Duration::from_secs(5), |_, environment| {
            Ok(SendEnv(environment.clone()))
        })
        .await
    }

    pub(crate) async fn call(
        webview: &Webview,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let (tx, rx) = oneshot::channel::<Result<String, String>>();
        let (method_owned, params_text) = (method.to_string(), params.to_string());
        with_core(webview, Duration::from_secs(5), move |core, _| {
            let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                move |result, json| {
                    let _ = tx.send(match result {
                        Ok(()) => Ok(json),
                        Err(error) => Err(format!("{error}: {json}")),
                    });
                    Ok(())
                },
            ));
            // SAFETY:UI 线程上的 COM 调用;HSTRING 在调用期间存活。
            unsafe {
                core.CallDevToolsProtocolMethod(
                    &HSTRING::from(method_owned.as_str()),
                    &HSTRING::from(params_text.as_str()),
                    &handler,
                )
            }
        })
        .await?;
        match tokio::time::timeout(timeout, rx).await {
            Err(_) => Err(format!("CDP {method} 超时({} ms)", timeout.as_millis())),
            Ok(Err(_)) => Err(format!("CDP {method} 的完成回调没有触发")),
            Ok(Ok(Err(error))) => Err(format!("CDP {method} 失败: {error}")),
            Ok(Ok(Ok(json))) => serde_json::from_str(&json)
                .map_err(|e| format!("CDP {method} 返回的不是 JSON: {e}")),
        }
    }

    pub(crate) async fn subscribe(
        webview: &Webview,
        generation: u64,
        names: &'static [&'static str],
        handler: super::EventHandler,
    ) -> Result<(), String> {
        with_core(webview, Duration::from_secs(5), move |core, _| {
            for name in names {
                let name: &'static str = name;
                // SAFETY:UI 线程上的 COM 调用。
                let receiver =
                    unsafe { core.GetDevToolsProtocolEventReceiver(&HSTRING::from(name))? };
                let handler = handler.clone();
                let callback = DevToolsProtocolEventReceivedEventHandler::create(Box::new(
                    move |_sender, args| {
                        if let Some(args) = args {
                            let mut raw = PWSTR::null();
                            // SAFETY:回调在 UI 线程;take_pwstr 释放 COM 分配的字符串。
                            if unsafe { args.ParameterObjectAsJson(&mut raw) }.is_ok() {
                                let text = webview2_com::take_pwstr(raw);
                                let value = serde_json::from_str(&text).unwrap_or(Value::Null);
                                handler(name, value);
                            }
                        }
                        Ok(())
                    },
                ));
                let mut token = 0i64;
                // SAFETY:同上。
                unsafe { receiver.add_DevToolsProtocolEventReceived(&callback, &mut token)? };
                RECEIVERS.with(|keep| keep.borrow_mut().push((generation, receiver, token)));
            }
            Ok(())
        })
        .await
    }

    /// 释放某代面板的事件接收器(须在 UI 线程上执行)。
    pub(crate) fn release_on_ui_thread(generation: u64) {
        RECEIVERS.with(|keep| {
            keep.borrow_mut().retain(|(owner, receiver, token)| {
                if *owner == generation {
                    // SAFETY:UI 线程上的 COM 调用;失败(webview 已销毁)无害。
                    let _ = unsafe { receiver.remove_DevToolsProtocolEventReceived(*token) };
                    false
                } else {
                    true
                }
            })
        });
    }

    /// 历史导航与状态。
    pub(crate) async fn go(webview: &Webview, action: &'static str) -> Result<(), String> {
        with_core(webview, Duration::from_secs(5), move |core, _| {
            // SAFETY:UI 线程上的 COM 调用。
            unsafe {
                match action {
                    "back" => core.GoBack(),
                    "forward" => core.GoForward(),
                    _ => core.Stop(),
                }
            }
        })
        .await
    }

    pub(crate) async fn history(webview: &Webview) -> Result<(bool, bool), String> {
        with_core(webview, Duration::from_secs(5), |core, _| {
            let mut back = windows::core::BOOL(0);
            let mut forward = windows::core::BOOL(0);
            // SAFETY:UI 线程上的 COM 调用,出参指向本栈上的 BOOL。
            unsafe {
                core.CanGoBack(&mut back)?;
                core.CanGoForward(&mut forward)?;
            }
            Ok((back.as_bool(), forward.as_bool()))
        })
        .await
    }

    pub(crate) async fn open_devtools(pane: &super::PaneWebview) -> Result<(), String> {
        if !super::devtools_target_allowed(pane.0.label()) {
            return Err("DevTools 只能对预览面板打开".into());
        }
        // SAFETY:UI 线程上的 COM 调用;目标已由 PaneWebview 类型与上面的 label 校验双重限定。
        with_core(&pane.0, Duration::from_secs(5), |core, _| unsafe {
            core.OpenDevToolsWindow()
        })
        .await
    }
}

#[cfg(windows)]
pub(crate) use imp::{
    call, go, history, main_environment, open_devtools, release_on_ui_thread, subscribe,
};

#[cfg(not(windows))]
mod imp_stub {
    use serde_json::Value;
    use std::time::Duration;
    use tauri::Webview;

    const UNSUPPORTED: &str = "网页预览面板目前只支持 Windows(WebView2)";

    pub(crate) async fn call(
        _webview: &Webview,
        _method: &str,
        _params: Value,
        _timeout: Duration,
    ) -> Result<Value, String> {
        Err(UNSUPPORTED.into())
    }
    pub(crate) async fn subscribe(
        _webview: &Webview,
        _generation: u64,
        _names: &'static [&'static str],
        _handler: super::EventHandler,
    ) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub(crate) fn release_on_ui_thread(_generation: u64) {}
    pub(crate) async fn go(_webview: &Webview, _action: &'static str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub(crate) async fn history(_webview: &Webview) -> Result<(bool, bool), String> {
        Err(UNSUPPORTED.into())
    }
    pub(crate) async fn open_devtools(_pane: &super::PaneWebview) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
}

#[cfg(not(windows))]
pub(crate) use imp_stub::{call, go, history, open_devtools, release_on_ui_thread, subscribe};

/// Runtime.evaluate(returnByValue),页面异常转成错误。
pub(crate) async fn evaluate(
    webview: &Webview,
    expression: &str,
    await_promise: bool,
    timeout: Duration,
) -> Result<Value, String> {
    let result = call(
        webview,
        "Runtime.evaluate",
        serde_json::json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": await_promise,
            "userGesture": true,
        }),
        timeout,
    )
    .await?;
    if let Some(details) = result.get("exceptionDetails") {
        let description = details["exception"]["description"]
            .as_str()
            .or_else(|| details["text"].as_str())
            .unwrap_or("页面脚本异常");
        return Err(format!("页面脚本异常: {description}"));
    }
    Ok(result
        .pointer("/result/value")
        .cloned()
        .unwrap_or(Value::Null))
}
