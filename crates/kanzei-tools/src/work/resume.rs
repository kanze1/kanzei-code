//! 为恢复工作提供分支、HEAD 和未提交改动的有界快照。

use serde::Serialize;

use super::command_output;

/// 恢复现场快照。字段都是**事实**,不含建议——怎么用写在注入块的说明里。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResumeWorktree {
    /// 当前分支名;detached 时是 `HEAD`。
    pub branch: String,
    /// `<短 sha> <提交标题>`。
    pub head: String,
    /// 未提交改动,形如 `crates/x.rs +12/-3` 或 `docs/new.md (untracked)`。
    pub uncommitted: Vec<String>,
    /// 未提交文件总数(`uncommitted` 可能被截断)。
    pub uncommitted_files: usize,
}

/// 清单的展示预算；超出时保留总数，是否提交由实际任务决定。
const RESUME_WORKTREE_MAX_FILES: usize = 20;

/// 采集恢复现场。没有未提交改动就返回 None——干净树没有现场要交代。
///
/// 排除 `.kanzei/**`:托管文档每轮都在动,列进来会把真正的代码改动淹掉。
pub(super) fn collect_resume_worktree(cwd: &std::path::Path) -> Option<ResumeWorktree> {
    let numstat = String::from_utf8_lossy(&command_output(
        cwd,
        &[
            "diff",
            "--numstat",
            "HEAD",
            "--",
            ".",
            ":(exclude).kanzei/**",
        ],
    ))
    .to_string();
    let untracked = String::from_utf8_lossy(&command_output(
        cwd,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "--",
            ".",
            ":(exclude).kanzei/**",
        ],
    ))
    .to_string();

    let mut files: Vec<String> = Vec::new();
    for line in numstat.lines() {
        let mut fields = line.splitn(3, '\t');
        let added = fields.next().unwrap_or_default().trim();
        let deleted = fields.next().unwrap_or_default().trim();
        let path = fields.next().unwrap_or_default().trim();
        if path.is_empty() {
            continue;
        }
        files.push(format!("{path} +{added}/-{deleted}"));
    }
    for line in untracked.lines() {
        let path = line.trim();
        if !path.is_empty() {
            files.push(format!("{path} (untracked)"));
        }
    }
    if files.is_empty() {
        return None;
    }

    let uncommitted_files = files.len();
    files.truncate(RESUME_WORKTREE_MAX_FILES);
    let branch =
        String::from_utf8_lossy(&command_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]))
            .trim()
            .to_string();
    let head = String::from_utf8_lossy(&command_output(cwd, &["log", "-1", "--format=%h %s"]))
        .trim()
        .to_string();
    Some(ResumeWorktree {
        branch,
        head,
        uncommitted: files,
        uncommitted_files,
    })
}
