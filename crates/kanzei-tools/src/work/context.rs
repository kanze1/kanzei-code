//! 当前裁决随每次模型请求刷新，不追加进历史。
use super::WorkItemSummary;
use crate::docstore::{DocKind, Entry};
use kanzei_harness::{Component, HarnessDraft, ProfileKind, ResolveCtx};

/// Facts are supplied by the engine; the model only writes semantic milestone handoffs.
pub(super) fn recent_test_evidence(
    root: &std::path::Path,
    entry_id: &str,
) -> Vec<serde_json::Value> {
    let mut records = crate::test_record::records_for_entry(root, entry_id);
    records.sort_by_key(|record| {
        record["id"]
            .as_str()
            .unwrap_or_default()
            .strip_prefix("T-")
            .and_then(|id| id.parse::<u64>().ok())
            .unwrap_or(0)
    });
    records.into_iter().rev().take(3).map(|record| {
        let summary = record["fields"].as_array().and_then(|fields| {
            fields.iter().find(|field| field["key"] == "摘要")
                .and_then(|field| field["value"].as_str())
        }).unwrap_or_default();
        serde_json::json!({
            "id": record["id"], "status": record["status"],
            "title": record["title"].as_str().unwrap_or_default().chars().take(120).collect::<String>(),
            "summary": summary.chars().take(260).collect::<String>(),
        })
    }).collect()
}

pub(super) fn recent_completed(
    documents: [(&[Entry], &[Entry], &DocKind); 2],
) -> Vec<WorkItemSummary> {
    documents
        .into_iter()
        .flat_map(|(active, archive, kind)| {
            active
                .iter()
                .chain(archive.iter())
                .filter(|entry| kind.terminal.contains(&entry.status.as_str()))
                .rev()
                .take(4)
                .map(move |entry| WorkItemSummary {
                    id: entry.id.clone(),
                    kind: if kind.prefix == "R" {
                        "requirement"
                    } else {
                        "defect"
                    }
                    .into(),
                    title: entry.title.clone(),
                    lifecycle_status: entry.status.clone(),
                    block_reasons: Vec::new(),
                })
        })
        .collect()
}

pub struct WorkControlContext(pub kanzei_harness::auto_run::WorkPriority);

impl Component for WorkControlContext {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile == ProfileKind::Dev {
            let priority = self.0;
            draft.context.insert(
                "dev/control",
                kanzei_harness::context::refreshing_source("dev/control", move |ctx| {
                    Some(super::resolved_control_prompt(
                        &ctx.cwd,
                        &ctx.project_root,
                        priority,
                    ))
                }),
            );
        }
        Ok(())
    }
}

/// 仅当前选中工作的适用规则；状态已在同一次裁决中取得，不额外查询队列/git。
pub(super) fn conditional_guidance(state: &super::ResolvedControlState) -> String {
    let mut rules = Vec::new();
    if state.resume_reconcile.is_some() {
        rules.push(
            "resume_reconcile 提示进展可能陈旧：先核对已落地代码和提交，再修正进展并继续。"
                .to_owned(),
        );
    }
    if state.resume_worktree.is_some() {
        rules.push(
            "resume_worktree 已提供本步工作树事实；直接读相关文件，不重复查询同一份 git 状态。"
                .to_owned(),
        );
    }
    if let Some(item) = &state.selected {
        let field = |name: &str| {
            item.fields
                .iter()
                .find(|f| f.name == name)
                .map(|f| f.value.as_str())
                .unwrap_or("")
        };
        if item.kind == "work_unit" {
            rules.push("当前是 work_unit：交接用 work checkpoint；完成走 verify → 按原验收逐条 evidence → complete，进展不复制回父 Outcome。".into());
        } else {
            if !field("批次").is_empty() {
                rules.push("批次规则：总数按实际工作选定且不超过 10。每批结束一次更新 批次 k/N 和语义进展；提交主题带 <ID> B<k>。完成时计数必须对应真实完成数，超大范围拆为后续条目，不虚报填满。".into());
            }
            if matches!(field("复杂度"), "中" | "大")
                && field("进展").is_empty()
                && field("设计冻结").is_empty()
            {
                rules.push("首次实现前做一次 Design freeze：不变式、权威数据源、预期修改文件、最小验证四项，写入设计冻结字段。已有约定沿用，只有新证据推翻时才修订。".into());
            }
        }
    }
    if rules.is_empty() {
        String::new()
    } else {
        format!(
            "<current-work-policy>\n{}\n</current-work-policy>",
            rules.join("\n")
        )
    }
}
