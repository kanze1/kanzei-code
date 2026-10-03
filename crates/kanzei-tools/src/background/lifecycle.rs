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
        "bg{}-{}",
        now_ms(),
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
        kanzei_harness::managed_fence::set_observer(|phase, window| {
            for process in running_processes() {
                let root = PathBuf::from(&process.project_root);
                if !window.applies_to(&root) {
                    continue;
                }
                let key = (process.id.clone(), window.id);
                match phase {
                    kanzei_harness::managed_fence::WindowPhase::Opened => {
                        // A new tool must not start writing while a guard is
                        // restoring this process's protected tree.
                        let _baseline = process.baseline.lock().unwrap();
                        let snapshot = ManagedSnapshot::capture(&root);
                        window_open_snapshots()
                            .lock()
                            .unwrap()
                            .insert(key, snapshot);
                    }
                    kanzei_harness::managed_fence::WindowPhase::Closed => {
                        let opened = { window_open_snapshots().lock().unwrap().remove(&key) };
                        #[cfg(test)]
                        run_before_absorb_hook(&process.id);
                        // Read/absorb under the same lock: another completed
                        // window cannot lose its update to this commit.
                        let mut baseline = process.baseline.lock().unwrap();
                        let current = ManagedSnapshot::capture(&root);
                        let before = opened.as_ref().unwrap_or(&baseline);
                        if let Some(change) = crate::managed::diff(before, &current) {
                            let paths: Vec<&str> = change
                                .touched()
                                .into_iter()
                                .map(|s| s.as_str())
                                .filter(|p| kanzei_harness::managed_fence::covers(window.spec, p))
                                .collect();
                            if !paths.is_empty() {
                                baseline.absorb_paths(&current, &paths);
                            }
                        }
                    }
                }
            }
            if phase == kanzei_harness::managed_fence::WindowPhase::Closed {
                // A process can end while a tool is running. Its opening snapshot
                // is no longer needed even though it was skipped above.
                window_open_snapshots()
                    .lock()
                    .unwrap()
                    .retain(|(_, id), _| *id != window.id);
            }
        });
    });
}

type WindowSnapshotMap = Mutex<HashMap<(String, u64), ManagedSnapshot>>;

fn window_open_snapshots() -> &'static WindowSnapshotMap {
    static SNAPSHOTS: OnceLock<WindowSnapshotMap> = OnceLock::new();
    SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
type BeforeAbsorbHook = Arc<dyn Fn(&str) + Send + Sync>;

#[cfg(test)]
fn before_absorb_hook() -> &'static Mutex<Option<BeforeAbsorbHook>> {
    static HOOK: OnceLock<Mutex<Option<BeforeAbsorbHook>>> = OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(super) fn set_before_absorb_hook(hook: Option<BeforeAbsorbHook>) {
    *before_absorb_hook().lock().unwrap() = hook;
}

#[cfg(test)]
fn run_before_absorb_hook(process_id: &str) {
    let hook = before_absorb_hook().lock().unwrap().clone();
    if let Some(hook) = hook {
        hook(process_id);
    }
}

#[cfg(test)]
fn before_reconcile_hook() -> &'static Mutex<Option<BeforeAbsorbHook>> {
    static HOOK: OnceLock<Mutex<Option<BeforeAbsorbHook>>> = OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(super) fn set_before_reconcile_hook(hook: Option<BeforeAbsorbHook>) {
    *before_reconcile_hook().lock().unwrap() = hook;
}

#[cfg(test)]
fn run_before_reconcile_hook(process_id: &str) {
    let hook = before_reconcile_hook().lock().unwrap().clone();
    if let Some(hook) = hook {
        hook(process_id);
    }
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
    let guarded = process.clone();
    let handle = tokio::spawn(async move {
        loop {
            let was_running = guarded.is_running();
            #[cfg(test)]
            if !was_running {
                let hook = final_guard_hook().lock().unwrap().clone();
                if let Some(hook) = hook {
                    hook(&guarded.id);
                }
            }
            if let Err(error) = reconcile_result(&guarded, true).await {
                if !guarded.is_running() {
                    return Err(error);
                }
                // Preserve the existing guard's retry semantics if termination
                // itself failed. The writer must not become unguarded.
                tracing::error!(%error, process=%guarded.id, "background restore/termination incomplete");
            }
            if !was_running {
                break;
            }
            tokio::time::sleep(GUARD_TICK).await;
        }
        Ok(())
    });
    process.guard_completion.publish(handle);
}

#[cfg(test)]
fn final_guard_hook() -> &'static Mutex<Option<BeforeAbsorbHook>> {
    static HOOK: OnceLock<Mutex<Option<BeforeAbsorbHook>>> = OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}
#[cfg(test)]
pub(crate) fn set_final_guard_hook(hook: Option<BeforeAbsorbHook>) {
    *final_guard_hook().lock().unwrap() = hook;
}

/// 一次对账。返回 Some = 检测到越界(已隔离并回滚)。
pub(super) async fn reconcile(
    process: &Arc<BackgroundProcess>,
    kill_on_breach: bool,
) -> Option<BreachRecord> {
    reconcile_result(process, kill_on_breach)
        .await
        .ok()
        .flatten()
}

