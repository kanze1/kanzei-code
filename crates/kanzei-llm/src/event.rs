use serde::{Deserialize, Serialize};
use serde_json::Value;

/// token 用量。cache_read/cache_write 单独记账(prompt cache 命中透明化)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// Provider-side web searches reported or counted from hosted-search events.
    #[serde(default)]
    pub web_search_requests: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
    Refusal,
    Other(String),
}

/// 所有协议归一后的流事件(对应 opencode V2 的 LLMEvent)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmEvent {
    StepStart,
    TextStart {
        index: usize,
    },
    TextDelta {
        index: usize,
        text: String,
    },
    TextEnd {
        index: usize,
    },
    ReasoningStart {
        index: usize,
    },
    ReasoningDelta {
        index: usize,
        text: String,
    },
    ReasoningEnd {
        index: usize,
        signature: Option<String>,
    },
    ToolInputStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolInputDelta {
        index: usize,
        delta: String,
    },
    /// 完整工具调用。`input` 为解析结果(失败时为 Null);`raw_input` 保留原文,
    /// 供上层修复回路(宽容解析+schema 纠错反馈)使用——绝不因坏 JSON 直接崩给用户。
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
        raw_input: String,
    },
    /// Provider-side hosted tool announcement; never routed to a local Tool executor.
    HostedStart {
        index: usize,
        id: String,
        name: String,
        channel: String,
        protocol: String,
    },
    /// Full raw server item persisted in assistant history and replayed only to its origin route.
    HostedItem {
        index: usize,
        channel: String,
        protocol: String,
        kind: String,
        raw: Value,
    },
    StepFinish {
        reason: FinishReason,
        usage: Usage,
    },
}
