//! websearch 工具(R-023):通过 DuckDuckGo HTML 搜索页返回结构化结果。

use async_trait::async_trait;
use kanzei_harness::{KanzeiConfig, Tool, ToolCtx, ToolOutput};
use kanzei_llm::proxy::build_http_client;
use kanzei_llm::ProxyConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const SEARCH_URL: &str = "https://html.duckduckgo.com/html/";
const MAX_QUERY_CHARS: usize = 500;
const MAX_RESULTS: usize = 10;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Deserialize, JsonSchema)]
struct WebSearchInput {
    /// 搜索关键词。
    query: String,
    /// 返回结果数量，默认 5，最大 10。
    #[serde(default)]
    max_results: Option<usize>,
}

#[derive(Serialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &'static str {
        "websearch"
    }

    fn description(&self) -> String {
        "Search the web and return structured title, URL, and snippet results. Params: query; optional max_results.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(WebSearchInput)).unwrap()
    }

    fn resources(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![SEARCH_URL.into()]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: WebSearchInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let query = input.query.trim();
        if query.is_empty() {
            return ToolOutput::error("query must not be empty");
        }
        let query: String = query.chars().take(MAX_QUERY_CHARS).collect();
        let limit = input.max_results.unwrap_or(5).clamp(1, MAX_RESULTS);
        let proxy = match KanzeiConfig::load(&ctx.cwd).ok().and_then(|c| c.proxy) {
            Some(p) if p == "off" => ProxyConfig::Disabled,
            Some(p) if p == "env" => ProxyConfig::Env,
            Some(p) if !p.is_empty() => ProxyConfig::Explicit(p),
            _ => ProxyConfig::Env,
        };
        let client = match build_http_client(&proxy) {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("http client: {e}")),
        };
        let response = match client
            .get(SEARCH_URL)
            .query(&[("q", &query)])
            .header("user-agent", "Mozilla/5.0 kanzei/0.1")
            .header("accept", "text/html")
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("search failed: {e}")),
        };
        let status = response.status();
        if !status.is_success() {
            return ToolOutput::error(format!("search returned HTTP {}", status.as_u16()));
        }
        let mut body = Vec::new();
        let mut stream = response;
        loop {
            match stream.chunk().await {
                Ok(Some(chunk)) => {
                    let take = chunk.len().min(MAX_RESPONSE_BYTES - body.len());
                    body.extend_from_slice(&chunk[..take]);
                    if body.len() >= MAX_RESPONSE_BYTES {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => return ToolOutput::error(format!("search response read failed: {e}")),
            }
        }
        let html = String::from_utf8_lossy(&body);
        let results = parse_results(&html)
            .into_iter()
            .take(limit)
            .collect::<Vec<_>>();
        ToolOutput::ok(
            serde_json::json!({
                "query": query,
                "results": results,
                "truncated": body.len() >= MAX_RESPONSE_BYTES,
            })
            .to_string(),
        )
    }
}

fn parse_results(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("result__a") {
        rest = &rest[start..];
        let Some(href_start) = rest.find("href=\"") else {
            break;
        };
        let after_href = &rest[href_start + 6..];
        let Some(end) = after_href.find('"') else {
            break;
        };
        let raw_url = &after_href[..end];
        let Some(title_start) = after_href[end..].find('>') else {
            break;
        };
        let title_body = &after_href[end + title_start + 1..];
        let Some(title_end) = title_body.find("</a>") else {
            break;
        };
        let title = crate::webfetch::html_to_text(&title_body[..title_end])
            .trim()
            .to_string();
        let url = decode_result_url(raw_url);
        let snippet = title_body[title_end + 4..]
            .find("result__snippet")
            .and_then(|offset| title_body[title_end + 4 + offset..].find('>'))
            .and_then(|offset| {
                let body = &title_body[title_end + 4 + offset + 1..];
                body.find('<')
                    .map(|end| crate::webfetch::html_to_text(&body[..end]))
            })
            .unwrap_or_default()
            .trim()
            .to_string();
        if !url.is_empty() && !title.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }
        rest = &title_body[title_end + 4..];
    }
    results
}

fn decode_result_url(raw: &str) -> String {
    let raw = raw.replace("&amp;", "&");
    raw.split("uddg=")
        .nth(1)
        .map(|url| {
            url.split('&')
                .next()
                .unwrap_or(url)
                .replace("%3A", ":")
                .replace("%2F", "/")
                .replace("%3F", "?")
                .replace("%3D", "=")
                .replace("%26", "&")
        })
        .unwrap_or(raw)
}

#[cfg(test)]
mod tests {
    use super::parse_results;

    #[test]
    fn parses_duckduckgo_results() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa&amp;rut=x">Example <b>title</b></a><a class="result__snippet">A useful snippet</a>"#;
        let results = parse_results(html);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/a");
        assert_eq!(results[0].title, "Example title");
    }
}
