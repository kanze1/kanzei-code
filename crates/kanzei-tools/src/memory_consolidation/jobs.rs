//! One OS-backed owner per project, shared by automatic, manual and CLI callers.
use std::collections::HashMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use kanzei_memory::memory::{InboxCheckpoint, MemoryJobProgress, MemoryJobStatus, MemoryStore};
pub type JobSnapshot = MemoryJobProgress;

pub(super) struct JobControl {
    pub halt: kanzei_core::CancellationToken,
    pub snapshot: Mutex<JobSnapshot>,
    store: MemoryStore,
}

static JOBS: LazyLock<Mutex<HashMap<PathBuf, Arc<JobControl>>>> = LazyLock::new(Mutex::default);
const MAX_REQUEST_ID_BYTES: usize = 128;

pub(super) fn previous_request(
    root: &Path,
    request_id: &str,
) -> anyhow::Result<Option<JobSnapshot>> {
    anyhow::ensure!(
        !request_id.trim().is_empty()
            && request_id.len() <= MAX_REQUEST_ID_BYTES
            && kanzei_memory::memory::redact_credentials(request_id) == request_id,
        "记忆整理请求标识无效"
    );
    let state = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))?;
    let session = kanzei_core::project_session_id(root);
    state
        .latest_matching_event(&session, "memory.job", "$.request_id", request_id)?
        .map(|event| serde_json::from_value(event.payload).map_err(Into::into))
        .transpose()
}

pub(super) fn request_snapshot(
    root: &Path,
    request_id: &str,
) -> anyhow::Result<Option<JobSnapshot>> {
    let Some(mut previous) = previous_request(root, request_id)? else {
        return Ok(None);
    };
    if matches!(
        previous.status,
        MemoryJobStatus::Running | MemoryJobStatus::Cancelling
    ) {
        if let Some(current) = snapshot(root)?.filter(|job| job.id == previous.id) {
            previous = current;
            if matches!(
                previous.status,
                MemoryJobStatus::Running | MemoryJobStatus::Cancelling
            ) {
                previous.status = MemoryJobStatus::Busy;
            }
        } else {
            previous.status = MemoryJobStatus::Interrupted;
        }
    }
    Ok(Some(previous))
}

fn record_request(store: &MemoryStore, job: &JobSnapshot) -> anyhow::Result<()> {
    let Some(request_id) = job.request_id.as_deref() else {
        return Ok(());
    };
    let root = store
        .project_root
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("记忆作业缺少所属项目"))?;
    let state = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))?;
    let session = kanzei_core::project_session_id(root);
    state.create_session(&session, &root.display().to_string(), None)?;
    let value = serde_json::to_value(job)?;
    if state
        .latest_matching_event(&session, "memory.job", "$.request_id", request_id)?
        .is_none_or(|event| event.payload != value)
    {
        state.append_event(&session, "memory.job", &value)?;
    }
    Ok(())
}

pub(super) struct JobLease {
    root: PathBuf,
    pub control: Arc<JobControl>,
    file: Option<File>,
}

fn scoped_lock(root: &Path) -> anyhow::Result<kanzei_harness::permission::ScopedFile> {
    let path = root.join(".kanzei/memory-consolidation.lock");
    let file = kanzei_harness::permission::ScopedFile::resolve(root, root, &path)?;
    anyhow::ensure!(
        kanzei_harness::permission::FileResource::resolve(root, &path)?
            .sensitivity()
            .is_none(),
        "记忆作业锁不能指向敏感文件"
    );
    Ok(file)
}

impl JobLease {
    pub fn begin(
        root: &Path,
        store: &MemoryStore,
        request_id: Option<&str>,
    ) -> anyhow::Result<Option<Self>> {
        std::fs::create_dir_all(root.join(".kanzei"))?;
        let root = std::fs::canonicalize(root)?;
        let target = scoped_lock(&root)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(target.path())?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        if let Some(id) = request_id {
            if previous_request(&root, id)?.is_some() {
                return Ok(None);
            }
        }
        let started_at_ms = super::now_ms();
        let control = Arc::new(JobControl {
            halt: Default::default(),
            store: store.clone(),
            snapshot: Mutex::new(JobSnapshot {
                id: format!(
                    "memory-job-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos()
                ),
                owner_pid: std::process::id(),
                request_id: request_id.map(str::to_owned),
                status: MemoryJobStatus::Running,
                error: None,
                started_at_ms,
                steps: 0,
                input_tokens: 0,
                output_tokens: 0,
                usage_reported: false,
                provider: None,
                model: None,
            }),
        });
        let initial = control.snapshot.lock().unwrap().clone();
        record_request(store, &initial)?;
        store.write_inbox_checkpoint(&InboxCheckpoint {
            batch_id: initial.id.clone(),
            status: "processing".into(),
            input_notes: 0,
            input_bytes: 0,
            success_notes: 0,
            pending_after: store.pending_notes(),
            failure_reason: None,
            consecutive_failures: 0,
            updated_at_ms: super::now_ms(),
            job: Some(initial),
        })?;
        JOBS.lock()
            .map_err(|_| anyhow::anyhow!("memory job registry poisoned"))?
            .insert(root.clone(), control.clone());
        Ok(Some(Self {
            root,
            control,
            file: Some(file),
        }))
    }
}

