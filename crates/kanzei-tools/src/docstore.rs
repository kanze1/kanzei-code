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
    bidirectional: false,
};

pub const DEFECTS: DocKind = DocKind {
    rel_path: ".kanzei/project/defects.md",
    heading: "Defects",
    prefix: "D",
    statuses: &["open", "fixing", "fixed", "wontfix"],
    terminal: &["fixed", "wontfix"],
    severities: Some(&["high", "medium", "low"]),
    bidirectional: false,
};

pub const SOURCES: DocKind = DocKind {
    rel_path: ".kanzei/research/sources.md",
    heading: "Sources",
    prefix: "S",
    statuses: &["active", "archived"],
    terminal: &["archived"],
    severities: None,
    bidirectional: false,
};

pub const FINDINGS: DocKind = DocKind {
    rel_path: ".kanzei/research/findings.md",
    heading: "Findings",
    prefix: "F",
    statuses: &["draft", "confirmed", "dropped"],
    terminal: &["confirmed", "dropped"],
    severities: None,
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

pub struct DocStore {
    pub kind: &'static DocKind,
    pub path: PathBuf,
}

impl DocStore {
    pub fn open(project_root: &Path, kind: &'static DocKind) -> Self {
        DocStore {
            kind,
            path: project_root.join(kind.rel_path),
        }
    }

    pub fn load(&self) -> std::io::Result<Vec<Entry>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(parse(self.kind, &text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, entries: &[Entry]) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, render(self.kind, entries))
    }

    pub fn next_id(&self, entries: &[Entry]) -> String {
        let max = entries
            .iter()
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

/// 宽容解析:`## ` 开头即条目;ID 缺失/状态缺失都不报错(手改友好)。
pub fn parse(kind: &DocKind, text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            entries.push(parse_heading(kind, rest));
        } else if let Some(entry) = entries.last_mut() {
            if let Some(bullet) = trimmed.trim_start().strip_prefix("- ") {
                if let Some((key, value)) = bullet.split_once(':') {
                    entry
                        .fields
                        .push((key.trim().to_string(), value.trim().to_string()));
                }
            }
        }
    }
    entries
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
        if t.ends_with(']') {
            if let Some(pos) = t.rfind('[') {
                status = t[pos + 1..t.len() - 1].trim().to_string();
                title = t[..pos].trim_end().to_string();
                continue;
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
