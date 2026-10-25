//! Stop requests and actual owner completion share one resource lifecycle.
use super::{cleanup_failed, AgentTeam};
use anyhow::{bail, Result};

impl AgentTeam {
    pub fn is_quiescent(&self) -> bool {
        let active = self.0.active.lock().unwrap();
        let mut owners = self.0.worker_owners.lock().unwrap();
        owners.retain(|owner| !owner.is_released());
        owners.is_empty() && active.is_empty()
    }
    pub async fn settled(&self) {
        loop {
            let owners = self.0.worker_owners.lock().unwrap().clone();
            for owner in owners {
                owner.released().await;
            }
            if self.is_quiescent() {
                return;
            }
        }
    }
    /// Signal existing workers while their command producers are draining.
    /// This does not publish resource completion or persist terminal outcomes.
    pub fn cancel_workers(&self) {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        for mailbox in self.0.child_mailboxes.lock().unwrap().values() {
            mailbox.close();
        }
        for worker in self.0.active.lock().unwrap().values() {
            worker.cancel();
        }
    }
    /// Every runnable/waiting child belongs to this stop, including children
    /// suspended without a live worker. Failures remain part of the receipt.
    fn request_stop_all(&self) -> Result<()> {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        for mailbox in self.0.child_mailboxes.lock().unwrap().values() {
            mailbox.close();
        }
        let mut ids: Vec<_> = self.0.active.lock().unwrap().keys().cloned().collect();
        let mut errors = Vec::new();
        match self.list() {
            Ok(jobs) => ids.extend(
                jobs.into_iter()
                    .filter(|j| j.active() || cleanup_failed(j))
                    .map(|j| j.id),
            ),
            Err(error) => errors.push(error.to_string()),
        }
        ids.sort();
        ids.dedup();
        for id in ids {
            if let Err(error) = self.stop_locked(&id) {
                errors.push(format!("{id}: {error}"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            bail!("子任务停止未完成：{}", errors.join("; "))
        }
    }
    pub async fn stop_and_settle(&self) -> Result<()> {
        let requested = self.request_stop_all();
        self.settled().await;
        requested?;
        for job in self.list()? {
            if job.active() || cleanup_failed(&job) {
                bail!("子任务 {} 未结清：{}", job.id, job.latest);
            }
        }
        Ok(())
    }
    pub fn stop_all(&self) {
        if let Err(error) = self.request_stop_all() {
            tracing::error!(%error,"child stop state could not be saved");
        }
    }
}
