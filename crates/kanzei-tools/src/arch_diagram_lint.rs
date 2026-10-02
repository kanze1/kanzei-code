//! 手写图(`docs/architecture/NN_slug.md`)扫描与 mermaid 子集 lint(UI2-0926 #7,
//! docs/design/architecture_diagrams.md §8)。只对「确定写错」的形态报 error;可疑但不一定错的
//! (规模、疑似拼错、字面量 \n、行号锚点漂移)只报警告,门禁必须可满足。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use super::{DIAGRAM_DIR, MAX_EDGES, MAX_NODES, RESERVED_IDS, SEMANTIC_CLASSES};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LintIssue {
    /// 文件内行号(1 起)。
    pub line: usize,
    pub severity: Severity,
    /// D1–D9。
    pub code: &'static str,
    pub message: String,
    /// 一句修法,弱模型照着改就行。
    pub hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagramDoc {
    /// 文件名去掉 .md。
    pub id: String,
    /// 项目根相对路径。
    pub path: String,
    pub title: String,
    pub summary: String,
    /// 第一个 mermaid 围栏里的源码(LF)。
    pub source: String,
    /// 源码第一行在文件里的行号(1 起;没有围栏时为 0)。
    pub source_line: usize,
    pub issues: Vec<LintIssue>,
}

impl DiagramDoc {
    pub fn errors(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }
}

fn issue(
    line: usize,
    severity: Severity,
    code: &'static str,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> LintIssue {
    LintIssue {
        line,
        severity,
        code,
        message: message.into(),
        hint: hint.into(),
    }
}

/// `NN_snake_case.md`:两位数字前缀决定标签页顺序。
fn is_diagram_file_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    let bytes = stem.as_bytes();
    bytes.len() > 3
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2] == b'_'
        && stem[3..]
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !stem.ends_with('_')
}

/// 扫描 `docs/architecture/*.md`(按文件名排序),逐个取标题/说明/首个 mermaid 围栏并 lint。
pub fn scan_diagrams(root: &Path) -> Vec<DiagramDoc> {
    let dir = root.join(DIAGRAM_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".md") && !n.eq_ignore_ascii_case("README.md"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let text = std::fs::read_to_string(dir.join(&name)).unwrap_or_default();
            parse_diagram_doc(root, &format!("{DIAGRAM_DIR}/{name}"), &text)
        })
        .collect()
}

/// 解析一份图文档(CRLF 兼容)并 lint。
pub fn parse_diagram_doc(root: &Path, path: &str, text: &str) -> DiagramDoc {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let id = file_name
        .strip_suffix(".md")
        .unwrap_or(file_name)
        .to_string();
    let mut issues = Vec::new();
    if !is_diagram_file_name(file_name) {
        issues.push(issue(
            1,
            Severity::Error,
            "D2",
            format!("文件名 `{file_name}` 不合约定"),
            "改名为「两位数字_小写蛇形.md」,如 03_memory_flow.md(数字决定标签页顺序)",
        ));
    }
    let first_content = lines.iter().position(|l| !l.trim().is_empty());
    let title = match first_content {
        Some(index) if lines[index].starts_with("# ") => lines[index][2..].trim().to_string(),
        _ => {
            issues.push(issue(
                first_content.map_or(1, |i| i + 1),
                Severity::Error,
                "D1",
                "第一行必须是 `# 标题`",
                "在文件第一行写 `# 这张图叫什么`",
            ));
            String::new()
        }
    };
    // 说明:标题之后第一段(遇到空行、标题或围栏为止)。
    let mut summary_lines = Vec::new();
    let start = first_content.map_or(0, |i| i + 1);
    for line in lines.iter().skip(start) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if summary_lines.is_empty() {
                continue;
            }
            break;
        }
        if trimmed.starts_with('#') || trimmed.starts_with("```") {
            break;
        }
        summary_lines.push(trimmed);
    }
    let summary = summary_lines.join(" ");

    let fence_at = lines.iter().position(|l| {
        l.trim_start().starts_with("```") && l.trim().trim_start_matches('`').trim() == "mermaid"
    });
    let (source, source_line) = match fence_at {
        None => {
            issues.push(issue(
                lines.len().max(1),
                Severity::Error,
                "D1",
                "没有 ```mermaid 围栏",
                "在说明之后加一个 ```mermaid … ``` 代码块,里面写 flowchart",
            ));
            (String::new(), 0)
        }
        Some(open) => {
            let close = lines
                .iter()
                .enumerate()
                .skip(open + 1)
                .find(|(_, l)| l.trim() == "```")
                .map(|(i, _)| i);
            match close {
                None => {
                    issues.push(issue(
                        open + 1,
                        Severity::Error,
                        "D1",
                        "mermaid 围栏没有闭合",
                        "在图源码最后一行之后补一行 ```",
                    ));
                    (lines[open + 1..].join("\n"), open + 2)
                }
                Some(close) => (lines[open + 1..close].join("\n"), open + 2),
            }
        }
    };
    if source_line > 0 {
        issues.extend(lint_mermaid(root, &source, source_line));
    }
    issues.sort_by_key(|i| (i.line, i.code));
    DiagramDoc {
        id,
        path: path.to_string(),
        title,
        summary,
        source,
        source_line,
        issues,
    }
}

