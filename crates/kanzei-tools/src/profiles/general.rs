//! Projectless conversations use the ordinary tool/runtime harness without project workflow.
use kanzei_harness::{AgentDef, AgentMode, Component, HarnessDraft, ProfileScope, ResolveCtx};

/// Default primary instructions for projectless requests. User-defined Agent
/// overrides are registered later; tool and context scope is applied separately.
pub struct GeneralChatProfile;

impl Component for GeneralChatProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if !kanzei_harness::is_general_conversation_root(&ctx.project_root) {
            return Ok(());
        }
        for name in [
            "general",
            kanzei_harness::DEV_AUTO_AGENT,
            kanzei_harness::DEV_PAIR_AGENT,
        ] {
            draft.agents.insert(
                name,
                AgentDef {
                    name: name.into(),
                    profile: ProfileScope::All,
                    model: "primary".into(),
                    mode: AgentMode::Primary,
                    steps: 0,
                    system: include_str!("general_system.md").into(),
                },
            );
        }
        Ok(())
    }
}

/// Apply projectless tool/context boundaries after user and application components.
/// Agent templates are registered earlier, so custom definitions retain their override order.
pub struct GeneralChatScope;

impl Component for GeneralChatScope {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if !kanzei_harness::is_general_conversation_root(&ctx.project_root) {
            return Ok(());
        }
        for name in [
            "req",
            "defect",
            "work",
            "idea",
            "decision",
            "test_record",
            "incident",
            "architecture",
            "conventions",
            "prior_art",
        ] {
            draft.tools.remove(name);
            draft.deferred_tools.remove(name);
        }
        let available = draft.tools.names().map(str::to_owned).collect();
        draft.permissions.retain_available_tool_hints(&available);
        let contexts: Vec<_> = draft.context.names().map(str::to_owned).collect();
        for name in contexts {
            if !name.starts_with("core/") && !name.starts_with("team/") {
                draft.context.remove(&name);
            }
        }
        draft.context.remove("core/project-state");
        draft.context.insert(
            "core/env",
            kanzei_harness::refreshing_source("core/env", |ctx: &ResolveCtx| {
                Some(crate::base::environment_context(ctx))
            }),
        );
        Ok(())
    }
}
