//! 记忆收件箱(R-255 第一刀,纯搬迁自 store.rs)。
//!
//! 独立理由:收件箱是「主 agent 的草稿投递 + manager 消化」的独立变更理由——`memory_note`
//! 投递(memory_note 工具)、SOP 候选逐条可见(pending_note_list)、按指纹丢弃
//! (discard_note)与整箱清空(clear_inbox)互不相关。它与准入(add)、生命周期
//! (promote)、检索(search)正交:改草稿格式不必读懂准入策略(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):inbox 读-拼-写回必须持记忆树锁(R-215/D-368)——与 append_note/
//! discard_note/clear_inbox 共用同一把锁,避免并发 append 互吃;树锁同时与 bash
//! 围栏窗口互斥,窗口内落盘不被围栏误回滚。

use std::path::PathBuf;

use super::store::MemoryStore;
use super::today;

impl MemoryStore {
    pub fn read_inbox(&self) -> String {
        std::fs::read_to_string(self.root.join("inbox.md")).unwrap_or_default()
    }

    /// D-409:分批读 inbox——取前 `max_notes` 个 `## note` 块的原文。
    /// 返回 (批次文本, 本批 note 数)。不再整箱(曾达 251KB/201 条)塞进单轮 prompt。
    pub fn read_inbox_batch(&self, max_notes: usize) -> (String, usize) {
        let text = self.read_inbox();
        let mut blocks: Vec<String> = Vec::new();
        let mut block: Vec<&str> = Vec::new();
        let mut in_block = false;
        for line in text.lines() {
            if line.starts_with("## note ") {
                if in_block {
                    blocks.push(block.join("\n"));
                    block.clear();
                }
                in_block = true;
            }
            if in_block {
                block.push(line);
            }
        }
        if in_block {
            blocks.push(block.join("\n"));
        }
        let take = blocks.len().min(max_notes);
        (blocks[..take].join("\n\n"), take)
    }

    /// manager 消化完毕后清空草稿箱(整箱内容已在触发 prompt 里,清空即"已消费")。
    pub fn clear_inbox(&self) -> anyhow::Result<()> {
        let path = self.root.join("inbox.md");
        // R-215 语义不变 + D-368:改用记忆树锁——与 append_note/discard_note 共用
        // 同一把锁,整箱清空锁内执行避免与并发 append 交错(append 持锁读-拼-写回,
        // clear 持锁覆盖,不会互吃);同时树锁与 bash 围栏窗口互斥,窗口内清空不被
        // 围栏误回滚。
        let _lock = self.tree_lock()?;
        if path.is_file() {
            crate::atomic_file::write_atomic(&path, "# Memory Inbox\n")?;
        }
        Ok(())
    }

