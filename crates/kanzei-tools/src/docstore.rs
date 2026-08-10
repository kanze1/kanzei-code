//! 结构化文档引擎:需求/缺陷/来源/发现 的统一底座。
//! 真源是纯 markdown(用户可任意编辑器手改,解析宽容);
//! 结构(ID 分配、状态机、格式)由本引擎在写入侧强制——文档永远写不坏。
//!
//! 条目格式:
//! ```markdown
//! ## R-001 标题 [doing] (high)
//! - 验收: ...
//! - refs: S-001 S-002
//! ```

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub struct DocKind {
    /// 相对项目根,如 ".kanzei/project/requirements.md"。
    pub rel_path: &'static str,
    pub heading: &'static str,
    /// ID 前缀(R/D/S/F)。
    pub prefix: &'static str,
    /// 有序状态列表,首个为初始态。
    pub statuses: &'static [&'static str],
    /// 终态(close 的合法目标)。
    pub terminal: &'static [&'static str],
    pub severities: Option<&'static [&'static str]>,
    pub priorities: Option<&'static [&'static str]>,
    /// 标签受控词表(conventions §1.35 用户定调):None = 该文档不参与标签分类。
    /// 写入口校验:「标签:」值必须命中词表,词表外拒绝并提示合法值。
    pub tags: Option<&'static [&'static str]>,
    /// 非终态之间允许自由往返(目标 active⇄paused);false = 只进不退。
    pub bidirectional: bool,
    /// 允许经 `reopen` 退回初始态的非终态列表(空 = 不支持退回)。
    /// D-241:fixing 长期无人续推时没有合法退回通道,agent 只能手改 markdown 或
    /// 让僵尸条目永远占着「进行中」语义;reopen 把「推不动就退回」变成可执行动作。
    pub reopen_from: &'static [&'static str],
}

pub const REQUIREMENTS: DocKind = DocKind {
    rel_path: ".kanzei/project/requirements.md",
    heading: "Requirements",
    prefix: "R",
    statuses: &["todo", "doing", "done", "dropped"],
    terminal: &["done", "dropped"],
    severities: None,
    priorities: Some(&["P0", "P1", "P2", "P3"]),
    tags: Some(&["核心", "后端", "前端", "模型", "发布", "流程"]),
    bidirectional: false,
    reopen_from: &["doing"],
};

pub const DEFECTS: DocKind = DocKind {
    rel_path: ".kanzei/project/defects.md",
    heading: "Defects",
    prefix: "D",
    statuses: &["open", "fixing", "fixed", "wontfix"],
    terminal: &["fixed", "wontfix"],
    severities: Some(&["high", "medium", "low"]),
    priorities: Some(&["P0", "P1", "P2", "P3"]),
    tags: Some(&["核心", "后端", "前端", "模型", "发布", "流程"]),
    bidirectional: false,
    reopen_from: &["fixing"],
};

pub const SOURCES: DocKind = DocKind {
    rel_path: ".kanzei/research/sources.md",
    heading: "Sources",
    prefix: "S",
    statuses: &["active", "archived"],
    terminal: &["archived"],
    severities: None,
    priorities: None,
    tags: None,
    bidirectional: false,
    reopen_from: &[],
};

pub const FINDINGS: DocKind = DocKind {
    rel_path: ".kanzei/research/findings.md",
    heading: "Findings",
    prefix: "F",
    statuses: &["draft", "confirmed", "dropped"],
    terminal: &["confirmed", "dropped"],
    severities: None,
    priorities: None,
    tags: None,
    bidirectional: false,
    reopen_from: &[],
};

/// 跨会话记忆(R-098):记"已确认的事实与踩过的坑",与追踪文档职责分离——
/// 追踪文档记"要做什么/做到哪",记忆记"已经查清楚了什么"。
/// stale 是终态:被推翻或过期的结论 archive 出去,不再进入上下文。
pub const MEMORY: DocKind = DocKind {
    rel_path: ".kanzei/project/memory.md",
    heading: "Memory",
    prefix: "M",
    statuses: &["active", "stale"],
    terminal: &["stale"],
    severities: None,
    priorities: None,
    tags: None,
    // 结论可能被重新确认,允许 active⇄stale 往返。
    bidirectional: true,
    reopen_from: &[],
};

/// 设计决策(R-110):讨论与设计的沉淀——像需求/缺陷一样可追踪、可引用、可检索。
/// accepted 是常驻态(决策生效中);被新决策取代时 superseded 并归档,拒绝即 rejected。
pub const DECISIONS: DocKind = DocKind {
    rel_path: ".kanzei/project/decisions.md",
    heading: "Decisions",
    prefix: "A",
    statuses: &["draft", "accepted", "superseded", "rejected"],
    terminal: &["superseded", "rejected"],
    severities: None,
    priorities: None,
    tags: None,
    bidirectional: false,
    reopen_from: &[],
};

/// 长期目标(R-019):agent 每次运行注入活跃目标,无明确任务时自主推进。
pub const GOALS: DocKind = DocKind {
    rel_path: ".kanzei/project/goals.md",
    heading: "Goals",
    prefix: "G",
    statuses: &["active", "paused", "achieved", "dropped"],
    terminal: &["achieved", "dropped"],
    severities: None,
    priorities: None,
    tags: None,
    bidirectional: true,
    reopen_from: &[],
};

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub status: String,
    pub severity: Option<String>,
    /// 自由字段(bullet),refs 也存这里(key = "refs")。
    pub fields: Vec<(String, String)>,
}

/// 单条目批次数上限(2026-08-10 用户定调:批数由 agent 按实际工作量自定,上限 10)。
/// 这是**写入侧**门禁的判据,读路径不做钳制——理由见 declared_batch_progress。
pub const MAX_BATCHES: u32 = 10;

