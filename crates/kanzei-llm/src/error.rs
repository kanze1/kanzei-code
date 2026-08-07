use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("provider returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("provider rate limited HTTP {status}: {body}")]
    RateLimited {
        status: u16,
        kind: Option<String>,
        body: String,
        retry_after: Option<u64>,
    },
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

    pub(crate) fn classify_http_with_retry_after(
        status: u16,
        body: String,
        retry_after: Option<u64>,
    ) -> Self {
        if matches!(status, 429 | 529) {
            return LlmError::RateLimited {
                status,
                kind: None,
                body,
                retry_after,
            };
        }
        // 不同 provider 对输入过大的响应码并不一致(400/413/422)，只要
        // 错误体明确指出上下文超限，就交给 runner 的有界压缩重试处理。
        if matches!(status, 400 | 413 | 422) && is_overflow_message(&body) {
            return LlmError::ContextOverflow { message: body };
        }
        LlmError::Http { status, body }
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self, LlmError::RateLimited { .. })
    }

    pub(crate) fn classify_provider(kind: String, message: String) -> Self {
        // 限流/过载的 kind 优先于消息文本：配额文案可能包含 token/limit，
        // 但这类错误不能触发会破坏历史的上下文压缩重试。
        if is_rate_limit_kind(&kind) {
            return LlmError::RateLimited {
                status: 0,
                kind: Some(kind),
                body: message,
                retry_after: None,
            };
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
            LlmError::RateLimited { kind: Some(kind), body, .. }
                if kind == "rate_limit_error" && body.contains("token limit")
        ));
    }

    #[test]
    fn http_rate_limit_keeps_status_and_retry_after() {
        let error = LlmError::classify_http_with_retry_after(429, "slow down".into(), Some(7));
        assert!(error.is_rate_limited());
        assert!(matches!(
            error,
            LlmError::RateLimited {
                status: 429,
                kind: None,
                body,
                retry_after: Some(7)
            } if body == "slow down"
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
