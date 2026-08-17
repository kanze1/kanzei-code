//! Local HTTP bridge for mobile notifications and messages (R-270).
//!
//! R-270 批1 改造:
//! - 监听可切 LAN(0.0.0.0,默认仍回环 127.0.0.1——回环行为不变);
//! - 设备配对:桌面端生成一次性配对码,移动端用配对码换每设备独立 token;
//!   设备列表可单独撤销(移除即该 token 立即 401,其它设备不受影响);
//! - 每连接独立线程处理(替换原单线程 accept 循环),长请求不阻塞其它连接。
//!
//! 协议契约沿用 docs/design/r059_mobile_agent_communication.md 阶段A字段定义。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::State;

use crate::normalized_project_root;
use crate::state::{MobileDeviceInfo, MobileService, MobileServiceInfo, SessionRuntime};
use crate::AppState;

/// D-386:随机源——配对码/设备 token 不再用「pid+纳秒」可预测形态,改用
/// 纳秒 + 进程内递增计数器 + 随机种子混合(无 rand 依赖,std 实现)。
/// 统计上不可由外部观察值预测出下一个值(每次调用递增计数,纳秒量级时间熵)。
static TOKEN_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TOKEN_SEED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

fn token_seed() -> u64 {
    *TOKEN_SEED.get_or_init(|| {
        // 进程地址空间熵 + 启动时间纳秒,作为计数器初始偏移。
        let addr_entropy = (&TOKEN_SEED as *const _ as usize) as u64;
        let time_entropy = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        addr_entropy.rotate_left(32) ^ time_entropy
    })
}

/// 生成不可预测的随机串(hex)。计数递增 + 纳秒混合,防外部观察值预测。
fn random_token(prefix: &str) -> String {
    let counter = TOKEN_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mixed = (token_seed() ^ now).wrapping_add(counter.rotate_left(17));
    format!("{prefix}-{mixed:016x}-{counter:04x}")
}

/// 生成设备凭据(device_id + device_token),随机源。
fn generate_device_credentials() -> (String, String) {
    (random_token("dev"), random_token("kz-device"))
}

fn mobile_json_response(status: &str, body: &serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes()
    .into_iter()
    .chain(body)
    .collect()
}

fn mobile_query(path: &str, key: &str) -> Option<String> {
    path.split('?')
        .nth(1)?
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.replace('+', " "))
}

/// R-270:设备 token 认证。从请求头取 `Authorization: Bearer <token>`,
/// 在设备表里找得到即通过(撤销 = 从表移除,移除后立即 401)。
fn mobile_authorized(request: &str, devices: &HashMap<String, String>) -> bool {
    request.lines().any(|line| {
        line.to_ascii_lowercase()
            .strip_prefix("authorization: bearer ")
            .and_then(|value| value.split_whitespace().next())
            .map(|token| devices.values().any(|device_token| device_token == token))
            .unwrap_or(false)
    })
}

/// 读完整请求(头 + body,按 Content-Length,已 trim D-063)。
fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let Ok(count) = stream.read(&mut chunk) else {
            return None;
        };
        if count == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if buffer.len() > 65_536 {
            return None;
        }
    };
    let request_head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let content_length = request_head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::to_owned)
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < header_end + content_length {
        let Ok(count) = stream.read(&mut chunk) else {
            return None;
        };
        if count == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..count]);
    }
    let body = buffer[header_end..header_end + content_length].to_vec();
    Some((request_head, body))
}

