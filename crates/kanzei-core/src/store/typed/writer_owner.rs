//! A bounded, dedicated blocking owner for a turn's durable writer.
use super::{SessionFactError, SessionTurnTerminal, TypedWriterState};
use crate::runner::WorkContextReport;
use kanzei_llm::Message;
use std::path::Path;
use std::sync::{mpsc, Arc};

type Operation = Box<dyn FnOnce(&mut TypedWriterState) + Send>;

/// The callback contract requires a durable receipt before the runner proceeds.
/// One queued operation is sufficient: callers serialize a turn, and every
/// operation acknowledges completion rather than accumulating an unbounded log.
pub struct TypedSessionWriter {
    sender: Option<mpsc::SyncSender<Operation>>,
    owner: Option<std::thread::JoinHandle<()>>,
    errors: Vec<String>,
    terminal: bool,
}

fn wait_for_owner<R>(wait: impl FnOnce() -> R) -> R {
    if tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        // Hand the executor worker back to Tokio while a synchronous callback
        // waits for disk. Plain synchronous/current-thread callers need no
        // executor handoff; the owner itself never depends on that runtime.
        tokio::task::block_in_place(wait)
    } else {
        wait()
    }
}

impl TypedSessionWriter {
    pub fn new(state_path: &Path, session_id: &str, turn_id: &str) -> Self {
        let state_path = state_path.to_path_buf();
        let session_id = session_id.to_owned();
        let turn_id = turn_id.to_owned();
        let (sender, receiver) = mpsc::sync_channel::<Operation>(1);
        let owner = std::thread::Builder::new()
            .name("kanzei-session-writer".into())
            .spawn(move || {
                let mut state = TypedWriterState::new(&state_path, &session_id, &turn_id);
                while let Ok(operation) = receiver.recv() {
                    operation(&mut state);
                }
                // The SQLite connection and execution lease drop on this thread.
            })
            .expect("cannot start durable session writer owner");
        Self {
            sender: Some(sender),
            owner: Some(owner),
            errors: Vec::new(),
            terminal: false,
        }
    }

    fn execute<R: Send + 'static>(
        &mut self,
        operation: impl FnOnce(&mut TypedWriterState) -> R + Send + 'static,
    ) -> R {
        let (sender, receiver) = mpsc::sync_channel(1);
        let request: Operation = Box::new(move |state| {
            let value = operation(state);
            // A receipt includes the state after this operation. A database
            // rejection cannot be mistaken for successful queue admission.
            let _ = sender.send((value, state.errors().to_vec(), state.is_terminal()));
        });
        let (value, errors, terminal) = wait_for_owner(|| {
            self.sender
                .as_ref()
                .expect("writer owner is closed")
                .send(request)
                .expect("durable session writer owner stopped unexpectedly");
            receiver
                .recv()
                .expect("durable session writer did not return a receipt")
        });
        self.errors = errors;
        self.terminal = terminal;
        value
    }

    pub(in crate::store) fn with_execution_owner(
        mut self,
        owner: Arc<super::super::session_execution::SessionExecutionGuard>,
    ) -> Self {
        self.execute(move |state| state._execution_owner = Some(owner));
        self
    }

    pub fn user_message(&mut self, input_id: &str, message: Message) -> bool {
        let input_id = input_id.to_owned();
        self.execute(move |state| state.user_message(&input_id, message))
    }
    pub fn steering_message(&mut self, input_id: &str, message: Message) -> bool {
        let input_id = input_id.to_owned();
        self.execute(move |state| state.steering_message(&input_id, message))
    }
    pub fn take_pending_steers(
        &mut self,
    ) -> Result<Vec<super::super::AdmittedInput>, SessionFactError> {
        self.execute(TypedWriterState::take_pending_steers)
    }
    pub fn turn_started(&mut self, source_step: u32, max_steps: u32) {
        self.execute(move |state| state.turn_started(source_step, max_steps));
    }
    pub fn push_text(&mut self, text: &str) {
        let text = text.to_owned();
        self.execute(move |state| state.push_text(&text));
    }
    pub fn flush_due(&mut self) {
        self.execute(TypedWriterState::flush_due);
    }
    pub fn stream_restarted(&mut self) {
        self.execute(TypedWriterState::stream_restarted);
    }
    pub fn assistant_committed(&mut self, source_step: u32, message: Message) -> bool {
        self.execute(move |state| state.assistant_committed(source_step, message))
    }
    pub fn tool_results_committed(&mut self, source_step: u32, message: Message) -> bool {
        self.execute(move |state| state.tool_results_committed(source_step, message))
    }
    pub fn finish(&mut self, terminal: SessionTurnTerminal) {
        self.execute(move |state| state.finish(terminal));
    }
    pub fn finish_with_input_outcome(
        &mut self,
        input_id: &str,
        terminal: SessionTurnTerminal,
        payload: &serde_json::Value,
    ) -> bool {
        let input_id = input_id.to_owned();
        let payload = payload.clone();
        self.execute(move |state| state.finish_with_input_outcome(&input_id, terminal, &payload))
    }
    pub fn write_shadow_report(&mut self, legacy: &[Message]) {
        let legacy = legacy.to_vec();
        self.execute(move |state| state.write_shadow_report(&legacy));
    }
    pub fn commit_work_context_surface(
        &mut self,
        report: &WorkContextReport,
        source: &[Message],
        surface: &[Message],
    ) -> bool {
        let report = report.clone();
        let source = source.to_vec();
        let surface = surface.to_vec();
        self.execute(move |state| state.commit_work_context_surface(&report, &source, &surface))
    }
    pub fn record_error(&mut self, error: impl std::fmt::Display) {
        let error = error.to_string();
        self.execute(move |state| state.record_error(error));
    }
    pub fn errors(&self) -> &[String] {
        &self.errors
    }
    pub fn is_terminal(&self) -> bool {
        self.terminal
    }

    #[cfg(test)]
    pub(super) fn inspect<R: Send + 'static>(
        &mut self,
        inspect: impl FnOnce(&mut TypedWriterState) -> R + Send + 'static,
    ) -> R {
        self.execute(inspect)
    }
}

impl Drop for TypedSessionWriter {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(owner) = self.owner.take() {
            // Dropping a turn is itself an owner boundary, never a detached
            // background close that can outlive the session lease.
            let settled = wait_for_owner(|| owner.join());
            if settled.is_err() && !std::thread::panicking() {
                panic!("durable session writer owner panicked");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn waiting_for_durable_receipt_releases_the_only_executor_worker() {
        // The owner is intentionally held before producing a receipt. This
        // models disk contention without a sleep or a timing-sensitive database.
        let mut writer = TypedSessionWriter::new(Path::new("unused.db"), "session", "turn");
        let (entered, waiting) = tokio::sync::oneshot::channel();
        let (release, released) = mpsc::channel();
        let fallback_release = release.clone();
        let caller = tokio::spawn(async move {
            let caller_thread = std::thread::current().id();
            writer.inspect(move |_| {
                assert_ne!(std::thread::current().id(), caller_thread);
                entered.send(()).unwrap();
                released.recv().unwrap();
            });
            writer
        });
        waiting.await.unwrap();
        let independent = tokio::spawn(async move {
            tokio::task::yield_now().await;
            release.send(()).unwrap();
        });
        let progressed = tokio::time::timeout(std::time::Duration::from_secs(5), independent).await;
        if progressed.is_err() {
            // Permit cleanup even if the regression starves the executor.
            let _ = fallback_release.send(());
        }
        assert!(
            progressed.is_ok(),
            "a durable receipt starved unrelated runtime work"
        );
        drop(caller.await.unwrap());
    }
}
