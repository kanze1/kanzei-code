//! persistent 后台服务的跨 run 注册表、发现、接管与终止域。
//!
//! 注册表格式与普通后台进程 registry 共用父模块的原语；这里仅拆出持久化生命周期，
//! 通过父模块的私有协作函数继续使用同一守卫、日志和回收语义。

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::{BackgroundOwner, BackgroundProcess, ManagedSnapshot};

/// R-180 B3:跨 run 注册表条目——persistent 服务的持久化登记。
///
/// 落盘于 `<temp>/kanzei-bg-logs/<项目hash>/registry.json`(与日志同目录,项目级
/// 发现基于该目录)。全部字段可序列化,重启后按此重建内存对象(接管/杀掉/标失败)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentEntry {
    pub id: String,
    pub command: String,
    pub project_root: String,
    pub workdir: String,
    pub owner: BackgroundOwner,
    pub started_at_ms: u128,
    pub pid: u32,
    /// 日志文件名(registry 同目录下)。
    pub log: String,
}

pub(crate) fn registry_path(project_root: &Path) -> PathBuf {
    std::env::temp_dir()
        .join("kanzei-bg-logs")
        .join(super::project_hash(project_root))
        .join("registry.json")
}

pub(super) fn load_registry(project_root: &Path) -> Vec<PersistentEntry> {
    let path = registry_path(project_root);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// 写注册表走 atomic_file 原语(验收⑤:全仓不出现第二套写原语)。
pub(super) fn save_registry(project_root: &Path, entries: &[PersistentEntry]) {
    let path = registry_path(project_root);
    if let Ok(text) = serde_json::to_string_pretty(entries) {
        let _ = crate::atomic_file::write_atomic(&path, &text);
    }
}

/// 从注册表移除指定条目。进程自然退出/显式停止/被杀后调用,不留幽灵条目。
pub(super) fn remove_registry_entry(project_root: &Path, id: &str) {
    let mut entries = load_registry(project_root);
    let before = entries.len();
    entries.retain(|e| e.id != id);
    if entries.len() != before {
        save_registry(project_root, &entries);
    }
}

/// R-180 验收②:列出跨 run 注册表中上次登记的 persistent 服务,并给出 pid 活性。
///
/// 返回 `(条目, pid 是否存活)`。幽灵条目(pid 已死——强杀 kzapp 后进程没能活下来)
/// 由调用方用 [`mark_registry_failed`] 标失败并清理,本函数只读不写。
pub fn discover_persistent(project_root: &Path) -> Vec<(PersistentEntry, bool)> {
    load_registry(project_root)
        .into_iter()
        .map(|entry| {
            let alive = crate::shell::process_alive(entry.pid);
            (entry, alive)
        })
        .collect()
}

/// 把注册表条目标记为失败并移除(pid 已死的幽灵条目)。返回是否命中。
pub fn mark_registry_failed(project_root: &Path, id: &str) -> bool {
    if let Some(process) = super::get(id) {
        if process.is_running() {
            return false;
        }
        for completion in [&process.exit_completion, &process.guard_completion] {
            match completion.completed() {
                Err(error) => {
                    super::lifecycle::record_cleanup_error(&process, &error);
                    return false;
                }
                Ok(Err(error)) => super::lifecycle::record_cleanup_error(&process, &error),
                Ok(Ok(())) => {}
            }
        }
        if let Err(error) = super::lifecycle::finish_restore(&process) {
            super::lifecycle::record_cleanup_error(&process, &error);
            return false;
        }
    }
    let mut entries = load_registry(project_root);
    let before = entries.len();
    entries.retain(|e| e.id != id);
    if entries.len() != before {
        save_registry(project_root, &entries);
        true
    } else {
        false
    }
}

/// R-180 验收②"接管":把注册表里 pid 仍存活的长驻服务接回当前进程的内存注册表,
/// 之后可用 process output/stop 操作。返回 None = 条目不存在或 pid 已死。
///
/// 接管后重新拍基线并挂守卫:长驻服务脱离 owner run 不等于脱离文件隔离(D-174
/// 归因/回滚约束原样生效,验收④)。
pub async fn adopt_persistent(project_root: &Path, id: &str) -> Option<Arc<BackgroundProcess>> {
    if let Some(existing) = super::get(id)
        .filter(|p| p.project_root == project_root.display().to_string() && p.is_running())
    {
        return Some(existing);
    }
    let entries = load_registry(project_root);
    let entry = entries.iter().find(|e| e.id == id)?.clone();
    if !crate::shell::process_alive(entry.pid) {
        return None;
    }
    let log_path = std::env::temp_dir()
        .join("kanzei-bg-logs")
        .join(super::project_hash(project_root))
        .join(&entry.log);
    let tail = log_snapshot(&log_path).await;
    let full_output = Arc::new(Mutex::new(super::read_log_tail(&log_path).await));
    let output = Arc::new(Mutex::new(tail.1));
    let output_total = Arc::new(std::sync::atomic::AtomicU64::new(tail.0));
    let truncated = Arc::new(AtomicBool::new(false));
    let exit: Arc<Mutex<Option<Option<i32>>>> = Arc::new(Mutex::new(None));
    // 没有子进程句柄可 wait,用 pid 活性轮询推进 exit:pid 消失即视为终止。
    let exit_watch = exit.clone();
    let watch_pid = entry.pid;
    let watch_root = project_root.to_path_buf();
    let watch_id = entry.id.clone();
    let watch_log = log_path.clone();
    let watch_output = output.clone();
    let watch_full = full_output.clone();
    let watch_total = output_total.clone();
    let baseline = Arc::new(Mutex::new(ManagedSnapshot::capture(project_root)));
    let guarded = crate::managed::managed_scope_exists(project_root);
    let process = Arc::new(BackgroundProcess {
        stdin: tokio::sync::Mutex::new(None),
        id: entry.id.clone(),
        command: entry.command.clone(),
        project_root: entry.project_root.clone(),
        workdir: entry.workdir.clone(),
        owner: entry.owner.clone(),
        persistent: true,
        log_path: Some(log_path),
        full_output,
        started_at_ms: entry.started_at_ms,
        pid: Some(entry.pid),
        output,
        output_total,
        truncated,
        exit,
        baseline,
        breaches: Arc::new(Mutex::new(Vec::new())),
        guard_completion: super::GuardCompletion::new(guarded),
        exit_completion: super::GuardCompletion::new(true),
    });
    super::registry()
        .lock()
        .unwrap()
        .insert(process.id.clone(), process.clone());
    let completed = process.clone();
    let watcher = tokio::spawn(async move {
        loop {
            let (size, bytes) = log_snapshot(&watch_log).await;
            {
                let mut output = watch_output.lock().unwrap();
                *output = bytes.clone();
                watch_total.store(size, std::sync::atomic::Ordering::SeqCst);
            }
            *watch_full.lock().unwrap() = super::read_log_tail(&watch_log).await;
            if !crate::shell::process_alive(watch_pid) {
                *exit_watch.lock().unwrap() = Some(None);
                let result = match completed.guard_completion.wait().await {
                    Ok(()) => super::lifecycle::finish_restore(&completed),
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    super::lifecycle::record_cleanup_error(&completed, &error);
                    return Err(error);
                }
                super::registry().lock().unwrap().remove(&watch_id);
                remove_registry_entry(&watch_root, &watch_id);
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    process.exit_completion.publish(watcher);
    if guarded {
        super::install_window_observer_once();
        super::spawn_guard(process.clone());
    }
    Some(process)
}

async fn log_snapshot(path: &Path) -> (u64, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return (0, vec![]);
    };
    let Ok(meta) = file.metadata().await else {
        return (0, vec![]);
    };
    let start = meta
        .len()
        .saturating_sub(super::MAX_BACKGROUND_OUTPUT as u64);
    if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return (0, vec![]);
    }
    let mut bytes = Vec::new();
    let _ = file.take(meta.len() - start).read_to_end(&mut bytes).await;
    (start + bytes.len() as u64, bytes)
}

/// R-180 验收②"杀掉":终止注册表里长驻服务的进程树并移除条目。
///
/// 若该服务已接回内存注册表(adopt 过),先做终态对账再清出磁盘注册表;
/// 内存对象保留在注册表供 output 回看最后日志(与 stop 语义一致)。
pub async fn kill_registered_result(project_root: &Path, id: &str) -> Result<bool, String> {
    let entries = load_registry(project_root);
    let Some(entry) = entries.iter().find(|e| e.id == id).cloned() else {
        return Ok(false);
    };
    if let Some(process) = super::get(id) {
        super::lifecycle::stop_owned(&process).await?;
    } else if crate::shell::process_alive(entry.pid) && !crate::shell::kill_tree(entry.pid).await {
        return Err(format!("未能终止 persistent 服务 {id}，注册项已保留"));
    }
    remove_registry_entry(project_root, id);
    Ok(true)
}

pub async fn kill_registered(project_root: &Path, id: &str) -> bool {
    match kill_registered_result(project_root, id).await {
        Ok(found) => found,
        Err(error) => {
            tracing::error!(%error, process=id, "persistent stop cleanup failed");
            false
        }
    }
}
