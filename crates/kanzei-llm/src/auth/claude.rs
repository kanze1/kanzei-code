//! Claude Code 订阅凭证:复用 Claude Code 的登录态(~/.claude/.credentials.json)。
//! 不发起 OAuth 授权流程；凭证过期时提示用户先用 Claude Code 重新登录。

use serde_json::Value;

use crate::error::LlmError;

const CREDENTIALS_FILE: &str = ".claude/.credentials.json";
const OAUTH_BETA: &str = "oauth-2025-04-20";

/// 组装 Claude Code OAuth 调用 Anthropic Messages API 所需的请求头。
pub fn claude_headers() -> Result<Vec<(String, String)>, LlmError> {
    let path = dirs::home_dir()
        .ok_or_else(|| LlmError::Config("cannot locate home dir".into()))?
        .join(CREDENTIALS_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        LlmError::Config(format!(
            "无法读取 {}({e})。先运行 Claude Code 登录。",
            path.display()
        ))
    })?;
    let credentials: Value = serde_json::from_str(&text)
        .map_err(|e| LlmError::Config(format!("{} 解析失败: {e}", path.display())))?;
    headers_from_credentials(&credentials, &path)
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
        let now = chrono::Utc::now().timestamp_millis();
        if expires_at <= now {
            return Err(LlmError::Config(format!(
                "{} 中的 Claude OAuth 凭证已过期，请先重新登录",
                path.display()
            )));
        }
    }

    Ok(vec![
        ("authorization".into(), format!("Bearer {access_token}")),
        ("anthropic-version".into(), "2023-06-01".into()),
        ("anthropic-beta".into(), OAUTH_BETA.into()),
        ("x-app".into(), "cli".into()),
    ])
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
