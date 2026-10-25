//! A receipt for the final execution lease release, including late writers.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Default)]
pub struct ExecutionCompletion(Arc<State>);

#[derive(Default)]
struct State {
    released: AtomicBool,
    changed: tokio::sync::Notify,
}

impl ExecutionCompletion {
    pub fn is_released(&self) -> bool {
        self.0.released.load(Ordering::Acquire)
    }

    pub async fn released(&self) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.is_released() {
                return;
            }
            changed.await;
        }
    }

    pub(super) fn finish(&self) {
        self.0.released.store(true, Ordering::Release);
        self.0.changed.notify_waiters();
    }
}
