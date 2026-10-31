//! Creation admission and atomic publication of memory records and review proposals.

use super::super::admission::MemoryAdmission;
use super::super::{today, MemoryEntry};
use super::{AddOutcome, MemoryStore, Novelty};

/// A model proposal with engine-validated provenance. Always requires user review.
pub struct ReviewCandidateDraft<'a> {
    pub category: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub body: &'a str,
    pub idempotency_key: &'a str,
    pub provenance: &'a serde_json::Value,
}

impl MemoryStore {
    /// 写入门禁:枚举校验 + description 必填 + 精确标题去重(可 force)+ refs 来源契约
    /// + subject 状态不变量(同 category+subject 至多一条 active,force 不可绕,R-149)。
    #[allow(clippy::too_many_arguments)] // 记忆条目的稳定写入接口；参数均直接映射持久化字段。
    pub fn add(
        &self,
        category: &str,
        title: &str,
        description: &str,
        body: &str,
        source: &str,
        refs: &[String],
        subject: Option<&str>,
        force: bool,
    ) -> anyhow::Result<AddOutcome> {
        self.add_record(
            category,
            title,
            description,
            body,
            source,
            refs,
            subject,
            force,
            &[],
        )
    }

    pub fn add_review_candidate(
        &self,
        draft: ReviewCandidateDraft<'_>,
    ) -> anyhow::Result<AddOutcome> {
        let _lock = self.tree_lock()?;
        if let Some((_, entry)) = self.load_all_checked()?.into_iter().find(|(_, entry)| {
            entry
                .extras
                .iter()
                .any(|(key, value)| key == "proposal_id" && value == draft.idempotency_key)
        }) {
            return Ok(AddOutcome::Duplicate(entry));
        }
        let extras = vec![
            ("proposal_id".into(), draft.idempotency_key.into()),
            ("review_required".into(), "true".into()),
            (
                "provenance".into(),
                serde_json::to_string(draft.provenance)?,
            ),
        ];
        self.add_record(
            draft.category,
            draft.title,
            draft.description,
            draft.body,
            "conversation-dream",
            &[],
            None,
            false,
            &extras,
        )
    }

