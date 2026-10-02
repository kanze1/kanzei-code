//! Child snapshots never change the parent's index or branch. Adoption applies only
//! the child's delta and fails before writing when the current tree conflicts.
use anyhow::{bail, Context, Result};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn command(root: &Path) -> Command {
    let mut c = Command::new("git");
    #[cfg(windows)]
    crate::hide_console(&mut c);
    c.current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    c
}
pub fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = command(root).args(args).output()?;
    if !out.status.success() {
        bail!("git: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8(out.stdout)?.trim().into())
}
pub fn snapshot(root: &Path, tag: &str) -> Result<String> {
    let index =
        std::env::temp_dir().join(format!("kanzei-agent-index-{tag}-{}", super::fresh_id()));
    struct Index(PathBuf);
    impl Drop for Index {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(self.0.with_extension("lock"));
        }
    }
    let _cleanup = Index(index.clone());
    let run = |args: &[&str]| -> Result<String> {
        let out = command(root)
            .env("GIT_INDEX_FILE", &index)
            .args(args)
            .output()?;
        if !out.status.success() {
            bail!("{}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(String::from_utf8(out.stdout)?.trim().into())
    };
    let parent = git(root, &["rev-parse", "HEAD"]).context("可写子任务需要已有提交的 Git 项目")?;
    run(&["read-tree", &parent])?;
    // Feed explicit, literal paths. Negative pathspecs make git add fail when a
    // matching runtime database is ignored, and add-all can read a live SQLite file.
    let listed = command(root)
        .env("GIT_INDEX_FILE", &index)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()?;
    if !listed.status.success() {
        bail!("Cannot enumerate workspace snapshot");
    }
    let mut paths = Vec::new();
    for path in listed.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        if path.starts_with(b".kanzei/state.db") || path.starts_with(b".kanzei/agent-memory/") {
            continue;
        }
        paths.extend_from_slice(path);
        paths.push(0);
    }
    if !paths.is_empty() {
        let mut child = command(root)
            .env("GIT_INDEX_FILE", &index)
            .args([
                "--literal-pathspecs",
                "add",
                "-A",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .context("snapshot stdin")?
            .write_all(&paths)?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("{}", String::from_utf8_lossy(&out.stderr));
        }
    }
    let tree = run(&["write-tree"])?;
    let out = command(root)
        .env("GIT_AUTHOR_NAME", "Kanzei")
        .env("GIT_AUTHOR_EMAIL", "agent@local")
        .env("GIT_COMMITTER_NAME", "Kanzei")
        .env("GIT_COMMITTER_EMAIL", "agent@local")
        .args([
            "commit-tree",
            &tree,
            "-p",
            &parent,
            "-m",
            "Subagent workspace checkpoint",
        ])
        .output()?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8(out.stdout)?.trim().into())
}
pub fn prepare(root: &Path, id: &str) -> Result<(PathBuf, String)> {
    let base = snapshot(root, id)?;
    let (path, _) = crate::worktree::worktree_target(root, &format!("agent-{id}"))
        .map_err(anyhow::Error::msg)?;
    let target = crate::worktree::git_arg_path(&path);
    git(root, &["worktree", "add", "--detach", &target, &base])?;
    Ok((path, base))
}

/// Reuse isolated writer snapshots for an application-owned conversation workspace.
/// Git is bookkeeping here; it does not register a project or impose delivery commits.
pub fn ensure_general_repository(root: &Path, store_root: &Path) -> Result<()> {
    anyhow::ensure!(
        kanzei_harness::is_general_conversation_root(store_root),
        "Not a general conversation store"
    );
    let allowed = store_root.join("artifacts").canonicalize()?;
    let actual = root.canonicalize()?;
    anyhow::ensure!(
        actual.starts_with(&allowed) && actual != allowed,
        "General writer workspace outside conversation artifacts"
    );
    let _lock =
        kanzei_base::atomic_file::lock_exclusive(&actual.join(".kanzei/general-workspace.lock"))?;
    if !actual.join(".git").exists() {
        git(&actual, &["init", "--quiet"])?;
    }
    let top = PathBuf::from(git(&actual, &["rev-parse", "--show-toplevel"])?).canonicalize()?;
    anyhow::ensure!(
        top == actual,
        "General writer repository belongs to another directory"
    );
    if git(&actual, &["rev-parse", "--verify", "HEAD"]).is_err() {
        git(
            &actual,
            &[
                "-c",
                "user.name=Kanzei",
                "-c",
                "user.email=agent@local",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "Conversation workspace baseline",
            ],
        )?;
    }
    Ok(())
}
pub fn result(tree: &Path, base: &str, id: &str) -> Result<(String, Vec<String>)> {
    let head = snapshot(tree, id)?;
    let files = git(
        tree,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--name-only",
            base,
            &head,
        ],
    )?
    .lines()
    .map(str::to_owned)
    .collect();
    // Keep durable results reachable after Git garbage collection. This changes
    // a private bookkeeping ref, never the user's branch or index.
    use sha2::{Digest, Sha256};
    let key = format!(
        "{:x}",
        Sha256::digest(format!("{}:{id}", crate::worktree::worktree_key(tree)))
    );
    git(
        tree,
        &[
            "update-ref",
            &format!("refs/kanzei/agent-results/{key}"),
            &head,
        ],
    )?;
    Ok((head, files))
}
pub fn diff(root: &Path, base: &str, head: &str) -> Result<String> {
    git(root, &["diff", "--no-ext-diff", "--unified=3", base, head])
}
pub fn adopt(root: &Path, base: &str, head: &str) -> Result<()> {
    let patch = command(root)
        .args(["diff", "--binary", "--no-ext-diff", base, head])
        .output()?;
    if !patch.status.success() {
        bail!("无法读取子任务改动");
    }
    if patch.stdout.is_empty() {
        return Ok(());
    }
    for check in [true, false] {
        let mut c = command(root);
        c.arg("apply");
        if check {
            c.arg("--check");
        }
        let mut child = c
            .arg("--whitespace=nowarn")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .context("patch stdin")?
            .write_all(&patch.stdout)?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!(
                "改动未采纳，保留子任务工作树：{}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    }
    Ok(())
}
