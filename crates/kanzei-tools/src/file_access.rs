use kanzei_harness::permission::{execution_rules, Effect, FileResource, ScopedFile};
use kanzei_harness::{ToolCtx, ToolOutput};
use std::path::Path;

pub(crate) fn resolve(
    ctx: &ToolCtx,
    path: &Path,
    action: &str,
) -> Result<ScopedFile, Box<ToolOutput>> {
    let rejected = |error: std::io::Error| {
        Box::new(ToolOutput::needs_correction(
            "FILESYSTEM_PATH_REJECTED",
            error.to_string(),
        ))
    };
    let file = ScopedFile::resolve(&ctx.cwd, &ctx.project_root, path).map_err(rejected)?;
    let resource = FileResource::resolve(&ctx.cwd, path).map_err(rejected)?;
    if let Some(sensitivity) = resource.sensitivity() {
        let allowed = execution_rules().is_some_and(|rules| {
            rules.evaluate_file_with_rule(action, &resource).0 == Effect::Allow
        });
        if !allowed {
            return Err(Box::new(ToolOutput::needs_confirmation(
                "SENSITIVE_FILE_APPROVAL_REQUIRED",
                format!(
                    "{} is a {}; an explicit file-specific permission is required",
                    file.path().display(),
                    sensitivity.description()
                ),
            )));
        }
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn sensitive_files_require_consent_across_file_tools_and_recursive_search() {
        let root = std::env::temp_dir().join(format!(
            "kz-sensitive-files-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".env"), "PRIVATE_MARKER=secret").unwrap();
        std::fs::write(root.join("visible.txt"), "PUBLIC_MARKER=visible").unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let cases: Vec<(Box<dyn Tool>, serde_json::Value)> = vec![
            (Box::new(crate::read::ReadTool), json!({"path":".env"})),
            (
                Box::new(crate::grep::GrepTool),
                json!({"path":".env","pattern":"MARKER"}),
            ),
            (
                Box::new(crate::glob::GlobTool),
                json!({"path":".env","pattern":"*"}),
            ),
            (
                Box::new(crate::write::WriteTool),
                json!({"path":".env","content":"changed"}),
            ),
            (
                Box::new(crate::edit::EditTool::default()),
                json!({"path":".env","old_string":"PRIVATE","new_string":"changed"}),
            ),
            (
                Box::new(crate::edit::InsertTool),
                json!({"path":".env","anchor":"PRIVATE","content":"changed","position":"after"}),
            ),
        ];
        for (tool, input) in cases {
            let output = tool.execute(input, &ctx).await;
            assert_eq!(
                output.code,
                Some("SENSITIVE_FILE_APPROVAL_REQUIRED"),
                "{}: {}",
                tool.name(),
                output.content
            );
            assert!(!output.content.contains("PRIVATE_MARKER"));
        }
        let rules = kanzei_harness::permission::Ruleset::new(vec![kanzei_harness::rule(
            "grep",
            "*",
            Effect::Allow,
        )]);
        let output = kanzei_harness::permission::scope(
            rules,
            crate::grep::GrepTool.execute(json!({"path":".","pattern":"MARKER"}), &ctx),
        )
        .await;
        assert!(
            output.content.contains("PUBLIC_MARKER"),
            "{}",
            output.content
        );
        assert!(!output.content.contains("PRIVATE_MARKER"));
        let rules = kanzei_harness::permission::Ruleset::new(vec![kanzei_harness::rule(
            "read",
            ".env",
            Effect::Allow,
        )]);
        let output = kanzei_harness::permission::scope(
            rules,
            crate::read::ReadTool.execute(json!({"path":".env"}), &ctx),
        )
        .await;
        assert!(
            output.content.contains("PRIVATE_MARKER=secret"),
            "{}",
            output.content
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".env")).unwrap(),
            "PRIVATE_MARKER=secret"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn every_file_tool_rejects_external_absolute_paths_and_research_junctions() {
        let root = std::env::temp_dir().join(format!(
            "kz-file-boundary-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("project");
        let outside = root.join("outside");
        std::fs::create_dir_all(project.join(".kanzei")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "PRIVATE_CONTENT").unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(project.join(".kanzei").join("research"))
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success());
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, project.join(".kanzei/research")).unwrap();
        let ctx = ToolCtx::new(project.clone(), project.clone());
        for path in [
            outside.join("secret.txt"),
            project.join(".kanzei/research/secret.txt"),
        ] {
            let cases: Vec<(Box<dyn Tool>, serde_json::Value)> = vec![
                (Box::new(crate::read::ReadTool), json!({"path":path})),
                (
                    Box::new(crate::grep::GrepTool),
                    json!({"path":path,"pattern":"PRIVATE"}),
                ),
                (
                    Box::new(crate::glob::GlobTool),
                    json!({"path":path,"pattern":"*"}),
                ),
                (
                    Box::new(crate::write::WriteTool),
                    json!({"file_path":path,"content":"changed"}),
                ),
                (
                    Box::new(crate::edit::EditTool::default()),
                    json!({"file_path":path,"old_string":"PRIVATE","new_string":"changed"}),
                ),
                (
                    Box::new(crate::edit::InsertTool),
                    json!({"file_path":path,"anchor":"PRIVATE","content":"changed","position":"after"}),
                ),
            ];
            for (tool, input) in cases {
                let output = tool.execute(input, &ctx).await;
                assert_eq!(
                    output.code,
                    Some("FILESYSTEM_PATH_REJECTED"),
                    "{}: {}",
                    tool.name(),
                    output.content
                );
                assert!(!output.content.contains("PRIVATE_CONTENT"));
            }
        }
        assert_eq!(
            std::fs::read_to_string(outside.join("secret.txt")).unwrap(),
            "PRIVATE_CONTENT"
        );
        #[cfg(windows)]
        std::fs::remove_dir(project.join(".kanzei/research")).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(project.join(".kanzei/research")).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
