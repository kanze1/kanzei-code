//! File tree snapshot, preview, git status and single-file git diff for the context drawer.

use std::path::{Path, PathBuf};

use serde_json::json;

use kanzei_tools::files::{aggregate_dirs, scan_incremental, FileEntry};

const MAX_DIFF_BYTES: usize = 512 * 1024;

pub(crate) fn resolve_root(project_dir: &str) -> PathBuf {
    crate::normalized_project_root(Path::new(project_dir))
}

pub(crate) fn resolve_target_root(
    project_dir: &str,
    worktree_path: Option<&str>,
) -> Result<PathBuf, String> {
    let project_root = resolve_root(project_dir);
    match worktree_path.filter(|path| !path.trim().is_empty()) {
        Some(path) => kanzei_tools::worktree::validate_worktree_path(&project_root, path),
        None => Ok(project_root),
    }
}

fn parse_porcelain_z(raw: &str) -> (usize, Vec<String>) {
    let mut parts = raw.split('\0');
    let mut changes = 0usize;
    let mut untracked = Vec::new();
    while let Some(entry) = parts.next() {
        if entry.is_empty() {
            continue;
        }
        changes += 1;
        let bytes = entry.as_bytes();
        if bytes.len() >= 2 && (matches!(bytes[0], b'R' | b'C') || matches!(bytes[1], b'R' | b'C'))
        {
            // `-z` 模式下重命名/复制条目后紧跟原始路径段，跳过以免重复计入。
            let _ = parts.next();
        }
        if let Some(path) = entry.strip_prefix("?? ") {
            if !path.is_empty() {
                untracked.push(path.to_string());
            }
        }
    }
    (changes, untracked)
}

fn parse_numstat_z(raw: &str) -> (u64, u64, Vec<serde_json::Value>) {
    let mut parts = raw.split('\0');
    let mut files = Vec::new();
    let mut additions = 0u64;
    let mut deletions = 0u64;
    while let Some(entry) = parts.next() {
        if entry.is_empty() {
            continue;
        }
        let mut fields = entry.splitn(3, '\t');
        let (Some(add_s), Some(del_s), Some(path_field)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let path = if path_field.is_empty() {
            // `-z` 重命名/复制格式：<add>\t<del>\t\0<old>\0<new>\0
            let _old = parts.next();
            match parts.next() {
                Some(new_path) if !new_path.is_empty() => new_path,
                _ => continue,
            }
        } else {
            path_field
        };
        let binary = add_s == "-" || del_s == "-";
        let add: u64 = add_s.parse().unwrap_or(0);
        let del: u64 = del_s.parse().unwrap_or(0);
        additions += add;
        deletions += del;
        files.push(json!({
            "path": path,
            "additions": add,
            "deletions": del,
            "binary": binary,
        }));
    }
    (additions, deletions, files)
}

/// git 概览:分支 + 未提交改动数(状态栏与对话伴生上下文抽屉共用)。
#[tauri::command]
pub async fn git_status(
    project_dir: String,
    worktree_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let root = resolve_target_root(&project_dir, worktree_path.as_deref())?;
    tokio::task::spawn_blocking(move || {
        match kanzei_tools::project_state::git_state_of(&root) {
            kanzei_tools::project_state::GitState::Repo { .. } => {}
            kanzei_tools::project_state::GitState::None => {
                return json!({
                    "repo": "none", "branch": null, "changes": 0, "last": null,
                    "additions": 0, "deletions": 0, "files": [],
                });
            }
            kanzei_tools::project_state::GitState::Parent { toplevel } => {
                return json!({
                    "repo": "parent", "toplevel": toplevel, "branch": null, "changes": 0,
                    "last": null, "additions": 0, "deletions": 0, "files": [],
                });
            }
        }
        let run_raw = |args: &[&str]| -> Option<String> {
            let out = crate::state::hidden_command("git")
                .args(args)
                .current_dir(&root)
                .output()
                .ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        };
        let run = |args: &[&str]| -> Option<String> { run_raw(args).map(|s| s.trim().to_string()) };
        let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"]);
        let porcelain_z = run_raw(&["status", "--porcelain", "-z"]).unwrap_or_default();
        let (changes, untracked) = parse_porcelain_z(&porcelain_z);
        let last = run(&["log", "-1", "--format=%h %s"]);
        let numstat_z = run_raw(&["diff", "--numstat", "-z", "HEAD"]).unwrap_or_default();
        let (additions, deletions, mut files) = parse_numstat_z(&numstat_z);
        for path in untracked {
            files.push(json!({ "path": path, "additions": 0, "deletions": 0, "untracked": true }));
        }
        json!({
            "repo": "own",
            "branch": branch, "changes": changes, "last": last,
            "additions": additions, "deletions": deletions, "files": files,
        })
    })
    .await
    .map_err(|e| e.to_string())
}

/// 进程内快照缓存(按项目根):size+mtime 未变的文件直接复用。
static SNAPSHOT_CACHE: std::sync::Mutex<Option<(PathBuf, Vec<FileEntry>)>> =
    std::sync::Mutex::new(None);

/// 整树快照:文件清单 + 目录聚合。
#[tauri::command]
pub async fn files_snapshot(
    project_dir: String,
    worktree_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let root = resolve_target_root(&project_dir, worktree_path.as_deref())?;
    let (entries, reused) = {
        let mut guard = SNAPSHOT_CACHE.lock().unwrap();
        let previous = guard
            .as_ref()
            .filter(|(cached_root, _)| *cached_root == root)
            .map(|(_, entries)| entries.as_slice());
        let (entries, reused) = scan_incremental(&root, previous);
        *guard = Some((root.clone(), entries.clone()));
        (entries, reused)
    };
    let dirs = aggregate_dirs(&entries);
    let files: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "path": entry.path,
                "size": entry.size,
                "lines": entry.lines,
                "chars": entry.chars,
                "oversized": entry.oversized,
            })
        })
        .collect();
    Ok(json!({
        "files": files,
        "dirs": dirs,
        "reused": reused,
    }))
}