/// 条目的批次进度 (已完成, 总数)。
///
/// 总数**只认条目自己声明的 `批次: k/N`**;没声明 = 不分批(1 格),既不画进度条
/// 也不受关闭门禁约束——批数由 agent 定,引擎不替他猜(2026-08-10 定调)。原先按
/// 复杂度给的固定默认值(中 3/大 8)已删:它经关闭门禁直接支配了没声明批次的中/大
/// 条目,让它们必然撞门(D-242 影响①)。
///
/// 判据只有这一份:UI 的格子与关闭门禁都从这里取,不要在前端再抄一张映射表。
pub fn batch_progress(entry: &Entry) -> (u32, u32) {
    match declared_batch_progress(entry) {
        // 显式声明优先:总数为 0 视为没声明,避免除零与"0/0 格"这种空表达。
        Some((done, total)) if total > 0 => (done.min(total), total),
        _ => (0, 1),
    }
}

/// 从条目字段读取手写的批次副本。`None` 表示未声明或格式无效。
///
/// 读路径故意**不按 MAX_BATCHES 钳制**,两条理由:①归档里真实存在 11/11、16/16 的
/// 条目,钳到 10 会把历史显示成假数;②关闭门禁走的也是这里——把声明的 12 钳成 10,
/// git 推导出 10 个批次标记时门禁就会放行两个根本没做的批次,硬门禁被静默降级成软
/// 提示。上限因此加在写入侧(check_declared_batches)。
pub fn declared_batch_progress(entry: &Entry) -> Option<(u32, u32)> {
    entry
        .fields
        .iter()
        .find(|(k, _)| k == "批次" || k.eq_ignore_ascii_case("batches"))
        .and_then(|(_, v)| parse_batches(v))
        .filter(|(_, total)| *total > 0)
}

/// Git 提交历史可用时，以它的已完成数覆盖手写副本；总批数仍只由条目声明决定。
pub fn batch_progress_with_derived_done(entry: &Entry, derived_done: Option<u32>) -> (u32, u32) {
    let (declared_done, total) = batch_progress(entry);
    let done = derived_done.unwrap_or(declared_done).min(total);
    (done, total)
}

/// 解析 `3/11`;宽容对待空格与全角斜杠(手写文档常见)。
fn parse_batches(raw: &str) -> Option<(u32, u32)> {
    let normalized = raw.replace('／', "/");
    let (done, total) = normalized.trim().split_once('/')?;
    Some((done.trim().parse().ok()?, total.trim().parse().ok()?))
}

/// 写入侧校验:条目**本次声明**批次时的唯一判据。
///
/// `existing_total` = 该条目**当前已有**的批次总数;新建(add)没有既有值,传 `None`。
/// 上限只约束「新声明或被抬高的总数」:本次总数**不高于既有值**就放行(哪怕既有值是
/// 11、16),高于既有值且超过上限才拒。理由:归档/存量里真实存在 11/11、16/16 的历史
/// 条目,`3/11` → `4/11` 是它们的**正常逐批推进**,不是新声明——拦下来只会逼 agent
/// 为了动一条历史条目去篡改它的总数;而抬高总数(`3/11` → `3/16`)是货真价实的新声明,
/// 必须撞门。新建没有既有值,按 `<= MAX_BATCHES` 严格约束。
///
/// 选择「拒绝并报错」而不是「钳到 10」:钳制既会改写归档里的真值,又会把关闭门禁
/// 静默放宽(声明 12 被钳成 10 时,做完 10 批就放行)。拒绝发生在写入当下,agent
/// 拿到的是可执行的出路(拆后续条目),而不是事后撞门。
///
/// 调用方必须只把**本次传入的字段值**喂进 `raw`,绝不能拿合并后的整条目来校验——
/// 否则归档里 11/11、16/16 的历史条目一旦被触碰就再也关不掉。既有值只作为上限的
/// 基准出现在 `existing_total` 里,不参与格式与 `done > total` 的判定。
pub fn check_declared_batches(
    raw: &str,
    existing_total: Option<u32>,
) -> Result<(u32, u32), String> {
    let Some((done, total)) = parse_batches(raw) else {
        return Err(format!("批次字段要写成 `k/N`(如 `0/3`),实际收到 `{raw}`。"));
    };
    if total == 0 {
        return Err("批次总数不能为 0;不分批就别写这个字段。".into());
    }
    if done > total {
        return Err(format!("已完成 {done} 超过总数 {total},先核对再写。"));
    }
    // 基准 = 既有总数(新建按 0 算)。只有把总数抬到基准之上、又越过上限,才是新声明。
    let baseline = existing_total.unwrap_or(0);
    if total > MAX_BATCHES && total > baseline {
        let 既有 = if baseline > MAX_BATCHES {
            format!(
                "该条目既有总数是 {baseline}(历史真值),照它原样往前推(只改已完成数)\
                 或改小总数都放行,抬到 {total} 不行。"
            )
        } else {
            String::new()
        };
        return Err(format!(
            "批数上限 {MAX_BATCHES}(2026-08-10 用户定调),实际声明 {total}。\
             批数由你按工作量定,但超过 {MAX_BATCHES} 批说明这个条目本身太大:\
             把能收口的部分做完关闭,剩下的开成后续条目,不要靠加批次把一条撑到底。{既有}"
        ));
    }
    Ok((done, total))
}

impl Entry {
    pub fn refs(&self) -> Vec<String> {
        self.fields
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("refs"))
            .flat_map(|(_, v)| v.split([' ', ',']))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }
}

#[derive(Debug, Clone)]
enum TemplateLine {
    Raw(String),
    Field(String),
}

#[derive(Debug, Clone)]
struct EntryTemplate {
    id: String,
    lines: Vec<TemplateLine>,
}

#[derive(Debug, Clone)]
struct DocumentTemplate {
    preamble: Vec<String>,
    entries: Vec<EntryTemplate>,
}

#[derive(Debug, Clone)]
struct ParsedDocument {
    entries: Vec<Entry>,
    template: DocumentTemplate,
}

