//! edit 工具:精确字符串替换。自举开发的关键——比 write 整文件覆写安全得多。
//! 硬门禁:old_string 必须唯一命中(除非 replace_all);未命中/多命中都给出
//! 可操作的纠错反馈;写后语法校验(设计红线 5)。
//! D-113 门禁:仅换行符差异自动容忍;同一文件连续两次未命中后,错误反馈
//! 直接附带文件实际内容(等于替模型重读),杜绝盲试与整文件重写兜底。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
/// 连续未命中达到该次数后,反馈附带文件实际片段。
const MISSES_BEFORE_EXCERPT: u32 = 2;
const EXCERPT_LINES: usize = 40;
const EXCERPT_MAX_CHARS: usize = 3000;

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

#[derive(Default)]
pub struct EditTool {
    /// 每文件连续未命中/多命中计数;成功一次即清零。
    misses: Mutex<HashMap<PathBuf, u32>>,
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn description(&self) -> String {
        "Replace an exact string in a file. Params: path, old_string (must match exactly and uniquely), new_string; optional replace_all. Line-ending differences (\\r\\n vs \\n) are tolerated automatically.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(EditInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["path"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::write_worktree(ctx)
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
        let path = ctx
            .cwd
            .join(kanzei_harness::permission::normalize_resource(&input.path));
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

        // 第一层:逐字节精确匹配。
        let mut old_string = input.old_string.clone();
        let mut new_string = input.new_string.clone();
        let mut haystack = content.clone();
        let mut ending_note = "";
        let mut count = haystack.matches(&old_string).count();
        if count == 0 {
            // 第二层:仅换行符差异(\r\n vs \n)自动归一后匹配——这是自举轨迹里
            // 编辑连败螺旋的头号失败源,归一命中时按文件主导风格写回。
            let normalized_content = content.replace("\r\n", "\n");
            let normalized_old = input.old_string.replace("\r\n", "\n");
            let normalized_count = normalized_content.matches(&normalized_old).count();
            if normalized_count > 0 {
                haystack = normalized_content;
                old_string = normalized_old;
                new_string = input.new_string.replace("\r\n", "\n");
                count = normalized_count;
                ending_note = " (line endings differed and were normalized automatically)";
            }
        }

        if count == 0 {
            return self.miss_feedback(&path, &input.old_string, &content);
        }
        if count > 1 && !input.replace_all {
            let miss_count = self.record_miss(&path);
            return ToolOutput::error(format!(
                "old_string matches {count} locations in {}; make it unique with more context, or set replace_all=true.{}",
                path.display(),
                if miss_count >= MISSES_BEFORE_EXCERPT {
                    "\nDo NOT fall back to rewriting the whole file via shell — add surrounding lines to old_string until it is unique."
                } else {
                    ""
                }
            ));
        }

        let updated = if input.replace_all {
            haystack.replace(&old_string, &new_string)
        } else {
            haystack.replacen(&old_string, &new_string, 1)
        };
        // 归一匹配的写回:按原文件主导换行风格还原,避免半 CRLF 半 LF。
        let updated = if !ending_note.is_empty() && dominant_crlf(&content) {
            updated.replace('\n', "\r\n")
        } else {
            updated
        };
        if let Err(e) = tokio::fs::write(&path, updated.as_bytes()).await {
            return ToolOutput::error(format!("cannot write {}: {e}", path.display()));
        }
        self.misses.lock().unwrap().remove(&path);
        let mut message = format!(
            "replaced {count} occurrence(s) in {}{ending_note}",
            path.display()
        );
        if let Some(warning) = crate::write::validate_syntax(&path, &updated) {
            message.push_str(&format!("\nWARNING: {warning}"));
        }
        let display = crate::write::diff_display(&input.path, &content, &updated);
        ToolOutput::ok(message).with_display(display)
    }
}

impl EditTool {
    fn record_miss(&self, path: &Path) -> u32 {
        let mut misses = self.misses.lock().unwrap();
        let count = misses.entry(path.to_path_buf()).or_insert(0);
        *count += 1;
        *count
    }

