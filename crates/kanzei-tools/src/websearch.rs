//! websearch 工具(R-023):通过 DuckDuckGo HTML 搜索页返回结构化结果。

use async_trait::async_trait;
use futures::future::join_all;
use kanzei_harness::config::KanzeiConfig;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use kanzei_llm::auth::codex::codex_headers;
use kanzei_llm::proxy::build_http_client;
use kanzei_llm::ProxyConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const SEARCH_URL: &str = "https://html.duckduckgo.com/html/";
const MAX_QUERY_CHARS: usize = 500;
const MAX_RESULTS: usize = 10;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum SearchRecency {
    Day,
    Week,
    Month,
    Year,
}

impl SearchRecency {
    fn days(self) -> u16 {
        match self {
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 30,
            Self::Year => 365,
        }
    }

    fn duckduckgo_filter(self) -> &'static str {
        match self {
            Self::Day => "d",
            Self::Week => "w",
            Self::Month => "m",
            Self::Year => "y",
        }
    }
}

#[derive(Clone, Deserialize, JsonSchema)]
struct SearchQuery {
    /// 搜索关键词。
    q: String,
    /// 最近一天、一周、一月或一年。
    #[serde(default)]
    recency: Option<SearchRecency>,
    /// 域名白名单过滤(仅主机名,如 docs.rs)。
    #[serde(default)]
    domains: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct WebSearchInput {
    /// 向后兼容的单查询形态;与 queries 二选一。
    #[serde(default)]
    query: Option<String>,
    /// 批量查询(1–4 项);与 query 二选一。
    #[serde(default)]
    #[schemars(length(min = 1, max = 4))]
    queries: Option<Vec<SearchQuery>>,
    /// 每查询返回结果数量，默认 5，最大 10。
    #[serde(default)]
    max_results: Option<usize>,
    /// R-248:正在填写 prior-art.md 时必传，对应 topic；每次调用消耗一轮预算。
    #[serde(default)]
    prior_art_topic: Option<String>,
}

#[derive(Clone, Serialize)]
struct SearchResult {
    #[serde(rename = "ref")]
    reference: String,
    title: String,
    url: String,
    snippet: String,
}

#[derive(Serialize)]
struct QuerySearchOutput {
    query: String,
    backend: String,
    fallback: Option<String>,
    results: Vec<SearchResult>,
    truncated: bool,
    error: Option<String>,
}

struct CodexBackend {
    endpoint: String,
    model: String,
    headers: Vec<(String, String)>,
}

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &'static str {
        "websearch"
    }

    fn description(&self) -> String {
        "Search the web with 1-4 queries; each query may set recency (day/week/month/year) and domains. Returns result refs that webfetch accepts; Codex /alpha/search falls back to DuckDuckGo when unavailable. Legacy single `query` remains supported. In prior-art work, pass prior_art_topic so the search-round budget is enforced.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(WebSearchInput)).unwrap();
        schema["anyOf"] = serde_json::json!([
            {"required":["query"],"not":{"required":["queries"]}},
            {"required":["queries"],"not":{"required":["query"]}}
        ]);
        schema
    }

    fn resources(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![
            SEARCH_URL.into(),
            "https://chatgpt.com/backend-api/codex/alpha/search".into(),
        ]
    }

    fn resources_with_ctx(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> Vec<String> {
        let mut resources = vec![SEARCH_URL.into()];
        if let Ok((base_url, _)) = resolve_codex_primary(ctx) {
            resources.push(format!("{}/alpha/search", base_url.trim_end_matches('/')));
        }
        resources
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
            Ok(value) => value,
            Err(output) => return output,
        };
        let (queries, legacy_single) = match normalize_search_queries(input.query, input.queries) {
            Ok(value) => value,
            Err(error) => return ToolOutput::needs_correction("INVALID_SEARCH_QUERIES", error),
        };
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
            Ok(client) => client,
            Err(error) => return ToolOutput::error(format!("http client: {error}")),
        };
        let (codex, codex_unavailable) = match load_codex_backend(ctx, &proxy).await {
            Ok(backend) => (Some(backend), None),
            Err(reason) => (None, Some(reason)),
        };
        let mut outcomes = join_all(queries.into_iter().map(|query| {
            search_one_query(
                query,
                limit,
                codex.as_ref(),
                codex_unavailable.as_deref(),
                &client,
                SEARCH_URL,
            )
        }))
        .await;
        for outcome in &mut outcomes {
            if let Err(error) = persist_result_references(ctx, &mut outcome.results) {
                return ToolOutput::error(format!(
                    "search succeeded but result reference persistence failed: {error}"
                ));
            }
        }
        if outcomes.iter().all(|outcome| outcome.error.is_some()) {
            return ToolOutput::failed(
                "WEB_SEARCH_BACKENDS_UNAVAILABLE",
                serde_json::to_string(&serde_json::json!({
                    "queries": outcomes,
                    "prior_art_budget": prior_art_budget.map(|(used, limit)| serde_json::json!({"used": used, "limit": limit})),
                }))
                .unwrap_or_else(|_| "all web search backends failed".into()),
            );
        }
        if legacy_single {
            let outcome = outcomes.pop().expect("one normalized legacy query");
            return ToolOutput::ok(
                serde_json::json!({
                    "query": outcome.query,
                    "backend": outcome.backend,
                    "fallback": outcome.fallback,
                    "results": outcome.results,
                    "truncated": outcome.truncated,
                    "error": outcome.error,
                    "prior_art_budget": prior_art_budget.map(|(used, limit)| serde_json::json!({"used": used, "limit": limit})),
                })
                .to_string(),
            );
        }
        ToolOutput::ok(
            serde_json::json!({
                "queries": outcomes,
                "prior_art_budget": prior_art_budget.map(|(used, limit)| serde_json::json!({"used": used, "limit": limit})),
            })
            .to_string(),
        )
    }
}

