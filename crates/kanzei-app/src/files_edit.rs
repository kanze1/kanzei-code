//! 文件页编辑(UI2-0926 #6「文件浏览要带编辑功能，而且也是做成可拖拽伸缩的」)。
//!
//! 文件页原来只有读通道(`file_preview`);这里补上写通道,以及读写两侧共用的三件事:
//!
//! - **路径解析 [`resolve_in_root`]**:唯一的规范化入口,`file_preview`/`file_stat`/`file_write`
//!   都走它。先词法拒绝(空串、NUL 与控制字符、绝对路径、UNC、`..`;Windows 上另拒盘符与段内冒号即 ADS、
//!   `<>"|?*`、保留设备名、段尾点或空格——别的平台上这些是合法文件名),再按真实路径判定是否仍在项目根内——
//!   目录链接(junction/软链)指出根外的一律拒绝,指向根内受限目录的也按真实路径判只读,不给绕过的余地。
//! - **写入策略 [`write_policy`]**:`.git` 内部、托管文档(`kanzei_tools::MANAGED_ROOTS` 单源,
//!   直接改会被托管围栏隔离并回滚)、kanzei 内部状态文件只读,在任意一个 `.kanzei` 段之后都生效
//!   (树里嵌着的另一个 kanzei 项目同样受它自己的围栏管);`.kanzei/kanzei.toml` 与 `.kanzei/research/**` 可写。
//! - **文本探测 [`detect_text`]**:BOM、换行(与 Monaco 建模同一条多数派规则)、混合换行、UTF-8。
//!   编辑器读出内容时会丢 BOM、把混合换行统一成多数派,所以 BOM 由后端剥离并在保存时补回,
//!   CRLF 文件保存后仍是 CRLF;非 UTF-8(如 GBK)只读,否则保存会把原字节写坏。
//!
//! 冲突检测按**内容指纹**比对并交换(`kanzei_base::content_hash`,与标注 stamp、写日志同族):
//! 打开时记下指纹,保存时只有磁盘指纹仍相同才原子替换(`write_atomic_cas`),否则返回
//! `status: "conflict"` 和磁盘现在的指纹,由前端出冲突横幅。mtime 只给轮询做粗筛,不参与判定。
//! 用户选「覆盖磁盘版本」时,被覆盖的磁盘版本先复制进 `.kanzei/quarantine/files-overwrite-<ms>/`,
//! 误覆盖可以取回。
//!
//! 与代理的关系:
//! - 保存成功后记一条写日志(`process_id = "files-view"`,只留指纹;根下没有 `.kanzei` 时不记,不凭空建目录)。worktree 线跑 bash 时
//!   跨树围栏按「路径 + 指纹 + 窗口内」吸收有日志解释的变化,用户手改不会被报成他线越界。
//! - 代理的 edit/insert 每次调用都现读磁盘再匹配锚点,用户保存后的下一次编辑天然按新内容走
//!   (锚点被改掉时返回未命中并附上文件实际片段,等于重读;见本文件测试)。唯一整文件盲写的是
//!   write 工具,由先读后写账本(R-367,按同一种内容指纹判「读后被改」)收口;保存改变了指纹,
//!   账本落地后自动判 FILE_CHANGED_SINCE_READ,这里不另发通知。
//! - 不进回退检查点(R-366):检查点按用户消息记,kanzei 之外的改动按设计属外部改动,回退默认跳过。

use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

/// 可编辑/可完整预览的上限;超过只预览前 4MB 且只读(与 file_preview 原上限一致)。
pub(crate) const MAX_EDIT_BYTES: u64 = 4 * 1024 * 1024;
/// 二进制判据:头 8KB 含 NUL(与原 file_preview 一致)。
const BINARY_SNIFF_BYTES: usize = 8192;
/// 写日志里本通道的归属身份。
pub(crate) const FILES_VIEW_PROCESS: &str = "files-view";
/// 覆盖前留证的 quarantine 类型(kanzei-tools quarantine.rs KNOWN_KINDS 已登记)。
pub(crate) const OVERWRITE_EVIDENCE_KIND: &str = "files-overwrite";

/// 解析后的目标:`abs` 是真实路径(存在时 canonical;不存在时 = 最近存在祖先的 canonical + 剩余段),
/// `rel` 是相对项目根的真实路径(`/` 分隔、真实大小写),`exists` 指目标文件系统对象存在。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub abs: PathBuf,
    pub rel: String,
    pub exists: bool,
}

fn is_reserved_device_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_lowercase();
    if matches!(
        stem.as_str(),
        "con" | "prn" | "aux" | "nul" | "conin$" | "conout$"
    ) {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && (stem.starts_with("com") || stem.starts_with("lpt"))
        && bytes[3].is_ascii_digit()
        && bytes[3] != b'0'
}

