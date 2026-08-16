//! 流式客户端:Route(协议 × endpoint × headers)+ LlmClient(HTTP + 代理)。

use futures::{Stream, StreamExt};

use crate::error::LlmError;
use crate::event::LlmEvent;
use crate::protocol::{self, ProtocolKind};
use crate::proxy::{build_http_client, ProxyConfig};
use crate::request::LlmRequest;
use crate::request::Part;
use crate::sse::SseParser;

pub const MAX_TRANSPORT_RETRIES: u32 = 2;
pub const MAX_RATE_LIMIT_RETRIES: u32 = 2;

/// D-402:流建立前可原地退避重试的状态码——限流(429/529)与服务端瞬态错误
/// (500/502/503/504)。夜间 provider 过载窗口的 503「一发即致命」是过夜自动
/// 运行停摆的主因(state.db 取证 39 次);有界重试(MAX_RATE_LIMIT_RETRIES)
/// 并尊重 Retry-After。非瞬态 4xx(认证/参数错)重试只会原样复现,不在此列。
fn pre_stream_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 529 | 500 | 502 | 503 | 504)
}

fn transport_retry_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(500 * attempt as u64)
}

fn rate_limit_retry_delay(retry_after: Option<&str>, attempt: u32) -> std::time::Duration {
    let seconds = retry_after
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.min(30))
        .unwrap_or((attempt as u64).min(30));
    std::time::Duration::from_secs(seconds)
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub base_url: String,
    /// 认证等固定 headers(如 x-api-key / authorization)。
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Route {
    pub kind: ProtocolKind,
    pub endpoint: Endpoint,
}

impl Route {
    /// 本路由能否接收图片/文档输入(R-249)。
    ///
    /// 唯一真源:请求前的硬拒绝(见 `stream_with_retry`)与工具结果的图片投递
    /// (kanzei-core 的 tool_images_to_parts)共用这一处判断。两边各写一个 match
    /// 迟早会漂移,而漂移的表现是「硬拒绝放行了、投递却丢图」这类只在特定 provider
    /// 上复现的静默错误。
    pub fn supports_images(&self) -> bool {
        self.kind != ProtocolKind::DeepSeekResponses
    }

    pub fn anthropic(api_key: &str) -> Route {
        Route::anthropic_at("https://api.anthropic.com", api_key)
    }

    pub fn anthropic_at(base_url: &str, api_key: &str) -> Route {
        Route::anthropic_with_headers_at(
            base_url,
            vec![
                ("x-api-key".into(), api_key.into()),
                ("anthropic-version".into(), "2023-06-01".into()),
            ],
        )
    }

    pub fn anthropic_with_headers_at(base_url: &str, headers: Vec<(String, String)>) -> Route {
        Route {
            kind: ProtocolKind::AnthropicMessages,
            endpoint: Endpoint {
                base_url: base_url.trim_end_matches('/').to_string(),
                headers,
            },
        }
    }

    /// OpenAI Responses 协议端点,headers 由调用方备好(如 codex 订阅凭证)。
    pub fn openai_responses_at(base_url: &str, headers: Vec<(String, String)>) -> Route {
        Route {
            kind: ProtocolKind::OpenAiResponses,
            endpoint: Endpoint {
                base_url: base_url.trim_end_matches('/').to_string(),
                headers,
            },
        }
    }

    /// DeepSeek Responses 端点。请求体使用独立方言，不能复用 Codex 的
    /// encrypted reasoning/store/include 参数。
    pub fn deepseek_responses_at(base_url: &str, api_key: &str) -> Route {
        Route {
            kind: ProtocolKind::DeepSeekResponses,
            endpoint: Endpoint {
                base_url: base_url.trim_end_matches('/').to_string(),
                headers: vec![("authorization".into(), format!("Bearer {api_key}"))],
            },
        }
    }

