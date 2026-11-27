//! 模型列表 IPC(command 侧,纯搬迁自 run.rs,R-253 批0)。
//!
//! 独立理由:模型枚举/探测与运行编排零耦合——`models_list` 是「UI 顶栏模型下拉
//! 的数据源」,`push_ollama_models` 是 Ollama /api/tags 探测,`build_model_route`
//! 是供独立启动路径复用的 route 包装。它们既不经 `run_task` 也不碰事件循环,
//! 留在 run.rs 里只会让「运行主链路」文件继续膨胀(照 files_view.rs 模式)。

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use serde_json::json;

use kanzei_harness::config::{KanzeiConfig, ProviderConfig};

const MODELS_CACHE_TTL: Duration = Duration::from_secs(30);
static MODELS_CACHE: LazyLock<Mutex<HashMap<String, (Instant, serde_json::Value)>>> =
    LazyLock::new(Mutex::default);

fn config_fingerprint(config: &KanzeiConfig) -> String {
    let snapshot = serde_json::to_value(config).unwrap_or(serde_json::Value::Null);
    kanzei_core::stable_json_hash(&snapshot)
}

/// 探测 provider 的 /models 或 Ollama /api/tags 并收集模型清单(原 run.rs 1863)。
pub(crate) async fn push_ollama_models(
    items: &mut Vec<serde_json::Value>,
    name: &str,
    base_url: &str,
) -> anyhow::Result<()> {
    let tags_url = format!("{}/api/tags", base_url.trim_end_matches("/v1"));
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    let v = client
        .get(&tags_url)
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;
    let models = v["models"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Ollama response is missing the models array"))?;
    for m in models {
        if let Some(n) = m["name"].as_str() {
            items.push(json!({ "id": format!("{name}:{n}"), "label": format!("{name}:{n}") }));
        }
    }
    Ok(())
}

#[allow(dead_code)] // 供独立启动路径复用；主运行链当前走 build_run_harness 后的统一 route。
pub(crate) async fn build_model_route(
    resolved: &kanzei_harness::config::ResolvedModel,
    proxy: &kanzei_llm::ProxyConfig,
) -> anyhow::Result<kanzei_llm::Route> {
    kanzei_core::build_route(resolved, proxy).await
}

/// 模型列表 Tauri command(原 run.rs 2295):返回模型下拉候选项。
/// 按配置指纹缓存 30 秒，减少切项目与频繁开关面板导致的重复网络探测；传 `force: true` 强制刷新。
#[tauri::command]
pub(crate) async fn models_list(
    project_dir: Option<String>,
    force: Option<bool>,
) -> Result<serde_json::Value, String> {
    let config = if let Some(project_dir) = project_dir {
        KanzeiConfig::load_at_root(&crate::normalized_project_root(Path::new(&project_dir)))
    } else {
        let cwd = std::env::current_dir().map_err(|_| "no working dir")?;
        KanzeiConfig::load(&cwd)
    }
    .map_err(|e| e.to_string())?;
    let fingerprint = config_fingerprint(&config);
    if !force.unwrap_or(false) {
        if let Ok(cache) = MODELS_CACHE.lock() {
            if let Some((at, value)) = cache.get(&fingerprint) {
                if at.elapsed() < MODELS_CACHE_TTL {
                    return Ok(value.clone());
                }
            }
        }
    }
    let discovered = discover_models(&config).await;
    if let Ok(mut cache) = MODELS_CACHE.lock() {
        cache.retain(|_, (at, _)| at.elapsed() < MODELS_CACHE_TTL);
        cache.insert(fingerprint, (Instant::now(), discovered.clone()));
    }
    Ok(discovered)
}

async fn discover_models(config: &KanzeiConfig) -> serde_json::Value {
    let mut items = Vec::new();
    for role in ["primary", "fast", "compact"] {
        if let Ok(resolved) = config.resolve_model(role) {
            let direct = format!("{}:{}", resolved.provider_name, resolved.model);
            items.push(json!({ "id": role, "label": format!("{role} → {direct}") }));
            // 角色项用于显示配置来源，直指项用于顶栏按进程选择。即使 provider 的
            // /models 探测失败，当前配置的实际模型也不能从下拉里消失(例如 DeepSeek)。
            if !items.iter().any(|item| item["id"] == direct) {
                items.push(json!({ "id": direct, "label": direct }));
            }
        }
    }
    let proxy = proxy_config(config.proxy.as_deref());
    // Provider endpoints are independent. Probe them concurrently so their
    // request deadlines do not add up.
    let probes = config.providers.iter().map(|(name, provider)| {
        let proxy = proxy.clone();
        async move {
            let result = probe_provider_models(name.clone(), provider.clone(), proxy).await;
            (name, result.map_err(|error| error.to_string()))
        }
    });
    let mut errors = Vec::new();
    for (name, result) in futures::future::join_all(probes).await {
        match result {
            Ok(discovered) => items.extend(discovered),
            Err(message) => errors.push(json!({ "provider": name, "message": message })),
        }
    }

    let mut seen = HashSet::new();
    items.retain(|item| {
        item["id"]
            .as_str()
            .is_some_and(|id| seen.insert(id.to_string()))
    });
    json!({ "models": items, "errors": errors })
}

pub(crate) fn proxy_config(value: Option<&str>) -> kanzei_llm::ProxyConfig {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(custom) => kanzei_llm::ProxyConfig::Explicit(custom.to_string()),
    }
}

