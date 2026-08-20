//! MemoryStore:单 scope 的记忆仓库。
//! 硬门禁在写入侧:ID 引擎分配、枚举校验、description 必填、精确重复拒绝;
//! INDEX.md 与 index.db(FTS5/hits)都是派生物,损坏可由文件全量重建;
//! 写入 tmp+rename 原子替换,不做跨进程锁(用户定调:竞争冲突留给 agent 事后解决)。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use super::admission::{topic_overlap, MemoryAdmission};
use super::lifecycle::MemoryLifecycle;
pub(crate) use super::retrieval::{intent_query, segment_cjk};
use super::{date_days, parse_entry, render_entry, today, MemoryEntry, MemoryScope, STATUSES};

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
    /// 状态不变量(R-149):同 scope+category+subject 至多一条 active。
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
    pub candidate_files_before: usize,
    pub candidate_files_after: usize,
    pub candidate_index_before: usize,
    pub candidate_index_after: usize,
    pub promoted: Vec<String>,
    pub deprecated: Vec<String>,
    pub untouched: Vec<String>,
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

/// 决策权重(R-149):召回≥3 的条目按采纳率温和降权/提权(×0.6~×1.3)。
/// 下限 0.6 不清零:prompt_hints 只注入索引行,「看行即用不拉正文」会被记为未采纳,
/// 样本天然有偏——只降权不淘汰,淘汰决定留给人与整理流程。参数为初始值,待实证复核。
/// D-366:排序决策属于检索门面,定义在 index.rs(此处不再使用)。
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
        store.migrate_legacy(project_root);
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

    /// D-368:记忆树互斥锁(跨进程 + 跨线程)。锁目标 = 记忆根目录本身,锁文件 =
    /// 同目录 `<root>.lock`(project scope = `.kanzei/memory.lock`)。
    ///
    /// bash 围栏收口(kanzei-tools managed.rs `acquire_managed_locks`)短暂取同一目标的
    /// 共享锁——确保 after 快照不会读到 memory 写入(条目/INDEX.md/inbox.md/
    /// voided-ids.md/归档搬移/index.db 重建)的中间态;超过预算(默认 3s)明确报错。
    ///
    /// 为什么锁目录而不是逐文件:动态条目文件(M-xxx.md、inbox.md)创建前无法预锁,
    /// 锁整树一劳永逸;写操作毫秒级,持有窗口极短。锁文件落在 `.kanzei/`(非托管根,
    /// 不进围栏镜像)且被 `.kanzei/**/*.lock` 忽略,围栏快照对它无感。
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

    /// 扫描根目录(不含 archive/)加载全部条目。宽容:解析不了的文件跳过并计入 integrity。
    pub fn load_all(&self) -> Vec<(PathBuf, MemoryEntry)> {
        let mut out = Vec::new();
        let Ok(dir) = std::fs::read_dir(&self.root) else {
            return out;
        };
        for item in dir.flatten() {
            let path = item.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("INDEX.md")
                || path.file_name().and_then(|n| n.to_str()) == Some("inbox.md")
            {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                let entry = parse_entry(&text);
                if !entry.id.is_empty() {
                    out.push((path, entry));
                }
            }
        }
        out.sort_by(|a, b| a.1.id.cmp(&b.1.id));
        out
    }

    pub fn has_archived_id(&self, id: &str) -> bool {
        self.load_archived_ids()
            .iter()
            .any(|archived| archived == id)
    }

    pub(crate) fn load_archived_ids(&self) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(dir) = std::fs::read_dir(self.archive_dir()) else {
            return out;
        };
        for item in dir.flatten() {
            if let Some(name) = item.path().file_stem().and_then(|n| n.to_str()) {
                if let Some(id) = name.split('-').take(2).collect::<Vec<_>>().get(0..2) {
                    out.push(format!("{}-{}", id[0], id[1]));
                }
            }
        }
        out
    }

    /// 归档条数(D-217):stale/失效条目经 archive_dead 搬入 archive/ 后在此计数,
    /// 供整理清单展示「已归档待复查」积压。只读,不触发扫描副作用。
    pub fn archived_count(&self) -> usize {
        let Ok(dir) = std::fs::read_dir(self.archive_dir()) else {
            return 0;
        };
        dir.flatten()
            .filter(|p| p.path().extension().and_then(|e| e.to_str()) == Some("md"))
            .count()
    }

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
        // D-368:整个 add(含 classify_novelty 的 FTS 探测——首次会建 index.db)持记忆
        // 树锁,与 bash 围栏窗口互斥。内层 write_entry/refresh_derived 的 tree_lock
        // 同线程重入,由 FileLock 重入计数放行。
        let _tree_lock = self.tree_lock()?;
        // D-495:写入前先修复已有的派生索引失步,避免 classify_novelty 使用过期 FTS。
        self.ensure_derived_consistent()?;
        // R-255 第二刀:准入链(枚举/必填/subject 不变式/交付拒收/指纹/判重)提纯到
        // MemoryAdmission,store 只做「查全部条目 + 依结果分流」。
        MemoryAdmission::validate_basic(category, title, description, body)?;
        let title = title.trim();
        let description = description.trim();
        let subject = subject.map(str::trim).filter(|s| !s.is_empty());
        let entries = self.load_all();
        if let Some(existing) = MemoryAdmission::find_subject_conflict(&entries, category, subject)
        {
            return Ok(AddOutcome::SubjectConflict(existing.clone()));
        }
        MemoryAdmission::check_delivery_state(title, subject)?;
        {
            let text = format!("{description}\n{body}");
            let inbox = self.read_inbox();
            let existing_fps = {
                let mut fps = Vec::new();
                for (_, e) in self.load_all() {
                    fps.extend(super::fp_markers(&format!("{}\n{}", e.description, e.body)));
                }
                if let Some(global) = MemoryStore::global() {
                    for (_, e) in global.load_all() {
                        fps.extend(super::fp_markers(&format!("{}\n{}", e.description, e.body)));
                    }
                }
                fps
            };
            MemoryAdmission::check_fingerprint(&text, &inbox, existing_fps.into_iter())?;
        }
        // 语义探测下沉:Uncertain(有 FTS 命中但非精确)即拒并返回候选(force 跳过)。
        if !force {
            let (novelty, candidates) = self.classify_novelty(title, description, body);
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
            let mut extras: Vec<(String, String)> = Vec::new();
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
        super::record_memory_lifecycle_event(
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
        if let Some(status) = status {
            // 兼容旧档别名:stale → deprecated(R-165 兼容映射,写入侧统一归一化)。
            let status = super::normalize_status(status);
            if !STATUSES.contains(&status) {
                anyhow::bail!("invalid status `{status}`; valid: {}", STATUSES.join(" | "));
            }
        }
        let entries = self.load_all();
        let Some((path, mut entry)) = entries.into_iter().find(|(_, e)| e.id == id) else {
            anyhow::bail!("unknown memory id `{id}`");
        };
        let previous_status = entry.status.clone();
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
        entry.updated = today();
        // 文件名沿用旧路径(slug 终身不改)。
        self.write_entry(&entry, Some(&path))?;
        self.refresh_derived()?;
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
        let previous_status = entry.status.clone();
        // R-255 第二刀:provenance 门禁(状态机/episode 真实/证据先落库)提纯到
        // MemoryLifecycle::promote_guard;store 只做查条目 + 置 active + 落盘。
        MemoryLifecycle.promote_guard(id, &entry, sources, source_hash, self.scope, &self.root)?;
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
        let before = self.load_all();
        let mut report = CandidateReconcileReport {
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
            .filter(|(_, entry)| entry.status == "candidate")
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
        // D-368:所有条目落盘统一持记忆树锁,并与围栏收口共享锁互斥;超预算明确报错。
        let _tree_lock = self.tree_lock()?;
        std::fs::create_dir_all(&self.root)?;
        let path = match existing_path {
            Some(p) => p.to_path_buf(),
            None => self.root.join(format!("{}.md", entry.file_stem())),
        };
        crate::atomic_file::write_atomic(&path, &render_entry(entry))?;
        // R-268:写后记写日志——围栏收口对账的归因凭据(memory 写入口与 tracker
        // 同口径:先写文档再记日志)。global scope 无项目主根,不记。
        if let Some(project_root) = &self.project_root {
            if let Ok(relative) = path.strip_prefix(project_root) {
                let rendered = render_entry(entry);
                // D-399:record 失败至少告警(契约「宁可失败不静默」)。
                if let Err(e) = kanzei_base::write_log::record(
                    project_root,
                    &kanzei_base::write_log::WriteLogEntry {
                        at_ms: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or_default(),
                        path: relative.display().to_string().replace('\\', "/"),
                        fingerprint: kanzei_base::content_hash(rendered.as_bytes()),
                        content: rendered.into_bytes(),
                        run_id: None,
                        process_id: None,
                    },
                ) {
                    eprintln!("[write-log] record failed for {}: {e}", relative.display());
                }
            }
        }
        Ok(())
    }

    fn record_write_log(&self, path: &Path, content: Vec<u8>) {
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
                fingerprint: kanzei_base::content_hash(&content),
                content,
                run_id: None,
                process_id: None,
            },
        );
    }

    /// 归档失效条目(D-231/R-165 验收③):deprecated/invalid 移入 archive/ 带墓碑。
    /// 返回归档条数。引擎强制:任何 refresh_derived(写操作后)都会先归档,
    /// 主目录只留 active/candidate——归档条目不在 load_all/FTS/检索范围内,
    /// ID 由 load_archived_ids 保留永不复用。
    pub fn archive_dead(&self) -> usize {
        let entries = self.load_all();
        let mut archived = 0usize;
        for (path, entry) in &entries {
            if entry.status != "deprecated" && entry.status != "invalid" {
                continue;
            }
            let archive_dir = self.archive_dir();
            std::fs::create_dir_all(&archive_dir).ok();
            let dest = archive_dir.join(format!("{}.md", entry.file_stem()));
            // 墓碑:保留文件(内容即追溯),目标已存在则跳过(防重复归档覆盖)。
            if dest.exists() {
                if std::fs::remove_file(path).is_ok() {
                    self.record_write_log(path, Vec::new());
                }
            } else if std::fs::rename(path, &dest).is_ok() {
                archived += 1;
                // D-480:rename 同时改变源路径和 archive 目标路径。两条日志都要记，
                // 围栏才能把「源删除 + 墓碑落盘」识别为同一次合法 memory_stale。
                self.record_write_log(path, Vec::new());
                self.record_write_log(&dest, render_entry(entry).into_bytes());
            }
        }
        archived
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
            let Some((id_part, description)) = payload.split_once(" — ") else {
                anyhow::bail!("INDEX 行缺少 description 分隔符: {line}");
            };
            let Some((id, _rest)) = id_part.split_once(" [") else {
                anyhow::bail!("INDEX 行缺少 id/category: {line}");
            };
            let Some(entry) = active.iter().find(|entry| entry.id == id) else {
                anyhow::bail!("INDEX 行引用不存在或非 active 条目: {id}");
            };
            if entry.description != description {
                anyhow::bail!(
                    "INDEX description 与 {id} 源文件不一致: index={description:?}, source={:?}",
                    entry.description
                );
            }
            seen.push(id);
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
        // D-368:派生物重建(归档搬移 + INDEX.md + FTS index.db)整体持记忆树锁,
        // 并与围栏收口共享锁互斥,让 after 快照只看到完整终态。
        let _tree_lock = self.tree_lock()?;
        let _archived = self.archive_dead();
        let entries = self.load_all();
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
        Self::assert_index_matches_entries(&index, &entries)?;
        crate::atomic_file::write_atomic(&self.index_md(), &index)?;
        // R-268:INDEX.md 是围栏可见的托管文件,写后记日志(同 write_entry 口径)。
        if let Some(project_root) = &self.project_root {
            if let Ok(relative) = self.index_md().strip_prefix(project_root) {
                // D-399:record 失败至少告警。
                if let Err(e) = kanzei_base::write_log::record(
                    project_root,
                    &kanzei_base::write_log::WriteLogEntry {
                        at_ms: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or_default(),
                        path: relative.display().to_string().replace('\\', "/"),
                        fingerprint: kanzei_base::content_hash(index.as_bytes()),
                        content: index.into_bytes(),
                        run_id: None,
                        process_id: None,
                    },
                ) {
                    eprintln!("[write-log] record failed for {}: {e}", relative.display());
                }
            }
        }

        let conn = self.open_db()?;
        conn.execute("DELETE FROM memory_fts", [])?;
        for (_, e) in &entries {
            // CJK 单字切分入索引,与查询侧 fts_query 对称。
            conn.execute(
                "INSERT INTO memory_fts(id, title, description, body, category, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    e.id,
                    segment_cjk(&e.title),
                    segment_cjk(&e.description),
                    segment_cjk(&e.body),
                    e.category,
                    e.status
                ],
            )?;
        }
        // R-268:FTS index.db 是围栏可见的托管文件(SQLite 二进制),写后记日志——
        // 指纹 + 内容快照(库通常小;围栏收口按日志吸收,不把它当越界回滚)。
        if let Some(project_root) = &self.project_root {
            if let Ok(relative) = self.db_path().strip_prefix(project_root) {
                if let Ok(db_bytes) = std::fs::read(self.db_path()) {
                    // D-399:record 失败至少告警。
                    if let Err(e) = kanzei_base::write_log::record(
                        project_root,
                        &kanzei_base::write_log::WriteLogEntry {
                            at_ms: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis())
                                .unwrap_or_default(),
                            path: relative.display().to_string().replace('\\', "/"),
                            fingerprint: kanzei_base::content_hash(&db_bytes),
                            content: db_bytes,
                            run_id: None,
                            process_id: None,
                        },
                    ) {
                        eprintln!("[write-log] record failed for {}: {e}", relative.display());
                    }
                }
            }
        }
        Ok(())
    }

    /// FTS 派生物与文件真源的 id 集合比对(只看目录文件名,不读内容——检索热路径
    /// 上的守护必须廉价)。任何差集都判失步;查询失败按未失步处理(刚建库的空表
    /// 走正常路径,不在这里制造额外故障面)。
    /// D-495:统一的派生索引守护。失步时立即按 Markdown 真源全量重建,
    /// 供写入和检索共同调用,避免只在检索热路径修复。
    pub(crate) fn ensure_derived_consistent(&self) -> anyhow::Result<()> {
        let conn = self.open_db()?;
        if self.fts_desynced(&conn) {
            drop(conn);
            self.refresh_derived()?;
        }
        Ok(())
    }

    pub(crate) fn fts_desynced(&self, conn: &Connection) -> bool {
        let mut fts_ids: Vec<String> =
            match conn.prepare("SELECT id FROM memory_fts").and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(0))
                    .map(|rows| rows.flatten().collect())
            }) {
                Ok(ids) => ids,
                Err(_) => return false,
            };
        let mut file_ids: Vec<String> = Vec::new();
        let Ok(dir) = std::fs::read_dir(&self.root) else {
            return false;
        };
        for item in dir.flatten() {
            let path = item.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name == "INDEX.md" || name == "inbox.md" {
                continue;
            }
            // 文件名形如 `M-009-slug.md`:id 是前两段(scope 前缀 + 编号)。
            let mut parts = name.split('-');
            if let (Some(prefix), Some(number)) = (parts.next(), parts.next()) {
                if !number.is_empty() {
                    file_ids.push(format!("{}-{number}", prefix.to_ascii_uppercase()));
                }
            }
        }
        fts_ids.sort();
        fts_ids.dedup();
        file_ids.sort();
        file_ids.dedup();
        fts_ids != file_ids
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
mod tests {
    use super::*;
    // D-366:检索排序决策在 index,测试里需要"决策排序后 top-k"语义时经检索门面。
    use crate::memory::index::{IndexQuery, SqliteMemoryIndex};

