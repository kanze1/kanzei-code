use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    // reqwest 的 Display 只给出 `error sending request for url (...)` 这一层，
    // Windows 上真正可行动的原因（DNS、连接拒绝、TLS、超时）藏在 source 链里。
    // GUI 最终只会拿 error.to_string()，所以必须在这里把整条因果链保留下来。
    #[error("transport error: {details}", details = error_chain_message(.0))]
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

fn error_chain_message(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let detail = cause.to_string();
        if !detail.is_empty() && !message.contains(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        source = cause.source();
    }
    message
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
        Self::classify_provider_with_code(kind, None, message)
    }

    pub(crate) fn classify_provider_with_code(
        kind: String,
        code: Option<String>,
        message: String,
    ) -> Self {
        // 限流/过载的 kind 优先于消息文本：配额文案可能包含 token/limit，
        // 但这类错误不能触发会破坏历史的上下文压缩重试。
        if is_rate_limit_kind(&kind) || code.as_deref().is_some_and(is_rate_limit_kind) {
            return LlmError::RateLimited {
                status: 0,
                kind: Some(kind),
                body: message,
                retry_after: None,
            };
        }
        if is_overflow_message(&kind)
            || code.as_deref().is_some_and(is_overflow_message)
            || is_overflow_message(&message)
        {
            return LlmError::ContextOverflow { message };
        }
        LlmError::Provider {
            kind: code.unwrap_or(kind),
            message,
        }
    }
}

fn is_rate_limit_kind(kind: &str) -> bool {
    let lower = kind.to_ascii_lowercase();
    // D-402:overload 族按子串匹配——DeepSeek 报 `server_is_overloaded`,与
    // Anthropic 的 `overloaded_error` 拼法不同;精确枚举漏掉它,过载被判成
    // 致命错误终止整轮(state.db 取证 14+ 次,过夜停摆成因之一)。
    lower.contains("overload")
        || matches!(
            lower.as_str(),
            "rate_limit_error" | "rate_limit" | "too_many_requests"
        )
}

fn is_overflow_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "prompt is too long",
        "context length",
        "maximum context",
        "context_length_exceeded",
        "exceeds the context window",
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

    #[derive(Debug)]
    struct TestCause(&'static str);

    impl std::fmt::Display for TestCause {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl std::error::Error for TestCause {}

    #[derive(Debug)]
    struct TestOuter {
        source: TestCause,
    }

    impl std::fmt::Display for TestOuter {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("request failed")
        }
    }

    impl std::error::Error for TestOuter {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.source)
        }
    }

    #[test]
    fn transport_error_keeps_actionable_source_chain() {
        let error = TestOuter {
            source: TestCause("connection refused"),
        };

        assert_eq!(
            error_chain_message(&error),
            "request failed: connection refused"
        );
    }

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

    /// D-402:DeepSeek 的 `server_is_overloaded` 必须归限流族(可重试),
    /// 不得因拼法与 overloaded_error 不同而被判致命。
    #[test]
    fn server_is_overloaded_classifies_as_rate_limited() {
        let error = LlmError::classify_provider(
            "server_is_overloaded".into(),
            "Our servers are currently overloaded. Please try again later.".into(),
        );
        assert!(error.is_rate_limited());
        assert!(!error.is_context_overflow());
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
