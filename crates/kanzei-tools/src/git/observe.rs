//! In-process repository observations: no Git executable, hooks, filters or pager.
//! Worktree diff bytes are opened through the same capability as `read`.
use std::io::Read;
use std::path::Path;

use git2::{Repository, RepositoryOpenFlags, Status, StatusOptions};
use kanzei_harness::{ToolCtx, ToolOutput};

const MAX_DIFF_BYTES: usize = 1024 * 1024;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

pub struct StatusObservation {
    pub head_before: Result<String, String>,
    pub head_after: Result<String, String>,
    pub branch: String,
    pub porcelain: Vec<u8>,
}

fn open(cwd: &Path) -> Result<Repository, git2::Error> {
    Repository::open_ext(
        cwd,
        RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&Path>(),
    )
}

fn head(repo: &Repository) -> Result<String, String> {
    repo.head()
        .and_then(|reference| reference.peel_to_commit())
        .map(|commit| commit.id().to_string())
        .map_err(|error| error.to_string())
}

fn status_columns(status: Status) -> [u8; 2] {
    if status.is_conflicted() {
        return *b"UU";
    }
    if status.is_wt_new() {
        return *b"??";
    }
    let index = if status.is_index_new() {
        b'A'
    } else if status.is_index_deleted() {
        b'D'
    } else if status.is_index_renamed() {
        b'R'
    } else if status.is_index_typechange() {
        b'T'
    } else if status.is_index_modified() {
        b'M'
    } else {
        b' '
    };
    let worktree = if status.is_wt_deleted() {
        b'D'
    } else if status.is_wt_renamed() {
        b'R'
    } else if status.is_wt_typechange() {
        b'T'
    } else if status.is_wt_modified() {
        b'M'
    } else {
        b' '
    };
    [index, worktree]
}

pub fn status(cwd: &Path) -> Result<StatusObservation, String> {
    let repo = open(cwd).map_err(|error| error.to_string())?;
    let head_before = head(&repo);
    let branch = repo
        .head()
        .and_then(|reference| reference.shorthand().map(str::to_owned))
        .unwrap_or_else(|_| "unborn HEAD".into());
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .exclude_submodules(true)
        .update_index(false)
        .renames_head_to_index(true);
    let entries = repo
        .statuses(Some(&mut options))
        .map_err(|error| error.to_string())?;
    let mut porcelain = Vec::new();
    for entry in entries.iter() {
        let columns = status_columns(entry.status());
        if columns == *b"  " {
            continue;
        }
        porcelain.extend(columns);
        porcelain.push(b' ');
        let delta = entry.index_to_workdir().or_else(|| entry.head_to_index());
        let path = delta
            .as_ref()
            .and_then(|delta| delta.new_file().path_bytes())
            .unwrap_or_else(|| entry.path_bytes());
        porcelain.extend(path);
        porcelain.push(0);
        if columns.contains(&b'R') {
            if let Some(old) = delta
                .as_ref()
                .and_then(|delta| delta.old_file().path_bytes())
            {
                porcelain.extend(old);
                porcelain.push(0);
            }
        }
    }
    Ok(StatusObservation {
        head_before,
        head_after: head(&repo),
        branch,
        porcelain,
    })
}

pub fn render_status(observation: &StatusObservation) -> String {
    let mut out = format!("## {}\n", observation.branch);
    for record in observation
        .porcelain
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        out.push_str(&String::from_utf8_lossy(record));
        out.push('\n');
    }
    out.trim_end().into()
}

fn failure(error: impl std::fmt::Display) -> Box<ToolOutput> {
    Box::new(ToolOutput::error(error.to_string()))
}

fn append_patch(
    out: &mut String,
    old: &[u8],
    new: &[u8],
    path: &Path,
) -> Result<(), Box<ToolOutput>> {
    let mut options = git2::DiffOptions::new();
    options.ignore_submodules(true).update_index(false);
    let mut patch = git2::Patch::from_buffers(old, Some(path), new, Some(path), Some(&mut options))
        .map_err(failure)?;
    let mut bytes = Vec::new();
    patch
        .print(&mut |_, _, line| {
            if matches!(line.origin(), '+' | '-' | ' ') {
                bytes.push(line.origin() as u8);
            }
            bytes.extend(line.content());
            out.len() + bytes.len() <= MAX_DIFF_BYTES
        })
        .map_err(|_| failure("Git diff exceeds 1 MiB; select fewer explicit files"))?;
    out.push_str(&String::from_utf8_lossy(&bytes));
    Ok(())
}