/// 词法检查,返回规范化后的段(`/` 与 `\` 都当分隔符;连续分隔与 `.` 段忽略)。
/// 只做纯字符串判定,不碰文件系统。
///
/// 所有平台都拒:空、NUL 与控制字符、`/` 或 `\` 开头(绝对路径、UNC;旧 file_preview 会把开头的斜杠
/// 剥掉当相对路径,那会把项目外的绝对路径静默改读成项目里的同名文件——前端 toProjectRel 先把项目根下的
/// 绝对路径转成相对路径,其余一律拒绝,是有意的行为变化)、`..`。
/// 只在 Windows 上拒(在那里是非法或有歧义的名字,别的平台上是合法文件名):段内冒号(盘符、ADS)、
/// `<>"|?*`、段尾点或空格、保留设备名。
pub(crate) fn lexical_segments(rel: &str) -> Result<Vec<String>, String> {
    let bad = |why: &str| Err(format!("路径不合法({why}): {rel}"));
    if rel.trim().is_empty() {
        return bad("空路径");
    }
    if rel.contains('\0') {
        return bad("含 NUL");
    }
    if rel.starts_with('/') || rel.starts_with('\\') {
        return bad("必须是相对项目根的路径");
    }
    let mut segments = Vec::new();
    for segment in rel.split(['/', '\\']) {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return bad("不允许 ..");
        }
        if segment.chars().any(char::is_control) {
            return bad("含控制字符");
        }
        if cfg!(windows) {
            if segment.contains(':') {
                return bad("不允许盘符或冒号");
            }
            if segment
                .chars()
                .any(|ch| matches!(ch, '<' | '>' | '"' | '|' | '?' | '*'))
            {
                return bad("含 Windows 文件名禁用字符");
            }
            if segment.ends_with('.') || segment.ends_with(' ') {
                return bad("段尾不能是点或空格");
            }
            if is_reserved_device_name(segment) {
                return bad("Windows 保留设备名");
            }
        }
        segments.push(segment.to_string());
    }
    if segments.is_empty() {
        return bad("空路径");
    }
    Ok(segments)
}

fn rel_of(canon: &Path, canon_root: &Path) -> Option<String> {
    let rest = canon.strip_prefix(canon_root).ok()?;
    let parts = rest
        .components()
        .map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(parts.join("/"))
}

/// 唯一的路径规范化入口:词法拒绝 + 真实路径包含判定。见模块头。
pub(crate) fn resolve_in_root(root: &Path, rel: &str) -> Result<Resolved, String> {
    let segments = lexical_segments(rel)?;
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("项目根不可用 {}: {e}", root.display()))?;
    let candidate = segments
        .iter()
        .fold(root.to_path_buf(), |path, segment| path.join(segment));
    let escape = || format!("路径越界(真实路径不在项目根内): {rel}");
    match std::fs::canonicalize(&candidate) {
        Ok(canon) => {
            if !canon.starts_with(&canon_root) {
                return Err(escape());
            }
            let rel = rel_of(&canon, &canon_root).ok_or_else(escape)?;
            if rel.is_empty() {
                return Err(format!("不是文件: {rel}"));
            }
            Ok(Resolved {
                abs: canon,
                rel,
                exists: true,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // 链接本身在、目标不在(悬空链接):不当成「不存在」,否则新建会穿过链接写到别处。
            if std::fs::symlink_metadata(&candidate).is_ok() {
                return Err(format!("链接目标不存在: {rel}"));
            }
            // 沿祖先找最近的存在目录,按它的真实路径判包含,再拼上剩余段。
            let mut existing = candidate.clone();
            let mut rest: Vec<String> = Vec::new();
            loop {
                let Some(name) = existing
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                else {
                    return Err(escape());
                };
                rest.push(name);
                existing = existing
                    .parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(escape)?;
                if std::fs::symlink_metadata(&existing).is_ok() {
                    break;
                }
            }
            let canon_parent = std::fs::canonicalize(&existing).map_err(|e| e.to_string())?;
            if !canon_parent.starts_with(&canon_root) {
                return Err(escape());
            }
            if !canon_parent.is_dir() {
                return Err(format!("父路径不是目录: {rel}"));
            }
            rest.reverse();
            let abs = rest
                .iter()
                .fold(canon_parent.clone(), |path, segment| path.join(segment));
            let prefix = rel_of(&canon_parent, &canon_root).ok_or_else(escape)?;
            let tail = rest.join("/");
            let rel = if prefix.is_empty() {
                tail
            } else {
                format!("{prefix}/{tail}")
            };
            Ok(Resolved {
                abs,
                rel,
                exists: false,
            })
        }
        Err(error) => Err(format!("无法访问 {rel}: {error}")),
    }
}

/// 只读原因码。前端按码给出人话(READONLY_TEXT),顺序即优先级之外的路径策略部分。
///
/// 托管与内部规则在**任意一个** `.kanzei` 段之后都生效,不只项目根这一层:树里嵌着的另一个 kanzei 项目
/// (`sub/.kanzei/project/*.md`)有它自己的托管围栏,在文件页改了同样会被隔离并回滚。
pub(crate) fn write_policy(rel: &str) -> Option<&'static str> {
    let lower = rel.replace('\\', "/").to_ascii_lowercase();
    let segments: Vec<&str> = lower
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.contains(&".git") {
        return Some("git");
    }
    segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| **segment == ".kanzei")
        .find_map(|(index, _)| kanzei_dir_policy(&segments[index..].join("/")))
}

/// `tail` 以 `.kanzei` 段开头(小写、`/` 分隔)。`MANAGED_ROOTS` 全部以 `.kanzei/` 开头(测试钉住)。
fn kanzei_dir_policy(tail: &str) -> Option<&'static str> {
    let under = |prefix: &str| tail == prefix || tail.starts_with(&format!("{prefix}/"));
    if kanzei_tools::MANAGED_ROOTS
        .iter()
        .any(|root| under(&root.to_ascii_lowercase()))
    {
        return Some("managed");
    }
    if let Some(inner) = tail.strip_prefix(".kanzei/") {
        let file = inner.rsplit('/').next().unwrap_or_default();
        let internal_dir = [
            "artifacts",
            ".write-log",
            "quarantine",
            "summaries",
            "worktrees",
        ]
        .iter()
        .any(|dir| inner == *dir || inner.starts_with(&format!("{dir}/")));
        let state_db =
            inner == "state.db" || inner.starts_with("state.db-") || inner.starts_with("state.db.");
        if internal_dir
            || state_db
            || inner == "file-annotations.json"
            || file.ends_with(".lock")
            || file.ends_with(".tmp")
        {
            return Some("internal");
        }
    }
    None
}

