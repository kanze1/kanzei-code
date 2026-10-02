//! `kz worktree` 建线/合并(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:worktree CLI 是「桌面 processes.rs 同一份内核」的命令面——建线、
//! 合并前冲突预检与安全合并均复用 `kanzei_tools::worktree`,不复制 git plumbing;
//! 拆出后 worktree 生命周期变更不必读懂 run 的装配。

use super::{explicit_main_root, main_project_root, usage};

/// R-207 验收①:CLI 与桌面共用同一 worktree 实现(建线/合并预检)。
///
/// 只读/建线操作全部走 `kanzei_tools::worktree`(桌面 processes.rs 的同一份内核),
/// 不复制 git plumbing;跨进程并发安全由 git ref CAS 保证,与桌面一致。
pub(crate) async fn worktree_cli(args: &[String]) -> anyhow::Result<()> {
    let root_flag = args
        .iter()
        .position(|arg| arg == "--project-root")
        .and_then(|idx| args.get(idx + 1))
        .cloned();
    let cwd = std::env::current_dir()?;
    let root_flag_path = root_flag.as_deref().map(std::path::Path::new);
    let project_root = main_project_root(explicit_main_root(root_flag_path).as_deref(), &cwd)?;
    match args.first().map(String::as_str) {
        Some("create") => {
            let name = args.get(1).ok_or_else(|| {
                anyhow::anyhow!("用法: kz worktree create <name> [--project-root <path>]")
            })?;
            let (info, _receipt) =
                kanzei_tools::worktree::create_worktree_with_receipt(&project_root, name)
                    .map_err(anyhow::Error::msg)?;
            println!("已建工作树: {}", info.path);
            println!("分支: {}", info.branch);
            println!("clean: {}", info.clean);
            Ok(())
        }
        Some("merge") => {
            let path = args.get(1).ok_or_else(|| {
                anyhow::anyhow!("用法: kz worktree merge <worktree-path> [--project-root <path>]")
            })?;
            let result = kanzei_tools::worktree::merge_worktree(&project_root, path)
                .map_err(anyhow::Error::msg)?;
            println!("{result}");
            Ok(())
        }
        Some("merge-preview") => {
            let path = args.get(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "用法: kz worktree merge-preview <worktree-path> [--project-root <path>]"
                )
            })?;
            let worktree = kanzei_tools::worktree::validate_worktree_path(&project_root, path)
                .map_err(anyhow::Error::msg)?;
            let branch = kanzei_tools::worktree::worktree_current_branch(&worktree)
                .map_err(anyhow::Error::msg)?;
            let check = kanzei_tools::worktree::worktree_command(
                &project_root,
                &["merge-tree", "--write-tree", "HEAD", &branch],
            )
            .map_err(anyhow::Error::msg)?;
            if check.status.success() {
                println!("无冲突,可安全合并");
            } else {
                let conflicts = kanzei_tools::worktree::parse_merge_tree_conflicts(&check.stdout);
                println!("合并前冲突检测失败,双方改动已保留。冲突文件:");
                for conflict in &conflicts {
                    println!("  {conflict}");
                }
            }
            Ok(())
        }
        _ => {
            usage();
            anyhow::bail!(
                "用法: kz worktree <create <name>|merge-preview <path>|merge <path>> [--project-root <path>]"
            )
        }
    }
}
