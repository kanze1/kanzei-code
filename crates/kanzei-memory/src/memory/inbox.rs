//! 记忆收件箱(R-255 第一刀,纯搬迁自 store.rs)。
//!
//! 独立理由:收件箱是「主 agent 的草稿投递 + manager 消化」的独立变更理由——`memory_note`
//! 投递(memory_note 工具)、SOP 候选逐条可见(pending_note_list)、按指纹丢弃
//! (discard_note)与整箱清空(clear_inbox)互不相关。它与准入(add)、生命周期
//! (promote)、检索(search)正交:改草稿格式不必读懂准入策略(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):inbox 读-拼-写回必须持记忆树锁(R-215/D-368)——与 append_note/
//! discard_note/clear_inbox 共用同一把锁,避免并发 append 互吃;树锁还与 bash 围栏
//! 的收口共享锁互斥,确保 after 快照不读到写事务中间态。

use std::path::PathBuf;

use super::store::MemoryStore;
use super::today;
use serde::{Deserialize, Serialize};

/// One bounded slice of the inbox sent to a single manager run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxBatch {
    pub text: String,
    pub note_ids: Vec<String>,
    pub note_count: usize,
    pub bytes: usize,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct InboxNote {
    pub id: String,
    pub hint: String,
    pub summary: String,
    pub detail: String,
}

/// Engine-assigned provenance, outside the escaped untrusted note body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InboxSource {
    Conversation {
        sha256: String,
        chunk_index: usize,
        chunk_bytes: usize,
    },
}

pub struct SourceDraftNote {
    pub id: String,
    pub summary: String,
    pub detail: String,
    pub source: InboxSource,
}

impl InboxBatch {
    pub fn source(&self) -> anyhow::Result<Option<InboxSource>> {
        let blocks = inbox_note_blocks(&self.text);
        let sources = blocks
            .iter()
            .filter_map(|block| {
                block
                    .lines()
                    .find_map(|line| line.strip_prefix("- source-json: "))
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(
            sources.is_empty() || (sources.len() == 1 && blocks.len() == 1),
            "有来源的片段不能与其它草稿合并"
        );
        sources
            .first()
            .map(|source| serde_json::from_str(source).map_err(Into::into))
            .transpose()
    }
    pub fn notes(&self) -> Vec<(String, String)> {
        inbox_note_blocks(&self.text)
            .iter()
            .map(|block| (note_id(block), note_summary(block).to_owned()))
            .collect()
    }
}

/// Crash-recoverable progress for the last inbox batch.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryJobStatus {
    Running,
    Cancelling,
    Completed,
    Busy,
    Incomplete,
    Cancelled,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryJobProgress {
    pub id: String,
    #[serde(default)]
    pub request_id: Option<String>,
    pub owner_pid: u32,
    pub status: MemoryJobStatus,
    pub started_at_ms: u64,
    pub steps: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub usage_reported: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxCheckpoint {
    pub batch_id: String,
    pub status: String,
    pub input_notes: usize,
    pub input_bytes: usize,
    pub success_notes: usize,
    pub pending_after: usize,
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub consecutive_failures: usize,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub job: Option<MemoryJobProgress>,
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

fn note_id(block: &str) -> String {
    block
        .lines()
        .find_map(|line| line.strip_prefix("- note_id: "))
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "legacy-{}",
                kanzei_base::content_hash(block.trim().as_bytes())
            )
        })
}

fn note_summary(block: &str) -> &str {
    block
        .lines()
        .find_map(|line| line.strip_prefix("- summary: "))
        .unwrap_or("")
}

fn inbox_header(text: &str) -> String {
    let header = text
        .lines()
        .take_while(|line| !line.starts_with("## note "))
        .collect::<Vec<_>>()
        .join("\n");
    if header.trim().is_empty() {
        "# Memory Inbox\n".into()
    } else {
        format!("{}\n", header.trim_end())
    }
}

fn has_source(block: &str) -> bool {
    block
        .lines()
        .any(|line| line.starts_with("- source-json: "))
}

/// Identity first; old callers may use a unique summary or its bracketed fingerprint.
/// Body text is never a selector. Ambiguity must not delete several notes.
fn selected_note(blocks: &[String], selector: &str) -> anyhow::Result<Option<usize>> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Ok(None);
    }
    let exact = blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| note_id(block) == selector)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let matches = if exact.is_empty() {
        blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| note_summary(block).contains(selector))
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    } else {
        exact
    };
    anyhow::ensure!(matches.len() <= 1, "草稿标识不唯一，请使用 note_id");
    Ok(matches.first().copied())
}

