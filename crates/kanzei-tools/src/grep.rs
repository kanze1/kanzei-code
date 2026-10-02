//! grep 工具(R-026):ripgrep 内核(grep-searcher),head-limit 早停——
//! 命中数够了立即停止,绝不全仓扫完再排序(设计红线 3,Kimi Code 反面教材)。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

const DEFAULT_LIMIT: usize = 50;
const MAX_LINE_CHARS: usize = 300;

#[derive(Deserialize, JsonSchema)]
struct GrepInput {
    /// 正则(ripgrep 语法)
    pattern: String,
    /// 起始目录或文件(默认 cwd)
    #[serde(default, alias = "dir")]
    path: Option<String>,
    /// 只搜匹配此 glob 的文件,如 "*.rs"
    #[serde(default)]
    glob: Option<String>,
    /// 最多返回条数(默认 50)
    #[serde(default)]
    limit: Option<usize>,
    /// 只列出含匹配的文件路径
    #[serde(default)]
    files_only: bool,
    /// 统计模式:返回每个文件的匹配行数 + 总数,不返回具体行。
    /// 完整扫描(不早停)——"数数/聚合"本就要求全量,与默认的 head-limit 早停是两种语义。
    #[serde(default)]
    count: bool,
    /// R-325:忽略大小写(等价 rg -i)。
    #[serde(default, alias = "ignore_case")]
    case_insensitive: bool,
    /// R-325:匹配行前后各带 N 行上下文(等价 rg -C)。被 before/after 单独设置时覆盖。
    #[serde(default)]
    context: Option<usize>,
    /// R-325:匹配行**前** N 行上下文(等价 rg -B)。
    #[serde(default)]
    before_context: Option<usize>,
    /// R-325:匹配行**后** N 行上下文(等价 rg -A)。
    #[serde(default)]
    after_context: Option<usize>,
    /// R-325:多行模式——模式可横跨若干行且 `.` 匹配换行(等价 rg -U --multiline-dotall)。
    #[serde(default)]
    multiline: bool,
}

/// 上下文行数上限。放开无界会让一次 grep 把整个文件倒进上下文——那是 read 的活。
const MAX_CONTEXT_LINES: usize = 20;

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> String {
        "Search file contents by regex (ripgrep engine), early-stops at limit. Params: pattern; optional path, glob, limit, files_only, count (per-file match counts + total, full scan), case_insensitive, context / before_context / after_context (context lines around each match, capped at 20; context lines print with `-` instead of `:` like ripgrep), multiline (pattern may span lines, `.` matches newline). \
         Independent grep calls in the SAME step run in parallel: batch several patterns together rather than one per step.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(GrepInput)).unwrap()
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        // R-244 批4:grep 迁移走统一 pipeline(与 read/glob 同构;guards/策略/
        // 观察者现阶段空,权限判定在 drive 层)。SubagentBase 只读族至此全走通道。
        let input2 = input.clone();
        let ctx2 = ctx.clone();
        kanzei_harness::tool_pipeline::run_tool_pipeline(
            "grep",
            input,
            ctx,
            &[],
            async move { grep_body(self, &input2, &ctx2).await },
            &[],
            &[],
        )
        .await
    }
}

/// R-244 批4:grep 工具本体(原 execute 主体),供 pipeline body 调用。
async fn grep_body(tool: &dyn Tool, input: &serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
    let input: GrepInput = match crate::parse_input(tool, input.clone()) {
        Ok(v) => v,
        Err(out) => return out,
    };
    let base = match &input.path {
        Some(p) => ctx.cwd.join(p),
        None => ctx.cwd.clone(),
    };
    if !base.exists() {
        return ToolOutput::error(format!("path not found: {}", base.display()));
    }
    // 输出路径一律相对 cwd,**不**相对 `path` 参数:否则 `path="crates"` 搜出来的
    // `kanzei/tests/x.rs` 直接喂给 read 就扑空,而输出里没有任何地方交代 base 是
    // `crates`。实测自举因此连续两次 read 失败,还花了一大段推理去猜"为什么 glob
    // 有 crates/ 前缀而 grep 没有"。检索工具的输出必须能直接当路径用。
    let rel_root = ctx.cwd.clone();
    let result = tokio::task::spawn_blocking(move || run_grep(&base, &rel_root, input)).await;
    match result {
        Ok(Ok(text)) => ToolOutput::ok(text),
        Ok(Err(e)) => ToolOutput::error(e),
        Err(e) => ToolOutput::error(format!("grep task panicked: {e}")),
    }
}