fn normalize_search_queries(
    query: Option<String>,
    queries: Option<Vec<SearchQuery>>,
) -> Result<(Vec<SearchQuery>, bool), String> {
    let (mut queries, legacy_single) = match (query, queries) {
        (Some(query), None) => (
            vec![SearchQuery {
                q: query,
                recency: None,
                domains: Vec::new(),
            }],
            true,
        ),
        (None, Some(queries)) => (queries, false),
        (Some(_), Some(_)) => return Err("provide exactly one of query or queries".into()),
        (None, None) => return Err("provide query or queries".into()),
    };
    if queries.is_empty() || queries.len() > 4 {
        return Err("queries must contain 1 to 4 items".into());
    }
    for query in &mut queries {
        query.q = query.q.trim().chars().take(MAX_QUERY_CHARS).collect();
        if query.q.is_empty() {
            return Err("each query q must not be empty".into());
        }
        if query.domains.len() > 10 {
            return Err("each query may specify at most 10 domains".into());
        }
        let mut domains = Vec::new();
        for domain in &query.domains {
            let normalized = normalize_domain(domain)?;
            if !domains.contains(&normalized) {
                domains.push(normalized);
            }
        }
        query.domains = domains;
    }
    Ok((queries, legacy_single))
}

fn normalize_domain(domain: &str) -> Result<String, String> {
    let domain = domain.trim().trim_end_matches('.');
    if domain.is_empty()
        || domain.contains(['/', ':', '?', '#', '@', '*', ' '])
        || domain.contains('\\')
    {
        return Err(format!(
            "invalid domain filter `{domain}`; provide a hostname only"
        ));
    }
    let parsed = reqwest::Url::parse(&format!("https://{domain}/"))
        .map_err(|_| format!("invalid domain filter `{domain}`"))?;
    let host = parsed.host_str().unwrap_or_default();
    if !host.eq_ignore_ascii_case(domain) || parsed.path() != "/" {
        return Err(format!(
            "invalid domain filter `{domain}`; provide a hostname only"
        ));
    }
    Ok(host.to_ascii_lowercase())
}

