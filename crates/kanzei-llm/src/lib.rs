//! kanzei-llm: LLM 协议层。
//!
//! 四正交轴(参考 opencode V2):Protocol(说什么 API)× Endpoint(发到哪)
//! × Auth(headers 注入)× Framing(SSE)。所有协议归一为 [`LlmEvent`] 流。

pub mod atomic_file;
pub mod auth;
pub mod client;
pub mod error;
pub mod event;
pub mod protocol;
pub mod proxy;
pub mod request;
pub mod sse;

pub use client::{Endpoint, LlmClient, Route};
pub use error::LlmError;
pub use event::{FinishReason, LlmEvent, Usage};
pub use proxy::ProxyConfig;
pub use request::{LlmRequest, Message, Part, ReasoningEffort, Role, ToolSpec};
