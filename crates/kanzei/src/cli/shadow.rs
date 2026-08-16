//! `kz shadow` 会话投影 shadow gate 统计(R-242 批4)。
//!
//! 只读诊断命令:对项目 state.db 的 `session.shadow_compared` 事件做达标统计,
//! 按「未知差异=0、typed_write_errors=0」口径(验收⑤)输出 ShadowVerdictStats。
//! 不写任何数据,不改运行态;`--mismatches` 列出每条 equal=false 的差异明细
//! (类别:failed_turn / empty_legacy / stale_snapshot / unknown),供审计归因。

use std::path::PathBuf;

use super::{explicit_main_root, main_project_root};

pub(crate) async fn shadow_cli(args: &[String]) -> anyhow::Result<()> {
    let mut root_override: Option<&str> = None;
    let mut list_mismatches = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--project-root" => {
                i += 1;
                root_override = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--project-root 需要路径"))?
                        .as_str(),
                );
            }
            "--mismatches" => list_mismatches = true,
            other => anyhow::bail!("未知参数: {other}"),
        }
        i += 1;
    }

    let cwd = std::env::current_dir()?;
    let project_root = main_project_root(
        explicit_main_root(root_override.map(PathBuf::from).as_deref()).as_deref(),
        &cwd,
    )?;
    let state_path = kanzei_core::project_state_path(&project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    let session_id = kanzei_core::project_session_id(&project_root);
    let events = store.list_events_by_type(&session_id, 0, "session.shadow_compared")?;
    let stats = kanzei_core::summarize_shadow_reports(&events);
    println!("项目根: {}", project_root.display());
    println!("会话:   {session_id}");
    println!(
        "shadow 对比: 共 {} turn | equal {} | 预期差异 {} | 未知差异 {} | 写错误轮 {}",
        stats.total,
        stats.equal,
        stats.expected_mismatch,
        stats.unknown_mismatch,
        stats.typed_write_error_turns
    );
    if stats.total > 0 && stats.unknown_mismatch == 0 && stats.typed_write_error_turns == 0 {
        println!("判定:   达标(未知差异=0, 写错误=0)");
    } else {
        println!("判定:   未达标(见上方计数)");
    }
    if list_mismatches {
        for event in events
            .iter()
            .filter(|event| event.payload["equal"].as_bool() == Some(false))
        {
            let turn = event.payload["turn_id"].as_str().unwrap_or("-");
            let expected = event.payload["expected_mismatch"]
                .as_bool()
                .unwrap_or(false);
            let class = event.payload["mismatch_class"]
                .as_str()
                .unwrap_or("unknown");
            let first = event.payload["first_mismatch"]
                .as_u64()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into());
            println!(
                "  seq {:<8} turn {:<24} equal=false expected={:<5} class={:<14} first_mismatch={}",
                event.sequence, turn, expected, class, first
            );
        }
    }
    Ok(())
}
