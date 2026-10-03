//! 零第三方依赖的共享底层：文件替换、文件锁、路径形态、内容指纹和写后凭据。
//! memory、tools、harness、llm 可直接依赖它，不必通过另一个业务 crate。
//! 本层保存事实并返回错误；是否允许写入、如何处理冲突与回滚由调用层决定。

pub mod atomic_file;
pub mod path_form;
pub mod write_log;

/// FNV-1a 64 位哈希 → 十六进制内容指纹(R-203 从 tools/files.rs 下沉单源)。
/// 用途:记忆文件正文戳(store.rs 的 stale/改判指纹)与桌面端文件视图 stamp
/// 同源;纯函数,输入 bytes 输出稳定指纹,不涉及文件系统。
pub fn content_hash(bytes: &[u8]) -> String {
    format!("fnv-{:016x}", fnv1a(bytes))
}

/// The same fingerprint as `content_hash`, without loading the file into memory.
pub fn file_content_hash(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        for byte in &buffer[..count] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    Ok(format!("fnv-{hash:016x}"))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::content_hash;

    #[test]
    fn content_hash_稳定且可区分() {
        let a = content_hash(b"old_string not found");
        let b = content_hash(b"old_string not found");
        let c = content_hash(b"cargo build network error");
        assert_eq!(a, b, "同内容必须同指纹");
        assert_ne!(a, c, "不同内容必须可区分");
        assert!(a.starts_with("fnv-"), "{a}");
    }
}
