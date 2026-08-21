//! websearch 工具(R-023):通过 DuckDuckGo HTML 搜索页返回结构化结果。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use kanzei_llm::proxy::build_http_client;
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
    /// R-248:正在填写 prior-art.md 时必传，对应 topic；每次调用消耗一轮预算。
    #[serde(default)]
    prior_art_topic: Option<String>,
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
        "Search the web and return structured title, URL, and snippet results. Params: query; optional max_results. When researching prior-art, pass prior_art_topic so the mechanical websearch round limit is enforced.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(WebSearchInput)).unwrap()
    }

    fn resources(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![SEARCH_URL.into()]
    }

    /// R-323 并发审计:**按入参分流**,不能一刀切成只读。
    ///
    /// 检索本身是纯网络读,但带 `prior_art_topic` 时 `execute` 会调
    /// `prior_art::consume_search_round` **扣减该 topic 的轮次预算**——那是一次
    /// 读-改-写。两个同 topic 的调用并发扣减会互相吃掉对方的写入,预算形同虚设。
    ///
    /// 锁键用 prior-art 专属前缀而不是工作树键:预算落在 `.kanzei/research/`,
    /// 与代码树写入毫无关系,拿工作树键会让它和 edit/bash 无谓地互斥。
    fn concurrency(&self, input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        match input.get("prior_art_topic").and_then(|v| v.as_str()) {
            Some(topic) if !topic.trim().is_empty() => ToolConcurrency::WorktreeWrite(format!(
                "prior-art:{}",
                ctx.project_write_key()
                    .replace(0x5c as char, "/")
                    .to_lowercase()
            )),
            _ => ToolConcurrency::shared_worktree(ctx),
        }
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
        let prior_art_budget = if let Some(topic) = input.prior_art_topic.as_deref() {
            match crate::prior_art::consume_search_round(&ctx.project_root, topic) {
                Ok((used, limit)) => Some((used, limit)),
                Err(error) => return ToolOutput::needs_correction("PRIOR_ART_SEARCH_LIMIT", error),
            }
        } else {
            None
        };
        let proxy = crate::tool_proxy(ctx);
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
            Err(e) => return ToolOutput::error(search_failure_message(&e.to_string())),
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
                "prior_art_budget": prior_art_budget.map(|(used, limit)| serde_json::json!({"used": used, "limit": limit})),
            })
            .to_string(),
        )
    }
}

fn search_failure_message(error: &str) -> String {
    format!(
        "search failed: {error}. DuckDuckGo HTML 端点当前不可达；不要静默重试。若已有论文/项目地址，改用 webfetch；学术检索可直接访问 arXiv abs/pdf 或 `https://export.arxiv.org/api/query?...`，并继续携带 research topic/task_id。"
    )
}

/// D-571:research profile 专用包装。base/dev 的普通搜索保持原 API；research
/// 直调必须绑定活动 loop task，不能绕过 begin_search。
pub struct ResearchWebSearchTool;

