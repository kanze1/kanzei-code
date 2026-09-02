use super::{ReconcileClass, ReconciliationReport, ResolvedControlState};
use crate::work::reconcile;
use serde_json::json;

pub(super) fn prompt_safe_block_reason(reason: &str) -> String {
    if reason.contains("机械对账") || reason.contains("源码指纹") || reason.contains("fingerprint")
    {
        "机械对账详情已裁剪，请使用 `work reconcile` 查看结构化结果".into()
    } else {
        reason.into()
    }
}

pub(super) fn reconciliation_gap(classification: ReconcileClass) -> &'static str {
    match classification {
        ReconcileClass::Stale => "缺少可验证的声明或交付证据",
        ReconcileClass::ImplementedUncommitted => "本条目改动面尚有未提交源码",
        ReconcileClass::CommittedUnverified => "本条目交付缺少验证证据",
        ReconcileClass::VerifiedUnclosed => "无新增对账缺口，仍待条目关闭",
    }
}

pub(super) fn reconciliation_output(
    report: &ReconciliationReport,
    project_root: &std::path::Path,
) -> serde_json::Value {
    let items = report
        .items
        .iter()
        .map(|item| {
            let ledger_rows = crate::work::log::deliver_facts(project_root, &item.id).len();
            json!({
                "id": item.id,
                "class": reconcile::classification_name(item.classification),
                "ledger_rows": ledger_rows,
                "source_file_count": item.source_files.len(),
                "test_record_ids": item.test_record_ids,
                "gap": reconciliation_gap(item.classification),
            })
        })
        .collect::<Vec<_>>();
    json!({"items": items, "counts": report.counts})
}

pub(super) fn structured_control_output(mut state: ResolvedControlState) -> ResolvedControlState {
    for item in &mut state.reconciliation.items {
        if item.title.contains("fingerprint") || item.title.contains("指纹") {
            item.title = "reconciliation item".into();
        }
        item.reasons = vec![reconciliation_gap(item.classification).into()];
        item.declared_commit = None;
        item.current_head.clear();
        item.declared_source_fingerprint = None;
        item.evidence_source_fingerprints.clear();
        item.source_files.clear();
    }
    state.reason = prompt_safe_block_reason(&state.reason);
    if let Some(item) = &mut state.selected {
        for reason in &mut item.block_reasons {
            *reason = prompt_safe_block_reason(reason);
        }
    }
    for item in &mut state.blocked_items {
        for reason in &mut item.block_reasons {
            *reason = prompt_safe_block_reason(reason);
        }
    }
    state
}