/// 形状开/闭定界符(长的在前,贪心匹配)。
const SHAPES: [(&str, &str); 12] = [
    ("(((", ")))"),
    ("([", "])"),
    ("[[", "]]"),
    ("[(", ")]"),
    ("((", "))"),
    ("{{", "}}"),
    ("[/", "/]"),
    ("[\\", "\\]"),
    ("(", ")"),
    ("[", "]"),
    ("{", "}"),
    (">", "]"),
];

#[derive(Debug, Default)]
struct NodeRef {
    id: String,
    /// 带形状/标签(即「声明」);裸 id 为 false。
    labelled: bool,
    classes: Vec<String>,
}

/// 语句行里的节点引用(近似解析:认不出的片段直接跳过,绝不因此报 error)。
/// 返回 (节点引用, 边数, 未加引号且含特殊字符的标签)。
fn scan_statement(line: &str) -> (Vec<NodeRef>, usize, Vec<String>) {
    let chars: Vec<char> = line.chars().collect();
    let mut nodes = Vec::new();
    let mut edges = 0usize;
    let mut bad_labels = Vec::new();
    let mut i = 0usize;
    let is_id_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.';
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() || c == '&' || c == ';' {
            i += 1;
            continue;
        }
        // 边:连续的 - = . < > o x ~ 组合,可带 |文字|。
        if matches!(c, '-' | '=' | '.' | '<' | '~') {
            let start = i;
            while i < chars.len()
                && matches!(chars[i], '-' | '=' | '.' | '<' | '>' | '~' | 'o' | 'x')
            {
                // o/x 只在箭头尾部算(`--o`),否则是下一个 id 的开头。
                if matches!(chars[i], 'o' | 'x')
                    && (i + 1 < chars.len() && is_id_char(chars[i + 1]))
                {
                    break;
                }
                i += 1;
            }
            let op: String = chars[start..i].iter().collect();
            if matches!(op.as_str(), "--" | "==" | "-.") {
                // `A -- 文字 --> B` 形态:开头只有 `--`/`==`/`-.`,跳到收尾箭头为止。
                let rest: String = chars[i..].iter().collect();
                let closer = ["-->", "---", "==>", "===", ".->", "-.-", "--x", "--o"]
                    .iter()
                    .filter_map(|c| rest.find(c).map(|pos| (pos, c.chars().count())))
                    .min();
                let Some((pos, len)) = closer else {
                    return (nodes, edges, bad_labels);
                };
                i += rest[..pos].chars().count() + len;
                edges += 1;
            } else if op.contains("--")
                || op.contains("==")
                || op.contains("-.")
                || op.contains("~~")
            {
                edges += 1;
            }
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '|' {
                if let Some(end) = (i + 1..chars.len()).find(|&j| chars[j] == '|') {
                    i = end + 1;
                } else {
                    return (nodes, edges, bad_labels);
                }
            }
            continue;
        }
        if !is_id_char(c) {
            // 看不懂:放弃这一行剩余部分(宁可漏报,不误报)。
            break;
        }
        let start = i;
        while i < chars.len() && is_id_char(chars[i]) {
            // `a-->b`:id 在第一个 `--` / `-.` / `==` 前结束。
            if chars[i] == '-' && i + 1 < chars.len() && matches!(chars[i + 1], '-' | '.' | '>') {
                break;
            }
            if chars[i] == '.' && i + 1 < chars.len() && chars[i + 1] == '-' {
                break;
            }
            i += 1;
        }
        let id: String = chars[start..i].iter().collect();
        let mut node = NodeRef {
            id,
            ..Default::default()
        };
        let rest: String = chars[i..].iter().collect();
        if let Some((open, close)) = SHAPES.iter().find(|(open, _)| rest.starts_with(open)) {
            node.labelled = true;
            let after_open = i + open.chars().count();
            let body: String = chars[after_open..].iter().collect();
            let trimmed = body.trim_start();
            if trimmed.starts_with('"') {
                // 带引号:找配对引号,再找闭合定界符。
                let lead = body.chars().count() - trimmed.chars().count();
                let q0 = after_open + lead;
                let Some(q1) = (q0 + 1..chars.len()).find(|&j| chars[j] == '"') else {
                    // 引号标签跨行(markdown 字符串):节点已声明,续行由调用方跳过。
                    nodes.push(node);
                    return (nodes, edges, bad_labels);
                };
                let tail: String = chars[q1 + 1..].iter().collect();
                let Some(pos) = tail.find(close) else {
                    return (nodes, edges, bad_labels);
                };
                i = q1 + 1 + tail[..pos].chars().count() + close.chars().count();
            } else {
                let Some(pos) = body.find(close) else {
                    return (nodes, edges, bad_labels);
                };
                let label = &body[..pos];
                if label.contains(['(', ')', '[', ']', '{', '}', '|', '"']) {
                    bad_labels.push(label.to_string());
                }
                i = after_open + label.chars().count() + close.chars().count();
            }
        }
        while i + 2 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' && chars[i + 2] == ':' {
            let s = i + 3;
            let mut e = s;
            while e < chars.len()
                && (chars[e].is_alphanumeric() || chars[e] == '_' || chars[e] == '-')
            {
                e += 1;
            }
            node.classes.push(chars[s..e].iter().collect());
            i = e;
        }
        nodes.push(node);
    }
    (nodes, edges, bad_labels)
}

