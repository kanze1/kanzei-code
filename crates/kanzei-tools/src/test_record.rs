//! test_record 工具:测试记录 `.kanzei/project/tests.md` 的专用写通道。
//!
//! R-080 的根因是「权限严了却没有配套工具」的另一个实例:`.kanzei/project/*`
//! 对 write/edit 硬 deny、shell 对托管目录回滚,而测试记录没有任何专用写入
//! 通道——test_run_record 是 Tauri 命令,agent 侧没有对应工具,于是 tests.md
//! 永远不存在,左侧栏永远显示"暂无测试记录",归档分支永不执行。
//!
//! 本工具把解析/快照/自动归档/追加记录逻辑下沉到 kanzei-tools:
//! - app 的 `test_runs_snapshot` / `test_run_record` 改为薄封装调用本模块,
//!   避免两套格式解析与归档逻辑漂移;
//! - agent 侧获得 `test_record` 工具:跑完测试后记录一条,状态
//!   running/passed/failed/skipped,终态(passed/failed/skipped)由快照自动
//!   归档进 tests-archive.md,左侧栏展示 active + archived。

use std::path::Path;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// 测试记录(相对项目根)。
pub const TEST_RUNS_REL: &str = ".kanzei/project/tests.md";
/// 测试记录归档(相对项目根)。
pub const TEST_RUNS_ARCHIVE_REL: &str = ".kanzei/project/tests-archive.md";

const VALID_STATUS: &[&str] = &["running", "passed", "failed", "skipped"];

#[derive(Deserialize, JsonSchema)]
struct TestRecordInput {
    /// 测试标题(如 "cargo test -p kanzei-llm")
    title: String,
    /// running | passed | failed | skipped
    status: String,
    /// 实际执行的命令(可选)
    #[serde(default)]
    command: Option<String>,
    /// 结果摘要(可选)
    #[serde(default)]
    summary: Option<String>,
}

pub struct TestRecordTool;

#[async_trait]
impl Tool for TestRecordTool {
    fn name(&self) -> &'static str {
        "test_record"
    }

    fn description(&self) -> String {
        format!(
            "Record a test run into `{TEST_RUNS_REL}` (the ONLY write channel for it — \
             write/edit are denied there). Call it after running tests: title (what was run), \
             status (running/passed/failed/skipped), optional command and summary. Terminal \
             statuses (passed/failed/skipped) are auto-archived into `{TEST_RUNS_ARCHIVE_REL}` \
             on snapshot; the sidebar lists active + archived runs."
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(TestRecordInput)).unwrap()
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        // 写 tests.md,属工作树写操作,不能与其他写工具并行。
        ToolConcurrency::write_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: TestRecordInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let root = ctx.project_root.clone();
        match append_test_run(&root, &input.title, &input.status, input.command.as_deref(), input.summary.as_deref()) {
            Ok(snapshot) => ToolOutput::ok(render_snapshot(&snapshot)),
            Err(err) => ToolOutput::error(err),
        }
    }
}

/// 解析 tests.md / tests-archive.md 的 `## T-xxx 标题 [status]` 块。
pub fn parse_test_blocks(text: &str) -> Vec<(String, serde_json::Value)> {
    text.split("\n## ")
        .filter_map(|raw| {
            let block = if raw.starts_with("## ") {
                raw.to_string()
            } else {
                format!("## {raw}")
            };
            let header = block.lines().next()?.trim_start_matches("## ").trim();
            let status_start = header.rfind('[')?;
            let status_end = header[status_start..].find(']')? + status_start;
            let status = header[status_start + 1..status_end].trim();
            let before = header[..status_start].trim();
            let (id, title) = before
                .split_once(' ')
                .map(|(id, title)| (id.to_string(), title.to_string()))
                .unwrap_or_else(|| (before.to_string(), String::new()));
            let fields = block
                .lines()
                .skip(1)
                .filter_map(|line| line.trim().strip_prefix("- "))
                .filter_map(|line| line.split_once(':'))
                .map(|(key, value)| json!({ "key": key.trim(), "value": value.trim() }))
                .collect::<Vec<_>>();
            Some((
                block.trim_end().to_string(),
                json!({ "id": id, "title": title, "status": status, "fields": fields }),
            ))
        })
        .collect()
}

