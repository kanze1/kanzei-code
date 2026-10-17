//! Route 装配:ResolvedModel(kanzei.toml)→ 可调用的 Route。
//! CLI 与桌面端共用,避免两份 provider 分支逻辑。

use kanzei_harness::ResolvedModel;
use kanzei_llm::{ProxyConfig, Route};

pub async fn build_route(resolved: &ResolvedModel, proxy: &ProxyConfig) -> anyhow::Result<Route> {
    let endpoint = reqwest::Url::parse(&resolved.provider.base_url)?;
    anyhow::ensure!(
        endpoint.username().is_empty() && endpoint.password().is_none(),
        "provider endpoint must not contain URL credentials"
    );
    if resolved.provider.auth.as_deref() == Some("codex") {
        anyhow::ensure!(endpoint.scheme() == "https" && endpoint.host_str() == Some("chatgpt.com") && endpoint.port_or_known_default() == Some(443) && endpoint.path().trim_end_matches('/') == "/backend-api/codex", "Codex subscription credentials may only be sent to https://chatgpt.com/backend-api/codex");
    }
    // 直填 key 优先(设置页可直接填);否则读 api_key_env 指向的环境变量。
    let api_key = resolved
        .provider
        .api_key
        .clone()
        .filter(|k| !k.trim().is_empty())
        .or_else(|| {
            resolved
                .provider
                .api_key_env
                .as_deref()
                .and_then(|name| std::env::var(name).ok())
        });
    if api_key.is_some() {
        anyhow::ensure!(
            endpoint.scheme() == "https",
            "API credentials require an approved HTTPS provider endpoint"
        );
    }

    // 特殊认证优先(codex = 复用订阅登录态,含按需刷新)。
    if resolved.provider.auth.as_deref() == Some("codex") {
        let headers = kanzei_llm::auth::codex::codex_headers(proxy).await?;
        return Ok(
            Route::openai_responses_at(&resolved.provider.base_url, headers)
                .with_provider_identity(&resolved.provider_name, resolved.provider.auth.as_deref()),
        );
    }

    if resolved.provider.auth.as_deref() == Some("claude") && api_key.is_none() {
        anyhow::bail!(
            "Claude subscription login has been disabled. Configure an Anthropic API key for provider `{}`.",
            resolved.provider_name
        );
    }

    match resolved
        .provider
        .effective_protocol(&resolved.provider_name)
    {
        "anthropic" => {
            let key = api_key.ok_or_else(|| {
                // 带上实际解析到的模型:用户常常以为自己已经换了 provider,而配置没生效
                // (设置页表单没保存,或项目级 kanzei.toml 覆盖了全局)。只报 provider
                // 名的话,看不出"这跟我设的那个不是一回事"(D-157)。
                anyhow::anyhow!(
                    "provider `{}` 需要环境变量 {};本次解析到的模型是 `{}:{}`——若与你设置的不符,\
                     检查设置页是否已保存,以及项目级 .kanzei/kanzei.toml 是否覆盖了全局",
                    resolved.provider_name,
                    resolved
                        .provider
                        .api_key_env
                        .as_deref()
                        .unwrap_or("<api_key_env>"),
                    resolved.provider_name,
                    resolved.model,
                )
            })?;
            Ok(Route::anthropic_at(&resolved.provider.base_url, &key)
                .with_provider_identity(&resolved.provider_name, resolved.provider.auth.as_deref()))
        }
        "openai" => Ok(
            Route::openai_at(&resolved.provider.base_url, api_key.as_deref())
                .with_provider_identity(&resolved.provider_name, resolved.provider.auth.as_deref()),
        ),
        "openai-responses" => {
            let headers = api_key
                .map(|k| vec![("authorization".to_string(), format!("Bearer {k}"))])
                .unwrap_or_default();
            Ok(
                Route::openai_responses_at(&resolved.provider.base_url, headers)
                    .with_provider_identity(
                        &resolved.provider_name,
                        resolved.provider.auth.as_deref(),
                    ),
            )
        }
        "deepseek-responses" => {
            let key = api_key.ok_or_else(|| {
                anyhow::anyhow!(
                    "provider `{}` 需要 API key；设置页直填 api_key 或配置 api_key_env",
                    resolved.provider_name
                )
            })?;
            Ok(
                Route::deepseek_responses_at(&resolved.provider.base_url, &key)
                    .with_provider_identity(
                        &resolved.provider_name,
                        resolved.provider.auth.as_deref(),
                    ),
            )
        }
        other => anyhow::bail!(
            "unknown protocol `{other}` for provider `{}`",
            resolved.provider_name
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(url: &str, auth: Option<&str>, key: Option<&str>) -> ResolvedModel {
        ResolvedModel {
            provider_name: "fixture".into(),
            model: "fixture".into(),
            provider: kanzei_harness::config::ProviderConfig {
                protocol: "openai-responses".into(),
                base_url: url.into(),
                auth: auth.map(str::to_owned),
                api_key: key.map(str::to_owned),
                api_key_env: None,
                context_limit: None,
            },
        }
    }

    #[tokio::test]
    async fn credential_targets_are_validated_before_authentication() {
        for url in [
            "https://attacker.invalid/backend-api/codex",
            "http://chatgpt.com/backend-api/codex",
            "https://chatgpt.com.evil.invalid/backend-api/codex",
            "https://chatgpt.com:444/backend-api/codex",
            "https://chatgpt.com/unrelated",
        ] {
            let error = build_route(&model(url, Some("codex"), None), &ProxyConfig::Disabled)
                .await
                .err()
                .unwrap()
                .to_string();
            assert!(
                error.contains("Codex subscription credentials may only"),
                "{error}"
            );
        }
        let error = build_route(
            &model("http://127.0.0.1:1/v1", None, Some("private-test-key")),
            &ProxyConfig::Disabled,
        )
        .await
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("HTTPS"));
        assert!(!error.contains("private-test-key"));
        assert!(build_route(
            &model(
                "https://approved.example/v1",
                None,
                Some("private-test-key")
            ),
            &ProxyConfig::Disabled
        )
        .await
        .is_ok());
        assert!(build_route(
            &model("http://127.0.0.1:1/v1", None, None),
            &ProxyConfig::Disabled
        )
        .await
        .is_ok());
    }
}
