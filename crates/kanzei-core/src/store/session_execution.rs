//! One execution owner per canonical database/session, across desktop and CLI.
//! The existing thread-affine FileLock stays on a dedicated worker until every
//! execution/writer guard is dropped; process death releases the OS lock.

mod completion;
pub use completion::ExecutionCompletion;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use kanzei_base::atomic_file::{self, FileLock};
use sha2::{Digest, Sha256};

use super::{SessionFactError, SessionStore, StoreError, TypedSessionWriter};

fn target(state_path: &Path, session_id: &str) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(state_path)?;
    let canonical = match absolute.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = absolute.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "数据库路径没有父目录")
            })?;
            std::fs::create_dir_all(parent)?;
            parent
                .canonicalize()?
                .join(absolute.file_name().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "数据库路径没有文件名")
                })?)
        }
        Err(error) => return Err(error),
    };
    let mut directory = canonical.as_os_str().to_os_string();
    directory.push(".session-owners");
    Ok(PathBuf::from(directory).join(format!("{:x}", Sha256::digest(session_id.as_bytes()))))
}

fn lock(target: &Path) -> io::Result<FileLock> {
    atomic_file::try_lock_exclusive(target, Duration::ZERO)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            "当前对话正在运行，请等待本轮完成后再重试",
        )
    })
}

/// Send owner token. It is shared with the typed writer so a late flush keeps
/// ownership after its main async task is aborted. No SQLite lock is held here.
pub struct SessionExecutionGuard {
    state_path: PathBuf,
    session_id: String,
    target: PathBuf,
    release: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
    writers: Arc<WriterCompletion>,
    completion: ExecutionCompletion,
}

struct WriterCompletion {
    count: std::sync::atomic::AtomicUsize,
    changed: tokio::sync::Notify,
}
pub(super) struct WriterOwner {
    _owner: Arc<SessionExecutionGuard>,
    _completion: WriterReceipt,
}
struct WriterReceipt(Arc<WriterCompletion>);
impl Drop for WriterReceipt {
    fn drop(&mut self) {
        self.0
            .count
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        self.0.changed.notify_waiters();
    }
}

/// Fail immediately when this session is already executing. Waiting here would
/// deadlock a runner that invokes `kz run` against its own primary conversation.
pub fn try_acquire(state_path: &Path, session_id: &str) -> io::Result<SessionExecutionGuard> {
    let target = target(state_path, session_id)?;
    let worker_target = target.clone();
    let (release, released) = mpsc::channel::<()>();
    let (ready, acquired) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("session-execution-owner".into())
        .spawn(move || match lock(&worker_target) {
            Ok(_lock) => {
                if ready.send(Ok(())).is_ok() {
                    let _ = released.recv();
                }
            }
            Err(error) => {
                let _ = ready.send(Err(error));
            }
        })?;
    let outcome = acquired
        .recv()
        .map_err(|error| io::Error::other(format!("session execution worker failed: {error}")));
    match outcome {
        Ok(Ok(())) => Ok(SessionExecutionGuard {
            state_path: state_path.to_path_buf(),
            session_id: session_id.into(),
            target,
            release: Some(release),
            worker: Some(worker),
            completion: ExecutionCompletion::default(),
            writers: Arc::new(WriterCompletion {
                count: std::sync::atomic::AtomicUsize::new(0),
                changed: tokio::sync::Notify::new(),
            }),
        }),
        Ok(Err(error)) | Err(error) => {
            drop(release);
            let _ = worker.join();
            Err(error)
        }
    }
}

impl SessionExecutionGuard {
    pub fn completion(&self) -> ExecutionCompletion {
        self.completion.clone()
    }

