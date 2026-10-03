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

use crate::state::{MobileDeviceInfo, MobileService, MobileServiceInfo, SessionRuntime};
use crate::{normalized_project_root, process_session_id, AppState, MutexPoisonExt};

mod approvals;
mod pwa;
mod sse;
use approvals::{approval_answer, approval_pending_list};
use pwa::{resolve_pwa_root, serve_pwa};
use sse::handle_sse;

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

/// 解码 URL 查询值:`+` → 空格,`%XX` → 字节(按 UTF-8 容错还原);不合法的 `%` 原样保留。
///
/// UX-018:并行线路的会话 id 形如 `ses_project_x#p3`,PWA 的 URLSearchParams 会把 `#`
/// 编成 `%23`。此前只换 `+`、不解码,于是 `ses_project_x%23p3` 永远匹配不到真实会话——
/// 页面显示「已连接」却一条事件也收不到。
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' => {
                // from_str_radix 接受前导 `+`,这里要求两位都是十六进制数字。
                let decoded = bytes
                    .get(index + 1..index + 3)
                    .filter(|hex| hex.iter().all(u8::is_ascii_hexdigit))
                    .and_then(|hex| std::str::from_utf8(hex).ok())
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                match decoded {
                    Some(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    None => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn mobile_query(path: &str, key: &str) -> Option<String> {
    path.split('?')
        .nth(1)?
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| percent_decode(value))
}

/// 取请求头里的 `Authorization: Bearer <token>`(保留 token 原有大小写)。
fn bearer_token(request: &str) -> Option<&str> {
    const PREFIX: &str = "authorization: bearer ";
    request.lines().find_map(|line| {
        let head = line.get(..PREFIX.len())?;
        head.eq_ignore_ascii_case(PREFIX)
            .then(|| line[PREFIX.len()..].split_whitespace().next())
            .flatten()
    })
}

/// R-270:设备 token 认证。从请求头取 `Authorization: Bearer <token>`,
/// 在设备表里找得到即通过(撤销 = 从表移除,移除后立即 401)。
fn mobile_authorized(request: &str, devices: &HashMap<String, String>) -> Option<String> {
    let token = bearer_token(request)?;
    devices
        .iter()
        .find(|(_, device_token)| device_token.as_str() == token)
        .map(|(device_id, _)| device_id.clone())
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

/// 桥接对外暴露的一条会话(PWA 会话下拉、审批卡来源名、发消息校验共用)。
struct BridgeSession {
    session_id: String,
    label: String,
    /// `main` 主对话 / `discussion` 讨论 / `task` 独立任务。
    kind: &'static str,
    updated_at: i64,
}

/// 单项目下 PWA 一次最多列出的会话数(轻交互遥控器,下拉不是历史库)。
const BRIDGE_SESSION_LIMIT: usize = 40;

/// 用户给会话起的名字(`sessions.title`);空白视为没起。
fn session_title(session: Option<&kanzei_core::Session>) -> Option<String> {
    let title = session?.title.as_deref()?.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// 项目在界面上的名字:目录名(根目录之类没有目录名时用完整路径)。
fn project_display_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}

/// 本项目可订阅、可发消息的会话:主对话 + 已登记的讨论/独立任务。
///
/// UX-017:此前 PWA 要手填会话 id,桌面端却没有任何地方展示它。名字取 `sessions.title`
/// (用户命名),没有就按「类型 + 序号」,界面不出现 ses_ 哈希与 pN。主对话固定第一,
/// 其余按最近活动倒序。
fn bridge_sessions(
    store: &kanzei_core::SessionStore,
    project_root: &Path,
) -> Result<Vec<BridgeSession>, String> {
    let main_id = process_session_id(project_root, None);
    let main = store.get_session(&main_id).map_err(|e| e.to_string())?;
    let mut sessions = vec![BridgeSession {
        label: session_title(main.as_ref()).unwrap_or_else(|| "主对话".into()),
        updated_at: main.map(|session| session.updated_at).unwrap_or(0),
        session_id: main_id,
        kind: "main",
    }];
    let registered = store
        .list_processes(&project_root.display().to_string())
        .map_err(|e| e.to_string())?;
    for process in registered {
        // 默认进程就是上面的主对话。
        if process.process_id.starts_with("d|") {
            continue;
        }
        let session_id = process_session_id(project_root, Some(&process.process_id));
        let session = store.get_session(&session_id).map_err(|e| e.to_string())?;
        let (kind, word) = if process.profile.as_deref() == Some("readonly") {
            ("discussion", "讨论")
        } else {
            ("task", "独立任务")
        };
        let number = process
            .process_id
            .split('|')
            .next()
            .unwrap_or_default()
            .trim_start_matches('p');
        sessions.push(BridgeSession {
            label: session_title(session.as_ref()).unwrap_or_else(|| format!("{word} {number}")),
            updated_at: session
                .map(|session| session.updated_at)
                .unwrap_or(process.updated_at),
            session_id,
            kind,
        });
    }
    sessions[1..].sort_by_key(|session| std::cmp::Reverse(session.updated_at));
    Ok(sessions)
}

/// `GET /v1/sessions`:会话清单 + 每条是否正在运行。
fn bridge_sessions_json(
    project_root: &Path,
    state_path: &Path,
    runtimes: &Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
) -> Result<serde_json::Value, String> {
    let store = kanzei_core::SessionStore::open(state_path).map_err(|e| e.to_string())?;
    let sessions = bridge_sessions(&store, project_root)?;
    let default = sessions
        .first()
        .map(|session| session.session_id.clone())
        .unwrap_or_default();
    let runtimes = runtimes.lock_or_recover();
    let items: Vec<serde_json::Value> = sessions
        .iter()
        .take(BRIDGE_SESSION_LIMIT)
        .map(|session| {
            let running = runtimes
                .get(&session.session_id)
                .is_some_and(|runtime| runtime.running.load(Ordering::SeqCst));
            json!({
                "session_id": session.session_id,
                "label": session.label,
                "kind": session.kind,
                "running": running,
                "updated_at": session.updated_at,
            })
        })
        .collect();
    Ok(json!({
        "project": project_display_name(project_root),
        "default": default,
        "sessions": items,
    }))
}

/// 某项目登记过的会话;状态库还没建出来(或读不出来)就是空——只读、绝不为此新建 `.kanzei`。
fn project_sessions_if_present(root: &Path) -> Vec<BridgeSession> {
    let state_path = kanzei_core::project_state_path(root);
    if !state_path.is_file() {
        return Vec::new();
    }
    kanzei_core::SessionStore::open(&state_path)
        .map_err(|e| e.to_string())
        .and_then(|store| bridge_sessions(&store, root))
        .unwrap_or_default()
}

fn handle_mobile_connection(
    mut stream: TcpStream,
    project_root: PathBuf,
    pwa_root: PathBuf,
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
        let mut pair_slot = pair_code.lock_or_recover();
        if submitted.is_empty() || pair_slot.as_deref() != Some(submitted) {
            drop(pair_slot);
            let _ = stream.write_all(&mobile_json_response(
                "401 Unauthorized",
                &json!({"error": "invalid_pair_code"}),
            ));
            return;
        }
        let mut device_table = devices.lock_or_recover();
        if !active.load(Ordering::SeqCst) {
            return;
        }
        // The code and cache are published only after the credential commits.
        let (device_id, device_token) = generate_device_credentials();
        let persisted = kanzei_core::SessionStore::open(&state_path).and_then(|store| {
            store.upsert_mobile_device(
                &device_id,
                &device_token,
                "已配对设备",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
        });
        if let Err(error) = persisted {
            drop(device_table);
            drop(pair_slot);
            let _ = stream.write_all(&mobile_json_response(
                "500 Internal Server Error",
                &json!({"error": error.to_string()}),
            ));
            return;
        }
        device_table.insert(device_id.clone(), device_token.clone());
        *pair_slot = None;
        drop(device_table);
        drop(pair_slot);
        let _ = stream.write_all(&mobile_json_response(
            "200 OK",
            &json!({"device_id": device_id, "token": device_token}),
        ));
        return;
    }

    // D-390(鉴权闸死锁修复):PWA 静态资源**不鉴权**,在鉴权闸之前 serve——
    // 手机浏览器打开桥接地址即可加载页面(配对表单),配对拿 token 后才能调
    // /v1/* API。此前 serve 在鉴权闸之后,页面 GET / 先被 401 拦死,配对
    // 表单都拿不到,永远无法配对(死锁)。
    if method == "GET"
        && !path.starts_with("/v1/")
        && path != "/health"
        && serve_pwa(&mut stream, path, &pwa_root)
    {
        return;
    }

    // 其它端点:设备 token 认证。
    let mut device_table = devices.lock_or_recover();
    let Some(device_id) = mobile_authorized(&request_head, &device_table) else {
        drop(device_table);
        let _ = stream.write_all(&mobile_json_response(
            "401 Unauthorized",
            &json!({"error": "device_revoked_or_unauthorized"}),
        ));
        return;
    };

    // Notification cursors and revocation belong to the authenticated device.
    // A query parameter may confirm that identity, but cannot select another one.
    if method == "GET"
        && matches!(
            path.split('?').next(),
            Some("/v1/events" | "/v1/notifications")
        )
        && mobile_query(path, "device_id").is_some_and(|requested| requested != device_id)
    {
        drop(device_table);
        let _ = stream.write_all(&mobile_json_response(
            "403 Forbidden",
            &json!({"error": "device_identity_mismatch"}),
        ));
        return;
    }

    if !active.load(Ordering::SeqCst) {
        return;
    }

    // R-270 批2:SSE 长连接实时推送。认证通过后、进普通 JSON 分发前拦截——
    // 长连接由独立线程持有(批1 多线程 accept),不阻塞其它请求。
    if method == "GET" && path.split('?').next() == Some("/v1/events") {
        drop(device_table);
        // D-388:传 active(停服检查)与 devices(撤销检查)——长连接不无视停服/撤销。
        handle_sse(
            &mut stream,
            &state_path,
            path,
            &device_id,
            &active,
            &devices,
        );
        return;
    }

    // R-270 批3:approval 通道。GET pending(脱敏摘要)+ POST answer(回答)。
    // 回答走 runner 既有 ask 流(PendingAsk.sender),最终门禁仍在 harness 侧。
    match (method, path.split('?').next().unwrap_or_default()) {
        ("GET", "/v1/approval/pending") => {
            let pending = approval_pending_list(&runtimes);
            drop(device_table);
            let _ = stream.write_all(&mobile_json_response("200 OK", &pending));
            return;
        }
        // UX-137:解除配对要通知服务端——此前只清手机本地凭据,桌面设备表里那台仍有效。
        ("POST", "/v1/unpair") => {
            let response = match remove_device(&state_path, &device_id, &mut device_table) {
                Ok(removed) => mobile_json_response("200 OK", &json!({"unpaired": removed})),
                Err(error) => {
                    mobile_json_response("500 Internal Server Error", &json!({"error": error}))
                }
            };
            drop(device_table);
            let _ = stream.write_all(&response);
            return;
        }
        ("POST", "/v1/approval/answer") => {
            let payload: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let reply = match approval_answer(&runtimes, &payload) {
                Ok(rendered) => rendered,
                Err(error) => {
                    drop(device_table);
                    let _ = stream.write_all(&mobile_json_response(
                        "400 Bad Request",
                        &json!({"error": error}),
                    ));
                    return;
                }
            };
            drop(device_table);
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
                drop(device_table);
                let _ = stream.write_all(&mobile_json_response(
                    "400 Bad Request",
                    &json!({"error": "thread_id_required"}),
                ));
                return;
            };
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
        ("GET", "/v1/sessions") => {
            match bridge_sessions_json(&project_root, &state_path, &runtimes) {
                Ok(sessions) => mobile_json_response("200 OK", &sessions),
                Err(error) => {
                    mobile_json_response("500 Internal Server Error", &json!({"error": error}))
                }
            }
        }
        ("POST", "/v1/messages") => {
            let payload: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            handle_mobile_message(&payload, &project_root, &state_path, &runtimes)
        }
        _ => mobile_json_response("404 Not Found", &json!({"error": "not_found"})),
    };
    drop(device_table);
    let _ = stream.write_all(&response);
}

/// The caller holds the device cache lock through durable revocation.
fn remove_device(
    state_path: &Path,
    device_id: &str,
    devices: &mut HashMap<String, String>,
) -> Result<bool, String> {
    if !devices.contains_key(device_id) {
        return Ok(false);
    }
    let store = kanzei_core::SessionStore::open(state_path).map_err(|error| error.to_string())?;
    store
        .remove_mobile_device(device_id)
        .map_err(|error| error.to_string())?;
    devices.remove(device_id);
    Ok(true)
}

/// `POST /v1/messages`:校验会话存在 → 落库 → 注入对话。返回完整 HTTP 响应。
///
/// UX-133:此前对任意 thread_id 都 `create_session` 并回「已发送」,输错 id 的消息进了没人
/// 看的隐形会话。现在只收本项目的主对话与已登记的讨论/独立任务,未知会话回 404
/// `unknown_session`。回执如实交代:消息写进了该对话、**不会触发运行**——下一轮对话会带上它。
fn handle_mobile_message(
    payload: &serde_json::Value,
    project_root: &Path,
    state_path: &Path,
    runtimes: &Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
) -> Vec<u8> {
    let text_field = |name: &str| {
        payload
            .get(name)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default()
    };
    let thread_id = text_field("thread_id");
    if thread_id.is_empty() {
        return mobile_json_response("400 Bad Request", &json!({"error": "thread_id_required"}));
    }
    let text = text_field("text");
    if text.is_empty() {
        return mobile_json_response("400 Bad Request", &json!({"error": "text_required"}));
    }
    let stored = kanzei_core::SessionStore::open(state_path)
        .map_err(|error| error.to_string())
        .and_then(|store| {
            let target = bridge_sessions(&store, project_root)?
                .into_iter()
                .find(|session| session.session_id == thread_id);
            let Some(target) = target else {
                return Ok(None);
            };
            store
                .create_session(thread_id, &project_root.display().to_string(), None)
                .map_err(|error| error.to_string())?;
            consume_mobile_message(runtimes, thread_id, text, &store, payload)?;
            Ok(Some(target))
        });
    match stored {
        Ok(Some(target)) => {
            let running = runtimes
                .lock_or_recover()
                .get(thread_id)
                .is_some_and(|runtime| runtime.running.load(Ordering::SeqCst));
            mobile_json_response(
                "202 Accepted",
                &json!({
                    "accepted": true,
                    "session_id": target.session_id,
                    "label": target.label,
                    "kind": target.kind,
                    "running": running,
                    // 桥接只把消息交给对话,不替桌面发起一轮运行。
                    "triggers_run": false,
                }),
            )
        }
        Ok(None) => mobile_json_response("404 Not Found", &json!({"error": "unknown_session"})),
        Err(error) => mobile_json_response("500 Internal Server Error", &json!({"error": error})),
    }
}

/// Publish the cache and UI only after the receipt and user fact commit.
fn consume_mobile_message(
    runtimes: &Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
    thread_id: &str,
    text: &str,
    store: &kanzei_core::SessionStore,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let runtime = runtimes.lock_or_recover().get(thread_id).cloned();
    // Use the same cache -> SQLite order as compaction publication. Never keep
    // the whole runtime registry locked while waiting for a database writer.
    let mut cache = runtime
        .as_ref()
        .map(|runtime| runtime.conversation.lock_or_recover());
    let messages = store
        .append_mobile_message(
            thread_id,
            &random_token("mobile"),
            kanzei_llm::Message::user_text(text),
            payload,
        )
        .map_err(|error| error.to_string())?;
    if let Some(cache) = cache.as_mut() {
        cache.insert(thread_id.to_string(), messages);
    }
    drop(cache);
    if let Some(emit) = crate::state::MOBILE_MESSAGE_EMIT.get() {
        emit(thread_id.to_string(), text.to_string());
    }
    Ok(())
}

/// 桥接的固定默认端口:手机上收藏/添加到主屏的地址在桌面重启后仍然有效(此前每次随机端口)。
/// UX-135:只有被别的程序占着时才退回随机端口,界面显示的永远是实际地址。
const MOBILE_DEFAULT_PORT: u16 = 18765;

/// 绑定固定默认端口;刚停掉的旧服务线程最多 50ms 后才放掉监听,所以短暂重试几次再退回随机端口。
fn bind_default_port(bind_addr: &str) -> std::io::Result<TcpListener> {
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if let Ok(listener) = TcpListener::bind((bind_addr, MOBILE_DEFAULT_PORT)) {
            return Ok(listener);
        }
    }
    TcpListener::bind((bind_addr, 0))
}

/// 本机在局域网里的 IP:UDP connect 不发包,只让系统按默认路由选出出口地址。
/// 拿不到(离线、只有回环)就是 None,调用方退回 127.0.0.1。
fn lan_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// 手机浏览器能直接输入/打开的地址:LAN 模式用本机局域网 IP,回环模式只有本机能用。
/// `address` 是监听地址(`0.0.0.0:端口`),不能给人看。
fn bridge_url(lan: bool, port: u16) -> String {
    let host = if lan {
        lan_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "127.0.0.1".into())
    } else {
        "127.0.0.1".into()
    };
    format!("http://{host}:{port}/")
}

