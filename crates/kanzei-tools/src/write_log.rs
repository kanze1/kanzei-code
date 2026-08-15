//! 托管文档写日志(R-268):专用工具对托管文档的合法写入凭据。
//!
//! # 为什么需要它
//!
//! D-364 的不变式「窗口内没有写者」靠**锁**实现:bash 围栏持共享档贯穿整个命令窗口
//! (默认 120s、上限 600s),排他写者(req/defect/idea/decision/test_record/memory)预算仅
//! 3s——一条线跑 `cargo check` 时,另一条线的写者必然拿不到锁,轮末的 test_record/
//! req update 被外线长 bash 拖死(D-382 修完围栏互斥后并行吞吐的主要残余)。
//!
//! 本模块把不变式换成「**窗口内的变化可归因**」:专用工具每写一个托管文档,就落一条
//! 写日志(路径 + 写后内容指纹 + 身份);bash 围栏收口对账时,窗口内出现的变化逐路径
//! 查日志——日志命中且终态一致的吸收进基线(合法写入,不是越界),未命中的照旧隔离
//! 回滚。写者从此**不取跨窗口互斥**,只保留毫秒级文件锁(自己写自己读的原子性)。
//!
//! # 存储形态
//!
//! 每一条目一个 JSON 文件,落在 `.kanzei/.write-log/` 下(注意:**不在** `.kanzei/project`
//! 或 `.kanzei/memory` 之下——围栏快照只拍这两个托管根,写日志天然免疫,不会把自己
//! 算进 diff)。文件名 = `<毫秒时间戳>-<进程号>-<序号>.json`,同毫秒并发用进程号+序号
//! 区分,创建即原子(整文件写入)。清理由调用方按窗口推进(见 `prune_before`)。
//!
//! # 写入顺序契约
//!
//! 先写文档、**再**记日志(日志是「写后」凭据)。围栏收口时只认「窗口起点之后」的
//! 日志;文档已变而日志未落(写入还在窗口边界外)会被判越界回滚——这是「不可归因
//! = 越界」的保守语义,合法写入发生在窗口内就必然留下日志。

use std::path::{Path, PathBuf};

/// 写日志条目。`path` 是相对项目根的托管路径(`/` 分隔,与快照键同口径)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WriteLogEntry {
    /// 写入时刻(ms since epoch)。
    pub(crate) at_ms: u128,
    /// 相对项目根的托管路径,如 `.kanzei/project/requirements.md`。
    pub(crate) path: String,
    /// 写后内容的 sha256 指纹(hex)。
    pub(crate) sha256: String,
    /// 写后内容。托管文档通常很小(需求/缺陷/测试记录),整存便于回滚到最后一次
    /// 合法日志内容;超限大内容由调用方决定是否携带(空 = 只留指纹,只能判一致
    /// 不能回滚)。
    #[serde(default)]
    pub(crate) content: Vec<u8>,
    /// 归属身份(可审计:是谁写的)。
    #[serde(default)]
    pub(crate) run_id: Option<String>,
    #[serde(default)]
    pub(crate) process_id: Option<String>,
}

/// 写日志根目录:`.kanzei/.write-log/`。
fn log_root(project_root: &Path) -> PathBuf {
    project_root.join(".kanzei").join(".write-log")
}

/// 记录一条合法写入。先写文档、再调本函数(见模块头「写入顺序契约」)。
///
/// 生产消费者 = 专用写工具(req/defect/test_record/memory 等,批2 接入);
/// 当前已被 managed.rs 围栏收口测试消费。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn record(project_root: &Path, entry: &WriteLogEntry) -> std::io::Result<PathBuf> {
    let root = log_root(project_root);
    std::fs::create_dir_all(&root)?;
    let file = root.join(format!(
        "{}-{}-{}.json",
        entry.at_ms,
        std::process::id(),
        // 同毫秒同进程的并发写者:用路径指纹做序号,天然区分不同文档;
        // 同一文档在同一毫秒的两次写(几乎不可能)由最后一次覆盖前一次,
        // 对账只认最新终态,覆盖无害。
        crate::worktree::worktree_key(Path::new(&entry.path)).replace(['/', '\\'], "_")
    ));
    let json = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
    std::fs::write(&file, json)?;
    Ok(file)
}

