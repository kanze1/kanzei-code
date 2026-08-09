//! Settings command boundary and global configuration commands.

use std::path::{Path, PathBuf};

use serde_json::json;
use tauri::State;

use crate::{AppState, SettingsPayload};

#[tauri::command]
pub fn settings_get(project_dir: Option<String>) -> serde_json::Value {
    let path = crate::global_config_path();
    let mut config: kanzei_harness::KanzeiConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    config.fill_defaults();
    let providers: Vec<serde_json::Value> = config.providers.iter().map(|(name, p)| {
        let key_present = if p.api_key.as_deref().is_some_and(|k| !k.trim().is_empty()) {
            Some(true)
        } else {
            p.api_key_env.as_deref().map(|env| std::env::var(env).map(|v| !v.is_empty()).unwrap_or(false))
        };
        json!({
            "name": name, "protocol": p.protocol, "baseUrl": p.base_url,
            "apiKeyEnv": p.api_key_env, "apiKey": p.api_key, "keyPresent": key_present,
            "auth": p.auth, "contextLimit": p.context_limit,
        })
    }).collect();
    let effective = project_dir.as_deref().map(PathBuf::from).filter(|p| p.is_dir())
        .and_then(|root| kanzei_harness::KanzeiConfig::load(&root).ok()).map(|merged| json!({
            "primary": merged.models.primary, "fast": merged.models.fast,
            "reasoning": merged.models.reasoning, "codexFastMode": merged.models.codex_fast_mode,
        }));
    json!({
        "path": path.display().to_string(), "primary": config.models.primary, "fast": config.models.fast,
        "proxy": config.proxy.unwrap_or_else(|| "env".into()),
        "profileDefault": config.profile.default.unwrap_or_else(|| "dev".into()),
        "reasoning": config.models.reasoning.unwrap_or_else(|| "off".into()),
        "codexFastMode": config.models.codex_fast_mode.unwrap_or(false), "providers": providers,
        "effective": effective,
        "projectConfig": project_dir.as_deref().and_then(|d| kanzei_harness::config::discover_project_root(Path::new(d)))
            .map(|root| root.join(".kanzei").join("kanzei.toml").display().to_string()),
    })
}

#[tauri::command]
pub fn settings_save(payload: SettingsPayload) -> Result<(), String> { crate::settings_save(payload) }
#[tauri::command]
pub fn settings_open() -> Result<(), String> { crate::settings_open() }
fn project_permission_config(project_dir: &str) -> PathBuf {
    kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| PathBuf::from(project_dir))
        .join(".kanzei")
        .join("kanzei.toml")
}

#[tauri::command]
pub fn permission_rules_get(project_dir: String) -> Result<serde_json::Value, String> {
    let path = project_permission_config(&project_dir);
    let config: kanzei_harness::KanzeiConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    let rules = config.permissions.rules.iter().enumerate()
        .filter(|(_, rule)| rule.effect == kanzei_harness::permission::Effect::Allow)
        .map(|(index, rule)| json!({
            "index": index, "action": rule.action, "resource": rule.resource, "effect": rule.effect,
        }))
        .collect::<Vec<_>>();
    Ok(json!({ "path": path.display().to_string(), "rules": rules }))
}

#[tauri::command]
pub fn permission_rule_delete(project_dir: String, index: usize) -> Result<(), String> {
    let path = project_permission_config(&project_dir);
    let text = std::fs::read_to_string(&path).map_err(|error| format!("读取权限规则失败: {error}"))?;
    let mut config: kanzei_harness::KanzeiConfig = toml::from_str(&text).map_err(|error| format!("配置格式错误: {error}"))?;
    let Some(rule) = config.permissions.rules.get(index) else { return Err("权限规则不存在或已被删除".into()); };
    if rule.effect != kanzei_harness::permission::Effect::Allow { return Err("只能删除已记住的放行规则".into()); }
    config.permissions.rules.remove(index);
    let text = toml::to_string_pretty(&config).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("写入权限规则失败: {error}"))
}
#[tauri::command]
pub async fn provider_test(
    protocol: String,
    base_url: String,
    api_key_env: Option<String>,
    api_key: Option<String>,
    auth: Option<String>,
    proxy: Option<String>,
) -> Result<String, String> {
    if matches!(auth.as_deref(), Some("codex") | Some("claude")) {
        return Ok("订阅登录态通道,无需 key 测试".into());
    }
    let key = api_key.filter(|k| !k.trim().is_empty())
        .or_else(|| api_key_env.as_deref().and_then(|e| std::env::var(e).ok()))
        .filter(|k| !k.trim().is_empty());
    let config = kanzei_harness::KanzeiConfig::load(Path::new(".")).unwrap_or_default();
    let proxy_value = proxy.or(config.proxy);
    let proxy = match proxy_value.as_deref() {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(p) => kanzei_llm::ProxyConfig::Explicit(p.to_string()),
    };
    let client = kanzei_llm::proxy::build_http_client(&proxy).map_err(|e| e.to_string())?;
    let base = base_url.trim_end_matches('/');
    let request = match protocol.as_str() {
        "anthropic" => {
            let mut request = client.get(format!("{base}/v1/models")).header("anthropic-version", "2023-06-01");
            if let Some(key) = &key { request = request.header("x-api-key", key); }
            request
        }
        _ => {
            let mut request = client.get(format!("{base}/models"));
            if let Some(key) = &key { request = request.bearer_auth(key); }
            request
        }
    };
    match request.timeout(std::time::Duration::from_secs(15)).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            Ok(match status {
                200 => format!("✓ 可用(HTTP 200{})", if key.is_some() { ",key 有效" } else { ",无鉴权" }),
                401 | 403 => format!("✗ key 无效(HTTP {status})——检查 key 是否过期/复制完整;moonshot 注意 .cn 与 .ai 的 key 不通用"),
                404 => "✗ 端点 404——base_url 可能不对(需要以 /v1 结尾?)".into(),
                _ => format!("? HTTP {status}——通道可达但响应异常"),
            })
        }
        Err(error) if error.is_timeout() => Ok("✗ 超时——检查网络/代理设置(本地服务不走代理)".into()),
        Err(error) if error.is_connect() => Ok("✗ 连接失败——服务未启动或代理不通".into()),
        Err(error) => Ok(format!("✗ 请求失败:{error}")),
    }
}

#[allow(dead_code)]
fn _state_type_marker(_: Option<State<'_, AppState>>) {}
