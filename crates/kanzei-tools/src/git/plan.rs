//! 提交前覆盖计划：在 stage/commit 之前把改动范围与测试证据缺口结构化暴露。
//!
//! 计划本身只读；它不替代 source_test_gate，而是让默认 finalize 路径在真正
//! stage/commit 前拿到同一套 affected crate 与精确测试命令。

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use super::{
    is_source_path, is_tracker_path, normalize_files, run_git, source_crates,
    source_endorsement_fingerprint_for_paths,
};
use crate::test_record::{last_passed, last_passed_for_fingerprint, TEST_RUNS_GOVERNANCE_PATHS};
use kanzei_harness::ToolOutput;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EvidenceSummary {
    pub command: String,
    pub coverage: String,
    pub passed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommitPlan {
    pub files: Vec<String>,
    pub safe_stage_set: Vec<String>,
    pub unsafe_files: Vec<String>,
    pub affected_crates: Vec<String>,
    pub required_evidence: Vec<String>,
    pub satisfied_evidence: Vec<EvidenceSummary>,
    pub missing_evidence: Vec<String>,
    pub governance_metadata: Vec<String>,
    pub test_command: Option<String>,
    pub ready: bool,
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
    let changed = changed_paths(cwd).await?;
    Ok(plan_for_files(project_root, cwd, files, changed))
}

fn plan_for_files(
    project_root: &Path,
    cwd: &Path,
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
    let required_evidence: Vec<String> = affected_crates
        .iter()
        .map(|crate_name| format!("cargo test -p {crate_name}"))
        .collect();
    let test_command = (!required_evidence.is_empty()).then(|| required_evidence.join(" && "));

    let mut satisfied_evidence = Vec::new();
    let mut missing_evidence = Vec::new();
    if !required_evidence.is_empty() {
        let fingerprint =
            source_endorsement_fingerprint_for_paths(cwd, &source_files).unwrap_or_default();
        let latest = if fingerprint.is_empty() {
            last_passed(project_root)
        } else {
            last_passed_for_fingerprint(project_root, &fingerprint)
                .or_else(|| last_passed(project_root))
        };
        match latest {
            Some((passed_at, coverage, command, _)) => {
                satisfied_evidence.push(EvidenceSummary {
                    command,
                    coverage: coverage.describe(),
                    passed_at: Some(passed_at),
                });
                for (crate_name, required) in affected_crates.iter().zip(required_evidence.iter()) {
                    if !coverage.covers(crate_name) {
                        missing_evidence.push(required.clone());
                    }
                }
            }
            None => missing_evidence.extend(required_evidence.iter().cloned()),
        }
    }

    let mut governance_metadata: Vec<String> = files
        .iter()
        .filter(|path| is_tracker_path(path))
        .cloned()
        .collect();
    if !required_evidence.is_empty() {
        governance_metadata.extend(
            TEST_RUNS_GOVERNANCE_PATHS
                .iter()
                .map(|path| (*path).to_string()),
        );
    }
    governance_metadata.sort();
    governance_metadata.dedup();

    let ready = unsafe_files.is_empty() && missing_evidence.is_empty();
    CommitPlan {
        files,
        safe_stage_set,
        unsafe_files,
        affected_crates,
        required_evidence,
        satisfied_evidence,
        missing_evidence,
        governance_metadata,
        test_command,
        ready,
    }
}

async fn changed_paths(cwd: &Path) -> Result<BTreeSet<String>, String> {
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
    .await?;
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
    use crate::test_record::TEST_RUNS_REL;
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
    async fn plan_exposes_affected_crate_missing_evidence_and_safe_set() {
        let root = temp_repo("missing");
        let src = root.join("crates/kanzei-app/src/lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(&src, "pub fn probe() {}\n").unwrap();
        std::fs::write(
            root.join(".kanzei/project/tests.md"),
            "# Test Runs\n\n## T-1786922727000 frontend [passed]\n- 命令: node scripts/ui-runtime-smoke.mjs\n- 收尾: 1786922727\n",
        )
        .unwrap();
        let plan = build_commit_plan(&root, &root, &["crates/kanzei-app/src/lib.rs".to_string()])
            .await
            .unwrap();
        assert_eq!(plan.affected_crates, vec!["kanzei-app"]);
        assert_eq!(plan.required_evidence, vec!["cargo test -p kanzei-app"]);
        assert_eq!(plan.missing_evidence, vec!["cargo test -p kanzei-app"]);
        assert_eq!(plan.safe_stage_set, vec!["crates/kanzei-app/src/lib.rs"]);
        assert!(plan
            .governance_metadata
            .contains(&TEST_RUNS_REL.to_string()));
        assert!(!plan.ready);
        let repeated =
            build_commit_plan(&root, &root, &["crates/kanzei-app/src/lib.rs".to_string()])
                .await
                .unwrap();
        assert_eq!(repeated.missing_evidence, plan.missing_evidence);
        assert_eq!(repeated.test_command, plan.test_command);
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn plan_marks_workspace_evidence_satisfied_for_each_crate() {
        let root = temp_repo("satisfied");
        let src = root.join("crates/kanzei-tools/src/lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(&src, "pub fn probe() {}\n").unwrap();
        let fingerprint = source_endorsement_fingerprint_for_paths(
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        )
        .unwrap();
        std::fs::write(
            root.join(".kanzei/project/tests.md"),
            format!(
                "# Test Runs\n\n## T-1786922727001 workspace [passed]\n- 命令: cargo test --workspace\n- 收尾: 1786922727\n- 源码指纹: {fingerprint}\n"
            ),
        )
        .unwrap();
        let plan = build_commit_plan(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        )
        .await
        .unwrap();
        assert_eq!(plan.missing_evidence, Vec::<String>::new());
        assert!(plan.ready);
        assert_eq!(plan.satisfied_evidence.len(), 1);
        std::fs::remove_dir_all(root).ok();
    }
}