fn resolve_codex_primary(ctx: &ToolCtx) -> Result<(String, String), String> {
    let config = KanzeiConfig::load_at_root(&ctx.project_root)
        .map_err(|_| "Codex search route configuration unavailable".to_string())?;
    let mut defaults = KanzeiConfig::default();
    defaults.fill_defaults();
    for reference in [
        "primary",
        "fast",
        "compact",
        defaults.models.primary.as_deref().unwrap_or(""),
    ] {
        if let Ok(resolved) = config.resolve_model(reference) {
            if resolved.provider.auth.as_deref() == Some("codex") {
                return Ok((resolved.provider.base_url, resolved.model));
            }
        }
    }
    Err("no configured Codex subscription search route".into())
}

async fn load_codex_backend(ctx: &ToolCtx, proxy: &ProxyConfig) -> Result<CodexBackend, String> {
    let (base_url, model) = resolve_codex_primary(ctx)?;
    let headers = tokio::time::timeout(std::time::Duration::from_secs(10), codex_headers(proxy))
        .await
        .map_err(|_| "Codex credential lookup timed out".to_string())?
        .map_err(|_| "Codex subscription credentials unavailable".to_string())?;
    Ok(CodexBackend {
        endpoint: format!("{}/alpha/search", base_url.trim_end_matches('/')),
        model,
        headers,
    })
}

async fn search_one_query(
    query: SearchQuery,
    limit: usize,
    codex: Option<&CodexBackend>,
    codex_unavailable: Option<&str>,
    client: &reqwest::Client,
    duckduckgo_url: &str,
) -> QuerySearchOutput {
    let codex_result = match codex {
        Some(backend) => search_codex(client, backend, &query, limit).await,
        None => Err(codex_unavailable
            .unwrap_or("Codex search route unavailable")
            .to_string()),
    };
    match codex_result {
        Ok((results, truncated)) => QuerySearchOutput {
            query: query.q,
            backend: "codex_alpha_search".into(),
            fallback: None,
            results,
            truncated,
            error: None,
        },
        Err(codex_error) => {
            let fallback = format!(
                "Codex /alpha/search unavailable ({codex_error}); automatically fell back to DuckDuckGo."
            );
            match search_duckduckgo(client, duckduckgo_url, &query, limit).await {
                Ok((results, truncated)) => QuerySearchOutput {
                    query: query.q,
                    backend: "duckduckgo".into(),
                    fallback: Some(fallback),
                    results,
                    truncated,
                    error: None,
                },
                Err(error) => QuerySearchOutput {
                    query: query.q,
                    backend: "none".into(),
                    fallback: Some(fallback),
                    results: Vec::new(),
                    truncated: false,
                    error: Some(search_failure_message(&error)),
                },
            }
        }
    }
}

fn search_cooldowns(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}
async fn search_codex(
    client: &reqwest::Client,
    backend: &CodexBackend,
    query: &SearchQuery,
    limit: usize,
) -> Result<(Vec<SearchResult>, bool), String> {
    {
        let mut cooldowns = search_cooldowns().lock().unwrap_or_else(|e| e.into_inner());
        cooldowns.retain(|_, at| at.elapsed().as_secs() < 60);
        if cooldowns.contains_key(&backend.endpoint) {
            return Err("backend cooling down after a failed request".into());
        }
    }
    let first = match search_codex_attempt(client, backend, query, limit).await {
        Ok(result) => return Ok(result),
        Err(error) => error,
    };
    let transient = first.contains("request failed")
        || [
            "HTTP 429",
            "HTTP 500",
            "HTTP 502",
            "HTTP 503",
            "HTTP 504",
            "error reading",
        ]
        .iter()
        .any(|text| first.contains(text));
    let error = if transient {
        match search_codex_attempt(client, backend, query, limit).await {
            Ok(result) => return Ok(result),
            Err(second) => format!("{first}; bounded retry: {second}"),
        }
    } else {
        first
    };
    let mut cooldowns = search_cooldowns().lock().unwrap_or_else(|e| e.into_inner());
    if cooldowns.len() >= 64 {
        cooldowns.clear();
    }
    cooldowns.insert(backend.endpoint.clone(), std::time::Instant::now());
    Err(error)
}
fn allowed_result(url: &str, domains: &[String]) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    domains.is_empty()
        || domains.iter().any(|domain| {
            host.eq_ignore_ascii_case(domain)
                || host.to_ascii_lowercase().ends_with(&format!(".{domain}"))
        })
}

