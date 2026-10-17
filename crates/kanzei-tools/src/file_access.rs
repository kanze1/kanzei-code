use kanzei_harness::permission::ScopedFile;
use kanzei_harness::{ToolCtx, ToolOutput};
use std::path::Path;

pub(crate) fn resolve(ctx: &ToolCtx, path: &Path) -> Result<ScopedFile, Box<ToolOutput>> {
    ScopedFile::resolve(&ctx.cwd, &ctx.project_root, path).map_err(|error| {
        Box::new(ToolOutput::needs_correction(
            "FILESYSTEM_PATH_REJECTED",
            error.to_string(),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::Tool;
    use serde_json::json;

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
        std::fs::remove_dir(project.join(".kanzei/research")).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
