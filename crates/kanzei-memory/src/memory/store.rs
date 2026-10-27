//! MemoryStore:单 scope 的记忆仓库。
//! 硬门禁在写入侧:ID 引擎分配、枚举校验、description 必填、精确重复拒绝;
//! INDEX.md 与 index.db(FTS5/hits)都是派生物,损坏可由文件全量重建;
//! 写入在记忆树锁内完成,使用 tmp+rename 原子替换并刷新派生索引。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use super::admission::topic_overlap;
use super::lifecycle::MemoryLifecycle;
pub(crate) use super::retrieval::{intent_query, segment_cjk};
use super::{date_days, render_entry, today, MemoryEntry, MemoryScope, STATUSES};

// 归档读取、归档搬移与记忆图谱的 `area:` 字段(拆出以控制本文件体量,见 docs/design/metrics_baseline.md 回涨闸)。
mod archive;
mod creation;
mod source;

pub use creation::ReviewCandidateDraft;

/// 检索结果(含派生指标)。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub entry: MemoryEntry,
    pub path: PathBuf,
    pub snippet: String,
    pub hits: u64,
    pub score: f64,
}

/// FTS 候选(存储侧原始命中):bm25 相关度随候选返回,不含任何决策加权。
/// 排序/加权决策由检索门面(SqliteMemoryIndex)统一实现(D-366 边界切净)。
#[derive(Debug, Clone)]
pub struct SearchCandidate {
    pub entry: MemoryEntry,
    pub path: PathBuf,
    pub snippet: String,
    pub hits: u64,
    /// FTS5 bm25 原始相关度(负值,越小越相关)。检索侧取负并叠加决策权重。
    pub bm25: f64,
}

/// 一次开跑预检索的完整明细(R-125):召回了什么、为什么召回、注入了多少。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecallRound {
    pub recall_id: String,
    pub at: i64,
    pub prompt_head: String,
    pub injected_bytes: usize,
    pub hits: Vec<RecallHit>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RecallHit {
    pub id: String,
    pub title: String,
    pub scope: String,
    pub category: String,
    pub score: f64,
    pub snippet: String,
    /// 召回之后正文是否真的被拉取过——「是否产生作用」的机械判据。
    pub fetched: bool,
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// add 的去重门禁结果。
#[derive(Debug)]
pub enum AddOutcome {
    Added(MemoryEntry),
    /// 精确标题重复:拒绝写入并返回既有条目(要求转 update 或 force)。
    Duplicate(MemoryEntry),
    /// 同 scope+category+subject 至多一条 active/candidate，冲突由准入侧判定。
    /// 冲突返回既有条目,force 不可绕——状态就地覆盖(memory_update),绝不并存。
    SubjectConflict(MemoryEntry),
    /// R-216:语义探测不确定(有 FTS 命中但非精确)——拒绝写入并返回候选条目,
    /// 要求先用 memory_update 更新既有条目或明确 force。近似去重的硬闸。
    Uncertain(Vec<MemoryEntry>),
}

/// R-165 批2 novelty gate 三档分流结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Novelty {
    /// 明显新:与既有 active 记忆无重叠,直接 PROPOSE。
    New,
    /// 明显重复:标题规范化精确命中既有 active 记忆,NOOP。
    Duplicate,
    /// 不确定:有语义命中但非精确,才起 LLM 判断。
    Uncertain,
}

/// 一轮 candidate 自动处置的可审计结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateReconcileReport {
    pub retention: super::MemoryRetentionReport,
    pub candidate_files_before: usize,
    pub candidate_files_after: usize,
    pub candidate_index_before: usize,
    pub candidate_index_after: usize,
    pub promoted: Vec<String>,
    pub deprecated: Vec<String>,
    pub untouched: Vec<String>,
    pub merged: Vec<String>,
    /// B4:一次性 global candidate 复核的审计投影。
    pub global_candidate_files_before: usize,
    pub global_candidate_files_after: usize,
    pub global_reviewed: usize,
    pub global_deprecated: usize,
    pub global_recall_rows: usize,
    pub global_review_already_recorded: bool,
}

impl Novelty {
    pub fn as_str(&self) -> &'static str {
        match self {
            Novelty::New => "new",
            Novelty::Duplicate => "duplicate",
            Novelty::Uncertain => "uncertain",
        }
    }
}

/// 单个 scope 的记忆源条目与派生索引仓库。检索排序由 SqliteMemoryIndex 负责。
pub struct MemoryStore {
    pub scope: MemoryScope,
    pub root: PathBuf,
    /// R-268:项目主根(写日志用,global scope 为 None——global 记忆不在任何项目的
    /// 托管围栏内,无需记写日志)。
    pub project_root: Option<PathBuf>,
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        MemoryStore {
            scope: self.scope,
            root: self.root.clone(),
            project_root: self.project_root.clone(),
        }
    }
}