/// 文本探测结果。`eol` 按 Monaco 建模同一规则取多数派:(CR + CRLF) × 2 > 总数 → CRLF。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextInfo {
    pub bom: bool,
    pub crlf: bool,
    pub mixed_eol: bool,
    pub utf8: bool,
}

impl TextInfo {
    pub(crate) fn eol(&self) -> &'static str {
        if self.crlf {
            "crlf"
        } else {
            "lf"
        }
    }
}

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

pub(crate) fn detect_text(bytes: &[u8], truncated: bool) -> TextInfo {
    let bom = bytes.starts_with(UTF8_BOM);
    let body = if bom { &bytes[UTF8_BOM.len()..] } else { bytes };
    let (mut cr, mut lf, mut crlf) = (0usize, 0usize, 0usize);
    let mut index = 0;
    while index < body.len() {
        match body[index] {
            b'\r' if body.get(index + 1) == Some(&b'\n') => {
                crlf += 1;
                index += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        index += 1;
    }
    let total = cr + lf + crlf;
    let kinds = [cr, lf, crlf].iter().filter(|count| **count > 0).count();
    let utf8 = match std::str::from_utf8(body) {
        Ok(_) => true,
        // 截断预览可能正好切在多字节字符中间:只差结尾不完整不算编码问题。
        Err(error) => truncated && error.error_len().is_none(),
    };
    TextInfo {
        bom,
        crlf: total > 0 && (cr + crlf) * 2 > total,
        mixed_eol: kinds > 1 || cr > 0,
        utf8,
    }
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|byte| *byte == 0)
}

fn mtime_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn readonly_error(code: &str) -> String {
    format!("READONLY:{code}")
}

/// `file_preview` 的实现(同步,由命令放进阻塞线程池)。原有四个字段语义不变,其余只加不改。
pub(crate) fn preview_at(root: &Path, rel: &str) -> Result<serde_json::Value, String> {
    let resolved = resolve_in_root(root, rel)?;
    if !resolved.exists {
        return Err(format!("无法打开 {rel}: 文件不存在"));
    }
    let meta = std::fs::metadata(&resolved.abs).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!("不是文件: {rel}"));
    }
    let truncated = meta.len() > MAX_EDIT_BYTES;
    let bytes = if truncated {
        use std::io::Read;
        let mut file = std::fs::File::open(&resolved.abs).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; MAX_EDIT_BYTES as usize];
        let mut filled = 0;
        while filled < buf.len() {
            let n = file.read(&mut buf[filled..]).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buf.truncate(filled);
        buf
    } else {
        std::fs::read(&resolved.abs).map_err(|e| e.to_string())?
    };
    let binary = is_binary(&bytes);
    let info = detect_text(&bytes, truncated);
    let body = if info.bom && !binary {
        &bytes[UTF8_BOM.len()..]
    } else {
        &bytes[..]
    };
    let content = if binary {
        String::new()
    } else {
        String::from_utf8_lossy(body).into_owned()
    };
    let readonly = if binary {
        Some("binary")
    } else if truncated {
        Some("truncated")
    } else if !info.utf8 {
        Some("encoding")
    } else if let Some(code) = write_policy(&resolved.rel) {
        Some(code)
    } else if meta.permissions().readonly() {
        Some("attr")
    } else {
        None
    };
    Ok(json!({
        "content": content,
        "binary": binary,
        "truncated": truncated,
        "size": meta.len(),
        // 截断时拿不到整文件指纹,不能做比较并交换:null 即只读。
        "hash": (!truncated).then(|| kanzei_tools::content_hash(&bytes)),
        "bom": info.bom && !binary,
        "eol": info.eol(),
        "mixedEol": info.mixed_eol && !binary,
        "encoding": if !binary && info.utf8 { "utf-8" } else { "unknown" },
        "mtimeMs": mtime_ms(&meta),
        "readonly": readonly,
    }))
}

/// `file_stat` 的实现:不读内容,只给轮询粗筛用。不存在不算错(exists:false),越界算错。
pub(crate) fn stat_at(root: &Path, rel: &str) -> Result<serde_json::Value, String> {
    let resolved = resolve_in_root(root, rel)?;
    let meta = if resolved.exists {
        std::fs::metadata(&resolved.abs)
            .ok()
            .filter(|meta| meta.is_file())
    } else {
        None
    };
    Ok(match meta {
        Some(meta) => json!({ "exists": true, "size": meta.len(), "mtimeMs": mtime_ms(&meta) }),
        None => json!({ "exists": false, "size": 0, "mtimeMs": null }),
    })
}

fn result_json(
    status: &str,
    hash: Option<String>,
    abs: &Path,
    exists: bool,
    evidence: Option<String>,
) -> serde_json::Value {
    let meta = std::fs::metadata(abs).ok().filter(|meta| meta.is_file());
    json!({
        "status": status,
        "hash": hash,
        "size": meta.as_ref().map(|meta| meta.len()).unwrap_or(0),
        "mtimeMs": meta.as_ref().and_then(mtime_ms),
        "exists": exists,
        "evidence": evidence,
    })
}

fn current_hash(abs: &Path) -> Option<String> {
    std::fs::read(abs)
        .ok()
        .map(|bytes| kanzei_tools::content_hash(&bytes))
}

/// 写入请求。`expected_hash = None` 表示「新建」(磁盘上已有同名文件即冲突)。
pub(crate) struct WriteRequest<'a> {
    pub rel: &'a str,
    pub content: &'a str,
    pub expected_hash: Option<&'a str>,
    pub bom: bool,
    pub evidence: bool,
}

