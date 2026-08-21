//! webfetch 工具(R-023):抓取网页转纯文本。走 kanzei 代理策略(loopback 豁免),
//! 响应大小与输出长度双重截断;research 模式的主力工具。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use kanzei_llm::proxy::build_http_client;
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

/// D-571:research profile 专用包装，要求联网读取归属于活动检索任务。
pub struct ResearchWebFetchTool;

/// R-217:URL 资源规范化——去掉 scheme,保留 域名+路径(+端口)。
/// `https://docs.rs/crate/x` → `docs.rs/crate/x`,`http://example.com` → `example.com/`。
/// 这样权限规则可用 `docs.rs/*` 形态做域名级白名单,与既有 wildcard_match 直接配合。
pub fn normalize_url_resource(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.trim_end_matches('/').to_string()
}
pub struct RawFetch {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Fetch bounded raw bytes for callers that need a binary fallback (for example arXiv PDF).
pub async fn fetch_bytes(url: &str, ctx: &ToolCtx, max_bytes: usize) -> Result<RawFetch, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("url must start with http:// or https://".into());
    }
    let proxy = crate::tool_proxy(ctx);
    let client = build_http_client(&proxy).map_err(|error| format!("http client: {error}"))?;
    let response = client
        .get(url)
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) kanzei/0.1",
        )
        .header(
            "accept",
            "text/html,application/xhtml+xml,text/plain,application/pdf,*/*",
        )
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| format!("fetch failed: {error}"))?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut body = Vec::new();
    let mut stream = response;
    loop {
        match stream.chunk().await {
            Ok(Some(chunk)) => {
                let remaining = max_bytes.saturating_sub(body.len());
                body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                if body.len() >= max_bytes {
                    break;
                }
            }
            Ok(None) => break,
            Err(error) => return Err(format!("read failed: {error}")),
        }
    }
    Ok(RawFetch {
        status,
        content_type,
        body,
    })
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &'static str {
        "webfetch"
    }

    fn description(&self) -> String {
        "Fetch a URL and return readable text (HTML stripped). Params: url; optional max_chars."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(WebFetchInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        // R-217:资源形态 = 域名+路径(去掉 scheme),使规则可用 `docs.rs/*` 精确白名单。
        // 规则如 rule("webfetch", "docs.rs/*", Allow) 匹配 https://docs.rs/... 与
        // http://docs.rs/...;`*` 匹配一切(默认 Ask 不因形态变化而放宽)。
        let url = input["url"].as_str().unwrap_or("*");
        vec![normalize_url_resource(url)]
    }

    /// R-323 并发审计:生产路径**只读**——文件内的 `std::fs::write` 全部位于
    /// `#[cfg(test)] mod tests` 之后(测试夹具),`execute` 本身不落盘。
    /// 原先走 `Exclusive` 默认是「未审计」而非「不安全」,白白把可并行的调用串起来。
    /// 网络抓取不碰工作树,与任何读写都无冲突。
    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: WebFetchInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let fetched = match fetch_bytes(&input.url, ctx, MAX_RESPONSE_BYTES).await {
            Ok(value) => value,
            Err(error) => return ToolOutput::error(error),
        };
        let text = String::from_utf8_lossy(&fetched.body);
        let rendered =
            if fetched.content_type.contains("html") || text.trim_start().starts_with('<') {
                html_to_text(&text)
            } else {
                text.into_owned()
            };
        let cap = input.max_chars.unwrap_or(MAX_OUTPUT_CHARS).max(100);
        let mut out: String = rendered.chars().take(cap).collect();
        if rendered.chars().count() > cap {
            out.push_str("\n…(截断)");
        }
        ToolOutput::ok(format!(
            "HTTP {} · {}\n\n{}",
            fetched.status,
            input.url,
            out.trim()
        ))
    }
}

#[async_trait]
impl Tool for ResearchWebFetchTool {
    fn name(&self) -> &'static str {
        "webfetch"
    }

    fn description(&self) -> String {
        "Research webfetch：必须提供 topic 与 research_loop begin_search 返回的 task_id；调用随后仍使用标准 url/max_chars。".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = WebFetchTool.input_schema();
        schema["properties"]["topic"] = serde_json::json!({
            "type": "string",
            "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$"
        });
        schema["properties"]["task_id"] = serde_json::json!({ "type": "string" });
        schema["required"] = serde_json::json!(["url", "topic", "task_id"]);
        schema
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        WebFetchTool.resources(input)
    }

    async fn execute(&self, mut input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let topic = input
            .get("topic")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let task_id = input
            .get("task_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if topic.is_empty() || task_id.is_empty() {
            return ToolOutput::needs_correction(
                "RESEARCH_LOOP_TASK_REQUIRED",
                "research webfetch 必须提供 topic 与 begin_search 返回的 task_id",
            );
        }
        if let Err(error) =
            crate::research_loop::validate_external_task(&ctx.project_root, topic, task_id)
        {
            return ToolOutput::needs_correction("RESEARCH_LOOP_TASK_REQUIRED", error);
        }
        if let Some(object) = input.as_object_mut() {
            object.remove("topic");
            object.remove("task_id");
        }
        WebFetchTool.execute(input, ctx).await
    }
}

