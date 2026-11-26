//! 提交前覆盖计划：在 stage/commit 之前把改动范围与建议测试命令结构化暴露。

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use super::{is_source_path, normalize_files, run_git, source_crates};
use kanzei_harness::ToolOutput;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommitPlan {
    pub files: Vec<String>,
    pub safe_stage_set: Vec<String>,
    pub unsafe_files: Vec<String>,
    pub affected_crates: Vec<String>,
    pub test_command: Option<String>,
    pub ready: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

impl CommitPlan {
    pub(crate) fn render(&self) -> ToolOutput {
        match serde_json::to_string_pretty(self) {
            Ok(text) => ToolOutput::ok(text),
            Err(error) => ToolOutput::error(format!("无法渲染 commit_plan: {error}")),
        }
    }

    pub(crate) fn blocker(&self, phase: &str) -> ToolOutput {
        match serde_json::to_string_pretty(self) {
            Ok(text) => ToolOutput::error(format!("[{phase}] commit_plan 未就绪:\n{text}")),
            Err(error) => ToolOutput::error(format!("[{phase}] commit_plan 渲染失败: {error}")),
        }
    }
}

pub(crate) async fn build_commit_plan(
    project_root: &Path,
    cwd: &Path,
    raw_files: &[String],
) -> Result<CommitPlan, String> {
    let files = normalize_files(cwd, raw_files, true)?;
    let mut diagnostics = Vec::new();
    let changed = changed_paths(cwd, &mut diagnostics).await?;
    let mut plan = plan_for_files(project_root, files, changed);
    plan.diagnostics = diagnostics;
    Ok(plan)
}

fn plan_for_files(
    project_root: &Path,
    files: Vec<String>,
    changed: BTreeSet<String>,
) -> CommitPlan {
    let safe_stage_set: Vec<String> = files
        .iter()
        .filter(|path| changed.contains(&path.replace('\\', "/")))
        .cloned()
        .collect();
    let unsafe_files: Vec<String> = files
        .iter()
        .filter(|path| !changed.contains(&path.replace('\\', "/")))
        .cloned()
        .collect();

    let source_files: Vec<String> = files
        .iter()
        .filter(|path| is_source_path(path))
        .cloned()
        .collect();
    let affected_crates: Vec<String> = source_crates(&source_files).into_iter().collect();
    let test_commands: Vec<String> = affected_crates
        .iter()
        .filter(|_| !kanzei_harness::is_general_conversation_root(project_root))
        .map(|crate_name| format!("cargo test -p {crate_name}"))
        .collect();
    let test_command = (!test_commands.is_empty()).then(|| test_commands.join(" && "));

    let ready = unsafe_files.is_empty();
    CommitPlan {
        files,
        safe_stage_set,
        unsafe_files,
        affected_crates,
        test_command,
        ready,
        diagnostics: Vec::new(),
    }
}

async fn changed_paths(
    cwd: &Path,
    diagnostics: &mut Vec<String>,
) -> Result<BTreeSet<String>, String> {
    let text = run_git(
        cwd,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--no-renames",
            "-z",
        ],
    )
    .await?
    .take_stdout(diagnostics);
    Ok(text
        .split('\0')
        .filter_map(|entry| {
            let bytes = entry.as_bytes();
            (bytes.len() >= 4 && bytes[2] == b' ').then(|| entry[3..].replace('\\', "/"))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn temp_repo(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-commit-plan-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "user.name", "test"]);
        root
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    }

    #[tokio::test]
    async fn plan_exposes_affected_crate_and_safe_set() {
        let root = temp_repo("basic");
        let src = root.join("crates/kanzei-app/src/lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "pub fn probe() {}\n").unwrap();
        let plan = build_commit_plan(&root, &root, &["crates/kanzei-app/src/lib.rs".to_string()])
            .await
            .unwrap();
        assert_eq!(plan.affected_crates, vec!["kanzei-app"]);
        assert_eq!(
            plan.test_command.as_deref(),
            Some("cargo test -p kanzei-app")
        );
        assert_eq!(plan.safe_stage_set, vec!["crates/kanzei-app/src/lib.rs"]);
        assert!(plan.ready);
        std::fs::remove_dir_all(root).ok();
    }
}
