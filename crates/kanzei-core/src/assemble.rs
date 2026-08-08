//! Route 装配:ResolvedModel(kanzei.toml)→ 可调用的 Route。
//! CLI 与桌面端共用,避免两份 provider 分支逻辑。

use kanzei_harness::ResolvedModel;
use kanzei_llm::{ProxyConfig, Route};

pub async fn build_route(resolved: &ResolvedModel, proxy: &ProxyConfig) -> anyhow::Result<Route> {
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

    // 特殊认证优先(codex = 复用订阅登录态,含按需刷新)。
    if resolved.provider.auth.as_deref() == Some("codex") {
        let headers = kanzei_llm::auth::codex::codex_headers(proxy).await?;
        return Ok(Route::openai_responses_at(
            &resolved.provider.base_url,
            headers,
        ));
    }

    if resolved.provider.auth.as_deref() == Some("claude") {
        let headers = kanzei_llm::auth::claude::claude_headers(proxy).await?;
        return Ok(Route::anthropic_with_headers_at(
            &resolved.provider.base_url,
            headers,
        ));
    }

    match resolved.provider.protocol.as_str() {
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