pub struct DocStore {
    pub kind: &'static DocKind,
    pub path: PathBuf,
    preserved: Arc<Mutex<Option<DocumentTemplate>>>,
    preserved_archive: Arc<Mutex<Option<DocumentTemplate>>>,
}
impl DocStore {
    pub fn open(project_root: &Path, kind: &'static DocKind) -> Self {
        DocStore {
            kind,
            path: project_root.join(kind.rel_path),
            preserved: Arc::new(Mutex::new(None)),
            preserved_archive: Arc::new(Mutex::new(None)),
        }
    }

    pub fn load(&self) -> std::io::Result<Vec<Entry>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                let parsed = parse_document(self.kind, &text);
                *self.preserved.lock().unwrap() = Some(parsed.template.clone());
                Ok(parsed.entries)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, entries: &[Entry]) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let template = self.preserved.lock().unwrap().clone();
        let text = template
            .as_ref()
            .map(|template| render_with_template(self.kind, entries, template))
            .unwrap_or_else(|| render(self.kind, entries));
        std::fs::write(&self.path, text)
    }

    /// ID 分配扫活跃 + 归档 + 废弃账本:归档移走或主动废弃过的编号都绝不复用。
    pub fn next_id(&self, entries: &[Entry]) -> String {
        let archived = self.load_archive().unwrap_or_default();
        let max = entries
            .iter()
            .chain(archived.iter())
            .filter_map(|e| {
                e.id.strip_prefix(self.kind.prefix)?
                    .strip_prefix('-')?
                    .parse::<u32>()
                    .ok()
            })
            .chain(self.voided_ids().keys().copied())
            .max()
            .unwrap_or(0);
        format!("{}-{:03}", self.kind.prefix, max + 1)
    }

    /// 归档文件:同目录 `<name>-archive.md`(如 requirements-archive.md)。
    pub fn archive_file(&self) -> PathBuf {
        match self.path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => self.path.with_file_name(format!("{stem}-archive.md")),
            None => self.path.with_extension("archive.md"),
        }
    }

    pub fn load_archive(&self) -> std::io::Result<Vec<Entry>> {
        match std::fs::read_to_string(self.archive_file()) {
            Ok(text) => {
                let parsed = parse_document(self.kind, &text);
                *self.preserved_archive.lock().unwrap() = Some(parsed.template.clone());
                Ok(parsed.entries)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// 修复“历史归档 ID 被后来的活动条目复用”：保留活动条目的当前 ID，
    /// 把语义不同的历史归档条目迁到下一个未使用 ID。若两份内容相同，说明更像
    /// 归档半途而废，不能靠改号掩盖，应人工判断哪一份才该保留。
    pub fn repair_reused_archived_id(&self, id: &str) -> std::io::Result<String> {
        let active = self.load()?;
        let mut archived = self.load_archive()?;
        let Some(active_entry) = active.iter().find(|entry| entry.id == id) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{id} 不在活动文档中"),
            ));
        };
        let Some(archived_pos) = archived.iter().position(|entry| entry.id == id) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{id} 不在归档文档中"),
            ));
        };
        if active_entry == &archived[archived_pos] {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{id} 的活动与归档内容相同，疑似未完成归档，拒绝自动改号"),
            ));
        }
        let issues = self.integrity_issues(&active);
        if issues.len() != 1 || !issues[0].contains(id) || issues[0].contains(',') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("除 {id} 复用外仍有其他完整性问题: {}", issues.join("; ")),
            ));
        }

        let new_id = self.next_id(&active);
        let archived_entry = &mut archived[archived_pos];
        archived_entry.id = new_id.clone();
        archived_entry.title = archived_entry.title.replace(id, &new_id);
        for (_, value) in &mut archived_entry.fields {
            *value = value.replace(id, &new_id);
        }

        let mut template = self
            .preserved_archive
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "归档模板未加载")
            })?;
        let entry_template = template
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("归档模板中找不到 {id}"),
                )
            })?;
        entry_template.id = new_id.clone();
        for line in &mut entry_template.lines {
            if let TemplateLine::Raw(text) = line {
                *text = text.replace(id, &new_id);
            }
        }

        let text = render_with_template(self.kind, &archived, &template).replacen(
            &format!("# {}\n", self.kind.heading),
            &format!("# {} Archive\n", self.kind.heading),
            1,
        );
        std::fs::write(self.archive_file(), text)?;
        *self.preserved_archive.lock().unwrap() = Some(template);
        Ok(new_id)
    }

    /// 终态条目移入归档文件(追加,幂等):活跃文件只留进行中的,前端与
    /// 上下文注入都不再被完成项干扰;历史仍可随时翻(get 会回落到归档)。
    /// 返回被移动的条目 ID——调用方必须能告知"哪些条目去了哪个文件"(D-112)。
    pub fn archive_terminal(&self) -> std::io::Result<Vec<String>> {
        let entries = self.load()?;
        let (terminal, live): (Vec<Entry>, Vec<Entry>) = entries
            .into_iter()
            .partition(|e| self.kind.terminal.contains(&e.status.as_str()));
        if terminal.is_empty() {
            return Ok(Vec::new());
        }
        let mut archived = self.load_archive()?;
        let moved: Vec<String> = terminal.iter().map(|e| e.id.clone()).collect();
        let active_template = self.preserved.lock().unwrap().clone();
        let mut archive_template =
            self.preserved_archive
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(DocumentTemplate {
                    preamble: Vec::new(),
                    entries: Vec::new(),
                });
        if let Some(active_template) = active_template {
            for entry in &terminal {
                if archive_template
                    .entries
                    .iter()
                    .all(|template| template.id != entry.id)
                {
                    if let Some(template) = active_template
                        .entries
                        .iter()
                        .find(|template| template.id == entry.id)
                    {
                        archive_template.entries.push(template.clone());
                    }
                }
            }
        }
        archived.extend(terminal);
        let archived_text = render_with_template(self.kind, &archived, &archive_template);
        let text = archived_text.replacen(
            &format!("# {}\n", self.kind.heading),
            &format!("# {} Archive\n", self.kind.heading),
            1,
        );
        if let Some(parent) = self.archive_file().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(self.archive_file(), text)?;
        self.save(&live)?;
        Ok(moved)
    }

    /// 编号账本:`<stem>-ids.md`,记录被**主动废弃**的编号及理由。
    ///
    /// 缺号本身只说明"这个号现在没有条目",不等于数据丢失——分配后又撤销、
    /// 手工整理时合并掉重复条目,都会留下合法空洞。把两者混为一谈的后果实测过
    /// (D-173 复盘):完整性门禁把合法空洞判成丢失,又不提供安全的交代通道,
    /// 模型只好伪造一个 `[wontfix]` 墓碑去骗过门禁,反而污染了真实缺陷统计。
    pub fn ledger_file(&self) -> PathBuf {
        match self.path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => self.path.with_file_name(format!("{stem}-ids.md")),
            None => self.path.with_extension("ids.md"),
        }
    }

    /// 已废弃编号 → 理由。解析宽容:`- D-171: 理由` 形式,认不出的行忽略。
    pub fn voided_ids(&self) -> std::collections::BTreeMap<u32, String> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(text) = std::fs::read_to_string(self.ledger_file()) else {
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

    /// 主动废弃一个编号。理由必填,且该编号当前必须真的不存在于活动/归档——
    /// 拿它去"清掉"一个还活着的条目是删数据,不是记账。
    pub fn void_id(&self, id: &str, reason: &str) -> std::io::Result<()> {
        let invalid =
            |message: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, message);
        let reason = reason.trim();
        if reason.len() < 4 {
            return Err(invalid(
                "废弃编号必须写明理由(为什么这个号不该有条目、依据是什么)".into(),
            ));
        }
        let Some(number) = self.id_number(id) else {
            return Err(invalid(format!(
                "`{id}` 不是 {} 前缀的合法编号",
                self.kind.prefix
            )));
        };
        if self.load()?.iter().any(|entry| entry.id == id)
            || self.load_archive()?.iter().any(|entry| entry.id == id)
        {
            return Err(invalid(format!(
                "{id} 仍存在于活动或归档文档中,不能作为空洞注销;要终结它请用 close/archive"
            )));
        }
        if self.voided_ids().contains_key(&number) {
            return Ok(());
        }
        let path = self.ledger_file();
        let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            format!(
                "# {} ID Ledger\n\n引擎维护:记录被主动废弃的编号及理由。\n\
                 缺号只有登记在此才算已交代;其余缺号 = 账实不符,必须查清。\n",
                self.kind.heading
            )
        });
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("- {id}: {reason}\n"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)
    }

    /// 在指定编号处补回一条丢失的条目(从 git 历史捞回来后落盘)。
    /// 只允许补真正的空洞,并且按编号插回原位——ID 顺序即分配顺序。
    pub fn restore_entry(&self, entry: Entry) -> std::io::Result<()> {
        let invalid =
            |message: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, message);
        let Some(number) = self.id_number(&entry.id) else {
            return Err(invalid(format!(
                "`{}` 不是 {} 前缀的合法编号",
                entry.id, self.kind.prefix
            )));
        };
        let mut entries = self.load()?;
        if entries.iter().any(|e| e.id == entry.id)
            || self.load_archive()?.iter().any(|e| e.id == entry.id)
        {
            return Err(invalid(format!("{} 已存在,不是空洞", entry.id)));
        }
        if self.voided_ids().contains_key(&number) {
            return Err(invalid(format!(
                "{} 已登记为主动废弃,先从 {} 里删掉那一行再补条目",
                entry.id,
                self.ledger_file().display()
            )));
        }
        let position = entries
            .iter()
            .position(|e| self.id_number(&e.id).is_some_and(|n| n > number))
            .unwrap_or(entries.len());
        entries.insert(position, entry);
        self.save(&entries)
    }

    fn id_number(&self, id: &str) -> Option<u32> {
        id.strip_prefix(self.kind.prefix)?
            .strip_prefix('-')?
            .parse::<u32>()
            .ok()
    }

    /// 数据完整性检测(D-112 / D-173):同一 ID 同时出现在活动与归档 = 归档半途而废;
    /// 活动∪归档∪废弃账本之外的缺号 = **账实不符**,必须查清后二选一交代掉。
    ///
    /// 注意措辞:缺号不等于"已确认的数据丢失"。ID 由引擎顺序分配,缺号说明这个号
    /// 曾被分配却没有条目,可能是丢了,也可能是合法撤销——工具无法从文件本身分辨,
    /// 所以只报"未交代",并同时给出两条**结构化**的合法出路(补回 / 注销),
    /// 而不是逼模型伪造一个墓碑条目来消音。
    pub fn integrity_issues(&self, active: &[Entry]) -> Vec<String> {
        let archived = self.load_archive().unwrap_or_default();
        let parse_num = |id: &str| {
            id.strip_prefix(self.kind.prefix)
                .and_then(|rest| rest.strip_prefix('-'))
                .and_then(|num| num.parse::<u32>().ok())
        };
        let active_ids: std::collections::BTreeSet<u32> =
            active.iter().filter_map(|e| parse_num(&e.id)).collect();
        let archive_ids: std::collections::BTreeSet<u32> =
            archived.iter().filter_map(|e| parse_num(&e.id)).collect();
        let voided = self.voided_ids();
        let mut issues = Vec::new();
        let both: Vec<u32> = active_ids.intersection(&archive_ids).copied().collect();
        if !both.is_empty() {
            issues.push(format!(
                "present in BOTH active and archive (incomplete archive?): {}",
                format_ids(self.kind.prefix, &both)
            ));
        }
        // 账本里登记为废弃、却又真的存在条目:账实不符的另一半,同样要报。
        let resurrected: Vec<u32> = voided
            .keys()
            .filter(|n| active_ids.contains(n) || archive_ids.contains(n))
            .copied()
            .collect();
        if !resurrected.is_empty() {
            issues.push(format!(
                "recorded as voided in {} but an entry exists: {} — delete the ledger line or renumber the entry",
                self.ledger_file().display(),
                format_ids(self.kind.prefix, &resurrected)
            ));
        }
        let Some(max) = active_ids
            .iter()
            .chain(archive_ids.iter())
            .chain(voided.keys())
            .max()
            .copied()
        else {
            return issues;
        };
        let missing: Vec<u32> = (1..=max)
            .filter(|n| {
                !active_ids.contains(n) && !archive_ids.contains(n) && !voided.contains_key(n)
            })
            .collect();
        if !missing.is_empty() {
            issues.push(format!(
                "UNACCOUNTED ids — absent from the active file, the archive AND the void ledger: {}. \
                 An engine-allocated id with no entry is either lost data or a withdrawn allocation, \
                 and this file cannot tell which. Settle each one: recover it \
                 (`git log -S \"## <id>\" -- {}` then `repair_missing_id`), or record why it was \
                 withdrawn (`void_id` with a reason). Do NOT invent a placeholder entry to silence \
                 this — that corrupts the real statistics.",
                format_ids(self.kind.prefix, &missing),
                self.kind.rel_path,
            ));
        }
        issues
    }

    /// 状态流转校验:前进(列表序)或进终态;后退/未知状态拒绝。
    pub fn transition_allowed(&self, from: &str, to: &str) -> Result<(), String> {
        let idx = |s: &str| self.kind.statuses.iter().position(|x| *x == s);
        let Some(to_idx) = idx(to) else {
            return Err(format!(
                "unknown status `{to}`; valid: {}",
                self.kind.statuses.join(" → ")
            ));
        };
        if self.kind.terminal.contains(&to) {
            return Ok(());
        }
        // 双向类型(目标):非终态之间自由往返(active⇄paused)。
        if self.kind.bidirectional {
            return Ok(());
        }
        match idx(from) {
            Some(from_idx) if to_idx >= from_idx => Ok(()),
            Some(_) => Err(format!(
                "cannot move backward `{from}` → `{to}`; forward only ({}). Hand-edit the markdown if you really need to reopen.",
                self.kind.statuses.join(" → ")
            )),
            // 用户手改出的未知状态:宽容,允许任意流转。
            None => Ok(()),
        }
    }
}

