//! Backend failures and genuinely empty result pages are different observations.
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchErrorCode {
    Unavailable,
    UnsupportedFilter,
    Unauthorized,
    RateLimited,
    HttpStatus,
    Timeout,
    Transport,
    BodyRead,
    Challenge,
    LayoutChanged,
    InvalidResponse,
    IncompleteResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchFailure {
    pub code: SearchErrorCode,
    pub message: String,
    pub http_status: Option<u16>,
}

impl SearchFailure {
    pub fn new(code: SearchErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            http_status: None,
        }
    }
    pub fn http(backend: &str, status: u16) -> Self {
        Self {
            code: match status {
                401 | 403 => SearchErrorCode::Unauthorized,
                429 => SearchErrorCode::RateLimited,
                _ => SearchErrorCode::HttpStatus,
            },
            message: format!("{backend} HTTP {status}"),
            http_status: Some(status),
        }
    }
    pub fn transport(backend: &str, error: &reqwest::Error) -> Self {
        Self::new(
            if error.is_timeout() {
                SearchErrorCode::Timeout
            } else {
                SearchErrorCode::Transport
            },
            format!("{backend} request failed"),
        )
    }
}
impl std::fmt::Display for SearchFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchStatus {
    Results,
    Empty,
    Filtered,
    Partial,
    Failed,
}

pub struct SearchData {
    pub results: Vec<super::SearchResult>,
    pub truncated: bool,
    pub status: SearchStatus,
}

#[derive(Serialize)]
pub struct SearchAttempt {
    pub backend: String,
    pub dispatched: bool,
    pub status: SearchStatus,
    pub failure: Option<SearchFailure>,
}

pub fn duckduckgo_page(
    html: &str,
    truncated: bool,
    query: &super::SearchQuery,
    limit: usize,
) -> Result<SearchData, SearchFailure> {
    let document = dom_query::Document::from(html);
    if document.select(".anomaly-modal, #anomaly-modal, #challenge-form, .challenge-form, form[action*='anomaly.js'], script[src*='anomaly.js']").exists() {
        return Err(SearchFailure::new(SearchErrorCode::Challenge,"DuckDuckGo returned a CAPTCHA challenge"));
    }
    let parsed = super::parse_results(&document);
    if parsed.is_empty() {
        if truncated {
            return Err(SearchFailure::new(
                SearchErrorCode::IncompleteResponse,
                "DuckDuckGo response was truncated before a result or an empty-page marker",
            ));
        }
        if document
            .select(".no-results, .no-results__message")
            .exists()
        {
            return Ok(SearchData {
                results: Vec::new(),
                truncated: false,
                status: SearchStatus::Empty,
            });
        }
        return Err(SearchFailure::new(
            SearchErrorCode::LayoutChanged,
            "DuckDuckGo HTML layout is not recognized; zero results are unverified",
        ));
    }
    let results = parsed
        .into_iter()
        .filter(|result| super::allowed_result(&result.url, &query.domains, &query.exclude_domains))
        .take(limit)
        .collect::<Vec<_>>();
    let status = if truncated {
        SearchStatus::Partial
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

#[cfg(test)]
mod tests {
    use super::*;
    fn query() -> super::super::SearchQuery {
        super::super::SearchQuery {
            q: "security".into(),
            recency: None,
            domains: vec!["docs.rs".into()],
            ..Default::default()
        }
    }
    #[test]
    fn empty_filtered_challenge_and_unknown_are_distinct_observations() {
        assert_eq!(
            duckduckgo_page(
                "<div class='no-results'>No results</div>",
                false,
                &query(),
                5
            )
            .unwrap()
            .status,
            SearchStatus::Empty
        );
        let filtered="<div class='result'><a href='https://other.example/a' class='result__a'>Other</a></div>";
        assert_eq!(
            duckduckgo_page(filtered, false, &query(), 5)
                .unwrap()
                .status,
            SearchStatus::Filtered
        );
        assert_eq!(
            duckduckgo_page(
                "<form id='challenge-form'>captcha</form>",
                false,
                &query(),
                5
            )
            .err()
            .unwrap()
            .code,
            SearchErrorCode::Challenge
        );
        assert_eq!(
            duckduckgo_page("<h1>Something changed</h1>", false, &query(), 5)
                .err()
                .unwrap()
                .code,
            SearchErrorCode::LayoutChanged
        );
        assert_eq!(
            duckduckgo_page("<div class='no-results'>", true, &query(), 5)
                .err()
                .unwrap()
                .code,
            SearchErrorCode::IncompleteResponse
        );
    }
    #[test]
    fn html_attributes_and_result_cards_preserve_exact_snippet_ownership() {
        let html="<div class='result'><a href='https://docs.rs/a' class='result__a'>First <b>source</b></a><span class='result__snippet'>first evidence</span></div><div class='result'><a class='result__a' href='https://docs.rs/b'>Second</a><span class='result__snippet'>second evidence</span></div>";
        let data = duckduckgo_page(html, false, &query(), 5).unwrap();
        assert_eq!(data.results.len(), 2);
        assert_eq!(data.results[0].snippet, "first evidence");
        assert_eq!(data.results[1].snippet, "second evidence");
        assert_eq!(
            duckduckgo_page(html, true, &query(), 5).unwrap().status,
            SearchStatus::Partial
        );
    }
}
