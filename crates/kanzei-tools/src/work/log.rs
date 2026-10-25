//! 认领、局部交付与完成声明的追加式过程记录，不改变条目生命周期。

use std::path::Path;

use serde::Serialize;

/// 过程事实(相对项目根)。
pub const WORK_LOG_REL: &str = ".kanzei/artifacts/work-log.jsonl";

const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct WorkLogRecord<'a> {
    schema_version: u32,
    /// `claim` | `handoff` | `deliver`
    event: &'a str,
    at_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<&'a str>,
    #[serde(flatten)]
    detail: serde_json::Value,
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

/// 追加并持久化过程事实。调用方必须处理错误，不能把写入失败回报为已登记。
pub(crate) fn append(
    project_root: &Path,
    event: &str,
    id: Option<&str>,
    ctx: &kanzei_harness::ToolCtx,
    detail: serde_json::Value,
) -> anyhow::Result<()> {
    let path = project_root.join(WORK_LOG_REL);
    let line_identity = crate::work::line_identity(&ctx.cwd, project_root);
    let record = WorkLogRecord {
        schema_version: SCHEMA_VERSION,
        event,
        at_ms: now_millis(),
        id,
        run_id: ctx.run_id.as_deref(),
        line: line_identity.as_deref(),
        detail,
    };
    let mut line = serde_json::to_string(&record)?;
    line.push('\n');
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _guard = kanzei_base::atomic_file::lock_exclusive(&path)?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    file.sync_data()?;
    Ok(())
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DeliverFact {
    pub commit: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub test_record_ids: Vec<String>,
}

/// 读取指定条目的交付账本行；坏行/其他 event 忽略，账本是追加式过程事实。
pub(crate) fn deliver_facts(project_root: &Path, entry_id: &str) -> Vec<DeliverFact> {
    let Ok(text) = std::fs::read_to_string(project_root.join(WORK_LOG_REL)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            (value.get("event").and_then(serde_json::Value::as_str) == Some("deliver"))
                .then_some(())?;
            (value.get("id").and_then(serde_json::Value::as_str) == Some(entry_id)).then_some(())?;
            serde_json::from_value(value).ok()
        })
        .collect()
}

/// 返回当前运行/线路最近一次成功 claim 的条目 id，供 finalize 归属 deliver。
/// 没有同一 run/line 的 claim 时返回 None，避免把无主提交误归给别线条目。
pub(crate) fn latest_claim_id(
    project_root: &Path,
    ctx: &kanzei_harness::ToolCtx,
) -> Option<String> {
    let text = std::fs::read_to_string(project_root.join(WORK_LOG_REL)).ok()?;
    let current_line = super::line_identity(&ctx.cwd, project_root);
    let current_run = ctx.run_id.as_deref();
    text.lines().rev().find_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        if value.get("event").and_then(serde_json::Value::as_str) != Some("claim") {
            return None;
        }
        let same_run = current_run.is_some()
            && value.get("run_id").and_then(serde_json::Value::as_str) == current_run;
        let same_line = current_line.as_deref().is_some()
            && value.get("line").and_then(serde_json::Value::as_str) == current_line.as_deref();
        if !(same_run || same_line) {
            return None;
        }
        value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deliver账本按条目读取改动面和测试记录() {
        let dir = std::env::temp_dir().join(format!(
            "kz-deliver-facts-{}-{}",
            std::process::id(),
            now_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = kanzei_harness::ToolCtx::new(dir.clone(), dir.clone());
        append(
            &dir,
            "deliver",
            Some("R-353"),
            &ctx,
            serde_json::json!({
                "commit": "abcdef1234567890",
                "paths": ["crates/example/src/lib.rs"],
                "test_record_ids": ["T-001"],
                "source": "engine"
            }),
        )
        .unwrap();
        append(
            &dir,
            "deliver",
            Some("D-other"),
            &ctx,
            serde_json::json!({
                "commit": "1111111111111111",
                "paths": ["crates/other/src/lib.rs"]
            }),
        )
        .unwrap();
        let facts = deliver_facts(&dir, "R-353");
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].commit, "abcdef1234567890");
        assert_eq!(facts[0].paths, ["crates/example/src/lib.rs"]);
        assert_eq!(facts[0].test_record_ids, ["T-001"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 追加两条并各自成行() {
        let dir = std::env::temp_dir().join(format!(
            "kz-work-log-{}-{}",
            std::process::id(),
            now_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = kanzei_harness::ToolCtx::new(dir.clone(), dir.clone());

        append(
            &dir,
            "claim",
            Some("R-001"),
            &ctx,
            serde_json::json!({"reason": "队首", "unblocks": 2}),
        )
        .unwrap();
        append(
            &dir,
            "handoff",
            None,
            &ctx,
            serde_json::json!({"criterion": "验收①达成", "evidence_refs": ["a.rs:1"]}),
        )
        .unwrap();

        let text = std::fs::read_to_string(dir.join(WORK_LOG_REL)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["event"], "claim");
        assert_eq!(first["id"], "R-001");
        assert_eq!(first["reason"], "队首");
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["event"], "handoff");
        assert_eq!(second["evidence_refs"][0], "a.rs:1");
        assert!(second.get("id").is_none(), "无 id 时不写空字段");
        std::fs::remove_dir_all(dir).ok();
    }
}
