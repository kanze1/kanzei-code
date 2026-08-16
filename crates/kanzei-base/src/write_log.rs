//! 托管文档写日志(R-268):专用工具对托管文档的合法写入凭据。
//!
//! 为什么在 kanzei-base:写日志的**读侧**在 kanzei-tools 的 bash 围栏(收口对账),
//! **写侧**在专用写工具——tracker(kanzei-tools)与 memory(kanzei-memory)都要记。
//! kanzei-memory 不依赖 kanzei-tools,所以写日志必须落在两者共同的最底层
//! kanzei-base(与 atomic_file/FileLock 同级)。
//!
//! 格式是**纯 std 行编码**(每文件一条,四行):不引入 serde——kanzei-base 是
//! 零依赖原语层(设计约束见 lib.rs 头注释)。指纹用 [`content_hash`](fnv 稳定
//! 哈希),足够区分不同内容,不需要密码学强度。
//!
//! 文件名只含安全字符(`[A-Za-z0-9._-]`,其余压成 `_`):`:` 在 Windows 文件名里
//! 是**替代数据流(ADS)分隔符**,盘符路径直接拼进文件名会把内容写进隐藏 ADS、
//! 主文件留 0 字节(实测:整条日志静默消失,围栏对账读不到,合法写入被误回滚)。
//!
//! 写入顺序契约:先写文档、**再**记日志(日志是「写后」凭据)。围栏收口时只认
//! 「窗口起点之后」的日志;文档已变而日志未落会被判越界回滚——「不可归因 = 越界」
//! 的保守语义,合法写入发生在窗口内就必然留下日志。

use std::path::{Path, PathBuf};

/// 写日志条目。`path` 是相对项目根的托管路径(`/` 分隔,与快照键同口径)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteLogEntry {
    /// 写入时刻(ms since epoch)。
    pub at_ms: u128,
    /// 相对项目根的托管路径,如 `.kanzei/project/requirements.md`。
    pub path: String,
    /// 写后内容的指纹(fnv-1a hex,见 [`crate::content_hash`])。
    pub fingerprint: String,
    /// 写后内容。托管文档通常很小(需求/缺陷/测试记录),整存便于回滚到
    /// 最后一次合法日志内容;超限大内容由调用方决定是否携带(空 = 只留指纹,
    /// 只能判一致不能回滚)。
    pub content: Vec<u8>,
    /// 归属身份(可审计:是谁写的)。
    pub run_id: Option<String>,
    pub process_id: Option<String>,
}

/// 写日志根目录:`.kanzei/.write-log/`。
fn log_root(project_root: &Path) -> PathBuf {
    project_root.join(".kanzei").join(".write-log")
}

/// 记录一条合法写入。先写文档、再调本函数(见模块头「写入顺序契约」)。
///
/// 写日志根目录与条目文件按约定创建;任一步失败返回 Err,调用方决定是否上报
/// (日志丢失 = 该次写入失去归因凭据,围栏收口会把它当越界——宁可失败也不要
/// 静默吞掉,让调用方看到并重试)。
pub fn record(project_root: &Path, entry: &WriteLogEntry) -> std::io::Result<PathBuf> {
    let root = log_root(project_root);
    std::fs::create_dir_all(&root)?;
    // 文件名只留安全字符(见模块头 ADS 陷阱说明)。
    let key = entry
        .path
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let file = root.join(format!(
        "{}-{}-{}.log",
        entry.at_ms,
        std::process::id(),
        // 同毫秒同进程的并发写者:用路径指纹做序号,天然区分不同文档;
        // 同一文档在同一毫秒的两次写(几乎不可能)由最后一次覆盖前一次,
        // 对账只认最新终态,覆盖无害。
        key
    ));
    crate::atomic_file::write_atomic(&file, &encode(entry))?;
    Ok(file)
}

/// 行编码:每文件六行,换行分隔。
///   at_ms
///   path(不含换行——托管相对路径不含 \n)
///   fingerprint
///   content 的十六进制(空内容 = 空行,读取端按 `16 字节 = 1 行` 还原)
///   run_id(空行 = None)
///   process_id(空行 = None)
fn encode(entry: &WriteLogEntry) -> String {
    let mut lines = Vec::with_capacity(6);
    lines.push(entry.at_ms.to_string());
    lines.push(entry.path.clone());
    lines.push(entry.fingerprint.clone());
    // 十六进制编码 content:可含任意字节(中文正文、换行),纯 ASCII 安全。
    let mut hex = String::with_capacity(entry.content.len() * 2);
    for byte in &entry.content {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    lines.push(hex);
    lines.push(entry.run_id.clone().unwrap_or_default());
    lines.push(entry.process_id.clone().unwrap_or_default());
    lines.join("\n") + "\n"
}

fn decode(text: &str) -> Option<WriteLogEntry> {
    let mut lines = text.lines();
    let at_ms = lines.next()?.parse().ok()?;
    let path = lines.next()?.to_string();
    let fingerprint = lines.next()?.to_string();
    let hex = lines.next()?.to_string();
    let run_id = lines.next()?;
    let process_id = lines.next()?;
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut content = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        content.push(((hi << 4) | lo) as u8);
    }
    Some(WriteLogEntry {
        at_ms,
        path,
        fingerprint,
        content,
        run_id: (!run_id.is_empty()).then(|| run_id.to_string()),
        process_id: (!process_id.is_empty()).then(|| process_id.to_string()),
    })
}