pub(crate) fn format_codex_catalog_error(error: &kanzei_llm::LlmError) -> String {
    match error {
        kanzei_llm::LlmError::Transport(source) if source.is_timeout() => {
            format!("Codex 网络请求超时，请检查当前网络和代理设置：{error}")
        }
        kanzei_llm::LlmError::Transport(source) if source.is_connect() => {
            format!("无法连接 Codex 服务，请检查当前网络和代理设置：{error}")
        }
        kanzei_llm::LlmError::Http {
            status: 401 | 403, ..
        } => {
            format!("Codex 订阅授权未通过，请检查 Codex CLI 登录：{error}")
        }
        _ => error.to_string(),
    }
}

pub(crate) async fn codex_catalog(proxy: &kanzei_llm::ProxyConfig) -> Result<Vec<String>, String> {
    kanzei_llm::auth::codex::models::list_models(proxy)
        .await
        .map_err(|error| format_codex_catalog_error(&error))
}

async fn probe_provider_models(
    name: String,
    provider: ProviderConfig,
    proxy: kanzei_llm::ProxyConfig,
) -> anyhow::Result<Vec<serde_json::Value>> {
    if provider.auth.as_deref() == Some("codex") {
        return codex_catalog(&proxy)
            .await
            .map(|models| model_items(&name, models))
            .map_err(anyhow::Error::msg);
    }

    if provider.base_url.contains("11434") {
        let mut items = Vec::new();
        push_ollama_models(&mut items, &name, &provider.base_url).await?;
        return Ok(items);
    }

    let protocol = provider.effective_protocol(&name);
    let is_anthropic = protocol == "anthropic";
    if !is_anthropic
        && !matches!(
            protocol,
            "openai" | "openai-responses" | "deepseek-responses"
        )
    {
        return Ok(Vec::new());
    }

    let key = provider
        .api_key
        .clone()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| {
            provider
                .api_key_env
                .as_deref()
                .and_then(|env| std::env::var(env).ok())
        });
    if is_anthropic && key.is_none() {
        return Ok(Vec::new());
    }

    let base = provider.base_url.trim_end_matches('/');
    let url = if is_anthropic && !base.ends_with("/v1") {
        format!("{base}/v1/models")
    } else {
        format!("{base}/models")
    };
    anyhow::ensure!(
        key.is_none() || reqwest::Url::parse(&url)?.scheme() == "https",
        "API credentials require HTTPS"
    );
    let client = kanzei_llm::proxy::build_http_client_without_redirect(&proxy)?;
    let mut request = client.get(&url).timeout(std::time::Duration::from_secs(6));
    if is_anthropic {
        if let Some(key) = &key {
            request = request
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01");
        }
    } else if let Some(key) = &key {
        request = request.bearer_auth(key);
    }

    let response = request.send().await?.error_for_status()?;
    anyhow::ensure!(
        response.status().is_success(),
        "model discovery returned HTTP {}",
        response.status()
    );
    let value = response.json::<serde_json::Value>().await?;
    let models = value["data"]
        .as_array()
        .or_else(|| value["models"].as_array())
        .ok_or_else(|| anyhow::anyhow!("provider response is missing the model catalog array"))?;
    Ok(model_items(
        &name,
        models
            .iter()
            .filter_map(|model| model["id"].as_str())
            .map(str::to_string),
    ))
}

