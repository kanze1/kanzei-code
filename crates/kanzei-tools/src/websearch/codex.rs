//! Subscription search is a direct, bounded command request, not a Responses model turn.
//! Its private endpoint is shape-checked on every response and has no internal retry.
use super::health::{SearchData, SearchErrorCode, SearchFailure, SearchStatus};
use super::{
    allowed_result, read_search_body, NativeBackend, SearchQuery, SearchRecency, SearchResult,
};
use serde_json::{json, Value};
use std::collections::HashSet;

const MAX_SEARCH_OUTPUT_TOKENS: u32 = 2500;

pub(super) async fn search(
    client: &reqwest::Client,
    backend: &NativeBackend,
    query: &SearchQuery,
    limit: usize,
) -> Result<SearchData, SearchFailure> {
    let ticks = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| {
            SearchFailure::new(
                SearchErrorCode::Unavailable,
                "System clock precedes the search request epoch",
            )
        })?
        .as_nanos();
    let request_id = format!("kz-search-{}-{ticks}", std::process::id());
    let mut command = json!({"q": query.q});
    if !query.domains.is_empty() {
        command["domains"] = json!(query.domains);
    }
    if let Some(recency) = query.recency {
        command["recency"] = json!(match recency {
            SearchRecency::Day => 1,
            SearchRecency::Week => 7,
            SearchRecency::Month => 30,
            SearchRecency::Year => 365,
        });
    }
    let body = json!({"id":request_id,"model":backend.model,"commands":{"search_query":[command]},
        "settings":{"allowed_callers":["direct"],"external_web_access":true},"max_output_tokens":MAX_SEARCH_OUTPUT_TOKENS});
    let mut request = client
        .post(&backend.endpoint)
        .header("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(90))
        .json(&body);
    for (name, value) in &backend.headers {
        request = request.header(name, value);
    }
    let response = request
        .send()
        .await
        .map_err(|error| SearchFailure::transport("Codex direct search", &error))?;
    if !response.status().is_success() {
        return Err(SearchFailure::http(
            "Codex direct search",
            response.status().as_u16(),
        ));
    }
    let (bytes, truncated) = read_search_body(response).await?;
    if truncated {
        return Err(SearchFailure::new(
            SearchErrorCode::IncompleteResponse,
            "Codex search exceeded its response byte budget",
        ));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("Codex search returned invalid JSON"))?;
    parse(&value, query, limit, &request_id)
}

fn invalid(message: &str) -> SearchFailure {
    SearchFailure::new(SearchErrorCode::InvalidResponse, message)
}

fn source_field<'a>(item: &'a Value, index: usize, name: &str) -> Result<&'a str, SearchFailure> {
    item[name]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            invalid(&format!(
                "Codex search results[{index}].{name} must be a non-empty string"
            ))
        })
}

fn parse_source(item: &Value, index: usize) -> Result<(&str, &str, &str, &str), SearchFailure> {
    if item["type"] != "text_result" {
        return Err(invalid(&format!(
            "Codex search results[{index}].type must be text_result"
        )));
    }
    let url = source_field(item, index, "url")?;
    let title = source_field(item, index, "title")?;
    let reference = source_field(item, index, "ref_id")?;
    let snippet = item["snippet"].as_str().ok_or_else(|| {
        invalid(&format!(
            "Codex search results[{index}].snippet must be a string"
        ))
    })?;
    if !allowed_result(url, &[], &[]) {
        return Err(invalid(&format!(
            "Codex search results[{index}].url must be an allowed HTTP source URL"
        )));
    }
    Ok((url, title, reference, snippet))
}