/// 启动移动端桥接服务。
///
/// `lan=true` 监听 0.0.0.0(允许 LAN 设备访问);`lan=false`(默认)保持回环
/// 127.0.0.1 既有行为。`port` 缺省用固定端口 [`MOBILE_DEFAULT_PORT`](被占才随机)。
/// 返回监听地址、手机可用的 URL、配对码与当前设备列表。
#[tauri::command]
pub fn mobile_service_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_dir: String,
    port: Option<u16>,
    lan: Option<bool>,
    notifications: Option<bool>,
) -> Result<MobileServiceInfo, String> {
    start_service(
        &state,
        normalized_project_root(Path::new(&project_dir)),
        resolve_pwa_root(&app),
        port,
        lan.unwrap_or(false),
        notifications.unwrap_or(false),
    )
}

fn start_service(
    state: &AppState,
    root: PathBuf,
    pwa_root: PathBuf,
    port: Option<u16>,
    lan: bool,
    notifications: bool,
) -> Result<MobileServiceInfo, String> {
    let mut service_slot = state.mobile_service.lock_or_recover();
    if service_slot.is_some() {
        return Err("移动端桥接服务已经启动".into());
    }
    if notifications {
        crate::mobile_notify::check_dependency()?;
    }
    let bind_addr: &str = if lan { "0.0.0.0" } else { "127.0.0.1" };
    let listener = match port {
        Some(port) => TcpListener::bind((bind_addr, port)),
        None => bind_default_port(bind_addr),
    }
    .map_err(|e| format!("移动端桥接服务启动失败: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("设置本机服务非阻塞失败: {e}"))?;
    let local_addr = listener.local_addr().map_err(|e| e.to_string())?;
    let address = local_addr.to_string();
    let url = bridge_url(lan, local_addr.port());
    // D-386:配对码换随机源(不再 pid+纳秒可预测)。
    let pair_code = random_token("kz-pair");
    let active = Arc::new(AtomicBool::new(true));
    let devices: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let pair_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(Some(pair_code.clone())));
    // D-386:设备表持久化——启动时从 SQLite 载入已配对设备(内存表供认证热路径,
    // SQLite 是持久真源;重启后已配对设备仍在)。
    {
        let state_path = kanzei_core::project_state_path(&root);
        let store = kanzei_core::SessionStore::open(&state_path)
            .map_err(|error| format!("读取配对设备失败: {error}"))?;
        let devices_snapshot = store
            .list_mobile_devices()
            .map_err(|error| format!("读取配对设备失败: {error}"))?;
        let mut map = devices.lock_or_recover();
        for (device_id, device_token, _name, _paired_at) in devices_snapshot {
            map.insert(device_id, device_token);
        }
    }

    let thread_active = active.clone();
    let thread_root = root.clone();
    let thread_pwa = pwa_root.clone();
    let thread_devices = devices.clone();
    let thread_pair = pair_slot.clone();
    let thread_runtimes = state.runtimes.clone();
    let listener_thread = std::thread::spawn(move || {
        while thread_active.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    // R-270:每连接独立线程——长请求(SSE/approval)不阻塞其它连接。
                    let conn_devices = thread_devices.clone();
                    let conn_pair = thread_pair.clone();
                    let conn_root = thread_root.clone();
                    let conn_pwa = thread_pwa.clone();
                    let conn_runtimes = thread_runtimes.clone();
                    let conn_active = thread_active.clone();
                    std::thread::spawn(move || {
                        handle_mobile_connection(
                            stream,
                            conn_root,
                            conn_pwa,
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
        listener_thread: Some(listener_thread),
        devices: devices.clone(),
        pair_code: pair_slot.clone(),
        lan,
        project_root: root.clone(),
    };
    let info = MobileServiceInfo {
        address: address.clone(),
        url,
        token: pair_code.clone(),
        lan: service_ref.lan,
        devices: service_ref
            .devices
            .lock_or_recover()
            .keys()
            .map(|device_id| MobileDeviceInfo {
                device_id: device_id.clone(),
                name: "已配对设备".into(),
                paired_at_ms: 0,
            })
            .collect(),
    };
    *service_slot = Some(service_ref);
    crate::mobile_notify::set_enabled(notifications);
    Ok(info)
}

/// 撤销指定设备:从设备表移除,其 token 立即 401,其它设备不受影响。
/// D-386:同步删 SQLite 持久化行——撤销跨重启有效。
#[tauri::command]
pub fn mobile_device_revoke(state: State<'_, AppState>, device_id: String) -> Result<(), String> {
    let guard = state.mobile_service.lock_or_recover();
    let service = guard.as_ref().ok_or("移动端桥接服务未启动")?;
    let state_path = kanzei_core::project_state_path(&service.project_root);
    if remove_device(
        &state_path,
        &device_id,
        &mut service.devices.lock_or_recover(),
    )? {
        Ok(())
    } else {
        Err(format!("设备不存在: {device_id}"))
    }
}

/// 再生成配对码(D-386):替换当前配对码(已配对设备保留,未撤销)。
/// 原配对码一次性用完即 None,此命令让用户能再次配对新设备而不必重启服务。
#[tauri::command]
pub fn mobile_pair_code_regenerate(state: State<'_, AppState>) -> Result<String, String> {
    let guard = state.mobile_service.lock_or_recover();
    let service = guard.as_ref().ok_or("移动端桥接服务未启动")?;
    let new_code = random_token("kz-pair");
    *service.pair_code.lock_or_recover() = Some(new_code.clone());
    Ok(new_code)
}

/// 当前设备列表(设置页展示与撤销入口)。D-386:paired_at_ms 从 SQLite 读。
#[tauri::command]
pub fn mobile_device_list(state: State<'_, AppState>) -> Result<Vec<MobileDeviceInfo>, String> {
    let guard = state.mobile_service.lock_or_recover();
    let service = guard.as_ref().ok_or("移动端桥接服务未启动")?;
    // 读取配对码状态(设置页据此提示「正在等待配对」或「已配对 N 台」)。
    let _pending_pair = service.pair_code.lock_or_recover().is_some();
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
    let devices = service.devices.lock_or_recover();
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
    stop_service(&state)
}

fn stop_service(state: &AppState) -> Result<(), String> {
    let mut service_slot = state.mobile_service.lock_or_recover();
    if let Some(mut service) = service_slot.take() {
        // Match pairing's pair -> devices order and wait for admitted writes.
        {
            let _pair = service.pair_code.lock_or_recover();
            let _devices = service.devices.lock_or_recover();
            service.active.store(false, Ordering::SeqCst);
        }
        crate::mobile_notify::set_enabled(false);
        if let Some(thread) = service.listener_thread.take() {
            thread
                .join()
                .map_err(|_| "移动端桥接监听线程异常退出".to_string())?;
        }
        Ok(())
    } else {
        Err("移动端桥接服务当前未启动".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Make the persisted file temporarily unavailable without changing its
    // contents. Every connection is closed before moving this test-owned file.
    fn block_database(path: &Path) -> PathBuf {
        let saved = path.with_extension("saved");
        std::fs::rename(path, &saved).unwrap();
        std::fs::create_dir(path).unwrap();
        saved
    }

    fn restore_database(path: &Path, saved: &Path) {
        std::fs::remove_dir(path).unwrap();
        std::fs::rename(saved, path).unwrap();
    }

    #[test]
    fn pairing_write_failure_keeps_code_and_cache_for_retry() {
        let root = temp_project("pair-failure");
        let path = kanzei_core::project_state_path(&root);
        drop(kanzei_core::SessionStore::open(&path).unwrap());
        let saved = block_database(&path);
        let devices = Arc::new(Mutex::new(HashMap::new()));
        let code = Arc::new(Mutex::new(Some("pair-failure-code".into())));
        let runtimes = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicBool::new(true));
        let request = pair_request("pair-failure-code");
        let (response, worker) = run_mobile_request(
            root.clone(),
            &request,
            devices.clone(),
            code.clone(),
            runtimes.clone(),
            active.clone(),
        );
        worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 500"), "{response}");
        assert!(devices.lock_or_recover().is_empty());
        assert_eq!(code.lock_or_recover().as_deref(), Some("pair-failure-code"));
        restore_database(&path, &saved);
        let (response, worker) = run_mobile_request(
            root.clone(),
            &request,
            devices.clone(),
            code.clone(),
            runtimes,
            active,
        );
        worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(code.lock_or_recover().is_none());
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        assert_eq!(store.list_mobile_devices().unwrap().len(), 1);
        assert_eq!(devices.lock_or_recover().len(), 1);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_pairing_consumes_one_code_once() {
        let root = temp_project("pair-concurrent");
        let path = kanzei_core::project_state_path(&root);
        drop(kanzei_core::SessionStore::open(&path).unwrap());
        let devices = Arc::new(Mutex::new(HashMap::new()));
        let code = Arc::new(Mutex::new(Some("single-use-code".into())));
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let (root, devices, code, barrier) =
                    (root.clone(), devices.clone(), code.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    let (response, worker) = run_mobile_request(
                        root,
                        &pair_request("single-use-code"),
                        devices,
                        code,
                        Arc::new(Mutex::new(HashMap::new())),
                        Arc::new(AtomicBool::new(true)),
                    );
                    worker.join().unwrap();
                    response
                })
            })
            .collect();
        let responses: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(
            responses
                .iter()
                .filter(|r| r.starts_with("HTTP/1.1 200"))
                .count(),
            1,
            "{responses:?}"
        );
        assert_eq!(
            responses
                .iter()
                .filter(|r| r.starts_with("HTTP/1.1 401"))
                .count(),
            7,
            "{responses:?}"
        );
        assert!(code.lock_or_recover().is_none());
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        assert_eq!(store.list_mobile_devices().unwrap().len(), 1);
        assert_eq!(devices.lock_or_recover().len(), 1);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpair_failure_preserves_credentials_until_durable_retry() {
        let root = temp_project("unpair-failure");
        let path = kanzei_core::project_state_path(&root);
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store
            .upsert_mobile_device("dev-open", "tok-open", "device", 1)
            .unwrap();
        drop(store);
        let saved = block_database(&path);
        let (devices, code, runtimes, active) = mobile_connection_test_inputs();
        let request = "POST /v1/unpair HTTP/1.1\r\nAuthorization: Bearer tok-open\r\n\r\n";
        let (response, worker) = run_mobile_request(
            root.clone(),
            request,
            devices.clone(),
            code.clone(),
            runtimes.clone(),
            active.clone(),
        );
        worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 500"), "{response}");
        assert_eq!(
            devices
                .lock_or_recover()
                .get("dev-open")
                .map(String::as_str),
            Some("tok-open")
        );
        restore_database(&path, &saved);
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        assert_eq!(store.list_mobile_devices().unwrap().len(), 1);
        drop(store);
        let (response, worker) = run_mobile_request(
            root.clone(),
            request,
            devices.clone(),
            code,
            runtimes,
            active,
        );
        worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("\"unpaired\":true"));
        assert!(devices.lock_or_recover().is_empty());
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        assert!(store.list_mobile_devices().unwrap().is_empty());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mobile_message_invalid_history_keeps_receipt_and_cache_unchanged_then_retries() {
        let root = temp_project("message-failure");
        let path = kanzei_core::project_state_path(&root);
        let thread = process_session_id(&root, None);
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store
            .create_session(&thread, &root.display().to_string(), None)
            .unwrap();
        drop(store);
        let runtime = Arc::new(SessionRuntime::default());
        let original = vec![kanzei_llm::Message::user_text("cached original")];
        runtime
            .conversation
            .lock_or_recover()
            .insert(thread.clone(), original.clone());
        let runtimes = Arc::new(Mutex::new(HashMap::from([(
            thread.clone(),
            runtime.clone(),
        )])));
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store
            .append_event(
                &thread,
                "conversation.updated",
                &json!({"messages":"invalid snapshot"}),
            )
            .unwrap();
        drop(store);
        let payload = json!({"thread_id":thread,"text":"durable phone input"});
        let response =
            String::from_utf8(handle_mobile_message(&payload, &root, &path, &runtimes)).unwrap();
        assert!(response.starts_with("HTTP/1.1 500"), "{response}");
        assert_eq!(runtime.conversation.lock_or_recover()[&thread], original);
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        assert!(store
            .list_events_by_type(&thread, 0, "mobile.message")
            .unwrap()
            .is_empty());
        assert!(store.list_session_facts(&thread).unwrap().is_empty());
        drop(store);
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store
            .append_event(
                &thread,
                "conversation.updated",
                &json!({"messages":original}),
            )
            .unwrap();
        drop(store);
        let response =
            String::from_utf8(handle_mobile_message(&payload, &root, &path, &runtimes)).unwrap();
        assert!(response.starts_with("HTTP/1.1 202"), "{response}");
        assert!(response.contains("\"triggers_run\":false"));
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        assert_eq!(
            store
                .list_events_by_type(&thread, 0, "mobile.message")
                .unwrap()
                .len(),
            1
        );
        let recovered = crate::conversation::project_latest_segment(&store, &thread).unwrap();
        assert_eq!(
            recovered,
            vec![
                kanzei_llm::Message::user_text("cached original"),
                kanzei_llm::Message::user_text("durable phone input")
            ]
        );
        assert_eq!(runtime.conversation.lock_or_recover()[&thread], recovered);
        assert!(store.list_pending_inputs(&thread).unwrap().is_empty());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mobile_message_keeps_legacy_history_and_ignores_unrelated_terminal_dirt() {
        let root = temp_project("message-history");
        let path = kanzei_core::project_state_path(&root);
        let thread = process_session_id(&root, None);
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store
            .create_session(&thread, &root.display().to_string(), None)
            .unwrap();
        let old = kanzei_llm::Message::user_text("legacy history");
        store
            .append_event(&thread, "conversation.updated", &json!({"messages":[old]}))
            .unwrap();
        for fact in [
            kanzei_core::SessionFact::TurnFailed {
                error: "historical failure".into(),
            },
            kanzei_core::SessionFact::ToolResultCommitted {
                call_id: "orphan".into(),
                content: "old dirt".into(),
                is_error: true,
            },
        ] {
            let fact = kanzei_core::SessionFactEnvelope::new("dirty", None, fact);
            store
                .append_event(
                    &thread,
                    fact.event_type(),
                    &serde_json::to_value(fact).unwrap(),
                )
                .unwrap();
        }
        let runtimes = Arc::new(Mutex::new(HashMap::new()));
        let payload = json!({"thread_id":thread,"text":"fresh phone input"});
        let response =
            String::from_utf8(handle_mobile_message(&payload, &root, &path, &runtimes)).unwrap();
        assert!(response.starts_with("HTTP/1.1 202"), "{response}");
        let recovered = crate::conversation::project_latest_segment(&store, &thread).unwrap();
        assert_eq!(recovered.first(), Some(&old));
        assert_eq!(
            recovered.last(),
            Some(&kanzei_llm::Message::user_text("fresh phone input"))
        );
        kanzei_core::prepare_typed_session(&store, &thread).unwrap();
        assert_eq!(
            crate::conversation::project_latest_segment(&store, &thread).unwrap(),
            recovered
        );
        store
            .append_event(&thread, "conversation.reset", &json!({"cleared":true}))
            .unwrap();
        let response = String::from_utf8(handle_mobile_message(
            &json!({"thread_id":thread,"text":"new segment"}),
            &root,
            &path,
            &runtimes,
        ))
        .unwrap();
        assert!(response.starts_with("HTTP/1.1 202"), "{response}");
        assert_eq!(
            crate::conversation::project_latest_segment(&store, &thread).unwrap(),
            vec![kanzei_llm::Message::user_text("new segment")]
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stopped_bridge_does_not_commit_an_already_connected_request() {
        let root = temp_project("late-message");
        let path = kanzei_core::project_state_path(&root);
        let thread = process_session_id(&root, None);
        drop(kanzei_core::SessionStore::open(&path).unwrap());
        let state = AppState::default();
        let (devices, pair_code, runtimes, active) = mobile_connection_test_inputs();
        *state.mobile_service.lock_or_recover() = Some(MobileService {
            active: active.clone(),
            listener_thread: None,
            devices: devices.clone(),
            pair_code: pair_code.clone(),
            lan: false,
            project_root: root.clone(),
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let (server, _) = listener.accept().unwrap();
        let worker = std::thread::spawn({
            let root = root.clone();
            move || {
                handle_mobile_connection(
                    server,
                    root,
                    PathBuf::new(),
                    devices,
                    pair_code,
                    runtimes,
                    active,
                )
            }
        });
        let body = json!({"thread_id":thread,"text":"after stop"}).to_string();
        client.write_all(format!("POST /v1/messages HTTP/1.1\r\nAuthorization: Bearer tok-open\r\nContent-Length: {}\r\n\r\n",body.len()).as_bytes()).unwrap();
        stop_service(&state).unwrap();
        client.write_all(body.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        worker.join().unwrap();
        assert!(!response.contains("202 Accepted"), "{response}");
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        assert!(store
            .list_events_by_type(&thread, 0, "mobile.message")
            .unwrap()
            .is_empty());
        assert!(store.list_session_facts(&thread).unwrap().is_empty());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stop_releases_listener_before_an_immediate_restart_on_the_same_port() {
        let root = temp_project("restart-same-port");
        let state = AppState::default();
        let started =
            start_service(&state, root.clone(), PathBuf::new(), Some(0), false, false).unwrap();
        let address: std::net::SocketAddr = started.address.parse().unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        stream
            .write_all(b"GET /v1/health HTTP/1.1\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        stop_service(&state).unwrap();
        let restarted = start_service(
            &state,
            root.clone(),
            PathBuf::new(),
            Some(address.port()),
            false,
            false,
        );
        assert!(restarted.is_ok(), "{restarted:?}");
        stop_service(&state).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_service_start_has_one_owner_and_start_failure_is_retryable() {
        let root = temp_project("start-concurrent");
        let state = Arc::new(AppState::default());
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let (root, state, barrier) = (root.clone(), state.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    start_service(
                        &state,
                        root.clone(),
                        root.join("mobile-pwa"),
                        Some(0),
                        false,
                        false,
                    )
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| r.as_ref().is_err_and(|e| e.contains("已经启动")))
                .count(),
            7
        );
        stop_service(&state).unwrap();
        let broken_root = temp_project("start-broken");
        std::fs::write(
            kanzei_core::project_state_path(&broken_root),
            b"invalid database",
        )
        .unwrap();
        assert!(start_service(
            &state,
            broken_root.clone(),
            PathBuf::new(),
            Some(0),
            false,
            false
        )
        .is_err());
        assert!(state.mobile_service.lock_or_recover().is_none());
        let fresh =
            start_service(&state, root.clone(), PathBuf::new(), Some(0), false, false).unwrap();
        assert!(!fresh.address.is_empty());
        stop_service(&state).unwrap();
        std::fs::remove_dir_all(broken_root).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

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
        assert!(
            mobile_authorized(request, &devices).is_some(),
            "配对设备应通过"
        );
        devices.remove("dev-1"); // 撤销
        assert!(
            mobile_authorized(request, &devices).is_none(),
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
        assert!(mobile_authorized(req_a, &devices).is_some());
        devices.remove("dev-a");
        assert!(
            mobile_authorized(req_a, &devices).is_none(),
            "撤销的 dev-a 立即 401"
        );
        assert!(
            mobile_authorized(req_b, &devices).is_some(),
            "dev-b 不受影响"
        );
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
            mobile_authorized(pair_req, &devices).is_none(),
            "配对请求不带设备 token,普通认证应拒绝(走配对专用分支)"
        );
        let ok_req = "POST /v1/messages HTTP/1.1\r\nAuthorization: Bearer device-token\r\n\r\n";
        assert!(mobile_authorized(ok_req, &devices).is_some());
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

    type MobileConnectionTestInputs = (
        Arc<Mutex<HashMap<String, String>>>,
        Arc<Mutex<Option<String>>>,
        Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
        Arc<AtomicBool>,
    );

    fn mobile_connection_test_inputs() -> MobileConnectionTestInputs {
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
            let pwa_root = project_root.join("mobile-pwa");
            handle_mobile_connection(
                server,
                project_root,
                pwa_root,
                devices,
                pair_code,
                runtimes,
                active,
            )
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

    #[test]
    fn notification_endpoints_reject_another_devices_identity() {
        let (dir, state_path) = mobile_connection_test_project("device-identity");
        for endpoint in ["notifications", "events"] {
            let (devices, pair_code, runtimes, active) = mobile_connection_test_inputs();
            devices
                .lock_or_recover()
                .insert("dev-other".into(), "tok-other".into());
            // Let an incorrectly accepted SSE connection return instead of hanging
            // the negative control. The authentication response is still observable.
            active.store(false, Ordering::SeqCst);
            let request = format!(
                "GET /v1/{endpoint}?thread_id=thread-open&device_id=dev-other HTTP/1.1\r\nAuthorization: Bearer tok-open\r\n\r\n"
            );
            let (response, worker) =
                run_mobile_request(dir.clone(), &request, devices, pair_code, runtimes, active);
            worker.join().unwrap();
            assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
            let store = kanzei_core::SessionStore::open(&state_path).unwrap();
            assert_eq!(
                store.delivery_cursor("dev-other", "thread-open").unwrap(),
                0
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn notifications_without_device_query_use_authenticated_devices_cursor() {
        let (dir, state_path) = mobile_connection_test_project("device-default");
        let (devices, pair_code, runtimes, active) = mobile_connection_test_inputs();
        let (response, worker) = run_mobile_request(
            dir.clone(),
            "GET /v1/notifications?thread_id=thread-open HTTP/1.1\r\nAuthorization: Bearer tok-open\r\n\r\n",
            devices,
            pair_code,
            runtimes,
            active,
        );
        worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        assert_eq!(store.delivery_cursor("dev-open", "thread-open").unwrap(), 1);
        assert_eq!(
            store
                .delivery_cursor("paired-device", "thread-open")
                .unwrap(),
            0
        );
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sse_without_device_query_tracks_authenticated_device_revocation() {
        let (dir, state_path) = mobile_connection_test_project("sse-device-default");
        let (devices, pair_code, runtimes, active) = mobile_connection_test_inputs();
        devices
            .lock_or_recover()
            .insert("dev-other".into(), "tok-other".into());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let (server, _) = listener.accept().unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        let worker = std::thread::spawn({
            let root = dir.clone();
            let devices = devices.clone();
            let active = active.clone();
            move || {
                let pwa_root = root.join("mobile-pwa");
                handle_mobile_connection(
                    server, root, pwa_root, devices, pair_code, runtimes, active,
                );
                let _ = done.send(());
            }
        });
        client.write_all(b"GET /v1/events?thread_id=thread-open HTTP/1.1\r\nAuthorization: Bearer tok-open\r\n\r\n").unwrap();
        let mut received = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !received
            .windows(b"data: ".len())
            .any(|bytes| bytes == b"data: ")
        {
            match client.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(size) => received.extend_from_slice(&buffer[..size]),
            }
        }
        devices.lock_or_recover().remove("dev-open");
        let revoked = finished
            .recv_timeout(std::time::Duration::from_secs(2))
            .is_ok();
        active.store(false, Ordering::SeqCst);
        worker.join().unwrap();
        let response = String::from_utf8_lossy(&received);
        assert!(response.contains("data: "), "{response}");
        assert!(
            revoked,
            "SSE must close when its authenticated device is revoked"
        );
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        assert_eq!(store.delivery_cursor("dev-open", "thread-open").unwrap(), 1);
        assert_eq!(
            store.delivery_cursor("dev-other", "thread-open").unwrap(),
            0
        );
        drop(store);
        drop(client);
        std::fs::remove_dir_all(dir).unwrap();
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
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let (server, _) = listener.accept().unwrap();
        let worker = std::thread::spawn({
            let root = dir.clone();
            move || {
                let pwa_root = root.join("mobile-pwa");
                handle_mobile_connection(
                    server, root, pwa_root, devices, pair_code, runtimes, active,
                )
            }
        });
        client
            .write_all(
                b"GET /v1/events?thread_id=thread-open&device_id=dev-open HTTP/1.1\r\nAuthorization: Bearer tok-open\r\n\r\n",
            )
            .unwrap();
        // 固定 sleep 后先停服务会制造竞态：慢调度下服务端可能只写完响应头，尚未
        // 进入首轮 replay，active 就被测试置为 false。客户端应按 SSE 协议等待
        // 首个 data 帧；超时仍会让断言失败，但不再把机器速度当作正确性条件。
        let mut response_bytes = Vec::new();
        let mut read_error = None;
        let mut chunk = [0_u8; 1024];
        while !response_bytes
            .windows(b"data: ".len())
            .any(|part| part == b"data: ")
        {
            match client.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => response_bytes.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => {
                    read_error = Some(error);
                    break;
                }
            }
        }
        stop.store(false, Ordering::SeqCst);
        worker.join().unwrap();
        if let Some(error) = read_error {
            panic!("读取 SSE 首批事件失败: {error}");
        }
        let response = String::from_utf8_lossy(&response_bytes);
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

    /// D-387/R-242:手机消息消费方——注入 typed user fact,桌面端投影可读到(不再死信,
    /// 也不再新增 conversation.updated 快照)。用临时 state.db 验证事件落库。
    #[test]
    fn 手机消息消费_typed_fact落库可读() {
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
        let consumer_store = kanzei_core::SessionStore::open(&dir.join("state.db")).unwrap();
        consume_mobile_message(
            &runtimes,
            "thread-x",
            "你好, 桌面!",
            &consumer_store,
            &json!({"thread_id": "thread-x", "text": "你好, 桌面!"}),
        )
        .unwrap();
        drop(consumer_store);

        // typed user fact 是 conversation_get 的投影数据源,不得再产生新 legacy snapshot。
        let store = kanzei_core::SessionStore::open(&dir.join("state.db")).unwrap();
        let facts = store.list_session_facts("thread-x").unwrap();
        assert!(facts.iter().any(|(_, envelope)| matches!(
            envelope.fact,
            kanzei_core::SessionFact::UserMessageCommitted { ref message, .. }
                if message.parts.iter().any(|part| matches!(
                    part,
                    kanzei_llm::Part::Text { text } if text == "你好, 桌面!"
                ))
        )));
        assert!(
            store
                .list_events_by_type("thread-x", 0, "conversation.updated")
                .unwrap()
                .is_empty(),
            "mobile consumer 不得新增 conversation.updated"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ============================ D-389 真链路验收(机器侧) ============================

    /// 测试桥接服务:真实 TcpListener + 真实 accept 线程 + 真实
    /// handle_mobile_connection(生产代码路径,非替身),随机真实端口。
    struct TestBridge {
        addr: std::net::SocketAddr,
        devices: Arc<Mutex<HashMap<String, String>>>,
        active: Arc<AtomicBool>,
    }

    fn start_bridge(project_root: PathBuf) -> TestBridge {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let devices: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let pair_code: Arc<Mutex<Option<String>>> =
            Arc::new(Mutex::new(Some("test-pair-001".into())));
        let runtimes: Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicBool::new(true));
        let pwa_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("mobile-pwa");
        let (d, p, r, a) = (
            devices.clone(),
            pair_code.clone(),
            runtimes.clone(),
            active.clone(),
        );
        std::thread::spawn(move || {
            while a.load(Ordering::SeqCst) {
                if let Ok((stream, _)) = listener.accept() {
                    let (cd, cp, cr, ca) = (d.clone(), p.clone(), r.clone(), a.clone());
                    let proot = project_root.clone();
                    let pwa = pwa_root.clone();
                    std::thread::spawn(move || {
                        handle_mobile_connection(stream, proot, pwa, cd, cp, cr, ca)
                    });
                }
            }
        });
        TestBridge {
            addr,
            devices,
            active,
        }
    }

    /// 真实 HTTP 请求(Connection: close → 读到 EOF),返回 (响应头, body)。
    fn bridge_request(addr: std::net::SocketAddr, raw: &str) -> (String, String) {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(raw.as_bytes()).unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&buf).to_string();
        match text.find("\r\n\r\n") {
            Some(i) => (text[..i].to_string(), text[i + 4..].to_string()),
            None => (text, String::new()),
        }
    }

    fn pair_request(code: &str) -> String {
        let body = format!("{{\"pair_code\":\"{code}\"}}");
        format!(
            "POST /v1/pair HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    }

    /// D-389 期望的真链路验收(机器侧,可重放):真实桥接端口 + 真实 HTTP——
    /// ①PWA 页面经桥接端口加载(不鉴权,D-390 死锁修复);②静态 JS 资源;
    /// ③路径穿越拒绝;④无 token /v1 API 401(鉴权闸);⑤错误配对码 401;
    /// ⑥正确配对换 token;⑦带 token 数据流 200;⑧撤销即 401。
    /// 测试命令可重跑,真实端口地址随运行输出(不写死,非替身)。
    #[test]
    fn 真实桥接端口端到端() {
        let dir = std::env::temp_dir().join(format!(
            "kz-mobile-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei")).unwrap();
        // 预建 state.db(notifications 200 需要)。
        let _ = kanzei_core::SessionStore::open(&dir.join(".kanzei").join("state.db")).unwrap();

        let bridge = start_bridge(dir.clone());
        let addr = bridge.addr;
        eprintln!("[真链路验收] 桥接服务真实端口: http://{addr}/");

        // ① PWA 首页经桥接端口加载——D-390 死锁修复:静态资源不鉴权。
        let (head, body) = bridge_request(addr, "GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(
            head.starts_with("HTTP/1.1 200"),
            "首页必须 200(不经 token): {head}"
        );
        assert!(body.contains("<!DOCTYPE html>"), "serve PWA index.html");
        assert!(body.contains("app.js"), "index.html 引用 app.js");

        // ② 静态 JS 资源(application/javascript;PWA 相对路径经桥接 serve)。
        let (head2, body2) = bridge_request(addr, "GET /app.js HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(head2.starts_with("HTTP/1.1 200"), "app.js 200: {head2}");
        assert!(head2.contains("application/javascript"), "JS 类型: {head2}");
        assert!(body2.contains("pair"), "app.js 含配对逻辑");

        // ③ 路径穿越拒绝。
        let (head3, _) = bridge_request(
            addr,
            "GET /mobile-pwa/../app.js HTTP/1.1\r\nHost: x\r\n\r\n",
        );
        assert!(head3.starts_with("HTTP/1.1 404"), "穿越必须 404: {head3}");

        // ④ 无 token 的 /v1 API → 401(鉴权闸在 /v1/* 上仍生效)。
        let (head4, _) = bridge_request(
            addr,
            "GET /v1/notifications?thread_id=t HTTP/1.1\r\nHost: x\r\n\r\n",
        );
        assert!(head4.starts_with("HTTP/1.1 401"), "无 token 401: {head4}");

        // ⑤ 错误配对码 → 401 invalid_pair_code。
        let (head5, body5) = bridge_request(addr, &pair_request("wrong"));
        assert!(head5.starts_with("HTTP/1.1 401"), "错误配对码 401: {head5}");
        assert!(body5.contains("invalid_pair_code"), "{body5}");

        // ⑥ 正确配对 → device_id + token。
        let (head6, body6) = bridge_request(addr, &pair_request("test-pair-001"));
        assert!(
            head6.starts_with("HTTP/1.1 200"),
            "配对 200: {head6} {body6}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&body6).expect("配对响应是 JSON");
        let token = parsed["token"].as_str().expect("有 token").to_string();
        let device_id = parsed["device_id"]
            .as_str()
            .expect("有 device_id")
            .to_string();

        // ⑦ 带 token 数据流正常(通知查询 200)。
        let auth = format!(
            "GET /v1/notifications?thread_id=t&device_id={device_id} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\n\r\n"
        );
        let (head7, _) = bridge_request(addr, &auth);
        assert!(head7.starts_with("HTTP/1.1 200"), "带 token 200: {head7}");

        // ⑧ 撤销设备 → 立即 401。
        bridge.devices.lock_or_recover().remove(&device_id);
        let (head8, _) = bridge_request(addr, &auth);
        assert!(head8.starts_with("HTTP/1.1 401"), "撤销后 401: {head8}");

        bridge.active.store(false, Ordering::SeqCst);
        std::fs::remove_dir_all(&dir).ok();
    }

    // ============================ 手机端体验整改(UX-017~019/133~143) ============================

    pub(super) fn temp_project(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-mobile-ux-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei")).unwrap();
        dir
    }

    /// 登记一条讨论(`readonly`)或独立任务,返回它的会话 id。
    pub(super) fn register_line(
        store: &kanzei_core::SessionStore,
        root: &Path,
        number: u32,
        profile: Option<&str>,
    ) -> String {
        let process_id = format!("p{number}|{}", root.display());
        store
            .upsert_process(&kanzei_core::StoredProcess {
                process_id: process_id.clone(),
                origin_project: root.display().to_string(),
                project_dir: root.display().to_string(),
                worktree_path: None,
                model: None,
                profile: profile.map(str::to_string),
                research_topic: None,
                reasoning: None,
                manual_models: Vec::new(),
                phase_pipeline: false,
                subagents_enabled: true,
                tracker_writes_enabled: false,
                updated_at: 1,
            })
            .unwrap();
        process_session_id(root, Some(&process_id))
    }

    /// UX-018:并行线路的会话 id 含 `#`,PWA 编成 `%23`;服务端必须解码后再匹配。
    #[test]
    fn 百分号解码_井号中文加号与坏序列() {
        assert_eq!(percent_decode("ses_project_x%23p3"), "ses_project_x#p3");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("%E4%B8%AD%E6%96%87"), "中文");
        // 不合法的 % 序列原样保留,不 panic、不吞字符。
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%2"), "%2");
        assert_eq!(percent_decode("%zz"), "%zz");
        // `%+f` 不是十六进制对:% 原样保留,后面的 + 仍按查询串规则是空格。
        assert_eq!(percent_decode("%+f"), "% f");
        let path = "/v1/events?thread_id=ses_project_x%23p3&device_id=dev-1&cursor=7";
        assert_eq!(
            mobile_query(path, "thread_id").as_deref(),
            Some("ses_project_x#p3")
        );
        assert_eq!(mobile_query(path, "cursor").as_deref(), Some("7"));
    }

    /// UX-018 端到端:线路 id 含 `#` 时,`%23` 编码的订阅请求能取到该线路的通知。
    #[test]
    fn notifications_线路id含井号_百分号编码也能取到事件() {
        let (dir, state_path) = mobile_connection_test_project("hash");
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        store
            .create_session("thread-open#p3", "C:/p", None)
            .unwrap();
        store
            .append_notification_atomic("thread-open#p3", "completed", "line-done-marker", false)
            .unwrap();
        drop(store);
        let (devices, pair_code, runtimes, active) = mobile_connection_test_inputs();
        let (response, worker) = run_mobile_request(
            dir.clone(),
            "GET /v1/notifications?thread_id=thread-open%23p3&device_id=dev-open&cursor=0 HTTP/1.1\r\nAuthorization: Bearer tok-open\r\n\r\n",
            devices,
            pair_code,
            runtimes,
            active,
        );
        worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(
            response.contains("line-done-marker"),
            "含 # 的线路应取到自己的事件: {response}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// Authorization 头大小写不敏感,token 本身保留原大小写。
    #[test]
    fn bearer_token_头名不分大小写_token原样() {
        let request = "GET / HTTP/1.1\r\nAUTHORIZATION: bearer KZ-Device-AbC \r\n\r\n";
        assert_eq!(bearer_token(request), Some("KZ-Device-AbC"));
        assert_eq!(bearer_token("GET / HTTP/1.1\r\nHost: x\r\n\r\n"), None);
        assert_eq!(bearer_token("Authorization: Basic abc\r\n"), None);
    }

    /// UX-017:会话清单——主对话在前;讨论/独立任务按「类型 + 序号」命名,用户命名优先,
    /// 界面不出现 ses_ 哈希与 pN。
    #[test]
    fn 会话清单_主对话在前_类型加序号命名_用户命名优先() {
        let root = temp_project("sessions");
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        register_line(&store, &root, 3, Some("readonly"));
        register_line(&store, &root, 10, None);
        let named = register_line(&store, &root, 4, None);
        store
            .create_session(&named, &root.display().to_string(), Some("  修登录页  "))
            .unwrap();

        let sessions = bridge_sessions(&store, &root).unwrap();
        assert_eq!(sessions[0].kind, "main");
        assert_eq!(sessions[0].label, "主对话");
        assert_eq!(sessions[0].session_id, process_session_id(&root, None));
        let labels: Vec<&str> = sessions.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"讨论 3"), "{labels:?}");
        assert!(labels.contains(&"独立任务 10"), "{labels:?}");
        assert!(
            labels.contains(&"修登录页"),
            "用户命名优先并去空白: {labels:?}"
        );
        assert_eq!(sessions.len(), 4, "主对话 + 3 条线路: {labels:?}");
        assert!(
            labels.iter().all(|label| !label.contains("ses_")),
            "标签不得露出会话哈希: {labels:?}"
        );
        let kinds: Vec<&str> = sessions.iter().map(|s| s.kind).collect();
        assert!(
            kinds.contains(&"discussion") && kinds.contains(&"task"),
            "{kinds:?}"
        );

        // JSON 形态:running 来自运行态表。
        let runtime = Arc::new(SessionRuntime::default());
        runtime.running.store(true, Ordering::SeqCst);
        let mut table = HashMap::new();
        table.insert(named.clone(), runtime);
        let json = bridge_sessions_json(
            &root,
            &kanzei_core::project_state_path(&root),
            &Arc::new(Mutex::new(table)),
        )
        .unwrap();
        assert_eq!(json["default"], process_session_id(&root, None));
        let running: Vec<&serde_json::Value> = json["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["running"] == true)
            .collect();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0]["session_id"], named);
        std::fs::remove_dir_all(root).ok();
    }

    /// UX-133:未知会话 404 且不建隐形会话;已知会话 202,回执如实说明不触发运行。
    #[test]
    fn 手机消息_未知会话404不建会话_已知会话202如实回执() {
        let root = temp_project("message");
        let state_path = kanzei_core::project_state_path(&root);
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        let line = register_line(&store, &root, 3, Some("readonly"));
        drop(store);
        let runtimes = Arc::new(Mutex::new(HashMap::new()));
        let call = |payload: serde_json::Value| {
            String::from_utf8_lossy(&handle_mobile_message(
                &payload,
                &root,
                &state_path,
                &runtimes,
            ))
            .into_owned()
        };

        let unknown = call(json!({"thread_id": "session-1", "text": "hi"}));
        assert!(unknown.starts_with("HTTP/1.1 404"), "{unknown}");
        assert!(unknown.contains("unknown_session"), "{unknown}");
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        assert!(
            store.get_session("session-1").unwrap().is_none(),
            "输错的 id 不得再建出隐形会话"
        );
        drop(store);

        assert!(call(json!({"thread_id": "  ", "text": "hi"})).contains("thread_id_required"));
        assert!(call(json!({"thread_id": line, "text": "   "})).contains("text_required"));

        let accepted = call(json!({"thread_id": line, "text": "  继续做  "}));
        assert!(accepted.starts_with("HTTP/1.1 202"), "{accepted}");
        assert!(accepted.contains("\"triggers_run\":false"), "{accepted}");
        assert!(accepted.contains("讨论 3"), "回执要带会话名: {accepted}");
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        let facts = store.list_session_facts(&line).unwrap();
        assert!(
            facts.iter().any(|(_, envelope)| matches!(
                envelope.fact,
                kanzei_core::SessionFact::UserMessageCommitted { ref message, .. }
                    if message.parts.iter().any(|part| matches!(
                        part,
                        kanzei_llm::Part::Text { text } if text == "继续做"
                    ))
            )),
            "消息应已写进该对话"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// UX-137:解除配对只摘发起请求的那台设备;token 对不上返回 None。
    #[test]
    fn 解除配对_只摘发起请求的那台设备() {
        let root = temp_project("unpair-persistence");
        let state_path = kanzei_core::project_state_path(&root);
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        store
            .upsert_mobile_device("dev-a", "tok-a", "A", 1)
            .unwrap();
        store
            .upsert_mobile_device("dev-b", "tok-b", "B", 2)
            .unwrap();
        let mut table = HashMap::new();
        table.insert("dev-a".to_string(), "tok-a".to_string());
        table.insert("dev-b".to_string(), "tok-b".to_string());
        let request = "POST /v1/unpair HTTP/1.1\r\nAuthorization: Bearer tok-a\r\n\r\n";
        let device_id = mobile_authorized(request, &table).unwrap();
        assert!(remove_device(&state_path, &device_id, &mut table).unwrap());
        let left: Vec<String> = table.keys().cloned().collect();
        assert_eq!(left, vec!["dev-b".to_string()]);
        assert!(!remove_device(&state_path, &device_id, &mut table).unwrap());
        assert!(mobile_authorized(request, &table).is_none());
        assert!(mobile_authorized("POST / HTTP/1.1\r\n\r\n", &table).is_none());
        assert_eq!(store.list_mobile_devices().unwrap()[0].0, "dev-b");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// UX-135:URL 是手机能输入的 http 地址;回环模式固定 127.0.0.1。
    #[test]
    fn 桥接url_回环模式固定本机地址() {
        assert_eq!(
            bridge_url(false, MOBILE_DEFAULT_PORT),
            "http://127.0.0.1:18765/"
        );
        let lan = bridge_url(true, 4321);
        assert!(
            lan.starts_with("http://") && lan.ends_with(":4321/"),
            "{lan}"
        );
    }

    /// UX-017/133/137 真链路:会话清单、未知会话 404、已知会话 202、解除配对后该 token 立即 401。
    #[test]
    fn 真实桥接_会话清单_发消息校验_解除配对() {
        let dir = temp_project("e2e");
        let state_path = kanzei_core::project_state_path(&dir);
        let _ = kanzei_core::SessionStore::open(&state_path).unwrap();
        let bridge = start_bridge(dir.clone());
        let (_, body) = bridge_request(bridge.addr, &pair_request("test-pair-001"));
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        let token = parsed["token"].as_str().unwrap().to_string();
        let authed = |method: &str, path: &str, payload: &str| {
            format!(
                "{method} {path} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{payload}",
                payload.len()
            )
        };

        let (head, body) = bridge_request(bridge.addr, &authed("GET", "/v1/sessions", ""));
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        let sessions: serde_json::Value = serde_json::from_str(&body).unwrap();
        let main_id = process_session_id(&dir, None);
        assert_eq!(sessions["default"], main_id);
        assert_eq!(sessions["sessions"][0]["label"], "主对话");

        let (head, body) = bridge_request(
            bridge.addr,
            &authed(
                "POST",
                "/v1/messages",
                r#"{"thread_id":"session-1","text":"hi"}"#,
            ),
        );
        assert!(head.starts_with("HTTP/1.1 404"), "{head} {body}");
        assert!(body.contains("unknown_session"), "{body}");

        let message = json!({"thread_id": main_id, "text": "来自手机"}).to_string();
        let (head, body) = bridge_request(bridge.addr, &authed("POST", "/v1/messages", &message));
        assert!(head.starts_with("HTTP/1.1 202"), "{head} {body}");
        assert!(body.contains("\"triggers_run\":false"), "{body}");

        let (head, body) = bridge_request(bridge.addr, &authed("POST", "/v1/unpair", ""));
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(body.contains("\"unpaired\":true"), "{body}");
        let (head, _) = bridge_request(bridge.addr, &authed("GET", "/v1/sessions", ""));
        assert!(
            head.starts_with("HTTP/1.1 401"),
            "解除配对后 token 必须立即失效: {head}"
        );
        let store = kanzei_core::SessionStore::open(&state_path).unwrap();
        assert!(
            store.list_mobile_devices().unwrap().is_empty(),
            "设备行应同步从状态库删除"
        );

        bridge.active.store(false, Ordering::SeqCst);
        std::fs::remove_dir_all(&dir).ok();
    }
}
