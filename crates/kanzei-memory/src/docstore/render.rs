//! 渲染域(R-257 B3):条目/文档的 markdown 输出——render/render_with_template/
//! render_entry/render_heading/ensure_blank_separator。自 docstore.rs 原样迁出,
//! 零行为变更。

use super::model::{DocKind, Entry};
use super::parse::{push_field, DocumentTemplate, EntryTemplate, TemplateLine};

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
            push_field(&mut out, key, value);
        }
    }
    out
}

pub(crate) fn render_with_template(
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
    // D-329:模板尾部的空行是条目间距的残影(间距由 ensure_blank_separator 统一
    // 负责)。原样渲染会让追加的新字段落在空行之后——每次 update/close 都多出一段
    // 不可寻址的游离空段,且随写次数累积。渲染时裁掉尾部空 Raw,新字段紧跟末字段。
    let mut line_count = template.lines.len();
    while line_count > 0 {
        match &template.lines[line_count - 1] {
            TemplateLine::Raw(raw) if raw.trim().is_empty() => line_count -= 1,
            _ => break,
        }
    }
    let mut used = vec![false; entry.fields.len()];
    for line in &template.lines[..line_count] {
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
                    push_field(out, current_key, value);
                }
            }
        }
    }
    for (index, (key, value)) in entry.fields.iter().enumerate() {
        if !used[index] {
            push_field(out, key, value);
        }
    }
}

fn render_entry(out: &mut String, entry: &Entry) {
    render_heading(out, entry);
    for (key, value) in &entry.fields {
        push_field(out, key, value);
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
