//! 当前裁决随每次模型请求刷新，不追加进历史。
use kanzei_harness::{Component, HarnessDraft, ProfileKind, ResolveCtx};

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
