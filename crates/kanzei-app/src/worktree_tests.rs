//! F4/K2:`process_create` 建线的测试 —— worktree 真实绑定、一树一线查重、
//! 落库失败整体回滚、`project_dir` 恒主根,以及 K2 补的并发/仲裁/残留可见性。
//!
//! R-177 验收⑦ 的落点:`processes.rs` 在 F4 之前**零测试**(既没有 `mod tests`
//! 也没有任何 `#[test]`),这个文件是它的开张。
//!
//! 夹具一律用真 `git init` + 一次真实提交:`git worktree add ... HEAD` 需要 HEAD
//! 有基点,而只伪造磁盘形态(建几个空目录)验不出 git 到底认不认那条路径——F4
//! 修的第一个真问题(`\\?\` 前缀 git 不收)恰恰只有真跑 git 才暴露得出来。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    create_process, create_worktree, create_worktree_arbitrated, create_worktree_with_receipt,
    restore_processes_from_store, rollback_worktree, worktree_status, worktree_target,
    WorktreeReceipt,
};
use crate::state::{ensure_default_process, process_session_id, AppState};

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
    let output = super::hidden_command("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git 无法执行");
    assert!(
        output.status.success(),
        "git {args:?} 失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// 真仓夹具:`git init` + 一个受版本控制的 seed.txt + 一次提交。
///
/// 提交真实文件(而不是 `--allow-empty`)是为了让「未提交改动」那条测试能同时
/// 验到 `files` 与 `diff` 两路都非空——空仓里改不出 diff。
fn git_repo(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(unique(tag));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "."]);
    git(&root, &["config", "user.email", "f4@kanzei.test"]);
    git(&root, &["config", "user.name", "f4"]);
    std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
    git(&root, &["add", "seed.txt"]);
    git(&root, &["commit", "-qm", "init"]);
    root
}

fn worktree_registry(root: &Path) -> String {
    git(root, &["worktree", "list", "--porcelain"])
}

/// git 的工作树清单里有没有这条路径(按 `worktree_key` 归一比,避开大小写/分隔符差异)。
fn registry_has(root: &Path, worktree: &Path) -> bool {
    let key = super::worktree_key(worktree);
    worktree_registry(root)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .any(|path| super::worktree_key(Path::new(path.trim())) == key)
}

fn branch_exists(root: &Path, branch: &str) -> bool {
    !git(root, &["branch", "--list", branch]).trim().is_empty()
}

/// 测试里手工收尾用的凭据:分支停在哪儿现取,语义等价于「刚建出来还没动过」。
fn rollback_receipt(root: &Path, worktree: &Path, branch: &str) -> WorktreeReceipt {
    let sha = git(root, &["rev-parse", "--verify", "--quiet", branch])
        .trim()
        .to_string();
    WorktreeReceipt {
        worktree: worktree.to_path_buf(),
        branch: branch.to_string(),
        branch_sha: (!sha.is_empty()).then_some(sha),
    }
}

fn cleanup(root: &Path, worktrees: &[PathBuf]) {
    for worktree in worktrees {
        let _ = std::fs::remove_dir_all(worktree);
    }
    let _ = std::fs::remove_dir_all(root);
}

