//! 只在持久化边界处理旧六行格式。已丢失的空文件/删除含义不能猜测。

use super::{invalid, LoggedContent, WriteLogEntry};
use std::io;

pub(super) fn encode(entry: &WriteLogEntry) -> String {
    let content = match &entry.content {
        LoggedContent::Stored(bytes) => {
            use std::fmt::Write;
            let mut hex = String::from("data:");
            for byte in bytes {
                write!(&mut hex, "{byte:02x}").unwrap();
            }
            hex
        }
        LoggedContent::FingerprintOnly => "omitted".into(),
        LoggedContent::Deleted => "deleted".into(),
    };
    format!(
        "{}\n{}\n{}\n{content}\n{}\n{}\n",
        entry.at_ms,
        entry.path,
        entry.fingerprint,
        entry.run_id.as_deref().unwrap_or(""),
        entry.process_id.as_deref().unwrap_or("")
    )
}

pub(super) fn decode(text: &str) -> io::Result<WriteLogEntry> {
    let lines: Vec<_> = text.lines().collect();
    let [at_ms, path, fingerprint, content, run_id, process_id] = lines.as_slice() else {
        return Err(invalid("应为六行写日志"));
    };
    let content = match *content {
        "omitted" => LoggedContent::FingerprintOnly,
        "deleted" => LoggedContent::Deleted,
        old if old.starts_with("data:") => LoggedContent::Stored(decode_hex(&old[5..])?),
        "" if *fingerprint != crate::content_hash(&[]) => LoggedContent::FingerprintOnly,
        "" => {
            return Err(invalid(
                "旧日志未区分空文件与删除；保留原日志并人工核实，不能自动回滚",
            ))
        }
        old => LoggedContent::Stored(decode_hex(old)?),
    };
    if let LoggedContent::Stored(bytes) = &content {
        if *fingerprint != crate::content_hash(bytes) {
            return Err(invalid("写日志正文与指纹不一致，不能用于自动恢复"));
        }
    }
    Ok(WriteLogEntry {
        at_ms: at_ms.parse().map_err(invalid)?,
        path: (*path).into(),
        fingerprint: (*fingerprint).into(),
        content,
        run_id: (!run_id.is_empty()).then(|| (*run_id).into()),
        process_id: (!process_id.is_empty()).then(|| (*process_id).into()),
    })
}

fn decode_hex(hex: &str) -> io::Result<Vec<u8>> {
    let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(invalid("正文 hex 长度应为偶数"));
    }
    pairs
        .iter()
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| invalid("正文包含非 hex 字符"))?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| invalid("正文包含非 hex 字符"))?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}
