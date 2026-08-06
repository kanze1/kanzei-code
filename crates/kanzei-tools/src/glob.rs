//! glob 工具(R-026):按模式找文件,尊重 .gitignore,按修改时间新→旧排序,head-limit 截断。
//! 设计红线 3:扫描有界,绝不无限 stat。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

const DEFAULT_LIMIT: usize = 100;
/// 扫描上界:访问文件数达到该值即停(防巨型仓库拖死)。
const SCAN_CAP: usize = 50_000;

#[derive(Deserialize, JsonSchema)]
struct GlobInput {
    /// 模式,如 "**/*.rs"、"src/**/*.ts"
    pattern: String,
    /// 起始目录(默认 cwd)
    #[serde(default, alias = "dir", alias = "cwd")]
    path: Option<String>,
    /// 最多返回条数(默认 100)
    #[serde(default)]
    limit: Option<usize>,
}

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &'static str {
        "glob"
    }

    fn description(&self) -> String {
        "Find files by glob pattern (e.g. **/*.rs), newest first, respects .gitignore. Params: pattern; optional path, limit.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(GlobInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: GlobInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let base = match &input.path {
            Some(p) => ctx.cwd.join(p),
            None => ctx.cwd.clone(),
        };
        if !base.is_dir() {
            return ToolOutput::error(format!("directory not found: {}", base.display()));
        }
        let limit = input.limit.unwrap_or(DEFAULT_LIMIT).max(1);
        let pattern = input.pattern.clone();

        let result = tokio::task::spawn_blocking(move || run_glob(&base, &pattern, limit)).await;
        match result {
            Ok(Ok(text)) => ToolOutput::ok(text),
            Ok(Err(e)) => ToolOutput::error(e),
            Err(e) => ToolOutput::error(format!("glob task panicked: {e}")),
        }
    }
}

fn run_glob(base: &std::path::Path, pattern: &str, limit: usize) -> Result<String, String> {
    let matcher = globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .build()
        .map_err(|e| format!("invalid glob pattern `{pattern}`: {e}"))?
        .compile_matcher();

    let mut hits: Vec<(std::time::SystemTime, String)> = Vec::new();
    let mut scanned = 0usize;
    let mut capped = false;
    for entry in ignore::WalkBuilder::new(base).build().flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        scanned += 1;
        if scanned > SCAN_CAP {
            capped = true;
            break;
        }
        let rel = entry
            .path()
            .strip_prefix(base)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if matcher.is_match(&rel) {
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            hits.push((mtime, rel));
        }
    }
    if hits.is_empty() {
        return Ok(format!("(no files match `{pattern}`)"));
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    let total = hits.len();
    let mut out: Vec<String> = hits.into_iter().take(limit).map(|(_, p)| p).collect();
    if total > limit {
        out.push(format!("... ({} more; raise limit or narrow pattern)", total - limit));
    }
    if capped {
        out.push(format!("(scan capped at {SCAN_CAP} files)"));
    }
    Ok(out.join("\n"))
}
