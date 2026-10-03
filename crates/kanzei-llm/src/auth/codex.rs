//! Codex 订阅凭证:复用 Codex CLI 的登录态(~/.codex/auth.json)。
//! 只读+按需刷新写回,与 Codex CLI 完全兼容——不自己发起 OAuth 授权流程。

use serde_json::{json, Value};

use crate::error::LlmError;
use crate::proxy::{build_http_client, ProxyConfig};

/// access_token 寿命较长,codex CLI 的惯例是临近一个月才刷新。
const REFRESH_AFTER_DAYS: i64 = 25;
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";

/// 过期判定(R-297 提纯):last_refresh 距 now 达 REFRESH_AFTER_DAYS 天即需刷新。
/// now 由调用方注入——生产传真实时钟,测试传伪造时钟覆盖边界。
/// RFC3339 解析失败或字段缺失按过期处理(宁可多刷一次,不可带着过期令牌裸奔)。
fn is_stale(last_refresh: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> bool {
    last_refresh
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| {
            now.signed_duration_since(t.with_timezone(&chrono::Utc))
                .num_days()
                >= REFRESH_AFTER_DAYS
        })
        .unwrap_or(true)
}

/// 刷新响应写回(R-297 提纯):body 里非空的 access_token/refresh_token/id_token
/// 写回 auth.tokens,并把 last_refresh 更新为 now 的 RFC3339。必须包含有效
/// access_token；可选的 refresh_token/id_token 未换发时保留旧值。
fn apply_refresh_response(
    auth: &mut Value,
    body: &Value,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), LlmError> {
    if body["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return Err(LlmError::Config(
            "Codex 刷新响应缺少 access_token，未修改凭证".into(),
        ));
    }
    for key in ["access_token", "refresh_token", "id_token"] {
        if let Some(value) = body[key].as_str().filter(|s| !s.is_empty()) {
            auth["tokens"][key] = json!(value);
        }
    }
    auth["last_refresh"] = json!(now.to_rfc3339());
    Ok(())
}

/// 组装调用 chatgpt.com/backend-api/codex 所需的请求头(必要时先刷新令牌)。
pub async fn codex_headers(proxy: &ProxyConfig) -> Result<Vec<(String, String)>, LlmError> {
    let path = dirs::home_dir()
        .ok_or_else(|| LlmError::Config("cannot locate home dir".into()))?
        .join(".codex")
        .join("auth.json");
    codex_headers_at(path, proxy.clone(), TOKEN_URL.to_string()).await
}

async fn codex_headers_at(
    path: std::path::PathBuf,
    proxy: ProxyConfig,
    token_url: String,
) -> Result<Vec<(String, String)>, LlmError> {
    let runtime = tokio::runtime::Handle::current();
    // FileLock is thread-owned. Keep read, refresh and commit on one blocking
    // worker, never carry its non-Send guard across a migrating async task.
    tokio::task::spawn_blocking(move || {
        let _lock = kanzei_base::atomic_file::lock_exclusive(&path)
            .map_err(|e| LlmError::Config(format!("凭据锁失败: {e}")))?;
        runtime.block_on(headers_locked(&path, &proxy, &token_url))
    })
    .await
    .map_err(|e| LlmError::Config(format!("凭据读取任务失败: {e}")))?
}

