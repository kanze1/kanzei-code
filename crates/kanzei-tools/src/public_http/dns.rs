//! Public DNS is a separate trust boundary from the OS resolver. A proxy/TUN
//! can return reserved Fake-IPs; accepting or pinning those would either bypass
//! SSRF checks or prevent every public download. Use one authenticated resolver,
//! pinned at bootstrap as well as at the eventual origin; never fall back to OS
//! DNS or ask an HTTP proxy to resolve an unchecked origin hostname.

use super::{pinned_request, validate_addresses, Response};
use kanzei_harness::config::PublicDns;
use kanzei_llm::ProxyConfig;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};

// Bounded DNS JSON documents, independently of the web/image body budget.
const MAX_DNS_BYTES: usize = 64 * 1024;
const A: u16 = 1;
const AAAA: u16 = 28;

pub(super) async fn resolve(
    host: &str,
    port: u16,
    resolver: &PublicDns,
    proxy: &ProxyConfig,
) -> Result<Vec<SocketAddr>, String> {
    let endpoint = reqwest::Url::parse(&resolver.endpoint)
        .map_err(|error| format!("PUBLIC_DNS_CONFIG: invalid endpoint: {error}"))?;
    if endpoint.scheme() != "https"
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(
            "PUBLIC_DNS_CONFIG: endpoint must be an HTTPS URL without credentials or fragment"
                .into(),
        );
    }
    let bootstrap = validate_addresses(&[SocketAddr::new(
        resolver.bootstrap,
        endpoint
            .port_or_known_default()
            .ok_or("PUBLIC_DNS_CONFIG: endpoint has no port")?,
    )])?;
    let query = |record_type| query(&endpoint, bootstrap, host, record_type, proxy);
    let (ipv4, ipv6) = tokio::try_join!(query(A), query(AAAA))?;
    let addresses: Vec<_> = ipv4
        .into_iter()
        .chain(ipv6)
        .map(|ip| SocketAddr::new(ip, port))
        .collect();
    // Every answer from both families must pass, including mapped IPv6. This is
    // deliberately before choosing a connection target, even when IPv4 exists.
    validate_addresses(&addresses)?;
    Ok(addresses)
}

async fn query(
    endpoint: &reqwest::Url,
    bootstrap: SocketAddr,
    host: &str,
    record_type: u16,
    proxy: &ProxyConfig,
) -> Result<Vec<IpAddr>, String> {
    let mut url = endpoint.clone();
    url.query_pairs_mut()
        .append_pair("name", host)
        .append_pair("type", &record_type.to_string());
    let response = pinned_request(
        &url,
        bootstrap,
        proxy,
        "application/dns-json",
        MAX_DNS_BYTES,
    )
    .await
    .map_err(|error| format!("PUBLIC_DNS_QUERY: {host} type {record_type}: {error}"))?;
    parse_response(response, host, record_type)
        .map_err(|error| format!("PUBLIC_DNS_RESPONSE: {host} type {record_type}: {error}"))
}

#[derive(Deserialize)]
struct DnsResponse {
    #[serde(rename = "Status")]
    status: u16,
    #[serde(rename = "TC")]
    truncated: bool,
    #[serde(rename = "Question")]
    question: Questions,
    #[serde(rename = "Answer", default)]
    answers: Vec<Record>,
}

// AliDNS returns one object; the Google/Cloudflare JSON schema uses an array.
#[derive(Deserialize)]
#[serde(untagged)]
enum Questions {
    One(Question),
    Many(Vec<Question>),
}

#[derive(Deserialize)]
struct Question {
    name: String,
    #[serde(rename = "type")]
    record_type: u16,
}

#[derive(Deserialize)]
struct Record {
    #[serde(rename = "type")]
    record_type: u16,
    data: String,
}