#[async_trait]
impl Tool for ResearchWebSearchTool {
    fn name(&self) -> &'static str {
        "websearch"
    }

    fn description(&self) -> String {
        "Research websearch：必须提供 topic 与 research_loop begin_search 返回的 task_id；调用随后仍使用标准 query/max_results。prior-art 搜索还要传 prior_art_topic 以消耗其独立轮次预算。".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = WebSearchTool.input_schema();
        schema["properties"]["topic"] = serde_json::json!({
            "type": "string",
            "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$"
        });
        schema["properties"]["task_id"] = serde_json::json!({ "type": "string" });
        schema["required"] = serde_json::json!(["query", "topic", "task_id"]);
        schema
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        WebSearchTool.resources(input)
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
                "research websearch 必须提供 topic 与 begin_search 返回的 task_id",
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
        WebSearchTool.execute(input, ctx).await
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
        // 逐级把基址累进 rest,避免像原实现那样在第三步丢掉 result__snippet 的偏移,
        // 导致起点落在开标签内部、snippet 带 `snippet">` 垃圾前缀(D-069)。
        let snippet = {
            let rest = &title_body[title_end + 4..];
            rest.find("result__snippet")
                .map(|offset| &rest[offset..])
                .and_then(|rest| rest.find('>').map(|offset| &rest[offset + 1..]))
                .and_then(|rest| {
                    rest.find('<')
                        .map(|end| crate::webfetch::html_to_text(&rest[..end]))
                })
                .unwrap_or_default()
                .trim()
                .to_string()
        };
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
    use super::*;

    #[test]
    fn parses_duckduckgo_results() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa&amp;rut=x">Example <b>title</b></a><a class="result__snippet">A useful snippet</a>"#;
        let results = parse_results(html);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/a");
        assert_eq!(results[0].title, "Example title");
        // 原实现丢了一级基址,这里会拿到 `snippet">A useful snippet`(D-069)
        assert_eq!(results[0].snippet, "A useful snippet");
    }

    /// 中文摘要含多字节字符,错位起点会切在字符中间直接 panic(D-069)。
    #[test]
    fn 中文摘要不错位也不panic() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2F%E4%B8%AD%E6%96%87">中文标题</a><a class="result__snippet">这是一段中文摘要内容</a>"#;
        let results = parse_results(html);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "中文标题");
        assert_eq!(results[0].snippet, "这是一段中文摘要内容");
    }

    /// websearch 复用 webfetch 的 HTML 解析，Unicode 不能让搜索标题解析崩溃或泄漏脚本内容。
    #[test]
    fn unicode_title_keeps_visible_text_and_skips_script() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com">İ <SCRIPT>ẞ hidden</SCRIPT>可见标题</a>"#;
        let results = parse_results(html);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "İ 可见标题");
        assert!(!results[0].title.contains("hidden"));
    }

    #[test]
    fn 端点失败诊断给出arxiv与webfetch降级通道() {
        let message = search_failure_message("connection refused");
        assert!(message.contains("DuckDuckGo HTML"));
        assert!(message.contains("webfetch"));
        assert!(message.contains("export.arxiv.org/api/query"));
        assert!(message.contains("不要静默重试"));
    }

    #[tokio::test]
    async fn research_websearch缺活动任务在联网前被拒绝() {
        let root =
            std::env::temp_dir().join(format!("kz-research-search-gate-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let output = ResearchWebSearchTool
            .execute(
                serde_json::json!({"query": "test", "topic": "topic", "task_id": "forged"}),
                &ctx,
            )
            .await;
        assert_eq!(output.code, Some("RESEARCH_LOOP_TASK_REQUIRED"));
        assert!(output.content.contains("尚未启动检索环"));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn prior_art预算耗尽时websearch本体在联网前拒绝() {
        let root =
            std::env::temp_dir().join(format!("kz-prior-art-search-tool-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let start = crate::prior_art::start_scaffold(
            &root,
            "prior-art-gate",
            crate::prior_art::PriorArtTrigger::ExplicitUser,
            None,
        )
        .unwrap();
        let text = std::fs::read_to_string(&start.absolute_path)
            .unwrap()
            .replace("websearch_round_limit: 4", "websearch_round_limit: 1");
        std::fs::write(&start.absolute_path, text).unwrap();
        assert_eq!(
            crate::prior_art::consume_search_round(&root, "prior-art-gate").unwrap(),
            (1, 1)
        );
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let output = WebSearchTool
            .execute(
                serde_json::json!({"query": "must not reach network", "prior_art_topic": "prior-art-gate"}),
                &ctx,
            )
            .await;
        assert_eq!(output.code, Some("PRIOR_ART_SEARCH_LIMIT"));
        assert!(output.content.contains("1/1"));
        std::fs::remove_dir_all(root).ok();
    }
}

#[cfg(test)]
mod concurrency_audit_tests {
    use super::WebSearchTool;
    use kanzei_harness::{Tool, ToolConcurrency, ToolCtx};
    use serde_json::json;

    fn ctx() -> ToolCtx {
        ToolCtx::new(
            std::path::PathBuf::from("/repo/wt"),
            std::path::PathBuf::from("/repo/main"),
        )
    }

    /// R-323:不带 prior_art_topic 的检索是纯网络读,必须能并行。
    /// 这是本次审计的收益点——原先走 Exclusive 默认,三条检索白白串行。
    #[test]
    fn 纯检索可并行() {
        let ctx = ctx();
        let a = WebSearchTool.concurrency(&json!({"query": "a"}), &ctx);
        let b = WebSearchTool.concurrency(&json!({"query": "b"}), &ctx);
        assert!(!a.conflicts_with(&b), "纯检索之间不该冲突");
        assert!(matches!(a, ToolConcurrency::Shared(_)));
    }

    /// 带 prior_art_topic 时会读-改-写轮次预算,必须互斥——
    /// 并发扣减会互相吃掉对方的写入,预算形同虚设。
    #[test]
    fn 带先行方案主题的检索互斥() {
        let ctx = ctx();
        let a = WebSearchTool.concurrency(&json!({"query": "a", "prior_art_topic": "t1"}), &ctx);
        let b = WebSearchTool.concurrency(&json!({"query": "b", "prior_art_topic": "t2"}), &ctx);
        assert!(a.conflicts_with(&b), "同项目的预算扣减必须串行");
        // 但它不该和代码树写入互斥:预算落在 .kanzei/research/,与 edit/bash 无关。
        let code_write = ToolConcurrency::write_worktree(&ctx);
        assert!(!a.conflicts_with(&code_write), "预算锁不该拖住代码树写入");
    }

    /// 空白 topic 视为没给,回落纯读——否则 `"prior_art_topic": " "` 会白白上锁。
    #[test]
    fn 空白主题回落纯读() {
        let c = WebSearchTool.concurrency(&json!({"query": "a", "prior_art_topic": "  "}), &ctx());
        assert!(matches!(c, ToolConcurrency::Shared(_)));
    }
}
