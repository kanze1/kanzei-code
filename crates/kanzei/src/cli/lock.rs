//! `kz lock status` 外部写入者可见性(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:lock 是「跨进程写租约撤销后剩余的外部写入者可见性」只读通道——主根/git
//! 工作树改动/活跃线,不做互斥(协作式可见性,R-181 降级交付);拆出后可见性报告口径
//! 变更不必读懂 run 的装配(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):`lock_status_report` 是纯函数(可测),只读不阻塞;检测 ≠ 互斥,
//! 真正的隔离是 worktree(R-182)。

use std::path::Path;

use super::{explicit_main_root, main_project_root};

pub(crate) async fn lock_cli(args: &[String]) -> anyhow::Result<()> {
    let action = args.first().map(String::as_str).unwrap_or("status");
    if action != "status" {
        anyhow::bail!(
            "kz lock 只支持 status(跨进程写租约已由 R-182 撤销;剩余价值是外部写入者可见性)。\
             用法:`kz lock status`"
        );
    }
    let cwd = std::env::current_dir()?;
    let project_root = main_project_root(explicit_main_root(None).as_deref(), &cwd)?;
    let report = lock_status_report(&project_root, &cwd);
    for line in &report {
        println!("{line}");
    }
    Ok(())
}

/// `kz lock status` 的可见性报告(纯函数,可测试)。
///
/// 返回逐行文本:主根、cwd、git 工作树未提交改动、活跃线。只读,不阻塞。
pub(crate) fn lock_status_report(project_root: &Path, cwd: &Path) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!("project-root: {}", project_root.display()));
    out.push(format!("cwd: {}", cwd.display()));

    // git 工作树未提交改动 = 任何写入者(含外部 agent)留下的可见痕迹。
    // 可见性入口专用只读命令,不进 agent 工具链,无权限门禁问题;输出原样透传。
    let git_status = std::process::Command::new("git")
        .args(["status", "--short", "--branch"])
        .current_dir(project_root)
        .output();
    match git_status {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() || text == "(clean worktree)" {
                out.push("工作树: clean(无未提交改动)".into());
            } else {
                out.push("工作树未提交改动(外部 agent 或本进程可能正在写):".into());
                for line in text.lines() {
                    out.push(format!("  {line}"));
                }
            }
        }
        Ok(output) => out.push(format!(
            "git status 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => out.push(format!("git status 不可用(可见性降级,不阻塞): {error}")),
    }

    // 活跃线:state.db processes 表里 origin 为本项目根的登记。
    let state_path = kanzei_core::project_state_path(project_root);
    if state_path.exists() {
        match kanzei_core::SessionStore::open(&state_path) {
            Ok(store) => {
                let root_str = project_root.display().to_string();
                match store.list_processes(&root_str) {
                    Ok(processes) if !processes.is_empty() => {
                        out.push(format!("活跃线/进程({}):", processes.len()));
                        for process in &processes {
                            let branch = process.worktree_path.as_deref().unwrap_or("(主树)");
                            out.push(format!(
                                "  {} · 分支/树 {} · updated {}",
                                process.process_id, branch, process.updated_at
                            ));
                        }
                    }
                    Ok(_) => out.push("活跃线: 无(本主根当前没有登记进程)".into()),
                    Err(error) => out.push(format!("活跃线查询失败: {error}")),
                }
            }
            Err(error) => out.push(format!("state.db 打开失败(只读可见性降级,不阻塞): {error}")),
        }
    } else {
        out.push("活跃线: 无 state.db(只读可见性,不阻塞)".into());
    }
    out.push(
        "提示:检测 ≠ 互斥。外部 agent 动仓库前看这份状态;真正的隔离是 worktree(R-182)。".into(),
    );
    out
}
