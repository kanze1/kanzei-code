//! Worktree 解析域(R-257 B4):`git worktree list --porcelain` 的记录模型与解析、
//! 按分支查工作树。自 git.rs 原样迁出,零行为变更。

/// `git worktree list --porcelain` 的一条记录。
///
/// R-177 内容③:线清单的**真源**是 git,不是前端 localStorage。手工
/// `git worktree add` 出来的树、以及 kzapp 换了一台机器/清了缓存之后的树,
/// 都必须能被发现——localStorage 清单三者一个都做不到。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: std::path::PathBuf,
    /// 检出的分支短名(`refs/heads/` 已剥)。detached / bare 时为 None。
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
}

/// 解析 `git worktree list --porcelain`。
///
/// porcelain 的形状:每条记录以 `worktree <path>` 开头,后面跟若干属性行,
/// 记录之间用空行分隔。`branch` / `bare` / `detached` / `locked` / `prunable`
/// 都可能出现,`locked` 与 `prunable` 还可能带一个原因串(`locked <reason>`)。
pub fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeEntry> {
    let mut out: Vec<WorktreeEntry> = Vec::new();
    for line in porcelain.lines() {
        let line = line.trim_end();
        if let Some(path) = line.strip_prefix("worktree ") {
            out.push(WorktreeEntry {
                path: std::path::PathBuf::from(path.trim()),
                branch: None,
                bare: false,
                detached: false,
                locked: false,
                prunable: false,
            });
            continue;
        }
        let Some(current) = out.last_mut() else {
            continue; // 属性行出现在任何 `worktree` 之前:不是合法输出,跳过。
        };
        if let Some(head) = line.strip_prefix("branch ") {
            current.branch = Some(
                head.trim()
                    .strip_prefix("refs/heads/")
                    .unwrap_or(head.trim())
                    .to_string(),
            );
        } else if line == "bare" {
            current.bare = true;
        } else if line == "detached" {
            current.detached = true;
        } else if line == "locked" || line.starts_with("locked ") {
            current.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            current.prunable = true;
        }
    }
    out
}

/// 找到检出了指定分支的工作树路径。
pub(crate) fn worktree_for_branch(porcelain: &str, branch: &str) -> Option<std::path::PathBuf> {
    parse_worktree_list(porcelain)
        .into_iter()
        .find(|entry| entry.branch.as_deref() == Some(branch))
        .map(|entry| entry.path)
}