/// 展示用路径:相对 cwd,可直接喂给 read/edit。
///
/// 必须与**匹配用**的 base 相对路径分开:`glob` 参数是相对 `path` 子树匹配的
/// (`glob="*.rs"` 配 `path="crates"` 时匹配 `kanzei/src/x.rs`),把匹配基准也改成
/// cwd 会静默改变 glob 语义。这里只改显示。
pub(crate) fn display_path(path: &std::path::Path, rel_root: &std::path::Path) -> String {
    path.strip_prefix(rel_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn run_grep(
    base: &std::path::Path,
    rel_root: &std::path::Path,
    input: GrepInput,
) -> Result<String, String> {
    use grep_searcher::sinks::UTF8;
    use grep_searcher::{BinaryDetection, SearcherBuilder};

    // R-325:大小写与多行必须在 matcher 构造期决定——RegexMatcher 建好之后改不了。
    let matcher = grep_regex::RegexMatcherBuilder::new()
        .case_insensitive(input.case_insensitive)
        .multi_line(input.multiline)
        .dot_matches_new_line(input.multiline)
        .build(&input.pattern)
        .map_err(|e| format!("invalid regex `{}`: {e}", input.pattern))?;
    let glob_matcher = match &input.glob {
        Some(g) => Some(
            globset::GlobBuilder::new(g)
                .literal_separator(false)
                .build()
                .map_err(|e| format!("invalid glob `{g}`: {e}"))?
                .compile_matcher(),
        ),
        None => None,
    };
    let limit = input.limit.unwrap_or(DEFAULT_LIMIT).max(1);

    if input.count {
        return run_count(base, rel_root, &input, &matcher, &glob_matcher);
    }

    // R-325:上下文行数——before/after 单独给就用它,否则回落 context;两侧都封顶。
    let ctx_default = input.context.unwrap_or(0);
    let before = input
        .before_context
        .unwrap_or(ctx_default)
        .min(MAX_CONTEXT_LINES);
    let after = input
        .after_context
        .unwrap_or(ctx_default)
        .min(MAX_CONTEXT_LINES);
    // files_only 只要文件名,带上下文没有意义,还会让早停判据错乱。
    let want_context = (before > 0 || after > 0) && !input.files_only;

    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .before_context(if want_context { before } else { 0 })
        .after_context(if want_context { after } else { 0 })
        .multi_line(input.multiline)
        .build();

    let mut lines: Vec<String> = Vec::new();
    let mut done = false;
    // 默认 hidden(true) 会把 .kanzei/(需求、缺陷、规范全在这)、.github/、.claude/ 整个跳过,
    // 模型据此得出"文件不存在"的假阴性;仍尊重 .gitignore,不会扫进 target/(D-071)。
    for entry in ignore::WalkBuilder::new(base)
        .hidden(false)
        .build()
        .flatten()
    {
        if done {
            break;
        }
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(base)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(gm) = &glob_matcher {
            let name = entry
                .path()
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            if !gm.is_match(&rel) && !gm.is_match(name.as_ref()) {
                continue;
            }
        }
        let shown = display_path(entry.path(), rel_root);
        if want_context {
            // 上下文模式:UTF8 sink 只回调 matched,拿不到 context 行,必须自己实现 Sink。
            let mut sink = ContextSink {
                shown: &shown,
                lines: &mut lines,
                limit,
                done: &mut done,
            };
            let _ = searcher.search_path(&matcher, entry.path(), &mut sink);
            continue;
        }
        let mut file_hit = false;
        let _ = searcher.search_path(
            &matcher,
            entry.path(),
            UTF8(|line_no, text| {
                if lines.len() >= limit {
                    done = true;
                    return Ok(false); // 早停:够数就掐断本文件
                }
                if input.files_only {
                    if !file_hit {
                        lines.push(shown.clone());
                        file_hit = true;
                    }
                    return Ok(false); // 文件级:首个命中即跳下一个文件
                }
                let trimmed: String = text.trim_end().chars().take(MAX_LINE_CHARS).collect();
                lines.push(format!("{shown}:{line_no}: {trimmed}"));
                Ok(true)
            }),
        );
    }

    if lines.is_empty() {
        return Ok(format!("(no matches for `{}`)", input.pattern));
    }
    let mut out = lines.join("\n");
    if done {
        out.push_str(&format!(
            "\n... (stopped at limit {limit}; narrow the pattern or raise limit)"
        ));
    }
    Ok(out)
}

/// 统计模式:每个文件的匹配行数 + 总数。完整扫描,不做 head-limit 早停——
/// "数数"的要求就是全量,与默认搜索的早停语义不同;这是用户主动选择的聚合通道,
/// 不是默认路径上的全仓扫描。
fn run_count(
    base: &std::path::Path,
    rel_root: &std::path::Path,
    input: &GrepInput,
    matcher: &grep_regex::RegexMatcher,
    glob_matcher: &Option<globset::GlobMatcher>,
) -> Result<String, String> {
    use grep_searcher::sinks::UTF8;
    use grep_searcher::{BinaryDetection, SearcherBuilder};

    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .build();

    let mut file_counts: Vec<(String, u64)> = Vec::new();
    for entry in ignore::WalkBuilder::new(base)
        .hidden(false)
        .build()
        .flatten()
    {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(base)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(gm) = glob_matcher {
            let name = entry
                .path()
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            if !gm.is_match(&rel) && !gm.is_match(name.as_ref()) {
                continue;
            }
        }
        let mut count = 0u64;
        let _ = searcher.search_path(
            matcher,
            entry.path(),
            UTF8(|_, _| {
                count += 1;
                Ok(true)
            }),
        );
        if count > 0 {
            file_counts.push((display_path(entry.path(), rel_root), count));
        }
    }

    if file_counts.is_empty() {
        return Ok(format!("(no matches for `{}`)", input.pattern));
    }
    let total: u64 = file_counts.iter().map(|(_, c)| c).sum();
    let mut out = file_counts
        .iter()
        .map(|(f, c)| format!("{f}: {c}"))
        .collect::<Vec<_>>()
        .join("\n");
    out.push_str(&format!(
        "\n(total {total} matches in {} files)",
        file_counts.len()
    ));
    Ok(out)
}

/// R-325:带上下文的搜索 sink。
///
/// `grep_searcher::sinks::UTF8` 只回调 `matched`,拿不到 `context` 行,所以带上下文时
/// 必须自己实现 [`grep_searcher::Sink`]。输出沿用 ripgrep 的惯例:**匹配行用 `:`
/// 分隔,上下文行用 `-`**——模型见惯这个形态,不必再学一套标记。
///
/// 早停语义与无上下文路径一致:总行数(含上下文)到 limit 就掐断,并置 `done`
/// 让外层停止遍历后续文件。
struct ContextSink<'a> {
    shown: &'a str,
    lines: &'a mut Vec<String>,
    limit: usize,
    done: &'a mut bool,
}

impl ContextSink<'_> {
    /// 追加一行;返回 false 表示已到上限,调用方应停止。
    fn push(&mut self, line_no: Option<u64>, sep: char, bytes: &[u8]) -> bool {
        if self.lines.len() >= self.limit {
            *self.done = true;
            return false;
        }
        let text = String::from_utf8_lossy(bytes);
        let trimmed: String = text.trim_end().chars().take(MAX_LINE_CHARS).collect();
        let shown = self.shown;
        match line_no {
            Some(n) => self.lines.push(format!("{shown}{sep}{n}{sep} {trimmed}")),
            // 多行模式下 grep_searcher 可能不给行号;不编造,留空位。
            None => self.lines.push(format!("{shown}{sep} {trimmed}")),
        }
        true
    }
}