/// `file_write` 的实现。统一返回 `{status: "saved"|"conflict", hash, size, mtimeMs, exists, evidence}`;
/// 只读、越界、超限、IO 失败才是 Err(只读以 `READONLY:<code>` 开头)。
pub(crate) fn write_at(
    root: &Path,
    request: WriteRequest<'_>,
) -> Result<serde_json::Value, String> {
    let resolved = resolve_in_root(root, request.rel)?;
    if let Some(code) = write_policy(&resolved.rel) {
        return Err(readonly_error(code));
    }
    let text = if request.bom {
        format!("\u{FEFF}{}", request.content)
    } else {
        request.content.to_string()
    };
    if text.len() as u64 > MAX_EDIT_BYTES {
        return Err(format!(
            "内容超过 {}MB,不能在文件页保存",
            MAX_EDIT_BYTES / 1024 / 1024
        ));
    }
    let mut evidence: Option<String> = None;
    if resolved.exists {
        let meta = std::fs::metadata(&resolved.abs).map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err(format!("不是文件: {}", resolved.rel));
        }
        if meta.len() > MAX_EDIT_BYTES {
            return Err(readonly_error("truncated"));
        }
        if meta.permissions().readonly() {
            return Err(readonly_error("attr"));
        }
        let bytes = std::fs::read(&resolved.abs).map_err(|e| e.to_string())?;
        if is_binary(&bytes) {
            return Err(readonly_error("binary"));
        }
        if !detect_text(&bytes, false).utf8 {
            return Err(readonly_error("encoding"));
        }
        let current = kanzei_tools::content_hash(&bytes);
        if request.expected_hash != Some(current.as_str()) {
            return Ok(result_json(
                "conflict",
                Some(current),
                &resolved.abs,
                true,
                None,
            ));
        }
        // 覆盖他人改过的版本前先留证:先写证据、再替换;替换没发生就把证据撤掉。
        let mut evidence_dir: Option<PathBuf> = None;
        if request.evidence {
            let dir_name = format!("{OVERWRITE_EVIDENCE_KIND}-{}", now_ms());
            let dir = root.join(".kanzei").join("quarantine").join(&dir_name);
            let target = resolved
                .rel
                .split('/')
                .fold(dir.clone(), |path, segment| path.join(segment));
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("留证目录创建失败,未覆盖: {e}"))?;
            }
            std::fs::write(&target, &bytes).map_err(|e| format!("留证失败,未覆盖: {e}"))?;
            evidence = Some(format!(".kanzei/quarantine/{dir_name}/{}", resolved.rel));
            evidence_dir = Some(dir);
        }
        if let Err(error) =
            kanzei_tools::atomic_file::write_atomic_cas(&resolved.abs, &text, &current, |live| {
                kanzei_tools::content_hash(live.as_bytes())
            })
        {
            if let Some(dir) = evidence_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            if error.starts_with("stale expected_hash") {
                return Ok(result_json(
                    "conflict",
                    current_hash(&resolved.abs),
                    &resolved.abs,
                    resolved.abs.is_file(),
                    None,
                ));
            }
            return Err(error);
        }
    } else {
        if request.expected_hash.is_some() {
            // 打开后被删:不偷偷重建,交给用户选「重新创建」(那时 expected_hash = None)。
            return Ok(result_json("conflict", None, &resolved.abs, false, None));
        }
        if let Some(parent) = resolved.abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
        }
        let created = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&resolved.abs);
        match created {
            Ok(mut file) => {
                file.write_all(text.as_bytes())
                    .and_then(|()| file.sync_all())
                    .map_err(|e| format!("写入失败: {e}"))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Ok(result_json(
                    "conflict",
                    current_hash(&resolved.abs),
                    &resolved.abs,
                    true,
                    None,
                ));
            }
            Err(error) => return Err(format!("创建失败: {error}")),
        }
    }
    let written = kanzei_tools::content_hash(text.as_bytes());
    // 写后凭据(写日志契约:先写文档、再记日志)。失败只告警:保存本身已成功,
    // 丢的只是跨树围栏的归因凭据。根下没有 `.kanzei`(resolve_root 退回到了打开的目录本身)就不记:
    // 那里没有围栏会读它,不凭空建出 `.kanzei/.write-log`——与代理 write 工具无 run 身份时不建
    // `.kanzei` 同一口径。覆盖留证照常建目录(安全比整洁重要)。
    if root.join(".kanzei").is_dir() {
        if let Err(error) = kanzei_tools::write_log::record(
            root,
            &kanzei_tools::write_log::WriteLogEntry {
                at_ms: now_ms(),
                path: resolved.rel.clone(),
                fingerprint: written.clone(),
                content: Vec::new(),
                run_id: None,
                process_id: Some(FILES_VIEW_PROCESS.to_string()),
            },
        ) {
            tracing::warn!(
                "files-view write-log record failed for {}: {error}",
                resolved.rel
            );
        }
    }
    Ok(result_json(
        "saved",
        Some(written),
        &resolved.abs,
        true,
        evidence,
    ))
}

