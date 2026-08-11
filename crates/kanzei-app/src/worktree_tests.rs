//! F4/K2:`process_create` 建线的测试 —— worktree 真实绑定、一树一线查重、
//! 落库失败整体回滚、`project_dir` 恒主根,以及 K2 补的并发/仲裁/残留可见性。
//!
//! R-177 验收⑦ 的落点:`processes.rs` 在 F4 之前**零测试**(既没有 `mod tests`
//! 也没有任何 `#[test]`),这个文件是它的开张。
//!
//! 夹具一律用真 `git init` + 一次真实提交:`git worktree add ... HEAD` 需要 HEAD
//! 有基点,而只伪造磁盘形态(建几个空目录)验不出 git 到底认不认那条路径——F4
//! 修的第一个真问题(`\\?\` 前缀 git 不收)恰恰只有真跑 git 才暴露得出来。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kanzei_harness::{Tool, ToolCtx};

use super::{
    acquire_project_write_lease_within, create_process, create_worktree,
    create_worktree_arbitrated, create_worktree_with_receipt, discard_worktree_checked,
    merge_worktree, reclaim_worktree_on_close, restore_processes_from_store, rollback_worktree,
    worktree_diff, worktree_status, worktree_target, WorktreeReceipt,
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

/// 目录的逐文件字节快照。F13 原方案写 sha256；测试内直接保留全部字节更强：
/// 任一文件新增、删除、改名或一个字节变化都会让 BTreeMap 不相等，且不引入哈希依赖。
fn tree_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else if path.is_file() {
                out.insert(
                    path.strip_prefix(base).unwrap().to_path_buf(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    if root.is_dir() {
        walk(root, root, &mut out);
    }
    out
}

fn discard_clean_tree_and_branch(root: &Path, worktree: &Path, branch: &str) {
    discard_worktree_checked(root, &worktree.display().to_string()).unwrap();
    git(root, &["branch", "-D", branch]);
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
    assert_eq!(
        info.branch.as_deref(),
        Some(branch.as_str()),
        "ProcessInfo 必须把真实 git 分支名交给前端,不能再靠页签编号猜"
    );
    assert!(!info.tracker_writes, "新分支线的 tracker 写入必须默认关闭");
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
/// # 夹具为什么不再是「把 state.db 的位置占成目录」
///
/// K2' 把一树一线查重改成查「内存表 ∪ state.db」,而查重发生在**建树之前** —— 库打不开
/// 就在建树之前失败,这条测试要验的回滚路径根本走不到,测试会变成绿的空壳。
///
/// 现在的夹具直接踩 `insert_new_process` 的第二道闸:预置一行 `p1|<项目>`,但把它的
/// `origin_project` 写成**别的项目**。于是 `list_processes(本项目)` 看不见它(编号照常
/// 算出 p1、查重照常放行),而插入时主键撞上 ⇒ 返回 false ⇒ 建线在**落库这一步**失败,
/// 此时工作树与分支都已经建出来了,正是回滚要收的那一刻。这也正是 `insert_new_process`
/// 文档里写的契约本身(「宁可让建线失败,也不许静默改写既有行」)。
#[tokio::test]
async fn 落库失败时worktree被回收_不留半绑定态() {
    let root = git_repo("kz-f4-rollback");
    let canonical = crate::normalized_project_root(&root);
    let project = canonical.display().to_string();
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&canonical))
        .expect("夹具:库必须开得起来,失败要发生在落库那一步");
    store
        .insert_new_process(&kanzei_core::StoredProcess {
            process_id: format!("p1|{project}"),
            // 挂在别的主项目名下:本项目的 list_processes 看不见它,编号与查重都不受影响。
            origin_project: "C:/另一个项目".into(),
            project_dir: "C:/另一个项目".into(),
            worktree_path: None,
            model: None,
            profile: None,
            reasoning: None,
            manual_models: Vec::new(),
            phase_pipeline: false,
            tracker_writes_enabled: false,
            updated_at: 1,
        })
        .unwrap();
    drop(store);
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
    // 失败必须来自库(state.db)的落库那一步,不是来自 git、也不是来自查重 ——
    // 否则这条测试验的就不是「建出来之后再回滚」那条路径了。
    assert!(
        error.contains("state.db") && error.contains("拒绝覆盖既有注册"),
        "这次失败应当来自落库(insert_new_process 第二道闸)而不是建树/查重: {error}"
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

#[test]
fn worktree_diff返回真实改动与diff() {
    let root = git_repo("kz-f13-diff");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("diff");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    let created = create_worktree(&canonical, &name).unwrap();

    std::fs::write(target.join("seed.txt"), "line changed seed\n").unwrap();
    std::fs::write(target.join("untracked.txt"), "new line file\n").unwrap();
    let info = worktree_diff(
        canonical.display().to_string(),
        target.display().to_string(),
    )
    .unwrap();
    assert_eq!(info.branch, branch);
    assert!(!info.clean);
    assert!(info.files.iter().any(|file| file.contains("seed.txt")));
    assert!(info.files.iter().any(|file| file.contains("untracked.txt")));
    assert!(
        info.diff.contains("seed.txt") && info.diff.contains("line changed seed"),
        "worktree_diff 必须返回真实 tracked diff:\n{}",
        info.diff
    );
    assert_eq!(info.path, created.path);

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

#[test]
fn worktree_merge干净时以no_ff落地() {
    let root = git_repo("kz-f13-merge-clean");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("merge-clean");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    create_worktree(&canonical, &name).unwrap();

    std::fs::write(target.join("line.txt"), "from line\n").unwrap();
    git(&target, &["add", "line.txt"]);
    git(&target, &["commit", "-qm", "line commit"]);
    let result = merge_worktree(&canonical, &target.display().to_string()).unwrap();
    assert!(result.contains(&branch));
    assert_eq!(
        std::fs::read_to_string(canonical.join("line.txt"))
            .unwrap()
            .trim(),
        "from line",
        "Windows core.autocrlf 可改工作区换行,但文件内容必须来自分支线"
    );
    let parents = git(&canonical, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "--no-ff 必须留下双亲 merge commit,实得:{parents}"
    );

    discard_clean_tree_and_branch(&canonical, &target, &branch);
    cleanup(&root, &[target]);
}

#[test]
fn worktree_merge冲突时不执行合并且保留双方() {
    let root = git_repo("kz-f13-merge-conflict");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("merge-conflict");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    create_worktree(&canonical, &name).unwrap();

    std::fs::write(target.join("seed.txt"), "line side\n").unwrap();
    git(&target, &["add", "seed.txt"]);
    git(&target, &["commit", "-qm", "line side"]);
    std::fs::write(canonical.join("seed.txt"), "main side\n").unwrap();
    git(&canonical, &["add", "seed.txt"]);
    git(&canonical, &["commit", "-qm", "main side"]);
    let main_head = git(&canonical, &["rev-parse", "HEAD"]);
    let line_head = git(&canonical, &["rev-parse", &branch]);

    let error = merge_worktree(&canonical, &target.display().to_string())
        .expect_err("同一行的相反修改必须被 merge-tree 预检拦住");
    assert!(error.contains("双方改动已保留"), "{error}");
    assert!(error.contains("seed.txt"), "冲突结果必须点名文件:{error}");
    assert_eq!(git(&canonical, &["rev-parse", "HEAD"]), main_head);
    assert_eq!(git(&canonical, &["rev-parse", &branch]), line_head);
    assert_eq!(
        std::fs::read_to_string(canonical.join("seed.txt")).unwrap(),
        "main side\n"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("seed.txt")).unwrap(),
        "line side\n"
    );

    discard_clean_tree_and_branch(&canonical, &target, &branch);
    cleanup(&root, &[target]);
}

#[test]
fn worktree_discard有未提交改动时保留现场() {
    let root = git_repo("kz-f13-discard-dirty");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("discard-dirty");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    create_worktree(&canonical, &name).unwrap();
    std::fs::write(target.join("keep-me.txt"), "uncommitted\n").unwrap();

    let error = discard_worktree_checked(&canonical, &target.display().to_string())
        .expect_err("未提交改动必须阻止普通 worktree remove");
    assert!(error.contains("已保留以便恢复"), "{error}");
    assert!(target.join("keep-me.txt").is_file());
    assert!(registry_has(&canonical, &target));
    assert!(branch_exists(&canonical, &branch));

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

#[tokio::test]
async fn 线上闭环_主树源码零改动_worktree内kanzei副本字节不变() {
    let root = git_repo("kz-f13-loop");
    let canonical = crate::normalized_project_root(&root);
    std::fs::create_dir_all(canonical.join(".kanzei/project")).unwrap();
    std::fs::write(canonical.join(".kanzei/kanzei.toml"), "profile = \"dev\"\n").unwrap();
    kanzei_tools::docstore::DocStore::open(&canonical, &kanzei_tools::docstore::REQUIREMENTS)
        .save(&[])
        .unwrap();
    git(&canonical, &["add", ".kanzei"]);
    git(&canonical, &["commit", "-qm", "seed kanzei assets"]);
    let main_seed = std::fs::read(canonical.join("seed.txt")).unwrap();

    let state = AppState::default();
    let name = unique("loop");
    let info = create_process(
        &state,
        &canonical.display().to_string(),
        None,
        None,
        None,
        None,
        Some(name),
    )
    .await
    .unwrap();
    let worktree = PathBuf::from(info.worktree_path.as_ref().unwrap());
    let branch = info.branch.as_deref().unwrap();
    let kanzei_before = tree_bytes(&worktree.join(".kanzei"));
    assert!(
        !kanzei_before.is_empty(),
        "夹具必须真的把 .kanzei checkout 到线上"
    );

    std::fs::create_dir_all(worktree.join("src")).unwrap();
    std::fs::write(worktree.join("src/line.rs"), "pub fn line() {}\n").unwrap();
    git(&worktree, &["add", "src/line.rs"]);
    git(&worktree, &["commit", "-qm", "line source"]);

    let ctx = ToolCtx::new(worktree.clone(), canonical.clone());
    let tracker = kanzei_tools::tracker::TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &kanzei_tools::docstore::REQUIREMENTS,
        requires_refs: None,
    };
    let tracker_out = tracker
        .execute(
            serde_json::json!({
                "action": "add",
                "title": "F13 main-root tracker",
                "fields": {"验收": "tracker follows project_root"}
            }),
            &ctx,
        )
        .await;
    assert!(!tracker_out.is_error, "{}", tracker_out.content);
    let memory_out = kanzei_tools::memory::MemoryNoteTool
        .execute(
            serde_json::json!({
                "summary": format!("F13 main-root memory {}", unique("note")),
                "detail": "memory follows project_root",
                "refs": []
            }),
            &ctx,
        )
        .await;
    assert!(!memory_out.is_error, "{}", memory_out.content);

    let session_id = process_session_id(&canonical, Some(&info.id));
    let store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
    store
        .create_session(&session_id, &canonical.display().to_string(), None)
        .unwrap();
    store
        .append_event(
            &session_id,
            "conversation.updated",
            &serde_json::json!({"messages": [kanzei_llm::Message::user_text("F13 line history")]}),
        )
        .unwrap();
    drop(store);

    assert_eq!(
        std::fs::read(canonical.join("seed.txt")).unwrap(),
        main_seed
    );
    assert!(
        !canonical.join("src/line.rs").exists(),
        "线内源码不得泄漏到主树"
    );
    assert!(
        std::fs::read_to_string(canonical.join(".kanzei/project/requirements.md"))
            .unwrap()
            .contains("F13 main-root tracker")
    );
    assert!(
        !std::fs::read_to_string(worktree.join(".kanzei/project/requirements.md"))
            .unwrap()
            .contains("F13 main-root tracker")
    );
    assert!(kanzei_tools::memory::project_memory_root(&canonical)
        .join("inbox.md")
        .is_file());
    assert!(kanzei_core::project_state_path(&canonical).is_file());
    assert!(!kanzei_core::project_state_path(&worktree).exists());
    assert_eq!(
        tree_bytes(&worktree.join(".kanzei")),
        kanzei_before,
        "线内 .kanzei 副本必须逐文件逐字节零改动"
    );

    discard_clean_tree_and_branch(&canonical, &worktree, branch);
    cleanup(&root, &[worktree]);
}

#[tokio::test]
async fn 删树后线的会话历史仍可回放() {
    let root = git_repo("kz-f13-history");
    let canonical = crate::normalized_project_root(&root);
    let state = AppState::default();
    let info = create_process(
        &state,
        &canonical.display().to_string(),
        None,
        None,
        None,
        None,
        Some(unique("history")),
    )
    .await
    .unwrap();
    let worktree = PathBuf::from(info.worktree_path.as_ref().unwrap());
    let branch = info.branch.as_deref().unwrap();
    let session_id = process_session_id(&canonical, Some(&info.id));
    let messages = vec![kanzei_llm::Message::user_text(
        "history survives tree removal",
    )];
    let store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
    store
        .create_session(&session_id, &canonical.display().to_string(), None)
        .unwrap();
    store
        .append_event(
            &session_id,
            "conversation.updated",
            &serde_json::json!({"messages": messages}),
        )
        .unwrap();
    drop(store);

    discard_clean_tree_and_branch(&canonical, &worktree, branch);
    assert!(!worktree.exists());
    let reopened =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&canonical)).unwrap();
    let replayed = crate::conversation::recover_messages(&reopened, &session_id).unwrap();
    assert_eq!(replayed.len(), 1);
    assert!(serde_json::to_string(&replayed[0])
        .unwrap()
        .contains("history survives tree removal"));
    assert_eq!(
        crate::conversation::conversation_list(
            canonical.display().to_string(),
            Some(info.id.clone())
        )
        .unwrap()
        .len(),
        1,
        "删树后历史列表入口也必须仍能发现这段会话"
    );
    drop(reopened);

    cleanup(&root, &[worktree]);
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

/// K2' 验收②:**认领失败时零回滚** —— 调用前后 git 状态逐字节相同。
///
/// 这是新架构的核心不变量的机械形态。`git branch <name> <base>` 的 ref 创建是原子的
/// (CAS,已存在即失败),所以它一旦失败就证明**本次调用一个东西都没建出来**,于是
/// 一个字节都不许删。老版在这条路上是 `worktree add -b` 撞名失败 → 走进清理块 →
/// 拿着调用前采样的过期布尔量去 `worktree remove --force` + 删分支,并发下删的正是
/// 赢家刚建好的东西。
///
/// 判据取两份**全量**快照:`git worktree list --porcelain` 与 `git branch --list`,
/// 逐字节比较。仓里还种了一棵目录被挪走的旁观者工作树,让「有没有偷偷 prune 过」
/// 也落进同一个判据里。
#[test]
fn 认领失败时零回滚_git状态逐字节不变() {
    let root = git_repo("kz-k2-zerorollback");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("zerorollback");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    // 场景:这个名字被一条**已经存在的分支**占着(「放弃工作树」只删目录、分支照旧留着,
    // 之后同名再建就是这一幕),而且这条分支上挂着用户还没合并的活。
    let bystander = park_bystander_worktree(&root, &canonical);
    git(&root, &["branch", &branch]);
    std::fs::write(root.join("user.txt"), "用户在这条分支上的活\n").unwrap();
    git(&root, &["add", "user.txt"]);
    git(&root, &["commit", "-qm", "用户的活"]);
    git(&root, &["branch", "-f", &branch, "HEAD"]);
    git(&root, &["reset", "-q", "--hard", "HEAD~1"]);
    let branch_head = git(&root, &["rev-parse", &branch]).trim().to_string();

    let worktrees_before = worktree_registry(&canonical);
    let branches_before = git(&root, &["branch", "--list"]);

    let error = create_worktree(&canonical, &name).expect_err("名字被分支占着时建树必然失败");

    assert_eq!(
        worktree_registry(&canonical),
        worktrees_before,
        "认领失败必须零回滚:git 的工作树清单要逐字节相同"
    );
    assert_eq!(
        git(&root, &["branch", "--list"]),
        branches_before,
        "认领失败必须零回滚:分支清单要逐字节相同"
    );
    assert_eq!(
        git(&root, &["rev-parse", &branch]).trim(),
        branch_head,
        "被占用的那条分支必须原封不动(它上面挂着还没合并的活)"
    );
    assert!(!target.exists(), "失败仍然不许在磁盘上留下目录");

    // 验收④ 前半:错误必须点名「被一条已存在的**分支**占着」并给出可执行动作 ——
    // app 里没有删分支的入口,只给 git 原文等于把这个名字变成死结。
    assert!(
        error.contains(&branch),
        "错误必须点名占着这个名字的分支: {error}"
    );
    assert!(
        error.contains("分支占着"),
        "错误必须说清占用者是分支(不是目录残留): {error}"
    );
    assert!(
        error.contains(&format!("branch -D {branch}")),
        "错误必须给出确切可执行的回收命令: {error}"
    );
    assert!(
        error.contains("log --oneline"),
        "删分支前必须先给出「确认没有要保留的提交」的确切命令: {error}"
    );

    git(&root, &["branch", "-D", &branch]);
    cleanup(&root, &[target, bystander]);
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
            error.contains("已绑定到线")
                || error.contains("工作树已存在")
                || error.contains("分支占着"),
            "落败者必须拿到明确的「这个名字已经有主了」,不是 git 的原始报错: {error}"
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
            write_scope: canonical.clone(),
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
                manual_models: Vec::new(),
                phase_pipeline: true,
                tracker_writes_enabled: false,
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
/// K2' 再补一刀:加了项目名之后,项目名与工作树名之间若用 `-` 连接,分量边界还原不出来
/// —— `sanitize_component` 把一切非 `[A-Za-z0-9_-]` 压成 `-`,于是两个分量里都可能有
/// `-`,「项目 a + 名字 b-c」与「项目 a-b + 名字 c」落到同一条路径,冲突换个形态回来。
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

    // 分量拼接的歧义:两组不同的(项目, 名字)不得落到同一条路径。
    let (left, _) = worktree_target(&parent.join("a"), "b-c").unwrap();
    let (right, _) = worktree_target(&parent.join("a-b"), "c").unwrap();
    assert_ne!(
        left, right,
        "「项目 a + 名字 b-c」与「项目 a-b + 名字 c」必须落在不同路径: {left:?} / {right:?}"
    );
    // 名字里的非法字符被压成 `-` 之后也不得制造歧义。
    let (dotted, _) = worktree_target(&parent.join("a"), "b c").unwrap();
    assert_eq!(
        dotted, left,
        "名字归一是既定行为(空格压成 -),这里只钉住它不改变分量边界"
    );
    let _ = std::fs::remove_dir_all(&parent);
}

// ══ K2':认领改成 git ref CAS —— 跨进程才是真正的判据 ═════════════════════════

/// 跨进程验收① 的子进程入口。父测试用 `--exact` 重入**本测试二进制**来起真实 OS 进程。
const CROSS_PROCESS_CHILD: &str = "processes::tests::k2_cross_process_child";

/// 验收① 的子进程:只做一件事——在给定仓里建一条带同名工作树的线,把结果打到 stdout。
///
/// 正常跑测试套时三个环境变量都不在,它是空操作(所以不需要 `#[ignore]`,也就不必让
/// 父进程去传 `--ignored`)。
#[tokio::test]
async fn k2_cross_process_child() {
    let (Ok(root), Ok(name), Ok(go)) = (
        std::env::var("KZ_K2_CHILD_ROOT"),
        std::env::var("KZ_K2_CHILD_NAME"),
        std::env::var("KZ_K2_CHILD_GO"),
    ) else {
        return;
    };
    // 起跑线:两个进程都装载完毕后父进程才放行,让它们尽量同时打进 git。
    let go = PathBuf::from(go);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !go.exists() {
        assert!(std::time::Instant::now() < deadline, "等父进程放行超时");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let state = AppState::default();
    let line = match create_process(&state, &root, None, None, None, None, Some(name)).await {
        Ok(info) => format!("OK {}", info.worktree_path.unwrap_or_default()),
        Err(error) => format!("ERR {}", error.replace('\n', " ⏎ ")),
    };
    println!("KZ_RESULT {line}");
}

/// 验收①:**两个真实 OS 进程**同时建同名工作树,赢家的树与分支完好无损。
///
/// 上一版用写租约挡这个竞态,而 `MemoryCoordinator` 是 `AppState` 里的进程内内存对象
/// (设计基线 §6.2:`kz` CLI、自举循环、第二个 kzapp 实例都看不见它)。所以那条破坏
/// 一字未减,只是从「线程之间」搬到了「进程之间」—— 用两个线程测永远测不出来,必须起
/// **两个 OS 进程**。这条测试在「认领 = `worktree add -b`」的老实现上必红:输家的 add
/// 失败 → 清理块拿着调用前采样的过期布尔量 → `worktree remove --force` + 删分支 →
/// 删掉的是赢家刚建好的树和分支。
#[test]
fn 跨进程并发建同名树_赢家的树与分支完好无损() {
    let root = git_repo("kz-k2-xproc");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("xproc");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    let head = git(&canonical, &["rev-parse", "HEAD"]).trim().to_string();
    let before = worktree_registry(&canonical).matches("worktree ").count();
    // 先把 state.db 的库结构建出来:两个子进程会同时开它,建表这一步不必也去竞争。
    drop(
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&canonical))
            .expect("预建 state.db"),
    );

    let go = std::env::temp_dir().join(unique("kz-k2-go"));
    let exe = std::env::current_exe().expect("拿到测试二进制自身路径");
    let mut kids = Vec::new();
    for _ in 0..2 {
        kids.push(
            super::hidden_command(&exe.display().to_string())
                .args([
                    CROSS_PROCESS_CHILD,
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("KZ_K2_CHILD_ROOT", root.display().to_string())
                .env("KZ_K2_CHILD_NAME", &name)
                .env("KZ_K2_CHILD_GO", go.display().to_string())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("起真实 OS 子进程"),
        );
    }
    std::thread::sleep(std::time::Duration::from_millis(600));
    std::fs::write(&go, b"go").unwrap();

    let mut outcomes = Vec::new();
    for kid in kids {
        let output = kid.wait_with_output().expect("等子进程结束");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        // libtest 把 `--nocapture` 的输出接在进度行尾巴上(`test … ... KZ_RESULT …`),
        // 所以按标记切,不按行首匹配。子进程那侧已经把换行换成了 ⏎,结果必在一行内。
        let line = stdout.find("KZ_RESULT ").map(|at| {
            stdout[at + "KZ_RESULT ".len()..]
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        });
        match line {
            Some(line) => outcomes.push(line),
            None => panic!(
                "子进程没有给出结果(它是不是没跑到那个测试?)\nstdout:\n{stdout}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }
    let _ = std::fs::remove_file(&go);

    let winners = outcomes
        .iter()
        .filter(|line| line.starts_with("OK "))
        .count();
    assert_eq!(
        winners, 1,
        "两个真实 OS 进程同时建同名树,只许一个胜出: {outcomes:?}"
    );

    // ——赢家的东西一个字节都不许被输家动过(这才是这条致命的后果所在)。
    assert!(
        target.is_dir(),
        "赢家的工作树目录必须还在: {}\n{outcomes:?}",
        target.display()
    );
    assert!(
        branch_exists(&canonical, &branch),
        "赢家的分支必须还在: {branch}\n{outcomes:?}"
    );
    assert_eq!(
        git(&canonical, &["rev-parse", &format!("refs/heads/{branch}")]).trim(),
        head,
        "赢家分支的 SHA 必须仍停在建出来时那个基点(被输家 update-ref/branch -D 动过就不是了)"
    );
    assert_eq!(
        git(&target, &["rev-parse", "HEAD"]).trim(),
        head,
        "赢家工作树的 HEAD 必须仍解析得出(被 update-ref 打断会变成全零)"
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

    let loser = outcomes
        .iter()
        .find(|line| line.starts_with("ERR "))
        .expect("必须有一个落败者");
    assert!(
        loser.contains("分支占着")
            || loser.contains("已绑定到线")
            || loser.contains("工作树已存在"),
        "落败者必须拿到点名的明确错误,不是 git 原文: {loser}"
    );

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// 验收④ 后半:一树一线查重必须**查库**,并点名占着这棵树的那条线。
///
/// 老版查重只扫内存进程表,而同一个函数里的编号分配偏偏是查库的——自相矛盾。后果:
/// 重启后(内存表空、库里还绑着)同名建线一路绕过查重,撞进 `create_worktree` 的目录
/// 预检,给出的文案会教用户 `worktree remove --force` 一棵**仍被库里某条线绑着、且带
/// 未提交改动**的活树,而且完全不点名那条线。
#[tokio::test]
async fn 查重必须查库_重启后同名建线点名占用的线() {
    let root = git_repo("kz-k2-storedup");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("storedup");
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
    // 线上有还没提交的活:老版文案教人 force 删掉的正是这种树。
    std::fs::write(target.join("还没提交的活.txt"), "别删我\n").unwrap();

    // 重启:内存进程表清空,库里那条绑定还在。
    state.processes.lock().unwrap().clear();
    assert!(
        state.processes.lock().unwrap().is_empty(),
        "前提:内存表是空的(重启后的形态)"
    );

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
    .expect_err("库里已经有线绑着这棵树,同名建线必须被拒");
    assert!(
        error.contains(&first.id),
        "拒绝文案必须点名库里绑着它的那条线 {}: {error}",
        first.id
    );
    assert!(
        !error.contains("worktree remove --force"),
        "查重命中时不许教用户 force 删掉一棵还有主的活树: {error}"
    );
    assert!(
        target.join("还没提交的活.txt").exists(),
        "被拒时不许动那棵树里的东西"
    );

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// 验收⑤:写租约获取必须有上界,超时报明确错误,而不是永久 pending。
///
/// 协调器的排队是无限期的,而取消入口(`cancel_waiter`)在 app 界面上够不到——没有上界
/// 时一条卡死的 writer 能把建线/建树/合并/放弃全变成永远转圈,用户只能杀进程。
///
/// 顺带钉住实现细节:超时分支**不能**直接丢掉排队用的 future(丢掉 = 丢掉 oneshot 接收
/// 端 ⇒ 协调器交接时 `tx.send` 把租约退回并就地 drop ⇒ drop 回调再锁同一把 std Mutex
/// ⇒ 自锁死)。所以这里还断言超时之后排队者已经从协调器里消失,且释放持有者之后协调器
/// 仍然能正常把租约交给下一个人。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 写租约获取有超时_不留永久pending() {
    use kanzei_harness::orchestration::{ProjectExecutionCoordinator, WriterLeaseRequest};

    let root = git_repo("kz-k2-leasetimeout");
    let canonical = crate::normalized_project_root(&root);
    let state = AppState::default();
    let holder = state
        .coordinator
        .acquire_writer_lease(WriterLeaseRequest {
            write_scope: canonical.clone(),
            run_id: "k2-timeout-holder".into(),
            process_id: "k2".into(),
            reason: "占位写者(永不主动释放)".into(),
        })
        .await
        .unwrap();

    let started = std::time::Instant::now();
    let error = acquire_project_write_lease_within(
        &state,
        &canonical,
        "建线测试",
        std::time::Duration::from_millis(200),
    )
    .await
    .expect_err("有人一直占着写租约时,取租约必须超时报错而不是永远等下去");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "超时必须真的按上界返回,而不是被别的东西唤醒: {:?}",
        started.elapsed()
    );
    assert!(
        error.contains("超时") && error.contains("建线测试"),
        "超时文案必须说清是什么操作超时: {error}"
    );
    assert!(
        error.contains("没有创建任何东西"),
        "超时文案必须说清没有半成品留下: {error}"
    );
    assert!(
        state
            .coordinator
            .snapshot(&canonical)
            .waiting_writers
            .is_empty(),
        "超时之后排队者必须从协调器里消失,不许留成永久 pending: {:?}",
        state.coordinator.snapshot(&canonical).waiting_writers
    );

    // 超时路径没有把协调器搞坏:持有者一放,下一个照常拿得到。
    drop(holder);
    let next = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        acquire_project_write_lease_within(
            &state,
            &canonical,
            "超时之后照常取",
            std::time::Duration::from_secs(5),
        ),
    )
    .await
    .expect("超时路径不得把协调器留在死锁/半状态")
    .expect("持有者释放后必须能立刻拿到租约");
    drop(next);
    cleanup(&root, &[]);
}

/// 默认档必须是「有上界」,不是某次调用忘了传就退回无限等。
#[test]
fn 写租约默认上界是有限的() {
    assert!(
        super::WRITE_LEASE_TIMEOUT > std::time::Duration::ZERO,
        "默认上界不能是 0(那样正常操作全部立刻超时)"
    );
    assert!(
        super::WRITE_LEASE_TIMEOUT <= std::time::Duration::from_secs(600),
        "默认上界必须真的能兜住卡死的 writer,不能大到等于没有"
    );
}

/// 次要②:建树期间**不许**独占全局进程表锁。
///
/// 老版把 `git worktree add`(一次全量检出)+ `status` + `diff` 全压在
/// `state.processes` 的 guard 里,理由是「查重必须原子」。代价是这几百毫秒到数秒里,
/// `process_list` / `process_update` / `process_close` / `run_prompt` 全部卡在同一把锁
/// 上 —— 界面对所有线冻结。
///
/// 判据是机械的,不是「读代码看着像在锁外」:用 `post-checkout` 钩子把 `git worktree
/// add` **确定性地**卡住 3 秒(2026-08-11 实测 git 在 `worktree add` 的检出之后会跑这个
/// 钩子,实测耗时 3.2s),在这段窗口里从另一个线程反复 `try_lock` 全局进程表——一次都
/// 不许失败。
///
/// 夹具前提:git 能跑 `#!/bin/sh` 钩子(Git for Windows 自带 sh)。跑不动钩子的话
/// `worktree add` 会秒回,下面那条「前提」断言会先红出来,不会伪装成绿。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 建树期间进程表锁不被独占() {
    let root = git_repo("kz-k2-lockfree");
    let hooks = root.join("kz-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(hooks.join("post-checkout"), "#!/bin/sh\nsleep 3\n").unwrap();
    git(
        &root,
        &[
            "config",
            "core.hooksPath",
            &hooks.display().to_string().replace('\\', "/"),
        ],
    );

    let canonical = crate::normalized_project_root(&root);
    let name = unique("lockfree");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();
    let state = Arc::new(AppState::default());
    let task = {
        let state = Arc::clone(&state);
        let project = root.display().to_string();
        let name = name.clone();
        tokio::spawn(async move {
            create_process(&state, &project, None, None, None, None, Some(name)).await
        })
    };

    // 等 git 真的走进钩子(钩子在里面 sleep 3s),再在这个窗口里反复抢锁。
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert!(
        !task.is_finished(),
        "夹具前提不成立:钩子没有把 git worktree add 卡住(git 跑不了 sh 钩子?),\
         这条测试此刻证明不了任何事"
    );
    let mut probes = 0usize;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1200);
    while std::time::Instant::now() < deadline {
        // 抢到就立刻还回去:这里验的是「抢得到」,不是「占着」。
        drop(
            state
                .processes
                .try_lock()
                .expect("建树期间不许独占全局进程表锁——界面会对所有线冻结"),
        );
        probes += 1;
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(probes > 10, "抢锁次数太少,窗口没打开: {probes}");
    assert!(
        !task.is_finished(),
        "前提:抢锁期间建树确实还没结束(钩子还卡着)"
    );

    let info = tokio::time::timeout(std::time::Duration::from_secs(60), task)
        .await
        .expect("建树最终要结束")
        .expect("建线任务不许 panic")
        .expect("建线本身必须成功");
    assert!(info.worktree_path.is_some());

    git(&root, &["config", "--unset", "core.hooksPath"]);
    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// 重要3:关线时对绑定工作树的处置 —— 干净且已合并的自动回收。
///
/// 老版关线一个 git 命令都不发:库里的绑定行删了,树和分支留在磁盘上无人认领,
/// app 内再没有入口能收,之后同名建线就撞进「工作树已存在」/「分支已存在」。
#[test]
fn 关线时干净且已合并的工作树被自动回收() {
    let root = git_repo("kz-k2-closeclean");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("closeclean");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    create_worktree(&canonical, &name).unwrap();
    assert!(target.is_dir());
    // 线上干了活并提交,然后收活合进主线——这正是「收完活再关线」的主流程形态。
    std::fs::write(target.join("line.txt"), "线上的活\n").unwrap();
    git(&target, &["add", "line.txt"]);
    git(&target, &["commit", "-qm", "线上的活"]);
    git(
        &canonical,
        &["merge", "-q", "--no-ff", "-m", "收活", &branch],
    );

    reclaim_worktree_on_close(&canonical, &target)
        .expect("干净且已合并的工作树,关线时必须被回收掉");
    assert!(!target.exists(), "干净且已合并 ⇒ 树必须被摘掉");
    assert!(
        !branch_exists(&canonical, &branch),
        "干净且已合并 ⇒ 分支必须被回收(留着就堵死同名再建)"
    );
    // 回收之后同名可以立刻再建:名字不会被自己的残留占死。
    let rebuilt = create_worktree(&canonical, &name).expect("回收之后同名建线必须能成功");
    assert_eq!(rebuilt.branch, branch);

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}

/// 重要3 反方向:有未提交改动 / 分支没合过来的树,关线时**一律保留**,并给出可执行回收动作。
///
/// 自动删一棵还挂着几小时活的树是丢工作,比留下孤儿严重得多。
#[test]
fn 关线时有活的工作树必须保留并给出回收动作() {
    let root = git_repo("kz-k2-closedirty");
    let canonical = crate::normalized_project_root(&root);
    let name = unique("closedirty");
    let (target, branch) = worktree_target(&canonical, &name).unwrap();

    create_worktree(&canonical, &name).unwrap();
    std::fs::write(target.join("还没提交.txt"), "几小时的活\n").unwrap();

    let kept =
        reclaim_worktree_on_close(&canonical, &target).expect_err("有未提交改动的树,关线时不许删");
    assert!(target.is_dir(), "有活的树必须原样留着");
    assert!(
        target.join("还没提交.txt").exists(),
        "里面的活一个字节都不许动"
    );
    assert!(branch_exists(&canonical, &branch), "分支同样必须留着");
    assert!(
        kept.contains(&target.display().to_string()) && kept.contains(&branch),
        "保留说明必须点名路径与分支: {kept}"
    );
    assert!(
        kept.contains("worktree remove --force") && kept.contains(&format!("branch -D {branch}")),
        "保留说明必须给出可执行的回收命令: {kept}"
    );

    // 干净但**没合并**同样保留。
    std::fs::remove_file(target.join("还没提交.txt")).unwrap();
    std::fs::write(target.join("committed.txt"), "提交了但没合\n").unwrap();
    git(&target, &["add", "committed.txt"]);
    git(&target, &["commit", "-qm", "提交了但没合"]);
    let unmerged =
        reclaim_worktree_on_close(&canonical, &target).expect_err("分支还没并进主线时不许删");
    assert!(
        unmerged.contains("还没并进主线"),
        "保留原因必须说清是「没合过来」而不是「有未提交改动」: {unmerged}"
    );
    assert!(target.is_dir());
    assert!(branch_exists(&canonical, &branch));

    rollback_worktree(&canonical, &rollback_receipt(&canonical, &target, &branch)).unwrap();
    cleanup(&root, &[target]);
}
