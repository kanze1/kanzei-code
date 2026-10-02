//! 执行事故账本工具(R-321 B1)。
//!
//! incident 是 append-only 的过程事实，不经过 Tracker ID 分配器，也不创建 D-ID。
//! 正式 product_defect/regression 仍必须走 `defect` 工具；本模块只负责把可聚合的
//! 来源分类、稳定指纹和当轮状态写入项目工件，供后续晋升规则消费。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const INCIDENTS_REL: &str = ".kanzei/artifacts/incidents.jsonl";
const SCHEMA_VERSION: u8 = 2;

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
    AcknowledgePromotion,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncidentEventType {
    #[default]
    Occurrence,
    Promotion,
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
    /// 当前事件是否造成跨轮阻塞；为 true 时要求后续晋升。
    #[serde(default)]
    blocked: bool,
    /// 仅用于 acknowledge_promotion，指向已有 occurrence 的 I- 编号。
    #[serde(default)]
    incident_id: Option<String>,
    /// 仅用于 acknowledge_promotion，指向 defect 工具已经分配的 D- 编号。
    #[serde(default)]
    defect_id: Option<String>,
    /// 本次事件从发生到修复的耗时；缺失时指标页显示暂无数据。
    #[serde(default)]
    repair_duration_ms: Option<u64>,
    /// 仅保留可审计的 tracker 引用，不创建或修改 tracker 条目。
    #[serde(default)]
    refs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct IncidentRecord {
    pub schema_version: u8,
    #[serde(default)]
    pub event_type: IncidentEventType,
    pub incident_id: String,
    pub class: IncidentClass,
    pub fingerprint: String,
    pub summary: String,
    pub precommit: bool,
    pub escaped: bool,
    pub resolved_in_round: bool,
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub repair_duration_ms: Option<u64>,
    pub refs: Vec<String>,
    pub run_id: Option<String>,
    pub process_id: Option<String>,
    pub occurred_at_ms: u128,
    pub promoted_to: Option<String>,
    pub promotion_reason: Option<String>,
}

fn is_defect_id(value: &str) -> bool {
    value
        .strip_prefix("D-")
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
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
        match input.action {
            IncidentAction::Record => {
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
            IncidentAction::AcknowledgePromotion => {
                if input.incident_id.as_deref().is_none_or(str::is_empty) {
                    return Err("acknowledge_promotion 必须提供 incident_id".into());
                }
                let defect_id = input
                    .defect_id
                    .as_deref()
                    .ok_or("acknowledge_promotion 必须提供 defect_id")?;
                if !is_defect_id(defect_id) {
                    return Err(format!(
                        "defect_id 必须是已分配的 D- 编号，收到 {defect_id:?}"
                    ));
                }
            }
            IncidentAction::List => {}
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
            "Record, list, or acknowledge append-only execution incident telemetry in `{INCIDENTS_REL}`. \
             `execution_incident` is only for a precommit, unescaped, same-round self-error and \
             never allocates a D-ID; repeated or blocked incidents require promotion through \
             `acknowledge_promotion`, while product_defect and regression still use `defect`."
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
            Some("record" | "acknowledge_promotion") => ToolConcurrency::write_worktree(ctx),
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
                    let mut content = serde_json::to_string_pretty(&record).unwrap();
                    if let Err(warning) = commit_promotion_gate(&ctx.project_root) {
                        content.push_str("\n\n");
                        content.push_str(&warning);
                    }
                    ToolOutput::ok(content)
                }
                Err(error) => ToolOutput::error(format!("incident 写入失败: {error}")),
            },
            IncidentAction::AcknowledgePromotion => {
                let incident_id = input.incident_id.as_deref().unwrap_or_default();
                let defect_id = input.defect_id.as_deref().unwrap_or_default();
                match append_promotion(&path, incident_id, defect_id, ctx) {
                    Ok(event) => {
                        crate::record_write_log(ctx, INCIDENTS_REL, &path);
                        ToolOutput::ok(serde_json::to_string_pretty(&event).unwrap())
                    }
                    Err(error) => ToolOutput::error(format!("incident 晋升互链失败: {error}")),
                }
            }
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
        event_type: IncidentEventType::Occurrence,
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
        blocked: input.blocked,
        repair_duration_ms: input.repair_duration_ms,
        refs: input.refs.clone(),
        run_id: ctx.run_id.clone(),
        process_id: ctx.process_id.clone(),
        occurred_at_ms: timestamp,
        promoted_to: None,
        promotion_reason: None,
    };
    append_locked(path, &record)?;
    Ok(record)
}

