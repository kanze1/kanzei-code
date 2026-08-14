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
pub fn worktree_command(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
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

/// 目录残留挡住这个名字时的文案(D-004 口径:必须自带可执行的解法)。
///
/// 这条预检是「名字被目录占死」唯一的出口。走到这里时**一树一线查重已经放行**
/// (`create_process` 那一侧查的是内存表 ∪ state.db),也就是说没有任何一条线绑着它 ——
/// 它是无主残留:上一条线关闭时留下的、或上次回滚没收干净的。里面**可能有未提交改动**,
/// 所以文案先给「看一眼」,再给「删掉」,最后才给分支那一半。
fn residual_worktree_error(root: &Path, worktree: &Path, branch: &str) -> String {
    format!(
        "工作树已存在: {};app 的线清单里没有线绑着它(无主残留:上一条线关闭时留下,\
         或上次回滚没收干净),里面可能还有未提交改动。\
         先看一眼 `git -C \"{}\" status --porcelain`;确认可丢弃后执行 \
         `git -C \"{}\" worktree remove --force \"{}\"`,该命令报「不是工作树」就直接\
         删掉这个目录;同名分支 {branch} 若也要一并回收,再执行 \
         `git -C \"{}\" branch -D {branch}`,然后重试",
        worktree.display(),
        git_arg_path(worktree),
        git_arg_path(root),
        git_arg_path(worktree),
        git_arg_path(root),
    )
}

/// 认领(`git branch`)失败时的文案。
///
/// 最常见的成因是**分支残留**:「放弃工作树」只删目录、分支照旧留着(`worktree_discard`
/// 的返回值自己写着「分支仍保留」),于是同名再建撞上它。老版把 git 的原文
/// `fatal: a branch named '…' already exists` 直接抛给用户 —— 不带任何解法,而 app 里
/// **根本没有删分支的入口**,这个名字就此变成死结。按 D-004 口径,这里必须点名
/// 「是一条已存在的**分支**占着这个名字」(不是目录残留)并给出可执行动作。
///
/// 判「是不是撞名」用的是 `branch_exists` 复查,不是匹配 git 的错误文本 —— git 的输出
/// 会被本地化,匹配字符串在中文 git 上直接失效。
fn branch_claim_error(root: &Path, branch: &str, worktree: &Path, git_error: &str) -> String {
    if branch_exists(root, branch) {
        format!(
            "这个工作树名字被一条已存在的分支占着: {branch}(不是目录残留:{} 并不存在)。\
             本次调用没有创建任何东西,磁盘与 git 状态一个字节都没动。\
             app 里没有删分支的入口,请先确认它上面没有要保留的提交:\
             `git -C \"{}\" log --oneline HEAD..{branch}`;确认可丢弃后执行 \
             `git -C \"{}\" branch -D {branch}` 再重试,想保留就换一个工作树名字",
            worktree.display(),
            git_arg_path(root),
            git_arg_path(root),
        )
    } else {
        format!("认领分支 {branch} 失败,本次建线没有创建任何东西(磁盘与 git 状态未变): {git_error}")
    }
}

/// 本地分支是否已经存在。用 `rev-parse --verify` 走全名 `refs/heads/<branch>`,
/// 不用 `branch --list`(那是 glob 匹配)。
pub fn branch_exists(root: &Path, branch: &str) -> bool {
    let refname = format!("refs/heads/{branch}");
    worktree_command(root, &["rev-parse", "--verify", "--quiet", &refname])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 40/64 位十六进制且非全零才算一个可用作 CAS 判据的对象名。
fn is_object_name(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value.chars().all(|ch| ch.is_ascii_hexdigit())
        && value.chars().any(|ch| ch != '0')
}

/// 解析一个 ref 的对象名;解析不出来(不存在 / git 出错 / 形态不对)返回 None。
///
/// 全零对象名必须挡掉:`git update-ref -d <ref> 000…0` 不是「比较后删除」,
/// 它的语义是另一回事,拿它当判据等于把 CAS 退化成无条件删除。
pub fn rev_parse(root: &Path, refname: &str) -> Option<String> {
    let output = worktree_command(root, &["rev-parse", "--verify", "--quiet", refname]).ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    is_object_name(&value).then_some(value)
}

/// 建工作树(非 Tauri 内核):`worktree_create` 命令与 `create_process` 建线共用。
///
/// **它本身不取工作树元数据闸**:两个调用方都在外层取(`worktree_create` →
/// `create_worktree_arbitrated`,建线 → `create_process`),闸不可重入,在这里再取
/// 一次就是自己等自己。源码 writer lease 与建树无关,正确性也不由锁提供,见下。
///
/// # 并发安全靠 git 的 ref CAS,不靠锁(K2' 返工)
///
/// 老版是 `git worktree add -b <branch> <path> HEAD`:**建分支与建树是同一条命令**,
/// 于是「这条分支是不是我建的」只能靠**调用前采样**的布尔量去猜。采样是过去时——并发
/// 下别人在采样与 add 之间把树和分支建好了,自己的 add 因此失败,清理块就拿着过期判断
/// 去 `worktree remove --force` + 删分支,**删的是赢家刚建好的东西**。上一版拿写租约挡
/// 这个窗口,但协调器是进程内对象,换成两个 OS 进程(kz CLI / 自举循环 / 第二个 kzapp)
/// 破坏原样复现。
///
/// 现在把「认领」与「建树」拆开,并让认领先行:
///
/// 1. `git branch <branch> HEAD` —— **ref 创建是原子的(CAS),已存在即失败**。
///    成功 ⇒ 这条分支是我建的,这个结论**跨进程**成立,由 git 保证;
///    失败 ⇒ 别人(或用户)拥有这个名字 ⇒ **报错返回,零回滚**。
/// 2. `git worktree add <path> <branch>` —— 挂到一条已经归我的分支上。
/// 3. 失败回滚:因为第 1 步成功过,我确知分支归我 ⇒ 按 sha 做 CAS 删除;目录也只可能是
///    本次建出来的(第 0 步的 `worktree.exists()` 预检已排除事前存在)。
///
/// 核心不变量与它的类型层面形态见 [`WorktreeReceipt`]。
///
/// # `git worktree add` 之后的每一步都必须能回滚
///
/// 认领成功之后有两条失败路径,两条都会在磁盘与 git 里留下**界面上没有任何入口能收掉**
/// 的残留,所以两条都得自己收干净:
///
/// 1. **`add` 自己失败**:分支已经落地(第 1 步建的)。不删它,下一次同名建线就撞
///    `a branch named '…' already exists`,**第二次起永久失败**。
/// 2. **`add` 成功、后面的工作区探测失败**:树已经挂上、分支已经落地,用 `?` 直接抛错
///    就留下一棵孤儿树 + 一条孤儿分支。`create_process` 的整体回滚**救不了它** ——
///    那一层只回滚它自己那一步(落库),`create_worktree` 返回 Err 时它认为这里已经
///    什么都没建出来。
pub fn create_worktree(root: &Path, name: &str) -> Result<WorktreeInfo, String> {
    create_worktree_with_receipt(root, name).map(|(info, _)| info)
}

/// [`create_worktree`] 加一张回滚凭据:建线要拿它在落库失败时回滚。
pub fn create_worktree_with_receipt(
    root: &Path,
    name: &str,
) -> Result<(WorktreeInfo, WorktreeReceipt), String> {
    let (worktree, branch) = worktree_target(root, name)?;
    // ⓪ 目录残留预检。在认领之前,所以走到这里一定**零回滚**(还没有凭据)。
    if worktree.exists() {
        return Err(residual_worktree_error(root, &worktree, &branch));
    }
    // ① 原子认领:ref 创建是 CAS,已存在即失败。成功即拥有,跨进程成立。
    let claim = worktree_command(root, &["branch", &branch, "HEAD"])?;
    if !claim.status.success() {
        // 零回滚:第 1 步失败即证明本次调用没有创建出任何东西,一个字节都不许删。
        let failure = String::from_utf8_lossy(&claim.stderr).trim().to_string();
        return Err(branch_claim_error(root, &branch, &worktree, &failure));
    }
    // 认领成功 ⇒ 分支归我 ⇒ 从这里起才有凭据(见 WorktreeReceipt 的不变量)。
    let receipt = WorktreeReceipt {
        branch_sha: rev_parse(root, &format!("refs/heads/{branch}")),
        worktree,
        branch,
    };
    // ② 挂树到已经归我的那条分支上(不再用 `-b`:认领已经在第 1 步做完了)。
    let output = worktree_command(
        root,
        &[
            "worktree",
            "add",
            &git_arg_path(&receipt.worktree),
            &receipt.branch,
        ],
    )?;
    if !output.status.success() {
        let failure = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(with_residue(failure, discard_worktree(root, &receipt)));
    }
    let (files, diff) = match worktree_status(root, &receipt.worktree) {
        Ok(probed) => probed,
        Err(error) => return Err(with_residue(error, rollback_worktree(root, &receipt))),
    };
    Ok((
        WorktreeInfo {
            path: git_arg_path(&receipt.worktree),
            branch: receipt.branch.clone(),
            clean: files.is_empty(),
            files,
            diff,
            // 刚建出来还没绑线;绑定发生在 process_create 里,清单由 worktree_list 合并。
            bound_process: None,
        },
        receipt,
    ))
}

/// 回收一条建到一半的线:摘工作树 + 按凭据删分支。
///
/// `Err(残留说明)` = 有东西没收掉,里面点名了残留路径和可执行的清理动作。
pub fn rollback_worktree(root: &Path, receipt: &WorktreeReceipt) -> Result<(), String> {
    discard_worktree(root, receipt)
}

/// 把失败原因与回滚残留拼成一条错误。
///
/// D-004 口径:回滚收不干净是用户**必须**知道的事(不清理掉,这个工作树名字就一直
/// 用不了),不能像老版那样 `let _ =` 吞掉。
pub fn with_residue(error: String, rollback: Result<(), String>) -> String {
    match rollback {
        Ok(()) => error,
        Err(residue) => format!("{error}\n{residue}"),
    }
}

/// 回滚的实现:先 remove 工作树(分支正被它 checkout,不先摘就删不掉),再按凭据删分支。
///
/// 目录删得下手的依据是 [`WorktreeReceipt`] 的不变量:凭据只在认领成功之后构造,而认领
/// 之前那道 `worktree.exists()` 预检已经排除了「目录事前就在」,所以凭据里的目录必然是
/// 本次调用建出来的。分支那一侧另有 sha CAS 兜底。
///
/// # 为什么不再调 `git worktree prune`
///
/// 老版在 remove 失败的兜底路径上调 `git worktree prune`。prune 是**全仓**操作:
/// 2026-08-11 实测,它会把仓里所有「目录当前不可达」的工作树登记项一并摘掉 ——
/// 包括跟本次目标毫无关系的、用户只是把目录临时挪走或放在未挂载盘上的那些。
/// 而 add 失败路径**必然**走到这个兜底(目标压根没登记成功,`worktree remove` 一定
/// 失败),于是每一次建线失败都在静默地改写全仓的工作树清单。
///
/// 替代品是定点摘除:删目录之前先从 `<worktree>/.git` 读出这棵树自己的管理目录,
/// 目录删掉之后只删那一个。实测 git 在 add 失败时会自己清掉半成品登记项,所以这条
/// 路多数时候无事可做——但它至少不会碰别人。
pub fn discard_worktree(root: &Path, receipt: &WorktreeReceipt) -> Result<(), String> {
    let target = git_arg_path(&receipt.worktree);
    let removed = worktree_command(root, &["worktree", "remove", "--force", &target])
        .map(|output| output.status.success())
        .unwrap_or(false);
    let mut dir_error = None;
    if !removed {
        // 先记下管理目录(读的是还没被删的 `<worktree>/.git`),删完目录再定点摘。
        let admin = worktree_admin_dir(&receipt.worktree);
        if let Err(error) = std::fs::remove_dir_all(&receipt.worktree) {
            if receipt.worktree.exists() {
                dir_error = Some(error.to_string());
            }
        }
        if !receipt.worktree.exists() {
            if let Some(admin) = admin {
                let _ = std::fs::remove_dir_all(admin);
            }
        }
    }

    let mut branch_residue = false;
    if let Some(sha) = receipt.branch_sha.as_deref() {
        if branch_exists(root, &receipt.branch) {
            // 目录还在、且 git 仍把它登记为工作树时,这条分支正被它 checkout:
            // 此时删 ref 只会把残留工作树的 HEAD 打断(实测会变成全零),留着更安全。
            let still_checked_out =
                receipt.worktree.exists() && worktree_is_registered(root, &receipt.worktree);
            if !still_checked_out {
                let refname = format!("refs/heads/{}", receipt.branch);
                let _ = worktree_command(root, &["update-ref", "-d", &refname, sha]);
            }
            branch_residue = branch_exists(root, &receipt.branch);
        }
    }

    if dir_error.is_none() && !branch_residue {
        return Ok(());
    }
    let mut lines =
        vec!["回滚未收干净,下面的残留要手动清理,否则同名建线会一直报「工作树已存在」:".to_string()];
    if let Some(error) = dir_error {
        lines.push(format!(
            "  · 工作树目录仍在: {}(删除失败: {error})",
            receipt.worktree.display()
        ));
        lines.push(format!(
            "    等占用它的程序退出后执行 `git -C \"{}\" worktree remove --force \"{}\"`;\
             该命令报「不是工作树」就直接删掉这个目录",
            git_arg_path(root),
            target
        ));
    }
    if branch_residue {
        lines.push(format!("  · 分支仍在: {}", receipt.branch));
        lines.push(format!(
            "    确认上面没有要保留的提交后执行 `git -C \"{}\" branch -D {}`",
            git_arg_path(root),
            receipt.branch
        ));
    }
    Err(lines.join("\n"))
}

/// 合并命令的可测试内核。写租约由 Tauri 命令在调用前获取；这里保留从路径校验、
/// merge-tree 预检到 `--no-ff` 合并的完整可观察语义。
pub fn merge_worktree(root: &Path, worktree_path: &str) -> Result<String, String> {
    let worktree = validate_worktree_path(root, worktree_path)?;
    let branch = worktree_current_branch(&worktree)?;
    let check = worktree_command(root, &["merge-tree", "--write-tree", "HEAD", &branch])?;
    if !check.status.success() {
        let conflicts = parse_merge_tree_conflicts(&check.stdout);
        return Err(format!(
            "合并前冲突检测失败,双方改动已保留。冲突文件:\n{}\n{}",
            conflicts.join("\n"),
            String::from_utf8_lossy(&check.stdout)
        ));
    }
    let output = worktree_command(root, &["merge", "--no-ff", &branch])?;
    if !output.status.success() {
        return Err(format!(
            "合并未完成,请在主项目中解决并保留工作树:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(format!(
        "已合并工作树分支 {branch};工作树仍保留,可检查后显式放弃"
    ))
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

    fn unique(tag: &str) -> String {
        format!(
            "{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git 执行失败");
        assert!(
            output.status.success(),
            "git {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn git_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(unique(tag));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "test@kanzei.dev"]);
        git(&dir, &["config", "user.name", "kanzei test"]);
        std::fs::write(dir.join("seed.txt"), "seed\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "seed"]);
        dir
    }

    #[test]
    fn 建树回滚同名重建_凭证claim与rollback闭环() {
        // 核心生命周期语义:建树成功 → 凭据有效 → 回滚删干净 → 同名可立即重建。
        // 这是「不留孤儿树与孤儿分支」的可执行形态(K2' 不变量)。
        let root = git_repo("kz-wt-b2");
        let (info, receipt) = create_worktree_with_receipt(&root, "line-a").unwrap();
        assert!(receipt.worktree.is_dir(), "建树后目录必须存在");
        assert!(branch_exists(&root, &receipt.branch), "认领分支必须存在");
        assert!(receipt.branch_sha.is_some(), "凭据必须带认领时 sha");
        assert_eq!(info.branch, receipt.branch);
        assert!(info.clean, "新建树工作区必须干净");

        rollback_worktree(&root, &receipt).unwrap();
        assert!(!receipt.worktree.exists(), "回滚后目录必须删除");
        assert!(!branch_exists(&root, &receipt.branch), "回滚后分支必须删除");

        // 回滚干净 ⇒ 同名立即重建成功(否则名字被残留占死,第二次起永久失败)。
        let (_, receipt2) = create_worktree_with_receipt(&root, "line-a").unwrap();
        rollback_worktree(&root, &receipt2).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn 目录残留预检_零回滚并带解法文案() {
        let root = git_repo("kz-wt-b2b");
        // 先正常建一棵,再同一名字再建:目录已存在 ⇒ 零回滚报错(不删任何东西)。
        let (_, receipt) = create_worktree_with_receipt(&root, "dup").unwrap();
        let err = create_worktree_with_receipt(&root, "dup").unwrap_err();
        assert!(err.contains("工作树已存在"), "{err}");
        assert!(receipt.worktree.exists(), "零回滚:已有树必须原样保留");
        assert!(
            branch_exists(&root, &receipt.branch),
            "零回滚:分支必须原样保留"
        );
        rollback_worktree(&root, &receipt).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }
}