/// 条目号目标(`R-123`):与渲染器 04-diagram.js 的 REF_TARGET 同一集合(含研究发现 F-)。
fn is_ref_target(target: &str) -> bool {
    target.len() > 2
        && matches!(
            target.as_bytes()[0],
            b'R' | b'D' | b'I' | b'S' | b'T' | b'F'
        )
        && target.as_bytes()[1] == b'-'
        && target[2..].chars().all(|c| c.is_ascii_digit())
}

/// `path[:行[-行]]` → (路径, 起始行)。
fn split_line_anchor(target: &str) -> (&str, Option<usize>) {
    if let Some((head, tail)) = target.rsplit_once(':') {
        let line_part = tail.split('-').next().unwrap_or("");
        if !line_part.is_empty() && line_part.chars().all(|c| c.is_ascii_digit()) {
            return (head, line_part.parse().ok());
        }
    }
    (target, None)
}

fn click_target_problem(root: &Path, target: &str) -> Option<String> {
    if is_ref_target(target) {
        return None;
    }
    let (path, _) = split_line_anchor(target);
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.split('/').any(|seg| seg == "..")
    {
        return Some(format!("目标 `{target}` 不是项目内的相对路径"));
    }
    if normalized.is_empty() || !root.join(&normalized).exists() {
        return Some(format!("目标文件不存在:`{normalized}`"));
    }
    None
}

