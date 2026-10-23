//! Native loopback bridge for tools that need the visible desktop. No JS token.
use crate::{
    runtime_service::{frame_read, frame_write, request, Endpoint},
    MutexPoisonExt,
};
use kanzei_harness::{ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::Duration,
};
use tauri::Manager;

static DESKTOP: LazyLock<Mutex<Option<Endpoint>>> = LazyLock::new(Mutex::default);
#[derive(Clone)]
struct NativeOwner {
    endpoint: Endpoint,
    run: Option<String>,
    window: String,
}
static NATIVE_OWNERS: LazyLock<Mutex<HashMap<String, NativeOwner>>> = LazyLock::new(Mutex::default);
struct DesktopPlan {
    endpoint: Option<Endpoint>,
    backend: Option<kanzei_tools::browser::Backend>,
    target: Option<crate::preview::PageTarget>,
}
static PLANS: LazyLock<Mutex<HashMap<String, Result<DesktopPlan, String>>>> =
    LazyLock::new(Mutex::default);

async fn connected_native_action(
    stream: &tokio::net::TcpStream,
    action: impl std::future::Future<Output = ToolOutput>,
    cancel: tokio_util::sync::CancellationToken,
) -> ToolOutput {
    let mut disconnected = [0u8; 1];
    tokio::pin!(action);
    tokio::select! {
        biased;
        _ = stream.peek(&mut disconnected) => {
            cancel.cancel();
            action.await;
            ToolOutput::failed("NATIVE_CLIENT_DISCONNECTED", "调用方已断开；原生输入进程已收尾")
        },
        output = &mut action => output,
    }
}
pub(crate) fn native_target(ctx: &ToolCtx) -> Option<String> {
    let endpoint = DESKTOP.lock_or_recover().clone()?;
    let meta = read_meta(&endpoint, ctx.process_id.as_deref())?;
    meta["nativeTarget"]["id"].as_str().map(str::to_owned)
}
pub(crate) async fn native_action(
    input: Value,
    ctx: &ToolCtx,
    planned: Option<String>,
) -> ToolOutput {
    let Some(planned) = planned else {
        return ToolOutput::needs_correction(
            "NATIVE_WINDOW_REQUIRED",
            "请先在更多菜单为当前对话选择原生窗口",
        );
    };
    let endpoint = DESKTOP.lock_or_recover().clone();
    let Some(endpoint) = endpoint else {
        return ToolOutput::error("桌面窗口未连接");
    };
    let Some(process) = ctx.process_id.as_ref() else {
        return ToolOutput::error("原生控制缺少对话归属");
    };
    NATIVE_OWNERS.lock_or_recover().insert(
        process.clone(),
        NativeOwner {
            endpoint: endpoint.clone(),
            run: ctx.run_id.clone(),
            window: planned.clone(),
        },
    );
    let mut call = tool_request("desktop", input, ctx, None);
    call["nativeTarget"] = json!(planned);
    match request(&endpoint, call).await {
        Ok(value) => serde_json::from_value(value)
            .unwrap_or_else(|error| ToolOutput::error(format!("原生窗口回执无效: {error}"))),
        Err(error) => ToolOutput::failed(
            "NATIVE_ACTION_UNVERIFIED",
            format!("原生动作未验证，未重试: {error}"),
        ),
    }
}
pub(crate) async fn release_native(process: &str) -> Result<(), String> {
    let owner = NATIVE_OWNERS.lock_or_recover().get(process).cloned();
    if let Some(owner) = owner {
        request(
            &owner.endpoint,
            json!({"action":"native_release","processId":process,"runId":owner.run,"windowId":owner.window}),
        )
        .await?;
        let mut owners = NATIVE_OWNERS.lock_or_recover();
        if owners.get(process).is_some_and(|current| {
            current.run == owner.run
                && current.endpoint.addr == owner.endpoint.addr
                && current.window == owner.window
        }) {
            owners.remove(process);
        }
    }
    Ok(())
}
pub(crate) fn attach(value: &Value) {
    if let Ok(endpoint) = serde_json::from_value::<Endpoint>(value.clone()) {
        if endpoint
            .addr
            .parse::<std::net::SocketAddr>()
            .is_ok_and(|a| a.ip().is_loopback())
        {
            *DESKTOP.lock_or_recover() = Some(endpoint);
        }
    }
}
fn plan_key(input: &Value, ctx: &ToolCtx) -> String {
    kanzei_core::stable_json_hash(&json!([
        ctx.project_root,
        ctx.process_id,
        ctx.run_id,
        input
    ]))
}
// Resource checks are synchronous. Only a short, local metadata read is allowed here.
fn read_meta(endpoint: &Endpoint, process: Option<&str>) -> Option<Value> {
    use std::io::{Read, Write};
    let address = endpoint.addr.parse::<std::net::SocketAddr>().ok()?;
    if !address.ip().is_loopback() {
        return None;
    }
    let timeout = Duration::from_millis(800);
    let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let body =
        serde_json::to_vec(&json!({"action":"meta","token":endpoint.token,"processId":process}))
            .ok()?;
    stream.write_all(&(body.len() as u32).to_be_bytes()).ok()?;
    stream.write_all(&body).ok()?;
    let mut size = [0; 4];
    stream.read_exact(&mut size).ok()?;
    let size = u32::from_be_bytes(size) as usize;
    if size > 64 * 1024 {
        return None;
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).ok()?;
    let response: Value = serde_json::from_slice(&bytes).ok()?;
    (response["ok"] == true).then(|| response["value"].clone())
}
pub(crate) fn browser_resources(input: &Value, ctx: &ToolCtx) -> Vec<String> {
    let plan = prepare_browser(input, ctx);
    let url = plan
        .as_ref()
        .ok()
        .and_then(|plan| plan.target.as_ref().map(|target| target.url.clone()))
        .or_else(|| {
            kanzei_tools::browser::current_url(kanzei_tools::browser::Backend::Headless, ctx)
        });
    let mut plans = PLANS.lock_or_recover();
    if plans.len() > 256 {
        plans.clear();
    }
    plans.insert(plan_key(input, ctx), plan);
    kanzei_tools::browser::resources_for(input, Some(&ctx.cwd), url.as_deref())
}
fn prepare_browser(input: &Value, ctx: &ToolCtx) -> Result<DesktopPlan, String> {
    use kanzei_tools::browser::{Backend, BrowserAction};
    let parsed =
        kanzei_tools::browser::parse_browser_input(input.clone()).map_err(|e| e.model_content())?;
    kanzei_tools::browser::validate(&parsed, BrowserAction::parse(&parsed.action)?)?;
    let previous = crate::browser_selection::selected(ctx)?;
    let endpoint = DESKTOP.lock_or_recover().clone();
    let target = endpoint
        .as_ref()
        .and_then(|endpoint| read_meta(endpoint, ctx.process_id.as_deref()))
        .and_then(|meta| {
            serde_json::from_value::<Option<crate::preview::PageTarget>>(meta["target"].clone())
                .ok()
                .flatten()
        });
    let backend = parsed
        .backend
        .or_else(|| previous.as_ref().map(|selection| selection.backend))
        .or_else(|| target.as_ref().map(|_| Backend::Pane));
    if backend == Some(Backend::Pane) && parsed.backend.is_none() {
        if let Some(previous) = &previous {
            if previous.page_id.as_deref() != target.as_ref().map(|target| target.id.as_str()) {
                return Err(
                    "原预览页面已关闭或被替换；重新选择 backend: pane 并 open 后再操作".into(),
                );
            }
        }
    }
    Ok(DesktopPlan {
        endpoint,
        backend,
        target: if backend == Some(Backend::Pane) {
            target
        } else {
            None
        },
    })
}
pub(crate) async fn browser(input: Value, ctx: &ToolCtx) -> ToolOutput {
    let plan = PLANS.lock_or_recover().remove(&plan_key(&input, ctx));
    let Some(plan) = plan else {
        return ToolOutput::error("浏览器上下文已过期，请重新检查后再操作");
    };
    let mut plan = match plan {
        Ok(plan) => plan,
        Err(error) => return ToolOutput::needs_correction("BROWSER_TARGET_CHANGED", error),
    };
    use kanzei_tools::browser::Backend;
    if let (Some(endpoint), Some(target)) = (&plan.endpoint, &plan.target) {
        if let Err(error) =
            crate::browser_selection::remember(ctx, Backend::Pane, Some(target.id.clone()))
        {
            return ToolOutput::error(error);
        }
        return tool_call(endpoint, "browser", input, ctx, Some(target.clone())).await;
    }
    // Resource authorization has already completed. Creating about:blank is
    // allowed only for an explicit open; click/evaluate/relative actions never
    // migrate from their planned headless page to a different browser.
    if input["action"] == "open" && plan.backend != Some(Backend::Headless) {
        if let (Some(endpoint), Some(process)) = (&plan.endpoint, ctx.process_id.as_deref()) {
            if let Ok(Ok(value)) = tokio::time::timeout(
                Duration::from_secs(9),
                request(
                    endpoint,
                    json!({"action":"ui_probe","kind":if plan.backend == Some(Backend::Pane) { "select_preview" } else { "preview" },"arg":process}),
                ),
            )
            .await
            {
                if value["ready"] == true {
                    plan.target = read_meta(endpoint, ctx.process_id.as_deref())
                        .and_then(|meta| serde_json::from_value(meta["target"].clone()).ok());
                    if let Some(target) = plan.target {
                        if let Err(error) = crate::browser_selection::remember(
                            ctx,
                            Backend::Pane,
                            Some(target.id.clone()),
                        ) {
                            return ToolOutput::error(error);
                        }
                        return tool_call(endpoint, "browser", input, ctx, Some(target)).await;
                    }
                }
            }
        }
    }
    if plan.backend == Some(Backend::Pane) {
        return ToolOutput::needs_correction(
            "BROWSER_TARGET_LOST",
            "内部面板不可用；动作未转移到独立浏览器",
        );
    }
    if let Err(error) = crate::browser_selection::remember(ctx, Backend::Headless, None) {
        return ToolOutput::error(error);
    }
    match kanzei_tools::browser::parse_browser_input(input) {
        Ok(input) => kanzei_tools::browser::execute_headless(input, ctx, true).await,
        Err(output) => *output,
    }
}
pub(crate) async fn screenshot(ctx: &ToolCtx) -> ToolOutput {
    let endpoint = DESKTOP.lock_or_recover().clone();
    match endpoint {
        Some(endpoint) => tool_call(&endpoint, "ui_screenshot", json!({}), ctx, None).await,
        None => ToolOutput::error("桌面窗口未连接，无法截图；后台任务仍可继续"),
    }
}
pub(crate) async fn probe(kind: &str, arg: &str) -> Result<Value, String> {
    let endpoint = DESKTOP
        .lock_or_recover()
        .clone()
        .ok_or("桌面窗口未连接，无法读取界面；后台任务仍可继续")?;
    request(
        &endpoint,
        json!({"action":"ui_probe","kind":kind,"arg":arg}),
    )
    .await
}
async fn tool_call(
    endpoint: &Endpoint,
    tool: &str,
    input: Value,
    ctx: &ToolCtx,
    target: Option<crate::preview::PageTarget>,
) -> ToolOutput {
    let call = tool_request(tool, input, ctx, target);
    match tokio::time::timeout(Duration::from_secs(90), request(endpoint, call)).await {
        Ok(Ok(value)) => serde_json::from_value(value).unwrap_or_else(|error| {
            ToolOutput::failed(
                "DESKTOP_INVALID_TOOL_OUTPUT",
                format!("桌面工具返回格式无效: {error}"),
            )
        }),
        Ok(Err(error)) => ToolOutput::error(format!("桌面工具未完成：{error}；请重新检查页面状态")),
        Err(_) => ToolOutput::error("桌面连接中断或响应超时；该工具动作未重试，请重新检查页面状态"),
    }
}
pub(crate) fn install(app: &tauri::AppHandle) -> Result<Endpoint, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let endpoint = Endpoint {
        addr: listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .to_string(),
        token: app.invoke_key().into(),
        pid: std::process::id(),
        protocol: 1,
        executable: std::env::current_exe()
            .unwrap_or_default()
            .display()
            .to_string(),
    };
    let credential = endpoint.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).expect("desktop listener");
        while let Ok((mut stream, _)) = listener.accept().await {
            let token = credential.token.clone();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    let value =
                        tokio::time::timeout(Duration::from_secs(5), frame_read(&mut stream))
                            .await
                            .map_err(|_| "桌面请求超时")??;
                    if value["token"] != token {
                        return Err("桌面认证失败".into());
                    }
                    if value["action"] == "meta" {
                        return Ok(json!({"target":crate::preview::page_target(value["processId"].as_str()),"nativeTarget":crate::native_desktop::selected(value["processId"].as_str())}));
                    }
                    if value["action"] == "ui_probe" {
                        let kind = value["kind"].as_str().ok_or("缺少探针类型")?;
                        let arg = value["arg"].as_str().ok_or("缺少探针参数")?;
                        return crate::ui_probe(kind, arg).await;
                    }
                    if value["action"] == "native_release" {
                        crate::native_desktop::release(value["processId"].as_str().ok_or("缺少原生控制归属")?, value["runId"].as_str(), Some(value["windowId"].as_str().ok_or("缺少原生窗口归属")?)).await?;
                        return Ok(json!({"released":true}));
                    }
                    if value["action"] != "tool" {
                        return Err("未知桌面请求".into());
                    }
                    if app.get_window("main").is_none() {
                        return Err("桌面已关闭".into());
                    }
                    let ctx = tool_context(&value)?;
                    let rules = serde_json::from_value::<Option<kanzei_harness::Ruleset>>(
                        value["rules"].clone(),
                    )
                    .map_err(|error| format!("桌面权限快照无效: {error}"))?
                    .unwrap_or_default();
                    let out = kanzei_harness::permission::scope(rules, async {
                        let result = match value["tool"].as_str() {
                            Some("desktop") => {
                                let cancel = tokio_util::sync::CancellationToken::new();
                                let output = connected_native_action(&stream,
                                    crate::native_desktop::execute(value["input"].clone(), &ctx, value["nativeTarget"].as_str().unwrap_or(""), &cancel), cancel.clone()).await;
                                if cancel.is_cancelled() {
                                    crate::native_desktop::release(ctx.process_id.as_deref().ok_or("原生取消缺少对话归属")?, ctx.run_id.as_deref(), value["nativeTarget"].as_str()).await?;
                                }
                                output
                            },
                            Some("browser") => {
                                let input = match kanzei_tools::browser::parse_browser_input(
                                    value["input"].clone(),
                                ) {
                                    Ok(v) => v,
                                    Err(out) => return Ok(*out),
                                };
                                if crate::preview::route_for(ctx.process_id.as_deref())
                                    != crate::preview::Backend::Pane
                                {
                                    ToolOutput::error("预览窗口已关闭或切换，请重新检查浏览器状态")
                                } else {
                                    let target: crate::preview::PageTarget = serde_json::from_value(value["target"].clone()).map_err(|e| format!("缺少授权页面身份: {e}"))?;
                                    crate::preview::agent::execute_checked(
                                        &input,
                                        &ctx,
                                        Some(&target),
                                    )
                                    .await
                                    .unwrap_or_else(|| ToolOutput::error("预览窗口已关闭"))
                                }
                            }
                            Some("ui_screenshot") => {
                                crate::harness_ext::capture_desktop(&ctx).await
                            }
                            _ => return Err("不支持的桌面工具".into()),
                        };
                        Ok::<_, String>(result)
                    })
                    .await?;
                    Ok::<_, String>(output(out))
                }
                .await;
                let reply = match result {
                    Ok(value) => json!({"ok":true,"value":value}),
                    Err(error) => json!({"ok":false,"value":error}),
                };
                let _ = frame_write(&mut stream, &reply).await;
            });
        }
    });
    attach(&json!(endpoint));
    Ok(endpoint)
}
fn output(out: ToolOutput) -> Value {
    json!(out)
}

