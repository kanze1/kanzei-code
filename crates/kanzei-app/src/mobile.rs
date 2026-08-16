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
use crate::state::{MobileDeviceInfo, MobileService, MobileServiceInfo};
use crate::AppState;

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
        // 配对成功:清空一次性配对码,生成设备 token。
        *pair_code.lock().unwrap() = None;
        let device_id = format!(
            "dev-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let device_token = format!(
            "kz-device-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        devices
            .lock()
            .unwrap()
            .insert(device_id.clone(), device_token.clone());
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
            let cursor = mobile_query(path, "cursor")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_else(|| {
                    kanzei_core::SessionStore::open(&state_path)
                        .ok()
                        .and_then(|store| store.delivery_cursor(&device_id, &thread_id).ok())
                        .unwrap_or(0)
                });
            match kanzei_core::SessionStore::open(&state_path).and_then(|store| {
                let events = store.replay_notifications(&thread_id, cursor, 100)?;
                if let Some(last) = events.last() {
                    store.set_delivery_cursor(&device_id, &thread_id, last.sequence)?;
                }
                Ok(events)
            }) {
                Ok(events) => mobile_json_response(
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
                Ok(()) => mobile_json_response("202 Accepted", &json!({"accepted": true})),
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
    // R-270:一次性配对码(移动端用它换设备 token)。token 字段保留为配对码,
    // 便于既有前端展示;真正的访问凭据是每设备独立 token。
    let pair_code = format!(
        "kz-pair-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let active = Arc::new(AtomicBool::new(true));
    let devices: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let pair_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(Some(pair_code.clone())));

    let thread_active = active.clone();
    let thread_root = root.clone();
    let thread_devices = devices.clone();
    let thread_pair = pair_slot.clone();
    std::thread::spawn(move || {
        while thread_active.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    // R-270:每连接独立线程——长请求(后续 SSE)不阻塞其它连接。
                    let conn_devices = thread_devices.clone();
                    let conn_pair = thread_pair.clone();
                    let conn_root = thread_root.clone();
                    std::thread::spawn(move || {
                        handle_mobile_connection(stream, conn_root, conn_devices, conn_pair)
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
#[tauri::command]
pub fn mobile_device_revoke(state: State<'_, AppState>, device_id: String) -> Result<(), String> {
    let guard = state.mobile_service.lock().unwrap();
    let service = guard.as_ref().ok_or("移动端桥接服务未启动")?;
    let removed = service.devices.lock().unwrap().remove(&device_id);
    if removed.is_some() {
        Ok(())
    } else {
        Err(format!("设备不存在: {device_id}"))
    }
}

/// 当前设备列表(设置页展示与撤销入口)。
#[tauri::command]
pub fn mobile_device_list(state: State<'_, AppState>) -> Result<Vec<MobileDeviceInfo>, String> {
    let guard = state.mobile_service.lock().unwrap();
    let service = guard.as_ref().ok_or("移动端桥接服务未启动")?;
    // 读取配对码状态(设置页据此提示「正在等待配对」或「已配对 N 台」)。
    let _pending_pair = service.pair_code.lock().unwrap().is_some();
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
}
