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
        // 不同 provider 对输入过大的响应码并不一致(400/413/422)，只要
        // 错误体明确指出上下文超限，就交给 runner 的有界压缩重试处理。
        if matches!(status, 400 | 413 | 422) && is_overflow_message(&body) {
            return LlmError::ContextOverflow { message: body };
        }
        LlmError::Http { status, body }
    }

    pub(crate) fn classify_provider(kind: String, message: String) -> Self {
        // 限流/过载的 kind 优先于消息文本：配额文案可能包含 token/limit，
        // 但这类错误不能触发会破坏历史的上下文压缩重试。
        if is_rate_limit_kind(&kind) {
            return LlmError::Provider { kind, message };
        }
        if is_overflow_message(&message) {
            return LlmError::ContextOverflow { message };
        }
        LlmError::Provider { kind, message }
    }
}

fn is_rate_limit_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "rate_limit_error"
            | "rate_limit"
            | "too_many_requests"
            | "overloaded_error"
            | "overloaded"
    )
}

fn is_overflow_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "prompt is too long",
        "context length",
        "maximum context",
        "context_length_exceeded",
        "input is too long",
        "maximum prompt",
        "too many tokens",
        "token limit",
        "input_tokens",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_kind_with_token_message_does_not_trigger_context_compression() {
        let error = LlmError::classify_provider(
            "rate_limit_error".into(),
            "token limit reached for this minute".into(),
        );

        assert!(!error.is_context_overflow());
        assert!(matches!(
            error,
            LlmError::Provider { kind, message }
                if kind == "rate_limit_error" && message.contains("token limit")
        ));
    }

    #[test]
    fn genuine_context_overflow_still_triggers_compression() {
        let error = LlmError::classify_provider(
            "invalid_request_error".into(),
            "prompt is too long for the model context".into(),
        );

        assert!(error.is_context_overflow());
    }
}
