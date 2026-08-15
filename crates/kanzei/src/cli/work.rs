//! `kz work` 取活裁决(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:work 是「结构化取活裁决」的命令面——`next` 取活、`claim` 原子占用,
//! 与 run/tracker 正交;拆出后 work 状态机变更不必读懂 run 的装配(照 files_view.rs 模式)。

use kanzei_harness::{Tool, ToolCtx};

use super::{explicit_main_root, main_project_root};

pub(crate) async fn work_cli(args: &[String]) -> anyhow::Result<()> {
    let action = args.first().map(String::as_str).unwrap_or("next");
    if !matches!(action, "next" | "claim") {
        anyhow::bail!("work action 必须是 next 或 claim");
    }
    let priority = if args.iter().any(|arg| arg == "--requirement-first") {
        kanzei_harness::auto_run::WorkPriority::RequirementFirst
    } else {
        kanzei_harness::auto_run::WorkPriority::DefectFirst
    };
    let mut input = serde_json::json!({"action": action});
    if action == "claim" {
        let id = args
            .get(1)
            .filter(|id| !id.starts_with('-'))
            .ok_or_else(|| anyhow::anyhow!("work claim 需要 R-xxx 或 D-xxx"))?;
        input["id"] = serde_json::json!(id);
        if let Some(position) = args.iter().position(|arg| arg == "--reason") {
            if let Some(reason) = args.get(position + 1) {
                input["reason"] = serde_json::json!(reason);
            }
        }
    }
    let cwd = std::env::current_dir()?;
    let project_root = main_project_root(explicit_main_root(None).as_deref(), &cwd)?;
    let ctx = ToolCtx::new(cwd, project_root).with_work_priority(priority);
    let output = kanzei_tools::WorkTool.execute(input, &ctx).await;
    if output.is_error {
        anyhow::bail!(output.content);
    }
    println!("{}", output.content);
    Ok(())
}