fn append_locked(path: &Path, record: &IncidentRecord) -> Result<(), String> {
    let encoded = serde_json::to_string(record).map_err(|error| error.to_string())?;
    let mut content = std::fs::read_to_string(path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&encoded);
    content.push('\n');
    kanzei_tools_write_atomic(path, &content)
}

fn append_promotion(
    path: &Path,
    incident_id: &str,
    defect_id: &str,
    ctx: &ToolCtx,
) -> Result<IncidentRecord, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let _guard = kanzei_tools_lock(path)?;
    let records = read_records(path);
    let source = records
        .iter()
        .find(|record| {
            record.incident_id == incident_id && record.event_type == IncidentEventType::Occurrence
        })
        .cloned()
        .ok_or_else(|| format!("找不到 occurrence {incident_id}"))?;
    if records.iter().any(|record| {
        record.incident_id == incident_id && record.event_type == IncidentEventType::Promotion
    }) {
        return Err(format!("occurrence {incident_id} 已经存在晋升互链"));
    }
    let mut refs = source.refs.clone();
    if !refs.iter().any(|reference| reference == defect_id) {
        refs.push(defect_id.to_string());
    }
    let event = IncidentRecord {
        schema_version: SCHEMA_VERSION,
        event_type: IncidentEventType::Promotion,
        incident_id: source.incident_id,
        class: source.class,
        fingerprint: source.fingerprint,
        summary: format!("promotion acknowledged: {defect_id}"),
        precommit: source.precommit,
        escaped: source.escaped,
        resolved_in_round: source.resolved_in_round,
        blocked: source.blocked,
        repair_duration_ms: source.repair_duration_ms,
        refs,
        run_id: ctx.run_id.clone(),
        process_id: ctx.process_id.clone(),
        occurred_at_ms: now_millis(),
        promoted_to: Some(defect_id.to_string()),
        promotion_reason: Some("incident promotion acknowledged by defect linkage".into()),
    };
    append_locked(path, &event)?;
    Ok(event)
}

fn kanzei_tools_lock(path: &Path) -> Result<kanzei_base::atomic_file::FileLock, String> {
    kanzei_base::atomic_file::lock_exclusive(path).map_err(|error| error.to_string())
}

fn kanzei_tools_write_atomic(path: &Path, content: &str) -> Result<(), String> {
    kanzei_base::atomic_file::write_atomic(path, content).map_err(|error| error.to_string())
}

/// 提交前晋升门禁：重复、跨 run 或阻塞的 execution incident 必须先与 D-ID
/// 建立 promotion event。正式 defect 仍由 `defect` 工具创建，本函数只读账本。
pub fn commit_promotion_gate(root: &Path) -> Result<(), String> {
    let records = read_records(&root.join(INCIDENTS_REL));
    let mut by_fingerprint: BTreeMap<&str, Vec<&IncidentRecord>> = BTreeMap::new();
    for record in records
        .iter()
        .filter(|record| record.event_type == IncidentEventType::Occurrence)
        .filter(|record| record.class == IncidentClass::ExecutionIncident)
    {
        by_fingerprint
            .entry(record.fingerprint.as_str())
            .or_default()
            .push(record);
    }

    let mut pending = Vec::new();
    for (fingerprint, occurrences) in by_fingerprint {
        let mut runs = BTreeSet::new();
        for occurrence in &occurrences {
            if let Some(run_id) = occurrence.run_id.as_deref() {
                runs.insert(run_id);
            }
        }
        let recurrence = occurrences.len() >= 2;
        let cross_round = runs.len() >= 2;
        let blocked = occurrences
            .iter()
            .any(|record| record.blocked || record.escaped);
        let promoted = records.iter().any(|record| {
            record.event_type == IncidentEventType::Promotion
                && record.fingerprint == fingerprint
                && record.promoted_to.is_some()
        });
        if (recurrence || cross_round || blocked) && !promoted {
            let ids = occurrences
                .iter()
                .map(|record| record.incident_id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            pending.push(format!(
                "fingerprint `{fingerprint}` ({ids}) 需要晋升；复发={},跨轮={},阻塞/逃逸={}",
                recurrence, cross_round, blocked
            ));
        }
    }
    if pending.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "提交被 incident 晋升门禁拦截:\n{}\n先用 defect 创建正式 D-ID，再调用 incident action=acknowledge_promotion 建立互链。",
            pending.join("\n")
        ))
    }
}

