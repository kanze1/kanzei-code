use super::*;
use std::{io::Write, process::Stdio, time::Instant};

/// App 与 CLI 共用的隐藏入口；在窗口、更新器和主运行器初始化之前处理。
pub fn worker_entry(args: &[String]) -> Option<i32> {
    if args.first().map(String::as_str) != Some("--verification-worker") {
        return None;
    }
    let result = (|| -> anyhow::Result<()> {
        ensure!(
            args.len() == 3,
            "verification worker requires project and id"
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(worker_command(args))
    })();
    Some(if result.is_ok() { 0 } else { 1 })
}

pub(super) async fn run(root: &Path, id: &str) -> anyhow::Result<()> {
    let root = root.canonicalize()?;
    let Some(_lease) =
        crate::atomic_file::try_lock_exclusive(&job_file(&root, id)?, Duration::ZERO)?
    else {
        return Ok(());
    };
    let mut job = read_job(&root, id)?;
    if job.status != "queued" {
        return Ok(());
    }
    job.worker_pid = Some(std::process::id());
    save(&mut job)?;
    let result = execute(&mut job).await;
    if let Err(error) = result {
        job.status = "interrupted".into();
        job.error = Some(error.to_string());
    }
    finish(&mut job)
}

async fn execute(job: &mut VerificationJob) -> anyhow::Result<()> {
    let cancelled = job_file(&job.project, &job.id)?.with_extension("cancel");
    let queue_dir = artifact_root()?.join("resources");
    std::fs::create_dir_all(&queue_dir)?;
    let resource = queue_dir.join(hash(&job.resource));
    let resource_lock;
    loop {
        if cancelled.exists() {
            job.status = "cancelled".into();
            return Ok(());
        }
        if !is_current(job)? {
            job.status = "superseded".into();
            return Ok(());
        }
        if let Some(lock) = crate::atomic_file::try_lock_exclusive(&resource, Duration::ZERO)? {
            resource_lock = lock;
            break;
        }
        save(job)?;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let _resource_lock = resource_lock;
    ensure!(
        snapshot::intact(&job.snapshot, &job.manifest)?,
        "snapshot source changed before verification"
    );
    job.status = "running".into();
    save(job)?;
    let mut log = std::fs::File::create(&job.log_path)?;
    writeln!(
        log,
        "job={}\nsource=sha256:{}\nenvironment={}\nplatform={}\ncommand={}\n",
        job.id, job.source_fingerprint, job.environment, job.platform, job.command
    )?;
    let shell = crate::shell::detected_shell();
    let mut command = tokio::process::Command::new(&shell.program);
    let text = crate::shell::command_with_utf8_output(shell.name, &job.command);
    command
        .args(&shell.args)
        .arg(text)
        .current_dir(&job.snapshot)
        .env("PATH", crate::shell::fresh_path())
        .env_remove("KANZEI_PROJECT_ROOT")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    job.command_pid = child.id();
    save(job)?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            job.exit_code = status.code();
            job.status = if status.success() { "passed" } else { "failed" }.into();
            if !status.success() {
                job.error = Some(format!(
                    "命令退出 {}；见 {}",
                    status,
                    job.log_path.display()
                ));
            }
            break;
        }
        let stop = if cancelled.exists() {
            Some("cancelled")
        } else if started.elapsed().as_millis() as u64 > job.timeout_ms {
            Some("timed_out")
        } else if std::fs::metadata(&job.log_path)?.len() > 64 * 1024 * 1024 {
            Some("output_limit")
        } else {
            None
        };
        if let Some(reason) = stop {
            if let Some(pid) = child.id() {
                crate::shell::kill_tree(pid).await;
            }
            #[cfg(unix)]
            {
                extern "C" {
                    fn kill(pid: i32, signal: i32) -> i32;
                }
                // SAFETY: 此处 pid 是刚创建且仍持有的子进程，其进程组由上面的 process_group(0) 建立。
                if let Some(pid) = child.id() {
                    unsafe {
                        kill(-(pid as i32), 9);
                    }
                }
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            job.status = reason.into();
            job.error = Some(format!("验证停止：{reason}；见 {}", job.log_path.display()));
            break;
        }
        if now().saturating_sub(job.updated_at) > 1000 {
            if !is_current(job)? {
                std::fs::write(&cancelled, b"unit superseded")?;
            }
            save(job)?;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if job.status == "passed" && !snapshot::intact(&job.snapshot, &job.manifest)? {
        job.status = "stale".into();
        job.error = Some("验证命令修改了快照中的源码；此结果不能证明冻结版本".into());
    }
    Ok(())
}

fn is_current(job: &VerificationJob) -> anyhow::Result<bool> {
    Ok(store(&job.project)?
        .get_work_unit(&job.unit_id)?
        .is_some_and(|u| {
            u.status == WorkUnitStatus::Verifying
                && u.background_verification
                    .is_some_and(|v| v.job_id == job.id && v.pending)
        }))
}
