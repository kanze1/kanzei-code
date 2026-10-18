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
type DesktopPlan = Option<(Endpoint, String)>;
static PLANS: LazyLock<Mutex<HashMap<String, DesktopPlan>>> = LazyLock::new(Mutex::default);
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
    kanzei_core::store::stable_json_hash(&json!([
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
    let plan = DESKTOP.lock_or_recover().clone().and_then(|endpoint| {
        let meta = read_meta(&endpoint, ctx.process_id.as_deref())?;
        (meta["pane"] == true).then(|| (endpoint, meta["url"].as_str().unwrap_or("").to_owned()))
    });
    let url = plan.as_ref().map(|(_, url)| url.clone()).or_else(|| {
        kanzei_tools::browser::current_url(kanzei_tools::browser::Backend::Headless, ctx)
    });
    let mut plans = PLANS.lock_or_recover();
    if plans.len() > 256 {
        plans.clear();
    }
    plans.insert(plan_key(input, ctx), plan);
    kanzei_tools::browser::resources_for(input, Some(&ctx.cwd), url.as_deref())
}
pub(crate) async fn browser(input: Value, ctx: &ToolCtx) -> ToolOutput {
    let plan = PLANS.lock_or_recover().remove(&plan_key(&input, ctx));
    let Some(plan) = plan else {
        return ToolOutput::error("浏览器上下文已过期，请重新检查后再操作");
    };
    if let Some((endpoint, url)) = plan {
        return tool_call(&endpoint, "browser", input, ctx, Some(url)).await;
    }
    // Resource authorization has already completed. Creating about:blank is
    // allowed only for an explicit open; click/evaluate/relative actions never
    // migrate from their planned headless page to a different browser.
    if input["action"] == "open" {
        let endpoint = DESKTOP.lock_or_recover().clone();
        if let (Some(endpoint), Some(process)) = (endpoint, ctx.process_id.as_deref()) {
            if let Ok(Ok(value)) = tokio::time::timeout(
                Duration::from_secs(9),
                request(
                    &endpoint,
                    json!({"action":"ui_probe","kind":"preview","arg":process}),
                ),
            )
            .await
            {
                if value["ready"] == true {
                    return tool_call(
                        &endpoint,
                        "browser",
                        input,
                        ctx,
                        Some(value["url"].as_str().unwrap_or("about:blank").to_owned()),
                    )
                    .await;
                }
            }
        }
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
    expected_url: Option<String>,
) -> ToolOutput {
    let call = tool_request(tool, input, ctx, expected_url);
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
                        let meta = crate::preview::current_meta();
                        let pane =
                            crate::preview::route(meta.as_ref(), value["processId"].as_str())
                                == crate::preview::Backend::Pane;
                        return Ok(json!({"pane":pane,"url":meta.map(|m|m.url)}));
                    }
                    if value["action"] == "ui_probe" {
                        let kind = value["kind"].as_str().ok_or("缺少探针类型")?;
                        let arg = value["arg"].as_str().ok_or("缺少探针参数")?;
                        return crate::ui_probe(kind, arg).await;
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
                                    crate::preview::agent::execute_checked(
                                        &input,
                                        &ctx,
                                        value["expectedUrl"].as_str(),
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
    Ok(endpoint)
}
fn output(out: ToolOutput) -> Value {
    json!(out)
}

fn tool_request(tool: &str, input: Value, ctx: &ToolCtx, expected_url: Option<String>) -> Value {
    json!({"action":"tool","tool":tool,"input":input,"cwd":ctx.cwd,"root":ctx.project_root,
        "processId":ctx.process_id,"sessionId":ctx.session_id,"runId":ctx.run_id,
        "browserOwner":ctx.browser_owner,"expectedUrl":expected_url,"rules":kanzei_harness::permission::execution_rules()})
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
            Some("https://example.com/".into()),
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
