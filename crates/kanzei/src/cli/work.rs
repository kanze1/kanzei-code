//! `kz work` 取活裁决(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:work 是「结构化取活裁决」的命令面——`next` 取活、`claim` 原子占用,
//! 与 run/tracker 正交;拆出后 work 状态机变更不必读懂 run 的装配(照 files_view.rs 模式)。

use kanzei_harness::{Tool, ToolCtx};

use super::{explicit_main_root, main_project_root};

pub(crate) async fn work_cli(args: &[String]) -> anyhow::Result<()> {
    let cli_action = args.first().map(String::as_str).unwrap_or("next");
    let action = cli_action.replace('-', "_");
    if !matches!(
        action.as_str(),
        "next"
            | "claim"
            | "create_unit"
            | "get_unit"
            | "list_units"
            | "checkpoint"
            | "block"
            | "unblock"
            | "verify"
            | "evidence"
            | "complete"
            | "supersede"
    ) {
        anyhow::bail!("未知 work action `{cli_action}`；运行 `kz help` 查看可用动作");
    }
    let priority = if args.iter().any(|arg| arg == "--requirement-first") {
        kanzei_harness::auto_run::WorkPriority::RequirementFirst
    } else {
        kanzei_harness::auto_run::WorkPriority::DefectFirst
    };
    let mut input = serde_json::json!({"action": action});
    let id_actions = [
        "claim",
        "get_unit",
        "checkpoint",
        "block",
        "unblock",
        "verify",
        "evidence",
        "complete",
        "supersede",
    ];
    if id_actions.contains(&action.as_str()) {
        let id = args
            .get(1)
            .filter(|id| !id.starts_with('-'))
            .ok_or_else(|| anyhow::anyhow!("work {cli_action} 需要条目 ID"))?;
        input["id"] = serde_json::json!(id);
    }
    for (flag, key) in [
        ("--requirement", "requirement_id"),
        ("--objective", "objective"),
        ("--base-revision", "base_revision"),
        ("--summary", "summary"),
        ("--next-action", "next_action"),
        ("--criterion", "criterion"),
        ("--reason", "reason"),
    ] {
        if let Some(position) = args.iter().position(|arg| arg == flag) {
            let value = args
                .get(position + 1)
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| anyhow::anyhow!("{flag} 需要值"))?;
            input[key] = serde_json::json!(value);
        }
    }
    for (flag, key) in [
        ("--scope", "scope"),
        ("--depends-on", "dependencies"),
        ("--acceptance", "acceptance"),
        ("--verify-with", "verification"),
        ("--decision", "decisions"),
        ("--retrieval-ref", "retrieval_refs"),
        ("--evidence", "evidence_refs"),
    ] {
        let values = args
            .iter()
            .enumerate()
            .filter(|(_, arg)| arg.as_str() == flag)
            .map(|(position, _)| {
                args.get(position + 1)
                    .filter(|value| !value.starts_with("--"))
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("{flag} 需要值"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        if !values.is_empty() {
            input[key] = serde_json::json!(values);
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
