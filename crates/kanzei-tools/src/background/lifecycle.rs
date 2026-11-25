//! 后台进程注册表与回收生命周期。

use kanzei_base::sync::MutexPoisonExt;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use super::BackgroundProcess;

type Registry = Mutex<HashMap<String, Arc<BackgroundProcess>>>;

pub(super) fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn next_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "bg{}-{}",
        now_ms(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    )
}

pub(super) fn record_cleanup_error(process: &BackgroundProcess, error: &str) {
    let message = format!("\n[background] {error}\n");
    super::registration::append_bounded(&process.output, &process.truncated, message.as_bytes());
    process
        .output_total
        .fetch_add(message.len() as u64, std::sync::atomic::Ordering::SeqCst);
}

pub(super) fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

pub(super) fn project_hash(root: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn get(id: &str) -> Option<Arc<BackgroundProcess>> {
    registry().lock_or_recover().get(id).cloned()
}

pub fn list(project_root: &Path) -> Vec<Arc<BackgroundProcess>> {
    let root = project_root.display().to_string();
    let mut items: Vec<Arc<BackgroundProcess>> = registry()
        .lock_or_recover()
        .values()
        .filter(|p| p.project_root == root)
        .cloned()
        .collect();
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}

pub(super) async fn stop_owned(process: &Arc<BackgroundProcess>) -> Result<bool, String> {
    let was_running = process.is_running();
    if was_running {
        let pid = process
            .pid
            .ok_or_else(|| format!("后台进程 {} 没有可终止的 pid", process.id))?;
        if !crate::shell::kill_tree(pid).await {
            return Err(format!("未能终止后台进程 {}，进程仍在运行", process.id));
        }
        process.mark_terminated();
    }
    if let Err(error) = process.exit_completion.wait().await {
        record_cleanup_error(process, &error);
    }
    if process.is_running() {
        return Err(format!(
            "后台进程 {} 仍在运行，不能完成停止清理",
            process.id
        ));
    }
    if process.persistent {
        super::remove_registry_entry(Path::new(&process.project_root), &process.id)
            .inspect_err(|error| record_cleanup_error(process, error))?;
    }
    Ok(was_running)
}

pub async fn stop_result(id: &str) -> Result<bool, String> {
    let Some(process) = get(id) else {
        return Ok(false);
    };
    stop_owned(&process).await
}

pub async fn stop(id: &str) -> bool {
    match stop_result(id).await {
        Ok(stopped) => stopped,
        Err(error) => {
            if let Some(process) = get(id) {
                record_cleanup_error(&process, &error);
            }
            tracing::error!(%error, process=id, "background stop cleanup failed");
            false
        }
    }
}

pub async fn kill_project(project_root: &Path) -> usize {
    kill_processes(list(project_root)).await
}

pub async fn kill_process(project_root: &Path, process_id: &str) -> usize {
    kill_processes(
        list(project_root)
            .into_iter()
            .filter(|process| process.owner.process_id == process_id)
            .collect(),
    )
    .await
}

async fn kill_processes(processes: Vec<Arc<BackgroundProcess>>) -> usize {
    let mut killed = 0usize;
    for process in processes {
        if process.persistent {
            continue;
        }
        match stop_owned(&process).await {
            Ok(true) => killed += 1,
            Ok(false) => {}
            Err(error) => {
                record_cleanup_error(&process, &error);
                tracing::error!(%error, process=%process.id, "background reap cleanup failed");
            }
        }
    }
    killed
}