fn tool_request(
    tool: &str,
    input: Value,
    ctx: &ToolCtx,
    target: Option<crate::preview::PageTarget>,
) -> Value {
    json!({"action":"tool","tool":tool,"input":input,"cwd":ctx.cwd,"root":ctx.project_root,
        "processId":ctx.process_id,"sessionId":ctx.session_id,"runId":ctx.run_id,
        "browserOwner":ctx.browser_owner,"target":target,"rules":kanzei_harness::permission::execution_rules()})
}

fn tool_context(value: &Value) -> Result<ToolCtx, String> {
    let mut ctx = ToolCtx::new(
        value["cwd"].as_str().ok_or("缺少工作目录")?.into(),
        value["root"].as_str().ok_or("缺少项目")?.into(),
    );
    ctx.process_id = value["processId"].as_str().map(str::to_owned);
    ctx.session_id = value["sessionId"].as_str().map(str::to_owned);
    ctx.run_id = value["runId"].as_str().map(str::to_owned);
    ctx.browser_owner = value["browserOwner"].as_str().map(str::to_owned);
    Ok(ctx)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn native_client_eof_waits_for_the_pending_action_cleanup() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        struct Cancelled(Arc<AtomicBool>);
        impl Drop for Cancelled {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let guard = Cancelled(cancelled.clone());
        let (started, receiving) = tokio::sync::oneshot::channel();
        let cancel = tokio_util::sync::CancellationToken::new();
        let action_cancel = cancel.clone();
        let (observed, cancellation) = tokio::sync::oneshot::channel();
        let (allow_cleanup, cleanup) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            super::connected_native_action(
                &server,
                async move {
                    let _guard = guard;
                    started.send(()).unwrap();
                    action_cancel.cancelled().await;
                    observed.send(()).unwrap();
                    cleanup.await.unwrap();
                    kanzei_harness::ToolOutput::failed("CANCELLED", "executor reaped")
                },
                cancel,
            )
            .await
        });
        receiving.await.unwrap();
        drop(client);
        cancellation.await.unwrap();
        assert!(
            !task.is_finished(),
            "EOF cannot acknowledge stop before the executor finishes cleanup"
        );
        allow_cleanup.send(()).unwrap();
        let output = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(output.code.as_deref(), Some("NATIVE_CLIENT_DISCONNECTED"));
        assert!(
            cancelled.load(Ordering::SeqCst),
            "Disconnect must cancel the action future, including its owned helper child"
        );
    }
    use super::*;

    #[test]
    fn source_ownership_and_structured_rejection_survive_desktop_transport() {
        let mut ctx = ToolCtx::new("worktree".into(), "project".into());
        ctx.session_id = Some("session-source".into());
        ctx.run_id = Some("run-source".into());
        ctx.process_id = Some("p-source".into());
        ctx.browser_owner = Some("child-source".into());
        let request = tool_request(
            "browser",
            json!({"action":"source"}),
            &ctx,
            Some(crate::preview::PageTarget {
                id: "pane:owner".into(),
                document: 12,
                url: "https://example.com/".into(),
            }),
        );
        let received = tool_context(&request).unwrap();
        assert_eq!(received.cwd, ctx.cwd);
        assert_eq!(received.project_root, ctx.project_root);
        assert_eq!(received.session_id, ctx.session_id);
        assert_eq!(received.run_id, ctx.run_id);
        assert_eq!(received.process_id, ctx.process_id);
        assert_eq!(received.browser_owner, ctx.browser_owner);
        let source =
            ToolOutput::needs_correction("BROWSER_SOURCE_UNVERIFIED", "Document is still loading");
        let roundtrip: ToolOutput = serde_json::from_value(output(source.clone())).unwrap();
        assert_eq!(roundtrip.outcome, source.outcome);
        assert_eq!(roundtrip.code, source.code);
        assert_eq!(roundtrip.model_content(), source.model_content());
    }
}
