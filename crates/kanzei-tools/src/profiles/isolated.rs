use super::*;

/// An independent capability set. Normal profile, Markdown and desktop additions
/// are never installed; broad authorization cannot make an absent tool executable.
pub struct IsolatedProfile;

impl Component for IsolatedProfile {
    fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
        crate::base::add_source_policy(draft);
        for tool in [
            Arc::new(crate::read::ReadTool) as Arc<dyn kanzei_harness::Tool>,
            Arc::new(crate::glob::GlobTool),
            Arc::new(crate::grep::GrepTool),
            Arc::new(crate::files::FilesTool),
            Arc::new(crate::symbols::SymbolsTool),
            Arc::new(crate::sandbox::SandboxTool),
            Arc::new(crate::question::QuestionTool),
            Arc::new(kanzei_harness::ToolSearchTool),
        ] {
            let name = tool.name();
            draft.tools.insert(name, tool);
        }
        for name in [
            "read",
            "glob",
            "grep",
            "files",
            "symbols",
            "question",
            "tool_search",
        ] {
            draft.permissions.push(rule(name, "*", Effect::Allow));
        }
        draft.permissions.push(rule("sandbox", "*", Effect::Ask));
        draft.agents.insert(
            "isolated",
            AgentDef {
                name: "isolated".into(),
                profile: ProfileScope::All,
                model: "primary".into(),
                mode: AgentMode::Primary,
                steps: 0,
                system: include_str!("isolated_system.md").into(),
            },
        );
        Ok(())
    }
}
