//! A desktop may only send commands to the exact binary that owns its runtime.
use super::{request, Endpoint};
use crate::MutexPoisonExt;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tauri::Manager;

static RETIRING: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
const BUSY: &str = "旧版后台仍有任务运行，完成或停止任务后将自动升级连接";

pub(super) async fn invoke_url(webview: &tauri::WebviewWindow) -> Result<tauri::Url, String> {
    // The endpoint can be reachable while the hidden webview is still about:blank.
    // Wait for navigation before invoking, preserving Tauri's normal ACL checks.
    for _ in 0..100 {
        let url = webview.url().map_err(|e| e.to_string())?;
        if url.scheme() != "about" {
            return Ok(url);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("后台界面尚未就绪，请稍后重试".into())
}

pub(super) struct Admission;
impl Drop for Admission {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}
pub(super) fn admit() -> Result<Admission, String> {
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    let guard = Admission;
    if RETIRING.load(Ordering::SeqCst) {
        return Err("后台正在升级，请稍后重试".into());
    }
    Ok(guard)
}

async fn rpc(endpoint: &Endpoint, value: Value) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(10), request(endpoint, value))
        .await
        .map_err(|_| "后台版本检查超时".to_string())?
}
fn matching(hello: &Value) -> bool {
    hello["build"].as_str() == Some(env!("KANZEI_RUNTIME_BUILD"))
}
fn busy_snapshot(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(busy_snapshot),
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            (key == "running" && value == &Value::Bool(true))
                || (key == "running_count" && value.as_u64().is_some_and(|n| n > 0))
                || (key == "state"
                    && matches!(
                        value.as_str(),
                        Some("queued" | "waiting" | "waiting_user" | "running" | "stopping")
                    ))
                || busy_snapshot(value)
        }),
        _ => false,
    }
}

fn roots() -> Vec<std::path::PathBuf> {
    let mut roots = crate::durable_questions::roots();
    if let Ok(general) = crate::general_chat::storage_root() {
        if general.is_dir() {
            roots.push(general);
        }
    }
    roots
}

/// Returns false only after proving no live owner remains. Never launch a second
/// version beside an incompatible owner of the same databases.
pub(super) async fn accept(endpoint: &Endpoint) -> Result<bool, String> {
    let Ok(hello) = rpc(endpoint, json!({"action":"ping"})).await else {
        return Ok(false);
    };
    if matching(&hello) {
        return Ok(true);
    }
    if hello["build"].as_str().and_then(|s| s.parse::<u128>().ok())
        > env!("KANZEI_RUNTIME_BUILD").parse::<u128>().ok()
    {
        return Err("后台已更新，请重新打开新版桌面".into());
    }
    if hello["build"].as_str().is_some() {
        rpc(endpoint, json!({"action":"retire_if_idle"})).await?;
    } else {
        // Migration from releases without a build handshake. Probe every known
        // project, including projectless storage, before gracefully retiring it.
        let snapshot = rpc(
            endpoint,
            json!({"action":"invoke","command":"workspace_overview","args":{}}),
        )
        .await?;
        if busy_snapshot(&snapshot) {
            return Err(BUSY.into());
        }
        for root in roots() {
            let processes = rpc(
                endpoint,
                json!({"action":"invoke","command":"process_list","args":{"projectDir":root}}),
            )
            .await?;
            if busy_snapshot(&processes) {
                return Err(BUSY.into());
            }
            for process in processes.as_array().into_iter().flatten() {
                let jobs = rpc(endpoint, json!({"action":"invoke","command":"agent_team_command","args":{"projectDir":root,"processId":process["id"],"input":{"action":"list"}}})).await?;
                if busy_snapshot(&jobs) {
                    return Err(BUSY.into());
                }
            }
            let terminals = rpc(endpoint, json!({"action":"invoke","command":"terminal_monitor","args":{"projectDir":root,"action":"list"}})).await?;
            if busy_snapshot(&terminals) {
                return Err(BUSY.into());
            }
        }
        rpc(endpoint, json!({"action":"shutdown"})).await?;
    }
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if rpc(endpoint, json!({"action":"ping"})).await.is_err() {
            return Ok(false);
        }
    }
    Err("旧版后台尚未退出，暂未提交新任务".into())
}

pub(super) fn retire_if_idle(app: &tauri::AppHandle) -> Result<Value, String> {
    RETIRING.store(true, Ordering::SeqCst);
    let state = app.state::<crate::AppState>();
    let busy = IN_FLIGHT.load(Ordering::SeqCst) > 0
        || state
            .runtimes
            .lock_or_recover()
            .values()
            .any(|r| r.running.load(Ordering::SeqCst) || r.compacting.load(Ordering::SeqCst))
        || state.processes.lock_or_recover().values().any(|process| {
            let owner = crate::process_session_id(&process.origin_project.0, Some(&process.id));
            kanzei_tools::team::find(&process.origin_project.0, &owner).is_some_and(|team| {
                team.list()
                    .map_or(true, |jobs| jobs.iter().any(|job| job.active()))
            })
        })
        || roots().iter().any(|root| {
            kanzei_tools::background::list(root)
                .iter()
                .any(|p| p.is_running())
        });
    if busy {
        RETIRING.store(false, Ordering::SeqCst);
        return Err(BUSY.into());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        app.exit(0);
    });
    Ok(json!({"retiring":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_or_different_builds_are_not_compatible() {
        assert!(!matching(&json!({"pid":1,"protocol":1})));
        assert!(!matching(&json!({"build":"previous"})));
        assert!(matching(&json!({"build":env!("KANZEI_RUNTIME_BUILD")})));
    }
    #[test]
    fn nested_live_tasks_prevent_legacy_retirement() {
        assert!(busy_snapshot(
            &json!({"projects":[{"lines":[{"running":true}]}]})
        ));
        assert!(busy_snapshot(&json!([{ "running_count":1 }])));
        assert!(!busy_snapshot(
            &json!({"projects":[{"lines":[{"running":false}]}]})
        ));
    }
}
