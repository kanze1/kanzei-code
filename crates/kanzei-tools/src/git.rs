//! 结构化 Git 工具(拆解 R-257 B4):按域切分——
//! tool(适配层:GitTool/输入契约/命令分发)、commands(git 命令执行)、
//! finalize(交付工作流 + 门禁组件)、worktree(worktree 解析)。
//! 零外部 API 面变更,公共面(GitTool/parse_worktree_list/WorktreeEntry/
//! staged_source_fingerprint)原样保留。

mod commands;
#[cfg(test)]
pub(crate) use commands::*;
#[cfg(test)]
use kanzei_harness::ToolCtx;
mod finalize;
pub use finalize::staged_source_fingerprint;
#[cfg(test)]
pub(crate) use finalize::*;
mod tool;
pub use tool::GitTool;
#[cfg(test)]
pub(crate) use tool::*;
mod worktree;
#[cfg(test)]
pub(crate) use worktree::worktree_for_branch;
pub use worktree::{parse_worktree_list, WorktreeEntry};

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::Tool;

    /// R-227 验收①:tracker diff 出现 `T-<数字>xxx` 占位符即拒;真实 10 位 ID 放行;
    /// 非 tracker 文件不受影响;无占位符放行。
    #[test]
    fn placeholder_id_gate_拒绝占位符_放行真实_id与非tracker() {
        // 新增行带占位符 → 拒。
        let diff = "\
diff --git a/.kanzei/project/requirements-archive.md b/.kanzei/project/requirements-archive.md
index 0000000..1111111 100644
--- a/.kanzei/project/requirements-archive.md
+++ b/.kanzei/project/requirements-archive.md
@@ -1,1 +1,1 @@
+- 全量 cargo test --workspace 全绿(T-1786565xxx,harness 118)";
        let paths = vec![".kanzei/project/requirements-archive.md".into()];
        let err = placeholder_id_gate(diff, &paths).unwrap_err();
        assert!(err.contains("占位符"), "{err}");
        assert!(err.contains("T-1786565xxx"), "{err}");

        // 真实 10 位 ID → 放行。
        let real = "\
diff --git a/.kanzei/project/requirements-archive.md b/.kanzei/project/requirements-archive.md
+ 全量 cargo test --workspace 全绿(T-1786565346,harness 118)";
        assert!(
            placeholder_id_gate(real, &paths).is_ok(),
            "真实 ID 必须放行"
        );

        // 无占位符 → 放行。
        let clean = "\
diff --git a/.kanzei/project/requirements.md b/.kanzei/project/requirements.md
+ - 进展: 实现已落地,验证通过";
        assert!(placeholder_id_gate(clean, &paths).is_ok());

        // 非 tracker 文件即使含 `T-123xxx` 也不拦(源码/文档不在门禁范围)。
        let source = "\
diff --git a/crates/kanzei-tools/src/tracker.rs b/crates/kanzei-tools/src/tracker.rs
+ // T-123xxx 这里不是占位符,是代码示例";
        let source_paths = vec!["crates/kanzei-tools/src/tracker.rs".into()];
        assert!(placeholder_id_gate(source, &source_paths).is_ok());

        // 无 tracker 路径 → 直接放行(不扫)。
        assert!(placeholder_id_gate(diff, &source_paths).is_ok());

        // D-357 验收①:只删占位符的 diff 必须放行。archive_fill 回填后的清理提交
        // 就是这个形态——删掉带占位符的旧行、写回带真值的新行。连它一起拒,门禁
        // 就把自己配套的清理通道堵死了。
        let cleanup = "\
diff --git a/.kanzei/project/requirements-archive.md b/.kanzei/project/requirements-archive.md
--- a/.kanzei/project/requirements-archive.md
+++ b/.kanzei/project/requirements-archive.md
@@ -1,1 +1,1 @@
-- 全量 cargo test --workspace 全绿(T-1786565xxx,harness 118)
+- 全量 cargo test --workspace 全绿(T-1786565346,harness 118)";
        assert!(
            placeholder_id_gate(cleanup, &paths).is_ok(),
            "回填清理提交(删占位符、加真值)必须放行,否则 archive_fill 的成果提交不出去"
        );

        // D-357 验收③:diff 文件头不参与判定。`+++ b/xxx` 以 `+` 开头,但它是头不是内容。
        let header_only = "\
diff --git a/.kanzei/project/T-1786565xxx.md b/.kanzei/project/T-1786565xxx.md
--- a/.kanzei/project/T-1786565xxx.md
+++ b/.kanzei/project/T-1786565xxx.md
@@ -1,1 +1,1 @@
+- 进展: 一切正常";
        assert!(
            placeholder_id_gate(header_only, &paths).is_ok(),
            "占位符只出现在 diff 文件头里时不该拦"
        );

        // D-357 验收④:同一 diff 既删旧占位符又加新占位符 → 仍拒(新增的那个才是罪)。
        let mixed = "\
diff --git a/.kanzei/project/requirements-archive.md b/.kanzei/project/requirements-archive.md
--- a/.kanzei/project/requirements-archive.md
+++ b/.kanzei/project/requirements-archive.md
@@ -1,2 +1,2 @@
-- 旧证据(T-1786565xxx)
+- 新证据(T-1786566xxx)";
        let mixed_err = placeholder_id_gate(mixed, &paths).unwrap_err();
        assert!(
            mixed_err.contains("T-1786566xxx") && !mixed_err.contains("T-1786565xxx"),
            "只该点名新增的那个占位符,不该把被删掉的也算进去:{mixed_err}"
        );
    }

    /// R-177 内容③:解析器抽出来即补单测——它此前零直接覆盖,只被 merge_ff 间接用到。
    /// 表驱动覆盖 `--porcelain` 的全部行形态。
    #[test]
    fn parse_worktree_list识别分支_bare_detached_locked_prunable() {
        let porcelain = "\
worktree C:/proj/kanzei
HEAD 1111111111111111111111111111111111111111
branch refs/heads/dev

worktree C:/proj/.kanzei-worktree-kanzei.f9
HEAD 2222222222222222222222222222222222222222
branch refs/heads/kanzei/thread-f9

worktree C:/proj/detached-tree
HEAD 3333333333333333333333333333333333333333
detached

worktree C:/proj/bare-tree
bare

worktree C:/proj/locked-tree
HEAD 4444444444444444444444444444444444444444
locked 手工锁住的原因

worktree C:/proj/gone-tree
HEAD 5555555555555555555555555555555555555555
branch refs/heads/gone
prunable gitdir file points to non-existent location
";
        let entries = parse_worktree_list(porcelain);
        assert_eq!(entries.len(), 6, "{entries:?}");

        assert_eq!(entries[0].path, std::path::PathBuf::from("C:/proj/kanzei"));
        assert_eq!(
            entries[0].branch.as_deref(),
            Some("dev"),
            "分支短名要剥前缀"
        );
        assert!(!entries[0].bare && !entries[0].detached);

        assert_eq!(
            entries[1].branch.as_deref(),
            Some("kanzei/thread-f9"),
            "含 / 的分支名不能被截断"
        );

        assert!(entries[2].detached && entries[2].branch.is_none());
        assert!(entries[3].bare && entries[3].branch.is_none());
        assert!(entries[4].locked, "locked 带原因串时也要认出来");
        assert!(entries[5].prunable, "prunable 带原因串时也要认出来");

        // 空输入与孤儿属性行都不能 panic,也不能凭空造出记录。
        assert!(parse_worktree_list("").is_empty());
        assert!(parse_worktree_list("branch refs/heads/x\nbare\n").is_empty());
    }

    fn temp_repo(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-git-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.invalid"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Kanzei Test"])
            .current_dir(&root)
            .status()
            .unwrap();
        root
    }

    #[test]
    fn paths_must_be_explicit_files() {
        let root = temp_repo("paths");
        std::fs::create_dir_all(root.join("src")).unwrap();
        assert!(normalize_files(&root, &[".".into()], true).is_err());
        assert!(normalize_files(&root, &["src".into()], true).is_err());
        assert!(normalize_files(&root, &["../x".into()], true).is_err());
        assert_eq!(
            normalize_files(&root, &["src/main.rs".into()], true).unwrap(),
            vec!["src/main.rs"]
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-178:normalize_resource 在 Windows 上把整条路径 to_lowercase,而 git pathspec
    /// 大小写敏感——小写化会让含大写字母的文件(INDEX.md、Cargo.lock)匹配不到,
    /// stage 静默失败。传给 git 的路径必须保留原始大小写,安全校验照常生效。
    #[test]
    fn paths_keep_original_case_while_still_escaping_check() {
        let root = temp_repo("case");
        std::fs::create_dir_all(root.join("src")).unwrap();
        // 大小写必须原样保留。
        assert_eq!(
            normalize_files(&root, &["INDEX.md".into()], true).unwrap(),
            vec!["INDEX.md"]
        );
        assert_eq!(
            normalize_files(&root, &["src/MyFile.rs".into()], true).unwrap(),
            vec!["src/MyFile.rs"]
        );
        // `..` 折叠等安全语义不受影响。
        assert_eq!(
            normalize_files(&root, &["./src/../INDEX.md".into()], true).unwrap(),
            vec!["INDEX.md"]
        );
        assert!(normalize_files(&root, &["../escape.txt".into()], true).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn stage_uppercase_paths_and_verify_index() {
        let root = temp_repo("case");
        std::fs::write(root.join("INDEX.md"), "# index\n").unwrap();
        std::fs::write(root.join("Cargo.lock"), "lock\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let staged = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["INDEX.md","Cargo.lock"]}),
                &ctx,
            )
            .await;
        assert!(!staged.is_error, "{}", staged.content);
        assert!(
            staged.content.contains("staged_hash: "),
            "{}",
            staged.content
        );
        // 暂存区里必须是原始大小写的路径,而不是被小写化的 index.md。
        let index = staged_paths(&root).await.unwrap();
        assert!(index.contains(&"INDEX.md".to_string()), "{index:?}");
        assert!(index.contains(&"Cargo.lock".to_string()), "{index:?}");
        assert!(!index.contains(&"index.md".to_string()), "{index:?}");
        std::fs::remove_dir_all(root).ok();
    }

    /// D-347:含非 ASCII(中文)文件名的暂存区,后续 stage 必须能被正常追加/覆盖。
    /// 根因是 staged_paths 读 index 路径时 git 默认 core.quotepath=true 输出带引号的
    /// 八进制转义,与请求的真实 UTF-8 路径比较必不相等——即使请求已显式包含该中文
    /// 路径,也会被误判为"index 里存在请求外路径"而拒绝(D-263 的覆盖检查是字面
    /// 集合比较,不能因表示形式不同而误判)。
    #[tokio::test]
    async fn stage_after_non_ascii_path_is_not_foreign() {
        let root = temp_repo("cn");
        std::fs::write(root.join("目录.md"), "# 手册\n").unwrap();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let first = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["目录.md"]}),
                &ctx,
            )
            .await;
        assert!(
            !first.is_error,
            "首次 stage 中文路径失败: {}",
            first.content
        );
        // 修复前:existing 里是转义串,请求已显式包含"目录.md"仍被拒(foreign 误报)。
        // D-263 覆盖检查要求请求列出全部既有路径,这里完整列出 = 正常追加语义。
        let second = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["目录.md","a.txt"]}),
                &ctx,
            )
            .await;
        assert!(
            !second.is_error,
            "请求已包含中文路径仍被误判 foreign: {}",
            second.content
        );
        // 暂存区路径必须以真实 UTF-8 呈现,而不是转义形式。
        let paths = staged_paths(&root).await.unwrap();
        assert!(paths.contains(&"目录.md".to_string()), "{paths:?}");
        assert!(paths.contains(&"a.txt".to_string()), "{paths:?}");
        assert!(
            !paths.iter().any(|p| p.contains("\\347")),
            "路径仍是 quotepath 转义形式: {paths:?}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-208:log 是只读查询,模型排查"最近改了什么"的高频入口——此前没有这个
    /// action,实测模型直接调 `{"action":"log"}` 被拒,只能转投 bash(每次 ask)。
    #[tokio::test]
    async fn log_returns_recent_commits_and_honors_path_filter() {
        let root = temp_repo("log");
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-q", "-m", "第一条:加入 a.txt"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "b.txt"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-q", "-m", "第二条:加入 b.txt"])
            .current_dir(&root)
            .status()
            .unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let all = GitTool
            .execute(serde_json::json!({"action": "log"}), &ctx)
            .await;
        assert!(!all.is_error, "{}", all.content);
        assert!(
            all.content.contains("第一条") && all.content.contains("第二条"),
            "{}",
            all.content
        );
        // count 生效:只要最近 1 条。
        let one = GitTool
            .execute(serde_json::json!({"action": "log", "count": 1}), &ctx)
            .await;
        assert!(
            one.content.contains("第二条") && !one.content.contains("第一条"),
            "{}",
            one.content
        );
        // 路径过滤:只看 a.txt 的历史。
        let filtered = GitTool
            .execute(
                serde_json::json!({"action": "log", "files": ["a.txt"]}),
                &ctx,
            )
            .await;
        assert!(
            filtered.content.contains("第一条") && !filtered.content.contains("第二条"),
            "{}",
            filtered.content
        );
        std::fs::remove_dir_all(root).ok();
    }

    fn git_in(dir: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed in {}", dir.display());
    }

    fn commit_file(dir: &std::path::Path, name: &str, content: &str, message: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        git_in(dir, &["add", name]);
        git_in(dir, &["commit", "-q", "-m", message]);
    }

    /// 发版形态:main 检出在另一棵工作树,merge_ff 要找到那棵树并在里面快进,
    /// 让分支引用与工作区文件一起前进——这是 bash 拦 merge 后发版流程的唯一通道。
    #[tokio::test]
    async fn merge_ff_fast_forwards_branch_checked_out_in_linked_worktree() {
        let root = temp_repo("ffwt");
        commit_file(&root, "a.txt", "v1\n", "初始提交");
        git_in(&root, &["branch", "rel"]);
        git_in(&root, &["switch", "-q", "-c", "dev"]);
        let release = root.join("release-tree");
        git_in(
            &root,
            &["worktree", "add", "-q", release.to_str().unwrap(), "rel"],
        );
        commit_file(&root, "a.txt", "v2\n", "dev 前进一步");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let out = GitTool
            .execute(
                serde_json::json!({"action":"merge_ff","from":"dev","into":"rel"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("fast-forwarded rel"),
            "{}",
            out.content
        );
        // 引用和工作区文件都要真的前进。
        let dev = run_git(&root, &["rev-parse", "dev"]).await.unwrap();
        let rel = run_git(&root, &["rev-parse", "rel"]).await.unwrap();
        assert_eq!(dev, rel);
        // autocrlf 环境下检出内容可能是 CRLF,断言前归一。
        let checked_out = std::fs::read_to_string(release.join("a.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(checked_out, "v2\n");
        git_in(
            &root,
            &["worktree", "remove", "--force", release.to_str().unwrap()],
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// 分支没检出在任何工作树时退化为纯引用快进;历史分叉必须干净失败。
    #[tokio::test]
    async fn merge_ff_updates_unchecked_branch_and_refuses_divergence() {
        let root = temp_repo("ffref");
        commit_file(&root, "a.txt", "v1\n", "初始提交");
        git_in(&root, &["branch", "archive"]);
        git_in(&root, &["switch", "-q", "-c", "dev"]);
        commit_file(&root, "a.txt", "v2\n", "dev 前进");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let out = GitTool
            .execute(
                serde_json::json!({"action":"merge_ff","from":"dev","into":"archive"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("ref-only"), "{}", out.content);
        let dev = run_git(&root, &["rev-parse", "dev"]).await.unwrap();
        let archive = run_git(&root, &["rev-parse", "archive"]).await.unwrap();
        assert_eq!(dev, archive);
        // 制造分叉:archive 上单独长一个提交,dev 再前进,快进必须被拒。
        git_in(&root, &["switch", "-q", "archive"]);
        commit_file(&root, "b.txt", "x\n", "archive 单独前进");
        git_in(&root, &["switch", "-q", "dev"]);
        commit_file(&root, "a.txt", "v3\n", "dev 再前进");
        let rejected = GitTool
            .execute(
                serde_json::json!({"action":"merge_ff","from":"archive","into":"dev"}),
                &ctx,
            )
            .await;
        assert!(rejected.is_error, "{}", rejected.content);
        assert!(rejected.content.contains("快进"), "{}", rejected.content);
        std::fs::remove_dir_all(root).ok();
    }

    /// 选项注入与区间语法要在碰 git 之前被拒掉。
    #[tokio::test]
    async fn merge_ff_rejects_malformed_refs() {
        let root = temp_repo("ffbad");
        commit_file(&root, "a.txt", "v1\n", "初始提交");
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        for bad in ["--exec=evil", "a..b", "a b", "HEAD~1", ""] {
            let out = GitTool
                .execute(serde_json::json!({"action":"merge_ff","from": bad}), &ctx)
                .await;
            assert!(out.is_error, "`{bad}` 应被拒绝:{}", out.content);
        }
        let out = GitTool
            .execute(
                serde_json::json!({"action":"merge_ff","from":"HEAD","into":"-evil"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn stage_hash_is_required_and_detects_index_change() {
        let root = temp_repo("cas");
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let staged = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["a.txt"]}),
                &ctx,
            )
            .await;
        assert!(!staged.is_error, "{}", staged.content);
        let hash = staged
            .content
            .lines()
            .find_map(|line| line.strip_prefix("staged_hash: "))
            .unwrap()
            .to_string();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        run_git(&root, &["add", "--", "b.txt"]).await.unwrap();
        let rejected = GitTool
            .execute(
                serde_json::json!({"action":"commit","message":"x","expected_hash":hash}),
                &ctx,
            )
            .await;
        assert!(rejected.is_error);
        assert!(
            rejected.content.contains("staged content changed"),
            "{}",
            rejected.content
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-263:自举 stage 只暂存本次显式列出的文件,工作区里他人的改动
    /// 留在原地,并且被点名可见(不静默吞掉也不静默跳过)。
    #[tokio::test]
    async fn stage_leaves_foreign_changes_unstaged_and_names_them() {
        let root = temp_repo("d263");
        commit_file(&root, "base.txt", "base\n", "初始提交");
        // 本轮要提交的文件。
        std::fs::write(root.join("mine.txt"), "mine\n").unwrap();
        // 并发线/他人改的文件(未跟踪 + 已跟踪被改)。
        std::fs::write(root.join("theirs-new.txt"), "theirs\n").unwrap();
        std::fs::write(root.join("base.txt"), "base changed\n").unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let staged = GitTool
            .execute(
                serde_json::json!({"action":"stage","files":["mine.txt"]}),
                &ctx,
            )
            .await;
        assert!(!staged.is_error, "{}", staged.content);
        // 只暂存了 mine.txt。
        let staged_paths = staged_paths(&root).await.unwrap();
        assert_eq!(staged_paths, vec!["mine.txt"], "清单外改动不得入暂存区");
        // 他人的改动仍在工作区,且被点名。
        assert_eq!(
            std::fs::read_to_string(root.join("theirs-new.txt")).unwrap(),
            "theirs\n",
            "未跟踪的他人文件不能被动过"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("base.txt")).unwrap(),
            "base changed\n",
            "已跟踪的他人改动不能被动过"
        );
        assert!(
            staged.content.contains("NOT staged by this request"),
            "应点名未纳入的改动: {}",
            staged.content
        );
        assert!(
            staged.content.contains("theirs-new.txt") && staged.content.contains("base.txt"),
            "点名的文件清单应覆盖他人改动: {}",
            staged.content
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-264 验收② + R-209:git.rs 提交门禁、发版门禁(verify.ps1)与 CI(ci.yml)
    /// 的**完整检查项集合**机械同步——任一侧增删一步即红(不再只比对 fmt/clippy 两项)。
    ///
    /// 口径:verify.ps1 的 `Step-With-Timing "<key>"` 键集合必须等于固定清单
    /// {fmt, clippy, test, ui_syntax, ui_runtime, ui_lint, parallel_lines_regression,
    /// ui_a11y, ui_i18n, ui_markdown};每个键在 ci.yml 里有对应标记(命令文本或
    /// smoke 脚本名);smoke 脚本与 npm ci 在两侧同现同隐。
    #[test]
    fn gate_checklists_align_across_git_verify_and_ci() {
        // 仓库根:git.rs 在 crates/kanzei-tools/src/,CARGO_MANIFEST_DIR 是
        // crates/kanzei-tools,上溯两级即仓库根。
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let ci = std::fs::read_to_string(repo_root.join(".github/workflows/ci.yml")).unwrap();
        let verify = std::fs::read_to_string(repo_root.join("scripts/verify.ps1")).unwrap();

        // ① verify.ps1 检查键集合(Step-With-Timing "<key>")必须等于固定清单。
        fn verify_check_keys(text: &str) -> std::collections::BTreeSet<String> {
            let mut keys = std::collections::BTreeSet::new();
            let needle = "Step-With-Timing \"";
            let mut start = 0;
            while let Some(pos) = text[start..].find(needle) {
                let after = start + pos + needle.len();
                let key_end = text[after..]
                    .find('"')
                    .map(|e| after + e)
                    .unwrap_or(text.len());
                keys.insert(text[after..key_end].to_string());
                start = key_end;
            }
            keys
        }
        let expected: std::collections::BTreeSet<String> = [
            "fmt",
            "clippy",
            "test",
            "ui_syntax",
            "ui_runtime",
            "ui_lint",
            "parallel_lines_regression",
            "ui_a11y",
            "ui_i18n",
            "ui_markdown",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let actual = verify_check_keys(&verify);
        assert_eq!(
            actual, expected,
            "verify.ps1 检查键集合必须等于固定清单——新增/删除门禁时 ci.yml 与 git.rs 也要同步"
        );

        // ② 每个键在 ci.yml 有对应标记(命令文本或 smoke 脚本名)。
        let markers: [(&str, &str); 10] = [
            ("fmt", "cargo fmt --all -- --check"),
            ("clippy", "cargo clippy --workspace --all-targets"),
            ("test", "cargo test --workspace"),
            ("ui_syntax", "node --check"),
            ("ui_runtime", "ui-runtime-smoke.mjs"),
            ("ui_lint", "ui-lint-smoke.mjs"),
            ("parallel_lines_regression", "parallel-lines-regression.mjs"),
            ("ui_a11y", "ui-a11y-smoke.mjs"),
            ("ui_i18n", "ui-i18n-smoke.mjs"),
            ("ui_markdown", "ui-markdown-smoke.mjs"),
        ];
        for (key, marker) in markers {
            assert!(ci.contains(marker), "ci.yml 缺检查 {key}(标记 {marker})");
        }

        // ③ 反向:smoke 脚本在两侧同现同隐;npm ci 必须存在(ui-lint 依赖 eslint)。
        for script in [
            "ui-runtime-smoke.mjs",
            "ui-lint-smoke.mjs",
            "parallel-lines-regression.mjs",
            "ui-a11y-smoke.mjs",
            "ui-i18n-smoke.mjs",
            "ui-markdown-smoke.mjs",
        ] {
            assert_eq!(
                ci.contains(script),
                verify.contains(script),
                "smoke 脚本 {script} 必须在 verify.ps1 与 ci.yml 两侧同现同隐"
            );
        }
        assert!(
            ci.contains("npm ci"),
            "ci.yml 必须 npm ci(ui-lint 依赖 eslint)"
        );

        // ④ 门禁实现(git 模块 finalize.rs)也含同一命令文本。
        let this = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/git/finalize.rs"),
        )
        .unwrap();
        assert!(
            this.contains("cargo fmt --all -- --check"),
            "fmt_gate 命令与 CI 不一致"
        );

        // ⑤ clippy 的三处分工是**刻意**的,不是漂移——所以逐处正向断言,而不是
        //    断言三处相同。省下的是 25.7s(37.9s → 12.2s)编译时间,代价是测试代码
        //    的 lint 违规会本地绿、push 后 CI 红。任何一处改动都必须回到这里改。
        //
        //    提交门禁(git.rs):check --all-targets 保编译底线 + 轻量 clippy 做 lint
        assert!(
            this.contains("cargo check --workspace --all-targets"),
            "compile_gate 必须保留 --all-targets:它是测试代码的编译底线,\
             clippy 变轻之后没有别的东西覆盖测试代码能不能编译"
        );
        assert!(
            this.contains("cargo clippy --workspace -- -D warnings"),
            "clippy_gate 应为不含 --all-targets 的轻量形态"
        );
        //    verify.ps1:轻量 clippy(编译覆盖由紧随其后的 test 步骤提供)
        assert!(
            verify.contains("cargo clippy --workspace --manifest-path"),
            "verify.ps1 的 clippy 应为轻量形态(不带 --all-targets)"
        );
        //    ci.yml:全量 clippy——测试代码的 lint 覆盖只剩这一处,丢了就真没人管了
        assert!(
            ci.contains("cargo clippy --workspace --all-targets -- -D warnings"),
            "ci.yml 必须保留 --all-targets 全量 clippy:本地两处都已转轻量,\
             测试代码的 lint 覆盖只由 CI 承担"
        );
    }

    /// D-264 验收①:构造「新增文件带 fmt 违规」场景,提交前被拦并明说违规位置。
    /// 在临时最小 cargo 工程上直接调 fmt_gate——门禁只读不写,跑完删目录。
    #[tokio::test]
    async fn fmt_gate_rejects_unformatted_source_and_names_file() {
        let dir = std::env::temp_dir().join(format!(
            "kz-fmtgate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"fmt-gate-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        // 故意不格式化:rustfmt 会要求改成 `pub fn x() -> i32 { 1 }`。
        std::fs::write(dir.join("src/lib.rs"), "pub fn  x( ) -> i32 { 1 }\n").unwrap();

        let err = fmt_gate(&dir).await.unwrap_err();
        assert!(err.contains("提交被拦下"), "应点名门禁: {err}");
        // Windows 上 rustfmt/clippy 输出 `src\lib.rs`(反斜杠),Unix 是正斜杠;
        // 断言文件名片段兼容两种分隔符。
        assert!(err.contains("lib.rs"), "应点名违规文件: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-264 验收①(clippy 侧):构造「新增文件带 clippy 违规」场景,提交前被拦。
    /// 最小工程一条 unused variable 即可让 `-D warnings` 红。
    #[tokio::test]
    async fn clippy_gate_rejects_lint_violation_and_names_file() {
        let dir = std::env::temp_dir().join(format!(
            "kz-clippygate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"clippy-gate-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        )
        .unwrap();
        // unused_variables 是默认 warn,-D warnings 下必红。
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn probe(flag: bool) -> i32 { let unused = 1; if flag { 1 } else { 0 } }\n",
        )
        .unwrap();

        let err = clippy_gate(&dir).await.unwrap_err();
        assert!(err.contains("提交被拦下"), "应点名门禁: {err}");
        // 同上:Windows 输出反斜杠路径,断言文件名片段兼容两种分隔符。
        assert!(err.contains("lib.rs"), "应点名违规文件: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 编译错误必须被 clippy_gate 拦下,且报错含 `-->` 位置。
    /// (原为 R-210 验收①「clippy 覆盖 check」的实证;clippy 转轻量后,这条覆盖
    /// 改由 clippy_gate 内先跑的 compile_gate 提供,断言不变、机制换了。)
    #[tokio::test]
    async fn clippy_gate_rejects_compile_error_with_position() {
        let dir = std::env::temp_dir().join(format!(
            "kz-clippycomp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"clippy-compile-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        )
        .unwrap();
        // 未定义符号:编译错误,不是 lint。check 删掉后必须仍被 clippy 拦截。
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn probe() -> i32 { undefined_symbol_here }\n",
        )
        .unwrap();

        let err = clippy_gate(&dir).await.unwrap_err();
        assert!(err.contains("提交被拦下"), "编译错误必须拦下提交: {err}");
        assert!(
            err.contains("-->"),
            "报错必须含 --> 位置(clippy 编译覆盖 check 的实证): {err}"
        );
        assert!(err.contains("lib.rs"), "应点名出错文件: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// clippy 转轻量之后的护栏:**测试代码**的编译错误必须仍被拦下。
    ///
    /// clippy 去掉 `--all-targets` 后就不再看测试目标了,测试代码能不能编译完全
    /// 靠 clippy_gate 内先跑的 compile_gate(`check --all-targets`)。这条测试盯的
    /// 就是那条底线:它一旦红,说明有人把 compile_gate 也改轻了——而那正是
    /// 2026-08-09 事故的形态(破损代码配着自写的 passed 记录进库)。
    #[tokio::test]
    async fn clippy_gate_rejects_compile_error_in_test_code() {
        let dir = std::env::temp_dir().join(format!(
            "kz-clippytestcomp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"clippy-testcomp-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        )
        .unwrap();
        // 库代码干净——轻量 clippy 看不出任何问题。
        std::fs::write(dir.join("src/lib.rs"), "pub fn probe() -> i32 { 1 }\n").unwrap();
        // 破损只在集成测试里:未定义符号。只有 --all-targets 的编译才会碰到它。
        std::fs::write(
            dir.join("tests/broken.rs"),
            "#[test]\nfn t() { let _: i32 = undefined_symbol_in_test_code(); }\n",
        )
        .unwrap();

        let err = clippy_gate(&dir).await.unwrap_err();
        assert!(
            err.contains("提交被拦下"),
            "测试代码编译不过必须拦下提交: {err}"
        );
        assert!(
            err.contains("broken.rs"),
            "应点名出错的测试文件(证明 --all-targets 编译覆盖仍在): {err}"
        );
        assert!(err.contains("-->"), "报错必须含 --> 位置: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-334 验收②:finalize 在测试**之前**先拦 fmt——构造 fmt 不过的源码,
    /// finalize 报 fmt gate 阶段,而不是先跑测试再在 commit 才拦。
    #[tokio::test]
    async fn finalize_rejects_fmt_before_tests() {
        let dir = temp_repo("finalize-fmt");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"finalize-fmt-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        // 故意不格式化:fmt gate 应拦截。
        std::fs::write(dir.join("src/lib.rs"), "pub fn  x( ) -> i32 { 1 }\n").unwrap();
        // test_record 需要项目骨架。
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();

        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };
        let out = GitTool
            .execute(
                serde_json::json!({
                    "action": "finalize",
                    "files": ["src/lib.rs", "Cargo.toml"],
                    "message": "finalize fmt gate test",
                }),
                &ctx,
            )
            .await;
        assert!(out.is_error, "fmt 不过必须拦下 finalize: {}", out.content);
        assert!(
            out.content.contains("fmt gate failed"),
            "应点名 fmt gate 阶段: {}",
            out.content
        );
        // 未被 stage、未提交——不留半状态。
        let status = run_git(&dir, &["status", "--porcelain"]).await.unwrap();
        assert!(
            status.contains("??") || status.contains(" M"),
            "fmt 拦截后不得 stage/commit: {status}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// D-334 成功路径:干净的最小工程,finalize 一次完成测试→record→stage→commit。
    /// 断言:返回 complete、commit 出现在 git log、test_record 有 passed 记录。
    #[tokio::test]
    async fn finalize_runs_tests_records_stages_and_commits() {
        let dir = temp_repo("finalize-ok");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"finalize-ok-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        // 干净且带一个会过的测试的源码(rustfmt 规范格式,过 fmt gate)。
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn x() -> i32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {\n        assert_eq!(crate::x(), 1);\n    }\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        // 初始提交(empty repo 无法 stage 后看 log;先提交 README 占位)。
        commit_file(&dir, "README.md", "finalize probe\n", "init");

        let ctx = ToolCtx {
            cwd: dir.clone(),
            project_root: dir.clone(),
            ..Default::default()
        };
        let out = GitTool
            .execute(
                serde_json::json!({
                    "action": "finalize",
                    "files": ["src/lib.rs", "Cargo.toml"],
                    "message": "finalize success test",
                }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "finalize 应成功: {}", out.content);
        assert!(out.content.contains("complete"), "{}", out.content);

        // commit 落地。
        let log = run_git(&dir, &["log", "--oneline", "-1"]).await.unwrap();
        assert!(
            log.contains("finalize success test"),
            "finalize 提交应出现在 log: {log}"
        );
        // test_record 有 passed 记录。
        let records = crate::test_record::test_runs_snapshot(&dir).unwrap();
        let text = serde_json::to_string(&records).unwrap_or_default();
        assert!(
            text.contains("git finalize (auto)") && text.contains("\"passed\""),
            "finalize 应写 passed test_record: {text}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// R-212 验收①:前端冒烟记录(非 Rust)不能背书 Rust 源码提交——时间戳比
    /// 改动新也不行,覆盖面不匹配即拦。
    #[test]
    fn source_test_gate_frontend_smoke_cannot_back_rust_change() {
        let root = temp_repo("gate-frontend");
        let project = root.join(".kanzei").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let src = root.join("crates/kanzei-tools/src/lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "pub fn x() {}\n").unwrap();
        // 最近 passed 记录是前端冒烟,收尾 = 现在(时序满足,只测相关性)。
        let now = now_secs();
        std::fs::write(
            project.join("tests.md"),
            format!(
                "# Test Runs\n\n## T-{now} 前端冒烟 [passed]\n- 命令: node scripts/ui-runtime-smoke.mjs\n- 收尾: {now}\n"
            ),
        )
        .unwrap();
        let err = source_test_gate(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        )
        .unwrap_err();
        assert!(err.contains("kanzei-tools"), "缺口应点名 crate: {err}");
        assert!(
            err.contains("前端冒烟") || err.contains("非 Rust"),
            "应指明记录覆盖面类型: {err}"
        );
        assert!(
            err.contains("cargo test -p kanzei-tools"),
            "应指明该跑什么: {err}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// R-212 验收②③:覆盖面与暂存 crate 求交——定向记录背书对应 crate、workspace
    /// 记录背书任意 crate、不匹配时拦截并点名缺口;非 crate 源码(scripts/)不受
    /// crate 相关性约束。
    #[test]
    fn source_test_gate_coverage_intersects_with_staged_crates() {
        let root = temp_repo("gate-coverage");
        let project = root.join(".kanzei").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let tools_src = root.join("crates/kanzei-tools/src/lib.rs");
        std::fs::create_dir_all(tools_src.parent().unwrap()).unwrap();
        std::fs::write(&tools_src, "pub fn x() {}\n").unwrap();
        let core_src = root.join("crates/kanzei-core/src/lib.rs");
        std::fs::create_dir_all(core_src.parent().unwrap()).unwrap();
        std::fs::write(&core_src, "pub fn y() {}\n").unwrap();
        // 收尾时间用写入时刻(实时时钟)而非测试开头固定 now:ps1 在测试末尾写入时,
        // 收尾时间必须晚于/等于其 mtime,否则 mtime 分支会确定性拦截(跨秒竞态)。
        let write_record = |command: &str| {
            let t = now_secs();
            std::fs::write(
                project.join("tests.md"),
                format!("# Test Runs\n\n## T-{t} 记录 [passed]\n- 命令: {command}\n- 收尾: {t}\n"),
            )
            .unwrap();
        };
        // 定向记录背书对应 crate。
        write_record("cargo test -p kanzei-tools");
        assert!(
            source_test_gate(
                &root,
                &root,
                &["crates/kanzei-tools/src/lib.rs".to_string()]
            )
            .is_ok(),
            "定向记录必须背书对应 crate"
        );
        // workspace 记录背书任意 crate。
        write_record("cargo test --workspace");
        assert!(
            source_test_gate(&root, &root, &["crates/kanzei-core/src/lib.rs".to_string()]).is_ok(),
            "workspace 记录必须背书任意 crate"
        );
        // 不匹配:kanzei-core 记录背书不了 kanzei-tools 改动,拦截文案点名缺口。
        write_record("cargo test -p kanzei-core");
        let err = source_test_gate(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        )
        .unwrap_err();
        assert!(err.contains("kanzei-tools"), "应点名暂存 crate: {err}");
        assert!(
            err.contains("kanzei-core"),
            "应指出记录覆盖了别的 crate: {err}"
        );
        assert!(
            err.contains("cargo test -p kanzei-tools"),
            "应指明该跑什么: {err}"
        );
        // 非 crate 源码(scripts/)不受 crate 相关性约束,前端记录可背书。
        let ps1 = root.join("scripts/hello.ps1");
        std::fs::create_dir_all(ps1.parent().unwrap()).unwrap();
        std::fs::write(&ps1, "Write-Host hi\n").unwrap();
        // 注意:write_record 用测试开头的固定 now 作收尾时间;ps1 写入必须与 now
        // 同秒才能让 mtime 分支放行——测试在秒内完成,既有设计,勿加 sleep。
        write_record("node scripts/ui-runtime-smoke.mjs");
        match source_test_gate(&root, &root, &["scripts/hello.ps1".to_string()]) {
            Ok(()) => {}
            Err(err) => panic!("非 crate 源码不应被 crate 相关性拦截: {err}"),
        }
        std::fs::remove_dir_all(root).ok();
    }

    /// D-332 验收④:test_record 收尾记录暂存源码指纹;source_test_gate 优先比指纹——
    /// 指纹一致即背书成立(不再被 test_record 自己写 tests.md 的 mtime 误伤);
    /// 源码改动(fmt/手改)后指纹不一致则拦截。
    #[test]
    fn source_test_gate_prefers_fingerprint_over_mtime() {
        let root = temp_repo("gate-fingerprint");
        let project = root.join(".kanzei").join("project");
        std::fs::create_dir_all(&project).unwrap();
        let src = root.join("crates/kanzei-tools/src/lib.rs");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, "pub fn x() {}\n").unwrap();
        // 提交初始版本,再改源码并 stage —— 模拟「改完代码准备提交」。
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);
        git_in(&root, &["commit", "-m", "init"]);
        std::fs::write(&src, "pub fn x() { let a = 1; }\n").unwrap();
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);

        // 收尾时记录的指纹(与当前 staged 源码一致)。
        let fp = staged_source_fingerprint(&root).unwrap();
        assert!(!fp.is_empty(), "有暂存源码就必须有指纹");
        let now = now_secs();
        // 记录收尾时间设为「过去」(源码 mtime 更新),但指纹一致 → 应放行。
        std::fs::write(
            project.join("tests.md"),
            format!(
                "# Test Runs\n\n## T-{now} 记录 [passed]\n- 命令: cargo test -p kanzei-tools\n- 收尾: {}\n- 源码指纹: {fp}\n",
                now - 99999
            ),
        )
        .unwrap();
        assert!(
            source_test_gate(
                &root,
                &root,
                &["crates/kanzei-tools/src/lib.rs".to_string()]
            )
            .is_ok(),
            "指纹一致时,即使收尾时间早于源码 mtime 也应放行(test_record 写 tests.md 不误伤)"
        );

        // 源码再改(未重测)→ 指纹不一致 → 拦截。
        std::fs::write(&src, "pub fn x() { let b = 2; }\n").unwrap();
        git_in(&root, &["add", "crates/kanzei-tools/src/lib.rs"]);
        let err = source_test_gate(
            &root,
            &root,
            &["crates/kanzei-tools/src/lib.rs".to_string()],
        )
        .unwrap_err();
        assert!(
            err.contains("源码指纹") && err.contains("不一致"),
            "指纹不一致应拦截并点名: {err}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// D-332:staged_source_fingerprint 只对源码路径求 hash——只 stage tests.md(非源码)
    /// 时指纹为空,门禁退回 mtime 逻辑,不产生「空指纹 vs 有指纹」的误判。
    #[test]
    fn staged_source_fingerprint_ignores_non_source_paths() {
        let root = temp_repo("gate-fp-nonsource");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(root.join(".kanzei/project/tests.md"), "# Test Runs\n").unwrap();
        git_in(&root, &["add", ".kanzei/project/tests.md"]);
        assert_eq!(
            staged_source_fingerprint(&root).unwrap(),
            "",
            "只有非源码暂存时指纹应为空(门禁走 mtime)"
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// R-261:纯前端资源(kanzei-app/ui/ 下 js/css/html)不算 Rust source——
    /// 它们由前端冒烟集背书,不要求 cargo test -p kanzei-app 跑全套 Rust 测试。
    /// staged 同时含 Rust 源码时,Rust 部分仍按原规则要求测试背书。
    #[test]
    fn 纯前端ui资源不算rust源码_门禁放行而rust源码规则不变() {
        assert!(!is_source_path("crates/kanzei-app/ui/01-core.js"));
        assert!(!is_source_path("crates/kanzei-app/ui/style.css"));
        assert!(!is_source_path("crates/kanzei-app/ui/index.html"));
        assert!(
            is_source_path("crates/kanzei-tools/src/lib.rs"),
            "Rust 源码仍算 source"
        );
        assert!(is_source_path("scripts/verify.ps1"), "scripts 仍算 source");

        // 纯前端 staged:source_test_gate 放行(无 Rust 源码 → 不需要测试背书)。
        let root = temp_repo("gate-frontend-only");
        std::fs::create_dir_all(root.join("crates/kanzei-app/ui")).unwrap();
        std::fs::write(
            root.join("crates/kanzei-app/ui/01-core.js"),
            "console.log('x');\n",
        )
        .unwrap();
        git_in(&root, &["add", "crates/kanzei-app/ui/01-core.js"]);
        assert_eq!(
            staged_source_fingerprint(&root).unwrap(),
            "",
            "纯前端暂存指纹应为空(不算 Rust source)"
        );
        assert!(
            source_test_gate(
                &root,
                &root,
                &["crates/kanzei-app/ui/01-core.js".to_string()],
            )
            .is_ok(),
            "纯前端改动不得被 source_test_gate 拦"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
