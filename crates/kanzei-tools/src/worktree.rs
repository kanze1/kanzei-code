//! Worktree 生命周期内核(R-207):建线/回执/回滚/合并预检/状态。
//!
//! 2026-08-16 从 kanzei-app/src/processes.rs 下沉——worktree 业务原桌面独占并自带
//! git plumbing,与 tools/git.rs 双轨;R-183(kz 无人值守)与 R-181(外部 agent 入局)
//! 需要 CLI 侧同一套线管理能力。本模块只承载**纯 git 逻辑与类型**,进程绑定台账
//! (bound_process)与写租约等 AppState 交互留在 kanzei-app 的 Tauri 接线层。
//!
//! 核心语义零变更:
//! - 原子认领:ref 创建是 CAS(`git branch <branch> HEAD`),成功即拥有,跨进程成立;
//! - 凭据回滚:[`WorktreeReceipt`] 只在认领成功后构造,分支侧 sha CAS 兜底;
//! - `git worktree prune` 是全仓操作,禁止调用(定点摘除见 discard);
//! - 合并前 `merge-tree --write-tree` 预检 + `merge --no-ff` 落地。

use std::path::{Path, PathBuf};

/// 一条线的工作树信息(原 kanzei-app state.rs 同构类型,下沉后桌面/CLI 共用)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
    pub files: Vec<String>,
    pub clean: bool,
    pub diff: String,
    /// 占着这棵树的线 id(清单来自 git,绑定关系来自进程表)。
    /// None = git 认得这棵树但没有线绑着它(手工建的,或者线已经关了)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_process: Option<String>,
}

/// 建线回滚凭据。**不变量:凭据只在认领成功之后构造**——目录预检与 `git branch`
/// 两道闸都过了才有它,所以「有凭据」⇔「这棵树的目录与这条分支都是本次调用建出来
/// 的」。认领失败与目录残留两条路径**根本产不出凭据**,回滚代码对它们不可达——
/// 这就是「零回滚」的机械形态。它不依赖锁,跨进程成立。
///
/// 分支那一侧还有第二道判据:只有停在**建出来时那个 sha** 上才允许删(线上已经有提交
/// 就删不动)。见 [`WorktreeReceipt::branch_sha`]。
#[derive(Debug, Clone)]
pub struct WorktreeReceipt {
    pub worktree: PathBuf,
    pub branch: String,
    /// `Some(sha)` = 认领成功那一刻这条分支停在 `sha`;回滚用
    /// `git update-ref -d <ref> <sha>` 做**原子比较后删除**——ref 已经不在这个 sha 上
    /// (线上已经有提交 / 别人重建了它)就删不动,这正是要的。
    ///
    /// `None` = sha 解析不出来(git 出错等),无法做 CAS;回滚一律不碰,也不当残留报。
    pub branch_sha: Option<String>,
}

/// 执行 git 命令(禁止创建控制台窗口,D-238 同源)。
fn worktree_command(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut command = std::process::Command::new("git");
    #[cfg(windows)]
    crate::hide_console(&mut command);
    command
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git 执行失败: {e}"))
}

/// 把路径变成可以交给 git 当**命令行参数**的形态(剥掉 Windows 扩展长度前缀)。
///
/// `normalized_project_root` 会 `canonicalize`,Windows 上产出 `\\?\C:\...`。
/// 2026-08-11 实测:该形态作为进程的 `current_dir` 完全可用(Win32 API 认),但
/// **作为 git 的参数一律失败** —— `git worktree add -b b \\?\C:\x\wt HEAD` 报
/// `could not create leading directories of '//?/C:/x/wt/.git': Invalid argument`。
/// 所以凡是把路径交给 git 当参数的地方都得先过这一层,否则「建线」在 Windows 上
/// 一次都成功不了(R-177 验收①)。反过来 `current_dir` 不必剥,保持原样即可。
pub fn git_arg_path(path: &Path) -> String {
    let raw = path.display().to_string();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string()
}

/// 一树一线查重用的路径键。
///
/// 查重必须发生在**建树之前**(D4 定案),那时目标目录还不存在、`canonicalize`
/// 必然失败,所以这里是「能规范化就规范化,不能就退回字面量归一」:统一分隔符、
/// 剥扩展长度前缀、去尾分隔符,Windows 上再小写。已绑定的线存的是已存在的真实
/// 路径(canonicalize 成功),目标路径存的是尚不存在的字面量(canonicalize 失败),
/// 两条路径经过同一套归一后仍然可比——这是查重能对上的原因。
pub fn worktree_key(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let raw = resolved.display().to_string().replace('\\', "/");
    let stripped = match raw.strip_prefix("//?/UNC/") {
        Some(rest) => format!("//{rest}"),
        None => raw.strip_prefix("//?/").unwrap_or(&raw).to_string(),
    };
    let trimmed = stripped.trim_end_matches('/');
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_string()
    }
}