impl MemoryStore {
    pub fn open(scope: MemoryScope, root: PathBuf) -> Self {
        MemoryStore {
            scope,
            root,
            project_root: None,
        }
    }

    pub fn project(project_root: &Path) -> Self {
        let store = MemoryStore::open(
            MemoryScope::Project,
            super::project_memory_root(project_root),
        );
        // 保持路径句柄构造的兼容接口;迁移失败必须显式诊断,原始来源仍保留供重入。
        if let Err(error) = store.migrate_legacy(project_root) {
            tracing::error!(
                project_root = %project_root.display(),
                legacy_path = %project_root.join(".kanzei/project/memory.md").display(),
                error = %format!("{error:#}"),
                "旧记忆迁移未完成；源文件已保留，下次打开会重新检查"
            );
        }
        MemoryStore {
            scope: store.scope,
            root: store.root,
            project_root: Some(project_root.to_path_buf()),
        }
    }

    pub fn global() -> Option<Self> {
        Some(MemoryStore::open(
            MemoryScope::Global,
            super::global_memory_root()?,
        ))
    }

    pub fn global_for_project(project_root: &Path) -> Option<Self> {
        let mut store = Self::global()?;
        store.project_root = Some(project_root.to_path_buf());
        Some(store)
    }

    /// D-368:记忆树互斥锁(跨进程 + 跨线程)。锁目标 = 记忆根目录本身；
    /// 协调文件由 atomic_file 按完整物理路径派生，统一放在用户缓存中。
    ///
    /// bash 围栏收口(kanzei-tools managed.rs `acquire_managed_locks`)短暂取同一目标的
    /// 共享锁——确保 after 快照不会读到 memory 写入(条目/INDEX.md/inbox.md/
    /// voided-ids.md/归档搬移/index.db 重建)的中间态;超过预算(默认 3s)明确报错。
    ///
    /// 为什么锁目录而不是逐文件:动态条目文件(M-xxx.md、inbox.md)创建前无法预锁,
    /// 锁整树一劳永逸;写操作毫秒级,持有窗口极短。协调文件不落在工作区，
    /// 不进入围栏镜像，也不影响项目根发现。
    pub(crate) fn tree_lock(&self) -> anyhow::Result<crate::atomic_file::FileLock> {
        Ok(crate::atomic_file::lock_exclusive(&self.root)?)
    }

    fn archive_dir(&self) -> PathBuf {
        self.root.join("archive")
    }

    fn index_md(&self) -> PathBuf {
        self.root.join("INDEX.md")
    }

    fn db_path(&self) -> PathBuf {
        self.root.join("index.db")
    }

    /// 扫描根目录(不含 archive/)加载有效条目。被跳过的读取/格式问题由
    /// `integrity_issues` 重新扫描报告；需要完整快照的调用方使用 `load_all_checked`。
    pub fn load_all(&self) -> Vec<(PathBuf, MemoryEntry)> {
        source::scan(&self.root).entries
    }

    /// 演化:改内容/钩子/状态;created 与 extras 保留,updated 刷新。
    ///
    /// D-282 两道守卫:
    /// ①**description 主题一致性**——新 description 必须与条目现有主题
    ///   (title + 旧 description + body)有共同 token;交集为空说明疑似选错条目
    ///   或主题漂移(实测:manager 把 tracker 字段语义条目 M-044 的 description
    ///   换成 edit 主题内容),拒绝并给出旧/新对照(兼做 manager 的复盘轨迹)。
    /// ②**CAS(expected_hash)**——与 conventions 工具同源:调用方先拿到条目
    ///   当前渲染内容的 hash,update 时带回来校验;不一致说明期间有别的写,
    ///   拒绝。用户定调 memory 不做跨进程锁(竞争留给 agent 事后解决),CAS
    ///   是乐观并发保护,不引入锁。不传 expected_hash 则跳过(CAS 可选)。
    #[allow(clippy::too_many_arguments)] // id+4 内容字段+hash+开关,D-282 需要全参
    pub fn update(
        &self,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        body: Option<&str>,
        status: Option<&str>,
        expected_hash: Option<&str>,
        enforce_topic: bool,
    ) -> anyhow::Result<MemoryEntry> {
        self.update_with_area(
            id,
            title,
            description,
            body,
            status,
            None,
            expected_hash,
            enforce_topic,
        )
    }

