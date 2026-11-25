//! DevAuto 与 DevPair 的工具、权限、上下文和 Agent 装配。

use super::*;

/// Specialized capabilities stay discoverable without expanding their schemas at startup.
/// Queue execution keeps `work` resident only in DevAuto; DevPair loads it on demand.
pub const DEVELOPMENT_DEFERRED_TOOLS: &[&str] = &[
    "process",
    "sandbox",
    "files",
    "browser",
    "idea",
    "decision",
    "memory_stats",
    "req",
    "defect",
    "test_record",
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

        draft.tools.insert(
            "idea",
            Arc::new(TrackerTool {
                tool_name: "idea",
                noun: "idea",
                kind: &IDEAS,
                requires_refs: None,
            }),
        );
        draft.tools.insert(
            "req",
            Arc::new(TrackerTool {
                tool_name: "req",
                noun: "requirement",
                kind: &REQUIREMENTS,
                requires_refs: None,
            }),
        );
        draft.tools.insert(
            "defect",
            Arc::new(TrackerTool {
                tool_name: "defect",
                noun: "defect",
                kind: &DEFECTS,
                requires_refs: None,
            }),
        );
        draft.tools.insert("work", Arc::new(WorkTool));
        draft
            .permissions
            .push(rule("work", "read:next", Effect::Allow));
        // 设计决策沉淀(R-110):讨论定下的方案与取舍像需求/缺陷一样落条目。
        draft.tools.insert(
            "decision",
            Arc::new(TrackerTool {
                tool_name: "decision",
                noun: "decision",
                kind: &DECISIONS,
                requires_refs: None,
            }),
        );

        // Memory 系统(R-104,文件优先分级记忆):主 agent 有 检索/草稿投递/概览，
        // 以及 R-316 限定的既有条目单字段文本纠错(action=correct)。新增、删除、
        // 状态与 extra 字段仍属 M2 的 memory-manager 子代理。
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

        // 测试记录专用写通道(R-080):tests.md 是托管文件,bash 会回滚、write/edit
        // 硬 deny,agent 没有别的合法路径;写仍是写操作,按默认 ask 逐次询问。
        draft
            .tools
            .insert("test_record", Arc::new(crate::test_record::TestRecordTool));

        // 硬 deny:项目文档与记忆文件只能走专用工具(用户手改不受此限——这是模型的门禁)。
        // 每条 deny 都必须挂上它的合法替代路径:resolve 会校验那个工具真的注册了,
        // 拒绝理由也由此推导,不会再固定说一句不存在的 "use the dedicated tool"。
        // 顺序=特化在前、兜底在后;managed_for 取首个命中。
        for action in ["write", "edit", "insert"] {
            for (resource, tool, note) in [
                (
                    "*.kanzei/project/requirements*",
                    Some("req"),
                    "需求条目:ID 由引擎分配、状态机受限",
                ),
                (
                    "*.kanzei/project/defects*",
                    Some("defect"),
                    "缺陷条目:ID 由引擎分配、状态机受限",
                ),
                (
                    "*.kanzei/project/ideas*",
                    Some("idea"),
                    "原始想法:ID 由引擎分配、状态机受限",
                ),
                (
                    "*.kanzei/project/decisions*",
                    Some("decision"),
                    "设计决策:ID 由引擎分配、状态机受限",
                ),
                (
                    "*.kanzei/project/tests*",
                    Some("test_record"),
                    "测试记录:终态自动归档,由 test_record 追加",
                ),
                (
                    "*.kanzei/memory/*",
                    Some("memory_note"),
                    "记忆库:新增/删除/状态/extra 字段属 memory-manager；既有 title/description/body 机械纠错用 memory_note action=correct",
                ),
                ("*.kanzei/project/*", None, "用户手写的项目资产,模型只读"),
            ] {
                if kanzei_harness::is_general_conversation_root(&ctx.project_root)
                    && resource.contains(".kanzei/project/")
                {
                    continue;
                }
                draft.permissions.push_managed_hard_deny(
                    rule(action, resource, Effect::Deny),
                    tool,
                    Some(note),
                );
            }
        }

        // 决策与想法共用版本快照和有界索引，读取不改变条目状态。
        super::project_indexes::register(draft);

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
                // 0 = 无轮数上限(用户定调)。
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
        if ctx.profile == ProfileKind::DevPair {
            draft.deferred_tools.insert("work".into());
        }
        Ok(())
    }
}