fn read_test_records(path: &Path) -> Vec<(String, serde_json::Value)> {
    std::fs::read_to_string(path)
        .map(|text| parse_test_blocks(&text))
        .unwrap_or_default()
}

/// 快照:读取 active + archived,并把 active 中的终态记录自动归档。
/// 返回 { active, archived, path, archive_path }。
pub fn test_runs_snapshot(root: &Path) -> Result<serde_json::Value, String> {
    let active_path = root.join(TEST_RUNS_REL);
    let archive_path = root.join(TEST_RUNS_ARCHIVE_REL);
    let active = read_test_records(&active_path);
    let mut live_blocks = Vec::new();
    let mut archived_blocks = Vec::new();
    for (block, record) in active {
        let status = record["status"].as_str().unwrap_or_default();
        if matches!(status, "passed" | "failed" | "skipped") {
            archived_blocks.push(block);
        } else {
            live_blocks.push(block);
        }
    }
    if !archived_blocks.is_empty() {
        let mut archived_text = std::fs::read_to_string(&archive_path)
            .unwrap_or_else(|_| "# Test Runs Archive\n".into());
        for block in archived_blocks {
            archived_text.push_str("\n\n");
            archived_text.push_str(&block);
        }
        if let Some(parent) = archive_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&archive_path, archived_text).map_err(|e| e.to_string())?;
        let active_text = if live_blocks.is_empty() {
            "# Test Runs\n".to_string()
        } else {
            format!("# Test Runs\n\n{}\n", live_blocks.join("\n\n"))
        };
        std::fs::write(&active_path, active_text).map_err(|e| e.to_string())?;
    }
    let live = read_test_records(&active_path)
        .into_iter()
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    let archived = read_test_records(&archive_path)
        .into_iter()
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    Ok(json!({
        "active": live,
        "archived": archived,
        "path": active_path.display().to_string(),
        "archive_path": archive_path.display().to_string(),
    }))
}

/// 追加一条测试记录并返回最新快照(等价于 app 侧 test_run_record)。
pub fn append_test_run(
    root: &Path,
    title: &str,
    status: &str,
    command: Option<&str>,
    summary: Option<&str>,
) -> Result<serde_json::Value, String> {
    if !VALID_STATUS.contains(&status) {
        return Err(format!(
            "测试状态必须是 {} 之一",
            VALID_STATUS.join("、")
        ));
    }
    let path = root.join(TEST_RUNS_REL);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let id = format!(
        "T-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs()
    );
    let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| "# Test Runs\n".into());
    text.push_str(&format!("\n\n## {id} {} [{status}]\n", title.trim()));
    if let Some(command) = command.filter(|value| !value.trim().is_empty()) {
        text.push_str(&format!("- 命令: {}\n", command.trim()));
    }
    if let Some(summary) = summary.filter(|value| !value.trim().is_empty()) {
        text.push_str(&format!("- 摘要: {}\n", summary.trim()));
    }
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    test_runs_snapshot(root)
}