/// 把任意字符串压成能当目录名/分支名用的形态。
fn sanitize_component(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

/// 工作树的目标路径与分支名——只算不落盘。
///
/// 与 `create_worktree` 分开,是因为一树一线查重要在建树**之前**拿到目标路径
/// (D4 定案:目标树已被绑定则拒绝,此时一棵树都不许多出来)。
///
/// # 路径里必须带项目名
///
/// 老版是 `root.parent().join(".kanzei-worktree-<name>")` —— 不含项目名。同一个父目录
/// 下放两个项目(`~/code/a`、`~/code/b`,这是常见布局)时,两边各建一棵叫 `dev` 的
/// 工作树会**落到同一条路径** `~/code/.kanzei-worktree-dev`:后建的那边看见目录已存在
/// 直接失败,而错误说的是「工作树已存在」,用户在自己项目里根本找不到它;更糟的是
/// 回滚会去动另一个项目的目录。加上项目名之后,同一父目录下的项目目录名必然互不相同,
/// 冲突面就此消失。
///
/// # 项目名与工作树名之间的分隔符不能是 `-`
///
/// [`sanitize_component`] 把一切非 `[A-Za-z0-9_-]` 压成 `-`,所以两个分量里都可能出现
/// `-`。用 `-` 连接就还原不出边界:「项目 `a` + 名字 `b-c`」与「项目 `a-b` + 名字 `c`」
/// 都落成 `.kanzei-worktree-a-b-c`,两个不同项目的工作树撞进同一条路径——正是上面刚
/// 修掉的那类冲突换个形态回来。用 `.` 连接就没有歧义:`sanitize_component` 永远产不出
/// `.`(它是被压掉的那一类),所以第一个 `.` 必然是分隔符本身。
pub fn worktree_target(root: &Path, name: &str) -> Result<(PathBuf, String), String> {
    let safe_name = sanitize_component(name);
    if safe_name.is_empty() {
        return Err("工作树名称不能为空".into());
    }
    let parent = root.parent().unwrap_or(root);
    let project_tag = root
        .file_name()
        .map(|value| sanitize_component(&value.to_string_lossy()))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "project".into());
    Ok((
        parent.join(format!(".kanzei-worktree-{project_tag}.{safe_name}")),
        // 分支名不必带项目名:分支活在本仓库里,跨项目不共享命名空间。
        format!("kanzei/thread-{safe_name}"),
    ))
}

