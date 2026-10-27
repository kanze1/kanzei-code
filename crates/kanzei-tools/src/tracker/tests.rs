use super::TrackerTool;
use kanzei_harness::{Tool, ToolCtx};
use kanzei_memory::docstore::{DocStore, Entry, DEFECTS, FINDINGS, IDEAS, REQUIREMENTS, SOURCES};
use serde_json::json;
use std::process::Command;

fn entry(id: &str) -> Entry {
    Entry {
        id: id.into(),
        title: format!("t-{id}"),
        status: "todo".into(),
        severity: None,
        fields: vec![],
    }
}

fn discovery_record() -> String {
    json!({
        "Intent": "明确用户真正要什么",
        "Explicit": "用户原话",
        "Assumptions": "无",
        "Ambiguities": "记录待确认语义",
        "领域对象": "文章与收藏",
        "最小成功闭环": "登记后可进入确认流程",
        "延后决策": "扩展站点范围"
    })
    .to_string()
}

fn m6_tracker_fixture(tag: &str) -> (std::path::PathBuf, DocStore, TrackerTool, ToolCtx) {
    let dir = std::env::temp_dir().join(format!(
        "kz-m6-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store.save(&[entry("R-001")]).unwrap();
    std::fs::write(store.archive_file(), "# Requirements Archive\n").unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone()).with_identity(
        "m6-tree".into(),
        "m6-project".into(),
        "m6-run".into(),
        "m6-process".into(),
    );
    (dir, store, tool, ctx)
}

// Include themed fragments in this module to keep existing tracker::tests::<name> paths.
include!("tests/integrity_and_receipts.rs");
include!("tests/archive_and_maintenance.rs");
include!("tests/schema_and_research.rs");
include!("tests/writes_and_validation.rs");
include!("tests/close_and_reopen.rs");
include!("tests/scheduling.rs");
include!("tests/requirements_and_ideas.rs");
