//! Account-specific Codex model discovery. The endpoint is fixed so login
//! credentials cannot be redirected to a provider-configured host.

use serde::Deserialize;

use crate::{proxy::build_http_client_without_redirect, LlmError, ProxyConfig};

// The catalog gates entries by Codex protocol compatibility, not Kanzei's
// package version. 0.163.0 is the catalog contract verified with Codex CLI.
const CODEX_CATALOG_CLIENT_VERSION: &str = "0.163.0";
const CODEX_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";

#[derive(Deserialize)]
struct Catalog {
    models: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    slug: String,
    visibility: String,
    priority: i64,
}

pub async fn list_models(proxy: &ProxyConfig) -> Result<Vec<String>, LlmError> {
    let client = build_http_client_without_redirect(proxy)?;
    let headers = super::codex_headers(proxy).await?;
    fetch_models(&client, CODEX_MODELS_URL, headers).await
}

async fn fetch_models(
    client: &reqwest::Client,
    url: &str,
    headers: Vec<(String, String)>,
) -> Result<Vec<String>, LlmError> {
    let mut request = client
        .get(url)
        .query(&[("client_version", CODEX_CATALOG_CLIENT_VERSION)]);
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
            socket.write_all(response.as_bytes()).await.unwrap();
            lines
        });
        (url, server)
    }

    #[tokio::test]
    async fn discovers_new_models_and_honors_visibility_and_priority() {
        let (url, server) = serve_once(
            "200 OK",
            "",
            r#"{"models":[
                {"slug":"gpt-6-sol","visibility":"list","priority":3},
                {"slug":"future-model","visibility":"list","priority":0},
                {"slug":"gpt-6.1-sol","visibility":"list","priority":1},
                {"slug":"internal-review","visibility":"hide","priority":0},
                {"slug":"gpt-6.1-sol","visibility":"list","priority":2}
            ]}"#,
        )
        .await;
        let client = build_http_client_without_redirect(&ProxyConfig::Disabled).unwrap();
        let models = fetch_models(
            &client,
            &url,
            vec![
                ("authorization".into(), "Bearer test-token".into()),
                ("chatgpt-account-id".into(), "test-account".into()),
            ],
        )
        .await
        .unwrap();
        assert_eq!(models, ["future-model", "gpt-6.1-sol", "gpt-6-sol"]);
        let request = server.await.unwrap();
        assert!(request[0].contains("/models?client_version=0.163.0"));
        assert!(request
            .iter()
            .any(|line| line == "authorization: Bearer test-token"));
        assert!(request
            .iter()
            .any(|line| line == "chatgpt-account-id: test-account"));
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
            let (url, server) = serve_once(status, "", body).await;
            let client = build_http_client_without_redirect(&ProxyConfig::Disabled).unwrap();
            assert!(fetch_models(&client, &url, Vec::new()).await.is_err());
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn catalog_redirects_are_rejected() {
        let (target, target_server) = serve_once("200 OK", "", r#"{"models":[]}"#).await;
        let (url, server) = serve_once("302 Found", &format!("Location: {target}\r\n"), "").await;
        let client = build_http_client_without_redirect(&ProxyConfig::Disabled).unwrap();
        let result = fetch_models(&client, &url, Vec::new()).await;
        assert!(matches!(result, Err(LlmError::Http { status: 302, .. })));
        server.await.unwrap();
        assert!(!target_server.is_finished());
        target_server.abort();
    }

    #[tokio::test]
    #[ignore = "requires the user's Codex login and network; run explicitly"]
    async fn live_catalog_contains_gpt_6_1_sol() {
        let models = list_models(&ProxyConfig::Env).await.unwrap();
        assert!(models.iter().any(|slug| slug == "gpt-6.1-sol"));
        println!("visible Codex models: {}", models.join(", "));
    }
}
