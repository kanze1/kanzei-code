//! 冻结源码的持久验证队列。由 bash 的既有命令授权与 guards 入场，独立进程执行。
mod process;
mod snapshot;
mod worker;

use anyhow::{ensure, Context};
use kanzei_core::{SessionStore, WorkFact, WorkUnitStatus};
use kanzei_harness::ToolCtx;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub use worker::worker_entry;
pub async fn worker_command(args: &[String]) -> anyhow::Result<()> {
    ensure!(
        args.len() == 3,
        "verification worker requires project and id"
    );
    if let Err(error) = process::contain_worker() {
        let mut job = read_job(&Path::new(&args[1]).canonicalize()?, &args[2])?;
        job.status = "interrupted".into();
        job.error = Some(format!("cannot contain verification worker: {error}"));
        finish(&mut job)?;
        return Err(error.into());
    }
    worker::run(Path::new(&args[1]), &args[2]).await
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VerificationRequest {
    pub unit_id: String,
    /// 本命令实际验证的 acceptance 原文；通过后只为这些标准登记证据。
    pub criteria: Vec<String>,
    /// 共享构建资源键。相同键串行；开发线不等待它。
    #[serde(default = "default_resource")]
    pub resource: String,
    /// 工具链/平台说明。记录于证据；本批不跨环境复用测试结果。
    pub environment: String,
}
fn default_resource() -> String {
    "local-build".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VerificationJob {
    pub id: String,
    /// Results return to the conversation that submitted this job, regardless of its ID format.
    #[serde(default)]
    pub session_id: Option<String>,
    pub project: PathBuf,
    pub source: PathBuf,
    pub snapshot: PathBuf,
    pub unit_id: String,
    pub command: String,
    pub criteria: Vec<String>,
    pub resource: String,
    pub environment: String,
    pub platform: String,
    pub source_fingerprint: String,
    pub manifest: Vec<snapshot::SourceFile>,
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub timeout_ms: u64,
    pub log_path: PathBuf,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub worker_pid: Option<u32>,
    pub command_pid: Option<u32>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn hash(data: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(data.as_ref()))
}

fn artifact_root() -> anyhow::Result<PathBuf> {
    let base = if cfg!(test) {
        std::env::temp_dir()
    } else {
        dirs::data_local_dir().context("local application data directory unavailable")?
    };
    Ok(base.join("kanzei/verification"))
}
fn job_file(root: &Path, id: &str) -> anyhow::Result<PathBuf> {
    ensure!(
        id.starts_with("v-") && id.len() == 34 && id[2..].chars().all(|c| c.is_ascii_hexdigit()),
        "invalid verification id"
    );
    Ok(root.join(".kanzei/verification").join(format!("{id}.json")))
}
fn read_job(root: &Path, id: &str) -> anyhow::Result<VerificationJob> {
    let job: VerificationJob = serde_json::from_slice(&std::fs::read(job_file(root, id)?)?)?;
    ensure!(
        job.id == id && job.project == root.canonicalize()?,
        "verification ownership mismatch"
    );
    Ok(job)
}
fn save(job: &mut VerificationJob) -> anyhow::Result<()> {
    job.updated_at = now();
    crate::atomic_file::write_atomic_bytes(
        &job_file(&job.project, &job.id)?,
        &serde_json::to_vec_pretty(job)?,
    )?;
    Ok(())
}
fn store(root: &Path) -> anyhow::Result<SessionStore> {
    Ok(SessionStore::open(&kanzei_core::project_state_path(root))?)
}
fn work_lock(root: &Path) -> anyhow::Result<crate::atomic_file::FileLock> {
    Ok(crate::atomic_file::lock_exclusive(
        &root.join(".kanzei/project/work-selection"),
    )?)
}

pub fn list_jobs(root: &Path) -> anyhow::Result<Vec<VerificationJob>> {
    let dir = root.join(".kanzei/verification");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let root = root.canonicalize()?;
    let mut jobs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|v| v.to_str())
            .context("invalid job filename")?;
        jobs.push(read_job(&root, id)?);
    }
    jobs.sort_by_key(|j| std::cmp::Reverse(j.created_at));
    Ok(jobs)
}