impl JobControl {
    pub fn publish(&self) -> anyhow::Result<()> {
        let snapshot = self.snapshot.lock().unwrap();
        self.persist(&snapshot)
    }

    pub fn transition(&self, status: MemoryJobStatus, error: Option<&str>) -> anyhow::Result<()> {
        let mut current = self.snapshot.lock().unwrap();
        let mut next = current.clone();
        next.status = status;
        if let Some(error) = error {
            next.error = Some(kanzei_memory::memory::redact_credentials(error));
        }
        self.persist(&next)?;
        *current = next;
        Ok(())
    }

    fn persist(&self, snapshot: &JobSnapshot) -> anyhow::Result<()> {
        let mut checkpoint = self
            .store
            .read_inbox_checkpoint_checked()?
            .ok_or_else(|| anyhow::anyhow!("记忆作业检查点丢失"))?;
        anyhow::ensure!(
            checkpoint
                .job
                .as_ref()
                .is_some_and(|job| job.id == snapshot.id),
            "记忆作业检查点归属已经变化"
        );
        checkpoint.job = Some(snapshot.clone());
        checkpoint.updated_at_ms = super::now_ms();
        self.store.write_inbox_checkpoint(&checkpoint)?;
        if !matches!(
            snapshot.status,
            MemoryJobStatus::Running | MemoryJobStatus::Cancelling
        ) {
            record_request(&self.store, snapshot)?;
        }
        Ok(())
    }

    pub fn persistence_failed(&self, error: anyhow::Error) {
        let mut snapshot = self.snapshot.lock().unwrap();
        snapshot.error = Some(kanzei_memory::memory::redact_credentials(&format!(
            "作业进度保存失败: {error}"
        )));
        snapshot.status = MemoryJobStatus::Failed;
        self.halt.cancel();
    }
}

impl Drop for JobLease {
    fn drop(&mut self) {
        let status = self.control.snapshot.lock().unwrap().status;
        let status = if matches!(
            status,
            MemoryJobStatus::Running | MemoryJobStatus::Cancelling
        ) {
            MemoryJobStatus::Interrupted
        } else {
            status
        };
        if let Err(error) = self.control.transition(status, None) {
            tracing::error!(%error, "memory job final checkpoint failed");
        }
        drop(self.file.take());
        let mut jobs = JOBS.lock().unwrap_or_else(|poison| poison.into_inner());
        if jobs
            .get(&self.root)
            .is_some_and(|control| Arc::ptr_eq(control, &self.control))
        {
            jobs.remove(&self.root);
        }
    }
}

fn local(root: &Path) -> anyhow::Result<Option<Arc<JobControl>>> {
    if !root.try_exists()? {
        return Ok(None);
    }
    let root = std::fs::canonicalize(root)?;
    Ok(JOBS
        .lock()
        .map_err(|_| anyhow::anyhow!("memory job registry poisoned"))?
        .get(&root)
        .cloned())
}

pub fn can_cancel(root: &Path) -> anyhow::Result<bool> {
    Ok(local(root)?.is_some_and(|control| {
        matches!(
            control.snapshot.lock().unwrap().status,
            MemoryJobStatus::Running | MemoryJobStatus::Cancelling
        )
    }))
}