impl grep_searcher::Sink for ContextSink<'_> {
    type Error = std::io::Error;

    fn matched(
        &mut self,
        _searcher: &grep_searcher::Searcher,
        mat: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        Ok(self.push(mat.line_number(), ':', mat.bytes()))
    }

    fn context(
        &mut self,
        _searcher: &grep_searcher::Searcher,
        ctx: &grep_searcher::SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        Ok(self.push(ctx.line_number(), '-', ctx.bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-grep-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "// no match here\n").unwrap();
        std::fs::write(root.join("docs/c.md"), "fn doc() {}\n").unwrap();
        root
    }

    /// count 模式:每文件匹配行数 + 总数,完整扫描不早停。
    #[test]
    fn count模式按文件计数并汇总() {
        let root = fixture("count");
        let input = GrepInput {
            pattern: "fn".into(),
            path: None,
            glob: None,
            limit: None,
            files_only: false,
            count: true,
            case_insensitive: false,
            context: None,
            before_context: None,
            after_context: None,
            multiline: false,
        };
        let matcher = grep_regex::RegexMatcher::new("fn").unwrap();
        let out = run_count(&root, &root, &input, &matcher, &None).unwrap();
        assert!(out.contains("src/a.rs: 2"), "{out}");
        assert!(out.contains("docs/c.md: 1"), "{out}");
        assert!(!out.contains("src/b.rs"), "{out}");
        assert!(out.contains("(total 3 matches in 2 files)"), "{out}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// count 模式无命中时给明确的空结果,而不是空字符串。
    #[test]
    fn count模式无命中返回明确空结果() {
        let root = fixture("count-none");
        let input = GrepInput {
            pattern: "zzz_none_zzz".into(),
            path: None,
            glob: None,
            limit: None,
            files_only: false,
            count: true,
            case_insensitive: false,
            context: None,
            before_context: None,
            after_context: None,
            multiline: false,
        };
        let matcher = grep_regex::RegexMatcher::new("zzz_none_zzz").unwrap();
        let out = run_count(&root, &root, &input, &matcher, &None).unwrap();
        assert_eq!(out, "(no matches for `zzz_none_zzz`)");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 传了 `path` 子目录时,输出路径必须仍相对 cwd——即带上该子目录前缀,
    /// 让模型可以把结果直接喂给 read。
    ///
    /// 修复前:输出相对 `path` 参数(这里会是 `a.rs`),而输出里没有一个字交代
    /// base 是 `src`。实测自举把 `kanzei/tests/x.rs` 直接喂给 read 连扑两次空,
    /// 还花了一大段推理去猜"为什么 glob 有 crates/ 前缀而 grep 没有"。
    #[test]
    fn 传了path子目录时输出路径仍相对cwd() {
        let root = fixture("relroot");
        let input = GrepInput {
            pattern: "fn".into(),
            path: Some("src".into()),
            glob: None,
            limit: None,
            files_only: true,
            count: false,
            case_insensitive: false,
            context: None,
            before_context: None,
            after_context: None,
            multiline: false,
        };
        // base = 子目录(决定扫描范围),rel_root = cwd(决定显示基准)
        let out = run_grep(&root.join("src"), &root, input).unwrap();
        assert!(
            out.contains("src/a.rs"),
            "输出应带 src/ 前缀、可直接当路径用,实际: {out}"
        );
        assert!(
            !out.lines().any(|l| l.trim() == "a.rs"),
            "不应出现相对 path 参数的裸文件名(那种路径喂给 read 会扑空): {out}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 显示基准换成 cwd 之后,`glob` 过滤的语义必须原样不变——它仍相对 `path`
    /// 子树匹配。把匹配基准也一起改掉会静默改变 glob 行为,这条钉住边界。
    #[test]
    fn glob过滤仍相对path子树匹配不受显示基准影响() {
        let root = fixture("globsem");
        let input = GrepInput {
            pattern: "fn".into(),
            path: Some("src".into()),
            // 相对 src/ 子树是 `a.rs`;若匹配基准被误改成 cwd,则实际待匹配串是
            // `src/a.rs`,这个模式就会落空。
            glob: Some("a.rs".into()),
            limit: None,
            files_only: true,
            count: false,
            case_insensitive: false,
            context: None,
            before_context: None,
            after_context: None,
            multiline: false,
        };
        let out = run_grep(&root.join("src"), &root, input).unwrap();
        assert!(
            out.contains("src/a.rs"),
            "glob 应相对 src/ 子树匹配到 a.rs,实际: {out}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    // ---- R-325:上下文 / 大小写 / 多行 ----

    fn ctx_fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-grep-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("x.rs"),
            "line1
line2
TARGET here
line4
line5
",
        )
        .unwrap();
        root
    }

    fn base_input(pattern: &str) -> GrepInput {
        GrepInput {
            pattern: pattern.into(),
            path: None,
            glob: None,
            limit: Some(100),
            files_only: false,
            count: false,
            case_insensitive: false,
            context: None,
            before_context: None,
            after_context: None,
            multiline: false,
        }
    }

    /// context 两侧都带,匹配行用 `:`、上下文行用 `-`(ripgrep 惯例)。
    #[test]
    fn 上下文两侧各带一行且标记区分匹配与上下文() {
        let root = ctx_fixture("ctx");
        let out = run_grep(
            &root,
            &root,
            GrepInput {
                context: Some(1),
                ..base_input("TARGET")
            },
        )
        .unwrap();
        assert!(out.contains("x.rs:3: TARGET here"), "匹配行用 `:`: {out}");
        assert!(
            out.contains("x.rs-2- line2"),
            "前一行是上下文,用 `-`: {out}"
        );
        assert!(
            out.contains("x.rs-4- line4"),
            "后一行是上下文,用 `-`: {out}"
        );
        assert!(
            !out.contains("line1"),
            "只要 1 行上下文,line1 不该出现: {out}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// before/after 可以不对称,且各自覆盖 context。
    #[test]
    fn 前后上下文可不对称且覆盖context() {
        let root = ctx_fixture("asym");
        let out = run_grep(
            &root,
            &root,
            GrepInput {
                context: Some(1),
                before_context: Some(2),
                after_context: Some(0),
                ..base_input("TARGET")
            },
        )
        .unwrap();
        assert!(
            out.contains("x.rs-1- line1"),
            "before=2 应拿到 line1: {out}"
        );
        assert!(out.contains("x.rs-2- line2"));
        assert!(!out.contains("line4"), "after=0 不该有后置上下文: {out}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 上下文行数封顶:一次 grep 不该把整个文件倒进上下文(那是 read 的活)。
    #[test]
    fn 上下文行数封顶() {
        let root = std::env::temp_dir().join(format!(
            "kz-grep-cap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        // 200 行,TARGET 在正中——两侧都远超封顶,才测得出封顶是否真的生效。
        let mut body = String::new();
        for n in 1..=200 {
            body.push_str(if n == 100 {
                "TARGET
"
            } else {
                "filler
"
            });
        }
        std::fs::write(root.join("big.rs"), body).unwrap();
        let out = run_grep(
            &root,
            &root,
            GrepInput {
                context: Some(9999),
                limit: Some(10_000),
                ..base_input("TARGET")
            },
        )
        .unwrap();
        let emitted = out.lines().filter(|l| !l.starts_with("...")).count();
        assert!(
            emitted <= 1 + MAX_CONTEXT_LINES * 2,
            "封顶未生效:发出 {emitted} 行,上限应为 {}",
            1 + MAX_CONTEXT_LINES * 2
        );
        assert!(out.contains("big.rs:100: TARGET"), "匹配行必须在: {out}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// files_only 与上下文互斥:只要文件名时带上下文没有意义。
    #[test]
    fn files_only_不受上下文影响() {
        let root = ctx_fixture("fo");
        let out = run_grep(
            &root,
            &root,
            GrepInput {
                files_only: true,
                context: Some(3),
                ..base_input("TARGET")
            },
        )
        .unwrap();
        assert_eq!(out.trim(), "x.rs", "files_only 应只回文件名: {out}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 忽略大小写() {
        let root = ctx_fixture("ci");
        let hit = run_grep(
            &root,
            &root,
            GrepInput {
                case_insensitive: true,
                ..base_input("target")
            },
        )
        .unwrap();
        assert!(hit.contains("TARGET here"), "-i 应命中: {hit}");
        let miss = run_grep(&root, &root, base_input("target")).unwrap();
        assert!(miss.contains("(no matches"), "默认区分大小写: {miss}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 多行模式:模式可横跨若干行,`.` 匹配换行。
    #[test]
    fn 多行模式可跨行匹配() {
        let root = ctx_fixture("ml");
        let out = run_grep(
            &root,
            &root,
            GrepInput {
                multiline: true,
                ..base_input("line2.*TARGET")
            },
        )
        .unwrap();
        assert!(out.contains("TARGET"), "多行模式应跨行命中: {out}");
        let single = run_grep(&root, &root, base_input("line2.*TARGET")).unwrap();
        assert!(
            single.contains("(no matches"),
            "单行模式不该跨行命中: {single}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 上下文模式仍受 limit 早停约束,不会因为上下文放大而无界输出。
    #[test]
    fn 上下文模式仍然遵守limit早停() {
        let root = ctx_fixture("limit");
        let out = run_grep(
            &root,
            &root,
            GrepInput {
                context: Some(2),
                limit: Some(2),
                ..base_input("TARGET")
            },
        )
        .unwrap();
        let body = out.lines().filter(|l| !l.starts_with("...")).count();
        assert!(body <= 2, "含上下文的总行数必须受 limit 约束: {out}");
        assert!(out.contains("stopped at limit"), "到限要给出提示: {out}");
        std::fs::remove_dir_all(&root).ok();
    }
}
