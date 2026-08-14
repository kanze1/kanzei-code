//! MemoryStore:单 scope 的记忆仓库。
//! 硬门禁在写入侧:ID 引擎分配、枚举校验、description 必填、精确重复拒绝;
//! INDEX.md 与 index.db(FTS5/hits)都是派生物,损坏可由文件全量重建;
//! 写入 tmp+rename 原子替换,不做跨进程锁(用户定调:竞争冲突留给 agent 事后解决)。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use super::{
    date_days, parse_entry, render_entry, today, MemoryEntry, MemoryScope, CATEGORIES, STATUSES,
};

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

fn now_ms() -> i64 {
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
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        MemoryStore {
            scope: self.scope,
            root: self.root.clone(),
        }
    }
}

impl MemoryStore {
    pub fn open(scope: MemoryScope, root: PathBuf) -> Self {
        MemoryStore { scope, root }
    }

    pub fn project(project_root: &Path) -> Self {
        let store = MemoryStore::open(
            MemoryScope::Project,
            super::project_memory_root(project_root),
        );
        store.migrate_legacy(project_root);
        store
    }

    pub fn global() -> Option<Self> {
        Some(MemoryStore::open(
            MemoryScope::Global,
            super::global_memory_root()?,
        ))
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

    fn load_archived_ids(&self) -> Vec<String> {
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

    /// ID 分配扫活跃+归档,编号绝不复用(同 tracker 哲学)。
    pub fn next_id(&self, entries: &[(PathBuf, MemoryEntry)]) -> String {
        let prefix = self.scope.prefix();
        let parse = |id: &str| {
            id.strip_prefix(prefix)?
                .strip_prefix('-')?
                .parse::<u32>()
                .ok()
        };
        let max = entries
            .iter()
            .filter_map(|(_, e)| parse(&e.id))
            .chain(self.load_archived_ids().iter().filter_map(|id| parse(id)))
            .max()
            .unwrap_or(0);
        format!("{}-{:03}", prefix, max + 1)
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
        if !CATEGORIES.contains(&category) {
            anyhow::bail!(
                "invalid category `{category}`; valid: {}",
                CATEGORIES.join(" | ")
            );
        }
        let title = title.trim();
        let description = description.trim();
        if title.is_empty() {
            anyhow::bail!("title must not be empty");
        }
        if description.is_empty() {
            anyhow::bail!("description must not be empty — it is the retrieval hook");
        }
        // 空正文条目是纯噪声:占编号、进 FTS、召回出来啥也没有。2026-08-12 清理时
        // 库里躺着 3 条只有 frontmatter 的条目(M-039/M-048/M-049),都是 manager
        // 在权限受限的自动轮里批量写记忆时产的。写入侧直接拒。
        if body.trim().is_empty() {
            anyhow::bail!(
                "body must not be empty — an entry with only frontmatter is unusable: \
                 put the actual finding (what happened, what to do instead) in the body"
            );
        }
        // 状态不变量先于标题去重与 R-216 语义闸,且不受 force 影响:状态就地覆盖,
        // 绝不并存。同 scope+category+subject 至多一条 active——同 subject 的 add
        // 必须先报 SubjectConflict,不能先被语义闸拦成 Uncertain(测试锚点)。
        let subject = subject.map(str::trim).filter(|s| !s.is_empty());
        let entries = self.load_all();
        if let Some(subject) = subject {
            if let Some((_, existing)) = entries.iter().find(|(_, e)| {
                e.status == "active"
                    && e.category == category
                    && e.extras.iter().any(|(k, v)| k == "subject" && v == subject)
            }) {
                return Ok(AddOutcome::SubjectConflict(existing.clone()));
            }
        }
        // R-216 三闸:记忆写入侧质量闸门,全部为硬拒。force=true = 显式跳过
        // (用户/调用方声明「这是新知识,不查重不查指纹」),与既有 duplicate
        // 去重的 force 语义一致。
        // ① 交付状态拒收:标题/subject 命中「R-/D- 编号 + 已交付/勿重复/验收边界」
        //    形态时,这是 tracker 的状态,不是记忆——拒绝并指路 tracker。
        let title_lc = title.to_lowercase();
        let subject_lc = subject.map(str::to_lowercase).unwrap_or_default();
        let is_delivery_state = ["已交付", "勿重复", "验收边界", "delivered", "do not repeat"]
            .iter()
            .any(|kw| {
                title_lc.contains(&kw.to_lowercase()) || subject_lc.contains(&kw.to_lowercase())
            });
        if !force && is_delivery_state && has_tracker_id(title) {
            anyhow::bail!(
                "标题/subject 命中交付状态形态(R-/D- 编号 + 已交付/勿重复/验收边界)——\
                 这是 tracker 条目的状态,不是记忆。\
                 记忆记「怎么做」的约束,不记「哪个条目交付了」;交付状态请写在 requirements/defects 里,refs 引用即可。"
            );
        }
        // ② 指纹一致性:新条目携带 [fp:] 必须与来源 note 中引擎生成的指纹逐字一致。
        //    拒绝自造指纹(实证:M-055/M-056 编造 [fp:...] 冒充引擎生成)。
        let fp_markers = super::fp_markers(body);
        if !force && !fp_markers.is_empty() {
            let inbox = self.read_inbox();
            let existing_fps = {
                let mut fps = Vec::new();
                for (_, e) in self.load_all() {
                    fps.extend(super::fp_markers(&e.body));
                }
                if let Some(global) = MemoryStore::global() {
                    for (_, e) in global.load_all() {
                        fps.extend(super::fp_markers(&e.body));
                    }
                }
                fps
            };
            for fp in &fp_markers {
                let legit = inbox.contains(fp.as_str()) || existing_fps.contains(fp);
                if !legit {
                    anyhow::bail!(
                        "body 携带指纹 {fp} 但该指纹不存在于 inbox 来源 note 或任何既有条目——\
                         指纹是引擎从失败信号生成的,禁止自造(实证 M-055/M-056)。\
                         去掉自造指纹,或先用 memory_note 记录真实来源。"
                    );
                }
            }
        }
        // ③ 语义探测下沉:Uncertain(有 FTS 命中但非精确)即拒并返回候选。
        if !force {
            let (novelty, candidates) = self.classify_novelty(title, description, body);
            if novelty == Novelty::Uncertain {
                let cand: Vec<MemoryEntry> = candidates.into_iter().collect();
                if !cand.is_empty() {
                    return Ok(AddOutcome::Uncertain(cand));
                }
            }
        }
        if !force {
            let normalized = normalize_title(title);
            if let Some((_, existing)) = entries.iter().find(|(_, e)| {
                (e.status == "active" || e.status == "candidate")
                    && e.category == category
                    && normalize_title(&e.title) == normalized
            }) {
                return Ok(AddOutcome::Duplicate(existing.clone()));
            }
            // 近似去重(2026-08-12):标题一字不差才算重复太弱了——同一个坑换个
            // 说法、换个 category 就能再落一条。实测一个「tracker update 字段语义」
            // 的坑堆出 8 条 sop、一个「bash 里 git mutation 被拒」堆出 5 条(fact
            // 与 sop 混着),标题两两都不相同,旧闸门一条都没拦住。
            // 判据:标题切词后的包含度(交集 / 较短一侧),跨 category 也查。
            if let Some(existing) = entries
                .iter()
                .map(|(_, e)| e)
                .filter(|e| e.status == "active" || e.status == "candidate")
                .filter(|e| title_containment(title, &e.title) >= TITLE_DUP_THRESHOLD)
                .max_by(|a, b| {
                    title_containment(title, &a.title)
                        .partial_cmp(&title_containment(title, &b.title))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                return Ok(AddOutcome::Duplicate(existing.clone()));
            }
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
        Ok(entry)
    }

    /// candidate → shadow(R-166):进入评估期,可被离线回放评估但不注入生产检索。
    /// 与 promote 不同,to_shadow 不需要 provenance——评估本身就是验证,
    /// 评估通过后 promote(带证据)才进 active。状态机:只有 candidate 可进 shadow。
    pub fn to_shadow(&self, id: &str) -> anyhow::Result<MemoryEntry> {
        let entries = self.load_all();
        let Some((path, mut entry)) = entries.into_iter().find(|(_, e)| e.id == id) else {
            anyhow::bail!("unknown memory id `{id}`");
        };
        if entry.status != "candidate" {
            anyhow::bail!(
                "cannot to_shadow `{id}`: status is `{}`, only candidate can enter shadow",
                entry.status
            );
        }
        entry.status = "shadow".into();
        entry.updated = today();
        self.write_entry(&entry, Some(&path))?;
        self.refresh_derived()?;
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
        if entry.status != "candidate" && entry.status != "shadow" {
            anyhow::bail!(
                "cannot promote `{id}`: status is `{}`, only candidate|shadow can be promoted",
                entry.status
            );
        }
        // 证据落 state.db memory_sources 表(与 episodes 同库,可 join)。
        // 仅 project scope 有 state.db(global 记忆无 episode 证据源)。
        let hash = source_hash.unwrap_or("compiler").to_string();
        let store = if self.scope == MemoryScope::Project {
            let db_path = self.root.join("..").join("state.db");
            match kanzei_core::SessionStore::open(&db_path) {
                Ok(store) => Some(store),
                Err(e) => anyhow::bail!(
                    "cannot promote `{id}`: cannot open state.db for provenance check ({e}) — \
                     证据落库失败,拒绝晋升"
                ),
            }
        } else {
            None
        };
        // R-213:promote 前校验每个 episode_id 真实存在——「无来源不入 active」必须是
        // 「来源指向真实轮次」,而不是只看数组非空,否则 manager 编造 id 也能蒙混过关。
        if let Some(store) = &store {
            for (episode_id, _, _) in sources {
                if !store.episode_exists(*episode_id)? {
                    anyhow::bail!(
                        "cannot promote `{id}`: episode_id {episode_id} does not exist in \
                         state.db episodes — provenance requires real episodes, not fabricated ids"
                    );
                }
            }
        }
        // R-213:证据先落库、全部成功才置 active——写证据失败不产生 active 条目,
        // 也不留下「active 却无证据」的半成品(回滚由顺序天然保证,无需手动回滚)。
        if let Some(store) = &store {
            for (episode_id, event_start, event_end) in sources {
                if let Err(e) =
                    store.record_memory_source(id, *episode_id, *event_start, *event_end, &hash)
                {
                    anyhow::bail!(
                        "cannot promote `{id}`: failed to record memory_source evidence \
                         (episode {episode_id}): {e} — promotion aborted, entry stays {}",
                        entry.status
                    );
                }
            }
        }
        entry.status = "active".into();
        entry.updated = today();
        self.write_entry(&entry, Some(&path))?;
        self.refresh_derived()?;
        Ok(entry)
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
        let age_limit = max_age_days.max(1);
        for (path, entry) in before {
            if entry.status != "candidate" {
                continue;
            }
            let recurrence = entry
                .fingerprint()
                .as_deref()
                .map(|fingerprint| self.recurrence_count(fingerprint))
                .unwrap_or(0);
            if current_episode_id.is_some_and(|episode_id| {
                recurrence >= 3
                    && entry.fingerprint().is_some()
                    && self
                        .promote(
                            &entry.id,
                            &[(episode_id, None, None)],
                            Some("candidate-reconcile"),
                        )
                        .is_ok()
            }) {
                report.promoted.push(entry.id);
                continue;
            }
            let age = today_days
                .zip(date_days(&entry.updated))
                .map(|(now, updated)| now.saturating_sub(updated));
            if age.is_some_and(|days| days >= age_limit) {
                let reason = format!(
                    "(auto-deprecated: candidate 超过 {age_limit} 个日历日未完成晋升，\
                     无满足条件的 recurrence/provenance；原路径 {})",
                    path.display()
                );
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
        let after = self.load_all();
        report.candidate_files_after = after
            .iter()
            .filter(|(_, entry)| entry.status == "candidate")
            .count();
        report.candidate_index_after = self.candidate_index_count();
        Ok(report)
    }

    fn candidate_index_count(&self) -> usize {
        let Ok(conn) = self.open_db() else { return 0 };
        conn.query_row(
            "SELECT COUNT(*) FROM memory_fts WHERE status = 'candidate'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count.max(0) as usize)
        .unwrap_or(0)
    }

    fn write_entry(&self, entry: &MemoryEntry, existing_path: Option<&Path>) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let path = match existing_path {
            Some(p) => p.to_path_buf(),
            None => self.root.join(format!("{}.md", entry.file_stem())),
        };
        crate::atomic_file::write_atomic(&path, &render_entry(entry))?;
        Ok(())
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
                let _ = std::fs::remove_file(path);
            } else if std::fs::rename(path, &dest).is_ok() {
                archived += 1;
            }
        }
        archived
    }

    /// 重建全部派生物:INDEX.md 与 FTS 索引。任何写操作后调用;损坏时可手动全量重建。
    /// R-165 批3:先归档失效条目,再以归档后的集合重建(主目录只含 active/candidate)。
    pub fn refresh_derived(&self) -> anyhow::Result<()> {
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
        crate::atomic_file::write_atomic(&self.index_md(), &index)?;

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
        Ok(())
    }

    /// FTS 派生物与文件真源的 id 集合比对(只看目录文件名,不读内容——检索热路径
    /// 上的守护必须廉价)。任何差集都判失步;查询失败按未失步处理(刚建库的空表
    /// 走正常路径,不在这里制造额外故障面)。
    fn fts_desynced(&self, conn: &Connection) -> bool {
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

    fn open_db(&self) -> anyhow::Result<Connection> {
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

    /// 落一次召回明细,返回 recall_id。注入字节数由调用方给(它才知道最终注入了多长)。
    pub fn record_recall(&self, prompt: &str, hits: &[SearchHit], injected_bytes: usize) -> String {
        let at = now_ms();
        let recall_id = format!("{at}-{}", self.scope.prefix());
        let Ok(conn) = self.open_db() else {
            return recall_id;
        };
        let head: String = prompt.chars().take(160).collect();
        for hit in hits {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO memory_recalls
                 (recall_id, at, prompt_head, injected_bytes, entry_id, title, scope, category, score, snippet, fetched)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                     COALESCE((SELECT fetched FROM memory_recalls WHERE recall_id = ?1 AND entry_id = ?5), 0))",
                params![
                    recall_id,
                    at,
                    head,
                    injected_bytes as i64,
                    hit.entry.id,
                    hit.entry.title,
                    hit.entry.scope,
                    hit.entry.category,
                    hit.score,
                    hit.snippet,
                ],
            );
        }
        recall_id
    }

    /// 标记某条记忆的正文在召回之后确实被拉取过 = 这次召回起了作用。
    /// 只回填最近一次召回:更早的那次已经有自己的结论,不能被后来的行为追认。
    pub fn mark_recall_fetched(&self, entry_id: &str) {
        let Ok(conn) = self.open_db() else { return };
        let _ = conn.execute(
            "UPDATE memory_recalls SET fetched = 1
             WHERE entry_id = ?1 AND recall_id = (
                 SELECT recall_id FROM memory_recalls WHERE entry_id = ?1 ORDER BY at DESC LIMIT 1)",
            params![entry_id],
        );
    }

    /// 最近若干次召回,按轮次聚合(新的在前)。
    pub fn recalls(&self, limit: usize) -> Vec<RecallRound> {
        let Ok(conn) = self.open_db() else {
            return Vec::new();
        };
        let Ok(mut statement) = conn.prepare(
            "SELECT recall_id, at, prompt_head, injected_bytes, entry_id, title, scope, category, score, snippet, fetched
             FROM memory_recalls ORDER BY at DESC, entry_id ASC",
        ) else {
            return Vec::new();
        };
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                RecallHit {
                    id: row.get(4)?,
                    title: row.get(5)?,
                    scope: row.get(6)?,
                    category: row.get(7)?,
                    score: row.get(8)?,
                    snippet: row.get(9)?,
                    fetched: row.get::<_, i64>(10)? != 0,
                },
            ))
        });
        let Ok(rows) = rows else { return Vec::new() };
        let mut rounds: Vec<RecallRound> = Vec::new();
        for row in rows.flatten() {
            let (recall_id, at, prompt_head, injected, hit) = row;
            match rounds.iter_mut().find(|r| r.recall_id == recall_id) {
                Some(round) => round.hits.push(hit),
                None => {
                    if rounds.len() >= limit {
                        break;
                    }
                    rounds.push(RecallRound {
                        recall_id,
                        at,
                        prompt_head,
                        injected_bytes: injected as usize,
                        hits: vec![hit],
                    });
                }
            }
        }
        rounds
    }