pub fn snapshot(root: &Path) -> anyhow::Result<Option<JobSnapshot>> {
    if let Some(control) = local(root)? {
        return Ok(Some(control.snapshot.lock().unwrap().clone()));
    }
    let mut store = MemoryStore::open(
        kanzei_memory::memory::MemoryScope::Project,
        kanzei_memory::memory::project_memory_root(root),
    );
    store.project_root = Some(root.to_path_buf());
    let Some(mut job) = store
        .read_inbox_checkpoint_checked()?
        .and_then(|checkpoint| checkpoint.job)
    else {
        return Ok(None);
    };
    if matches!(
        job.status,
        MemoryJobStatus::Running | MemoryJobStatus::Cancelling
    ) {
        let path = root.join(".kanzei/memory-consolidation.lock");
        if !path.try_exists()? {
            job.status = MemoryJobStatus::Interrupted;
        } else {
            let target = scoped_lock(root)?;
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(target.path())?;
            match file.try_lock() {
                Ok(()) => job.status = MemoryJobStatus::Interrupted,
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }
    Ok(Some(job))
}

pub fn cancel(root: &Path, id: &str) -> anyhow::Result<bool> {
    let Some(control) = local(root)? else {
        return Ok(false);
    };
    {
        let mut snapshot = control.snapshot.lock().unwrap();
        anyhow::ensure!(snapshot.id == id, "记忆作业已经变化，请刷新后停止");
        if !matches!(
            snapshot.status,
            MemoryJobStatus::Running | MemoryJobStatus::Cancelling
        ) {
            return Ok(false);
        }
        snapshot.status = MemoryJobStatus::Cancelling;
    }
    control.halt.cancel();
    control.publish()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_receipts_survive_new_jobs_and_prevent_delayed_reexecution() {
        let root = std::env::temp_dir().join(format!(
            "kz-memory-request-{}-{}",
            std::process::id(),
            super::super::now_ms()
        ));
        let store = MemoryStore::project(&root);
        store
            .append_note("original draft", "preserve on stop", "fact", &[])
            .unwrap();
        let first = JobLease::begin(&root, &store, Some("request-a"))
            .unwrap()
            .unwrap();
        let first_id = first.control.snapshot.lock().unwrap().id.clone();
        first.control.snapshot.lock().unwrap().status = MemoryJobStatus::Cancelled;
        drop(first);
        let second = JobLease::begin(&root, &store, Some("request-b"))
            .unwrap()
            .unwrap();
        second.control.snapshot.lock().unwrap().status = MemoryJobStatus::Completed;
        drop(second);
        assert!(JobLease::begin(&root, &store, Some("request-a"))
            .unwrap()
            .is_none());
        let replay = request_snapshot(&root, "request-a").unwrap().unwrap();
        assert_eq!(replay.id, first_id);
        assert_eq!(replay.status, MemoryJobStatus::Cancelled);
        assert_eq!(store.pending_notes(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_crashed_owner_is_recoverable_without_losing_the_queue() {
        const ENV: &str = "KANZEI_INTERNAL_MEMORY_JOB_CRASH_FIXTURE_ROOT";
        if let Some(root) = std::env::var_os(ENV) {
            let root = PathBuf::from(root);
            assert!(std::fs::canonicalize(&root)
                .unwrap()
                .starts_with(std::fs::canonicalize(std::env::temp_dir()).unwrap()));
            assert_eq!(
                std::fs::read(root.join("fixture-owned")).unwrap(),
                b"kanzei-memory-job-crash"
            );
            let store = MemoryStore::project(&root);
            let _job = JobLease::begin(&root, &store, None).unwrap().unwrap();
            std::process::exit(77);
        }
        let root = std::env::temp_dir().join(format!(
            "kz-memory-job-crash-{}-{}",
            std::process::id(),
            super::super::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("fixture-owned"), b"kanzei-memory-job-crash").unwrap();
        let store = MemoryStore::project(&root);
        store
            .append_note(
                "crash-preserved-note",
                "preserve this pending source",
                "fact",
                &[],
            )
            .unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "memory_consolidation::jobs::tests::a_crashed_owner_is_recoverable_without_losing_the_queue", "--nocapture"]).env(ENV, &root);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        assert_eq!(command.output().unwrap().status.code(), Some(77));
        assert_eq!(
            store
                .read_inbox_checkpoint_checked()
                .unwrap()
                .unwrap()
                .job
                .unwrap()
                .status,
            MemoryJobStatus::Running
        );
        assert_eq!(
            snapshot(&root).unwrap().unwrap().status,
            MemoryJobStatus::Interrupted
        );
        assert_eq!(store.pending_notes(), 1);
        let resumed = JobLease::begin(&root, &store, None).unwrap().unwrap();
        assert_eq!(store.pending_notes(), 1);
        drop(resumed);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ownership_is_cross_handle_and_can_be_released_after_an_await() {
        let root = std::env::temp_dir().join(format!(
            "kz-memory-job-{}-{}",
            std::process::id(),
            super::super::now_ms()
        ));
        let store = MemoryStore::project(&root);
        let first = JobLease::begin(&root, &store, None).unwrap().unwrap();
        let id = snapshot(&root).unwrap().unwrap().id;
        assert!(JobLease::begin(&root, &store, None).unwrap().is_none());
        assert!(cancel(&root, "stale-job").is_err());
        assert!(!first.control.halt.is_cancelled());
        assert!(cancel(&root, &id).unwrap());
        assert!(first.control.halt.is_cancelled());
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            drop(first);
        })
        .await
        .unwrap();
        assert_eq!(
            snapshot(&root).unwrap().unwrap().status,
            MemoryJobStatus::Interrupted
        );
        let next = JobLease::begin(&root, &store, None).unwrap().unwrap();
        assert_ne!(next.control.snapshot.lock().unwrap().id, id);
        drop(next);
        std::fs::remove_dir_all(root).unwrap();
    }
}