async fn reconcile_result(
    process: &Arc<BackgroundProcess>,
    kill_on_breach: bool,
) -> Result<Option<BreachRecord>, String> {
    let result = reconcile_restore(process);
    if kill_on_breach && !matches!(result, Ok(None)) {
        if let Some(pid) = process.pid {
            if crate::shell::kill_tree(pid).await {
                process.mark_terminated();
            }
        }
    }
    result
}

fn reconcile_restore(process: &Arc<BackgroundProcess>) -> Result<Option<BreachRecord>, String> {
    let root = PathBuf::from(&process.project_root);
    if !crate::managed::managed_scope_exists(&root) {
        return Ok(None);
    }
    #[cfg(test)]
    run_before_reconcile_hook(&process.id);
    let (breach, restore) = {
        // Closed commits and new Opened snapshots use this same mutex. Keep
        // the decision and synchronous restore together so a stale baseline
        // cannot roll back a legitimate write that just finished.
        let baseline = process.baseline.lock().unwrap();
        let current = ManagedSnapshot::capture(&root);
        let Some(change) = crate::managed::diff(&baseline, &current) else {
            return Ok(None);
        };
        let (_, breach) =
            change.partition(|path| kanzei_harness::managed_fence::write_in_progress(&root, path));
        if breach.is_empty() {
            return Ok(None);
        }
        let restore = crate::managed::quarantine_and_restore(
            &root,
            &baseline,
            &breach,
            &[],
            &format!("bg-{}", process.id),
        );
        (breach, restore)
    };
    let (quarantine, restored) = match restore {
        Ok(result) => result,
        Err(error) => {
            let message = format!("\n[managed-files] 自动回滚失败：{error}\n");
            super::registration::append_bounded(
                &process.output,
                &process.truncated,
                message.as_bytes(),
            );
            process
                .output_total
                .fetch_add(message.len() as u64, std::sync::atomic::Ordering::SeqCst);
            return Err(format!("后台托管文件回滚失败：{error}"));
        }
    };
    let record = BreachRecord {
        at_ms: now_ms(),
        touched: breach.touched().into_iter().cloned().collect(),
        quarantine: quarantine.display().to_string(),
        restored,
    };
    process.record_breach(record.clone());
    // Only the window-close observer commits legitimate changes. An unrelated
    // breach must neither absorb an open window nor overwrite a newer baseline
    // published while process termination was awaiting completion.
    Ok(Some(record))
}

fn record_cleanup_error(process: &BackgroundProcess, error: &str) {
    let message = format!("\n[managed-files] {error}\n");
    super::registration::append_bounded(&process.output, &process.truncated, message.as_bytes());
    process
        .output_total
        .fetch_add(message.len() as u64, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn child_processes(
    root: &Path,
    owner: &str,
    child: &str,
) -> Vec<Arc<BackgroundProcess>> {
    let prefix = format!("{owner}:{child}:");
    list(root)
        .into_iter()
        .filter(|process| {
            process.owner.process_id == child
                && process
                    .owner
                    .run_id
                    .strip_prefix(&prefix)
                    .is_some_and(|attempt| attempt.parse::<u32>().is_ok())
        })
        .collect()
}
pub(crate) async fn kill_child_processes(root: &Path, owner: &str, child: &str) -> usize {
    kill_processes(child_processes(root, owner, child)).await
}
pub(crate) async fn wait_child_cleanup(
    root: &Path,
    owner: &str,
    child: &str,
) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    for process in child_processes(root, owner, child)
        .into_iter()
        .filter(|p| !p.persistent)
    {
        if let Err(error) = process.exit_completion.wait().await {
            record_cleanup_error(&process, &error);
            notes.push(error);
        }
        if let Err(error) = process.guard_completion.wait().await {
            record_cleanup_error(&process, &error);
            notes.push(error);
        }
        if process.is_running() {
            return Err(format!(
                "后台进程 {} 仍在运行，不能完成子任务清理",
                process.id
            ));
        }
        // The join result proves that the old writer ended; a fresh restore
        // retries failures rather than making a cached JoinError permanent.
        reconcile_restore(&process)?;
    }
    Ok(notes)
}

pub(crate) fn retry_child_cleanup(root: &Path, owner: &str, child: &str) -> Result<(), String> {
    for process in child_processes(root, owner, child)
        .into_iter()
        .filter(|p| !p.persistent)
    {
        if process.is_running() {
            return Err(format!("后台进程 {} 仍在运行，请先明确停止", process.id));
        }
        if let Err(error) = process.exit_completion.completed()? {
            record_cleanup_error(&process, &error);
        }
        if let Err(error) = process.guard_completion.completed()? {
            record_cleanup_error(&process, &error);
        }
        reconcile_restore(&process)?;
    }
    Ok(())
}

pub(crate) fn has_child_cleanup_records(root: &Path, owner: &str, child: &str) -> bool {
    child_processes(root, owner, child)
        .iter()
        .any(|p| !p.persistent)
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
        if !crate::shell::kill_tree(pid).await {
            return false;
        }
        process.mark_terminated();
    }
    if process.is_running() {
        return false;
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
