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
    /// 非终态之间允许自由往返(目标 active⇄paused);false = 只进不退。
    pub bidirectional: bool,
}

pub const REQUIREMENTS: DocKind = DocKind {
    rel_path: ".kanzei/project/requirements.md",
    heading: "Requirements",
    prefix: "R",
    statuses: &["todo", "doing", "done", "dropped"],
    terminal: &["done", "dropped"],
    severities: None,
    priorities: Some(&["P0", "P1", "P2", "P3"]),
    bidirectional: false,
};

pub const DEFECTS: DocKind = DocKind {
    rel_path: ".kanzei/project/defects.md",
    heading: "Defects",
    prefix: "D",
    statuses: &["open", "fixing", "fixed", "wontfix"],
    terminal: &["fixed", "wontfix"],
    severities: Some(&["high", "medium", "low"]),
    priorities: Some(&["P0", "P1", "P2", "P3"]),
    bidirectional: false,
};

pub const SOURCES: DocKind = DocKind {
    rel_path: ".kanzei/research/sources.md",
    heading: "Sources",
    prefix: "S",
    statuses: &["active", "archived"],
    terminal: &["archived"],
    severities: None,
    priorities: None,
    bidirectional: false,
};

pub const FINDINGS: DocKind = DocKind {
    rel_path: ".kanzei/research/findings.md",
    heading: "Findings",
    prefix: "F",
    statuses: &["draft", "confirmed", "dropped"],
    terminal: &["confirmed", "dropped"],
    severities: None,
    priorities: None,
    bidirectional: false,
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
    // 结论可能被重新确认,允许 active⇄stale 往返。
    bidirectional: true,
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
    bidirectional: false,
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
    bidirectional: true,
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

    /// ID 分配扫活跃 + 归档两个文件:归档移走条目后编号绝不复用。
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
        let mut archive_template = self.preserved_archive.lock().unwrap().clone().unwrap_or(DocumentTemplate {
            preamble: Vec::new(),
            entries: Vec::new(),
        });
        if let Some(active_template) = active_template {
            for entry in &terminal {
                if archive_template.entries.iter().all(|template| template.id != entry.id) {
                    if let Some(template) = active_template.entries.iter().find(|template| template.id == entry.id) {
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

    /// 数据完整性检测(D-112):ID 由引擎顺序分配且无删除操作,因此
    /// 活动∪归档中的缺号即数据丢失;同一 ID 出现在两份文档中即归档半途而废。
    /// 任何一种都值得在每次工具调用时向模型告警。
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
        let mut issues = Vec::new();
        let both: Vec<u32> = active_ids.intersection(&archive_ids).copied().collect();
        if !both.is_empty() {
            issues.push(format!(
                "present in BOTH active and archive (incomplete archive?): {}",
                format_ids(self.kind.prefix, &both)
            ));
        }
        let Some(max) = active_ids.iter().chain(archive_ids.iter()).max().copied() else {
            return issues;
        };
        let missing: Vec<u32> = (1..=max)
            .filter(|n| !active_ids.contains(n) && !archive_ids.contains(n))
            .collect();
        if !missing.is_empty() {
            issues.push(format!(
                "MISSING from both active and archive — likely data loss, restore from git history: {}",
                format_ids(self.kind.prefix, &missing)
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
                    entry
                        .fields
                        .push((key.clone(), value.trim().to_string()));
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

fn render_with_template(
    kind: &DocKind,
    entries: &[Entry],
    template: &DocumentTemplate,
) -> String {
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
        let entry_template = template.entries.iter().find(|candidate| candidate.id == entry.id);
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
                if let Some((index, (current_key, value))) = entry
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
            .save(&[mk("R-001", "done"), mk("R-002", "doing"), mk("R-003", "dropped")])
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
        store.save(&[mk("R-001"), mk("R-002"), mk("R-003")]).unwrap();

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
        for keep in ["手写说明不能丢", "### 子标题", "- 备注: 保留", "- 验收: 略"] {
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
        store.save(&[mk("D-001", "open"), mk("D-004", "open")]).unwrap();
        std::fs::write(
            store.archive_file(),
            "# Defects Archive\n\n## D-002 done [fixed]\n\n## D-004 dup [fixed]\n",
        )
        .unwrap();
        let issues = store.integrity_issues(&store.load().unwrap());
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues[0].contains("D-004"), "{issues:?}");
        assert!(issues[1].contains("D-003"), "{issues:?}");
        assert!(issues[1].contains("MISSING"), "{issues:?}");

        // 完整状态:无告警。
        store.save(&[mk("D-001", "open"), mk("D-003", "open"), mk("D-004", "open")]).unwrap();
        std::fs::write(store.archive_file(), "# Defects Archive\n\n## D-002 done [fixed]\n").unwrap();
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
