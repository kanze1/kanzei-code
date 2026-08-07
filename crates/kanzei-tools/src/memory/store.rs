//! MemoryStore:单 scope 的记忆仓库。
//! 硬门禁在写入侧:ID 引擎分配、枚举校验、description 必填、精确重复拒绝;
//! INDEX.md 与 index.db(FTS5/hits)都是派生物,损坏可由文件全量重建;
//! 写入 tmp+rename 原子替换,不做跨进程锁(用户定调:竞争冲突留给 agent 事后解决)。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use super::{
    parse_entry, render_entry, today, MemoryEntry, MemoryScope, CATEGORIES, STATUSES,
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

/// add 的去重门禁结果。
pub enum AddOutcome {
    Added(MemoryEntry),
    /// 精确标题重复:拒绝写入并返回既有条目(要求转 update 或 force)。
    Duplicate(MemoryEntry),
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

    /// 写入门禁:枚举校验 + description 必填 + 精确标题去重(可 force)。
    pub fn add(
        &self,
        category: &str,
        title: &str,
        description: &str,
        body: &str,
        source: &str,
        force: bool,
    ) -> anyhow::Result<AddOutcome> {
        if !CATEGORIES.contains(&category) {
            anyhow::bail!("invalid category `{category}`; valid: {}", CATEGORIES.join(" | "));
        }
        let title = title.trim();
        let description = description.trim();
        if title.is_empty() {
            anyhow::bail!("title must not be empty");
        }
        if description.is_empty() {
            anyhow::bail!("description must not be empty — it is the retrieval hook");
        }
        let entries = self.load_all();
        if !force {
            let normalized = normalize_title(title);
            if let Some((_, existing)) = entries.iter().find(|(_, e)| {
                e.status == "active"
                    && e.category == category
                    && normalize_title(&e.title) == normalized
            }) {
                return Ok(AddOutcome::Duplicate(existing.clone()));
            }
        }
        let now = today();
        let entry = MemoryEntry {
            id: self.next_id(&entries),
            scope: self.scope.label().into(),
            category: category.into(),
            title: title.into(),
            description: description.into(),
            status: "active".into(),
            created: now.clone(),
            updated: now,
            source: source.into(),
            extras: Vec::new(),
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
            entry.status = status.into();
        }
        entry.updated = today();
        // 文件名沿用旧路径(slug 终身不改)。
        self.write_entry(&entry, Some(&path))?;
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

    /// 重建全部派生物:INDEX.md 与 FTS 索引。任何写操作后调用;损坏时可手动全量重建。
    pub fn refresh_derived(&self) -> anyhow::Result<()> {
        let entries = self.load_all();
        // INDEX.md:一行一条(仅 active),stale/归档折叠为计数。
        let mut index = format!("# Memory Index ({})\n\n", self.scope.label());
        let mut stale = 0usize;
        for (_, e) in &entries {
            if e.status == "active" {
                index.push_str(&format!(
                    "- {} [{}] {} — {}\n",
                    e.id, e.category, e.title, e.description
                ));
            } else {
                stale += 1;
            }
        }
        if stale > 0 {
            index.push_str(&format!("\n({stale} stale 条待归档)\n"));
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
                 last_hit_at INTEGER NOT NULL DEFAULT 0);",
        )?;
        Ok(conn)
    }

