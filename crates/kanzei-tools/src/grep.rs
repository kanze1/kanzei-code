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
}

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> String {
        "Search file contents by regex (ripgrep engine), early-stops at limit. Params: pattern; optional path, glob, limit, files_only, count (per-file match counts + total, full scan).".into()
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

    let matcher = grep_regex::RegexMatcher::new(&input.pattern)
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

    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
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
        };
        let out = run_grep(&root.join("src"), &root, input).unwrap();
        assert!(
            out.contains("src/a.rs"),
            "glob 应相对 src/ 子树匹配到 a.rs,实际: {out}"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