/// 工作树的真实工作区状态:未提交文件清单 + diff。
///
/// `create_worktree` 与 `worktree_diff` 共用同一次探测。建线以前返回的是硬编码
/// 乐观值(空 files / clean=true / 空 diff),收活流程会把「线还有活没提交」当成
/// 干净合并——那是丢工作,不是显示问题。
pub fn worktree_status(root: &Path, worktree: &Path) -> Result<(Vec<String>, String), String> {
    let target = git_arg_path(worktree);
    let output = worktree_command(root, &["-C", &target, "status", "--porcelain"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let diff_output =
        worktree_command(root, &["-C", &target, "diff", "--no-ext-diff", "--binary"])?;
    if !diff_output.status.success() {
        return Err(String::from_utf8_lossy(&diff_output.stderr)
            .trim()
            .to_string());
    }
    Ok((
        files,
        String::from_utf8_lossy(&diff_output.stdout).to_string(),
    ))
}

/// 取工作树当前分支。R-179 顺手修:原 `worktree_field(root, worktree, field)`
/// 的 `field` 参数是死分支——`"branch"` 与 else 两支返回同一个值;调用点全部
/// 只传 `"branch"`,故收敛为单值函数,去掉 `field` 与 `root` 两个死参数。
pub fn worktree_current_branch(worktree: &Path) -> Result<String, String> {
    let output = worktree_command(worktree, &["branch", "--show-current"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        return Err(format!("工作树没有可合并分支: {}", worktree.display()));
    }
    Ok(branch)
}

/// 校验一条 worktree 路径:**必须是 git 自己认得的工作树**,且不是主根。
///
/// R-177 内容③/验收④:判据从「位于项目同级目录之下」改成「出现在
/// `git worktree list --porcelain` 里」。两个方向都更对:
/// - **更严**——兄弟目录里的另一个 git 仓、或者随便一个同级目录,以前都能通过
///   路径前缀检查混进来,现在过不了;
/// - **更全**——手工 `git worktree add` 到别处的树以前一律被拒,现在只要 git
///   认得就能合并/放弃(验收④要求这类树也能被发现)。
pub fn validate_worktree_path(root: &Path, worktree_path: &str) -> Result<PathBuf, String> {
    let worktree =
        std::fs::canonicalize(worktree_path).map_err(|e| format!("工作树不存在或无法解析: {e}"))?;
    if worktree_key(&worktree) == worktree_key(root) {
        return Err("不能对项目主树本身执行工作树操作".into());
    }
    let known = git_worktrees(root)?;
    if !known
        .iter()
        .any(|entry| worktree_key(&entry.path) == worktree_key(&worktree))
    {
        return Err(format!(
            "git 不认得这棵工作树: {}。只有 `git worktree list` 列出的树才能操作。",
            worktree.display()
        ));
    }
    Ok(worktree)
}

/// 本项目在 git 眼里的全部工作树(**含主树自己**)。
pub fn git_worktrees(root: &Path) -> Result<Vec<crate::WorktreeEntry>, String> {
    let output = worktree_command(root, &["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(crate::parse_worktree_list(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// git 是否仍把这条路径登记为本仓库的工作树。
pub fn worktree_is_registered(root: &Path, worktree: &Path) -> bool {
    let key = worktree_key(worktree);
    worktree_command(root, &["worktree", "list", "--porcelain"])
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.strip_prefix("worktree "))
                .any(|path| worktree_key(Path::new(path.trim())) == key)
        })
        .unwrap_or(false)
}

/// 工作树的 git 管理目录(`<worktree>/.git` 是一个写着 `gitdir: …` 的文件)。
///
/// 拿它是为了**定点**摘掉登记项,替掉老版的 `git worktree prune`——见
/// `discard_worktree` 里的说明。
pub fn worktree_admin_dir(worktree: &Path) -> Option<PathBuf> {
    let marker = std::fs::read_to_string(worktree.join(".git")).ok()?;
    let path = PathBuf::from(marker.trim().strip_prefix("gitdir:")?.trim());
    path.is_dir().then_some(path)
}

/// R-179 验收③:合并前冲突预检的**可读形态**——从 `merge-tree --write-tree`
/// 输出提取 CONFLICT 行中的文件名,供 UI 列出「哪些文件冲突」,而不是一句
/// 「有冲突」。提取不到冲突行时返回空列表(调用方回退到原始输出)。
pub fn parse_merge_tree_conflicts(stdout: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(stdout);
    text.lines()
        .filter(|line| line.contains("CONFLICT"))
        .map(|line| {
            // 标准格式两种:
            //  `CONFLICT (content): Merge conflict in src/foo.rs` — 路径在
            //   小写 "conflict in " 之后(内容冲突);
            //  `CONFLICT (modify/delete): src/gone.rs deleted in HEAD ...` —
            //   路径紧跟冒号(修改/删除类)。先找小写 "conflict in "(内容类,
            //   冒号后是 "Merge conflict in ..."),再回退冒号后的首个路径段;
            //   都没有就整行(至少列出冲突标记)。
            if let Some((_, path)) = line.split_once("conflict in ") {
                return path.trim().to_string();
            }
            line.split_once(": ")
                .map(|(_, rest)| {
                    rest.split_once(' ')
                        .map(|(path, _)| path.to_string())
                        .unwrap_or_else(|| rest.trim().to_string())
                })
                .unwrap_or_else(|| line.trim().to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn git_arg_path_剥离windows扩展长度前缀_unc保留() {
        assert_eq!(
            git_arg_path(Path::new(r"\\?\C:\Users\x\wt")),
            r"C:\Users\x\wt"
        );
        assert_eq!(
            git_arg_path(Path::new(r"\\?\UNC\server\share")),
            r"\\server\share"
        );
        assert_eq!(git_arg_path(Path::new(r"C:\plain")), r"C:\plain");
    }

    #[test]
    fn worktree_key_大小写与尾分隔符归一_不存在路径退回字面量() {
        // 同一个已存在目录,两种写法必须同键。
        let root = std::env::temp_dir().join(format!("kz-wt-key-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(worktree_key(&root), worktree_key(&root.join(".")));
        // 不存在的目标路径:canonicalize 失败,退回字面量归一,仍可比。
        let missing = root.join("sub");
        let missing2 = root.join("sub/");
        assert_eq!(worktree_key(&missing), worktree_key(&missing2));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn worktree_target_项目名加名字_分隔符用点_分支带前缀() {
        let root = std::env::temp_dir().join(format!("kz-wt-tgt-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let (path, branch) = worktree_target(&root, "dev-line").unwrap();
        assert_eq!(branch, "kanzei/thread-dev-line");
        assert!(path.to_string_lossy().contains(".kanzei-worktree-"));
        assert!(path.extension().is_some(), "名字应经 `.` 连在项目名后");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parse_merge_tree_conflicts_提取内容与modify_delete路径() {
        let out = b"CONFLICT (content): Merge conflict in src/foo.rs\n\
                    CONFLICT (modify/delete): src/gone.rs deleted in HEAD and modified in branch\n";
        let conflicts = parse_merge_tree_conflicts(out);
        assert_eq!(conflicts, vec!["src/foo.rs", "src/gone.rs"]);
        assert!(parse_merge_tree_conflicts(b"nothing here").is_empty());
    }
}
