//! 网页预览静态服务(UI2-0926 #8,docs/design/preview_pane.md §2)。
//!
//! 本地 HTML 与模型写的代码片段一律经这里以 `http://127.0.0.1:<随机端口>` 提供,
//! **不用** file://,也**不用** Tauri 的 register_uri_scheme_protocol:
//! - file:// 下 ES module 与 fetch 相对资源会失败,多文件页面常常白屏;
//! - 自注册的 custom protocol 在 Windows 上形如 `http://<proto>.localhost`,Tauri 的
//!   `is_local_url` 把它当本地源,能调全部 app 命令(kanzei 没有 app ACL manifest)。
//!   127.0.0.1 是远程源,预览面板里的页面拿不到任何 IPC(B0 实测:invoke 一律
//!   `not allowed by ACL`)。
//!
//! 边界:
//! - 只绑 127.0.0.1:0,进程内单例,桌面与 CLI 共用;
//! - 每次运行生成 128 位随机 token 作为路径前缀:`/t/{token}/r/{root_id}/{rel}`(登记过的根)、
//!   `/t/{token}/s/{id}.html`(内存片段,LRU 32 个、单个 ≤ 2 MB);
//! - 只接受 GET/HEAD;百分号解码(中文路径、含空格的 `kanzei code`);逐段拒绝 `..`/`.`/`\`/`:`;
//!   canonicalize 之后必须仍在根内(符号链接越界同样 404);
//! - 目录没有 index.html 就 404,不列目录;`Cache-Control: no-store`;不加 CORS 头。
//! - 根路径引用(Vite 构建产物的 `/assets/x.js`)按 Referer 从引用页所在目录逐级向上找,
//!   仍限定在同一个根内。

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::Digest;

/// 内存片段的数量上限(LRU)。
pub const SNIPPET_CAPACITY: usize = 32;
/// 单个片段的字节上限。
pub const SNIPPET_MAX_BYTES: usize = 2 * 1024 * 1024;
/// 请求头(含请求行)读取上限。
const MAX_HEADER_BYTES: usize = 16 * 1024;

/// 进程内单例的静态服务。
pub struct PreviewServer {
    port: u16,
    token: String,
    registry: Arc<Mutex<Registry>>,
}

#[derive(Default)]
struct Registry {
    roots: Vec<Root>,
    snippets: VecDeque<(String, Arc<Vec<u8>>)>,
}

#[derive(Clone)]
struct Root {
    id: String,
    /// `std::fs::canonicalize` 的结果(Windows 上是 `\\?\` 形态),与请求侧同一口径比较。
    dir: PathBuf,
}

static SERVER: Mutex<Option<&'static PreviewServer>> = Mutex::new(None);

/// 取(必要时启动)进程内单例。启动失败不缓存,下次调用重试。
pub fn global() -> Result<&'static PreviewServer, String> {
    let mut slot = SERVER
        .lock()
        .map_err(|_| "预览静态服务锁中毒".to_string())?;
    if let Some(server) = *slot {
        return Ok(server);
    }
    let server = PreviewServer::start().map_err(|e| format!("预览静态服务启动失败: {e}"))?;
    let leaked: &'static PreviewServer = Box::leak(Box::new(server));
    *slot = Some(leaked);
    Ok(leaked)
}

