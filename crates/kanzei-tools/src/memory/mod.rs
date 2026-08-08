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

/// global scope 根目录;KANZEI_HOME 供测试与多环境覆盖。
pub fn global_memory_root() -> Option<PathBuf> {
    let home = std::env::var("KANZEI_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".kanzei")))?;
    Some(home.join("memory"))
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

/// 每轮最多投递的失败草稿条数:防止一轮异常把 inbox 灌爆、manager 被撑死。
const MAX_FAILURE_NOTES_PER_RUN: usize = 3;

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
/// 值不值得写成记忆条目仍由 memory-manager 判定——引擎不做语义判断。
pub fn harvest_failures(
    store: &MemoryStore,
    signals: &[kanzei_core::FailureSignal],
) -> usize {
    let mut delivered = 0usize;
    for signal in signals {
        if delivered >= MAX_FAILURE_NOTES_PER_RUN {
            break;
        }
        let fingerprint = format!("[fp:{}|{}]", signal.tool, signal.kind);
        if store.note_fingerprint_seen(&fingerprint) {
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
            "- 错误原文: {}\n- 涉及目标: {}\n- 判断要点: 这是环境/工具契约类的可复用知识,还是本次任务内的一次性噪声(例如 TDD 里预期的测试失败、自己写错又立刻改对的编译错误)?是前者才建条目,后者判 NOOP。",
            signal.sample.replace('\n', " "),
            if signal.targets.is_empty() {
                "(无)".to_string()
            } else {
                signal.targets.join(", ")
            },
        );
        if store.append_note(&summary, &detail, "fact").is_ok() {
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
pub fn harvest_sop(
    store: &MemoryStore,
    entry: &kanzei_core::CompletedEntry,
    prompt: &str,
) -> bool {
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
    store.append_note(&summary, &detail, "sop").is_ok()
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
    let summary = format!("完成 {}({})的根因候选{}", entry.id, entry.status, fingerprint);
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
    store.append_note(&summary, &detail, "fact").is_ok()
}

/// 开跑预检索(R-106):拿用户 prompt 对两级记忆做一次 BM25,命中则返回
/// 提示块(只给索引行不给正文,拉正文是模型自己的决定)。无命中返回 None。
pub fn prompt_hints(project_root: &std::path::Path, prompt: &str) -> Option<String> {
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut stores = vec![MemoryStore::project(project_root)];
    stores.extend(MemoryStore::global());
    for store in &stores {
        if let Ok(found) = store.search(prompt, None, Some("active"), 3) {
            hits.extend(found);
        }
    }
    if hits.is_empty() {
        return None;
    }
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(3);
    let lines: Vec<String> = hits
        .iter()
        .map(|h| {
            format!(
                "{} [{}/{}] {} — {}",
                h.entry.id, h.entry.scope, h.entry.category, h.entry.title, h.entry.description
            )
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
            .add("sop", "发版 SOP 两条通道", "发版发布安装更新必读", "package.ps1", "user", false)
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

        assert!(harvest_sop(&store, &entry, "把侧栏与文档页的职责分开"), "首次应投出候选");
        let inbox = store.read_inbox();
        assert!(inbox.contains("R-123"), "候选未记录来源条目");
        assert!(inbox.contains("read → edit → bash → req"), "候选未给出工具顺序(提炼没有原料)");
        assert!(inbox.contains("把侧栏与文档页的职责分开"), "候选未记录触发任务");
        assert!(inbox.contains("合并进那一条"), "候选未要求与既有 SOP 判重合并");
        assert!(inbox.contains("判 NOOP"), "候选未给出「一次性流程不该产出」的出口");

        // 同一条目重复触发(重跑、同轮多次收口)不该堆出一摞一样的模板。
        assert!(!harvest_sop(&store, &entry, "再来一次"), "同一条目不应重复投候选");
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
        assert!(list.iter().all(|(hint, _, _)| hint == "sop"), "分类提示未解析出来");
        assert!(store.discard_note("[sop:R-123]").unwrap(), "丢弃应生效");
        let after = store.pending_note_list();
        assert_eq!(after.len(), 1, "丢弃后应只剩另一条");
        assert!(after[0].1.contains("D-166"));
        assert!(
            !store.read_inbox().contains("read → edit → bash → req"),
            "丢弃只删了摘要行,明细成了孤儿",
        );
        assert!(!store.discard_note("[sop:R-123]").unwrap(), "重复丢弃应返回 false");
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
        assert!(inbox.contains("修 D-166 编辑表单渲染"), "候选未记录触发任务");
        assert!(inbox.contains("edit ×2"), "候选未带上失败信号原料");
        assert!(inbox.contains("old_string not found"), "候选未带上错误指纹");
        assert!(inbox.contains("scope=project, category=fact"), "候选未指明 fact 落位");
        assert!(inbox.contains("判 NOOP"), "候选未给出「一次性 bug 不产出」的出口");

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
        assert!(harvest_entry_fact(&store, &other, "收口 R-124", &[]), "不同条目应各投一次");
        assert!(store.read_inbox().contains("(无失败信号)"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn civil_date_is_correct() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_672), (2026, 8, 7));
    }
}
