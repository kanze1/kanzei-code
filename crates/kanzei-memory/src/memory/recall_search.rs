//! Bounded blocking search owned by the process, shared across recall policies.
//!
//! Filesystem/SQLite calls cannot be cancelled by checking a clock before they
//! start. Keep them off the caller, allow only one running and one queued search,
//! and give every request its own project, deadline and reply channel. A slow
//! filesystem cannot accumulate workers across runs. Late results never reach
//! another request, and expired queued requests do no filesystem work.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use super::{IndexHit, IndexQuery, SqliteMemoryIndex};

type SearchResult = anyhow::Result<Vec<IndexHit>>;

#[derive(Debug)]
struct Request {
    project_root: PathBuf,
    query: String,
    deadline: Instant,
    reply: SyncSender<SearchResult>,
}

pub(super) struct RecallSearch {
    sender: SyncSender<Request>,
    _owner: std::thread::JoinHandle<()>,
}

impl RecallSearch {
    pub(super) fn shared() -> Result<Arc<Self>, String> {
        static SEARCH: OnceLock<Result<Arc<RecallSearch>, String>> = OnceLock::new();
        SEARCH
            .get_or_init(|| {
                Self::start()
                    .map(Arc::new)
                    .map_err(|error| error.to_string())
            })
            .clone()
    }

    pub(super) fn start() -> std::io::Result<Self> {
        Self::spawn(|root, query| {
            SqliteMemoryIndex::new(root).probe_lexical(&IndexQuery::text(query), 3)
        })
    }

    pub(super) fn spawn(
        mut search: impl FnMut(&Path, &str) -> SearchResult + Send + 'static,
    ) -> std::io::Result<Self> {
        // A busy search cannot create an unbounded backlog or new worker threads.
        let (sender, receiver): (_, Receiver<Request>) = mpsc::sync_channel(1);
        let owner = std::thread::Builder::new()
            .name("kanzei-failure-recall".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    if Instant::now() >= request.deadline {
                        continue;
                    }
                    let result = search(&request.project_root, &request.query);
                    if Instant::now() < request.deadline {
                        // One response fits without waiting, even if the caller
                        // timed out or dropped its receiver in the meantime.
                        let _ = request.reply.send(result);
                    }
                }
            })?;
        Ok(Self {
            sender,
            _owner: owner,
        })
    }

    pub(super) fn search(
        &self,
        project_root: &Path,
        query: String,
        deadline: Instant,
    ) -> SearchResult {
        anyhow::ensure!(
            Instant::now() < deadline,
            "failure recall deadline exceeded"
        );
        let (reply, receive) = mpsc::sync_channel(1);
        self.sender
            .try_send(Request {
                project_root: project_root.to_path_buf(),
                query,
                deadline,
                reply,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => anyhow::anyhow!("failure recall search is busy"),
                mpsc::TrySendError::Disconnected(_) => {
                    anyhow::anyhow!("failure recall search owner stopped")
                }
            })?;
        let result = receive.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        anyhow::ensure!(
            Instant::now() < deadline,
            "failure recall deadline exceeded"
        );
        result
            .map_err(|_| anyhow::anyhow!("failure recall search owner stopped without a result"))?
    }

    #[cfg(test)]
    pub(super) fn finish(self) {
        drop(self.sender);
        self._owner.join().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn policies_share_one_process_search_owner() {
        let first = RecallSearch::shared().unwrap();
        let second = RecallSearch::shared().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn deadline_skips_queued_work_and_late_results_cannot_cross_requests() {
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let worker = RecallSearch::spawn(move |root, query| {
            observed
                .lock()
                .unwrap()
                .push((root.to_path_buf(), query.to_string()));
            if query == "blocked" {
                entered.send(()).unwrap();
                resume.recv().unwrap();
            }
            Ok(vec![IndexHit {
                id: query.into(),
                category: "fact".into(),
                action: "result".into(),
                status: "active".into(),
                score: 1.0,
            }])
        })
        .unwrap();
        let (first_reply, first_result) = mpsc::sync_channel(1);
        worker
            .sender
            .try_send(Request {
                project_root: "first-project".into(),
                query: "blocked".into(),
                deadline: Instant::now() + Duration::from_secs(5),
                reply: first_reply,
            })
            .unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();

        let error = worker
            .search(
                Path::new("expired-project"),
                "expired queued search".into(),
                Instant::now() + Duration::from_millis(10),
            )
            .unwrap_err();
        assert!(error.to_string().contains("deadline"));
        let busy = worker
            .search(
                Path::new("busy-project"),
                "must not accumulate".into(),
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap_err();
        assert!(busy.to_string().contains("busy"));
        // Cancellation closes only this request's receiver, never another reply.
        drop(first_result);
        release.send(()).unwrap();
        let (fresh_reply, fresh_result) = mpsc::sync_channel(1);
        worker
            .sender
            .send(Request {
                project_root: "fresh-project".into(),
                query: "fresh".into(),
                deadline: Instant::now() + Duration::from_secs(5),
                reply: fresh_reply,
            })
            .unwrap();
        let fresh = fresh_result
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(fresh[0].id, "fresh");
        worker.finish();
        assert_eq!(
            *calls.lock().unwrap(),
            [
                (PathBuf::from("first-project"), "blocked".into()),
                (PathBuf::from("fresh-project"), "fresh".into())
            ]
        );
    }

    #[test]
    fn errors_remain_errors_and_successful_search_keeps_its_result() {
        let worker = RecallSearch::spawn(|_, query| {
            anyhow::ensure!(query != "broken", "corrupt index");
            Ok(Vec::new())
        })
        .unwrap();
        assert!(worker
            .search(
                Path::new("project"),
                "broken".into(),
                Instant::now() + Duration::from_secs(5)
            )
            .unwrap_err()
            .to_string()
            .contains("corrupt index"));
        assert!(worker
            .search(
                Path::new("project"),
                "empty".into(),
                Instant::now() + Duration::from_secs(5)
            )
            .unwrap()
            .is_empty());
        worker.finish();
    }
}