/// 轻量 HTML→文本:去 script/style,剥标签,压空白;不引第三方解析器。
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 4);
    let chars = html.char_indices();
    // 待匹配的标签标记均为 ASCII；只做 ASCII 折叠，避免 Unicode 大小写映射改变字节偏移。
    let lower = html.to_ascii_lowercase();
    let mut skip_until: Option<usize> = None;
    let mut in_tag = false;
    for (i, c) in chars {
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

#[cfg(test)]
mod tests {
    use super::{html_to_text, normalize_url_resource, ResearchWebFetchTool};
    use kanzei_harness::{Tool, ToolCtx};

    /// R-217:URL 资源规范化——去掉 scheme,域名+路径形态可直接配白名单规则。
    #[test]
    fn url资源规范化_去掉scheme保留域名路径() {
        assert_eq!(
            normalize_url_resource("https://docs.rs/crate/x"),
            "docs.rs/crate/x"
        );
        assert_eq!(
            normalize_url_resource("http://example.com/page"),
            "example.com/page"
        );
        assert_eq!(normalize_url_resource("https://example.com"), "example.com");
        assert_eq!(
            normalize_url_resource("https://example.com/"),
            "example.com"
        );
        // 非 http 前缀原样保留(不误伤)。
        assert_eq!(normalize_url_resource("ftp://x/y"), "ftp://x/y");
    }

    /// R-182 内容④:代理配置是**主根**资产,从 worktree 跑时不能读分支副本。
    ///
    /// 两处联网工具共用 `crate::tool_proxy`,这一条同时守住它们两个。
    #[test]
    fn 联网工具取代理配置用主根_不读worktree里的分支副本() {
        use kanzei_harness::ToolCtx;
        let tag = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let main_root = std::env::temp_dir().join(format!("kz-proxy-main-{tag}"));
        let worktree = std::env::temp_dir().join(format!("kz-proxy-tree-{tag}"));
        for root in [&main_root, &worktree] {
            std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        }
        std::fs::write(
            main_root.join(".kanzei/kanzei.toml"),
            "proxy = \"http://127.0.0.1:12000\"\n",
        )
        .unwrap();
        // 分支副本写成 off:读错了就会变成「不走代理」。
        std::fs::write(worktree.join(".kanzei/kanzei.toml"), "proxy = \"off\"\n").unwrap();
        let ctx = ToolCtx {
            cwd: worktree.clone(),
            project_root: main_root.clone(),
            ..Default::default()
        };
        let proxy = crate::tool_proxy(&ctx);
        assert!(
            matches!(&proxy, kanzei_llm::proxy::ProxyConfig::Explicit(url) if url == "http://127.0.0.1:12000"),
            "必须取主根那份配置,实得: {proxy:?}"
        );
        std::fs::remove_dir_all(&worktree).ok();
        std::fs::remove_dir_all(&main_root).ok();
    }

    #[test]
    fn unicode_text_does_not_shift_script_and_style_offsets() {
        let html = "<p>İ 前文</p><SCRIPT>ẞ hidden script</SCRIPT><STYLE>ẞ hidden style</STYLE><p>尾文 ẞ</p>";
        let text = html_to_text(html);

        assert!(text.contains("İ 前文"));
        assert!(text.contains("尾文 ẞ"));
        assert!(!text.contains("hidden script"));
        assert!(!text.contains("hidden style"));
    }

    #[tokio::test]
    async fn research_webfetch缺活动任务在联网前被拒绝() {
        let root =
            std::env::temp_dir().join(format!("kz-research-fetch-gate-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let output = ResearchWebFetchTool
            .execute(
                serde_json::json!({"url": "https://example.test", "topic": "topic", "task_id": "forged"}),
                &ctx,
            )
            .await;
        assert_eq!(output.code, Some("RESEARCH_LOOP_TASK_REQUIRED"));
        assert!(output.content.contains("尚未启动检索环"));
        std::fs::remove_dir_all(root).ok();
    }
}