fn handle_mobile_connection(
    mut stream: TcpStream,
    project_root: PathBuf,
    devices: Arc<Mutex<HashMap<String, String>>>,
    pair_code: Arc<Mutex<Option<String>>>,
    runtimes: Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
    active: Arc<AtomicBool>,
) {
    let Some((request_head, body)) = read_request(&mut stream) else {
        return;
    };
    let request_line = request_head.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let state_path = kanzei_core::project_state_path(&project_root);

    // R-270:配对端点不要求设备 token(用一次性配对码)。
    if method == "POST" && path.starts_with("/v1/pair") {
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        let submitted = payload
            .get("pair_code")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let expected = pair_code.lock().unwrap().clone().unwrap_or_default();
        if submitted.is_empty() || submitted != expected {
            let _ = stream.write_all(&mobile_json_response(
                "401 Unauthorized",
                &json!({"error": "invalid_pair_code"}),
            ));
            return;
        }
        // 配对成功:清空一次性配对码,生成设备 token(D-386:写 SQLite 持久化)。
        *pair_code.lock().unwrap() = None;
        let (device_id, device_token) = generate_device_credentials();
        devices
            .lock()
            .unwrap()
            .insert(device_id.clone(), device_token.clone());
        // D-386:设备表落 SQLite——重启后已配对设备仍在,撤销跨重启有效。
        if let Ok(store) = kanzei_core::SessionStore::open(&state_path) {
            let _ = store.upsert_mobile_device(
                &device_id,
                &device_token,
                "已配对设备",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            );
        }
        let _ = stream.write_all(&mobile_json_response(
            "200 OK",
            &json!({"device_id": device_id, "token": device_token}),
        ));
        return;
    }

    // 其它端点:设备 token 认证。
    if !mobile_authorized(&request_head, &devices.lock().unwrap()) {
        let _ = stream.write_all(&mobile_json_response(
            "401 Unauthorized",
            &json!({"error": "device_revoked_or_unauthorized"}),
        ));
        return;
    }

    // R-270 批2:SSE 长连接实时推送。认证通过后、进普通 JSON 分发前拦截——
    // 长连接由独立线程持有(批1 多线程 accept),不阻塞其它请求。
    if method == "GET" && path.split('?').next() == Some("/v1/events") {
        // D-388:传 active(停服检查)与 devices(撤销检查)——长连接不无视停服/撤销。
        handle_sse(&mut stream, &state_path, path, &active, &devices);
        return;
    }

    // R-270 批4:PWA 静态页 serve。手机浏览器打开桥接地址加载 PWA(随桌面端发版
    // 分发,不另起服务)。serve 目录 = 应用资源 mobile-pwa/(R-271 填充完整界面)。
    if method == "GET"
        && !path.starts_with("/v1/")
        && path != "/health"
        && serve_pwa(&mut stream, path)
    {
        return;
    }

    // R-270 批3:approval 通道。GET pending(脱敏摘要)+ POST answer(回答)。
    // 回答走 runner 既有 ask 流(PendingAsk.sender),最终门禁仍在 harness 侧。
    match (method, path.split('?').next().unwrap_or_default()) {
        ("GET", "/v1/approval/pending") => {
            let pending = approval_pending_list(&runtimes);
            let _ = stream.write_all(&mobile_json_response("200 OK", &pending));
            return;
        }
        ("POST", "/v1/approval/answer") => {
            let payload: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let reply = match approval_answer(&runtimes, &payload) {
                Ok(rendered) => rendered,
                Err(error) => {
                    let _ = stream.write_all(&mobile_json_response(
                        "400 Bad Request",
                        &json!({"error": error}),
                    ));
                    return;
                }
            };
            let _ = stream.write_all(&mobile_json_response("200 OK", &reply));
            return;
        }
        _ => {}
    }

    let response = match (method, path.split('?').next().unwrap_or_default()) {
        ("GET", "/health") => mobile_json_response(
            "200 OK",
            &json!({"status": "ok", "transport": "local_http"}),
        ),
        ("GET", "/v1/notifications") => {
            let Some(thread_id) = mobile_query(path, "thread_id") else {
                let _ = stream.write_all(&mobile_json_response(
                    "400 Bad Request",
                    &json!({"error": "thread_id_required"}),
                ));
                return;
            };
            let device_id =
                mobile_query(path, "device_id").unwrap_or_else(|| "paired-device".into());
            let cursor_param =
                mobile_query(path, "cursor").and_then(|value| value.parse::<u64>().ok());
            match kanzei_core::SessionStore::open(&state_path).and_then(|store| {
                let cursor = match cursor_param {
                    Some(cursor) => cursor,
                    None => store.delivery_cursor(&device_id, &thread_id)?,
                };
                let events = store.replay_notifications(&thread_id, cursor, 100)?;
                if let Some(last) = events.last() {
                    store.set_delivery_cursor(&device_id, &thread_id, last.sequence)?;
                }
                Ok((events, cursor))
            }) {
                Ok((events, cursor)) => mobile_json_response(
                    "200 OK",
                    &json!({"events": events, "cursor": events.last().map(|event| event.sequence).unwrap_or(cursor)}),
                ),
                Err(error) => mobile_json_response(
                    "500 Internal Server Error",
                    &json!({"error": error.to_string()}),
                ),
            }
        }
        ("POST", "/v1/messages") => {
            let payload: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let Some(thread_id) = payload.get("thread_id").and_then(|value| value.as_str()) else {
                let _ = stream.write_all(&mobile_json_response(
                    "400 Bad Request",
                    &json!({"error": "thread_id_required"}),
                ));
                return;
            };
            match kanzei_core::SessionStore::open(&state_path).and_then(|store| {
                store.create_session(thread_id, &project_root.display().to_string(), None)?;
                store.append_event(thread_id, "mobile.message", &payload)?;
                Ok(())
            }) {
                Ok(()) => {
                    // D-387:消费方——把手机发的消息注入对应线程的对话历史(内存
                    // conversation + 事件),桌面端打开该会话即可见。此前只 append_event
                    // 零消费,手机消息落库即死信(R-059「双向通信」核销失效)。
                    let text = payload
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if !text.is_empty() {
                        consume_mobile_message(&runtimes, thread_id, text, &state_path);
                    }
                    mobile_json_response("202 Accepted", &json!({"accepted": true}))
                }
                Err(error) => mobile_json_response(
                    "500 Internal Server Error",
                    &json!({"error": error.to_string()}),
                ),
            }
        }
        _ => mobile_json_response("404 Not Found", &json!({"error": "not_found"})),
    };
    let _ = stream.write_all(&response);
}

