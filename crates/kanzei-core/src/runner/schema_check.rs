//! R-250:子代理结构化返回的校验器。
//!
//! **为什么是自己写的而不是引 jsonschema**:这里校验的是子代理的返回体,不是任意
//! 用户 schema。实际用到的关键字就 `type` / `required` / `properties` / `items` /
//! `enum` 五个;完整 JSON Schema 的 `$ref`、`allOf`、`patternProperties` 一类在这个
//! 场景不会出现。换来的是**精确到字段的诊断**——不合规时告诉模型「哪个字段、期望
//! 什么、实际是什么」,它才改得动;通用校验器给的错误路径往往还得再解释一遍。
//!
//! 未识别的关键字一律**忽略**(不报错):schema 是模型写的,多写了 `description`、
//! `minimum` 之类不该导致校验失败。宁可漏判,不可错判——错判会让本来正确的返回被
//! 打回重试,浪费一整轮子代理。

use serde_json::Value;

/// 校验 `value` 是否符合 `schema`。合规返回 `None`,否则返回第一处问题的说明。
///
/// `path` 是当前位置的人类可读路径(顶层传 `"$"`),诊断里原样带出。
pub(crate) fn validate(value: &Value, schema: &Value, path: &str) -> Option<String> {
    let Some(schema) = schema.as_object() else {
        // schema 本身不是对象:当作「无约束」,不拦。
        return None;
    };

    if let Some(expected) = schema.get("type") {
        if let Some(problem) = check_type(value, expected, path) {
            return Some(problem);
        }
    }

    if let Some(allowed) = schema.get("enum").and_then(|v| v.as_array()) {
        if !allowed.iter().any(|candidate| candidate == value) {
            return Some(format!(
                "{path}: value {} is not one of the allowed values {}",
                compact(value),
                compact(&Value::Array(allowed.clone()))
            ));
        }
    }

    if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
        // required 只对对象有意义;value 不是对象时上面的 type 检查已经拦过了,
        // 没写 type 的话这里静默跳过,不无中生有。
        if let Some(object) = value.as_object() {
            for key in required.iter().filter_map(|k| k.as_str()) {
                if !object.contains_key(key) {
                    return Some(format!("{path}: missing required field `{key}`"));
                }
            }
        }
    }

    if let (Some(properties), Some(object)) = (
        schema.get("properties").and_then(|v| v.as_object()),
        value.as_object(),
    ) {
        for (key, sub_schema) in properties {
            // 缺失的可选字段不校验——是否必须由 required 单独判定。
            if let Some(sub_value) = object.get(key) {
                if let Some(problem) = validate(sub_value, sub_schema, &format!("{path}.{key}")) {
                    return Some(problem);
                }
            }
        }
    }

    if let (Some(item_schema), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, item) in array.iter().enumerate() {
            if let Some(problem) = validate(item, item_schema, &format!("{path}[{index}]")) {
                return Some(problem);
            }
        }
    }

    None
}

/// `type` 允许是字符串或字符串数组(JSON Schema 两种写法都常见)。
fn check_type(value: &Value, expected: &Value, path: &str) -> Option<String> {
    let names: Vec<&str> = match expected {
        Value::String(name) => vec![name.as_str()],
        Value::Array(list) => list.iter().filter_map(|v| v.as_str()).collect(),
        // type 写成别的形状:视为没写,不拦。
        _ => return None,
    };
    if names.is_empty() || names.iter().any(|name| matches_type(value, name)) {
        return None;
    }
    Some(format!(
        "{path}: expected type {}, got {} ({})",
        names.join(" or "),
        actual_type(value),
        compact(value)
    ))
}

fn matches_type(value: &Value, name: &str) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        // integer 是 number 的子集;i64/u64 都算,浮点整数值(3.0)也放行——
        // 模型经常把整数写成 3.0,为此打回重试不划算。
        "integer" => value.as_i64().is_some() || value.as_f64().is_some_and(|f| f.fract() == 0.0),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        // 不认识的 type 名:不拦。
        _ => true,
    }
}

fn actual_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// 诊断里回显实际值,但不能把整个返回体糊进去——截断到能看清是什么就够。
fn compact(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(120) {
        Some((idx, _)) => format!("{}…", &text[..idx]),
        None => text,
    }
}

