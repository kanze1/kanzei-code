//! `kz artifacts` R-245 B2:只读查看运行状态库、artifact 与 shadow telemetry 占用。
//!
//! 这里故意不复用 SessionStore::open：普通 open 会执行迁移 housekeeping，统计入口
//! 必须只读。清理、VACUUM、checkpoint 和备份处置留给后续显式整理批次。

use std::path::PathBuf;

use super::{explicit_main_root, main_project_root, PROJECT_ROOT_FLAG};

#[derive(Debug, Default, PartialEq, Eq)]
struct ArtifactArgs {
    json: bool,
    project_root: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> anyhow::Result<ArtifactArgs> {
    let mut parsed = ArtifactArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "stats" => {}
            "--json" => parsed.json = true,
            PROJECT_ROOT_FLAG => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow::anyhow!("--project-root 需要路径"))?;
                parsed.project_root = Some(PathBuf::from(value));
                index += 1;
            }
            other => anyhow::bail!(
                "未知 artifacts 参数: {other}; 用法: kz artifacts stats [--json] [--project-root <path>]"
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
            "shadow telemetry: {} files / {} bytes",
            report.shadow_files, report.shadow_bytes
        );
        println!(
            "migration backups: {} files / {} bytes",
            report.migration_backup_files, report.migration_backup_bytes
        );
        println!("unreferenced artifacts: pending explicit cleanup batch");
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
                project_root: Some(PathBuf::from("C:/project")),
            }
        );
    }
}
