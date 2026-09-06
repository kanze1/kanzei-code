//! 当前裁决随每次模型请求刷新，不追加进历史。
use super::WorkItemSummary;
use crate::docstore::{DocKind, Entry};
use kanzei_harness::{Component, HarnessDraft, ProfileKind, ResolveCtx};

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
