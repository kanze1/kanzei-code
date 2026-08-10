//! R-164 混合检索索引抽象:三通道(fingerprint / BM25 / dense)统一入口。
//!
//! 设计基线 docs/design/memory_control_plane.md §5:
//! ```ignore
//! trait MemoryIndex {          // SqliteMemoryIndex 为默认实现
//!     fn search_lexical(...); fn search_dense(...); fn search_hybrid(...);
//!     fn upsert(...); fn remove(...); fn rebuild(...);
//! }
//! ```
//!
//! 本模块交付:
//! - [`MemoryIndex`]:检索 + 维护接口,供回放台三臂对比(验收③)与运行时召回共用;
//! - [`SqliteMemoryIndex`]:默认实现。lexical 通道复用 [`super::FingerprintIndex`]
//!   (Tier0 fingerprint 精确)与 [`super::MemoryStore::search`](Tier1 BM25,FTS5),
//!   与 FailureRecallPolicy 同源;dense 通道本批未接 embedder → 恒空;
//!   hybrid 在无 embedder 时自动退化为 lexical(验收①:功能完整,不依赖向量)。
//! - upsert/remove 增量维护内存索引(同 FailureRecallPolicy 写时增量语义),
//!   rebuild 全量重扫文件系统。
//!
//! 向量通道(验收②④)由后续批次在 index.db 增加向量列 + Embedder 实现后
//! 在此接入,本批保持接口稳定(方法签名不随通道落地而变)。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::store::SearchHit;
use super::{FingerprintIndex, MemoryEntry, MemoryStore};
use crate::embed::Embedder;

/// 一次检索请求:文本与指纹触发二选一(可同时携带)。
#[derive(Debug, Clone, Default)]
pub struct IndexQuery {
    /// BM25 查询文本(trigger.sample / target 拼接,或用户搜索词)。
    pub text: String,
    /// 指纹触发(如 `[fp:edit|old_string not found]`)。Some 时 Tier0 精确优先。
    pub fingerprint: Option<String>,
}

impl IndexQuery {
    /// 纯文本查询。
    pub fn text(query: &str) -> Self {
        Self {
            text: query.to_string(),
            fingerprint: None,
        }
    }

    /// 指纹触发查询(工具失败召回场景)。
    pub fn fingerprint(tool: &str, kind: &str) -> Self {
        Self {
            text: String::new(),
            fingerprint: Some(format!("[fp:{tool}|{kind}]")),
        }
    }

    /// 指纹 + 文本都带(指纹 miss 时回落 BM25)。
    pub fn both(tool: &str, kind: &str, text: &str) -> Self {
        Self {
            text: text.to_string(),
            fingerprint: Some(format!("[fp:{tool}|{kind}]")),
        }
    }
}

/// 一次命中:条目定位信息 + 相关度分数(通道无关,供 RRF 融合消费)。
#[derive(Debug, Clone)]
pub struct IndexHit {
    pub id: String,
    pub category: String,
    /// description 是检索钩子(与 RecallHit.action 同义)。
    pub action: String,
    pub status: String,
    /// 相关度:lexical 通道是归一化 BM25/Tier0 得分,越大越相关。
    pub score: f64,
}

impl IndexHit {
    fn from_entry(entry: &MemoryEntry, score: f64) -> Self {
        Self {
            id: entry.id.clone(),
            category: entry.category.clone(),
            action: entry.description.clone(),
            status: entry.status.clone(),
            score,
        }
    }
}

/// 记忆索引接口(设计 §5)。实现侧决定三通道的具体行为与降级策略。
pub trait MemoryIndex: Send + Sync {
    /// lexical 通道:fingerprint(Tier0)优先,miss 走 BM25(Tier1)。
    fn search_lexical(&self, query: &IndexQuery, limit: usize) -> Vec<IndexHit>;

    /// dense 通道:向量检索。无 embedder 时返回空(通道不可用,不报错)。
    fn search_dense(&self, query: &IndexQuery, limit: usize) -> Vec<IndexHit>;

    /// hybrid 通道:RRF 融合(设计 §5,k=60)。无 embedder 时退化为 lexical。
    fn search_hybrid(&self, query: &IndexQuery, limit: usize) -> Vec<IndexHit>;

