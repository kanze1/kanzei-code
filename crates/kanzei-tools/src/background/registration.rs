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
pub fn register(
    mut child: tokio::process::Child,
    command: String,
    project_root: &Path,
    workdir: &Path,
    owner: BackgroundOwner,
    baseline: ManagedSnapshot,
    persistent: bool,
) -> Arc<BackgroundProcess> {
    let id = next_id();
    let output = Arc::new(Mutex::new(Vec::new()));
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

    for stream in [stdout.take().map(Ok), stderr.take().map(Err)]
        .into_iter()
        .flatten()
    {
        let output = output.clone();
        let truncated = truncated.clone();
        let full_output = full_output.clone();
        let log_path = log_path.clone();
        tokio::spawn(async move {
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
                        append_bounded(&output, &truncated, &chunk[..n]);
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
        });
    }

    let process = Arc::new(BackgroundProcess {
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
        truncated,
        exit,
        baseline: Arc::new(Mutex::new(baseline)),
        breaches: Arc::new(Mutex::new(Vec::new())),
    });
    registry().lock().unwrap().insert(id, process.clone());
    // 必须在内存注册表插入后再启动 wait 任务：否则瞬时退出的子进程可能
    // 先完成 wait、删除一个尚不存在的条目，随后又被插入成幽灵。
    {
        let exit = process.exit.clone();
        let reg_root = project_root.to_path_buf();
        let reg_id = process.id.clone();
        let reg_persistent = process.persistent;
        tokio::spawn(async move {
            let status = child.wait().await.ok().and_then(|s| s.code());
            let mut recorded_exit = exit.lock().unwrap();
            if recorded_exit.is_none() {
                *recorded_exit = Some(status);
            }
            drop(recorded_exit);
            registry().lock().unwrap().remove(&reg_id);
            if reg_persistent {
                remove_registry_entry(&reg_root, &reg_id);
            }
        });
    }
    // 托管项目才需要守卫;非托管项目没有托管树可对账,不必空转。
    if crate::managed::managed_scope_exists(project_root) {
        install_window_observer_once();
        spawn_guard(process.clone());
    }
    process
}
