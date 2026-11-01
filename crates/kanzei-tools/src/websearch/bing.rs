//! Public search for models without hosted search. No backend retry or implicit fallback.
use super::{health::*, SearchQuery, SearchResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use kanzei_harness::ToolCtx;

pub(super) const SEARCH_URL: &str = "https://www.bing.com/search";

pub(super) async fn search(
    query: &SearchQuery,
    limit: usize,
    ctx: &ToolCtx,
) -> Result<SearchData, SearchFailure> {
    if query.recency.is_some() || query.locale.is_some() {
        return Err(SearchFailure::new(
            SearchErrorCode::UnsupportedFilter,
            "Bing HTML search does not support the requested recency or DuckDuckGo locale filter",
        ));
    }
    let mut url = reqwest::Url::parse(SEARCH_URL).unwrap();
    let mut text = query.q.clone();
    if query.domains.len() == 1 {
        text.push_str(&format!(" site:{}", query.domains[0]));
    }
    for domain in &query.exclude_domains {
        text.push_str(&format!(" -site:{domain}"));
    }
    // The desktop's default language is Chinese. Pin the search market rather
    // than inheriting the exit node's market (for example a Japanese proxy).
    url.query_pairs_mut()
        .append_pair("q", &text)
        .append_pair("setlang", "zh-Hans")
        .append_pair("cc", "CN");
    let response = crate::webfetch::fetch_bytes(url.as_str(), ctx, super::MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| SearchFailure::new(SearchErrorCode::Transport, error))?;
    if response.status != 200 {
        return Err(SearchFailure::http("Bing", response.status));
    }
    let html = std::str::from_utf8(&response.body).map_err(|_| {
        SearchFailure::new(
            SearchErrorCode::InvalidResponse,
            "Bing search response is not UTF-8",
        )
    })?;
    parse(html, query, limit)
}

fn source_url(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw).ok()?;
    if url.host_str() == Some("www.bing.com") && url.path() == "/ck/a" {
        let encoded = url.query_pairs().find(|(key, _)| key == "u")?.1;
        let bytes = URL_SAFE_NO_PAD.decode(encoded.strip_prefix("a1")?).ok()?;
        String::from_utf8(bytes).ok()
    } else {
        Some(url.into())
    }
}

fn parse(html: &str, query: &SearchQuery, limit: usize) -> Result<SearchData, SearchFailure> {
    let document = dom_query::Document::from(html);
    if document
        .select("#b_captcha, #b_captcha_container, form[action*='challenge']")
        .exists()
    {
        return Err(SearchFailure::new(
            SearchErrorCode::Challenge,
            "Bing returned a CAPTCHA challenge",
        ));
    }
    let mut parsed = Vec::new();
    for entry in document.select("li.b_algo").iter() {
        let anchor = entry.select("h2 a");
        let title = anchor.text().trim().to_owned();
        let Some(url) = anchor.attr("href").and_then(|raw| source_url(&raw)) else {
            continue;
        };
        if title.is_empty() || !super::allowed_result(&url, &[], &[]) {
            continue;
        }
        parsed.push(SearchResult {
            reference: String::new(),
            title,
            url,
            snippet: entry.select(".b_caption p").text().trim().to_owned(),
        });
    }
    if parsed.is_empty() {
        if document.select(".b_no").exists() && !document.select("li.b_algo").exists() {
            return Ok(SearchData {
                results: Vec::new(),
                truncated: false,
                status: SearchStatus::Empty,
                evidence: None,
            });
        }
        return Err(SearchFailure::new(
            SearchErrorCode::LayoutChanged,
            "Bing HTML layout is not recognized; zero results are unverified",
        ));
    }
    let results: Vec<_> = parsed
        .into_iter()
        .filter(|r| super::allowed_result(&r.url, &query.domains, &query.exclude_domains))
        .take(limit)
        .collect();
    let status = if results.is_empty() {
        SearchStatus::Filtered
    } else {
        SearchStatus::Results
    };
    Ok(SearchData {
        results,
        status,
        truncated: false,
        evidence: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_sources_and_filters_preserve_distinct_failure_states() {
        let encoded = URL_SAFE_NO_PAD.encode("https://docs.rs/rust?a=1&b=2");
        let html = format!("<ol><li class='b_algo'><h2><a href='https://www.bing.com/ck/a?u=a1{encoded}'>Rust docs</a></h2><div class='b_caption'><p>Source evidence</p></div></li></ol>");
        let mut query = SearchQuery::default();
        let data = parse(&html, &query, 5).unwrap();
        assert_eq!(data.results[0].url, "https://docs.rs/rust?a=1&b=2");
        assert_eq!(data.results[0].snippet, "Source evidence");
        query.domains = vec!["example.com".into()];
        assert_eq!(
            parse(&html, &query, 5).unwrap().status,
            SearchStatus::Filtered
        );
        assert_eq!(
            parse("<div class='b_no'>No results</div>", &query, 5)
                .unwrap()
                .status,
            SearchStatus::Empty
        );
        assert!(matches!(
            parse("<form action='/challenge'></form>", &query, 5),
            Err(SearchFailure {
                code: SearchErrorCode::Challenge,
                ..
            })
        ));
        assert!(matches!(
            parse("<html>changed</html>", &query, 5),
            Err(SearchFailure {
                code: SearchErrorCode::LayoutChanged,
                ..
            })
        ));
        assert!(source_url("https://www.bing.com/ck/a?u=bad").is_none());
    }

    #[tokio::test]
    #[ignore = "requires reachable live Bing search pages"]
    async fn live_search_returns_attributed_original_urls() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path().into(), dir.path().into());
        let query = SearchQuery {
            q: "Rust language documentation".into(),
            ..Default::default()
        };
        let data = search(&query, 5, &ctx).await.unwrap();
        assert!(!data.results.is_empty());
        assert!(data
            .results
            .iter()
            .all(|r| super::super::allowed_result(&r.url, &[], &[]) && !r.title.is_empty()));
    }
}
