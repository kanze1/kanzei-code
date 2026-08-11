//! kanzei-core: session 运行时。
//! M0:内存态一次性 runner;M2 起换成 SQLite 事件溯源 + steer/queue 调度。

pub mod assemble;
pub mod history;
pub mod notification;
pub mod orchestration;
pub mod phase;
pub mod replay;
pub mod runner;
pub mod store;

pub use assemble::build_route;
pub use history::filter_message_history;
pub use notification::{
    AgentMessage, AgentNotification, InMemoryBroker, NotificationSubscription, PublishMessage,
};
pub use phase::{PhaseOrchestrator, ScoutTask};
pub use runner::{
    completed_entry, mask_volatile_payload, normalize_fp_marker, run_once, run_once_with_parts,
    run_read_agent, summarize_failures, summarize_metrics, summarize_tools, AskFuture, AskPolicy,
    AskReply, AskRequest, AskResponse, CompletedEntry, FailureSignal, RecallHit, RecallPolicy,
    RecallTrigger, RecallWatch, RunEvent, RunMetrics, RunSummary, RunnerConfig, SubagentRuntime,
    TaskCancellations,
};
pub use store::{
    project_session_id, project_state_path, AdmittedInput, Delivery, EpisodeRecord, FunnelCounts,
    RecallEvent, Session, SessionStore, StoreError, StoredEvent, StoredProcess,
};
