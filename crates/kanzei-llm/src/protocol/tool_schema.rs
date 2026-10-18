//! Equivalent tool-schema forms accepted by Chat Completions implementations.
use serde_json::{json, Value};

pub(super) fn chat_parameters(input: &Value) -> Value {
    let mut schema = input.clone();
    let Some(object) = schema.as_object_mut() else {
        return schema;
    };
    // An omitted properties map is valid JSON Schema. Ollama serializes it as
    // null, which its schema parser rejects even for a parameterless tool.
    if object.get("type").and_then(Value::as_str) == Some("object") {
        object.entry("properties").or_insert_with(|| json!({}));
    }
    let Some(legacy) = object.remove("definitions") else {
        return schema;
    };
    // Ollama preserves $defs but discards draft-07's definitions at the tool
    // parameter boundary. Relocate the collection and its absolute local refs
    // together; preserve shared and recursive definitions without inlining.
    let mappings: &[(&str, &str)] = if let Some(modern) = object.remove("$defs") {
        // Both vocabularies may coexist and contain the same definition names.
        object.insert(
            "$defs".into(),
            json!({"legacy": {"$defs": legacy}, "modern": {"$defs": modern}}),
        );
        &[
            ("#/definitions", "#/$defs/legacy/$defs"),
            ("#/$defs", "#/$defs/modern/$defs"),
        ]
    } else {
        object.insert("$defs".into(), legacy);
        &[("#/definitions", "#/$defs")]
    };
    relocate_refs(&mut schema, mappings);
    schema
}

fn relocate_refs(schema: &mut Value, mappings: &[(&str, &str)]) {
    if let Some(array) = schema.as_array_mut() {
        for child in array {
            relocate_refs(child, mappings);
        }
        return;
    }
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    if let Some(Value::String(reference)) = object.get_mut("$ref") {
        for (from, to) in mappings {
            if let Some(suffix) = reference.strip_prefix(from) {
                if suffix.is_empty() || suffix.starts_with('/') {
                    *reference = format!("{to}{suffix}");
                    break;
                }
            }
        }
    }
    // Visit schema locations, never instance data in enum/const/default/examples
    // or property names that happen to equal a schema keyword.
    for keyword in [
        "properties",
        "patternProperties",
        "definitions",
        "$defs",
        "dependencies",
        "dependentSchemas",
    ] {
        if let Some(children) = object.get_mut(keyword).and_then(Value::as_object_mut) {
            for child in children.values_mut() {
                relocate_refs(child, mappings);
            }
        }
    }
    for keyword in [
        "items",
        "prefixItems",
        "additionalItems",
        "additionalProperties",
        "unevaluatedItems",
        "unevaluatedProperties",
        "contains",
        "propertyNames",
        "allOf",
        "anyOf",
        "oneOf",
        "not",
        "if",
        "then",
        "else",
        "contentSchema",
    ] {
        if let Some(child) = object.get_mut(keyword) {
            relocate_refs(child, mappings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nullable_array_keeps_its_query_constraints_and_nested_enum() {
        let input = json!({
            "type":"object", "additionalProperties":false,
            "properties": {
                "queries":{"type":["array","null"],"minItems":1,"maxItems":4,
                    "items":{"$ref":"#/definitions/SearchQuery"}},
                "default":{"$ref":"#/definitions/SearchQuery"},
                "definitions":{"type":"string"}
            },
            "definitions": {
                "SearchQuery":{"type":"object","required":["q"],"properties":{
                    "q":{"type":"string","minLength":1,"maxLength":500},
                    "recency":{"anyOf":[{"$ref":"#/definitions/SearchRecency"},{"type":"null"}]},
                    "locale":{"type":["string","null"],"enum":["cn-zh","us-en",null]}
                }},
                "SearchRecency":{"type":"string","enum":["day","week","month","year"]}
            },
            "anyOf":[{"required":["queries"]},{"required":["query"]}],
            "examples":[{"$ref":"#/definitions/literal-instance-value"}]
        });
        let sent = chat_parameters(&input);
        assert!(sent.get("definitions").is_none());
        let query_ref = sent["properties"]["queries"]["items"]["$ref"]
            .as_str()
            .unwrap();
        let query = sent.pointer(query_ref.strip_prefix('#').unwrap()).unwrap();
        assert_eq!(query["required"], json!(["q"]));
        assert_eq!(
            query["properties"]["q"],
            input["definitions"]["SearchQuery"]["properties"]["q"]
        );
        assert_eq!(
            query["properties"]["locale"]["enum"],
            json!(["cn-zh", "us-en", null])
        );
        let recency_ref = query["properties"]["recency"]["anyOf"][0]["$ref"]
            .as_str()
            .unwrap();
        assert_eq!(
            sent.pointer(recency_ref.strip_prefix('#').unwrap())
                .unwrap(),
            &input["definitions"]["SearchRecency"]
        );
        assert_eq!(sent["properties"]["default"]["$ref"], query_ref);
        for key in ["examples", "anyOf", "additionalProperties"] {
            assert_eq!(sent[key], input[key]);
        }
        for key in ["type", "minItems", "maxItems"] {
            assert_eq!(
                sent["properties"]["queries"][key],
                input["properties"]["queries"][key]
            );
        }
        assert!(
            input.get("$defs").is_none(),
            "the shared tool schema must stay unchanged"
        );
    }

    #[test]
    fn both_definition_namespaces_keep_distinct_targets_and_recursion() {
        let input = json!({"type":"object", "properties":{
            "old":{"$ref":"#/definitions/Item"}, "new":{"$ref":"#/$defs/Item"},
            "escaped":{"$ref":"#/definitions/a~1b~0c"},
            "external":{"$ref":"https://example.test/schema#/definitions/Item"}
        }, "definitions":{
            "Item":{"type":"object","properties":{"next":{"$ref":"#/definitions/Item"}}},
            "a/b~c":{"type":"integer"}
        }, "$defs":{"Item":{"type":"string"}}});
        let sent = chat_parameters(&input);
        let target = |reference: &Value| {
            sent.pointer(reference.as_str().unwrap().strip_prefix('#').unwrap())
                .unwrap()
        };
        let old = target(&sent["properties"]["old"]["$ref"]);
        assert_eq!(old["type"], "object");
        assert_eq!(target(&old["properties"]["next"]["$ref"]), old);
        assert_eq!(target(&sent["properties"]["new"]["$ref"])["type"], "string");
        assert_eq!(
            target(&sent["properties"]["escaped"]["$ref"])["type"],
            "integer"
        );
        assert_eq!(
            sent["properties"]["external"],
            input["properties"]["external"]
        );
    }

    #[test]
    fn empty_object_and_modern_schemas_preserve_their_meaning() {
        let empty = chat_parameters(&json!({"type":"object"}));
        assert_eq!(empty, json!({"type":"object","properties":{}}));
        let modern = json!({"type":"object","properties":{"value":{"$ref":"#/$defs/Item"}},"$defs":{"Item":{"type":"string"}}});
        assert_eq!(chat_parameters(&modern), modern);
    }
}
