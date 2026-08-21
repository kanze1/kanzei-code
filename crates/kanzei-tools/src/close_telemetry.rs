//! 条目关闭收尾链遥测(R-311 B2)与滚动汇总(R-311 B3)。
//!
//! 关闭动作只记录事实，不改变既有拒绝条件；统计读取本模块的 JSONL、R-310
//! tool-failures 遥测与 test_record，不把缺环升级成新的操作门禁。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

pub const CLOSE_TELEMETRY_REL: &str = ".kanzei/artifacts/close-telemetry.jsonl";
const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloseEvidence {
    pub compile: bool,
    pub targeted_tests: bool,
    pub regression: bool,
    pub acceptance_reconciliation: bool,
    pub commit: bool,
}

impl CloseEvidence {
    fn missing(&self) -> Vec<String> {
        [
            ("编译", self.compile),
            ("定向测试", self.targeted_tests),
            ("回归", self.regression),
            ("验收对照", self.acceptance_reconciliation),
            ("提交", self.commit),
        ]
        .into_iter()
        .filter_map(|(name, present)| (!present).then_some(name.to_string()))
        .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloseTelemetry {
    pub schema_version: u8,
    pub entry_id: String,
    pub status: String,
    pub batch: Option<String>,
    pub occurred_at: u64,
    pub head: Option<String>,
    pub evidence: CloseEvidence,
    pub missing: Vec<String>,
    pub missing_count: usize,
    pub rework_index: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RollingCloseMetrics {
    pub format_version: String,
    pub telemetry_records: usize,
    pub closed_entries: usize,
    pub instrumented_entries: usize,
    pub complete_chain_records: usize,
    pub chain_completeness_rate: f64,
    pub missing_evidence_total: usize,
    pub navigation_calls: u64,
    pub navigation_failures: u64,
    pub navigation_failure_rate: f64,
    pub gate_rejections: u64,
    pub rework_count: usize,
    pub by_entry: Vec<EntryCloseMetric>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryCloseMetric {
    pub entry_id: String,
    pub batches: usize,
    pub complete_batches: usize,
    pub missing_evidence: usize,
}

fn write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn telemetry_path(root: &Path) -> PathBuf {
    root.join(CLOSE_TELEMETRY_REL)
}

fn field<'a>(entry: &'a crate::docstore::Entry, name: &str) -> Option<&'a str> {
    entry
        .fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn batch(entry: &crate::docstore::Entry) -> Option<String> {
    field(entry, "批次")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn passed_commands(root: &Path, entry_id: &str) -> Vec<String> {
    crate::test_record::records_for_entry(root, entry_id)
        .into_iter()
        .filter(|record| record["status"].as_str() == Some("passed"))
        .filter_map(|record| {
            record["fields"].as_array()?.iter().find_map(|item| {
                (item["key"].as_str() == Some("命令"))
                    .then(|| item["value"].as_str().unwrap_or_default().to_string())
            })
        })
        .collect()
}

fn acceptance_reconciled(entry: &crate::docstore::Entry) -> bool {
    let acceptance = field(entry, "验收").unwrap_or_default();
    let progress = field(entry, "进展").unwrap_or_default();
    let clauses: Vec<char> = acceptance
        .chars()
        .filter(|character| ('①'..='⑳').contains(character))
        .collect();
    clauses.is_empty()
        || (clauses.iter().all(|clause| progress.contains(*clause))
            && (progress.contains("T-")
                || progress.contains("file:")
                || progress.contains("验收降级")))
}

fn current_head(root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn commit_mentions(root: &Path, entry_id: &str) -> bool {
    let output = std::process::Command::new("git")
        .args(["log", "-n", "100", "--format=%s"])
        .current_dir(root)
        .output();
    output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(entry_id))
        .unwrap_or(false)
}

fn prior_records(root: &Path, entry_id: &str) -> Vec<CloseTelemetry> {
    read_records(root)
        .into_iter()
        .filter(|record| record.entry_id == entry_id)
        .collect()
}

/// 记录一次成功的 close 收尾链检查。写失败只返回诊断，不影响原 close 事务。
pub fn record_close(
    root: &Path,
    entry: &crate::docstore::Entry,
    status: &str,
) -> Result<CloseTelemetry, String> {
    let commands = passed_commands(root, &entry.id);
    let compile = commands.iter().any(|command| {
        command.contains("cargo test")
            || command.contains("cargo check")
            || command.contains("cargo build")
            || command.contains("verify.ps1")
    });
    let targeted_tests = commands.iter().any(|command| {
        command.contains("cargo test -p") || command.contains("cargo test --package")
    });
    let regression = commands.iter().any(|command| {
        command.contains("cargo test --workspace")
            || command.contains("verify.ps1")
            || command.to_ascii_lowercase().contains("regression")
    });
    let evidence = CloseEvidence {
        compile,
        targeted_tests,
        regression,
        acceptance_reconciliation: acceptance_reconciled(entry),
        commit: commit_mentions(root, &entry.id),
    };
    let missing = evidence.missing();
    let previous = prior_records(root, &entry.id);
    let record = CloseTelemetry {
        schema_version: SCHEMA_VERSION,
        entry_id: entry.id.clone(),
        status: status.to_string(),
        batch: batch(entry),
        occurred_at: now_secs(),
        head: current_head(root),
        missing_count: missing.len(),
        missing,
        rework_index: previous.len() + 1,
        evidence,
    };
    let encoded = serde_json::to_string(&record).map_err(|error| error.to_string())?;
    let path = telemetry_path(root);
    let _guard = write_lock()
        .lock()
        .map_err(|_| "close telemetry lock poisoned")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&encoded);
    content.push('\n');
    crate::atomic_file::write_atomic(&path, &content).map_err(|error| error.to_string())?;
    Ok(record)
}

pub fn read_records(root: &Path) -> Vec<CloseTelemetry> {
    std::fs::read_to_string(telemetry_path(root))
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn navigation_stats(root: &Path) -> (u64, u64) {
    let dir = root.join(".kanzei/artifacts/tool-failures");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .fold((0, 0), |(calls, failures), value| {
            (
                calls + value["calls"].as_u64().unwrap_or_default(),
                failures + value["failure_count"].as_u64().unwrap_or_default(),
            )
        })
}

fn gate_rejections(root: &Path) -> u64 {
    let dir = root.join(".kanzei/artifacts/tool-failures");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .flat_map(|value| value["events"].as_array().cloned().unwrap_or_default())
        .filter(|event| {
            matches!(event["tool_name"].as_str(), Some("req") | Some("defect"))
                && event["outcome"].as_str() == Some("failed")
        })
        .count() as u64
}

fn tracker_closed_entry_ids(root: &Path) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for kind in [&crate::docstore::REQUIREMENTS, &crate::docstore::DEFECTS] {
        let store = crate::docstore::DocStore::open(root, kind);
        if let Ok(entries) = store.load_archive() {
            ids.extend(entries.into_iter().map(|entry| entry.id));
        }
        if let Ok(entries) = store.load() {
            ids.extend(
                entries
                    .into_iter()
                    .filter(|entry| kind.terminal.contains(&entry.status.as_str()))
                    .map(|entry| entry.id),
            );
        }
    }
    ids
}

pub fn rolling_metrics(root: &Path) -> RollingCloseMetrics {
    let records = read_records(root);
    let (navigation_calls, navigation_failures) = navigation_stats(root);
    let tracker_ids = tracker_closed_entry_ids(root);
    let mut ids = tracker_ids.clone();
    let mut instrumented_ids = BTreeSet::new();
    let mut by_entry: BTreeMap<String, EntryCloseMetric> = BTreeMap::new();
    let mut complete_chain_records = 0;
    let mut missing_evidence_total = 0;
    let mut rework_count = 0;
    for record in &records {
        ids.insert(record.entry_id.clone());
        instrumented_ids.insert(record.entry_id.clone());
        let item = by_entry
            .entry(record.entry_id.clone())
            .or_insert_with(|| EntryCloseMetric {
                entry_id: record.entry_id.clone(),
                ..EntryCloseMetric::default()
            });
        item.batches += 1;
        item.missing_evidence += record.missing_count;
        if record.missing_count == 0 {
            item.complete_batches += 1;
            complete_chain_records += 1;
        }
        missing_evidence_total += record.missing_count;
        rework_count += record.rework_index.saturating_sub(1);
    }
    RollingCloseMetrics {
        format_version: format!("v{SCHEMA_VERSION}"),
        telemetry_records: records.len(),
        closed_entries: ids.len(),
        instrumented_entries: instrumented_ids.len(),
        complete_chain_records,
        chain_completeness_rate: if records.is_empty() {
            0.0
        } else {
            complete_chain_records as f64 / records.len() as f64
        },
        missing_evidence_total,
        navigation_calls,
        navigation_failures,
        navigation_failure_rate: if navigation_calls == 0 {
            0.0
        } else {
            navigation_failures as f64 / navigation_calls as f64
        },
        gate_rejections: gate_rejections(root),
        rework_count,
        by_entry: by_entry.into_values().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docstore::Entry;

    fn entry(id: &str) -> Entry {
        Entry {
            id: id.into(),
            title: format!("test {id}"),
            status: "doing".into(),
            severity: None,
            fields: vec![
                ("批次".into(), "1/1".into()),
                ("验收".into(), "①编译".into()),
                (
                    "进展".into(),
                    "① T-1780000000 cargo test -p kanzei-tools".into(),
                ),
            ],
        }
    }

    #[test]
    fn close_telemetry_writes_missing_chain_and_rolling_report() {
        let root = std::env::temp_dir().join(format!("kz-close-telemetry-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let record = record_close(&root, &entry("R-001"), "done").unwrap();
        assert_eq!(record.entry_id, "R-001");
        assert!(record.missing.contains(&"定向测试".to_string()));
        let report = rolling_metrics(&root);
        assert_eq!(report.telemetry_records, 1);
        assert_eq!(report.closed_entries, 1);
        assert_eq!(report.rework_count, 0);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rolling_report_covers_ten_closed_entries() {
        let root =
            std::env::temp_dir().join(format!("kz-close-telemetry-ten-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        for index in 0..10 {
            let _ = record_close(&root, &entry(&format!("R-{index:03}")), "done").unwrap();
        }
        let report = rolling_metrics(&root);
        assert_eq!(report.closed_entries, 10);
        assert_eq!(report.instrumented_entries, 10);
        assert_eq!(report.telemetry_records, 10);
        assert_eq!(report.by_entry.len(), 10);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn complete_chain_is_counted_without_rejecting_report() {
        let root = std::env::temp_dir().join(format!(
            "kz-close-telemetry-complete-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(root.join(".kanzei/project/tests.md"), "## T-1780000000 cargo test -p kanzei-tools [passed]\n- 命令: cargo test -p kanzei-tools\n").unwrap();
        let mut item = entry("R-002");
        item.fields = vec![
            ("批次".into(), "1/1".into()),
            ("进展".into(), "① T-1780000000 file:line 提交".into()),
        ];
        let _ = record_close(&root, &item, "done").unwrap();
        let report = rolling_metrics(&root);
        assert_eq!(report.closed_entries, 1);
        assert_eq!(report.telemetry_records, 1);
        std::fs::remove_dir_all(root).ok();
    }
}
