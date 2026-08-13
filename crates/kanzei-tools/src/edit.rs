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
const MISSES_BEFORE_EXCERPT: u32 = 1;
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
    /// 明确承认这次替换是要删掉内容(净删除超过阈值时必须显式置 true)
    #[serde(default)]
    allow_deletion: bool,
}

/// 净删除多少行就要求显式确认。设 3 而不是 1:一两行的收缩在正常改写里太常见,
/// 每次都要确认会把门禁变成噪音,反而被无脑加 flag 绕过。
const NET_DELETE_CONFIRM_LINES: usize = 3;

/// old_string 里有、new_string 里没有的非空行(按 trim 后的多重集差)。
/// 这是「替换顺手吃掉邻居」的直接信号:R-158 那两处回退(Responses 的 reasoning
/// effort 整段、设置页思考强度说明段)净行数分别是 0 和 +2,靠行数门禁一个都拦不住,
/// 但两次都在这个列表里明明白白。
fn dropped_lines(old: &str, new: &str) -> Vec<String> {
    let mut kept: HashMap<&str, usize> = HashMap::new();
    for line in new.lines() {
        *kept.entry(line.trim()).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    for line in old.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match kept.get_mut(trimmed) {
            Some(remaining) if *remaining > 0 => *remaining -= 1,
            _ => out.push(trimmed.to_string()),
        }
    }
    out
}

