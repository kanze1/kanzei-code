//! 模型列表 IPC(command 侧,纯搬迁自 run.rs,R-253 批0)。
//!
//! 独立理由:模型枚举/探测与运行编排零耦合——`models_list` 是「UI 顶栏模型下拉
//! 的数据源」,`push_ollama_models` 是 Ollama /api/tags 探测,`build_model_route`
//! 是供独立启动路径复用的 route 包装。它们既不经 `run_task` 也不碰事件循环,
//! 留在 run.rs 里只会让「运行主链路」文件继续膨胀(照 files_view.rs 模式)。

use std::{collections::HashSet, path::PathBuf};

use serde_json::json;

use kanzei_harness::config::{KanzeiConfig, ProviderConfig};

/// 探测 provider 的 /models 或 Ollama /api/tags 并收集模型清单(原 run.rs 1863)。
pub(crate) async fn push_ollama_models(
    items: &mut Vec<serde_json::Value>,
    name: &str,
    base_url: &str,
) {
    let tags_url = format!("{}/api/tags", base_url.trim_end_matches("/v1"));
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    else {
        return;
    };
    let Ok(resp) = client.get(&tags_url).send().await else {
        return;
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return;
    };
    for m in v["models"].as_array().unwrap_or(&Vec::new()) {
        if let Some(n) = m["name"].as_str() {
            items.push(json!({ "id": format!("{name}:{n}"), "label": format!("{name}:{n}") }));
        }
    }
}

#[allow(dead_code)] // 供独立启动路径复用；主运行链当前走 build_run_harness 后的统一 route。
pub(crate) async fn build_model_route(
    resolved: &kanzei_harness::config::ResolvedModel,
    proxy: &kanzei_llm::ProxyConfig,
) -> anyhow::Result<kanzei_llm::Route> {
    kanzei_core::build_route(resolved, proxy).await
}

/// 模型列表 Tauri command(原 run.rs 2295):返回顶栏模型下拉的候选项。
#[tauri::command]
pub(crate) async fn models_list(project_dir: Option<String>) -> Result<serde_json::Value, String> {
    let cwd = project_dir
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .ok_or("no working dir")?;
    let config = KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for role in ["primary", "fast"] {
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
    for (name, provider) in &config.providers {
        if provider.auth.as_deref() == Some("codex") {
            for model in [
                "gpt-6-astra",
                "gpt-6-sol",
                "gpt-6-luna",
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-5.6-luna",
            ] {
                items.push(
                    json!({ "id": format!("{name}:{model}"), "label": format!("{name}:{model}") }),
                );
            }
        }
    }

    let proxy = match config.proxy.as_deref() {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(custom) => kanzei_llm::ProxyConfig::Explicit(custom.to_string()),
    };
    // Provider endpoints are independent. Probe them concurrently so several slow/offline
    // providers do not add their six-second timeouts one after another.
    let probes = config.providers.iter().map(|(name, provider)| {
        probe_provider_models(name.clone(), provider.clone(), proxy.clone())
    });
    for discovered in futures::future::join_all(probes).await {
        items.extend(discovered);
    }

    let mut seen = HashSet::new();
    items.retain(|item| {
        item["id"]
            .as_str()
            .is_some_and(|id| seen.insert(id.to_string()))
    });
    Ok(json!(items))
}

async fn probe_provider_models(
    name: String,
    provider: ProviderConfig,
    proxy: kanzei_llm::ProxyConfig,
) -> Vec<serde_json::Value> {
    // Codex subscription uses its authentication-specific model catalog.
    if provider.auth.as_deref() == Some("codex") {
        return Vec::new();
    }

    if provider.base_url.contains("11434") {
        let mut items = Vec::new();
        push_ollama_models(&mut items, &name, &provider.base_url).await;
        return items;
    }

    let protocol = provider.effective_protocol(&name);
    let is_anthropic = protocol == "anthropic";
    if !is_anthropic
        && !matches!(
            protocol,
            "openai" | "openai-responses" | "deepseek-responses"
        )
    {
        return Vec::new();
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
        return Vec::new();
    }

    let base = provider.base_url.trim_end_matches('/');
    let url = if is_anthropic && !base.ends_with("/v1") {
        format!("{base}/v1/models")
    } else {
        format!("{base}/models")
    };
    let Ok(client) = kanzei_llm::proxy::build_http_client(&proxy) else {
        return Vec::new();
    };
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

    let Ok(response) = request.send().await else {
        return Vec::new();
    };
    let Ok(response) = response.error_for_status() else {
        return Vec::new();
    };
    let Ok(value) = response.json::<serde_json::Value>().await else {
        return Vec::new();
    };
    let models = value["data"]
        .as_array()
        .or_else(|| value["models"].as_array());
    models
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str())
        .map(|id| json!({ "id": format!("{name}:{id}"), "label": format!("{name}:{id}") }))
        .collect()
}