/// R-177 验收① 前半:`worktree_path` 不再恒 `None`,而是一条真实存在的路径,
/// 分支是 `kanzei/thread-<name>`;同时钉住 `project_dir` 恒主根这条口径。
#[tokio::test]
async fn 建线后worktree_path是真实路径() {
    let root = git_repo("kz-f4-bind");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("bind");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    let state = AppState::default();
    let info = create_process(
        &state,
        &root.display().to_string(),
        None,
        None,
        None,
        None,
        Some(name.clone()),
    )
    .await
    .unwrap();

    let bound = info
        .worktree_path
        .clone()
        .expect("建线必须给出真实 worktree_path,不能还是 None");
    assert!(
        Path::new(&bound).is_dir(),
        "worktree_path 必须指向真实存在的目录: {bound}"
    );
    assert_eq!(
        git(Path::new(&bound), &["branch", "--show-current"]).trim(),
        branch,
        "建线的分支必须是 kanzei/thread-<name>"
    );
    // 恒主根:worktree 路径只由 worktree_path 承担,不许渗进 project_dir。
    assert_eq!(info.project_dir, canonical.display().to_string());
    assert_eq!(info.origin_project, canonical.display().to_string());
    assert_ne!(info.project_dir, bound);

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// R-177 验收⑤ / D4 定案:同一棵工作树不得同时绑两条线,且**查重先于建树** ——
/// 拒绝的时候磁盘上不许多出任何东西。
#[tokio::test]
async fn 同一worktree不得绑定第二条线() {
    let root = git_repo("kz-f4-dup");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("dup");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    let state = AppState::default();
    let first = create_process(
        &state,
        &root.display().to_string(),
        None,
        None,
        None,
        None,
        Some(name.clone()),
    )
    .await
    .unwrap();

    let before = worktree_registry(&canonical);
    let error = create_process(
        &state,
        &root.display().to_string(),
        None,
        None,
        None,
        None,
        Some(name.clone()),
    )
    .await
    .expect_err("同一棵工作树不得绑定第二条线");
    assert!(
        error.contains(&first.id),
        "拒绝文案必须点名已经绑着它的那条线: {error}"
    );
    assert_eq!(
        worktree_registry(&canonical),
        before,
        "查重必须发生在建树之前:被拒时 git 侧一棵树都不许多出来"
    );

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// R-177 验收① 后半:任一步失败整体回滚,不留半绑定态。
///
/// 用「把 state.db 的位置占成目录」制造落库失败:`SessionStore::open` 拿目录当
/// 数据库必然报错,且这是纯文件系统手段,不依赖权限位在 CI 与本机的差异。
#[tokio::test]
async fn 落库失败时worktree被回收_不留半绑定态() {
    let root = git_repo("kz-f4-rollback");
    let canonical = crate::normalized_project_root(&root);
    std::fs::create_dir_all(kanzei_core::project_state_path(&canonical)).unwrap();
    let name = unique("rollback");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    let state = AppState::default();
    let error = create_process(
        &state,
        &root.display().to_string(),
        None,
        None,
        None,
        None,
        Some(name.clone()),
    )
    .await
    .expect_err("落库失败必须让整次建线失败");
    // 失败必须来自库(state.db),不是来自 git —— 否则这条测试验的就不是回滚路径了。
    assert!(
        error.contains("state.db"),
        "这次失败应当来自落库而不是建树: {error}"
    );

    assert!(!target.exists(), "回滚必须删掉已经建出来的 worktree 目录");
    assert!(
        !branch_exists(&canonical, &branch),
        "回滚必须删掉已经建出来的分支 {branch}"
    );
    let expected_id = format!("p1|{}", canonical.display());
    let processes = state.processes.lock().unwrap();
    assert!(
        processes.get(&expected_id).is_none(),
        "内存进程表不得留下这条线"
    );
    assert!(
        processes.values().all(|p| p.worktree_path.is_none()),
        "内存进程表不得留下任何半绑定的线"
    );
    drop(processes);

    cleanup(&root, &[target]);
}

/// 建线返回的 `clean`/`files`/`diff` 必须是真实探测,不是硬编码乐观值。
///
/// 两个方向一起验:刚建出来的树确实干净;同一个探测函数(`create_worktree` 用的
/// 就是它)在有未提交改动时确实报 files 与 diff 非空。以前这三个字段是写死的
/// 空/true/空,收活流程会把「线还有活没提交」当成干净合并。
#[test]
fn worktree_create返回的clean反映真实工作区() {
    let root = git_repo("kz-f4-clean");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("clean");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    let fresh = create_worktree(&canonical, &name).unwrap();
    assert_eq!(fresh.branch, branch);
    assert!(fresh.clean, "刚建出的工作树必须报干净");
    assert!(fresh.files.is_empty());
    assert!(fresh.diff.is_empty());

    std::fs::write(Path::new(&fresh.path).join("seed.txt"), "线上改过了\n").unwrap();
    let (files, diff) = worktree_status(&canonical, Path::new(&fresh.path)).unwrap();
    assert!(!files.is_empty(), "未提交改动必须出现在 files 里");
    assert!(
        diff.contains("seed.txt"),
        "未提交改动必须出现在 diff 里: {diff}"
    );

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// 不传 `worktree_name` 时行为与今天逐字节一致:`worktree_path` 恒 `None`、
/// `p{n}` 递增规则不变、`project_dir` 是主根。
#[tokio::test]
async fn 不带worktree_name时行为与今天一致() {
    let root = git_repo("kz-f4-plain");
    let canonical = crate::normalized_project_root(&root);
    let state = AppState::default();

    let first = create_process(
        &state,
        &root.display().to_string(),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let second = create_process(
        &state,
        &root.display().to_string(),
        Some("deepseek:deepseek-v4-flash".into()),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(first.worktree_path, None);
    assert_eq!(second.worktree_path, None);
    assert_eq!(first.id, format!("p1|{}", canonical.display()));
    assert_eq!(second.id, format!("p2|{}", canonical.display()));
    assert_eq!(first.project_dir, canonical.display().to_string());
    assert_eq!(second.model.as_deref(), Some("deepseek:deepseek-v4-flash"));
    assert!(
        !first.phase_pipeline,
        "「勘察复核」缺省仍是关(与 process_tests.rs 的默认值测试同一条口径)"
    );
    assert_eq!(
        worktree_registry(&canonical).matches("worktree ").count(),
        1,
        "不建线时不得有任何多余工作树"
    );

    cleanup(&root, &[]);
}

// ── 以下四条补的是 F4 首版漏掉的两条失败路径(建树成功之后 / 建树自己失败),
//    以及「回滚只许删自己建的分支」这个反方向的问题。夹具全部是确定性的 git 配置
//    注入,不依赖权限位、磁盘满、路径长度这些跟环境走的东西。

/// 致命 1:`git worktree add` **成功之后**的失败必须整体回滚。
///
/// 夹具:`diff.algorithm = bogus`。2026-08-11 实测这一条让 `status --porcelain` 与
/// `diff` 双双 exit 128,而 `worktree add` 照常成功(exit 0)—— 正好把执行停在
/// 「树已经挂上、分支已经落地、工作区探测失败」这一点上,也就是首版用 `?` 直接抛
/// 出去的那一行。抛出去的后果是一棵孤儿树 + 一条孤儿分支:界面上没有任何入口能
/// 看见它们,`create_process` 的回滚也够不着(它认为这一层什么都没建出来)。
#[test]
fn 建树成功后探测失败必须整体回滚_不留孤儿树与孤儿分支() {
    let root = git_repo("kz-f4-probefail");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("probefail");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    // K2:先种一棵目录不可达的旧工作树,让下面「清单不变」的断言真的能证伪 prune。
    let bystander = park_bystander_worktree(&root, &canonical);
    let before = worktree_registry(&canonical);

    git(&root, &["config", "diff.algorithm", "bogus"]);
    let error = create_worktree(&canonical, &name).expect_err("探测失败必须让整次建树失败");
    assert!(
        error.contains("diff.algorithm"),
        "这次失败应当来自工作区探测,不是来自 add: {error}"
    );

    assert!(!target.exists(), "回滚必须删掉已经挂上的 worktree 目录");
    assert!(
        !branch_exists(&canonical, &branch),
        "回滚必须删掉已经落地的分支 {branch}"
    );
    assert_eq!(
        worktree_registry(&canonical),
        before,
        "回滚后 git 的工作树清单必须与调用前逐字节相同(含那棵目录不可达的旁观者)"
    );

    // 验收②:这次失败之后同名重建能成功(不是被自己留下的残留永久堵死)。
    git(&root, &["config", "--unset", "diff.algorithm"]);
    let rebuilt = create_worktree(&canonical, &name).expect("同名重建必须能成功");
    assert_eq!(rebuilt.branch, branch);
    assert!(Path::new(&rebuilt.path).is_dir());

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target, bystander]);
}

/// 致命 2:`git worktree add` **自己失败**时不得留下孤儿分支。
///
/// 夹具:`checkout.workers = bogus`。2026-08-11 实测 `git worktree add -b` 是**先建
/// 分支再挂树**的,这条注入让它在建完分支之后的检出这一步 exit 128 —— 与「目标是
/// 非空目录 / 目标是文件 / 前置目录建不出 / 路径超长」这四种真实失败模式实测到的
/// 结果一致:分支统统留在原地。首版的清理块只 `remove_dir_all` + `prune`,于是
/// 同名建线**第二次起永久失败**(`a branch named '…' already exists`),而 app 内
/// 没有任何入口能删掉那条分支。
#[test]
fn 建树失败不得留下孤儿分支_同名可立即重建() {
    let root = git_repo("kz-f4-addfail");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("addfail");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    let bystander = park_bystander_worktree(&root, &canonical);
    let before = worktree_registry(&canonical);

    git(&root, &["config", "checkout.workers", "bogus"]);
    let error = create_worktree(&canonical, &name).expect_err("建树必须失败");
    assert!(
        error.contains("checkout.workers"),
        "这次失败应当来自 add 本身: {error}"
    );

    assert!(!target.exists(), "建树失败必须回收已经建出来的目录");
    assert!(
        !branch_exists(&canonical, &branch),
        "建树失败不得留下孤儿分支 {branch}"
    );
    assert_eq!(
        worktree_registry(&canonical),
        before,
        "建树失败后 git 的工作树清单必须与调用前逐字节相同"
    );

    // 验收②:这才是这条致命的真正后果所在——不删分支的话这一步永远失败。
    git(&root, &["config", "--unset", "checkout.workers"]);
    let rebuilt = create_worktree(&canonical, &name).expect("同名重建必须能成功");
    assert_eq!(rebuilt.branch, branch);

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target, bystander]);
}

/// 反方向:回滚只许删**本次调用建出来**的分支。
///
/// 「add 失败了就 `branch -D`」是错的——要问的不是「我该删哪条分支」,而是「这条
/// 分支是不是我建的」。同名分支若在调用前就存在,`add -b` 必然因它而失败,那条分支
/// 是用户的东西(可能挂着还没合并的活),删掉就是丢数据。
#[test]
fn 回滚不得删掉调用前就存在的同名分支() {
    let root = git_repo("kz-f4-keepbranch");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("keepbranch");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    // 用户自己先有一条同名分支,上面挂着一个提交。
    git(&root, &["branch", &branch]);
    let head = git(&root, &["rev-parse", &branch]).trim().to_string();

    let error = create_worktree(&canonical, &name).expect_err("同名分支已存在时建树必然失败");
    assert!(
        error.contains("already exists"),
        "这次失败应当来自分支撞名: {error}"
    );
    assert!(
        branch_exists(&canonical, &branch),
        "回滚只许删本次建出来的分支,不许碰调用前就存在的 {branch}"
    );
    assert_eq!(
        git(&root, &["rev-parse", &branch]).trim(),
        head,
        "那条分支必须原封不动"
    );
    assert!(!target.exists(), "失败仍然不许在磁盘上留下目录");

    git(&root, &["branch", "-D", &branch]);
    cleanup(&root, &[target]);
}

/// 复核点:`project_dir` 是不是**真的**恒主根?
///
/// 这是 §0 定案 2 的前提——不恒主根,`#p{n}` 的唯一性就不成立,session_id 降级的
/// 理由随之失效。全仓 `ProcessHandle` 的字面量构造点(production 侧)恰 3 处:
/// `state.rs` 的 `ensure_default_process` 1 处、`processes.rs` 的
/// `restore_processes_from_store` 与 `register_process` 各 1 处。这条测试
/// **先用源码把「恰 3 处」钉死**(多出第 4 个构造点就红,逼作者在那里也证明这条
/// 口径),**再逐处验行为**。三处合起来才是穷尽:
///
/// - `ensure_default_process` 取调用方给的 root —— 验它原样落进 `project_dir`;
/// - `register_process` 取 `normalized_project_root` —— 验带 worktree 时也不渗进去;
/// - `restore_processes_from_store` 取库值,而库值只由建线/`persist_process` 从上面
///   两种 handle 写出 —— 验一次真实的落库/回读往返之后仍是主根(不动点)。
#[tokio::test]
async fn project_dir恒主根_三个构造点逐处成立() {
    let sources = [
        include_str!("processes.rs"),
        include_str!("state.rs"),
        // 未来若新增构造点所在的文件,加进来一起数。
    ];
    // 只数**表达式位置**的 `ProcessHandle {`:类型定义(`struct ProcessHandle {`)与
    // 返回类型后面跟函数体大括号(`-> ProcessHandle {`)长得一样但不是构造点。
    let sites: usize = sources
        .iter()
        .map(|source| {
            source
                .match_indices("ProcessHandle {")
                .filter(|(at, _)| {
                    let head = &source[..*at];
                    !head.ends_with("struct ") && !head.ends_with("-> ")
                })
                .count()
        })
        .sum();
    assert_eq!(
        sites, 3,
        "ProcessHandle 的构造点变了({sites} 处);新构造点必须同样保证 project_dir 恒主根,\
         证明完再改这个数字"
    );

    let root = git_repo("kz-f4-mainroot");
    let canonical = crate::normalized_project_root(&root);
    let expected = canonical.display().to_string();
    let name = unique("mainroot");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    let state = AppState::default();

    // 构造点 1:默认进程。
    let default = ensure_default_process(&state, &canonical);
    assert_eq!(default.project_dir, expected);
    assert_eq!(default.worktree_path, None);

    // 构造点 2:建线(带 worktree —— 这是唯一可能把树路径渗进 project_dir 的入口)。
    let info = create_process(
        &state,
        &root.display().to_string(),
        None,
        None,
        None,
        None,
        Some(name.clone()),
    )
    .await
    .unwrap();
    assert_eq!(info.project_dir, expected);
    assert_eq!(info.origin_project, expected);
    assert_ne!(info.worktree_path.as_deref(), Some(expected.as_str()));

    // 构造点 3:从 state.db 回读重建(库值是上一步落库出去的,验它是不动点)。
    state.processes.lock().unwrap().clear();
    restore_processes_from_store(&state, &canonical).unwrap();
    let processes = state.processes.lock().unwrap();
    let restored = processes.get(&info.id).expect("这条线必须能从库里恢复出来");
    assert_eq!(restored.project_dir, expected, "回读之后仍须是主根");
    assert_eq!(restored.origin_project, expected);
    assert_eq!(restored.worktree_path, info.worktree_path);
    for process in processes.values() {
        assert_eq!(
            process.project_dir, expected,
            "内存表里每一条线的 project_dir 都必须是主根"
        );
    }
    drop(processes);

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// D-176 红线:线的 `session_id` 只由**主根** + `p{n}` 决定,worktree 不参与。
///
/// 这正是 §0 定案 2(不给 session_id 加 worktree 后缀)的前提:`project_dir` 恒
/// 主根 ⇒ `#p{n}` 在项目内已唯一,后缀带不来唯一性收益,却会让既有会话历史失联。
#[tokio::test]
async fn 线的session_id仍由主根算出() {
    let root = git_repo("kz-f4-session");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("session");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    let state = AppState::default();
    let info = create_process(
        &state,
        &root.display().to_string(),
        None,
        None,
        None,
        None,
        Some(name.clone()),
    )
    .await
    .unwrap();
    assert!(info.worktree_path.is_some(), "这条线确实绑了工作树");
    assert_eq!(
        info.session_id,
        process_session_id(&canonical, Some(&info.id)),
        "session_id 必须由主根算出"
    );
    assert_eq!(
        info.session_id,
        format!("{}#p1", kanzei_core::project_session_id(&canonical)),
        "身份串形态不变:主根会话 id + #p{{n}},没有 worktree 后缀"
    );

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

// ══ K2 返工:并发下的回滚曾经是破坏动作 ══════════════════════════════════════

/// 在仓里留下一棵**目录已经不可达**的工作树,当 `git worktree prune` 的对照物。
///
/// 返回被挪走的目录,交给 `cleanup` 收尾。
fn park_bystander_worktree(repo: &Path, canonical: &Path) -> PathBuf {
    let live = repo.parent().unwrap().join(unique("kz-bystander"));
    let parked = repo.parent().unwrap().join(unique("kz-bystander-moved"));
    git(
        canonical,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "kanzei/bystander",
            &live.display().to_string(),
            "HEAD",
        ],
    );
    std::fs::rename(&live, &parked).expect("把旁观者工作树的目录挪走");
    assert!(
        worktree_registry(canonical).contains("prunable"),
        "旁观者必须以 prunable 的形态留在清单里,否则这个对照物没意义"
    );
    parked
}

/// 验收①:两个调用者同时建**同名**工作树,赢的一方的树与分支必须完好无损。
///
/// 这是 K2 返工的正主。批次 D 把回滚从「删目录」升级成「删目录 + `branch -D`」,
/// 而「这条分支是不是我建的」是一个**调用前采样的布尔量** + 无锁 check-then-act:
/// 建线走 `state.processes` 的 Mutex、`worktree_create` 走协调器的写租约,两把锁
/// 互不可见,可以真并发。输的一方的 `add` 因为赢家已经建好而失败,清理块于是拿着
/// 过期的判断 `worktree remove --force` + `branch -D` 掉**赢家刚建好的树和分支**。
///
/// 修法是让两条路进同一个仲裁(写租约)。这条测试把两条路混在一起打:一半走建线、
/// 一半走 `worktree_create` 的内核,同一个名字,同一个 `AppState`(⇒ 同一个协调器)。
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn 并发建同名树_赢家的树与分支必须完好无损() {
    let root = git_repo("kz-k2-race");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("race");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    let before = worktree_registry(&canonical).matches("worktree ").count();

    let state = Arc::new(AppState::default());
    let project = root.display().to_string();
    let mut racers = Vec::new();
    for index in 0..8 {
        let state = Arc::clone(&state);
        let project = project.clone();
        let name = name.clone();
        let canonical = canonical.clone();
        racers.push(tokio::spawn(async move {
            if index % 2 == 0 {
                create_process(&state, &project, None, None, None, None, Some(name))
                    .await
                    .map(|info| info.worktree_path.unwrap_or_default())
            } else {
                create_worktree_arbitrated(&state, &canonical, &name)
                    .await
                    .map(|info| info.path)
            }
        }));
    }
    let mut results = Vec::new();
    for racer in racers {
        results.push(racer.await.expect("建线任务不许 panic"));
    }

    let winners = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(winners, 1, "并发建同一棵工作树只许一条胜出: {results:?}");

    // ——赢家的东西必须一个字节都没被输家动过(这才是这条致命的真正后果所在,
    //   所以它排在「落败者的错误文案」前面:先证明没被破坏,再挑剔措辞)。
    assert!(
        target.is_dir(),
        "赢家的工作树目录必须还在: {}",
        target.display()
    );
    assert_eq!(
        git(&target, &["branch", "--show-current"]).trim(),
        branch,
        "赢家的工作树必须仍然 checkout 在自己的分支上"
    );
    assert!(
        branch_exists(&canonical, &branch),
        "赢家的分支必须还在: {branch}"
    );
    assert!(
        git(&target, &["rev-parse", "HEAD"]).trim().len() >= 40,
        "赢家工作树的 HEAD 必须仍然解析得出(被 update-ref 打断会变成全零)"
    );
    assert!(
        registry_has(&canonical, &target),
        "赢家的工作树必须仍在 git 的清单里:\n{}",
        worktree_registry(&canonical)
    );
    assert_eq!(
        worktree_registry(&canonical).matches("worktree ").count(),
        before + 1,
        "磁盘上只许多出一棵工作树"
    );

    for error in results.iter().filter_map(|result| result.as_ref().err()) {
        assert!(
            error.contains("已绑定到线") || error.contains("工作树已存在"),
            "落败者必须拿到明确的「这棵树已经有主了」,不是 git 的原始报错: {error}"
        );
    }

    let bound = state
        .processes
        .lock()
        .unwrap()
        .values()
        .filter(|process| process.worktree_path.is_some())
        .count();
    assert!(bound <= 1, "内存表里绑着这棵树的线最多一条");

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// 反方向的双保险:分支**动过**(不再停在建出来时那个 sha)就不许删。
///
/// 采样出来的「这条分支是我建的」是过去时,并发下可能已经过期。凭据里记的 sha 把
/// 身份钉死:`git update-ref -d <ref> <sha>` 是原子比较后删除,ref 不在这个 sha 上
/// 就删不动。老版无条件 `branch -D`,这条测试在它上面必红。
#[test]
fn 回滚不得删掉sha已经变了的同名分支() {
    let root = git_repo("kz-k2-sha");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("sha");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    let (_info, receipt) = create_worktree_with_receipt(&canonical, &name).unwrap();
    let built_at = receipt.branch_sha.clone().expect("凭据必须记下分支的 sha");

    // 线上干了活并提交:分支从 built_at 挪走了。
    std::fs::write(target.join("work.txt"), "线上的活\n").unwrap();
    git(&target, &["add", "work.txt"]);
    git(&target, &["commit", "-qm", "线上的活"]);
    let moved_to = git(&canonical, &["rev-parse", &branch]).trim().to_string();
    assert_ne!(moved_to, built_at, "前提:分支确实挪走了");

    let residue =
        rollback_worktree(&canonical, &receipt).expect_err("分支删不掉必须报出来,不许静默吞掉");
    assert!(
        branch_exists(&canonical, &branch),
        "分支已经不在建出来时那个 sha 上了,回滚不许删它(上面挂着没合并的提交)"
    );
    assert_eq!(
        git(&canonical, &["rev-parse", &branch]).trim(),
        moved_to,
        "那条分支必须原封不动"
    );
    assert!(
        residue.contains(&branch),
        "残留说明必须点名这条分支: {residue}"
    );

    git(&canonical, &["branch", "-D", &branch]);
    cleanup(&root, &[target]);
}

/// 验收②:`add` 失败的回滚不得把仓里**别的**缺失工作树 prune 掉。
///
/// `git worktree prune` 是全仓操作。老版的兜底路径必然执行到它(目标压根没登记成功,
/// `worktree remove` 一定失败),于是每次建线失败都在静默改写全仓的工作树清单——
/// 把用户只是临时挪走目录、或放在未挂载盘上的那些树,从清单里摘掉。
#[test]
fn add失败不得prune掉仓里别的缺失工作树() {
    let root = git_repo("kz-k2-prune");
    let canonical = crate::normalized_project_root(&root);
    let bystander = park_bystander_worktree(&root, &canonical);
    let before = worktree_registry(&canonical);
    assert!(before.contains("prunable"), "前提:仓里有一棵目录不可达的树");

    let name = unique("prune");
    git(&root, &["config", "checkout.workers", "bogus"]);
    create_worktree(&canonical, &name).expect_err("建树必须失败,才能走到回滚的兜底路径");

    let after = worktree_registry(&canonical);
    assert_eq!(
        after, before,
        "回滚只许处置本次目标;仓里别的缺失工作树必须原样留在清单里\n前:{before}\n后:{after}"
    );
    assert!(
        after.contains("prunable"),
        "那棵目录不可达的旁观者必须原样留在清单里(prune 跑过就没了): {after}"
    );

    git(&root, &["config", "--unset", "checkout.workers"]);
    cleanup(&root, &[bystander]);
}

/// 验收③:目录清理失败必须**可见**,且名字不会永久中毒。
///
/// 老版回滚删目录是 best-effort + `let _ =` 吞错。一旦删不掉(Windows 上有程序占着
/// 里面的文件是家常便饭),`create_worktree` 开头的 `worktree.exists()` 预检会让这个
/// 名字**永久**返回「工作树已存在」,而用户既不知道发生了什么,app 里也没有清理入口。
///
/// 夹具:用 `share_mode(0)` 独占打开工作树里的一个文件。实测这会让
/// `git worktree remove --force` 与 `remove_dir_all` 双双失败(Windows 不允许删除
/// 被独占打开的文件,也不允许改名它的父目录)——这正是真实世界里最常见的那种失败。
#[cfg(windows)]
#[test]
fn 目录清理失败必须点名残留路径_且名字不会永久中毒() {
    use std::os::windows::fs::OpenOptionsExt;

    let root = git_repo("kz-k2-residue");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("residue");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    let (_info, receipt) = create_worktree_with_receipt(&canonical, &name).unwrap();
    // FILE_SHARE 全关:谁也删不掉它,连 git 自己都删不掉。
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(target.join("seed.txt"))
        .expect("独占打开工作树里的文件");

    let residue = rollback_worktree(&canonical, &receipt).expect_err("清理失败必须报出来");
    assert!(
        residue.contains(&target.display().to_string()),
        "错误里必须点名残留的工作树路径: {residue}"
    );
    assert!(
        residue.contains("worktree remove --force"),
        "错误里必须给出可执行的清理动作: {residue}"
    );
    assert!(target.exists(), "前提:这次目录确实没删掉");

    // 名字此刻被残留挡着——但挡人的那条错误同样得自带解法,不能只说「已存在」。
    let blocked = create_worktree(&canonical, &name).expect_err("残留还在时同名建线必然失败");
    assert!(
        blocked.contains(&target.display().to_string())
            && blocked.contains("worktree remove --force"),
        "「工作树已存在」必须点名路径并给出清理动作,否则这个名字就成了死结: {blocked}"
    );

    // 占用消失后,按错误里说的做一遍就能恢复:名字不是永久中毒。
    drop(lock);
    std::fs::remove_dir_all(&target).expect("占用消失后目录可删");
    let rebuilt = create_worktree(&canonical, &name).expect("清理之后同名建线必须能成功");
    assert_eq!(rebuilt.branch, branch);

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// 验收④:建线走的是**和 `worktree_create` 同一个**写仲裁。
///
/// 机械判据不是「代码里有 acquire_writer_lease 这一行」,而是:项目上已经有人持有
/// 写租约时,建线**必须排队**;租约一放,它立刻拿到。这条测试同时把
/// `create_worktree_arbitrated` 放进同一个断言里——两条路排的是同一个队。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 建线与worktree_create排同一个写租约() {
    use kanzei_harness::orchestration::{ProjectExecutionCoordinator, WriterLeaseRequest};

    let root = git_repo("kz-k2-lease");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("lease");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    let state = Arc::new(AppState::default());
    // 项目上先有一个写者(等价于主对话正在写),建线必须排在它后面。
    let holder = state
        .coordinator
        .acquire_writer_lease(WriterLeaseRequest {
            project_root: canonical.clone(),
            run_id: "k2-holder".into(),
            process_id: "k2".into(),
            reason: "占位写者".into(),
        })
        .await
        .unwrap();

    let task = {
        let state = Arc::clone(&state);
        let project = root.display().to_string();
        let name = name.clone();
        tokio::spawn(async move {
            create_process(&state, &project, None, None, None, None, Some(name)).await
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !task.is_finished(),
        "有人持有项目写租约时,建线必须排队——不排队就说明它绕过了仲裁"
    );
    assert_eq!(
        state.coordinator.snapshot(&canonical).waiting_writers.len(),
        1,
        "排队者必须出现在同一个协调器的快照里(同一个仲裁入口的机械证据)"
    );
    assert!(!target.exists(), "排队期间一棵树都不许建出来");

    drop(holder);
    let info = tokio::time::timeout(std::time::Duration::from_secs(30), task)
        .await
        .expect("租约释放后建线应立刻被唤醒")
        .expect("建线任务不许 panic")
        .expect("建线本身必须成功");
    assert!(info.worktree_path.is_some());
    assert!(target.is_dir());

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// 验收⑤:编号分配要看库,且**绝不覆盖**库里既有行。
///
/// 老版只从内存表算 `max(p{n}) + 1`。重启后内存表是空的而库里还留着上次的线,于是
/// 新线跟旧线重号,`upsert_process` 的 `ON CONFLICT DO UPDATE` 把旧行连
/// `worktree_path` 一起改写——旧线绑的那棵树从此在库里失联。
#[tokio::test]
async fn 编号看库_不覆盖库里既有行的worktree_path() {
    let root = git_repo("kz-k2-number");
    let canonical = crate::normalized_project_root(&root);
    let project = canonical.display().to_string();
    let state_path = kanzei_core::project_state_path(&canonical);
    let store = kanzei_core::SessionStore::open(&state_path).unwrap();

    // 库里已有 p1(绑着一棵树)与 p2,而内存表是空的——正是重启后的形态。
    let old_tree = "C:/somewhere/.kanzei-worktree-old".to_string();
    for (id, worktree) in [("p1", Some(old_tree.clone())), ("p2", None)] {
        store
            .insert_new_process(&kanzei_core::StoredProcess {
                process_id: format!("{id}|{project}"),
                origin_project: project.clone(),
                project_dir: project.clone(),
                worktree_path: worktree,
                model: Some("deepseek:deepseek-v4-flash".into()),
                profile: None,
                reasoning: None,
                phase_pipeline: true,
                updated_at: 1,
            })
            .unwrap();
    }

    let state = AppState::default();
    assert!(
        state.processes.lock().unwrap().is_empty(),
        "前提:内存进程表是空的"
    );
    let info = create_process(
        &state,
        &root.display().to_string(),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        info.id,
        format!("p3|{project}"),
        "编号必须越过库里已有的 p1/p2"
    );

    let p1 = store
        .get_process(&format!("p1|{project}"))
        .unwrap()
        .expect("库里的 p1 必须还在");
    assert_eq!(
        p1.worktree_path.as_deref(),
        Some(old_tree.as_str()),
        "库里旧线的 worktree_path 一个字节都不许被新线改写"
    );
    assert!(p1.phase_pipeline, "旧线的其它字段同样不许被动");
    assert_eq!(p1.model.as_deref(), Some("deepseek:deepseek-v4-flash"));

    // Windows 上 state.db 的连接还开着就删不掉目录,夹具会残留。
    drop(store);
    cleanup(&root, &[]);
}

/// 次要②:同一个父目录下的两个项目,用同名工作树不得撞同一条路径。
///
/// 老版路径是 `root.parent().join(".kanzei-worktree-<name>")` —— 不含项目名。
#[test]
fn 同父目录下两个项目的同名工作树不撞路径() {
    let parent = std::env::temp_dir().join(unique("kz-k2-two"));
    std::fs::create_dir_all(&parent).unwrap();
    let mut targets = Vec::new();
    for project in ["alpha", "beta"] {
        let root = parent.join(project);
        std::fs::create_dir_all(&root).unwrap();
        let (target, _) = worktree_target(&root, "dev").unwrap();
        targets.push(target);
    }
    assert_ne!(
        targets[0], targets[1],
        "同父目录下两个项目的同名工作树必须落在不同路径: {targets:?}"
    );
    assert!(targets[0].display().to_string().contains("alpha"));
    assert!(targets[1].display().to_string().contains("beta"));
    let _ = std::fs::remove_dir_all(&parent);
}
