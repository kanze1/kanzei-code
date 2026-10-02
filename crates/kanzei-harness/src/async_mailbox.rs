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
        Self {
            deliver: Arc::new(deliver),
            closed,
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
    pub async fn cancelled(&self) {
        let mut rx = self.closed.subscribe();
        let _ = rx.wait_for(|closed| *closed).await;
    }
}