async fn headers_locked(
    path: &std::path::Path,
    proxy: &ProxyConfig,
    token_url: &str,
) -> Result<Vec<(String, String)>, LlmError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        LlmError::Config(format!(
            "无法读取 {}({e})。先在终端运行 `codex login` 登录 Codex CLI。",
            path.display()
        ))
    })?;
    let mut auth: Value = serde_json::from_str(&text)
        .map_err(|e| LlmError::Config(format!("auth.json 解析失败: {e}")))?;

    // R-297:过期判定提成纯函数(时间源注入),生产传真实时钟,测试伪造时钟覆盖边界。
    let stale = is_stale(auth["last_refresh"].as_str(), chrono::Utc::now());

    if stale {
        refresh(&mut auth, path, proxy, token_url).await?;
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
    token_url: &str,
) -> Result<(), LlmError> {
    let before = auth.clone();
    let refresh_token = auth["tokens"]["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| LlmError::Config("auth.json 缺少 refresh_token,重新 `codex login`".into()))?
        .to_string();
    let client = build_http_client(proxy)?;
    let response = client
        .post(token_url)
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
    apply_refresh_response(auth, &body, chrono::Utc::now())?;
    // 原子替换 + 写前重读:刷新期间 Codex CLI 可能已抢先刷过并轮换了 refresh_token,
    // 此时采纳磁盘那份而不是覆盖回去(D-061)。commit 返回的才是最终生效的凭证。
    *auth = crate::auth::store::commit(path, auth, |disk, _mine| {
        // Compare against what we actually read, not our later response time:
        // a CLI login/refresh during the network call must not be overwritten.
        disk != &before
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_refresh_is_serialized_and_external_login_is_preserved() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
        for external_login in [false, true] {
            let dir = std::env::temp_dir().join(format!(
                "kz-refresh-{}-{}",
                std::process::id(),
                pseudo_uuid()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("auth.json");
            let original = json!({"tokens":{"access_token":"old", "refresh_token":"old-refresh", "account_id":"test"},"last_refresh":"2020-01-01T00:00:00Z"});
            std::fs::write(&path, original.to_string()).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/token", listener.local_addr().unwrap());
            let (started, ready) = tokio::sync::oneshot::channel();
            let (release, released) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut reader = tokio::io::BufReader::new(&mut socket);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).await.unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                }
                reader.read_exact(&mut vec![0; length]).await.unwrap();
                started.send(()).unwrap();
                released.await.unwrap();
                let body = r#"{"access_token":"refreshed","refresh_token":"rotated"}"#;
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            });
            let first = tokio::spawn(codex_headers_at(
                path.clone(),
                ProxyConfig::Disabled,
                url.clone(),
            ));
            tokio::time::timeout(std::time::Duration::from_secs(5), ready)
                .await
                .unwrap()
                .unwrap();
            if external_login {
                // The official CLI does not take our lock. Its update is earlier
                // than our refresh response, but must still win over our snapshot.
                let newer = json!({"tokens":{"access_token":"external", "refresh_token":"external-refresh", "account_id":"other-account"}, "last_refresh":chrono::Utc::now().to_rfc3339()});
                std::fs::write(&path, newer.to_string()).unwrap();
            }
            let second = tokio::spawn(codex_headers_at(path.clone(), ProxyConfig::Disabled, url));
            release.send(()).unwrap();
            let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                (
                    first.await.unwrap().unwrap(),
                    second.await.unwrap().unwrap(),
                )
            })
            .await
            .unwrap();
            server.await.unwrap();
            let expected = if external_login {
                "Bearer external"
            } else {
                "Bearer refreshed"
            };
            for headers in [first, second] {
                assert!(headers
                    .iter()
                    .any(|(key, value)| key == "authorization" && value == expected));
            }
            let stored: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(
                stored["tokens"]["refresh_token"],
                if external_login {
                    "external-refresh"
                } else {
                    "rotated"
                }
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    fn utc(rfc3339: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    /// R-297 验收:过期判定用伪造时钟覆盖边界——距 now 25 天及以上需刷新,
    /// 25 天内不刷;RFC3339 解析失败或字段缺失按过期处理。
    #[test]
    fn is_stale_伪造时钟覆盖刷新边界() {
        let now = utc("2026-02-01T00:00:00Z");
        // 恰好 25 天 → 过期(>=)。
        assert!(is_stale(Some("2026-01-07T00:00:00Z"), now));
        // 超过 25 天 → 过期。
        assert!(is_stale(Some("2026-01-01T00:00:00Z"), now));
        // 24 天 → 未过期。
        assert!(!is_stale(Some("2026-01-08T00:00:00Z"), now));
        // 字段缺失 → 按过期处理(宁可多刷一次)。
        assert!(is_stale(None, now));
        // RFC3339 解析失败 → 按过期处理。
        assert!(is_stale(Some("not-a-date"), now));
        // 带时区偏移的 RFC3339 也能解析(归一化到 UTC 后比较)。
        assert!(is_stale(Some("2026-01-07T08:00:00+08:00"), now));
    }

    /// R-297 验收:刷新写回路径——服务端换发的 token 写回,空值不覆盖旧值,
    /// last_refresh 更新为注入时钟的 RFC3339。
    #[test]
    fn apply_refresh_response_写回新令牌并保留未换发字段() {
        let mut auth = json!({
            "tokens": { "access_token": "old-access", "refresh_token": "old-refresh", "id_token": "old-id" },
            "last_refresh": "2026-01-01T00:00:00Z"
        });
        let body = json!({
            "access_token": "new-access",
            "refresh_token": "",
            "id_token": "new-id",
            "unknown_field": "ignored"
        });
        let now = utc("2026-02-01T00:00:00Z");
        apply_refresh_response(&mut auth, &body, now).unwrap();
        assert_eq!(auth["tokens"]["access_token"], "new-access");
        // refresh_token 为空串:不覆盖,保留旧值。
        assert_eq!(auth["tokens"]["refresh_token"], "old-refresh");
        assert_eq!(auth["tokens"]["id_token"], "new-id");
        assert_eq!(auth["last_refresh"], now.to_rfc3339());
    }

    /// HTTP 成功也必须有有效令牌，否则保留整个旧快照（包括刷新时间）。
    #[test]
    fn 刷新响应缺令牌时写回不产生半截凭证() {
        let mut auth = json!({ "tokens": { "access_token": "keep" }, "last_refresh": "old" });
        let body = json!({ "error": "invalid_grant" });
        let now = utc("2026-02-01T00:00:00Z");
        assert!(apply_refresh_response(&mut auth, &body, now).is_err());
        // 错误响应无 token 字段:原 access_token 和刷新时间都保留。
        assert_eq!(auth["tokens"]["access_token"], "keep");
        assert_eq!(auth["last_refresh"], "old");
    }
}
