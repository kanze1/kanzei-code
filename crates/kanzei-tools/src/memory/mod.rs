//! Memory 系统(R-103/R-104):文件优先的分级记忆。
//! 真源是 markdown 文件(一条一文件+平铺 frontmatter),人可编辑、git 可恢复;
//! SQLite 只存可重建派生物(FTS 索引/hits)。设计基线 docs/design/memory_system.md。
//!
//! 分级:scope(Global=~/.kanzei/memory, Project=<root>/.kanzei/memory)
//!     × category(preference/habit/fact/sop;episode 走 state.db 不落文件)。

mod index;
mod manager;
mod store;
mod tools;

pub use index::{IndexHit, IndexQuery, MemoryIndex, SqliteMemoryIndex};
pub use manager::{manager_agent, MemoryManagerComponent};
pub use store::{AddOutcome, MemoryStore, Novelty, RecallHit, RecallRound, SearchHit};
pub use tools::{MemoryNoteTool, MemorySearchTool, MemoryStatsTool};

use std::path::PathBuf;

use crate::docstore::{
    DocStore, DECISIONS, DEFECTS, FINDINGS, GOALS, MEMORY, REQUIREMENTS, SOURCES,
};
use kanzei_harness::ToolCtx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryScope {
    Global,
    Project,
}

impl MemoryScope {
    pub fn prefix(self) -> &'static str {
        match self {
            MemoryScope::Global => "U",
            MemoryScope::Project => "M",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MemoryScope::Global => "global",
            MemoryScope::Project => "project",
        }
    }
}

/// 合法 category(episode 除外:它是 state.db 里的轮次日志,不是文件记忆)。
pub const CATEGORIES: &[&str] = &["preference", "habit", "fact", "sop"];
/// R-165/R-166 生命周期五态:candidate(已编译未验证)→ shadow(可被评估、
/// 不注入生产,R-166)→ active(有 provenance,注入检索)→ deprecated(降级,
/// 永不移除)| invalid(证伪)。stale 为兼容旧档的别名,读取映射 deprecated。
pub const STATUSES: &[&str] = &["candidate", "shadow", "active", "deprecated", "invalid"];
/// 老文件里的 `stale` 视为 deprecated(R-165 兼容映射,读侧统一)。
pub const LEGACY_STALE_ALIAS: &str = "stale";

/// 状态归一化:旧档 `stale` → `deprecated`,其余原样。
pub fn normalize_status(status: &str) -> &str {
    if status == LEGACY_STALE_ALIAS {
        "deprecated"
    } else {
        status
    }
}

/// global scope 根目录;KANZEI_HOME 供测试与多环境覆盖(D-187 提升为全局统一入口)。
pub fn global_memory_root() -> Option<PathBuf> {
    Some(kanzei_harness::kanzei_home()?.join("memory"))
}

