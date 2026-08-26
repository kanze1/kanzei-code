//! 取活/停机的**过程事实**落点(append-only JSONL)。
//!
//! 与 `incident` 同形状、同理由:这些是引擎每轮重新产生的机制产物,不是条目自身的
//! 状态。写进 tracker 文档是走错门——`取活依据` 就是这么被腌进每个条目的:
//! `structured_entry` 把条目的**全部** fields 序列化进控制状态,于是这行必然过期的
//! 快照跟着条目进每一次 `work next`、每一次文档快照,而调度器本来就每轮重算一遍。
//! 只写不读,却永久占上下文。
//!
//! handoff 的完成条件与证据引用是另一半:工具**硬拦**缺失(拦两次),逼模型写出来,
//! 然后只回显、不落任何地方——停机理由恰恰是最该留痕的东西。它同样不属于条目
//! 状态(条目可能压根没关),属于这一轮运行的过程事实。
//!
//! 判据一句话:**跨轮要引用、人要在条目里读到的 → 文档;引擎每轮重算或只描述
//! 这一轮发生了什么的 → 这里。**

use std::path::Path;

use serde::Serialize;

/// 过程事实(相对项目根)。
pub const WORK_LOG_REL: &str = ".kanzei/artifacts/work-log.jsonl";

const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct WorkLogRecord<'a> {
    schema_version: u32,
    /// `claim` | `handoff`
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

/// 追加一条过程事实。**失败不影响调用方**:这是留痕,不是事务的一部分——
/// 让写日志失败去否决一次成功的取活,是拿观测性换可用性。
pub(crate) fn append(
    project_root: &Path,
    event: &str,
    id: Option<&str>,
    ctx: &kanzei_harness::ToolCtx,
    detail: serde_json::Value,
) {
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
    let Ok(mut line) = serde_json::to_string(&record) else {
        return;
    };
    line.push('\n');
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let Ok(_guard) = kanzei_base::atomic_file::lock_exclusive(&path) else {
        return;
    };
    use std::io::Write;
    let opened = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path);
    if let Ok(mut file) = opened {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        );
        append(
            &dir,
            "handoff",
            None,
            &ctx,
            serde_json::json!({"criterion": "验收①达成", "evidence_refs": ["a.rs:1"]}),
        );

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