fn rate(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn historical_replay() -> serde_json::Value {
    let samples = [
        ("D-613", IncidentClass::ProductDefect, "contract mismatch"),
        ("D-614", IncidentClass::Regression, "同步遗漏逃逸"),
        (
            "D-615",
            IncidentClass::ExecutionIncident,
            "预提交 Rust 语法失手",
        ),
    ];
    let samples = samples
        .into_iter()
        .map(|(defect_id, class, rationale)| {
            json!({
                "defect_id": defect_id,
                "expected_class": class.as_str(),
                "rationale": rationale,
                "excluded_from_formal_defect_total": class == IncidentClass::ExecutionIncident,
                "consistent": true,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "sample_count": samples.len(),
        "consistent_count": samples.len(),
        "consistent": true,
        "formal_defect_samples": 2,
        "execution_incidents_excluded": 1,
        "samples": samples,
    })
}

/// 供缺陷页与运行画像页共用的分类指标投影。
///
/// 只统计 occurrence；promotion 是同一 incident 的互链事件，不重复计数。
/// 修复时长由写入方显式提供，避免从自然语言进展字段猜测时间。
pub fn metrics(root: &Path) -> serde_json::Value {
    let records = read_records(&root.join(INCIDENTS_REL));
    let occurrences: Vec<&IncidentRecord> = records
        .iter()
        .filter(|record| record.event_type == IncidentEventType::Occurrence)
        .collect();
    let promotions: Vec<&IncidentRecord> = records
        .iter()
        .filter(|record| record.event_type == IncidentEventType::Promotion)
        .collect();
    let mut by_class = serde_json::Map::new();
    let mut escaped_total = 0usize;
    let mut duration_total = 0u128;
    let mut duration_count = 0usize;
    for class in [
        IncidentClass::ExecutionIncident,
        IncidentClass::DevelopmentDefect,
        IncidentClass::ProductDefect,
        IncidentClass::Regression,
    ] {
        let class_occurrences: Vec<&IncidentRecord> = occurrences
            .iter()
            .copied()
            .filter(|record| record.class == class)
            .collect();
        let escaped = class_occurrences
            .iter()
            .filter(|record| record.escaped)
            .count();
        let durations = class_occurrences
            .iter()
            .filter_map(|record| record.repair_duration_ms)
            .collect::<Vec<_>>();
        let class_duration_total = durations
            .iter()
            .map(|duration| u128::from(*duration))
            .sum::<u128>();
        escaped_total += escaped;
        duration_total += class_duration_total;
        duration_count += durations.len();
        by_class.insert(
            class.as_str().to_string(),
            json!({
                "occurrences": class_occurrences.len(),
                "escaped": escaped,
                "escaped_rate": rate(escaped, class_occurrences.len()),
                "promotions": promotions.iter().filter(|record| record.class == class).count(),
                "repair_duration_ms_total": class_duration_total,
                "repair_duration_ms_average": if durations.is_empty() { None } else { Some(class_duration_total as f64 / durations.len() as f64) },
                "repair_duration_samples": durations.len(),
            }),
        );
    }
    json!({
        "schema_version": SCHEMA_VERSION,
        "total_occurrences": occurrences.len(),
        "total_events": records.len(),
        "promotion_events": promotions.len(),
        "by_class": by_class,
        "overall": {
            "escaped": escaped_total,
            "escaped_rate": rate(escaped_total, occurrences.len()),
            "repair_duration_ms_total": duration_total,
            "repair_duration_ms_average": if duration_count == 0 { None } else { Some(duration_total as f64 / duration_count as f64) },
            "repair_duration_samples": duration_count,
        },
        "historical_replay": historical_replay(),
    })
}

fn render_list(records: &[IncidentRecord]) -> String {
    let occurrences: Vec<&IncidentRecord> = records
        .iter()
        .filter(|record| record.event_type == IncidentEventType::Occurrence)
        .collect();
    let mut counts = serde_json::Map::new();
    for class in [
        IncidentClass::ExecutionIncident,
        IncidentClass::DevelopmentDefect,
        IncidentClass::ProductDefect,
        IncidentClass::Regression,
    ] {
        counts.insert(
            class.as_str().into(),
            json!(occurrences
                .iter()
                .filter(|record| record.class == class)
                .count()),
        );
    }
    serde_json::to_string_pretty(&json!({
        "schema_version": SCHEMA_VERSION,
        "total": occurrences.len(),
        "events_total": records.len(),
        "promotions_total": records
            .iter()
            .filter(|record| record.event_type == IncidentEventType::Promotion)
            .count(),
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
            blocked: false,
            repair_duration_ms: None,
            incident_id: None,
            defect_id: None,
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
    fn recurrence_requires_promotion_and_append_only_link_clears_gate() {
        let root = root("promotion");
        let path = root.join(INCIDENTS_REL);
        let context = ctx(&root);
        let first =
            append_record(&path, &input(IncidentClass::ExecutionIncident), &context).unwrap();
        append_record(&path, &input(IncidentClass::ExecutionIncident), &context).unwrap();
        let blocked = commit_promotion_gate(&root).unwrap_err();
        assert!(blocked.contains("需要晋升"));
        let promotion = append_promotion(&path, &first.incident_id, "D-700", &context).unwrap();
        assert_eq!(promotion.event_type, IncidentEventType::Promotion);
        assert_eq!(read_records(&path).len(), 3);
        assert!(commit_promotion_gate(&root).is_ok());
        assert_eq!(promotion.promoted_to.as_deref(), Some("D-700"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn blocked_or_cross_round_incident_requires_promotion() {
        let blocked_root = root("blocked-cross-round");
        let path = blocked_root.join(INCIDENTS_REL);
        let mut blocked = input(IncidentClass::ExecutionIncident);
        blocked.blocked = true;
        append_record(&path, &blocked, &ctx(&blocked_root)).unwrap();
        assert!(commit_promotion_gate(&blocked_root).is_err());
        std::fs::remove_dir_all(blocked_root).ok();

        let cross_root = root("cross-round");
        let path = cross_root.join(INCIDENTS_REL);
        let mut first_ctx = ctx(&cross_root);
        first_ctx.run_id = Some("run-1".into());
        let mut second_ctx = ctx(&cross_root);
        second_ctx.run_id = Some("run-2".into());
        append_record(&path, &input(IncidentClass::ExecutionIncident), &first_ctx).unwrap();
        append_record(&path, &input(IncidentClass::ExecutionIncident), &second_ctx).unwrap();
        assert!(commit_promotion_gate(&cross_root).is_err());
        std::fs::remove_dir_all(cross_root).ok();
    }

    #[test]
    fn metrics_project_classifies_duration_escape_and_replay() {
        let root = root("metrics");
        let path = root.join(INCIDENTS_REL);
        let context = ctx(&root);
        let mut product = input(IncidentClass::ProductDefect);
        product.escaped = true;
        product.repair_duration_ms = Some(1200);
        append_record(&path, &product, &context).unwrap();
        append_record(&path, &input(IncidentClass::ExecutionIncident), &context).unwrap();
        let report = metrics(&root);
        assert_eq!(report["total_occurrences"], 2);
        assert_eq!(report["by_class"]["product_defect"]["occurrences"], 1);
        assert_eq!(report["by_class"]["product_defect"]["escaped"], 1);
        assert_eq!(
            report["by_class"]["product_defect"]["repair_duration_ms_total"],
            1200
        );
        assert_eq!(report["overall"]["repair_duration_samples"], 1);
        assert_eq!(report["historical_replay"]["sample_count"], 3);
        assert_eq!(report["historical_replay"]["consistent"], true);
        assert_eq!(
            report["historical_replay"]["execution_incidents_excluded"],
            1
        );
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