    /// Recover only under this exact database/session owner. The caller may
    /// already have promoted/started its current input; only that input survives.
    pub fn prepare(
        &self,
        store: &SessionStore,
        session_id: &str,
        current_input: Option<&str>,
    ) -> Result<(), SessionFactError> {
        self.validate_store(store, session_id)?;
        store.seed_latest_legacy_snapshot(&self.session_id)?;
        store.recover_interrupted_session_facts_owned(
            &self.session_id,
            "process_restarted",
            current_input,
            super::typed::RecoveryTerminal::Failed,
        )?;
        Ok(())
    }

    /// The supervisor has joined the worker and all writers before closing stopped facts.
    pub fn settle_stopped(&self, store: &SessionStore) -> Result<(), SessionFactError> {
        self.validate_store(store, &self.session_id)?;
        store.recover_interrupted_session_facts_owned(
            &self.session_id,
            "user_stopped",
            None,
            super::typed::RecoveryTerminal::Stopped,
        )?;
        Ok(())
    }

    /// Recover a worker that exited without committing its own terminal.
    pub fn settle_failed(
        &self,
        store: &SessionStore,
        reason: &str,
    ) -> Result<(), SessionFactError> {
        self.validate_store(store, &self.session_id)?;
        store.recover_interrupted_session_facts_owned(
            &self.session_id,
            reason,
            None,
            super::typed::RecoveryTerminal::Failed,
        )?;
        Ok(())
    }

    fn validate_store(
        &self,
        store: &SessionStore,
        session_id: &str,
    ) -> Result<(), SessionFactError> {
        let database = store
            .path
            .as_ref()
            .ok_or_else(|| StoreError::InvalidInput("执行守卫不能用于另一份内存数据库".into()))?;
        if session_id != self.session_id
            || target(database, session_id).map_err(StoreError::from)? != self.target
        {
            return Err(StoreError::InvalidInput("执行守卫不属于这份数据库".into()).into());
        }
        Ok(())
    }

    pub fn writer(self: &Arc<Self>, turn_id: &str) -> TypedSessionWriter {
        self.writers
            .count
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        TypedSessionWriter::new(&self.state_path, &self.session_id, turn_id).with_execution_owner(
            WriterOwner {
                _owner: Arc::clone(self),
                _completion: WriterReceipt(Arc::clone(&self.writers)),
            },
        )
    }

    /// Call after the worker joined: no new writer may be registered afterwards.
    pub async fn writers_settled(&self) {
        loop {
            let changed = self.writers.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self
                .writers
                .count
                .load(std::sync::atomic::Ordering::Acquire)
                == 0
            {
                return;
            }
            changed.await;
        }
    }
}

