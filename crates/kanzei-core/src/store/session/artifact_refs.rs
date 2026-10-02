//! Managed tool-result references in structured events and archived message text.

use std::collections::BTreeMap;
use std::path::Path;

pub(super) fn collect_artifact_references(
    value: &serde_json::Value,
    references: &mut BTreeMap<String, u64>,
) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(artifact_id) = object.get("artifact_id").and_then(|value| value.as_str()) {
                *references.entry(format!("id:{artifact_id}")).or_default() += 1;
            }
            if let Some(relative_path) = object
                .get("relative_path")
                .and_then(|value| value.as_str())
                .and_then(normalize_artifact_relative_path)
            {
                *references
                    .entry(format!("path:{relative_path}"))
                    .or_default() += 1;
            }
            for child in object.values() {
                collect_artifact_references(child, references);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                collect_artifact_references(child, references);
            }
        }
        serde_json::Value::String(text) => {
            // Compaction/read pointers may live in message text, including absolute paths.
            // Only the normalized managed suffix is a deletion-protecting reference.
            let normalized = text.replace('\\', "/");
            for (start, _) in normalized.match_indices(".kanzei/artifacts/tool-results/") {
                let suffix = &normalized[start..];
                let end = suffix
                    .find(|c: char| {
                        c.is_whitespace()
                            || matches!(
                                c,
                                '`' | '"' | '\'' | '<' | '>' | '）' | ')' | '，' | '；' | '[' | ']'
                            )
                    })
                    .unwrap_or(suffix.len());
                if let Some(path) = normalize_artifact_relative_path(&suffix[..end]) {
                    *references.entry(format!("path:{path}")).or_default() += 1;
                }
            }
        }
        _ => {}
    }
}

pub(super) fn normalize_artifact_relative_path(value: &str) -> Option<String> {
    use std::path::Component;

    let mut components = Vec::new();
    for component in Path::new(value).components() {
        match component {
            Component::Normal(component) => {
                components.push(component.to_string_lossy().into_owned())
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return None,
        }
    }
    let normalized = components.join("/");
    if normalized
        .strip_prefix(".kanzei/artifacts/tool-results/")
        .is_some_and(|rest| !rest.is_empty())
    {
        Some(normalized)
    } else {
        None
    }
}
