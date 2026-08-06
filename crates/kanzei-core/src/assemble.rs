//! Route 装配:ResolvedModel(kanzei.toml)→ 可调用的 Route。
//! CLI 与桌面端共用,避免两份 provider 分支逻辑。

use kanzei_harness::ResolvedModel;
use kanzei_llm::{ProxyConfig, Route};

pub async fn build_route(resolved: &ResolvedModel, proxy: &ProxyConfig) -> anyhow::Result<Route> {
    let api_key = resolved
        .provider
        .api_key_env
        .as_deref()
        .and_then(|name| std::env::var(name).ok());

    // 特殊认证优先(codex = 复用订阅登录态,含按需刷新)。
    if resolved.provider.auth.as_deref() == Some("codex") {
        let headers = kanzei_llm::auth::codex::codex_headers(proxy).await?;
        return Ok(Route::openai_responses_at(
            &resolved.provider.base_url,
            headers,
        ));
    }

    match resolved.provider.protocol.as_str() {
        "anthropic" => {
            let key = api_key.ok_or_else(|| {
                anyhow::anyhow!(
                    "provider `{}` 需要环境变量 {}",
                    resolved.provider_name,
                    resolved
                        .provider
                        .api_key_env
                        .as_deref()
                        .unwrap_or("<api_key_env>")
                )
            })?;
            Ok(Route::anthropic_at(&resolved.provider.base_url, &key))
        }
        "openai" => Ok(Route::openai_at(
            &resolved.provider.base_url,
            api_key.as_deref(),
        )),
        "openai-responses" => {
            let headers = api_key
                .map(|k| vec![("authorization".to_string(), format!("Bearer {k}"))])
                .unwrap_or_default();
            Ok(Route::openai_responses_at(
                &resolved.provider.base_url,
                headers,
            ))
        }
        other => anyhow::bail!(
            "unknown protocol `{other}` for provider `{}`",
            resolved.provider_name
        ),
    }
}