    /// OpenAI 兼容端点。本地服务(Ollama/LM Studio)不需要 key,传 None 即可。
    /// base_url 含版本前缀,如 `http://127.0.0.1:11434/v1`。
    pub fn openai_at(base_url: &str, api_key: Option<&str>) -> Route {
        let mut headers = Vec::new();
        if let Some(key) = api_key.filter(|k| !k.is_empty()) {
            headers.push(("authorization".into(), format!("Bearer {key}")));
        }
        Route {
            kind: ProtocolKind::OpenAiChat,
            endpoint: Endpoint {
                base_url: base_url.trim_end_matches('/').to_string(),
                headers,
            },
        }
    }
}

/// R-175 B1b:derive Clone——后台模式 tokio::spawn 需把 client move 进 'static
/// async 块,主代理与后台子代理各持一份 owned(内部 reqwest::Client 是 Clone)。
#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(proxy: &ProxyConfig) -> Result<Self, LlmError> {
        Ok(LlmClient {
            http: build_http_client(proxy)?,
        })
    }

    /// 发起一次 provider turn,返回 LlmEvent 流。
    /// 事件即产即消,内部不缓存整个响应(内存红线)。
    pub async fn stream(
        &self,
        route: &Route,
        request: &LlmRequest,
    ) -> Result<std::pin::Pin<Box<dyn Stream<Item = Result<LlmEvent, LlmError>> + Send>>, LlmError>
    {
        self.stream_with_retry_notice(route, request, |_, _| {})
            .await
    }

    /// 与 [`stream`] 相同,但在流建立前发生临时网络错误时回调重试状态。
    /// 回调只发生在请求尚未收到响应时;一旦流建立,后续读取错误绝不重放请求。
    pub async fn stream_with_retry_notice<F>(
        &self,
        route: &Route,
        request: &LlmRequest,
        mut on_retry: F,
    ) -> Result<std::pin::Pin<Box<dyn Stream<Item = Result<LlmEvent, LlmError>> + Send>>, LlmError>
    where
        F: FnMut(u32, std::time::Duration),
    {
        if !route.supports_images()
            && request
                .messages
                .iter()
                .flat_map(|message| &message.parts)
                .any(|part| matches!(part, Part::Image { .. } | Part::Document { .. }))
        {
            return Err(LlmError::Config(
                "DeepSeek Responses 当前不支持图片或文件输入；请改用文本或选择支持附件的 provider"
                    .into(),
            ));
        }
        let body = protocol::build_body(route.kind, request);
        let url = format!(
            "{}{}",
            route.endpoint.base_url,
            protocol::request_path(route.kind)
        );

        let mut builder = self
            .http
            .post(&url)
            .header("accept", "text/event-stream")
            .header("content-type", "application/json");
        for (k, v) in &route.endpoint.headers {
            builder = builder.header(k, v);
        }

        let builder = builder.json(&body);
        // R-022:流建立前的瞬断(代理抖动/连接超时)自动重试,退避 0.5s/1s。
        // 流一旦建立绝不重放(工具副作用不可重复)。
        let mut attempt: u32 = 0;
        let mut rate_limit_attempt: u32 = 0;
        let response = loop {
            let rb = builder
                .try_clone()
                .ok_or_else(|| LlmError::Config("request not clonable for retry".into()))?;
            match rb.send().await {
                Ok(response)
                    if pre_stream_retryable_status(response.status().as_u16())
                        && rate_limit_attempt < MAX_RATE_LIMIT_RETRIES =>
                {
                    rate_limit_attempt += 1;
                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok());
                    let delay = rate_limit_retry_delay(retry_after, rate_limit_attempt);
                    on_retry(rate_limit_attempt, delay);
                    tracing::warn!(
                        attempt = rate_limit_attempt,
                        delay_ms = delay.as_millis(),
                        status = response.status().as_u16(),
                        "provider rate limited or server error before stream, retrying"
                    );
                    let _ = response.bytes().await;
                    tokio::time::sleep(delay).await;
                }
                Ok(r) => break r,
                Err(e) if attempt < MAX_TRANSPORT_RETRIES && (e.is_connect() || e.is_timeout()) => {
                    attempt += 1;
                    let delay = transport_retry_delay(attempt);
                    on_retry(attempt, delay);
                    tracing::warn!(attempt, delay_ms = delay.as_millis(), error = %e, "transport error before stream, retrying");
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(LlmError::Transport(e)),
            }
        };
        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u64>().ok())
                .map(|seconds| seconds.min(30));
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::classify_http_with_retry_after(
                status.as_u16(),
                text,
                retry_after,
            ));
        }

        let mut bytes = response.bytes_stream();
        let mut parser = SseParser::default();
        let mut state = protocol::make_state(route.kind);

        let stream = async_stream::try_stream! {
            while let Some(chunk) = bytes.next().await {
                let chunk = chunk.map_err(LlmError::Transport)?;
                for sse in parser.feed(&chunk) {
                    for event in state.step(&sse)? {
                        yield event;
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        pre_stream_retryable_status, rate_limit_retry_delay, transport_retry_delay, LlmClient,
        Route, MAX_RATE_LIMIT_RETRIES, MAX_TRANSPORT_RETRIES,
    };
    use crate::proxy::ProxyConfig;
    use crate::request::{LlmRequest, Message, ReasoningEffort};
    use futures::StreamExt;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// D-402:5xx 服务端瞬态错误进入流建立前重试轨道;非瞬态 4xx 不重试。
    #[test]
    fn pre_stream_retry_covers_server_errors_not_client_errors() {
        for status in [429u16, 529, 500, 502, 503, 504] {
            assert!(pre_stream_retryable_status(status), "{status} 应重试");
        }
        for status in [400u16, 401, 403, 404, 413, 422] {
            assert!(!pre_stream_retryable_status(status), "{status} 不应重试");
        }
    }

    #[test]
    fn transport_retry_is_bounded_and_uses_backoff() {
        assert_eq!(MAX_TRANSPORT_RETRIES, 2);
        assert_eq!(transport_retry_delay(1).as_millis(), 500);
        assert_eq!(transport_retry_delay(2).as_millis(), 1000);
    }

    #[test]
    fn rate_limit_retry_uses_header_and_caps_delay() {
        assert_eq!(MAX_RATE_LIMIT_RETRIES, 2);
        assert_eq!(rate_limit_retry_delay(Some("7"), 1).as_secs(), 7);
        assert_eq!(rate_limit_retry_delay(Some("999"), 1).as_secs(), 30);
        assert_eq!(rate_limit_retry_delay(Some("bad"), 2).as_secs(), 2);
    }

    #[tokio::test]
    async fn rate_limit_http_retries_with_retry_after_then_returns_stream() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(0usize));
        let server_requests = requests.clone();
        let server = tokio::spawn(async move {
            for attempt in 1..=3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0u8; 4096];
                let _ = socket.read(&mut request).await.unwrap();
                *server_requests.lock().unwrap() += 1;
                let response = if attempt < 3 {
                    "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 9\r\nConnection: close\r\n\r\ntry later"
                        .to_string()
                } else {
                    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n";
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(), body
                    )
                };
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });

        let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
        let route = Route::openai_at(&format!("http://{address}/v1"), None);
        let request = LlmRequest {
            model: "mock".into(),
            system: vec![],
            messages: vec![Message::user_text("hello")],
            tools: vec![],
            max_tokens: 32,
            temperature: None,
            reasoning: ReasoningEffort::Off,
            service_tier: None,
        };
        let retries = Arc::new(Mutex::new(Vec::new()));
        let retry_log = retries.clone();
        let mut stream = client
            .stream_with_retry_notice(&route, &request, move |attempt, delay| {
                retry_log.lock().unwrap().push((attempt, delay));
            })
            .await
            .unwrap();
        let mut text = String::new();
        while let Some(event) = stream.next().await {
            if let crate::event::LlmEvent::TextDelta { text: delta, .. } = event.unwrap() {
                text.push_str(&delta);
            }
        }
        server.await.unwrap();

        assert_eq!(*requests.lock().unwrap(), 3);
        assert_eq!(retries.lock().unwrap().len(), 2);
        assert_eq!(text, "ok");
    }
}