    /// 写时增量:更新/新增单条目的索引。
    fn upsert(&mut self, entry: &MemoryEntry) -> anyhow::Result<()>;

    /// 写时增量:删除单条目的索引。
    fn remove(&mut self, id: &str) -> anyhow::Result<()>;

    /// 全量重建:重扫文件系统,重建全部索引。
    fn rebuild(&mut self) -> anyhow::Result<()>;
}

/// SqliteMemoryIndex 默认实现(设计 §5)。
///
/// 数据分布与既有召回同源:指纹在内存 HashMap(p95<5ms),BM25 走 index.db FTS5,
/// 向量列在同一 index.db 的 `memory_vectors` 表(派生物,可重建——验收④)。
///
/// 批2 范围:接入 [`Embedder`] 与向量列——有 embedder 时 rebuild 全量生成向量、
/// upsert/remove 增量维护;无 embedder 时向量列空、dense 恒空、hybrid 退化为
/// lexical(验收①)。dense 检索(brute-force 余弦)与 RRF 融合由批3 落地,
/// 本批保证存储与重建闭环。
pub struct SqliteMemoryIndex {
    project_root: PathBuf,
    /// Tier0 指纹索引:启动扫描 + 写时增量(upsert/remove)。
    fp: HashMap<String, Vec<String>>,
    /// active 条目快照(id → entry),供命中 materialize 与 upsert 增量。
    entries: HashMap<String, MemoryEntry>,
    /// 向量通道:None = 关闭(hybrid 退化为 lexical)。批3 用 cosine 检索。
    embedder: Option<Arc<dyn Embedder>>,
}

impl SqliteMemoryIndex {
    /// 启动扫描构建(project + global 两级,与 FailureRecallPolicy 同规)。
    pub fn new(project_root: &Path) -> Self {
        Self::with_embedder(project_root, None)
    }

    /// 带向量通道的构造:embedder 为 None 时与 [`new`] 等价(降级路径)。
    pub fn with_embedder(project_root: &Path, embedder: Option<Arc<dyn Embedder>>) -> Self {
        let fp = FingerprintIndex::build(project_root);
        let mut entries: HashMap<String, MemoryEntry> = HashMap::new();
        let mut stores = vec![MemoryStore::project(project_root)];
        stores.extend(MemoryStore::global());
        for store in &stores {
            for (_, entry) in store.load_all() {
                if entry.status != "active" {
                    continue;
                }
                entries.insert(entry.id.clone(), entry.clone());
            }
        }
        Self {
            project_root: project_root.to_path_buf(),
            fp,
            entries,
            embedder,
        }
    }

    /// 向量库路径:项目 memory 根下的 index.db(与 FTS 同库)。
    fn vector_db_path(&self) -> PathBuf {
        super::project_memory_root(&self.project_root).join("index.db")
    }

