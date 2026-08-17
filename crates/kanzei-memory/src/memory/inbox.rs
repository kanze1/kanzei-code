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
use serde::{Deserialize, Serialize};

/// One bounded slice of the inbox sent to a single manager run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxBatch {
    pub text: String,
    pub note_count: usize,
    pub bytes: usize,
    pub estimated_tokens: usize,
}

/// Crash-recoverable progress for the last inbox batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxCheckpoint {
    pub batch_id: String,
    pub status: String,
    pub input_notes: usize,
    pub input_bytes: usize,
    pub success_notes: usize,
    pub pending_after: usize,
    pub failure_reason: Option<String>,
    pub updated_at_ms: u64,
}

fn inbox_note_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.starts_with("## note ") && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        if line.starts_with("## note ") || !current.is_empty() {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

impl MemoryStore {
    pub fn read_inbox(&self) -> String {
        std::fs::read_to_string(self.root.join("inbox.md")).unwrap_or_default()
    }

    /// Read the oldest notes under all three manager-run budgets.
    ///
    /// The first note is always admitted even when it exceeds a budget, so a
    /// single oversized note cannot permanently starve the queue. Successful
    /// manager runs discard individual notes; the next call therefore resumes
    /// from the first remaining note without relying on a fragile byte cursor.
    pub fn read_inbox_batch(
        &self,
        max_notes: usize,
        max_bytes: usize,
        max_tokens: usize,
    ) -> Option<InboxBatch> {
        let _lock = self.tree_lock().ok()?;
        let text = std::fs::read_to_string(self.root.join("inbox.md")).ok()?;
        let mut selected = Vec::new();
        let mut bytes = 0;
        let mut estimated_tokens = 0;
        for block in inbox_note_blocks(&text) {
            let separator_bytes = usize::from(!selected.is_empty());
            let block_bytes = block.len();
            let block_tokens = block.chars().count().div_ceil(4).max(1);
            let separator_tokens = usize::from(!selected.is_empty());
            let over_budget = !selected.is_empty()
                && (selected.len() >= max_notes.max(1)
                    || bytes + separator_bytes + block_bytes > max_bytes.max(1)
                    || estimated_tokens + separator_tokens + block_tokens > max_tokens.max(1));
            if over_budget {
                break;
            }
            bytes += separator_bytes + block_bytes;
            estimated_tokens += separator_tokens + block_tokens;
            selected.push(block);
        }
        if selected.is_empty() {
            return None;
        }
        Some(InboxBatch {
            text: selected.join("\n"),
            note_count: selected.len(),
            bytes,
            estimated_tokens,
        })
    }

    fn inbox_checkpoint_path(&self) -> PathBuf {
        self.root.join("inbox.checkpoint.json")
    }

    fn record_inbox_write_log(&self, path: &PathBuf, content: &[u8]) {
        let Some(project_root) = &self.project_root else {
            return;
        };
        let Ok(relative) = path.strip_prefix(project_root) else {
            return;
        };
        let _ = kanzei_base::write_log::record(
            project_root,
            &kanzei_base::write_log::WriteLogEntry {
                at_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or_default(),
                path: relative.display().to_string().replace('\\', "/"),
                fingerprint: kanzei_base::content_hash(content),
                content: content.to_vec(),
                run_id: None,
                process_id: None,
            },
        );
    }

    pub fn write_inbox_checkpoint(&self, checkpoint: &InboxCheckpoint) -> anyhow::Result<()> {
        let _lock = self.tree_lock()?;
        let text = serde_json::to_string_pretty(checkpoint)? + "\n";
        crate::atomic_file::write_atomic(&self.inbox_checkpoint_path(), &text)?;
        self.record_inbox_write_log(&self.inbox_checkpoint_path(), text.as_bytes());
        Ok(())
    }

    pub fn read_inbox_checkpoint(&self) -> Option<InboxCheckpoint> {
        let text = std::fs::read_to_string(self.inbox_checkpoint_path()).ok()?;
        serde_json::from_str(&text).ok()
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
            self.record_inbox_write_log(&path, b"# Memory Inbox\n");
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
        self.record_inbox_write_log(&path, text.as_bytes());
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
        let path = self.root.join("inbox.md");
        crate::atomic_file::write_atomic(&path, &next)?;
        self.record_inbox_write_log(&path, next.as_bytes());
        Ok(removed)
    }

    pub fn pending_notes(&self) -> usize {
        std::fs::read_to_string(self.root.join("inbox.md"))
            .map(|t| t.lines().filter(|l| l.starts_with("## note ")).count())
            .unwrap_or(0)
    }
}