    /// FTS 检索:bm25 取 topN 后按 log(1+hits) 与 active 加权在 Rust 侧重排。
    /// 命中即计 hits(强化循环:常用记忆自然浮上来)。
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
            let hit_count: u64 = conn
                .query_row(
                    "SELECT hits FROM memory_hits WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            // bm25 越小越相关(fts5 返回负值);取负得正相关度。
            let mut score = -bm25;
            score *= 1.0 + (1.0 + hit_count as f64).ln() * 0.2;
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
        hits_out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
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
    ) -> anyhow::Result<MemoryEntry> {
        if duplicates.is_empty() {
            anyhow::bail!("merge needs at least one duplicate id");
        }
        if duplicates.iter().any(|d| d == primary) {
            anyhow::bail!("primary id cannot appear in duplicates");
        }
        let entries = self.load_all();
        for id in std::iter::once(&primary.to_string()).chain(duplicates.iter()) {
            if !entries.iter().any(|(_, e)| &e.id == id) {
                anyhow::bail!("unknown memory id `{id}`");
            }
        }
        let merged = self.update(primary, title, description, body, None)?;
        for id in duplicates {
            let (path, mut entry) = self
                .load_all()
                .into_iter()
                .find(|(_, e)| &e.id == id)
                .expect("checked above");
            entry.status = "stale".into();
            entry.updated = today();
            entry
                .extras
                .retain(|(k, _)| !k.eq_ignore_ascii_case("superseded_by"));
            entry.extras.push(("superseded_by".into(), primary.to_string()));
            self.write_entry(&entry, Some(&path))?;
        }
        self.refresh_derived()?;
        Ok(merged)
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
    pub fn append_note(&self, summary: &str, detail: &str, category_hint: &str) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join("inbox.md");
        let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| "# Memory Inbox\n".into());
        text.push_str(&format!(
            "\n## note {} {}\n- summary: {}\n{}{}",
            today(),
            if category_hint.is_empty() { "".to_string() } else { format!("[{category_hint}]") },
            summary.trim(),
            if detail.trim().is_empty() { String::new() } else { format!("{}\n", detail.trim()) },
            "",
        ));
        atomic_write(&path, &text)?;
        Ok(path)
    }

    pub fn pending_notes(&self) -> usize {
        std::fs::read_to_string(self.root.join("inbox.md"))
            .map(|t| t.lines().filter(|l| l.starts_with("## note ")).count())
            .unwrap_or(0)
    }

    /// legacy 迁移:R-098 的 .kanzei/project/memory.md(tracker M-条目)→ 一条一文件。
    /// 幂等:legacy 文件不存在即跳过;迁移后原文件改写为指路牌。
    fn migrate_legacy(&self, project_root: &Path) {
        let legacy = project_root.join(".kanzei").join("project").join("memory.md");
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
                status: if legacy_entry.status == "stale" { "stale" } else { "active" }.into(),
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

/// FTS5 MATCH 表达式:每个用户词 → 引号短语(内部 CJK 已单字切分),防语法注入。
fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|t| segment_cjk(&t.replace('"', "")))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
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

    fn add(store: &MemoryStore, category: &str, title: &str, desc: &str, body: &str) -> MemoryEntry {
        match store.add(category, title, desc, body, "user", false).unwrap() {
            AddOutcome::Added(e) => e,
            AddOutcome::Duplicate(e) => panic!("unexpected duplicate of {}", e.id),
        }
    }

    #[test]
    fn add_assigns_ids_and_builds_derived_index() {
        let (dir, store) = temp_store();
        let a = add(&store, "fact", "CRLF 是 edit 未命中主因", "换行符问题必读", "正文 A");
        let b = add(&store, "sop", "发版 SOP", "做发版相关任务必读", "1. 测试 2. 推送 3. 发布");
        assert_eq!((a.id.as_str(), b.id.as_str()), ("M-001", "M-002"));
        let index = std::fs::read_to_string(store.root.join("INDEX.md")).unwrap();
        assert!(index.contains("M-001 [fact] CRLF 是 edit 未命中主因 — 换行符问题必读"));
        assert!(index.contains("M-002 [sop]"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn exact_duplicate_title_is_rejected_unless_forced() {
        let (dir, store) = temp_store();
        add(&store, "habit", "gh 要走本地代理", "gh 网络失败时必读", "HTTPS_PROXY=127.0.0.1:12000");
        let outcome = store
            .add("habit", "gh 要走本地代理!", "重复", "x", "user", false)
            .unwrap();
        assert!(matches!(outcome, AddOutcome::Duplicate(ref e) if e.id == "U-001" || e.id == "M-001"));
        let forced = store
            .add("habit", "gh 要走本地代理!", "强制新增", "x", "user", true)
            .unwrap();
        assert!(matches!(forced, AddOutcome::Added(_)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn search_ranks_and_records_hits_and_rebuilds_after_db_loss() {
        let (dir, store) = temp_store();
        add(&store, "fact", "CRLF 是 edit 未命中主因", "处理 edit 替换失败换行符问题必读", "自动容忍已落地");
        add(&store, "sop", "发版 SOP 两条通道", "发版发布安装更新相关必读", "package.ps1 -Publish 后静默装 setup");
        let hits = store.search("发版 更新", None, Some("active"), 5).unwrap();
        assert_eq!(hits[0].entry.id, "M-002", "{:?}", hits.iter().map(|h| &h.entry.id).collect::<Vec<_>>());
        assert!(hits[0].snippet.contains('['), "snippet 高亮: {}", hits[0].snippet);
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
    fn update_evolves_and_stale_downranks() {
        let (dir, store) = temp_store();
        let e = add(&store, "fact", "旧结论", "某场景必读", "V1");
        let updated = store
            .update(&e.id, None, None, Some("V2 修订"), Some("stale"))
            .unwrap();
        assert_eq!(updated.status, "stale");
        assert_eq!(updated.body, "V2 修订");
        assert_eq!(updated.created, e.created);
        // stale 默认不出现在 active 过滤下
        let active_only = store.search("结论", None, Some("active"), 5).unwrap();
        assert!(active_only.is_empty());
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
        let text = std::fs::read_to_string(&path).unwrap().replace("id: M-002", "id: M-003");
        std::fs::write(&path, text).unwrap();
        let issues = store.integrity_issues();
        assert!(issues.iter().any(|i| i.contains("M-002")), "{issues:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn merge_keeps_primary_and_tombstones_duplicates() {
        let (dir, store) = temp_store();
        let a = add(&store, "habit", "gh 走代理", "gh 网络问题必读", "端口 12000");
        let b = add(&store, "habit", "gh 需要 HTTPS_PROXY", "gh 超时必读", "同上");
        let merged = store
            .merge(&a.id, &[b.id.clone()], None, Some("gh 网络失败/超时必读"), Some("HTTPS_PROXY=http://127.0.0.1:12000"))
            .unwrap();
        assert_eq!(merged.id, a.id);
        assert_eq!(merged.description, "gh 网络失败/超时必读");
        let entries = store.load_all();
        let (_, dup) = entries.iter().find(|(_, e)| e.id == b.id).unwrap();
        assert_eq!(dup.status, "stale");
        assert!(dup.extras.iter().any(|(k, v)| k == "superseded_by" && v == &a.id));
        // 未知 id 与自我合并都拒绝
        assert!(store.merge(&a.id, &[a.id.clone()], None, None, None).is_err());
        assert!(store.merge("M-999", &[b.id.clone()], None, None, None).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn inbox_roundtrip_and_clear() {
        let (dir, store) = temp_store();
        store.append_note("纯 ui 改动只跑 node 检查", "细节", "habit").unwrap();
        store.append_note("发版走两条通道", "", "sop").unwrap();
        assert_eq!(store.pending_notes(), 2);
        assert!(store.read_inbox().contains("纯 ui 改动只跑 node 检查"));
        store.clear_inbox().unwrap();
        assert_eq!(store.pending_notes(), 0);
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
        assert_eq!(entries.len(), 2);
        let m1 = entries.iter().find(|(_, e)| e.id == "M-001").unwrap();
        assert_eq!(m1.1.category, "fact");
        assert_eq!(m1.1.source, "migration");
        assert!(m1.1.body.contains("依据: 实测"));
        let m2 = entries.iter().find(|(_, e)| e.id == "M-002").unwrap();
        assert_eq!(m2.1.status, "stale");
        // 原文件变为指路牌,重复 open 不再迁移
        let legacy = std::fs::read_to_string(dir.join(".kanzei/project/memory.md")).unwrap();
        assert!(legacy.contains("已迁移"));
        let again = MemoryStore::project(&dir);
        assert_eq!(again.load_all().len(), 2);
        std::fs::remove_dir_all(dir).ok();
    }
}
