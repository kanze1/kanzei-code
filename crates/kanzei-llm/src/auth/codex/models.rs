//! Account-specific Codex model discovery. The endpoint is fixed so login
//! credentials cannot be redirected to a provider-configured host.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{proxy::build_http_client_without_redirect, LlmError, ProxyConfig};

// The catalog gates entries by Codex protocol compatibility, not Kanzei's
// package version. Keep a verified floor and adopt any newer version recorded
// by the user's local Codex client cache (~/.codex/models_cache.json).
const DEFAULT_CODEX_CATALOG_CLIENT_VERSION: &str = "0.171.0";
const CODEX_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";

#[derive(Deserialize)]
struct Catalog {
    models: Vec<Model>,
}

#[derive(Deserialize)]
struct CachedCatalogVersion {
    client_version: Option<String>,
}

#[derive(Deserialize)]
struct Model {
    slug: String,
    visibility: String,
    priority: i64,
}

fn parse_semver_triplet(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn resolve_catalog_client_version(cached_version: Option<&str>) -> String {
    let floor = parse_semver_triplet(DEFAULT_CODEX_CATALOG_CLIENT_VERSION)
        .expect("default Codex catalog client version must be valid semver");
    if let Some(candidate) = cached_version.map(str::trim) {
        if let Some(parsed) = parse_semver_triplet(candidate) {
            if parsed > floor {
                return candidate.to_string();
            }
        }
    }
    DEFAULT_CODEX_CATALOG_CLIENT_VERSION.to_string()
}

fn cached_client_version_at(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let parsed: CachedCatalogVersion = serde_json::from_str(&text).ok()?;
    parsed.client_version
}

fn current_catalog_client_version() -> String {
    let cached = dirs::home_dir()
        .map(|home| home.join(".codex").join("models_cache.json"))
        .and_then(|path| cached_client_version_at(&path));
    resolve_catalog_client_version(cached.as_deref())
}

pub async fn list_models(proxy: &ProxyConfig) -> Result<Vec<String>, LlmError> {
    let started = Instant::now();
    let client = build_http_client_without_redirect(proxy)?;
    let headers = super::codex_headers(proxy).await?;
    let remaining = super::ACCOUNT_REQUEST_TIMEOUT
        .saturating_sub(started.elapsed())
        .max(Duration::from_millis(1));
    let client_version = current_catalog_client_version();
    fetch_models_with_timeout(
        &client,
        CODEX_MODELS_URL,
        headers,
        &client_version,
        remaining,
    )
    .await
}

async fn fetch_models_with_timeout(
    client: &reqwest::Client,
    url: &str,
    headers: Vec<(String, String)>,
    client_version: &str,
    timeout: Duration,
) -> Result<Vec<String>, LlmError> {
    let mut request = client
        .get(url)
        .timeout(timeout)
        .query(&[("client_version", client_version)]);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(LlmError::Http {
            status: response.status().as_u16(),
            body: "Codex model catalog request failed".into(),
        });
    }
    let mut catalog: Catalog = response.json().await?;
    catalog.models.sort_by_key(|model| model.priority);
    let mut seen = std::collections::HashSet::new();
    let models: Vec<_> = catalog
        .models
        .into_iter()
        .filter(|model| model.visibility == "list" && !model.slug.trim().is_empty())
        .map(|model| model.slug)
        .filter(|slug| seen.insert(slug.clone()))
        .collect();
    if models.is_empty() {
        return Err(LlmError::Protocol(
            "Codex model catalog contains no visible models".into(),
        ));
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    async fn serve_once(
        status: &str,
        extra_headers: &str,
        body: &str,
        response_delay: Duration,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n{body}",
            body.len()
        );
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut reader = tokio::io::BufReader::new(&mut socket);
            let mut lines = Vec::new();
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).await.unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                lines.push(line.trim().to_string());
            }
            if !response_delay.is_zero() {
                tokio::time::sleep(response_delay).await;
            }
            let _ = socket.write_all(response.as_bytes()).await;
            lines
        });
        (url, server)
    }

    #[test]
    fn resolve_catalog_client_version_prefers_newer_local_semver_and_rejects_invalid() {
        assert_eq!(
            resolve_catalog_client_version(None),
            DEFAULT_CODEX_CATALOG_CLIENT_VERSION
        );
        assert_eq!(
            resolve_catalog_client_version(Some("0.163.0")),
            DEFAULT_CODEX_CATALOG_CLIENT_VERSION
        );
        assert_eq!(
            resolve_catalog_client_version(Some("not-a-version")),
            DEFAULT_CODEX_CATALOG_CLIENT_VERSION
        );
        assert_eq!(
            resolve_catalog_client_version(Some("0.171.0-beta")),
            DEFAULT_CODEX_CATALOG_CLIENT_VERSION
        );
        assert_eq!(resolve_catalog_client_version(Some("0.175.2")), "0.175.2");
        assert_eq!(resolve_catalog_client_version(Some(" 1.0.0 ")), "1.0.0");
    }

    #[tokio::test]
    async fn discovers_models_and_honors_visibility_and_priority() {
        let (url, server) = serve_once(
            "200 OK",
            "",
            r#"{"models":[
                {"slug":"gpt-6-sol","visibility":"list","priority":3},
                {"slug":"gpt-6.2-sol","visibility":"list","priority":0},
                {"slug":"gpt-6.1-sol","visibility":"list","priority":1},
                {"slug":"internal-review","visibility":"hide","priority":0},
                {"slug":"gpt-6.1-sol","visibility":"list","priority":2}
            ]}"#,
            Duration::ZERO,
        )
        .await;
        let client = build_http_client_without_redirect(&ProxyConfig::Disabled).unwrap();
        let models = fetch_models_with_timeout(
            &client,
            &url,
            vec![
                ("authorization".into(), "Bearer test-token".into()),
                ("chatgpt-account-id".into(), "test-account".into()),
            ],
            DEFAULT_CODEX_CATALOG_CLIENT_VERSION,
            super::super::ACCOUNT_REQUEST_TIMEOUT,
        )
        .await
        .unwrap();
        assert_eq!(models, ["gpt-6.2-sol", "gpt-6.1-sol", "gpt-6-sol"]);
        let request = server.await.unwrap();
        assert!(request[0].contains(&format!(
            "/models?client_version={DEFAULT_CODEX_CATALOG_CLIENT_VERSION}"
        )));
        assert!(request
            .iter()
            .any(|line| line == "authorization: Bearer test-token"));
        assert!(request
            .iter()
            .any(|line| line == "chatgpt-account-id: test-account"));
    }

    #[tokio::test]
    async fn fetch_models_enforces_request_timeout() {
        let (url, server) = serve_once(
            "200 OK",
            "",
            r#"{"models":[{"slug":"gpt-6.2-sol","visibility":"list","priority":1}]}"#,
            Duration::from_millis(400),
        )
        .await;
        let client = build_http_client_without_redirect(&ProxyConfig::Disabled).unwrap();
        let error = fetch_models_with_timeout(
            &client,
            &url,
            Vec::new(),
            DEFAULT_CODEX_CATALOG_CLIENT_VERSION,
            Duration::from_millis(80),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, LlmError::Transport(ref source) if source.is_timeout()));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn failures_and_empty_catalogs_are_not_successful_discovery() {
        for (status, body) in [
            ("401 Unauthorized", "{}"),
            ("200 OK", r#"{"models":[]}"#),
            ("200 OK", r#"{"unexpected":[]}"#),
            (
                "200 OK",
                r#"{"models":[{"slug":"hidden","visibility":"hide","priority":0}]}"#,
            ),
        ] {
            let (url, server) = serve_once(status, "", body, Duration::ZERO).await;
            let client = build_http_client_without_redirect(&ProxyConfig::Disabled).unwrap();
            assert!(fetch_models_with_timeout(
                &client,
                &url,
                Vec::new(),
                DEFAULT_CODEX_CATALOG_CLIENT_VERSION,
                super::super::ACCOUNT_REQUEST_TIMEOUT,
            )
            .await
            .is_err());
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn catalog_redirects_are_rejected() {
        let (target, target_server) =
            serve_once("200 OK", "", r#"{"models":[]}"#, Duration::ZERO).await;
        let (url, server) = serve_once(
            "302 Found",
            &format!("Location: {target}\r\n"),
            "",
            Duration::ZERO,
        )
        .await;
        let client = build_http_client_without_redirect(&ProxyConfig::Disabled).unwrap();
        let result = fetch_models_with_timeout(
            &client,
            &url,
            Vec::new(),
            DEFAULT_CODEX_CATALOG_CLIENT_VERSION,
            super::super::ACCOUNT_REQUEST_TIMEOUT,
        )
        .await;
        assert!(matches!(result, Err(LlmError::Http { status: 302, .. })));
        server.await.unwrap();
        assert!(!target_server.is_finished());
        target_server.abort();
    }

    #[tokio::test]
    #[ignore = "requires the user's Codex login and network; run explicitly"]
    async fn live_catalog_contains_latest_codex_models() {
        let models = list_models(&ProxyConfig::Env).await.unwrap();
        assert!(models.iter().any(|slug| slug == "gpt-6.2-sol"));
        assert!(models.iter().any(|slug| slug == "gpt-6.1-sol"));
        println!("visible Codex models: {}", models.join(", "));
    }
}
