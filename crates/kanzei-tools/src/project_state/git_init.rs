//! Git 初始化(新建项目、横幅「初始化 Git」、git 工具 action=init 共用;由 project_state.rs 拆出)。

use std::path::Path;

use serde::Serialize;

use super::{git_state, invalidate, GitState};

/// `.kanzei/.gitignore` 的内容:运行时文件(可重建的派生物、本机现场)不进版本库;
/// 项目资产(`project/*.md` tracker、`memory/*.md`、`kanzei.toml`)照常入库。
pub const KANZEI_GITIGNORE: &str = "\
# kanzei 运行时文件:可重建的派生物与本机现场,不进版本库。
# 项目资产照常入库:project/*.md(需求/缺陷/决策/测试记录)、memory/*.md、kanzei.toml。
state.db
state.db-*
state.db.v*.bak
*.lock
*.tmp
.write-log/
artifacts/
summaries/
quarantine/
file-annotations.json
memory/index.db
memory/index.db-*
memory/inbox.md
memory/inbox.checkpoint.json
project/auto-run-alerts.jsonl
";

/// 缺失时写 `.kanzei/.gitignore`(已存在绝不覆盖——那可能是用户改过的)。返回是否新写。
pub fn ensure_kanzei_gitignore(root: &Path) -> std::io::Result<bool> {
    let dir = root.join(".kanzei");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(".gitignore");
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(&path, KANZEI_GITIGNORE)?;
    Ok(true)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GitInitOutcome {
    /// 本次新建了仓库(已经是仓库时为 false)。
    pub created: bool,
    pub branch: String,
    /// 做了首次提交。
    pub committed: bool,
    /// 想做首次提交但本机没配 git user.name / user.email。
    pub identity_missing: bool,
    /// 本次新写了 `.kanzei/.gitignore`。
    pub gitignore_written: bool,
    /// 想做首次提交但没做成(待提交文件过多、签名失败、钩子拒绝等)的可行动说明;
    /// 仓库本身已建好,没有提交。没出错时不序列化(旧调用方的 JSON 形状不变)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_error: Option<String>,
}

/// 首次提交的默认提交信息(「初始化并首次提交」不让用户先想一句话)。
pub const FIRST_COMMIT_MESSAGE: &str = "初始化项目(kanzei)";
/// 首次提交最多纳入多少个未被忽略的文件:超过多半是 node_modules / target 之类的大目录没被忽略,
/// 一把梭 `git add -A` 会把它们全塞进版本库,事后很难收拾——先拦下,让用户补好 `.gitignore`。
pub const FIRST_COMMIT_FILE_LIMIT: usize = 5000;

fn git_sync(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new("git");
    command
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null());
    crate::hide_console(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("找不到 git(不在 PATH 上?): {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "git {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// 在 `root` 建一个独立仓库(默认分支 main),补 `.kanzei/.gitignore`;`initial_commit` 时若本机
/// 配了 git 身份就做一次首提交(并行线/工作树需要 HEAD)。`root` 自己已经是仓库时只补忽略规则;
/// 它**还没有任何提交**且要首提交时(UX-128:「初始化并首次提交」、没提交横幅上的「首次提交」)补做这一次提交,
/// 已有提交的仓库绝不再提交。首提交没做成不算 `git_init` 失败——仓库已经建好了,原因进
/// [`GitInitOutcome::identity_missing`] / [`GitInitOutcome::commit_error`],由调用方给用户说清下一步。
/// 位于上级仓库内时照样在 `root` 建嵌套仓库——调用方负责先征得同意(git 工具的 init 是 Ask)。
pub fn git_init(root: &Path, initial_commit: bool) -> Result<GitInitOutcome, String> {
    let root = crate::path_form::simplify(root);
    let mut outcome = GitInitOutcome {
        gitignore_written: ensure_kanzei_gitignore(&root)
            .map_err(|error| format!("写 .kanzei/.gitignore 失败: {error}"))?,
        ..GitInitOutcome::default()
    };
    if let GitState::Repo {
        branch,
        has_commits,
    } = git_state(&root)
    {
        outcome.branch = branch.unwrap_or_default();
        if initial_commit && !has_commits {
            first_commit(&root, &mut outcome);
        }
        invalidate();
        return Ok(outcome);
    }
    if git_sync(&root, &["init", "-b", "main"]).is_err() {
        // git < 2.28 没有 -b:先 init,再把 HEAD 指到 main。
        git_sync(&root, &["init"])?;
        git_sync(&root, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    }
    outcome.created = true;
    outcome.branch = "main".into();
    if initial_commit {
        first_commit(&root, &mut outcome);
    }
    invalidate();
    Ok(outcome)
}

/// 做首次提交(`git add -A` + 默认提交信息 + `--allow-empty`,空项目也能得到 HEAD)。
/// 每一种没做成的情况都写进 `outcome` 并附下一步怎么办,不返回 Err。
fn first_commit(root: &Path, outcome: &mut GitInitOutcome) {
    let name = git_sync(root, &["config", "user.name"]).unwrap_or_default();
    let email = git_sync(root, &["config", "user.email"]).unwrap_or_default();
    if name.is_empty() || email.is_empty() {
        outcome.identity_missing = true;
        return;
    }
    // 先数一下会被 add 进去的文件:远超常识量多半是大目录没被忽略,别一把梭。
    match git_sync(root, &["ls-files", "--others", "--exclude-standard"]) {
        Ok(list) => {
            let count = list.lines().filter(|line| !line.trim().is_empty()).count();
            if count > FIRST_COMMIT_FILE_LIMIT {
                outcome.commit_error = Some(too_many_files_hint(count));
                return;
            }
        }
        Err(error) => {
            outcome.commit_error = Some(commit_failure_hint(&error));
            return;
        }
    }
    let result = git_sync(root, &["add", "-A"]).and_then(|_| {
        git_sync(
            root,
            &["commit", "--allow-empty", "-q", "-m", FIRST_COMMIT_MESSAGE],
        )
    });
    match result {
        Ok(_) => outcome.committed = true,
        Err(error) => outcome.commit_error = Some(commit_failure_hint(&error)),
    }
}

fn too_many_files_hint(count: usize) -> String {
    format!(
        "待提交的文件有 {count} 个(超过 {FIRST_COMMIT_FILE_LIMIT} 个),多半是 node_modules / target 之类的目录没有被忽略。\
         先在项目根的 .gitignore 里把它们排除,再点「首次提交」。"
    )
}

/// 把 git 提交失败的原话补上下一步怎么办(原话保留在后面,方便搜索)。
fn commit_failure_hint(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    let next = if lower.contains("gpg") || lower.contains("sign") {
        "看起来是提交签名失败:检查 git 的 commit.gpgsign 与签名工具配置,或在终端用 `git -c commit.gpgsign=false commit --allow-empty -m \"init\"` 完成首次提交。"
    } else if lower.contains("hook") {
        "看起来是 git 钩子拒绝了提交:检查 .git/hooks 里的 pre-commit / commit-msg。"
    } else if lower.contains("lock") {
        "仓库里有残留的 .git/index.lock:确认没有别的 git 程序在运行后删掉它,再重试。"
    } else {
        "可以在项目目录里手动运行 `git add -A` 和 `git commit -m \"init\"` 看具体原因,处理后再重试。"
    };
    format!("首次提交没有成功。{next}\n{error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_state::tests::temp_root;

    #[test]
    fn git_init_建库_写忽略规则_不覆盖已有文件() {
        if crate::shell::find_executable("git", &crate::shell::fresh_path()).is_none() {
            return;
        }
        let root = temp_root("init");
        let outcome = git_init(&root, false).expect("git init");
        assert!(outcome.created && outcome.gitignore_written);
        assert_eq!(outcome.branch, "main");
        assert!(root.join(".git").is_dir());
        let ignore = std::fs::read_to_string(root.join(".kanzei").join(".gitignore")).unwrap();
        for rule in ["state.db-*", "*.lock", "artifacts/", ".write-log/"] {
            assert!(
                ignore.lines().any(|line| line == rule),
                "缺 {rule}:\n{ignore}"
            );
        }
        assert!(
            !ignore
                .lines()
                .any(|line| line.starts_with("project") && !line.contains("auto-run-alerts")),
            "tracker 必须入库:\n{ignore}"
        );
        assert_eq!(
            git_state(&root),
            GitState::Repo {
                branch: Some("main".into()),
                has_commits: false
            }
        );
        // 已是仓库:不重建、不覆盖用户改过的忽略规则。
        std::fs::write(root.join(".kanzei").join(".gitignore"), "custom\n").unwrap();
        let again = git_init(&root, false).unwrap();
        assert!(!again.created && !again.gitignore_written);
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei").join(".gitignore")).unwrap(),
            "custom\n"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn git_init_首提交取决于本机身份() {
        if crate::shell::find_executable("git", &crate::shell::fresh_path()).is_none() {
            return;
        }
        let root = temp_root("init-commit");
        std::fs::write(root.join(".kanzei").join("state.db-wal"), "x").unwrap();
        std::fs::write(
            root.join(".kanzei").join("project").join("requirements.md"),
            "# R\n",
        )
        .unwrap();
        let outcome = git_init(&root, true).expect("git init");
        if outcome.identity_missing {
            assert!(!outcome.committed);
            assert!(!git_state(&root).supports_worktrees());
        } else {
            assert!(outcome.committed, "{outcome:?}");
            assert!(
                git_state(&root).supports_worktrees(),
                "首提交后应能开并行线"
            );
            let tracked = git_sync(&root, &["ls-files"]).unwrap();
            assert!(tracked.contains(".kanzei/.gitignore"), "{tracked}");
            assert!(
                tracked.contains(".kanzei/project/requirements.md"),
                "{tracked}"
            );
            assert!(!tracked.contains("state.db"), "{tracked}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// UX-128:「初始化并首次提交」——已经 init 过、还没有提交的仓库也能补这一次提交;
    /// 已有提交的仓库绝不再提交。身份用仓库本地配置钉死,不依赖本机全局设置。
    #[test]
    fn git_init_对没有提交的仓库补首提交_已有提交不再提交() {
        if crate::shell::find_executable("git", &crate::shell::fresh_path()).is_none() {
            return;
        }
        let root = temp_root("init-late-commit");
        std::fs::write(root.join("a.txt"), "x").unwrap();
        let first = git_init(&root, false).expect("git init");
        assert!(first.created && !first.committed);
        git_sync(&root, &["config", "user.name", "kz-test"]).unwrap();
        git_sync(&root, &["config", "user.email", "kz-test@example.com"]).unwrap();
        assert!(!git_state(&root).supports_worktrees());

        let late = git_init(&root, true).expect("补首提交");
        assert!(!late.created, "仓库已存在,不是新建");
        assert!(late.committed, "{late:?}");
        assert!(
            late.commit_error.is_none() && !late.identity_missing,
            "{late:?}"
        );
        assert!(
            git_state(&root).supports_worktrees(),
            "提交之后应能开并行线"
        );
        let tracked = git_sync(&root, &["ls-files"]).unwrap();
        assert!(tracked.contains("a.txt"), "{tracked}");
        assert_eq!(
            git_sync(&root, &["log", "-1", "--format=%s"]).unwrap(),
            FIRST_COMMIT_MESSAGE
        );

        // 已有提交:再要首提交也不动(不会又多一个提交)。
        std::fs::write(root.join("b.txt"), "y").unwrap();
        let again = git_init(&root, true).unwrap();
        assert!(!again.created && !again.committed, "{again:?}");
        assert_eq!(
            git_sync(&root, &["rev-list", "--count", "HEAD"]).unwrap(),
            "1"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 首提交没做成时给的是可行动的说明(下一步怎么办 + git 原话),不是一句裸的 git 报错。
    #[test]
    fn 首提交失败说明可行动() {
        let signing = commit_failure_hint("git commit 失败: error: gpg failed to sign the data");
        assert!(signing.contains("commit.gpgsign"), "{signing}");
        assert!(
            signing.contains("gpg failed to sign"),
            "原话要保留:{signing}"
        );
        assert!(commit_failure_hint("pre-commit hook exited 1").contains("钩子"));
        assert!(commit_failure_hint("Unable to create '.git/index.lock'").contains("index.lock"));
        let other = commit_failure_hint("git add -A 失败: boom");
        assert!(
            other.contains("git add -A") && other.contains("boom"),
            "{other}"
        );
        let many = too_many_files_hint(FIRST_COMMIT_FILE_LIMIT + 1);
        assert!(
            many.contains(&(FIRST_COMMIT_FILE_LIMIT + 1).to_string()),
            "{many}"
        );
        assert!(
            many.contains(".gitignore") && many.contains("首次提交"),
            "{many}"
        );
    }
}
