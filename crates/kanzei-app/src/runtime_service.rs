//! A detached runtime owns execution. Desktop windows are reconnectable clients.
//! The loopback protocol is length-prefixed (not HTTP), authenticated and bounded.
use crate::MutexPoisonExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tauri::{Emitter, Listener, Manager};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
mod compatibility;

const LIMIT: usize = 32 * 1024 * 1024;
const PROTOCOL: u32 = 1;
static CLIENT: OnceLock<Client> = OnceLock::new();
pub(crate) fn is_service() -> bool {
    std::env::args().any(|arg| arg == "--runtime-service")
}
pub(crate) fn embedded() -> bool {
    std::env::var_os("KANZEI_EMBEDDED_RUNTIME").is_some()
}
pub(crate) async fn process_roots(process_id: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let client = CLIENT.get().ok_or("后台未就绪")?;
    let endpoint = client.endpoint().await?;
    serde_json::from_value(
        request(&endpoint, json!({"action":"process_roots","id":process_id})).await?,
    )
    .map_err(|e| e.to_string())
}
pub(crate) fn directory() -> PathBuf {
    // Separate portable builds/profiles cannot silently borrow another version's runtime.
    let exe = std::env::current_exe().unwrap_or_default();
    let key = kanzei_core::store::stable_json_hash(&json!(exe)).replace(':', "-");
    kanzei_harness::kanzei_home()
        .unwrap_or_else(|| std::env::temp_dir().join("kanzei-home"))
        .join("runtime")
        .join(key)
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Endpoint {
    pub(crate) addr: String,
    pub(crate) token: String,
    pub(crate) pid: u32,
    pub(crate) protocol: u32,
    pub(crate) executable: String,
}
struct Client {
    endpoint: tokio::sync::Mutex<Option<Endpoint>>,
    app: tauri::AppHandle,
    desktop: Option<Endpoint>,
    failed: Mutex<Option<(std::time::Instant, String)>>,
    stopping: std::sync::atomic::AtomicBool,
}
#[derive(Default)]
struct Journal {
    seq: u64,
    bytes: usize,
    events: VecDeque<Value>,
}
impl Journal {
    fn append(&mut self, name: &str, payload: Value) {
        self.seq += 1;
        let event = json!({"seq":self.seq,"name":name,"payload":payload});
        self.bytes += event.to_string().len();
        self.events.push_back(event);
        while self.events.len() > 4096 || self.bytes > 16 * 1024 * 1024 {
            if let Some(event) = self.events.pop_front() {
                self.bytes -= event.to_string().len();
            }
        }
    }
    fn since(&self, after: u64) -> Value {
        json!({"cursor":self.seq,"lost":self.events.front().is_some_and(|e| e["seq"].as_u64().unwrap_or(0) > after + 1),
            "events":self.events.iter().filter(|e| e["seq"].as_u64().unwrap_or(0)>after).collect::<Vec<_>>()})
    }
}
fn local_command(command: &str) -> bool {
    command.starts_with("plugin:")
        || command.starts_with("preview_")
        || command.starts_with("voice_")
        || command.starts_with("memory_chat_")
        || command.starts_with("update_")
        || matches!(
            command,
            "runtime_status"
                | "runtime_shutdown"
                | "projects_pick"
                | "export_pick_dir"
                | "docs_open"
                | "settings_open"
                | "project_config_open"
                | "agent_directory_open"
                | "open_delivered_path"
                | "reveal_path"
                | "open_with"
                | "open_tools_list"
                | "open_tools_save"
                | "save_delivered_file"
                | "app_info"
        )
}
pub(crate) fn route<F>(handler: F) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static
where
    F: Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static,
{
    move |invoke| {
        if is_service() || embedded() || local_command(invoke.message.command()) {
            return handler(invoke);
        }
        let command = invoke.message.command().to_owned();
        let tauri::ipc::InvokeBody::Json(args) = invoke.message.payload() else {
            invoke.resolver.reject("后台运行只接收结构化参数");
            return true;
        };
        let args = args.clone();
        invoke.resolver.respond_async(async move {
            let client = CLIENT
                .get()
                .ok_or_else(|| tauri::ipc::InvokeError::from("后台尚未就绪"))?;
            client
                .invoke(&command, args)
                .await
                .map_err(tauri::ipc::InvokeError::from)
        });
        true
    }
}
pub(crate) async fn frame_write(stream: &mut TcpStream, value: &Value) -> Result<(), String> {
    let data = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if data.len() > LIMIT {
        return Err("后台请求过大".into());
    }
    stream
        .write_u32(data.len() as u32)
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(&data).await.map_err(|e| e.to_string())
}
pub(crate) async fn frame_read(stream: &mut TcpStream) -> Result<Value, String> {
    let size = stream.read_u32().await.map_err(|e| e.to_string())? as usize;
    if size > LIMIT {
        return Err("后台消息过大".into());
    }
    let mut data = vec![0; size];
    stream
        .read_exact(&mut data)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&data).map_err(|e| e.to_string())
}
pub(crate) async fn request(endpoint: &Endpoint, mut value: Value) -> Result<Value, String> {
    let address: std::net::SocketAddr = endpoint.addr.parse().map_err(|_| "后台地址无效")?;
    if !address.ip().is_loopback() || endpoint.protocol != PROTOCOL {
        return Err("后台协议不匹配".into());
    }
    let mut stream = tokio::time::timeout(Duration::from_secs(2), TcpStream::connect(address))
        .await
        .map_err(|_| "后台连接超时")?
        .map_err(|e| e.to_string())?;
    value["token"] = json!(endpoint.token);
    frame_write(&mut stream, &value).await?;
    let reply = frame_read(&mut stream).await?;
    if reply["ok"] == true {
        Ok(reply["value"].clone())
    } else {
        Err(reply["value"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| reply["value"].to_string()))
    }
}
impl Client {
    async fn endpoint(&self) -> Result<Endpoint, String> {
        if self.stopping.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("后台正在退出".into());
        }
        let mut cached = self.endpoint.lock().await;
        if let Some(endpoint) = &*cached {
            return Ok(endpoint.clone());
        }
        if let Some((at, error)) = self.failed.lock_or_recover().as_ref() {
            if at.elapsed() < Duration::from_secs(30) {
                return Err(error.clone());
            }
        }
        let path = directory().join("endpoint.json");
        if let Ok(data) = std::fs::read(&path) {
            if let Ok(endpoint) = serde_json::from_slice::<Endpoint>(&data) {
                if compatibility::accept(&endpoint).await? {
                    *cached = Some(endpoint.clone());
                    return Ok(endpoint);
                }
            }
        }
        let mut command =
            std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        command
            .arg("--runtime-service")
            .env_remove("KANZEI_E2E_CDP")
            .env_remove("WEBVIEW2_USER_DATA_FOLDER")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0200);
        }
        std::fs::create_dir_all(directory()).map_err(|e| e.to_string())?;
        if let Ok(log) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory().join("startup.log"))
        {
            command.stderr(log);
        }
        let mut child = command.spawn().map_err(|e| format!("后台启动失败：{e}"))?;
        for _ in 0..150 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Ok(data) = std::fs::read(&path) {
                if let Ok(endpoint) = serde_json::from_slice::<Endpoint>(&data) {
                    if compatibility::accept(&endpoint).await? {
                        *cached = Some(endpoint.clone());
                        return Ok(endpoint);
                    }
                }
            }
        }
        let error = match child.try_wait() {
            Ok(Some(code)) => format!(
                "后台启动失败（{code}），详见 {}",
                directory().join("startup.log").display()
            ),
            _ => "后台启动超时，请重试；任务未提交".into(),
        };
        *self.failed.lock_or_recover() = Some((std::time::Instant::now(), error.clone()));
        Err(error)
    }
    async fn invoke(&self, command: &str, args: Value) -> Result<Value, String> {
        let endpoint = self.endpoint().await?;
        // Never retry a mutation after an ambiguous connection loss.
        request(
            &endpoint,
            json!({"action":"invoke","command":command,"args":args}),
        )
        .await
    }
}
pub(crate) fn install_client(app: &tauri::AppHandle) {
    if embedded() {
        return;
    }
    let desktop = crate::desktop_bridge::install(app)
        .map_err(|error| tracing::warn!(%error,"desktop bridge unavailable"))
        .ok();
    let _ = CLIENT.set(Client {
        endpoint: tokio::sync::Mutex::new(None),
        app: app.clone(),
        desktop,
        failed: Mutex::new(None),
        stopping: std::sync::atomic::AtomicBool::new(false),
    });
    tauri::async_runtime::spawn(async {
        let client = CLIENT.get().unwrap();
        let mut cursor = 0;
        let mut attached_pid = 0;
        loop {
            let result = async {
                let endpoint = client.endpoint().await?;
                if endpoint.pid != attached_pid {
                    let hello = request(&endpoint, json!({"action":"ping"})).await?;
                    cursor = hello["cursor"].as_u64().unwrap_or(0);
                    attached_pid = endpoint.pid;
                    let _ = client.app.emit(
                        "kz:runtime-resync",
                        json!({"connected":true,"pid":attached_pid}),
                    );
                }
                let batch = request(
                    &endpoint,
                    json!({"action":"events","after":cursor,"desktop":client.desktop}),
                )
                .await?;
                if batch["lost"] == true {
                    let _ = client
                        .app
                        .emit("kz:runtime-resync", json!({"connected":true,"lost":true}));
                }
                for event in batch["events"].as_array().into_iter().flatten() {
                    if let Some(name) = event["name"].as_str() {
                        let _ = client.app.emit(name, event["payload"].clone());
                    }
                }
                cursor = batch["cursor"].as_u64().unwrap_or(cursor);
                Ok::<_, String>(())
            }
            .await;
            if let Err(error) = result {
                let _ = client.app.emit(
                    "kz:runtime-resync",
                    json!({"connected":false,"error":error}),
                );
                *client.endpoint.lock().await = None;
                attached_pid = 0;
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    });
}
#[tauri::command]
pub(crate) async fn runtime_status() -> Result<Value, String> {
    if embedded() {
        return Ok(json!({"mode":"embedded","pid":std::process::id()}));
    }
    let client = CLIENT.get().ok_or("后台未就绪")?;
    let endpoint = client.endpoint().await?;
    let mut value = request(&endpoint, json!({"action":"ping"})).await?;
    value["mode"] = json!("detached");
    value["uiPid"] = json!(std::process::id());
    Ok(value)
}
/// Updating replaces the executable, so its detached owner must release it too.
pub(crate) async fn shutdown_for_update() -> Result<(), String> {
    if embedded() {
        return Ok(());
    }
    let client = CLIENT.get().ok_or("后台未就绪")?;
    let endpoint = client.endpoint().await?;
    client
        .stopping
        .store(true, std::sync::atomic::Ordering::SeqCst);
    if let Err(error) = request(&endpoint, json!({"action":"shutdown"})).await {
        client
            .stopping
            .store(false, std::sync::atomic::Ordering::SeqCst);
        return Err(error);
    }
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if request(&endpoint, json!({"action":"ping"})).await.is_err() {
            return Ok(());
        }
    }
    client
        .stopping
        .store(false, std::sync::atomic::Ordering::SeqCst);
    Err("后台尚未退出，未启动安装器".into())
}
pub(crate) fn cancel_update_shutdown() {
    if let Some(client) = CLIENT.get() {
        client
            .stopping
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}
#[tauri::command]
pub(crate) async fn runtime_shutdown() -> Result<Value, String> {
    let client = CLIENT.get().ok_or("后台未就绪")?;
    let endpoint = client.endpoint().await?;
    let value = request(&endpoint, json!({"action":"shutdown"})).await?;
    client.app.exit(0);
    Ok(value)
}
pub(crate) fn service_lock() -> Result<Option<kanzei_tools::atomic_file::FileLock>, String> {
    std::fs::create_dir_all(directory()).map_err(|e| e.to_string())?;
    kanzei_tools::atomic_file::try_lock_exclusive(&directory().join("owner"), Duration::ZERO)
        .map_err(|e| e.to_string())
}
pub(crate) fn install_service(
    app: &tauri::AppHandle,
    webview: &tauri::WebviewWindow,
) -> Result<(), String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let endpoint = Endpoint {
        addr: listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .to_string(),
        token: app.invoke_key().into(),
        pid: std::process::id(),
        protocol: PROTOCOL,
        executable: std::env::current_exe()
            .unwrap_or_default()
            .display()
            .to_string(),
    };
    let journal = Arc::new(Mutex::new(Journal::default()));
    let notify = Arc::new(tokio::sync::Notify::new());
    for &name in EVENTS {
        let journal = journal.clone();
        let notify = notify.clone();
        let event_app = app.clone();
        app.listen(name, move |event| {
            if let Ok(mut payload) = serde_json::from_str::<Value>(event.payload()) {
                if matches!(name, "kz:done" | "kz:auto-fail") {
                    crate::runtime_continuation::arm(&event_app, &payload);
                    payload["runtimeManaged"] = json!(true);
                }
                journal.lock_or_recover().append(name, payload);
                notify.notify_waiters();
            }
        });
    }
    kanzei_tools::atomic_file::write_atomic_bytes(
        &directory().join("endpoint.json"),
        &serde_json::to_vec(&endpoint).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let app = app.clone();
    let webview = webview.clone();
    tauri::async_runtime::spawn(async move {
        let listener = TcpListener::from_std(listener).expect("nonblocking runtime listener");
        while let Ok((mut stream, _)) = listener.accept().await {
            let endpoint = endpoint.clone();
            let app = app.clone();
            let webview = webview.clone();
            let journal = journal.clone();
            let notify = notify.clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    let input = tokio::time::timeout(Duration::from_secs(10), frame_read(&mut stream)).await.map_err(|_| "后台请求超时")??;
                    if input["token"].as_str() != Some(endpoint.token.as_str()) { return Err("后台认证失败".into()); }
                    match input["action"].as_str().unwrap_or("") {
                        "ping" => Ok(json!({"pid":endpoint.pid,"protocol":PROTOCOL,"build":env!("KANZEI_RUNTIME_BUILD"),"cursor":journal.lock_or_recover().seq})),
                        "retire_if_idle" => compatibility::retire_if_idle(&app),
                        "process_roots" => {
                            let state=app.state::<crate::AppState>();
                            let processes=state.processes.lock_or_recover();
                            let process=processes.get(input["id"].as_str().unwrap_or("")).ok_or("对话不存在")?;
                            let mut roots=vec![process.project_dir.0.clone()];
                            if let Some(worktree)=&process.worktree_path {roots.insert(0,worktree.0.clone());}
                            Ok(json!(roots))
                        },
                        "events" => {
                            crate::desktop_bridge::attach(&input["desktop"]);
                            let after = input["after"].as_u64().unwrap_or(0);
                            let notified = notify.notified(); tokio::pin!(notified); notified.as_mut().enable();
                            let current = journal.lock_or_recover().seq;
                            if after >= current { let _ = tokio::time::timeout(Duration::from_secs(2), notified).await; }
                            Ok(journal.lock_or_recover().since(after))
                        }
                        "shutdown" => {
                            let app = app.clone(); tauri::async_runtime::spawn(async move { let processes=app.state::<crate::AppState>().processes.lock_or_recover().values().cloned().collect::<Vec<_>>();
                                for process in processes { if let Some(window)=app.get_window("main") { let _=crate::commands::run::stop_run(window,app.state::<crate::AppState>(),Some(process.origin_project.0.display().to_string()),Some(process.id.clone())); } }
                                for root in crate::durable_questions::roots() { for terminal in kanzei_tools::background::list(&root) { let _=kanzei_tools::background::stop(&terminal.id).await; } }
                                tokio::time::sleep(Duration::from_secs(1)).await; app.exit(0); });
                            Ok(json!({"stopped":true}))
                        }
                        "invoke" => {
                            let _admission = compatibility::admit()?;
                            let command = input["command"].as_str().ok_or("缺少命令")?;
                            if local_command(command) { return Err("此命令需要桌面界面".into()); }
                            let url = compatibility::invoke_url(&webview).await?;
                            let (sender, receiver) = tokio::sync::oneshot::channel();
                            webview.clone().on_message(tauri::webview::InvokeRequest {
                                cmd:command.into(),callback:tauri::ipc::CallbackFn(0),error:tauri::ipc::CallbackFn(1),
                                url,body:tauri::ipc::InvokeBody::Json(input["args"].clone()),
                                headers:Default::default(),invoke_key:app.invoke_key().into(),
                            }, Box::new(move |_,_,response,_,_| {
                                let result = match response {
                                    tauri::ipc::InvokeResponse::Ok(tauri::ipc::InvokeResponseBody::Json(text)) => serde_json::from_str(&text).map_err(|e| e.to_string()),
                                    tauri::ipc::InvokeResponse::Ok(_) => Err("后台命令返回了不支持的二进制数据".into()),
                                    tauri::ipc::InvokeResponse::Err(error) => Err(error.0.as_str().map(str::to_owned).unwrap_or_else(|| error.0.to_string())),
                                }; let _ = sender.send(result);
                            }));
                            receiver.await.map_err(|_| "后台响应通道关闭".to_string())?
                        }
                        _ => Err("未知后台请求".into()),
                    }
                }.await;
                let reply = match result {
                    Ok(value) => json!({"ok":true,"value":value}),
                    Err(error) => json!({"ok":false,"value":error}),
                };
                let _ = frame_write(&mut stream, &reply).await;
            });
        }
    });
    Ok(())
}

// Kept explicit so additions are reviewed with the cross-process event contract.
include!("runtime_events.rs");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_reports_eviction_and_keeps_monotonic_cursor() {
        let mut journal = Journal::default();
        for i in 0..4100 {
            journal.append("kz:text", json!(i));
        }
        assert_eq!(journal.since(0)["lost"], true);
        assert_eq!(journal.since(4099)["events"].as_array().unwrap().len(), 1);
        assert_eq!(journal.since(4100)["events"], json!([]));
    }
    #[tokio::test]
    async fn protocol_rejects_network_endpoints() {
        let endpoint = Endpoint {
            addr: "192.0.2.1:80".into(),
            token: "test".into(),
            pid: 1,
            protocol: PROTOCOL,
            executable: String::new(),
        };
        assert!(request(&endpoint, json!({}))
            .await
            .unwrap_err()
            .contains("协议"));
    }
    #[tokio::test]
    async fn oversized_frame_is_rejected_before_allocating_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = tokio::spawn(async move {
            let mut socket = TcpStream::connect(address).await.unwrap();
            socket.write_u32((LIMIT + 1) as u32).await.unwrap();
        });
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(frame_read(&mut stream).await.unwrap_err().contains("过大"));
        client.await.unwrap();
    }
}
