//! 执行事故账本工具(R-321 B1)。
//!
//! incident 是 append-only 的过程事实，不经过 Tracker ID 分配器，也不创建 D-ID。
//! 正式 product_defect/regression 仍必须走 `defect` 工具；本模块只负责把可聚合的
//! 来源分类、稳定指纹和当轮状态写入项目工件，供后续晋升规则消费。

use std::path::Path;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const INCIDENTS_REL: &str = ".kanzei/artifacts/incidents.jsonl";
const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncidentClass {
    ExecutionIncident,
    DevelopmentDefect,
    ProductDefect,
    Regression,
}

impl IncidentClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::ExecutionIncident => "execution_incident",
            Self::DevelopmentDefect => "development_defect",
            Self::ProductDefect => "product_defect",
            Self::Regression => "regression",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum IncidentAction {
    Record,
    List,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct IncidentInput {
    action: IncidentAction,
    #[serde(default)]
    class: Option<IncidentClass>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    /// 事故是否已经逃逸当前预提交工作树。
    #[serde(default)]
    escaped: bool,
    /// 事故是否在当前轮内修复；execution_incident 必须为 true。
    #[serde(default)]
    resolved_in_round: bool,
    /// 仅保留可审计的 tracker 引用，不创建或修改 tracker 条目。
    #[serde(default)]
    refs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct IncidentRecord {
    pub schema_version: u8,
    pub incident_id: String,
    pub class: IncidentClass,
    pub fingerprint: String,
    pub summary: String,
    pub precommit: bool,
    pub escaped: bool,
    pub resolved_in_round: bool,
    pub refs: Vec<String>,
    pub run_id: Option<String>,
    pub process_id: Option<String>,
    pub occurred_at_ms: u128,
}

pub struct IncidentTool;

impl IncidentTool {
    fn validate(input: &IncidentInput) -> Result<(), String> {
        if let Some(fingerprint) = &input.fingerprint {
            let trimmed = fingerprint.trim();
            if trimmed.is_empty() || trimmed.len() > 256 {
                return Err("fingerprint 必须为 1-256 个非空字符".into());
            }
        }
        if let Some(summary) = &input.summary {
            if summary.trim().is_empty() || summary.len() > 4_000 {
                return Err("summary 必须为 1-4000 个非空字符".into());
            }
        }
        for reference in &input.refs {
            if !matches!(reference.get(0..2), Some("R-") | Some("D-") | Some("T-")) {
                return Err(format!(
                    "refs 只允许 R-/D-/T- 追踪条目编号，收到 {reference:?}"
                ));
            }
        }
        if matches!(input.action, IncidentAction::Record) {
            let class = input.class.ok_or("record 必须提供 class")?;
            if input.fingerprint.is_none() {
                return Err("record 必须提供稳定 fingerprint".into());
            }
            if input.summary.is_none() {
                return Err("record 必须提供 summary".into());
            }
            if class == IncidentClass::ExecutionIncident
                && (input.escaped || !input.resolved_in_round)
            {
                return Err(
                    "execution_incident 只能记录预提交、未逃逸且当轮已修复的自致错误".into(),
                );
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Tool for IncidentTool {
    fn name(&self) -> &'static str {
        "incident"
    }

    fn description(&self) -> String {
        format!(
            "Record or list append-only execution incident telemetry in `{INCIDENTS_REL}`. \
             `execution_incident` is only for a precommit, unescaped, same-round self-error and \
             never allocates a D-ID; product_defect and regression must still use `defect`."
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(IncidentInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["action"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        match input["action"].as_str() {
            Some("record") => ToolConcurrency::write_worktree(ctx),
            _ => ToolConcurrency::shared_worktree(ctx),
        }
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: IncidentInput = match crate::parse_input(self, input) {
            Ok(value) => value,
            Err(output) => return output,
        };
        if let Err(error) = Self::validate(&input) {
            return ToolOutput::needs_correction("INVALID_INCIDENT", error);
        }
        let path = ctx.project_root.join(INCIDENTS_REL);
        match input.action {
            IncidentAction::Record => match append_record(&path, &input, ctx) {
                Ok(record) => {
                    crate::record_write_log(ctx, INCIDENTS_REL, &path);
                    ToolOutput::ok(serde_json::to_string_pretty(&record).unwrap())
                }
                Err(error) => ToolOutput::error(format!("incident 写入失败: {error}")),
            },
            IncidentAction::List => {
                let records = read_records(&path);
                let class_filter = input.class;
                let fingerprint_filter = input.fingerprint.as_deref().map(str::trim);
                let filtered: Vec<_> = records
                    .into_iter()
                    .filter(|record| class_filter.is_none_or(|class| record.class == class))
                    .filter(|record| {
                        fingerprint_filter
                            .is_none_or(|fingerprint| record.fingerprint == fingerprint)
                    })
                    .collect();
                ToolOutput::ok(render_list(&filtered))
            }
        }
    }
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn read_records(path: &Path) -> Vec<IncidentRecord> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn append_record(
    path: &Path,
    input: &IncidentInput,
    ctx: &ToolCtx,
) -> Result<IncidentRecord, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let _guard = kanzei_tools_lock(path)?;
    let existing = read_records(path);
    let timestamp = now_millis();
    let incident_id = format!("I-{timestamp}-{}", existing.len() + 1);
    let record = IncidentRecord {
        schema_version: SCHEMA_VERSION,
        incident_id,
        class: input.class.ok_or("record 必须提供 class")?,
        fingerprint: input
            .fingerprint
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string(),
        summary: input
            .summary
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string(),
        precommit: true,
        escaped: input.escaped,
        resolved_in_round: input.resolved_in_round,
        refs: input.refs.clone(),
        run_id: ctx.run_id.clone(),
        process_id: ctx.process_id.clone(),
        occurred_at_ms: timestamp,
    };
    let encoded = serde_json::to_string(&record).map_err(|error| error.to_string())?;
    let mut content = std::fs::read_to_string(path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&encoded);
    content.push('\n');
    kanzei_tools_write_atomic(path, &content)?;
    Ok(record)
}

fn kanzei_tools_lock(path: &Path) -> Result<kanzei_base::atomic_file::FileLock, String> {
    kanzei_base::atomic_file::lock_exclusive(path).map_err(|error| error.to_string())
}

fn kanzei_tools_write_atomic(path: &Path, content: &str) -> Result<(), String> {
    kanzei_base::atomic_file::write_atomic(path, content).map_err(|error| error.to_string())
}

fn render_list(records: &[IncidentRecord]) -> String {
    let mut counts = serde_json::Map::new();
    for class in [
        IncidentClass::ExecutionIncident,
        IncidentClass::DevelopmentDefect,
        IncidentClass::ProductDefect,
        IncidentClass::Regression,
    ] {
        counts.insert(
            class.as_str().into(),
            json!(records
                .iter()
                .filter(|record| record.class == class)
                .count()),
        );
    }
    serde_json::to_string_pretty(&json!({
        "schema_version": SCHEMA_VERSION,
        "total": records.len(),
        "by_class": counts,
        "records": records,
    }))
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-incident-{name}-{}-{}",
            std::process::id(),
            now_millis()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn ctx(root: &Path) -> ToolCtx {
        ToolCtx::new(root.to_path_buf(), root.to_path_buf())
    }

    fn input(class: IncidentClass) -> IncidentInput {
        IncidentInput {
            action: IncidentAction::Record,
            class: Some(class),
            fingerprint: Some("rust-syntax:tool_exec.rs:ToolArtifact".into()),
            summary: Some("临时语法失手".into()),
            escaped: false,
            resolved_in_round: true,
            refs: vec!["D-615".into()],
        }
    }

    #[test]
    fn execution_incident_append_only_without_defect_id() {
        let root = root("append");
        let path = root.join(INCIDENTS_REL);
        let context = ctx(&root);
        let record =
            append_record(&path, &input(IncidentClass::ExecutionIncident), &context).unwrap();
        assert!(record.incident_id.starts_with("I-"));
        assert!(!record.incident_id.starts_with("D-"));
        assert_eq!(read_records(&path).len(), 1);
        append_record(&path, &input(IncidentClass::ExecutionIncident), &context).unwrap();
        assert_eq!(read_records(&path).len(), 2);
        assert_ne!(
            read_records(&path)[0].incident_id,
            read_records(&path)[1].incident_id
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn classification_and_aggregation_are_machine_readable() {
        let root = root("aggregate");
        let path = root.join(INCIDENTS_REL);
        let context = ctx(&root);
        append_record(&path, &input(IncidentClass::ExecutionIncident), &context).unwrap();
        let mut product = input(IncidentClass::ProductDefect);
        product.fingerprint = Some("contract:mismatch".into());
        append_record(&path, &product, &context).unwrap();
        let report = render_list(&read_records(&path));
        assert!(report.contains("\"execution_incident\": 1"));
        assert!(report.contains("\"product_defect\": 1"));
        assert!(report.contains("contract:mismatch"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn execution_incident_rejects_escape_or_unresolved_state() {
        let mut event = input(IncidentClass::ExecutionIncident);
        event.escaped = true;
        assert!(IncidentTool::validate(&event).is_err());
        event.escaped = false;
        event.resolved_in_round = false;
        assert!(IncidentTool::validate(&event).is_err());
    }
}