fn format_ids(prefix: &str, numbers: &[u32]) -> String {
    const SHOWN: usize = 10;
    let mut out: Vec<String> = numbers
        .iter()
        .take(SHOWN)
        .map(|n| format!("{prefix}-{n:03}"))
        .collect();
    if numbers.len() > SHOWN {
        out.push(format!("+{} more", numbers.len() - SHOWN));
    }
    out.join(", ")
}

/// 宽容解析:`## ` 开头即条目;ID 缺失/状态缺失都不报错(手改友好)。
pub fn parse(kind: &DocKind, text: &str) -> Vec<Entry> {
    parse_document(kind, text).entries
}

fn parse_document(kind: &DocKind, text: &str) -> ParsedDocument {
    let mut entries: Vec<Entry> = Vec::new();
    let mut templates = Vec::new();
    let mut preamble = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            let entry = parse_heading(kind, rest);
            templates.push(EntryTemplate {
                id: entry.id.clone(),
                lines: Vec::new(),
            });
            entries.push(entry);
        } else if let Some(entry) = entries.last_mut() {
            let template = templates.last_mut().expect("entry template exists");
            if let Some(bullet) = trimmed.trim_start().strip_prefix("- ") {
                if let Some((key, value)) = bullet.split_once(':') {
                    let key = key.trim().to_string();
                    entry.fields.push((key.clone(), value.trim().to_string()));
                    template.lines.push(TemplateLine::Field(key));
                } else {
                    template.lines.push(TemplateLine::Raw(line.to_string()));
                }
            } else {
                template.lines.push(TemplateLine::Raw(line.to_string()));
            }
        } else {
            preamble.push(line.to_string());
        }
    }
    ParsedDocument {
        entries,
        template: DocumentTemplate {
            preamble,
            entries: templates,
        },
    }
}
fn parse_heading(kind: &DocKind, rest: &str) -> Entry {
    let mut title = rest.trim().to_string();
    let mut status = String::new();
    let mut severity = None;

    // 从尾部剥离 (severity) 和 [status],顺序宽容。
    // severity 只在命中该文档类型的合法枚举时才剥离——标题自带的括号(如
    // "桌面端(类 VSCode 布局)")必须原样保留(狗粮暴露的 bug,见 D-002)。
    for _ in 0..2 {
        let t = title.trim_end();
        if t.ends_with(')') {
            if let (Some(pos), Some(valid)) = (t.rfind('('), kind.severities) {
                let candidate = t[pos + 1..t.len() - 1].trim();
                if valid.contains(&candidate) {
                    severity = Some(candidate.to_string());
                    title = t[..pos].trim_end().to_string();
                    continue;
                }
            }
        }
        // status 同样必须命中合法枚举才剥离,否则标题里的方括号(如 "支持 vec[index] 语法"、
        // "处理 [DONE] 帧")会被截断成非法状态并永久固化(D-070,与 D-002 同族)。
        if t.ends_with(']') {
            if let Some(pos) = t.rfind('[') {
                let candidate = t[pos + 1..t.len() - 1].trim();
                if kind.statuses.contains(&candidate) {
                    status = candidate.to_string();
                    title = t[..pos].trim_end().to_string();
                    continue;
                }
            }
        }
        break;
    }

    // 首 token 形如 X-123 视为 ID。
    let (id, title) = match title.split_once(' ') {
        Some((first, rest)) if looks_like_id(first) => (first.to_string(), rest.trim().to_string()),
        _ if looks_like_id(&title) => (title.clone(), String::new()),
        _ => (String::new(), title.clone()),
    };
    Entry {
        id,
        title,
        status,
        severity,
        fields: Vec::new(),
    }
}

