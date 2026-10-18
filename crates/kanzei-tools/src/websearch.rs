//! websearch 工具(R-023):通过 DuckDuckGo HTML 搜索页返回结构化结果。

use async_trait::async_trait;
use futures::future::join_all;
use kanzei_harness::config::{KanzeiConfig, SearchBackend};
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use kanzei_llm::auth::codex::codex_headers;
use kanzei_llm::proxy::build_http_client;
use kanzei_llm::ProxyConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
mod health;
use health::{SearchAttempt, SearchData, SearchErrorCode, SearchFailure, SearchStatus};

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
    status: SearchStatus,
    attempts: Vec<SearchAttempt>,
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
        "Search the web with 1-4 queries; each query may set recency (day/week/month/year) and domains. Returns result refs that webfetch accepts. The configured backend controls network routing: explicit duckduckgo/codex never contacts the other service; auto uses the primary model's provider and permits an explicit DuckDuckGo fallback. Codex /alpha/search is an unpublished adapter, not a supported public search API. Legacy single `query` remains supported. In prior-art work, pass prior_art_topic so the search-round budget is enforced.".into()
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

    fn resources_with_ctx(&self, input: &serde_json::Value, ctx: &ToolCtx) -> Vec<String> {
        let Ok(config) = KanzeiConfig::load_at_root(&ctx.project_root) else {
            return self.resources(input);
        };
        match selected_backend(&config) {
            Ok(SearchBackend::Duckduckgo) => vec![SEARCH_URL.into()],
            Ok(backend) => {
                let mut resources = Vec::new();
                if backend == SearchBackend::Auto {
                    resources.push(SEARCH_URL.into());
                }
                if let Ok((base_url, _)) = resolve_codex_primary(&config) {
                    resources.push(format!("{}/alpha/search", base_url.trim_end_matches('/')));
                }
                resources
            }
            Err(_) => self.resources(input),
        }
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
            Some(topic) if !topic.trim().is_empty() => ToolConcurrency::ResourceWrite(format!(
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
        let config = match KanzeiConfig::load_at_root(&ctx.project_root) {
            Ok(config) => config,
            Err(error) => return ToolOutput::failed("WEB_SEARCH_CONFIG", error.to_string()),
        };
        let backend = match selected_backend(&config) {
            Ok(backend) => backend,
            Err(error) => return ToolOutput::failed("WEB_SEARCH_CONFIG", error),
        };
        let (codex, codex_unavailable) = match backend {
            SearchBackend::Duckduckgo => (None, None),
            _ => match load_codex_backend(&config, &proxy).await {
                Ok(backend) => (Some(backend), None),
                Err(reason) => (None, Some(reason)),
            },
        };
        let mut outcomes = join_all(queries.into_iter().map(|query| {
            search_one_query(
                backend,
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
                "WEB_SEARCH_FAILED",
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
                    "status": outcome.status,
                    "attempts": outcome.attempts,
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

fn selected_backend(config: &KanzeiConfig) -> Result<SearchBackend, String> {
    match config.web.search_backend() {
        SearchBackend::Auto => match config.resolve_model("primary") {
            Ok(resolved) if resolved.provider.auth.as_deref() == Some("codex") => {
                Ok(SearchBackend::Auto)
            }
            Ok(_) => Ok(SearchBackend::Duckduckgo),
            Err(error) => Err(format!("primary search route configuration: {error}")),
        },
        explicit => Ok(explicit),
    }
}

fn resolve_codex_primary(config: &KanzeiConfig) -> Result<(String, String), String> {
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

async fn load_codex_backend(
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
) -> Result<CodexBackend, String> {
    let (base_url, model) = resolve_codex_primary(config)?;
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
    backend: SearchBackend,
    query: SearchQuery,
    limit: usize,
    codex: Option<&CodexBackend>,
    codex_unavailable: Option<&str>,
    client: &reqwest::Client,
    duckduckgo_url: &str,
) -> QuerySearchOutput {
    if backend == SearchBackend::Duckduckgo {
        return duckduckgo_outcome(query, limit, client, duckduckgo_url, None).await;
    }
    let result = match codex {
        Some(backend) => search_codex(client, backend, &query, limit).await,
        None => Err(SearchFailure::new(
            SearchErrorCode::Unavailable,
            codex_unavailable.unwrap_or("Codex search route unavailable"),
        )),
    };
    match result {
        Ok(data) => backend_outcome(query, "codex_alpha_search", None, Ok(data), true),
        Err(error) if backend == SearchBackend::Codex => backend_outcome(
            query,
            "codex_alpha_search",
            None,
            Err(error),
            codex.is_some(),
        ),
        Err(error) => {
            let fallback =
                format!("Codex /alpha/search unavailable ({error}); auto selected DuckDuckGo.");
            let mut outcome =
                duckduckgo_outcome(query, limit, client, duckduckgo_url, Some(fallback)).await;
            outcome.attempts.insert(
                0,
                SearchAttempt {
                    backend: "codex_alpha_search".into(),
                    dispatched: codex.is_some(),
                    status: SearchStatus::Failed,
                    failure: Some(error),
                },
            );
            outcome
        }
    }
}

fn backend_outcome(
    query: SearchQuery,
    backend: &str,
    fallback: Option<String>,
    result: Result<SearchData, SearchFailure>,
    dispatched: bool,
) -> QuerySearchOutput {
    match result {
        Ok(data) => QuerySearchOutput {
            query: query.q,
            backend: backend.into(),
            fallback,
            results: data.results,
            truncated: data.truncated,
            error: None,
            status: data.status,
            attempts: vec![SearchAttempt {
                backend: backend.into(),
                dispatched,
                status: data.status,
                failure: None,
            }],
        },
        Err(error) => QuerySearchOutput {
            query: query.q,
            backend: backend.into(),
            fallback,
            results: Vec::new(),
            truncated: false,
            error: Some(search_failure_message(&error.to_string())),
            status: SearchStatus::Failed,
            attempts: vec![SearchAttempt {
                backend: backend.into(),
                dispatched,
                status: SearchStatus::Failed,
                failure: Some(error),
            }],
        },
    }
}

async fn duckduckgo_outcome(
    query: SearchQuery,
    limit: usize,
    client: &reqwest::Client,
    endpoint: &str,
    fallback: Option<String>,
) -> QuerySearchOutput {
    let result = search_duckduckgo(client, endpoint, &query, limit).await;
    backend_outcome(query, "duckduckgo", fallback, result, true)
}
async fn search_codex(
    client: &reqwest::Client,
    backend: &CodexBackend,
    query: &SearchQuery,
    limit: usize,
) -> Result<SearchData, SearchFailure> {
    search_codex_attempt(client, backend, query, limit).await
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
) -> Result<SearchData, SearchFailure> {
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
        .map_err(|error| SearchFailure::transport("Codex /alpha/search", &error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(SearchFailure::http("Codex /alpha/search", status.as_u16()));
    }
    let (bytes, truncated) = read_search_body(response).await?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
        SearchFailure::new(
            SearchErrorCode::InvalidResponse,
            "Codex /alpha/search returned invalid JSON",
        )
    })?;
    let items = value["results"].as_array().ok_or_else(|| {
        SearchFailure::new(
            SearchErrorCode::InvalidResponse,
            "Codex /alpha/search response is missing results[]",
        )
    })?;
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
        return Err(SearchFailure::new(
            SearchErrorCode::InvalidResponse,
            "Codex /alpha/search result item shape changed",
        ));
    }
    let status = if truncated {
        SearchStatus::Partial
    } else if items.is_empty() {
        SearchStatus::Empty
    } else if results.is_empty() {
        SearchStatus::Filtered
    } else {
        SearchStatus::Results
    };
    Ok(SearchData {
        results,
        truncated,
        status,
    })
}

async fn search_duckduckgo(
    client: &reqwest::Client,
    endpoint: &str,
    query: &SearchQuery,
    limit: usize,
) -> Result<SearchData, SearchFailure> {
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
        .map_err(|error| SearchFailure::transport("DuckDuckGo", &error))?;
    if !response.status().is_success() {
        return Err(SearchFailure::http(
            "DuckDuckGo",
            response.status().as_u16(),
        ));
    }
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    if !matches!(content_type, "text/html" | "application/xhtml+xml") {
        let mut failure = SearchFailure::new(
            SearchErrorCode::InvalidResponse,
            "DuckDuckGo response is not HTML",
        );
        failure.http_status = Some(status);
        return Err(failure);
    }
    let (body, truncated) = read_search_body(response).await?;
    let html = String::from_utf8(body).map_err(|_| {
        SearchFailure::new(
            SearchErrorCode::InvalidResponse,
            "DuckDuckGo response is not valid UTF-8",
        )
    })?;
    health::duckduckgo_page(&html, truncated, query, limit).map_err(|mut failure| {
        failure.http_status = Some(status);
        failure
    })
}

async fn read_search_body(
    mut response: reqwest::Response,
) -> Result<(Vec<u8>, bool), SearchFailure> {
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
            Err(_) => {
                return Err(SearchFailure::new(
                    SearchErrorCode::BodyRead,
                    "error reading search response",
                ))
            }
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
        "search failed: {error}. 本次没有取得可验证搜索结果；不要静默重试。已知原始来源可用 webfetch 或 browser 读取；调查任务继续携带原 topic/task_id。"
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

fn parse_results(document: &dom_query::Document) -> Vec<SearchResult> {
    document.select("script, style").remove();
    document
        .select("a.result__a")
        .iter()
        .filter_map(|anchor| {
            let url = decode_result_url(anchor.attr("href")?.as_ref());
            let title = anchor.text().trim().to_owned();
            if title.is_empty() || url.is_empty() {
                return None;
            }
            let snippet = anchor
                .ancestors(None)
                .iter()
                .find(|parent| parent.has_class("result"))
                .map(|parent| parent.select(".result__snippet").text().trim().to_owned())
                .unwrap_or_default();
            Some(SearchResult {
                reference: String::new(),
                title,
                url,
                snippet,
            })
        })
        .collect()
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
    async fn a_changed_html_layout_is_not_evidence_of_zero_search_results() {
        let (url, server) = spawn_search_server(stub_response(
            "200 OK",
            "text/html",
            "<html><body><h1>Sign in to continue</h1></body></html>",
        ));
        let client = build_http_client(&ProxyConfig::Disabled).unwrap();
        let query = SearchQuery {
            q: "CodeBuddy security advisory".into(),
            recency: None,
            domains: Vec::new(),
        };
        let outcome = duckduckgo_outcome(query, 5, &client, &url, None).await;
        server.join().unwrap();
        assert!(
            outcome.error.is_some(),
            "an unknown page is not an empty successful search"
        );
    }

    #[tokio::test]
    async fn each_query_attempt_is_explicit_and_captcha_is_not_empty_success() {
        let (endpoint, server) = spawn_search_server(stub_response(
            "503 Service Unavailable",
            "application/json",
            "{}",
        ));
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
        let failure = search_codex(&client, &backend, &query, 5)
            .await
            .err()
            .unwrap();
        assert_eq!(failure.code, SearchErrorCode::HttpStatus);
        assert_eq!(failure.http_status, Some(503));
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
            .message
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
        let results = parse_results(&dom_query::Document::from(format!(
            "<div class=\"result\">{html}</div>"
        )));
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
        let results = parse_results(&dom_query::Document::from(format!(
            "<div class=\"result\">{html}</div>"
        )));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "中文标题");
        assert_eq!(results[0].snippet, "这是一段中文摘要内容");
    }

    /// websearch 复用 webfetch 的 HTML 解析，Unicode 不能让搜索标题解析崩溃或泄漏脚本内容。
    #[test]
    fn unicode_title_keeps_visible_text_and_skips_script() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com">İ <SCRIPT>ẞ hidden</SCRIPT>可见标题</a>"#;
        let results = parse_results(&dom_query::Document::from(format!(
            "<div class=\"result\">{html}</div>"
        )));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "İ 可见标题");
        assert!(!results[0].title.contains("hidden"));
    }

    #[test]
    fn 端点失败保留真实原因且不把任务导向另一研究主题() {
        let message = search_failure_message("connection refused");
        assert!(message.contains("connection refused"));
        assert!(message.contains("webfetch"));
        assert!(message.contains("browser"));
        assert!(!message.contains("arxiv"));
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

    #[test]
    fn search_backend_is_explicit_and_auto_uses_only_primary() {
        let mut config = KanzeiConfig::default();
        config.fill_defaults();
        config.providers.insert(
            "plain".into(),
            kanzei_harness::config::ProviderConfig {
                protocol: "openai".into(),
                base_url: "https://example.invalid/v1".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: None,
            },
        );
        config.models.primary = Some("plain:fixture".into());
        config.models.fast = Some("codex:fixture".into());
        config.models.compact = Some("codex:fixture".into());
        assert_eq!(
            selected_backend(&config).unwrap(),
            SearchBackend::Duckduckgo
        );
        config.models.primary = Some("codex:fixture".into());
        assert_eq!(selected_backend(&config).unwrap(), SearchBackend::Auto);
        config.web.search_backend = Some(SearchBackend::Duckduckgo);
        assert_eq!(
            selected_backend(&config).unwrap(),
            SearchBackend::Duckduckgo
        );
        config.web.search_backend = Some(SearchBackend::Codex);
        assert_eq!(selected_backend(&config).unwrap(), SearchBackend::Codex);
        assert!(serde_json::from_value::<KanzeiConfig>(serde_json::json!({
            "web": {"search_backend": "typo"}
        }))
        .is_err());
        config.web.search_backend = Some(SearchBackend::Auto);
        config.models.primary = Some("missing-provider:fixture".into());
        assert!(selected_backend(&config).is_err());
    }

    #[tokio::test]
    async fn explicit_duckduckgo_never_contacts_codex() {
        let unused_codex = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        unused_codex.set_nonblocking(true).unwrap();
        let backend = CodexBackend {
            endpoint: format!("http://{}/search", unused_codex.local_addr().unwrap()),
            model: "fixture".into(),
            headers: Vec::new(),
        };
        let html = r#"<a class="result__a" href="https://docs.rs/rust">Rust</a>"#;
        let (ddg_url, server) = spawn_search_server(stub_response("200 OK", "text/html", html));
        let client = build_http_client(&ProxyConfig::Disabled).unwrap();
        let outcome = search_one_query(
            SearchBackend::Duckduckgo,
            SearchQuery {
                q: "rust".into(),
                recency: None,
                domains: Vec::new(),
            },
            5,
            Some(&backend),
            None,
            &client,
            &ddg_url,
        )
        .await;
        assert_eq!(outcome.backend, "duckduckgo");
        assert!(outcome.error.is_none());
        assert!(outcome.fallback.is_none());
        assert_eq!(outcome.results.len(), 1);
        assert!(server.join().unwrap().contains("q=rust"));
        assert!(
            matches!(unused_codex.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }

    #[tokio::test]
    async fn explicit_codex_failure_never_contacts_duckduckgo() {
        let unused_ddg = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        unused_ddg.set_nonblocking(true).unwrap();
        let ddg_url = format!("http://{}/search", unused_ddg.local_addr().unwrap());
        let (codex_url, server) =
            spawn_search_server(stub_response("401 Unauthorized", "application/json", "{}"));
        let backend = CodexBackend {
            endpoint: codex_url,
            model: "fixture".into(),
            headers: Vec::new(),
        };
        let client = build_http_client(&ProxyConfig::Disabled).unwrap();
        let outcome = search_one_query(
            SearchBackend::Codex,
            SearchQuery {
                q: "rust".into(),
                recency: None,
                domains: Vec::new(),
            },
            5,
            Some(&backend),
            None,
            &client,
            &ddg_url,
        )
        .await;
        assert_eq!(outcome.backend, "codex_alpha_search");
        assert!(outcome.error.as_deref().unwrap().contains("HTTP 401"));
        assert!(outcome.fallback.is_none());
        server.join().unwrap();
        assert!(
            matches!(unused_ddg.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
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
            SearchBackend::Auto,
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
        let SearchData {
            results, truncated, ..
        } = search_codex(
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
