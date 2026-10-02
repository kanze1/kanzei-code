//! 暂存区契约：持久请求清单、显式路径范围和只依赖 stdout 的 CAS 指纹。

use super::run_git;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::Path;

#[cfg(test)]
pub(super) async fn staged_paths(cwd: &Path) -> Result<Vec<String>, String> {
    staged_paths_with_diagnostics(cwd, &mut Vec::new()).await
}

pub(super) async fn staged_paths_with_diagnostics(
    cwd: &Path,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    // D-347:必须 -c core.quotepath=false——git 默认对非 ASCII 路径输出带引号的
    // 八进制转义("docs/\347\233\256\345\275\225.md"),与请求的真实 UTF-8 路径
    // 比较必不相等,含中文文件名的暂存区会让后续 stage 全部误判 foreign。
    let text = run_git(
        cwd,
        &[
            "-c",
            "core.quotepath=false",
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
        ],
    )
    .await?
    .take_stdout(diagnostics);
    Ok(parse_staged_paths(&text))
}

pub(super) fn parse_staged_paths(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StageRequest {
    pub(super) token: String,
    pub(super) paths: Vec<String>,
}

pub(super) fn stage_manifest_path(cwd: &Path) -> Result<std::path::PathBuf, String> {
    let marker = cwd.join(".git");
    let git_dir = if marker.is_dir() {
        marker
    } else {
        let text = std::fs::read_to_string(&marker)
            .map_err(|e| format!("cannot read .git worktree pointer: {e}"))?;
        let raw = text
            .strip_prefix("gitdir:")
            .map(str::trim)
            .ok_or("invalid .git worktree pointer")?;
        let path = std::path::PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    let key_path = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut hasher = DefaultHasher::new();
    key_path.to_string_lossy().hash(&mut hasher);
    Ok(git_dir.join(format!(
        "kanzei-stage-request-{:016x}.json",
        hasher.finish()
    )))
}

pub(super) fn load_stage_request(
    cwd: &Path,
) -> Result<Option<(std::path::PathBuf, StageRequest)>, String> {
    let path = stage_manifest_path(cwd)?;
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read stage request manifest: {e}"))?;
    let request = serde_json::from_str(&text)
        .map_err(|e| format!("invalid stage request manifest {}: {e}", path.display()))?;
    Ok(Some((path, request)))
}

pub(super) fn write_stage_request(path: &Path, request: &StageRequest) -> Result<(), String> {
    let text = serde_json::to_string_pretty(request)
        .map_err(|e| format!("serialize stage request: {e}"))?;
    crate::atomic_file::write_atomic(path, &text).map_err(|e| e.to_string())
}

pub(super) fn remove_stage_request(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove stage request manifest: {error}")),
    }
}

pub(super) fn new_stage_token(cwd: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    cwd.to_string_lossy().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
        .hash(&mut hasher);
    format!("stage-{:016x}", hasher.finish())
}

pub(super) fn union_paths(existing: &[String], requested: &[String]) -> Vec<String> {
    let mut paths: BTreeSet<String> = existing.iter().cloned().collect();
    paths.extend(requested.iter().cloned());
    paths.into_iter().collect()
}

pub(super) async fn staged_state(
    cwd: &Path,
    diagnostics: &mut Vec<String>,
) -> Result<(String, String, Vec<String>), String> {
    let diff = run_git(
        cwd,
        &[
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-color",
        ],
    )
    .await?
    .take_stdout(diagnostics);
    let paths = staged_paths_with_diagnostics(cwd, diagnostics).await?;
    let hash = staged_hash(&diff);
    Ok((hash, diff, paths))
}

pub(super) fn staged_hash(diff: &str) -> String {
    let mut hasher = DefaultHasher::new();
    diff.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
