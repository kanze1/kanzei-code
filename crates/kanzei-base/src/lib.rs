//! kanzei-base:零依赖底层原语(R-208)。
//!
//! 承接原寄居在 kanzei-llm 的文件系统原语(atomic_file/FileLock,见
//! `docs/design/` 的 D-261 决策)。llm 是依赖图最底层,而 harness 不能反向依赖
//! llm——R-181 的跨进程 lease 契约在 harness、原语却在 llm,照单实施会撞依赖
//! 方向墙。拆出本 crate 后:llm/tools 从它取原语,harness 也可直接依赖。
//!
//! 本 crate 刻意保持零依赖:只放跨 crate 共享、无业务语义、纯 std 能实现的底座,
//! 不承担任何业务规则。

pub mod atomic_file;
pub mod write_log;

/// FNV-1a 64 位哈希 → 十六进制内容指纹(R-203 从 tools/files.rs 下沉单源)。
/// 用途:记忆文件正文戳(store.rs 的 stale/改判指纹)与桌面端文件视图 stamp
/// 同源;纯函数,输入 bytes 输出稳定指纹,不涉及文件系统。
pub fn content_hash(bytes: &[u8]) -> String {
    format!("fnv-{:016x}", fnv1a(bytes))
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