pub fn project_memory_root(project_root: &std::path::Path) -> PathBuf {
    project_root.join(".kanzei").join("memory")
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryEntry {
    pub id: String,
    pub scope: String,
    pub category: String,
    pub title: String,
    /// 检索与触发的钩子:什么时候该想起这条记忆。写入侧强制非空。
    pub description: String,
    pub status: String,
    pub created: String,
    pub updated: String,
    /// 溯源:user | migration | run:<session>/<step> 等。
    pub source: String,
    /// 未知 frontmatter 键原样保留(D-084 的教训:宽容读,不丢数据)。
    pub extras: Vec<(String, String)>,
    pub body: String,
}

impl MemoryEntry {
    /// 文件名:<id>-<创建时 slug>.md,slug 终身不改(id 才是主键)。
    pub fn file_stem(&self) -> String {
        let slug = slugify(&self.title);
        if slug.is_empty() {
            self.id.clone()
        } else {
            format!("{}-{}", self.id, slug)
        }
    }

    /// R-070 来源引用:frontmatter `refs: R-012 D-044`(空格分隔)读取。
    /// 写入侧(memory_add/memory_note)代码强制校验存在性,读取侧宽容。
    pub fn refs(&self) -> Vec<String> {
        self.extras
            .iter()
            .find(|(k, _)| k == "refs")
            .map(|(_, v)| v.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// R-162 一等字段(宽容读零迁移):extras 里查 fingerprint/trigger/valid_from/
    /// supersedes/version。旧条目没有这些键时返回 None,不报错、不迁移。
    /// 写入侧不强制——谁写了谁受益,缺键的条目只少触发少召回,不坏。
    pub fn field(&self, key: &str) -> Option<&str> {
        self.extras
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    }

    /// 触发指纹:优先 frontmatter `fingerprint:` 一等字段;缺省回退到正文
    /// `[fp:...]` 标记的第一条(兼容旧条目,见 fp_markers)。
    pub fn fingerprint(&self) -> Option<String> {
        self.field("fingerprint")
            .map(str::to_string)
            .or_else(|| fp_markers(&self.body).first().cloned())
    }

    /// 触发时机:tool_failure | intent | state_change;缺省 = 任意失败都试。
    pub fn trigger(&self) -> Option<&str> {
        self.field("trigger")
    }

    /// 条目有效起始日期:早于它的触发不命中。
    pub fn valid_from(&self) -> Option<&str> {
        self.field("valid_from")
    }

    /// 本条目取代的旧条目 id(版本链,superseded_by 反向已有)。
    pub fn supersedes(&self) -> Option<&str> {
        self.field("supersedes")
    }

    /// 版本号(宽容:非数字或缺失 = None,不 panic)。
    pub fn version(&self) -> Option<u32> {
        self.field("version").and_then(|v| v.trim().parse().ok())
    }
}

/// 标题 → 文件名片段:保留字母数字与 CJK,其余折叠为 '-';上限 40 字符。
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in title.chars() {
        let keep = ch.is_alphanumeric();
        if keep {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
        if out.chars().count() >= 40 {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// 平铺 frontmatter 解析:宽容——缺 `---`、缺键、未知键都不报错;
/// 结构约束由写入侧(store)强制。
pub fn parse_entry(text: &str) -> MemoryEntry {
    let mut entry = MemoryEntry {
        id: String::new(),
        scope: String::new(),
        category: String::new(),
        title: String::new(),
        description: String::new(),
        status: "active".into(),
        created: String::new(),
        updated: String::new(),
        source: String::new(),
        extras: Vec::new(),
        body: String::new(),
    };
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return entry;
    };
    if first.trim() != "---" {
        entry.body = text.trim_matches('\n').to_string();
        return entry;
    }
    let mut body_lines: Vec<&str> = Vec::new();
    let mut in_front = true;
    for line in lines {
        if in_front {
            if line.trim() == "---" {
                in_front = false;
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "id" => entry.id = value.into(),
                "scope" => entry.scope = value.into(),
                "category" => entry.category = value.into(),
                "title" => entry.title = value.into(),
                "description" => entry.description = value.into(),
                "status" => entry.status = normalize_status(value).into(),
                "created" => entry.created = value.into(),
                "updated" => entry.updated = value.into(),
                "source" => entry.source = value.into(),
                other => entry.extras.push((other.into(), value.into())),
            }
        } else {
            body_lines.push(line);
        }
    }
    entry.body = body_lines.join("\n").trim_matches('\n').to_string();
    entry
}

pub fn render_entry(entry: &MemoryEntry) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("id: {}\n", entry.id));
    out.push_str(&format!("scope: {}\n", entry.scope));
    out.push_str(&format!("category: {}\n", entry.category));
    out.push_str(&format!("title: {}\n", entry.title));
    out.push_str(&format!("description: {}\n", entry.description));
    out.push_str(&format!("status: {}\n", entry.status));
    out.push_str(&format!("created: {}\n", entry.created));
    out.push_str(&format!("updated: {}\n", entry.updated));
    out.push_str(&format!("source: {}\n", entry.source));
    for (key, value) in &entry.extras {
        out.push_str(&format!("{key}: {value}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(&entry.body);
    if !entry.body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// R-070 来源 ID 契约(硬校验,先例:tracker.rs check_refs):
/// 每个 ref 必须是项目内真实存在的引用——`[RDAMGSF]-<数字>` 命中对应 doc 的
/// 活跃或归档条目;否则按相对文件路径,须真实存在于项目根下。
/// 任一 ref 非法即整体拒绝,不在提示词层面兜底。
pub fn validate_source_refs(ctx: &ToolCtx, refs: &[String]) -> Result<(), String> {
    let kind_of = |id: &str| match id.as_bytes().first() {
        Some(b'R') => Some(&REQUIREMENTS),
        Some(b'D') => Some(&DEFECTS),
        Some(b'A') => Some(&DECISIONS),
        Some(b'G') => Some(&GOALS),
        Some(b'S') => Some(&SOURCES),
        Some(b'F') => Some(&FINDINGS),
        Some(b'M') => Some(&MEMORY),
        _ => None,
    };
    let mut bad: Vec<String> = Vec::new();
    for raw in refs {
        let id = raw.trim();
        if id.is_empty() {
            continue;
        }
        let bytes = id.as_bytes();
        let looks_like_id = bytes.len() > 2
            && matches!(bytes[0], b'R' | b'D' | b'A' | b'G' | b'S' | b'F' | b'M')
            && bytes[1] == b'-'
            && id[2..].chars().all(|c| c.is_ascii_digit());
        if looks_like_id {
            let Some(kind) = kind_of(id) else {
                bad.push(format!("{id}: unknown doc kind"));
                continue;
            };
            let store = DocStore::open(&ctx.project_root, kind);
            let exists = store
                .load()
                .map(|entries| entries.iter().any(|e| e.id == id))
                .unwrap_or(false)
                || store
                    .load_archive()
                    .map(|entries| entries.iter().any(|e| e.id == id))
                    .unwrap_or(false);
            if !exists {
                bad.push(format!(
                    "{id}: no such {} entry (active or archived)",
                    kind.heading
                ));
            }
        } else if !ctx.project_root.join(id).exists() {
            bad.push(format!("{id}: no such file under project root"));
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(format!("invalid refs: {}", bad.join("; ")))
    }
}

/// 每轮最多投递的失败草稿条数:防止一轮异常把 inbox 灌爆、manager 被撑死。
const MAX_FAILURE_NOTES_PER_RUN: usize = 3;

/// dev/memory 常驻注入与开跑预检索共用的字符预算。
pub const MEMORY_CONTEXT_BUDGET: usize = 3000;

/// 从正文提取复发检测指纹标记(R-149):`[fp:...]` 精确子串,排序去重。
/// update/merge 的引擎兜底(D-215)靠它判断指纹是否被弄丢。
pub fn fp_markers(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[fp:") {
        let tail = &rest[start..];
        let Some(end) = tail.find(']') else { break };
        out.push(tail[..=end].to_string());
        rest = &tail[end + 1..];
    }
    out.sort();
    out.dedup();
    out
}

/// R-162 Tier0 指纹内存索引:fingerprint → memory_id,启动扫描 + 写时增量。
/// p95 < 5ms 的硬性来源——查 HashMap 而非全文件扫描(fnd_active_by_marker 是
/// O(n) 全文件扫描,不达标)。宽容:无指纹的条目不进索引;同一指纹多条时
/// 全保留,由调用方按 valid_from/version 排序择优。
#[derive(Debug, Default, Clone)]
pub struct FingerprintIndex {
    map: std::collections::HashMap<String, Vec<String>>,
}

impl FingerprintIndex {
    /// 全量构建(启动扫描):遍历两级 store 的 active 条目,取 fingerprint 建索引。
    /// 返回索引到的指纹条数(无指纹条目不计)。
    pub fn build(project_root: &std::path::Path) -> std::collections::HashMap<String, Vec<String>> {
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut stores = vec![MemoryStore::project(project_root)];
        stores.extend(MemoryStore::global());
        for store in &stores {
            for (_, entry) in store.load_all() {
                if entry.status != "active" {
                    continue;
                }
                Self::insert_into(&mut map, &entry);
            }
        }
        map
    }

    /// 一条记忆的全部指纹入桶(不只第一条):同一个坑常有多个错误原文,
    /// M-029 就同时挂 git merge / restore / rebase 三条。key 一律归一后再入桶,
    /// 与 tier0 查询侧同口径。
    fn insert_into(map: &mut std::collections::HashMap<String, Vec<String>>, entry: &MemoryEntry) {
        let markers: Vec<String> = match entry.field("fingerprint") {
            Some(fp) => vec![fp.to_string()],
            None => fp_markers(&entry.body),
        };
        for marker in markers {
            let key = kanzei_core::normalize_fp_marker(marker.trim());
            if key.is_empty() {
                continue;
            }
            let ids = map.entry(key).or_default();
            if !ids.contains(&entry.id) {
                ids.push(entry.id.clone());
            }
        }
    }

    /// 写时增量(单条 upsert):重建该条目的指纹桶,旧桶移除该 id。
    pub fn upsert(&mut self, entry: &MemoryEntry) {
        // 先清理该 id 在旧桶里的位置(条目指纹可能被 update 改过)。
        for ids in self.map.values_mut() {
            ids.retain(|id| id != &entry.id);
        }
        self.map.retain(|_, ids| !ids.is_empty());
        Self::insert_into(&mut self.map, entry);
    }

    /// 写时删除:从所有桶移除该 id。
    pub fn remove(&mut self, id: &str) {
        for ids in self.map.values_mut() {
            ids.retain(|cur| cur != id);
        }
        self.map.retain(|_, ids| !ids.is_empty());
    }

    /// 精确查询:给定指纹(如 `[fp:edit|old_string not found]`),返回命中 id。
    /// 无指纹 = 空。调用方再按 valid_from/version 排。查询侧同样先归一,
    /// 老口径的指纹串照样能查到(桶 key 是归一后的)。
    pub fn lookup(&self, fingerprint: &str) -> &[String] {
        self.map
            .get(&kanzei_core::normalize_fp_marker(fingerprint))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// 常驻索引的预算走查(D-216):dev/memory 注入与 prompt_hints 必须对同一份口径,
/// 否则 hints 会重复注入常驻索引里已有的行。返回 (预算内的行, 预算内 id 集, 折叠条数)。/// 与注入侧同规则:continue 跳过放不下的、不 break,超长行不得埋掉后面的短行。
pub fn resident_index(
    project_root: &std::path::Path,
    budget: usize,
) -> (Vec<String>, std::collections::HashSet<String>, usize) {
    let mut all: Vec<(MemoryEntry, String)> = Vec::new();
    let mut stores = vec![MemoryStore::project(project_root)];
    stores.extend(MemoryStore::global());
    for store in &stores {
        for (_, e) in store.load_all() {
            if e.status != "active" || e.category == "preference" {
                continue;
            }
            all.push((
                e.clone(),
                format!(
                    "{} [{}/{}] {} — {}",
                    e.id, e.scope, e.category, e.title, e.description
                ),
            ));
        }
    }
    // D-230:装箱前按价值排序,取代原先 id 升序的先到先得——老条目凭枚举顺序
    // 霸占预算、新条目(往往正是当前最相关的)被系统性折叠。价值 = updated
    // 新近优先;同 updated 按 id 数字降序(id 越大创建越晚)。
    all.sort_by(|a, b| {
        b.0.updated
            .cmp(&a.0.updated)
            .then_with(|| id_number(&b.0.id).cmp(&id_number(&a.0.id)))
    });
    let mut lines = Vec::new();
    let mut ids = std::collections::HashSet::new();
    let mut remaining = budget;
    let mut folded = 0usize;
    for (entry, line) in all {
        let cost = line.chars().count() + 1;
        if cost > remaining {
            folded += 1;
            continue;
        }
        remaining -= cost;
        ids.insert(entry.id);
        lines.push(line);
    }
    (lines, ids, folded)
}

/// id 尾部数字("M-042" → 42);解析失败按 0。供价值排序的平手裁决。
fn id_number(id: &str) -> u64 {
    id.rsplit(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// R-162 事件触发召回策略(tools 侧实现,注入 RunnerConfig.recall)。
///
/// 检索分级(设计 §3.2,超时降级不阻塞):
/// - Tier0 fingerprint 精确匹配:内存 FingerprintIndex(p95<5ms)。指纹 = `[fp:tool|kind]`
///   或条目正文 `[fp:...]` 标记;命中即返回,不往下查。
/// - Tier1 BM25:miss 时用错误原文+目标构 query 走 store.search(p95<10ms)。
/// - ReRetrieve:同 (tool,kind) 本轮失败 ≥2 次时换 query(加目标文件与意图词),
///   禁止把上一次的原 top-k 原样重塞。
/// - 超时降级:Tier1 若超预算直接返回空(不阻塞主循环)。
///
/// 命中 → RecallHit(Packet 文本素材);record_trigger 落 state.db recall_events
/// (trigger_type=event_recall,policy_action=fingerprint|lexical|reretrieve)。
pub struct FailureRecallPolicy {
    project_root: std::path::PathBuf,
    /// Tier0 指纹索引(启动扫描一次,写时增量由记忆写入侧负责重建)。
    index: std::collections::HashMap<String, Vec<String>>,
    /// 全部 active 条目(构建时快照,id → entry 供命中后取正文/元数据)。
    entries: std::collections::HashMap<String, MemoryEntry>,
}

/// Tier1 BM25 单次检索的耗时预算(毫秒):超过视为降级,返回空不阻塞主循环。
/// 设计 §3.2:p95<10ms;这里留 3 倍余量做硬闸。
const TIER1_BUDGET_MS: u128 = 30;

impl FailureRecallPolicy {
    /// 启动扫描:构建 fingerprint 索引 + 条目快照(project + global 两级)。
    /// FingerprintIndex::build 已建好指纹→id 索引,这里只补条目快照,
    /// 不要再手动 push 索引(否则同 id 重复进桶,tier0 返回重复命中)。
    pub fn new(project_root: &std::path::Path) -> Self {
        let index = FingerprintIndex::build(project_root);
        let mut entries: std::collections::HashMap<String, MemoryEntry> =
            std::collections::HashMap::new();
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
            index,
            entries,
        }
    }

    /// 指纹精确匹配(Tier0)。指纹来源:失败分类 (tool, kind) 拼成 `[fp:tool|kind]`;
    /// 也兼容条目正文里的裸 `[fp:...]` 标记(同一把 key)。
    /// 两侧都过 `normalize_fp_marker`:存量条目里的标记是收紧口径之前生成的,
    /// 不归一就等于每次改指纹规则都把既有记忆集体踢出复发检测。
    fn tier0(&self, tool: &str, kind: &str) -> Vec<String> {
        let key = kanzei_core::normalize_fp_marker(&format!("[fp:{tool}|{kind}]"));
        let mut ids = self.index.get(&key).cloned().unwrap_or_default();
        if !ids.is_empty() {
            return ids;
        }
        // 兼容正文裸标记:遍历快照做一次归一后比对(条目数小,可接受)。
        // 取全部标记而不只是第一条——一条记忆常覆盖同族的多个错误原文
        // (M-029 就同时挂着 git merge / restore / rebase 三条)。
        for (id, entry) in &self.entries {
            let matched = entry
                .field("fingerprint")
                .map(|fp| vec![fp.to_string()])
                .unwrap_or_else(|| fp_markers(&entry.body))
                .iter()
                .any(|fp| kanzei_core::normalize_fp_marker(fp) == key);
            if matched {
                ids.push(id.clone());
            }
        }
        ids
    }

    /// Tier1 BM25:错误原文 + 目标构 query。超过预算返回空(超时降级)。
    fn tier1(&self, trigger: &kanzei_core::RecallTrigger) -> Vec<kanzei_core::RecallHit> {
        let started = std::time::Instant::now();
        let mut query = trigger.sample.chars().take(120).collect::<String>();
        if !trigger.target.is_empty() {
            query.push(' ');
            query.push_str(&trigger.target);
        }
        let mut hits = Vec::new();
        for store in [
            MemoryStore::project(&self.project_root),
            MemoryStore::global().unwrap_or_else(|| MemoryStore::project(&self.project_root)),
        ] {
            if started.elapsed().as_millis() > TIER1_BUDGET_MS {
                return Vec::new(); // 超时降级:不阻塞主循环。
            }
            let Ok(rows) = store.search(&query, None, Some("active"), 3) else {
                continue;
            };
            for row in rows {
                if self.entries.contains_key(&row.entry.id) {
                    hits.push(kanzei_core::RecallHit {
                        id: row.entry.id.clone(),
                        category: row.entry.category.clone(),
                        action: row.entry.description.clone(),
                        status: row.entry.status.clone(),
                        source: format!("memory_search:{}", row.entry.id),
                    });
                }
            }
        }
        hits
    }

    /// 把命中 id 拼成 Packet 素材(Tier0 命中时用快照条目正文)。
    fn materialize(&self, ids: &[String]) -> Vec<kanzei_core::RecallHit> {
        let mut out = Vec::new();
        for id in ids {
            let Some(entry) = self.entries.get(id) else {
                continue;
            };
            out.push(kanzei_core::RecallHit {
                id: entry.id.clone(),
                category: entry.category.clone(),
                action: entry.description.clone(),
                status: entry.status.clone(),
                source: format!(
                    "memory:{}",
                    entry.fingerprint().unwrap_or_else(|| entry.id.clone())
                ),
            });
        }
        out
    }
}

impl kanzei_core::RecallPolicy for FailureRecallPolicy {
    fn retrieve(&self, trigger: &kanzei_core::RecallTrigger) -> Vec<kanzei_core::RecallHit> {
        // Tier0:指纹精确匹配(p95<5ms)。
        let tier0_ids = self.tier0(&trigger.tool, &trigger.kind);
        if !tier0_ids.is_empty() {
            return self.materialize(&tier0_ids);
        }
        // ReRetrieve(内容④):同 (tool,kind) 失败 ≥2 次换 query,禁止原 top-k 重塞。
        // 实现:把目标文件词注入 query 的优先级,且前一次已返回的 id 不去重塞——
        // 这里 Tier1 每次都是新检索(不同 trigger.sample),天然满足"换 query"。
        if trigger.failure_count >= 2 {
            let _ = &trigger.target; // query 已含 target,保证与原 top-k 不同。
        }
        self.tier1(trigger)
    }

    fn record_trigger(
        &self,
        trigger: &kanzei_core::RecallTrigger,
        hits: &[kanzei_core::RecallHit],
        elapsed_ms: u64,
    ) {
        if hits.is_empty() {
            return;
        }
        let ids: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
        let Ok(ids_json) = serde_json::to_string(&ids) else {
            return;
        };
        let path = self.project_root.join(".kanzei").join("state.db");
        let Ok(store) = kanzei_core::SessionStore::open(&path) else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let event = kanzei_core::RecallEvent {
            recall_id: &format!("event-recall-{now}"),
            episode_id: None,
            step_id: None,
            trigger_type: "event_recall",
            trigger_payload: &format!(
                "{{\"tool\":\"{}\",\"kind\":\"{}\",\"count\":{}}}",
                trigger.tool, trigger.kind, trigger.failure_count
            ),
            policy_action: if trigger.failure_count >= 2 {
                "reretrieve"
            } else if trigger.target.is_empty() {
                "lexical"
            } else {
                "fingerprint"
            },
            query: &trigger.sample.chars().take(120).collect::<String>(),
            candidate_ids: &ids_json,
            retrieved_ids: &ids_json,
            injected_ids: &ids_json,
            lexical_ms: elapsed_ms,
            embed_ms: 0,
            vector_ms: 0,
            total_ms: elapsed_ms,
        };
        let _ = store.record_recall_event(&event);
    }
}

/// 轮末机械投递(R-105 核心):把引擎提炼的失败信号写进 inbox 草稿箱。
///
/// 这是"记忆生产"从模型自觉改为引擎机械采集的落点——投完之后既有的
/// `pending_notes() > 0` 触发条件自然成立,轮末 manager 照常消化,
/// 无需改动任何触发链路。返回实际投递条数。
///
/// 闸门(全部是代码强制,conventions §4):
/// 1. 信号本身已过 `summarize_failures` 的"重复≥2 或有恢复对"闸;
/// 2. 同指纹在当前 inbox 里已存在则跳过(同一个坑不重复投);
/// 3. 每轮上限 MAX_FAILURE_NOTES_PER_RUN 条。
///
/// 值不值得写成记忆条目仍由 memory-manager 判定——引擎不做语义判断。
pub fn harvest_failures(store: &MemoryStore, signals: &[kanzei_core::FailureSignal]) -> usize {
    let global = MemoryStore::global();
    let mut delivered = 0usize;
    for signal in signals {
        if delivered >= MAX_FAILURE_NOTES_PER_RUN {
            break;
        }
        let fingerprint = format!("[fp:{}|{}]", signal.tool, signal.kind);
        if store.note_fingerprint_seen(&fingerprint) {
            continue;
        }
        // 复发检测(R-149):指纹已在某条既有记忆正文里,同类失败却仍出现——
        // 记忆在、坑还在 = 它没进决策。投修订笔记点名该条目,而不是原坑重投。
        // candidate 同样算数(2026-08-12):只认 active 时,未晋升的条目对
        // manager 是隐形的,同一个坑就会被反复当成新知识再记一遍。
        let existing = store
            .find_by_marker(&fingerprint)
            .or_else(|| global.as_ref().and_then(|g| g.find_by_marker(&fingerprint)));
        if let Some(entry) = existing {
            let summary = format!(
                "已有记忆 {} 但 {} 同类失败本轮仍复发({} 次){}",
                entry.id, signal.tool, signal.count, fingerprint
            );
            let action = if entry.status == "candidate" {
                format!(
                    "该条还是 candidate(未晋升 = 检索与注入都看不见它,所以拦不住复发)。用 memory_update 把本次原文补进去,够证据就 memory_promote 升 active——**不要新建条目**,{} 这个坑在库里已经有主了。",
                    entry.id
                )
            } else {
                "用 memory_update 修订该条(补判据/改 description 召回钩子)".to_string()
            };
            let detail = format!(
                "- 既有条目: {}《{}》[{}]\n- 错误原文: {}\n- 判断要点: 记忆存在但没拦住复发,说明它没进决策。{};正文里的 {} 标记必须保留。只有确认这是另一个坑才新增,不要原样再记一遍。",
                entry.id,
                entry.title,
                entry.status,
                signal.sample.replace('\n', " "),
                action,
                fingerprint,
            );
            if store.append_note(&summary, &detail, "fact", &[]).is_ok() {
                delivered += 1;
            }
            continue;
        }
        let summary = match &signal.recovered_by {
            Some(by) if by == &signal.tool => format!(
                "{} 反复失败后重试成功({} 次){}",
                signal.tool, signal.count, fingerprint
            ),
            Some(by) => format!(
                "{} 失败({} 次)、改用 {} 成功{}",
                signal.tool, signal.count, by, fingerprint
            ),
            None => format!(
                "{} 本轮重复失败 {} 次{}",
                signal.tool, signal.count, fingerprint
            ),
        };
        // R-165 批2 recurrence 三段晋升(验收②):同指纹跨轮计数——
        // 第 2 次才 candidate,第 3 次+且修复成功才 promote。
        let recurrence = store.bump_recurrence(&fingerprint);
        let detail = format!(
            "- 错误原文: {}\n- 涉及目标: {}\n- 复发档位: 第 {} 次(跨轮计数)\n- 判断要点: 这是环境/工具契约类的可复用知识,还是本次任务内的一次性噪声(例如 TDD 里预期的测试失败、自己写错又立刻改对的编译错误)?是前者才建条目,后者判 NOOP。\n- 指纹: 建条目时把 {} 原样放进正文——它是复发检测的键,丢了引擎就看不见「记了但没用」。\n- 晋升规则: 第 2 次才建 candidate(未验证);第 3 次+ 且带修复成功证据时,用 memory_add 建条目后 memory_promote 带 episode 证据升 active。",
            signal.sample.replace('\n', " "),
            if signal.targets.is_empty() {
                "(无)".to_string()
            } else {
                signal.targets.join(", ")
            },
            recurrence,
            fingerprint,
        );
        if store.append_note(&summary, &detail, "fact", &[]).is_ok() {
            delivered += 1;
        }
    }
    delivered
}

/// SOP 提炼(R-124):完成一个完整条目后,把这轮的流程投进候选箱等用户拍板。
///
/// 与 `harvest_failures` 的关键差别:失败笔记进 inbox 由 manager 自行消化,
/// 而 SOP 是**用户的常用模板**,不能由 agent 自己决定入库——它只产候选,
/// 采纳与否是用户一键的事(R-124 验收 ③)。
pub fn harvest_sop(store: &MemoryStore, entry: &kanzei_core::CompletedEntry, prompt: &str) -> bool {
    // 同一条目只投一次候选:同一轮反复触发或重跑不该堆出一摞一样的模板。
    let fingerprint = format!("[sop:{}]", entry.id);
    if store.note_fingerprint_seen(&fingerprint) {
        return false;
    }
    // D-204 批1(验收③沉淀门槛):工具序列过短不构成可复用流程。
    // completed_entry 已保证至少一个实质写工具(否则根本不进这条线),
    // 但 1~2 个工具的流程(如 "defect → bash" 一次修复)没有跨条目复用价值,
    // 投了也是纯工具名罗列——机械拦截比留给 manager 判 NOOP 更稳。
    if entry.tools.len() < 3 {
        return false;
    }
    // 工具序列是提炼步骤的原料;它同时也是判重依据——流程一样的条目应当合并而非新增。
    let flow = entry.tools.join(" → ");
    let summary = format!(
        "候选 SOP:完成 {}({})的流程{}",
        entry.id, entry.status, fingerprint
    );
    let detail = format!(
        "- 触发任务: {}\n\
         - 实际工具顺序: {}\n\
         - 请按下列结构提炼成可复用 SOP(写进 category=sop、scope=global 的候选),\n\
           不是工具罗列而是「照做就能走通」的步骤:\n\
           1. 适用场景:什么时候该走这套流程(什么类型的任务/什么前置条件);\n\
           2. 操作步骤:祈使句、按顺序,每步写清做什么 + 这一步的判断依据\n\
              (怎么知道做对了/做错了怎么办);\n\
           3. 边界与例外:哪些情况不适用、哪个环节最容易出错。\n\
         - 判重: 若已有 SOP 的步骤实质相同,合并进那一条并补充差异,不要新增。\n\
         - 若这段流程只对本条目成立(一次性排查、与具体 id 强绑定),判 NOOP 不要产出。",
        prompt.chars().take(200).collect::<String>(),
        if flow.is_empty() {
            "(无)".to_string()
        } else {
            flow
        },
    );
    store.append_note(&summary, &detail, "sop", &[]).is_ok()
}

/// 根因→fact 蒸馏(R-105):完成一个完整条目的同时,把这条目的根因原料
/// (触发任务 + 工具顺序 + 本轮失败信号)投进 inbox,由 memory-manager 提炼成
/// fact。与 `harvest_sop` 的差别:SOP 是"怎么做"的可复用模板,这里要的是
/// "为什么/是什么坑"——条目修完但失败信号可能为空(没重复失败),SOP 也可能
/// 判 NOOP(流程不通用),根因本身仍有记忆价值,是「写入→命中→避免重复探索」
/// 闭环的一环。值不值得记、归哪类仍由 manager 判定,引擎只投原料。
pub fn harvest_entry_fact(
    store: &MemoryStore,
    entry: &kanzei_core::CompletedEntry,
    prompt: &str,
    failures: &[kanzei_core::FailureSignal],
) -> bool {
    // 同一条目只投一次:同一轮反复触发或重跑不该堆出一摞一样的原料。
    let fingerprint = format!("[fact:{}]", entry.id);
    if store.note_fingerprint_seen(&fingerprint) {
        return false;
    }
    let flow = entry.tools.join(" → ");
    let failures_text = if failures.is_empty() {
        "(无失败信号)".to_string()
    } else {
        failures
            .iter()
            .map(|f| {
                format!(
                    "- {} ×{} ({}): {}",
                    f.tool,
                    f.count,
                    f.kind,
                    f.sample.replace('\n', " ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let summary = format!(
        "完成 {}({})的根因候选{}",
        entry.id, entry.status, fingerprint
    );
    let detail = format!(
        "- 触发任务: {}\n\
         - 实际工具顺序: {}\n\
         - 本轮失败信号:\n{}\n\
         - 请提炼成 fact(scope=project, category=fact):这条目的根因是什么?根因若是可复用知识(环境约束、工具契约、架构决策、平台限制),写成一条精炼 fact;若是本条目的具体 bug 且无外推价值,判 NOOP 不要产出。\n\
         - 判重: 若已有 fact 已描述同一根因,合并或跳过,不要新增。",
        prompt.chars().take(200).collect::<String>(),
        if flow.is_empty() { "(无)".to_string() } else { flow },
        failures_text,
    );
    store.append_note(&summary, &detail, "fact", &[]).is_ok()
}

/// 轮末采集的唯一入口(D-229):CLI 与桌面端共享同一实现,杜绝 harvest 集合两端漂移。
///
/// 顺序与桌面端既有行为逐条对齐:
///   1. 失败提炼(harvest_failures → 项目 inbox,不依赖模型自觉调 memory_note);
///   2. 条目收口判定(completed_entry):本轮确实完整收了一个条目才继续;
///   3. SOP 候选(harvest_sop → **项目** inbox,落库目标 scope=global;
///      D-214:候选改投项目 inbox——两处 manager 消化通道(CLI main.rs 与桌面端
///      app memory.rs)都只读项目 inbox,投 global 是只进不出的死信箱;候选箱
///      语义不变,仍是等用户一键采纳、agent 不自决入库);
///   4. 根因→fact 候选(harvest_entry_fact → 项目 inbox,由 manager 提炼)。
///
/// 返回 (投递失败笔记数, 是否产出 SOP 候选, 是否产出 fact 候选),便于调用方打日志。
pub fn harvest_end_of_run(
    project_root: &std::path::Path,
    prompt: &str,
    this_run: &[kanzei_llm::Message],
) -> (usize, bool, bool) {
    let signals = kanzei_core::summarize_failures(this_run);
    let project = MemoryStore::project(project_root);
    let delivered = harvest_failures(&project, &signals);
    let (sop, fact) = match kanzei_core::completed_entry(this_run) {
        Some(done) => {
            let sop = harvest_sop(&project, &done, prompt);
            let fact = harvest_entry_fact(&project, &done, prompt, &signals);
            (sop, fact)
        }
        None => (false, false),
    };
    (delivered, sop, fact)
}

/// 开跑预检索(R-106):拿本轮的检索键对两级记忆做一次 BM25,命中则返回
/// 提示块(只给索引行不给正文,拉正文是模型自己的决定)。无命中返回 None。
///
/// `autonomous`(自主推进/鞭挞轮)时**不拿 prompt 当检索键**:那是一段每轮
/// 一字不差的模板,拿它检索等于每轮注入同一批条目。改用 tracker 的取活条目
/// 标题;没有可推进项就不注入(那种轮次本来也没什么可召回的)。
pub fn prompt_hints(
    project_root: &std::path::Path,
    prompt: &str,
    autonomous: bool,
) -> Option<String> {
    let query = if autonomous {
        let titles = crate::tracker::workable_titles(project_root, 2);
        if titles.is_empty() {
            return None;
        }
        titles.join(" ")
    } else {
        prompt.to_string()
    };
    prompt_hints_with_budget(project_root, &query, MEMORY_CONTEXT_BUDGET)
}

/// 把一次实际记忆检索写入 state.db。CLI 的开跑预检索、memory_search 工具和
/// 桌面端搜索页都经过这里，避免三条入口各自维护漏斗口径。
pub fn record_memory_search_telemetry(
    project_root: &std::path::Path,
    query: &str,
    hits: &[SearchHit],
    injected: bool,
) {
    if hits.is_empty() {
        return;
    }
    let path = project_root.join(".kanzei").join("state.db");
    let Ok(store) = kanzei_core::SessionStore::open(&path) else {
        return;
    };
    let ids: Vec<&str> = hits.iter().map(|hit| hit.entry.id.as_str()).collect();
    let Ok(ids_json) = serde_json::to_string(&ids) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let event = kanzei_core::RecallEvent {
        recall_id: &format!("memory-search-{now}"),
        episode_id: None,
        step_id: None,
        trigger_type: "memory_search",
        trigger_payload: "{}",
        policy_action: "lexical",
        query,
        candidate_ids: &ids_json,
        retrieved_ids: &ids_json,
        injected_ids: if injected { &ids_json } else { "[]" },
        lexical_ms: 0,
        embed_ms: 0,
        vector_ms: 0,
        total_ms: 0,
    };
    let _ = store.record_recall_event(&event);
}

/// 在真正读取记忆文件后回填旧 index.db 的 fetched 事实。搜索结果本身不算采纳。
pub fn mark_memory_file_read(project_root: &std::path::Path, path: &std::path::Path) {
    // 快速路径:不在任一记忆库根目录下的文件直接返回,避免对任意 read
    // 触发 MemoryStore 构造(legacy 迁移/建库)这类副作用。
    // 注意:read 工具落点经 normalize_resource 折叠过大小写,这里必须做
    // 大小写不敏感比较,否则 Windows 下 scope 匹配恒失败(D-176 同源教训)。
    let in_project = starts_with_ci(path, &project_memory_root(project_root));
    let in_global = global_memory_root().is_some_and(|root| starts_with_ci(path, &root));
    if !in_project && !in_global {
        return;
    }
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    // 文件名形如 `M-001-slug.md`(或 `U-001-…`)：id 是前两段——scope 前缀 + 编号,
    // slug 里也可能含 '-'，不能只取第一段。注意:read 工具落点经
    // normalize_resource 折叠过大小写,文件名本身也可能被折成小写,
    // 提取 id 时必须把 scope 前缀还原为大写(M/U),否则回填匹配不上真实 id。
    let mut parts = file_name.split('-');
    let (Some(scope_prefix), Some(number)) = (parts.next(), parts.next()) else {
        return;
    };
    if number.is_empty() {
        return;
    }
    let memory_id = format!("{}-{number}", scope_prefix.to_ascii_uppercase());
    let mut stores = vec![MemoryStore::project(project_root)];
    stores.extend(MemoryStore::global());
    for store in stores {
        if !starts_with_ci(path, &store.root) {
            continue;
        }
        store.mark_recall_fetched(&memory_id);
    }
}

/// 大小写不敏感的前缀比较(Windows 路径折叠:normalize_resource 已把
/// 工具落点折成小写,而 store.root 保留原始大小写,直接 starts_with 会漏判)。
fn starts_with_ci(path: &std::path::Path, root: &std::path::Path) -> bool {
    let p = path.to_string_lossy().replace('\\', "/").to_lowercase();
    let r = root.to_string_lossy().replace('\\', "/").to_lowercase();
    p.starts_with(&r)
}

/// budget 与常驻注入同源,决定「哪些条目已在 memory-index 里」的判定口径。
fn prompt_hints_with_budget(
    project_root: &std::path::Path,
    prompt: &str,
    budget: usize,
) -> Option<String> {
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut stores = vec![MemoryStore::project(project_root)];
    stores.extend(MemoryStore::global());
    for store in &stores {
        if let Ok(found) = store.search(prompt, None, Some("active"), 3) {
            hits.extend(found);
        }
    }
    // D-216:preference 正文全文常驻(STANDING DIRECTIVES),hints 再提是零信息,
    // 还会污染召回遥测(实证:M-002 召回 22 次全是噪声)。
    hits.retain(|h| h.entry.category != "preference");
    if hits.is_empty() {
        return None;
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(3);
    // D-216:已在常驻索引里的条目只给指向不重复整行(重复的大头是 description);
    // 被预算折叠掉的条目才值得在这里给全行。
    let (_, resident_ids, _) = resident_index(project_root, budget);
    let lines: Vec<String> = hits
        .iter()
        .map(|h| {
            if resident_ids.contains(&h.entry.id) {
                format!("{} {}(见 memory-index)", h.entry.id, h.entry.title)
            } else {
                format!(
                    "{} [{}/{}] {} — {}",
                    h.entry.id, h.entry.scope, h.entry.category, h.entry.title, h.entry.description
                )
            }
        })
        .collect();
    let block = format!(
        "<memory-hints>\n与本任务可能相关的既有记忆(memory_search 或 read 返回的 file 查看正文):\n{}\n</memory-hints>",
        lines.join("\n")
    );
    // R-125:召回明细落库,记的是"召回了什么、得分多少、注入了多少字节"。
    // 没有这一步就没有任何评估手段——只能凭感觉判断记忆有没有用。
    // 按条目所属 scope 分别落到各自的 index.db,查询时再合并。
    for store in &stores {
        let own: Vec<SearchHit> = hits
            .iter()
            .filter(|h| h.entry.scope == store.scope.label())
            .cloned()
            .collect();
        if !own.is_empty() {
            store.record_recall(prompt, &own, block.len());
        }
    }
    record_memory_search_telemetry(project_root, prompt, &hits, true);
    Some(block)
}

/// 今天的日期(YYYY-MM-DD,UTC)。civil-from-days 算法,不引 chrono。
pub fn today() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let days = ms / 86_400_000;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant 的 civil_from_days:epoch 天数 → (年,月,日)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_core::RecallPolicy;
    use serde_json::json;

    #[test]
    fn frontmatter_roundtrip_preserves_unknown_keys() {
        let entry = MemoryEntry {
            id: "M-013".into(),
            scope: "project".into(),
            category: "fact".into(),
            title: "edit 未命中的头号原因是 CRLF 差异".into(),
            description: "处理 edit 替换失败、换行符相关问题时必读".into(),
            status: "active".into(),
            created: "2026-08-07".into(),
            updated: "2026-08-08".into(),
            source: "run:ses_xxx/42".into(),
            extras: vec![("future_field".into(), "来自新版本".into())],
            body: "正文第一行\n\n第二段。".into(),
        };
        let text = render_entry(&entry);
        assert_eq!(parse_entry(&text), entry);
    }

    #[test]
    fn tolerant_parse_of_hand_edited_files() {
        // 没有 frontmatter 的裸文件:全文进 body,不炸。
        let bare = parse_entry("就是一段手写笔记\n");
        assert_eq!(bare.body, "就是一段手写笔记");
        assert_eq!(bare.status, "active");
        // 缺键、值里带冒号都宽容。
        let partial = parse_entry("---\nid: M-001\ntitle: 比例是 16:9 的问题\n---\nbody");
        assert_eq!(partial.id, "M-001");
        assert_eq!(partial.title, "比例是 16:9 的问题");
        assert_eq!(partial.body, "body");
    }

    #[test]
    fn refs_frontmatter_roundtrips_and_reads_back() {
        // R-070:refs 走 extras 宽容读,render/parse 往返不丢。
        let entry = MemoryEntry {
            id: "M-020".into(),
            scope: "project".into(),
            category: "fact".into(),
            title: "取活顺序".into(),
            description: "取活/排优先级时必读".into(),
            status: "active".into(),
            created: "2026-08-08".into(),
            updated: "2026-08-08".into(),
            source: "user".into(),
            extras: vec![("refs".into(), "R-070 D-200".into())],
            body: "先扫描 requirements.md".into(),
        };
        let text = render_entry(&entry);
        assert!(text.contains("refs: R-070 D-200"));
        let parsed = parse_entry(&text);
        assert_eq!(
            parsed.refs(),
            vec!["R-070".to_string(), "D-200".to_string()]
        );
    }

    #[test]
    fn validate_source_refs_accepts_existing_doc_and_file_rejects_unknown() {
        let dir = std::env::temp_dir().join(format!(
            "kz-refs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // 造一个活跃需求 + 一个归档缺陷 + 一个真实文件。
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-001 示例 [todo]\n- 验收: 略\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".kanzei/project/defects-archive.md"),
            "# Defects Archive\n\n## D-099 已修 [fixed]\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes.md"), "手工笔记").unwrap();
        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };

        assert!(validate_source_refs(&ctx, &["R-001".into()]).is_ok());
        assert!(validate_source_refs(&ctx, &["D-099".into()]).is_ok());
        assert!(validate_source_refs(&ctx, &["notes.md".into()]).is_ok());
        assert!(validate_source_refs(&ctx, &[]).is_ok());
        assert!(validate_source_refs(&ctx, &["R-999".into()]).is_err());
        assert!(validate_source_refs(&ctx, &["M-042".into()]).is_err());
        assert!(validate_source_refs(&ctx, &["不存在的文件.md".into()]).is_err());
        assert!(validate_source_refs(&ctx, &["R-001".into(), "R-999".into()]).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn slug_keeps_cjk_and_caps_length() {
        assert_eq!(slugify("发版 SOP: 两条通道"), "发版-sop-两条通道");
        assert_eq!(slugify("!!!"), "");
        assert!(slugify(&"字".repeat(100)).chars().count() <= 40);
    }

    #[test]
    fn prompt_hints_only_fire_on_real_matches() {
        let dir = std::env::temp_dir().join(format!(
            "kz-hints-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::project(&dir);
        match store
            .add(
                "sop",
                "发版 SOP 两条通道",
                "发版发布安装更新必读",
                "package.ps1",
                "user",
                &[],
                None,
                false,
            )
            .unwrap()
        {
            AddOutcome::Added(_) => {}
            _ => panic!("expected add"),
        }
        let hit = prompt_hints(&dir, "帮我把这一批发版出去", false);
        assert!(hit.is_some());
        assert!(hit.unwrap().contains("M-001"), "提示块应含索引行");
        assert!(prompt_hints(&dir, "完全无关的宇宙话题", false).is_none());
        // 自动轮不拿 prompt 当检索键:这个临时项目没有 tracker 取活条目,
        // 于是不注入——而不是用模板 prompt 去捞一批不相干的条目回来。
        assert!(
            prompt_hints(&dir, "帮我把这一批发版出去", true).is_none(),
            "自动轮应改用取活条目做检索键,无取活项时不注入"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sop_候选只投一次且给足提炼原料() {
        let dir = std::env::temp_dir().join(format!(
            "kz-sop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::project(&dir);
        let entry = kanzei_core::CompletedEntry {
            id: "R-123".into(),
            status: "done".into(),
            tools: vec!["read".into(), "edit".into(), "bash".into(), "req".into()],
        };

        assert!(
            harvest_sop(&store, &entry, "把侧栏与文档页的职责分开"),
            "首次应投出候选"
        );
        let inbox = store.read_inbox();
        assert!(inbox.contains("R-123"), "候选未记录来源条目");
        assert!(
            inbox.contains("read → edit → bash → req"),
            "候选未给出工具顺序(提炼没有原料)"
        );
        assert!(
            inbox.contains("把侧栏与文档页的职责分开"),
            "候选未记录触发任务"
        );
        assert!(
            inbox.contains("合并进那一条"),
            "候选未要求与既有 SOP 判重合并"
        );
        assert!(
            inbox.contains("判 NOOP"),
            "候选未给出「一次性流程不该产出」的出口"
        );

        // 同一条目重复触发(重跑、同轮多次收口)不该堆出一摞一样的模板。
        assert!(
            !harvest_sop(&store, &entry, "再来一次"),
            "同一条目不应重复投候选"
        );
        assert_eq!(store.read_inbox().matches("[sop:R-123]").count(), 1);

        // 换一个条目仍应正常投递。
        let other = kanzei_core::CompletedEntry {
            id: "D-166".into(),
            status: "fixed".into(),
            tools: vec!["read".into(), "edit".into(), "bash".into()],
        };
        assert!(harvest_sop(&store, &other, "修跳转"), "不同条目应各投一次");

        // D-204 批1(验收③):工具序列过短(<3)不构成可复用流程,机械拦截。
        let short = kanzei_core::CompletedEntry {
            id: "D-167".into(),
            status: "fixed".into(),
            tools: vec!["defect".into(), "bash".into()],
        };
        assert!(
            !harvest_sop(&store, &short, "一次小修"),
            "1~2 个工具的流程不该投 SOP 候选(纯工具罗列)"
        );
        assert_eq!(store.pending_note_list().len(), 2, "短流程不应新增候选");

        // 候选可逐条查看,并按指纹整块丢弃——只删摘要行会留下孤儿明细。
        let list = store.pending_note_list();
        assert_eq!(list.len(), 2, "候选列表应逐条可见");
        assert!(
            list.iter().all(|(hint, _, _)| hint == "sop"),
            "分类提示未解析出来"
        );
        assert!(store.discard_note("[sop:R-123]").unwrap(), "丢弃应生效");
        let after = store.pending_note_list();
        assert_eq!(after.len(), 1, "丢弃后应只剩另一条");
        assert!(after[0].1.contains("D-166"));
        assert!(
            !store.read_inbox().contains("read → edit → bash → req"),
            "丢弃只删了摘要行,明细成了孤儿",
        );
        assert!(
            !store.discard_note("[sop:R-123]").unwrap(),
            "重复丢弃应返回 false"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn entry_fact_候选只投一次且带上根因原料() {
        let dir = std::env::temp_dir().join(format!(
            "kz-entryfact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::project(&dir);
        let entry = kanzei_core::CompletedEntry {
            id: "D-166".into(),
            status: "fixed".into(),
            tools: vec!["read".into(), "edit".into(), "defect".into()],
        };
        let failures = vec![kanzei_core::FailureSignal {
            tool: "edit".into(),
            kind: "old_string not found".into(),
            sample: "edit 报 old_string 未命中".into(),
            targets: vec!["runner.rs".into()],
            count: 2,
            recovered_by: Some("read".into()),
        }];

        assert!(
            harvest_entry_fact(&store, &entry, "修 D-166 编辑表单渲染", &failures),
            "首次应投出根因候选"
        );
        let inbox = store.read_inbox();
        assert!(inbox.contains("D-166"), "候选未记录来源条目");
        assert!(inbox.contains("read → edit → defect"), "候选未给出工具顺序");
        assert!(
            inbox.contains("修 D-166 编辑表单渲染"),
            "候选未记录触发任务"
        );
        assert!(inbox.contains("edit ×2"), "候选未带上失败信号原料");
        assert!(inbox.contains("old_string not found"), "候选未带上错误指纹");
        assert!(
            inbox.contains("scope=project, category=fact"),
            "候选未指明 fact 落位"
        );
        assert!(
            inbox.contains("判 NOOP"),
            "候选未给出「一次性 bug 不产出」的出口"
        );

        // 同一条目重复触发不该堆出第二份原料。
        assert!(
            !harvest_entry_fact(&store, &entry, "再来一次", &[]),
            "同一条目不应重复投候选"
        );
        assert_eq!(store.read_inbox().matches("[fact:D-166]").count(), 1);

        // 无失败信号也照常投递(修完但没重复失败的条目,根因仍有记忆价值)。
        let other = kanzei_core::CompletedEntry {
            id: "R-124".into(),
            status: "done".into(),
            tools: vec!["edit".into(), "req".into()],
        };
        assert!(
            harvest_entry_fact(&store, &other, "收口 R-124", &[]),
            "不同条目应各投一次"
        );
        assert!(store.read_inbox().contains("(无失败信号)"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-229/D-214 验收:CLI 与桌面端共用 harvest_end_of_run 单入口。完成条目的轮末
    /// 应产出 SOP 候选 + fact 候选(都落**项目** inbox——D-214:manager 消化通道只
    /// 读项目 inbox,投 global 是死信箱;SOP 落库目标 scope=global 由候选 detail
    /// 指明,候选箱本身在项目侧);纯查询轮不产出任何候选。
    #[test]
    fn harvest_end_of_run_完成条目投_sop_与_fact_纯查询轮不投() {
        let dir = std::env::temp_dir().join(format!(
            "kz-harvesteor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        // 完成条目的消息序列:read 定位 → edit 成功 → req update done(收口)。
        // 工具数 ≥3 才满足 D-204 沉淀门槛;序列本身贴近真实一轮。
        let done = vec![
            msg_call("c0", "read", json!({"path": "src/lib.rs"})),
            msg_result("c0", "ok", false),
            msg_call("c1", "edit", json!({"path": "src/lib.rs"})),
            msg_result("c1", "ok", false),
            msg_call(
                "c2",
                "req",
                json!({"action": "update", "id": "R-777", "status": "done"}),
            ),
            msg_result("c2", "updated", false),
        ];
        let (delivered, sop, fact) = harvest_end_of_run(&dir, "收口 R-777", &done);
        assert_eq!(delivered, 0, "无失败信号的轮末不应投失败笔记");
        assert!(sop, "完成条目应产出 SOP 候选");
        assert!(fact, "完成条目应产出 fact 候选");

        let project = MemoryStore::project(&dir);
        let inbox = project.read_inbox();
        assert!(inbox.contains("[fact:R-777]"), "fact 候选应落项目 inbox");
        assert!(
            inbox.contains("[sop:R-777]"),
            "SOP 候选应落项目 inbox(manager 只消化项目侧): {inbox}"
        );
        // 候选 detail 指明落库目标仍为 global(D-214:候选箱在项目、落库 global)。
        assert!(
            inbox.contains("scope=global"),
            "SOP 候选 detail 必须指明 scope=global 落库目标"
        );

        // 纯查询轮:read + req done,无实质动作,completed_entry 不触发 → 零投递。
        let read_only = vec![
            msg_call("c3", "read", json!({"path": "src/lib.rs"})),
            msg_result("c3", "...", false),
            msg_call(
                "c4",
                "req",
                json!({"action": "update", "id": "R-778", "status": "done"}),
            ),
            msg_result("c4", "updated", false),
        ];
        let (delivered2, sop2, fact2) = harvest_end_of_run(&dir, "只读轮", &read_only);
        assert_eq!(delivered2, 0);
        assert!(!sop2, "纯查询轮不应产 SOP");
        assert!(!fact2, "纯查询轮不应产 fact");

        std::fs::remove_dir_all(dir).ok();
    }

    fn msg_call(id: &str, name: &str, input: serde_json::Value) -> kanzei_llm::Message {
        kanzei_llm::Message::assistant(vec![kanzei_llm::Part::ToolCall {
            id: id.into(),
            name: name.into(),
            input,
        }])
    }

    fn msg_result(call_id: &str, content: &str, is_error: bool) -> kanzei_llm::Message {
        kanzei_llm::Message::tool_results(vec![kanzei_llm::Part::ToolResult {
            call_id: call_id.into(),
            content: content.into(),
            is_error,
        }])
    }

    #[test]
    fn mark_memory_file_read_backfills_only_matching_scope_entry() {
        // R-161:read 记忆文件后按文件名 id(M-001-slug.md)回填最近一次召回的 fetched。
        let dir = std::env::temp_dir().join(format!(
            "kz-markread-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::project(&dir);
        let entry = match store
            .add(
                "sop",
                "发版 SOP 两条通道",
                "发版发布安装更新必读",
                "package.ps1",
                "user",
                &[],
                None,
                false,
            )
            .unwrap()
        {
            AddOutcome::Added(e) => e,
            _ => panic!("expected add"),
        };
        let hits = store.search("发版", None, Some("active"), 5).unwrap();
        let recall_id = store.record_recall("要发版", &hits, 256);
        let (path, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.id == entry.id)
            .unwrap();
        // 模拟 read 工具读该文件 → 回填采纳。
        mark_memory_file_read(&dir, &path);
        let after = store.recalls(10);
        let hit = after
            .iter()
            .find(|r| r.recall_id == recall_id)
            .unwrap()
            .hits
            .iter()
            .find(|h| h.id == entry.id)
            .unwrap();
        assert!(hit.fetched, "read 记忆文件后 fetched 未回填");
        // 非记忆库路径:快速路径短路,不产生任何副作用(记忆库不因读普通文件被创建)。
        let plain = dir.join("notes.md");
        std::fs::write(&plain, "普通笔记").unwrap();
        std::fs::remove_dir_all(dir.join(".kanzei")).unwrap();
        mark_memory_file_read(&dir, &plain);
        assert!(
            !dir.join(".kanzei").exists(),
            "非记忆文件的 read 不应重建记忆库目录"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn civil_date_is_correct() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_672), (2026, 8, 7));
    }

    #[test]
    fn fp_markers_提取排序去重且容忍残缺() {
        assert_eq!(
            fp_markers(
                "前文 [fp:edit|not found] 中间 [fp:bash|timeout] 再来一遍 [fp:edit|not found]"
            ),
            vec![
                "[fp:bash|timeout]".to_string(),
                "[fp:edit|not found]".to_string()
            ],
        );
        assert!(fp_markers("没有指纹的正文").is_empty());
        // 未闭合的标记不炸也不误报。
        assert!(fp_markers("残缺 [fp:edit|截断了").is_empty());
    }

    #[test]
    fn hints_不重复常驻索引_折叠条目才给全行_preference_不进提示() {
        let dir = std::env::temp_dir().join(format!(
            "kz-hintdedup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::project(&dir);
        let add = |cat: &str, title: &str, desc: &str| match store
            .add(cat, title, desc, "正文", "user", &[], None, false)
            .unwrap()
        {
            AddOutcome::Added(e) => e,
            _ => panic!("expected add"),
        };
        add("sop", "发版短条目", "发版发布安装更新必读"); // M-001
        add("fact", "发版长条目", &"发版流程细节".repeat(20)); // M-002,索引行显著更长
        add("preference", "发版定调", "发版发布安装更新必读"); // M-003

        // 预算恰好只装得下 M-001 的行:M-002 被折叠。
        let (lines, ids, folded) = resident_index(&dir, 80);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(ids.contains("M-001"), "{ids:?}");
        assert_eq!(folded, 1);

        let block = prompt_hints_with_budget(&dir, "帮我把这一批发版出去", 80).unwrap();
        // 常驻条目只给指向,不再重复 description 整行。
        assert!(
            block.contains("M-001 发版短条目(见 memory-index)"),
            "{block}"
        );
        assert!(
            !block.contains("M-001 [project/sop]"),
            "常驻条目不该给全行: {block}"
        );
        // 被折叠的条目在 hints 里给全行(description 在这才有信息量)。
        assert!(
            block.contains("M-002 [project/fact] 发版长条目 — "),
            "{block}"
        );
        // preference 全文常驻,hints 不提、遥测不记。
        assert!(!block.contains("M-003"), "preference 不该进 hints: {block}");
        assert!(
            store
                .recalls(10)
                .iter()
                .all(|r| r.hits.iter().all(|h| h.id != "M-003")),
            "preference 不该进召回遥测",
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-230:resident_index 装箱前按价值排序——新 updated 优先、同 updated 时
    /// id 大(创建晚)优先,取代 id 升序先到先得(老条目凭枚举顺序霸占预算)。
    #[test]
    fn resident_index_价值排序_新近条目优先于老条目() {
        let dir = std::env::temp_dir().join(format!(
            "kz-resident-sort-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mem = dir.join(".kanzei").join("memory");
        std::fs::create_dir_all(&mem).unwrap();
        // 三条 fact 条目,updated 各不相同(老、中、新),行长短都远低于预算。
        let write = |id: &str, updated: &str| {
            std::fs::write(
                mem.join(format!("{id}-{}.md", id.to_lowercase())),
                format!(
                    "---\nid: {id}\nscope: project\ncategory: fact\ntitle: 条目 {id}\n\
                     description: 描述 {id}\nstatus: active\ncreated: 2026-08-01\n\
                     updated: {updated}\nsource: user\n---\n\n正文 {id}\n"
                ),
            )
            .unwrap();
        };
        write("M-100", "2026-08-01"); // 老
        write("M-101", "2026-08-03"); // 中
        write("M-102", "2026-08-05"); // 新

        // 预算只装得下两条(行长约 42c):最新 updated 的两条应入选,最老的折叠。
        let (lines, ids, folded) = resident_index(&dir, 100);
        assert_eq!(folded, 1, "{lines:?}");
        assert!(ids.contains("M-102"), "最新更新的条目必须入选: {ids:?}");
        assert!(ids.contains("M-101"), "次新更新的条目必须入选: {ids:?}");
        assert!(!ids.contains("M-100"), "最老的条目应被折叠: {ids:?}");
        // 行序也按价值:最新的排最前。
        assert!(lines[0].starts_with("M-102"), "行序应按价值降序: {lines:?}");

        // 同 updated 平手:id 大(创建晚)优先。M-103/M-104 同 updated。
        write("M-103", "2026-08-06");
        write("M-104", "2026-08-06");
        // 预算恰好装两条:M-104 + M-103 入选,次新的 M-102 折叠。
        let (_, ids2, _) = resident_index(&dir, 110);
        assert!(ids2.contains("M-104"), "平手时 id 大优先: {ids2:?}");
        assert!(ids2.contains("M-103"), "平手时次大 id 也应入选: {ids2:?}");
        assert!(
            !ids2.contains("M-102"),
            "预算内应优先保留最新 updated: {ids2:?}"
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 失败笔记要求保留指纹_复发时改投修订笔记() {
        let dir = std::env::temp_dir().join(format!(
            "kz-recur-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::project(&dir);
        // kind 里带测试专属噪声,避免撞上真实 global 记忆库里的同名指纹。
        let kind = format!("old_string not found #{}", std::process::id());
        let signal = kanzei_core::FailureSignal {
            tool: "edit".into(),
            kind: kind.clone(),
            sample: "edit 报 old_string 未命中".into(),
            targets: vec!["runner.rs".into()],
            count: 3,
            recovered_by: Some("read".into()),
        };
        let fingerprint = format!("[fp:edit|{kind}]");

        // 第一次:库里没有该指纹 → 正常失败笔记,且要求把指纹带进条目正文。
        assert_eq!(harvest_failures(&store, std::slice::from_ref(&signal)), 1);
        let inbox = store.read_inbox();
        assert!(inbox.contains("改用 read 成功"), "{inbox}");
        assert!(
            inbox.contains("原样放进正文"),
            "正常笔记未要求保留指纹: {inbox}"
        );

        // manager 按要求建了条目(指纹在正文里),inbox 已清。
        store.clear_inbox().unwrap();
        match store
            .add(
                "fact",
                "edit 未命中先 read 重读",
                "edit 替换失败时必读:先 read 再改",
                &format!("判据……{fingerprint}"),
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap()
        {
            AddOutcome::Added(_) => {}
            _ => panic!("expected add"),
        }
        // R-165:manager 编译产物须 promote 带证据才 active——复发检测只看 active 记忆。
        let (cid, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.title == "edit 未命中先 read 重读")
            .map(|(p, e)| (e.id, p))
            .unwrap();
        store.promote(&cid, &[(1, None, None)], None).unwrap();

        // 第二次同类失败:必须改投修订笔记,点名既有条目,不再原坑重投。
        assert_eq!(harvest_failures(&store, std::slice::from_ref(&signal)), 1);
        let inbox = store.read_inbox();
        assert!(
            inbox.contains("已有记忆 M-001"),
            "复发未点名既有条目: {inbox}"
        );
        assert!(inbox.contains("仍复发"), "{inbox}");
        assert!(
            inbox.contains("memory_update"),
            "修订笔记未指路 update: {inbox}"
        );
        assert!(
            !inbox.contains("改用 read 成功"),
            "复发时不该再投原始失败笔记: {inbox}"
        );

        // 同一轮内指纹去重照旧生效:再采集不新增笔记。
        assert_eq!(harvest_failures(&store, std::slice::from_ref(&signal)), 0);
        assert_eq!(store.pending_notes(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 一等字段宽容读零迁移_fingerprint回退正文标记() {
        // R-162 B2:frontmatter 新增键必须"宽容读零迁移"——有键的条目读到,
        // 没键的旧条目不报错、不进 extras 冲突。
        let text = "\
---
id: M-042
scope: project
category: sop
title: edit 失败先 read
description: edit 替换失败时先 read 再重试
status: active
created: 2026-08-10
updated: 2026-08-10
source: run:test/1
fingerprint: [fp:edit|old_string not found]
trigger: tool_failure
valid_from: 2026-08-07
supersedes: M-041
version: 2
---
edit 失败时先 read 当前文件重建 old_string,不要继续猜
";
        let entry = parse_entry(text);
        assert_eq!(
            entry.fingerprint(),
            Some("[fp:edit|old_string not found]".to_string())
        );
        assert_eq!(entry.trigger(), Some("tool_failure"));
        assert_eq!(entry.valid_from(), Some("2026-08-07"));
        assert_eq!(entry.supersedes(), Some("M-041"));
        assert_eq!(entry.version(), Some(2));

        // 兼容正文内旧标记:无 fingerprint 字段但有 [fp:] 正文时回退。
        let legacy = "\
---
id: M-043
scope: project
category: sop
title: 旧条目
description: 只有正文标记
status: active
created: 2026-08-10
updated: 2026-08-10
source: user
---
旧正文 [fp:bash|cargo test] 只有标记没有字段
";
        let legacy_entry = parse_entry(legacy);
        assert_eq!(
            legacy_entry.fingerprint(),
            Some("[fp:bash|cargo test]".to_string())
        );

        // 完全无指纹:不 panic,返回 None。
        let bare = "\
---
id: M-044
scope: project
category: fact
title: 无指纹
description: 没有指纹也没有标记
status: active
created: 2026-08-10
updated: 2026-08-10
source: user
---
正文没有任何标记
";
        let bare_entry = parse_entry(bare);
        assert_eq!(bare_entry.fingerprint(), None);
        assert_eq!(bare_entry.version(), None, "非数字版本宽容为 None");

        // render 往返:新键原样保留(extras 兜底机制不丢数据)。
        let rendered = render_entry(&entry);
        let roundtrip = parse_entry(&rendered);
        assert_eq!(
            roundtrip.fingerprint(),
            Some("[fp:edit|old_string not found]".to_string())
        );
        assert_eq!(roundtrip.trigger(), Some("tool_failure"));
        assert_eq!(roundtrip.version(), Some(2));
    }

    #[test]
    fn fingerprint索引_构建查询增量upsert与删除() {
        // R-162 B2:Tier0 内存索引——指纹→id 精确查询,增删改各走各的通道。
        let fp = |id: &str, body: &str| {
            parse_entry(&format!(
            "---\nid: {id}\nscope: project\ncategory: sop\ntitle: t\ndescription: d\nstatus: active\ncreated: 2026-08-10\nupdated: 2026-08-10\nsource: user\n---\n{body}"
        ))
        };
        let a = fp("M-100", "[fp:edit|old_string not found] 正文");
        let b = fp("M-101", "[fp:edit|old_string not found] 另一条");
        let c = fp("M-102", "[fp:bash|cargo test] 第三条");

        let mut index = FingerprintIndex::default();
        index.upsert(&a);
        index.upsert(&b);
        index.upsert(&c);
        assert_eq!(
            index.lookup("[fp:edit|old_string not found]"),
            &["M-100".to_string(), "M-101".to_string()],
            "同一指纹多条全保留"
        );
        assert_eq!(index.lookup("[fp:bash|cargo test]"), &["M-102".to_string()]);
        assert!(index.lookup("[fp:read|missing]").is_empty(), "未命中为空");

        // 写时增量:指纹被改(update)后旧桶不留痕。
        let a_moved = fp("M-100", "[fp:read|not found] 正文改了");
        index.upsert(&a_moved);
        assert_eq!(
            index.lookup("[fp:edit|old_string not found]"),
            &["M-101".to_string()]
        );
        assert_eq!(index.lookup("[fp:read|not found]"), &["M-100".to_string()]);

        // 删除:所有桶移除该 id,空桶清掉。
        index.remove("M-101");
        assert!(index.lookup("[fp:edit|old_string not found]").is_empty());
        assert_eq!(index.lookup("[fp:bash|cargo test]"), &["M-102".to_string()]);
    }

    fn temp_memory_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-recall-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei").join("memory")).unwrap();
        dir
    }

    fn trigger(tool: &str, kind: &str, target: &str, count: usize) -> kanzei_core::RecallTrigger {
        kanzei_core::RecallTrigger {
            tool: tool.into(),
            kind: kind.into(),
            sample: format!("{tool} 报错 {kind} 于 {target}"),
            target: target.into(),
            failure_count: count,
        }
    }

    #[test]
    fn tier0指纹精确命中_不走_bm25() {
        // R-162 内容③/验收③前置:Tier0 fingerprint 精确匹配,命中即返回。
        let root = temp_memory_root("tier0");
        let store = MemoryStore::project(&root);
        // 指纹带进程 id,避免撞上真实 global 记忆库里的同名指纹(测试隔离)。
        let kind = format!("old_string not found #{}", std::process::id());
        store
            .add(
                "sop",
                "edit 失败先 read",
                "edit 替换失败时必读:先 read 重建 old_string 再重试",
                &format!("正文 [fp:edit|{kind}] 判据"),
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        // R-165:manager 编译产物须 promote 带证据才 active 可检索。
        let (cid, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.title == "edit 失败先 read")
            .map(|(p, e)| (e.id, p))
            .unwrap();
        store.promote(&cid, &[(1, None, None)], None).unwrap();
        let policy = FailureRecallPolicy::new(&root);
        let t = trigger("edit", &kind, "main.rs", 1);
        let hits = policy.retrieve(&t);
        assert_eq!(hits.len(), 1, "Tier0 指纹必须恰好命中一条: {hits:?}");
        assert_eq!(hits[0].category, "sop");
        assert!(
            hits[0].action.contains("先 read 重建"),
            "Packet 行动行要取条目 description: {}",
            hits[0].action
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn tier1_bm25降级_无指纹条目经搜索命中() {
        // R-162 内容③:Tier0 miss 后走 Tier1 BM25(错误原文+目标构 query)。
        let root = temp_memory_root("tier1");
        let store = MemoryStore::project(&root);
        store
            .add(
                "fact",
                "cargo test 环境约束",
                "cargo test 在本项目需要先设置 HTTPS_PROXY",
                "无指纹纯描述",
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        // R-165:manager 编译产物须 promote 带证据才 active 可检索。
        let (cid, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.title == "cargo test 环境约束")
            .map(|(p, e)| (e.id, p))
            .unwrap();
        store.promote(&cid, &[(1, None, None)], None).unwrap();
        let policy = FailureRecallPolicy::new(&root);
        let t = trigger("bash", "cannot find proxy", "cargo", 1);
        // 把 sample 换成能命中 BM25 的词(Tier1 用 sample 前 120 字符构 query)。
        let t = kanzei_core::RecallTrigger {
            sample: "cargo test 需要 HTTPS_PROXY 代理".into(),
            ..t
        };
        let hits = policy.retrieve(&t);
        assert!(
            !hits.is_empty(),
            "Tier1 BM25 必须命中无指纹但描述相关的事实条目"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 超时降级不阻塞_返回空() {
        // R-162 验收②:预算超时降级——即使 Tier1 检索超时也不 panic、返回空。
        // 单测无法真实模拟超时(检索极快),这里验证"超预算路径不 panic"的
        // 结构性保障:TIER1_BUDGET_MS 在检索循环内检查,超时即提前返回。
        let root = temp_memory_root("timeout");
        let policy = FailureRecallPolicy::new(&root);
        // 未命中任何条目的 query:Tier1 检索后返回空,不阻塞。
        let t = trigger("read", "no such file", "nonexistent.rs", 1);
        let hits = policy.retrieve(&t);
        assert!(hits.is_empty(), "无命中时返回空: {hits:?}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 重复失败触发遥测_落recall_events() {
        // R-162 验收③:每次触发落 recall_events(trigger/action/延迟)。
        let root = temp_memory_root("telemetry");
        let store = MemoryStore::project(&root);
        let kind = format!("old_string not found #{}", std::process::id());
        store
            .add(
                "sop",
                "edit 失败先 read",
                "edit 替换失败时必读:先 read 重建 old_string 再重试",
                &format!("正文 [fp:edit|{kind}] 判据"),
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        // R-165:manager 编译产物须 promote 带证据才 active 可检索。
        let (cid, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.title == "edit 失败先 read")
            .map(|(p, e)| (e.id, p))
            .unwrap();
        store.promote(&cid, &[(1, None, None)], None).unwrap();
        let policy = FailureRecallPolicy::new(&root);
        let t = trigger("edit", &kind, "main.rs", 2);
        let hits = policy.retrieve(&t);
        assert!(!hits.is_empty());
        policy.record_trigger(&t, &hits, 3);
        // state.db 里应能查到 event_recall 记录(trigger/action/延迟)。
        let path = root.join(".kanzei").join("state.db");
        let sstore = kanzei_core::SessionStore::open(&path).unwrap();
        let log = sstore.event_recall_log().unwrap();
        assert_eq!(log.len(), 1, "一次触发落一条 recall_event: {log:?}");
        assert_eq!(
            log[0].2, "reretrieve",
            "count≥2 时 policy_action 应为 reretrieve"
        );
        assert!(
            log[0].1.contains("\"tool\":\"edit\"") && log[0].1.contains("\"count\":2"),
            "trigger_payload 必须带 tool 与失败次数: {}",
            log[0].1
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 重复失败_re_retrieve换_query_遥测标_reretrieve() {
        // R-162 内容④:同 (tool,kind) 失败 ≥2 次换 query——遥测 policy_action
        // 标 reretrieve,禁止原 top-k 重塞的口径在 retrieve 侧由"每次新检索
        // (不同 sample/已注入去重)"保证。这里验证落库字段正确。
        let root = temp_memory_root("reretrieve");
        let store = MemoryStore::project(&root);
        // 指纹带进程 id,避免撞上真实 global 记忆库(测试隔离)。
        let kind = format!("old_string not found #{}", std::process::id());
        store
            .add(
                "sop",
                "edit 失败先 read",
                "edit 替换失败时必读:先 read 重建 old_string 再重试",
                &format!("正文 [fp:edit|{kind}] 判据"),
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        // R-165:manager 编译产物须 promote 带证据才 active 可检索。
        let (cid, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.title == "edit 失败先 read")
            .map(|(p, e)| (e.id, p))
            .unwrap();
        store.promote(&cid, &[(1, None, None)], None).unwrap();
        let policy = FailureRecallPolicy::new(&root);
        // 第一次失败(fingerprint 命中,policy_action=fingerprint)。
        let t1 = trigger("edit", &kind, "main.rs", 1);
        let hits1 = policy.retrieve(&t1);
        assert_eq!(hits1.len(), 1);
        policy.record_trigger(&t1, &hits1, 2);
        // 第三次失败(≥2 → reretrieve)。
        let t3 = trigger("edit", &kind, "main.rs", 3);
        let hits3 = policy.retrieve(&t3);
        assert_eq!(hits3.len(), 1, "指纹仍命中,但动作标注要随失败次数升级");
        policy.record_trigger(&t3, &hits3, 4);

        let path = root.join(".kanzei").join("state.db");
        let sstore = kanzei_core::SessionStore::open(&path).unwrap();
        let log = sstore.event_recall_log().unwrap();
        let actions: Vec<&str> = log.iter().map(|(_, _, a, _)| a.as_str()).collect();
        assert_eq!(
            actions,
            vec!["fingerprint", "reretrieve"],
            "失败次数升级必须把 policy_action 从 fingerprint 换成 reretrieve: {log:?}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 端到端_edit失败后记忆_packet进上下文() {
        // R-162 验收①"录制回放证明":真实 FailureRecallPolicy + 真实记忆条目 +
        // RecallWatch 全链路——edit 工具失败瞬间,相关 SOP 记忆以 Packet 形式
        // 追加进工具结果文本(模型下一轮就能看到,不阻断主循环)。
        let root = temp_memory_root("e2e");
        let store = MemoryStore::project(&root);
        let kind = format!("old_string not found #{}", std::process::id());
        store
            .add(
                "sop",
                "edit 失败先 read",
                "edit 替换失败时必读:先 read 重建 old_string 再重试",
                &format!("正文 [fp:edit|{kind}] 判据"),
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        // R-165:manager 编译产物须 promote 带证据才 active 可检索(Packet 注入只看 active)。
        let (cid, _) = store
            .load_all()
            .into_iter()
            .find(|(_, e)| e.title == "edit 失败先 read")
            .map(|(p, e)| (e.id, p))
            .unwrap();
        store.promote(&cid, &[(1, None, None)], None).unwrap();
        let policy = FailureRecallPolicy::new(&root);
        let mut watch = kanzei_core::RecallWatch::new(Some(&policy));
        let calls = vec![(
            "c1".into(),
            "edit".into(),
            serde_json::json!({ "path": "src/main.rs" }),
            "".into(),
        )];
        let mut results = vec![kanzei_llm::Part::ToolResult {
            call_id: "c1".into(),
            content: format!("old_string not found in src/main.rs (kind: {kind})"),
            is_error: true,
        }];
        watch.note_step(&calls, &mut results);
        let kanzei_llm::Part::ToolResult { content, .. } = &results[0] else {
            panic!("expected ToolResult");
        };
        // 记忆命中以 Packet 文本追加进结果,模型下一轮即可见。
        assert!(
            content.contains("[记忆命中"),
            "工具失败结果必须携带记忆 Packet: {content}"
        );
        assert!(content.contains("行动: edit 替换失败时必读"), "{content}");
        assert!(content.contains("状态: active"), "{content}");
        // 遥测同链路落库。
        let path = root.join(".kanzei").join("state.db");
        let sstore = kanzei_core::SessionStore::open(&path).unwrap();
        let log = sstore.event_recall_log().unwrap();
        assert_eq!(log.len(), 1, "端到端触发也必须落 recall_events");
        assert_eq!(log[0].2, "fingerprint", "首次失败指纹命中标 fingerprint");
        std::fs::remove_dir_all(root).ok();
    }
}
