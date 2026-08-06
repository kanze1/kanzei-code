//! 流式客户端:Route(协议 × endpoint × headers)+ LlmClient(HTTP + 代理)。

use futures::{Stream, StreamExt};

use crate::error::LlmError;
use crate::event::LlmEvent;
use crate::protocol::{self, ProtocolKind};
use crate::proxy::{build_http_client, ProxyConfig};
use crate::request::LlmRequest;
use crate::sse::SseParser;

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
    pub fn anthropic(api_key: &str) -> Route {
        Route::anthropic_at("https://api.anthropic.com", api_key)
    }

    pub fn anthropic_at(base_url: &str, api_key: &str) -> Route {
        Route {
            kind: ProtocolKind::AnthropicMessages,
            endpoint: Endpoint {
                base_url: base_url.trim_end_matches('/').to_string(),
                headers: vec![
                    ("x-api-key".into(), api_key.into()),
                    ("anthropic-version".into(), "2023-06-01".into()),
                ],
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
    ) -> Result<impl Stream<Item = Result<LlmEvent, LlmError>> + Send + Unpin, LlmError> {
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
        let response = loop {
            let rb = builder
                .try_clone()
                .ok_or_else(|| LlmError::Config("request not clonable for retry".into()))?;
            match rb.send().await {
                Ok(r) => break r,
                Err(e) if attempt < 2 && (e.is_connect() || e.is_timeout() || e.is_request()) => {
                    attempt += 1;
                    tracing::warn!(attempt, error = %e, "transport error before stream, retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64))
                        .await;
                }
                Err(e) => return Err(LlmError::Transport(e)),
            }
        };
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::classify_http(status.as_u16(), text));
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
