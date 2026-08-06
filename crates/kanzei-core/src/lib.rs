//! kanzei-core: session 运行时。
//! M0:内存态一次性 runner;M2 起换成 SQLite 事件溯源 + steer/queue 调度。

pub mod assemble;
pub mod runner;
pub mod store;

pub use assemble::build_route;
pub use runner::{run_once, AskFuture, AskReply, RunEvent, RunSummary, RunnerConfig, SubagentRuntime};
pub use store::{
    project_session_id, project_state_path, AdmittedInput, Delivery, Session, SessionStore,
    StoreError, StoredEvent,
};
