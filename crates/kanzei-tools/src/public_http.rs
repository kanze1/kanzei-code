//! Public web transport: validate all DNS answers and pin the connection target,
//! including HTTP proxy CONNECT, while retaining the original TLS server name.

use kanzei_harness::ToolCtx;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncReadExt;

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_DIAGNOSTIC_BYTES: u64 = 8 * 1024;

pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) content_type: String,
    pub(crate) location: Option<String>,
    pub(crate) body: Vec<u8>,
    pub(crate) truncated: bool,
}

pub(crate) async fn fetch(
    url: &reqwest::Url,
    ctx: &ToolCtx,
    cap: usize,
) -> Result<Response, String> {
    let host = url.host_str().ok_or("web URL has no host")?;
    let port = url
        .port_or_known_default()
        .ok_or("web URL has no HTTP(S) port")?;
    let addresses: Vec<_> = if let Some(ip) = url.host().and_then(|host| match host {
        url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        _ => None,
    }) {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| "DNS resolution timed out".to_string())?
        .map_err(|error| format!("DNS resolution failed: {error}"))?
        .collect()
    };
    let target = validate_addresses(&addresses)?;
    let proxy = crate::tool_proxy(ctx);
    #[cfg(test)]
    let proxy = TEST_PROXY.try_with(Clone::clone).unwrap_or(proxy);
    pinned_request(url, target, &proxy, cap).await
}

fn validate_addresses(addresses: &[SocketAddr]) -> Result<SocketAddr, String> {
    if addresses.is_empty() {
        return Err("DNS returned no addresses".into());
    }
    if addresses.iter().any(|address| !is_public(address.ip())) {
        return Err("WEBFETCH_ADDRESS_BLOCKED: web requests cannot target loopback, private, link-local, metadata, multicast or reserved addresses".into());
    }
    Ok(addresses[0])
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let value = u32::from(ip);
            // IANA non-global IPv4 ranges, expressed as network/prefix pairs.
            const NON_GLOBAL: &[(u32, u32)] = &[
                (0x00000000, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 4),
                (0xf0000000, 4),
            ];
            !NON_GLOBAL
                .iter()
                .any(|(network, prefix)| value >> (32 - prefix) == network >> (32 - prefix))
        }
        IpAddr::V6(ip) => {
            if let Some(ip) = ip.to_ipv4_mapped() {
                return is_public(IpAddr::V4(ip));
            }
            let segments = ip.segments();
            // Accept global unicast, excluding protocol/documentation and 6to4
            // ranges which can encode a different endpoint family.
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && segments[1] <= 0x01ff)
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && (segments[1] & 0xf000) == 0)
        }
    }
}

fn curl_path() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let system =
            std::env::var_os("SystemRoot").ok_or("Windows system directory is unavailable")?;
        let path = PathBuf::from(system).join("System32/curl.exe");
        if path.is_file() {
            return Ok(path);
        }
    }
    #[cfg(not(windows))]
    for path in ["/usr/bin/curl", "/bin/curl"] {
        if std::path::Path::new(path).is_file() {
            return Ok(PathBuf::from(path));
        }
    }
    Err("The system curl executable is required for DNS-pinned public web requests".into())
}