/// 从子代理的自由文本里取出 JSON 体。
///
/// 模型即使被要求「只返回 JSON」也常裹一层 ```json 围栏或加一句前言,为此判失败
/// 再跑一轮子代理太贵。这里做**有限的**宽容:剥围栏、取第一个完整的顶层
/// `{...}` 或 `[...]`。仍然解析不出来才算失败。
pub(crate) fn extract_json(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    // ```json ... ``` / ``` ... ```
    if let Some(rest) = trimmed.strip_prefix("```") {
        let body = rest
            .split_once('\n')
            .map(|(_, body)| body)
            .unwrap_or(rest)
            .trim_end()
            .trim_end_matches("```")
            .trim();
        if let Ok(value) = serde_json::from_str::<Value>(body) {
            return Some(value);
        }
    }
    // 前后有散文:按括号配对切出第一个完整的顶层结构。
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let Some(start) = trimmed.find(open) {
            let mut depth = 0usize;
            let mut in_string = false;
            let mut escaped = false;
            for (offset, ch) in trimmed[start..].char_indices() {
                if in_string {
                    match ch {
                        _ if escaped => escaped = false,
                        '\\' => escaped = true,
                        '"' => in_string = false,
                        _ => {}
                    }
                    continue;
                }
                match ch {
                    '"' => in_string = true,
                    c if c == open => depth += 1,
                    c if c == close => {
                        depth -= 1;
                        if depth == 0 {
                            let slice = &trimmed[start..start + offset + ch.len_utf8()];
                            if let Ok(value) = serde_json::from_str::<Value>(slice) {
                                return Some(value);
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "required": ["findings"],
            "properties": {
                "findings": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["file", "line"],
                        "properties": {
                            "file": {"type": "string"},
                            "line": {"type": "integer"},
                            "severity": {"type": "string", "enum": ["low", "high"]}
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn accepts_conforming_value() {
        let value = json!({"findings": [{"file": "a.rs", "line": 12, "severity": "high"}]});
        assert_eq!(validate(&value, &schema(), "$"), None);
    }

    #[test]
    fn diagnoses_missing_required_with_field_name() {
        // 验收④:诊断必须指出是哪个字段,不能笼统报错。
        let value = json!({"findings": [{"file": "a.rs"}]});
        let problem = validate(&value, &schema(), "$").expect("应判不合规");
        assert!(problem.contains("line"), "{problem}");
        assert!(
            problem.contains("$.findings[0]"),
            "路径要定位到元素: {problem}"
        );
    }

    #[test]
    fn diagnoses_wrong_type_with_path_and_actual() {
        let value = json!({"findings": [{"file": 1, "line": 2}]});
        let problem = validate(&value, &schema(), "$").expect("应判不合规");
        assert!(problem.contains("$.findings[0].file"), "{problem}");
        assert!(problem.contains("string"), "{problem}");
        assert!(problem.contains("number"), "要说清实际类型: {problem}");
    }

    #[test]
    fn diagnoses_enum_violation() {
        let value = json!({"findings": [{"file": "a", "line": 1, "severity": "medium"}]});
        let problem = validate(&value, &schema(), "$").expect("应判不合规");
        assert!(problem.contains("severity"), "{problem}");
    }

    #[test]
    fn top_level_type_mismatch_is_caught() {
        let problem = validate(&json!([1, 2]), &schema(), "$").expect("应判不合规");
        assert!(problem.contains("expected type object"), "{problem}");
    }

    #[test]
    fn unknown_keywords_and_extra_fields_are_tolerated() {
        // 宁可漏判不可错判:多余字段与不认识的关键字都不该打回重试。
        let s = json!({
            "type": "object",
            "properties": {"a": {"type": "string", "minLength": 3, "description": "x"}},
            "additionalProperties": false
        });
        let value = json!({"a": "hi", "b": 1});
        assert_eq!(validate(&value, &s, "$"), None);
    }

    #[test]
    fn integer_accepts_whole_floats() {
        // 模型常把整数写成 3.0,为此重试一轮不划算。
        let s = json!({"type": "object", "properties": {"n": {"type": "integer"}}});
        assert_eq!(validate(&json!({"n": 3.0}), &s, "$"), None);
        assert!(validate(&json!({"n": 3.5}), &s, "$").is_some());
    }

    #[test]
    fn extract_json_handles_bare_fence_and_prose() {
        let want = json!({"ok": true});
        assert_eq!(extract_json(r#"{"ok":true}"#), Some(want.clone()));
        assert_eq!(
            extract_json("```json\n{\"ok\":true}\n```"),
            Some(want.clone())
        );
        assert_eq!(extract_json("```\n{\"ok\":true}\n```"), Some(want.clone()));
        assert_eq!(
            extract_json("Here is the result:\n{\"ok\":true}\nHope that helps."),
            Some(want)
        );
        assert_eq!(extract_json("no json at all"), None);
    }

    #[test]
    fn extract_json_ignores_braces_inside_strings() {
        // 字符串里的 } 不能提前收口,否则切出半截 JSON 又解析失败。
        let text = r#"prefix {"msg": "a } b", "n": 1} suffix"#;
        assert_eq!(extract_json(text), Some(json!({"msg": "a } b", "n": 1})));
    }

    #[test]
    fn extract_json_supports_top_level_array() {
        assert_eq!(extract_json("result: [1,2,3]"), Some(json!([1, 2, 3])));
    }
}