/// D-387:手机消息消费方——把 POST /v1/messages 的消息注入对应线程的对话历史,
/// 让桌面端可见(此前只 append_event 零消费,消息落库即死信)。
///
/// ①注入内存 conversation(对应 runtime,若在跑):后续轮末写回自动持久化;
/// ②立即 append_event("conversation.updated", {messages: 全量})持久化(会话未在
/// 跑也能被 conversation_get 读到);
/// ③MOBILE_MESSAGE_EMIT 通知 UI 刷新(kz:mobile-message 事件)。
fn consume_mobile_message(
    runtimes: &Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
    thread_id: &str,
    text: &str,
    state_path: &Path,
) {
    let message = kanzei_llm::Message::user_text(text);
    // ①注入内存 conversation(会话在跑时),并收集该会话消息全量用于持久化。
    let mut persisted_messages: Option<Vec<kanzei_llm::Message>> = None;
    let runtimes = runtimes.lock().unwrap();
    if let Some(runtime) = runtimes.get(thread_id) {
        let mut conversation = runtime.conversation.lock().unwrap();
        let messages = conversation.entry(thread_id.to_string()).or_default();
        messages.push(message.clone());
        persisted_messages = Some(messages.clone());
    }
    // ②持久化 conversation.updated 事件(D-387 核心:即使会话未在跑也落库,
    // conversation_get 可读,消息不再死信;在跑时带上全量历史与轮末写回同构)。
    if let Ok(store) = kanzei_core::SessionStore::open(state_path) {
        let messages = persisted_messages.unwrap_or_else(|| vec![message.clone()]);
        let _ = store.append_event(
            thread_id,
            "conversation.updated",
            &json!({ "messages": messages }),
        );
    }
    // ③UI 通知(桌面端可见,即使会话未在跑也刷新列表)。
    if let Some(emit) = crate::state::MOBILE_MESSAGE_EMIT.get() {
        emit(thread_id.to_string(), text.to_string());
    }
}

/// R-270 批3:approval pending 列表——遍历所有 runtime 的 asks,产出**脱敏摘要**
/// (不暴露对话/资源全文,只给 id/kind/action/session;resource 截断到 80 字符)。
fn approval_pending_list(
    runtimes: &Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
) -> serde_json::Value {
    let runtimes = runtimes.lock().unwrap();
    let mut items = Vec::new();
    for runtime in runtimes.values() {
        let asks = runtime.asks.lock().unwrap();
        for (id, pending) in asks.iter() {
            let (kind, action, resource) = match &pending.request {
                kanzei_core::AskRequest::Permission { action, resource } => {
                    ("permission", action.clone(), resource.clone())
                }
                kanzei_core::AskRequest::Question { question, .. } => {
                    ("question", "question".into(), question.clone())
                }
            };
            // 脱敏:resource 截断,避免把完整路径/敏感内容推给 LAN 设备。
            let resource_truncated: String = resource.chars().take(80).collect();
            let truncated = if resource.chars().count() > 80 {
                format!("{resource_truncated}…")
            } else {
                resource_truncated
            };
            items.push(json!({
                "id": id,
                "kind": kind,
                "action": action,
                "resource": truncated,
                "session_id": pending.session_id,
            }));
        }
    }
    json!({ "pending": items, "count": items.len() })
}

/// R-270 批3:回答一个 pending ask。`reply` 取值:
/// - permission: "allow" | "deny"(always 与 once 的持久化/会话语义由桌面端处理,
///   移动端只做放行/拒绝二选一——approval 门禁仍在 harness 侧);
/// - question: 任意文本(answer)或 "cancel"。
///
/// 回答走 runner 既有 ask 流:找到 PendingAsk 后通过 sender 发送 AskResponse,
/// runner 的权限门禁按回复放行/拒绝——本通道不新增能力面,只回答既有询问。
fn approval_answer(
    runtimes: &Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
    payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = payload
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "answer 需要 id(数字)".to_string())?;
    let reply = payload
        .get("reply")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if reply.is_empty() {
        return Err("answer 需要 reply(allow|deny 或回答文本)".to_string());
    }
    let runtimes = runtimes.lock().unwrap();
    let mut found = None;
    for runtime in runtimes.values() {
        if let Some(pending) = runtime.asks.lock().unwrap().remove(&id) {
            found = Some(pending);
            break;
        }
    }
    let pending = found.ok_or_else(|| format!("ask {id} 不存在或已回答"))?;
    let response = match &pending.request {
        kanzei_core::AskRequest::Question { .. } => {
            if reply == "cancel" {
                kanzei_core::AskResponse::Cancelled
            } else {
                kanzei_core::AskResponse::Answer(reply.clone())
            }
        }
        kanzei_core::AskRequest::Permission { .. } => {
            let decision = match reply.as_str() {
                "allow" => kanzei_core::AskReply::AllowOnce,
                _ => kanzei_core::AskReply::Deny,
            };
            kanzei_core::AskResponse::Permission(decision)
        }
    };
    pending
        .sender
        .send(response)
        .map_err(|_| format!("ask {id} 的接收端已关闭(runner 已取消该询问)"))?;
    Ok(json!({ "answered": id, "session_id": pending.session_id }))
}

