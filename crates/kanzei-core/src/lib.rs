//! kanzei-core: session 运行时。
//! M0:内存态一次性 runner;M2 起换成 SQLite 事件溯源 + steer/queue 调度。

pub mod assemble;
pub mod history;
pub mod notification;
pub mod runner;
pub mod store;

pub use assemble::build_route;
pub use history::filter_message_history;
pub use notification::{
    AgentMessage, AgentNotification, InMemoryBroker, NotificationSubscription, PublishMessage,
};
pub use runner::{
    completed_entry, run_once, run_once_with_parts, summarize_failures, summarize_metrics,
    summarize_tools, AskFuture, AskReply, AskRequest, AskResponse, CompletedEntry, FailureSignal,
    RunEvent, RunMetrics, RunSummary, RunnerConfig, SubagentRuntime,
};
pub use store::{
    project_session_id, project_state_path, AdmittedInput, Delivery, EpisodeRecord, RecallEvent,
    Session, SessionStore, StoreError, StoredEvent,
};