    /// FTS 候选集访问(存储侧):bm25 取候选 + status/shadow 过滤,不做任何
    /// 排序/加权决策——ranking 只属于检索门面(SqliteMemoryIndex,D-366)。
    /// 一致性守护保留在热路径:FTS 是派生物,失步自动重建(实证:2026-08-13
    /// 清理事故后 M-058~062 对 BM25 完全不可见;id 集合比对只读目录名)。
    /// 返回按 bm25 升序(FTS5 负值,越小越相关)的完整候选,由检索侧截断。
    pub fn search_candidates(
        &self,
        query: &str,
        category: Option<&str>,
        status: Option<&str>,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let match_expr = fts_query(query);
        if match_expr.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.open_db()?;
        if self.fts_desynced(&conn) {
            drop(conn);
            self.refresh_derived()?;
            conn = self.open_db()?;
        }
        let mut sql = String::from(
            "SELECT id, snippet(memory_fts, -1, '[', ']', '…', 12), bm25(memory_fts)
             FROM memory_fts WHERE memory_fts MATCH ?1",
        );
        if category.is_some() {
            sql.push_str(" AND category = ?2");
        }
        sql.push_str(" ORDER BY bm25(memory_fts) LIMIT 24");
        let mut statement = conn.prepare(&sql)?;
        let rows: Vec<(String, String, f64)> = match category {
            Some(cat) => statement
                .query_map(params![match_expr, cat], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<Result<_, _>>()?,
            None => statement
                .query_map(params![match_expr], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<Result<_, _>>()?,
        };
        let entries = self.load_all();
        let mut out: Vec<SearchCandidate> = Vec::new();
        for (id, snippet, bm25) in rows {
            let Some((path, entry)) = entries.iter().find(|(_, e)| e.id == id) else {
                continue; // 索引落后于文件:integrity_issues 会报,检索先跳过。
            };
            if let Some(want) = status {
                if entry.status != want {
                    continue;
                }
            }
            // R-166:shadow 条目不注入生产检索——默认/其他 status 查询一律跳过,
            // 只有显式查 shadow 才可见(评估器用)。与 0.5 降权不同,这是硬排除。
            if entry.status == "shadow" && status != Some("shadow") {
                continue;
            }
            let hit_count: u64 = conn
                .query_row(
                    "SELECT hits FROM memory_hits WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            out.push(SearchCandidate {
                entry: entry.clone(),
                path: path.clone(),
                snippet: unsegment_cjk(&snippet),
                hits: hit_count,
                bm25,
            });
        }
        Ok(out)
    }

    /// 检索命中追踪(观测,R-125):对最终命中的条目 hits+1。
    /// hits 不参与排序(R-150 自增强退役),只作效果画像;由检索门面在
    /// 决策排序完成后调用(纯探测不记,避免污染观测)。
    pub fn record_hits(&self, ids: &[String]) {
        let Ok(conn) = self.open_db() else { return };
        let now = now_ms();
        for id in ids {
            let _ = conn.execute(
                "INSERT INTO memory_hits(id, hits, last_hit_at) VALUES (?1, 1, ?2)
                 ON CONFLICT(id) DO UPDATE SET hits = hits + 1, last_hit_at = ?2",
                params![id, now],
            );
        }
    }

    /// R-165 批2 novelty gate 三档:明显新 → PROPOSE、明显重复 → NOOP、
    /// 不确定 → 才起 LLM 判断(验收④)。
    /// 机械判据:标题规范化精确命中既有 active 记忆 = 明显重复;
    /// FTS 无任何命中 = 明显新;有命中但非精确 = 不确定。
    /// R-165 批2 novelty gate:R-216 下沉为 add 硬闸,并扩到双 scope 探测
    /// (project + global 都查 active)。返回 (判定, 候选)。语义探测 = description+body
    /// 的 FTS 命中:有命中即 Uncertain(候选返回),无命中 New;精确标题重复 Duplicate。
    pub fn classify_novelty(
        &self,
        title: &str,
        description: &str,
        body: &str,
    ) -> (Novelty, Vec<MemoryEntry>) {
        let normalized = normalize_title(title);
        let entries = self.load_all();
        let dup = entries
            .iter()
            .any(|(_, e)| e.status == "active" && normalize_title(&e.title) == normalized);
        if dup {
            return (Novelty::Duplicate, Vec::new());
        }
        // 用描述+正文做 FTS 探测:有明显语义命中即不确定,否则新。
        let probe = format!("{} {}", description, body);
        let mut candidates = Vec::new();
        // 双 scope:project 的 add 要同时看 global 的 active,避免英文改写 M-044 类
        // 穿透(project 里没有原条目,global 里有)。global 的 add 只看自身。
        let mut scopes: Vec<MemoryStore> = Vec::new();
        scopes.push(self.clone());
        if self.scope == MemoryScope::Project {
            if let Some(global) = MemoryStore::global() {
                scopes.push(global);
            }
        }
        for scope in &scopes {
            // D-366:novelty 探测用存储侧候选集(不排序),取 bm25 序 top3——无召回
            // 历史时 decision_weight=1.0,与旧 search 的决策排序 top3 完全一致。
            if let Ok(cands) = scope.search_candidates(&probe, None, Some("active")) {
                for hit in cands.into_iter().take(3) {
                    // R-216:只有「与新增标题语义高度重合」的候选才算 Uncertain——
                    // FTS 命中但标题包含度低于去重阈值(0.55)的是相关但不同的新知识,
                    // 不能拦(否则合法新增被误伤)。英文改写 M-044 类:改写标题与原标题
                    // 切词后包含度高,仍会被拦(验收①)。
                    let containment = title_containment(title, &hit.entry.title);
                    if containment >= TITLE_DUP_THRESHOLD
                        && !candidates
                            .iter()
                            .any(|c: &MemoryEntry| c.id == hit.entry.id)
                    {
                        candidates.push(hit.entry);
                    }
                }
            }
        }
        if !candidates.is_empty() {
            (Novelty::Uncertain, candidates)
        } else {
            (Novelty::New, Vec::new())
        }
    }

    /// 落一条 novelty 三档分流计数遥测(验收④)。
    pub fn record_novelty(&self, verdict: &Novelty, fingerprint: &str, note_head: &str) {
        let Ok(conn) = self.open_db() else { return };
        let _ = conn.execute(
            "INSERT INTO novelty_events(at, verdict, fingerprint, note_head) VALUES (?1, ?2, ?3, ?4)",
            params![now_ms(), verdict.as_str(), fingerprint, note_head.chars().take(80).collect::<String>()],
        );
    }

    /// R-165 批2 recurrence 三段晋升的跨轮计数(验收②):
    /// 同指纹跨轮复发计数,第 2 次才 candidate、第 3 次且带修复成功才 promote。
    /// 返回第几次出现(1-based)。持久化在 index.db,manager 消化失败笔记时查。
    pub fn bump_recurrence(&self, fingerprint: &str) -> u32 {
        let Ok(conn) = self.open_db() else { return 1 };
        let now = now_ms();
        let _ = conn.execute(
            "INSERT INTO recurrence_counts(fingerprint, count, last_at) VALUES (?1, 1, ?2)
             ON CONFLICT(fingerprint) DO UPDATE SET count = count + 1, last_at = ?2",
            params![fingerprint, now],
        );
        conn.query_row(
            "SELECT count FROM recurrence_counts WHERE fingerprint = ?1",
            params![fingerprint],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as u32)
        .unwrap_or(1)
    }

    /// 查询某指纹当前的复发次数(只读,不递增)。
    pub fn recurrence_count(&self, fingerprint: &str) -> u32 {
        let Ok(conn) = self.open_db() else { return 0 };
        conn.query_row(
            "SELECT count FROM recurrence_counts WHERE fingerprint = ?1",
            params![fingerprint],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as u32)
        .unwrap_or(0)
    }

    /// 完整性检测(D-112 同款哲学):ID 重复、缺号、解析失败的文件都要可见。
    pub fn integrity_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let entries = self.load_all();
        let mut seen = std::collections::BTreeMap::new();
        for (path, e) in &entries {
            if let Some(previous) = seen.insert(e.id.clone(), path.clone()) {
                issues.push(format!(
                    "duplicate id {}: {} and {}",
                    e.id,
                    previous.display(),
                    path.display()
                ));
            }
        }
        let prefix = self.scope.prefix();
        let parse = |id: &str| {
            id.strip_prefix(prefix)?
                .strip_prefix('-')?
                .parse::<u32>()
                .ok()
        };
        let archived = self.load_archived_ids();
        let voided = self.voided_ids();
        let mut numbers: std::collections::BTreeSet<u32> = entries
            .iter()
            .map(|(_, e)| e.id.as_str())
            .chain(archived.iter().map(String::as_str))
            .filter_map(parse)
            .collect();
        // D-321:登记在 voided-ids.md 里的编号是"已交代的缺号",不再是账实不符。
        numbers.extend(voided.keys().copied());
        // 登记为 voided 的编号如果又出现了条目(手工改号/恢复),账实不符,必须可见。
        for (number, reason) in &voided {
            let alive = entries.iter().any(|(_, e)| parse(&e.id) == Some(*number))
                || archived.iter().any(|id| parse(id) == Some(*number));
            if alive {
                issues.push(format!(
                    "{prefix}-{number:03} recorded as voided ({reason}) but an entry exists — \
                     delete the voided-ids.md line or renumber the entry"
                ));
            }
        }
        if let Some(&max) = numbers.iter().max() {
            let missing: Vec<String> = (1..=max)
                .filter(|n| !numbers.contains(n))
                .map(|n| format!("{prefix}-{n:03}"))
                .collect();
            if !missing.is_empty() {
                // D-321:文案必须诚实——只有目录真在 git 版本控制下才指引 restore from git;
                // 否则给出可执行的处置(检查回收站/备份,或登记 voided-ids.md 注销)。
                let hint = if self.under_git() {
                    "data loss? restore from git; or acknowledge the gap via voided-ids.md"
                } else {
                    "data loss (no git backup) — check recycle bin/backup, or acknowledge \
                     the gap via voided-ids.md"
                };
                issues.push(format!("MISSING ids ({hint}): {}", missing.join(", ")));
            }
        }
        issues
    }