impl Drop for SessionExecutionGuard {
    fn drop(&mut self) {
        // The worker never owns an Arc to this guard or calls back into runtime/
        // SQLite. Signal before joining, and release before the next queue round.
        drop(self.release.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.completion.finish();
    }
}

/// Direct recovery callers must not close a live owner's turn. Memory stores
/// cannot be shared across processes and retain their existing test semantics.
pub(super) fn lock_recovery(
    store: &SessionStore,
    session_id: &str,
) -> Result<Option<FileLock>, StoreError> {
    store
        .path
        .as_ref()
        .map(|path| target(path, session_id).and_then(|target| lock(&target)))
        .transpose()
        .map_err(StoreError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Delivery, SessionFact, SessionTurnTerminal};
    use kanzei_llm::Message;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "kz-session-owner-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn store(&self) -> SessionStore {
            let store = SessionStore::open(&self.0.join("state.db")).unwrap();
            store.create_session("ses", "project", None).unwrap();
            store
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn owned_writer_blocks_alias_and_public_recovery_until_its_last_drop() {
        let fixture = Fixture::new("late-writer");
        let store = fixture.store();
        let path = fixture.0.join("state.db");
        let owner = Arc::new(try_acquire(&path, "ses").unwrap());
        owner.prepare(&store, "ses", None).unwrap();
        store
            .admit_input("ses", "input", "run", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses").unwrap().unwrap();
        assert!(store.start_input("input").unwrap());
        let completed = owner.completion();
        let mut writer = owner.writer("turn");
        writer.user_message("input", Message::user_text("run"));
        writer.turn_started(1, 2);
        drop(owner);
        assert!(!completed.is_released());
        let busy = try_acquire(&fixture.0.join("./state.db"), "ses")
            .err()
            .unwrap();
        assert_eq!(busy.kind(), io::ErrorKind::WouldBlock);
        assert!(crate::prepare_typed_session(&store, "ses").is_err());
        assert!(store
            .recover_interrupted_session_facts("ses", "wrong")
            .is_err());
        assert!(!store
            .list_session_facts("ses")
            .unwrap()
            .iter()
            .any(|(_, fact)| { matches!(fact.fact, SessionFact::TurnFailed { .. }) }));
        // Database and session are both identity components.
        drop(try_acquire(&path, "other-session").unwrap());
        drop(try_acquire(&fixture.0.join("other.db"), "ses").unwrap());
        writer.assistant_committed(
            1,
            Message::assistant(vec![kanzei_llm::Part::Text {
                text: "done".into(),
            }]),
        );
        writer.finish(SessionTurnTerminal::Completed);
        assert!(store.finish_input("input", true).unwrap());
        assert!(writer.errors().is_empty(), "{:?}", writer.errors());
        drop(writer);
        assert!(completed.is_released());
        // Last Drop joins release; immediate reacquisition cannot spuriously be busy.
        drop(try_acquire(&path, "ses").unwrap());
    }

    #[test]
    fn owned_prepare_rejects_another_database_or_session_without_mutation() {
        let fixture = Fixture::new("identity");
        let store = fixture.store();
        store
            .create_session("other-session", "project", None)
            .unwrap();
        let other = SessionStore::open(&fixture.0.join("other.db")).unwrap();
        other.create_session("ses", "project", None).unwrap();
        let owner = try_acquire(&fixture.0.join("state.db"), "ses").unwrap();
        assert!(owner.prepare(&other, "ses", None).is_err());
        assert!(owner.prepare(&store, "other-session", None).is_err());
        assert!(store.list_events("ses", 0).unwrap().is_empty());
        assert!(other.list_events("ses", 0).unwrap().is_empty());
    }

    #[test]
    fn rejected_terminal_keeps_writer_nonterminal_and_owner_until_failure_cleanup() {
        let fixture = Fixture::new("terminal-rejected");
        let store = fixture.store();
        let path = fixture.0.join("state.db");
        let owner = Arc::new(try_acquire(&path, "ses").unwrap());
        owner.prepare(&store, "ses", None).unwrap();
        store
            .admit_input("ses", "input", "run", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("ses").unwrap();
        store.start_input("input").unwrap();
        let mut writer = owner.writer("turn");
        writer.user_message("input", Message::user_text("run"));
        writer.turn_started(1, 2);
        assert!(writer.assistant_committed(
            1,
            Message::assistant(vec![kanzei_llm::Part::Text {
                text: "done".into()
            }])
        ));
        store.connection.execute_batch("CREATE TRIGGER reject_terminal BEFORE INSERT ON session_events
            WHEN NEW.event_type='session.turn_completed' BEGIN SELECT RAISE(ABORT, 'terminal rejected'); END;").unwrap();
        writer.finish(SessionTurnTerminal::Completed);
        assert!(!writer.is_terminal());
        assert!(writer
            .errors()
            .last()
            .unwrap()
            .contains("terminal rejected"));
        drop(owner);
        assert!(try_acquire(&path, "ses").is_err());
        assert!(store.finish_input("input", false).unwrap());
        assert_eq!(
            store.input_status("input").unwrap().as_deref(),
            Some("failed")
        );
        drop(writer);
        store
            .connection
            .execute_batch("DROP TRIGGER reject_terminal")
            .unwrap();
        let next = try_acquire(&path, "ses").unwrap();
        next.prepare(&store, "ses", None).unwrap();
        assert_eq!(
            store
                .list_session_facts("ses")
                .unwrap()
                .iter()
                .filter(|(_, fact)| { matches!(fact.fact, SessionFact::TurnFailed { .. }) })
                .count(),
            1
        );
    }

    #[test]
    fn recovery_atomically_fails_orphaned_claims_preserving_current_and_pending_inputs() {
        let fixture = Fixture::new("claims");
        let store = fixture.store();
        for input in ["orphan-running", "orphan-promoted", "current"] {
            store
                .admit_input("ses", input, input, Delivery::Queue)
                .unwrap();
            store.promote_next_queue("ses").unwrap().unwrap();
        }
        assert!(store.start_input("orphan-running").unwrap());
        assert!(store.start_input("current").unwrap());
        store
            .admit_input("ses", "pending", "queued", Delivery::Queue)
            .unwrap();
        store
            .admit_input("ses", "steer", "insert", Delivery::Steer)
            .unwrap();
        store.create_session("other", "project", None).unwrap();
        store
            .admit_input("other", "other-input", "other", Delivery::Queue)
            .unwrap();
        store.promote_next_queue("other").unwrap();
        store.start_input("other-input").unwrap();
        let mut interrupted =
            TypedSessionWriter::new(&fixture.0.join("state.db"), "ses", "old-turn");
        interrupted.user_message("orphan-running", Message::user_text("old"));
        interrupted.turn_started(1, 2);
        interrupted.push_text(&"x".repeat(2048));
        interrupted.flush_due();
        drop(interrupted);
        let owner = try_acquire(&fixture.0.join("state.db"), "ses").unwrap();
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_orphan_finish BEFORE UPDATE ON session_inputs
             WHEN NEW.status='failed' BEGIN SELECT RAISE(ABORT, 'orphan finish rejected'); END;",
            )
            .unwrap();
        assert!(owner.prepare(&store, "ses", Some("current")).is_err());
        assert_eq!(
            store.input_status("orphan-running").unwrap().as_deref(),
            Some("running")
        );
        assert!(
            !store
                .list_session_facts("ses")
                .unwrap()
                .iter()
                .any(|(_, fact)| { matches!(fact.fact, SessionFact::TurnFailed { .. }) }),
            "input recovery failure must roll back the terminal fact too"
        );
        store
            .connection
            .execute_batch("DROP TRIGGER reject_orphan_finish")
            .unwrap();
        owner.prepare(&store, "ses", Some("current")).unwrap();
        for orphan in ["orphan-running", "orphan-promoted"] {
            assert_eq!(
                store.input_status(orphan).unwrap().as_deref(),
                Some("failed")
            );
        }
        for (input, expected) in [
            ("current", "running"),
            ("pending", "pending"),
            ("steer", "pending"),
            ("other-input", "running"),
        ] {
            assert_eq!(
                store.input_status(input).unwrap().as_deref(),
                Some(expected)
            );
        }
        assert_eq!(
            store
                .list_session_facts("ses")
                .unwrap()
                .iter()
                .filter(|(_, fact)| {
                    fact.turn_id == "old-turn"
                        && matches!(fact.fact, SessionFact::TurnFailed { .. })
                })
                .count(),
            1
        );
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    use std::future::Future;
    #[tokio::test]
    async fn writer_completion_receipt_waits_for_actual_writer_drop() {
        let root = kanzei_base::test_support::TestDir::new("kz-writer-receipt-");
        let path = root.join("state.db");
        let store = SessionStore::open(&path).unwrap();
        store.create_session("s", "project", None).unwrap();
        let owner = Arc::new(try_acquire(&path, "s").unwrap());
        let writer = owner.writer("t");
        let waiting = owner.writers_settled();
        tokio::pin!(waiting);
        let pending = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(waiting.as_mut().poll(cx).is_pending())
        })
        .await;
        assert!(pending);
        drop(writer);
        waiting.await;
        owner.settle_stopped(&store).unwrap();
    }
}
