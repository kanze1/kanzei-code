//! 统一引用分词(docs/design/doc_reference_graph.md §2;记忆图谱 memory_knowledge_graph.md §3)。
//!
//! 仓里曾有三套互不一致的切分规则:`Entry::refs` 只拆半角空格与逗号,`MemoryEntry::refs`
//! 只拆空白,`parse_release_condition` 连全角标点都认。本模块是唯一的一份:
//!
//! - 拆分符:半角/全角空格、半角/全角逗号、顿号;
//! - 剥掉括号注释(`R-149 (medium)`、`R-1（已归档）`);
//! - 展开区间 `R-161~R-167` / `D-104～D-110` / `R-161~167`(上限 [`RANGE_LIMIT`] 个,超限整体记脏);
//! - 识别关系前缀 `依据:` / `实现:` / `取代:` / `来源:`(半角或全角冒号;前缀后留空格也认下一个 token);
//! - 认不出的 token 记为「脏 token」上报,不静默丢弃。
//!
//! 放在 harness(依赖图里 memory 与 tools 共同的最低层)是为了让 `MemoryEntry::refs`
//! 与 `Entry::refs` 之后迁移时共用这一份(R-368 B1)。本模块不依赖 regex。

/// 区间展开上限:`R-1~R-99999` 这类笔误不能把一行 refs 变成十万条边。
pub const RANGE_LIMIT: usize = 200;

/// 关系类型(doc_reference_graph §2 的闭集前缀;无前缀 = 关联)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefRel {
    /// 无前缀:关联(默认,不阻塞)。
    Refs,
    /// `依据:`
    Basis,
    /// `实现:`
    Implements,
    /// `取代:`
    Supersedes,
    /// `来源:`
    DerivedFrom,
}

impl RefRel {
    /// 序列化键(图谱 IPC 的 `rel` 字段与前端 REL_KEYS 共用)。
    pub fn as_str(self) -> &'static str {
        match self {
            RefRel::Refs => "refs",
            RefRel::Basis => "basis",
            RefRel::Implements => "implements",
            RefRel::Supersedes => "supersedes",
            RefRel::DerivedFrom => "derived_from",
        }
    }

    fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            "依据" => Some(RefRel::Basis),
            "实现" => Some(RefRel::Implements),
            "取代" => Some(RefRel::Supersedes),
            "来源" => Some(RefRel::DerivedFrom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefToken {
    pub rel: RefRel,
    pub token: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SplitRefs {
    pub tokens: Vec<RefToken>,
    /// 认不出的原文 token(非编号、非路径、非 `area:`),以及超限区间。
    pub dirty: Vec<String>,
}

fn is_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ',' | '，' | '、' | '\u{3000}')
}

/// 剥掉半角/全角括号注释(不嵌套;未闭合的左括号原样保留,让它作为脏 token 暴露出来)。
fn strip_annotations(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '(' || ch == '（' {
            let rest: String = chars.clone().collect();
            if let Some(end) = rest.find([')', '）']) {
                let consumed = rest[..end].chars().count() + 1;
                for _ in 0..consumed {
                    chars.next();
                }
                out.push(' ');
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// `R-012` → Some(("R", "012"));字母前缀 1~3 个大写 ASCII,数字 1 位以上。
pub fn parse_id(token: &str) -> Option<(&str, &str)> {
    let (prefix, digits) = token.split_once('-')?;
    let prefix_ok =
        (1..=3).contains(&prefix.len()) && prefix.bytes().all(|b| b.is_ascii_uppercase());
    let digits_ok = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    (prefix_ok && digits_ok).then_some((prefix, digits))
}

fn looks_like_path(token: &str) -> bool {
    let path_chars = token
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '/' | '\\' | '.' | '_' | '-' | ':' | '#'));
    let has_shape = token.contains('/')
        || token.contains('\\')
        || token.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty() && !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric())
        });
    path_chars && has_shape
}

/// 区间 `R-161~R-167` / `R-161～167`:返回展开结果;不是区间返回 None;超限返回 Some(Err)。
fn expand_range(token: &str) -> Option<Result<Vec<String>, ()>> {
    let (left, right) = token.split_once(['~', '～'])?;
    let (prefix, start_digits) = parse_id(left)?;
    let end_digits = match parse_id(right) {
        Some((right_prefix, digits)) if right_prefix == prefix => digits,
        Some(_) => return None,
        None if !right.is_empty() && right.bytes().all(|b| b.is_ascii_digit()) => right,
        None => return None,
    };
    let start: usize = start_digits.parse().ok()?;
    let end: usize = end_digits.parse().ok()?;
    if end < start || end - start + 1 > RANGE_LIMIT {
        return Some(Err(()));
    }
    let width = start_digits.len();
    Some(Ok((start..=end)
        .map(|n| format!("{prefix}-{n:0width$}"))
        .collect()))
}

