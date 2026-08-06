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
        if let Err(e) = tokio::fs::write(&path, input.content.as_bytes()).await {
            return ToolOutput::error(format!("cannot write {}: {e}", path.display()));
        }
        let mut message = format!("wrote {} bytes to {}", input.content.len(), path.display());
        if let Some(warning) = validate_syntax(&path, &input.content) {
            message.push_str(&format!("\nWARNING: {warning}"));
        }
        ToolOutput::ok(message)
    }
}

/// 写后语法校验(不阻断,只告知)。M1 扩展 TOML/YAML/XML。
fn validate_syntax(path: &std::path::Path, content: &str) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "json" => serde_json::from_str::<serde_json::Value>(content)
            .err()
            .map(|e| format!("file was written but is not valid JSON: {e}")),
        _ => None,
    }
}
