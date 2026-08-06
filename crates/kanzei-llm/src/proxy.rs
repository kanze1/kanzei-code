//! 代理解析。语义移植自 opencode 的 proxy-env.ts(即 npm proxy-from-env):
//! `<scheme>_proxy` / `all_proxy` / `no_proxy`(支持 `*`、`host:port`、前导 `.`/`*` 后缀匹配),
//! 环境变量小写优先、大写兜底。显式配置(kanzei.toml `proxy = "..."`)优先于环境变量。

use crate::error::LlmError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProxyConfig {
    /// 从环境变量解析(默认)。
    #[default]
    Env,
    /// 显式指定,如 `http://127.0.0.1:12000`。
    Explicit(String),
    /// 强制直连。
    Disabled,
}

/// 构建应用了代理策略的 reqwest Client。
/// 一律先 no_proxy() 关掉 reqwest 自带的 env 探测,由我们统一决定,行为可预测。
/// 超时硬约束(D-003):连接 15s、流式读间隔 180s——网络不通必须快速报错,绝不静默挂起。
pub fn build_http_client(config: &ProxyConfig) -> Result<reqwest::Client, LlmError> {
    let builder = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(180));
    let builder = match config {
        ProxyConfig::Disabled => builder,
        ProxyConfig::Explicit(proxy) => {
            // 先验证一次格式,实际路由用 custom 以豁免 loopback。
            reqwest::Proxy::all(proxy.as_str())
                .map_err(|e| LlmError::Config(format!("invalid proxy `{proxy}`: {e}")))?;
            let proxy = proxy.clone();
            builder.proxy(reqwest::Proxy::custom(move |url: &url::Url| {
                if is_loopback(url.host_str().unwrap_or("")) {
                    None
                } else {
                    Some(proxy.clone())
                }
            }))
        }
        // env 优先;GUI 双击启动没有环境变量时,退回 Windows 系统代理(注册表)。
        ProxyConfig::Env => builder.proxy(reqwest::Proxy::custom(|url: &url::Url| {
            if is_loopback(url.host_str().unwrap_or("")) {
                return None;
            }
            proxy_for_url(url, &|key| std::env::var(key).ok()).or_else(system_proxy)
        })),
    };
    builder.build().map_err(LlmError::Transport)
}

/// Windows 系统代理(Internet Settings 注册表);其他平台恒 None。
#[cfg(windows)]
fn system_proxy() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = key.get_value("ProxyEnable").ok()?;
    if enabled != 1 {
        return None;
    }
    let server: String = key.get_value("ProxyServer").ok()?;
    // 格式:"host:port" 或 "http=h:p;https=h:p;..."
    let pick = if server.contains('=') {
        server
            .split(';')
            .find_map(|part| part.strip_prefix("https="))
            .or_else(|| {
                server
                    .split(';')
                    .find_map(|part| part.strip_prefix("http="))
            })?
            .to_string()
    } else {
        server
    };
    if pick.is_empty() {
        return None;
    }
    Some(if pick.contains("://") {
        pick
    } else {
        format!("http://{pick}")
    })
}

#[cfg(not(windows))]
fn system_proxy() -> Option<String> {
    None
}

/// 给定目标 URL,返回应使用的代理地址(env 注入便于测试)。
pub fn proxy_for_url(url: &url::Url, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let hostname = url.host_str()?;
    let scheme = url.scheme();
    let port = url.port().or(default_port(scheme))?;

    if !should_proxy(hostname, port, env) {
        return None;
    }

    let proxy = lookup(env, &format!("{scheme}_proxy"))
        .or_else(|| lookup(env, "all_proxy"))
        .filter(|v| !v.is_empty())?;
    if proxy.contains("://") {
        Some(proxy)
    } else {
        Some(format!("{scheme}://{proxy}"))
    }
}

fn lookup(env: &dyn Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    env(&key.to_lowercase()).or_else(|| env(&key.to_uppercase()))
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

fn is_loopback(hostname: &str) -> bool {
    hostname == "localhost"
        || hostname == "::1"
        || hostname == "[::1]"
        || hostname.starts_with("127.")
}

fn should_proxy(hostname: &str, port: u16, env: &dyn Fn(&str) -> Option<String>) -> bool {
    // loopback 永远直连(本地模型 Ollama/LM Studio 不能被送进代理)。
    if is_loopback(hostname) {
        return false;
    }
    let Some(no_proxy) = lookup(env, "no_proxy").filter(|v| !v.is_empty()) else {
        return true;
    };
    if no_proxy == "*" {
        return false;
    }
    for entry in no_proxy
        .split([',', ' ', '\t', '\n'])
        .filter(|s| !s.is_empty())
    {
        let (entry_host, entry_port) = match entry.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() => (h, p.parse::<u16>().ok()),
            _ => (entry, None),
        };
        if let Some(p) = entry_port {
            if p != port {
                continue;
            }
        }
        let matched = if let Some(suffix) = entry_host.strip_prefix('*') {
            hostname.ends_with(suffix)
        } else if entry_host.starts_with('.') {
            hostname.ends_with(entry_host)
        } else {
            hostname == entry_host
        };
        if matched {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn picks_scheme_proxy_then_all_proxy() {
        let url = url::Url::parse("https://api.anthropic.com/v1/messages").unwrap();
        let env = [("https_proxy", "http://127.0.0.1:12000")];
        assert_eq!(
            proxy_for_url(&url, &env_of(&env)),
            Some("http://127.0.0.1:12000".into())
        );
        let env = [("all_proxy", "127.0.0.1:12000")];
        assert_eq!(
            proxy_for_url(&url, &env_of(&env)),
            Some("https://127.0.0.1:12000".into())
        );
    }

    #[test]
    fn no_proxy_star_and_suffix() {
        let url = url::Url::parse("https://internal.corp.example.com/x").unwrap();
        let base = ("https_proxy", "http://p:1");
        assert_eq!(
            proxy_for_url(&url, &env_of(&[base, ("no_proxy", "*")])),
            None
        );
        assert_eq!(
            proxy_for_url(&url, &env_of(&[base, ("no_proxy", ".example.com")])),
            None
        );
        assert_eq!(
            proxy_for_url(&url, &env_of(&[base, ("no_proxy", "*.example.com")])),
            None
        );
        assert!(proxy_for_url(&url, &env_of(&[base, ("no_proxy", "other.com")])).is_some());
    }

    #[test]
    fn loopback_never_proxied() {
        let env = [("https_proxy", "http://p:1"), ("http_proxy", "http://p:1")];
        for target in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:1234/v1",
            "https://[::1]:8443/x",
        ] {
            let url = url::Url::parse(target).unwrap();
            assert_eq!(proxy_for_url(&url, &env_of(&env)), None, "{target}");
        }
    }

    #[test]
    fn no_proxy_port_must_match() {
        let url = url::Url::parse("https://api.example.com/x").unwrap();
        let base = ("https_proxy", "http://p:1");
        assert!(
            proxy_for_url(&url, &env_of(&[base, ("no_proxy", "api.example.com:8080")])).is_some()
        );
        assert_eq!(
            proxy_for_url(&url, &env_of(&[base, ("no_proxy", "api.example.com:443")])),
            None
        );
    }
}
