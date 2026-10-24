//! Model-facing collaboration is a projection of runtime participants and Git trees.
//! File inventories belong to trees; they must never be expanded per participant.

use std::collections::HashMap;
use std::path::{Component, Path};

use serde::Deserialize;
use serde_json::{json, Value};

use super::CollaborationLine;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StatusQuery {
    pub(super) path: Option<String>,
}

impl StatusQuery {
    pub(super) fn parse(input: Value) -> Result<Self, String> {
        let mut query: Self = serde_json::from_value(input).map_err(|error| error.to_string())?;
        if let Some(path) = &mut query.path {
            let normalized = path.replace('\\', "/");
            let components = Path::new(&normalized).components().collect::<Vec<_>>();
            if components.is_empty()
                || components
                    .iter()
                    .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
            {
                return Err(
                    "path must be a project-relative file or directory without '..'".into(),
                );
            }
            *path = components
                .iter()
                .filter_map(|part| match part {
                    Component::Normal(name) => Some(name.to_string_lossy()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/");
            if path.is_empty() {
                return Err("path must name a project-relative file or directory".into());
            }
        }
        Ok(query)
    }
}

fn path_overlaps(file: &str, query: &str) -> bool {
    let directory_boundary = file.ends_with('/');
    let file = Path::new(file);
    let query = Path::new(query);
    file.starts_with(query) || (directory_boundary && query.starts_with(file))
}

pub(super) fn model_snapshot(
    origin: &str,
    lines: &[CollaborationLine],
    path: Option<&str>,
) -> Value {
    let mut tree_ids = HashMap::new();
    let mut worktrees = Vec::new();
    let mut participants = Vec::new();
    for line in lines {
        let root = line.worktree_path.as_deref().unwrap_or(origin);
        let next = worktrees.len();
        let tree_id = *tree_ids.entry(root).or_insert(next);
        if tree_id == next {
            let path_changed = path.and_then(|path| {
                line.changed_files_error.is_none().then(|| {
                    line.changed_files
                        .iter()
                        .any(|file| path_overlaps(file, path))
                })
            });
            worktrees.push(json!({
                "id": tree_id, "path": root, "branch": line.branch,
                "claim": line.claim, "claim_error": line.claim_error,
                "changed_file_count": line.changed_files.len(),
                "changed_files_error": line.changed_files_error,
                "path_changed": path_changed,
                "writer_run_id": line.writer_run_id, "waiting_writers": line.waiting_writers,
            }));
        }
        participants.push(json!({
            "process_id": line.process_id, "label": line.label, "worktree": tree_id,
            "status": line.status, "status_error": line.status_error,
            "phase": line.phase, "tool": line.current_tool,
        }));
    }
    json!({"lines": participants, "worktrees": worktrees, "query_path": path})
}

pub(super) fn render_lines(title: &str, snapshot: &Value) -> String {
    format!(
        "## {title}\n{snapshot}\nGit changes and tracker claims belong to worktrees, not individual conversations. Counts include untracked files; they do not establish authorship or a conflict. Before editing or staging a shared path, call `collaboration_status` with its project-relative `path` to refresh the relevant trees, then inspect their Git diffs as needed."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_accepts_relative_files_and_directories_and_rejects_invalid_inputs() {
        for (input, expected) in [("src\\main.rs", "src/main.rs"), ("./src/", "src")] {
            assert_eq!(
                StatusQuery::parse(json!({"path": input}))
                    .unwrap()
                    .path
                    .as_deref(),
                Some(expected)
            );
        }
        assert!(StatusQuery::parse(json!({})).unwrap().path.is_none());
        for input in [
            json!({"path": ""}),
            json!({"path": "."}),
            json!({"path": "../src"}),
            json!({"path": "/src"}),
            json!({"path": 1}),
            json!({"paths": ["src"]}),
        ] {
            assert!(StatusQuery::parse(input.clone()).is_err(), "{input}");
        }
    }

    #[test]
    fn query_scope_uses_path_components_including_untracked_directory_boundaries() {
        assert!(path_overlaps("src/main.rs", "src"));
        assert!(path_overlaps("src/main.rs", "src/main.rs"));
        assert!(path_overlaps("nested-repo/", "nested-repo/src/main.rs"));
        assert!(!path_overlaps("src/main.rs", "src/main"));
        assert!(!path_overlaps("src/main.rs", "src/main.rs/child"));
        assert!(!path_overlaps("src2/main.rs", "src"));
    }
}
