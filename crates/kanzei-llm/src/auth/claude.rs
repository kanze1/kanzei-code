//! Claude Code 订阅凭证:复用 Claude Code 的登录态(~/.claude/.credentials.json)。
//! 不发起 OAuth 授权流程；只在令牌临近过期时使用 refresh_token 自动刷新。

use serde_json::{json, Value};

use crate::error::LlmError;
use crate::proxy::{build_http_client, ProxyConfig};

const CREDENTIALS_FILE: &str = ".claude/.credentials.json";
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const REFRESH_BEFORE_MS: i64 = 5 * 60 * 1000;
const OAUTH_BETA: &str = "oauth-2025-04-20";

/// 组装 Claude Code OAuth 调用 Anthropic Messages API 所需的请求头。
/// 优先级:CLAUDE_CODE_OAUTH_TOKEN 环境变量(`claude setup-token` 生成的长效令牌,
/// 新版 Claude Code 的推荐方式)→ ~/.claude/.credentials.json(旧版登录态,可刷新)。
pub async fn claude_headers(proxy: &ProxyConfig) -> Result<Vec<(String, String)>, LlmError> {
    if let Ok(token) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(bearer_headers(&token));
        }
    }
    let path = dirs::home_dir()
        .ok_or_else(|| LlmError::Config("cannot locate home dir".into()))?
        .join(CREDENTIALS_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        LlmError::Config(format!(
            "无法读取 {}({e}),且未设置 CLAUDE_CODE_OAUTH_TOKEN。\
             运行 `claude setup-token` 生成长效令牌并设入该环境变量。",
            path.display()
        ))
    })?;
    let mut credentials: Value = serde_json::from_str(&text)
        .map_err(|e| LlmError::Config(format!("{} 解析失败: {e}", path.display())))?;
    refresh_if_needed(&mut credentials, &path, proxy).await?;
    headers_from_credentials(&credentials, &path)
}

async fn refresh_if_needed(
    credentials: &mut Value,
    path: &std::path::Path,
    proxy: &ProxyConfig,
) -> Result<(), LlmError> {
    let oauth = &credentials["claudeAiOauth"];
    let expires_at = oauth["expiresAt"].as_i64().unwrap_or_default();
    if expires_at > chrono::Utc::now().timestamp_millis() + REFRESH_BEFORE_MS {
        return Ok(());
    }
    let refresh_token = oauth["refreshToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            LlmError::Config(format!(
                "{} 凭证已过期且无 refreshToken(新版 Claude Code 可能已不维护此文件)。\
                 运行 `claude setup-token` 生成长效令牌,设置环境变量 CLAUDE_CODE_OAUTH_TOKEN 后重试。",
                path.display()
            ))
        })?;

    let client = build_http_client(proxy)?;
    let response = client
        .post(TOKEN_URL)
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": CLIENT_ID,
        }))
        .send()
        .await
        .map_err(LlmError::Transport)?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(LlmError::Config(format!(
            "Claude OAuth 令牌刷新失败(HTTP {status}): {}。请重新登录。",
            body["error"].as_str().unwrap_or("unknown")
        )));
    }
    let access_token = body["access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| LlmError::Config("Claude OAuth 刷新响应缺少 access_token".into()))?;
    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    credentials["claudeAiOauth"]["accessToken"] = json!(access_token);
    if let Some(token) = body["refresh_token"].as_str().filter(|token| !token.is_empty()) {
        credentials["claudeAiOauth"]["refreshToken"] = json!(token);
    }
    credentials["claudeAiOauth"]["expiresAt"] =
        json!(chrono::Utc::now().timestamp_millis() + expires_in * 1000);
    let serialized = serde_json::to_string_pretty(credentials)
        .map_err(|e| LlmError::Config(format!("serialize {}: {e}", path.display())))?;
    std::fs::write(path, serialized)
        .map_err(|e| LlmError::Config(format!("write {}: {e}", path.display())))?;
    tracing::info!("claude OAuth token refreshed");
    Ok(())
}

fn headers_from_credentials(
    credentials: &Value,
    path: &std::path::Path,
) -> Result<Vec<(String, String)>, LlmError> {
    let oauth = &credentials["claudeAiOauth"];
    let access_token = oauth["accessToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| LlmError::Config(format!("{} 缺少 claudeAiOauth.accessToken，请重新登录", path.display())))?;
    if let Some(expires_at) = oauth["expiresAt"].as_i64() {
        if expires_at <= chrono::Utc::now().timestamp_millis() {
            return Err(LlmError::Config(format!(
                "{} 中的 Claude OAuth 凭证已过期，请先重新登录",
                path.display()
            )));
        }
    }

    Ok(bearer_headers(access_token))
}

fn bearer_headers(access_token: &str) -> Vec<(String, String)> {
    vec![
        ("authorization".into(), format!("Bearer {access_token}")),
        ("anthropic-version".into(), "2023-06-01".into()),
        ("anthropic-beta".into(), OAUTH_BETA.into()),
        ("x-app".into(), "cli".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::headers_from_credentials;
    use serde_json::json;

    #[test]
    fn 解析_claude_oauth_headers() {
        let credentials = json!({
            "claudeAiOauth": {
                "accessToken": "access-token",
                "expiresAt": 4_000_000_000_000i64
            }
        });
        let headers = headers_from_credentials(&credentials, std::path::Path::new("credentials"))
            .unwrap();
        assert_eq!(headers[0], ("authorization".into(), "Bearer access-token".into()));
        assert!(!headers.iter().any(|(name, _)| name == "x-api-key"));
    }

    #[test]
    fn 缺少_access_token_返回配置错误() {
        let credentials = json!({ "claudeAiOauth": {} });
        let error = headers_from_credentials(&credentials, std::path::Path::new("credentials"))
            .unwrap_err();
        assert!(error.to_string().contains("accessToken"));
    }
}