pub(crate) fn diff(
    ctx: &ToolCtx,
    files: &[String],
    staged: bool,
) -> Result<String, Box<ToolOutput>> {
    let repo = open(&ctx.cwd).map_err(failure)?;
    let index = repo.index().map_err(failure)?;
    let selected = |path: &str| files.is_empty() || files.iter().any(|file| file == path);
    let mut out = String::new();
    if staged {
        let tree = match repo.head() {
            Ok(reference) => Some(reference.peel_to_tree().map_err(failure)?),
            Err(error)
                if matches!(
                    error.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                None
            }
            Err(error) => return Err(failure(error)),
        };
        let changes = repo
            .diff_tree_to_index(tree.as_ref(), Some(&index), None)
            .map_err(failure)?;
        for delta in changes.deltas() {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .ok_or_else(|| failure("Git delta has no path"))?;
            if !selected(&path.to_string_lossy()) {
                continue;
            }
            for resource in [delta.old_file().path(), delta.new_file().path()]
                .into_iter()
                .flatten()
            {
                crate::file_access::resolve(ctx, resource, "read")?;
            }
            let old = if delta.old_file().id().is_zero() {
                None
            } else {
                Some(repo.find_blob(delta.old_file().id()).map_err(failure)?)
            };
            let new = if delta.new_file().id().is_zero() {
                None
            } else {
                Some(repo.find_blob(delta.new_file().id()).map_err(failure)?)
            };
            append_patch(
                &mut out,
                old.as_ref().map_or(&[], |blob| blob.content()),
                new.as_ref().map_or(&[], |blob| blob.content()),
                path,
            )?;
        }
    } else {
        for entry in index.iter() {
            let path = std::str::from_utf8(&entry.path).map_err(failure)?;
            if !selected(path) || entry.mode == 0o160000 {
                continue;
            }
            let file = crate::file_access::resolve(ctx, Path::new(path), "read")?;
            let mut new = Vec::new();
            match file.open() {
                Ok(reader) => {
                    reader
                        .take(MAX_FILE_BYTES + 1)
                        .read_to_end(&mut new)
                        .map_err(failure)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(failure(error)),
            }
            let old = repo.find_blob(entry.id).map_err(failure)?;
            if new.len() as u64 > MAX_FILE_BYTES || old.size() as u64 > MAX_FILE_BYTES {
                return Err(failure(
                    "Git diff file exceeds 8 MiB; use a bounded file read",
                ));
            }
            if old.content() != new {
                append_patch(&mut out, old.content(), &new, Path::new(path))?;
            }
        }
    }
    Ok(if out.is_empty() {
        "(no diff)".into()
    } else {
        out
    })
}

pub(crate) fn log(cwd: &Path, files: &[String], count: usize) -> Result<String, String> {
    let repo = open(cwd).map_err(|error| error.to_string())?;
    let mut walk = repo.revwalk().map_err(|error| error.to_string())?;
    if repo.is_empty().map_err(|error| error.to_string())? {
        return Ok("(no commits)".into());
    }
    walk.push_head().map_err(|error| error.to_string())?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .map_err(|error| error.to_string())?;
    let mut lines = Vec::new();
    for id in walk {
        let commit = repo
            .find_commit(id.map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        if !files.is_empty() {
            let tree = commit.tree().map_err(|error| error.to_string())?;
            let parent = if commit.parent_count() > 0 {
                Some(
                    commit
                        .parent(0)
                        .and_then(|parent| parent.tree())
                        .map_err(|error| error.to_string())?,
                )
            } else {
                None
            };
            let changes = repo
                .diff_tree_to_tree(parent.as_ref(), Some(&tree), None)
                .map_err(|error| error.to_string())?;
            if !changes.deltas().any(|delta| {
                [delta.old_file().path(), delta.new_file().path()]
                    .into_iter()
                    .flatten()
                    .any(|path| files.iter().any(|file| path == Path::new(file)))
            }) {
                continue;
            }
        }
        let stamp = chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
            .map(|date| date.format("%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown time".into());
        lines.push(format!(
            "{} {} {} | {}",
            &commit.id().to_string()[..7],
            stamp,
            commit.author().name().unwrap_or("unknown author"),
            commit
                .summary()
                .map_err(|error| error.to_string())?
                .unwrap_or("")
        ));
        if lines.len() >= count {
            break;
        }
    }
    Ok(if lines.is_empty() {
        "(no commits)".into()
    } else {
        lines.join("\n")
    })
}