/// 统一分词:见模块注释。纯函数,不访问文件系统(存在性校验归调用方)。
pub fn split_refs(value: &str) -> SplitRefs {
    let cleaned = strip_annotations(value);
    let mut out = SplitRefs::default();
    let mut pending_rel: Option<RefRel> = None;
    for raw in cleaned.split(is_separator).filter(|t| !t.is_empty()) {
        let mut rel = pending_rel.take().unwrap_or(RefRel::Refs);
        let mut token = raw;
        if let Some((prefix, rest)) = raw.split_once([':', '：']) {
            if let Some(prefixed) = RefRel::from_prefix(prefix) {
                rel = prefixed;
                if rest.is_empty() {
                    pending_rel = Some(prefixed);
                    continue;
                }
                token = rest;
            }
        }
        match expand_range(token) {
            Some(Ok(ids)) => {
                out.tokens
                    .extend(ids.into_iter().map(|token| RefToken { rel, token }));
                continue;
            }
            Some(Err(())) => {
                out.dirty.push(raw.to_string());
                continue;
            }
            None => {}
        }
        let known =
            parse_id(token).is_some() || token.starts_with("area:") || looks_like_path(token);
        if known {
            out.tokens.push(RefToken {
                rel,
                token: token.to_string(),
            });
        } else {
            out.dirty.push(raw.to_string());
        }
    }
    if let Some(rel) = pending_rel {
        // 前缀后面什么都没有:整个前缀本身就是脏的。
        out.dirty.push(format!(
            "{}:",
            match rel {
                RefRel::Basis => "依据",
                RefRel::Implements => "实现",
                RefRel::Supersedes => "取代",
                RefRel::DerivedFrom => "来源",
                RefRel::Refs => "",
            }
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(value: &str) -> Vec<String> {
        split_refs(value)
            .tokens
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn split_refs_contract() {
        let range = ids("R-161~R-167");
        assert_eq!(range.len(), 7, "{range:?}");
        assert_eq!(range.first().map(String::as_str), Some("R-161"));
        assert_eq!(range.last().map(String::as_str), Some("R-167"));
        assert_eq!(ids("D-104～D-110").len(), 7);
        assert_eq!(ids("R-161~167").len(), 7, "右端可省略前缀");
        assert_eq!(
            ids("M-009~M-011"),
            vec!["M-009", "M-010", "M-011"],
            "保留零填充宽度"
        );

        let prefixed = split_refs("依据:A-007 实现：docs/design/x.md 取代: A-001 来源:I-003");
        let rels: Vec<(RefRel, &str)> = prefixed
            .tokens
            .iter()
            .map(|t| (t.rel, t.token.as_str()))
            .collect();
        assert_eq!(
            rels,
            vec![
                (RefRel::Basis, "A-007"),
                (RefRel::Implements, "docs/design/x.md"),
                (RefRel::Supersedes, "A-001"),
                (RefRel::DerivedFrom, "I-003"),
            ]
        );
        assert!(prefixed.dirty.is_empty(), "{:?}", prefixed.dirty);

        assert_eq!(ids("R-149 (medium)"), vec!["R-149"]);
        assert_eq!(ids("R-1（已归档） D-2"), vec!["R-1", "D-2"]);
        assert_eq!(
            ids("R-1、R-2，R-3,R-4\u{3000}R-5"),
            vec!["R-1", "R-2", "R-3", "R-4", "R-5"]
        );

        let dirty = split_refs("??? R-1 随便写写");
        assert_eq!(dirty.tokens.len(), 1);
        assert_eq!(dirty.dirty, vec!["???".to_string(), "随便写写".to_string()]);

        let huge = split_refs("R-1~R-99999");
        assert!(huge.tokens.is_empty(), "超限区间不展开");
        assert_eq!(huge.dirty, vec!["R-1~R-99999".to_string()]);
        assert_eq!(ids("R-1~R-200").len(), RANGE_LIMIT, "恰好上限仍展开");

        assert_eq!(
            ids("crates/kanzei-tools/src/tracker.rs area:kanzei-tools/edit 13-memory.js"),
            vec![
                "crates/kanzei-tools/src/tracker.rs",
                "area:kanzei-tools/edit",
                "13-memory.js"
            ]
        );
        assert_eq!(
            split_refs("依据:").dirty,
            vec!["依据:".to_string()],
            "悬空前缀记脏"
        );
        assert_eq!(parse_id("R-012"), Some(("R", "012")));
        assert_eq!(parse_id("r-1"), None);
        assert_eq!(parse_id("R-"), None);
    }
}
