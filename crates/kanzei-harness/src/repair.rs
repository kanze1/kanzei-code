//! 宽容 JSON 解析(设计红线 1):模型幻觉常见的坏格式先修再收。
//! 覆盖:尾逗号、单引号字符串、裸键、Python 字面量(True/False/None)、```json 围栏。
//! 修不动才返回 None,由上层把 schema+错误点回喂模型。

use serde_json::Value;

pub fn tolerant_parse(raw: &str) -> Option<Value> {
    let raw = strip_fences(raw.trim());
    if let Ok(v) = serde_json::from_str(raw) {
        return Some(v);
    }
    let repaired = repair(raw);
    serde_json::from_str(&repaired).ok()
}

fn strip_fences(raw: &str) -> &str {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("```json").or_else(|| raw.strip_prefix("```")) {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }
    raw
}

/// 单遍字符扫描修复。字符串内内容原样保留(含转义)。
fn repair(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len() + 16);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            // 双引号字符串:原样拷贝
            '"' => {
                out.push('"');
                i += 1;
                while i < chars.len() {
                    let c = chars[i];
                    out.push(c);
                    i += 1;
                    if c == '\\' {
                        if i < chars.len() {
                            out.push(chars[i]);
                            i += 1;
                        }
                    } else if c == '"' {
                        break;
                    }
                }
            }
            // 单引号字符串 → 双引号(内部的双引号转义、\' 还原为 ')
            '\'' => {
                out.push('"');
                i += 1;
                while i < chars.len() {
                    let c = chars[i];
                    if c == '\\' && i + 1 < chars.len() && chars[i + 1] == '\'' {
                        out.push('\'');
                        i += 2;
                    } else if c == '\'' {
                        i += 1;
                        break;
                    } else if c == '"' {
                        out.push_str("\\\"");
                        i += 1;
                    } else {
                        out.push(c);
                        i += 1;
                    }
                }
                out.push('"');
            }
            // 尾逗号:逗号后面(跳过空白)是 ] 或 } 就丢弃
            ',' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && (chars[j] == ']' || chars[j] == '}') {
                    i += 1; // 丢弃逗号
                } else {
                    out.push(',');
                    i += 1;
                }
            }
            // 裸标识符:可能是裸键、True/False/None、或合法的 true/false/null
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                let is_key = j < chars.len() && chars[j] == ':';
                if is_key {
                    out.push('"');
                    out.push_str(&word);
                    out.push('"');
                } else {
                    match word.as_str() {
                        "True" => out.push_str("true"),
                        "False" => out.push_str("false"),
                        "None" | "Null" => out.push_str("null"),
                        _ => out.push_str(&word),
                    }
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn passes_valid_json_through() {
        assert_eq!(tolerant_parse(r#"{"a": 1}"#), Some(json!({"a": 1})));
    }

    #[test]
    fn fixes_trailing_commas() {
        assert_eq!(
            tolerant_parse(r#"{"a": [1, 2,], "b": 3,}"#),
            Some(json!({"a": [1, 2], "b": 3}))
        );
    }

    #[test]
    fn fixes_single_quotes_and_bare_keys() {
        assert_eq!(
            tolerant_parse(r#"{path: 'a.txt', mode: "fast"}"#),
            Some(json!({"path": "a.txt", "mode": "fast"}))
        );
        // 单引号串里的双引号要转义
        assert_eq!(
            tolerant_parse(r#"{'cmd': 'echo "hi"'}"#),
            Some(json!({"cmd": "echo \"hi\""}))
        );
    }

    #[test]
    fn fixes_python_literals_and_fences() {
        assert_eq!(
            tolerant_parse("```json\n{\"ok\": True, \"x\": None}\n```"),
            Some(json!({"ok": true, "x": null}))
        );
    }

    #[test]
    fn keeps_string_contents_untouched() {
        assert_eq!(
            tolerant_parse(r#"{"note": "keys: stay, True None ',' "}"#),
            Some(json!({"note": "keys: stay, True None ',' "}))
        );
    }

    #[test]
    fn unfixable_returns_none() {
        assert_eq!(tolerant_parse(r#"{"a": oops"#), None);
    }
}
