//! FTS 的 OR 命中只生成候选；按查询锚点与适用描述决定是否值得呈现。
use super::MemoryEntry;

fn terms(query: &str) -> Vec<String> {
    super::retrieval::intent_query(query)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn identifier(term: &str) -> bool {
    term.is_ascii()
        && term.chars().any(|c| c.is_ascii_alphanumeric())
        && (term.contains('_')
            || term.contains('.')
            || (term.contains('-') && term.chars().any(|c| c.is_ascii_digit())))
}

fn contains(text: &str, term: &str) -> bool {
    if term.is_ascii() {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
            .any(|token| token == term)
    } else {
        text.contains(term)
    }
}

/// 符号、文件名和条目编号是显式检索约束；dense 通道也不能绕过。
pub(super) fn anchors_match(query: &str, entry: &MemoryEntry) -> bool {
    let terms = terms(query);
    let anchors: Vec<_> = terms.iter().filter(|term| identifier(term)).collect();
    if anchors.is_empty() {
        return true;
    }
    let text = format!(
        "{} {} {} {} {:?}",
        entry.id, entry.title, entry.description, entry.body, entry.extras
    )
    .to_lowercase();
    anchors.iter().any(|anchor| contains(&text, anchor))
}

pub(super) fn lexical_weight(query: &str, entry: &MemoryEntry) -> Option<f64> {
    if !anchors_match(query, entry) {
        return None;
    }
    let terms = terms(query);
    if terms.is_empty() {
        return None;
    }
    let scope = format!("{} {} {}", entry.id, entry.title, entry.description).to_lowercase();
    let body = entry.body.to_lowercase();
    let scope_hits = terms.iter().filter(|term| contains(&scope, term)).count();
    let hits = terms
        .iter()
        .filter(|term| contains(&scope, term) || contains(&body, term))
        .count();
    let coverage = hits as f64 / terms.len() as f64;
    let anchored = terms.iter().any(|term| identifier(term));
    // 多词查询只有正文中的零星交集，通常只是历史叙述共词。允许空结果。
    if !anchored && ((scope_hits == 0 && coverage < 0.5) || coverage < 0.2) {
        return None;
    }
    Some(1.0 + coverage + scope_hits as f64 / terms.len() as f64)
}