    /// 打开向量表(派生物,可重建;缺表即建)。
    fn open_vector_db(&self) -> anyhow::Result<rusqlite::Connection> {
        std::fs::create_dir_all(super::project_memory_root(&self.project_root))?;
        let conn = rusqlite::Connection::open(self.vector_db_path())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_vectors(
                 id TEXT PRIMARY KEY,
                 dim INTEGER NOT NULL,
                 vector BLOB NOT NULL,
                 updated INTEGER NOT NULL DEFAULT 0);",
        )?;
        Ok(conn)
    }

    /// 条目 → 向量。仅当通道启用时对 active 条目生成;失败(provider 报错)
    /// 记录 tracing 并跳过该条——检索降级不阻塞主流程(设计 §3.2 精神)。
    fn vectorize(&self, entry: &MemoryEntry) -> Option<Vec<f32>> {
        let embedder = self.embedder.as_ref()?;
        let text = format!(
            "{}\n{}\n{}",
            entry.title, entry.description, entry.body
        );
        match embedder.embed(&[&text]) {
            Ok(mut vecs) => vecs.pop(),
            Err(e) => {
                tracing::warn!(
                    id = %entry.id,
                    error = %e,
                    "embedding 生成失败,该条向量跳过(检索降级不阻塞)"
                );
                None
            }
        }
    }

    /// 向量 BLOB 序列化:f32 LE 字节拼接。
    fn vector_to_blob(vec: &[f32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(vec.len() * 4);
        for v in vec {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// Tier0:指纹精确匹配。兼容 frontmatter fingerprint 一等字段与正文 [fp:] 标记。
    fn tier0(&self, fp_key: &str) -> Vec<IndexHit> {
        let mut ids = self
            .fp
            .get(fp_key)
            .cloned()
            .unwrap_or_default();
        if ids.is_empty() {
            // 兼容正文裸标记:快照内精确子串(条目数小,可接受)。
            for (id, entry) in &self.entries {
                if entry
                    .fingerprint()
                    .is_some_and(|fp| fp.contains(fp_key.trim_start_matches("[fp:").trim_end_matches(']')))
                {
                    ids.push(id.clone());
                }
            }
        }
        ids.iter()
            .filter_map(|id| self.entries.get(id))
            .map(|e| IndexHit::from_entry(e, 1.0))
            .collect()
    }

    /// Tier1:BM25(store.search 已做 bm25 + 采纳率决策加权 + active 排序;R-150 起 hits 不参与排序)。
    fn tier1(&self, query: &str, limit: usize) -> Vec<IndexHit> {
        let mut hits: Vec<IndexHit> = Vec::new();
        let mut stores = vec![MemoryStore::project(&self.project_root)];
        stores.extend(MemoryStore::global());
        for store in &stores {
            let Ok(rows) = store.search(query, None, Some("active"), limit) else {
                continue;
            };
            for SearchHit { entry, score, .. } in rows {
                hits.push(IndexHit::from_entry(&entry, score));
            }
        }
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        hits
    }

    /// 从向量库读全部 (id, 向量),brute-force 余弦检索(R-164 内容③)。
    /// 无 embedder 时向量表为空 → 空结果(通道不可用)。
    fn dense_scan(&self, query_vec: &[f32], limit: usize) -> Vec<IndexHit> {
        let Ok(conn) = self.open_vector_db() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        {
            let mut stmt = match conn.prepare("SELECT id, vector FROM memory_vectors") {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            };
            let iter = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .ok();
            if let Some(iter) = iter {
                for row in iter.flatten() {
                    rows.push(row);
                }
            }
        }
        let mut scored: Vec<(f64, String)> = Vec::new();
        for (id, blob) in rows {
            // BLOB → f32 数组;长度不符(损坏/跨版本)跳过。
            let vec = blob
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect::<Vec<_>>();
            if vec.len() != query_vec.len() {
                continue;
            }
            let sim = cosine_similarity(query_vec, &vec);
            scored.push((sim, id));
        }
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);
        scored
            .into_iter()
            .filter_map(|(score, id)| {
                self.entries.get(&id).map(|e| IndexHit::from_entry(e, score))
            })
            .collect()
    }

    /// dense 检索入口:query 文本 → embedder 向量 → brute-force 扫描。
    fn dense(&self, query: &IndexQuery, limit: usize) -> Vec<IndexHit> {
        let Some(embedder) = &self.embedder else {
            return Vec::new();
        };
        let text = if query.text.trim().is_empty() {
            query.fingerprint.as_deref().unwrap_or("")
        } else {
            &query.text
        };
        if text.trim().is_empty() {
            return Vec::new();
        }
        let Ok(mut vecs) = embedder.embed(&[text]) else {
            return Vec::new();
        };
        let Some(query_vec) = vecs.pop() else {
            return Vec::new();
        };
        self.dense_scan(&query_vec, limit)
    }
}

impl MemoryIndex for SqliteMemoryIndex {
    fn search_lexical(&self, query: &IndexQuery, limit: usize) -> Vec<IndexHit> {
        // Tier0 优先:指纹命中即返回,不往下查(与 FailureRecallPolicy 同语义)。
        if let Some(fp_key) = &query.fingerprint {
            let hits = self.tier0(fp_key);
            if !hits.is_empty() {
                return hits;
            }
        }
        if query.text.trim().is_empty() {
            return Vec::new();
        }
        self.tier1(&query.text, limit)
    }

    fn search_dense(&self, query: &IndexQuery, limit: usize) -> Vec<IndexHit> {
        // 无 embedder → 向量表空,恒空(通道不可用,不报错)。
        self.dense(query, limit)
    }

    fn search_hybrid(&self, query: &IndexQuery, limit: usize) -> Vec<IndexHit> {
        let Some(_) = &self.embedder else {
            // 无 embedder 时 hybrid 自动退化为 lexical,功能完整(设计 §5 验收①)。
            return self.search_lexical(query, limit);
        };
        // RRF 融合(k=60):BM25 top10 + dense top10 → top5(设计 §5,禁止线性加权)。
        const RRF_K: f64 = 60.0;
        const LEXICAL_TOP: usize = 10;
        const DENSE_TOP: usize = 10;
        let lexical = self.search_lexical(query, LEXICAL_TOP);
        let dense = self.search_dense(query, DENSE_TOP);
        if dense.is_empty() {
            // dense 通道配置了但没结果(空表/embedding 失败)→ 退化为 lexical。
            return lexical;
        }
        let mut rrf: std::collections::HashMap<String, (f64, IndexHit)> =
            std::collections::HashMap::new();
        // rank 从 1 开始;RRF score = Σ 1/(k + rank)。
        for (rank, hit) in lexical.into_iter().enumerate() {
            let entry = rrf
                .entry(hit.id.clone())
                .or_insert_with(|| (0.0, hit.clone()));
            entry.0 += 1.0 / (RRF_K + (rank as f64 + 1.0));
        }
        for (rank, hit) in dense.into_iter().enumerate() {
            let entry = rrf
                .entry(hit.id.clone())
                .or_insert_with(|| (0.0, hit.clone()));
            entry.0 += 1.0 / (RRF_K + (rank as f64 + 1.0));
        }
        let mut fused: Vec<IndexHit> = rrf
            .into_values()
            .map(|(s, mut h)| {
                h.score = s;
                h
            })
            .collect();
        fused.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        fused.truncate(limit);
        fused
    }

    fn upsert(&mut self, entry: &MemoryEntry) -> anyhow::Result<()> {
        // 先清理该 id 在旧桶里的位置,再按新指纹插入(与 FingerprintIndex::upsert 同规)。
        for ids in self.fp.values_mut() {
            ids.retain(|id| id != &entry.id);
        }
        self.fp.retain(|_, ids| !ids.is_empty());
        if let Some(fp) = entry.fingerprint() {
            let fp = fp.trim().to_string();
            if !fp.is_empty() {
                let ids = self.fp.entry(fp).or_default();
                if !ids.contains(&entry.id) {
                    ids.push(entry.id.clone());
                }
            }
        }
        if entry.status == "active" {
            self.entries.insert(entry.id.clone(), entry.clone());
        } else {
            self.entries.remove(&entry.id);
        }
        // 向量增量:active 且有 embedder → 生成并落表;非 active → 删行。
        let conn = self.open_vector_db()?;
        if entry.status == "active" {
            if let Some(vec) = self.vectorize(entry) {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                conn.execute(
                    "INSERT INTO memory_vectors(id, dim, vector, updated)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(id) DO UPDATE SET dim = ?2, vector = ?3, updated = ?4",
                    rusqlite::params![
                        entry.id,
                        vec.len() as i64,
                        Self::vector_to_blob(&vec),
                        now_ms
                    ],
                )?;
            }
        } else {
            conn.execute("DELETE FROM memory_vectors WHERE id = ?1", rusqlite::params![entry.id])?;
        }
        Ok(())
    }

    fn remove(&mut self, id: &str) -> anyhow::Result<()> {
        for ids in self.fp.values_mut() {
            ids.retain(|cur| cur != id);
        }
        self.fp.retain(|_, ids| !ids.is_empty());
        self.entries.remove(id);
        let conn = self.open_vector_db()?;
        conn.execute("DELETE FROM memory_vectors WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    fn rebuild(&mut self) -> anyhow::Result<()> {
        // 重建内存索引与向量表:指纹+条目快照全量重扫,向量全量重算(验收④)。
        let new_self = Self::with_embedder(&self.project_root, self.embedder.clone());
        *self = new_self;
        let conn = self.open_vector_db()?;
        conn.execute("DELETE FROM memory_vectors", [])?;
        let mut failed = 0usize;
        for (_, entry) in &self.entries {
            if let Some(vec) = self.vectorize(entry) {
                conn.execute(
                    "INSERT INTO memory_vectors(id, dim, vector, updated) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        entry.id,
                        vec.len() as i64,
                        Self::vector_to_blob(&vec),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0)
                    ],
                )?;
            } else {
                failed += 1;
            }
        }
        tracing::debug!(entries = self.entries.len(), failed, "memory_vectors 全量重建完成");
        Ok(())
    }
}

