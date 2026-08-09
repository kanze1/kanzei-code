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
}

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> String {
        "Search file contents by regex (ripgrep engine), early-stops at limit. Params: pattern; optional path, glob, limit, files_only.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(GrepInput)).unwrap()
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: GrepInput = match crate::parse_input(self, input) {
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
        let result = tokio::task::spawn_blocking(move || run_grep(&base, input)).await;
        match result {
            Ok(Ok(text)) => ToolOutput::ok(text),
            Ok(Err(e)) => ToolOutput::error(e),
            Err(e) => ToolOutput::error(format!("grep task panicked: {e}")),
        }
    }
}

fn run_grep(base: &std::path::Path, input: GrepInput) -> Result<String, String> {
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
                        lines.push(rel.clone());
                        file_hit = true;
                    }
                    return Ok(false); // 文件级:首个命中即跳下一个文件
                }
                let trimmed: String = text.trim_end().chars().take(MAX_LINE_CHARS).collect();
                lines.push(format!("{rel}:{line_no}: {trimmed}"));
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
