//! DevAuto 与 DevPair 的工具、权限、上下文和 Agent 装配。

use super::*;

/// B1 定稿:CLI Dev 延迟层名单,只在 DevelopmentProfile 已注册全部工具后加入草稿。
pub const DEVELOPMENT_DEFERRED_TOOLS: &[&str] = &[
    "process",
    "sandbox",
    "files",
    "incident",
    "conventions",
    "architecture",
    "prior_art",
    "browser",
    "latex",
    "plot",
    "idea",
    "decision",
    "memory_stats",
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
        // R-321 B1:执行事故与正式缺陷分层。incident 只追加项目工件，不分配 D-ID；
        // record 仍按写操作逐次询问，list 可直接读取聚合结果。
        draft
            .tools
            .insert("incident", Arc::new(crate::incident::IncidentTool));
        draft
            .permissions
            .push(rule("incident", "list", Effect::Allow));
        draft
            .permissions
            .push(rule("incident", "record", Effect::Ask));

        // 架构索引的专用写通道(D-173):原先这个资源族只有硬 deny 没有工具,
        // 合法路径不可达,模型就去找 shell 旁路。读/校验放行,写仍逐次询问。
        draft.tools.insert(
            "architecture",
            Arc::new(crate::architecture::ArchitectureTool),
        );
        for read_only in ["context", "get", "check", "regenerate", "diagrams"] {
            draft
                .permissions
                .push(rule("architecture", read_only, Effect::Allow));
        }

        // 开发规范 conventions.md 的专用写通道(D-235):与 D-173 同根因——write/edit
        // 硬 deny 而合法路径不可达,模型就去找 shell 旁路。get(读全文+hash)放行,
        // patch(逐字替换)写操作仍逐次询问,和 architecture update 同一保守口径。
        draft
            .tools
            .insert("conventions", Arc::new(crate::conventions::ConventionsTool));
        draft
            .permissions
            .push(rule("conventions", "get", Effect::Allow));

        // 硬 deny:项目文档与记忆文件只能走专用工具(用户手改不受此限——这是模型的门禁)。
        // 每条 deny 都必须挂上它的合法替代路径:resolve 会校验那个工具真的注册了,
        // 拒绝理由也由此推导,不会再固定说一句不存在的 "use the dedicated tool"。
        // 顺序=特化在前、兜底在后;managed_for 取首个命中。
        for action in ["write", "edit", "insert"] {
            for (resource, tool, note) in [
                (
                    "*.kanzei/project/architecture/*",
                    Some("architecture"),
                    "架构索引:链接与命名由引擎校验",
                ),
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
                    "*.kanzei/project/conventions*",
                    Some("conventions"),
                    "开发规范:create 新建、propose 草案、patch 定点维护",
                ),
                (
                    "*.kanzei/memory/*",
                    Some("memory_note"),
                    "记忆库:新增/删除/状态/extra 字段属 memory-manager；既有 title/description/body 机械纠错用 memory_note action=correct",
                ),
                // 兜底族:.kanzei/project 下其余文件(conventions.md 等)是用户手写资产,
                // 模型没有任何合法写通道——如实说成"能力未实现",不要编一个工具名。
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

        register_project_conventions(draft);

        // 本轮装配时的项目概览；后续按需查询，不要求修改前固定预读。
        draft.context.insert(
            "development/project-knowledge",
            fallible_source(
                "development/project-knowledge",
                ContextRequirement::Optional,
                |ctx: &ResolveCtx| {
                    let tree = crate::base::code_tree_root(&ctx.cwd, &ctx.project_root);
                    let overview =
                        crate::project_knowledge::snapshot_checked(&ctx.project_root, &tree)?
                            .overview(MEMORY_CONTEXT_BUDGET)
                            .map_err(anyhow::Error::msg)?;
                    Ok((!overview.is_empty()).then_some(overview))
                },
            ),
        );

        draft.context.insert(
            "development/design-index",
            fallible_source("development/design-index", ContextRequirement::Optional, |ctx: &ResolveCtx| {
                let path = ctx.project_root.join(crate::architecture::ARCHITECTURE_REL);
                let Some(text) = read_optional_context_file(&path)? else { return Ok(None); };
                if text.trim().is_empty() {
                    return Ok(None);
                }
                Ok(Some(format!(
                    "<design-index>\nfile: {}\nrevision: {}\n项目维护的设计导航，正文按需读取；索引标注不代表实现或验收结果。\n</design-index>",
                    path.display(), crate::normalized_text_hash(&text)
                )))
            }),
        );

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
        Ok(())
    }
}
