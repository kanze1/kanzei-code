//! Actor-lived delivery, deliberately independent of a single model turn.
use std::{fmt, sync::Arc};

#[derive(Debug, Clone)]
pub struct AsyncNotice {
    /// Stable within the owning actor, so a retried callback is admitted once.
    pub id: String,
    pub text: String,
}

/// The runner drains steering only between complete tool/result batches.
#[derive(Clone)]
pub struct InputInbox(Arc<dyn Fn() -> Result<Vec<AsyncNotice>, String> + Send + Sync>);
impl fmt::Debug for InputInbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InputInbox")
    }
}
impl InputInbox {
    pub fn new(
        read: impl Fn() -> Result<Vec<AsyncNotice>, String> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(read))
    }
    pub fn take(&self) -> Result<Vec<AsyncNotice>, String> {
        (self.0)()
    }
}

#[derive(Clone)]
pub struct AsyncMailbox {
    deliver: Arc<dyn Fn(AsyncNotice) -> Result<(), String> + Send + Sync>,
    closed: tokio::sync::watch::Sender<bool>,
    background_requests: tokio::sync::watch::Sender<u64>,
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
        Self {
            deliver: Arc::new(deliver),
            closed,
            background_requests,
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
    /// An admitted steering input releases an eligible foreground shell without
    /// cancelling it. Subscribe before starting the command: old requests must
    /// never background a command started for a later input.
    pub fn background_requests(&self) -> tokio::sync::watch::Receiver<u64> {
        self.background_requests.subscribe()
    }
    pub fn request_background(&self) {
        self.background_requests
            .send_modify(|sequence| *sequence += 1);
    }
    pub async fn cancelled(&self) {
        let mut rx = self.closed.subscribe();
        let _ = rx.wait_for(|closed| *closed).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
