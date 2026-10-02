//! 协议抽象:每种 wire API 提供 body 构造 + 流式状态机(SSE 事件 → LlmEvent)。

pub mod anthropic;
pub mod deepseek_responses;
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
    /// DeepSeek 原生 Responses API（明文 reasoning 历史，无 store/include）。
    DeepSeekResponses,
}

pub trait ProtocolState: Send {
    fn step(&mut self, event: &SseEvent) -> Result<Vec<LlmEvent>, LlmError>;

    /// D-424:字节流走完后的收尾,由 client 在 SSE 循环退出后调一次。
    ///
    /// 不发 `[DONE]` 的 provider 此前只能靠 `finish_reason` 提前收尾——而提前收尾
    /// 会把「finish_reason 之后还在来的 tool_call 参数增量」永久锁在门外(累加器
    /// 已被 take 走、发射标志已置位),放出去的是半截 JSON。收尾点挪到真正的流末,
    /// 这类截断就不存在了。默认空实现:已在流内收过尾的状态机什么都不用做。
    fn finish(&mut self) -> Vec<LlmEvent> {
        Vec::new()
    }
}

fn default_channel(kind: ProtocolKind) -> &'static str {
    match kind {
        ProtocolKind::AnthropicMessages => "anthropic_messages",
        ProtocolKind::OpenAiChat => "openai_chat",
        ProtocolKind::OpenAiResponses => "openai_responses",
        ProtocolKind::DeepSeekResponses => "deepseek_responses",
    }
}

pub fn build_body(kind: ProtocolKind, request: &LlmRequest) -> serde_json::Value {
    build_body_for_route(kind, request, default_channel(kind))
}

/// Route-aware variant required for strict provider-channel Hosted replay.
pub fn build_body_for_route(
    kind: ProtocolKind,
    request: &LlmRequest,
    provider_channel: &str,
) -> serde_json::Value {
    match kind {
        ProtocolKind::AnthropicMessages => {
            anthropic::build_body_for_channel(request, provider_channel)
        }
        ProtocolKind::OpenAiChat => openai::build_body(request),
        ProtocolKind::OpenAiResponses => {
            openai_responses::build_body_for_channel(request, provider_channel)
        }
        ProtocolKind::DeepSeekResponses => deepseek_responses::build_body(request),
    }
}

pub fn make_state(kind: ProtocolKind) -> Box<dyn ProtocolState> {
    make_state_for_route(kind, default_channel(kind))
}

pub fn make_state_for_route(kind: ProtocolKind, provider_channel: &str) -> Box<dyn ProtocolState> {
    match kind {
        ProtocolKind::AnthropicMessages => Box::new(
            anthropic::AnthropicState::with_provider_channel(provider_channel),
        ),
        ProtocolKind::OpenAiChat => Box::new(openai::OpenAiState::default()),
        ProtocolKind::OpenAiResponses => {
            Box::new(openai_responses::ResponsesState::with_route_identity(
                provider_channel,
                "openai_responses",
            ))
        }
        ProtocolKind::DeepSeekResponses => {
            Box::new(openai_responses::ResponsesState::with_route_identity(
                provider_channel,
                "deepseek_responses",
            ))
        }
    }
}

pub fn request_path(kind: ProtocolKind) -> &'static str {
    match kind {
        ProtocolKind::AnthropicMessages => "/v1/messages",
        // base_url 需含版本前缀,如 http://127.0.0.1:11434/v1
        ProtocolKind::OpenAiChat => "/chat/completions",
        // codex: base_url = https://chatgpt.com/backend-api/codex
        ProtocolKind::OpenAiResponses => "/responses",
        ProtocolKind::DeepSeekResponses => "/responses",
    }
}
