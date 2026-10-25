//! Desktop policy for writes to shared project tracker records.

use kanzei_harness::{Component, Effect, HarnessDraft, ResolveCtx, Rule};

/// R-177 F11:分支线默认只读主根 tracker。规则放在 ConfigComponent 之后,
/// 因而用户的通用 kanzei.toml allow 不能意外打开这条线级显式开关。
pub(super) struct TrackerWritePolicyComponent {
    pub(super) block: bool,
}

impl Component for TrackerWritePolicyComponent {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        if !self.block {
            return Ok(());
        }
        for action in [
            "req", "defect", "idea", "decision", "source", "finding", "work",
        ] {
            draft.permissions.push_denial_note(
                Rule {
                    action: action.into(),
                    resource: "write:*".into(),
                    effect: Effect::Deny,
                },
                "当前独立任务未开启「改主项目需求记录」；读取仍可用。请在并行线路页该任务的卡片上开启后，再修改主项目的需求、缺陷等记录。",
            );
        }
        Ok(())
    }
}