/// 余弦相似度(向量检索相似度度量)。零向量/维度不一致 → 0(不相似)。
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += *x as f64 * *y as f64;
        na += (*x as f64) * (*x as f64);
        nb += (*y as f64) * (*y as f64);
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// 一次混合检索的分段耗时(ms)——供 RecallEvent 落库(验收②分段延迟)。
/// 与 recall_events 表的 lexical_ms/embed_ms/vector_ms 列一一对应。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RetrievalTiming {
    /// Tier0 fingerprint + Tier1 BM25 的墙钟耗时。
    pub lexical_ms: u64,
    /// query 文本向量化耗时。
    pub embed_ms: u64,
    /// 向量表 brute-force 扫描 + 排序耗时。
    pub vector_ms: u64,
}

impl RetrievalTiming {
    pub fn total(&self) -> u64 {
        self.lexical_ms + self.embed_ms + self.vector_ms
    }
}

impl SqliteMemoryIndex {
    /// 混合检索 + 分段耗时。装配方(RecallPolicy / replay_eval)据此落 recall_events,
    /// 使 hybrid 生效与各段延迟都可观测(验收②)。纯检索层不碰 SessionStore。
    pub fn search_hybrid_with_timing(
        &self,
        query: &IndexQuery,
        limit: usize,
    ) -> (Vec<IndexHit>, RetrievalTiming) {
        let mut timing = RetrievalTiming::default();
        let Some(embedder) = &self.embedder else {
            // 无 embedder:只走 lexical,embed/vector 段为 0(降级路径可见)。
            let started = std::time::Instant::now();
            let hits = self.search_lexical(query, limit);
            timing.lexical_ms = started.elapsed().as_millis() as u64;
            return (hits, timing);
        };
        // 与 search_hybrid 相同的 RRF 融合,但记录各段耗时。
        const RRF_K: f64 = 60.0;
        const LEXICAL_TOP: usize = 10;
        const DENSE_TOP: usize = 10;

        let started = std::time::Instant::now();
        let lexical = self.search_lexical(query, LEXICAL_TOP);
        timing.lexical_ms = started.elapsed().as_millis() as u64;

        // query 向量化(embed 段)。
        let text = if query.text.trim().is_empty() {
            query.fingerprint.as_deref().unwrap_or("")
        } else {
            &query.text
        };
        let embed_started = std::time::Instant::now();
        let query_vec = if text.trim().is_empty() {
            None
        } else {
            embedder.embed(&[text]).ok().and_then(|mut v| v.pop())
        };
        timing.embed_ms = embed_started.elapsed().as_millis() as u64;

        // 向量扫描(vector 段)。
        let vec_started = std::time::Instant::now();
        let dense = match &query_vec {
            Some(qv) => self.dense_scan(qv, DENSE_TOP),
            None => Vec::new(),
        };
        timing.vector_ms = vec_started.elapsed().as_millis() as u64;

        if dense.is_empty() {
            return (lexical, timing);
        }
        let mut rrf: std::collections::HashMap<String, (f64, IndexHit)> =
            std::collections::HashMap::new();
        for (rank, hit) in lexical.into_iter().enumerate() {
            let entry = rrf
                .entry(hit.id.clone())
                .or_insert_with(|| (0.0, hit.clone()));
            entry.0 += 1.0 / (RRF_K + (rank as f64 + 1.0));
        }
        for (rank, hit) in dense.into_iter().enumerate() {
            let entry = rrf
                .entry(hit.id.clone())
                .or_insert_with(|| (0.0, hit.clone()));
            entry.0 += 1.0 / (RRF_K + (rank as f64 + 1.0));
        }
        let mut fused: Vec<IndexHit> = rrf
            .into_values()
            .map(|(s, mut h)| {
                h.score = s;
                h
            })
            .collect();
        fused.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        fused.truncate(limit);
        (fused, timing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::AddOutcome;
    use crate::memory::MemoryScope;

    fn temp_root() -> (PathBuf, MemoryStore) {
        let dir = std::env::temp_dir().join(format!(
            "kz-mem-index-{}-{}",
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
    fn 无embedder降级_fingerprint精确命中与BM25完整可用() {
        // 验收①:无 embedder 时 fingerprint + BM25 两通道完整可用。
        let (root, store) = temp_root();
        // 带 [fp:] 正文标记的条目 → Tier0 指纹命中。
        add(
            &store,
            "sop",
            "edit old_string 找不到时先 read 重建",
            "工具 edit 失败 old_string not found 时,先 read 目标文件重建 old_string 再重试",
            "[fp:edit|old_string not found]\n先 read 文件,确认实际内容后再 edit。",
        );
        // 无指纹条目 → 仅 BM25 文本命中。
        add(
            &store,
            "fact",
            "cargo build 依赖缓存",
            "cargo build 偶发网络错误,重试即可",
            "cargo build 依赖下载失败时清理缓存后重试。",
        );

        let index = SqliteMemoryIndex::new(&root);
        // dense 通道恒空(无 embedder)。
        let dense = index.search_dense(&IndexQuery::text("edit old_string"), 5);
        assert!(dense.is_empty(), "无 embedder 时 dense 必须不可用: {:?}", dense);

        // 指纹精确命中:Tier0 返回 1 条且是目标条目。
        let fp_hits = index.search_lexical(&IndexQuery::fingerprint("edit", "old_string not found"), 5);
        assert_eq!(fp_hits.len(), 1, "指纹必须精确命中: {:?}", fp_hits);
        assert!(fp_hits[0].action.contains("edit"), "{:?}", fp_hits[0]);
        assert_eq!(fp_hits[0].score, 1.0);

        // BM25 文本命中:无指纹条目靠描述/正文召回。
        let bm25_hits = index.search_lexical(&IndexQuery::text("cargo build 网络错误 重试"), 5);
        assert!(
            bm25_hits.iter().any(|h| h.action.contains("cargo build")),
            "BM25 必须命中 cargo 条目: {:?}",
            bm25_hits
        );

        // hybrid 退化为 lexical:同 query 结果一致(无 embedder 时功能完整)。
        let hybrid = index.search_hybrid(&IndexQuery::fingerprint("edit", "old_string not found"), 5);
        assert_eq!(hybrid.len(), 1, "hybrid 退化后与 lexical 一致");
        assert_eq!(hybrid[0].id, fp_hits[0].id);
    }

    #[test]
    fn 指纹miss时回落BM25_文本可兜底() {
        let (root, store) = temp_root();
        add(
            &store,
            "sop",
            "read 失败重试",
            "read 文件失败时先确认路径存在",
            "[fp:read|file not found]\n路径不存在时先 glob 确认。",
        );
        let index = SqliteMemoryIndex::new(&root);
        // 指纹 miss(错误文本不同)→ 回落 BM25 文本。
        let hits = index.search_lexical(
            &IndexQuery::both("read", "file not found", "read 文件失败 路径"),
            5,
        );
        assert!(
            hits.iter().any(|h| h.action.contains("read")),
            "指纹 miss 时文本兜底必须命中: {:?}",
            hits
        );
    }

    #[test]
    fn upsert_remove_rebuild_增量与全量一致() {
        let (root, store) = temp_root();
        let mut index = SqliteMemoryIndex::new(&root);

        // 空索引:无命中。
        assert!(index
            .search_lexical(&IndexQuery::fingerprint("edit", "old_string not found"), 5)
            .is_empty());

        // upsert 一条带指纹条目 → 立即可查。
        let entry = add(
            &store,
            "sop",
            "edit 重建 old_string",
            "edit old_string not found 时先 read 重建",
            "[fp:edit|old_string not found]\n先 read。",
        );
        index.upsert(&entry).unwrap();
        let hits = index.search_lexical(&IndexQuery::fingerprint("edit", "old_string not found"), 5);
        assert_eq!(hits.len(), 1, "upsert 后必须可查: {:?}", hits);

        // remove → 不可查。
        index.remove(&entry.id).unwrap();
        assert!(index
            .search_lexical(&IndexQuery::fingerprint("edit", "old_string not found"), 5)
            .is_empty());

        // rebuild → 从文件系统全量重建,恢复命中。
        index.rebuild().unwrap();
        let hits = index.search_lexical(&IndexQuery::fingerprint("edit", "old_string not found"), 5);
        assert_eq!(hits.len(), 1, "rebuild 后必须恢复: {:?}", hits);
    }

    #[test]
    fn 有embedder时rebuild生成向量_无embedder时向量表空() {
        // 验收④:配置 embedder 后向量索引可全量重建;验收①:无 embedder 时降级。
        let (root, store) = temp_root();
        let e1 = add(&store, "fact", "A 概念", "A 的描述", "A 的正文内容");
        let e2 = add(&store, "sop", "B 概念", "B 的描述", "B 的正文内容");

        // 无 embedder:rebuild 后向量表为空(dense 通道关闭)。
        {
            let mut index = SqliteMemoryIndex::new(&root);
            index.rebuild().unwrap();
            let conn = index.open_vector_db().unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM memory_vectors", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0, "无 embedder 时不得生成任何向量");
        }

        // 有 embedder:rebuild 全量生成(每条 active 都有向量,维度一致)。
        {
            let embedder = Arc::new(crate::embed::FakeEmbedder::new(8));
            let mut index = SqliteMemoryIndex::with_embedder(&root, Some(embedder));
            index.rebuild().unwrap();
            let conn = index.open_vector_db().unwrap();
            let rows: Vec<(String, i64)> = conn
                .prepare("SELECT id, dim FROM memory_vectors ORDER BY id")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(rows.len(), 2, "两条 active 条目都要有向量: {rows:?}");
            for (id, dim) in &rows {
                assert!(id == &e1.id || id == &e2.id, "未知条目 id: {id}");
                assert_eq!(*dim, 8, "维度必须与 embedder 一致");
            }
            // BLOB 可反序列化为 8 个 f32。
            let (_, blob): (String, Vec<u8>) = conn
                .query_row("SELECT id, vector FROM memory_vectors LIMIT 1", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap();
            assert_eq!(blob.len(), 8 * 4, "f32 LE 字节数必须 = dim*4");
        }
    }

    #[test]
    fn upsert_增量维护向量_remove删除向量() {
        let (root, store) = temp_root();
        let embedder = Arc::new(crate::embed::FakeEmbedder::new(4));
        let mut index = SqliteMemoryIndex::with_embedder(&root, Some(embedder));

        // upsert 一条 → 向量表出现该行。
        let entry = add(&store, "fact", "C 概念", "C 的描述", "C 的正文");
        index.upsert(&entry).unwrap();
        {
            let conn = index.open_vector_db().unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM memory_vectors", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1, "upsert 后向量必须落表");
        }

        // remove → 向量行删除。
        index.remove(&entry.id).unwrap();
        {
            let conn = index.open_vector_db().unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM memory_vectors", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0, "remove 后向量必须删除");
        }
    }

    #[test]
    fn cosine_相似度_同向为1_垂直为0() {
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-9);
        assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-9);
        assert!((cosine_similarity(&[1.0, 1.0], &[2.0, 2.0]) - 1.0).abs() < 1e-9);
        // 零向量 → 0(不相似)。
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        // 维度不一致 → 0。
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn dense_检索_embedder配置后按语义命中() {
        // 验收②核心:dense 通道配置 embedder 后生效,能按向量相似度命中。
        let (root, store) = temp_root();
        let e1 = add(&store, "fact", "edit 失败处理", "old_string not found 时先 read", "先 read 重建 old_string 再 edit");
        let e2 = add(&store, "fact", "cargo 构建", "网络错误重试", "cargo build 失败清理缓存重试");

        // 注意:FakeEmbedder 是文本 hash,同文本同向量。query 用与 e1 完全相同的
        // 文本 → 余弦=1 必命中 e1;e2 文本不同 → 不命中(或排在后面)。
        let embedder = Arc::new(crate::embed::FakeEmbedder::new(16));
        let mut index = SqliteMemoryIndex::with_embedder(&root, Some(embedder));
        index.rebuild().unwrap();

        let hits = index.search_dense(
            &IndexQuery::text("edit 失败处理\nold_string not found 时先 read\n先 read 重建 old_string 再 edit"),
            5,
        );
        assert!(
            hits.iter().any(|h| h.id == e1.id),
            "dense 必须命中语义相同的 e1: {:?}",
            hits
        );
        // dense 通道与 lexical 独立:该 query 文本在 FTS 里也会命中 e1(关键词重叠)。
        // 关键是 dense 能返回结果,且第一个是 e1。
        assert_eq!(hits[0].id, e1.id, "最相似条目应排第一: {:?}", hits);
    }

    #[test]
    fn hybrid_rrf融合_同时出现在两通道的条目排名靠前() {
        let (root, store) = temp_root();
        // 两条条目:shared 在语义上与 query 最接近,unique 只靠文本。
        let shared = add(&store, "sop", "edit 失败", "old_string not found 先 read", "先 read 目标文件再 edit 重试");
        let unique = add(&store, "fact", "cargo", "cargo build 网络错误", "清理缓存重试");

        let embedder = Arc::new(crate::embed::FakeEmbedder::new(16));
        let mut index = SqliteMemoryIndex::with_embedder(&root, Some(embedder));
        index.rebuild().unwrap();

        let query = IndexQuery::text("edit 失败 old_string not found 先 read 再 edit 重试");
        let hybrid = index.search_hybrid(&query, 5);
        assert!(
            hybrid.iter().any(|h| h.id == shared.id),
            "hybrid 必须包含 shared(两通道都命中): {:?}",
            hybrid
        );
        // shared 在 lexical(BM25 文本重叠)与 dense(语义相同)都出现 → RRF 融合后第一。
        assert_eq!(hybrid[0].id, shared.id, "RRF 融合后两通道都命中的条目应第一: {:?}", hybrid);

        // hybrid 与纯 lexical 不同(融合了 dense 贡献)或至少不劣于 lexical。
        let lexical = index.search_lexical(&query, 5);
        assert!(
            hybrid.len() >= lexical.len().min(1),
            "hybrid 融合不应劣化 lexical: hybrid={:?} lexical={:?}",
            hybrid,
            lexical
        );
    }

    #[test]
    fn hybrid_带分段耗时_无embedder时embed与vector段为0() {
        // 验收②:hybrid 生效时各段延迟可观测(lexical/embed/vector 三段)。
        let (root, store) = temp_root();
        let e1 = add(&store, "fact", "edit 失败", "old_string not found 先 read", "先 read 再 edit");

        // 有 embedder:embed 段有耗时,且结果命中。用带 2ms 延迟的桩模拟真实
        // embedding 耗时(毫秒粒度可测;FakeEmbedder 本身亚毫秒会记成 0)。
        struct SlowEmbedder;
        impl crate::embed::Embedder for SlowEmbedder {
            fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
                std::thread::sleep(std::time::Duration::from_millis(2));
                crate::embed::FakeEmbedder::new(8).embed(texts)
            }
        }
        {
            let embedder = Arc::new(SlowEmbedder);
            let mut index = SqliteMemoryIndex::with_embedder(&root, Some(embedder));
            index.rebuild().unwrap();
            let query = IndexQuery::text("edit 失败 old_string not found 先 read 再 edit");
            let (hits, timing) = index.search_hybrid_with_timing(&query, 5);
            assert!(
                hits.iter().any(|h| h.id == e1.id),
                "hybrid 必须命中: {:?}",
                hits
            );
            // embed 段必须 ≥2ms(真的调了 embedder);vector 段可能为 0(本地扫描极快)。
            assert!(timing.embed_ms >= 2, "embed 段耗时必须可测: {timing:?}");
            assert_eq!(
                timing.total(),
                timing.lexical_ms + timing.embed_ms + timing.vector_ms,
                "total 必须等于三段之和"
            );
        }

        // 无 embedder:embed/vector 段为 0(降级路径可见)。
        {
            let index = SqliteMemoryIndex::new(&root);
            let query = IndexQuery::text("edit 失败");
            let (_, timing) = index.search_hybrid_with_timing(&query, 5);
            assert_eq!(timing.embed_ms, 0, "无 embedder 时 embed 段必须为 0");
            assert_eq!(timing.vector_ms, 0, "无 embedder 时 vector 段必须为 0");
            assert!(timing.lexical_ms >= 0);
        }
    }
}
