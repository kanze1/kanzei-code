//! MemoryStore:单 scope 的记忆仓库。
//! 硬门禁在写入侧:ID 引擎分配、枚举校验、description 必填、精确重复拒绝;
//! INDEX.md 与 index.db(FTS5/hits)都是派生物,损坏可由文件全量重建;
//! 写入 tmp+rename 原子替换,不做跨进程锁(用户定调:竞争冲突留给 agent 事后解决)。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use super::{parse_entry, render_entry, today, MemoryEntry, MemoryScope, CATEGORIES, STATUSES};

/// 检索结果(含派生指标)。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub entry: MemoryEntry,
    pub path: PathBuf,
    pub snippet: String,
    pub hits: u64,
    pub score: f64,
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
pub enum AddOutcome {
    Added(MemoryEntry),
    /// 精确标题重复:拒绝写入并返回既有条目(要求转 update 或 force)。
    Duplicate(MemoryEntry),
    /// 状态不变量(R-149):同 scope+category+subject 至多一条 active。
    /// 冲突返回既有条目,force 不可绕——状态就地覆盖(memory_update),绝不并存。
    SubjectConflict(MemoryEntry),
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
pub fn decision_weight(recalled: u64, fetched: u64) -> f64 {
    if recalled < 3 {
        return 1.0;
    }
    let rate = fetched.min(recalled) as f64 / recalled as f64;
    0.6 + 0.7 * rate
}

pub struct MemoryStore {
    pub scope: MemoryScope,
    pub root: PathBuf,
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
        dir.flatten().filter(|p| p.path().extension().and_then(|e| e.to_str()) == Some("md")).count()
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
        let subject = subject.map(str::trim).filter(|s| !s.is_empty());
        let entries = self.load_all();
        // 状态不变量先于标题去重,且不受 force 影响:状态就地覆盖,绝不并存。
        // 仅 active 持有 subject——candidate 未验证不占状态槽(R-165)。
        if let Some(subject) = subject {
            if let Some((_, existing)) = entries.iter().find(|(_, e)| {
                e.status == "active"
                    && e.category == category
                    && e.extras.iter().any(|(k, v)| k == "subject" && v == subject)
            }) {
                return Ok(AddOutcome::SubjectConflict(existing.clone()));
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
        let entry = MemoryEntry {
            id: self.next_id(&entries),
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
    pub fn update(
        &self,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        body: Option<&str>,
        status: Option<&str>,
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
        if let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) {
            entry.title = title.into();
        }
        if let Some(desc) = description.map(str::trim).filter(|d| !d.is_empty()) {
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
        entry.status = "active".into();
        entry.updated = today();
        self.write_entry(&entry, Some(&path))?;
        // 证据落 state.db memory_sources 表(与 episodes 同库,可 join)。
        // 仅 project scope 有 state.db(global 记忆无 episode 证据源)。
        let hash = source_hash.unwrap_or("compiler").to_string();
        if self.scope == MemoryScope::Project {
            let db_path = self.root.join("..").join("state.db");
            if let Ok(store) = kanzei_core::SessionStore::open(&db_path) {
                for (episode_id, event_start, event_end) in sources {
                    // episode_id 外键必须真实存在(state.db episodes),否则 INSERT 静默失败。
                    let _ = store.record_memory_source(
                        id,
                        *episode_id,
                        *event_start,
                        *event_end,
                        &hash,
                    );
                }
            }
        }
        self.refresh_derived()?;
        Ok(entry)
    }

    fn write_entry(&self, entry: &MemoryEntry, existing_path: Option<&Path>) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let path = match existing_path {
            Some(p) => p.to_path_buf(),
            None => self.root.join(format!("{}.md", entry.file_stem())),
        };
        atomic_write(&path, &render_entry(entry))
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
        atomic_write(&self.index_md(), &index)?;

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

    /// FTS 检索:bm25 取 topN 后按采纳率决策权重与 active 加权在 Rust 侧重排。
    /// 命中仍计 hits(R-125 观测),但 R-150 起 hits 不再参与排序(自增强退役)。
    pub fn search(
        &self,
        query: &str,
        category: Option<&str>,
        status: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<SearchHit>> {
        let match_expr = fts_query(query);
        if match_expr.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open_db()?;
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
        let recall_stats = self.recall_profile();
        let mut hits_out: Vec<SearchHit> = Vec::new();
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
            // bm25 越小越相关(fts5 返回负值);取负得正相关度。
            // R-150:退役 hits 乘子——搜索命中是自增强循环(常被搜到→排更前→更常被搜到),
            // 与采纳率权重「召回未采纳→沉底」方向冲突;理论 importance ≠ semantic salience。
            // 排序权重只留 bm25 相关度 + 采纳率决策价值,hit_count 降为观测(SearchHit.hits)。
            let mut score = -bm25;
            // R-149:反复被召回却从不被采纳的条目 = 语义显著但决策无关,温和沉底。
            // preference 豁免:其正文全文常驻(STANDING DIRECTIVES),模型永远不需要
            // 再拉正文,采纳率结构性偏低、无意义(实证:M-002 召回 22 采纳 4)。
            if entry.category != "preference" {
                if let Some(&(recalled, fetched)) = recall_stats.get(&id) {
                    score *= decision_weight(recalled, fetched);
                }
            }
            if entry.status != "active" {
                score *= 0.5;
            }
            hits_out.push(SearchHit {
                entry: entry.clone(),
                path: path.clone(),
                snippet: unsegment_cjk(&snippet),
                hits: hit_count,
                score,
            });
        }
        hits_out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits_out.truncate(limit);
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        for hit in &hits_out {
            conn.execute(
                "INSERT INTO memory_hits(id, hits, last_hit_at) VALUES (?1, 1, ?2)
                 ON CONFLICT(id) DO UPDATE SET hits = hits + 1, last_hit_at = ?2",
                params![hit.entry.id, now_ms],
            )?;
        }
        Ok(hits_out)
    }

    /// R-165 批2 novelty gate 三档:明显新 → PROPOSE、明显重复 → NOOP、
    /// 不确定 → 才起 LLM 判断(验收④)。
    /// 机械判据:标题规范化精确命中既有 active 记忆 = 明显重复;
    /// FTS 无任何命中 = 明显新;有命中但非精确 = 不确定。
    pub fn classify_novelty(&self, title: &str, description: &str, body: &str) -> Novelty {
        let normalized = normalize_title(title);
        let entries = self.load_all();
        let dup = entries
            .iter()
            .any(|(_, e)| e.status == "active" && normalize_title(&e.title) == normalized);
        if dup {
            return Novelty::Duplicate;
        }
        // 用描述+正文做 FTS 探测:有明显语义命中即不确定,否则新。
        let probe = format!("{} {}", description, body);
        match self.search(&probe, None, Some("active"), 3) {
            Ok(hits) if !hits.is_empty() => Novelty::Uncertain,
            _ => Novelty::New,
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
        let numbers: std::collections::BTreeSet<u32> = entries
            .iter()
            .map(|(_, e)| e.id.as_str())
            .chain(self.load_archived_ids().iter().map(String::as_str))
            .filter_map(parse)
            .collect();
        if let Some(&max) = numbers.iter().max() {
            let missing: Vec<String> = (1..=max)
                .filter(|n| !numbers.contains(n))
                .map(|n| format!("{prefix}-{n:03}"))
                .collect();
            if !missing.is_empty() {
                issues.push(format!(
                    "MISSING ids (data loss? restore from git): {}",
                    missing.join(", ")
                ));
            }
        }
        issues
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
        let mut merged = self.update(primary, title, description, body, None)?;
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
    pub fn find_active_by_marker(&self, marker: &str) -> Option<MemoryEntry> {
        self.load_all()
            .into_iter()
            .map(|(_, e)| e)
            .find(|e| e.status == "active" && e.body.contains(marker))
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
        if path.is_file() {
            atomic_write(&path, "# Memory Inbox\n")?;
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
        atomic_write(&path, &text)?;
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
        atomic_write(&self.root.join("inbox.md"), &next)?;
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
        let _ = atomic_write(
            &legacy,
            "# Memory\n\n(已迁移至 .kanzei/memory/,由 memory_search 检索;本文件不再使用。)\n",
        );
    }
}

fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
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

/// tmp+rename 原子替换(std::fs::rename 在 Windows 用 MOVEFILE_REPLACE_EXISTING)。
fn atomic_write(path: &Path, content: &str) -> anyhow::Result<()> {
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        match store
            .add(category, title, desc, body, "user", &[], None, false)
            .unwrap()
        {
            AddOutcome::Added(e) => e,
            AddOutcome::Duplicate(e) => panic!("unexpected duplicate of {}", e.id),
            AddOutcome::SubjectConflict(e) => panic!("unexpected subject conflict with {}", e.id),
        }
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
        let hits = store.search("发版 更新", None, Some("active"), 5).unwrap();
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
        // 命中计数生效
        let again = store.search("发版", None, None, 5).unwrap();
        assert!(again[0].hits >= 1);
        // 删库重建:真源是文件
        drop(again);
        std::fs::remove_file(store.db_path()).unwrap();
        store.refresh_derived().unwrap();
        let rebuilt = store.search("CRLF", None, None, 5).unwrap();
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
        let hits = store.search("发版 更新", None, Some("active"), 5).unwrap();
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
            .update(&e.id, None, None, Some("V2 修订"), Some("stale"))
            .unwrap();
        assert_eq!(updated.status, "deprecated"); // R-165:stale 兼容映射 deprecated
        assert_eq!(updated.body, "V2 修订");
        assert_eq!(updated.created, e.created);
        // stale 默认不出现在 active 过滤下
        let active_only = store.search("结论", None, Some("active"), 5).unwrap();
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
        // 有证据 → 晋升 active
        let promoted = store
            .promote(&candidate.id, &[(1, Some(0), Some(10))], Some("test-hash"))
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
        let dup = store.classify_novelty("GH 网络代理", "push 前必读", "");
        assert_eq!(dup, Novelty::Duplicate, "规范化标题应命中 active 记忆");
        // 明显新:无重叠词。
        let fresh = store.classify_novelty("diff 树渲染优化", "R-133 diff 渲染", "");
        assert_eq!(fresh, Novelty::New, "无关主题应判明显新");
        // 不确定:有语义命中但标题不同(代理相关)。
        let uncertain = store.classify_novelty("网络代理配置", "HTTPS_PROXY 与代理地址", "");
        assert_eq!(
            uncertain,
            Novelty::Uncertain,
            "语义相关但非精确应留 LLM 判断"
        );
        // 计数遥测落库。
        store.record_novelty(&dup, "", "GH 网络代理");
        store.record_novelty(&fresh, "", "diff 树渲染优化");
        store.record_novelty(&uncertain, "", "网络代理配置");
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
            .update(&entry.id, None, None, None, Some("deprecated"))
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
        let hits = store.search("将被推翻", None, None, 5).unwrap();
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
            .update(&invalid.id, None, None, None, Some("invalid"))
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
        let hits_default = store.search("影子评估", None, None, 5).unwrap();
        assert!(
            hits_default.iter().all(|h| h.entry.id != candidate.id),
            "shadow 不得进默认检索"
        );
        let hits_active = store.search("影子评估", None, Some("active"), 5).unwrap();
        assert!(
            hits_active.iter().all(|h| h.entry.id != candidate.id),
            "shadow 不得冒充 active"
        );
        // 显式查 shadow 可见(评估器通道)。
        let hits_shadow = store.search("影子评估", None, Some("shadow"), 5).unwrap();
        assert!(
            hits_shadow.iter().any(|h| h.entry.id == candidate.id),
            "显式查 shadow 应可见"
        );
        // shadow → active 仍需 provenance。
        let err = store.promote(&candidate.id, &[], None).unwrap_err();
        assert!(err.to_string().contains("no memory_sources evidence"));
        let promoted = store
            .promote(&candidate.id, &[(3, Some(0), Some(5))], Some("shadow-hash"))
            .unwrap();
        assert_eq!(promoted.status, "active");
        // 进 active 后默认检索可见。
        let hits_after = store.search("影子评估", None, None, 5).unwrap();
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
            .update(&active.id, Some("直写活跃条目改名"), None, None, None)
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
        store.update(&b.id, None, None, None, None).unwrap();
        let path = store.root.join(format!("{}.md", b.file_stem()));
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("id: M-002", "id: M-003");
        std::fs::write(&path, text).unwrap();
        let issues = store.integrity_issues();
        assert!(issues.iter().any(|i| i.contains("M-002")), "{issues:?}");
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
            .update(&first.id, None, None, None, Some("stale"))
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
    fn decision_weight_边界与单调性() {
        // 样本不足(召回<3)不动分。
        assert_eq!(decision_weight(0, 0), 1.0);
        assert_eq!(decision_weight(2, 0), 1.0);
        // 零采纳沉到下限 0.6,全采纳升到 1.3,中间线性。
        assert!((decision_weight(3, 0) - 0.6).abs() < 1e-9);
        assert!((decision_weight(4, 4) - 1.3).abs() < 1e-9);
        assert!((decision_weight(10, 5) - 0.95).abs() < 1e-9);
        // 脏数据防御:fetched > recalled 按全采纳截断。
        assert!((decision_weight(3, 9) - 1.3).abs() < 1e-9);
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
        let hits = store.search("发版", None, Some("active"), 5).unwrap();
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
        let ranked = store.search("发版", None, Some("active"), 5).unwrap();
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
            store
                .find_active_by_marker("[fp:edit|not found]")
                .map(|e| e.id),
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
        let hits = store.search("发版", None, Some("active"), 5).unwrap();
        assert_eq!(hits.len(), 2);
        for _ in 0..3 {
            store.record_recall("要发版了", &hits, 256);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // 两条同为召回 3/采纳 0:fact 吃 ×0.6,preference 保持 ×1.0 → 严格高分在前。
        let ranked = store.search("发版", None, Some("active"), 5).unwrap();
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
}
