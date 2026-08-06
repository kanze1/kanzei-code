//! 协议抽象:每种 wire API 提供 body 构造 + 流式状态机(SSE 事件 → LlmEvent)。

pub mod anthropic;
pub mod openai;
pub mod openai_responses;

use crate::error::LlmError;
use crate::event::LlmEvent;
use crate::request::LlmRequest;
use crate::sse::SseEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolKind {
    AnthropicMessages,
    /// OpenAI Chat Completions 兼容(OpenAI/Ollama/LM Studio/DeepSeek/Kimi/...)。
    OpenAiChat,
    /// OpenAI Responses(Codex 订阅后端 / OpenAI 平台 /v1/responses)。
    OpenAiResponses,
}

pub trait ProtocolState: Send {
    fn step(&mut self, event: &SseEvent) -> Result<Vec<LlmEvent>, LlmError>;
}

pub fn build_body(kind: ProtocolKind, request: &LlmRequest) -> serde_json::Value {
    match kind {
        ProtocolKind::AnthropicMessages => anthropic::build_body(request),
        ProtocolKind::OpenAiChat => openai::build_body(request),
        ProtocolKind::OpenAiResponses => openai_responses::build_body(request),
    }
}

pub fn make_state(kind: ProtocolKind) -> Box<dyn ProtocolState> {
    match kind {
        ProtocolKind::AnthropicMessages => Box::new(anthropic::AnthropicState::default()),
        ProtocolKind::OpenAiChat => Box::new(openai::OpenAiState::default()),
        ProtocolKind::OpenAiResponses => Box::new(openai_responses::ResponsesState::default()),
    }
}

pub fn request_path(kind: ProtocolKind) -> &'static str {
    match kind {
        ProtocolKind::AnthropicMessages => "/v1/messages",
        // base_url 需含版本前缀,如 http://127.0.0.1:11434/v1
        ProtocolKind::OpenAiChat => "/chat/completions",
        // codex: base_url = https://chatgpt.com/backend-api/codex
        ProtocolKind::OpenAiResponses => "/responses",
    }
}
