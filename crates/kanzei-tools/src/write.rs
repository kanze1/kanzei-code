//! write 工具。设计红线 5:结构化文本写入后做语法校验,坏格式以 warning 告知模型。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
struct WriteInput {
    /// 文件路径(绝对或相对 cwd)
    #[serde(alias = "file_path", alias = "filepath", alias = "file")]
    path: String,
    /// 完整文件内容
    #[serde(alias = "text", alias = "contents")]
    content: String,
}

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> String {
        "Write a file (overwrites). Params: path, content.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(WriteInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["path"].as_str().unwrap_or("*").to_string()]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: WriteInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let path = ctx
            .cwd
            .join(kanzei_harness::permission::normalize_resource(&input.path));
        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolOutput::error(format!("cannot create {}: {e}", parent.display()));
            }
        }
        // 覆写前抓旧内容,给 UI 出 diff(看得见改了什么,R-015)。
        let previous = tokio::fs::read_to_string(&path).await.ok();
        if let Err(e) = tokio::fs::write(&path, input.content.as_bytes()).await {
            return ToolOutput::error(format!("cannot write {}: {e}", path.display()));
        }
        let mut message = format!("wrote {} bytes to {}", input.content.len(), path.display());
        if let Some(warning) = validate_syntax(&path, &input.content) {
            message.push_str(&format!("\nWARNING: {warning}"));
        }
        let display = match previous {
            Some(old) => diff_display(&input.path, &old, &input.content),
            None => serde_json::json!({
                "kind": "create",
                "path": input.path,
                "bytes": input.content.len(),
                "preview": input.content.lines().take(30).collect::<Vec<_>>().join("\n"),
            }),
        };
        ToolOutput::ok(message).with_display(display)
    }
}

/// 统一 diff 展示(edit/write 共用)。上限截断,防止巨型文件撑爆前端。
pub(crate) fn diff_display(path: &str, old: &str, new: &str) -> serde_json::Value {
    let diff = similar::TextDiff::from_lines(old, new);
    let mut additions = 0usize;
    let mut deletions = 0usize;
    let mut text = String::new();
    let mut lines = Vec::new();
    let mut old_line = 1usize;
    let mut new_line = 1usize;
    let mut truncated = false;
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Insert => {
                additions += 1;
                "+"
            }
            similar::ChangeTag::Delete => {
                deletions += 1;
                "-"
            }
            similar::ChangeTag::Equal => " ",
        };
        let value = change.value().trim_end_matches('\n');
        if text.len() < 24 * 1024 {
            let (kind, old, new) = match change.tag() {
                similar::ChangeTag::Insert => ("add", None, Some(new_line)),
                similar::ChangeTag::Delete => ("del", Some(old_line), None),
                similar::ChangeTag::Equal => ("ctx", Some(old_line), Some(new_line)),
            };
            lines.push(serde_json::json!({
                "kind": kind,
                "old_line": old,
                "new_line": new,
                "text": value,
            }));
        } else {
            truncated = true;
        }
        match change.tag() {
            similar::ChangeTag::Insert => new_line += 1,
            similar::ChangeTag::Delete => old_line += 1,
            similar::ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
            }
        }
        // 等值上下文只保留短窗口由前端裁剪;此处限制总量。
        if text.len() < 24 * 1024 {
            text.push_str(sign);
            text.push_str(change.value());
            if !change.value().ends_with('\n') {
                text.push('\n');
            }
        }
    }
    if text.len() >= 24 * 1024 {
        truncated = true;
        text.push_str("(diff 截断)\n");
    }
    serde_json::json!({
        "kind": "diff",
        "path": path,
        "language": diff_language(path),
        "diff": text,
        "lines": lines,
        "additions": additions,
        "deletions": deletions,
        "truncated": truncated,
    })
}

fn diff_language(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "json" => "json",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "css" => "css",
        "html" | "htm" => "html",
        "py" => "python",
        "ps1" | "sh" | "bash" => "shell",
        _ => "text",
    }
}

/// 写后语法校验(不阻断,只告知)。edit 工具复用。
pub(crate) fn validate_syntax(path: &std::path::Path, content: &str) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "json" => serde_json::from_str::<serde_json::Value>(content)
            .err()
            .map(|e| format!("file was written but is not valid JSON: {e}")),
        "toml" => content
            .parse::<toml::Table>()
            .err()
            .map(|e| format!("file was written but is not valid TOML: {e}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{diff_display, WriteTool};

    #[test]
    fn diff_display_exposes_language_counts_and_line_numbers() {
        let display = diff_display(
            "src/main.rs",
            "fn main() {\nold();\n}\n",
            "fn main() {\nnew();\n}\n",
        );
        assert_eq!(display["kind"], "diff");
        assert_eq!(display["language"], "rust");
        assert_eq!(display["additions"], 1);
        assert_eq!(display["deletions"], 1);
        assert_eq!(display["lines"][0]["old_line"], 1);
        assert_eq!(display["lines"][0]["new_line"], 1);
        assert_eq!(display["lines"][1]["kind"], "del");
        assert_eq!(display["lines"][1]["old_line"], 2);
        assert_eq!(display["lines"][2]["kind"], "add");
        assert_eq!(display["lines"][2]["new_line"], 2);
    }

    #[tokio::test]
    async fn permission_path_and_write落点使用同一规范化结果() {
        use kanzei_harness::permission::{Effect, Rule, Ruleset};
        use kanzei_harness::{Tool, ToolCtx};

        let root = std::env::temp_dir().join(format!(
            "kanzei-d050-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let input_path = ".KANZEI/./project/../allowed.md";
        let normalized = kanzei_harness::permission::normalize_resource(input_path);
        assert_eq!(normalized, ".kanzei/allowed.md");

        let deny = Ruleset::new(vec![Rule {
            action: "write".into(),
            resource: "*.kanzei/project/*".into(),
            effect: Effect::Deny,
        }]);
        assert_eq!(
            deny.evaluate(
                "write",
                &kanzei_harness::permission::normalize_resource(".KANZEI\\project\\secret.md")
            ),
            Effect::Deny
        );

        let output = WriteTool
            .execute(
                serde_json::json!({"path": input_path, "content": "落点一致"}),
                &ToolCtx {
                    cwd: root.clone(),
                    project_root: root.clone(),
                },
            )
            .await;
        assert!(!output.is_error, "{output:?}");
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei/allowed.md")).unwrap(),
            "落点一致"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn diff_display_marks_large_output_as_truncated() {
        let old = "a\n".repeat(20_000);
        let new = "b\n".repeat(20_000);
        let display = diff_display("notes.txt", &old, &new);
        assert_eq!(display["language"], "text");
        assert_eq!(display["truncated"], true);
        assert!(display["lines"].as_array().unwrap().len() < 40_000);
    }
}