async fn search_codex_attempt(
    client: &reqwest::Client,
    backend: &CodexBackend,
    query: &SearchQuery,
    limit: usize,
) -> Result<(Vec<SearchResult>, bool), String> {
    let mut command = serde_json::json!({"q": query.q});
    if let Some(recency) = query.recency {
        command["recency"] = serde_json::json!(recency.days());
    }
    if !query.domains.is_empty() {
        command["domains"] = serde_json::json!(query.domains);
    }
    let body = serde_json::json!({
        "id": search_request_id(),
        "model": backend.model,
        "commands": {"search_query": [command]},
        "settings": {"allowed_callers": ["direct"], "external_web_access": true},
        "max_output_tokens": 2500,
    });
    let mut request = client
        .post(&backend.endpoint)
        .header("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(8))
        .json(&body);
    for (name, value) in &backend.headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request
        .send()
        .await
        .map_err(|_| "Codex /alpha/search request failed".to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Codex /alpha/search HTTP {}", status.as_u16()));
    }
    let (bytes, truncated) = read_search_body(response).await?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Codex /alpha/search returned invalid JSON".to_string())?;
    let items = value["results"]
        .as_array()
        .ok_or_else(|| "Codex /alpha/search response is missing results[]".to_string())?;
    let mut results = Vec::new();
    for item in items {
        let Some(url) = item["url"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };
        let Some(title) = item["title"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };
        if !allowed_result(url, &query.domains) {
            continue;
        }
        results.push(SearchResult {
            reference: String::new(),
            title: title.to_string(),
            url: url.to_string(),
            snippet: item["snippet"].as_str().unwrap_or_default().to_string(),
        });
        if results.len() >= limit {
            break;
        }
    }
    if !items.is_empty() && results.is_empty() && query.domains.is_empty() {
        return Err("Codex /alpha/search result item shape changed".into());
    }
    Ok((results, truncated))
}

async fn search_duckduckgo(
    client: &reqwest::Client,
    endpoint: &str,
    query: &SearchQuery,
    limit: usize,
) -> Result<(Vec<SearchResult>, bool), String> {
    let mut terms = query.q.clone();
    for domain in &query.domains {
        terms.push_str(&format!(" site:{domain}"));
    }
    let mut parameters = vec![("q", terms)];
    if let Some(recency) = query.recency {
        parameters.push(("df", recency.duckduckgo_filter().to_string()));
    }
    let response = client
        .get(endpoint)
        .query(&parameters)
        .header("user-agent", "Mozilla/5.0 kanzei/0.1")
        .header("accept", "text/html")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("DuckDuckGo HTTP {}", response.status().as_u16()));
    }
    let (body, truncated) = read_search_body(response)
        .await
        .map_err(|error| error.to_string())?;
    let html = String::from_utf8_lossy(&body);
    if html.contains("anomaly-modal")
        || html.contains("anomaly.js")
        || html.contains("challenge-form")
    {
        return Err("DuckDuckGo returned a CAPTCHA challenge".into());
    }
    Ok((
        parse_results(&html)
            .into_iter()
            .filter(|result| allowed_result(&result.url, &query.domains))
            .take(limit)
            .collect(),
        truncated,
    ))
}

