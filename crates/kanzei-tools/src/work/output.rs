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

/// 模型默认只接收当前决策；完整对账和字段由显式 detail 请求获取。
pub(super) fn structured_control_output(state: ResolvedControlState) -> serde_json::Value {
    let counts = json!({
        "queued_wip": state.queued_wip.len(), "blocked": state.blocked_items.len(),
        "parked": state.parked_items.len(), "foreign_wip": state.foreign_wip.len(),
        "reconciliation": state.reconciliation.counts,
    });
    let mut state = super::compact_for_context(state);
    state.reason = prompt_safe_block_reason(&state.reason);
    state.queued_wip.truncate(8);
    state.blocked_items.truncate(8);
    state.parked_items.truncate(8);
    state.foreign_wip.truncate(8);
    let mut value = serde_json::to_value(state).expect("control state serializes");
    value["queue_counts"] = counts;
    value["details"] = json!(
        "work next detail=true 查看完整状态；work reconcile 查看对账；req/defect get 查看指定条目"
    );
    // 历史进展与重复验证字段不随每次请求线性增长。原字段保持在 tracker 中。
    if let Some(selected) = value.get_mut("selected").filter(|v| v.is_object()) {
        let fields = selected["fields"].as_array().cloned().unwrap_or_default();
        let kept = [
            "目标", "内容", "边界", "验收", "批次", "进展", "验证", "复现", "根因", "期望",
        ]
        .into_iter()
        .filter_map(|name| {
            fields
                .iter()
                .rev()
                .find(|field| field["name"] == name)
                .cloned()
        })
        .collect::<Vec<_>>();
        selected["omitted_fields"] = json!(fields.len().saturating_sub(kept.len()));
        selected["fields"] = json!(kept);
    }
    bound_text(&mut value);
    value
}

fn bound_text(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) if text.chars().count() > 700 => {
            *text = text.chars().take(700).collect::<String>() + "…[已裁剪，完整内容见 detail]";
        }
        serde_json::Value::Array(items) => {
            items.truncate(12);
            for item in items {
                bound_text(item);
            }
        }
        serde_json::Value::Object(fields) => {
            for item in fields.values_mut() {
                bound_text(item);
            }
        }
        _ => {}
    }
}