    /// id 字符串 → 编号(M-042 → 42)。不符合本 scope 前缀返回 None。
    fn id_number(&self, id: &str) -> Option<u32> {
        let prefix = self.scope.prefix();
        id.strip_prefix(prefix)?
            .strip_prefix('-')?
            .parse::<u32>()
            .ok()
    }

    /// voided 台账文件(D-321 注销通道):删除/丢失的编号登记在此,integrity 不再当
    /// 账实不符报 MISSING;与 markdown 条目同哲学——人可编辑,行格式 `- M-xxx: 理由`。
    fn voided_ledger_file(&self) -> PathBuf {
        self.root.join("voided-ids.md")
    }

    /// 已注销编号 → 理由。解析宽容:认不出的行忽略。
    pub fn voided_ids(&self) -> std::collections::BTreeMap<u32, String> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(text) = std::fs::read_to_string(self.voided_ledger_file()) else {
            return out;
        };
        for line in text.lines() {
            let Some(body) = line.trim().strip_prefix("- ") else {
                continue;
            };
            let Some((id, reason)) = body.split_once(':') else {
                continue;
            };
            if let Some(number) = self.id_number(id.trim()) {
                out.insert(number, reason.trim().to_string());
            }
        }
        out
    }

    /// 主动注销一个缺失编号(如误删后确认无法恢复)。理由必填,且该编号当前必须真的
    /// 不存在于活动/归档——拿它去"清掉"一个还活着的条目是删数据,不是记账。
    pub fn void_id(&self, id: &str, reason: &str) -> anyhow::Result<()> {
        let reason = reason.trim();
        if reason.len() < 4 {
            anyhow::bail!("废弃编号必须写明理由(为什么这个号不该有条目、依据是什么)");
        }
        let Some(number) = self.id_number(id) else {
            anyhow::bail!("`{id}` 不是 {} 前缀的合法编号", self.scope.prefix());
        };
        if self.load_all().iter().any(|(_, e)| e.id == id)
            || self.load_archived_ids().iter().any(|a| a == id)
        {
            anyhow::bail!(
                "{id} 仍存在于活动或归档中,不能作为空洞注销;要终结它请用 memory_deprecate/清理流程"
            );
        }
        if self.voided_ids().contains_key(&number) {
            return Ok(());
        }
        let path = self.voided_ledger_file();
        let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            format!(
                "# {} Memory ID Ledger\n\n引擎维护:记录被主动废弃的编号及理由。\n\
                 缺号只有登记在此才算已交代;其余缺号 = 账实不符,必须查清。\n",
                self.scope.label()
            )
        });
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("- {id}: {reason}\n"));
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// 记忆根目录(或任一祖先,最多上溯 8 层)是否在 git 版本控制下——决定 MISSING
    /// 文案能否指引 restore from git(D-321:U-001~004 目录不在版本控制却提示 git 恢复,误导)。
    fn under_git(&self) -> bool {
        let mut dir: Option<&std::path::Path> = Some(&self.root);
        for _ in 0..8 {
            let Some(d) = dir else {
                break;
            };
            if d.join(".git").exists() {
                return true;
            }
            dir = d.parent();
        }
        false
    }

    /// 合并重复:并入首个 id(保住最老引用),其余 stale 并留 superseded_by 墓碑链接。
    pub fn merge(
        &self,
        primary: &str,
        duplicates: &[String],
        title: Option<&str>,
        description: Option<&str>,
        body: Option<&str>,
        confirmed: bool,
    ) -> anyhow::Result<MemoryEntry> {
        if duplicates.is_empty() {
            anyhow::bail!("merge needs at least one duplicate id");
        }
        if duplicates.iter().any(|d| d == primary) {
            anyhow::bail!("primary id cannot appear in duplicates");
        }
        let entries = self.load_all();
        for id in std::iter::once(&primary.to_string()).chain(duplicates.iter()) {
            if !entries.iter().any(|(_, e)| e.id.as_str() == id.as_str()) {
                anyhow::bail!("unknown memory id `{id}`");
            }
        }
        // R-165 批4 merge 保守闸(⑧):评估器落地前只合并同 fingerprint 或用户确认的。
        // 无用户确认时,primary 与每个 duplicate 必须共享至少一个 [fp:...] 标记,
        // 否则拒绝——合并会销毁证据链,不能靠 manager 自觉。
        if !confirmed {
            let fps_of = |id: &str| -> Vec<String> {
                entries
                    .iter()
                    .find(|(_, e)| e.id == id)
                    .map(|(_, e)| super::fp_markers(&e.body))
                    .unwrap_or_default()
            };
            let primary_fps = fps_of(primary);
            for dup in duplicates {
                let shared = fps_of(dup).iter().any(|f| primary_fps.contains(f));
                if !shared {
                    anyhow::bail!(
                        "merge 保守闸: `{primary}` 与 `{dup}` 无共享 fingerprint,且未获用户确认 \
                         (confirmed=true)——评估器落地前只合并同 fingerprint 或用户确认的条目"
                    );
                }
            }
        }
        let mut merged = self.update(primary, title, description, body, None, None, false)?;
        // D-215 引擎兜底:被并条目的复发指纹与来源引用不许静默蒸发——
        // 指纹丢了复发检测就瞎了,refs 丢了记忆与来源脱钩,这两样不能赌 manager 记得带。
        let mut carried_fps: Vec<String> = Vec::new();
        let mut union_refs: Vec<String> = merged.refs();
        for id in duplicates {
            let (_, dup) = self
                .load_all()
                .into_iter()
                .find(|(_, e)| &e.id == id)
                .expect("checked above");
            for marker in super::fp_markers(&dup.body) {
                if !merged.body.contains(&marker) && !carried_fps.contains(&marker) {
                    carried_fps.push(marker);
                }
            }
            for r in dup.refs() {
                if !union_refs.contains(&r) {
                    union_refs.push(r);
                }
            }
        }
        if !carried_fps.is_empty() || union_refs != merged.refs() {
            let (path, mut entry) = self
                .load_all()
                .into_iter()
                .find(|(_, e)| e.id == merged.id)
                .expect("primary exists");
            if !carried_fps.is_empty() {
                entry
                    .body
                    .push_str(&format!("\n\n(并入指纹: {})", carried_fps.join(" ")));
            }
            if !union_refs.is_empty() {
                entry.extras.retain(|(k, _)| k != "refs");
                entry.extras.push(("refs".into(), union_refs.join(" ")));
            }
            self.write_entry(&entry, Some(&path))?;
            merged = entry;
        }
        for id in duplicates {
            let (path, mut entry) = self
                .load_all()
                .into_iter()
                .find(|(_, e)| &e.id == id)
                .expect("checked above");
            entry.status = "deprecated".into();
            entry.updated = today();
            entry
                .extras
                .retain(|(k, _)| !k.eq_ignore_ascii_case("superseded_by"));
            entry
                .extras
                .push(("superseded_by".into(), primary.to_string()));
            self.write_entry(&entry, Some(&path))?;
        }
        self.refresh_derived()?;
        Ok(merged)
    }

    /// 按标题前缀找一条 active 偏好条目(开发重心这类"随时会改的定调")。
    pub fn find_preference(&self, title_prefix: &str) -> Option<MemoryEntry> {
        self.load_all().into_iter().map(|(_, e)| e).find(|e| {
            e.category == "preference" && e.status == "active" && e.title.starts_with(title_prefix)
        })
    }

    /// 用户直写的偏好 upsert:标题前缀命中就改,否则新增。
    /// 定调会被反复调整,必须复用同一条目——每次切换都新增会把索引撑爆且历史无从对照。
    /// (用户手改路径,不经 memory-manager;A-005:用户编辑本就不受写读分离约束。)
    pub fn upsert_preference(
        &self,
        title_prefix: &str,
        title: &str,
        description: &str,
        body: &str,
    ) -> anyhow::Result<MemoryEntry> {
        if let Some(existing) = self.find_preference(title_prefix) {
            let entries = self.load_all();
            let (path, mut entry) = entries
                .into_iter()
                .find(|(_, e)| e.id == existing.id)
                .expect("found above");
            entry.title = title.trim().to_string();
            entry.description = description.trim().to_string();
            entry.body = body.trim().to_string();
            entry.updated = today();
            self.write_entry(&entry, Some(&path))?;
            self.refresh_derived()?;
            return Ok(entry);
        }
        match self.add(
            "preference",
            title,
            description,
            body,
            "user",
            &[],
            None,
            true,
        )? {
            AddOutcome::Added(entry)
            | AddOutcome::Duplicate(entry)
            | AddOutcome::SubjectConflict(entry) => Ok(entry),
            // force=true 已跳过语义闸,Uncertain 不应出现;保守处理为取候选首条。
            AddOutcome::Uncertain(mut candidates) => Ok(candidates
                .pop()
                .ok_or_else(|| anyhow::anyhow!("force add failed: no entry"))?),
        }
    }

    /// 召回→采纳画像(R-149):id → (被开跑预检索召回次数, 其中正文被拉取次数)。
    /// 遗忘成本 F(m) 的经验代理:反复召回却从不采纳 = 语义显著但决策无关的头号嫌疑。
    pub fn recall_profile(&self) -> std::collections::BTreeMap<String, (u64, u64)> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(conn) = self.open_db() else { return out };
        let Ok(mut statement) = conn.prepare(
            "SELECT entry_id, COUNT(*), COALESCE(SUM(fetched), 0)
             FROM memory_recalls GROUP BY entry_id",
        ) else {
            return out;
        };
        if let Ok(rows) = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        }) {
            for (id, recalled, fetched) in rows.flatten() {
                out.insert(id, (recalled.max(0) as u64, fetched.max(0) as u64));
            }
        }
        out
    }

    /// 精确子串查找 active 条目正文里的指纹标记(复发检测,R-149)。
    /// 不用 FTS 相似度:弱模型只需原样复制标记,引擎侧零阈值、可单测。
    /// 按指纹找既有条目。**active 与 candidate 都算**——candidate 看不见正是
    /// 重复条目的生产线:manager 每消化一条同类 inbox note 都以为是新知识,
    /// 2026-08-12 清理时一个「tracker update 字段语义」的坑堆了 8 条 candidate。
    /// 匹配走归一后的标记比对,老口径的正文标记照样命中。
    pub fn find_by_marker(&self, marker: &str) -> Option<MemoryEntry> {
        let key = kanzei_core::normalize_fp_marker(marker);
        let mut hit: Option<MemoryEntry> = None;
        for (_, entry) in self.load_all() {
            if entry.status != "active" && entry.status != "candidate" {
                continue;
            }
            let matched = super::fp_markers(&entry.body)
                .iter()
                .chain(entry.field("fingerprint").map(str::to_string).iter())
                .any(|fp| kanzei_core::normalize_fp_marker(fp) == key);
            if !matched {
                continue;
            }
            // active 优先:同一个坑既有 active 又有 candidate 时,该改的是 active。
            if entry.status == "active" {
                return Some(entry);
            }
            hit.get_or_insert(entry);
        }
        hit
    }

    /// 效果画像(R-125):id → (累计命中, 最近命中时间毫秒)。最近命中时间为 0 = 从未命中,
    /// 前端据此标"长期零命中",这是判断某条记忆该不该留的直接依据。
    pub fn hit_profile(&self) -> std::collections::BTreeMap<String, (u64, i64)> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(conn) = self.open_db() else { return out };
        let Ok(mut statement) = conn.prepare("SELECT id, hits, last_hit_at FROM memory_hits")
        else {
            return out;
        };
        if let Ok(rows) = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        }) {
            for (id, hits, last) in rows.flatten() {
                out.insert(id, (hits.max(0) as u64, last));
            }
        }
        out
    }

    /// 命中统计(UI 展示):id → hits。库缺失返回空表(派生物语义)。
    pub fn hits_map(&self) -> std::collections::BTreeMap<String, u64> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(conn) = self.open_db() else {
            return out;
        };
        let Ok(mut statement) = conn.prepare("SELECT id, hits FROM memory_hits") else {
            return out;
        };
        if let Ok(rows) =
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        {
            for row in rows.flatten() {
                out.insert(row.0, row.1.max(0) as u64);
            }
        }
        out
    }

    pub fn read_inbox(&self) -> String {
        std::fs::read_to_string(self.root.join("inbox.md")).unwrap_or_default()
    }

    /// manager 消化完毕后清空草稿箱(整箱内容已在触发 prompt 里,清空即"已消费")。
    pub fn clear_inbox(&self) -> anyhow::Result<()> {
        let path = self.root.join("inbox.md");
        // R-215:与 append_note 同锁——整箱清空是兜底操作,锁内执行避免与并发
        // append 交错(append 持锁读-拼-写回,clear 持锁覆盖,不会互吃)。
        let _lock = crate::atomic_file::lock_exclusive(&path)?;
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
        // R-215:读-拼接-写回必须整体持锁——并发 append 若各读各的再各自写回,
        // 后写者覆盖先写者,note 无痕丢失(store.rs 原实现)。锁与 discard_note/
        // clear_inbox 共用同一把,消化与追加互斥。
        let _lock = crate::atomic_file::lock_exclusive(&path)?;
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
        // R-215:与 append_note 共用同一把锁,消化与追加互斥——锁内读-改-写回,
        // 不会把并发 append 的内容当旧快照覆盖掉。
        let path = self.root.join("inbox.md");
        let _lock = crate::atomic_file::lock_exclusive(&path)?;
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

    /// legacy 迁移:R-098 的 .kanzei/project/memory.md(tracker M-条目)→ 一条一文件。
    /// 幂等:legacy 文件不存在即跳过;迁移后原文件改写为指路牌。
    fn migrate_legacy(&self, project_root: &Path) {
        let legacy = project_root
            .join(".kanzei")
            .join("project")
            .join("memory.md");
        if !legacy.is_file() {
            return;
        }
        let Ok(text) = std::fs::read_to_string(&legacy) else {
            return;
        };
        let entries = crate::docstore::parse(&crate::docstore::MEMORY, &text);
        if entries.is_empty() {
            return;
        }
        let now = today();
        for legacy_entry in &entries {
            if legacy_entry.id.is_empty() {
                continue;
            }
            let body: String = legacy_entry
                .fields
                .iter()
                .map(|(k, v)| format!("- {k}: {v}\n"))
                .collect();
            let entry = MemoryEntry {
                id: legacy_entry.id.clone(),
                scope: self.scope.label().into(),
                category: "fact".into(),
                title: legacy_entry.title.clone(),
                description: format!("{}(迁移自 memory.md)", legacy_entry.title),
                status: if legacy_entry.status == "stale" {
                    "deprecated"
                } else {
                    "active"
                }
                .into(),
                created: now.clone(),
                updated: now.clone(),
                source: "migration".into(),
                extras: Vec::new(),
                body,
            };
            let _ = self.write_entry(&entry, None);
        }
        let _ = self.refresh_derived();
        let _ = crate::atomic_file::write_atomic(
            &legacy,
            "# Memory\n\n(已迁移至 .kanzei/memory/,由 memory_search 检索;本文件不再使用。)\n",
        );
    }
}