fn preview_lines(lines: &[String]) -> String {
    let shown = lines
        .iter()
        .take(5)
        .map(|l| {
            format!(
                "  - {}",
                if l.chars().count() > 100 {
                    format!("{}…", l.chars().take(100).collect::<String>())
                } else {
                    l.clone()
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if lines.len() > 5 {
        format!("{shown}\n  - …还有 {} 行", lines.len() - 5)
    } else {
        shown
    }
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
        "Replace an exact string in a file. Params: path, old_string (must match exactly and uniquely), new_string; optional replace_all, allow_deletion (required when the replacement removes 3+ net lines). Line-ending differences (\\r\\n vs \\n) are tolerated automatically. The result lists any line present in old_string but absent from new_string — read it: that is how a replacement silently eats the block next to your target.".into()
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
            return ToolOutput::noop(
                "EDIT_IDENTICAL_INPUT",
                "old_string and new_string are identical — nothing to do",
            );
        }
        if input.old_string.is_empty() {
            return ToolOutput::needs_correction(
                "EDIT_EMPTY_ANCHOR",
                "old_string must not be empty (use the write tool to create files)",
            );
        }
        let path = ctx
            .cwd
            .join(kanzei_harness::permission::normalize_resource(&input.path));
        match tokio::fs::metadata(&path).await {
            Ok(meta) if meta.len() > MAX_FILE_BYTES => {
                return ToolOutput::failed(
                    "EDIT_FILE_TOO_LARGE",
                    format!("{} is too large ({} bytes)", path.display(), meta.len()),
                )
            }
            Err(e) => {
                return ToolOutput::failed(
                    "EDIT_FILE_UNAVAILABLE",
                    format!("cannot access {}: {e}", path.display()),
                )
            }
            _ => {}
        }
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => {
                return ToolOutput::failed(
                    "EDIT_READ_FAILED",
                    format!("cannot read {}: {e}", path.display()),
                )
            }
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
            return ToolOutput::needs_correction("EDIT_ANCHOR_NOT_UNIQUE", format!(
                "old_string matches {count} locations in {}; make it unique with more context, or set replace_all=true.{}\nActual file context around the first match:\n{}",
                path.display(),
                if miss_count >= MISSES_BEFORE_EXCERPT {
                    "\nDo NOT fall back to rewriting the whole file via shell — add surrounding lines to old_string until it is unique."
                } else {
                    ""
                },
                excerpt_around(&content, input.old_string.lines().next().unwrap_or("")),
            ));
        }

        // 净删除门禁:替换掉的行数明显多于写回的行数,多半不是"改写"而是"顺手删掉了
        // 一段"。要删可以,但必须说出来——allow_deletion=true。
        let old_line_count = old_string.lines().count();
        let new_line_count = new_string.lines().count();
        let occurrences = if input.replace_all { count } else { 1 };
        let net_deleted = old_line_count.saturating_sub(new_line_count) * occurrences;
        let dropped = dropped_lines(&old_string, &new_string);
        // 「本想插入,却把锚点吃掉了」:新文本比原文更长(明显是在加东西),却没保住
        // 匹配到的原文行。三次实测都是这个形状——R-158 顶掉 Responses 的 reasoning
        // effort、删掉设置页思考强度说明,以及 R-153 批10 把 `pub(crate) fn
        // build_runner_config(` 换成了新函数的开头(净 +6 行,纯行数门禁拦不住)。
        // 要插入就把原文原样含进 new_string;确实是替换就显式说一声。
        let insertion_shaped_clobber = new_line_count > old_line_count && !dropped.is_empty();
        if insertion_shaped_clobber && !input.allow_deletion {
            return ToolOutput::needs_correction(
                "EDIT_INSERTION_WOULD_REPLACE_ANCHOR",
                format!(
                    "这次替换看着像插入(新文本多了 {} 行),却没保住 old_string 里的原文——\
                 十有八九是想在附近加内容,结果把匹配到的那段顶掉了。\
                 要插入请改用 insert 工具;确实是要替换掉它们,就置 allow_deletion=true。\
                 \n未被保留的原文:\n{}",
                    new_line_count - old_line_count,
                    preview_lines(&dropped)
                ),
            );
        }
        if net_deleted >= NET_DELETE_CONFIRM_LINES && !input.allow_deletion {
            return ToolOutput::needs_confirmation(
                "EDIT_NET_DELETION_REQUIRES_CONFIRMATION",
                format!(
                "这次替换净删除 {net_deleted} 行({old_line_count} 行换成 {new_line_count} 行{})。\
                 确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,\
                 说明 old_string 匹配到了不该动的位置——缩小 old_string 或改成插入式替换\
                 (把原文原样包含在 new_string 里)。\n将被删掉的内容:\n{}",
                if occurrences > 1 {
                    format!(",共 {occurrences} 处")
                } else {
                    String::new()
                },
                preview_lines(&dropped)
            ),
            );
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
            return ToolOutput::failed(
                "EDIT_WRITE_FAILED",
                format!("cannot write {}: {e}", path.display()),
            );
        }
        self.misses.lock().unwrap().remove(&path);
        let mut message = format!(
            "replaced {count} occurrence(s) in {}{ending_note}",
            path.display()
        );
        // 没到拦截线也要把丢掉的行报出来:替换吃掉邻居时净行数往往不减反增,
        // 唯一能立刻看见的信号就是"old_string 里有、new_string 里没有"的那几行。
        if !dropped.is_empty() {
            message.push_str(&format!(
                "\nNOTE: 本次替换未保留下面 {} 行(若非本意,立刻改回来):\n{}",
                dropped.len(),
                preview_lines(&dropped)
            ));
        }
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
        let first_line = old_string
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
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
        ToolOutput::needs_correction("EDIT_ANCHOR_NOT_FOUND", message)
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
        lines.iter().position(|l| l.contains(anchor)).unwrap_or(0)
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

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum InsertPosition {
    Before,
    After,
}

#[derive(Deserialize, JsonSchema)]
struct InsertInput {
    /// 文件路径(绝对或相对 cwd)
    #[serde(alias = "file_path", alias = "filepath", alias = "file")]
    path: String,
    /// 用于定位插入点的原文；必须精确且唯一命中，锚点本身不会被修改。
    #[serde(alias = "old_string", alias = "search")]
    anchor: String,
    /// 在锚点之前或之后原样插入的文本。
    #[serde(alias = "new_string", alias = "text")]
    content: String,
    position: InsertPosition,
}

/// 原生插入工具：锚点只用于定位，生成结果时永远原样保留。
pub struct InsertTool;

#[async_trait]
impl Tool for InsertTool {
    fn name(&self) -> &'static str {
        "insert"
    }

    fn description(&self) -> String {
        "Insert content before or after an exact unique anchor without replacing the anchor. Params: path, anchor, content, position (before|after). Content is inserted verbatim; include the desired newlines in content.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(InsertInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["path"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::write_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: InsertInput = match crate::parse_input(self, input) {
            Ok(value) => value,
            Err(output) => return output,
        };
        if input.anchor.is_empty() {
            return ToolOutput::needs_correction("INSERT_EMPTY_ANCHOR", "anchor must not be empty");
        }
        if input.content.is_empty() {
            return ToolOutput::noop(
                "INSERT_EMPTY_CONTENT",
                "content is empty — nothing to insert",
            );
        }

        let path = ctx
            .cwd
            .join(kanzei_harness::permission::normalize_resource(&input.path));
        match tokio::fs::metadata(&path).await {
            Ok(meta) if meta.len() > MAX_FILE_BYTES => {
                return ToolOutput::failed(
                    "INSERT_FILE_TOO_LARGE",
                    format!("{} is too large ({} bytes)", path.display(), meta.len()),
                )
            }
            Err(error) => {
                return ToolOutput::failed(
                    "INSERT_FILE_UNAVAILABLE",
                    format!("cannot access {}: {error}", path.display()),
                )
            }
            _ => {}
        }
        let original = match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(error) => {
                return ToolOutput::failed(
                    "INSERT_READ_FAILED",
                    format!("cannot read {}: {error}", path.display()),
                )
            }
        };

        let mut haystack = original.clone();
        let mut anchor = input.anchor.clone();
        let mut inserted = input.content.clone();
        let mut normalized_endings = false;
        let mut count = haystack.matches(&anchor).count();
        if count == 0 {
            let normalized = original.replace("\r\n", "\n");
            let normalized_anchor = input.anchor.replace("\r\n", "\n");
            let normalized_count = normalized.matches(&normalized_anchor).count();
            if normalized_count > 0 {
                haystack = normalized;
                anchor = normalized_anchor;
                inserted = input.content.replace("\r\n", "\n");
                count = normalized_count;
                normalized_endings = true;
            }
        }
        if count == 0 {
            return ToolOutput::needs_correction(
                "INSERT_ANCHOR_NOT_FOUND",
                format!(
                    "anchor not found in {} — re-read and copy the exact text. Actual file context:\n{}",
                    path.display(),
                    excerpt_around(&original, input.anchor.lines().next().unwrap_or("")),
                ),
            );
        }
        if count > 1 {
            return ToolOutput::needs_correction(
                "INSERT_ANCHOR_NOT_UNIQUE",
                format!(
                    "anchor matches {count} locations in {}; add surrounding lines until it is unique. Actual context around the first match:\n{}",
                    path.display(),
                    excerpt_around(&original, input.anchor.lines().next().unwrap_or("")),
                ),
            );
        }

        let replacement = match input.position {
            InsertPosition::Before => format!("{inserted}{anchor}"),
            InsertPosition::After => format!("{anchor}{inserted}"),
        };
        let updated = haystack.replacen(&anchor, &replacement, 1);
        let updated = if normalized_endings && dominant_crlf(&original) {
            updated.replace('\n', "\r\n")
        } else {
            updated
        };
        if let Err(error) = tokio::fs::write(&path, updated.as_bytes()).await {
            return ToolOutput::failed(
                "INSERT_WRITE_FAILED",
                format!("cannot write {}: {error}", path.display()),
            );
        }
        let mut message = format!(
            "inserted content {} unique anchor in {}",
            match input.position {
                InsertPosition::Before => "before",
                InsertPosition::After => "after",
            },
            path.display()
        );
        if normalized_endings {
            message.push_str(" (line endings differed and were normalized automatically)");
        }
        if let Some(warning) = crate::write::validate_syntax(&path, &updated) {
            message.push_str(&format!("\nWARNING: {warning}"));
        }
        ToolOutput::ok(message).with_display(crate::write::diff_display(
            &input.path,
            &original,
            &updated,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{EditTool, InsertTool};
    use kanzei_harness::{Tool, ToolCtx, ToolOutcome};
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
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        (dir, ctx)
    }

    #[tokio::test]
    async fn 插入形状却顶掉锚点必须拦下来() {
        // R-153 批10 实况:想在 build_runner_config 前面插一个新函数,new_string 却
        // 没带上被匹配的那行签名,结果文件里只剩一个孤零零的 `(`,整个 crate 编译不过。
        // 净行数 +6,纯行数门禁拦不住,只能靠"长了却没保住原文"这个形状。
        let (dir, ctx) = setup(
            "anchor",
            "fn a() {}\n\npub(crate) fn build(\n    x: u8,\n) {}\n",
        );
        let call = json!({
            "path": "target.txt",
            "old_string": "pub(crate) fn build(",
            "new_string": "pub(crate) async fn route(\n    y: u8,\n) -> u8 { y }\n\n("
        });
        let out = EditTool::default().execute(call.clone(), &ctx).await;
        assert!(out.is_error, "插入却吃掉锚点必须拦下: {}", out.content);
        assert_eq!(out.outcome, ToolOutcome::NeedsCorrection);
        assert_eq!(out.code, Some("EDIT_INSERTION_WOULD_REPLACE_ANCHOR"));
        assert!(
            out.content.contains("pub(crate) fn build("),
            "要指出丢了哪一行: {}",
            out.content
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("target.txt")).unwrap(),
            "fn a() {}\n\npub(crate) fn build(\n    x: u8,\n) {}\n",
            "被拦下时文件必须一字未动"
        );

        // 正确的插入写法:把原文原样含进 new_string,直接放行。
        let ok = EditTool::default()
            .execute(
                json!({
                    "path": "target.txt",
                    "old_string": "pub(crate) fn build(",
                    "new_string": "pub(crate) async fn route(y: u8) -> u8 { y }\n\npub(crate) fn build("
                }),
                &ctx,
            )
            .await;
        assert!(!ok.is_error, "保住原文的插入不该被拦: {}", ok.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn 净删除超阈值必须显式确认且不落盘() {
        let (dir, ctx) = setup("netdel", "keep\na\nb\nc\nd\ntail\n");
        let del = json!({"path": "target.txt", "old_string": "a\nb\nc\nd", "new_string": "a"});
        let out = EditTool::default().execute(del.clone(), &ctx).await;
        assert!(out.is_error, "净删除 3 行必须先拦下来: {}", out.content);
        assert_eq!(out.outcome, ToolOutcome::NeedsConfirmation);
        assert!(out.content.contains("allow_deletion"), "{}", out.content);
        assert!(
            out.content.contains("- b"),
            "要列出将被删掉的内容: {}",
            out.content
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("target.txt")).unwrap(),
            "keep\na\nb\nc\nd\ntail\n",
            "被拦下时文件必须一字未动"
        );
        let mut ack = del.as_object().unwrap().clone();
        ack.insert("allow_deletion".into(), json!(true));
        let out = EditTool::default()
            .execute(serde_json::Value::Object(ack), &ctx)
            .await;
        assert!(!out.is_error, "显式确认后应放行: {}", out.content);
        assert_eq!(
            std::fs::read_to_string(dir.join("target.txt")).unwrap(),
            "keep\na\ntail\n"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn 等量替换吃掉邻居时必须把丢失的行报出来() {
        // R-158 实况:写 reasoning effort 的整段被 service_tier 顶掉,净行数为 0,
        // 行数门禁一个都拦不住——唯一的信号就是"old 里有、new 里没有"的那几行。
        let (dir, ctx) = setup(
            "clobber",
            "if request.reasoning.enabled() {\n    body[\"effort\"] = x;\n}\n",
        );
        let out = EditTool::default()
            .execute(
                json!({
                    "path": "target.txt",
                    "old_string": "if request.reasoning.enabled() {\n    body[\"effort\"] = x;",
                    "new_string": "if let Some(tier) = t {\n    body[\"service_tier\"] = tier;"
                }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("NOTE"),
            "等量替换也要报丢失行: {}",
            out.content
        );
        assert!(
            out.content.contains("body[\"effort\"] = x;"),
            "被顶掉的那行必须出现在提示里: {}",
            out.content
        );
        std::fs::remove_dir_all(dir).ok();
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
        assert_eq!(
            saved, "fn main() {\r\n    new();\r\n}\r\n",
            "必须保持文件原有 CRLF 风格"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn first_miss_includes_file_excerpt() {
        let (dir, ctx) = setup("miss", "line one\nline two has anchor\nline three\n");
        let tool = EditTool::default();
        let miss =
            json!({"path": "target.txt", "old_string": "has anchor but wrong", "new_string": "x"});
        let first = tool.execute(miss.clone(), &ctx).await;
        assert!(first.is_error);
        assert!(
            first.content.contains("line two has anchor"),
            "首次未命中就必须给实际上下文: {}",
            first.content
        );
        assert_eq!(first.outcome, ToolOutcome::NeedsCorrection);
        assert_eq!(first.code, Some("EDIT_ANCHOR_NOT_FOUND"));
        let second = tool.execute(miss, &ctx).await;
        assert!(second.is_error);
        assert!(
            second.content.contains("line two has anchor"),
            "第二次必须附文件实际内容: {}",
            second.content
        );
        assert!(
            second.content.contains("Set-Content"),
            "必须明确禁止整文件重写兜底: {}",
            second.content
        );
        // 成功一次后计数清零。
        let ok = tool
            .execute(
                json!({"path": "target.txt", "old_string": "line three", "new_string": "line 3"}),
                &ctx,
            )
            .await;
        assert!(!ok.is_error, "{}", ok.content);
        let third = tool
            .execute(
                json!({"path": "target.txt", "old_string": "nope nope", "new_string": "x"}),
                &ctx,
            )
            .await;
        assert!(
            third.content.contains("Consecutive miss #1"),
            "成功后未命中计数应从 1 重新开始: {}",
            third.content
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn identical_edit_is_noop_not_failure() {
        let (dir, ctx) = setup("noop", "same\n");
        let out = EditTool::default()
            .execute(
                json!({"path": "target.txt", "old_string": "same", "new_string": "same"}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "provider 仍需阻止模型把 no-op 当落盘成功");
        assert_eq!(out.outcome, ToolOutcome::NoOp);
        assert_eq!(out.code, Some("EDIT_IDENTICAL_INPUT"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn insert_preserves_unique_anchor_before_and_after() {
        for (position, expected) in [
            ("before", "head\nnew\nANCHOR\ntail\n"),
            ("after", "head\nANCHOR\nnew\ntail\n"),
        ] {
            let (dir, ctx) = setup(position, "head\nANCHOR\ntail\n");
            let out = InsertTool
                .execute(
                    json!({
                        "path": "target.txt",
                        "anchor": "ANCHOR\n",
                        "content": "new\n",
                        "position": position,
                    }),
                    &ctx,
                )
                .await;
            assert!(!out.is_error, "{}", out.content);
            assert_eq!(
                std::fs::read_to_string(dir.join("target.txt")).unwrap(),
                expected
            );
            std::fs::remove_dir_all(dir).ok();
        }
    }

    #[tokio::test]
    async fn insert_rejects_missing_and_non_unique_anchor_without_writing() {
        let (dir, ctx) = setup("insert-guards", "same\nsame\n");
        let tool = InsertTool;
        let missing = tool
            .execute(
                json!({"path": "target.txt", "anchor": "absent", "content": "x", "position": "before"}),
                &ctx,
            )
            .await;
        assert_eq!(missing.code, Some("INSERT_ANCHOR_NOT_FOUND"));
        assert!(missing.content.contains("Actual file context"));
        let repeated = tool
            .execute(
                json!({"path": "target.txt", "anchor": "same", "content": "x", "position": "after"}),
                &ctx,
            )
            .await;
        assert_eq!(repeated.code, Some("INSERT_ANCHOR_NOT_UNIQUE"));
        assert_eq!(
            std::fs::read_to_string(dir.join("target.txt")).unwrap(),
            "same\nsame\n"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