fn model_items(name: &str, models: impl IntoIterator<Item = String>) -> Vec<serde_json::Value> {
    models
        .into_iter()
        .map(|model| {
            let id = format!("{name}:{model}");
            json!({ "id": id, "label": id })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    #[tokio::test]
    #[ignore = "requires Codex CLI login and the user's network; run explicitly"]
    async fn live_codex_catalog_uses_account_request_deadline() {
        let started = Instant::now();
        let models = codex_catalog(&kanzei_llm::ProxyConfig::Env).await.unwrap();
        assert!(!models.is_empty());
        println!(
            "{} Codex models after {:.2?}",
            models.len(),
            started.elapsed()
        );
    }

    #[test]
    fn proxy_config_trims_and_normalizes_empty_values() {
        assert_eq!(proxy_config(None), kanzei_llm::ProxyConfig::Env);
        assert_eq!(proxy_config(Some("")), kanzei_llm::ProxyConfig::Env);
        assert_eq!(proxy_config(Some("   ")), kanzei_llm::ProxyConfig::Env);
        assert_eq!(proxy_config(Some(" env ")), kanzei_llm::ProxyConfig::Env);
        assert_eq!(
            proxy_config(Some(" off ")),
            kanzei_llm::ProxyConfig::Disabled
        );
        assert_eq!(
            proxy_config(Some(" http://127.0.0.1:7890 ")),
            kanzei_llm::ProxyConfig::Explicit("http://127.0.0.1:7890".into())
        );
    }

    #[test]
    fn format_codex_catalog_error_classifies_auth_and_other_failures() {
        let auth_err = format_codex_catalog_error(&kanzei_llm::LlmError::Http {
            status: 401,
            body: "unauthorized".into(),
        });
        assert!(auth_err.contains("Codex 订阅授权未通过"));
        let forbidden_err = format_codex_catalog_error(&kanzei_llm::LlmError::Http {
            status: 403,
            body: "forbidden".into(),
        });
        assert!(forbidden_err.contains("Codex 订阅授权未通过"));
        let proto_err = format_codex_catalog_error(&kanzei_llm::LlmError::Protocol(
            "Codex model catalog contains no visible models".into(),
        ));
        assert!(proto_err.contains("no visible models"));
    }

    #[tokio::test]
    async fn discovery_preserves_configured_models_and_reports_partial_failure() {
        let mut config: KanzeiConfig = toml::from_str(
            "proxy = 'off'\n[models]\nprimary = 'broken:saved-model'\ncompact = 'broken:saved-compact'\n",
        )
        .unwrap();
        let mut servers = Vec::new();
        for (name, status, body) in [
            ("broken", "401 Unauthorized", "{}"),
            (
                "working",
                "200 OK",
                r#"{"data":[{"id":"discovered-model"}]}"#,
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base_url = format!("http://{}", listener.local_addr().unwrap());
            config.providers.insert(
                name.into(),
                serde_json::from_value(json!({ "base_url": base_url, "protocol": "openai" }))
                    .unwrap(),
            );
            servers.push(tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut reader = tokio::io::BufReader::new(&mut socket);
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).await.unwrap() > 0);
                    if line == "\r\n" { break; }
                }
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }));
        }
        let result = discover_models(&config).await;
        let ids: Vec<_> = result["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["id"].as_str())
            .collect();
        assert!(ids.contains(&"broken:saved-model"));
        assert!(ids.contains(&"broken:saved-compact"));
        assert!(ids.contains(&"working:discovered-model"));
        assert_eq!(result["errors"].as_array().unwrap().len(), 1);
        assert_eq!(result["errors"][0]["provider"], "broken");
        assert!(result["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("401"));
        for server in servers {
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn discovery_does_not_send_api_credentials_over_http() {
        let provider = serde_json::from_value(json!({ "protocol": "openai", "base_url": "http://127.0.0.1:1", "api_key": "test-key" })).unwrap();
        let error =
            probe_provider_models("unsafe".into(), provider, kanzei_llm::ProxyConfig::Disabled)
                .await
                .unwrap_err();
        assert!(error.to_string().contains("HTTPS"));
    }
}
