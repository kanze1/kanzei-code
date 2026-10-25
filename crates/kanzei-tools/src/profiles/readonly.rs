//! Readonly analysis defaults. Runtime admission also enforces each tool's
//! audited readonly contract, including the concrete operation of mixed tools.
use kanzei_harness::{
    rule, AgentDef, AgentMode, Component, Effect, HarnessDraft, ProfileKind, ProfileScope,
    ResolveCtx,
};

pub struct ReadonlyProfile;

impl Component for ReadonlyProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Readonly {
            return Ok(());
        }
        for action in ["read", "glob", "grep", "files", "symbols", "webfetch"] {
            draft.permissions.push(rule(action, "*", Effect::Allow));
        }
        for subcommand in ["status", "diff", "log"] {
            draft
                .permissions
                .push(rule("git", subcommand, Effect::Allow));
        }
        draft.permissions.push_managed_hard_deny(
            rule("git", "init", Effect::Deny),
            None,
            Some("只读档位不建仓库；需要实施时说明所需变更。"),
        );
        // Known effectful tools are denied at the ruleset as well as runtime
        // admission. New tools still need an audited readonly access contract.
        for action in [
            "write", "edit", "insert", "bash", "sandbox", "process", "browser", "latex", "plot",
        ] {
            draft.permissions.push_managed_hard_deny(
                rule(action, "*", Effect::Deny),
                None,
                Some("只读档位保持用户资源不变；可继续使用观察工具，说明需要实施的变更。"),
            );
        }
        draft.agents.insert(
            "readonly",
            AgentDef {
                name: "readonly".into(),
                profile: ProfileScope::All,
                model: "primary".into(),
                mode: AgentMode::Primary,
                steps: 0,
                system: include_str!("readonly_system.md").into(),
            },
        );
        Ok(())
    }
}
