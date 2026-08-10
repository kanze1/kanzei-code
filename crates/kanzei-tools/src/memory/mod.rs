//! Memory 系统(R-103/R-104):文件优先的分级记忆。
//! 真源是 markdown 文件(一条一文件+平铺 frontmatter),人可编辑、git 可恢复;
//! SQLite 只存可重建派生物(FTS 索引/hits)。设计基线 docs/design/memory_system.md。
//!
//! 分级:scope(Global=~/.kanzei/memory, Project=<root>/.kanzei/memory)
//!     × category(preference/habit/fact/sop;episode 走 state.db 不落文件)。

mod manager;
mod store;
mod tools;

pub use manager::{manager_agent, MemoryManagerComponent};
pub use store::{AddOutcome, MemoryStore, RecallHit, RecallRound, SearchHit};
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
pub const STATUSES: &[&str] = &["active", "stale"];

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
                "status" => entry.status = value.into(),
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

/// 常驻索引的预算走查(D-216):dev/memory 注入与 prompt_hints 必须对同一份口径,
/// 否则 hints 会重复注入常驻索引里已有的行。返回 (预算内的行, 预算内 id 集, 折叠条数)。
/// 与注入侧同规则:continue 跳过放不下的、不 break,超长行不得埋掉后面的短行。
pub fn resident_index(
    project_root: &std::path::Path,
    budget: usize,
) -> (Vec<String>, std::collections::HashSet<String>, usize) {
    let mut all: Vec<(String, String)> = Vec::new();
    let mut stores = vec![MemoryStore::project(project_root)];
    stores.extend(MemoryStore::global());
    for store in &stores {
        for (_, e) in store.load_all() {
            if e.status != "active" || e.category == "preference" {
                continue;
            }
            all.push((
                e.id.clone(),
                format!(
                    "{} [{}/{}] {} — {}",
                    e.id, e.scope, e.category, e.title, e.description
                ),
            ));
        }
    }
    let mut lines = Vec::new();
    let mut ids = std::collections::HashSet::new();
    let mut remaining = budget;
    let mut folded = 0usize;
    for (id, line) in all {
        let cost = line.chars().count() + 1;
        if cost > remaining {
            folded += 1;
            continue;
        }
        remaining -= cost;
        ids.insert(id);
        lines.push(line);
    }
    (lines, ids, folded)
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
        // 复发检测(R-149):指纹已在某条 active 记忆正文里,同类失败却仍出现——
        // 记忆在、坑还在 = 它没进决策。投修订笔记点名该条目,而不是原坑重投。
        let existing = store.find_active_by_marker(&fingerprint).or_else(|| {
            global
                .as_ref()
                .and_then(|g| g.find_active_by_marker(&fingerprint))
        });
        if let Some(entry) = existing {
            let summary = format!(
                "已有记忆 {} 但 {} 同类失败本轮仍复发({} 次){}",
                entry.id, signal.tool, signal.count, fingerprint
            );
            let detail = format!(
                "- 既有条目: {}《{}》\n- 错误原文: {}\n- 判断要点: 记忆存在但没拦住复发,说明它没进决策。用 memory_update 修订该条(补判据/改 description 召回钩子,正文里的 {} 标记必须保留);只有确认这是另一个坑才新增,不要原样再记一遍。",
                entry.id,
                entry.title,
                signal.sample.replace('\n', " "),
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
        let detail = format!(
            "- 错误原文: {}\n- 涉及目标: {}\n- 判断要点: 这是环境/工具契约类的可复用知识,还是本次任务内的一次性噪声(例如 TDD 里预期的测试失败、自己写错又立刻改对的编译错误)?是前者才建条目,后者判 NOOP。\n- 指纹: 建条目时把 {} 原样放进正文——它是复发检测的键,丢了引擎就看不见「记了但没用」。",
            signal.sample.replace('\n', " "),
            if signal.targets.is_empty() {
                "(无)".to_string()
            } else {
                signal.targets.join(", ")
            },
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
    // 工具序列是提炼步骤的原料;它同时也是判重依据——流程一样的条目应当合并而非新增。
    let flow = entry.tools.join(" → ");
    let summary = format!(
        "候选 SOP:完成 {}({})的流程{}",
        entry.id, entry.status, fingerprint
    );
    let detail = format!(
        "- 触发任务: {}\n\
         - 实际工具顺序: {}\n\
         - 请提炼成可复用步骤(祈使句、按顺序、每步说清做什么与判断依据),写进 category=sop、scope=global 的候选。\n\
         - 判重: 若已有 SOP 的步骤实质相同,合并进那一条并补充差异,不要新增。\n\
         - 若这段流程只对本条目成立(一次性排查、与具体 id 强绑定),判 NOOP 不要产出。",
        prompt.chars().take(200).collect::<String>(),
        if flow.is_empty() { "(无)".to_string() } else { flow },
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

/// 开跑预检索(R-106):拿用户 prompt 对两级记忆做一次 BM25,命中则返回
/// 提示块(只给索引行不给正文,拉正文是模型自己的决定)。无命中返回 None。
pub fn prompt_hints(project_root: &std::path::Path, prompt: &str) -> Option<String> {
    prompt_hints_with_budget(project_root, prompt, MEMORY_CONTEXT_BUDGET)
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
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let Some(memory_id) = file_name.split('-').next() else {
        return;
    };
    if memory_id.is_empty() {
        return;
    }
    let mut stores = vec![MemoryStore::project(project_root)];
    stores.extend(MemoryStore::global());
    for store in stores {
        if store.scope.label() == "project" && !path.starts_with(&store.root) {
            continue;
        }
        if store.scope.label() == "global" && !path.starts_with(&store.root) {
            continue;
        }
        store.mark_recall_fetched(memory_id);
    }
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
        let hit = prompt_hints(&dir, "帮我把这一批发版出去");
        assert!(hit.is_some());
        assert!(hit.unwrap().contains("M-001"), "提示块应含索引行");
        assert!(prompt_hints(&dir, "完全无关的宇宙话题").is_none());
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
            tools: vec!["edit".into()],
        };
        assert!(harvest_sop(&store, &other, "修跳转"), "不同条目应各投一次");

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
}
