//! Projectless conversations use the ordinary tool/runtime harness without project workflow.
use kanzei_harness::{AgentDef, AgentMode, Component, HarnessDraft, ProfileScope, ResolveCtx};

pub struct GeneralChatProfile;

impl Component for GeneralChatProfile {
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
            if !name.starts_with("core/") && !name.starts_with("team/") && name != "dev/memory" {
                draft.context.remove(&name);
            }
        }
        draft.context.remove("core/project-state");
        draft.context.insert("core/env", kanzei_harness::source("core/env", |ctx: &ResolveCtx| {
            Some(format!("Environment: OS {}, conversation workspace {}, shell {}. This conversation has no project; the shared conversation store is {}.",
                std::env::consts::OS, ctx.cwd.display(), crate::detected_shell().name, ctx.project_root.display()))
        }));
        // Keep global skills, custom agents, and their role prompts. Only built-in primary
        // development aliases need the neutral prompt, including CLI/scheduled entry points.
        for name in ["general", "dev", "dev-pair"] {
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