/// R-216:标题是否含 tracker 条目编号(R-xxx / D-xxx)。
fn has_tracker_id(title: &str) -> bool {
    let upper = title.to_uppercase();
    for prefix in ["R-", "D-"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return true;
            }
        }
        // 也可能编号在中间:「关于 R-012 的交付状态」。
        for (idx, _) in upper.match_indices(prefix) {
            let after = &upper[idx + 2..];
            if after.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 近似重复的判定阈值:两个标题切词后的包含度。0.55 是拿实测样本卡的——
/// 2026-08-12 归档的那 8 条「tracker update 字段语义」两两在 0.57~0.75,
/// 而 M-011《repair_reused_id 修复》与 M-012《完整性门禁拒绝写操作》这种
/// 同子系统但确属两条知识的只有 0.32,阈值落在中间。
const TITLE_DUP_THRESHOLD: f64 = 0.55;

/// 光看比例会误杀短标题:「安装通道切换 SOP」与「安装通道改为便携版」共享
/// 「安装通道」四个字就到 0.57,但那是两条知识。所以再加一道绝对量下限——
/// 真重复的那 8 条两两共享 12~16 个词,短标题的偶然同名到不了 8。
const TITLE_DUP_MIN_COMMON: usize = 8;

/// 标题切词:CJK 按单字、ASCII 按词(小写),标点丢弃。CJK 不分词就没法比,
/// 单字粒度对中文标题足够——记忆标题短,长词的顺序信息不重要。
fn title_tokens(title: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut word = String::new();
    for ch in title.chars() {
        if is_cjk(ch) {
            if !word.is_empty() {
                out.insert(std::mem::take(&mut word));
            }
            out.insert(ch.to_string());
        } else if ch.is_alphanumeric() {
            word.extend(ch.to_lowercase());
        } else if !word.is_empty() {
            out.insert(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.insert(word);
    }
    out
}

/// 包含度 = 交集 / 较短一侧。用包含度而不是 Jaccard:同一个坑的重复条目
/// 常常一条写得长、一条写得短,Jaccard 会被长的那条稀释掉。
/// 比例与绝对量两道都要过,少一道就会误杀短标题(见 TITLE_DUP_MIN_COMMON)。
fn title_containment(a: &str, b: &str) -> f64 {
    let (ta, tb) = (title_tokens(a), title_tokens(b));
    let shorter = ta.len().min(tb.len());
    let common = ta.intersection(&tb).count();
    if shorter < 6 || common < TITLE_DUP_MIN_COMMON {
        return 0.0;
    }
    common as f64 / shorter as f64
}

/// unicode61 把连续 CJK 当单个整词,子串查不到(拍板点③的即时实证)。
/// 零依赖解法:索引与查询两侧都做 CJK 单字切分;查询侧每个用户词作为
/// 相邻单字的短语匹配,保住精度;词间 OR 联接靠 bm25 排相关度。
fn segment_cjk(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if is_cjk(ch) {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            out.push(ch);
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

/// D-282:主题 token 交集计数。英文按词(小写)、CJK 按单字,去掉高频虚词
/// (的/了/是/在/与…),避免「新 description 全是通用字」被误判为同主题。
/// 返回 0 表示两段文本没有共同主题词——description 与条目主题漂移的判据。
fn topic_overlap(a: &str, b: &str) -> usize {
    fn tokens(text: &str) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        let mut word = String::new();
        for ch in text.chars() {
            if ch.is_ascii_alphanumeric() {
                word.push(ch.to_ascii_lowercase());
            } else {
                if word.len() >= 2 {
                    set.insert(word.clone());
                }
                word.clear();
                if is_cjk(ch) && !STOP_CHARS.contains(&ch) {
                    set.insert(ch.to_string());
                }
            }
        }
        if word.len() >= 2 {
            set.insert(word);
        }
        set
    }
    let ta = tokens(a);
    let tb = tokens(b);
    ta.intersection(&tb).count()
}

/// 主题判据里忽略的 CJK 虚词/通用字(单字交集噪音源)。
const STOP_CHARS: &[char] = &[
    '的', '了', '是', '在', '与', '和', '或', '及', '对', '为', '时', '要', '不', '会', '能', '可',
    '等', '这', '那', '就', '也', '被', '把', '从', '到', '以', '于', '之', '其', '它', '他', '她',
    '个', '条', '种', '次', '项', '处', '点', '段', '行', '列', '张', '件', '份', '页', '步', '轮',
    '批', '并', '且', '但', '而', '若', '则', '即', '如', '虽', '还', '又', '再', '更', '最', '很',
    '太', '仅', '只', '已', '未', '无', '有', '没', '别', '自', '各', '每', '某', '几', '两', '多',
    '少', '一', '二', '三', '四', '五', '六', '七', '八', '九', '十', '百', '千', '万', '零',
];

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// 展示用:把切分产生的 CJK 字间空格收回去。
fn unsegment_cjk(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &ch) in chars.iter().enumerate() {
        if ch == ' '
            && i > 0
            && i + 1 < chars.len()
            && is_cjk(chars[i - 1])
            && (is_cjk(chars[i + 1]) || matches!(chars[i + 1], '[' | ']' | '…'))
        {
            continue;
        }
        out.push(ch);
    }
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out
}

/// R-233 ②:从用户 prompt 提取意图词,而非原样整句进 FTS。
///
/// 整句中文 prompt 经 fts_query 拆 bigram 时,虚词会把实词边界错位
/// (「帮我把这一批发版出去」→ 批发/版出,匹配不到「发版」)。本函数:
/// ① ASCII 词原样保留(≥2 字母);② CJK 按 INTENT_BOUNDARY(虚词 + 请求
/// 语气/方向补语)切成内容段,去掉纯语气段;③ 每段 ≤4 字给整段短语、
/// ≥3 字段补交 bigram(跨真实词边界的段只给整段短语会漏词,如
/// 「批发版」=「这批」+「发版」)、>4 字段交 bigram;④ 去重、封顶 24 词。
/// 返回空格分隔的候选词文本,供 store.search 内部再走 fts_query
/// (或作 dense 通道的 query 文本)。
pub fn intent_query(prompt: &str) -> String {
    let mut terms: Vec<String> = Vec::new();
    let mut ascii = String::new();
    let mut cjk_run: Vec<char> = Vec::new();
    let flush_ascii = |ascii: &mut String, terms: &mut Vec<String>| {
        if ascii.len() >= 2 {
            terms.push(ascii.to_ascii_lowercase());
        }
        ascii.clear();
    };
    let flush_run = |run: &mut Vec<char>, terms: &mut Vec<String>| {
        if run.len() >= 2 {
            // 与 fts_query 的 CJK 段同规:≤4 字整段短语(短查询保精度)。
            if run.len() <= 4 {
                terms.push(run.iter().collect::<String>());
            }
            // 3 字以上的段可能跨真实词边界(「批发版」=「这批」+「发版」),
            // 只给整段短语会漏掉边界另一侧的词(短语要求逐字相邻),补交
            // bigram 提召回;噪声 bigram 靠 bm25 排下去。>4 字段本就交 bigram。
            if run.len() >= 3 {
                for pair in run.windows(2) {
                    terms.push(pair.iter().collect::<String>());
                }
            }
        }
        run.clear();
    };
    for ch in prompt.chars() {
        if is_cjk(ch) {
            flush_ascii(&mut ascii, &mut terms);
            if INTENT_BOUNDARY.contains(&ch) {
                // 虚词/语气字:结束当前内容段,不进查询词。
                flush_run(&mut cjk_run, &mut terms);
            } else {
                cjk_run.push(ch);
            }
        } else if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            flush_run(&mut cjk_run, &mut terms);
            ascii.push(ch);
        } else {
            flush_ascii(&mut ascii, &mut terms);
            flush_run(&mut cjk_run, &mut terms);
        }
    }
    flush_ascii(&mut ascii, &mut terms);
    flush_run(&mut cjk_run, &mut terms);
    let mut seen = std::collections::HashSet::new();
    terms.retain(|t| seen.insert(t.clone()));
    terms.truncate(24);
    terms.join(" ")
}

/// intent_query 的 CJK 边界字:虚词 + 请求语气/方向补语。出现在这些字处切断
/// 内容段——它们是高频噪音,混进查询词只会错位 bigram、稀释 bm25 相关度。
const INTENT_BOUNDARY: &[char] = &[
    // 纯功能字/助词/连词/副词:几乎不作实词词素,切断防噪音 bigram。注意
    // 别把实词词素放进来——可/能/对/为/时/要/不/从/到/自/项/条/页/步/
    // 行/处/轮(可靠、能力、针对、为了、时间、要求、自动、自举、项目、
    // 条目、页面、步骤、执行、处理、本轮)一旦切断,核心名词直接被拆没。
    '的', '了', '是', '在', '与', '和', '或', '及', '之', '其', '它', '他', '她', '被', '把', '这',
    '那', '就', '也', '都', '还', '又', '再', '更', '最', '很', '太', '仅', '只', '已', '未', '没',
    '别', '每', '某', '几', '两', '多', '少', '等', '并', '且', '但', '而', '若', '则', '即', '如',
    '虽', '各', // 数字:查询噪音。
    '一', '二', '三', '四', '五', '六', '七', '八', '九', '十', '百', '千', '万', '零',
    // 请求语气/人称/方向补语:任务 prompt 高频、无检索区分度(「帮我处理
    // 一下」「做出来」);主词通常落在段的另一侧,不受影响。
    '帮', '请', '我', '你', '您', '上', '下', '去', '来', '出', '进', '回', '过', '起', '走', '些',
    '嘛', '呢', '吧', '吗', '啊', '哦', '哪',
];

/// FTS5 MATCH 表达式(防语法注入,词间 OR 靠 bm25 排相关度):
/// ASCII 词原样引号;CJK 段 ≤4 字整段短语(短查询保精度),
/// >4 字拆 bigram 短语(整句 prompt 也能命中"发版"这类子串)。封顶 24 词。
fn fts_query(query: &str) -> String {
    let mut terms: Vec<String> = Vec::new();
    for token in query.split_whitespace() {
        let token = token.replace('"', "");
        let mut ascii = String::new();
        let mut cjk: Vec<char> = Vec::new();
        for ch in token.chars() {
            if is_cjk(ch) {
                flush_ascii(&mut ascii, &mut terms);
                cjk.push(ch);
            } else if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                flush_cjk(&mut cjk, &mut terms);
                ascii.push(ch);
            } else {
                flush_ascii(&mut ascii, &mut terms);
                flush_cjk(&mut cjk, &mut terms);
            }
        }
        flush_ascii(&mut ascii, &mut terms);
        flush_cjk(&mut cjk, &mut terms);
    }
    let mut seen = std::collections::HashSet::new();
    terms.retain(|t| seen.insert(t.clone()));
    terms.truncate(24);
    terms.join(" OR ")
}

fn flush_ascii(ascii: &mut String, terms: &mut Vec<String>) {
    if !ascii.is_empty() {
        terms.push(format!("\"{ascii}\""));
        ascii.clear();
    }
}

fn flush_cjk(cjk: &mut Vec<char>, terms: &mut Vec<String>) {
    match cjk.len() {
        0 => {}
        1..=4 => {
            let phrase: Vec<String> = cjk.iter().map(|c| c.to_string()).collect();
            terms.push(format!("\"{}\"", phrase.join(" ")));
        }
        _ => {
            for pair in cjk.windows(2) {
                terms.push(format!("\"{} {}\"", pair[0], pair[1]));
            }
        }
    }
    cjk.clear();
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
        let store = MemoryStore::open(MemoryScope::Project, dir.join(".kanzei").join("memory"));
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
        assert!(matches!(forced, AddOutcome::Added(_)));
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
        let (dir, store) = temp_store();
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
        // 真的是另一个坑时,force 仍然放行(逃生门不堵死)。
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
            AddOutcome::Added(_)
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
            store.record_recall("要发版了", &hits, 256);
            store.mark_recall_fetched(&b);
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
            store.record_recall("要发版了", &hits, 256);
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
