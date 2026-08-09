//! Local HTTP bridge for mobile notifications and messages.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::State;

use crate::{normalized_project_root, AppState, MobileService, MobileServiceInfo};

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

fn mobile_authorized(request: &str, token: &str) -> bool {
    request.lines().any(|line| {
        line.to_ascii_lowercase()
            .strip_prefix("authorization: bearer ")
            .is_some_and(|value| value.trim() == token)
    })
}

fn handle_mobile_connection(mut stream: TcpStream, project_root: PathBuf, token: String) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let Ok(count) = stream.read(&mut chunk) else { return };
        if count == 0 { return; }
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if buffer.len() > 65_536 { return; }
    };
    let request_head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    if !mobile_authorized(&request_head, &token) {
        let _ = stream.write_all(&mobile_json_response("401 Unauthorized", &json!({"error": "device_revoked_or_unauthorized"})));
        return;
    }
    let request_line = request_head.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let content_length = request_head
        .lines()
        .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(str::to_owned))
        // 标准头是 "Content-Length: 123",冒号后有空格;不 trim 会解析失败退回 0,
        // 导致 body 恒为空、所有 POST 都因缺字段返回 400(D-063)。
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < header_end + content_length {
        let Ok(count) = stream.read(&mut chunk) else { return };
        if count == 0 { return; }
        buffer.extend_from_slice(&chunk[..count]);
    }
    let body = &buffer[header_end..header_end + content_length];
    let state_path = kanzei_core::project_state_path(&project_root);
    let response = match (method, path.split('?').next().unwrap_or_default()) {
        ("GET", "/health") => mobile_json_response("200 OK", &json!({"status": "ok", "transport": "local_http"})),
        ("GET", "/v1/notifications") => {
            let Some(thread_id) = mobile_query(path, "thread_id") else {
                let _ = stream.write_all(&mobile_json_response("400 Bad Request", &json!({"error": "thread_id_required"})));
                return;
            };
            let device_id = mobile_query(path, "device_id").unwrap_or_else(|| "paired-device".into());
            let cursor = mobile_query(path, "cursor")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_else(|| kanzei_core::SessionStore::open(&state_path).ok()
                    .and_then(|store| store.delivery_cursor(&device_id, &thread_id).ok())
                    .unwrap_or(0));
            match kanzei_core::SessionStore::open(&state_path)
                .and_then(|store| {
                    let events = store.replay_notifications(&thread_id, cursor, 100)?;
                    if let Some(last) = events.last() {
                        store.set_delivery_cursor(&device_id, &thread_id, last.sequence)?;
                    }
                    Ok(events)
                }) {
                Ok(events) => mobile_json_response("200 OK", &json!({"events": events, "cursor": events.last().map(|event| event.sequence).unwrap_or(cursor)})),
                Err(error) => mobile_json_response("500 Internal Server Error", &json!({"error": error.to_string()})),
            }
        }
        ("POST", "/v1/messages") => {
            let payload: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
            let Some(thread_id) = payload.get("thread_id").and_then(|value| value.as_str()) else {
                let _ = stream.write_all(&mobile_json_response("400 Bad Request", &json!({"error": "thread_id_required"})));
                return;
            };
            match kanzei_core::SessionStore::open(&state_path).and_then(|store| {
                store.create_session(&thread_id, &project_root.display().to_string(), None)?;
                store.append_event(&thread_id, "mobile.message", &payload)?;
                Ok(())
            }) {
                Ok(()) => mobile_json_response("202 Accepted", &json!({"accepted": true})),
                Err(error) => mobile_json_response("500 Internal Server Error", &json!({"error": error.to_string()})),
            }
        }
        _ => mobile_json_response("404 Not Found", &json!({"error": "not_found"})),
    };
    let _ = stream.write_all(&response);
}

#[tauri::command]
pub fn mobile_service_start(
    state: State<'_, AppState>,
    project_dir: String,
    port: Option<u16>,
) -> Result<MobileServiceInfo, String> {
    if state.mobile_service.lock().unwrap().is_some() {
        return Err("移动端桥接服务已经启动".into());
    }
    let root = normalized_project_root(Path::new(&project_dir));
    let listener = TcpListener::bind(("127.0.0.1", port.unwrap_or(0)))
        .map_err(|e| format!("移动端桥接服务启动失败: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("设置本机服务非阻塞失败: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?.to_string();
    let token = format!(
        "kz-mobile-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos()
    );
    let active = Arc::new(AtomicBool::new(true));
    let thread_active = active.clone();
    let thread_root = root.clone();
    let thread_token = token.clone();
    std::thread::spawn(move || {
        while thread_active.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => handle_mobile_connection(stream, thread_root.clone(), thread_token.clone()),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(_) => break,
            }
        }
    });
    let info = MobileServiceInfo { address: address.clone(), token: token.clone() };
    *state.mobile_service.lock().unwrap() = Some(MobileService { active });
    Ok(info)
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
