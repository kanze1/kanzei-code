//! Actor-lived delivery, deliberately independent of a single model turn.
use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

#[derive(Debug, Clone)]
pub struct AsyncNotice {
    pub origin: kanzei_base::MessageOrigin,
    pub attachments: Vec<kanzei_base::InputAttachment>,
    /// Stable within the owning actor, so a retried callback is admitted once.
    pub id: String,
    pub text: String,
}

/// The runner drains steering before provider requests, between complete
/// tool/result batches. An uncommitted response can yield to that boundary.
#[derive(Clone)]
pub struct InputInbox {
    actor: Option<String>,
    read: Arc<dyn Fn() -> Result<Vec<AsyncNotice>, String> + Send + Sync>,
}
impl fmt::Debug for InputInbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputInbox")
            .field("actor", &self.actor)
            .finish_non_exhaustive()
    }
}
impl InputInbox {
    pub fn new(
        read: impl Fn() -> Result<Vec<AsyncNotice>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            actor: None,
            read: Arc::new(read),
        }
    }
    /// A child may inherit only an inbox explicitly bound to its own identity.
    pub fn for_actor(
        actor: impl Into<String>,
        read: impl Fn() -> Result<Vec<AsyncNotice>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            actor: Some(actor.into()),
            read: Arc::new(read),
        }
    }
    pub fn belongs_to(&self, actor: &str) -> bool {
        self.actor.as_deref() == Some(actor)
    }
    pub fn take(&self) -> Result<Vec<AsyncNotice>, String> {
        (self.read)()
    }
}

#[derive(Clone)]
pub struct AsyncMailbox {
    deliver: Arc<dyn Fn(AsyncNotice) -> Result<(), String> + Send + Sync>,
    closed: tokio::sync::watch::Sender<bool>,
    background_requests: tokio::sync::watch::Sender<u64>,
    input_requests: tokio::sync::watch::Sender<u64>,
    consumed_inputs: Arc<AtomicU64>,
}

impl fmt::Debug for AsyncMailbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncMailbox")
            .field("closed", &self.is_closed())
            .finish()
    }
}

impl AsyncMailbox {
    pub fn new(
        deliver: impl Fn(AsyncNotice) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        let (closed, _) = tokio::sync::watch::channel(false);
        let (background_requests, _) = tokio::sync::watch::channel(0);
        let (input_requests, _) = tokio::sync::watch::channel(0);
        Self {
            deliver: Arc::new(deliver),
            closed,
            background_requests,
            input_requests,
            consumed_inputs: Arc::new(AtomicU64::new(0)),
        }
    }
    pub fn publish(&self, notice: AsyncNotice) -> Result<(), String> {
        if self.is_closed() {
            return Err("原任务已停止，异步结果不再启动它".into());
        }
        (self.deliver)(notice)
    }
    pub fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }
    pub fn close(&self) {
        self.closed.send_replace(true);
    }
    /// Automatic result delivery can release an eligible foreground shell.
    /// Human input additionally uses InputRequests to cover late subscribers.
    pub fn background_requests(&self) -> tokio::sync::watch::Receiver<u64> {
        self.background_requests.subscribe()
    }
    pub fn request_background(&self) {
        self.background_requests
            .send_modify(|sequence| *sequence += 1);
    }
    /// Human steering can interrupt an unfinished response and release eligible
    /// tools. Automatic completion notices only request tool handoff: they must
    /// not repeatedly discard an answer being written for the user.
    pub fn request_input(&self) {
        self.input_requests.send_modify(|sequence| *sequence += 1);
        self.request_background();
    }
    pub async fn cancelled(&self) {
        let mut rx = self.closed.subscribe();
        let _ = rx.wait_for(|closed| *closed).await;
    }
}

/// A reader observes unconsumed input even when it subscribes after admission.
/// Only the runner's successful inbox transaction acknowledges delivery; a new
/// tool cannot miss input arriving between model completion and tool dispatch.
#[derive(Default)]
pub struct InputRequests(Option<InputReceiver>);

struct InputReceiver {
    receiver: tokio::sync::watch::Receiver<u64>,
    consumed: Arc<AtomicU64>,
    observed: u64,
}

