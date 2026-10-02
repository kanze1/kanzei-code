//! 桌面装配线的 harness 组件:讨论只读边界与分支线 tracker 写入策略(由 assembly.rs 拆出)。

use kanzei_harness::{Component, Effect, HarnessDraft, ResolveCtx, Rule};

// Applied after project configuration. Discussions can inspect shared facts,
// but never acquire work, change memory/requirements or publish deliverables.
pub(super) struct DiscussionBoundary;
impl Component for DiscussionBoundary {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != kanzei_harness::ProfileKind::Readonly {
            return Ok(());
        }
        for name in [
            "req", "defect", "idea", "decision", "source", "finding", "work",
        ] {
            draft.permissions.push_managed_hard_deny(
                kanzei_harness::rule(name, "write:*", Effect::Deny),
                None,
                Some("讨论只读；请将结论交给主对话执行"),
            );
        }
        for name in ["memory_note", "deliver"] {
            draft.permissions.push_managed_hard_deny(
                kanzei_harness::rule(name, "*", Effect::Deny),
                None,
                Some("讨论只读；请将结论交给主对话执行"),
            );
        }
        let names: Vec<_> = draft.tools.names().map(str::to_owned).collect();
        for name in names {
            let resource = match name.as_str() {
                "read" | "glob" | "grep" | "files" | "symbols" | "webfetch" | "websearch"
                | "memory_search" | "question" => continue,
                "req" | "defect" | "idea" | "decision" | "source" | "finding" | "work" => "write:*",
                // Git's mutable commands do not all use a write: resource prefix.
                "git" => {
                    for command in [
                        "commit",
                        "add",
                        "init",
                        "checkout",
                        "branch",
                        "worktree",
                        "push",
                        "pull",
                        "reset",
                        "restore",
                        "merge",
                        "cherry-pick",
                    ] {
                        draft.permissions.push_managed_hard_deny(
                            kanzei_harness::rule("git", command, Effect::Deny),
                            None,
                            Some("讨论只读；请将结论交给主对话执行"),
                        );
                    }
                    continue;
                }
                _ => "*",
            };
            draft.permissions.push_managed_hard_deny(
                kanzei_harness::rule(&name, resource, Effect::Deny),
                None,
                Some("讨论只读；请将结论交给主对话执行"),
            );
        }
        Ok(())
    }
}

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
