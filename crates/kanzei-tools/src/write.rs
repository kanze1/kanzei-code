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
        let path = ctx.cwd.join(&input.path);
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
        text.push_str("(diff 截断)\n");
    }
    serde_json::json!({
        "kind": "diff",
        "path": path,
        "diff": text,
        "additions": additions,
        "deletions": deletions,
    })
}

/// 写后语法校验(不阻断,只告知)。edit 工具复用。
pub(crate) fn validate_syntax(path: &std::path::Path, content: &str) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "json" => serde_json::from_str::<serde_json::Value>(content)
            .err()
            .map(|e| format!("file was written but is not valid JSON: {e}")),
        _ => None,
    }
}
