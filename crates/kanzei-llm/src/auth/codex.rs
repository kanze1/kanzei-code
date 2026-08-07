//! Codex 订阅凭证:复用 Codex CLI 的登录态(~/.codex/auth.json)。
//! 只读+按需刷新写回,与 Codex CLI 完全兼容——不自己发起 OAuth 授权流程。

use serde_json::{json, Value};

use crate::error::LlmError;
use crate::proxy::{build_http_client, ProxyConfig};

/// access_token 寿命较长,codex CLI 的惯例是临近一个月才刷新。
const REFRESH_AFTER_DAYS: i64 = 25;
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";

/// 组装调用 chatgpt.com/backend-api/codex 所需的请求头(必要时先刷新令牌)。
pub async fn codex_headers(proxy: &ProxyConfig) -> Result<Vec<(String, String)>, LlmError> {
    let path = dirs::home_dir()
        .ok_or_else(|| LlmError::Config("cannot locate home dir".into()))?
        .join(".codex")
        .join("auth.json");
    let text = std::fs::read_to_string(&path).map_err(|e| {
        LlmError::Config(format!(
            "无法读取 {}({e})。先在终端运行 `codex login` 登录 Codex CLI。",
            path.display()
        ))
    })?;
    let mut auth: Value = serde_json::from_str(&text)
        .map_err(|e| LlmError::Config(format!("auth.json 解析失败: {e}")))?;

    let stale = auth["last_refresh"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| {
            chrono::Utc::now()
                .signed_duration_since(t.with_timezone(&chrono::Utc))
                .num_days()
                >= REFRESH_AFTER_DAYS
        })
        .unwrap_or(true);

    if stale {
        refresh(&mut auth, &path, proxy).await?;
    }

    let access = auth["tokens"]["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| LlmError::Config("auth.json 缺少 access_token,重新 `codex login`".into()))?;
    let account = auth["tokens"]["account_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| LlmError::Config("auth.json 缺少 account_id,重新 `codex login`".into()))?;

    Ok(vec![
        ("authorization".into(), format!("Bearer {access}")),
        ("chatgpt-account-id".into(), account.to_string()),
        ("openai-beta".into(), "responses=experimental".into()),
        ("originator".into(), "codex_cli_rs".into()),
        ("session_id".into(), pseudo_uuid()),
    ])
}

async fn refresh(
    auth: &mut Value,
    path: &std::path::Path,
    proxy: &ProxyConfig,
) -> Result<(), LlmError> {
    let refresh_token = auth["tokens"]["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| LlmError::Config("auth.json 缺少 refresh_token,重新 `codex login`".into()))?
        .to_string();
    let client = build_http_client(proxy)?;
    let response = client
        .post(TOKEN_URL)
        .json(&json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "scope": "openid profile email",
        }))
        .send()
        .await
        .map_err(LlmError::Transport)?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(LlmError::Config(format!(
            "Codex 令牌刷新失败(HTTP {status}): {}。重新 `codex login` 后再试。",
            body["error"].as_str().unwrap_or("unknown")
        )));
    }
    for key in ["access_token", "refresh_token", "id_token"] {
        if let Some(value) = body[key].as_str().filter(|s| !s.is_empty()) {
            auth["tokens"][key] = json!(value);
        }
    }
    auth["last_refresh"] = json!(chrono::Utc::now().to_rfc3339());
    // 原子替换 + 写前重读:刷新期间 Codex CLI 可能已抢先刷过并轮换了 refresh_token,
    // 此时采纳磁盘那份而不是覆盖回去(D-061)。commit 返回的才是最终生效的凭证。
    *auth = crate::auth::store::commit(path, auth, |disk, mine| {
        crate::auth::store::newer_by_rfc3339(disk, mine, "/last_refresh")
    })?;
    tracing::info!("codex token refreshed");
    Ok(())
}

/// uuid v4 形状的会话 ID(避免引入 uuid 依赖;不用于安全用途)。
fn pseudo_uuid() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mix = nanos ^ (pid << 96) ^ 0x9e37_79b9_7f4a_7c15_u128.wrapping_mul(pid + 1);
    let hex = format!("{mix:032x}");
    format!(
        "{}-{}-4{}-a{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}