    #[allow(clippy::too_many_arguments)] // One admission transaction supplies all initial record fields and trusted provenance.
    fn add_record(
        &self,
        category: &str,
        title: &str,
        description: &str,
        body: &str,
        source: &str,
        refs: &[String],
        subject: Option<&str>,
        force: bool,
        metadata: &[(String, String)],
    ) -> anyhow::Result<AddOutcome> {
        for text in [title, description, body, source, subject.unwrap_or("")] {
            super::super::privacy::reject_credentials(text)?;
        }
        for reference in refs {
            super::super::privacy::reject_credentials(reference)?;
        }
        // D-368:整个 add(含 classify_novelty 的 FTS 探测——首次会建 index.db)持记忆
        // 树锁,与 bash 围栏窗口互斥。内层 write_entry/refresh_derived 的 tree_lock
        // 同线程重入,由 FileLock 重入计数放行。
        let _tree_lock = self.tree_lock()?;
        // D-495:写入前先修复已有的派生索引失步,避免 classify_novelty 使用过期 FTS。
        let entries = self.ensure_derived_consistent()?;
        // R-255 第二刀:准入链(枚举/必填/subject 不变式/交付拒收/指纹/判重)提纯到
        // MemoryAdmission,store 只做「查全部条目 + 依结果分流」。
        MemoryAdmission::validate_basic(category, title, description, body)?;
        let title = title.trim();
        let description = description.trim();
        let subject = subject.map(str::trim).filter(|s| !s.is_empty());
        if let Some(existing) = MemoryAdmission::find_subject_conflict(&entries, category, subject)
        {
            return Ok(AddOutcome::SubjectConflict(existing.clone()));
        }
        MemoryAdmission::check_delivery_state(title, subject)?;
        let text = format!("{description}\n{body}");
        let incoming_fps = super::super::fp_markers(&text);
        if !incoming_fps.is_empty() {
            let inbox = self.read_inbox();
            // The fingerprint source set is needed only when the incoming
            // record contains fingerprints. Reuse the checked local generation
            // and read the global source once for provenance and deduplication.
            let global_entries = if self.scope == super::super::MemoryScope::Project {
                MemoryStore::global()
                    .map(|global| global.index_source_entries())
                    .transpose()?
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let existing_fps = entries
                .iter()
                .chain(&global_entries)
                .flat_map(|(_, entry)| {
                    super::super::fp_markers(&format!("{}\n{}", entry.description, entry.body))
                });
            MemoryAdmission::check_fingerprint(&text, &inbox, existing_fps)?;
            if let Some(existing) =
                MemoryAdmission::find_fingerprint_conflict(&entries, &incoming_fps).or_else(|| {
                    MemoryAdmission::find_fingerprint_conflict(&global_entries, &incoming_fps)
                })
            {
                return Ok(AddOutcome::Duplicate(existing.clone()));
            }
        }
        // 语义探测下沉:Uncertain(有 FTS 命中但非精确)即拒并返回候选(force 跳过)。
        if !force {
            let (novelty, candidates) = self.classify_novelty(title, description, body)?;
            if novelty == Novelty::Uncertain {
                let cand: Vec<MemoryEntry> = candidates.into_iter().collect();
                if !cand.is_empty() {
                    return Ok(AddOutcome::Uncertain(cand));
                }
            }
        }
        // 精确 + 近似标题判重(force 跳过;判据含跨 category,见 MemoryAdmission)。
        if let Some(existing) = MemoryAdmission::find_duplicate(&entries, category, title) {
            return Ok(AddOutcome::Duplicate(existing.clone()));
        }
        let now = today();
        let extras = {
            let mut extras: Vec<(String, String)> = metadata.to_vec();
            let refs: Vec<&str> = refs
                .iter()
                .map(|r| r.trim())
                .filter(|r| !r.is_empty())
                .collect();
            if !refs.is_empty() {
                extras.push(("refs".to_string(), refs.join(" ")));
            }
            if let Some(subject) = subject {
                extras.push(("subject".to_string(), subject.to_string()));
            }
            extras
        };
        // R-215:next_id 冲突重试——并发 add 各自基于旧快照算 max,可能同号;
        // 第二个写者发现同 id 前缀文件已存在(被先写者占用)时重新取号,
        // 而不是静默覆盖。乐观并发:不引入跨进程锁,冲突时重试分配。
        let mut id = self.next_id(&entries);
        let mut id_retries = 0u32;
        loop {
            let id_prefix = format!("{}-", id);
            let taken = std::fs::read_dir(&self.root)
                .map(|rd| {
                    rd.flatten()
                        .any(|item| item.file_name().to_string_lossy().starts_with(&id_prefix))
                })
                .unwrap_or(false);
            if !taken || id_retries > 16 {
                break;
            }
            id_retries += 1;
            // 撞号:基于当前磁盘实际条目重新分配。
            let occupied = self.load_all();
            id = self.next_id(&occupied);
        }
        let entry = MemoryEntry {
            id,
            scope: self.scope.label().into(),
            category: category.into(),
            title: title.into(),
            description: description.into(),
            // R-165:source=="user" 是用户直写(最高权证据),直接 active;
            // manager/编译器产物落 candidate,须 memory_promote 带证据晋升。
            status: if source == "user" {
                "active".into()
            } else {
                "candidate".into()
            },
            created: now.clone(),
            updated: now,
            source: source.into(),
            extras,
            body: body.trim().to_string(),
        };
        self.write_entry(&entry, None)?;
        self.refresh_derived()?;
        super::super::record_memory_lifecycle_event(
            self.project_root.as_deref(),
            "memory_candidate_created",
            Some(&entry.id),
            &[],
            Some(source),
            refs,
            "candidate_created",
            None,
            Some(&entry.status),
        );
        Ok(AddOutcome::Added(entry))
    }
}
