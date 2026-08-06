use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("provider returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("context overflow: {message}")]
    ContextOverflow { message: String },
    #[error("provider error ({kind}): {message}")]
    Provider { kind: String, message: String },
    #[error("protocol violation: {0}")]
    Protocol(String),
    #[error("invalid configuration: {0}")]
    Config(String),
}

impl LlmError {
    pub fn is_context_overflow(&self) -> bool {
        matches!(self, LlmError::ContextOverflow { .. })
    }

    /// HTTP 错误分类:识别 context overflow(驱动压缩重试),其余原样返回。
    pub(crate) fn classify_http(status: u16, body: String) -> Self {
        if status == 400 && is_overflow_message(&body) {
            return LlmError::ContextOverflow { message: body };
        }
        LlmError::Http { status, body }
    }

    pub(crate) fn classify_provider(kind: String, message: String) -> Self {
        if is_overflow_message(&message) {
            return LlmError::ContextOverflow { message };
        }
        LlmError::Provider { kind, message }
    }
}

fn is_overflow_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["prompt is too long", "context length", "maximum context", "context_length_exceeded", "input is too long"]
        .iter()
        .any(|p| lower.contains(p))
}
