//! Model-facing collaboration is a projection of runtime participants and Git trees.
//! File inventories belong to trees; they must never be expanded per participant.

use std::collections::HashMap;
use std::path::{Component, Path};

use serde::Deserialize;
use serde_json::{json, Value};

use super::{CollaborationConversation, CollaborationWorktree};

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

fn changes_snapshot(files: &[String], error: Option<&str>, path: Option<&str>) -> Value {
    json!({
        "file_count": error.is_none().then_some(files.len()),
        "path_changed": path.and_then(|path| error.is_none().then(|| files.iter().any(|file| path_overlaps(file, path)))),
        "error": error,
    })
}

pub(super) fn worktree_snapshot(
    id: usize,
    tree: &CollaborationWorktree,
    path: Option<&str>,
) -> Value {
    json!({
        "id": id, "path": tree.path, "branch": tree.branch, "branch_error": tree.branch_error,
        "registered_work": {"source_field": "取得线", "value": tree.tracker_line,
            "entry_ids": tree.tracker_entries, "error": tree.tracker_error},
        "uncommitted": changes_snapshot(&tree.uncommitted_files, tree.uncommitted_error.as_deref(), path),
        "unmerged": changes_snapshot(&tree.unmerged_files, tree.unmerged_error.as_deref(), path),
        "writer_run_id": tree.writer_run_id, "waiting_writers": tree.waiting_writers,
    })
}

pub(super) fn model_snapshot(
    origin: &str,
    current_process_id: &str,
    conversations: &[CollaborationConversation],
    path: Option<&str>,
) -> Value {
    let mut tree_ids = HashMap::new();
    let mut worktrees = Vec::new();
    let mut participants = Vec::new();
    let mut current = None;
    for conversation in conversations {
        let root = conversation.worktree_path.as_deref().unwrap_or(origin);
        let next = worktrees.len();
        let tree_id = *tree_ids.entry(root).or_insert(next);
        if tree_id == next {
            let mut tree = conversation.tree.clone();
            tree.path = root.into();
            worktrees.push(worktree_snapshot(tree_id, &tree, path));
        }
        let identity = json!({"process_id": conversation.process_id, "session_id": conversation.session_id,
            "run_id": conversation.run_id, "worktree": tree_id});
        if conversation.process_id == current_process_id {
            current = Some(identity);
        }
        participants.push(json!({
            "process_id": conversation.process_id, "session_id": conversation.session_id,
            "run_id": conversation.run_id, "label": conversation.label, "worktree": tree_id,
            "status": conversation.status, "status_error": conversation.status_error,
            "phase": conversation.phase, "tool": conversation.current_tool,
            "run_age_ms": conversation.run_age_ms, "event_idle_ms": conversation.event_idle_ms,
        }));
    }
    json!({"project_root": origin, "current": current, "conversations": participants, "worktrees": worktrees, "query_path": path})
}

pub(super) fn render_snapshot(snapshot: &Value) -> String {
    format!(
        "## 当前项目协作状态\n{snapshot}\n对话记录执行状态，工作树承载代码与 Git 变化，项目共享资料归主根维护。结合当前任务、目标工作树和实际差异判断协作影响，保留无关改动。需要确认同树变更或跨树整合范围时，按相关路径查询最新协作状态与差异。登记关系来自 tracker 的取得线字段，不证明对话所有权；活动时间不证明进展或卡住。协作快照提供状态参考，不自行扩展任务或授权。"
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