    /// update 加记忆图谱的 `area:`(Some(&[]) 清除,None 不动):内容与区域一次写盘、一次
    /// refresh_derived,expected_hash 对「只改区域」同样生效(复核:area-only 曾绕过 CAS、
    /// 同时改内容与区域是两次独立写盘)。整段 读 → CAS → 写 持记忆树锁(同线程重入)。
    #[allow(clippy::too_many_arguments)] // update 的全参 + area
    pub fn update_with_area(
        &self,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        body: Option<&str>,
        status: Option<&str>,
        area: Option<&[String]>,
        expected_hash: Option<&str>,
        enforce_topic: bool,
    ) -> anyhow::Result<MemoryEntry> {
        if let Some(status) = status {
            // 兼容旧档别名:stale → deprecated(R-165 兼容映射,写入侧统一归一化)。
            let status = super::normalize_status(status);
            if !STATUSES.contains(&status) {
                anyhow::bail!("invalid status `{status}`; valid: {}", STATUSES.join(" | "));
            }
        }
        let tree_lock = self.tree_lock()?;
        let entries = self.load_all();
        let Some((path, mut entry)) = entries.into_iter().find(|(_, e)| e.id == id) else {
            if area.is_some() {
                anyhow::bail!("unknown memory id `{id}`(归档条目只读,不能设区域)");
            }
            anyhow::bail!("unknown memory id `{id}`");
        };
        let previous_status = entry.status.clone();
        let previous_content = super::memory_content_revision(&entry);
        // D-282 ②:CAS——写前校验调用方看到的版本没被别人改过。
        if let Some(expected) = expected_hash {
            let current = kanzei_base::content_hash(render_entry(&entry).as_bytes());
            if current != expected {
                anyhow::bail!(
                    "memory {id} 已被并发修改(expected_hash 不匹配):你拿到的是旧版本,\
                     重读当前条目后合并再写。当前 hash: {current}"
                );
            }
        }
        if let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) {
            entry.title = title.into();
        }
        if let Some(desc) = description.map(str::trim).filter(|d| !d.is_empty()) {
            // D-282 ①:description 主题一致性——新钩子必须与条目现有主题有共同词。
            // context 含旧 description(演化延续性),阈值 <2 拒绝(单字交集常被
            // 「确/配/理」这类通用字撞车,2 个以上才算有实质主题关联)。
            // enforce_topic=false(A-005 UI 用户直写 / merge)豁免:用户有权写任何内容。
            if enforce_topic {
                let context = format!("{} {} {}", entry.title, entry.description, entry.body);
                let overlap = topic_overlap(desc, &context);
                if overlap < 2 {
                    anyhow::bail!(
                        "拒绝写入:新 description 与条目 {id} 的现有主题共同词过少({overlap}),\
                         疑似选错条目或主题漂移(D-282)。旧 description: {:?}\n新 description: {:?}\n\
                         若确为同主题演化,请在新 description 里保留至少两个旧主题关键词\
                         (title/正文/旧钩子里的词)。",
                        entry.description,
                        desc,
                    );
                }
            }
            entry.description = desc.into();
        }
        if let Some(body) = body {
            entry.body = body.trim().to_string();
        }
        if let Some(status) = status {
            entry.status = super::normalize_status(status).into();
        }
        if let Some(areas) = area {
            archive::set_area_field(&mut entry, areas);
        }
        if entry.requires_review()
            && entry.status == "active"
            && super::memory_content_revision(&entry) != previous_content
        {
            entry.status = "candidate".into();
            entry.extras.retain(|(key, _)| {
                !key.starts_with("retention_") && key != "reviewed_content_revision"
            });
        }
        entry.updated = today();
        // 文件名沿用旧路径(slug 终身不改)。
        self.write_entry(&entry, Some(&path))?;
        self.refresh_derived()?;
        drop(tree_lock);
        if previous_status != "deprecated" && entry.status == "deprecated" {
            let source_refs = entry.refs();
            super::record_memory_lifecycle_event(
                self.project_root.as_deref(),
                "memory_deprecated",
                Some(&entry.id),
                &[],
                Some(&entry.source),
                &source_refs,
                "deprecated_status_update",
                Some(&previous_status),
                Some("deprecated"),
            );
        }
        Ok(entry)
    }

    /// R-316:主 agent 可用的同步文本修正。与 manager 的 update 刻意分离：
    /// 只接受既有条目、单个文本字段、调用方看到的 old_value + expected_hash，
    /// 并把依据与 old/new 原值写入 corrections.jsonl；不触碰 status/extras/编号。
    pub fn correct_text(
        &self,
        id: &str,
        field: &str,
        old_value: &str,
        new_value: &str,
        expected_hash: &str,
        basis: &str,
    ) -> anyhow::Result<MemoryEntry> {
        if !matches!(field, "title" | "description" | "body") {
            anyhow::bail!("memory text correction field must be title | description | body");
        }
        if old_value.is_empty() {
            anyhow::bail!("old_value must not be empty");
        }
        if new_value.trim().is_empty() {
            anyhow::bail!("new_value must not be empty");
        }
        if expected_hash.trim().is_empty() {
            anyhow::bail!("expected_hash is required for synchronous memory correction");
        }
        if basis.trim().is_empty() {
            anyhow::bail!("basis is required for synchronous memory correction");
        }

        let _tree_lock = self.tree_lock()?;
        let Some((path, mut entry)) = self.load_all().into_iter().find(|(_, e)| e.id == id) else {
            anyhow::bail!("unknown memory id `{id}`");
        };
        let current_hash = kanzei_base::content_hash(super::render_entry(&entry).as_bytes());
        if current_hash != expected_hash {
            anyhow::bail!(
                "memory {id} 已被并发修改(expected_hash 不匹配):请重读当前条目后重试。当前 hash: {current_hash}"
            );
        }
        let current_value = match field {
            "title" => &entry.title,
            "description" => &entry.description,
            "body" => &entry.body,
            _ => unreachable!(),
        };
        if current_value != old_value {
            anyhow::bail!(
                "memory {id} 的 {field} old_value 不匹配:当前值为 {:?}，拒绝覆盖错误版本",
                current_value
            );
        }
        let previous = entry.clone();
        let effective_new = new_value.trim().to_string();
        match field {
            "title" => entry.title = effective_new.clone(),
            "description" => entry.description = effective_new.clone(),
            "body" => entry.body = effective_new.clone(),
            _ => unreachable!(),
        }
        entry.updated = today();
        match self
            .write_entry(&entry, Some(&path))
            .and_then(|_| self.refresh_derived())
        {
            Ok(()) => {}
            Err(error) => {
                let rollback = self
                    .write_entry(&previous, Some(&path))
                    .and_then(|_| self.refresh_derived());
                anyhow::bail!(
                    "memory {id} correction failed: {error}; rollback={:?}",
                    rollback.err().map(|e| e.to_string())
                );
            }
        }

        let audit = serde_json::json!({
            "schema_version": 1,
            "event": "memory_text_corrected",
            "actor": "main-agent",
            "process_id": std::process::id(),
            "at_ms": now_ms(),
            "scope": self.scope.label(),
            "id": id,
            "field": field,
            "basis": basis.trim(),
            "expected_hash": expected_hash,
            "old_value": old_value,
            "new_value": effective_new,
        })
        .to_string();
        if let Err(error) = self.append_correction_audit(&(audit + "\n")) {
            let rollback = self
                .write_entry(&previous, Some(&path))
                .and_then(|_| self.refresh_derived());
            anyhow::bail!(
                "memory {id} correction audit failed: {error}; rollback={:?}",
                rollback.err().map(|e| e.to_string())
            );
        }
        Ok(entry)
    }

    fn append_correction_audit(&self, record: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join("corrections.jsonl");
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        text.push_str(record);
        crate::atomic_file::write_atomic(&path, &text)?;
        Ok(())
    }

    /// 升级 candidate|shadow → active(R-165 生命周期 PROMOTE)。
    /// provenance 硬约束:必须提供至少一条 memory_sources 证据(episode 区间),
    /// 无来源不入 active——证据编译语义的引擎强制,不靠 manager 自觉。
    /// `sources` 形如 (episode_id, event_start, event_end) 元组,非空才放行。
    /// R-166:允许 shadow → active(评估通过后进入生产);candidate 也可直接跳
    /// shadow 阶段进入 active(评估器未落地前的既有路径)。
    pub fn promote(
        &self,
        id: &str,
        sources: &[(i64, Option<i64>, Option<i64>)],
        source_hash: Option<&str>,
    ) -> anyhow::Result<MemoryEntry> {
        // 保持原错误顺序:sources 空先报(历史行为,测试锚定)。
        if sources.is_empty() {
            anyhow::bail!(
                "cannot promote `{id}`: no memory_sources evidence — R-165 provenance \
                 hard constraint, a candidate needs at least one episode source"
            );
        }
        let entries = self.load_all();
        let Some((path, mut entry)) = entries.into_iter().find(|(_, e)| e.id == id) else {
            anyhow::bail!("unknown memory id `{id}`");
        };
        if let Some(fingerprint) = entry.fingerprint() {
            let recurrence = self.recurrence_count(&fingerprint);
            if recurrence < super::lifecycle::ACTIVE_RECURRENCE_MIN {
                anyhow::bail!(
                    "cannot promote `{id}`: fingerprint `{fingerprint}` recurrence {recurrence} is below active threshold {}",
                    super::lifecycle::ACTIVE_RECURRENCE_MIN
                );
            }
        }
        let previous_status = entry.status.clone();
        // R-255 第二刀:provenance 门禁(状态机/episode 真实/证据先落库)提纯到
        // MemoryLifecycle::promote_guard;store 只做查条目 + 置 active + 落盘。
        MemoryLifecycle.promote_guard(id, &entry, sources, source_hash, self.scope, &self.root)?;
        if entry.requires_review() {
            let reviewed = super::memory_content_revision(&entry);
            entry
                .extras
                .retain(|(key, _)| key != "reviewed_content_revision");
            entry
                .extras
                .push(("reviewed_content_revision".into(), reviewed));
        }
        entry.status = "active".into();
        entry.updated = today();
        self.write_entry(&entry, Some(&path))?;
        self.refresh_derived()?;
        let episode_ids: Vec<i64> = sources
            .iter()
            .map(|(episode_id, _, _)| *episode_id)
            .collect();
        let source_refs = entry.refs();
        super::record_memory_lifecycle_event(
            self.project_root.as_deref(),
            "memory_candidate_promoted",
            Some(&entry.id),
            &episode_ids,
            source_hash.or(Some(&entry.source)),
            &source_refs,
            "provenance_verified",
            Some(&previous_status),
            Some("active"),
        );
        Ok(entry)
    }

    /// 按候选价值排序：无 fingerprint/recurrence 的条目最先清退，
    /// 同价值时优先清退更早未更新者。仅用于容量收敛，不参与 provenance 晋升。
    fn candidate_retention_key(&self, entry: &MemoryEntry) -> (u8, u8, u32, String, String) {
        let fingerprint = entry.fingerprint();
        let recurrence = fingerprint
            .as_deref()
            .map(|value| self.recurrence_count(value))
            .unwrap_or(0);
        (
            u8::from(fingerprint.is_some()),
            u8::from(recurrence > 0),
            recurrence,
            entry.updated.clone(),
            entry.id.clone(),
        )
    }

    /// 自动处置 candidate(R-195):
    /// - 有真实当轮 episode、复发计数≥3 且带 fingerprint → promote(active);
    /// - 没有晋升条件且超过 max_age_days 个日历日未处置 → deprecated 并归档;
    /// - 其余保持 candidate,不改变「未验证不注入」边界。
    ///
    /// 这是确定性的轮末闸门,不依赖 manager 是否正确调用工具。晋升仍复用
    /// promote 的 provenance 硬约束;清退复用 update/archive_dead,保留可追溯墓碑。
    pub fn reconcile_candidates(
        &self,
        current_episode_id: Option<i64>,
        max_age_days: i64,
    ) -> anyhow::Result<CandidateReconcileReport> {
        let deduplication = self.deduplicate_redundant_entries()?;
        let before = self.load_all();
        let mut report = CandidateReconcileReport {
            merged: deduplication.duplicates,
            candidate_files_before: before
                .iter()
                .filter(|(_, entry)| entry.status == "candidate")
                .count(),
            candidate_index_before: self.candidate_index_count(),
            ..Default::default()
        };
        let today_days = date_days(&today());
        for (path, entry) in before {
            if entry.status != "candidate" {
                continue;
            }
            let recurrence = entry
                .fingerprint()
                .as_deref()
                .map(|fingerprint| self.recurrence_count(fingerprint))
                .unwrap_or(0);
            let age = today_days
                .zip(date_days(&entry.updated))
                .map(|(now, updated)| now.saturating_sub(updated));
            // R-255 第二刀:晋升/清退判定提纯到 MemoryLifecycle(should_promote/
            // should_deprecate),store 只执行;promote 失败落回清退判定(与原一致)。
            if entry
                .extras
                .iter()
                .any(|(key, value)| key == "review_required" && value == "true")
            {
                report.untouched.push(entry.id.clone());
                continue; // A conversation inference waits for explicit review, never age or repetition.
            }
            if MemoryLifecycle.should_promote(&entry, recurrence, current_episode_id) {
                if let Some(episode_id) = current_episode_id {
                    if self
                        .promote(
                            &entry.id,
                            &[(episode_id, None, None)],
                            Some("candidate-reconcile"),
                        )
                        .is_ok()
                    {
                        report.promoted.push(entry.id);
                        continue;
                    }
                }
            }
            if let Some(reason) =
                MemoryLifecycle.should_deprecate(age, max_age_days, &path.display().to_string())
            {
                let body = format!("{}\n\n{reason}", entry.body.trim_end());
                if self
                    .update(
                        &entry.id,
                        None,
                        None,
                        Some(&body),
                        Some("deprecated"),
                        None,
                        false,
                    )
                    .is_ok()
                {
                    report.deprecated.push(entry.id);
                    continue;
                }
            }
            report.untouched.push(entry.id);
        }
        // R-295：健康水位是容量出口，避免新 candidate 持续挤占轮末整理窗口。
        // 先完成晋升/超龄清退，再按 fingerprint、recurrence、更新时间保留高价值项；
        // 每次清退仍走 update → refresh_derived → archive_dead，保留可追溯墓碑。
        let mut over_capacity = self
            .load_all()
            .into_iter()
            .filter(|(_, entry)| {
                entry.status == "candidate"
                    && !entry
                        .extras
                        .iter()
                        .any(|(key, value)| key == "review_required" && value == "true")
            })
            .collect::<Vec<_>>();
        if over_capacity.len() > crate::memory::CANDIDATE_MAX_COUNT {
            over_capacity.sort_by_key(|(_, entry)| self.candidate_retention_key(entry));
            let retire_count = over_capacity.len() - crate::memory::CANDIDATE_MAX_COUNT;
            for (path, entry) in over_capacity.into_iter().take(retire_count) {
                let fingerprint = entry.fingerprint().is_some();
                let recurrence = entry
                    .fingerprint()
                    .as_deref()
                    .map(|value| self.recurrence_count(value))
                    .unwrap_or(0);
                let body = format!(
                    "{}\n\n(auto-deprecated: candidate 超出健康水位 {}，按低价值优先清退；fingerprint={}，recurrence={}；原路径 {})",
                    entry.body.trim_end(),
                    crate::memory::CANDIDATE_MAX_COUNT,
                    fingerprint,
                    recurrence,
                    path.display()
                );
                if self
                    .update(
                        &entry.id,
                        None,
                        None,
                        Some(&body),
                        Some("deprecated"),
                        None,
                        false,
                    )
                    .is_ok()
                {
                    let id = entry.id.clone();
                    report.deprecated.push(id.clone());
                    // 该条第一阶段曾计入 untouched,实际已被容量出口清退,
                    // 从 untouched 移除,untouched 只保留最终仍为 candidate 的条目。
                    report.untouched.retain(|x| x != &id);
                }
            }
        }
        let after = self.load_all();
        report.candidate_files_after = after
            .iter()
            .filter(|(_, entry)| entry.status == "candidate")
            .count();
        report.candidate_index_after = self.candidate_index_count();
        Ok(report)
    }

    pub(crate) fn write_entry(
        &self,
        entry: &MemoryEntry,
        existing_path: Option<&Path>,
    ) -> anyhow::Result<()> {
        if entry.status == "active" && entry.requires_review() {
            let current = super::memory_content_revision(entry);
            anyhow::ensure!(
                entry
                    .extras
                    .iter()
                    .any(|(key, value)| key == "reviewed_content_revision" && value == &current),
                "对话候选必须通过当前内容版本的明确复核才能启用"
            );
        }
        let rendered = render_entry(entry);
        for text in [
            &entry.id,
            &entry.scope,
            &entry.category,
            &entry.title,
            &entry.description,
            &entry.body,
            &entry.source,
            &entry.created,
            &entry.updated,
        ] {
            super::privacy::reject_credentials(text)?;
        }
        for (key, value) in &entry.extras {
            let fields = serde_json::Map::from_iter([(key.clone(), serde_json::json!(value))]);
            super::privacy::reject_credentials(&serde_json::Value::Object(fields).to_string())?;
        }
        // D-368:所有条目落盘统一持记忆树锁,并与围栏收口共享锁互斥;超预算明确报错。
        let _tree_lock = self.tree_lock()?;
        std::fs::create_dir_all(&self.root)?;
        let path = match existing_path {
            Some(p) => p.to_path_buf(),
            None => self.root.join(format!("{}.md", entry.file_stem())),
        };
        crate::atomic_file::write_atomic(&path, &rendered)?;
        self.record_write_log(&path, Some(rendered.into_bytes()))?;
        Ok(())
    }

    /// 在记忆树写锁内调用；None 表示合法删除，Some(empty) 表示空文件。
    pub(super) fn record_write_log(
        &self,
        path: &Path,
        content: Option<Vec<u8>>,
    ) -> anyhow::Result<()> {
        let Some(project_root) = &self.project_root else {
            return Ok(());
        };
        let relative = path.strip_prefix(project_root)?;
        kanzei_base::write_log::record(
            project_root,
            &kanzei_base::write_log::WriteLogEntry {
                at_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis(),
                path: relative.display().to_string().replace('\\', "/"),
                fingerprint: kanzei_base::content_hash(content.as_deref().unwrap_or(&[])),
                content: content.map_or(
                    kanzei_base::write_log::LoggedContent::Deleted,
                    kanzei_base::write_log::LoggedContent::Stored,
                ),
                run_id: None,
                process_id: None,
            },
        )?;
        Ok(())
    }
    /// 检查 INDEX 的每条派生行仍与 Markdown 真源的 description 一致。
    /// 该断言必须位于写入前，避免生成器未来改动时静默重新引入串号。
    fn assert_index_matches_entries(
        index: &str,
        entries: &[(PathBuf, MemoryEntry)],
    ) -> anyhow::Result<()> {
        let active: Vec<&MemoryEntry> = entries
            .iter()
            .map(|(_, entry)| entry)
            .filter(|entry| entry.status == "active")
            .collect();
        let mut seen = Vec::new();
        for line in index.lines().filter(|line| line.starts_with("- ")) {
            let payload = line.trim_start_matches("- ");
            let Some((id_part, _rest)) = payload.split_once(" [") else {
                anyhow::bail!("INDEX 行缺少 id/category: {line}");
            };
            let Some(entry) = active.iter().find(|entry| entry.id == id_part) else {
                anyhow::bail!("INDEX 行引用不存在或非 active 条目: {id_part}");
            };
            let expected = format!(
                "- {} [{}] {} — {}",
                entry.id, entry.category, entry.title, entry.description
            );
            if line != expected {
                anyhow::bail!(
                    "INDEX 行与 {} 源文件不一致: index={line:?}, expected={expected:?}",
                    entry.id
                );
            }
            seen.push(id_part);
        }
        if seen.len() != active.len()
            || active
                .iter()
                .any(|entry| !seen.contains(&entry.id.as_str()))
        {
            anyhow::bail!("INDEX active 条目集合与 Markdown 真源不一致");
        }
        Ok(())
    }

    /// 重建全部派生物:INDEX.md 与 FTS 索引。任何写操作后调用;损坏时可手动全量重建。
    /// R-165 批3:先归档失效条目,再以归档后的集合重建(主目录只含 active/candidate)。
    pub fn refresh_derived(&self) -> anyhow::Result<()> {
        self.refresh_derived_inner(true)
    }

    /// 显式修复 INDEX/FTS 派生物：只接受调用者明确承担的修复动作，按 Markdown
    /// 真源重建，不放宽默认 refresh 的一致性硬门禁。
    pub fn repair_derived(&self) -> anyhow::Result<()> {
        self.refresh_derived_inner(false)
    }

    fn refresh_derived_inner(&self, verify_existing_index: bool) -> anyhow::Result<()> {
        // D-368:派生物重建(归档搬移 + INDEX.md + FTS index.db)整体持记忆树锁,
        // 并与围栏收口共享锁互斥,让 after 快照只看到完整终态。
        let _tree_lock = self.tree_lock()?;
        let _archived = self.archive_dead()?;
        let entries = self.index_source_entries()?;
        // INDEX.md:一行一条(仅 active),candidate 折叠为计数(未验证不占索引面)。
        let mut index = format!("# Memory Index ({})\n\n", self.scope.label());
        let mut candidates = 0usize;
        for (_, e) in &entries {
            if e.status == "active" {
                index.push_str(&format!(
                    "- {} [{}] {} — {}\n",
                    e.id, e.category, e.title, e.description
                ));
            } else {
                candidates += 1;
            }
        }
        if candidates > 0 {
            index.push_str(&format!("\n({candidates} candidate 条待验证晋升)\n"));
        }
        if verify_existing_index {
            Self::assert_index_matches_entries(&index, &entries)?;
        }
        crate::atomic_file::write_atomic(&self.index_md(), &index)?;
        self.record_write_log(&self.index_md(), Some(index.into_bytes()))?;

        self.sync_fts(&entries)?;
        // R-268:FTS index.db 是围栏可见的托管文件(SQLite 二进制),写后记日志——
        // 指纹 + 内容快照(库通常小;围栏收口按日志吸收,不把它当越界回滚)。
        self.record_write_log(&self.db_path(), Some(std::fs::read(self.db_path())?))?;
        Ok(())
    }

    /// Source revisions, not ID presence, determine whether a derived row is
    /// current. Both normal writes and external Markdown edits use this path.
    pub(crate) fn ensure_derived_consistent(&self) -> anyhow::Result<Vec<(PathBuf, MemoryEntry)>> {
        let _tree_lock = self.tree_lock()?;
        let entries = self.index_source_entries()?;
        if self.sync_fts(&entries)? {
            self.refresh_derived()?;
            // Refresh may archive externally deprecated/invalid entries. The
            // returned generation must describe the resulting live source.
            return self.index_source_entries();
        }
        Ok(entries)
    }

    pub(super) fn index_source_entries(&self) -> anyhow::Result<Vec<(PathBuf, MemoryEntry)>> {
        source::scan(&self.root).into_checked()
    }

    /// Strict source read for context assembly; an inaccessible memory is not an empty memory.
    pub fn load_all_checked(&self) -> anyhow::Result<Vec<(PathBuf, MemoryEntry)>> {
        let mut entries = self.index_source_entries()?;
        entries.sort_by(|a, b| a.1.id.cmp(&b.1.id));
        Ok(entries)
    }

    /// 迁移恢复必须包含归档,并传播读取错误,不能把读失败当成“目标还没有”。
    pub(super) fn migration_entries(&self) -> anyhow::Result<Vec<(PathBuf, MemoryEntry)>> {
        self.all_source_entries().into_checked()
    }

    pub(super) fn all_source_entries(&self) -> source::SourceScan {
        let mut scan = source::scan(&self.root);
        scan.extend(source::scan(&self.archive_dir()));
        scan
    }

    fn sync_fts(&self, entries: &[(PathBuf, MemoryEntry)]) -> anyhow::Result<bool> {
        use std::collections::BTreeMap;
        let mut conn = self.open_db()?;
        let tx = conn.transaction()?;
        let known: BTreeMap<String, String> = {
            let mut statement = tx.prepare("SELECT id, revision FROM memory_fts_revisions")?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let current: BTreeMap<&str, &MemoryEntry> = entries
            .iter()
            .map(|(_, entry)| (entry.id.as_str(), entry))
            .collect();
        // Legacy indices have no revisions. Delete their unversioned rows in
        // the same transaction that installs the replacement generation.
        let mut changed = tx.execute(
            "DELETE FROM memory_fts WHERE id NOT IN (SELECT id FROM memory_fts_revisions)",
            [],
        )? > 0;
        for id in known.keys().filter(|id| !current.contains_key(id.as_str())) {
            changed = true;
            tx.execute("DELETE FROM memory_fts WHERE id = ?1", params![id])?;
            tx.execute(
                "DELETE FROM memory_fts_revisions WHERE id = ?1",
                params![id],
            )?;
        }
        for (id, entry) in current {
            let revision = super::entry_revision(entry);
            if known.get(id) == Some(&revision) {
                // An interrupted old rebuild or external DB repair may have
                // removed the row while leaving revision metadata behind.
                let present: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM memory_fts WHERE id = ?1)",
                    params![id],
                    |row| row.get(0),
                )?;
                if present {
                    continue;
                }
            }
            changed = true;
            tx.execute("DELETE FROM memory_fts WHERE id = ?1", params![id])?;
            tx.execute("INSERT INTO memory_fts(id, title, description, body, category, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, segment_cjk(&entry.title), segment_cjk(&entry.description), segment_cjk(&entry.body), entry.category, entry.status])?;
            tx.execute("INSERT INTO memory_fts_revisions(id, revision) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision",
                params![id, revision])?;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub(crate) fn open_db(&self) -> anyhow::Result<Connection> {
        std::fs::create_dir_all(&self.root)?;
        let conn = Connection::open(self.db_path())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                 id UNINDEXED, title, description, body, category UNINDEXED, status UNINDEXED,
                 tokenize='unicode61');
             CREATE TABLE IF NOT EXISTS memory_hits(
                 id TEXT PRIMARY KEY,
                 hits INTEGER NOT NULL DEFAULT 0,
                 last_hit_at INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS memory_fts_revisions(
                 id TEXT PRIMARY KEY,
                 revision TEXT NOT NULL);
             -- R-125 召回明细。和 hits 一样是派生日志:丢了只丢历史,不丢事实,
             -- 所以放在可重建的 index.db 而不是记忆文件里。
             CREATE TABLE IF NOT EXISTS memory_recalls(
                 recall_id TEXT NOT NULL,
                 at INTEGER NOT NULL,
                 prompt_head TEXT NOT NULL,
                 injected_bytes INTEGER NOT NULL DEFAULT 0,
                 entry_id TEXT NOT NULL,
                 title TEXT NOT NULL DEFAULT '',
                 scope TEXT NOT NULL DEFAULT '',
                 category TEXT NOT NULL DEFAULT '',
                 score REAL NOT NULL DEFAULT 0,
                 snippet TEXT NOT NULL DEFAULT '',
                 -- 「是否产生作用」的判定证据:prompt_hints 只注入索引行,
                 -- 模型要用内容就必须再拉一次正文(memory_search / read file)。
                 -- 拉过 = 采纳,没拉 = 召回了但没用上。这是机械可判的,不靠猜。
                 fetched INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY(recall_id, entry_id));
             CREATE INDEX IF NOT EXISTS memory_recalls_at ON memory_recalls(at DESC);
             -- R-165 批2:novelty gate 三档分流计数遥测(验收④)。
             CREATE TABLE IF NOT EXISTS novelty_events(
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 at INTEGER NOT NULL,
                 verdict TEXT NOT NULL,           -- new | duplicate | uncertain
                 fingerprint TEXT NOT NULL DEFAULT '',
                 note_head TEXT NOT NULL DEFAULT '');
             -- D-578:manager 最终决策遥测,与 novelty_events 分开避免把投递判定混为一谈。
             CREATE TABLE IF NOT EXISTS manager_decisions(
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 at INTEGER NOT NULL,
                 decision TEXT NOT NULL,          -- noop | produced | rejected
                 reason TEXT NOT NULL DEFAULT '',
                 note_head TEXT NOT NULL DEFAULT '');
             CREATE INDEX IF NOT EXISTS manager_decisions_at ON manager_decisions(at DESC);
             -- R-165 批2:recurrence 三段晋升的跨轮持久计数(验收②)。
             CREATE TABLE IF NOT EXISTS recurrence_counts(
                 fingerprint TEXT PRIMARY KEY,
                 count INTEGER NOT NULL DEFAULT 0,
                 last_at INTEGER NOT NULL DEFAULT 0);",
        )?;
        Ok(conn)
    }
}

#[cfg(test)]
mod tests;