impl PreviewServer {
    fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let token = random_token();
        let registry = Arc::new(Mutex::new(Registry::default()));
        let thread_registry = registry.clone();
        let thread_token = token.clone();
        std::thread::Builder::new()
            .name("kz-preview-server".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let registry = thread_registry.clone();
                    let token = thread_token.clone();
                    std::thread::spawn(move || serve_connection(stream, &registry, &token));
                }
            })?;
        Ok(PreviewServer {
            port,
            token,
            registry,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:<port>`(无尾斜杠)。
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn prefix(&self) -> String {
        format!("{}/t/{}/", self.origin(), self.token)
    }

    /// 这个 URL 是不是本服务发出的(带本次运行的 token)。
    pub fn owns_url(&self, url: &str) -> bool {
        url.starts_with(&self.prefix())
    }

    /// 登记一个根目录,返回它的 root_id(同一目录重复登记得到同一 id)。
    pub fn register_root(&self, dir: &Path) -> Result<String, String> {
        let canonical = std::fs::canonicalize(dir)
            .map_err(|e| format!("目录不可解析 {}: {e}", dir.display()))?;
        if !canonical.is_dir() {
            return Err(format!("不是目录: {}", dir.display()));
        }
        let id = root_id(&canonical);
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| "预览静态服务锁中毒".to_string())?;
        if !registry.roots.iter().any(|root| root.id == id) {
            registry.roots.push(Root {
                id: id.clone(),
                dir: canonical,
            });
        }
        Ok(id)
    }

    /// 把本地文件(或含 index.html 的目录)换成静态服务 URL。
    ///
    /// `roots` 按优先级给出候选根(通常是代码树 cwd、项目主根):文件落在哪个根里就以它为根,
    /// 这样页面里的 `../` 相对引用还能在根内解析;都不在时以文件所在目录为根。
    pub fn url_for_path(&self, target: &Path, roots: &[PathBuf]) -> Result<String, String> {
        let canonical = std::fs::canonicalize(target)
            .map_err(|e| format!("本地文件不存在或无法解析: {} ({e})", target.display()))?;
        let is_dir = canonical.is_dir();
        if is_dir && !canonical.join("index.html").is_file() {
            return Err(format!(
                "{} 是目录且没有 index.html;请指定要打开的 HTML 文件",
                target.display()
            ));
        }
        let root = roots
            .iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .find(|root| root.is_dir() && canonical.starts_with(root))
            .or_else(|| {
                if is_dir {
                    Some(canonical.clone())
                } else {
                    canonical.parent().map(Path::to_path_buf)
                }
            })
            .ok_or_else(|| format!("无法确定 {} 的根目录", target.display()))?;
        let id = self.register_root(&root)?;
        let rel = canonical.strip_prefix(&root).unwrap_or(Path::new(""));
        let mut encoded: Vec<String> = rel
            .components()
            .map(|part| percent_encode(&part.as_os_str().to_string_lossy()))
            .collect();
        if is_dir {
            encoded.push(String::new());
        }
        Ok(format!("{}r/{id}/{}", self.prefix(), encoded.join("/")))
    }

    /// 登记一段内存 HTML,返回它的 URL。同内容去重(LRU 里挪到最新)。
    pub fn add_snippet(&self, html: &str) -> Result<String, String> {
        let id = add_snippet_to(&self.registry, html)?;
        Ok(format!("{}s/{id}.html", self.prefix()))
    }
}

fn add_snippet_to(registry: &Mutex<Registry>, html: &str) -> Result<String, String> {
    if html.len() > SNIPPET_MAX_BYTES {
        return Err(format!(
            "HTML 片段过大({} 字节 > {} 上限);请写成文件后用 path 打开",
            html.len(),
            SNIPPET_MAX_BYTES
        ));
    }
    let id = hex(&sha2::Sha256::digest(html.as_bytes()))[..16].to_string();
    let mut registry = registry
        .lock()
        .map_err(|_| "预览静态服务锁中毒".to_string())?;
    let body = match registry.snippets.iter().position(|(key, _)| *key == id) {
        Some(index) => registry.snippets.remove(index).map(|(_, body)| body),
        None => None,
    }
    .unwrap_or_else(|| Arc::new(html.as_bytes().to_vec()));
    registry.snippets.push_back((id.clone(), body));
    while registry.snippets.len() > SNIPPET_CAPACITY {
        registry.snippets.pop_front();
    }
    Ok(id)
}

fn root_id(canonical: &Path) -> String {
    let key = canonical.to_string_lossy().to_lowercase();
    hex(&sha2::Sha256::digest(key.as_bytes()))[..12].to_string()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 128 位随机 token:两份独立随机键的 SipHash(`RandomState` 的键取自 OS 随机源)
/// 再混入时间与进程号,经 SHA-256 压成 32 个 hex。
fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut digest = sha2::Sha256::new();
    for salt in 0..4u64 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(salt);
        digest.update(hasher.finish().to_le_bytes());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    digest.update(now.to_le_bytes());
    digest.update(std::process::id().to_le_bytes());
    hex(&digest.finalize())[..32].to_string()
}

/// 路径段百分号编码:只保留 RFC 3986 unreserved 字符。
pub fn percent_encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// 百分号解码;非法转义或解出非 UTF-8 时返回 None。
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = (*bytes.get(index + 1)? as char).to_digit(16)?;
            let lo = (*bytes.get(index + 2)? as char).to_digit(16)?;
            out.push((hi * 16 + lo) as u8);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// 解码后的路径段校验:空段跳过;`.`、`..`、含 `\`、`:`、NUL 的段一律拒绝。
fn safe_segments(decoded: &str) -> Option<Vec<&str>> {
    let mut segments = Vec::new();
    for segment in decoded.split('/') {
        if segment.is_empty() {
            continue;
        }
        if segment == "."
            || segment == ".."
            || segment.contains('\\')
            || segment.contains(':')
            || segment.contains('\0')
        {
            return None;
        }
        segments.push(segment);
    }
    Some(segments)
}

pub fn mime_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" | "md" | "log" | "csv" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        _ => "application/octet-stream",
    }
}

/// 一次请求(只取服务需要的部分)。
#[derive(Debug, Clone)]
struct Request {
    method: String,
    target: String,
    referer: Option<String>,
}

#[derive(Debug)]
enum Body {
    Empty,
    Bytes(Arc<Vec<u8>>),
    File(PathBuf, u64),
}

#[derive(Debug)]
struct Response {
    status: u16,
    content_type: &'static str,
    location: Option<String>,
    body: Body,
}

impl Response {
    fn status(status: u16) -> Self {
        Response {
            status,
            content_type: "text/plain; charset=utf-8",
            location: None,
            body: Body::Empty,
        }
    }

    fn content_length(&self) -> u64 {
        match &self.body {
            Body::Empty => 0,
            Body::Bytes(bytes) => bytes.len() as u64,
            Body::File(_, len) => *len,
        }
    }

    fn head(&self) -> String {
        let reason = match self.status {
            200 => "OK",
            204 => "No Content",
            301 => "Moved Permanently",
            400 => "Bad Request",
            404 => "Not Found",
            405 => "Method Not Allowed",
            _ => "Error",
        };
        let mut head = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",
            self.status,
            self.content_type,
            self.content_length()
        );
        if self.status == 405 {
            head.push_str("Allow: GET, HEAD\r\n");
        }
        if let Some(location) = &self.location {
            head.push_str(&format!("Location: {location}\r\n"));
        }
        head.push_str("\r\n");
        head
    }
}

/// 请求 → 响应的纯判定(不碰 socket,单测直接喂)。
fn respond(registry: &Mutex<Registry>, token: &str, request: &Request) -> Response {
    if request.method != "GET" && request.method != "HEAD" {
        return Response::status(405);
    }
    let raw_path = request
        .target
        .split(['?', '#'])
        .next()
        .unwrap_or("/")
        .to_string();
    let prefix = format!("/t/{token}/");
    let Some(rest) = raw_path.strip_prefix(&prefix) else {
        return respond_rooted(registry, token, &raw_path, request.referer.as_deref());
    };
    if let Some(name) = rest.strip_prefix("s/") {
        let id = name.strip_suffix(".html").unwrap_or(name);
        let Ok(registry) = registry.lock() else {
            return Response::status(404);
        };
        return match registry.snippets.iter().find(|(key, _)| key == id) {
            Some((_, body)) => Response {
                status: 200,
                content_type: "text/html; charset=utf-8",
                location: None,
                body: Body::Bytes(body.clone()),
            },
            None => Response::status(404),
        };
    }
    let Some(rooted) = rest.strip_prefix("r/") else {
        return Response::status(404);
    };
    let (root_id, rel) = rooted.split_once('/').unwrap_or((rooted, ""));
    let Some(root) = find_root(registry, root_id) else {
        return Response::status(404);
    };
    let Some(decoded) = percent_decode(rel) else {
        return Response::status(400);
    };
    let Some(segments) = safe_segments(&decoded) else {
        return Response::status(404);
    };
    let wants_dir = rel.is_empty() || rel.ends_with('/');
    serve_file(&root, &segments, wants_dir, &raw_path)
}

fn find_root(registry: &Mutex<Registry>, id: &str) -> Option<Root> {
    registry
        .lock()
        .ok()?
        .roots
        .iter()
        .find(|root| root.id == id)
        .cloned()
}

fn serve_file(root: &Root, segments: &[&str], wants_dir: bool, raw_path: &str) -> Response {
    let mut joined = root.dir.clone();
    for segment in segments {
        joined.push(segment);
    }
    let Ok(canonical) = std::fs::canonicalize(&joined) else {
        return Response::status(404);
    };
    if !canonical.starts_with(&root.dir) {
        return Response::status(404);
    }
    let file = if canonical.is_dir() {
        if !wants_dir {
            // 目录不带尾斜杠:重定向,否则页面里的相对引用会以上一级为基准。
            let mut response = Response::status(301);
            response.location = Some(format!("{raw_path}/"));
            return response;
        }
        let index = canonical.join("index.html");
        if !index.is_file() {
            return Response::status(404);
        }
        index
    } else {
        canonical
    };
    match std::fs::metadata(&file) {
        Ok(meta) if meta.is_file() => Response {
            status: 200,
            content_type: mime_for(&file),
            location: None,
            body: Body::File(file, meta.len()),
        },
        _ => Response::status(404),
    }
}

/// 根路径请求(不带 token 前缀):只在 Referer 指向本服务某个根内页面时,
/// 从引用页所在目录逐级向上找同名文件(Vite 构建产物的 `/assets/x.js`)。
fn respond_rooted(
    registry: &Mutex<Registry>,
    token: &str,
    raw_path: &str,
    referer: Option<&str>,
) -> Response {
    let fallback = || {
        if raw_path == "/favicon.ico" {
            // 没有图标就回 204:浏览器不再在控制台记一条 404 噪声。
            Response::status(204)
        } else {
            Response::status(404)
        }
    };
    let Some(referer) = referer else {
        return fallback();
    };
    let referer_path = referer
        .split_once("://")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.find('/').map(|index| &rest[index..]))
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let Some(rooted) = referer_path.strip_prefix(&format!("/t/{token}/r/")) else {
        return fallback();
    };
    let (root_id, referer_rel) = rooted.split_once('/').unwrap_or((rooted, ""));
    let Some(root) = find_root(registry, root_id) else {
        return fallback();
    };
    let (Some(wanted), Some(referer_decoded)) =
        (percent_decode(raw_path), percent_decode(referer_rel))
    else {
        return Response::status(400);
    };
    let (Some(wanted), Some(referer_segments)) =
        (safe_segments(&wanted), safe_segments(&referer_decoded))
    else {
        return Response::status(404);
    };
    if wanted.is_empty() {
        return fallback();
    }
    // 引用页所在目录:最后一段是文件名(除非 referer 以 / 结尾)。
    let depth = if referer_rel.ends_with('/') || referer_rel.is_empty() {
        referer_segments.len()
    } else {
        referer_segments.len().saturating_sub(1)
    };
    for level in (0..=depth).rev() {
        let mut candidate: Vec<&str> = referer_segments[..level].to_vec();
        candidate.extend(wanted.iter().copied());
        let response = serve_file(&root, &candidate, false, raw_path);
        if response.status == 200 {
            return response;
        }
    }
    fallback()
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?.take(MAX_HEADER_BYTES as u64));
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut referer = None;
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) => break,
            Ok(_) if header == "\r\n" || header == "\n" => break,
            Ok(_) => {
                if let Some((name, value)) = header.split_once(':') {
                    if name.trim().eq_ignore_ascii_case("referer") {
                        referer = Some(value.trim().to_string());
                    }
                }
            }
            Err(_) => return None,
        }
    }
    Some(Request {
        method,
        target,
        referer,
    })
}

fn serve_connection(mut stream: TcpStream, registry: &Mutex<Registry>, token: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let Some(request) = read_request(&stream) else {
        return;
    };
    let response = respond(registry, token, &request);
    let _ = write_response(&mut stream, &request, response);
}

fn write_response(
    stream: &mut TcpStream,
    request: &Request,
    response: Response,
) -> std::io::Result<()> {
    stream.write_all(response.head().as_bytes())?;
    if request.method != "HEAD" {
        match response.body {
            Body::Empty => {}
            Body::Bytes(bytes) => stream.write_all(&bytes)?,
            Body::File(path, _) => {
                let mut file = std::fs::File::open(path)?;
                std::io::copy(&mut file, stream)?;
            }
        }
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn fixture(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-preview-server-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("site/assets")).unwrap();
        std::fs::create_dir_all(dir.join("empty dir")).unwrap();
        std::fs::create_dir_all(dir.join("中文 目录")).unwrap();
        std::fs::write(dir.join("site/index.html"), "<p>index</p>").unwrap();
        std::fs::write(dir.join("site/app.mjs"), "export const a = 1;").unwrap();
        std::fs::write(dir.join("site/assets/x.js"), "console.log(1)").unwrap();
        std::fs::write(dir.join("中文 目录/页 面.html"), "<p>中文</p>").unwrap();
        std::fs::write(dir.join("secret.txt"), "secret").unwrap();
        dir
    }

    fn registry_with(dir: &Path) -> (Mutex<Registry>, String) {
        let canonical = std::fs::canonicalize(dir).unwrap();
        let id = root_id(&canonical);
        let registry = Mutex::new(Registry {
            roots: vec![Root {
                id: id.clone(),
                dir: canonical,
            }],
            snippets: VecDeque::new(),
        });
        (registry, id)
    }

    fn get(registry: &Mutex<Registry>, target: &str) -> Response {
        respond(
            registry,
            TOKEN,
            &Request {
                method: "GET".into(),
                target: target.into(),
                referer: None,
            },
        )
    }

    fn body_text(response: &Response) -> String {
        match &response.body {
            Body::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            Body::File(path, _) => std::fs::read_to_string(path).unwrap(),
            Body::Empty => String::new(),
        }
    }

    #[test]
    fn mime表覆盖常见前端资源() {
        for (name, mime) in [
            ("a.html", "text/html; charset=utf-8"),
            ("a.mjs", "text/javascript; charset=utf-8"),
            ("a.js", "text/javascript; charset=utf-8"),
            ("a.css", "text/css; charset=utf-8"),
            ("a.json", "application/json; charset=utf-8"),
            ("a.svg", "image/svg+xml"),
            ("a.PNG", "image/png"),
            ("a.wasm", "application/wasm"),
            ("a.woff2", "font/woff2"),
            ("a.map", "application/json; charset=utf-8"),
            ("a.ico", "image/x-icon"),
            ("a.bin", "application/octet-stream"),
        ] {
            assert_eq!(mime_for(Path::new(name)), mime, "{name}");
        }
    }

    #[test]
    fn 正确mime与内容且响应头禁缓存() {
        let dir = fixture("mime");
        let (registry, id) = registry_with(&dir);
        let response = get(&registry, &format!("/t/{TOKEN}/r/{id}/site/app.mjs"));
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "text/javascript; charset=utf-8");
        assert_eq!(body_text(&response), "export const a = 1;");
        let head = response.head();
        assert!(head.contains("Cache-Control: no-store"), "{head}");
        assert!(
            !head.to_ascii_lowercase().contains("access-control"),
            "{head}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 越界与错误token与非法方法一律拒绝() {
        let dir = fixture("escape");
        let (registry, id) = registry_with(&dir.join("site"));
        // `..` 一律拒绝,哪怕解析后仍在根内(不靠 canonicalize 兜底)。
        assert_eq!(
            get(
                &registry,
                &format!("/t/{TOKEN}/r/{id}/assets/../index.html")
            )
            .status,
            404
        );
        for target in [
            format!("/t/{TOKEN}/r/{id}/../secret.txt"),
            format!("/t/{TOKEN}/r/{id}/%2e%2e/secret.txt"),
            format!("/t/{TOKEN}/r/{id}/%2E%2E%2Fsecret.txt"),
            format!("/t/{TOKEN}/r/{id}/..%5csecret.txt"),
            format!("/t/{TOKEN}/r/{id}/C:%5cWindows%5cwin.ini"),
            format!("/t/wrong-token/r/{id}/index.html"),
            format!("/t/{TOKEN}/r/unknown-root/index.html"),
            "/index.html".to_string(),
        ] {
            let status = get(&registry, &target).status;
            assert!(
                status == 404 || status == 400,
                "{target} 必须被拒,实得 {status}"
            );
        }
        let post = respond(
            &registry,
            TOKEN,
            &Request {
                method: "POST".into(),
                target: format!("/t/{TOKEN}/r/{id}/index.html"),
                referer: None,
            },
        );
        assert_eq!(post.status, 405);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 符号链接越界同样404() {
        let dir = fixture("symlink");
        // 目录联接(junction)不需要管理员权限;符号链接在未开开发者模式的 Windows 上建不了。
        let link = dir.join("site").join("leak");
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&dir)
            .output()
            .is_ok_and(|out| out.status.success());
        #[cfg(not(windows))]
        let made = std::os::unix::fs::symlink(&dir, &link).is_ok();
        assert!(made, "建目录联接失败,越界判定没有被真正覆盖");
        let (registry, id) = registry_with(&dir.join("site"));
        // 每一段都合法(没有 ..),只有 canonicalize 之后的根内判定能拦住。
        assert_eq!(
            get(&registry, &format!("/t/{TOKEN}/r/{id}/leak/secret.txt")).status,
            404
        );
        assert_eq!(
            get(&registry, &format!("/t/{TOKEN}/r/{id}/index.html")).status,
            200
        );
        #[cfg(windows)]
        let _ = std::fs::remove_dir(&link);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 目录无index时404且不列目录_有index时带斜杠才给页面() {
        let dir = fixture("dir");
        let (registry, id) = registry_with(&dir);
        assert_eq!(
            get(&registry, &format!("/t/{TOKEN}/r/{id}/empty%20dir/")).status,
            404
        );
        let redirect = get(&registry, &format!("/t/{TOKEN}/r/{id}/site"));
        assert_eq!(redirect.status, 301);
        assert_eq!(
            redirect.location.as_deref(),
            Some(format!("/t/{TOKEN}/r/{id}/site/").as_str())
        );
        let index = get(&registry, &format!("/t/{TOKEN}/r/{id}/site/"));
        assert_eq!(index.status, 200);
        assert_eq!(body_text(&index), "<p>index</p>");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 百分号解码中文与空格路径() {
        let dir = fixture("cjk");
        let (registry, id) = registry_with(&dir);
        let rel = format!(
            "{}/{}",
            percent_encode("中文 目录"),
            percent_encode("页 面.html")
        );
        let response = get(&registry, &format!("/t/{TOKEN}/r/{id}/{rel}?v=1#top"));
        assert_eq!(response.status, 200);
        assert_eq!(body_text(&response), "<p>中文</p>");
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_decode("%E4%B8%AD"), Some("中".into()));
        assert_eq!(percent_decode("%zz"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 根路径引用按referer在同根内逐级找() {
        let dir = fixture("referer");
        let (registry, id) = registry_with(&dir);
        let request = |target: &str, referer: Option<String>| {
            respond(
                &registry,
                TOKEN,
                &Request {
                    method: "GET".into(),
                    target: target.into(),
                    referer,
                },
            )
        };
        let page = format!("http://127.0.0.1:1/t/{TOKEN}/r/{id}/site/index.html");
        let found = request("/assets/x.js", Some(page.clone()));
        assert_eq!(found.status, 200);
        assert_eq!(body_text(&found), "console.log(1)");
        // 没有 referer 或 referer 不是本服务的页面:不猜。
        assert_eq!(request("/assets/x.js", None).status, 404);
        assert_eq!(
            request("/assets/x.js", Some("http://example.com/a".into())).status,
            404
        );
        // 逐级向上也不越出根。
        assert_eq!(request("/../secret.txt", Some(page.clone())).status, 404);
        // 没有图标时 favicon 回 204,不留 404 噪声。
        assert_eq!(request("/favicon.ico", Some(page)).status, 204);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 片段lru淘汰_同内容去重_超限拒绝() {
        let registry = Mutex::new(Registry::default());
        let first = add_snippet_to(&registry, "<p>0</p>").unwrap();
        for index in 1..SNIPPET_CAPACITY {
            add_snippet_to(&registry, &format!("<p>{index}</p>")).unwrap();
        }
        // 重复登记第一段:挪到最新,不被下一次淘汰。
        assert_eq!(add_snippet_to(&registry, "<p>0</p>").unwrap(), first);
        let second = add_snippet_to(&registry, "<p>1</p>").unwrap();
        add_snippet_to(&registry, "<p>new-a</p>").unwrap();
        add_snippet_to(&registry, "<p>new-b</p>").unwrap();
        assert_eq!(registry.lock().unwrap().snippets.len(), SNIPPET_CAPACITY);
        let served = get(&registry, &format!("/t/{TOKEN}/s/{first}.html"));
        assert_eq!(served.status, 200);
        assert_eq!(served.content_type, "text/html; charset=utf-8");
        assert_eq!(
            get(&registry, &format!("/t/{TOKEN}/s/{second}.html")).status,
            200
        );
        let evicted = hex(&sha2::Sha256::digest(b"<p>2</p>"))[..16].to_string();
        assert_eq!(
            get(&registry, &format!("/t/{TOKEN}/s/{evicted}.html")).status,
            404
        );
        let huge = "x".repeat(SNIPPET_MAX_BYTES + 1);
        assert!(add_snippet_to(&registry, &huge)
            .unwrap_err()
            .contains("过大"));
    }

    /// 真 socket:HEAD 只回头不回体;GET 回体;URL 由 url_for_path 生成。
    #[test]
    fn 真实连接_head不带body_url_for_path可直接访问() {
        let dir = fixture("socket");
        let server = PreviewServer::start().unwrap();
        let url = server
            .url_for_path(
                &dir.join("中文 目录/页 面.html"),
                std::slice::from_ref(&dir),
            )
            .unwrap();
        assert!(server.owns_url(&url), "{url}");
        assert!(url.starts_with(&server.origin()));
        let path = url.trim_start_matches(&server.origin()).to_string();
        let fetch = |method: &str| {
            let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
            write!(
                stream,
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
            )
            .unwrap();
            let mut text = String::new();
            stream.read_to_string(&mut text).unwrap();
            text
        };
        let head = fetch("HEAD");
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
        assert!(
            head.contains(&format!("Content-Length: {}", "<p>中文</p>".len())),
            "{head}"
        );
        assert!(head.ends_with("\r\n\r\n"), "HEAD 不得带 body: {head:?}");
        let got = fetch("GET");
        assert!(got.ends_with("<p>中文</p>"), "{got}");
        // 目录 → 以该目录为根时 URL 以 / 结尾。
        let site = server.url_for_path(&dir.join("site"), &[]).unwrap();
        assert!(site.ends_with('/'), "{site}");
        assert!(server
            .url_for_path(&dir.join("empty dir"), &[])
            .unwrap_err()
            .contains("index.html"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn token是32位hex且每次不同() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