fn prepare(
    ctx: &ToolCtx,
    command: &str,
    timeout_ms: u64,
    request: &VerificationRequest,
) -> anyhow::Result<VerificationJob> {
    ensure!(!command.trim().is_empty(), "verification command required");
    ensure!(
        !request.environment.trim().is_empty() && request.environment.len() <= 2000,
        "environment description required (up to 2000 bytes)"
    );
    ensure!(
        !request.resource.trim().is_empty() && request.resource.len() <= 120,
        "resource key required (up to 120 bytes)"
    );
    let root = ctx.project_root.canonicalize()?;
    let source = ctx.cwd.canonicalize()?;
    let _guard = work_lock(&root)?;
    let db = store(&root)?;
    let unit = db
        .get_work_unit(&request.unit_id)?
        .context("unknown work unit")?;
    if let Some(v) = &unit.background_verification {
        if v.pending {
            let job = read_job(&root, &v.job_id)?;
            ensure!(
                job.command == command
                    && job.criteria == request.criteria
                    && job.environment == request.environment
                    && job.resource == request.resource
                    && job.source == source,
                "unit already has a different verification; cancel it before replacing"
            );
            return Ok(job);
        }
    }
    ensure!(
        unit.status == WorkUnitStatus::Active,
        "only active units can submit verification"
    );
    ensure!(
        !request.criteria.is_empty()
            && request.criteria.len() <= 32
            && request.criteria.iter().all(|c| unit.acceptance.contains(c)),
        "criteria must be exact acceptance entries for this unit"
    );
    ensure!(
        unit.claimed_by == crate::work::line_identity(&ctx.cwd, &ctx.project_root),
        "verification must be submitted by the owning work line"
    );
    let id = format!(
        "v-{}",
        &hash(format!(
            "{}:{}:{}:{:?}",
            root.display(),
            request.unit_id,
            std::process::id(),
            SystemTime::now()
        ))[..32]
    );
    let artifact = artifact_root()?.join(&id);
    let snapshot = artifact.join("source");
    let (manifest, fingerprint) = snapshot::freeze(&source, &snapshot)?;
    let mut job = VerificationJob {
        id,
        session_id: ctx.session_id.clone(),
        project: root,
        source,
        snapshot,
        unit_id: request.unit_id.clone(),
        command: command.into(),
        criteria: request.criteria.clone(),
        resource: request.resource.clone(),
        environment: request.environment.clone(),
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        source_fingerprint: fingerprint,
        manifest,
        status: "queued".into(),
        created_at: now(),
        updated_at: now(),
        timeout_ms: timeout_ms.clamp(1000, 600_000),
        log_path: artifact.join("output.log"),
        exit_code: None,
        error: None,
        worker_pid: None,
        command_pid: None,
    };
    std::fs::create_dir_all(job.project.join(".kanzei/verification"))?;
    save(&mut job)?;
    if let Err(error) = db.append_work_fact(
        &job.unit_id,
        WorkFact::VerificationQueued {
            job_id: job.id.clone(),
            snapshot_fingerprint: job.source_fingerprint.clone(),
        },
    ) {
        job.status = "interrupted".into();
        job.error = Some(error.to_string());
        save(&mut job)?;
        return Err(error.into());
    }
    Ok(job)
}

pub fn submit(
    ctx: &ToolCtx,
    command: &str,
    timeout_ms: u64,
    request: &VerificationRequest,
) -> anyhow::Result<VerificationJob> {
    let mut job = prepare(ctx, command, timeout_ms, request)?;
    if job.status != "queued" {
        return Ok(job);
    }
    if let Err(error) = process::spawn(&job) {
        job.status = "interrupted".into();
        job.error = Some(format!("cannot start verification worker: {error}"));
        finish(&mut job)?;
        return Err(error.into());
    }
    Ok(job)
}

pub fn cancel(root: &Path, id: &str) -> anyhow::Result<()> {
    let root = root.canonicalize()?;
    let job = read_job(&root, id)?;
    ensure!(
        matches!(job.status.as_str(), "queued" | "running"),
        "job is already terminal"
    );
    std::fs::write(
        job_file(&root, id)?.with_extension("cancel"),
        b"cancel requested",
    )?;
    Ok(())
}

/// 失去 worker 的任务不永久占 verifying；恢复只登记中断，不静默重跑命令。
pub fn recover(root: &Path) -> anyhow::Result<()> {
    for mut job in list_jobs(root)? {
        if !matches!(job.status.as_str(), "queued" | "running")
            || now().saturating_sub(job.updated_at) < 30_000
        {
            continue;
        }
        if let Some(_guard) = crate::atomic_file::try_lock_exclusive(
            &job_file(&job.project, &job.id)?,
            Duration::ZERO,
        )? {
            job.status = "interrupted".into();
            job.error = Some("验证进程已退出；保留日志，确认原因后再提交".into());
            finish(&mut job)?;
        }
    }
    Ok(())
}

fn finish(job: &mut VerificationJob) -> anyhow::Result<()> {
    let _guard = work_lock(&job.project)?;
    let db = store(&job.project)?;
    let current = db
        .get_work_unit(&job.unit_id)?
        .context("work unit removed")?;
    let pending = current
        .background_verification
        .as_ref()
        .is_some_and(|v| v.job_id == job.id && v.pending);
    if !pending || current.status != WorkUnitStatus::Verifying {
        if current
            .background_verification
            .as_ref()
            .is_some_and(|v| v.job_id == job.id && !v.pending)
            && job.criteria.iter().all(|c| {
                current.evidence.iter().any(|e| {
                    &e.criterion == c
                        && e.evidence_refs
                            .iter()
                            .any(|r| r.starts_with(&format!("verification:{};", job.id)))
                })
            })
        {
            job.status = "passed".into();
            job.error = None;
            return save(job);
        }
        job.status = "superseded".into();
        job.error = Some("单元已被阻塞、替换或重新提交；旧结果不修改当前状态".into());
        return save(job);
    }
    let passed = job.status == "passed";
    let mut facts = vec![WorkFact::VerificationConcluded {
        job_id: job.id.clone(),
        passed,
        reason: job
            .error
            .clone()
            .unwrap_or_else(|| format!("验证 {}，见 {}", job.status, job.log_path.display())),
    }];
    if passed {
        for criterion in &job.criteria {
            facts.push(WorkFact::EvidenceAdded {
                evidence: kanzei_core::WorkEvidence {
                    criterion: criterion.clone(),
                    evidence_refs: vec![format!(
                        "verification:{}; source=sha256:{}; log={}",
                        job.id,
                        job.source_fingerprint,
                        job.log_path.display()
                    )],
                },
            });
        }
        if current.acceptance.iter().all(|c| job.criteria.contains(c)) {
            facts.push(WorkFact::Completed);
        }
    }
    db.append_work_facts(&job.unit_id, facts)?;
    save(job)
}

#[cfg(test)]
mod tests;
