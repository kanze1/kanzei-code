//! R-270 批2:SSE 长连接实时推送(由 mobile.rs 拆出)。

use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::{mobile_json_response, mobile_query};
use crate::MutexPoisonExt;

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

pub(super) fn handle_sse(
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
        let device_exists = devices.lock_or_recover().contains_key(&device_id);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