/// 文件页轮询用:大小与修改时间(不读内容)。
#[tauri::command]
pub async fn file_stat(project_dir: String, path: String) -> Result<serde_json::Value, String> {
    let root = crate::files_view::resolve_root(&project_dir);
    tauri::async_runtime::spawn_blocking(move || stat_at(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// 文件页保存/新建。`expected_hash` 是打开时的内容指纹(新建传 null);`bom` 按打开时的探测回传;
/// `evidence` 只在「覆盖磁盘版本」时为 true。
#[tauri::command]
pub async fn file_write(
    project_dir: String,
    path: String,
    content: String,
    expected_hash: Option<String>,
    bom: Option<bool>,
    evidence: Option<bool>,
) -> Result<serde_json::Value, String> {
    let root = crate::files_view::resolve_root(&project_dir);
    tauri::async_runtime::spawn_blocking(move || {
        write_at(
            &root,
            WriteRequest {
                rel: &path,
                content: &content,
                expected_hash: expected_hash.as_deref(),
                bom: bom.unwrap_or(false),
                evidence: evidence.unwrap_or(false),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-files-edit-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn save(root: &Path, rel: &str, content: &str, expected: Option<&str>) -> serde_json::Value {
        write_at(
            root,
            WriteRequest {
                rel,
                content,
                expected_hash: expected,
                bom: false,
                evidence: false,
            },
        )
        .unwrap()
    }

    #[test]
    fn 指纹一致才写入并返回写出字节的指纹() {
        let root = temp_root("save");
        put(&root, "src/a.txt", b"one\n");
        let preview = preview_at(&root, "src/a.txt").unwrap();
        let hash = preview["hash"].as_str().unwrap().to_string();
        assert_eq!(hash, kanzei_tools::content_hash(b"one\n"));
        let result = save(&root, "src/a.txt", "two\n", Some(&hash));
        assert_eq!(result["status"], "saved");
        assert_eq!(std::fs::read(root.join("src/a.txt")).unwrap(), b"two\n");
        assert_eq!(result["hash"], kanzei_tools::content_hash(b"two\n"));
        assert_eq!(result["size"], 4);
        assert!(result["mtimeMs"].is_u64());
        assert_eq!(result["evidence"], serde_json::Value::Null);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn 打开后磁盘被改_返回冲突且不写入() {
        let root = temp_root("conflict");
        put(&root, "a.txt", b"base\n");
        let opened = preview_at(&root, "a.txt").unwrap()["hash"]
            .as_str()
            .unwrap()
            .to_string();
        std::fs::write(root.join("a.txt"), "agent changed\n").unwrap();
        let result = save(&root, "a.txt", "mine\n", Some(&opened));
        assert_eq!(result["status"], "conflict");
        assert_eq!(result["exists"], true);
        assert_eq!(
            result["hash"],
            kanzei_tools::content_hash(b"agent changed\n")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "agent changed\n"
        );
        // 带着冲突指纹覆盖(用户选「覆盖磁盘版本」)才写入。
        let current = result["hash"].as_str().unwrap().to_string();
        assert_eq!(
            save(&root, "a.txt", "mine\n", Some(&current))["status"],
            "saved"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "mine\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn 打开后被删_冲突exists为假_不带指纹才重新创建() {
        let root = temp_root("deleted");
        put(&root, "gone.txt", b"x");
        let opened = kanzei_tools::content_hash(b"x");
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        let result = save(&root, "gone.txt", "y", Some(&opened));
        assert_eq!(result["status"], "conflict");
        assert_eq!(result["exists"], false);
        assert!(result["hash"].is_null());
        assert!(!root.join("gone.txt").exists(), "冲突时不得偷偷重建");
        assert_eq!(save(&root, "gone.txt", "y", None)["status"], "saved");
        assert_eq!(std::fs::read_to_string(root.join("gone.txt")).unwrap(), "y");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn 新建_已存在返回冲突不覆盖_不存在时自动建父目录() {
        let root = temp_root("create");
        put(&root, "exists.md", b"keep");
        let result = save(&root, "exists.md", "", None);
        assert_eq!(result["status"], "conflict");
        assert_eq!(result["hash"], kanzei_tools::content_hash(b"keep"));
        assert_eq!(std::fs::read(root.join("exists.md")).unwrap(), b"keep");
        let created = save(&root, "new/deep/file.rs", "", None);
        assert_eq!(created["status"], "saved");
        assert_eq!(created["size"], 0);
        assert!(root.join("new/deep/file.rs").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bom与crlf往返字节不变() {
        let root = temp_root("bom");
        let original =
            b"\xEF\xBB\xBFfn main() {\r\n    println!(\"\xE4\xBD\xA0\xE5\xA5\xBD\");\r\n}\r\n";
        put(&root, "src/main.rs", original);
        let preview = preview_at(&root, "src/main.rs").unwrap();
        let content = preview["content"].as_str().unwrap();
        assert!(!content.starts_with('\u{FEFF}'), "content 必须去掉 BOM");
        assert_eq!(preview["bom"], true);
        assert_eq!(preview["eol"], "crlf");
        assert_eq!(preview["mixedEol"], false);
        assert_eq!(preview["encoding"], "utf-8");
        assert_eq!(preview["readonly"], serde_json::Value::Null);
        let result = write_at(
            &root,
            WriteRequest {
                rel: "src/main.rs",
                content,
                expected_hash: preview["hash"].as_str(),
                bom: true,
                evidence: false,
            },
        )
        .unwrap();
        assert_eq!(result["status"], "saved");
        assert_eq!(std::fs::read(root.join("src/main.rs")).unwrap(), original);
        assert_eq!(result["hash"], preview["hash"], "原样往返指纹不变");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn 换行探测与monaco同规则() {
        let cases: [(&[u8], &str, bool); 8] = [
            (b"a\r\nb\r\nc\r\n", "crlf", false),
            (b"a\nb\nc\n", "lf", false),
            (b"a\r\nb\r\nc\n", "crlf", true),
            (b"a\nb\nc\r\n", "lf", true),
            (b"a\r\nb\n", "lf", true),
            (b"a\rb\rc", "crlf", true),
            (b"a\nb\rc", "lf", true),
            (b"no newline", "lf", false),
        ];
        for (bytes, eol, mixed) in cases {
            let info = detect_text(bytes, false);
            assert_eq!(info.eol(), eol, "{:?}", String::from_utf8_lossy(bytes));
            assert_eq!(
                info.mixed_eol,
                mixed,
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
            assert!(info.utf8 && !info.bom);
        }
        assert!(detect_text(b"\xEF\xBB\xBFx", false).bom);
        assert!(
            !detect_text(b"\xC4\xE3\xBA\xC3", false).utf8,
            "GBK 字节不是 UTF-8"
        );
        assert!(
            detect_text(b"ok \xE4\xBD", true).utf8,
            "截断切在多字节中间不算编码问题"
        );
        assert!(!detect_text(b"ok \xE4\xBD", false).utf8);
    }

    /// 只在 Windows 上非法(或有歧义)的名字:别的平台上是合法文件名,不能拒。
    const WINDOWS_ONLY_BAD: [&str; 12] = [
        "C:/abs.txt",
        "C:abs.txt",
        "a:stream",
        "ok.txt:ads",
        "con",
        "sub/NUL.txt",
        "com1.log",
        "lpt9",
        "trail.",
        "space ",
        "what?.txt",
        "a<b>.txt",
    ];

    #[test]
    fn 路径词法拒绝() {
        let root = temp_root("lexical");
        put(&root, "ok.txt", b"ok");
        let universal = [
            "",
            "   ",
            "..",
            "../x",
            "a/../../x",
            "a\\..\\x",
            "/etc/passwd",
            "\\windows",
            "//server/share/x",
            "\\\\?\\C:\\x",
            "nul\0byte",
            "bell\u{7}.txt",
        ];
        let windows_only: &[&str] = if cfg!(windows) {
            &WINDOWS_ONLY_BAD
        } else {
            &[]
        };
        for bad in universal.iter().chain(windows_only) {
            let bad = *bad;
            assert!(lexical_segments(bad).is_err(), "应拒绝: {bad:?}");
            assert!(
                resolve_in_root(&root, bad).is_err(),
                "resolve 应拒绝: {bad:?}"
            );
            assert!(stat_at(&root, bad).is_err(), "file_stat 应拒绝: {bad:?}");
            assert!(
                preview_at(&root, bad).is_err(),
                "file_preview 应拒绝: {bad:?}"
            );
            assert!(
                write_at(
                    &root,
                    WriteRequest {
                        rel: bad,
                        content: "x",
                        expected_hash: None,
                        bom: false,
                        evidence: false
                    }
                )
                .is_err(),
                "file_write 应拒绝: {bad:?}"
            );
        }
        // 反斜杠分隔、`./` 前缀与重复分隔是同一个文件;com10 不是保留名。
        assert_eq!(resolve_in_root(&root, "./ok.txt").unwrap().rel, "ok.txt");
        assert_eq!(
            resolve_in_root(&root, "sub\\\\x.txt").unwrap().rel,
            "sub/x.txt"
        );
        assert!(lexical_segments("com10.txt").is_ok());
        assert!(lexical_segments("console.log").is_ok());
        // Windows 专属规则只在 Windows 上生效:别的平台上这些是合法文件名,改造前 file_preview 能打开。
        for name in WINDOWS_ONLY_BAD {
            assert_eq!(
                lexical_segments(name).is_err(),
                cfg!(windows),
                "{name:?} 只应在 Windows 上被词法拒绝"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    #[test]
    fn 目录链接指出根外一律拒绝_指向根内托管目录按真实路径只读() {
        let root = temp_root("junction");
        let outside = temp_root("junction-outside");
        put(&outside, "secret.txt", b"secret");
        put(&root, ".kanzei/project/requirements.md", b"# R\n");
        let escaped = junction(&root.join("out"), &outside);
        // mklink 只认反斜杠:目标路径逐段 join,不要写成 ".kanzei/project"。
        let inner = junction(&root.join("alias"), &root.join(".kanzei").join("project"));
        if !escaped || !inner {
            eprintln!("mklink /J 不可用,跳过目录链接用例(escaped={escaped}, inner={inner})");
            let _ = std::fs::remove_dir_all(&root);
            let _ = std::fs::remove_dir_all(&outside);
            return;
        }
        assert!(preview_at(&root, "out/secret.txt").is_err());
        assert!(stat_at(&root, "out/secret.txt").is_err());
        assert!(save_err(&root, "out/secret.txt", Some("x")).contains("越界"));
        assert!(save_err(&root, "out/new.txt", None).contains("越界"));
        assert!(!outside.join("new.txt").exists());
        assert_eq!(
            std::fs::read(outside.join("secret.txt")).unwrap(),
            b"secret"
        );
        // 根内链接:真实路径落在托管目录,按托管只读。
        let preview = preview_at(&root, "alias/requirements.md").unwrap();
        assert_eq!(preview["readonly"], "managed");
        assert_eq!(
            save_err(&root, "alias/requirements.md", Some("x")),
            "READONLY:managed"
        );
        assert_eq!(
            resolve_in_root(&root, "alias/requirements.md").unwrap().rel,
            ".kanzei/project/requirements.md"
        );
        let _ = std::fs::remove_dir(root.join("out"));
        let _ = std::fs::remove_dir(root.join("alias"));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    fn save_err(root: &Path, rel: &str, expected: Option<&str>) -> String {
        write_at(
            root,
            WriteRequest {
                rel,
                content: "changed",
                expected_hash: expected,
                bom: false,
                evidence: false,
            },
        )
        .unwrap_err()
    }

    #[test]
    fn 受限路径只读_写入被拒且文件不变() {
        let root = temp_root("policy");
        let cases = [
            (".git/config", "git"),
            ("sub/.GIT/HEAD", "git"),
            (".kanzei/project/requirements.md", "managed"),
            (".kanzei/Project/defects.md", "managed"),
            (".kanzei/memory/M-001-x.md", "managed"),
            (".kanzei/state.db-wal", "internal"),
            (".kanzei/state.db.v22.bak", "internal"),
            (".kanzei/artifacts/run_1.json", "internal"),
            (".kanzei/.write-log/1-2-x.log", "internal"),
            (".kanzei/quarantine/shell-1/a.md", "internal"),
            (".kanzei/file-annotations.json", "internal"),
            (".kanzei/research/memory.lock", "internal"),
            (".kanzei/worktrees/thread-a/src/lib.rs", "internal"),
            // 树里嵌着的另一个 kanzei 项目:它有自己的托管围栏,规则在任意一个 .kanzei 段之后都生效。
            ("sub/.kanzei/project/x.md", "managed"),
            ("vendor/other/.Kanzei/memory/M-1-y.md", "managed"),
            ("sub/.kanzei/state.db", "internal"),
        ];
        for (rel, code) in cases {
            put(&root, rel, b"orig\n");
            assert_eq!(write_policy(rel), Some(code), "{rel}");
            assert_eq!(
                preview_at(&root, rel).unwrap()["readonly"],
                code,
                "预览只读原因 {rel}"
            );
            let hash = kanzei_tools::content_hash(b"orig\n");
            assert_eq!(
                save_err(&root, rel, Some(&hash)),
                format!("READONLY:{code}")
            );
            assert_eq!(std::fs::read(root.join(rel)).unwrap(), b"orig\n", "{rel}");
        }
        // 新建也挡:不能在 .git 或托管目录里造文件。
        assert_eq!(
            save_err(&root, ".git/hooks/pre-commit", None),
            "READONLY:git"
        );
        assert_eq!(
            save_err(&root, ".kanzei/memory/M-999-new.md", None),
            "READONLY:managed"
        );
        for rel in [
            ".kanzei/kanzei.toml",
            ".kanzei/research/t/a.md",
            "docs/.gitkeep",
            "sub/.kanzei/kanzei.toml",
            "sub/kanzei/project/x.md",
        ] {
            put(&root, rel, b"orig\n");
            assert_eq!(write_policy(rel), None, "{rel}");
            let hash = kanzei_tools::content_hash(b"orig\n");
            assert_eq!(
                save(&root, rel, "new\n", Some(&hash))["status"],
                "saved",
                "{rel}"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn 非utf8与超过4mb只读_写入被拒字节不变() {
        let root = temp_root("encoding");
        let gbk = b"\xC4\xE3\xBA\xC3\r\n";
        put(&root, "gbk.txt", gbk);
        let preview = preview_at(&root, "gbk.txt").unwrap();
        assert_eq!(preview["readonly"], "encoding");
        assert_eq!(preview["encoding"], "unknown");
        let hash = kanzei_tools::content_hash(gbk);
        assert_eq!(save_err(&root, "gbk.txt", Some(&hash)), "READONLY:encoding");
        assert_eq!(std::fs::read(root.join("gbk.txt")).unwrap(), gbk);

        let big = vec![b'a'; MAX_EDIT_BYTES as usize + 10];
        put(&root, "big.txt", &big);
        let preview = preview_at(&root, "big.txt").unwrap();
        assert_eq!(preview["truncated"], true);
        assert_eq!(preview["readonly"], "truncated");
        assert!(preview["hash"].is_null(), "截断预览不能给整文件指纹");
        assert_eq!(
            preview["content"].as_str().unwrap().len(),
            MAX_EDIT_BYTES as usize
        );
        let hash = kanzei_tools::content_hash(&big);
        assert_eq!(
            save_err(&root, "big.txt", Some(&hash)),
            "READONLY:truncated"
        );
        assert_eq!(
            std::fs::read(root.join("big.txt")).unwrap().len(),
            big.len()
        );

        put(&root, "bin.dat", b"PNG\0\0data");
        let preview = preview_at(&root, "bin.dat").unwrap();
        assert_eq!(preview["binary"], true);
        assert_eq!(preview["readonly"], "binary");
        assert_eq!(preview["content"], "");
        let hash = kanzei_tools::content_hash(b"PNG\0\0data");
        assert_eq!(save_err(&root, "bin.dat", Some(&hash)), "READONLY:binary");
        // 内容超限也拒绝(不落半截)。
        let huge = "x".repeat(MAX_EDIT_BYTES as usize + 1);
        assert!(write_at(
            &root,
            WriteRequest {
                rel: "huge.txt",
                content: &huge,
                expected_hash: None,
                bom: false,
                evidence: false
            }
        )
        .is_err());
        assert!(!root.join("huge.txt").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn 覆盖磁盘版本前留证_可取回被覆盖的内容() {
        let root = temp_root("evidence");
        put(&root, "src/lib.rs", b"agent version\n");
        let current = kanzei_tools::content_hash(b"agent version\n");
        let result = write_at(
            &root,
            WriteRequest {
                rel: "src/lib.rs",
                content: "user version\n",
                expected_hash: Some(&current),
                bom: false,
                evidence: true,
            },
        )
        .unwrap();
        assert_eq!(result["status"], "saved");
        let evidence = result["evidence"].as_str().unwrap();
        assert!(
            evidence.starts_with(".kanzei/quarantine/files-overwrite-")
                && evidence.ends_with("/src/lib.rs"),
            "{evidence}"
        );
        assert_eq!(
            std::fs::read(root.join(evidence)).unwrap(),
            b"agent version\n"
        );
        assert_eq!(
            std::fs::read(root.join("src/lib.rs")).unwrap(),
            b"user version\n"
        );
        // 留证目录是清理内核认识的已知类型(不会被当未知证据永久保留)。
        let entries = kanzei_tools::quarantine::inspect(&root).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind.as_deref(), Some("files-overwrite"));
        // 指纹对不上时不留证(没发生覆盖)。
        let stale = write_at(
            &root,
            WriteRequest {
                rel: "src/lib.rs",
                content: "again\n",
                expected_hash: Some(&current),
                bom: false,
                evidence: true,
            },
        )
        .unwrap();
        assert_eq!(stale["status"], "conflict");
        assert_eq!(kanzei_tools::quarantine::inspect(&root).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn 保存后记写日志_跨树围栏据此吸收用户手改() {
        let root = temp_root("write-log");
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        put(&root, "notes/todo.md", b"a\n");
        let before = now_ms();
        let result = save(
            &root,
            "notes/todo.md",
            "b\n",
            Some(&kanzei_tools::content_hash(b"a\n")),
        );
        assert_eq!(result["status"], "saved");
        let entries = kanzei_tools::write_log::entries_after(&root, before);
        let entry = entries
            .iter()
            .find(|entry| entry.path == "notes/todo.md")
            .expect("保存后必须留下写日志");
        assert_eq!(entry.fingerprint, kanzei_tools::content_hash(b"b\n"));
        assert_eq!(entry.process_id.as_deref(), Some(FILES_VIEW_PROCESS));
        assert!(entry.run_id.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    /// resolve_root 找不到 `.kanzei` 时退回打开的目录本身:保存不能在那里凭空建出 `.kanzei/.write-log`
    /// (没有围栏会读它;代理 write 工具无 run 身份时同样不建)。覆盖留证照常建(安全比整洁重要)。
    #[test]
    fn 根下没有kanzei目录时保存不凭空建写日志_留证照常() {
        let root = temp_root("no-kanzei");
        put(&root, "a.txt", b"a\n");
        let result = save(
            &root,
            "a.txt",
            "b\n",
            Some(&kanzei_tools::content_hash(b"a\n")),
        );
        assert_eq!(result["status"], "saved");
        assert!(!root.join(".kanzei").exists(), "不得凭空建出 .kanzei");
        let current = kanzei_tools::content_hash(b"b\n");
        let overwritten = write_at(
            &root,
            WriteRequest {
                rel: "a.txt",
                content: "c\n",
                expected_hash: Some(&current),
                bom: false,
                evidence: true,
            },
        )
        .unwrap();
        let evidence = overwritten["evidence"].as_str().unwrap();
        assert_eq!(std::fs::read(root.join(evidence)).unwrap(), b"b\n");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn 托管根都以kanzei段开头_嵌套项目规则依赖这一点() {
        for root in kanzei_tools::MANAGED_ROOTS {
            assert!(
                root.to_ascii_lowercase().starts_with(".kanzei/"),
                "write_policy 只在 .kanzei 段处匹配托管根,{root} 不以 .kanzei/ 开头会漏判"
            );
        }
    }

    #[test]
    fn file_stat_存在给大小与时间_删除后exists为假() {
        let root = temp_root("stat");
        put(&root, "a.txt", b"12345");
        let stat = stat_at(&root, "a.txt").unwrap();
        assert_eq!(stat["exists"], true);
        assert_eq!(stat["size"], 5);
        assert!(stat["mtimeMs"].is_u64());
        std::fs::remove_file(root.join("a.txt")).unwrap();
        let stat = stat_at(&root, "a.txt").unwrap();
        assert_eq!(stat["exists"], false);
        assert!(stat["mtimeMs"].is_null());
        std::fs::create_dir_all(root.join("dir")).unwrap();
        assert_eq!(
            stat_at(&root, "dir").unwrap()["exists"],
            false,
            "目录不是文件"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// 代理的 edit 每次调用都现读磁盘:用户在文件页保存后,拿旧锚点的编辑返回未命中并附上
    /// 文件实际内容(等于重读),改没动过的地方则叠在用户的修改之上——用户的改动不会被吞。
    #[tokio::test]
    async fn 用户保存后代理edit按磁盘现状匹配() {
        let root = temp_root("agent-edit");
        put(&root, "notes.txt", b"alpha\nbeta\ngamma\n");
        let opened = kanzei_tools::content_hash(b"alpha\nbeta\ngamma\n");
        assert_eq!(
            save(
                &root,
                "notes.txt",
                "alpha\nBETA by user\ngamma\n",
                Some(&opened)
            )["status"],
            "saved"
        );
        let ctx = kanzei_harness::ResolveCtx {
            profile: kanzei_harness::ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root.clone(),
            config: std::sync::Arc::new(kanzei_harness::KanzeiConfig::default()),
        };
        let snapshot = crate::run::assembly::build_run_harness(false, None)
            .resolve(&ctx)
            .unwrap();
        let edit = snapshot
            .materialize_tools()
            .into_iter()
            .find(|tool| tool.name() == "edit")
            .expect("开发档位必须有 edit 工具");
        let tool_ctx = kanzei_harness::ToolCtx::new(root.clone(), root.clone());
        let stale = edit
            .execute(
                json!({ "path": "notes.txt", "old_string": "beta", "new_string": "BETA by agent" }),
                &tool_ctx,
            )
            .await;
        assert!(stale.is_error, "旧锚点必须未命中: {}", stale.content);
        assert!(
            stale.content.contains("BETA by user"),
            "未命中反馈应附上磁盘实际内容(等于重读): {}",
            stale.content
        );
        let merged = edit
            .execute(
                json!({ "path": "notes.txt", "old_string": "gamma", "new_string": "GAMMA" }),
                &tool_ctx,
            )
            .await;
        assert!(!merged.is_error, "{}", merged.content);
        assert_eq!(
            std::fs::read_to_string(root.join("notes.txt")).unwrap(),
            "alpha\nBETA by user\nGAMMA\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
