//! DevAuto 与 DevPair 的工具、权限、上下文和 Agent 装配。

use super::*;

/// Specialized capabilities stay discoverable without expanding their schemas at startup.
pub const DEVELOPMENT_DEFERRED_TOOLS: &[&str] = &[
    "process",
    "sandbox",
    "files",
    "browser",
    "memory_stats",
    "memory_note",
];

pub struct DevelopmentProfile;

impl Component for DevelopmentProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if !ctx.profile.is_development() {
            return Ok(());
        }
        draft.tools.insert(
            kanzei_harness::TOOL_SEARCH,
            Arc::new(kanzei_harness::ToolSearchTool),
        );
        draft
            .permissions
            .push(rule(kanzei_harness::TOOL_SEARCH, "*", Effect::Allow));

        // Memory 系统(文件优先分级记忆):主 agent 有 检索/草稿投递/概览及机械纠错。
        draft.tools.insert(
            "memory_search",
            Arc::new(kanzei_memory::memory::MemorySearchTool),
        );
        draft.tools.insert(
            "memory_note",
            Arc::new(kanzei_memory::memory::MemoryNoteTool),
        );
        draft.tools.insert(
            "memory_stats",
            Arc::new(kanzei_memory::memory::MemoryStatsTool),
        );
        for tool in ["memory_search", "memory_note", "memory_stats"] {
            draft.permissions.push(rule(tool, "*", Effect::Allow));
        }

        // 硬 deny:记忆库文件只能走专用记忆工具，避免直接覆盖破坏索引。
        for action in ["write", "edit", "insert"] {
            draft.permissions.push_managed_hard_deny(
                rule(action, "*.kanzei/memory/*", Effect::Deny),
                Some("memory_note"),
                Some("记忆库:新增/删除/状态/extra 字段属 memory-manager；既有 title/description/body 机械纠错用 memory_note action=correct"),
            );
        }

        draft.context.insert(
            "development/verification-policy",
            source("development/verification-policy", |ctx: &ResolveCtx| {
                super::policy::effective_verification_policy(ctx.profile, &ctx.config.cadence)
            }),
        );

        draft.agents.insert(
            kanzei_harness::DEV_AUTO_AGENT,
            AgentDef {
                name: kanzei_harness::DEV_AUTO_AGENT.into(),
                profile: ProfileScope::DevAuto,
                model: "primary".into(),
                mode: AgentMode::Primary,
                steps: 0,
                system: include_str!("dev_auto_system.md").into(),
            },
        );
        draft.agents.insert(
            kanzei_harness::DEV_PAIR_AGENT,
            AgentDef {
                name: kanzei_harness::DEV_PAIR_AGENT.into(),
                profile: ProfileScope::DevPair,
                model: "primary".into(),
                mode: AgentMode::Primary,
                steps: 0,
                system: include_str!("dev_pair_system.md").into(),
            },
        );
        draft.deferred_tools.extend(
            DEVELOPMENT_DEFERRED_TOOLS
                .iter()
                .map(|name| (*name).to_owned()),
        );
        Ok(())
    }
}
