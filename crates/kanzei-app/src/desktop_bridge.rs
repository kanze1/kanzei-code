//! Native loopback bridge for UI probes targeting the visible desktop. No JS token.
use crate::{
    runtime_service::{frame_read, frame_write, request, Endpoint},
    MutexPoisonExt,
};
use serde_json::{json, Value};
use std::{
    sync::{LazyLock, Mutex},
    time::Duration,
};

static DESKTOP: LazyLock<Mutex<Option<Endpoint>>> = LazyLock::new(Mutex::default);

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
    tauri::async_runtime::spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).expect("desktop listener");
        while let Ok((mut stream, _)) = listener.accept().await {
            let token = credential.token.clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    let value =
                        tokio::time::timeout(Duration::from_secs(5), frame_read(&mut stream))
                            .await
                            .map_err(|_| "桌面请求超时")??;
                    if value["token"] != token {
                        return Err("桌面认证失败".into());
                    }
                    if value["action"] == "ui_probe" {
                        let kind = value["kind"].as_str().ok_or("缺少探针类型")?;
                        let arg = value["arg"].as_str().ok_or("缺少探针参数")?;
                        return crate::ui_probe(kind, arg).await;
                    }
                    Err("未知桌面请求".into())
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