/// R-270 批4:PWA 静态页 serve。serve 应用资源目录 `mobile-pwa/` 下的文件
/// (随桌面端发版分发,不另起服务)。
///
/// 路径安全:只允许请求 `/` 或 `/mobile-pwa/...` 下的相对路径,拒绝 `..` 穿越。
/// 返回 true = 已 serve(200/404);false = 非静态资源路径(落回 JSON 分发)。
fn serve_pwa(stream: &mut TcpStream, path: &str) -> bool {
    // 定位 mobile-pwa 目录:应用资源在 crates/kanzei-app/mobile-pwa。
    let pwa_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("mobile-pwa");
    if !pwa_root.is_dir() {
        return false;
    }
    // 规范化请求路径:/ → index.html;/mobile-pwa/xxx → mobile-pwa/xxx。
    let relative = if path == "/" {
        "index.html"
    } else {
        match path.strip_prefix('/') {
            Some(rest) => rest,
            None => return false,
        }
    };
    // 路径穿越防护:任何含 `..` 或反斜杠的请求直接 404(不拼文件系统路径)。
    if relative.contains("..") || relative.contains('\\') {
        let _ = stream.write_all(&mobile_json_response(
            "404 Not Found",
            &json!({"error": "not_found"}),
        ));
        return true;
    }
    let file = pwa_root.join(relative);
    if !file.is_file() {
        let _ = stream.write_all(&mobile_json_response(
            "404 Not Found",
            &json!({"error": "not_found"}),
        ));
        return true;
    }
    let Ok(bytes) = std::fs::read(&file) else {
        let _ = stream.write_all(&mobile_json_response(
            "500 Internal Server Error",
            &json!({"error": "read_failed"}),
        ));
        return true;
    };
    let content_type = match file.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&bytes);
    true
}

/// R-270 批2:SSE 长连接实时推送(GET /v1/events)。
///
/// - 起始 cursor:取 `cursor` 查询参数,缺省用该 device 的 delivery_cursor
///   (断线重连沿用既有 cursor 补发——不丢终态);
/// - 轮询:`replay_notifications(thread_id, cursor, 100)` 逐批推进,新事件
///   逐条推 `data: <json>\n\n` 并推进 delivery_cursor;
/// - 心跳:无新事件时每 15s 推一条注释行保活(防代理/防火墙断连);
/// - 每连接独立线程(批1 多线程 accept),长连接挂着不阻塞其它请求;
/// - 连接断开(write 失败)即返回,线程收尾,不留僵尸;
/// - D-388:每轮检查 `active`(停服即断开)与设备表(被撤销即断开)——长连接
///   不无视停服/撤销,避免已撤销设备继续收事件、停服线程泄漏。
fn persist_delivery_cursor_and_advance<F>(
    cursor: &mut u64,
    sequence: u64,
    persist: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    persist()?;
    *cursor = sequence;
    Ok(())
}