    fn temp_store() -> (PathBuf, MemoryStore) {
        let dir = std::env::temp_dir().join(format!(
            "kz-memory-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::project(&dir);
        (dir, store)
    }

    fn add(
        store: &MemoryStore,
        category: &str,
        title: &str,
        desc: &str,
        body: &str,
    ) -> MemoryEntry {
        // force=true:测试 fixture 刻意构造批量条目,语义闸(R-216)会误拦「改写复述」,
        // 而 fixture 的目的正是造数据验证其它路径;真实 memory_add 不走这里。
        match store
            .add(category, title, desc, body, "user", &[], None, true)
            .unwrap()
        {
            AddOutcome::Added(e) => e,
            AddOutcome::Duplicate(e) => panic!("unexpected duplicate of {}", e.id),
            AddOutcome::SubjectConflict(e) => panic!("unexpected subject conflict with {}", e.id),
            AddOutcome::Uncertain(cands) => panic!("unexpected uncertain: {:?}", cands),
        }
    }

    fn record_current_recall(store: &MemoryStore, hits: &[SearchHit], injected_ids: &[&str]) {
        let root = store.project_root.as_deref().unwrap();
        let retrieved: Vec<&str> = hits.iter().map(|hit| hit.entry.id.as_str()).collect();
        let injected: Vec<&str> = retrieved
            .iter()
            .copied()
            .filter(|id| injected_ids.contains(id))
            .collect();
        let retrieved_json = serde_json::to_string(&retrieved).unwrap();
        let injected_json = serde_json::to_string(&injected).unwrap();
        let recall_id = format!("test-recall-{}", now_ms());
        let db = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root)).unwrap();
        db.record_recall_event(&kanzei_core::RecallEvent {
            recall_id: &recall_id,
            episode_id: None,
            step_id: None,
            trigger_type: "memory_search",
            trigger_payload: "{}",
            policy_action: "lexical",
            query: "要发版了",
            candidate_ids: &retrieved_json,
            retrieved_ids: &retrieved_json,
            injected_ids: &injected_json,
            lexical_ms: 1,
            embed_ms: 0,
            vector_ms: 0,
            total_ms: 1,
        })
        .unwrap();
    }

