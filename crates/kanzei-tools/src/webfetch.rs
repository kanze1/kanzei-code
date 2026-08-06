//! webfetch 工具(R-023):抓取网页转纯文本。走 kanzei 代理策略(loopback 豁免),
//! 响应大小与输出长度双重截断;research 模式的主力工具。

use async_trait::async_trait;
use kanzei_harness::{KanzeiConfig, Tool, ToolCtx, ToolOutput};
use kanzei_llm::proxy::build_http_client;
use kanzei_llm::ProxyConfig;
use schemars::JsonSchema;
use serde::Deserialize;

const MAX_RESPONSE_BYTES: usize = 3 * 1024 * 1024;
const MAX_OUTPUT_CHARS: usize = 40_000;

#[derive(Deserialize, JsonSchema)]
struct WebFetchInput {
    /// 要抓取的 URL(http/https)
    url: String,
    /// 输出字符上限(默认 40000)
    #[serde(default)]
    max_chars: Option<usize>,
}

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &'static str {
        "webfetch"
    }

    fn description(&self) -> String {
        "Fetch a URL and return readable text (HTML stripped). Params: url; optional max_chars.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(WebFetchInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["url"].as_str().unwrap_or("*").to_string()]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: WebFetchInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        if !input.url.starts_with("http://") && !input.url.starts_with("https://") {
            return ToolOutput::error("url must start with http:// or https://");
        }
        // 与 LLM 请求同一套代理策略(配置驱动,loopback 豁免)。
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
            .get(&input.url)
            .header("user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) kanzei/0.1")
            .header("accept", "text/html,application/xhtml+xml,text/plain,*/*")
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolOutput::error(format!("fetch failed: {e}")),
        };
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // 有界读取:超上限截断,不整包入内存之外再处理。
        let mut body: Vec<u8> = Vec::new();
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
                Err(e) => return ToolOutput::error(format!("read failed: {e}")),
            }
        }

        let text = String::from_utf8_lossy(&body);
        let rendered = if content_type.contains("html") || text.trim_start().starts_with('<') {
            html_to_text(&text)
        } else {
            text.into_owned()
        };
        let cap = input.max_chars.unwrap_or(MAX_OUTPUT_CHARS).max(100);
        let mut out: String = rendered.chars().take(cap).collect();
        if rendered.chars().count() > cap {
            out.push_str("\n…(截断)");
        }
        ToolOutput::ok(format!("HTTP {} · {}\n\n{}", status.as_u16(), input.url, out.trim()))
    }
}

/// 轻量 HTML→文本:去 script/style,剥标签,压空白;不引第三方解析器。
fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 4);
    let mut chars = html.char_indices().peekable();
    let lower = html.to_lowercase();
    let mut skip_until: Option<usize> = None;
    let mut in_tag = false;
    while let Some((i, c)) = chars.next() {
        if let Some(end) = skip_until {
            if i < end {
                continue;
            }
            skip_until = None;
        }
        if c == '<' {
            // script/style 整块跳过
            for (open, close) in [("<script", "</script>"), ("<style", "</style>")] {
                if lower[i..].starts_with(open) {
                    if let Some(pos) = lower[i..].find(close) {
                        skip_until = Some(i + pos + close.len());
                    } else {
                        skip_until = Some(html.len());
                    }
                }
            }
            if skip_until.is_none() {
                in_tag = true;
                // 块级标签换行
                for block in ["</p", "</div", "</li", "</h", "<br", "</tr", "</title"] {
                    if lower[i..].starts_with(block) {
                        out.push('\n');
                        break;
                    }
                }
            }
            continue;
        }
        if c == '>' {
            in_tag = false;
            continue;
        }
        if !in_tag && skip_until.is_none() {
            out.push(c);
        }
    }
    // 实体与空白压缩
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let mut compact = String::with_capacity(out.len());
    let mut blank_lines = 0;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_lines += 1;
            if blank_lines <= 1 {
                compact.push('\n');
            }
        } else {
            blank_lines = 0;
            compact.push_str(trimmed);
            compact.push('\n');
        }
    }
    compact
}
