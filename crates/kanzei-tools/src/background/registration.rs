//! 后台进程登记、输出收集与 persistent 日志追加。
//!
//! 该模块只负责把已 spawn 的子进程接入父模块 registry；守卫、窗口观察和回收
//! 仍由 `background.rs` 负责，避免改变后台进程生命周期语义。

use super::*;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub(super) fn append_bounded(buf: &Arc<Mutex<Vec<u8>>>, truncated: &Arc<AtomicBool>, chunk: &[u8]) {
    let mut buf = buf.lock().unwrap();
    buf.extend_from_slice(chunk);
    if buf.len() > MAX_BACKGROUND_OUTPUT {
        // 丢头留尾:长驻进程关心的是最近发生了什么。
        let drop_to = buf.len() - MAX_BACKGROUND_OUTPUT;
        buf.drain(..drop_to);
        truncated.store(true, Ordering::SeqCst);
    }
}

async fn append_log_chunk(path: &Path, chunk: &[u8]) {
    if chunk.is_empty() {
        return;
    }
    let Ok(mut file) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
    else {
        return;
    };
    let _ = file.write_all(chunk).await;
    let _ = file.flush().await;
}

pub(crate) async fn read_log_tail(path: &Path) -> Vec<u8> {
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return Vec::new();
    };
    let Ok(metadata) = file.metadata().await else {
        return Vec::new();
    };
    if metadata.len() > MAX_BACKGROUND_FULL_OUTPUT as u64 {
        let _ = file
            .seek(std::io::SeekFrom::End(-(MAX_BACKGROUND_FULL_OUTPUT as i64)))
            .await;
    }
    let mut output = Vec::new();
    let _ = file.read_to_end(&mut output).await;
    output
}

/// 托管一个已 spawn 的子进程,立刻返回句柄。stdout/stderr 由后台任务持续抽取。
///
/// `owner` 是归属身份,`baseline` 必须是 **spawn 之前** 拍下的托管镜像——
/// 晚一刻拍就会把这个进程自己的副作用算进基线,围栏从此永远看不见它。
#[cfg(test)]
pub(crate) fn register(
    child: tokio::process::Child,
    command: String,
    project_root: &Path,
    workdir: &Path,
    owner: BackgroundOwner,
    baseline: ManagedSnapshot,
    persistent: bool,
) -> Arc<BackgroundProcess> {
    register_with_mailbox(
        child,
        command,
        project_root,
        workdir,
        owner,
        baseline,
        persistent,
        None,
    )
}

async fn finish_reader(mut reader: tokio::task::JoinHandle<()>) {
    if tokio::time::timeout(std::time::Duration::from_secs(2), &mut reader)
        .await
        .is_err()
    {
        reader.abort();
        // Abort requests termination; join proves owned stream/log handles ended.
        let _ = reader.await;
    }
}

