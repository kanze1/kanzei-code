//! Validate declared inputs before a claim changes task state.
use super::*;
use kanzei_harness::permission::ScopedFile;
use kanzei_harness::ToolCtx;

pub(super) fn preflight(ctx: &ToolCtx, entry: &Entry) -> Result<serde_json::Value, String> {
    let identity =
        kanzei_harness::execution::ExecutionIdentity::capture(&ctx.cwd, &ctx.project_root)
            .map_err(|error| error.to_string())?;
    let mut source = None;
    if let Some(spec) = crate::docstore::requirement::RequirementSpec::from_entry(entry)? {
        let reference = spec.source.reference.trim();
        let path = reference.split('#').next().unwrap_or(reference);
        if !path.contains("://")
            && !path.is_empty()
            && (std::path::Path::new(path).is_absolute()
                || std::path::Path::new(path).extension().is_some())
        {
            if let Some(rules) = kanzei_harness::permission::execution_rules() {
                let resource = kanzei_harness::permission::FileResource::resolve(
                    &ctx.cwd,
                    std::path::Path::new(path),
                )
                .map_err(|error| error.to_string())?;
                if rules.evaluate_file_with_rule("read", &resource).0
                    == kanzei_harness::Effect::Deny
                {
                    return Err(format!(
                        "规范尚未取得读取授权，先通过 read 核对规范: {reference}"
                    ));
                }
            }
            let file = ScopedFile::resolve(&ctx.cwd, &ctx.project_root, std::path::Path::new(path))
                .map_err(|error| format!("规范不在本轮可读范围，请先绑定正确工作区或提供只读规范资源: {reference}: {error}"))?;
            let bytes = file
                .read_to_string()
                .map_err(|error| format!("规范尚未就绪，不能开始执行: {reference}: {error}"))?;
            source = Some(
                json!({"reference":reference,"path":file.path(),"content_hash":crate::content_hash(bytes.as_bytes())}),
            );
        }
    }
    Ok(json!({"identity":identity,"specification":source,"run_id":ctx.run_id}))
}

pub(super) fn check_revision(ctx: &ToolCtx, revision: &str) -> Result<(), String> {
    let identity =
        kanzei_harness::execution::ExecutionIdentity::capture(&ctx.cwd, &ctx.project_root)
            .map_err(|error| error.to_string())?;
    if identity
        .source
        .as_ref()
        .and_then(|source| source.head.as_ref())
        .is_some()
        && !revision.trim().is_empty()
    {
        let mut command = std::process::Command::new("git");
        crate::hide_console(&mut command);
        let resolved = command
            .current_dir(&ctx.cwd)
            .args([
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{revision}^{{commit}}"),
            ])
            .output()
            .map_err(|error| error.to_string())?;
        if !resolved.status.success() {
            return Err(format!("Work Unit 的基线不是可读取的提交: {revision}"));
        }
        let commit = String::from_utf8(resolved.stdout).map_err(|error| error.to_string())?;
        let mut command = std::process::Command::new("git");
        crate::hide_console(&mut command);
        let output = command
            .current_dir(&ctx.cwd)
            .args(["merge-base", "--is-ancestor", commit.trim(), "HEAD"])
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "执行工作区未包含 Work Unit 的基线 {revision}；先绑定正确工作树，再开始执行。{}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_or_foreign_spec_cannot_be_claimed_as_ready() {
        let dir = kanzei_base::test_support::TestDir::new("kz-claim-inputs-");
        let foreign = kanzei_base::test_support::TestDir::new("kz-foreign-inputs-");
        std::fs::write(foreign.join("design.md"), "new design").unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let mut entry = Entry {
            id: "R-001".into(),
            title: "unit".into(),
            status: "todo".into(),
            severity: None,
            fields: vec![],
        };
        let spec = crate::docstore::requirement::RequirementSpec {
            source: crate::docstore::requirement::RequirementSource {
                reference: foreign.join("design.md").display().to_string(),
                quote: "implement".into(),
            },
            ..Default::default()
        };
        entry.fields.extend(spec.fields());
        assert!(preflight(&ctx, &entry).is_err());
        let mut spec = spec;
        spec.source.reference = "design.md".into();
        entry.fields = spec.fields().into_iter().collect();
        assert!(preflight(&ctx, &entry).is_err());
        std::fs::write(dir.join("design.md"), "ready").unwrap();
        let result = preflight(&ctx, &entry).unwrap();
        assert!(result["specification"]["content_hash"].as_str().is_some());
        assert_eq!(entry.status, "todo");
    }

    #[tokio::test]
    async fn requirement_id_cannot_be_completed_as_a_work_unit() {
        use kanzei_harness::Tool;
        let dir = kanzei_base::test_support::TestDir::new("kz-unit-id-");
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let result = super::super::tool::WorkTool
            .execute(json!({"action":"complete","id":"R-034"}), &ctx)
            .await;
        assert_eq!(result.outcome, kanzei_harness::ToolOutcome::NeedsCorrection);
        assert_eq!(result.code.as_deref(), Some("WORK_UNIT_ID_REQUIRED"));
        assert!(!dir.join(".kanzei/state.db").exists());
    }
}
