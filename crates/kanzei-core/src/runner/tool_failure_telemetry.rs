//! R-310 B1:工具导航失手遥测。
//!
//! 记录发生在统一 runner 出口，避免各工具分别维护统计口径。原始
//! `ToolOutput` 不被修改；每个 tool_call_id 在同一 run 的账本中最多出现一次。

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use kanzei_harness::{ToolCtx, ToolOutput};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureClass {
    MissingPath,
    OutOfRange,
    MissingParameter,
    EmptySearch,
    PermissionDenied,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolFailureEvent {
    tool_call_id: String,
    tool_name: String,
    class: FailureClass,
    code: Option<String>,
    outcome: String,
    at_ms: u128,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RunTelemetry {
    schema_version: u8,
    run_id: String,
    #[serde(default)]
    calls: u64,
    #[serde(default)]
    call_ids: Vec<String>,
    events: Vec<ToolFailureEvent>,
}

fn write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn telemetry_path(ctx: &ToolCtx, run_id: &str) -> PathBuf {
    ctx.project_root
        .join(".kanzei")
        .join("artifacts")
        .join("tool-failures")
        .join(format!("{}.json", safe_component(run_id)))
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn classify(tool_name: &str, output: &ToolOutput) -> Option<FailureClass> {
    let code = output.code.unwrap_or_default();
    if code == "USER_DECLINED"
        || output.content.starts_with("permission denied")
        || output.content.starts_with("permission request declined")
    {
        return Some(FailureClass::PermissionDenied);
    }
    if matches!(
        code,
        "READ_PATH_NOT_FOUND"
            | "SYMBOLS_PATH_NOT_FOUND"
            | "EDIT_FILE_UNAVAILABLE"
            | "INSERT_FILE_UNAVAILABLE"
    ) || output.content.starts_with("path not found:")
        || output.content.starts_with("directory not found:")
    {
        return Some(FailureClass::MissingPath);
    }
    if code == "READ_RANGE_OUT_OF_BOUNDS" {
        return Some(FailureClass::OutOfRange);
    }
    if code == "INVALID_TOOL_INPUT"
        || output.content.contains("缺少必填参数")
        || output.content.contains("required")
    {
        return Some(FailureClass::MissingParameter);
    }
    if (tool_name == "grep" || tool_name == "glob")
        && (output.content.starts_with("(no matches for ")
            || output.content.starts_with("(no files match "))
    {
        return Some(FailureClass::EmptySearch);
    }
    output.is_error.then_some(FailureClass::Other)
}

fn record_outcome(
    ctx: &ToolCtx,
    run_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    class: Option<FailureClass>,
    code: Option<&str>,
    outcome: &str,
) {
    let path = telemetry_path(ctx, run_id);
    let _guard = write_lock().lock().expect("tool telemetry lock poisoned");
    let mut telemetry = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<RunTelemetry>(&text) {
            Ok(value) => value,
            Err(_) => return,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => RunTelemetry {
            schema_version: SCHEMA_VERSION,
            run_id: run_id.to_string(),
            calls: 0,
            call_ids: Vec::new(),
            events: Vec::new(),
        },
        Err(_) => return,
    };
    if telemetry.call_ids.iter().any(|id| id == tool_call_id) {
        return;
    }
    telemetry.call_ids.push(tool_call_id.to_string());
    telemetry.calls += 1;
    if let Some(class) = class {
        telemetry.events.push(ToolFailureEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            class,
            code: code.map(str::to_owned),
            outcome: outcome.to_string(),
            at_ms: now_ms(),
        });
    }
    let Ok(encoded) = serde_json::to_string_pretty(&telemetry) else {
        return;
    };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let _ = kanzei_base::atomic_file::write_atomic(&path, &encoded);
}

pub(crate) fn record_tool_failure(
    ctx: &ToolCtx,
    tool_call_id: &str,
    tool_name: &str,
    output: &ToolOutput,
) {
    let Some(run_id) = ctx.run_id.as_deref() else {
        return;
    };
    record_outcome(
        ctx,
        run_id,
        tool_call_id,
        tool_name,
        classify(tool_name, output),
        output.code,
        output.outcome.as_str(),
    );
}

pub(crate) fn record_permission_denied(ctx: &ToolCtx, tool_call_id: &str, tool_name: &str) {
    let Some(run_id) = ctx.run_id.as_deref() else {
        return;
    };
    record_outcome(
        ctx,
        run_id,
        tool_call_id,
        tool_name,
        Some(FailureClass::PermissionDenied),
        Some("USER_DECLINED"),
        "failed",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(root: &std::path::Path) -> ToolCtx {
        ToolCtx::new(root.to_path_buf(), root.to_path_buf()).with_identity(
            "worktree".into(),
            "project".into(),
            "run-navigation-test".into(),
            "process".into(),
        )
    }

    #[test]
    fn five_navigation_failure_shapes_have_stable_classes() {
        assert_eq!(
            classify(
                "read",
                &ToolOutput::failed("READ_PATH_NOT_FOUND", "path not found")
            ),
            Some(FailureClass::MissingPath)
        );
        assert_eq!(
            classify(
                "read",
                &ToolOutput::needs_correction("READ_RANGE_OUT_OF_BOUNDS", "legal range")
            ),
            Some(FailureClass::OutOfRange)
        );
        assert_eq!(
            classify(
                "edit",
                &ToolOutput::needs_correction("INVALID_TOOL_INPUT", "缺少必填参数 `path`")
            ),
            Some(FailureClass::MissingParameter)
        );
        assert_eq!(
            classify("grep", &ToolOutput::ok("(no matches for `nothing`)")),
            Some(FailureClass::EmptySearch)
        );
        assert_eq!(
            classify(
                "bash",
                &ToolOutput::error("permission denied by guard `x`: denied")
            ),
            Some(FailureClass::PermissionDenied)
        );
    }

    #[test]
    fn run_telemetry_aggregates_and_deduplicates_by_call_id() {
        let root = std::env::temp_dir().join(format!("kz-r310-{}", now_ms()));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ctx(&root);
        let missing = ToolOutput::failed("READ_PATH_NOT_FOUND", "path not found");
        let empty = ToolOutput::ok("(no matches for `nothing`)");
        record_tool_failure(&ctx, "call-1", "read", &missing);
        record_tool_failure(&ctx, "call-1", "read", &missing);
        record_tool_failure(&ctx, "call-2", "grep", &empty);

        let success = ToolOutput::ok("ok");
        record_tool_failure(&ctx, "call-3", "read", &success);

        let path = telemetry_path(&ctx, "run-navigation-test");
        let telemetry: RunTelemetry =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(telemetry.schema_version, SCHEMA_VERSION);
        assert_eq!(telemetry.run_id, "run-navigation-test");
        assert_eq!(telemetry.calls, 3);
        assert_eq!(telemetry.events.len(), 2);
        assert_eq!(telemetry.events[0].class, FailureClass::MissingPath);
        assert_eq!(telemetry.events[1].class, FailureClass::EmptySearch);
        let _ = std::fs::remove_dir_all(root);
    }
}
