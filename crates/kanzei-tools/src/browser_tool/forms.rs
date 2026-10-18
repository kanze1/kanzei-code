use super::{Backend, BrowserAction, BrowserInput};
use base64::Engine;
use kanzei_harness::{
    permission::{execution_rules, Effect, FileResource, ScopedFile},
    ToolCtx, ToolOutput,
};
use serde_json::{json, Value};
use std::{io::Read, path::Path};

const FORM_SOURCE: &str = include_str!("../../../../scripts/browser-form-action.mjs");
/// Bound decoded uploads before creating the browser's File objects.
pub const MAX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

pub fn form_action_expression(input: &BrowserInput, ctx: &ToolCtx) -> Result<String, String> {
    let action = match BrowserAction::parse(&input.action)? {
        BrowserAction::Type => "fill",
        BrowserAction::Select => "select",
        BrowserAction::Check => "check",
        BrowserAction::Upload => "upload",
        BrowserAction::Validate => "validate",
        _ => return Err("not a structured form action".into()),
    };
    let files = if action == "upload" {
        load_uploads(input.files.as_deref().ok_or("upload requires files")?, ctx)?
    } else {
        Vec::new()
    };
    let params = json!({"action":action,"selector":input.selector,"text":input.text,"values":input.values,"checked":input.checked,"files":files});
    let source = FORM_SOURCE.replacen("export function formAction", "function formAction", 1);
    Ok(format!(
        "(() => {{ {source}\nreturn formAction({params}); }})()"
    ))
}

fn load_uploads(paths: &[String], ctx: &ToolCtx) -> Result<Vec<Value>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let rules = execution_rules()
        .ok_or("FILE_UPLOAD_APPROVAL_REQUIRED: no execution permission snapshot")?;
    let mut remaining = MAX_UPLOAD_BYTES;
    let mut files = Vec::new();
    for path in paths {
        let path = Path::new(path);
        let resource = FileResource::resolve(&ctx.cwd, path).map_err(|error| error.to_string())?;
        if rules.evaluate_file_with_rule("browser", &resource).0 != Effect::Allow {
            return Err(format!("FILE_UPLOAD_APPROVAL_REQUIRED: {}", path.display()));
        }
        let access = ScopedFile::resolve(&ctx.cwd, &ctx.project_root, path)
            .map_err(|error| error.to_string())?;
        let file = access.open().map_err(|error| error.to_string())?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err("upload accepts regular files only".into());
        }
        if metadata.len() > remaining as u64 {
            return Err("UPLOAD_TOO_LARGE: total upload exceeds 16 MiB".into());
        }
        let mut bytes = Vec::new();
        file.take(remaining as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > remaining {
            return Err("UPLOAD_TOO_LARGE: file grew beyond the upload budget".into());
        }
        remaining -= bytes.len();
        let name = access
            .path()
            .file_name()
            .ok_or("upload file has no name")?
            .to_string_lossy();
        let mime = crate::preview_server::mime_for(access.path())
            .split(';')
            .next()
            .expect("MIME media type");
        files.push(json!({"name":name,"mime_type":mime,"data":base64::engine::general_purpose::STANDARD.encode(bytes)}));
    }
    Ok(files)
}

pub fn out_form(
    backend: Backend,
    action: &str,
    selector: &str,
    state: &Value,
    url: &str,
) -> ToolOutput {
    ToolOutput::ok(format!(
        "{}\nForm action: {action}\nSelector: {selector}\nURL: {url}\nObserved state: {state}",
        backend.line()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn uploads_require_file_authority_and_do_not_widen_sensitive_grants() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("visible.txt"), "authorized upload").unwrap();
        std::fs::write(root.path().join(".env"), "TEST_TOKEN=fixture-only").unwrap();
        let ctx = ToolCtx::new(root.path().into(), root.path().into());
        assert!(load_uploads(&["visible.txt".into()], &ctx)
            .unwrap_err()
            .contains("APPROVAL_REQUIRED"));
        let mut rules =
            kanzei_harness::Ruleset::new(vec![kanzei_harness::rule("browser", "*", Effect::Allow)]);
        kanzei_harness::permission::scope(rules.clone(), async {
            let visible = load_uploads(&["visible.txt".into()], &ctx).unwrap();
            assert_eq!(visible[0]["name"], "visible.txt");
            assert_eq!(
                visible[0]["data"],
                base64::engine::general_purpose::STANDARD.encode("authorized upload")
            );
            assert!(load_uploads(&[".env".into()], &ctx)
                .unwrap_err()
                .contains("APPROVAL_REQUIRED"));
        })
        .await;
        rules.push(kanzei_harness::rule(
            "browser",
            &root.path().join(".env").display().to_string(),
            Effect::Allow,
        ));
        kanzei_harness::permission::scope(rules, async {
            assert_eq!(load_uploads(&[".env".into()], &ctx).unwrap().len(), 1);
        })
        .await;
    }

    #[tokio::test]
    async fn upload_budget_is_enforced_before_loading_or_sending_file_bytes() {
        let root = tempfile::tempdir().unwrap();
        let file = std::fs::File::create(root.path().join("large.bin")).unwrap();
        file.set_len(MAX_UPLOAD_BYTES as u64 + 1).unwrap();
        let ctx = ToolCtx::new(root.path().into(), root.path().into());
        let rules =
            kanzei_harness::Ruleset::new(vec![kanzei_harness::rule("browser", "*", Effect::Allow)]);
        kanzei_harness::permission::scope(rules, async {
            assert!(load_uploads(&["large.bin".into()], &ctx)
                .unwrap_err()
                .contains("UPLOAD_TOO_LARGE"));
            assert!(load_uploads(&[".".into()], &ctx).is_err());
        })
        .await;
    }
}
