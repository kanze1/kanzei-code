//! Conversation lifecycle decisions. Hosts own effects; this model owns legal transitions.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Acquiring,
    Active,
    Cancelling,
    Reaping,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Existence {
    Open,
    Closing,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Run,
    Compact,
    History,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopOperation {
    pub operation_id: String,
    pub input_cutoff: i64,
    pub cancelled_inputs: usize,
    pub settled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub actor_epoch: String,
    pub revision: u64,
    pub phase: Phase,
    pub existence: Existence,
    pub activity: Option<Activity>,
    pub round_generation: u64,
    pub last_outcome: Option<String>,
    pub stop: Option<StopOperation>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum Event {
    Acquire(Activity),
    Acquired,
    NextRound,
    WorkerEnded,
    StopRequested(StopOperation),
    StopSettled,
    ResumeCleanup,
    ReaperAttached,
    Settled(String),
    Failed(String),
    CloseRequested,
    Closed,
}

#[derive(Debug, thiserror::Error)]
#[error("illegal conversation transition: {phase:?} + {event}")]
pub struct TransitionError {
    pub phase: Phase,
    pub event: &'static str,
}

pub struct ConversationSupervisor {
    snapshot: Snapshot,
}

impl Default for ConversationSupervisor {
    fn default() -> Self {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        Self {
            snapshot: Snapshot {
                actor_epoch: format!(
                    "{}-{stamp}-{}",
                    std::process::id(),
                    SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ),
                revision: 0,
                phase: Phase::Idle,
                existence: Existence::Open,
                activity: None,
                round_generation: 0,
                last_outcome: None,
                stop: None,
                error: None,
            },
        }
    }
}

impl ConversationSupervisor {
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }
    pub fn busy(&self) -> bool {
        self.snapshot.phase != Phase::Idle
    }
    pub fn stopping(&self) -> bool {
        self.snapshot
            .stop
            .as_ref()
            .is_some_and(|stop| !stop.settled)
    }
    pub fn apply(&mut self, event: Event) -> Result<Snapshot, TransitionError> {
        let s = &mut self.snapshot;
        let invalid = |event| TransitionError {
            phase: s.phase,
            event,
        };
        if s.existence == Existence::Closed {
            return if matches!(event, Event::CloseRequested | Event::Closed) {
                Ok(s.clone())
            } else {
                Err(invalid("actor_closed"))
            };
        }
        match event {
            Event::Acquire(activity) => {
                if s.existence != Existence::Open || s.phase != Phase::Idle {
                    return Err(invalid("acquire"));
                }
                s.phase = Phase::Acquiring;
                s.activity = Some(activity);
                s.error = None;
                s.last_outcome = None;
                s.stop = None;
            }
            Event::Acquired => {
                if s.phase != Phase::Acquiring {
                    return Err(invalid("acquired"));
                }
                s.phase = Phase::Active;
                s.round_generation += 1;
            }
            Event::NextRound => {
                if s.phase != Phase::Reaping || self_stopping(s) || s.existence != Existence::Open {
                    return Err(invalid("next_round"));
                }
                s.phase = Phase::Active;
                s.round_generation += 1;
            }
            Event::WorkerEnded => {
                if !matches!(s.phase, Phase::Active | Phase::Cancelling | Phase::Reaping) {
                    return Err(invalid("worker_ended"));
                }
                s.phase = Phase::Reaping;
            }
            Event::StopRequested(stop) => {
                if s.existence == Existence::Closed {
                    return Err(invalid("stop"));
                }
                if self_stopping(s) {
                    return Ok(s.clone());
                }
                s.phase = if matches!(s.phase, Phase::Active | Phase::Acquiring) {
                    Phase::Cancelling
                } else {
                    Phase::Reaping
                };
                s.stop = Some(stop);
            }
            Event::Settled(outcome) => {
                if s.phase != Phase::Reaping || self_stopping(s) {
                    return Err(invalid("settled"));
                }
                s.phase = Phase::Idle;
                s.activity = None;
                s.last_outcome = Some(outcome);
                s.error = None;
                if let Some(stop) = s.stop.as_mut() {
                    stop.settled = true;
                }
            }
            Event::ReaperAttached => {
                if !self_stopping(s) || !matches!(s.phase, Phase::Cancelling | Phase::Reaping) {
                    return Err(invalid("reaper_attached"));
                }
                s.activity = Some(Activity::Cleanup);
                s.error = None;
            }
            Event::ResumeCleanup => {
                if s.phase != Phase::Blocked || !self_stopping(s) {
                    return Err(invalid("resume_cleanup"));
                }
                s.phase = Phase::Reaping;
            }
            Event::StopSettled => {
                if s.phase != Phase::Reaping || !self_stopping(s) {
                    return Err(invalid("stop_settled"));
                }
                if let Some(stop) = s.stop.as_mut() {
                    stop.settled = true;
                }
            }
            Event::Failed(error) => {
                s.phase = Phase::Blocked;
                s.error = Some(error);
            }
            Event::CloseRequested => {
                if s.existence == Existence::Closed {
                    return Err(invalid("close"));
                }
                s.existence = Existence::Closing;
            }
            Event::Closed => {
                if s.existence != Existence::Closing || s.phase != Phase::Idle {
                    return Err(invalid("closed"));
                }
                s.existence = Existence::Closed;
            }
        }
        s.revision += 1;
        Ok(s.clone())
    }
}
fn self_stopping(s: &Snapshot) -> bool {
    s.stop.as_ref().is_some_and(|stop| !stop.settled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_cannot_publish_idle_before_join_and_reaping() {
        let mut s = ConversationSupervisor::default();
        assert!(s.apply(Event::Acquired).is_err());
        s.apply(Event::Acquire(Activity::Run)).unwrap();
        s.apply(Event::Acquired).unwrap();
        let stop = StopOperation {
            operation_id: "stop".into(),
            input_cutoff: 7,
            cancelled_inputs: 2,
            settled: false,
        };
        s.apply(Event::StopRequested(stop.clone())).unwrap();
        assert!(s.apply(Event::Settled("stopped".into())).is_err());
        let revision = s.snapshot().revision;
        s.apply(Event::StopRequested(StopOperation {
            input_cutoff: 99,
            ..stop
        }))
        .unwrap();
        assert_eq!(s.snapshot().stop.unwrap().input_cutoff, 7);
        assert_eq!(s.snapshot().revision, revision);
        s.apply(Event::WorkerEnded).unwrap();
        assert!(s.apply(Event::Settled("stopped".into())).is_err());
        s.apply(Event::StopSettled).unwrap();
        s.apply(Event::Settled("stopped".into())).unwrap();
        assert!(!s.busy());
        assert!(s.snapshot().stop.unwrap().settled);
    }
    #[test]
    fn closure_blocks_admission_and_epochs_are_not_reused() {
        let mut s = ConversationSupervisor::default();
        assert_ne!(
            s.snapshot().actor_epoch,
            ConversationSupervisor::default().snapshot().actor_epoch
        );
        s.apply(Event::CloseRequested).unwrap();
        assert!(s.apply(Event::Acquire(Activity::Run)).is_err());
        s.apply(Event::Closed).unwrap();
    }
}

#[cfg(test)]
mod race_tests {
    use super::*;
    fn stop(cutoff: i64) -> Event {
        Event::StopRequested(StopOperation {
            operation_id: "stop".into(),
            input_cutoff: cutoff,
            cancelled_inputs: 0,
            settled: false,
        })
    }
    #[test]
    fn cleanup_failure_blocks_admission_until_explicit_stop_recovery() {
        let mut s = ConversationSupervisor::default();
        s.apply(stop(3)).unwrap();
        s.apply(Event::Failed("resource still owned".into()))
            .unwrap();
        assert!(s.busy());
        assert!(s.apply(Event::Acquire(Activity::Run)).is_err());
        s.apply(Event::ResumeCleanup).unwrap();
        s.apply(Event::ReaperAttached).unwrap();
        s.apply(Event::StopSettled).unwrap();
        s.apply(Event::Settled("stopped".into())).unwrap();
        assert!(s.snapshot().error.is_none());
        s.apply(Event::Acquire(Activity::Run)).unwrap();
    }
    #[test]
    fn joined_parent_drains_new_input_only_after_stop_resources_settle() {
        let mut s = ConversationSupervisor::default();
        s.apply(Event::Acquire(Activity::Run)).unwrap();
        s.apply(Event::Acquired).unwrap();
        let generation = s.snapshot().round_generation;
        s.apply(stop(3)).unwrap();
        s.apply(Event::WorkerEnded).unwrap();
        assert!(s.apply(Event::NextRound).is_err());
        s.apply(Event::StopSettled).unwrap();
        s.apply(Event::NextRound).unwrap();
        assert_eq!(s.snapshot().round_generation, generation + 1);
        assert_eq!(s.snapshot().phase, Phase::Active);
    }
}
