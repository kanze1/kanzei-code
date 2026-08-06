//! edit 工具:精确字符串替换。自举开发的关键——比 write 整文件覆写安全得多。
//! 硬门禁:old_string 必须唯一命中(除非 replace_all);未命中/多命中都给出
//! 可操作的纠错反馈;写后语法校验(设计红线 5)。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Deserialize, JsonSchema)]
struct EditInput {
    /// 文件路径(绝对或相对 cwd)
    #[serde(alias = "file_path", alias = "filepath", alias = "file")]
    path: String,
    /// 要被替换的原文(必须与文件内容逐字符一致,含缩进)
    #[serde(alias = "old_str", alias = "old", alias = "search")]
    old_string: String,
    /// 替换后的新文本
    #[serde(alias = "new_str", alias = "new", alias = "replace")]
    new_string: String,
    /// 替换所有出现(默认 false:要求唯一命中)
    #[serde(default)]
    replace_all: bool,
}

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn description(&self) -> String {
        "Replace an exact string in a file. Params: path, old_string (must match exactly and uniquely), new_string; optional replace_all.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(EditInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["path"].as_str().unwrap_or("*").to_string()]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: EditInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        if input.old_string == input.new_string {
            return ToolOutput::error("old_string and new_string are identical — nothing to do");
        }
        if input.old_string.is_empty() {
            return ToolOutput::error(
                "old_string must not be empty (use the write tool to create files)",
            );
        }
        let path = ctx.cwd.join(&input.path);
        match tokio::fs::metadata(&path).await {
            Ok(meta) if meta.len() > MAX_FILE_BYTES => {
                return ToolOutput::error(format!(
                    "{} is too large ({} bytes)",
                    path.display(),
                    meta.len()
                ))
            }
            Err(e) => return ToolOutput::error(format!("cannot access {}: {e}", path.display())),
            _ => {}
        }
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(format!("cannot read {}: {e}", path.display())),
        };

        let count = content.matches(&input.old_string).count();
        if count == 0 {
            // 纠错反馈:给出最像的一行,帮模型对齐缩进/空白差异。
            let first_line = input.old_string.lines().next().unwrap_or("");
            let hint = content
                .lines()
                .find(|l| l.contains(first_line.trim()))
                .map(|l| format!("\nClosest line in file: `{l}`"))
                .unwrap_or_default();
            return ToolOutput::error(format!(
                "old_string not found in {} — it must match exactly, including whitespace.{hint}",
                path.display()
            ));
        }
        if count > 1 && !input.replace_all {
            return ToolOutput::error(format!(
                "old_string matches {count} locations in {}; make it unique with more context, or set replace_all=true.",
                path.display()
            ));
        }

        let updated = if input.replace_all {
            content.replace(&input.old_string, &input.new_string)
        } else {
            content.replacen(&input.old_string, &input.new_string, 1)
        };
        if let Err(e) = tokio::fs::write(&path, updated.as_bytes()).await {
            return ToolOutput::error(format!("cannot write {}: {e}", path.display()));
        }
        let mut message = format!("replaced {count} occurrence(s) in {}", path.display());
        if let Some(warning) = crate::write::validate_syntax(&path, &updated) {
            message.push_str(&format!("\nWARNING: {warning}"));
        }
        let display = crate::write::diff_display(&input.path, &content, &updated);
        ToolOutput::ok(message).with_display(display)
    }
}
