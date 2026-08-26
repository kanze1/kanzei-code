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
/// 锚点与文件之间的统一缩进差。`Add` = 文件比锚点多这段前缀,`Strip` = 少这段。
#[derive(Debug, Clone, PartialEq)]
enum IndentDelta {
    Add(String),
    Strip(String),
}

impl IndentDelta {
    fn is_noop(&self) -> bool {
        match self {
            IndentDelta::Add(p) | IndentDelta::Strip(p) => p.is_empty(),
        }
    }

    /// 按同一缩进差重排 new_string。空行不动;`Strip` 时任何一行前缀对不上就放弃
    /// ——宁可退回未命中反馈,也不写出缩进错乱的文本。
    fn reindent(&self, text: &str) -> Option<String> {
        if self.is_noop() {
            return Some(text.to_string());
        }
        let trailing_newline = text.ends_with('\n');
        let mut out = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                out.push(line.to_string());
                continue;
            }
            match self {
                IndentDelta::Add(prefix) => out.push(format!("{prefix}{line}")),
                // 任何一行剥不掉这段前缀就整体放弃(返回 None):宁可退回未命中反馈,
                // 也不写出缩进错乱的文本。
                IndentDelta::Strip(prefix) => {
                    out.push(line.strip_prefix(prefix.as_str())?.to_string());
                }
            }
        }
        let mut joined = out.join("\n");
        if trailing_newline {
            joined.push('\n');
        }
        Some(joined)
    }
}