impl MemoryStore {
    /// Queue the complete frozen source in one file transaction. Resolved IDs remain
    /// in the same file, so a crash or repeated dream cannot resurrect consumed chunks.
    pub fn queue_source_notes(&self, notes: &[SourceDraftNote]) -> anyhow::Result<Vec<String>> {
        std::fs::create_dir_all(&self.root)?;
        let _lock = self.tree_lock()?;
        let mut text = self.read_inbox_checked()?;
        if text.trim().is_empty() {
            text = "# Memory Inbox\n".into();
        }
        let blocks = inbox_note_blocks(&text);
        let header = inbox_header(&text);
        let resolved: std::collections::HashSet<_> = header
            .lines()
            .filter_map(|line| line.strip_prefix("- resolved_note_id: "))
            .collect();
        let mut queued = Vec::new();
        let mut unique = std::collections::HashSet::new();
        for note in notes {
            anyhow::ensure!(unique.insert(note.id.as_str()), "来源草稿不能有重复标识");
            anyhow::ensure!(
                !note.id.is_empty()
                    && note
                        .id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
                "来源草稿标识无效"
            );
            anyhow::ensure!(!note.summary.contains(['\n', '\r']), "来源摘要必须单行");
            for value in [&note.summary, &note.detail] {
                super::privacy::reject_credentials(value)?;
            }
            let block = format!("## note {} [conversation]\n- note_id: {}\n- summary: {}\n- source-json: {}\n- detail-json: {}\n",today(),note.id,note.summary,serde_json::to_string(&note.source)?,serde_json::to_string(&note.detail)?);
            if let Some(existing) = blocks.iter().find(|block| note_id(block) == note.id) {
                // Ignore presentation date; source identity and exact detail must stay frozen.
                anyhow::ensure!(
                    existing
                        .lines()
                        .skip(1)
                        .filter(|line| !line.trim().is_empty())
                        .eq(block.lines().skip(1).filter(|line| !line.trim().is_empty())),
                    "冻结草稿内容已改变"
                );
                queued.push(note.id.clone());
            } else if !resolved.contains(note.id.as_str()) {
                text.push('\n');
                text.push_str(&block);
                queued.push(note.id.clone());
            }
        }
        let path = self.root.join("inbox.md");
        crate::atomic_file::write_atomic(&path, &text)?;
        self.record_write_log(&path, Some(text.into_bytes()))?;
        Ok(queued)
    }
    pub fn read_inbox(&self) -> String {
        std::fs::read_to_string(self.root.join("inbox.md")).unwrap_or_default()
    }

    pub fn read_inbox_checked(&self) -> anyhow::Result<String> {
        match std::fs::read_to_string(self.root.join("inbox.md")) {
            Ok(text) => Ok(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error.into()),
        }
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
        self.bounded_inbox_batch(None, max_notes, max_bytes, max_tokens)
    }

    pub fn read_inbox_batch_for(
        &self,
        note_ids: &[String],
        max_notes: usize,
        max_bytes: usize,
        max_tokens: usize,
    ) -> Option<InboxBatch> {
        self.bounded_inbox_batch(Some(note_ids), max_notes, max_bytes, max_tokens)
    }

    pub fn pending_note_ids_checked(&self) -> anyhow::Result<Vec<String>> {
        let _lock = self.tree_lock()?;
        Ok(inbox_note_blocks(&self.read_inbox_checked()?)
            .iter()
            .map(|block| note_id(block))
            .collect())
    }