/// 读取 `at_ms` 之后(含)的全部写日志,按时间升序。
///
/// 任一文件损坏/读失败:**跳过并继续**——写日志只是归因凭据,坏一条不该让围栏
/// 收口整体失败(不可归因的路径自然落到「未命中 → 回滚」的保守侧)。
pub(crate) fn entries_after(project_root: &Path, at_ms: u128) -> Vec<WriteLogEntry> {
    let root = log_root(project_root);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out: Vec<WriteLogEntry> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            serde_json::from_slice::<WriteLogEntry>(&bytes).ok()
        })
        .filter(|entry| entry.at_ms >= at_ms)
        .collect();
    out.sort_by_key(|entry| entry.at_ms);
    out
}

/// 删除 `at_ms` 之前的日志条目(围栏收口推进窗口后调用,防止日志无限增长)。
///
/// 消费者 = 围栏收口推进窗口(批2 接入);当前被测试消费。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn prune_before(project_root: &Path, at_ms: u128) {
    let root = log_root(project_root);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(log) = serde_json::from_slice::<WriteLogEntry>(&bytes) {
                if log.at_ms < at_ms {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

/// 计算写后内容的 sha256 指纹(hex)。
pub(crate) fn fingerprint(content: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(content);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
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
            "kz-writelog-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        dir
    }

    #[test]
    fn 记录与读取_按时间过滤_升序返回() {
        let root = temp_root("basic");
        let t0 = now_ms();
        let entry1 = WriteLogEntry {
            at_ms: t0 + 1,
            path: ".kanzei/project/requirements.md".into(),
            sha256: fingerprint(b"one"),
            content: b"one".to_vec(),
            run_id: Some("run-1".into()),
            process_id: Some("proc-1".into()),
        };
        let entry2 = WriteLogEntry {
            at_ms: t0 + 3,
            path: ".kanzei/project/defects.md".into(),
            sha256: fingerprint(b"two"),
            content: b"two".to_vec(),
            run_id: Some("run-1".into()),
            process_id: Some("proc-1".into()),
        };
        record(&root, &entry1).unwrap();
        record(&root, &entry2).unwrap();

        // 全部读到(起点早于两条)。
        let all = entries_after(&root, 0);
        assert_eq!(all.len(), 2, "两条都应读到");
        assert_eq!(all[0].path, entry1.path, "按时间升序");
        assert_eq!(all[1].path, entry2.path);
        assert_eq!(all[1].content, b"two");

        // 只读 t0+2 之后的:第二条。
        let after = entries_after(&root, t0 + 2);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].path, entry2.path);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn 清理_只删窗口之前() {
        let root = temp_root("prune");
        let t0 = now_ms();
        record(
            &root,
            &WriteLogEntry {
                at_ms: t0,
                path: ".kanzei/project/requirements.md".into(),
                sha256: fingerprint(b"old"),
                content: b"old".to_vec(),
                run_id: None,
                process_id: None,
            },
        )
        .unwrap();
        record(
            &root,
            &WriteLogEntry {
                at_ms: t0 + 100,
                path: ".kanzei/project/defects.md".into(),
                sha256: fingerprint(b"new"),
                content: b"new".to_vec(),
                run_id: None,
                process_id: None,
            },
        )
        .unwrap();
        prune_before(&root, t0 + 50);
        let remaining = entries_after(&root, 0);
        assert_eq!(remaining.len(), 1, "旧的被清,新的保留");
        assert_eq!(remaining[0].path, ".kanzei/project/defects.md");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn 指纹_内容不同则不同_相同则相同() {
        assert_ne!(fingerprint(b"a"), fingerprint(b"b"));
        assert_eq!(fingerprint(b"same"), fingerprint(b"same"));
    }
}