fn leading_ws(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// 两行缩进之间的统一差;一方不是另一方的前缀(比如 tab 与空格混用)就没有统一解读。
fn indent_delta(file_line: &str, anchor_line: &str) -> Option<IndentDelta> {
    let file_indent = leading_ws(file_line);
    let anchor_indent = leading_ws(anchor_line);
    if let Some(extra) = file_indent.strip_prefix(anchor_indent) {
        return Some(IndentDelta::Add(extra.to_string()));
    }
    anchor_indent
        .strip_prefix(file_indent)
        .map(|extra| IndentDelta::Strip(extra.to_string()))
}

/// 第三层匹配的结果。
enum WhitespaceMatch {
    /// 唯一命中:文件里的实际原文(逐字节)与统一缩进差。
    One {
        matched: String,
        delta: IndentDelta,
    },
    /// 多处仅空白差异的候选——歧义就不猜,退回未命中反馈让模型补上下文。
    Ambiguous(usize),
    None,
}

/// 第三层:仅空白差异(行尾空白 / 统一缩进增删)的容错匹配。
///
/// 前两层(逐字节、CRLF 归一)之外,自举轨迹里剩下的未命中几乎全是同一个形状:
/// 模型凭记忆重写锚点时把缩进抄少了或抄多了,或吞掉了行尾空白。这类锚点在语义上
/// 是**唯一确定**的,却按 old_string not found 打回,换来一轮 read + 重试。
///
/// 判据故意收得很紧,只在"只可能有一种解读"时才自动应用:
/// ① 逐行 trim 后完全相等的**连续**行窗口;② 全文只有一个这样的窗口;
/// ③ 每个非空行的缩进差**完全一致**(统一缩进,不是零散错位)。
/// 任何一条不满足就返回 None/Ambiguous,退回原来的未命中反馈——不猜。
fn whitespace_tolerant_match(content: &str, anchor: &str) -> WhitespaceMatch {
    let anchor_lines: Vec<&str> = anchor.lines().collect();
    if anchor_lines.is_empty() {
        return WhitespaceMatch::None;
    }
    // 行起始字节偏移,用来把命中窗口还原成 content 的精确切片。
    let mut line_starts = Vec::new();
    let mut offset = 0usize;
    for line in content.split('\n') {
        line_starts.push(offset);
        offset += line.len() + 1;
    }
    let content_lines: Vec<&str> = content.split('\n').collect();
    if content_lines.len() < anchor_lines.len() {
        return WhitespaceMatch::None;
    }

    let mut hits: Vec<(usize, IndentDelta)> = Vec::new();
    for start in 0..=(content_lines.len() - anchor_lines.len()) {
        let mut delta: Option<IndentDelta> = None;
        let mut ok = true;
        for (i, anchor_line) in anchor_lines.iter().enumerate() {
            let file_line = content_lines[start + i];
            if file_line.trim() != anchor_line.trim() {
                ok = false;
                break;
            }
            if anchor_line.trim().is_empty() {
                continue;
            }
            match indent_delta(file_line, anchor_line) {
                Some(d) => match &delta {
                    Some(existing) if *existing != d => {
                        ok = false;
                        break;
                    }
                    Some(_) => {}
                    None => delta = Some(d),
                },
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            hits.push((start, delta.unwrap_or(IndentDelta::Add(String::new()))));
        }
    }

    match hits.len() {
        0 => WhitespaceMatch::None,
        1 => {
            let (start, delta) = hits.into_iter().next().unwrap();
            let begin = line_starts[start];
            let last = start + anchor_lines.len() - 1;
            let end = line_starts[last] + content_lines[last].len();
            WhitespaceMatch::One {
                matched: content[begin..end].to_string(),
                delta,
            }
        }
        n => WhitespaceMatch::Ambiguous(n),
    }
}

/// import 形状的行。`import type` 与值 import 可以共存且写法不同,排除在外。
fn import_shaped(line: &str) -> bool {
    let t = line.trim();
    (t.starts_with("import ")
        || t.starts_with("import{")
        || t.starts_with("import*")
        || t.starts_with("from ")
        || t.starts_with("#include"))
        && !t.starts_with("import type ")
}

/// 这次替换新加进来的行(new 有、old 没有,按 trim 后的多重集差)——`dropped_lines` 的反向。
fn added_lines(old: &str, new: &str) -> Vec<String> {
    let mut had: HashMap<&str, usize> = HashMap::new();
    for line in old.lines() {
        *had.entry(line.trim()).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    for line in new.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match had.get_mut(trimmed) {
            Some(remaining) if *remaining > 0 => *remaining -= 1,
            _ => out.push(trimmed.to_string()),
        }
    }
    out
}

/// 只在这些扩展名上查重复 import。Rust 的 `use` 在多个 inline mod 里逐字节重复是
/// 合法的(`mod a { use std::fmt; } mod b { use std::fmt; }`),不能一刀切;
/// JS/TS/Vue 这一族里,文件级出现两条完全相同的 import 一定是 bug。
const IMPORT_DUP_EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs", "vue", "svelte"];

fn checks_duplicate_imports(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMPORT_DUP_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// 这次替换新增的 import 行,在写回后的全文里出现了不止一次。
///
/// 实测形态:模型在 Vue 单文件组件里补 `SafeIcon` 的 import,而文件顶部已经有一条
/// 一模一样的——写进去要等到 lint 那一轮才发现,又是一轮定位 + 修复 + 记 incident。
/// 这是"写之前就能确定是错的"那一类,拦在写盘前最省。
fn duplicated_import_lines(updated: &str, added: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in added {
        if !import_shaped(line) {
            continue;
        }
        let occurrences = updated.lines().filter(|l| l.trim() == line).count();
        if occurrences > 1 && !out.contains(line) {
            out.push(line.clone());
        }
    }
    out
}

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
                let detail = if e.kind() == std::io::ErrorKind::NotFound {
                    crate::missing_path_hint(&path, &input.path, &ctx.project_root)
                } else {
                    format!("cannot access {}: {e}", path.display())
                };
                return ToolOutput::failed("EDIT_FILE_UNAVAILABLE", detail);
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

        // 第三层:仅空白差异(行尾空白 / 统一缩进增删)。只在唯一命中且缩进差一致时
        // 自动对齐——歧义或缩进错位一律退回未命中反馈,不猜。
        let mut whitespace_note = String::new();
        if count == 0 {
            let normalized_content = content.replace("\r\n", "\n");
            let normalized_old = input.old_string.replace("\r\n", "\n");
            match whitespace_tolerant_match(&normalized_content, &normalized_old) {
                WhitespaceMatch::One { matched, delta } => {
                    let normalized_new = input.new_string.replace("\r\n", "\n");
                    if let Some(reindented) = delta.reindent(&normalized_new) {
                        whitespace_note = format!(
                            "\nNOTE: old_string 与文件仅空白差异(唯一命中),已按文件实际缩进对齐{}。\
                             下次直接用文件里的原文当锚点可以省掉这一步。",
                            match &delta {
                                IndentDelta::Add(p) if !p.is_empty() =>
                                    format!(",new_string 每行补了 {} 个前导空白字符", p.len()),
                                IndentDelta::Strip(p) if !p.is_empty() =>
                                    format!(",new_string 每行去掉了 {} 个前导空白字符", p.len()),
                                _ => String::new(),
                            }
                        );
                        haystack = normalized_content;
                        old_string = matched;
                        new_string = reindented;
                        count = 1;
                        if ending_note.is_empty() {
                            ending_note = " (whitespace-only mismatch realigned automatically)";
                        }
                    }
                }
                WhitespaceMatch::Ambiguous(n) => {
                    self.record_miss(&path);
                    return ToolOutput::needs_correction(
                        "EDIT_ANCHOR_WHITESPACE_AMBIGUOUS",
                        format!(
                            "old_string 逐字节没命中,但文件里有 {n} 处只差空白的候选——歧义就不替你猜。\
                             把 old_string 换成文件里的原文(含缩进),或多带几行上下文让它唯一。\
                             \n第一处候选附近的实际内容:\n{}",
                            excerpt_around(
                                &content,
                                input.old_string.lines().next().unwrap_or("").trim()
                            ),
                        ),
                    );
                }
                WhitespaceMatch::None => {}
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
        // 「本想插入,却把锚点吃掉了」:新文本比原文更长(明显是在加东西),原文行却
        // **一行都没保住**。三次实测都是这个形状——R-158 顶掉 Responses 的 reasoning
        // effort、删掉设置页思考强度说明,以及 R-153 批10 把 `pub(crate) fn
        // build_runner_config(` 换成了新函数的开头(净 +6 行,纯行数门禁拦不住)。
        //
        // 判据必须是「全部丢失」而不是「任一丢失」:改写一段代码成更长的版本时,
        // 被改的那几行天然不在 new_string 里原样出现——按「任一丢失」拦,等于把
        // 最常见的合法编辑(增长式改写)整类拦死。D-352 实测:弱模型被连拦四次,
        // 按提示转投 insert 反而把注释插错位置污染文件,陷入清理-重试死循环。
        // 只要 new_string 保住了哪怕一行原文,就说明模型看见并保留了上下文,
        // 是改写不是误顶;真正的误顶(锚点整个消失)仍然全数命中这个判据。
        let old_nonempty_lines = old_string.lines().filter(|l| !l.trim().is_empty()).count();
        let insertion_shaped_clobber = new_line_count > old_line_count
            && old_nonempty_lines > 0
            && dropped.len() == old_nonempty_lines;
        if insertion_shaped_clobber && !input.allow_deletion {
            return ToolOutput::needs_correction(
                "EDIT_INSERTION_WOULD_REPLACE_ANCHOR",
                format!(
                    "这次替换看着像插入(新文本多了 {} 行),old_string 的原文却一行都没保留——\
                 十有八九是想在附近加内容,结果把匹配到的那段顶掉了。\
                 如果这就是有意的整段改写:原样重发这次调用并置 allow_deletion=true 即可放行。\
                 如果本意是插入新内容:把原文原样包含进 new_string,或改用 insert 工具。\
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
        // 新增的 import 在写回后的全文里出现两次 = 重复 import,写之前就能定性。
        // 拦在写盘前,省掉「写进去 → lint 报错 → 定位 → 修复 → 记 incident」整整一轮。
        if checks_duplicate_imports(&path) {
            let duplicated =
                duplicated_import_lines(&updated, &added_lines(&old_string, &new_string));
            if !duplicated.is_empty() {
                self.misses.lock().unwrap().remove(&path);
                return ToolOutput::needs_correction(
                    "EDIT_DUPLICATE_IMPORT",
                    format!(
                        "这次替换会让下面 {} 条 import 在 {} 里出现两次——文件里已经有一模一样的了。\
                         把它从 new_string 里去掉再重发;如果本意是改这条已有的 import,\
                         就把已有那条作为 old_string 直接替换,而不是再加一条。\n{}",
                        duplicated.len(),
                        path.display(),
                        preview_lines(&duplicated)
                    ),
                );
            }
        }
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
        // D-395:写日志凭据——edit 是专用写者,写后留痕供跨树围栏吸收。
        crate::write::record_worktree_write_log(ctx, &input.path, updated.as_bytes());
        self.misses.lock().unwrap().remove(&path);
        let mut message = format!(
            "replaced {count} occurrence(s) in {}{ending_note}{whitespace_note}",
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
        let validation = crate::local_validation::validate_after_write(
            &path,
            &ctx.project_root,
            Some(&content),
            &updated,
        )
        .await;
        message.push_str(&format!("\n{}", validation.summary));
        let mut display = crate::write::diff_display(&input.path, &content, &updated);
        if let Some(object) = display.as_object_mut() {
            object.insert("local_validation".into(), validation.display);
        }
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
                let detail = if error.kind() == std::io::ErrorKind::NotFound {
                    crate::missing_path_hint(&path, &input.path, &ctx.project_root)
                } else {
                    format!("cannot access {}: {error}", path.display())
                };
                return ToolOutput::failed("INSERT_FILE_UNAVAILABLE", detail);
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
        // D-395:写日志凭据——insert 是专用写者,写后留痕供跨树围栏吸收。
        crate::write::record_worktree_write_log(ctx, &input.path, updated.as_bytes());
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
        let validation = crate::local_validation::validate_after_write(
            &path,
            &ctx.project_root,
            Some(&original),
            &updated,
        )
        .await;
        message.push_str(&format!("\n{}", validation.summary));
        let mut display = crate::write::diff_display(&input.path, &original, &updated);
        if let Some(object) = display.as_object_mut() {
            object.insert("local_validation".into(), validation.display);
        }
        ToolOutput::ok(message).with_display(display)
    }
}

#[cfg(test)]
mod tests {
    use super::{EditTool, InsertTool};
    use kanzei_harness::{Tool, ToolCtx, ToolOutcome};
    use serde_json::json;

    fn setup(name: &str, content: &str) -> (std::path::PathBuf, ToolCtx) {
        setup_named(name, "target.txt", content)
    }

    fn setup_named(name: &str, file: &str, content: &str) -> (std::path::PathBuf, ToolCtx) {
        let dir = std::env::temp_dir().join(format!(
            "kz-edit-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), content).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        (dir, ctx)
    }

    /// 验收①:锚点只差缩进时按文件实际缩进自动对齐,不再打回 old_string not found。
    ///
    /// 这是自举轨迹里前两层(逐字节、CRLF)之外剩下的头号未命中形状:模型凭记忆重写
    /// 锚点把缩进抄丢了,语义上唯一确定,却要换一轮 read + 重试。
    #[tokio::test]
    async fn 锚点空白容错_仅缩进差异的锚点自动对齐() {
        let (dir, ctx) = setup(
            "ws-indent",
            "fn outer() {\n    if flag {\n        old_call();\n    }\n}\n",
        );
        let out = EditTool::default()
            .execute(
                json!({
                    "path": "target.txt",
                    "old_string": "if flag {\n    old_call();\n}",
                    "new_string": "if flag {\n    new_call();\n}"
                }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let saved = std::fs::read_to_string(dir.join("target.txt")).unwrap();
        assert_eq!(
            saved, "fn outer() {\n    if flag {\n        new_call();\n    }\n}\n",
            "重排后的缩进必须与文件原有层级一致"
        );
        assert!(out.content.contains("仅空白差异"), "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    /// 多行锚点里某一行带行尾空白时(逐字节必然未命中)照样对齐。
    /// 单行锚点靠子串命中,行尾空白本来就不构成未命中,不在这道判据的射程内。
    #[tokio::test]
    async fn 锚点空白容错_行内行尾空白差异被容忍() {
        let (dir, ctx) = setup("ws-trailing", "fn a() {   \n    old();\n}\n");
        let out = EditTool::default()
            .execute(
                json!({
                    "path": "target.txt",
                    "old_string": "fn a() {\n    old();\n}",
                    "new_string": "fn a() {\n    new();\n}"
                }),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let saved = std::fs::read_to_string(dir.join("target.txt")).unwrap();
        assert_eq!(saved, "fn a() {\n    new();\n}\n");
        std::fs::remove_dir_all(dir).ok();
    }

    /// 验收②:有多处只差空白的候选时不猜,报歧义并要求补上下文——
    /// 自动对齐的前提是"只可能有一种解读"。
    #[tokio::test]
    async fn 锚点空白容错_多处空白候选时报歧义而不猜() {
        // 逐字节零命中(文件里没有 4 空格缩进的 step()),去掉空白后两处候选。
        let (dir, ctx) = setup(
            "ws-ambiguous",
            "fn a() {\n  step();\n}\nfn b() {\n    step();\n}\n",
        );
        let out = EditTool::default()
            .execute(
                json!({
                    "path": "target.txt",
                    "old_string": "        step();",
                    "new_string": "        step2();"
                }),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("只差空白的候选"), "{}", out.content);
        let saved = std::fs::read_to_string(dir.join("target.txt")).unwrap();
        assert!(saved.contains("step();"), "歧义时不得写盘");
        std::fs::remove_dir_all(dir).ok();
    }

    /// 缩进差不统一(逐行错位)不算"仅空白差异",退回原来的未命中反馈。
    #[tokio::test]
    async fn 锚点空白容错_缩进差不统一时不自动对齐() {
        let (dir, ctx) = setup("ws-uneven", "    let a = 1;\n        let b = 2;\n");
        let out = EditTool::default()
            .execute(
                json!({
                    "path": "target.txt",
                    "old_string": "let a = 1;\n  let b = 2;",
                    "new_string": "let a = 9;\n  let b = 9;"
                }),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        let saved = std::fs::read_to_string(dir.join("target.txt")).unwrap();
        assert_eq!(saved, "    let a = 1;\n        let b = 2;\n");
        std::fs::remove_dir_all(dir).ok();
    }

    /// 验收③:新增的 import 在文件里已经有一条一模一样的,拦在写盘前。
    ///
    /// 实测形态是 Vue 单文件组件里重复补 `SafeIcon` 的 import——写进去要等 lint
    /// 那一轮才发现,又是一轮定位 + 修复 + 记 incident。
    #[tokio::test]
    async fn 锚点空白容错_重复import拦在写盘前() {
        let content =
            "import { SafeIcon } from './SafeIcon.vue'\nimport { ref } from 'vue'\n\nconst x = 1\n";
        let (dir, ctx) = setup_named("dup-import", "App.vue", content);
        let out = EditTool::default()
            .execute(
                json!({
                    "path": "App.vue",
                    "old_string": "const x = 1",
                    "new_string": "import { SafeIcon } from './SafeIcon.vue'\nconst x = 1"
                }),
                &ctx,
            )
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(out.content.contains("出现两次"), "{}", out.content);
        assert_eq!(
            std::fs::read_to_string(dir.join("App.vue")).unwrap(),
            content,
            "拦下时不得写盘"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// Rust 的 `use` 在多个 inline mod 里逐字节重复是合法的,不受这道判据影响;
    /// 首次引入一条**新** import 也照常放行。
    #[tokio::test]
    async fn 锚点空白容错_重复import判据不误伤rust与首次引入() {
        let (rs_dir, rs_ctx) = setup_named(
            "dup-import-rs",
            "lib.rs",
            "mod a {\n    use std::fmt;\n}\nmod b {\n    pub fn f() {}\n}\n",
        );
        let out = EditTool::default()
            .execute(
                json!({
                    "path": "lib.rs",
                    "old_string": "    pub fn f() {}",
                    "new_string": "    use std::fmt;\n    pub fn f() {}"
                }),
                &rs_ctx,
            )
            .await;
        assert!(
            !out.is_error,
            "Rust inline mod 重复 use 合法: {}",
            out.content
        );
        std::fs::remove_dir_all(rs_dir).ok();

        let (vue_dir, vue_ctx) = setup_named(
            "new-import-vue",
            "App.vue",
            "import { ref } from 'vue'\n\nconst x = 1\n",
        );
        let out = EditTool::default()
            .execute(
                json!({
                    "path": "App.vue",
                    "old_string": "const x = 1",
                    "new_string": "import { computed } from 'vue'\nconst x = 1"
                }),
                &vue_ctx,
            )
            .await;
        assert!(!out.is_error, "首次引入不该被拦: {}", out.content);
        std::fs::remove_dir_all(vue_dir).ok();
    }

    #[tokio::test]
    async fn missing_path_and_required_parameter_give_recovery_hints() {
        let (dir, ctx) = setup("path-hints", "target\n");
        std::fs::write(dir.join("target-near.txt"), "near\n").unwrap();
        let edit = EditTool::default()
            .execute(
                json!({"path": "targte.txt", "old_string": "x", "new_string": "y"}),
                &ctx,
            )
            .await;
        assert!(edit.is_error, "{}", edit.content);
        assert_eq!(edit.code, Some("EDIT_FILE_UNAVAILABLE"));
        assert!(edit.content.contains("target-near.txt"), "{}", edit.content);

        let insert = InsertTool
            .execute(
                json!({"path": "missing.txt", "anchor": "ANCHOR", "content": "x", "position": "after"}),
                &ctx,
            )
            .await;
        assert!(insert.is_error, "{}", insert.content);
        assert_eq!(insert.code, Some("INSERT_FILE_UNAVAILABLE"));
        assert!(insert.content.contains("target.txt"), "{}", insert.content);

        let missing_path = InsertTool
            .execute(
                json!({"anchor": "ANCHOR", "content": "x", "position": "after"}),
                &ctx,
            )
            .await;
        assert!(missing_path.is_error, "{}", missing_path.content);
        assert!(
            missing_path.content.contains("缺少必填参数 `path`"),
            "{}",
            missing_path.content
        );
        assert!(
            missing_path.content.contains("Example (one line)"),
            "{}",
            missing_path.content
        );
        std::fs::remove_dir_all(dir).ok();
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
    async fn 增长式改写保住部分原文必须放行() {
        // D-352 实况:把 match 的一个分支改写成更长的块——原分支行被改动(不在
        // new_string 里原样出现),但上下文行原样保留。旧判据「任一原文行丢失即拦」
        // 把这类最常见的合法改写整类拦死,弱模型被连拦四次后转投 insert 污染文件。
        let (dir, ctx) = setup(
            "rewrite",
            "match ev {\n    RunEvent::Text(text) => emit(\"kz:text\", text),\n    _ => {}\n}\n",
        );
        let out = EditTool::default()
            .execute(
                json!({
                    "path": "target.txt",
                    "old_string": "match ev {\n    RunEvent::Text(text) => emit(\"kz:text\", text),\n    _ => {}",
                    "new_string": "match ev {\n    RunEvent::Text(text) => {\n        push_text(&text);\n        emit(\"kz:text\", text);\n    }\n    _ => {}"
                }),
                &ctx,
            )
            .await;
        assert!(
            !out.is_error,
            "增长式改写保住了上下文行,不得拦截: {}",
            out.content
        );
        // 被改动的原行仍要在 NOTE 里报出来,供模型自查是否误吃邻居。
        assert!(
            out.content.contains("NOTE"),
            "改写丢行仍需提示: {}",
            out.content
        );
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