    fn bounded_inbox_batch(
        &self,
        note_ids: Option<&[String]>,
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
            if note_ids.is_some_and(|ids| !ids.contains(&note_id(&block))) {
                continue;
            }
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
            let sourced = has_source(&block);
            if sourced && !selected.is_empty() {
                break;
            }
            bytes += separator_bytes + block_bytes;
            estimated_tokens += separator_tokens + block_tokens;
            selected.push(block);
            if sourced {
                break;
            }
        }
        if selected.is_empty() {
            return None;
        }
        Some(InboxBatch {
            note_ids: selected.iter().map(|block| note_id(block)).collect(),
            text: selected.join("\n"),
            note_count: selected.len(),
            bytes,
            estimated_tokens,
        })
    }

    /// 取包含 `needle` 的第一条完整草稿,作为单条批次(界面逐条「采纳」用)。
    /// needle 为空或没命中返回 None(空 needle 会命中所有块,绝不能当「全部」)。
    /// 不套预算:这条是用户点名要整理的,再大也只有一条。
    pub fn read_inbox_note(&self, needle: &str) -> Option<InboxBatch> {
        let needle = needle.trim();
        if needle.is_empty() {
            return None;
        }
        let _lock = self.tree_lock().ok()?;
        let text = std::fs::read_to_string(self.root.join("inbox.md")).ok()?;
        let blocks = inbox_note_blocks(&text);
        let block = blocks.get(selected_note(&blocks, needle).ok()??)?.clone();
        Some(InboxBatch {
            note_ids: vec![note_id(&block)],
            note_count: 1,
            bytes: block.len(),
            estimated_tokens: block.chars().count().div_ceil(4).max(1),
            text: block,
        })
    }

    fn inbox_checkpoint_path(&self) -> PathBuf {
        self.root.join("inbox.checkpoint.json")
    }

    pub fn write_inbox_checkpoint(&self, checkpoint: &InboxCheckpoint) -> anyhow::Result<()> {
        let _lock = self.tree_lock()?;
        std::fs::create_dir_all(&self.root)?;
        let file = self.scoped_checkpoint()?;
        let mut checkpoint = checkpoint.clone();
        checkpoint.failure_reason = checkpoint
            .failure_reason
            .as_deref()
            .map(super::redact_credentials);
        if let Some(job) = checkpoint.job.as_mut() {
            job.error = job.error.as_deref().map(super::redact_credentials);
        }
        let text = serde_json::to_string_pretty(&checkpoint)? + "\n";
        file.write_atomic(text.as_bytes(), || Ok(()))?;
        self.record_write_log(&self.inbox_checkpoint_path(), Some(text.into_bytes()))?;
        Ok(())
    }

    pub fn read_inbox_checkpoint(&self) -> Option<InboxCheckpoint> {
        self.read_inbox_checkpoint_checked().ok().flatten()
    }

    pub fn read_inbox_checkpoint_checked(&self) -> anyhow::Result<Option<InboxCheckpoint>> {
        if !self.root.try_exists()? {
            return Ok(None);
        }
        let text = match self.scoped_checkpoint()?.read_to_string() {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        Ok(Some(serde_json::from_str(&text)?))
    }

    fn scoped_checkpoint(&self) -> anyhow::Result<kanzei_harness::permission::ScopedFile> {
        let root = self.project_root.as_deref().unwrap_or(&self.root);
        let path = self.inbox_checkpoint_path();
        let file = kanzei_harness::permission::ScopedFile::resolve(root, root, &path)?;
        anyhow::ensure!(
            kanzei_harness::permission::FileResource::resolve(root, &path)?
                .sensitivity()
                .is_none(),
            "记忆检查点不能指向敏感文件"
        );
        Ok(file)
    }

    /// manager 消化完毕后清空草稿箱(整箱内容已在触发 prompt 里,清空即"已消费")。
    pub fn clear_inbox(&self) -> anyhow::Result<()> {
        let path = self.root.join("inbox.md");
        // R-215 语义不变 + D-368:改用记忆树锁——与 append_note/discard_note 共用
        // 同一把锁,整箱清空锁内执行避免与并发 append 交错(append 持锁读-拼-写回,
        // clear 持锁覆盖,不会互吃);同时树锁与 bash 围栏收口共享锁互斥。
        let _lock = self.tree_lock()?;
        if path.is_file() {
            let previous = self.read_inbox_checked()?;
            let mut next = inbox_header(&previous);
            for block in inbox_note_blocks(&previous)
                .iter()
                .filter(|block| has_source(block))
            {
                next.push_str(&format!("- resolved_note_id: {}\n", note_id(block)));
            }
            crate::atomic_file::write_atomic(&path, &next)?;
            self.record_write_log(&path, Some(next.into_bytes()))?;
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
        self.append_note_with_audit(summary, detail, category_hint, refs, || {
            super::record_memory_lifecycle_event(
                self.project_root.as_deref(),
                "memory_note_queued",
                None,
                &[],
                refs.first().map(String::as_str),
                refs,
                "note_queued",
                None,
                Some("inbox"),
            );
        })
    }

    /// 把 inbox 内容事务与后置生命周期审计的锁边界固定在同一个可测入口。
    fn append_note_with_audit(
        &self,
        summary: &str,
        detail: &str,
        category_hint: &str,
        refs: &[String],
        audit: impl FnOnce(),
    ) -> anyhow::Result<PathBuf> {
        for text in [summary, detail, category_hint] {
            super::privacy::reject_credentials(text)?;
        }
        for reference in refs {
            super::privacy::reject_credentials(reference)?;
        }
        anyhow::ensure!(
            !summary.contains(['\r', '\n'])
                && !category_hint.contains(['\r', '\n'])
                && refs.iter().all(|value| !value.contains(['\r', '\n'])),
            "草稿摘要、分类和引用必须各自保持单行"
        );
        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join("inbox.md");
        // R-215 语义不变 + D-368:改用记忆树锁——读-拼接-写回整体持锁,并发 append
        // 各读各的再各自写回时后写者不会覆盖先写者(note 无痕丢失)。临界区只罩住
        // 内容事务及其围栏归因日志;SQLite 生命周期审计与 inbox 内容互不构成原子
        // 不变量,必须在释放树锁后执行。否则 12 个并发写者会把冷 CI 的数据库打开
        // 成本串进同一把锁,末尾写者等待超过 3s(D-604)。
        let tree_lock = self.tree_lock()?;
        let mut text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "# Memory Inbox\n".into(),
            Err(error) => return Err(error.into()),
        };
        let note_id = format!(
            "note-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
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
            "\n## note {} {}\n- note_id: {note_id}\n- summary: {}\n{}{}",
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
                format!("- detail-json: {}\n", serde_json::to_string(detail.trim())?)
            },
        ));
        crate::atomic_file::write_atomic(&path, &text)?;
        self.record_write_log(&path, Some(text.into_bytes()))?;
        drop(tree_lock);
        audit();
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
    pub fn pending_note_records(&self) -> Vec<InboxNote> {
        inbox_note_blocks(&self.read_inbox())
            .into_iter()
            .map(|block| {
                let hint = block
                    .lines()
                    .next()
                    .and_then(|head| head.split_once('['))
                    .and_then(|(_, rest)| rest.split_once(']'))
                    .map(|(hint, _)| hint.to_owned())
                    .unwrap_or_default();
                let mut detail = Vec::new();
                for line in block.lines().skip(1) {
                    if line.starts_with("- summary: ") || line.starts_with("- note_id: ") {
                        continue;
                    }
                    if let Some(value) = line.strip_prefix("- detail-json: ") {
                        detail.push(
                            serde_json::from_str::<String>(value)
                                .unwrap_or_else(|_| line.to_owned()),
                        );
                    } else if !line.trim().is_empty() {
                        detail.push(line.to_owned());
                    }
                }
                InboxNote {
                    id: note_id(&block),
                    hint,
                    summary: note_summary(&block).to_owned(),
                    detail: detail.join("\n"),
                }
            })
            .collect()
    }

    pub fn pending_note_list(&self) -> Vec<(String, String, String)> {
        self.pending_note_records()
            .into_iter()
            .map(|note| (note.hint, note.summary, note.detail))
            .collect()
    }
    /// 丢弃一条草稿(按其摘要里的指纹定位)。用户说不要的候选不该再进 manager 的消化范围。
    pub fn discard_note(&self, fingerprint: &str) -> anyhow::Result<bool> {
        self.discard_note_within(fingerprint, None)
    }

    pub fn discard_batch_note(
        &self,
        fingerprint: &str,
        note_ids: &[String],
    ) -> anyhow::Result<bool> {
        self.discard_note_within(fingerprint, Some(note_ids))
    }

    pub fn consumed_batch_notes(&self, batch: &InboxBatch) -> anyhow::Result<usize> {
        let present = inbox_note_blocks(&self.read_inbox_checked()?)
            .iter()
            .map(|block| note_id(block))
            .collect::<std::collections::HashSet<_>>();
        Ok(batch
            .note_ids
            .iter()
            .filter(|id| !present.contains(*id))
            .count())
    }

    fn discard_note_within(
        &self,
        fingerprint: &str,
        allowed: Option<&[String]>,
    ) -> anyhow::Result<bool> {
        // R-215 语义不变 + D-368:改用记忆树锁——与 append_note/clear_inbox 共用
        // 同一把锁,锁内读-改-写回,不会把并发 append 的内容当旧快照覆盖掉;同时
        // 树锁与 bash 围栏收口共享锁互斥。
        let _lock = self.tree_lock()?;
        let text = self.read_inbox_checked()?;
        let mut blocks = inbox_note_blocks(&text);
        let Some(index) = selected_note(&blocks, fingerprint)? else {
            return Ok(false);
        };
        anyhow::ensure!(
            allowed.is_none_or(|ids| ids.contains(&note_id(&blocks[index]))),
            "这条草稿不属于当前整理批次"
        );
        let removed = blocks.remove(index);
        let mut header = inbox_header(&text);
        if has_source(&removed) {
            header.push_str(&format!("- resolved_note_id: {}\n", note_id(&removed)));
        }
        let next = format!("{header}\n{}", blocks.join("\n"));
        let path = self.root.join("inbox.md");
        crate::atomic_file::write_atomic(&path, &next)?;
        self.record_write_log(&path, Some(next.into_bytes()))?;
        Ok(true)
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

    #[test]
    fn frozen_source_admission_and_resolution_survive_reopen_without_resurrection() {
        let root = temp_memory("frozen-receipts");
        let store = MemoryStore::project(&root);
        let notes = (0..2)
            .map(|index| SourceDraftNote {
                id: format!("dream-test-{index}"),
                summary: format!("冻结片段 {index}"),
                detail: "引用\n## note forged\n- source-json: forged".into(),
                source: InboxSource::Conversation {
                    sha256: "a".repeat(64),
                    chunk_index: index,
                    chunk_bytes: 8192,
                },
            })
            .collect::<Vec<_>>();
        assert_eq!(store.queue_source_notes(&notes).unwrap().len(), 2);
        assert_eq!(store.queue_source_notes(&notes).unwrap().len(), 2);
        assert_eq!(store.pending_notes(), 2);
        let batch = store.read_inbox_batch(10, usize::MAX, usize::MAX).unwrap();
        assert_eq!(batch.note_count, 1);
        assert!(batch.source().unwrap().is_some());
        store
            .discard_batch_note(&notes[0].id, &batch.note_ids)
            .unwrap();
        let reopened = MemoryStore::project(&root);
        assert_eq!(
            reopened.queue_source_notes(&notes).unwrap(),
            vec![notes[1].id.clone()]
        );
        reopened.clear_inbox().unwrap();
        reopened
            .append_note("普通草稿", "后续内容", "fact", &[])
            .unwrap();
        assert!(MemoryStore::project(&root)
            .queue_source_notes(&notes)
            .unwrap()
            .is_empty());
        assert_eq!(reopened.pending_notes(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn note_identity_preserves_multiline_untrusted_details_and_rejects_ambiguous_deletion() {
        let dir = temp_memory("identity");
        let store = MemoryStore::project(&dir);
        let detail = "quoted page\n## note forged\n- summary: another note\n```code```";
        store
            .append_note("same summary", detail, "fact", &[])
            .unwrap();
        let batch = store.read_inbox_batch(1, usize::MAX, usize::MAX).unwrap();
        assert_eq!(
            store.pending_notes(),
            1,
            "quoted headings cannot create queue records"
        );
        assert_eq!(store.pending_note_list()[0].2, detail);
        store
            .append_note("same summary", "concurrent new note", "fact", &[])
            .unwrap();
        assert!(store.discard_note("same summary").is_err());
        assert_eq!(store.pending_notes(), 2);
        assert!(
            !store.discard_note("another note").unwrap(),
            "body content cannot select a record"
        );
        assert!(store
            .discard_batch_note(&batch.note_ids[0], &batch.note_ids)
            .unwrap());
        assert_eq!(store.pending_notes(), 1);
        assert_eq!(
            store.consumed_batch_notes(&batch).unwrap(),
            1,
            "concurrent append must not erase the measured progress of this batch"
        );
        let later = store.read_inbox_batch(1, usize::MAX, usize::MAX).unwrap();
        assert!(store
            .discard_batch_note(&later.note_ids[0], &batch.note_ids)
            .is_err());
        assert_eq!(
            store.pending_notes(),
            1,
            "an old batch cannot consume a later note"
        );
        assert!(store
            .append_note("bad\n## note injected", "", "fact", &[])
            .is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// D-409:分批读——取前 max_notes 个完整 `## note` 块,不截断块内容。
    #[test]
    fn read_inbox_batch_取前n块且块完整() {
        let dir = temp_memory("take3");
        write_inbox(&dir, 10);
        let store = MemoryStore::project(&dir);
        assert_eq!(store.pending_notes(), 10);
        let batch = store
            .read_inbox_batch(3, usize::MAX, usize::MAX)
            .expect("非空 inbox 必有批次");
        assert_eq!(batch.note_count, 3, "取 3 块");
        assert!(batch.text.contains("## note 0"), "首块在");
        assert!(batch.text.contains("第 0 条"));
        assert!(batch.text.contains("## note 2"), "第三块在");
        assert!(!batch.text.contains("## note 3"), "第四块不在(只取前 3)");
        // 块完整:detail 与 summary 都在。
        assert!(
            batch.text.contains("- detail: 内容 2"),
            "第三块 detail 完整"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-409:max_notes 超块数取全部;预算为 0 时首块仍放行(超大条目不得饿死队列);
    /// 空 inbox 返回 None。
    #[test]
    fn read_inbox_batch_边界() {
        let dir = temp_memory("edge");
        write_inbox(&dir, 4);
        let store = MemoryStore::project(&dir);
        let all = store
            .read_inbox_batch(100, usize::MAX, usize::MAX)
            .expect("非空 inbox 必有批次");
        assert_eq!(all.note_count, 4, "超块数取全部");
        assert_eq!(all.text.matches("## note ").count(), 4);
        let first_always = store
            .read_inbox_batch(0, 0, 0)
            .expect("首块总是放行,单条超预算不得永久饿死队列");
        assert_eq!(first_always.note_count, 1);
        // 空 inbox。
        let dir2 = temp_memory("empty");
        let store2 = MemoryStore::project(&dir2);
        assert!(store2
            .read_inbox_batch(10, usize::MAX, usize::MAX)
            .is_none());
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&dir2).ok();
    }

    /// 界面逐条「采纳」:只取点名的那一条;空 needle 与没命中都不得退化成「整箱」,
    /// 空指纹的 discard 也不得清空收件箱。
    #[test]
    fn read_inbox_note_只取点名那条且空needle不取整箱() {
        let dir = temp_memory("single");
        write_inbox(&dir, 4);
        let store = MemoryStore::project(&dir);
        let one = store.read_inbox_note("第 2 条").expect("命中第三块");
        assert_eq!(one.note_count, 1);
        assert!(one.text.contains("## note 2") && one.text.contains("- detail: 内容 2"));
        assert!(!one.text.contains("## note 1") && !one.text.contains("## note 3"));
        assert!(store.read_inbox_note("").is_none());
        assert!(store.read_inbox_note("   ").is_none());
        assert!(store.read_inbox_note("不存在的摘要").is_none());
        assert!(!store.discard_note("").unwrap(), "空指纹不丢任何草稿");
        assert_eq!(store.pending_notes(), 4);
        assert!(store.discard_note("第 2 条").unwrap());
        assert_eq!(store.pending_notes(), 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-604 回归:生命周期审计可能因 state.db 写锁等待数秒,但 inbox 内容已经原子
    /// 落盘并记入围栏写日志后,这段等待不得继续占用 memory 树锁。否则其它
    /// memory_note 会在统一 3s 预算内形成锁车队并随机失败。
    #[test]
    fn 慢生命周期审计不占用memory树锁() {
        let dir = temp_memory("slow-lifecycle-audit");
        let store = std::sync::Arc::new(MemoryStore::project(&dir));
        let (audit_started_tx, audit_started_rx) = std::sync::mpsc::channel();
        let (audit_release_tx, audit_release_rx) = std::sync::mpsc::channel();
        let writer_store = store.clone();
        let writer = std::thread::spawn(move || {
            writer_store.append_note_with_audit("慢审计测试", "", "fact", &[], || {
                audit_started_tx.send(()).unwrap();
                audit_release_rx.recv().unwrap();
            })
        });

        audit_started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("内容事务后应进入生命周期审计");
        let tree_lock = crate::atomic_file::try_lock_exclusive(
            &crate::memory::project_memory_root(&dir),
            std::time::Duration::from_millis(500),
        )
        .unwrap();
        audit_release_tx.send(()).unwrap();
        writer.join().unwrap().unwrap();
        assert!(
            tree_lock.is_some(),
            "内容已落盘后,慢 SQLite 审计不得继续占用 memory 树锁"
        );
        drop(tree_lock);
        assert_eq!(store.pending_notes(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
