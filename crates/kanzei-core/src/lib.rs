//! kanzei-core: session 运行时。
//! M0:内存态一次性 runner;M2 起换成 SQLite 事件溯源 + steer/queue 调度。

pub mod runner;

pub use runner::{run_once, AskFuture, RunEvent, RunSummary, RunnerConfig};