impl InputRequests {
    pub fn new(mailbox: Option<&AsyncMailbox>) -> Self {
        Self(mailbox.map(|mailbox| InputReceiver {
            receiver: mailbox.input_requests.subscribe(),
            consumed: mailbox.consumed_inputs.clone(),
            observed: mailbox.consumed_inputs.load(Ordering::SeqCst),
        }))
    }

    pub fn take(&mut self, inbox: &InputInbox) -> Result<Vec<AsyncNotice>, String> {
        // Capture before reading. Input admitted during the transaction can at
        // worst cause another delivery boundary; it can never be acknowledged
        // without having been read. Failed transactions retain the signal.
        let through = self.0.as_ref().map(|r| *r.receiver.borrow()).unwrap_or(0);
        let inputs = inbox.take()?;
        if let Some(reader) = &mut self.0 {
            reader.consumed.fetch_max(through, Ordering::SeqCst);
            reader.observed = through;
        }
        Ok(inputs)
    }

    pub async fn changed(&mut self) {
        if let Some(reader) = &mut self.0 {
            let consumed = &reader.consumed;
            let observed = reader.observed;
            if let Ok(sequence) = reader
                .receiver
                .wait_for(|sequence| *sequence > observed.max(consumed.load(Ordering::SeqCst)))
                .await
            {
                reader.observed = *sequence;
                return;
            }
        }
        // A disconnected inbox cannot produce input; session cancellation is
        // handled independently by the caller, without spinning on a closed watch.
        std::future::pending().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_inherits_only_its_own_bound_inbox() {
        let parent = InputInbox::new(|| Ok(vec![]));
        let child = InputInbox::for_actor("child-a", || Ok(vec![]));
        assert!(!parent.belongs_to("child-a"));
        assert!(child.belongs_to("child-a"));
        assert!(!child.belongs_to("child-b"));
    }

    fn is_pending(requests: &mut InputRequests) -> bool {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        let mut changed = std::pin::pin!(requests.changed());
        matches!(
            changed
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        )
    }

    #[tokio::test]
    async fn pending_input_reaches_late_subscribers_once_and_acknowledgement_prevents_replay() {
        let mailbox = AsyncMailbox::new(|_| Ok(()));
        mailbox.request_input();
        let mut tool = InputRequests::new(Some(&mailbox));
        assert!(
            !is_pending(&mut tool),
            "input before tool dispatch must release it"
        );
        assert!(
            is_pending(&mut tool),
            "one reader observes each request once"
        );
        let mut runner = InputRequests::new(Some(&mailbox));
        runner.take(&InputInbox::new(|| Ok(vec![]))).unwrap();
        let mut next_tool = InputRequests::new(Some(&mailbox));
        assert!(
            is_pending(&mut next_tool),
            "consumed input cannot release later work"
        );
        mailbox.request_input();
        assert!(!is_pending(&mut next_tool));
        assert!(!is_pending(&mut runner));
    }

    #[tokio::test]
    async fn failed_inbox_transaction_keeps_input_pending_and_automatic_notices_do_not_interrupt() {
        let mailbox = AsyncMailbox::new(|_| Ok(()));
        let mut runner = InputRequests::new(Some(&mailbox));
        mailbox.request_background();
        assert!(is_pending(&mut runner));
        mailbox.request_input();
        assert!(runner
            .take(&InputInbox::new(|| Err("transaction failed".into())))
            .is_err());
        assert!(!is_pending(&mut runner));
        let mut late = InputRequests::new(Some(&mailbox));
        assert!(!is_pending(&mut late));
        runner.take(&InputInbox::new(|| Ok(vec![]))).unwrap();
        assert!(is_pending(&mut late));
        assert!(is_pending(&mut InputRequests::default()));
    }

    #[tokio::test]
    async fn background_requests_wake_current_commands_without_replaying_old_input() {
        let mailbox = AsyncMailbox::new(|_| Ok(()));
        let mut current = mailbox.background_requests();
        mailbox.request_background();
        current.changed().await.unwrap();
        let later = mailbox.background_requests();
        assert!(!later.has_changed().unwrap());
        mailbox.request_background();
        assert!(later.has_changed().unwrap());
        assert!(current.has_changed().unwrap());
    }
}