/// 读取 `at_ms` 之后(含)的全部写日志,按时间升序。
///
/// 任一文件损坏/读失败:**跳过并继续**——写日志只是归因凭据,坏一条不该让围栏
/// 收口整体失败(不可归因的路径自然落到「未命中 → 回滚」的保守侧)。
pub fn entries_after(project_root: &Path, at_ms: u128) -> Vec<WriteLogEntry> {
    let root = log_root(project_root);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out: Vec<WriteLogEntry> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("log") {
                return None;
            }
            let text = std::fs::read_to_string(&path).ok()?;
            decode(&text)
        })
        .filter(|entry| entry.at_ms >= at_ms)
        .collect();
    out.sort_by_key(|entry| entry.at_ms);
    out
}

/// 删除 `at_ms` 之前的日志条目(围栏收口推进窗口后调用,防止日志无限增长)。
pub fn prune_before(project_root: &Path, at_ms: u128) {
    let root = log_root(project_root);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Some(log) = decode(&text) {
                if log.at_ms < at_ms {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_ms() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-wlog-base-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        dir
    }

    fn entry(at: u128, path: &str, content: &[u8]) -> WriteLogEntry {
        WriteLogEntry {
            at_ms: at,
            path: path.into(),
            fingerprint: crate::content_hash(content),
            content: content.to_vec(),
            run_id: None,
            process_id: None,
        }
    }

    #[test]
    fn 记录与读取_按时间过滤_升序返回() {
        let root = temp_root("basic");
        let t0 = now_ms();
        record(
            &root,
            &entry(t0 + 1, ".kanzei/project/requirements.md", b"one"),
        )
        .unwrap();
        record(
            &root,
            &entry(t0 + 3, ".kanzei/project/defects.md", b"two\nline"),
        )
        .unwrap();

        let all = entries_after(&root, 0);
        assert_eq!(all.len(), 2, "两条都应读到");
        assert_eq!(all[0].path, ".kanzei/project/requirements.md", "按时间升序");
        assert_eq!(all[1].path, ".kanzei/project/defects.md");
        assert_eq!(all[1].content, b"two\nline", "多行内容十六进制还原");

        let after = entries_after(&root, t0 + 2);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].path, ".kanzei/project/defects.md");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn 清理_只删窗口之前() {
        let root = temp_root("prune");
        let t0 = now_ms();
        record(&root, &entry(t0, ".kanzei/project/requirements.md", b"old")).unwrap();
        record(
            &root,
            &entry(t0 + 100, ".kanzei/project/defects.md", b"new"),
        )
        .unwrap();
        prune_before(&root, t0 + 50);
        let remaining = entries_after(&root, 0);
        assert_eq!(remaining.len(), 1, "旧的被清,新的保留");
        assert_eq!(remaining[0].path, ".kanzei/project/defects.md");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// ADS 陷阱回归:文件名含 `:` 会写进隐藏流、主文件 0 字节——sanitize 必须保证
    /// 文件名只含安全字符,记录后能原样读回。
    #[test]
    fn 文件名sanitize_冒号不触发ads_记录可读回() {
        let root = temp_root("ads");
        let t0 = now_ms();
        let content = "含中文与换行的正文\n第二行";
        record(
            &root,
            &WriteLogEntry {
                at_ms: t0,
                path: ".kanzei/project/requirements.md".into(),
                fingerprint: crate::content_hash(content.as_bytes()),
                content: content.as_bytes().to_vec(),
                run_id: Some("run-1".into()),
                process_id: Some("proc-1".into()),
            },
        )
        .unwrap();
        let all = entries_after(&root, 0);
        assert_eq!(all.len(), 1, "记录必须能读回(文件名 sanitize 后不触发 ADS)");
        assert_eq!(all[0].content, content.as_bytes());
        assert_eq!(all[0].fingerprint, crate::content_hash(content.as_bytes()));
        // 文件名不得含冒号(ADS 分隔符)。
        for file in std::fs::read_dir(log_root(&root)).unwrap().flatten() {
            let name = file.file_name().to_string_lossy().to_string();
            assert!(!name.contains(':'), "文件名不得含冒号: {name}");
            assert!(name.ends_with(".log"), "文件名必须带 .log 后缀: {name}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
