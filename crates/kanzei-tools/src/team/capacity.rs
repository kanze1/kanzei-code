//! Adjustable capacity. Lowering a limit never cancels an existing worker.
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub(super) struct Capacity {
    state: Mutex<(usize, usize)>, // limit, in use
    changed: Notify,
}
impl Capacity {
    pub(super) fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((limit.max(1), 0)),
            changed: Notify::new(),
        })
    }
    pub(super) fn set_limit(&self, limit: usize) {
        self.state.lock().unwrap().0 = limit.max(1);
        self.changed.notify_waiters();
    }
    pub(super) async fn acquire(self: &Arc<Self>) -> Permit {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.state.lock().unwrap();
                if state.1 < state.0 {
                    state.1 += 1;
                    return Permit(self.clone());
                }
            }
            notified.await;
        }
    }
}
pub(super) struct Permit(Arc<Capacity>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().1 -= 1;
        self.0.changed.notify_waiters();
    }
}
pub(super) async fn acquire_pair(
    local: &Arc<Capacity>,
    global: &Arc<Capacity>,
) -> (Permit, Permit) {
    let local = local.acquire().await;
    let global = global.acquire().await;
    (local, global)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[tokio::test]
    async fn resize_and_global_limit_apply_without_cancelling_workers() {
        let global = Capacity::new(1);
        let a = Capacity::new(2);
        let b = Capacity::new(8);
        let permit = acquire_pair(&a, &global).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), acquire_pair(&b, &global))
                .await
                .is_err()
        );
        global.set_limit(2);
        let other = acquire_pair(&b, &global).await;
        global.set_limit(1);
        drop(permit);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), acquire_pair(&a, &global))
                .await
                .is_err()
        );
        drop(other);
        let _permit = acquire_pair(&a, &global).await;
    }
    #[tokio::test]
    async fn releasing_or_cancelling_waits_restores_local_capacity() {
        let local = Capacity::new(1);
        let global = Capacity::new(1);
        let permit = global.acquire().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), acquire_pair(&local, &global))
                .await
                .is_err()
        );
        drop(permit);
        let _pair = acquire_pair(&local, &global).await;
    }
}
