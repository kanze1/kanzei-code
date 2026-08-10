//! F4:`process_create` 建线的测试 —— worktree 真实绑定、一树一线查重、
//! 落库失败整体回滚、`project_dir` 恒主根。
//!
//! R-177 验收⑦ 的落点:`processes.rs` 在本批之前**零测试**(既没有 `mod tests`
//! 也没有任何 `#[test]`),这个文件是它的开张。
//!
//! 夹具一律用真 `git init` + 一次真实提交:`git worktree add ... HEAD` 需要 HEAD
//! 有基点,而只伪造磁盘形态(建几个空目录)验不出 git 到底认不认那条路径——本批
//! 修的第一个真问题(`\\?\` 前缀 git 不收)恰恰只有真跑 git 才暴露得出来。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    create_process, create_worktree, restore_processes_from_store, rollback_worktree,
    worktree_status, worktree_target,
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

fn branch_exists(root: &Path, branch: &str) -> bool {
    !git(root, &["branch", "--list", branch]).trim().is_empty()
}

fn cleanup(root: &Path, worktrees: &[PathBuf]) {
    for worktree in worktrees {
        let _ = std::fs::remove_dir_all(worktree);
    }
    let _ = std::fs::remove_dir_all(root);
}

/// R-177 验收① 前半:`worktree_path` 不再恒 `None`,而是一条真实存在的路径,
/// 分支是 `kanzei/thread-<name>`;同时钉住 `project_dir` 恒主根这条口径。
#[test]
fn 建线后worktree_path是真实路径() {
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

    rollback_worktree(&canonical, &target, &branch);
    cleanup(&root, &[target]);
}

/// R-177 验收⑤ / D4 定案:同一棵工作树不得同时绑两条线,且**查重先于建树** ——
/// 拒绝的时候磁盘上不许多出任何东西。
#[test]
fn 同一worktree不得绑定第二条线() {
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

    rollback_worktree(&canonical, &target, &branch);
    cleanup(&root, &[target]);
}

/// R-177 验收① 后半:任一步失败整体回滚,不留半绑定态。
///
/// 用「把 state.db 的位置占成目录」制造落库失败:`SessionStore::open` 拿目录当
/// 数据库必然报错,且这是纯文件系统手段,不依赖权限位在 CI 与本机的差异。
#[test]
fn 落库失败时worktree被回收_不留半绑定态() {
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

    rollback_worktree(&canonical, &target, &branch);
    cleanup(&root, &[target]);
}

/// 不传 `worktree_name` 时行为与今天逐字节一致:`worktree_path` 恒 `None`、
/// `p{n}` 递增规则不变、`project_dir` 是主根。
#[test]
fn 不带worktree_name时行为与今天一致() {
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
        "回滚后 git 的工作树清单必须与调用前逐字节相同"
    );

    // 验收②:这次失败之后同名重建能成功(不是被自己留下的残留永久堵死)。
    git(&root, &["config", "--unset", "diff.algorithm"]);
    let rebuilt = create_worktree(&canonical, &name).expect("同名重建必须能成功");
    assert_eq!(rebuilt.branch, branch);
    assert!(Path::new(&rebuilt.path).is_dir());

    rollback_worktree(&canonical, &target, &branch);
    cleanup(&root, &[target]);
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

    rollback_worktree(&canonical, &target, &branch);
    cleanup(&root, &[target]);
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

/// 复核点:一树一线查重是不是 check-then-act?
///
/// 不是。查重、建树、插内存表三件事同处 `state.processes` 的同一个 `MutexGuard`
/// 之内,所以并发调用者里**恰好一个**能通过查重。这条测试是那句话的可执行形态:
/// 8 个线程同时拿同一个 `worktree_name` 建线,断言成功恰 1 次、磁盘上恰多出一棵树、
/// 内存表里绑着它的线恰 1 条。若哪天有人把 `create_worktree` 挪到锁外「优化」掉那次
/// 阻塞 git 调用,这条会红。
#[test]
fn 一树一线查重在锁内_并发建同一棵树只许一条胜出() {
    let root = git_repo("kz-f4-race");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("race");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    let before = worktree_registry(&canonical).matches("worktree ").count();

    let state = Arc::new(AppState::default());
    let project = root.display().to_string();
    let racers = (0..8)
        .map(|_| {
            let state = Arc::clone(&state);
            let project = project.clone();
            let name = name.clone();
            std::thread::spawn(move || {
                create_process(&state, &project, None, None, None, None, Some(name))
            })
        })
        .collect::<Vec<_>>();
    let results = racers
        .into_iter()
        .map(|handle| handle.join().expect("建线线程不许 panic"))
        .collect::<Vec<_>>();

    let winners = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(winners, 1, "并发建同一棵工作树只许一条线胜出: {results:?}");
    for error in results.iter().filter_map(|result| result.as_ref().err()) {
        assert!(
            error.contains("已绑定到线"),
            "落败者必须是被查重挡下的,不是被 git 或落库挡下的: {error}"
        );
    }
    assert_eq!(
        worktree_registry(&canonical).matches("worktree ").count(),
        before + 1,
        "磁盘上只许多出一棵工作树"
    );
    let bound = state
        .processes
        .lock()
        .unwrap()
        .values()
        .filter(|process| process.worktree_path.is_some())
        .count();
    assert_eq!(bound, 1, "内存表里绑着这棵树的线只许有一条");

    rollback_worktree(&canonical, &target, &branch);
    cleanup(&root, &[target]);
}

/// 复核点:`project_dir` 是不是**真的**恒主根?
///
/// 这是 §0 定案 2 的前提——不恒主根,`#p{n}` 的唯一性就不成立,session_id 降级的
/// 理由随之失效。全仓 `ProcessHandle` 的字面量构造点(production 侧)恰 3 处:
/// `state.rs` 的 `ensure_default_process` 1 处、`processes.rs` 的
/// `restore_processes_from_store` 与 `create_process` 各 1 处。这条测试
/// **先用源码把「恰 3 处」钉死**(多出第 4 个构造点就红,逼作者在那里也证明这条
/// 口径),**再逐处验行为**。三处合起来才是穷尽:
///
/// - `ensure_default_process` 取调用方给的 root —— 验它原样落进 `project_dir`;
/// - `create_process` 取 `normalized_project_root` —— 验带 worktree 时也不渗进去;
/// - `restore_processes_from_store` 取库值,而库值只由 `persist_process` 从上面两种
///   handle 写出 —— 验一次真实的落库/回读往返之后仍是主根(不动点)。
#[test]
fn project_dir恒主根_三个构造点逐处成立() {
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
    .unwrap();
    assert_eq!(info.project_dir, expected);
    assert_eq!(info.origin_project, expected);
    assert_ne!(info.worktree_path.as_deref(), Some(expected.as_str()));

    // 构造点 3:从 state.db 回读重建(库值是上一步 persist 出去的,验它是不动点)。
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

    rollback_worktree(&canonical, &target, &branch);
    cleanup(&root, &[target]);
}

/// D-176 红线:线的 `session_id` 只由**主根** + `p{n}` 决定,worktree 不参与。
///
/// 这正是 §0 定案 2(不给 session_id 加 worktree 后缀)的前提:`project_dir` 恒
/// 主根 ⇒ `#p{n}` 在项目内已唯一,后缀带不来唯一性收益,却会让既有会话历史失联。
#[test]
fn 线的session_id仍由主根算出() {
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

    rollback_worktree(&canonical, &target, &branch);
    cleanup(&root, &[target]);
}
