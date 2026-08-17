//! 检索候选访问与查询构造(R-255 第三刀,从 store.rs 迁出)。
//!
//! `search_candidates` 是 FTS 存储侧候选集访问(不排序决策);`classify_novelty`
//! 是准入的语义探测(依赖候选集);`intent_query`/`fts_query`/`segment_cjk` 等是
//! 查询构造的机械辅助;`find_by_marker`/`recall_profile` 是精确查与召回画像。

use std::collections::BTreeMap;

use rusqlite::params;

use crate::memory::admission::{is_cjk, normalize_title, title_containment, TITLE_DUP_THRESHOLD};
use crate::memory::store::{MemoryStore, SearchCandidate};
use crate::memory::{MemoryEntry, MemoryScope, Novelty};

impl MemoryStore {
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
        if status.is_some() {
            sql.push_str(if category.is_some() {
                " AND status = ?3"
            } else {
                " AND status = ?2"
            });
        }
        sql.push_str(" ORDER BY bm25(memory_fts) LIMIT 24");
        let mut statement = conn.prepare(&sql)?;
        let rows: Vec<(String, String, f64)> = match (category, status) {
            (Some(cat), Some(want)) => statement
                .query_map(params![match_expr, cat, want], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<Result<_, _>>()?,
            (Some(cat), None) => statement
                .query_map(params![match_expr, cat], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<Result<_, _>>()?,
            (None, Some(want)) => statement
                .query_map(params![match_expr, want], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<Result<_, _>>()?,
            (None, None) => statement
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

    /// 检索命中追踪的判据入口(R-165 批2 novelty gate 三档):明显新 → PROPOSE、
    /// 明显重复 → NOOP、不确定 → 才起 LLM 判断(验收④)。
    /// 机械判据:标题规范化精确命中既有 active 记忆 = 明显重复;
    /// FTS 无任何命中 = 明显新;有命中但非精确 = 不确定。
    /// R-216 下沉为 add 硬闸,并扩到双 scope 探测(project + global 都查 active)。
    /// 返回 (判定, 候选)。语义探测 = description+body 的 FTS 命中:有命中即
    /// Uncertain(候选返回),无命中 New;精确标题重复 Duplicate。
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

    /// 精确子串查找 active/candidate 条目正文里的指纹标记(复发检测,R-149)。
    /// 不用 FTS 相似度:弱模型只需原样复制标记,引擎侧零阈值、可单测。
    /// **active 与 candidate 都算**——candidate 看不见正是重复条目的生产线
    /// (2026-08-12 清理时一个坑堆了 8 条 candidate)。匹配走归一后的标记比对。
    pub fn find_by_marker(&self, marker: &str) -> Option<MemoryEntry> {
        let key = kanzei_core::normalize_fp_marker(marker);
        let mut hit: Option<MemoryEntry> = None;
        for (_, entry) in self.load_all() {
            if entry.status != "active" && entry.status != "candidate" {
                continue;
            }
            let matched = crate::memory::fp_markers(&entry.body)
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

    /// 召回画像:entry_id → (召回次数, 被取用次数)。取自 memory_recalls。
    pub fn recall_profile(&self) -> BTreeMap<String, (u64, u64)> {
        let mut out = BTreeMap::new();
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
}

/// unicode61 把连续 CJK 当单个整词,子串查不到(拍板点③的即时实证)。
/// 零依赖解法:索引与查询两侧都做 CJK 单字切分;查询侧每个用户词作为
/// 相邻单字的短语匹配,保住精度;词间 OR 联接靠 bm25 排相关度。
pub(crate) fn segment_cjk(text: &str) -> String {
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
