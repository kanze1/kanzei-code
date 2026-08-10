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
    /// 关联条目 ID 列表(如 ["D-201", "R-153"]);写入「关联」字段,
    /// 建立测试→缺陷/需求的映射,供按条目反查测试记录
    #[serde(default)]
    refs: Option<Vec<String>>,
    /// 要收尾的既有记录 id(如 "T-1786254656");省略时按标题自动认领同名 running 记录
    #[serde(default)]
    id: Option<String>,
}

/// running 记录多久没收尾就算悬空。自举一轮的定向测试通常几分钟内出结果,
/// 半小时还挂着基本等于"跑完忘了登记"或"根本没跑"。
pub const STALE_RUNNING_SECS: u64 = 1800;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 记录 id 里带着创建时刻(T-<epoch>),据此算悬空时长。
fn running_age_secs(id: &str) -> Option<u64> {
    let stamp: u64 = id.strip_prefix("T-")?.parse().ok()?;
    now_secs().checked_sub(stamp)
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
        match record_test_run(
            &root,
            input.id.as_deref(),
            &input.title,
            &input.status,
            input.command.as_deref(),
            input.summary.as_deref(),
            input.refs.as_deref(),
        ) {
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
            // R-130:结构化「关联」字段——测试→缺陷/需求映射。旧记录没有此字段时
            // refs 为空,由 initialize_refs 从标题回填。
            let refs = fields
                .iter()
                .find(|f| f["key"] == "关联")
                .and_then(|f| f["value"].as_str())
                .map(|v| {
                    v.split_whitespace()
                        .filter(|part| is_entry_id(part))
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some((
                block.trim_end().to_string(),
                json!({
                    "id": id, "title": title, "status": status,
                    "fields": fields, "refs": refs
                }),
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
        .map(|(_, mut record)| {
            // 悬空标记:running 挂太久要在快照里看得见,否则"没跑"和"跑完忘了记"
            // 在界面和 agent 眼里长得一模一样。
            if record["status"].as_str() == Some("running") {
                if let Some(age) = running_age_secs(record["id"].as_str().unwrap_or_default()) {
                    record["age_secs"] = json!(age);
                    record["stale"] = json!(age >= STALE_RUNNING_SECS);
                }
            }
            record
        })
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

/// 登记一条测试记录。终态优先**就地收尾**已有的同名 running 记录,而不是再追加一条。
///
/// 此前工具只有 append 一条路径:agent 先记 running、跑完再记 passed,于是每跑一次测试
/// 就留下一条永远关不掉的 running——实测一天累积 41 条。悬空的 running 与"根本没跑过"
/// 在数据上完全一样,A-009/R-152 那条证据链就是被这个洞掏空的。
pub fn record_test_run(
    root: &Path,
    id: Option<&str>,
    title: &str,
    status: &str,
    command: Option<&str>,
    summary: Option<&str>,
    refs: Option<&[String]>,
) -> Result<serde_json::Value, String> {
    if !VALID_STATUS.contains(&status) {
        return Err(format!("测试状态必须是 {} 之一", VALID_STATUS.join("、")));
    }
    let path = root.join(TEST_RUNS_REL);
    let existing = read_test_records(&path);
    // 认领目标:显式 id 优先;否则终态自动认领同标题的 running 记录(最新的一条)。
    let target = existing.iter().rev().find(|(_, record)| {
        let record_id = record["id"].as_str().unwrap_or_default();
        let record_status = record["status"].as_str().unwrap_or_default();
        match id {
            Some(wanted) => record_id == wanted,
            None => {
                record_status == "running"
                    && record["title"].as_str().unwrap_or_default().trim() == title.trim()
                    && status != "running"
            }
        }
    });
    let Some((old_block, record)) = target else {
        if let Some(wanted) = id {
            return Err(format!(
                "找不到测试记录 {wanted};省略 id 可新登记一条,或先用 test_record 列表核对 id"
            ));
        }
        return append_test_run(root, title, status, command, summary, refs);
    };
    let record_id = record["id"].as_str().unwrap_or_default().to_string();
    let mut block = format!("## {record_id} {} [{status}]\n", title.trim());
    // 命令/摘要:本次没给就沿用原记录的,不要把已有信息覆盖成空。
    let inherited = |key: &str| -> Option<String> {
        record["fields"]
            .as_array()?
            .iter()
            .find(|f| f["key"].as_str() == Some(key))
            .and_then(|f| f["value"].as_str())
            .map(|v| v.to_string())
    };
    if let Some(command) = command
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
        .or_else(|| inherited("命令"))
    {
        block.push_str(&format!("- 命令: {command}\n"));
    }
    if let Some(summary) = summary
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
        .or_else(|| inherited("摘要"))
    {
        block.push_str(&format!("- 摘要: {summary}\n"));
    }
    if let Some(refs) = refs
        .filter(|list| !list.is_empty())
        .map(|list| list.join(" "))
        .or_else(|| inherited("关联"))
    {
        block.push_str(&format!("- 关联: {refs}\n"));
    }
    if status != "running" {
        // 收尾时刻:记录 id 是**开始**时间,而提交门禁要问的是"测试跑完在改完代码之后吗"。
        // 必须单独落一个终点时间,否则先起 running 再改代码就能骗过门禁。
        block.push_str(&format!("- 收尾: {}\n", now_secs()));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let updated = text.replace(old_block.as_str(), block.trim_end());
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    test_runs_snapshot(root)
}

/// 追加一条测试记录并返回最新快照(等价于 app 侧 test_run_record)。
pub fn append_test_run(
    root: &Path,
    title: &str,
    status: &str,
    command: Option<&str>,
    summary: Option<&str>,
    refs: Option<&[String]>,
) -> Result<serde_json::Value, String> {
    if !VALID_STATUS.contains(&status) {
        return Err(format!("测试状态必须是 {} 之一", VALID_STATUS.join("、")));
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
    if let Some(refs) = refs.filter(|list| !list.is_empty()) {
        text.push_str(&format!("- 关联: {}\n", refs.join(" ")));
    }
    if status != "running" {
        text.push_str(&format!("- 收尾: {}\n", now_secs()));
    }
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    test_runs_snapshot(root)
}

/// 最近一次「通过」的测试是什么时候收尾的(epoch 秒)。active + archive 一起看。
///
/// 取收尾时刻而不是记录 id:id 是测试**开始**的时间,先起 running 再改代码就能骗过门禁。
pub fn last_passed_at(root: &Path) -> Option<u64> {
    let mut newest = None;
    for rel in [TEST_RUNS_REL, TEST_RUNS_ARCHIVE_REL] {
        for (_, record) in read_test_records(&root.join(rel)) {
            if record["status"].as_str() != Some("passed") {
                continue;
            }
            let finished = record["fields"]
                .as_array()
                .and_then(|fields| {
                    fields
                        .iter()
                        .find(|f| f["key"].as_str() == Some("收尾"))
                        .and_then(|f| f["value"].as_str())
                        .and_then(|v| v.trim().parse::<u64>().ok())
                })
                .or_else(|| {
                    record["id"]
                        .as_str()
                        .and_then(|id| id.strip_prefix("T-"))
                        .and_then(|s| s.parse::<u64>().ok())
                });
            if let Some(at) = finished {
                newest = Some(newest.map_or(at, |cur: u64| cur.max(at)));
            }
        }
    }
    newest
}

/// 某条目(R-xxx/D-xxx)名下仍未收尾的 running 测试记录。
///
/// 判据是标题里是否出现该 id:测试记录本身没有结构化的 refs 字段,而实践中标题一律
/// 以条目号开头("R-153 批6 …")。宁可用这个朴素判据,也好过关闭时对未收尾的验证一无所知。
pub fn unclosed_running_for(root: &Path, entry_id: &str) -> Vec<(String, String)> {
    read_test_records(&root.join(TEST_RUNS_REL))
        .into_iter()
        .filter_map(|(_, record)| {
            if record["status"].as_str() != Some("running") {
                return None;
            }
            let title = record["title"].as_str().unwrap_or_default();
            title.contains(entry_id).then(|| {
                (
                    record["id"].as_str().unwrap_or_default().to_string(),
                    title.to_string(),
                )
            })
        })
        .collect()
}

/// R-130:按条目(R-xxx/D-xxx)反查关联的测试记录(active + archived)。
///
/// 判据**优先结构化 refs**(「关联」字段),标题命中作为兜底——旧记录没有 refs 时
/// 靠标题里出现的条目号照样能查到,保证初始化前后的查询口径一致。
pub fn records_for_entry(root: &Path, entry_id: &str) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for rel in [TEST_RUNS_REL, TEST_RUNS_ARCHIVE_REL] {
        for (_, mut record) in read_test_records(&root.join(rel)) {
            let refs_hit = record["refs"]
                .as_array()
                .map(|refs| refs.iter().any(|r| r.as_str() == Some(entry_id)))
                .unwrap_or(false);
            let title_hit = record["title"]
                .as_str()
                .map(|title| title.contains(entry_id))
                .unwrap_or(false);
            if refs_hit || title_hit {
                record["archived"] = json!(rel == TEST_RUNS_ARCHIVE_REL);
                out.push(record);
            }
        }
    }
    out
}

/// R-130:批量初始化/回填测试→条目映射。
///
/// 旧测试记录没有「关联」字段,查询只能靠标题命中。这里扫描 tests.md 全部记录,
/// 从标题里提取 `R-xxx` / `D-xxx` 条目号,补写「关联」字段,一次落盘。
/// 返回回填了多少条,方便调用方反馈(0 表示已全部结构化,无旧记录)。
pub fn initialize_refs(root: &Path) -> Result<serde_json::Value, String> {
    let path = root.join(TEST_RUNS_REL);
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut backfilled = 0usize;
    let updated = {
        let mut parts = text.split("\n## ").collect::<Vec<_>>();
        // 第一部分是文件头("# Test Runs"),不拆。
        let head = parts.remove(0);
        let mut blocks = Vec::with_capacity(parts.len() + 1);
        blocks.push(head.to_string());
        for raw in parts {
            let block = format!("## {raw}");
            // 已有「关联」字段就不动(结构化在前,标题回填只补缺失)。
            let has_refs = block.lines().any(|line| {
                line.trim()
                    .strip_prefix("- ")
                    .and_then(|l| l.split_once(':'))
                    .map(|(key, _)| key.trim() == "关联")
                    .unwrap_or(false)
            });
            if has_refs {
                blocks.push(block);
                continue;
            }
            // 从标题行提取条目号:## T-xxx 标题 [status]
            let header = block.lines().next().unwrap_or_default();
            let title = header.trim_start_matches("## ").trim();
            let title_only = title
                .split('[')
                .next()
                .unwrap_or(title)
                .trim()
                .trim_start_matches(|c: char| !c.is_ascii_digit() && c != ' ')
                .trim();
            let ids = extract_entry_ids(title_only);
            if ids.is_empty() {
                blocks.push(block);
                continue;
            }
            // 插到「收尾」行之前,保持字段块连贯。
            let mut lines = block.lines().collect::<Vec<_>>();
            let insert_at = lines
                .iter()
                .position(|line| line.trim_start().starts_with("- 收尾:"))
                .unwrap_or(lines.len());
            let ref_line = format!("- 关联: {}", ids.join(" "));
            lines.insert(insert_at, &ref_line);
            blocks.push(lines.join("\n"));
            backfilled += 1;
        }
        blocks.join("\n## ")
    };
    if updated == text {
        // 幂等:没有任何需要回填的记录时不动文件,避免每次打开测试页都触发一次写盘。
        return Ok(json!({ "backfilled": 0 }));
    }
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    Ok(json!({ "backfilled": backfilled }))
}

/// 从字符串里提取 `R-xxx` / `D-xxx` 条目号(去重保序)。
fn extract_entry_ids(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for tok in text.split_whitespace() {
        let tok = tok.trim_matches(|c: char| {
            !(c.is_ascii_alphanumeric() || c == '-' || c == '#' || c == ':')
        });
        if is_entry_id(tok) && seen.insert(tok.to_string()) {
            out.push(tok.to_string());
        }
    }
    out
}

/// 是否形如 `R-153` / `D-201`(条目号,字母 + 至少 2 位数字)。
fn is_entry_id(part: &str) -> bool {
    (part.starts_with("R-") || part.starts_with("D-"))
        && part.len() > 3
        && part[2..].chars().all(|c| c.is_ascii_digit())
}

/// 快照渲染成工具可读文本。
fn render_snapshot(snapshot: &serde_json::Value) -> String {
    let active = snapshot["active"].as_array().map(Vec::len).unwrap_or(0);
    let archived = snapshot["archived"].as_array().map(Vec::len).unwrap_or(0);
    let mut lines = vec![format!(
        "recorded. active: {active}, archived: {archived} (path: {})",
        snapshot["path"].as_str().unwrap_or_default()
    )];
    let mut stale = 0;
    for record in snapshot["active"].as_array().into_iter().flatten() {
        let is_stale = record["stale"].as_bool().unwrap_or(false);
        if is_stale {
            stale += 1;
        }
        lines.push(format!(
            "● {} {} [{}]{}",
            record["id"].as_str().unwrap_or_default(),
            record["title"].as_str().unwrap_or_default(),
            record["status"].as_str().unwrap_or_default(),
            if is_stale {
                format!(
                    " ⚠ 悬空 {} 分钟未收尾",
                    record["age_secs"].as_u64().unwrap_or(0) / 60
                )
            } else {
                String::new()
            },
        ));
        let refs = record["refs"].as_array().map(|r| {
            r.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        });
        if let Some(refs) = refs.filter(|r| !r.is_empty()) {
            lines.push(format!("   关联: {refs}"));
        }
    }
    if stale > 0 {
        lines.push(format!(
            "⚠ {stale} 条 running 记录已悬空:跑完请用 test_record 带上该条 id 收尾(status=passed/failed/skipped),\
             同标题的终态记录会自动认领。悬空记录会挡住相关条目的 close。"
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
        // R-141 后 ToolCtx::new 不再发现取根,但 DocStore/托管路径仍以 .kanzei
        // 为准;保留标记,让 fixture 与可能位于某个 checkout 之下的 CI 临时目录隔离。
        std::fs::create_dir(dir.join(".kanzei")).unwrap();
        dir
    }

    #[test]
    fn 终态就地收尾同名running而不是再追加一条() {
        let root = temp_project("claim");
        record_test_run(
            &root,
            None,
            "R-999 定向回归",
            "running",
            Some("cargo test"),
            None,
            None,
        )
        .unwrap();
        let snapshot = record_test_run(
            &root,
            None,
            "R-999 定向回归",
            "passed",
            None,
            Some("全绿"),
            None,
        )
        .unwrap();
        assert_eq!(
            snapshot["active"].as_array().unwrap().len(),
            0,
            "收尾后不该再有 running 挂着:{snapshot:#?}"
        );
        let archived = snapshot["archived"].as_array().unwrap();
        assert_eq!(
            archived.len(),
            1,
            "只应归档一条,而不是 running/passed 各一条"
        );
        assert_eq!(archived[0]["status"], "passed");
        let fields = archived[0]["fields"].as_array().unwrap();
        assert!(
            fields
                .iter()
                .any(|f| f["key"] == "命令" && f["value"] == "cargo test"),
            "收尾时没给命令就该沿用原记录的,不能覆盖成空:{fields:#?}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn 悬空的running记录会被标记并能按条目号查出来() {
        let root = temp_project("stale");
        // 直接写一条 20 天前的 running 记录:id 里的时间戳就是判据。
        let old_id = now_secs() - 20 * 86_400;
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!("# Test Runs\n\n## T-{old_id} R-153 批6 回归 [running]\n- 命令: cargo test\n"),
        )
        .unwrap();
        let snapshot = test_runs_snapshot(&root).unwrap();
        let active = &snapshot["active"].as_array().unwrap()[0];
        assert_eq!(
            active["stale"], true,
            "20 天前的 running 必须判悬空:{active:#?}"
        );
        assert_eq!(unclosed_running_for(&root, "R-153").len(), 1);
        assert_eq!(
            unclosed_running_for(&root, "R-158").len(),
            0,
            "不该误伤别的条目"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn last_passed_at_取收尾时刻而不是开始时刻() {
        let root = temp_project("passedat");
        // 先起 running(id 时刻在很久以前),再收尾:门禁要认收尾那一刻。
        let started = now_secs() - 3600;
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            format!("# Test Runs\n\n## T-{started} R-1 回归 [running]\n- 命令: cargo test\n"),
        )
        .unwrap();
        assert_eq!(last_passed_at(&root), None, "只有 running 时不该有背书");
        record_test_run(&root, None, "R-1 回归", "passed", None, Some("全绿"), None).unwrap();
        let at = last_passed_at(&root).expect("收尾后必须有时间戳");
        assert!(
            at >= started + 3000,
            "取的必须是收尾时刻({at})而不是开始时刻({started})——否则先起 running 再改代码就能骗过提交门禁"
        );
        std::fs::remove_dir_all(root).ok();
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
    fn parse_blocks_extracts_refs_from关联字段() {
        let text = "# Test Runs\n\n## T-3 R-153 批6 回归 [passed]\n- 命令: cargo test\n- 关联: D-201 R-153\n";
        let blocks = parse_test_blocks(text);
        assert_eq!(blocks.len(), 1);
        let refs = blocks[0].1["refs"].as_array().unwrap();
        let ids = refs.iter().filter_map(|r| r.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, vec!["D-201", "R-153"], "关联字段应解析为结构化 refs");
        // 非条目 token(命令、路径)不该混进 refs。
        let text2 = "# Test Runs\n\n## T-4 x [passed]\n- 关联: cargo D-x R-1 R-153\n";
        let blocks2 = parse_test_blocks(text2);
        let ids2 = blocks2[0].1["refs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids2, vec!["R-153"], "只认 R-/D- 开头的条目号:{ids2:?}");
    }

    #[test]
    fn records_for_entry_prefers_refs_and_falls_back_to_title() {
        let root = temp_project("refsquery");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-1 D-201 回归 [passed]\n- 命令: cargo test\n",
        )
        .unwrap();
        // 标题命中(旧记录无 refs 字段)。
        let by_title = records_for_entry(&root, "D-201");
        assert_eq!(by_title.len(), 1, "标题兜底查询应命中 D-201:{by_title:#?}");
        assert_eq!(by_title[0]["id"], json!("T-1"));
        // 无关条目不误伤。
        assert_eq!(records_for_entry(&root, "R-999").len(), 0);
        // 结构化 refs 查询:显式关联命中。
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-2 冒烟 [passed]\n- 关联: D-999\n- 命令: x\n",
        )
        .unwrap();
        let by_refs = records_for_entry(&root, "D-999");
        assert_eq!(by_refs.len(), 1, "refs 字段应命中 D-999:{by_refs:#?}");
        assert_eq!(by_refs[0]["id"], json!("T-2"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn initialize_refs_backfills_entry_ids_from_title() {
        let root = temp_project("initrefs");
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(TEST_RUNS_REL),
            "# Test Runs\n\n## T-1 R-153 批6 回归 [passed]\n- 命令: cargo test\n- 收尾: 123\n\n## T-2 冒烟测试 [passed]\n- 命令: x\n",
        )
        .unwrap();
        let result = initialize_refs(&root).unwrap();
        assert_eq!(
            result["backfilled"],
            json!(1),
            "只有标题含条目号的记录该回填"
        );
        let text = std::fs::read_to_string(root.join(TEST_RUNS_REL)).unwrap();
        assert!(
            text.contains("- 关联: R-153"),
            "标题里的 R-153 未回填进关联字段:\n{text}"
        );
        assert!(
            text.contains("## T-2 冒烟测试"),
            "无关记录不得被改写:\n{text}"
        );
        assert!(
            text.contains("收尾: 123"),
            "关联字段应插在收尾行之前,不破坏原字段:\n{text}"
        );
        // 幂等:再跑一次不应重复回填。
        let second = initialize_refs(&root).unwrap();
        assert_eq!(second["backfilled"], json!(0), "已结构化的记录不应重复回填");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn append_with_refs_writes_关联_field() {
        let root = temp_project("appendrefs");
        let snapshot = append_test_run(
            &root,
            "D-201 回归",
            "passed",
            Some("cargo test"),
            None,
            Some(&["D-201".to_string(), "R-153".to_string()]),
        )
        .unwrap();
        let archived = snapshot["archived"].as_array().unwrap();
        assert_eq!(archived[0]["refs"].as_array().unwrap().len(), 2);
        let ids = archived[0]["refs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["D-201", "R-153"]);
        // 终态记录已被快照归档到 archive:关联字段应落在归档文件里。
        let archive_text = std::fs::read_to_string(root.join(TEST_RUNS_ARCHIVE_REL)).unwrap();
        assert!(
            archive_text.contains("- 关联: D-201 R-153"),
            "关联字段未写入归档:\n{archive_text}"
        );
        std::fs::remove_dir_all(&root).ok();
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
        let snapshot = append_test_run(
            &root,
            "cargo test",
            "passed",
            Some("cargo test"),
            Some("全绿"),
            None,
        )
        .unwrap();
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
        append_test_run(&root, "long run", "running", None, None, None).unwrap();
        let snapshot = test_runs_snapshot(&root).unwrap();
        assert_eq!(snapshot["active"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["active"][0]["status"], json!("running"));
        assert_eq!(snapshot["archived"].as_array().unwrap().len(), 0);
        // 终态后自动归档:running 那条仍留 active,passed 那条进 archive。
        append_test_run(&root, "long run", "passed", None, None, None).unwrap();
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
        let err = append_test_run(&root, "x", "bogus", None, None, None).unwrap_err();
        assert!(err.contains("passed"), "{err}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn tool_records_and_returns_snapshot_text() {
        let root = temp_project("tool");
        let ctx = ToolCtx::new(root.clone(), root.clone());
        let out = TestRecordTool
            .execute(
                json!({"title": "cargo test -p kanzei-llm", "status": "passed", "command": "cargo test -p kanzei-llm"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(
            out.content.contains("active: 0, archived: 1"),
            "{}",
            out.content
        );
        assert!(
            out.content.contains("cargo test -p kanzei-llm [passed]"),
            "{}",
            out.content
        );
        assert!(root.join(TEST_RUNS_REL).exists());
        std::fs::remove_dir_all(&root).ok();
    }
}