    /// inbox 草稿箱:主 agent 的唯一写入口(memory_note),manager 在 M2 消化。
    /// refs 为来源引用(R-070):以 `- refs: R-012 D-044` 行写入草稿,
    /// manager 消化时经 memory_add 的 refs 参数把引用带进正式条目。
    pub fn append_note(
        &self,
        summary: &str,
        detail: &str,
        category_hint: &str,
        refs: &[String],
    ) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join("inbox.md");
        // R-215 语义不变 + D-368:改用记忆树锁——读-拼接-写回整体持锁,并发 append
        // 各读各的再各自写回时后写者不会覆盖先写者(note 无痕丢失);同时树锁与
        // bash 围栏窗口互斥,窗口内 memory_note 落盘不被围栏误回滚。
        let _lock = self.tree_lock()?;
        let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| "# Memory Inbox\n".into());
        let refs_line = {
            let refs: Vec<&str> = refs
                .iter()
                .map(|r| r.trim())
                .filter(|r| !r.is_empty())
                .collect();
            if refs.is_empty() {
                String::new()
            } else {
                format!("- refs: {}\n", refs.join(" "))
            }
        };
        text.push_str(&format!(
            "\n## note {} {}\n- summary: {}\n{}{}",
            today(),
            if category_hint.is_empty() {
                "".to_string()
            } else {
                format!("[{category_hint}]")
            },
            summary.trim(),
            refs_line,
            if detail.trim().is_empty() {
                String::new()
            } else {
                format!("{}\n", detail.trim())
            },
        ));
        crate::atomic_file::write_atomic(&path, &text)?;
        Ok(path)
    }

    /// 同一失败指纹是否已投递过草稿(跨轮去重:同一个坑不该每轮都投)。
    /// inbox 被 manager 清空后指纹随之失效——那时该坑要么已入库、要么被判 NOOP,
    /// 再次复现时重新投递是正确行为。
    pub fn note_fingerprint_seen(&self, fingerprint: &str) -> bool {
        self.read_inbox().contains(fingerprint)
    }

    /// 解析 inbox 里的待处理草稿(R-124:SOP 候选要能被用户逐条看见并处置)。
    /// 返回 (分类提示, 摘要行, 明细)。
    pub fn pending_note_list(&self) -> Vec<(String, String, String)> {
        let text = self.read_inbox();
        let mut out = Vec::new();
        let mut current: Option<(String, String, Vec<String>)> = None;
        for line in text.lines() {
            if let Some(head) = line.strip_prefix("## note ") {
                if let Some((hint, summary, detail)) = current.take() {
                    out.push((hint, summary, detail.join("\n")));
                }
                let hint = head
                    .split_once('[')
                    .and_then(|(_, rest)| rest.split_once(']'))
                    .map(|(h, _)| h.to_string())
                    .unwrap_or_default();
                current = Some((hint, String::new(), Vec::new()));
            } else if let Some(entry) = current.as_mut() {
                match line.strip_prefix("- summary: ") {
                    Some(summary) => entry.1 = summary.trim().to_string(),
                    None if !line.trim().is_empty() => entry.2.push(line.to_string()),
                    None => {}
                }
            }
        }
        if let Some((hint, summary, detail)) = current {
            out.push((hint, summary, detail.join("\n")));
        }
        out
    }

    /// 丢弃一条草稿(按其摘要里的指纹定位)。用户说不要的候选不该再进 manager 的消化范围。
    pub fn discard_note(&self, fingerprint: &str) -> anyhow::Result<bool> {
        // R-215 语义不变 + D-368:改用记忆树锁——与 append_note/clear_inbox 共用
        // 同一把锁,锁内读-改-写回,不会把并发 append 的内容当旧快照覆盖掉;同时
        // 树锁与 bash 围栏窗口互斥。
        let _lock = self.tree_lock()?;
        let text = self.read_inbox();
        if !text.contains(fingerprint) {
            return Ok(false);
        }
        // 按 `## note` 切块,整块保留或整块丢弃——只删摘要行会留下孤儿明细。
        let mut kept: Vec<&str> = Vec::new();
        let mut block: Vec<&str> = Vec::new();
        let mut in_block = false;
        let mut removed = false;
        for line in text.lines() {
            if line.starts_with("## note ") {
                if in_block {
                    if block.iter().any(|l| l.contains(fingerprint)) {
                        removed = true;
                    } else {
                        kept.extend(block.iter());
                    }
                    block.clear();
                }
                in_block = true;
            }
            if in_block {
                block.push(line);
            } else {
                kept.push(line);
            }
        }
        if in_block {
            if block.iter().any(|l| l.contains(fingerprint)) {
                removed = true;
            } else {
                kept.extend(block.iter());
            }
        }
        let mut next = kept.join("\n");
        if !next.ends_with('\n') {
            next.push('\n');
        }
        crate::atomic_file::write_atomic(&self.root.join("inbox.md"), &next)?;
        Ok(removed)
    }

    pub fn pending_notes(&self) -> usize {
        std::fs::read_to_string(self.root.join("inbox.md"))
            .map(|t| t.lines().filter(|l| l.starts_with("## note ")).count())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_memory(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-inbox-batch-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_inbox(dir: &Path, blocks: usize) {
        let mut text = String::new();
        for i in 0..blocks {
            text.push_str(&format!(
                "## note {i}\n- summary: 第 {i} 条\n- detail: 内容 {i}\n\n"
            ));
        }
        // project() 的 root 是 project_memory_root(project_root 下记忆目录),写入同位置。
        let memory_root = crate::memory::project_memory_root(dir);
        std::fs::create_dir_all(&memory_root).unwrap();
        std::fs::write(memory_root.join("inbox.md"), text).unwrap();
    }

    /// D-409:分批读——取前 max_notes 个完整 `## note` 块,不截断块内容。
    #[test]
    fn read_inbox_batch_取前N块且块完整() {
        let dir = temp_memory("take3");
        write_inbox(&dir, 10);
        let store = MemoryStore::project(&dir);
        assert_eq!(store.pending_notes(), 10);
        let (batch, count) = store.read_inbox_batch(3);
        assert_eq!(count, 3, "取 3 块");
        assert!(batch.contains("## note 0"), "首块在");
        assert!(batch.contains("第 0 条"));
        assert!(batch.contains("## note 2"), "第三块在");
        assert!(!batch.contains("## note 3"), "第四块不在(只取前 3)");
        // 块完整:detail 与 summary 都在。
        assert!(batch.contains("- detail: 内容 2"), "第三块 detail 完整");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-409:max_notes 超块数取全部;0 取空;空 inbox 返回空。
    #[test]
    fn read_inbox_batch_边界() {
        let dir = temp_memory("edge");
        write_inbox(&dir, 4);
        let store = MemoryStore::project(&dir);
        let (all, count) = store.read_inbox_batch(100);
        assert_eq!(count, 4, "超块数取全部");
        assert_eq!(all.matches("## note ").count(), 4);
        let (zero, zcount) = store.read_inbox_batch(0);
        assert_eq!(zcount, 0);
        assert_eq!(zero, "");
        // 空 inbox。
        let dir2 = temp_memory("empty");
        let store2 = MemoryStore::project(&dir2);
        let (empty, ec) = store2.read_inbox_batch(10);
        assert_eq!(ec, 0);
        assert_eq!(empty, "");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&dir2).ok();
    }
}
