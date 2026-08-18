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
    let entries = load_registry(project_root);
    let entry = entries.iter().find(|e| e.id == id)?.clone();
    if !crate::shell::process_alive(entry.pid) {
        return None;
    }
    let log_path = std::env::temp_dir()
        .join("kanzei-bg-logs")
        .join(super::project_hash(project_root))
        .join(&entry.log);
    let full_output = Arc::new(Mutex::new(super::read_log_tail(&log_path).await));
    let output = Arc::new(Mutex::new(Vec::new()));
    let truncated = Arc::new(AtomicBool::new(false));
    let exit: Arc<Mutex<Option<Option<i32>>>> = Arc::new(Mutex::new(None));
    // 没有子进程句柄可 wait,用 pid 活性轮询推进 exit:pid 消失即视为终止。
    let exit_watch = exit.clone();
    let watch_pid = entry.pid;
    let watch_root = project_root.to_path_buf();
    let watch_id = entry.id.clone();
    tokio::spawn(async move {
        loop {
            if !crate::shell::process_alive(watch_pid) {
                *exit_watch.lock().unwrap() = Some(None);
                super::registry().lock().unwrap().remove(&watch_id);
                remove_registry_entry(&watch_root, &watch_id);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    let baseline = Arc::new(Mutex::new(ManagedSnapshot::capture(project_root)));
    let process = Arc::new(BackgroundProcess {
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
        truncated,
        exit,
        baseline,
        breaches: Arc::new(Mutex::new(Vec::new())),
    });
    super::registry()
        .lock()
        .unwrap()
        .insert(process.id.clone(), process.clone());
    if crate::managed::managed_scope_exists(project_root) {
        super::install_window_observer_once();
        super::spawn_guard(process.clone());
    }
    Some(process)
}

/// R-180 验收②"杀掉":终止注册表里长驻服务的进程树并移除条目。
///
/// 若该服务已接回内存注册表(adopt 过),先做终态对账再清出磁盘注册表;
/// 内存对象保留在注册表供 output 回看最后日志(与 stop 语义一致)。
pub async fn kill_registered(project_root: &Path, id: &str) -> bool {
    let entries = load_registry(project_root);
    let Some(entry) = entries.iter().find(|e| e.id == id).cloned() else {
        return false;
    };
    let process = super::get(id);
    let killed = if crate::shell::process_alive(entry.pid) {
        crate::shell::kill_tree(entry.pid).await
    } else {
        false
    };
    if let Some(process) = process.as_ref().filter(|_| killed) {
        process.mark_terminated();
    }
    if let Some(p) = process {
        super::reconcile(&p, false).await;
    }
    remove_registry_entry(project_root, id);
    true
}