fn parse_response(response: Response, host: &str, record_type: u16) -> Result<Vec<IpAddr>, String> {
    if response.status != 200 || response.truncated {
        return Err(format!(
            "HTTP {} or oversized DNS document",
            response.status
        ));
    }
    let mime = response.content_type.split(';').next().unwrap_or("").trim();
    if !matches!(mime, "application/json" | "application/dns-json") {
        return Err(format!("expected DNS JSON, received {mime}"));
    }
    let document: DnsResponse = serde_json::from_slice(&response.body)
        .map_err(|error| format!("invalid DNS JSON: {error}"))?;
    if document.status != 0 || document.truncated {
        return Err(format!(
            "DNS status {} or truncated answer",
            document.status
        ));
    }
    let questions = match document.question {
        Questions::One(question) => vec![question],
        Questions::Many(questions) => questions,
    };
    if questions.len() != 1
        || questions[0].record_type != record_type
        || !questions[0]
            .name
            .trim_end_matches('.')
            .eq_ignore_ascii_case(host.trim_end_matches('.'))
    {
        return Err("DNS question does not match the requested name and type".into());
    }
    document
        .answers
        .into_iter()
        .filter(|record| matches!(record.record_type, A | AAAA))
        .map(|record| {
            let ip: IpAddr = record
                .data
                .parse()
                .map_err(|error| format!("invalid address answer: {error}"))?;
            if !matches!(
                (record.record_type, ip),
                (A, IpAddr::V4(_)) | (AAAA, IpAddr::V6(_))
            ) {
                return Err("DNS address family does not match its record type".into());
            }
            Ok(ip)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn response(document: Value) -> Response {
        Response {
            status: 200,
            content_type: "application/json; charset=UTF-8".into(),
            location: None,
            body: serde_json::to_vec(&document).unwrap(),
            truncated: false,
        }
    }

    fn answer() -> Value {
        json!({"Status":0,"TC":false,"Question":{"name":"www.bing.com.","type":1},
            "Answer":[{"type":5,"data":"origin.example."},{"type":1,"data":"8.8.8.8"}]})
    }

    #[test]
    fn cname_answers_and_both_question_formats_preserve_real_addresses() {
        let mut document = answer();
        assert_eq!(
            parse_response(response(document.clone()), "www.bing.com", A).unwrap(),
            vec!["8.8.8.8".parse::<IpAddr>().unwrap()]
        );
        document["Question"] = json!([document["Question"]]);
        assert!(parse_response(response(document), "WWW.BING.COM", A).is_ok());
    }

    #[test]
    fn malformed_failed_truncated_or_mismatched_dns_is_not_accepted() {
        for (key, value) in [
            ("Status", json!(2)),
            ("TC", json!(true)),
            ("Question", json!({"name":"other.example","type":1})),
            ("Question", json!({"name":"www.bing.com","type":28})),
            ("Question", json!([])),
            ("Answer", json!([{"type":1,"data":"not-an-address"}])),
            ("Answer", json!([{"type":1,"data":"::ffff:127.0.0.1"}])),
        ] {
            let mut document = answer();
            document[key] = value;
            assert!(
                parse_response(response(document), "www.bing.com", A).is_err(),
                "{key}"
            );
        }
        let mut redirect = response(answer());
        redirect.status = 302;
        assert!(parse_response(redirect, "www.bing.com", A).is_err());
        let mut oversized = response(answer());
        oversized.truncated = true;
        assert!(parse_response(oversized, "www.bing.com", A).is_err());
        let mut html = response(answer());
        html.content_type = "text/html".into();
        assert!(parse_response(html, "www.bing.com", A).is_err());
    }

    #[test]
    fn private_answers_in_either_family_still_block_the_entire_result() {
        for (record_type, address) in [
            (A, "198.18.0.30"),
            (A, "127.0.0.1"),
            (AAAA, "::ffff:127.0.0.1"),
            (AAAA, "fe80::1"),
        ] {
            let document = json!({"Status":0,"TC":false,
                "Question":{"name":"www.bing.com","type":record_type},
                "Answer":[{"type":record_type,"data":address}]});
            let addresses =
                parse_response(response(document), "www.bing.com", record_type).unwrap();
            let mut sockets = vec!["8.8.8.8:443".parse().unwrap()];
            sockets.extend(addresses.into_iter().map(|ip| SocketAddr::new(ip, 443)));
            assert!(validate_addresses(&sockets)
                .unwrap_err()
                .contains("WEBFETCH_ADDRESS_BLOCKED"));
        }
    }

    #[tokio::test]
    async fn invalid_or_private_bootstrap_is_rejected_before_network_access() {
        for (endpoint, bootstrap) in [
            ("http://dns.example/resolve", "8.8.8.8"),
            ("https://user:password@dns.example/resolve", "8.8.8.8"),
            ("https://dns.example/resolve#fragment", "8.8.8.8"),
            ("https://dns.example/resolve", "198.18.0.2"),
        ] {
            let dns = PublicDns {
                endpoint: endpoint.into(),
                bootstrap: bootstrap.parse().unwrap(),
            };
            assert!(resolve("www.bing.com", 443, &dns, &ProxyConfig::Disabled)
                .await
                .is_err());
        }
    }

    #[tokio::test]
    #[ignore = "requires access to the configured public DoH service and Bing over HTTPS"]
    async fn live_public_resolution_and_pinned_https_download() {
        let root = tempfile::tempdir().unwrap();
        let ctx = kanzei_harness::ToolCtx::new(root.path().into(), root.path().into());
        let addresses = resolve(
            "www.bing.com",
            443,
            &PublicDns::default(),
            &ProxyConfig::Env,
        )
        .await
        .unwrap();
        let os: Vec<_> = tokio::net::lookup_host(("www.bing.com", 443))
            .await
            .unwrap()
            .collect();
        println!("OS DNS: {os:?}; public DNS: {addresses:?}");
        assert!(addresses
            .iter()
            .all(|address| super::super::is_public(address.ip())));
        let url = reqwest::Url::parse("https://www.bing.com/").unwrap();
        let response = super::super::fetch(&url, &ctx, 1024 * 1024).await.unwrap();
        assert_eq!(response.status, 200);
        assert!(response.content_type.contains("text/html"));
        assert!(!response.body.is_empty());
        println!(
            "Downloaded {} original HTTPS bytes with validated public targets",
            response.body.len()
        );
    }
}
