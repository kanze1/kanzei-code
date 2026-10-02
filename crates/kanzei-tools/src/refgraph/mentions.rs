//! 正文编号提及抽取:`[RDAMU]-\d{2,4}`,前一个字符不是 ASCII 字母数字或下划线,
//! 后一个字符不是数字。regex crate 不支持 look-around,边界靠手工检查。

/// 一次提及:规范编号与所在行(1 起)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub id: String,
    pub line: usize,
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// 抽取 `prefixes`(如 `"RDAMU"`)开头的编号提及,按出现顺序,同一编号同一行只记一次。
pub fn ids_in(text: &str, prefixes: &str) -> Vec<Mention> {
    let bytes = text.as_bytes();
    let mut out: Vec<Mention> = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        let starts = prefixes.as_bytes().contains(&byte)
            && (i == 0 || !is_word_byte(bytes[i - 1]))
            && bytes.get(i + 1) == Some(&b'-');
        if starts {
            let digits = bytes[i + 2..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            if (2..=4).contains(&digits) {
                let id = &text[i..i + 2 + digits];
                if !out.iter().any(|m| m.id == id && m.line == line) {
                    out.push(Mention {
                        id: id.to_string(),
                        line,
                    });
                }
                i += 2 + digits;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `[[标题]]` 维基链接(按记忆标题解析,失败由调用方写 warnings)。
pub fn wikilinks(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(start) = rest.find("[[") {
            let tail = &rest[start + 2..];
            let Some(end) = tail.find("]]") else { break };
            let title = tail[..end].trim();
            if !title.is_empty() {
                out.push((title.to_string(), index + 1));
            }
            rest = &tail[end + 2..];
        }
    }
    out
}

/// 第一处出现 `needle` 的行号(1 起)。
pub fn line_of(text: &str, needle: &str) -> Option<usize> {
    let at = text.find(needle)?;
    Some(text[..at].matches('\n').count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_boundaries() {
        let hits = ids_in(
            "见R-123 与 D-04;XR-123、R-12345、abcM-001\n第二行 M-009, U-15 R-1",
            "RDAMU",
        );
        let ids: Vec<(&str, usize)> = hits.iter().map(|m| (m.id.as_str(), m.line)).collect();
        assert_eq!(
            ids,
            vec![("R-123", 1), ("D-04", 1), ("M-009", 2), ("U-15", 2)]
        );
        assert!(ids_in("R-12 R-12", "R").len() == 1, "同行去重");
        assert!(ids_in("_R-123", "R").is_empty(), "下划线算词字符");
        assert_eq!(ids_in("(R-123)", "R").len(), 1);
    }

    #[test]
    fn wikilinks_and_line_lookup() {
        assert_eq!(
            wikilinks("a [[记忆蒸馏改用 primary]] b\n[[x]] [[未闭合"),
            vec![
                ("记忆蒸馏改用 primary".to_string(), 1),
                ("x".to_string(), 2)
            ]
        );
        assert_eq!(line_of("a\nb\nc R-1", "R-1"), Some(3));
    }
}