/// 文件预览:复用 files_edit 统一路径解析、只读判定与编码/换行探测。
#[tauri::command]
pub async fn file_preview(
    project_dir: String,
    path: String,
    worktree_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let root = resolve_target_root(&project_dir, worktree_path.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || crate::files_edit::preview_at(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// 单文件 Git Diff:供对话伴生上下文抽屉就地对照改动。
#[tauri::command]
pub async fn file_git_diff(
    project_dir: String,
    path: String,
    worktree_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let root = resolve_target_root(&project_dir, worktree_path.as_deref())?;
    let resolved = crate::files_edit::resolve_in_root(&root, &path)?;
    let rel = resolved.rel;
    tokio::task::spawn_blocking(move || {
        match kanzei_tools::project_state::git_state_of(&root) {
            kanzei_tools::project_state::GitState::Repo { .. } => {}
            kanzei_tools::project_state::GitState::None => {
                return Ok(json!({
                    "repo": "none", "path": rel, "status": "no_repo",
                    "diff": "", "additions": 0, "deletions": 0, "truncated": false,
                }));
            }
            kanzei_tools::project_state::GitState::Parent { toplevel } => {
                return Ok(json!({
                    "repo": "parent", "toplevel": toplevel, "path": rel, "status": "no_repo",
                    "diff": "", "additions": 0, "deletions": 0, "truncated": false,
                }));
            }
        }
        let run_git = |args: &[&str]| -> Result<(bool, String), String> {
            let out = crate::state::hidden_command("git")
                .args(args)
                .current_dir(&root)
                .output()
                .map_err(|e| format!("执行 git 失败: {e}"))?;
            Ok((
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).into_owned(),
            ))
        };
        let (_, status_out) = run_git(&["status", "--porcelain", "--", &rel])?;
        let trimmed_status = status_out.trim();
        if trimmed_status.starts_with("??") {
            return Ok(json!({
                "repo": "own", "path": rel, "status": "untracked",
                "diff": "", "additions": 0, "deletions": 0, "truncated": false,
            }));
        }
        let (has_head, _) = run_git(&["rev-parse", "--verify", "HEAD"])?;
        let raw_diff = if has_head {
            run_git(&["diff", "--no-color", "HEAD", "--", &rel])?.1
        } else {
            let staged = run_git(&["diff", "--no-color", "--cached", "--", &rel])?.1;
            let unstaged = run_git(&["diff", "--no-color", "--", &rel])?.1;
            if staged.is_empty() {
                unstaged
            } else if unstaged.is_empty() {
                staged
            } else {
                format!("{staged}\n{unstaged}")
            }
        };
        let mut additions = 0u64;
        let mut deletions = 0u64;
        for line in raw_diff.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }
        let truncated = raw_diff.len() > MAX_DIFF_BYTES;
        let diff = if truncated {
            let mut end = MAX_DIFF_BYTES;
            while end > 0 && !raw_diff.is_char_boundary(end) {
                end -= 1;
            }
            raw_diff[..end].to_string()
        } else {
            raw_diff
        };
        let status = if diff.is_empty() {
            "clean"
        } else if trimmed_status.starts_with('D') || trimmed_status.starts_with(" D") {
            "deleted"
        } else {
            "modified"
        };
        Ok(json!({
            "repo": "own",
            "path": rel,
            "status": status,
            "diff": diff,
            "additions": additions,
            "deletions": deletions,
            "truncated": truncated,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let out = crate::state::hidden_command("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git must run");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[tokio::test]
    async fn 预览拒绝越界路径() {
        let root = kanzei_base::test_support::TestDir::new("fv-escape");
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::write(root.join("ok.txt"), "fine").unwrap();
        let outside = root
            .parent()
            .unwrap()
            .join(format!("kz-fv-outside-{}.txt", std::process::id()));
        std::fs::write(&outside, "secret").unwrap();

        let ok = file_preview(root.display().to_string(), "ok.txt".into(), None)
            .await
            .unwrap();
        assert_eq!(ok["content"], "fine");
        assert_eq!(ok["binary"], false);

        let escape = file_preview(
            root.display().to_string(),
            format!("../{}", outside.file_name().unwrap().to_string_lossy()),
            None,
        )
        .await;
        assert!(escape.is_err(), "越界路径必须拒绝: {escape:?}");
        std::fs::remove_file(&outside).ok();
    }

    #[tokio::test]
    async fn 预览识别二进制并拒进文本通道() {
        let root = kanzei_base::test_support::TestDir::new("fv-bin");
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::write(root.join("blob.bin"), [0u8, 159, 146, 150]).unwrap();
        let preview = file_preview(root.display().to_string(), "blob.bin".into(), None)
            .await
            .unwrap();
        assert_eq!(preview["binary"], true);
        assert_eq!(preview["content"], "");
    }

    #[tokio::test]
    async fn 工作树预览与快照和diff始终指向同一工作树并拒绝伪造路径() {
        let root = kanzei_base::test_support::TestDir::new("fv-worktree");
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "user.name", "Test"]);
        std::fs::write(root.join("a.txt"), "main-line\n").unwrap();
        git(&root, &["add", "a.txt"]);
        git(&root, &["commit", "-q", "-m", "init"]);

        let wt_parent = kanzei_base::test_support::TestDir::new("fv-worktree-branch");
        let wt_dir = wt_parent.join("feat-branch");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feat-branch",
                &wt_dir.display().to_string(),
            ],
        );
        std::fs::write(wt_dir.join("a.txt"), "branch-line\n").unwrap();
        std::fs::write(wt_dir.join("branch-only.txt"), "only-in-wt\n").unwrap();

        let project_str = root.display().to_string();
        let wt_str = wt_dir.display().to_string();

        let main_preview = file_preview(project_str.clone(), "a.txt".into(), None)
            .await
            .unwrap();
        let wt_preview = file_preview(project_str.clone(), "a.txt".into(), Some(wt_str.clone()))
            .await
            .unwrap();
        assert_eq!(main_preview["content"], "main-line\n");
        assert_eq!(wt_preview["content"], "branch-line\n");

        let wt_diff = file_git_diff(project_str.clone(), "a.txt".into(), Some(wt_str.clone()))
            .await
            .unwrap();
        assert_eq!(wt_diff["status"], "modified");
        assert!(wt_diff["diff"].as_str().unwrap().contains("+branch-line"));

        let wt_snap = files_snapshot(project_str.clone(), Some(wt_str.clone()))
            .await
            .unwrap();
        let wt_files: Vec<&str> = wt_snap["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["path"].as_str())
            .collect();
        assert!(wt_files.contains(&"branch-only.txt"));

        let fake_wt = kanzei_base::test_support::TestDir::new("fv-fake-wt");
        std::fs::write(fake_wt.join("a.txt"), "fake").unwrap();
        let rejected = file_preview(
            project_str,
            "a.txt".into(),
            Some(fake_wt.display().to_string()),
        )
        .await;
        assert!(rejected.is_err(), "未在 git 登记的工作树必须被拒绝");
    }

    #[tokio::test]
    async fn 单文件diff返回修改内容并拒绝越界路径() {
        let root = kanzei_base::test_support::TestDir::new("fv-diff");
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "user.name", "Test"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        git(&root, &["add", "a.txt"]);
        git(&root, &["commit", "-q", "-m", "init"]);

        let clean = file_git_diff(root.display().to_string(), "a.txt".into(), None)
            .await
            .unwrap();
        assert_eq!(clean["status"], "clean");
        assert_eq!(clean["diff"], "");

        std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        let modified = file_git_diff(root.display().to_string(), "a.txt".into(), None)
            .await
            .unwrap();
        assert_eq!(modified["status"], "modified");
        assert_eq!(modified["additions"], 2);
        assert_eq!(modified["deletions"], 1);
        assert!(modified["diff"].as_str().unwrap().contains("+three"));

        std::fs::write(root.join("new.txt"), "hello\n").unwrap();
        let untracked = file_git_diff(root.display().to_string(), "new.txt".into(), None)
            .await
            .unwrap();
        assert_eq!(untracked["status"], "untracked");

        let escape = file_git_diff(root.display().to_string(), "../escape.txt".into(), None).await;
        assert!(escape.is_err());
    }

    #[tokio::test]
    async fn git_status返回中文与含空格路径不带引号转义且可直接预览和diff() {
        let root = kanzei_base::test_support::TestDir::new("fv-unicode-path");
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "core.quotepath", "true"]);
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(root.join("dir with space")).unwrap();
        std::fs::write(root.join("docs/使用手册.md"), "第一行\n").unwrap();
        std::fs::write(root.join("dir with space/a b.txt"), "old\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "init"]);

        std::fs::write(root.join("docs/使用手册.md"), "第一行\n第二行\n").unwrap();
        std::fs::write(root.join("dir with space/新文件.md"), "untracked\n").unwrap();

        let project_str = root.display().to_string();
        let status = git_status(project_str.clone(), None).await.unwrap();
        let paths: Vec<&str> = status["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["path"].as_str())
            .collect();
        assert!(
            paths.contains(&"docs/使用手册.md"),
            "中文已修改路径不得被 git 引号/八进制转义: {paths:?}"
        );
        assert!(
            paths.contains(&"dir with space/新文件.md"),
            "中文/空格未跟踪路径不得被转义: {paths:?}"
        );
        assert!(paths
            .iter()
            .all(|p| !p.starts_with('"') && !p.ends_with('"')));

        let preview = file_preview(project_str.clone(), "docs/使用手册.md".into(), None)
            .await
            .unwrap();
        assert_eq!(preview["content"], "第一行\n第二行\n");
        let diff = file_git_diff(project_str, "docs/使用手册.md".into(), None)
            .await
            .unwrap();
        assert_eq!(diff["status"], "modified");
        assert!(diff["diff"].as_str().unwrap().contains("+第二行"));
    }
}