    /// 未命中反馈:第 1 次给最像的一行;连续第 2 次起直接附带文件实际片段
    /// (等于替模型重读),并明确禁止转向整文件重写。
    fn miss_feedback(&self, path: &Path, old_string: &str, content: &str) -> ToolOutput {
        let miss_count = self.record_miss(path);
        let first_line = old_string.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        let mut message = format!(
            "old_string not found in {} — it must match exactly, including whitespace.",
            path.display()
        );
        if miss_count < MISSES_BEFORE_EXCERPT {
            if let Some(closest) = content.lines().find(|l| l.contains(first_line.trim())) {
                message.push_str(&format!("\nClosest line in file: `{closest}`"));
            }
        } else {
            message.push_str(&format!(
                "\nConsecutive miss #{miss_count} on this file. The file's ACTUAL content around the closest match is below — align old_string to THIS text exactly. Do NOT rewrite the whole file via shell (Set-Content/Out-File are blocked); do NOT retry blindly:\n{}",
                excerpt_around(content, first_line.trim())
            ));
        }
        ToolOutput::error(message)
    }
}

fn dominant_crlf(content: &str) -> bool {
    let crlf = content.matches("\r\n").count();
    let total_newlines = content.matches('\n').count();
    crlf * 2 > total_newlines
}

/// 以 anchor 首次出现的行为中心截取带行号的片段;找不到 anchor 就从头截取。
fn excerpt_around(content: &str, anchor: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let center = if anchor.is_empty() {
        0
    } else {
        lines
            .iter()
            .position(|l| l.contains(anchor))
            .unwrap_or(0)
    };
    let start = center.saturating_sub(EXCERPT_LINES / 2);
    let end = (start + EXCERPT_LINES).min(lines.len());
    let mut out = String::new();
    for (offset, line) in lines[start..end].iter().enumerate() {
        let rendered = format!("{:>5}: {line}\n", start + offset + 1);
        if out.len() + rendered.len() > EXCERPT_MAX_CHARS {
            out.push_str("… (excerpt truncated)\n");
            break;
        }
        out.push_str(&rendered);
    }
    if end < lines.len() {
        out.push_str(&format!("… ({} more lines below)\n", lines.len() - end));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::EditTool;
    use kanzei_harness::{Tool, ToolCtx};
    use serde_json::json;

    fn setup(name: &str, content: &str) -> (std::path::PathBuf, ToolCtx) {
        let dir = std::env::temp_dir().join(format!(
            "kz-edit-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("target.txt"), content).unwrap();
        let ctx = ToolCtx::new(dir.clone());
        (dir, ctx)
    }

    #[tokio::test]
    async fn crlf_mismatch_is_tolerated_and_file_keeps_crlf() {
        // D-113:old_string 用 \n 而文件是 \r\n,过去直接未命中引发连败螺旋。
        let (dir, ctx) = setup("crlf", "fn main() {\r\n    old();\r\n}\r\n");
        let out = EditTool::default()
            .execute(
                json!({"path": "target.txt", "old_string": "fn main() {\n    old();\n}", "new_string": "fn main() {\n    new();\n}"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("normalized"), "{}", out.content);
        let saved = std::fs::read_to_string(dir.join("target.txt")).unwrap();
        assert_eq!(saved, "fn main() {\r\n    new();\r\n}\r\n", "必须保持文件原有 CRLF 风格");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn second_consecutive_miss_includes_file_excerpt() {
        let (dir, ctx) = setup("miss", "line one\nline two has anchor\nline three\n");
        let tool = EditTool::default();
        let miss = json!({"path": "target.txt", "old_string": "has anchor but wrong", "new_string": "x"});
        let first = tool.execute(miss.clone(), &ctx).await;
        assert!(first.is_error);
        assert!(!first.content.contains("line three"), "首次未命中不附全文: {}", first.content);
        let second = tool.execute(miss, &ctx).await;
        assert!(second.is_error);
        assert!(second.content.contains("line two has anchor"), "第二次必须附文件实际内容: {}", second.content);
        assert!(second.content.contains("Set-Content"), "必须明确禁止整文件重写兜底: {}", second.content);
        // 成功一次后计数清零。
        let ok = tool
            .execute(json!({"path": "target.txt", "old_string": "line three", "new_string": "line 3"}), &ctx)
            .await;
        assert!(!ok.is_error, "{}", ok.content);
        let third = tool
            .execute(json!({"path": "target.txt", "old_string": "nope nope", "new_string": "x"}), &ctx)
            .await;
        assert!(!third.content.contains("ACTUAL content"), "成功后计数应清零: {}", third.content);
        std::fs::remove_dir_all(dir).ok();
    }
}
