//! 解析域(R-257 B3):标题状态标记清洗、模板结构(TemplateLine/EntryTemplate/
//! DocumentTemplate/ParsedDocument)与宽容解析(parse/parse_document/parse_heading)。
//! 自 docstore.rs 原样迁出,零行为变更。

use super::model::{DocKind, Entry, ALL_STATUS_TOKENS};

/// 标题中是否含跨 DocKind 状态标记(形如 `[done]` / `[dropped]`,大小写不敏感)。
/// 状态的家是 header 方括号,不是标题——标题带状态标记即污染(D-331)。
pub fn title_status_marker(title: &str) -> Option<&'static str> {
    let lower = title.to_ascii_lowercase();
    ALL_STATUS_TOKENS
        .iter()
        .find(|tok| lower.contains(&format!("[{tok}]")))
        .copied()
}

/// 清除标题里的全部跨 DocKind 状态标记(D-331 纠错用):反复移除 `[token]`
/// (大小写不敏感)直到干净,再把多余空白折叠。只删标记,其余标题逐字保留。
pub fn strip_status_markers(title: &str) -> String {
    let mut out = title.to_string();
    loop {
        let lower = out.to_ascii_lowercase();
        let found = ALL_STATUS_TOKENS.iter().find_map(|tok| {
            let needle = format!("[{tok}]");
            lower.find(&needle).map(|idx| (idx, needle.len()))
        });
        match found {
            Some((idx, len)) => {
                out.replace_range(idx..idx + len, "");
            }
            None => break,
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
pub(crate) enum TemplateLine {
    Raw(String),
    Field(String),
}

#[derive(Debug, Clone)]
pub(crate) struct EntryTemplate {
    pub(crate) id: String,
    pub(crate) lines: Vec<TemplateLine>,
}

#[derive(Debug, Clone)]
pub(crate) struct DocumentTemplate {
    pub(crate) preamble: Vec<String>,
    pub(crate) entries: Vec<EntryTemplate>,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedDocument {
    pub(crate) entries: Vec<Entry>,
    pub(crate) template: DocumentTemplate,
}

/// 宽容解析:`## ` 开头即条目;ID 缺失/状态缺失都不报错(手改友好)。
pub fn parse(kind: &DocKind, text: &str) -> Vec<Entry> {
    parse_document(kind, text).entries
}

pub(crate) fn parse_document(kind: &DocKind, text: &str) -> ParsedDocument {
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
        // status 剥离判据(D-332 重构):只有「方括号在尾部且 [ 前是空白」才是状态标记
        // 形态——`vec[index]` 的 [ 前是字母、`[DONE] 帧` 的 ] 不在尾部,两者都不是状态
        // 标记,原样保留(D-070 与 D-002 同族)。形态符合时**合法/非法都剥离**:非法
        // candidate(如 requirement 上的 `[open]`)保留在 status 字段里,由调度层
        // fail-closed(INVALID + integrity 报错),不再静默变空字符串被当成可执行。
        if t.ends_with(']') {
            if let Some(pos) = t.rfind('[') {
                let candidate = t[pos + 1..t.len() - 1].trim();
                let preceded_by_space = pos > 0
                    && t[..pos]
                        .chars()
                        .last()
                        .map(|c| c.is_whitespace())
                        .unwrap_or(false);
                if preceded_by_space && !candidate.is_empty() {
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

/// 字段值写进文档前必须归一成**单行**——这是往返不变式的唯一守点。
///
/// 解析契约是「一行一个字段」(见 `parse_document`):只有 `- key: value` 那一行会
/// 成为字段,其余任何行都落进 `TemplateLine::Raw`——原样保留但**不可寻址**。于是
/// 带换行的字段值一旦写出去,第 2 行起就永久脱离字段体系:update 只改得到第一行,
/// 剩下的段落**没有任何工具能删**(tracker 直写被拒、git restore 被引擎拦、shell
/// 整文件重写被拦)。实测 D-239 因此积了 3 份重复的「验收复核」段落(M-056 记录)。
///
/// 这里把换行折成空格,保证「写进去的东西一定能原样解析回来」。段落结构会丢,但
/// 内容一字不少——比起产生删不掉的垃圾,这是明显更小的代价。四个渲染出口必须都
/// 走这里,漏一个就等于漏一条产生游离段落的路。
pub(crate) fn push_field(out: &mut String, key: &str, value: &str) {
    let single_line = value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    out.push_str(&format!("- {key}: {single_line}\n"));
}