    #[test]
    fn index_description_guard_rejects_mismatched_source() {
        let (dir, store) = temp_store();
        let entry = add(
            &store,
            "fact",
            "INDEX guard title",
            "INDEX guard — description",
            "INDEX guard body",
        );
        let entries = store.load_all();
        let valid = format!(
            "- {} [{}] {} — {}\n",
            entry.id, entry.category, entry.title, entry.description
        );
        assert!(MemoryStore::assert_index_matches_entries(&valid, &entries).is_ok());
        let invalid = valid.replace("INDEX guard — description", "wrong description");
        assert!(MemoryStore::assert_index_matches_entries(&invalid, &entries).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn status_filter_is_applied_before_fts_limit() {
        let (dir, store) = temp_store();
        for i in 0..30 {
            let outcome = store
                .add(
                    "fact",
                    &format!("状态窗口候选 {i}"),
                    "状态窗口检索",
                    "状态窗口正文",
                    "memory-manager",
                    &[],
                    None,
                    false,
                )
                .unwrap();
            assert!(matches!(outcome, AddOutcome::Added(ref entry) if entry.status == "candidate"));
        }
        let active = add(
            &store,
            "fact",
            "状态窗口 active",
            "状态窗口检索",
            "状态窗口正文",
        );
        let rows = store
            .search_candidates("状态窗口", None, Some("active"))
            .unwrap();
        assert_eq!(
            rows.len(),
            1,
            "active 查询不能被 candidate 挤出 LIMIT 窗口: {rows:?}"
        );
        assert_eq!(rows[0].entry.id, active.id);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 检索守护_外部写入的文件失步后自动重建索引() {
        // 2026-08-13 清理事故形态:手动移动/写入 .md 绕过写路径的增量维护,
        // FTS 失步——新条目对 BM25 完全不可见、已归档条目仍在索引。
        // search 前的 id 集合守护必须自动 refresh_derived。
        let (dir, store) = temp_store();
        add(&store, "fact", "常规条目", "常规检索钩子", "正文 A");
        // 外部写入:直接落文件,不走 add(不触发派生物维护)。
        std::fs::write(
            store.root.join("M-099-外部恢复条目.md"),
            "---\nid: M-099\nscope: project\ncategory: sop\ntitle: 外部恢复条目 quasar 约束\ndescription: 外部写入的检索钩子 quasar\nstatus: active\ncreated: 2026-08-13\nupdated: 2026-08-13\nsource: test\n---\n\n正文 quasar",
        )
        .unwrap();
        let rows = store
            .search_candidates("quasar", None, Some("active"))
            .unwrap();
        assert!(
            rows.iter().any(|r| r.entry.id == "M-099"),
            "失步守护必须重建索引,外部写入的条目才可检索: {rows:?}"
        );
        // 反向:外部删除(归档)后,索引里的幽灵条目不再命中。
        std::fs::remove_file(store.root.join("M-099-外部恢复条目.md")).unwrap();
        let rows = store
            .search_candidates("quasar", None, Some("active"))
            .unwrap();
        assert!(
            rows.iter().all(|r| r.entry.id != "M-099"),
            "外部删除后幽灵条目不得再命中: {rows:?}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 写入前发现失步会自动重建_fts并与主目录对齐() {
        let (dir, store) = temp_store();
        let first = add(&store, "fact", "写入前守护首条", "失步守护检索", "正文");
        {
            let conn = store.open_db().unwrap();
            conn.execute("DELETE FROM memory_fts WHERE id = ?1", params![first.id])
                .unwrap();
        }

        let second = add(&store, "fact", "写入前守护次条", "失步守护检索", "正文二");
        let conn = store.open_db().unwrap();
        let indexed: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(indexed as usize, store.load_all().len());
        for id in [first.id, second.id] {
            let present: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memory_fts WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(present, 1, "写入后 FTS 缺少 {id}");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn add_assigns_ids_and_builds_derived_index() {
        let (dir, store) = temp_store();
        let a = add(
            &store,
            "fact",
            "CRLF 是 edit 未命中主因",
            "换行符问题必读",
            "正文 A",
        );
        let b = add(
            &store,
            "sop",
            "发版 SOP",
            "做发版相关任务必读",
            "1. 测试 2. 推送 3. 发布",
        );
        assert_eq!((a.id.as_str(), b.id.as_str()), ("M-001", "M-002"));
        let index = std::fs::read_to_string(store.root.join("INDEX.md")).unwrap();
        assert!(index.contains("M-001 [fact] CRLF 是 edit 未命中主因 — 换行符问题必读"));
        assert!(index.contains("M-002 [sop]"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn exact_duplicate_title_is_rejected_unless_forced() {
        let (dir, store) = temp_store();
        add(
            &store,
            "habit",
            "gh 要走本地代理",
            "gh 网络失败时必读",
            "HTTPS_PROXY=127.0.0.1:12000",
        );
        let outcome = store
            .add(
                "habit",
                "gh 要走本地代理!",
                "重复",
                "x",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        assert!(
            matches!(outcome, AddOutcome::Duplicate(ref e) if e.id == "U-001" || e.id == "M-001")
        );
        let forced = store
            .add(
                "habit",
                "gh 要走本地代理!",
                "强制新增",
                "x",
                "user",
                &[],
                None,
                true,
            )
            .unwrap();
        assert!(matches!(forced, AddOutcome::Duplicate(_)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn search_ranks_and_records_hits_and_rebuilds_after_db_loss() {
        let (dir, store) = temp_store();
        add(
            &store,
            "fact",
            "CRLF 是 edit 未命中主因",
            "处理 edit 替换失败换行符问题必读",
            "自动容忍已落地",
        );
        add(
            &store,
            "sop",
            "发版 SOP 两条通道",
            "发版发布安装更新相关必读",
            "package.ps1 -Publish 后静默装 setup",
        );
        // D-366:排序决策在检索门面,这里经 index 取决策排序后的 top-k。
        let index = SqliteMemoryIndex::new(&dir);
        let hits = index.search_entries(&IndexQuery::text("发版 更新"), None, Some("active"), 5);
        assert_eq!(
            hits[0].entry.id,
            "M-002",
            "{:?}",
            hits.iter().map(|h| &h.entry.id).collect::<Vec<_>>()
        );
        assert!(
            hits[0].snippet.contains('['),
            "snippet 高亮: {}",
            hits[0].snippet
        );
        // 命中计数生效(search_entries 在决策排序后记 record_hits)
        let again = index.search_entries(&IndexQuery::text("发版"), None, None, 5);
        assert!(again[0].hits >= 1);
        // 删库重建:真源是文件
        drop(again);
        std::fs::remove_file(store.db_path()).unwrap();
        store.refresh_derived().unwrap();
        let rebuilt = index.search_entries(&IndexQuery::text("CRLF"), None, None, 5);
        assert_eq!(rebuilt[0].entry.id, "M-001");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 召回明细可回看且采纳与否可机械判定() {
        let (dir, store) = temp_store();
        add(
            &store,
            "fact",
            "CRLF 是 edit 未命中主因",
            "处理 edit 替换失败必读",
            "自动容忍已落地",
        );
        add(
            &store,
            "sop",
            "发版 SOP 两条通道",
            "发版发布安装更新必读",
            "package.ps1 -Publish",
        );
        let index = SqliteMemoryIndex::new(&dir);
        let hits = index.search_entries(&IndexQuery::text("发版 更新"), None, Some("active"), 5);
        assert!(!hits.is_empty());

        let recall_id = store.record_recall("这轮要发版", &hits, 512);
        let rounds = store.recalls(10);
        assert_eq!(rounds.len(), 1, "召回明细未落库");
        assert_eq!(rounds[0].recall_id, recall_id);
        assert_eq!(
            rounds[0].injected_bytes, 512,
            "未记录注入字节数,上下文账单无从算起"
        );
        assert!(
            rounds[0].prompt_head.contains("发版"),
            "未记录触发本次召回的 prompt"
        );
        assert!(
            rounds[0].hits.iter().all(|h| h.score != 0.0),
            "未记录检索得分,看不出为什么召回这几条"
        );
        // 关键:召回但没拉正文 = 没起作用,不能默认算数。
        assert!(
            rounds[0].hits.iter().all(|h| !h.fetched),
            "刚召回还没拉正文就被判为已采纳,评估口径失真",
        );

        // 拉了正文才算采纳,而且只回填最近一次召回。
        let target = rounds[0].hits[0].id.clone();
        store.mark_recall_fetched(&target);
        let after = store.recalls(10);
        assert!(
            after[0]
                .hits
                .iter()
                .find(|h| h.id == target)
                .unwrap()
                .fetched,
            "拉取正文后未标记为已采纳",
        );
        assert!(
            after[0].hits.iter().filter(|h| h.fetched).count() == 1,
            "采纳标记不应扩散到同轮未被拉取的条目",
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn update_evolves_and_stale_downranks() {
        let (dir, store) = temp_store();
        let e = add(&store, "fact", "旧结论", "某场景必读", "V1");
        let updated = store
            .update(
                &e.id,
                None,
                None,
                Some("V2 修订"),
                Some("stale"),
                None,
                false,
            )
            .unwrap();
        assert_eq!(updated.status, "deprecated"); // R-165:stale 兼容映射 deprecated
        assert_eq!(updated.body, "V2 修订");
        assert_eq!(updated.created, e.created);
        // stale 默认不出现在 active 过滤下(存储侧候选集过滤语义)
        let active_only = store
            .search_candidates("结论", None, Some("active"))
            .unwrap();
        assert!(active_only.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-165 批1:编译产物(candidate)必须带 episode 证据才能 promote 进 active——
    /// 无 provenance 的记忆永不可注入检索,引擎强制不靠 manager 自觉。
    #[test]
    fn promote_requires_provenance_hard_gate() {
        let (dir, store) = temp_store();
        // source != user → candidate(manager 编译产物)
        let e = store
            .add(
                "fact",
                "编译事实",
                "编译钩子",
                "body",
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(candidate) = e else {
            panic!("expected Added");
        };
        assert_eq!(candidate.status, "candidate");
        // 无证据 → 拒绝
        let err = store.promote(&candidate.id, &[], None).unwrap_err();
        assert!(err.to_string().contains("no memory_sources evidence"));
        // 状态仍是 candidate
        let (_, after) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == candidate.id)
            .unwrap();
        assert_eq!(after.status, "candidate");
        // 有证据 → 晋升 active(R-213:episode 必须真实存在,先 seed 真实轮次)
        let eid = crate::memory::seed_episode(&dir, "ses");
        let promoted = store
            .promote(
                &candidate.id,
                &[(eid, Some(0), Some(10))],
                Some("test-hash"),
            )
            .unwrap();
        assert_eq!(promoted.status, "active");
        // 晋升后再次 promote 拒绝(candidate|shadow 才可晋升,active 不行)
        let err2 = store
            .promote(&candidate.id, &[(2, None, None)], None)
            .unwrap_err();
        assert!(err2
            .to_string()
            .contains("only candidate|shadow can be promoted"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-165 批1:source=="user" 用户直写是最高权证据,直接 active(不走编译门禁)。
    #[test]
    fn user_written_entry_is_active_directly() {
        let (dir, store) = temp_store();
        let e = store
            .add(
                "preference",
                "开发重心",
                "取活必读",
                "先清缺陷",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(entry) = e else {
            panic!("expected Added");
        };
        assert_eq!(entry.status, "active");
        // find_preference 只找 active:user 直写偏好立即可用
        assert!(store.find_preference("开发重心").is_some());
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-165 批2(验收④):novelty gate 三档分流——明显重复 NOOP、
    /// 明显新 PROPOSE、不确定才留 LLM,计数落遥测表。
    #[test]
    fn novelty_gate_three_tiers_with_telemetry() {
        let (dir, store) = temp_store();
        store
            .add(
                "fact",
                "gh 网络代理",
                "push 前必读:设置 HTTPS_PROXY",
                "HTTPS_PROXY=http://127.0.0.1:12000",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        // 明显重复:规范化标题精确命中 active 记忆。
        let (dup, _) = store.classify_novelty("GH 网络代理", "push 前必读", "");
        assert_eq!(dup, Novelty::Duplicate, "规范化标题应命中 active 记忆");
        // 明显新:无重叠词。
        let (fresh, _) = store.classify_novelty("diff 树渲染优化", "R-133 diff 渲染", "");
        assert_eq!(fresh, Novelty::New, "无关主题应判明显新");
        // 不确定:有语义命中且标题高度重合——先建一条长标题条目,再 classify 一个
        // 共享 ≥8 token 的改写标题(R-216 口径:共享 token ≥8 且比例 ≥0.55)。
        let base = "secure configuring github network proxy connection and verification";
        store
            .add(
                "fact",
                base,
                "HTTPS_PROXY 与代理地址",
                "HTTPS_PROXY=http://127.0.0.1:12000",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        // 英文改写(调序+换词,共享全部 8+ token):R-216 验收①「英文改写 M-044 被拦」。
        let (uncertain, candidates) = store.classify_novelty(
            "verification and secure connection of github network proxy configuring",
            "HTTPS_PROXY 与代理地址",
            "",
        );
        assert_eq!(
            uncertain,
            Novelty::Uncertain,
            "高度重合英文改写应判不确定(留 add 硬闸拦截,验收①)"
        );
        assert!(!candidates.is_empty(), "Uncertain 应返回候选条目");
        assert!(
            candidates.iter().any(|c| c.title == base),
            "候选应含被改写的基础条目"
        );
        // 计数遥测落库。
        store.record_novelty(&dup, "", "GH 网络代理");
        store.record_novelty(&fresh, "", "diff 树渲染优化");
        store.record_novelty(&uncertain, "", base);
        let conn = store.open_db().unwrap();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM novelty_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 3, "三档分流都要有计数遥测记录");
        let verdicts: Vec<String> = conn
            .prepare("SELECT verdict FROM novelty_events ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(verdicts, vec!["duplicate", "new", "uncertain"]);
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-165 批2(验收②):recurrence 三段晋升——跨轮计数递增,
    /// 第 1 次=1、第 2 次=2、第 3 次=3,harvest_failures 按档位写笔记。
    #[test]
    fn recurrence_three_stage_promotion_counts() {
        let (dir, store) = temp_store();
        let fp = format!(
            "[fp:edit|old string not found #{}-{}]",
            std::process::id(),
            "rec"
        );
        assert_eq!(store.recurrence_count(&fp), 0, "未出现前计数为 0");
        assert_eq!(store.bump_recurrence(&fp), 1, "第 1 次出现");
        assert_eq!(
            store.bump_recurrence(&fp),
            2,
            "第 2 次出现 → candidate 档位"
        );
        assert_eq!(store.bump_recurrence(&fp), 3, "第 3 次出现 → promote 档位");
        assert_eq!(store.recurrence_count(&fp), 3, "只读查询不递增");
        assert_eq!(store.bump_recurrence(&fp), 4);
        // 计数持久化跨 store 实例(同一 index.db)。
        let reopened = MemoryStore::open(store.scope, store.root.clone());
        assert_eq!(reopened.recurrence_count(&fp), 4, "持久计数跨实例可见");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-165 批4(验收⑤):evidence(memory_sources)无自治写路径——
    /// 只有 promote() 写它,且每条证据落在 state.db 可 join。
    /// (代码审计:record_memory_source 生产调用方唯一 = store.rs promote 内。)
    #[test]
    fn promote_is_sole_evidence_writer_and_rows_land() {
        let (dir, store) = temp_store();
        let e = store
            .add(
                "fact",
                "证据编译条目",
                "证据钩子",
                "正文",
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(candidate) = e else {
            panic!("expected Added")
        };
        // 先初始化 state.db schema(promote 内 open 依赖 schema;测试环境无现成库)。
        let path = dir.join(".kanzei").join("state.db");
        kanzei_core::SessionStore::open(&path).unwrap();
        // episode 7 必须先存在:memory_sources 有外键 REFERENCES episodes。
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO episodes(episode_id, session_id, created_at, prompt_head, outcome, steps,
                                      input_tokens, output_tokens, tools_json, context_json)
                 VALUES (7, 'test-session', 0, '测试轮', 'ok', 1, 10, 10, '[]', '{}')",
                [],
            )
            .unwrap();
        }
        store
            .promote(
                &candidate.id,
                &[(7, Some(100), Some(200))],
                Some("audit-hash"),
            )
            .unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        let (memory_id, episode_id, source_hash): (String, i64, String) = conn
            .query_row(
                "SELECT memory_id, episode_id, source_hash FROM memory_sources WHERE memory_id = ?1",
                params![candidate.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(memory_id, candidate.id);
        assert_eq!(episode_id, 7);
        assert_eq!(source_hash, "audit-hash");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-213 验收①:伪造 episode_id 的 promote 被拒——「无来源不入 active」必须是
    /// 「来源指向真实轮次」,编造一个 episodes 表里不存在的 id 不得蒙混晋升。
    #[test]
    fn promote_rejects_fabricated_episode_id() {
        let (dir, store) = temp_store();
        let e = store
            .add(
                "fact",
                "伪造证据条目",
                "伪造钩子",
                "正文",
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(candidate) = e else {
            panic!("expected Added")
        };
        // state.db 里没有任何 episode,999_999 必然不存在。
        let err = store
            .promote(&candidate.id, &[(999_999, None, None)], None)
            .unwrap_err();
        assert!(
            err.to_string().contains("does not exist"),
            "伪造 episode_id 应被拒: {err}"
        );
        // 状态仍 candidate,未晋升。
        let (_, after) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == candidate.id)
            .unwrap();
        assert_eq!(after.status, "candidate");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-213 验收②:写证据失败不产生 active 条目——episode 校验通过后,evidence
    /// 落库若失败(如 memory_sources 表被破坏),promote 必须整体失败且条目保持原状态。
    #[test]
    fn promote_write_evidence_failure_does_not_activate() {
        let (dir, store) = temp_store();
        let e = store
            .add(
                "fact",
                "证据写失败条目",
                "写失败钩子",
                "正文",
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(candidate) = e else {
            panic!("expected Added")
        };
        // seed 真实 episode,让 episode_exists 校验通过。
        let eid = crate::memory::seed_episode(&dir, "ses");
        // 人为制造证据写失败:drop memory_sources 表(migrate 版本已对齐不会再重建)。
        let db = dir.join(".kanzei").join("state.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch("DROP TABLE memory_sources").unwrap();
        }
        let err = store
            .promote(&candidate.id, &[(eid, Some(0), Some(5))], Some("test"))
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to record memory_source evidence"),
            "写证据失败应整体失败: {err}"
        );
        // 未晋升:仍是 candidate。
        let (_, after) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == candidate.id)
            .unwrap();
        assert_eq!(after.status, "candidate");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-233 ②:intent_query 从用户 prompt 提取意图词——整句中文经虚词边界
    /// 切段后,实词 bigram 不再被虚词错位(「批发/版出」→「发版」可命中)。
    #[test]
    fn intent_query_extracts_content_terms_around_boundaries() {
        // 发版:整句 bigram 会错位成 批发/版出;意图词提取后能出 发版。
        let q = intent_query("帮我把这一批发版出去");
        assert!(q.contains("发版"), "应提取出发版: {q}");
        assert!(!q.contains("帮"), "语气字不应进查询词: {q}");
        // ASCII 词原样保留(≥2 字母)。
        let q2 = intent_query("评估 harness 质量");
        assert!(q2.contains("harness"), "{q2}");
        assert!(q2.contains("质量"), "{q2}");
        // 纯虚词/语气 prompt → 空(调用方据此跳过检索)。
        let q3 = intent_query("请帮我一下好吗");
        assert!(q3.trim().is_empty(), "{q3}");
        // 长内容段拆 bigram 仍保留实词对。
        let q4 = intent_query("发版流程要走哪些步骤");
        assert!(q4.contains("发版") && q4.contains("步骤"), "{q4}");
        // 封顶 24 词。
        let long = format!("{} 结束", "甲".repeat(60));
        let q5 = intent_query(&long);
        assert!(q5.split_whitespace().count() <= 24, "{q5}");
        // 端到端召回:意图词必须真的命中含「发版」的条目,而非只满足
        // 字符串包含——「批发版」整段短语(逐字相邻)匹配不到「发版」,
        // 必须靠 3-4 字段补交的 bigram「发版」召回。
        let (dir, store) = temp_store();
        let e = add(
            &store,
            "sop",
            "发版 SOP",
            "发布流程检索钩子",
            "发布版本的操作步骤正文",
        );
        let rows = store
            .search_candidates(&intent_query("帮我把这一批发版出去"), None, Some("active"))
            .unwrap();
        assert!(
            rows.iter().any(|r| r.entry.id == e.id),
            "「批发版」整段短语匹配不到「发版」,必须靠 bigram 召回: {rows:?}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-165 批3(验收③):deprecated/invalid 移入 archive/ 且默认检索不可见(D-231)。
    #[test]
    fn deprecated_moves_to_archive_and_hidden_from_search() {
        let (dir, _) = temp_store();
        let store = MemoryStore::project(&dir);
        let e = store
            .add(
                "fact",
                "将被推翻的结论",
                "旧钩子",
                "旧内容",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(entry) = e else {
            panic!("expected Added")
        };
        // 设置 deprecated → refresh_derived 自动归档。
        store
            .update(&entry.id, None, None, None, Some("deprecated"), None, false)
            .unwrap();
        // 主目录已无该文件,archive/ 有墓碑。
        assert!(!store
            .root
            .join(format!("{}.md", entry.file_stem()))
            .exists());
        let logs = kanzei_base::write_log::entries_after(&dir, 0);
        let source_path = format!(".kanzei/memory/{}.md", entry.file_stem());
        let archive_path = format!(".kanzei/memory/archive/{}.md", entry.file_stem());
        assert!(
            logs.iter().any(|log| {
                log.path == source_path
                    && log.content.is_empty()
                    && log.fingerprint == kanzei_base::content_hash(&[])
            }),
            "归档必须为源文件删除留下写日志: {logs:?}"
        );
        let archived_bytes = std::fs::read(
            store
                .archive_dir()
                .join(format!("{}.md", entry.file_stem())),
        )
        .unwrap();
        assert!(
            logs.iter().any(|log| {
                log.path == archive_path
                    && log.fingerprint == kanzei_base::content_hash(&archived_bytes)
            }),
            "归档必须为 archive 目标留下写日志: {logs:?}"
        );
        // load_all 不含它(默认检索范围),load_archived_ids 保留 ID 防复用。
        assert!(store
            .archive_dir()
            .join(format!("{}.md", entry.file_stem()))
            .exists());
        // load_all 不含它(默认检索范围),load_archived_ids 保留 ID 防复用。
        assert!(store.load_all().iter().all(|(_, e)| e.id != entry.id));
        assert!(store.load_archived_ids().contains(&entry.id));
        // 默认检索(search 无 status 过滤或 active 过滤)都不可见。
        let hits = store.search_candidates("将被推翻", None, None).unwrap();
        assert!(
            hits.iter().all(|h| h.entry.id != entry.id),
            "归档条目不得被检索"
        );
        // invalid 同样归档。
        let i = store
            .add(
                "fact",
                "证伪的假设",
                "证伪钩子",
                "证伪内容",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(invalid) = i else {
            panic!("expected Added")
        };
        store
            .update(&invalid.id, None, None, None, Some("invalid"), None, false)
            .unwrap();
        assert!(!store.load_all().iter().any(|(_, e)| e.id == invalid.id));
        assert!(store
            .archive_dir()
            .join(format!("{}.md", invalid.file_stem()))
            .exists());
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-166 批3(验收③):shadow 条目不注入生产检索但可被评估——默认检索
    /// (无 status 或 active 过滤)不可见,显式 status=shadow 查询可见(评估器用);
    /// shadow → active 需 provenance(candidate 同规则)。
    #[test]
    fn shadow_entry_is_evaluable_but_not_injected() {
        let (dir, store) = temp_store();
        let e = store
            .add(
                "fact",
                "影子评估条目",
                "影子钩子",
                "影子正文",
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(candidate) = e else {
            panic!("expected Added")
        };
        // candidate → shadow。
        let shadowed = store.to_shadow(&candidate.id).unwrap();
        assert_eq!(shadowed.status, "shadow");
        // 默认检索与 active 过滤都不可见(不注入生产)。
        let hits_default = store.search_candidates("影子评估", None, None).unwrap();
        assert!(
            hits_default.iter().all(|h| h.entry.id != candidate.id),
            "shadow 不得进默认检索"
        );
        let hits_active = store
            .search_candidates("影子评估", None, Some("active"))
            .unwrap();
        assert!(
            hits_active.iter().all(|h| h.entry.id != candidate.id),
            "shadow 不得冒充 active"
        );
        // 显式查 shadow 可见(评估器通道)。
        let hits_shadow = store
            .search_candidates("影子评估", None, Some("shadow"))
            .unwrap();
        assert!(
            hits_shadow.iter().any(|h| h.entry.id == candidate.id),
            "显式查 shadow 应可见"
        );
        // shadow → active 仍需 provenance。
        let err = store.promote(&candidate.id, &[], None).unwrap_err();
        assert!(err.to_string().contains("no memory_sources evidence"));
        let eid = crate::memory::seed_episode(&dir, "ses");
        let promoted = store
            .promote(
                &candidate.id,
                &[(eid, Some(0), Some(5))],
                Some("shadow-hash"),
            )
            .unwrap();
        assert_eq!(promoted.status, "active");
        // 进 active 后默认检索可见。
        let hits_after = store.search_candidates("影子评估", None, None).unwrap();
        assert!(
            hits_after.iter().any(|h| h.entry.id == candidate.id),
            "active 后应可检索"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-166 批3:非 candidate 不能进 shadow;shadow 不落入 archive_dead(它是
    /// 中间态,不是失效态)——refresh_derived 后主目录仍保留 shadow 文件。
    #[test]
    fn shadow_rejects_non_candidate_and_survives_refresh() {
        let (dir, store) = temp_store();
        let e = store
            .add(
                "fact",
                "直写活跃条目",
                "活跃钩子",
                "正文",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(active) = e else {
            panic!("expected Added")
        };
        assert_eq!(active.status, "active");
        let err = store.to_shadow(&active.id).unwrap_err();
        assert!(err.to_string().contains("only candidate can enter shadow"));
        // candidate 进 shadow 后 refresh_derived(任意写触发)不归档它。
        let c = store
            .add(
                "fact",
                "影子常驻",
                "常驻钩子",
                "正文",
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(candidate) = c else {
            panic!("expected Added")
        };
        store.to_shadow(&candidate.id).unwrap();
        // 用一次无关 update 触发 refresh_derived。
        store
            .update(
                &active.id,
                Some("直写活跃条目改名"),
                None,
                None,
                None,
                None,
                false,
            )
            .unwrap();
        assert!(
            store.load_all().iter().any(|(_, e)| e.id == candidate.id),
            "shadow 是中间态,不得被归档"
        );
        assert_eq!(
            store
                .load_all()
                .into_iter()
                .find(|(_, e)| e.id == candidate.id)
                .unwrap()
                .1
                .status,
            "shadow"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn integrity_detects_holes_and_duplicates() {
        let (dir, store) = temp_store();
        add(&store, "fact", "一号", "钩子一", "x");
        let b = add(&store, "fact", "三号占位", "钩子三", "x");
        // 手工制造缺号:把 M-002 位空出来(改 id 为 M-003)
        store
            .update(&b.id, None, None, None, None, None, false)
            .unwrap();
        let path = store.root.join(format!("{}.md", b.file_stem()));
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("id: M-002", "id: M-003");
        std::fs::write(&path, text).unwrap();
        let issues = store.integrity_issues();
        assert!(issues.iter().any(|i| i.contains("M-002")), "{issues:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-321:注销通道——缺号登记到 voided-ids.md 后 integrity 不再报 MISSING;
    /// 文案在无 git 时给可执行处置(检查备份/注销),而不是误导性 git 恢复指引。
    #[test]
    fn void_id_acknowledges_gap_and_message_is_honest() {
        let (dir, store) = temp_store();
        add(&store, "fact", "一号", "钩子一", "x");
        let b = add(&store, "fact", "三号占位", "钩子三", "x");
        // 手工制造缺号:把 M-002 位空出(改 id 为 M-003)
        let path = store.root.join(format!("{}.md", b.file_stem()));
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("id: M-002", "id: M-003");
        std::fs::write(&path, text).unwrap();
        // 临时目录无 git → 文案不得指引 git 恢复,必须给可执行处置
        let issues = store.integrity_issues();
        let missing = issues
            .iter()
            .find(|i| i.contains("MISSING ids"))
            .expect("应有 MISSING 报告");
        assert!(missing.contains("no git backup"), "{missing}");
        assert!(!missing.contains("restore from git"), "{missing}");
        assert!(missing.contains("voided-ids.md"), "{missing}");
        // 注销 M-002(确认丢失)→ 不再报 MISSING
        store.void_id("M-002", "误删且无备份,确认丢失").unwrap();
        let issues = store.integrity_issues();
        assert!(
            !issues.iter().any(|i| i.contains("M-002")),
            "注销后不得再报 M-002: {issues:?}"
        );
        // 台账落盘且人可读
        let ledger = std::fs::read_to_string(store.voided_ledger_file()).unwrap();
        assert!(
            ledger.contains("- M-002: 误删且无备份,确认丢失"),
            "{ledger}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-321:注销前置校验——活着的条目/短理由/错前缀都拒;重复注销幂等。
    #[test]
    fn void_id_validates_and_is_idempotent() {
        let (dir, store) = temp_store();
        let a = add(&store, "fact", "活条目", "钩子", "x");
        let err = store.void_id(&a.id, "想清掉活条目").unwrap_err();
        assert!(err.to_string().contains("仍存在于活动或归档"), "{err}");
        let err = store.void_id("M-999", "短").unwrap_err();
        assert!(err.to_string().contains("理由"), "{err}");
        let err = store.void_id("D-999", "理由足够长但前缀错").unwrap_err();
        assert!(err.to_string().contains("不是"), "{err}");
        // 缺号注销:两次调用幂等(第二次 Ok 且不重复写行)
        store.void_id("M-999", "测试注销的缺号").unwrap();
        store.void_id("M-999", "测试注销的缺号").unwrap();
        let ledger = std::fs::read_to_string(store.voided_ledger_file()).unwrap();
        assert_eq!(ledger.matches("- M-999:").count(), 1, "{ledger}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-321:登记为 voided 的编号若又出现条目(手工改号/恢复),账实不符必须可见。
    #[test]
    fn voided_id_resurrected_is_flagged() {
        let (dir, store) = temp_store();
        let a = add(&store, "fact", "一号", "钩子一", "x");
        let b = add(&store, "fact", "二号", "钩子二", "x");
        // 制造缺号:删掉 M-002 文件
        let path = store.root.join(format!("{}.md", b.file_stem()));
        std::fs::remove_file(&path).unwrap();
        store.void_id("M-002", "二号被误删").unwrap();
        assert!(
            !store.integrity_issues().iter().any(|i| i.contains("M-002")),
            "注销后缺号不应再报"
        );
        // 把 M-001 改号成 M-002(模拟手工恢复),复活必须被点名
        let p1 = store.root.join(format!("{}.md", a.file_stem()));
        let text = std::fs::read_to_string(&p1)
            .unwrap()
            .replace("id: M-001", "id: M-002");
        std::fs::write(&p1, text).unwrap();
        let issues = store.integrity_issues();
        assert!(
            issues
                .iter()
                .any(|i| i.contains("recorded as voided") && i.contains("M-002")),
            "{issues:?}"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-321:文案诚实性——目录在 git 版本控制下才指引 restore from git。
    #[test]
    fn missing_message_honors_git_presence() {
        let (dir, store) = temp_store();
        // 临时目录根放一个 .git,祖先探测应命中
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        add(&store, "fact", "一号", "钩子一", "x");
        let b = add(&store, "fact", "三号占位", "钩子三", "x");
        let path = store.root.join(format!("{}.md", b.file_stem()));
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("id: M-002", "id: M-003");
        std::fs::write(&path, text).unwrap();
        let issues = store.integrity_issues();
        let missing = issues
            .iter()
            .find(|i| i.contains("MISSING ids"))
            .expect("应有 MISSING 报告");
        assert!(missing.contains("restore from git"), "{missing}");
        assert!(!missing.contains("no git backup"), "{missing}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn merge_keeps_primary_and_tombstones_duplicates() {
        let (dir, store) = temp_store();
        let a = add(
            &store,
            "habit",
            "gh 走代理",
            "gh 网络问题必读",
            "端口 12000",
        );
        let b = add(
            &store,
            "habit",
            "gh 需要 HTTPS_PROXY",
            "gh 超时必读",
            "同上",
        );
        let merged = store
            .merge(
                &a.id,
                std::slice::from_ref(&b.id),
                None,
                Some("gh 网络失败/超时必读"),
                Some("HTTPS_PROXY=http://127.0.0.1:12000"),
                true, // 测试无共享指纹:confirmed=true 模拟用户确认(R-165 保守闸)
            )
            .unwrap();
        assert_eq!(merged.id, a.id);
        assert_eq!(merged.description, "gh 网络失败/超时必读");
        // R-165 批3:墓碑条目归档到 archive/(主目录只留 active/candidate)。
        assert!(store.load_all().iter().all(|(_, e)| e.id != b.id));
        let dup_path = std::fs::read_dir(store.archive_dir())
            .unwrap()
            .flatten()
            .find(|p| {
                p.path()
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(&b.id))
                    .unwrap_or(false)
            })
            .expect("archive 墓碑应存在")
            .path();
        let dup_text = std::fs::read_to_string(dup_path).unwrap();
        assert!(dup_text.contains("status: deprecated"));
        assert!(
            dup_text.contains(&format!("superseded_by: {}", a.id)),
            "{dup_text}"
        );
        // 未知 id 与自我合并都拒绝
        assert!(store
            .merge(&a.id, std::slice::from_ref(&a.id), None, None, None, true)
            .is_err());
        assert!(store
            .merge("M-999", std::slice::from_ref(&b.id), None, None, None, true)
            .is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-165 批4 merge 保守闸(⑧):评估器落地前只合并同 fingerprint 或用户确认的。
    #[test]
    fn merge_conservative_gate_requires_shared_fingerprint_or_confirmed() {
        let (dir, store) = temp_store();
        // R-216:指纹必须来自来源 note 才放行——先注入来源,再测 merge 保守闸
        // (保守闸本身验证共享指纹放行,不是指纹闸)。
        store
            .append_note("edit 未命中来源", "[fp:edit|not found]", "fact", &[])
            .unwrap();
        let a = add(&store, "fact", "主题甲", "钩子甲", "正文甲");
        let b = add(&store, "fact", "主题乙", "钩子乙", "正文乙");
        // 无 confirmed、无共享指纹 → 拒绝。
        let err = store
            .merge(&a.id, std::slice::from_ref(&b.id), None, None, None, false)
            .unwrap_err();
        assert!(
            err.to_string().contains("无共享 fingerprint"),
            "保守闸应点名缺共享指纹: {err}"
        );
        // 有共享指纹 → 放行。
        let c = store
            .add(
                "fact",
                "同坑另一个角度",
                "钩子丙",
                "正文 [fp:edit|not found]",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(c) = c else {
            panic!("expected Added")
        };
        let d = store
            .add(
                "fact",
                "同坑第三个角度",
                "钩子丁",
                "补充 [fp:edit|not found]",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        let AddOutcome::Added(d) = d else {
            panic!("expected Added")
        };
        let merged = store
            .merge(&c.id, std::slice::from_ref(&d.id), None, None, None, false)
            .unwrap();
        assert_eq!(merged.id, c.id);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn upsert_preference_reuses_one_entry_across_switches() {
        // 开发重心会被反复切换:必须复用同一条目,否则索引被同类定调撑爆、历史也无从对照。
        let (dir, store) = temp_store();
        let first = store
            .upsert_preference(
                "开发重心",
                "开发重心:缺陷优先",
                "取活时必读",
                "先扫 defects.md",
            )
            .unwrap();
        assert_eq!(first.category, "preference");
        let second = store
            .upsert_preference(
                "开发重心",
                "开发重心:需求优先",
                "取活时必读",
                "先扫 requirements.md",
            )
            .unwrap();
        assert_eq!(second.id, first.id, "切换必须改同一条,不能新增");
        assert_eq!(second.body, "先扫 requirements.md");
        assert_eq!(store.load_all().len(), 1);
        assert_eq!(
            store.find_preference("开发重心").map(|e| e.title),
            Some("开发重心:需求优先".to_string())
        );
        // 与其它偏好条目互不干扰
        store
            .upsert_preference(
                "提交署名",
                "提交署名:不带 Co-Authored-By",
                "提交时必读",
                "只署用户本人",
            )
            .unwrap();
        assert_eq!(store.load_all().len(), 2);
        assert!(store
            .find_preference("开发重心")
            .unwrap()
            .title
            .contains("需求优先"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn inbox_batch_respects_note_and_budget_limits() {
        let (dir, store) = temp_store();
        store.append_note("批次 A", "短内容", "fact", &[]).unwrap();
        store.append_note("批次 B", "短内容", "fact", &[]).unwrap();
        store.append_note("批次 C", "短内容", "fact", &[]).unwrap();

        let batch = store.read_inbox_batch(2, usize::MAX, usize::MAX).unwrap();
        assert_eq!(batch.note_count, 2);
        assert!(batch.text.contains("批次 A"));
        assert!(batch.text.contains("批次 B"));
        assert!(!batch.text.contains("批次 C"));
        assert_eq!(batch.bytes, batch.text.len());
        assert!(batch.estimated_tokens > 0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn inbox_batch_allows_one_oversized_note_and_checkpoint_roundtrips() {
        let (dir, store) = temp_store();
        store
            .append_note("超大首条", &"x".repeat(256), "fact", &[])
            .unwrap();
        store.append_note("后续条目", "y", "fact", &[]).unwrap();

        let batch = store.read_inbox_batch(10, 8, 2).unwrap();
        assert_eq!(batch.note_count, 1, "首条超限也必须被处理，不能饿死队列");
        assert!(batch.text.contains("超大首条"));

        let checkpoint = crate::memory::InboxCheckpoint {
            batch_id: "inbox-test-1".into(),
            status: "completed".into(),
            input_notes: 1,
            input_bytes: batch.bytes,
            success_notes: 1,
            pending_after: 1,
            failure_reason: None,
            consecutive_failures: 0,
            updated_at_ms: 42,
        };
        store.write_inbox_checkpoint(&checkpoint).unwrap();
        assert_eq!(store.read_inbox_checkpoint(), Some(checkpoint));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn inbox_batch_byte_budget_stops_before_next_note() {
        let (dir, store) = temp_store();
        store.append_note("第一条", "a", "fact", &[]).unwrap();
        store.append_note("第二条", "b", "fact", &[]).unwrap();
        let first = store.read_inbox_batch(10, 1, usize::MAX).unwrap();
        assert_eq!(first.note_count, 1);
        assert!(first.text.contains("第一条"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn inbox_roundtrip_and_clear() {
        let (dir, store) = temp_store();
        store
            .append_note("纯 ui 改动只跑 node 检查", "细节", "habit", &[])
            .unwrap();
        store.append_note("发版走两条通道", "", "sop", &[]).unwrap();
        assert_eq!(store.pending_notes(), 2);
        assert!(store.read_inbox().contains("纯 ui 改动只跑 node 检查"));
        store.clear_inbox().unwrap();
        assert_eq!(store.pending_notes(), 0);
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-215 验收①:20 条积压逐条 discard 能在数轮内收敛到 0(逐条销账而非整箱清)。
    #[test]
    fn 二十条积压逐条销账收敛到零() {
        let (dir, store) = temp_store();
        for i in 0..20 {
            store
                .append_note(&format!("积压 note 第 {i} 条"), "", "fact", &[])
                .unwrap();
        }
        assert_eq!(store.pending_notes(), 20);
        // 逐条按指纹销账:每条处理后删该条,不碰其余。
        for i in 0..20 {
            let removed = store.discard_note(&format!("积压 note 第 {i} 条")).unwrap();
            assert!(removed, "第 {i} 条应被销账");
            assert_eq!(store.pending_notes(), 19 - i, "销账后应逐条减少");
        }
        assert_eq!(store.pending_notes(), 0, "20 条应收敛到 0");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-215 验收③:消化清空窗口封死——discard 只删已处理指纹,并发 append 的新
    /// note 存活;整箱 clear 前先 discard 已见条,新 note 不被吃。
    #[test]
    fn 逐条销账不吃并发新note_窗口封死() {
        let (dir, store) = temp_store();
        store.append_note("已处理 note A", "", "fact", &[]).unwrap();
        // 模拟处理完 A 后、清空前,并发 append 了 B。
        store.append_note("并发新 note B", "", "fact", &[]).unwrap();
        // 只销账已处理的 A,B 必须存活。
        let removed = store.discard_note("已处理 note A").unwrap();
        assert!(removed);
        assert!(
            store.read_inbox().contains("并发新 note B"),
            "discard A 不得吃掉 B(窗口封死)"
        );
        assert_eq!(store.pending_notes(), 1, "B 应留在箱中待下轮处理");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-215 验收②:并发 append 压测零丢 note——多线程同时 append_note,全部落盘。
    #[test]
    fn 并发append零丢note() {
        let (dir, store) = temp_store();
        let store = std::sync::Arc::new(store);
        let mut handles = Vec::new();
        for i in 0..12 {
            let store = store.clone();
            handles.push(std::thread::spawn(move || {
                store
                    .append_note(&format!("并发 note {i}"), "", "fact", &[])
                    .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(store.pending_notes(), 12, "12 条并发 append 一条都不能丢");
        for i in 0..12 {
            assert!(
                store.read_inbox().contains(&format!("并发 note {i}")),
                "note {i} 应落盘"
            );
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn add_and_note_carry_refs_contract() {
        // R-070:add 带 refs 写入 frontmatter,读取方 refs() 还原;草稿带 refs 贯通到候选列表。
        let (dir, store) = temp_store();
        let entry = match store
            .add(
                "fact",
                "CRLF 是 edit 未命中主因",
                "换行符问题必读",
                "正文",
                "user",
                &["R-070".into(), "D-200".into()],
                None,
                false,
            )
            .unwrap()
        {
            AddOutcome::Added(e) => e,
            AddOutcome::Duplicate(e) => panic!("unexpected duplicate {}", e.id),
            AddOutcome::SubjectConflict(e) => panic!("unexpected subject conflict with {}", e.id),
            AddOutcome::Uncertain(cands) => panic!("unexpected uncertain: {:?}", cands),
        };
        assert_eq!(entry.refs(), vec!["R-070".to_string(), "D-200".to_string()]);
        // 落盘文件里真能看到 refs 键,重读后仍还原。
        let (path, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == entry.id)
            .unwrap();
        let file_text = std::fs::read_to_string(&path).unwrap();
        assert!(file_text.contains("refs: R-070 D-200"), "{file_text}");
        let reloaded = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == entry.id)
            .unwrap()
            .1;
        assert_eq!(reloaded.refs(), entry.refs());
        // 无 refs 时不写键。
        let plain = add(&store, "fact", "无来源条目", "普通", "x");
        assert!(plain.refs().is_empty());
        // 草稿贯通:refs 行进入 inbox,候选列表 detail 可见。
        store
            .append_note("踩坑", "细节说明", "fact", &["R-070".into()])
            .unwrap();
        let (_, summary, detail) = store.pending_note_list().pop().unwrap();
        assert_eq!(summary, "踩坑");
        assert!(detail.contains("refs: R-070"), "{detail}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn legacy_memory_md_migrates_once() {
        let dir = std::env::temp_dir().join(format!(
            "kz-memory-migrate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/memory.md"),
            "# Memory\n\n## M-001 子代理网络错误会整体失败 [active]\n- 依据: 实测\n\n## M-002 已推翻的结论 [stale]\n- 备注: 旧\n",
        )
        .unwrap();
        let store = MemoryStore::project(&dir);
        let entries = store.load_all();
        // R-165 批3:迁移的 stale 条目(deprecated)自动归档,主目录只留 active。
        assert_eq!(
            entries.len(),
            1,
            "deprecated 迁移条目应已归档: {:?}",
            entries
        );
        let m1 = entries.iter().find(|(_, e)| e.id == "M-001").unwrap();
        assert_eq!(m1.1.category, "fact");
        assert_eq!(m1.1.source, "migration");
        assert!(m1.1.body.contains("依据: 实测"));
        let archived_m2 = std::fs::read_dir(store.archive_dir())
            .unwrap()
            .flatten()
            .find(|p| {
                p.path()
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("M-002"))
                    .unwrap_or(false)
            })
            .expect("M-002 归档墓碑应存在");
        let m2_text = std::fs::read_to_string(archived_m2.path()).unwrap();
        assert!(m2_text.contains("status: deprecated"), "{m2_text}");
        // 原文件变为指路牌,重复 open 不再迁移
        let legacy = std::fs::read_to_string(dir.join(".kanzei/project/memory.md")).unwrap();
        assert!(legacy.contains("已迁移"));
        let again = MemoryStore::project(&dir);
        assert_eq!(again.load_all().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn subject_状态不变量_同主题至多一条_active_且_force_不可绕() {
        let (dir, store) = temp_store();
        let first = match store
            .add(
                "fact",
                "安装通道:NSIS 安装版",
                "查安装/更新通道时必读",
                "AppData 下",
                "user",
                &[],
                Some("安装通道"),
                false,
            )
            .unwrap()
        {
            AddOutcome::Added(e) => e,
            _ => panic!("expected add"),
        };
        // subject 写进 frontmatter,重读还原。
        let (path, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == first.id)
            .unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("subject: 安装通道"));

        // 标题不同、subject 相同 → 冲突,返回既有条目。
        let conflict = store
            .add(
                "fact",
                "安装通道改为便携版",
                "查安装通道必读",
                "新状态",
                "user",
                &[],
                Some("安装通道"),
                false,
            )
            .unwrap();
        assert!(matches!(conflict, AddOutcome::SubjectConflict(ref e) if e.id == first.id));
        // force 不可绕:状态不变量不是风格偏好。
        let forced = store
            .add(
                "fact",
                "安装通道改为便携版",
                "查安装通道必读",
                "新状态",
                "user",
                &[],
                Some("安装通道"),
                true,
            )
            .unwrap();
        assert!(matches!(forced, AddOutcome::SubjectConflict(ref e) if e.id == first.id));
        // 不同 category 同 subject 不冲突(键含 category)。
        assert!(matches!(
            store
                .add(
                    "sop",
                    "安装通道切换 SOP",
                    "切换安装通道时必读",
                    "步骤",
                    "user",
                    &[],
                    Some("安装通道"),
                    false
                )
                .unwrap(),
            AddOutcome::Added(_)
        ));
        // 旧状态 stale 后,同 subject 可重新建立(active 才占键)。
        store
            .update(&first.id, None, None, None, Some("stale"), None, false)
            .unwrap();
        assert!(matches!(
            store
                .add(
                    "fact",
                    "安装通道:便携版",
                    "查安装通道必读",
                    "新状态",
                    "user",
                    &[],
                    Some("安装通道"),
                    false
                )
                .unwrap(),
            AddOutcome::Added(_)
        ));
        let candidate = store
            .add(
                "sop",
                "安装通道候选状态",
                "安装通道候选验证时必读",
                "候选正文",
                "memory-manager",
                &[],
                Some("候选安装通道"),
                false,
            )
            .unwrap();
        assert!(matches!(candidate, AddOutcome::Added(ref e) if e.status == "candidate"));
        let candidate_duplicate = store
            .add(
                "sop",
                "安装通道候选状态更新",
                "安装通道候选验证时必读",
                "候选正文更新",
                "memory-manager",
                &[],
                Some("候选安装通道"),
                true,
            )
            .unwrap();
        assert!(matches!(
            candidate_duplicate,
            AddOutcome::SubjectConflict(_)
        ));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn add_拒绝空正文条目() {
        // 2026-08-12 清理时库里躺着 3 条只有 frontmatter 的条目(M-039/048/049),
        // 占编号、进 FTS、召回出来什么也没有。写入侧直接拒。
        let (dir, store) = temp_store();
        let err = match store.add(
            "fact",
            "标题在",
            "描述在",
            "   \n  ",
            "user",
            &[],
            None,
            false,
        ) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("空正文必须被拒"),
        };
        assert!(err.contains("body must not be empty"), "{err}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn add_近似标题跨状态跨类目判重() {
        let (dir, store) = temp_store();
        // source != "user" → 落 candidate:未晋升条目隐形正是重复的生产线。
        let first = match store
            .add(
                "sop",
                "defect update 字段键名与多字段处理 SOP 防英文 key 追加与旧内容丢弃的脏数据陷阱",
                "更新字段时必读",
                "正文",
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap()
        {
            AddOutcome::Added(e) => e,
            _ => panic!("首条应写入"),
        };
        assert_eq!(first.status, "candidate");
        // 换个说法、换个 category 再记一遍 —— 旧闸门(标题一字不差 + 同 category)
        // 一条都拦不住,这正是那 8 条重复的由来。
        match store
            .add(
                "fact",
                "defect/req update 字段键名与值处理 SOP 防脏数据",
                "缺陷更新时必读",
                "正文",
                "user",
                &[],
                None,
                false,
            )
            .unwrap()
        {
            AddOutcome::Duplicate(existing) => assert_eq!(existing.id, first.id),
            _ => panic!("近似重复没被拦住"),
        }
        // force 只绕过语义不确定闸，标题判重仍然生效。
        assert!(matches!(
            store
                .add(
                    "fact",
                    "defect/req update 字段键名与值处理 SOP 防脏数据",
                    "缺陷更新时必读",
                    "正文",
                    "user",
                    &[],
                    None,
                    true,
                )
                .unwrap(),
            AddOutcome::Duplicate(_)
        ));
        // 同子系统但确属两条知识的不能误杀(实测包含度 0.32,远低于阈值)。
        assert!(matches!(
            store
                .add(
                    "sop",
                    "活动归档同 ID 语义不同时用 repair_reused_id 修复勿直接编辑托管文档",
                    "完整性门禁报同号时必读",
                    "正文",
                    "user",
                    &[],
                    None,
                    false,
                )
                .unwrap(),
            AddOutcome::Added(_)
        ));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn find_by_marker_看得见candidate且吃老口径标记() {
        let (dir, store) = temp_store();
        store
            .append_note(
                "fixture source",
                "[fp:bash|`git merge` is blocked in bash: git mutations must use the structured `git` tool]",
                "fact",
                &[],
            )
            .unwrap();
        let entry = match store
            .add(
                "fact",
                "bash 里 git mutation 被拦",
                "git 操作被拒时必读",
                "正文\n[fp:bash|`git merge` is blocked in bash: git mutations must use the structured `git` tool]",
                "memory-manager",
                &[],
                None,
                // force:fixture 自造指纹验证 find_by_marker 归一,非真实写入口径
                // (R-216 指纹闸要求指纹先有来源 note;此处测的是 find 而非闸门)。
                true,
            )
            .unwrap()
        {
            AddOutcome::Added(e) => e,
            _ => panic!("应写入"),
        };
        assert_eq!(entry.status, "candidate");
        // 另一个子命令、同一道墙:归一后与正文里的老口径标记等价,
        // 且 candidate 不再隐形——否则 manager 会把同一个坑再记一遍。
        assert_eq!(
            store
                .find_by_marker("[fp:bash|`git restore` is blocked in bash: git mutations must use the structured `git` tool]")
                .map(|e| e.id),
            Some(entry.id.clone()),
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-216 验收②④:伪造指纹的 add 被拒——指纹必须来自 inbox 来源 note 或既有条目。
    #[test]
    fn 自造指纹的add被拒_来源note指纹放行() {
        let (dir, store) = temp_store();
        // 自造指纹(无来源)→ 拒绝。
        let err = store
            .add(
                "fact",
                "伪造指纹条目",
                "钩子",
                "正文 [fp:madeup|something]",
                "user",
                &[],
                None,
                false,
            )
            .unwrap_err();
        assert!(err.to_string().contains("禁止自造"), "{err}");
        // 来源 note 注入同指纹 → 放行。
        store
            .append_note("来源 note", "[fp:madeup|something]", "fact", &[])
            .unwrap();
        let ok = store
            .add(
                "fact",
                "合法指纹条目",
                "钩子",
                "正文 [fp:madeup|something]",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        assert!(matches!(ok, AddOutcome::Added(_)), "来源指纹应放行");
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-216 验收③④:标题命中交付状态形态(R-/D- 编号 + 已交付/勿重复/验收边界)→ 拒,
    /// 指路 tracker。交付状态是 tracker 的事,记忆记「怎么做」不记「哪个交付了」。
    #[test]
    fn 交付状态内容被拒并指路tracker() {
        let (dir, store) = temp_store();
        for bad_title in [
            "R-012 已交付,勿重复",
            "关于 D-044 的验收边界已达成",
            "R-055 delivered, do not repeat",
        ] {
            let err = store
                .add("fact", bad_title, "钩子", "正文", "user", &[], None, false)
                .unwrap_err();
            assert!(
                err.to_string().contains("tracker") && err.to_string().contains("交付"),
                "应指路 tracker: {err}"
            );
        }
        // 正常知识标题(含 R-/D- 引用但不含交付状态词)不受影响。
        let ok = store
            .add(
                "fact",
                "R-012 实现里 bash 前缀匹配的判定",
                "钩子",
                "正文",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        assert!(
            matches!(ok, AddOutcome::Added(_)),
            "含 R- 引用但非交付状态应放行"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-216 验收①④:英文改写 M-044 场景被 add 硬闸拦截并返回候选,指路 memory_update。
    #[test]
    fn 英文改写被add硬闸拦截返回候选() {
        let (dir, store) = temp_store();
        let base = "secure configuring github network proxy connection and verification";
        store
            .add(
                "fact",
                base,
                "HTTPS_PROXY 与代理地址",
                "HTTPS_PROXY=http://127.0.0.1:12000",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        // 英文改写(共享 ≥8 token):Uncertain → add 硬闸拒并返回候选。
        let out = store
            .add(
                "fact",
                "verification and secure connection of github network proxy configuring",
                "HTTPS_PROXY 与代理地址",
                "HTTPS_PROXY=http://127.0.0.1:12000",
                "user",
                &[],
                None,
                false,
            )
            .unwrap();
        match out {
            AddOutcome::Uncertain(candidates) => {
                assert!(
                    candidates.iter().any(|c| c.title == base),
                    "候选应含被改写的基础条目: {:?}",
                    candidates
                );
            }
            other => panic!("英文改写应被 Uncertain 硬闸拦截: {other:?}"),
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 零采纳条目在检索里沉底_高采纳浮上() {
        let (dir, store) = temp_store();
        // 两条在 bm25 上等价的条目(标题仅一字之差,description/body 同构)。
        add(
            &store,
            "fact",
            "发版通道甲",
            "发版发布安装更新必读",
            "正文等长条目一",
        );
        add(
            &store,
            "fact",
            "发版通道乙",
            "发版发布安装更新必读",
            "正文等长条目二",
        );
        // D-366:排序决策在检索门面,经 index 取决策排序结果。
        let index = SqliteMemoryIndex::new(&dir);
        let hits = index.search_entries(&IndexQuery::text("发版"), None, Some("active"), 5);
        assert_eq!(hits.len(), 2);
        let (a, b) = ("M-001".to_string(), "M-002".to_string());
        // 各召回 3 轮:甲从未被拉正文,乙每轮都被拉。recall_id 以毫秒为键,轮间隔 2ms。
        for _ in 0..3 {
            record_current_recall(&store, &hits, &[&b]);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let profile = store.recall_profile();
        assert_eq!(profile.get(&a), Some(&(3, 0)), "{profile:?}");
        assert_eq!(profile.get(&b), Some(&(3, 3)), "{profile:?}");
        // 乙(×1.3)必须压过甲(×0.6),无论 bm25 平局时的原始顺序。
        let ranked = index.search_entries(&IndexQuery::text("发版"), None, Some("active"), 5);
        assert_eq!(
            ranked[0].entry.id,
            b,
            "{:?}",
            ranked.iter().map(|h| &h.entry.id).collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn merge_自动搬运被并条目的指纹与refs() {
        // D-215:合并不许静默丢掉复发检测键与来源链——这不能赌 manager 记得带。
        let (dir, store) = temp_store();
        // R-216:指纹来源——先注入 inbox 来源 note,再 add 携带同指纹(测 merge 兜底)。
        store
            .append_note("edit 未命中来源", "[fp:edit|not found]", "fact", &[])
            .unwrap();
        let a = add(&store, "fact", "edit 未命中主因", "edit 失败必读", "判据 A");
        let b = match store
            .add(
                "fact",
                "edit 未命中另一坑",
                "edit 失败必读 2",
                "判据 B [fp:edit|not found]",
                "user",
                &["R-001".into()],
                None,
                false,
            )
            .unwrap()
        {
            AddOutcome::Added(e) => e,
            _ => panic!("expected add"),
        };
        let merged = store
            .merge(
                &a.id,
                std::slice::from_ref(&b.id),
                None,
                None,
                Some("合并后的正文(manager 忘了带指纹)"),
                true, // 测指纹兜底而非闸门:confirmed=true 放行
            )
            .unwrap();
        // 指纹被引擎兜底并入 primary 正文,复发检测继续可用。
        assert!(
            merged.body.contains("[fp:edit|not found]"),
            "{}",
            merged.body
        );
        assert_eq!(
            store.find_by_marker("[fp:edit|not found]").map(|e| e.id),
            Some(a.id.clone()),
        );
        // refs 并集进 primary。
        assert_eq!(merged.refs(), vec!["R-001".to_string()]);
        // R-165 批3:墓碑语义不变,但条目归档到 archive/。
        assert!(store.load_all().iter().all(|(_, e)| e.id != b.id));
        let dup_path = std::fs::read_dir(store.archive_dir())
            .unwrap()
            .flatten()
            .find(|p| {
                p.path()
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(&b.id))
                    .unwrap_or(false)
            })
            .expect("archive 墓碑应存在")
            .path();
        let dup_text = std::fs::read_to_string(dup_path).unwrap();
        assert!(dup_text.contains("status: deprecated"), "{dup_text}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn preference_豁免采纳率降权() {
        // preference 正文全文常驻,永远不需要拉正文——采纳率对它结构性无意义,
        // 同样的「召回 3 采纳 0」不得让定调条目在检索里被降权。
        let (dir, store) = temp_store();
        add(
            &store,
            "preference",
            "发版定调甲",
            "发版发布安装更新必读",
            "正文等长条目一",
        );
        add(
            &store,
            "fact",
            "发版通道乙",
            "发版发布安装更新必读",
            "正文等长条目二",
        );
        // D-366:排序决策在检索门面,经 index 取决策排序结果。
        let index = SqliteMemoryIndex::new(&dir);
        let hits = index.search_entries(&IndexQuery::text("发版"), None, Some("active"), 5);
        assert_eq!(hits.len(), 2);
        for _ in 0..3 {
            record_current_recall(&store, &hits, &[]);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // 两条同为召回 3/采纳 0:fact 吃 ×0.6,preference 保持 ×1.0 → 严格高分在前。
        let ranked = index.search_entries(&IndexQuery::text("发版"), None, Some("active"), 5);
        assert_eq!(
            ranked[0].entry.category,
            "preference",
            "{:?}",
            ranked
                .iter()
                .map(|h| (&h.entry.id, h.score))
                .collect::<Vec<_>>()
        );
        assert!(
            ranked[0].score > ranked[1].score,
            "豁免缺失时两条同权重打平,必须是严格大于: {:?}",
            ranked
                .iter()
                .map(|h| (&h.entry.id, h.score))
                .collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-282 ①:description 主题一致性。新 description 与条目现有主题无任何共同词
    /// (title/正文)时拒绝——防 manager 把 tracker 字段语义条目的钩子换成 edit 主题
    /// 内容(实测 M-044 被覆盖)。同主题演化放行。
    #[test]
    fn update拒绝主题漂移的description() {
        let (dir, store) = temp_store();
        let e = add(
            &store,
            "sop",
            "tracker update 字段语义",
            "处理 req/defect update 写字段时必读",
            "中文键精确匹配;英文键会追加;多行会产生游离段落",
        );
        // 主题漂移:edit 替换主题与 tracker 字段语义交集 <2 → 拒绝(D-282 真实形态)。
        let drifted = store
            .update(
                &e.id,
                None,
                Some("edit 替换时确认 allow_deletion 防止误删"),
                None,
                None,
                None,
                true,
            )
            .unwrap_err();
        assert!(
            drifted.to_string().contains("共同词过少"),
            "漂移 description 必须被拒: {drifted}"
        );
        // 条目内容未被改写:description 仍是旧钩子(漂移被拒后原样保留)。
        let after = store
            .load_all()
            .into_iter()
            .find(|(_, x)| x.id == e.id)
            .unwrap()
            .1;
        assert_eq!(
            after.description, "处理 req/defect update 写字段时必读",
            "漂移被拒后 description 不得被改写: {}",
            after.description
        );
        // 同主题演化(保留旧主题词)放行。
        store
            .update(
                &e.id,
                None,
                Some("处理 req/defect update 写字段时必读(含中文键/英文键/游离段落)"),
                None,
                None,
                None,
                true,
            )
            .unwrap();
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-341 验收②:轮末自动处置机制测试——满足条件的 candidate 自动 promote,
    /// 超期未处置自动 deprecated,未满足条件的不动;存量前后计数(文件+索引)可审计。
    #[test]
    fn reconcile_candidates_auto_promote_deprecate_and_keep() {
        let (dir, store) = temp_store();
        // ① 可晋升 candidate:带指纹 + 复发≥3 + 真实 episode(provenance 硬约束)。
        let fp = format!("[fp:edit|reconcile #{}-{}]", std::process::id(), "r");
        store
            .append_note("fixture source", &fp, "sop", &[])
            .unwrap();
        for _ in 0..3 {
            store.bump_recurrence(&fp);
        }
        let AddOutcome::Added(promotable) = store
            .add(
                "sop",
                "reconcile 可晋升",
                "自动晋升钩子",
                &format!("正文 {fp}"),
                "memory-manager",
                &[],
                None,
                true,
            )
            .unwrap()
        else {
            panic!("expected Added")
        };
        assert_eq!(promotable.status, "candidate");
        // ② 超期 candidate:手写 30 天前的文件(updated 远早于 max_age_days)。
        let old_name = "M-990-超期候选.md";
        std::fs::write(
            store.root.join(old_name),
            "---\nid: M-990\nscope: project\ncategory: sop\ntitle: reconcile 超期\ndescription: 超期钩子\nstatus: candidate\ncreated: 2026-07-01\nupdated: 2026-07-01\nsource: memory-manager\n---\n\n正文",
        )
        .unwrap();
        // ③ 未达标 candidate:无指纹、updated 今天 → 不动。
        let AddOutcome::Added(keep) = store
            .add(
                "sop",
                "reconcile 未达标",
                "未达标钩子",
                "正文无指纹",
                "memory-manager",
                &[],
                None,
                true,
            )
            .unwrap()
        else {
            panic!("expected Added")
        };
        assert_eq!(keep.status, "candidate");
        // 手写文件进 FTS,让索引计数与文件计数一致(存量 before 可审计)。
        store.refresh_derived().unwrap();
        let eid = crate::memory::seed_episode(&dir, "ses");
        let report = store.reconcile_candidates(Some(eid), 14).unwrap();
        let promotable_id = promotable.id.clone();
        let keep_id = keep.id.clone();
        assert_eq!(
            report.promoted,
            vec![promotable_id.clone()],
            "复发≥3 + 真实 episode 应自动晋升"
        );
        assert_eq!(report.deprecated, vec!["M-990"], "超期未处置应自动清退");
        assert_eq!(
            report.untouched,
            vec![keep_id.clone()],
            "未达标应保持 candidate"
        );
        assert_eq!(report.candidate_files_before, 3, "存量 3 条 candidate 文件");
        assert_eq!(
            report.candidate_files_after, 1,
            "promote + deprecated 后只剩未达标 1 条"
        );
        assert_eq!(report.candidate_index_before, 3, "索引与文件一致(before)");
        assert_eq!(report.candidate_index_after, 1, "索引与文件一致(after)");
        // 状态落地:promote → active(主目录文件仍在);deprecated → 归档(主目录无文件)。
        let (_, p) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == promotable_id)
            .unwrap();
        assert_eq!(p.status, "active", "晋升条目必须落 active");
        assert!(!store.root.join(old_name).exists(), "超期文件应移出主目录");
        assert!(
            store.load_archived_ids().contains(&"M-990".to_string()),
            "归档 ID 保留防复用"
        );
        // 不满足条件的不动:keep 仍是 candidate、文件仍在主目录(未验证不注入边界不变)。
        let (_, k) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == keep_id)
            .unwrap();
        assert_eq!(k.status, "candidate", "未达标条目必须保持 candidate");
        assert!(
            store.root.join(format!("{}.md", keep.file_stem())).exists(),
            "未达标条目文件必须留在主目录"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// R-295：候选超过健康水位时低价值优先清退，且清退走归档而非裸删。
    #[test]
    fn reconcile_candidates_capacity_retires_low_value_first() {
        let (dir, store) = temp_store();
        for index in 0..crate::memory::CANDIDATE_MAX_COUNT {
            let fingerprint = format!("[fp:capacity-{}-{}]", std::process::id(), index);
            store
                .append_note("capacity fixture source", &fingerprint, "fact", &[])
                .unwrap();
            store.bump_recurrence(&fingerprint);
            let AddOutcome::Added(entry) = store
                .add(
                    "fact",
                    &format!("capacity 高价值 {index}"),
                    &format!("capacity 高价值钩子 {index}"),
                    &format!("正文 {fingerprint}"),
                    "memory-manager",
                    &[],
                    None,
                    true,
                )
                .unwrap()
            else {
                panic!("expected high-value candidate")
            };
            assert_eq!(entry.status, "candidate");
        }
        let mut low_value_ids = Vec::new();
        for index in 0..6 {
            let AddOutcome::Added(entry) = store
                .add(
                    "fact",
                    &format!("capacity 低价值 {index}"),
                    &format!("capacity 低价值钩子 {index}"),
                    "正文无 fingerprint 且无 recurrence",
                    "memory-manager",
                    &[],
                    None,
                    true,
                )
                .unwrap()
            else {
                panic!("expected low-value candidate")
            };
            low_value_ids.push(entry.id);
        }
        store.refresh_derived().unwrap();
        let report = store.reconcile_candidates(None, 365).unwrap();
        assert_eq!(report.candidate_files_before, 30);
        assert_eq!(report.candidate_index_before, 30);
        assert_eq!(
            report.candidate_files_after,
            crate::memory::CANDIDATE_MAX_COUNT
        );
        assert_eq!(
            report.candidate_index_after,
            crate::memory::CANDIDATE_MAX_COUNT
        );
        assert_eq!(report.deprecated, low_value_ids);
        assert_eq!(
            report.untouched.len(),
            crate::memory::CANDIDATE_MAX_COUNT,
            "untouched 只保留最终仍为 candidate 的条目(容量出口清退的不再算 untouched)"
        );
        for id in low_value_ids {
            assert!(
                store.has_archived_id(&id),
                "低价值 candidate 必须保留归档墓碑: {id}"
            );
        }
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-282 ②:CAS——传 expected_hash 且期间有并发写时拒绝,防止人工维护与
    /// 轮末 manager 互相覆盖。
    #[test]
    fn update_cas拒绝过期expected_hash() {
        let (dir, store) = temp_store();
        let e = add(&store, "fact", "主题甲", "钩子甲", "正文甲");
        let render = |id: &str| {
            let entry = store
                .load_all()
                .into_iter()
                .find(|(_, x)| x.id == id)
                .unwrap()
                .1;
            render_entry(&entry)
        };
        let stale_hash = kanzei_base::content_hash(render(&e.id).as_bytes());
        // 期间别人改了条目(改 title)——模拟非 manager 写,豁免主题校验。
        store
            .update(&e.id, Some("主题甲改"), None, None, None, None, false)
            .unwrap();
        // 拿着旧 hash 再写 → 拒绝。
        let cas_err = store
            .update(
                &e.id,
                None,
                Some("钩子甲修订"),
                None,
                None,
                Some(&stale_hash),
                true,
            )
            .unwrap_err();
        assert!(
            cas_err.to_string().contains("已被并发修改"),
            "过期 hash 必须拒绝: {cas_err}"
        );
        // 用新 hash 写 → 放行。
        let fresh_hash = kanzei_base::content_hash(render(&e.id).as_bytes());
        store
            .update(
                &e.id,
                None,
                Some("钩子甲修订"),
                None,
                None,
                Some(&fresh_hash),
                true,
            )
            .unwrap();
        std::fs::remove_dir_all(dir).ok();
    }
}