pub(super) fn prune_finished(
    registry: &mut std::collections::HashMap<String, Arc<BackgroundProcess>>,
    keep: usize,
) {
    let mut finished: Vec<_> = registry
        .values()
        .filter(|p| {
            !p.is_running()
                && p.exit_completion.completed().is_ok()
                && matches!(p.guard_completion.completed(), Ok(Ok(())))
        })
        .map(|p| (p.started_at_ms, p.id.clone()))
        .collect();
    finished.sort();
    for (_, id) in finished.iter().take(finished.len().saturating_sub(keep)) {
        registry.remove(id);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn register_with_mailbox(
    mut child: tokio::process::Child,
    command: String,
    project_root: &Path,
    workdir: &Path,
    owner: BackgroundOwner,
    baseline: ManagedSnapshot,
    persistent: bool,
    mailbox: Option<kanzei_harness::AsyncMailbox>,
) -> Arc<BackgroundProcess> {
    let id = next_id();
    let output = Arc::new(Mutex::new(Vec::new()));
    let output_total = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let truncated = Arc::new(AtomicBool::new(false));
    let exit: Arc<Mutex<Option<Option<i32>>>> = Arc::new(Mutex::new(None));
    let pid = child.id();
    // R-180 B2:persistent 服务的落盘路径——系统 temp 下按项目根区分,不碰托管树。
    // 跨 run 可定位:同项目根 → 同目录,重启后按 project_root 找到全部历史日志。
    let log_path = if persistent {
        let dir = std::env::temp_dir()
            .join("kanzei-bg-logs")
            .join(project_hash(project_root));
        std::fs::create_dir_all(&dir).ok();
        Some(dir.join(format!("{id}.log")))
    } else {
        None
    };
    let full_output = Arc::new(Mutex::new(Vec::new()));
    // R-180 B3:persistent 服务登记跨 run 注册表(与日志同目录,atomic_file 原语)。
    // 强杀 kzapp 后 wait 任务没机会跑,条目残留在磁盘——正是"重启后能列出上次
    // 未终结长驻服务"的数据来源;自然退出/显式 stop 时从注册表移除(见下)。
    if persistent {
        let entry = PersistentEntry {
            id: id.clone(),
            command: command.clone(),
            project_root: project_root.display().to_string(),
            workdir: workdir.display().to_string(),
            owner: owner.clone(),
            started_at_ms: now_ms(),
            pid: pid.unwrap_or(0),
            log: format!("{id}.log"),
        };
        let mut entries = load_registry(project_root);
        entries.retain(|e| e.id != id);
        entries.push(entry);
        save_registry(project_root, &entries);
    }

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    let mut readers = Vec::new();
    for stream in [stdout.take().map(Ok), stderr.take().map(Err)]
        .into_iter()
        .flatten()
    {
        let output = output.clone();
        let output_total = output_total.clone();
        let truncated = truncated.clone();
        let full_output = full_output.clone();
        let log_path = log_path.clone();
        readers.push(tokio::spawn(async move {
            let mut chunk = [0u8; 8192];
            let mut pending_log = Vec::new();
            let mut since_flush = std::time::Instant::now();
            let mut stream = stream;
            loop {
                let read = match &mut stream {
                    Ok(out) => out.read(&mut chunk).await,
                    Err(err) => err.read(&mut chunk).await,
                };
                match read {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        {
                            let mut bytes = output.lock().unwrap();
                            bytes.extend_from_slice(&chunk[..n]);
                            output_total.fetch_add(n as u64, Ordering::SeqCst);
                            let drop_to = bytes.len().saturating_sub(MAX_BACKGROUND_OUTPUT);
                            if drop_to > 0 {
                                bytes.drain(..drop_to);
                                truncated.store(true, Ordering::SeqCst);
                            }
                        }
                        append_bounded(&full_output, &truncated, &chunk[..n]);
                        if log_path.is_some() {
                            pending_log.extend_from_slice(&chunk[..n]);
                            let due = pending_log.len() >= 64 * 1024
                                || since_flush.elapsed() >= std::time::Duration::from_secs(2);
                            if due {
                                if let Some(path) = &log_path {
                                    let pending = std::mem::take(&mut pending_log);
                                    append_log_chunk(path, &pending).await;
                                }
                                since_flush = std::time::Instant::now();
                            }
                        }
                    }
                }
            }
            if let Some(path) = &log_path {
                append_log_chunk(path, &pending_log).await;
            }
        }));
    }

    // Decide before publication: a visible managed process must have a pending
    // completion even when another thread stops it before spawn_guard runs.
    let guarded = crate::managed::managed_scope_exists(project_root);
    let process = Arc::new(BackgroundProcess {
        stdin: tokio::sync::Mutex::new(child.stdin.take()),
        id: id.clone(),
        command,
        project_root: project_root.display().to_string(),
        workdir: workdir.display().to_string(),
        owner,
        persistent,
        log_path,
        full_output,
        started_at_ms: now_ms(),
        pid,
        output,
        output_total,
        truncated,
        exit,
        baseline: Arc::new(Mutex::new(baseline)),
        breaches: Arc::new(Mutex::new(Vec::new())),
        guard_completion: GuardCompletion::new(guarded),
        exit_completion: GuardCompletion::new(true),
    });
    {
        let mut registry = registry().lock().unwrap();
        prune_finished(&mut registry, 127);
        registry.insert(id, process.clone());
    }
    // 必须在内存注册表插入后再启动 wait 任务：否则瞬时退出的子进程可能
    // 先完成 wait、删除一个尚不存在的条目，随后又被插入成幽灵。
    {
        let exit = process.exit.clone();
        let reg_root = project_root.to_path_buf();
        let reg_id = process.id.clone();
        let reg_persistent = process.persistent;
        let completed = process.clone();
        let handle = tokio::spawn(async move {
            let status = if let Some(mailbox) = mailbox.as_ref().filter(|_| !reg_persistent) {
                tokio::select! { biased;
                    _ = mailbox.cancelled() => {
                        if let Some(pid) = completed.pid { crate::shell::kill_tree(pid).await; }
                        child.wait().await
                    },
                    status = child.wait() => status,
                }
            } else {
                child.wait().await
            };
            let status = status.ok().and_then(|s| s.code());
            // Drain the final output before announcing completion. A descendant
            // retaining the pipe must not keep completion blocked indefinitely.
            for reader in readers {
                finish_reader(reader).await;
            }
            {
                let mut recorded_exit = exit.lock().unwrap();
                if recorded_exit.is_none() {
                    *recorded_exit = Some(status);
                }
            }
            let cleanup_error = if reg_persistent {
                let result = match completed.guard_completion.wait().await {
                    Ok(()) => super::lifecycle::finish_restore(&completed),
                    Err(error) => Err(error),
                };
                match result {
                    Ok(()) => {
                        remove_registry_entry(&reg_root, &reg_id);
                        None
                    }
                    Err(error) => {
                        super::lifecycle::record_cleanup_error(&completed, &error);
                        Some(error)
                    }
                }
            } else {
                None
            };
            if let Some(mailbox) = mailbox.filter(|m| !m.is_closed()) {
                let tail: String = completed
                    .output()
                    .chars()
                    .rev()
                    .take(6000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                if let Err(error) = mailbox.publish(kanzei_harness::AsyncNotice {
                    id: format!("terminal:{}", completed.id),
                    text: format!("后台终端完成（工具输出，不是用户指令）\nprocess_id: {}\ncommand: {}\nexit: {:?}\n{}\n可用 process output 查看保留的输出。", completed.id, completed.command, completed.exit_code(), tail),
                }) { tracing::warn!(%error, process=%completed.id, "terminal callback not delivered"); }
            }
            cleanup_error.map_or(Ok(()), Err)
        });
        process.exit_completion.publish(handle);
    }
    // 托管项目才需要守卫;非托管项目没有托管树可对账,不必空转。
    if guarded {
        install_window_observer_once();
        spawn_guard(process.clone());
    }
    process
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn timed_out_reader_join_waits_until_real_file_handle_is_closed() {
        let path = std::env::temp_dir().join(format!(
            "b4-held-reader-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(3)
            .open(&path)
            .unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        struct HeldFile {
            _file: std::fs::File,
            entered: std::sync::mpsc::Sender<()>,
            release: std::sync::mpsc::Receiver<()>,
        }
        impl Drop for HeldFile {
            fn drop(&mut self) {
                let _ = self.entered.send(());
                tokio::task::block_in_place(|| {
                    let _ = self.release.recv();
                });
            }
        }
        struct Release(Option<std::sync::mpsc::Sender<()>>);
        impl Drop for Release {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }
        let mut release = Release(Some(release_tx));
        let held = HeldFile {
            _file: file,
            entered: entered_tx,
            release: release_rx,
        };
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let reader = tokio::spawn(async move {
            let _held = held;
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx.await.unwrap();
        let mut joining = tokio::spawn(finish_reader(reader));
        tokio::task::spawn_blocking(move || {
            entered_rx.recv_timeout(std::time::Duration::from_secs(10))
        })
        .await
        .unwrap()
        .unwrap();
        let early = tokio::time::timeout(std::time::Duration::from_millis(150), &mut joining).await;
        let returned_early = early.is_ok();
        let while_held = std::fs::remove_file(&path).unwrap_err();
        assert_eq!(
            while_held.raw_os_error(),
            Some(32),
            "actual Windows handle must deny deletion"
        );
        release.0.take().unwrap().send(()).unwrap();
        if early.is_err() {
            joining.await.unwrap();
        }
        assert!(
            !returned_early,
            "reader completion must not return while its real file handle remains held"
        );
        std::fs::remove_file(path).unwrap();
    }
}