async fn read_search_body(mut response: reqwest::Response) -> Result<(Vec<u8>, bool), String> {
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                let remaining = MAX_RESPONSE_BYTES.saturating_sub(body.len());
                body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                if body.len() >= MAX_RESPONSE_BYTES {
                    return Ok((body, true));
                }
            }
            Ok(None) => return Ok((body, false)),
            Err(_) => return Err("error reading search response".into()),
        }
    }
}

fn persist_result_references(ctx: &ToolCtx, results: &mut [SearchResult]) -> Result<(), String> {
    for result in results {
        result.reference = crate::web_refs::remember(ctx, &result.url, &result.title)?;
    }
    Ok(())
}

fn search_request_id() -> String {
    let ticks = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("kz-search-{}-{ticks}", std::process::id())
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
        "Research websearch：必须提供 topic 与 research_loop begin_search 返回的 task_id；搜索参数继续支持 query/queries、recency、domains、max_results。prior-art 搜索还要传 prior_art_topic 消耗独立轮次预算。".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = WebSearchTool.input_schema();
        schema["properties"]["topic"] = serde_json::json!({
            "type": "string",
            "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$"
        });
        schema["properties"]["task_id"] = serde_json::json!({ "type": "string" });
        schema["required"] = serde_json::json!(["topic", "task_id"]);
        schema
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        WebSearchTool.resources(input)
    }

    fn resources_with_ctx(&self, input: &serde_json::Value, ctx: &ToolCtx) -> Vec<String> {
        WebSearchTool.resources_with_ctx(input, ctx)
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
                reference: String::new(),
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
    let absolute = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.clone()
    };
    reqwest::Url::parse(&absolute)
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "uddg")
                .map(|(_, value)| value.into_owned())
        })
        .unwrap_or(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn transient_search_failure_retries_once_and_captcha_is_not_empty_success() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/search", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for response in [
                stub_response("503 Service Unavailable", "application/json", "{}"),
                stub_response(
                    "200 OK",
                    "application/json",
                    r#"{"results":[{"title":"Docs","url":"https://docs.rs/a"}]}"#,
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let length = stream.read(&mut request).unwrap();
                assert!(length > 0);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        let client = build_http_client(&ProxyConfig::Disabled).unwrap();
        let query = SearchQuery {
            q: "rust".into(),
            recency: None,
            domains: vec!["docs.rs".into()],
        };
        let backend = CodexBackend {
            endpoint,
            model: "test".into(),
            headers: vec![],
        };
        assert_eq!(
            search_codex(&client, &backend, &query, 5)
                .await
                .unwrap()
                .0
                .len(),
            1
        );
        server.join().unwrap();
        let (url, server) = spawn_search_server(stub_response(
            "200 OK",
            "text/html",
            r#"<form class="challenge-form">CAPTCHA</form>"#,
        ));
        assert!(search_duckduckgo(&client, &url, &query, 5)
            .await
            .err()
            .unwrap()
            .contains("CAPTCHA"));
        server.join().unwrap();
        assert!(!allowed_result(
            "https://docs.rs.evil.example/a",
            &query.domains
        ));
        assert!(allowed_result("https://sub.docs.rs/a", &query.domains));
        assert!(!allowed_result("file:///C:/secret", &[]));
        assert_eq!(
            decode_result_url(
                "//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2F%E4%B8%AD%E6%96%87"
            ),
            "https://docs.rs/中文"
        );
    }

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

    #[test]
    fn search_results_get_session_scoped_refs_for_webfetch() {
        let project = std::env::temp_dir().join(format!(
            "kz-search-ref-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&project).unwrap();
        let ctx =
            ToolCtx::new(project.clone(), project.clone()).with_session_id("search-session".into());
        let mut results = vec![SearchResult {
            reference: String::new(),
            title: "Rust docs".into(),
            url: "https://docs.rs/rust".into(),
            snippet: "API reference".into(),
        }];
        persist_result_references(&ctx, &mut results).unwrap();
        assert!(results[0].reference.starts_with('w'));
        assert_eq!(
            crate::web_refs::resolve(&ctx, &results[0].reference).unwrap(),
            results[0].url
        );
        assert!(!crate::web_refs::was_fetched(&ctx, &results[0].url).unwrap());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn query_normalization_preserves_legacy_and_validates_batches_filters() {
        let (legacy, is_legacy) = normalize_search_queries(Some("  rust  ".into()), None).unwrap();
        assert!(is_legacy);
        assert_eq!(legacy[0].q, "rust");
        let (batch, is_legacy) = normalize_search_queries(
            None,
            Some(vec![SearchQuery {
                q: "rust release".into(),
                recency: Some(SearchRecency::Week),
                domains: vec!["docs.rs".into(), "docs.rs".into()],
            }]),
        )
        .unwrap();
        assert!(!is_legacy);
        assert_eq!(batch[0].recency.unwrap().days(), 7);
        assert_eq!(batch[0].domains, ["docs.rs"]);
        assert!(normalize_search_queries(Some("x".into()), Some(batch)).is_err());
        assert!(normalize_search_queries(None, Some(Vec::new())).is_err());
        assert!(normalize_domain("https://docs.rs/path").is_err());
    }

    fn stub_response(status: &str, content_type: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn spawn_search_server(response: String) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let length = stream.read(&mut request).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request[..length]).to_string()
        });
        (format!("http://{address}/search"), handle)
    }

    #[tokio::test]
    async fn codex_alpha_failure_falls_back_to_filtered_duckduckgo_with_note() {
        let (codex_url, codex_server) = spawn_search_server(stub_response(
            "503 Service Unavailable",
            "application/json",
            "{}",
        ));
        let ddg_html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2Frust">Rust docs</a><a class="result__snippet">API reference</a>"#;
        let (ddg_url, ddg_server) =
            spawn_search_server(stub_response("200 OK", "text/html", ddg_html));
        let client = build_http_client(&ProxyConfig::Disabled).unwrap();
        let backend = CodexBackend {
            endpoint: codex_url,
            model: "gpt-test".into(),
            headers: Vec::new(),
        };
        let outcome = search_one_query(
            SearchQuery {
                q: "Rust release".into(),
                recency: Some(SearchRecency::Week),
                domains: vec!["docs.rs".into()],
            },
            5,
            Some(&backend),
            None,
            &client,
            &ddg_url,
        )
        .await;
        assert_eq!(outcome.backend, "duckduckgo");
        assert!(outcome.fallback.as_deref().unwrap().contains("HTTP 503"));
        assert_eq!(outcome.results.len(), 1);
        assert_eq!(outcome.results[0].url, "https://docs.rs/rust");
        let _ = codex_server.join().unwrap();
        let ddg_request = ddg_server.join().unwrap();
        assert!(ddg_request.contains("df=w"), "{ddg_request}");
        assert!(ddg_request.contains("docs.rs"), "{ddg_request}");
    }

    #[tokio::test]
    async fn codex_alpha_results_are_parsed_without_fallback() {
        let body = serde_json::json!({
            "encrypted_output":"",
            "output":"Rust documentation",
            "results":[{
                "type":"text_result",
                "title":"Rust docs",
                "url":"https://docs.rs/rust",
                "snippet":"API reference",
                "domain":"docs.rs",
                "ref_id":"turn0search0"
            }]
        })
        .to_string();
        let (url, server) = spawn_search_server(stub_response("200 OK", "application/json", &body));
        let client = build_http_client(&ProxyConfig::Disabled).unwrap();
        let backend = CodexBackend {
            endpoint: url,
            model: "gpt-test".into(),
            headers: Vec::new(),
        };
        let (results, truncated) = search_codex(
            &client,
            &backend,
            &SearchQuery {
                q: "Rust release".into(),
                recency: None,
                domains: Vec::new(),
            },
            5,
        )
        .await
        .unwrap();
        assert!(!truncated);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust docs");
        assert_eq!(results[0].url, "https://docs.rs/rust");
        let _ = server.join().unwrap();
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