/// 快照渲染成工具可读文本。
fn render_snapshot(snapshot: &serde_json::Value) -> String {
    let active = snapshot["active"].as_array().map(Vec::len).unwrap_or(0);
    let archived = snapshot["archived"].as_array().map(Vec::len).unwrap_or(0);
    let mut lines = vec![format!(
        "recorded. active: {active}, archived: {archived} (path: {})",
        snapshot["path"].as_str().unwrap_or_default()
    )];
    for record in snapshot["active"].as_array().into_iter().flatten() {
        lines.push(format!(
            "● {} {} [{}]",
            record["id"].as_str().unwrap_or_default(),
            record["title"].as_str().unwrap_or_default(),
            record["status"].as_str().unwrap_or_default(),
        ));
    }
    for record in snapshot["archived"].as_array().into_iter().flatten() {
        lines.push(format!(
            "○ {} {} [{}]",
            record["id"].as_str().unwrap_or_default(),
            record["title"].as_str().unwrap_or_default(),
            record["status"].as_str().unwrap_or_default(),
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_project(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-test-record-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parse_blocks_extracts_id_title_status_and_fields() {
        let text = "# Test Runs\n\n## T-1 cargo test [passed]\n- 命令: cargo test\n- 摘要: 全绿\n";
        let blocks = parse_test_blocks(text);
        assert_eq!(blocks.len(), 1);
        let (block, record) = &blocks[0];
        assert_eq!(record["id"], json!("T-1"));
        assert_eq!(record["title"], json!("cargo test"));
        assert_eq!(record["status"], json!("passed"));
        let fields = record["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0]["key"], json!("命令"));
        assert_eq!(fields[0]["value"], json!("cargo test"));
        assert!(block.contains("## T-1 cargo test [passed]"));
    }

    #[test]
    fn parse_blocks_handles_running_without_fields() {
        let text = "# Test Runs\n\n## T-2 long run [running]\n";
        let blocks = parse_test_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].1["id"], json!("T-2"));
        assert_eq!(blocks[0].1["status"], json!("running"));
        assert_eq!(blocks[0].1["fields"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn append_then_snapshot_archives_terminal_status() {
        let root = temp_project("archive");
        let snapshot = append_test_run(&root, "cargo test", "passed", Some("cargo test"), Some("全绿")).unwrap();
        assert_eq!(snapshot["active"].as_array().unwrap().len(), 0);
        assert_eq!(snapshot["archived"].as_array().unwrap().len(), 1);
        let archived = &snapshot["archived"][0];
        assert_eq!(archived["title"], json!("cargo test"));
        assert_eq!(archived["status"], json!("passed"));
        // 归档文件确实落盘。
        assert!(root.join(TEST_RUNS_ARCHIVE_REL).exists());
        let archive_text = std::fs::read_to_string(root.join(TEST_RUNS_ARCHIVE_REL)).unwrap();
        assert!(archive_text.contains("cargo test [passed]"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn running_status_stays_active_until_terminal() {
        let root = temp_project("running");
        append_test_run(&root, "long run", "running", None, None).unwrap();
        let snapshot = test_runs_snapshot(&root).unwrap();
        assert_eq!(snapshot["active"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["active"][0]["status"], json!("running"));
        assert_eq!(snapshot["archived"].as_array().unwrap().len(), 0);
        // 终态后自动归档:running 那条仍留 active,passed 那条进 archive。
        append_test_run(&root, "long run", "passed", None, None).unwrap();
        let snapshot = test_runs_snapshot(&root).unwrap();
        assert_eq!(snapshot["active"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["active"][0]["status"], json!("running"));
        assert_eq!(snapshot["archived"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["archived"][0]["status"], json!("passed"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn invalid_status_is_rejected() {
        let root = temp_project("invalid");
        let err = append_test_run(&root, "x", "bogus", None, None).unwrap_err();
        assert!(err.contains("passed"), "{err}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn tool_records_and_returns_snapshot_text() {
        let root = temp_project("tool");
        let ctx = ToolCtx::new(root.clone());
        let out = TestRecordTool
            .execute(
                json!({"title": "cargo test -p kanzei-llm", "status": "passed", "command": "cargo test -p kanzei-llm"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("active: 0, archived: 1"), "{}", out.content);
        assert!(out.content.contains("cargo test -p kanzei-llm [passed]"), "{}", out.content);
        assert!(root.join(TEST_RUNS_REL).exists());
        std::fs::remove_dir_all(&root).ok();
    }
}