fn looks_like_id(s: &str) -> bool {
    match s.split_once('-') {
        Some((prefix, num)) => {
            !prefix.is_empty()
                && prefix.chars().all(|c| c.is_ascii_uppercase())
                && !num.is_empty()
                && num.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

pub fn render(kind: &DocKind, entries: &[Entry]) -> String {
    let mut out = format!("# {}\n", kind.heading);
    for e in entries {
        out.push('\n');
        out.push_str(&format!("## {} {}", e.id, e.title));
        if !e.status.is_empty() {
            out.push_str(&format!(" [{}]", e.status));
        }
        if let Some(sev) = &e.severity {
            out.push_str(&format!(" ({sev})"));
        }
        out.push('\n');
        for (key, value) in &e.fields {
            out.push_str(&format!("- {key}: {value}\n"));
        }
    }
    out
}

fn render_with_template(kind: &DocKind, entries: &[Entry], template: &DocumentTemplate) -> String {
    let mut out = String::new();
    if template.preamble.is_empty() {
        out.push_str(&format!("# {}\n", kind.heading));
    } else {
        for line in &template.preamble {
            out.push_str(line);
            out.push('\n');
        }
    }
    for entry in entries {
        let entry_template = template
            .entries
            .iter()
            .find(|candidate| candidate.id == entry.id);
        if let Some(entry_template) = entry_template {
            render_entry_with_template(&mut out, entry, entry_template);
        } else {
            render_entry(&mut out, entry);
        }
    }
    out
}

fn render_entry_with_template(out: &mut String, entry: &Entry, template: &EntryTemplate) {
    render_heading(out, entry);
    let mut used = vec![false; entry.fields.len()];
    for line in &template.lines {
        match line {
            TemplateLine::Raw(raw) => {
                // 连续空行折叠为一个(D-130):条目内部堆积的空行是引擎自己吐出来的,
                // 不是用户内容——真正的自由文本一行不丢,只压掉重复的空白。
                if raw.trim().is_empty() && (out.ends_with("\n\n") || out.is_empty()) {
                    continue;
                }
                out.push_str(raw);
                out.push('\n');
            }
            TemplateLine::Field(key) => {
                if let Some((index, (current_key, value))) =
                    entry
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(index, (current_key, _))| {
                            !used[*index] && current_key.eq_ignore_ascii_case(key)
                        })
                {
                    used[index] = true;
                    out.push_str(&format!("- {current_key}: {value}\n"));
                }
            }
        }
    }
    for (index, (key, value)) in entry.fields.iter().enumerate() {
        if !used[index] {
            out.push_str(&format!("- {key}: {value}\n"));
        }
    }
}

fn render_entry(out: &mut String, entry: &Entry) {
    render_heading(out, entry);
    for (key, value) in &entry.fields {
        out.push_str(&format!("- {key}: {value}\n"));
    }
}

/// 条目之间规范为恰好一个空行。
///
/// 不这么做会无限膨胀(D-130):解析时条目间的空行被存成上一条模板的
/// `TemplateLine::Raw("")`,渲染时原样写回,而这里若再无条件 `push('\n')`,
/// 每保存一次每条就多一行。实测 defects.md 已达 94% 空行、开头连着 225 个空行,
/// 把真实内容稀释到几乎不可读,还会把数据丢失这类关键 diff 埋掉。
/// 条目内的用户自由文本不受影响(D-060 的保留承诺仍成立),这里只规范条目间距——
/// 格式本就由引擎在写入侧强制(见模块头)。
fn ensure_blank_separator(out: &mut String) {
    if out.is_empty() {
        return;
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out.push_str("\n\n");
}

fn render_heading(out: &mut String, entry: &Entry) {
    ensure_blank_separator(out);
    out.push_str(&format!("## {} {}", entry.id, entry.title));
    if !entry.status.is_empty() {
        out.push_str(&format!(" [{}]", entry.status));
    }
    if let Some(sev) = &entry.severity {
        out.push_str(&format!(" ({sev})"));
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 批次夹具(fields: Vec<(&str, &str)>) -> Entry {
        Entry {
            id: "R-999".into(),
            title: "t".into(),
            status: "doing".into(),
            severity: None,
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn 批次进度_只认显式声明_未声明即不分批() {
        let make = 批次夹具;
        // 没写批次一律 (0,1),复杂度不再生成格数。原先中=3/大=8 的固定默认值经
        // tracker 关闭门禁(total>1 && done<total)直接把没声明批次的中/大条目
        // 拦死——批数由 agent 定之后,引擎不替他猜(D-242 影响①的回归锁)。
        assert_eq!(batch_progress(&make(vec![("复杂度", "大")])), (0, 1));
        assert_eq!(batch_progress(&make(vec![("复杂度", "中")])), (0, 1));
        assert_eq!(batch_progress(&make(vec![("复杂度", "小")])), (0, 1));
        assert_eq!(
            batch_progress(&make(vec![])),
            (0, 1),
            "没评估复杂度按一轮做完算"
        );

        // 写了就以它为准:归档里真实存在 11 批的拆解条目,读路径不得钳到上限 10。
        assert_eq!(
            batch_progress(&make(vec![("复杂度", "大"), ("批次", "3/11")])),
            (3, 11)
        );
        // 手写文档的宽容:空格与全角斜杠。
        assert_eq!(batch_progress(&make(vec![("批次", " 2 ／ 5 ")])), (2, 5));
        // 已完成不会超过总数;0/0 视为没声明,回落"不分批"而不是画 0 个格。
        assert_eq!(batch_progress(&make(vec![("批次", "9/5")])), (5, 5));
        assert_eq!(
            batch_progress(&make(vec![("复杂度", "中"), ("批次", "0/0")])),
            (0, 1)
        );
        assert_eq!(batch_progress(&make(vec![("批次", "乱写")])), (0, 1));
    }

    #[test]
    fn 声明批数上限十批_超出拒绝并给出出路() {
        assert_eq!(
            check_declared_batches("0/10", None),
            Ok((0, 10)),
            "10 是合法上界"
        );
        assert_eq!(
            check_declared_batches(" 3 ／ 7 ", None),
            Ok((3, 7)),
            "宽容解析一致"
        );

        let over = check_declared_batches("0/11", None).unwrap_err();
        assert!(over.contains("10"), "错误里要点名上限: {over}");
        assert!(
            over.contains("后续条目"),
            "只说不行不算数,必须给出可执行的出路(D-173 的教训): {over}"
        );

        assert!(
            check_declared_batches("0/0", None).is_err(),
            "总数 0 没有意义"
        );
        assert!(
            check_declared_batches("乱写", None).is_err(),
            "格式非法要挡住"
        );
        assert!(
            check_declared_batches("5/3", None).is_err(),
            "已完成不能超过总数"
        );

        // 读路径不钳制的回归锁:上限只在写入侧生效,历史条目照原样读出来。
        // 谁"顺手"把 10 也钳到读路径上,归档的 11/11 会显示成 10/10,
        // 且声明 12 批的条目做完 10 批就会被关闭门禁放行。
        assert_eq!(
            declared_batch_progress(&批次夹具(vec![("批次", "3/11")])),
            Some((3, 11))
        );
    }

    #[test]
    fn 上限只拦抬高的总数_历史超限条目照常逐批推进() {
        // 存量/归档里真实存在 11 批的条目。它们的正常推进是「改已完成数、不动总数」——
        // 门禁若对 total>10 一律拒,agent 想动这类条目就只能先篡改总数,门禁反而在
        // 逼人伪造历史。基准比较把两件事分开:抬高才是新声明。
        assert_eq!(
            check_declared_batches("4/11", Some(11)),
            Ok((4, 11)),
            "历史 3/11 推进到 4/11 是逐批推进,必须放行"
        );
        assert_eq!(
            check_declared_batches("3/11", Some(11)),
            Ok((3, 11)),
            "总数原样重写(等于既有值)也算不高于,放行"
        );
        assert_eq!(
            check_declared_batches("3/3", Some(11)),
            Ok((3, 3)),
            "把总数改小到实际批数是我们鼓励的收口路径"
        );

        let 抬高 = check_declared_batches("3/16", Some(11)).unwrap_err();
        assert!(抬高.contains("16"), "错误要点名本次声明: {抬高}");
        assert!(抬高.contains("11"), "错误要点名既有基准: {抬高}");
        assert!(抬高.contains("后续条目"), "仍要给出可执行的出路: {抬高}");

        assert!(
            check_declared_batches("0/12", Some(5)).is_err(),
            "既有值本身没超上限时,抬到 12 照旧撞门"
        );
        assert!(
            check_declared_batches("0/11", None).is_err(),
            "新建没有既有值,按 <=10 严格约束"
        );
        // 基准只放宽上限,不放宽其它判据。
        assert!(
            check_declared_batches("12/11", Some(11)).is_err(),
            "已完成超过总数,给了基准也不能放行"
        );
    }

    #[test]
    fn roundtrip() {
        let entries = vec![Entry {
            id: "R-001".into(),
            title: "支持本地模型".into(),
            status: "doing".into(),
            severity: None,
            fields: vec![
                ("验收".into(), "ollama 走通循环".into()),
                ("refs".into(), "D-003".into()),
            ],
        }];
        let text = render(&REQUIREMENTS, &entries);
        assert_eq!(parse(&REQUIREMENTS, &text), entries);
    }

    #[test]
    fn title_with_parens_survives_roundtrip() {
        // D-002:中文括号后缀曾被误剥为 severity。
        let entries = vec![Entry {
            id: "R-002".into(),
            title: "Tauri 桌面端(类 VSCode 布局)".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![],
        }];
        let text = render(&REQUIREMENTS, &entries);
        assert_eq!(parse(&REQUIREMENTS, &text), entries);
        // defects 文档里合法 severity 照常剥离
        let text = "## D-001 标题 [open] (high)\n";
        let parsed = parse(&DEFECTS, text);
        assert_eq!(parsed[0].severity.as_deref(), Some("high"));
        assert_eq!(parsed[0].title, "标题");
    }

    /// D-070:标题自带的方括号后缀不得被当成 status 剥离(与 D-002 同族)。
    #[test]
    fn title_with_brackets_survives_roundtrip() {
        let entries = vec![
            Entry {
                id: "R-100".into(),
                title: "支持 vec[index] 语法".into(),
                status: "todo".into(),
                severity: None,
                fields: vec![],
            },
            Entry {
                id: "R-101".into(),
                title: "处理 [DONE] 帧".into(),
                status: "doing".into(),
                severity: None,
                fields: vec![],
            },
        ];
        let text = render(&REQUIREMENTS, &entries);
        assert_eq!(parse(&REQUIREMENTS, &text), entries);

        // 方括号结尾但不是合法状态:必须原样留在标题里,不能被截断成非法状态
        let parsed = parse(&REQUIREMENTS, "## R-102 支持 vec[index]\n");
        assert_eq!(parsed[0].title, "支持 vec[index]");
        assert_eq!(parsed[0].status, "");

        // 合法状态照常剥离
        let parsed = parse(&REQUIREMENTS, "## R-103 普通标题 [done]\n");
        assert_eq!(parsed[0].title, "普通标题");
        assert_eq!(parsed[0].status, "done");
    }

    #[test]
    fn tolerant_parse_of_hand_edits() {
        let text = "# Whatever\n\n## R-002 没写状态\n- 备注: 手改的\n\n## 连ID都没有 [todo]\n";
        let entries = parse(&REQUIREMENTS, text);
        assert_eq!(entries[0].id, "R-002");
        assert_eq!(entries[0].status, "");
        assert_eq!(entries[1].id, "");
        assert_eq!(entries[1].title, "连ID都没有");
        assert_eq!(entries[1].status, "todo");
    }

    #[test]
    fn id_allocation_and_transitions() {
        let store = DocStore {
            kind: &DEFECTS,
            path: "x".into(),
            preserved: Arc::new(Mutex::new(None)),
            preserved_archive: Arc::new(Mutex::new(None)),
        };
        let entries = vec![
            Entry {
                id: "D-002".into(),
                title: "t".into(),
                status: "open".into(),
                severity: None,
                fields: vec![],
            },
            Entry {
                id: "D-009".into(),
                title: "t".into(),
                status: "open".into(),
                severity: None,
                fields: vec![],
            },
        ];
        assert_eq!(store.next_id(&entries), "D-010");
        assert!(store.transition_allowed("open", "fixing").is_ok());
        assert!(store.transition_allowed("open", "wontfix").is_ok());
        assert!(store.transition_allowed("fixing", "open").is_err());
        assert!(store.transition_allowed("open", "banana").is_err());
        assert!(store.transition_allowed("手改状态", "fixing").is_ok());
    }

    #[test]
    fn archive_moves_terminal_and_preserves_ids() {
        let dir = std::env::temp_dir().join(format!("kz-archive-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mk = |id: &str, status: &str| Entry {
            id: id.into(),
            title: "t".into(),
            status: status.into(),
            severity: None,
            fields: vec![],
        };
        store
            .save(&[
                mk("R-001", "done"),
                mk("R-002", "doing"),
                mk("R-003", "dropped"),
            ])
            .unwrap();

        assert_eq!(store.archive_terminal().unwrap(), vec!["R-001", "R-003"]);
        let live = store.load().unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "R-002");
        let archived = store.load_archive().unwrap();
        assert_eq!(archived.len(), 2);
        // 归档后 ID 分配仍延续全局最大值,不复用 R-003。
        assert_eq!(store.next_id(&live), "R-004");
        // 幂等:再跑一次不动任何东西。
        assert!(store.archive_terminal().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn 反复保存不会让文档膨胀出空行() {
        // D-130:每次保存给每条多插一个空行,实测把 defects.md 稀释到 94% 空行。
        // 不变量:load→save 是幂等的,连续保存后文件字节数必须稳定。
        let dir = std::env::temp_dir().join(format!(
            "kz-blank-bloat-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mk = |id: &str| Entry {
            id: id.into(),
            title: "标题".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![("验收".into(), "略".into())],
        };
        store
            .save(&[mk("R-001"), mk("R-002"), mk("R-003")])
            .unwrap();

        let mut sizes = Vec::new();
        for _ in 0..6 {
            let entries = store.load().unwrap();
            store.save(&entries).unwrap();
            sizes.push(std::fs::read_to_string(&store.path).unwrap().len());
        }
        assert!(
            sizes.windows(2).all(|w| w[0] == w[1]),
            "反复保存必须字节数稳定,实测: {sizes:?}"
        );

        // 已被历史膨胀污染的文档,一次保存即被规范回来。
        std::fs::write(
            &store.path,
            "# Requirements\n\n\n\n\n\n\n\n## R-001 标题 [todo]\n- 验收: 略\n\n\n\n\n\n\n## R-002 标题 [todo]\n- 验收: 略\n",
        )
        .unwrap();
        let entries = store.load().unwrap();
        store.save(&entries).unwrap();
        let text = std::fs::read_to_string(&store.path).unwrap();
        assert!(!text.contains("\n\n\n"), "不该留下连续空行:\n{text}");
        assert_eq!(store.load().unwrap().len(), 2, "规范化不得丢条目");

        // 条目内部的空行堆积同样要压掉,但用户自由文本一行不能少(D-060 承诺)。
        std::fs::write(
            &store.path,
            "# Requirements\n\n## R-001 标题 [todo]\n- 验收: 略\n\n\n\n手写说明不能丢\n\n\n\n### 子标题\n\n\n- 备注: 保留\n",
        )
        .unwrap();
        let entries = store.load().unwrap();
        store.save(&entries).unwrap();
        let text = std::fs::read_to_string(&store.path).unwrap();
        assert!(!text.contains("\n\n\n"), "条目内也不该留连续空行:\n{text}");
        for keep in ["手写说明不能丢", "### 子标题", "- 备注: 保留", "- 验收: 略"]
        {
            assert!(text.contains(keep), "自由内容丢失: {keep}\n{text}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn integrity_detects_missing_and_duplicated_ids() {
        // D-112:缺号=数据丢失;活动+归档同现=归档半途而废。
        let dir = std::env::temp_dir().join(format!(
            "kz-integrity-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &DEFECTS);
        let mk = |id: &str, status: &str| Entry {
            id: id.into(),
            title: "t".into(),
            status: status.into(),
            severity: None,
            fields: vec![],
        };
        // 活动: D-001 D-004;归档: D-002 D-004 → 缺 D-003,重复 D-004。
        store
            .save(&[mk("D-001", "open"), mk("D-004", "open")])
            .unwrap();
        std::fs::write(
            store.archive_file(),
            "# Defects Archive\n\n## D-002 done [fixed]\n\n## D-004 dup [fixed]\n",
        )
        .unwrap();
        let issues = store.integrity_issues(&store.load().unwrap());
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues[0].contains("D-004"), "{issues:?}");
        assert!(issues[1].contains("D-003"), "{issues:?}");
        assert!(issues[1].contains("UNACCOUNTED"), "{issues:?}");
        // 措辞不能再断言"数据丢失",也必须给出两条结构化出路(D-173)。
        assert!(issues[1].contains("void_id"), "{issues:?}");
        assert!(issues[1].contains("repair_missing_id"), "{issues:?}");

        // 完整状态:无告警。
        store
            .save(&[
                mk("D-001", "open"),
                mk("D-003", "open"),
                mk("D-004", "open"),
            ])
            .unwrap();
        std::fs::write(
            store.archive_file(),
            "# Defects Archive\n\n## D-002 done [fixed]\n",
        )
        .unwrap();
        assert!(store.integrity_issues(&store.load().unwrap()).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refs_extraction() {
        let e = Entry {
            id: "F-001".into(),
            title: "t".into(),
            status: "draft".into(),
            severity: None,
            fields: vec![("refs".into(), "S-001, S-002".into())],
        };
        assert_eq!(e.refs(), vec!["S-001", "S-002"]);
    }
}