/// 行号锚点漂移(警告):源码一改,写死的 `:行号` 就悄悄指到别处。两种能机械判定的情形——
/// 行号超过文件行数;提示以「标识符:」开头(`run_once_with_parts:…`)而锚定的那一行里没有这个标识符。
fn line_anchor_drift(root: &Path, target: &str, tip: &str) -> Option<String> {
    let (path, Some(line)) = split_line_anchor(target) else {
        return None;
    };
    let text = std::fs::read_to_string(root.join(path.replace('\\', "/"))).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    if line == 0 || line > lines.len() {
        return Some(format!(
            "`{target}` 的行号 {line} 超过文件行数 {}",
            lines.len()
        ));
    }
    let ident: String = tip
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let labelled = !ident.is_empty()
        && !ident.starts_with(|c: char| c.is_ascii_digit())
        && tip[ident.len()..].starts_with([':', '：']);
    if labelled && !lines[line - 1].contains(ident.as_str()) {
        return Some(format!(
            "`{target}` 这一行里没有 `{ident}`,行号可能已经漂移"
        ));
    }
    None
}

/// mermaid 子集 lint。`source_line` 是源码第一行在文件里的行号,输出行号都是文件行号。
pub fn lint_mermaid(root: &Path, source: &str, source_line: usize) -> Vec<LintIssue> {
    let normalized = source.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let file_line = |index: usize| source_line + index;
    let mut issues = Vec::new();
    if lines.iter().all(|l| l.trim().is_empty()) {
        issues.push(issue(
            source_line,
            Severity::Error,
            "D1",
            "mermaid 围栏是空的",
            "第一行写 `flowchart LR`(或 TB),下面写节点与连线",
        ));
        return issues;
    }
    // frontmatter(`---` … `---`)里不准有 config。
    let mut body_start = 0usize;
    if lines.first().map(|l| l.trim()) == Some("---") {
        if let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---") {
            for (offset, line) in lines[1..=end].iter().enumerate() {
                if line.trim_start().starts_with("config:") {
                    issues.push(issue(
                        file_line(offset + 1),
                        Severity::Error,
                        "D3",
                        "frontmatter 里写了 config(主题/布局由 kanzei 统一注入)",
                        "删掉 config 段;要标题就只留 `title:`",
                    ));
                }
            }
            body_start = end + 2;
        }
    }
    // 图种:frontmatter 与注释之后的第一行。class/sequence 等图的逐行检查会误报(`class Foo {` 不是语义类),
    // 架构图约定只写 flowchart,这里只报一条规则、不往下查。
    let header = lines
        .iter()
        .enumerate()
        .skip(body_start)
        .map(|(index, line)| (index, line.trim()))
        .find(|(_, line)| !line.is_empty() && !line.starts_with("%%"));
    if let Some((index, line)) = header {
        let kind = line.split_whitespace().next().unwrap_or("");
        if kind != "flowchart" && kind != "graph" {
            issues.push(issue(
                file_line(index),
                Severity::Error,
                "D1",
                format!("架构图只支持 flowchart,这里是 `{kind}`"),
                "第一行写 `flowchart LR`(或 TB);时序图、类图等放进设计文档里的 ```mermaid 围栏",
            ));
            return issues;
        }
    }
    let mut declared_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut labelled: BTreeSet<String> = BTreeSet::new();
    let mut first_seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut edge_count = 0usize;
    let mut clicks: Vec<(usize, String)> = Vec::new();
    let mut subgraph_ids: BTreeSet<String> = BTreeSet::new();
    let mut other_text = String::new();
    // 跨行的引号标签(markdown 字符串 "`**名**\n说明`" 的续行)不是语句,整段跳过。
    let mut in_string = false;
    let odd_quotes = |line: &str| line.matches('"').count() % 2 == 1;

    for (index, raw) in lines.iter().enumerate().skip(body_start) {
        let line = raw.trim();
        let at = file_line(index);
        if in_string {
            other_text.push_str(line);
            other_text.push('\n');
            if !odd_quotes(line) {
                continue;
            }
            in_string = false;
            // 收尾行:第一个引号闭合标签,之后是形状收尾、可选的 :::类名,再往后照常是语句。
            let tail = line.split_once('"').map_or("", |(_, rest)| rest);
            let mut tail = tail.trim_start_matches([')', ']', '}', '>', '/', '\\']);
            while let Some(rest) = tail.strip_prefix(":::") {
                let class: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                if !SEMANTIC_CLASSES.contains(&class.as_str()) {
                    issues.push(issue(
                        at,
                        Severity::Error,
                        "D4",
                        format!("未知的类名 `:::{class}`"),
                        format!("只能用 :::{}", SEMANTIC_CLASSES.join(" / :::")),
                    ));
                }
                tail = &rest[class.len()..];
            }
            if odd_quotes(tail) {
                in_string = true;
            }
            let (nodes, edges, _) = scan_statement(tail);
            edge_count += edges;
            for node in nodes {
                *declared_counts.entry(node.id.clone()).or_default() += 1;
                first_seen.entry(node.id.clone()).or_insert(at);
                if node.labelled {
                    labelled.insert(node.id.clone());
                }
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if !line.starts_with("%%") && odd_quotes(line) {
            in_string = true;
        }
        if line.starts_with("%%{") {
            issues.push(issue(
                at,
                Severity::Error,
                "D3",
                "出现了 %%{init} 指令(主题/布局由 kanzei 统一注入)",
                "删掉这一行;要强调某个节点就用 :::focus 这类语义类",
            ));
            continue;
        }
        if line.starts_with("%%") {
            continue;
        }
        if line.contains("\\n") {
            issues.push(issue(
                at,
                Severity::Warn,
                "D9",
                "标签里写了字面量 \\n(mermaid 不认,会原样显示)",
                "换行改用 <br/>",
            ));
        }
        let keyword = line.split_whitespace().next().unwrap_or("");
        match keyword {
            "flowchart" | "graph" | "direction" | "end" => continue,
            "style" | "classDef" | "linkStyle" => {
                issues.push(issue(
                    at,
                    Severity::Error,
                    "D4",
                    format!("出现了 {keyword}(图里不写颜色,配色由 kanzei 按主题注入)"),
                    format!(
                        "删掉这一行;需要区分节点就用语义类 :::{}",
                        SEMANTIC_CLASSES.join(" / :::")
                    ),
                ));
                continue;
            }
            "class" => {
                // class a,b focus
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(name) = parts.get(2) {
                    if !SEMANTIC_CLASSES.contains(name) {
                        issues.push(issue(
                            at,
                            Severity::Error,
                            "D4",
                            format!("未知的类名 `{name}`"),
                            format!("只能用 {}", SEMANTIC_CLASSES.join(" / ")),
                        ));
                    }
                }
                other_text.push_str(line);
                other_text.push('\n');
                continue;
            }
            "click" => {
                clicks.push((at, line.to_string()));
                continue;
            }
            "subgraph" => {
                let rest = line["subgraph".len()..].trim();
                let id: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                if !id.is_empty() {
                    if RESERVED_IDS.contains(&id.to_ascii_lowercase().as_str()) {
                        issues.push(issue(
                            at,
                            Severity::Error,
                            "D6",
                            format!("子图 id `{id}` 是保留字"),
                            format!("换个 id,如 `{id}_grp`"),
                        ));
                    }
                    subgraph_ids.insert(id);
                }
                other_text.push_str(line);
                other_text.push('\n');
                continue;
            }
            _ => {}
        }
        other_text.push_str(line);
        other_text.push('\n');
        let (nodes, edges, bad_labels) = scan_statement(line);
        edge_count += edges;
        for label in bad_labels {
            issues.push(issue(
                at,
                Severity::Error,
                "D6",
                format!("标签 `{label}` 含括号/竖线/引号却没加引号(mermaid 解析不了)"),
                format!("写成 id[\"{}\"](整段标签加双引号)", label.replace('"', "'")),
            ));
        }
        for node in nodes {
            if node.id.is_empty() {
                continue;
            }
            if RESERVED_IDS.contains(&node.id.to_ascii_lowercase().as_str()) {
                issues.push(issue(
                    at,
                    Severity::Error,
                    "D6",
                    format!("节点 id `{}` 是保留字", node.id),
                    format!("换个 id,如 `{}_node`,标签照旧写在方括号里", node.id),
                ));
            }
            for class in &node.classes {
                if !SEMANTIC_CLASSES.contains(&class.as_str()) {
                    issues.push(issue(
                        at,
                        Severity::Error,
                        "D4",
                        format!("未知的类名 `:::{class}`"),
                        format!("只能用 :::{}", SEMANTIC_CLASSES.join(" / :::")),
                    ));
                }
            }
            *declared_counts.entry(node.id.clone()).or_default() += 1;
            first_seen.entry(node.id.clone()).or_insert(at);
            if node.labelled {
                labelled.insert(node.id.clone());
            }
        }
    }

    // click 行:语法、id 已声明、目标存在且不越出项目根。
    let word_in = |text: &str, id: &str| {
        text.match_indices(id).any(|(pos, _)| {
            let before = text[..pos].chars().next_back();
            let after = text[pos + id.len()..].chars().next();
            let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            boundary(before) && boundary(after)
        })
    };
    for (at, line) in &clicks {
        let rest = line["click".len()..].trim();
        let id: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
        let after = rest[id.len()..].trim();
        let quoted: Vec<&str> = after.split('"').collect();
        // after 应为 `"target"` 或 `"target" "tip"`:split('"') 得 ["", target, "", ...]。
        let well_formed = !id.is_empty()
            && after.starts_with('"')
            && (quoted.len() == 3 || (quoted.len() == 5 && quoted[2].trim().is_empty()))
            && quoted.last().is_some_and(|tail| tail.trim().is_empty());
        if !well_formed {
            issues.push(issue(
                *at,
                Severity::Error,
                "D5",
                "click 语法不对",
                "写成 click 节点id \"项目相对路径[:行号]\" \"悬停提示\"(或目标写条目号 R-123)",
            ));
            continue;
        }
        if !declared_counts.contains_key(&id)
            && !subgraph_ids.contains(&id)
            && !word_in(&other_text, &id)
        {
            issues.push(issue(
                *at,
                Severity::Error,
                "D5",
                format!("click 的节点 `{id}` 在图里不存在"),
                "先声明这个节点,或把 id 改成已有节点",
            ));
        }
        if let Some(problem) = click_target_problem(root, quoted[1]) {
            issues.push(issue(
                *at,
                Severity::Error,
                "D5",
                problem,
                "目标写项目根相对路径(可带 :行号),先确认文件真的存在",
            ));
        } else if let Some(drift) = line_anchor_drift(root, quoted[1], quoted.get(3).unwrap_or(&""))
        {
            issues.push(issue(
                *at,
                Severity::Warn,
                "D5",
                drift,
                "改成当前的行号,或去掉 :行号 只锚到文件(文件不会漂移)",
            ));
        }
    }

    // D7:规模(警告)。
    let node_count = declared_counts.len();
    if node_count > MAX_NODES || edge_count > MAX_EDGES {
        issues.push(issue(
            source_line,
            Severity::Warn,
            "D7",
            format!(
                "图太大:{node_count} 个节点、{edge_count} 条边(上限 {MAX_NODES} / {MAX_EDGES})"
            ),
            "按子系统拆成几张图,每张一个文件",
        ));
    }
    // D8:裸 id 全图只出现一次——多半是拼错了已有节点的 id。
    for (id, count) in &declared_counts {
        if *count == 1 && !labelled.contains(id) && !subgraph_ids.contains(id) {
            let at = first_seen.get(id).copied().unwrap_or(source_line);
            issues.push(issue(
                at,
                Severity::Warn,
                "D8",
                format!("节点 `{id}` 全图只出现一次且没有标签,疑似拼错"),
                "核对 id 拼写;确是新节点就给它写上标签 id[\"说明\"]",
            ));
        }
    }
    issues
}

/// 规范化成展示用的一行:`第 N 行 [D5 error] 消息 → 修法`。
pub fn render_issue(path: &str, issue: &LintIssue) -> String {
    let severity = match issue.severity {
        Severity::Error => "error",
        Severity::Warn => "warn",
    };
    format!(
        "{path}:{} [{} {severity}] {} → 修法:{}",
        issue.line, issue.code, issue.message, issue.hint
    )
}