fn handle_sse(
    stream: &mut TcpStream,
    state_path: &Path,
    path: &str,
    active: &AtomicBool,
    devices: &Arc<Mutex<HashMap<String, String>>>,
) {
    let thread_id = match mobile_query(path, "thread_id") {
        Some(id) => id,
        None => {
            let _ = stream.write_all(&mobile_json_response(
                "400 Bad Request",
                &json!({"error": "thread_id_required"}),
            ));
            return;
        }
    };
    let device_id = mobile_query(path, "device_id").unwrap_or_else(|| "paired-device".into());
    let store = match kanzei_core::SessionStore::open(state_path) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("移动端 SSE 打开 state store 失败: {error}");
            return;
        }
    };
    let initial_cursor = mobile_query(path, "cursor")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| store.delivery_cursor(&device_id, &thread_id).unwrap_or(0));

    // SSE 响应头:长连接、不缓存、keep-alive。
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\
         Connection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let _ = stream.flush();

    // 关闭写超时:长连接要能一直挂着。
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));
    let mut cursor = initial_cursor;
    let mut last_heartbeat = std::time::Instant::now();
    loop {
        // D-388:停服检查——active=false 时断开长连接(停服不留泄漏线程)。
        if !active.load(Ordering::SeqCst) {
            return;
        }
        // D-388:撤销检查——设备已从表移除(token 失效)时断开,不再收事件。
        let device_exists = devices.lock().unwrap().contains_key(&device_id);
        if !device_exists {
            return;
        }
        // 有事件就推;无则心跳(SSE 注释行保活)。
        match store.replay_notifications(&thread_id, cursor, 100) {
            Ok(events) if !events.is_empty() => {
                for event in events {
                    let payload = serde_json::to_string(&event).unwrap_or_default();
                    let frame = format!("data: {payload}\n\n");
                    if stream.write_all(frame.as_bytes()).is_err() {
                        return; // 连接断开,收尾。
                    }
                    if let Err(error) =
                        persist_delivery_cursor_and_advance(&mut cursor, event.sequence, || {
                            store
                                .set_delivery_cursor(&device_id, &thread_id, event.sequence)
                                .map_err(|error| error.to_string())
                        })
                    {
                        eprintln!(
                            "移动端 delivery cursor 持久化失败，关闭连接等待重连重放: {error}"
                        );
                        return;
                    }
                }
                let _ = stream.flush();
                last_heartbeat = std::time::Instant::now();
            }
            Ok(_) => {
                if last_heartbeat.elapsed() >= std::time::Duration::from_secs(15) {
                    if stream.write_all(b": heartbeat\n\n").is_err() {
                        return;
                    }
                    let _ = stream.flush();
                    last_heartbeat = std::time::Instant::now();
                }
            }
            Err(_) => {
                // 读库失败(store 未初始化等):短暂退避后继续,不让连接猝死。
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

/// 启动移动端桥接服务。
///
/// `lan=true` 监听 0.0.0.0(允许 LAN 设备访问);`lan=false`(默认)保持回环
/// 127.0.0.1 既有行为。返回地址、配对码与当前设备列表。
#[tauri::command]
pub fn mobile_service_start(
    state: State<'_, AppState>,
    project_dir: String,
    port: Option<u16>,
    lan: Option<bool>,
) -> Result<MobileServiceInfo, String> {
    if state.mobile_service.lock().unwrap().is_some() {
        return Err("移动端桥接服务已经启动".into());
    }
    let root = normalized_project_root(Path::new(&project_dir));
    let lan = lan.unwrap_or(false);
    let bind_addr: &str = if lan { "0.0.0.0" } else { "127.0.0.1" };
    let listener = TcpListener::bind((bind_addr, port.unwrap_or(0)))
        .map_err(|e| format!("移动端桥接服务启动失败: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("设置本机服务非阻塞失败: {e}"))?;
    let address = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .to_string();
    // D-386:配对码换随机源(不再 pid+纳秒可预测)。
    let pair_code = random_token("kz-pair");
    let active = Arc::new(AtomicBool::new(true));
    let devices: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let pair_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(Some(pair_code.clone())));
    // D-386:设备表持久化——启动时从 SQLite 载入已配对设备(内存表供认证热路径,
    // SQLite 是持久真源;重启后已配对设备仍在)。
    {
        let state_path = kanzei_core::project_state_path(&root);
        if let Ok(store) = kanzei_core::SessionStore::open(&state_path) {
            if let Ok(devices_snapshot) = store.list_mobile_devices() {
                let mut map = devices.lock().unwrap();
                for (device_id, device_token, _name, _paired_at) in devices_snapshot {
                    map.insert(device_id, device_token);
                }
            }
        }
    }

    let thread_active = active.clone();
    let thread_root = root.clone();
    let thread_devices = devices.clone();
    let thread_pair = pair_slot.clone();
    let thread_runtimes = state.runtimes.clone();
    std::thread::spawn(move || {
        while thread_active.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    // R-270:每连接独立线程——长请求(SSE/approval)不阻塞其它连接。
                    let conn_devices = thread_devices.clone();
                    let conn_pair = thread_pair.clone();
                    let conn_root = thread_root.clone();
                    let conn_runtimes = thread_runtimes.clone();
                    let conn_active = thread_active.clone();
                    std::thread::spawn(move || {
                        handle_mobile_connection(
                            stream,
                            conn_root,
                            conn_devices,
                            conn_pair,
                            conn_runtimes,
                            conn_active,
                        )
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
    });
    let service_ref = MobileService {
        active: active.clone(),
        devices: devices.clone(),
        pair_code: pair_slot.clone(),
        lan,
        project_root: root.clone(),
    };
    let info = MobileServiceInfo {
        address: address.clone(),
        token: pair_code.clone(),
        lan: service_ref.lan,
        devices: service_ref
            .devices
            .lock()
            .unwrap()
            .keys()
            .map(|device_id| MobileDeviceInfo {
                device_id: device_id.clone(),
                name: "已配对设备".into(),
                paired_at_ms: 0,
            })
            .collect(),
    };
    *state.mobile_service.lock().unwrap() = Some(service_ref);
    Ok(info)
}

/// 撤销指定设备:从设备表移除,其 token 立即 401,其它设备不受影响。
/// D-386:同步删 SQLite 持久化行——撤销跨重启有效。
#[tauri::command]
pub fn mobile_device_revoke(state: State<'_, AppState>, device_id: String) -> Result<(), String> {
    let guard = state.mobile_service.lock().unwrap();
    let service = guard.as_ref().ok_or("移动端桥接服务未启动")?;
    let removed = service.devices.lock().unwrap().remove(&device_id);
    if removed.is_some() {
        // 同步删 SQLite(持久真源);内存表已删,失败只记日志不阻塞。
        let state_path = kanzei_core::project_state_path(&service.project_root);
        if let Ok(store) = kanzei_core::SessionStore::open(&state_path) {
            let _ = store.remove_mobile_device(&device_id);
        }
        Ok(())
    } else {
        Err(format!("设备不存在: {device_id}"))
    }
}

/// 再生成配对码(D-386):替换当前配对码(已配对设备保留,未撤销)。
/// 原配对码一次性用完即 None,此命令让用户能再次配对新设备而不必重启服务。
#[tauri::command]
pub fn mobile_pair_code_regenerate(state: State<'_, AppState>) -> Result<String, String> {
    let guard = state.mobile_service.lock().unwrap();
    let service = guard.as_ref().ok_or("移动端桥接服务未启动")?;
    let new_code = random_token("kz-pair");
    *service.pair_code.lock().unwrap() = Some(new_code.clone());
    Ok(new_code)
}

/// 当前设备列表(设置页展示与撤销入口)。D-386:paired_at_ms 从 SQLite 读。
#[tauri::command]
pub fn mobile_device_list(state: State<'_, AppState>) -> Result<Vec<MobileDeviceInfo>, String> {
    let guard = state.mobile_service.lock().unwrap();
    let service = guard.as_ref().ok_or("移动端桥接服务未启动")?;
    // 读取配对码状态(设置页据此提示「正在等待配对」或「已配对 N 台」)。
    let _pending_pair = service.pair_code.lock().unwrap().is_some();
    // paired_at_ms 从 SQLite 读(内存表只存 id→token);SQLite 读失败回落内存表。
    let state_path = kanzei_core::project_state_path(&service.project_root);
    if let Ok(store) = kanzei_core::SessionStore::open(&state_path) {
        if let Ok(rows) = store.list_mobile_devices() {
            return Ok(rows
                .into_iter()
                .map(|(device_id, _token, name, paired_at_ms)| MobileDeviceInfo {
                    device_id,
                    name,
                    paired_at_ms,
                })
                .collect());
        }
    }
    let devices = service.devices.lock().unwrap();
    Ok(devices
        .keys()
        .map(|device_id| MobileDeviceInfo {
            device_id: device_id.clone(),
            name: "已配对设备".into(),
            paired_at_ms: 0,
        })
        .collect())
}

#[tauri::command]
pub fn mobile_service_stop(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(service) = state.mobile_service.lock().unwrap().take() {
        service.active.store(false, Ordering::SeqCst);
        Ok(())
    } else {
        Err("移动端桥接服务当前未启动".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices_with(token: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("dev-1".to_string(), token.to_string());
        map
    }

    /// R-270:设备 token 认证——表内有 token 通过,表内没有(已撤销)401。
    #[test]
    fn 设备token认证_表内通过_撤销后拒绝() {
        let request =
            "GET /v1/notifications?thread_id=t HTTP/1.1\r\nAuthorization: Bearer tok-a\r\n\r\n";
        let mut devices = devices_with("tok-a");
        assert!(mobile_authorized(request, &devices), "配对设备应通过");
        devices.remove("dev-1"); // 撤销
        assert!(
            !mobile_authorized(request, &devices),
            "撤销后 token 必须立即失效"
        );
    }

    /// R-270:撤销一个设备不影响其它设备。
    #[test]
    fn 撤销不影响其它设备() {
        let mut devices = HashMap::new();
        devices.insert("dev-a".to_string(), "tok-a".to_string());
        devices.insert("dev-b".to_string(), "tok-b".to_string());
        let req_a =
            "GET /v1/notifications?thread_id=t HTTP/1.1\r\nAuthorization: Bearer tok-a\r\n\r\n";
        let req_b =
            "GET /v1/notifications?thread_id=t HTTP/1.1\r\nAuthorization: Bearer tok-b\r\n\r\n";
        assert!(mobile_authorized(req_a, &devices));
        devices.remove("dev-a");
        assert!(!mobile_authorized(req_a, &devices), "撤销的 dev-a 立即 401");
        assert!(mobile_authorized(req_b, &devices), "dev-b 不受影响");
    }

    /// D-386:随机源——配对码/设备 token 连续调用不同、带前缀、统计上不可预测
    /// (不再 pid+纳秒可预测形态)。
    #[test]
    fn 随机源_连续调用不同且带前缀() {
        let a = random_token("kz-pair");
        let b = random_token("kz-pair");
        assert_ne!(a, b, "连续两次配对码必须不同");
        assert!(a.starts_with("kz-pair-"), "配对码带前缀: {a}");
        let (dev_a, tok_a) = generate_device_credentials();
        let (dev_b, tok_b) = generate_device_credentials();
        assert_ne!(dev_a, dev_b);
        assert_ne!(tok_a, tok_b);
        assert!(dev_a.starts_with("dev-"));
        assert!(tok_a.starts_with("kz-device-"));
        assert!(tok_a.len() > 20, "token 应有足够熵: {tok_a}");
    }

    /// 读请求:Content-Length 带空格也能正确解析(D-063 回归)。
    #[test]
    fn 配对码与普通token分开判定() {
        // 配对码不等于任何设备 token;认证只看设备表。
        let devices = devices_with("device-token");
        let pair_req = "POST /v1/pair HTTP/1.1\r\nContent-Length: 10\r\n\r\n{\"x\":1}";
        assert!(
            !mobile_authorized(pair_req, &devices),
            "配对请求不带设备 token,普通认证应拒绝(走配对专用分支)"
        );
        let ok_req = "POST /v1/messages HTTP/1.1\r\nAuthorization: Bearer device-token\r\n\r\n";
        assert!(mobile_authorized(ok_req, &devices));
    }

    /// R-270 批2:SSE 断线重连 cursor 补发——带 cursor 参数时从该 cursor 起,
    /// 不丢已交付的终态(验收③核心)。
    #[test]
    fn sse起始cursor_参数优先_缺省用delivery_cursor() {
        // 带 cursor 参数:解析直接返回参数值。
        let path_with_cursor = "/v1/events?thread_id=t&device_id=dev-1&cursor=42";
        let parsed =
            mobile_query(path_with_cursor, "cursor").and_then(|value| value.parse::<u64>().ok());
        assert_eq!(parsed, Some(42), "带 cursor 参数应优先使用");

        // 不带 cursor:走 delivery_cursor(此处模拟 store 未初始化 → 0)。
        let path_no_cursor = "/v1/events?thread_id=t&device_id=dev-1";
        let no_cursor = mobile_query(path_no_cursor, "cursor")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        assert_eq!(no_cursor, 0, "无 cursor 参数时从 0 起(store 未建时)");
    }

    /// R-270 批2:SSE 端点识别——/v1/events 走长连接分支,普通 JSON 端点不受影响。
    #[test]
    fn sse端点识别_events走长连接_其它端点走json() {
        // mobile_query 提取 thread_id/device_id,供 SSE 连接使用。
        let path = "/v1/events?thread_id=t1&device_id=dev-1&cursor=7";
        assert_eq!(mobile_query(path, "thread_id").as_deref(), Some("t1"));
        assert_eq!(mobile_query(path, "device_id").as_deref(), Some("dev-1"));
        assert_eq!(mobile_query(path, "cursor").as_deref(), Some("7"));
        // 其它端点没有 SSE 专属参数,但 thread_id 查询仍通用。
        let notif = "/v1/notifications?thread_id=t2";
        assert_eq!(mobile_query(notif, "thread_id").as_deref(), Some("t2"));
    }

    /// R-270 批2:SSE 帧格式——data: 前缀 + 空行分隔(标准 SSE 协议)。
    #[test]
    fn sse帧格式_data前缀加空行() {
        let event = json!({"sequence": 1, "kind": "run.completed", "summary": "完成"});
        let payload = serde_json::to_string(&event).unwrap();
        let frame = format!("data: {payload}\n\n");
        assert!(
            frame.starts_with("data: "),
            "SSE 帧必须 data: 前缀: {frame}"
        );
        assert!(frame.ends_with("\n\n"), "SSE 帧必须以空行结束: {frame}");
        // 心跳是注释行。
        let heartbeat = ": heartbeat\n\n";
        assert!(heartbeat.starts_with(':'), "心跳是注释行");
        assert!(heartbeat.ends_with("\n\n"));
    }

    /// R-270 批3:approval pending 列表——遍历所有 runtime 的 asks 产出脱敏摘要,
    /// resource 截断不暴露完整内容。
    #[test]
    fn approval_pending_列出脱敏摘要() {
        let runtime = Arc::new(SessionRuntime::default());
        let (_sender, _rx) = tokio::sync::oneshot::channel();
        runtime.asks.lock().unwrap().insert(
            7,
            crate::PendingAsk {
                sender: _sender,
                request: kanzei_core::AskRequest::Permission {
                    action: "bash".into(),
                    resource: "long-resource-".repeat(30),
                },
                action: "bash".into(),
                resource: "long-resource-".repeat(30),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        let mut runtimes = HashMap::new();
        runtimes.insert("ses-1".to_string(), runtime);
        let runtimes = Arc::new(Mutex::new(runtimes));

        let list = approval_pending_list(&runtimes);
        assert_eq!(list["count"], 1, "应列出 1 条 pending");
        assert_eq!(list["pending"][0]["id"], 7);
        assert_eq!(list["pending"][0]["kind"], "permission");
        assert_eq!(list["pending"][0]["action"], "bash");
        // 脱敏:resource 截断到 80 字符。
        let resource = list["pending"][0]["resource"].as_str().unwrap();
        assert!(
            resource.chars().count() <= 81,
            "resource 必须截断: {resource}"
        );
    }

    /// R-270 批3:approval answer——permission allow/deny 经 sender 送达 runner,
    /// 门禁决策由 runner 侧执行(本通道不旁路)。
    #[test]
    fn approval_answer_permission放行与拒绝送达() {
        let runtime = Arc::new(SessionRuntime::default());
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        runtime.asks.lock().unwrap().insert(
            3,
            crate::PendingAsk {
                sender: tx,
                request: kanzei_core::AskRequest::Permission {
                    action: "bash".into(),
                    resource: "cargo build".into(),
                },
                action: "bash".into(),
                resource: "cargo build".into(),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        let mut runtimes = HashMap::new();
        runtimes.insert("ses-1".to_string(), runtime.clone());
        let runtimes = Arc::new(Mutex::new(runtimes));

        // allow → AllowOnce 送达。
        let answered = approval_answer(&runtimes, &json!({"id": 3, "reply": "allow"})).unwrap();
        assert_eq!(answered["answered"], 3);
        match rx.try_recv() {
            Ok(kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)) => {}
            other => panic!("allow 应送达 AllowOnce,实得: {other:?}"),
        }
        // 回答后 pending 移除:再答报不存在。
        let err = approval_answer(&runtimes, &json!({"id": 3, "reply": "deny"})).unwrap_err();
        assert!(err.contains("不存在或已回答"), "{err}");
    }

    /// R-270 批3:approval answer——deny 送达 Deny;question 回答送 Answer 文本。
    #[test]
    fn approval_answer_拒绝与问题回答() {
        let runtime = Arc::new(SessionRuntime::default());
        // deny 场景。
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        runtime.asks.lock().unwrap().insert(
            4,
            crate::PendingAsk {
                sender: tx,
                request: kanzei_core::AskRequest::Permission {
                    action: "bash".into(),
                    resource: "rm -rf".into(),
                },
                action: "bash".into(),
                resource: "rm -rf".into(),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        // question 场景。
        let (tx2, mut rx2) = tokio::sync::oneshot::channel();
        runtime.asks.lock().unwrap().insert(
            5,
            crate::PendingAsk {
                sender: tx2,
                request: kanzei_core::AskRequest::Question {
                    question: "确认继续?".into(),
                    options: vec!["是".into(), "否".into()],
                    default: None,
                    multiple: false,
                },
                action: "question".into(),
                resource: "确认继续?".into(),
                project_root: PathBuf::from("."),
                session_id: "ses-1".into(),
            },
        );
        let mut runtimes = HashMap::new();
        runtimes.insert("ses-1".to_string(), runtime.clone());
        let runtimes = Arc::new(Mutex::new(runtimes));

        approval_answer(&runtimes, &json!({"id": 4, "reply": "deny"})).unwrap();
        match rx.try_recv() {
            Ok(kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny)) => {}
            other => panic!("deny 应送达 Deny,实得: {other:?}"),
        }

        approval_answer(&runtimes, &json!({"id": 5, "reply": "是"})).unwrap();
        match rx2.try_recv() {
            Ok(kanzei_core::AskResponse::Answer(text)) => assert_eq!(text, "是"),
            other => panic!("question 回答应送达 Answer,实得: {other:?}"),
        }
    }

    fn mobile_connection_test_project(label: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "kz-mobile-open-count-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei")).unwrap();
        let state_path = kanzei_core::project_state_path(&dir);
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        store.create_session("thread-open", "C:/p", None).unwrap();
        store
            .append_notification_atomic("thread-open", "completed", "done", false)
            .unwrap();
        drop(store);
        (dir, state_path)
    }

    fn mobile_connection_test_inputs() -> (
        Arc<Mutex<HashMap<String, String>>>,
        Arc<Mutex<Option<String>>>,
        Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
        Arc<AtomicBool>,
    ) {
        let mut devices = HashMap::new();
        devices.insert("dev-open".into(), "tok-open".into());
        (
            Arc::new(Mutex::new(devices)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(AtomicBool::new(true)),
        )
    }

    fn run_mobile_request(
        project_root: PathBuf,
        request: &str,
        devices: Arc<Mutex<HashMap<String, String>>>,
        pair_code: Arc<Mutex<Option<String>>>,
        runtimes: Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
        active: Arc<AtomicBool>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        let (server, _) = listener.accept().unwrap();
        let worker = std::thread::spawn(move || {
            handle_mobile_connection(server, project_root, devices, pair_code, runtimes, active)
        });
        client.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        (response, worker)
    }

    /// D-502:普通通知请求在读取 delivery cursor 与回放/推进时只打开一次 store。
    #[test]
    fn notifications请求单次连接复用() {
        let (dir, state_path) = mobile_connection_test_project("json");
        let baseline = kanzei_core::store_open_count(&state_path);
        let (devices, pair_code, runtimes, active) = mobile_connection_test_inputs();
        let (response, worker) = run_mobile_request(
            dir.clone(),
            "GET /v1/notifications?thread_id=thread-open&device_id=dev-open HTTP/1.1\r\nAuthorization: Bearer tok-open\r\n\r\n",
            devices,
            pair_code,
            runtimes,
            active,
        );
        worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert_eq!(
            kanzei_core::store_open_count(&state_path) - baseline,
            1,
            "普通通知请求应只打开一次 state store"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-502:SSE 首批事件真实经移动端入口发送后,整个轮询连接只打开一次 store。
    #[test]
    fn sse轮询单连接复用() {
        let (dir, state_path) = mobile_connection_test_project("sse");
        let baseline = kanzei_core::store_open_count(&state_path);
        let (devices, pair_code, runtimes, active) = mobile_connection_test_inputs();
        let stop = active.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        let (server, _) = listener.accept().unwrap();
        let worker = std::thread::spawn({
            let root = dir.clone();
            move || handle_mobile_connection(server, root, devices, pair_code, runtimes, active)
        });
        client
            .write_all(
                b"GET /v1/events?thread_id=thread-open&device_id=dev-open HTTP/1.1\r\nAuthorization: Bearer tok-open\r\n\r\n",
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        stop.store(false, Ordering::SeqCst);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(
            response.contains("data: "),
            "SSE 应发送首批事件: {response}"
        );
        assert_eq!(
            kanzei_core::store_open_count(&state_path) - baseline,
            1,
            "SSE 轮询连接应只打开一次 state store"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-501:游标只有持久化成功后才前进；故障注入时保留旧游标。
    #[test]
    fn delivery_cursor持久化失败不前进_成功后才更新() {
        let mut cursor = 7;
        let failed = persist_delivery_cursor_and_advance(&mut cursor, 8, || {
            Err("injected cursor write failure".into())
        });
        assert!(failed.is_err(), "注入的持久化失败必须向调用方报告");
        assert_eq!(cursor, 7, "持久化失败不得推进内存游标");

        persist_delivery_cursor_and_advance(&mut cursor, 8, || Ok(())).unwrap();
        assert_eq!(cursor, 8, "持久化成功后才推进内存游标");
    }

    /// D-387:手机消息消费方——注入 conversation.updated 事件持久化,桌面端
    /// conversation_get 可读到(不再死信)。用临时 state.db 验证事件落库。
    #[test]
    fn 手机消息消费_事件落库可读() {
        // 临时文件库(内存库无 project_state_path,用临时目录建 state.db)。
        let dir = std::env::temp_dir().join(format!(
            "kz-mobile-consume-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = kanzei_core::SessionStore::open(&dir.join("state.db")).unwrap();
        store.create_session("thread-x", "C:/p", None).unwrap();
        drop(store);

        // 消费:注入消息(空 runtimes=会话未在跑,仍应持久化事件)。
        let runtimes: Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        consume_mobile_message(&runtimes, "thread-x", "你好, 桌面!", &dir.join("state.db"));

        // 事件落库:conversation.updated 含注入的消息(conversation_get 数据源)。
        let store = kanzei_core::SessionStore::open(&dir.join("state.db")).unwrap();
        let events = store
            .list_events_by_type("thread-x", 0, "conversation.updated")
            .unwrap();
        assert!(!events.is_empty(), "必须产出 conversation.updated 事件");
        let last = events.last().unwrap();
        let payload: serde_json::Value = last.payload.clone();
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1, "注入一条消息");
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["parts"][0]["text"], "你好, 桌面!");
        std::fs::remove_dir_all(&dir).ok();
    }
}