fn parse(
    value: &Value,
    query: &SearchQuery,
    limit: usize,
    request_id: &str,
) -> Result<SearchData, SearchFailure> {
    let items = value["results"]
        .as_array()
        .ok_or_else(|| invalid("Codex search response is missing results[]"))?;
    if !value["output"].is_string() || !value["encrypted_output"].is_string() {
        return Err(invalid(
            "Codex search response is missing its output envelope",
        ));
    }
    let mut results = Vec::new();
    let mut sources = Vec::new();
    let mut rejected = Vec::new();
    let mut seen = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        let (url, title, reference, snippet) = match parse_source(item, index) {
            Ok(source) => source,
            Err(failure) => {
                rejected.push((index, failure));
                continue;
            }
        };
        sources.push(json!({"ref":reference,"url":url}));
        if allowed_result(url, &query.domains, &query.exclude_domains) && seen.insert(url) {
            results.push(SearchResult {
                reference: String::new(),
                title: title.into(),
                url: url.into(),
                snippet: snippet.into(),
            });
        }
    }
    if sources.is_empty() && !rejected.is_empty() {
        return Err(invalid(&format!(
            "Codex search returned no valid sources; {} result(s) rejected; {}",
            rejected.len(),
            rejected[0].1.message
        )));
    }
    let truncated = results.len() > limit;
    results.truncate(limit);
    let status = if !rejected.is_empty() {
        SearchStatus::Partial
    } else if !results.is_empty() {
        SearchStatus::Results
    } else if items.is_empty() {
        SearchStatus::Empty
    } else {
        SearchStatus::Filtered
    };
    Ok(SearchData {
        results,
        truncated,
        status,
        evidence: Some(
            json!({"protocol":"codex_direct_search","request_id":request_id,
        "commands":1,"server_output_token_budget":MAX_SEARCH_OUTPUT_TOKENS,"sources":sources,
        "rejected_results":rejected.into_iter().map(|(index,failure)|json!({"index":index,"failure":failure})).collect::<Vec<_>>()}),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(url: &str) -> Value {
        json!({"type":"text_result","ref_id":"turn1search0","url":url,"title":"Observed source","snippet":"Actual search snippet"})
    }
    fn response(items: Vec<Value>) -> Value {
        json!({"results":items,"output":"Search results","encrypted_output":"opaque"})
    }

    #[test]
    fn malformed_or_missing_source_identity_never_becomes_a_healthy_empty_result() {
        for value in [
            json!({"output":"No evidence"}),
            json!({"results":[]}),
            response(vec![json!({"type":"text_result"})]),
            response(vec![source("file:///secret")]),
        ] {
            assert!(parse(&value, &SearchQuery::default(), 5, "request").is_err());
        }
        assert_eq!(
            parse(&response(vec![]), &SearchQuery::default(), 5, "request")
                .unwrap()
                .status,
            SearchStatus::Empty
        );
    }

    #[test]
    fn url_filters_are_rechecked_locally_and_upstream_evidence_is_preserved() {
        let query = SearchQuery {
            domains: vec!["docs.rs".into()],
            exclude_domains: vec!["private.docs.rs".into()],
            ..Default::default()
        };
        let value = response(vec![
            source("https://docs.rs/a"),
            source("https://private.docs.rs/secret"),
            source("https://unrelated.example/x"),
        ]);
        let data = parse(&value, &query, 5, "request").unwrap();
        assert_eq!(data.results.len(), 1);
        assert_eq!(data.results[0].url, "https://docs.rs/a");
        assert_eq!(
            data.evidence.unwrap()["sources"].as_array().unwrap().len(),
            3
        );
        let filtered = parse(
            &response(vec![source("https://unrelated.example/x")]),
            &query,
            5,
            "request",
        )
        .unwrap();
        assert_eq!(filtered.status, SearchStatus::Filtered);
    }

    #[test]
    fn malformed_sources_are_explicit_partial_results_without_discarding_valid_sources() {
        let mut missing_title = source("https://docs.rs/rejected");
        missing_title["title"] = json!(null);
        let value = response(vec![
            missing_title,
            source("https://docs.rs/valid"),
            source("file:///private-source"),
        ]);
        let data = parse(&value, &SearchQuery::default(), 5, "request").unwrap();
        assert_eq!(data.status, SearchStatus::Partial);
        assert_eq!(data.results.len(), 1);
        assert_eq!(data.results[0].url, "https://docs.rs/valid");
        assert!(!data.truncated);
        let evidence = data.evidence.unwrap();
        assert_eq!(evidence["sources"].as_array().unwrap().len(), 1);
        let rejected = evidence["rejected_results"].as_array().unwrap();
        assert_eq!(rejected.len(), 2);
        assert_eq!(rejected[0]["index"], 0);
        assert_eq!(rejected[0]["failure"]["code"], "invalid_response");
        assert!(rejected[0]["failure"]["message"]
            .as_str()
            .unwrap()
            .contains("results[0].title"));
        assert!(!evidence.to_string().contains("private-source"));
    }

    #[test]
    fn invalid_sources_cannot_become_healthy_filtered_results() {
        let query = SearchQuery {
            domains: vec!["docs.rs".into()],
            ..Default::default()
        };
        let data = parse(
            &response(vec![
                source("https://other.example/valid"),
                json!({"type":"text_result"}),
            ]),
            &query,
            5,
            "request",
        )
        .unwrap();
        assert_eq!(data.status, SearchStatus::Partial);
        assert!(data.results.is_empty());
        let failure = parse(
            &response(vec![json!({"type":"text_result"})]),
            &query,
            5,
            "request",
        )
        .err()
        .unwrap();
        assert_eq!(failure.code, SearchErrorCode::InvalidResponse);
        assert!(failure.message.contains("results[0].url"));
    }
}
