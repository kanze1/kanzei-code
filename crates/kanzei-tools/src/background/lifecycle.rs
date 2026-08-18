//! 后台进程注册表、窗口观察、守卫与回收生命周期。
//!
//! 该模块只拆出生命周期编排；`BackgroundProcess` 数据对象、登记/输出收集与
//! persistent 注册表仍通过父模块的私有协作函数共享，保持原有调用链和安全语义。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, Once, OnceLock};

use super::{BackgroundProcess, BreachRecord, ManagedSnapshot, GUARD_TICK};

type Registry = Mutex<HashMap<String, Arc<BackgroundProcess>>>;

pub(super) fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn next_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "bg{}",
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    )
}

/// 安装“观察合法写入窗口”的回调(harness 侧的窗口开合回调过来)。
///
/// D-258 精确吸收:窗口打开时拍「打开前」快照,关闭时只吸收窗口前后实际变化且
/// 落在声明前缀内的路径,不把整个前缀或后台偷写固化进基线。
pub(super) fn install_window_observer_once() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        kanzei_harness::managed_fence::set_observer(|phase, spec| {
            for process in running_processes() {
                let root = PathBuf::from(&process.project_root);
                let key = (process.id.clone(), spec.tool);
                match phase {
                    kanzei_harness::managed_fence::WindowPhase::Opened => {
                        let snapshot = ManagedSnapshot::capture(&root);
                        window_open_snapshots()
                            .lock()
                            .unwrap()
                            .entry(key)
                            .or_default()
                            .push(snapshot);
                    }
                    kanzei_harness::managed_fence::WindowPhase::Closed => {
                        let opened = {
                            let mut snapshots = window_open_snapshots().lock().unwrap();
                            let popped = snapshots.get_mut(&key).and_then(|stack| stack.pop());
                            if let Some(stack) = snapshots.get(&key) {
                                if stack.is_empty() {
                                    snapshots.remove(&key);
                                }
                            }
                            popped
                        };
                        let current = ManagedSnapshot::capture(&root);
                        let mut baseline = process.baseline();
                        let before = opened.as_ref().unwrap_or(&baseline);
                        if let Some(change) = crate::managed::diff(before, &current) {
                            let paths: Vec<&str> = change
                                .touched()
                                .into_iter()
                                .map(|s| s.as_str())
                                .filter(|p| kanzei_harness::managed_fence::covers(spec, p))
                                .collect();
                            if !paths.is_empty() {
                                baseline.absorb_paths(&current, &paths);
                                process.set_baseline(baseline);
                            }
                        }
                    }
                }
            }
        });
    });
}

type WindowSnapshotMap = Mutex<HashMap<(String, &'static str), Vec<ManagedSnapshot>>>;

fn window_open_snapshots() -> &'static WindowSnapshotMap {
    static SNAPSHOTS: OnceLock<WindowSnapshotMap> = OnceLock::new();
    SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn running_processes() -> Vec<Arc<BackgroundProcess>> {
    registry()
        .lock()
        .unwrap()
        .values()
        .filter(|p| p.is_running())
        .cloned()
        .collect()
}

/// 后台守卫:周期性把托管树与基线对账,越界即隔离、回滚并终止进程树。
pub(super) fn spawn_guard(process: Arc<BackgroundProcess>) {
    tokio::spawn(async move {
        loop {
            let was_running = process.is_running();
            reconcile(&process, true).await;
            if !was_running {
                break;
            }
            tokio::time::sleep(GUARD_TICK).await;
        }
    });
}

/// 一次对账。返回 Some = 检测到越界(已隔离并回滚)。
pub(super) async fn reconcile(
    process: &Arc<BackgroundProcess>,
    kill_on_breach: bool,
) -> Option<BreachRecord> {
    let root = PathBuf::from(&process.project_root);
    if !crate::managed::managed_scope_exists(&root) {
        return None;
    }
    let baseline = process.baseline();
    let current = ManagedSnapshot::capture(&root);
    let change = crate::managed::diff(&baseline, &current)?;
    let (legitimate, breach) = change.partition(kanzei_harness::managed_fence::write_in_progress);
    if breach.is_empty() {
        return None;
    }
    let (quarantine, restored) = crate::managed::quarantine_and_restore(
        &root,
        &baseline,
        &breach,
        &format!("bg-{}", process.id),
    );
    let record = BreachRecord {
        at_ms: now_ms(),
        touched: breach.touched().into_iter().cloned().collect(),
        quarantine: quarantine.display().to_string(),
        restored,
    };
    process.record_breach(record.clone());
    if kill_on_breach {
        if let Some(pid) = process.pid {
            if crate::shell::kill_tree(pid).await {
                process.mark_terminated();
            }
        }
    }
    let mut new_baseline = baseline;
    let legitimate_paths: Vec<&str> = legitimate
        .touched()
        .into_iter()
        .map(|s| s.as_str())
        .collect();
    new_baseline.absorb_paths(&current, &legitimate_paths);
    process.set_baseline(new_baseline);
    Some(record)
}

/// 收掉不属于当前 run 的后台任务(先终止,再做终态对账)。
pub async fn finish_foreign_owners(project_root: &Path, current_run_id: Option<&str>) -> usize {
    let current = current_run_id.unwrap_or("unowned");
    let mut finished = 0usize;
    for process in list(project_root) {
        if !process.is_running() || process.owner.run_id == current {
            continue;
        }
        if process.persistent {
            continue;
        }
        if let Some(pid) = process.pid {
            if crate::shell::kill_tree(pid).await {
                process.mark_terminated();
            }
        }
        reconcile(&process, false).await;
        finished += 1;
    }
    finished
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
    registry().lock().unwrap().get(id).cloned()
}

pub fn list(project_root: &Path) -> Vec<Arc<BackgroundProcess>> {
    let root = project_root.display().to_string();
    let mut items: Vec<Arc<BackgroundProcess>> = registry()
        .lock()
        .unwrap()
        .values()
        .filter(|p| p.project_root == root)
        .cloned()
        .collect();
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}

pub async fn stop(id: &str) -> bool {
    let Some(process) = get(id) else {
        return false;
    };
    if !process.is_running() {
        return false;
    }
    if let Some(pid) = process.pid {
        if crate::shell::kill_tree(pid).await {
            process.mark_terminated();
        }
    }
    reconcile(&process, false).await;
    if process.persistent {
        super::remove_registry_entry(Path::new(&process.project_root), &process.id);
    }
    true
}

pub async fn kill_project(project_root: &Path) -> usize {
    let mut killed = 0usize;
    for process in list(project_root) {
        if process.persistent {
            continue;
        }
        if process.is_running() {
            if let Some(pid) = process.pid {
                if crate::shell::kill_tree(pid).await {
                    process.mark_terminated();
                }
                killed += 1;
            }
            reconcile(&process, false).await;
        }
    }
    killed
}

pub async fn kill_process(project_root: &Path, process_id: &str) -> usize {
    let mut killed = 0usize;
    for process in list(project_root)
        .into_iter()
        .filter(|process| process.owner.process_id == process_id)
    {
        if process.persistent {
            continue;
        }
        if process.is_running() {
            if let Some(pid) = process.pid {
                if crate::shell::kill_tree(pid).await {
                    process.mark_terminated();
                }
                killed += 1;
            }
            reconcile(&process, false).await;
        }
    }
    killed
}