async fn pinned_request(
    url: &reqwest::Url,
    target: SocketAddr,
    proxy: &kanzei_llm::ProxyConfig,
    cap: usize,
) -> Result<Response, String> {
    let host = url.host_str().ok_or("web URL has no host")?;
    let port = url.port_or_known_default().ok_or("web URL has no port")?;
    let connect_to = format!(
        "{host}:{port}:{}:{}",
        match target.ip() {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        },
        target.port()
    );
    let mut command = tokio::process::Command::new(curl_path()?);
    command.args([
        "--disable",
        "--silent",
        "--show-error",
        "--http1.1",
        "--compressed",
        "--include",
        "--suppress-connect-headers",
        "--proto",
        "=http,https",
        "--connect-timeout",
        "15",
        "--max-time",
        "30",
        "--connect-to",
        &connect_to,
        "--proxy",
        &kanzei_llm::proxy::proxy_for_config(proxy, url).unwrap_or_default(),
        "--noproxy",
        "",
        "--proxytunnel",
        "--user-agent",
        "kanzei/0.1",
        "--url",
        url.as_str(),
    ]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start public web transport: {error}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or("web transport stdout unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("web transport stderr unavailable")?;
    let mut diagnostics = tokio::task::JoinSet::new();
    diagnostics.spawn(async move {
        let mut bytes = Vec::new();
        stderr
            .take(MAX_DIAGNOSTIC_BYTES)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let read = async {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        let header_end = loop {
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let end = end + 4;
                let status = String::from_utf8_lossy(&bytes[..end])
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .and_then(|value| value.parse::<u16>().ok())
                    .ok_or("invalid HTTP response status")?;
                if status >= 200 {
                    break end;
                }
                bytes.drain(..end);
                continue;
            }
            let count = stdout
                .read(&mut chunk)
                .await
                .map_err(|error| error.to_string())?;
            if count == 0 {
                return Err("web transport closed before response headers".to_string());
            }
            bytes.extend_from_slice(&chunk[..count]);
            if bytes.len() > MAX_HEADER_BYTES {
                return Err("web response headers exceed 64 KiB".into());
            }
        };
        if header_end > MAX_HEADER_BYTES {
            return Err("web response headers exceed 64 KiB".into());
        }
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let status = headers
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse().ok())
            .ok_or("invalid HTTP response status")?;
        let field = |name: &str| {
            headers.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_string())
            })
        };
        let content_type = field("content-type").unwrap_or_default();
        let location = field("location");
        let mut body = bytes[header_end..].to_vec();
        while body.len() <= cap {
            let count = stdout
                .read(&mut chunk)
                .await
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            body.extend_from_slice(
                &chunk[..count.min(cap.saturating_add(1).saturating_sub(body.len()))],
            );
        }
        let truncated = body.len() > cap;
        body.truncate(cap);
        Ok(Response {
            status,
            content_type,
            location,
            body,
            truncated,
        })
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(35), read)
        .await
        .map_err(|_| "public web transport timed out".to_string())
        .and_then(|result| result);
    if !result.as_ref().is_ok_and(|response| !response.truncated) {
        let _ = child.kill().await;
    }
    let status = child.wait().await.map_err(|error| error.to_string())?;
    let diagnostic = diagnostics
        .join_next()
        .await
        .transpose()
        .map_err(|error| error.to_string())?
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let response = result.map_err(|error| {
        if diagnostic.is_empty() {
            error
        } else {
            format!("{error}: {}", String::from_utf8_lossy(&diagnostic).trim())
        }
    })?;
    if !status.success() && !response.truncated {
        return Err(format!(
            "web transport failed: {}",
            String::from_utf8_lossy(&diagnostic).trim()
        ));
    }
    Ok(response)
}

#[cfg(test)]
tokio::task_local! { static TEST_PROXY: kanzei_llm::ProxyConfig; }

/// Route public fixture addresses through a local CONNECT proxy. Address
/// validation and the production pinned transport remain active in every test.
#[cfg(test)]
pub(crate) async fn fixture_scope<F: std::future::Future>(future: F) -> F::Output {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut servers = tokio::task::JoinSet::new();
    servers.spawn(async move {
        let mut tunnels = tokio::task::JoinSet::new();
        loop {
            let (mut incoming, _) = listener.accept().await.unwrap();
            tunnels.spawn(async move {
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0u8];
                    if incoming.read_exact(&mut byte).await.is_err() {
                        return;
                    }
                    header.push(byte[0]);
                    assert!(header.len() < 8192);
                }
                let header = String::from_utf8(header).unwrap();
                let target: SocketAddr = header
                    .lines()
                    .next()
                    .unwrap()
                    .strip_prefix("CONNECT ")
                    .unwrap()
                    .strip_suffix(" HTTP/1.1")
                    .unwrap()
                    .parse()
                    .unwrap();
                assert_eq!(
                    target.ip(),
                    "8.8.8.8".parse::<IpAddr>().unwrap(),
                    "proxy CONNECT must use the verified public IP"
                );
                let mut origin = tokio::net::TcpStream::connect(("127.0.0.1", target.port()))
                    .await
                    .unwrap();
                incoming
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut incoming, &mut origin).await;
            });
        }
    });
    TEST_PROXY
        .scope(
            kanzei_llm::ProxyConfig::Explicit(format!("http://{address}")),
            future,
        )
        .await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn proxy_connect_is_pinned_while_the_original_http_host_is_preserved() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            assert!(String::from_utf8(request)
                .unwrap()
                .contains(&format!("Host: original-host.invalid:{port}")));
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\npinned",
                )
                .await
                .unwrap();
        });
        fixture_scope(async {
            let proxy = TEST_PROXY.with(Clone::clone);
            let url = reqwest::Url::parse(&format!("http://original-host.invalid:{port}/resource"))
                .unwrap();
            let response = pinned_request(
                &url,
                SocketAddr::new("8.8.8.8".parse().unwrap(), port),
                &proxy,
                4096,
            )
            .await
            .unwrap();
            assert_eq!(response.body, b"pinned");
        })
        .await;
        server.await.unwrap();
    }
    #[test]
    fn all_dns_answers_and_mapped_ipv6_are_checked() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.168.1.1",
            "0.0.0.0",
            "::1",
            "::",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "2002:7f00:1::",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
        assert!(validate_addresses(&[
            "8.8.8.8:443".parse().unwrap(),
            "127.0.0.1:443".parse().unwrap()
        ])
        .is_err());
    }
}
