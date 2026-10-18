//! Query filters are validated before charging a search round or sending requests.
use super::SearchQuery;

// DuckDuckGo's documented region codes, checked 2026-10-18:
// https://duckduckgo.com/duckduckgo-help-pages/settings/params
pub const REGIONS: &[&str] = &[
    "xa-ar", "xa-en", "ar-es", "au-en", "at-de", "be-fr", "be-nl", "br-pt", "bg-bg", "ca-en",
    "ca-fr", "ct-ca", "cl-es", "cn-zh", "co-es", "hr-hr", "cz-cs", "dk-da", "ee-et", "fi-fi",
    "fr-fr", "de-de", "gr-el", "hk-tzh", "hu-hu", "in-en", "id-id", "id-en", "ie-en", "il-he",
    "it-it", "jp-jp", "kr-kr", "lv-lv", "lt-lt", "xl-es", "my-ms", "my-en", "mx-es", "nl-nl",
    "nz-en", "no-no", "pe-es", "ph-en", "ph-tl", "pl-pl", "pt-pt", "ro-ro", "ru-ru", "sg-en",
    "sk-sk", "sl-sl", "za-en", "es-es", "se-sv", "ch-de", "ch-fr", "ch-it", "tw-tzh", "th-th",
    "tr-tr", "ua-uk", "uk-en", "us-en", "ue-es", "ve-es", "vn-vi", "wt-wt",
];

pub fn host_matches(host: &str, domain: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn domains(values: &mut Vec<String>) -> Result<(), String> {
    if values.len() > 10 {
        return Err("at most 10 domains are allowed in each filter".into());
    }
    let mut normalized = Vec::new();
    for value in values.iter() {
        let value = super::normalize_domain(value)?;
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }
    *values = normalized;
    Ok(())
}

pub fn normalize(query: &mut SearchQuery) -> Result<(), String> {
    domains(&mut query.domains)?;
    domains(&mut query.exclude_domains)?;
    if !query.domains.is_empty()
        && query.domains.iter().all(|domain| {
            query
                .exclude_domains
                .iter()
                .any(|excluded| host_matches(domain, excluded))
        })
    {
        return Err(
            "all allowed domains are excluded; correct the filters before searching".into(),
        );
    }
    if let Some(region) = &mut query.locale {
        *region = region.trim().to_ascii_lowercase();
        if !REGIONS.contains(&region.as_str()) {
            return Err("locale must be a documented DuckDuckGo region code, e.g. cn-zh, us-en, hk-tzh or wt-wt".into());
        }
    }
    Ok(())
}

pub fn duckduckgo_parameters(query: &SearchQuery) -> Vec<(&'static str, String)> {
    let mut terms = query.q.clone();
    // A union of site operators is not documented. Multiple allowed domains are
    // enforced on results rather than inventing a provider-side Boolean dialect.
    if let [domain] = query.domains.as_slice() {
        terms.push_str(&format!(" site:{domain}"));
    }
    for domain in &query.exclude_domains {
        terms.push_str(&format!(" -site:{domain}"));
    }
    let mut parameters = vec![("q", terms)];
    if let Some(recency) = query.recency {
        parameters.push(("df", recency.duckduckgo_filter().into()));
    }
    if let Some(locale) = &query.locale {
        parameters.push(("kl", locale.clone()));
    }
    parameters
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regions_domain_exclusions_and_conflicts_are_checked_without_losing_query_text() {
        let query:SearchQuery=serde_json::from_value(serde_json::json!({"q":"CodeBuddy advisory","domains":["Docs.RS.","docs.rs"],"exclude_domains":["private.docs.rs"],"locale":" HK-TZH ","recency":"week"})).unwrap();
        let (queries, _) = super::super::normalize_search_queries(None, Some(vec![query])).unwrap();
        let query = &queries[0];
        assert_eq!(query.q, "CodeBuddy advisory");
        assert_eq!(query.domains, vec!["docs.rs"]);
        assert_eq!(query.locale.as_deref(), Some("hk-tzh"));
        let parameters = duckduckgo_parameters(query);
        assert!(parameters.contains(&("kl", "hk-tzh".into())));
        assert!(parameters.contains(&("df", "w".into())));
        assert!(parameters[0].1.contains("-site:private.docs.rs"));
        assert!(super::super::allowed_result(
            "https://docs.rs/a",
            &query.domains,
            &query.exclude_domains
        ));
        assert!(!super::super::allowed_result(
            "https://nested.private.docs.rs/a",
            &query.domains,
            &query.exclude_domains
        ));
        assert!(super::super::allowed_result(
            "https://docs.rs./a",
            &query.domains,
            &query.exclude_domains
        ));
        assert!(!super::super::allowed_result(
            "https://user:secret@docs.rs/a",
            &query.domains,
            &query.exclude_domains
        ));
        for value in [
            serde_json::json!({"q":"x","locale":"zz-zz"}),
            serde_json::json!({"q":"x","domains":["docs.rs"],"exclude_domains":["docs.rs"]}),
        ] {
            assert!(super::super::normalize_search_queries(
                None,
                Some(vec![serde_json::from_value(value).unwrap()])
            )
            .is_err());
        }
        assert!(serde_json::from_value::<SearchQuery>(
            serde_json::json!({"q":"x","local":"cn-zh"})
        )
        .is_err());
    }
}
