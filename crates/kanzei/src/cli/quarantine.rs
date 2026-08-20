//! `kz quarantine` 取证清理命令(D-566)。

use std::path::PathBuf;

use super::{explicit_main_root, main_project_root, PROJECT_ROOT_FLAG};

#[derive(Debug, Default, PartialEq, Eq)]
struct QuarantineArgs {
    apply: bool,
    kind: Option<String>,
    older_than_days: Option<u64>,
    project_root: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> anyhow::Result<QuarantineArgs> {
    let mut parsed = QuarantineArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--apply" => parsed.apply = true,
            "--dry-run" => {
                if parsed.apply {
                    anyhow::bail!("--apply 与 --dry-run 不能同时使用");
                }
            }
            "--type" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow::anyhow!("--type 需要 shell|shell-with-log|cross-tree|bg"))?;
                parsed.kind = Some(value.clone());
                index += 1;
            }
            "--older-than-days" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    anyhow::anyhow!("--older-than-days 需要非负整数")
                })?;
                parsed.older_than_days = Some(value.parse().map_err(|_| {
                    anyhow::anyhow!("--older-than-days 需要非负整数: {value}")
                })?);
                index += 1;
            }
            PROJECT_ROOT_FLAG => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow::anyhow!("--project-root 需要路径"))?;
                parsed.project_root = Some(PathBuf::from(value));
                index += 1;
            }
            other => anyhow::bail!(
                "未知 quarantine 参数: {other}; 用法: kz quarantine [--dry-run|--apply] [--type <kind>] [--older-than-days N] [--project-root <path>]"
            ),
        }
        index += 1;
    }
    Ok(parsed)
}

/// `kz quarantine` 默认只盘点；实际删除必须显式 `--apply` 且带筛选条件。
pub(crate) async fn quarantine_cli(args: &[String]) -> anyhow::Result<()> {
    let parsed = parse_args(args)?;
    let cwd = std::env::current_dir()?;
    let project_root = main_project_root(
        explicit_main_root(parsed.project_root.as_deref()).as_deref(),
        &cwd,
    )?;
    let before_ms = parsed.older_than_days.map(|days| {
        kanzei_tools::quarantine::now_ms()
            .saturating_sub(u128::from(days).saturating_mul(86_400_000))
    });
    let report = kanzei_tools::quarantine::cleanup(
        &project_root,
        parsed.kind.as_deref(),
        before_ms,
        parsed.apply,
    )?;
    println!("mode: {}", if report.dry_run { "dry-run" } else { "apply" });
    println!("scanned_dirs: {}", report.scanned_dirs);
    println!("eligible_dirs: {}", report.eligible_dirs);
    println!("eligible_bytes: {}", report.eligible_bytes);
    println!("removed_dirs: {}", report.removed_dirs);
    println!("freed_bytes: {}", report.freed_bytes);
    println!("preserved_dirs: {}", report.preserved_dirs);
    for path in report.preserved_paths {
        println!("preserved: {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认是dry_run且可解析类型日期和主根() {
        let parsed = parse_args(&[
            "--type".into(),
            "cross-tree".into(),
            "--older-than-days".into(),
            "30".into(),
            "--project-root".into(),
            "C:/project".into(),
        ])
        .unwrap();
        assert!(!parsed.apply);
        assert_eq!(parsed.kind.as_deref(), Some("cross-tree"));
        assert_eq!(parsed.older_than_days, Some(30));
        assert_eq!(parsed.project_root, Some(PathBuf::from("C:/project")));
    }

    #[test]
    fn apply与dry_run互斥且apply允许解析() {
        assert!(parse_args(&["--apply".into(), "--dry-run".into()]).is_err());
        assert!(parse_args(
            &["--apply", "--type", "bg"]
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>()
        )
        .is_ok());
    }
}
