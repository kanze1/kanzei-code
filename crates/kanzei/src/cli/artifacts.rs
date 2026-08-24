//! `kz artifacts` R-245 B2:只读查看运行状态库、artifact 与 shadow telemetry 占用。
//!
//! 这里故意不复用 SessionStore::open：普通 open 会执行迁移 housekeeping，统计入口
//! 必须只读。清理、VACUUM、checkpoint 和备份处置留给后续显式整理批次。

use std::path::PathBuf;

use super::{explicit_main_root, main_project_root, PROJECT_ROOT_FLAG};

#[derive(Debug, Default, PartialEq, Eq)]
struct ArtifactArgs {
    json: bool,
    plan: bool,
    delete: bool,
    session_id: Option<String>,
    confirm: bool,
    project_root: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> anyhow::Result<ArtifactArgs> {
    let mut parsed = ArtifactArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "stats" => {
                parsed.plan = false;
                parsed.delete = false;
            }
            "plan" | "--dry-run" => parsed.plan = true,
            "delete" => parsed.delete = true,
            "--session" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow::anyhow!("--session 需要会话 id"))?;
                parsed.session_id = Some(value.clone());
                index += 1;
            }
            "--confirm" => parsed.confirm = true,
            "--json" => parsed.json = true,
            PROJECT_ROOT_FLAG => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow::anyhow!("--project-root 需要路径"))?;
                parsed.project_root = Some(PathBuf::from(value));
                index += 1;
            }
            other => anyhow::bail!(
                "未知 artifacts 参数: {other}; 用法: kz artifacts stats [--json] | kz artifacts plan --dry-run [--json] [--project-root <path>] | kz artifacts delete --session <id> [--dry-run|--confirm] [--json]"
            ),
        }
        index += 1;
    }
    Ok(parsed)
}

pub(crate) async fn artifacts_cli(args: &[String]) -> anyhow::Result<()> {
    let parsed = parse_args(args)?;
    let cwd = std::env::current_dir()?;
    let project_root = main_project_root(
        explicit_main_root(parsed.project_root.as_deref()).as_deref(),
        &cwd,
    )?;
    let state_path = kanzei_core::project_state_path(&project_root);
    let store = kanzei_core::SessionStore::open_read_only(&state_path)?;
    if parsed.delete {
        let session_id = parsed
            .session_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("artifacts delete 需要 --session <id>"))?;
        let plan = store.session_deletion_plan(session_id, &project_root)?;
        if !parsed.confirm || parsed.plan {
            if parsed.json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                println!("session: {}", plan.session_id);
                println!("eligible: {}", plan.eligible);
                println!(
                    "events: {} | inputs: {} | episodes: {}",
                    plan.event_count, plan.input_count, plan.episode_count
                );
                println!(
                    "recall events: {} | memory sources: {}",
                    plan.recall_event_count, plan.memory_source_count
                );
                println!(
                    "target artifacts: {} | deletable artifacts: {}",
                    plan.target_artifacts.len(),
                    plan.deletable_artifacts.len()
                );
                if let Some(reason) = &plan.blocked_reason {
                    println!("blocked: {reason}");
                }
                println!(
                    "mode: dry-run; pass --confirm to delete the session and eligible artifacts"
                );
            }
            return Ok(());
        }
        let result = {
            drop(store);
            let store = kanzei_core::SessionStore::open(&state_path)?;
            store.delete_session(session_id, &project_root)?
        };
        if parsed.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("deleted session: {}", result.session_id);
            println!(
                "events: {} | inputs: {} | episodes: {}",
                result.deleted_events, result.deleted_inputs, result.deleted_episodes
            );
            println!(
                "recall events: {} | memory sources: {}",
                result.deleted_recall_events, result.deleted_memory_sources
            );
            println!("artifacts deleted: {}", result.deleted_artifacts.len());
            for error in &result.artifact_cleanup_errors {
                println!("artifact cleanup pending: {error}");
            }
        }
        return Ok(());
    }
    if parsed.plan {
        let plan = store.artifact_cleanup_plan(&project_root)?;
        if parsed.json {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        } else {
            println!(
                "artifact files: {} / {} bytes",
                plan.total_artifact_files, plan.total_artifact_bytes
            );
            println!(
                "referenced: {} / {} bytes",
                plan.referenced_artifact_files, plan.referenced_artifact_bytes
            );
            println!(
                "unreferenced: {} / {} bytes (estimated reclaim)",
                plan.unreferenced_artifact_files, plan.unreferenced_artifact_bytes
            );
            for file in &plan.unreferenced {
                println!(
                    "- {} | {} bytes | references: {}",
                    file.relative_path, file.bytes, file.reference_count
                );
            }
            println!("mode: dry-run; no artifact deletion, database mutation or expiry performed");
        }
        return Ok(());
    }
    let report = store.storage_report(&project_root)?;
    if parsed.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("项目根: {}", project_root.display());
        println!("state.db bytes: {}", report.state_db_bytes);
        println!("state.db-wal bytes: {}", report.wal_bytes);
        println!("state.db-shm bytes: {}", report.shm_bytes);
        println!(
            "sqlite pages: {} | freelist pages: {}",
            report.page_count, report.freelist_pages
        );
        println!(
            "artifacts: {} files / {} bytes",
            report.artifact_files, report.artifact_bytes
        );
        println!(
            "unreferenced artifacts: {} files / {} bytes",
            report.unreferenced_artifact_files, report.unreferenced_artifact_bytes
        );
        println!(
            "shadow telemetry: {} files / {} bytes",
            report.shadow_files, report.shadow_bytes
        );
        println!(
            "migration backups: {} files / {} bytes",
            report.migration_backup_files, report.migration_backup_bytes
        );
        println!("mode: read-only; no expiry, delete, checkpoint or VACUUM performed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_args, ArtifactArgs};
    use std::path::PathBuf;

    #[test]
    fn parses_read_only_stats_flags() {
        let args = vec![
            "stats".into(),
            "--json".into(),
            "--project-root".into(),
            "C:/project".into(),
        ];
        assert_eq!(
            parse_args(&args).unwrap(),
            ArtifactArgs {
                json: true,
                plan: false,
                delete: false,
                session_id: None,
                confirm: false,
                project_root: Some(PathBuf::from("C:/project")),
            }
        );
    }

    #[test]
    fn parses_session_delete_confirmation_and_dry_run() {
        let args = vec![
            "delete".into(),
            "--session".into(),
            "ses-1".into(),
            "--confirm".into(),
            "--json".into(),
        ];
        assert_eq!(
            parse_args(&args).unwrap(),
            ArtifactArgs {
                json: true,
                plan: false,
                delete: true,
                session_id: Some("ses-1".into()),
                confirm: true,
                project_root: None,
            }
        );
    }

    #[test]
    fn parses_explicit_dry_run_plan() {
        let args = vec!["plan".into(), "--dry-run".into(), "--json".into()];
        assert_eq!(
            parse_args(&args).unwrap(),
            ArtifactArgs {
                json: true,
                plan: true,
                delete: false,
                session_id: None,
                confirm: false,
                project_root: None,
            }
        );
    }
}
