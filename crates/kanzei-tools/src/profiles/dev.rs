//! Dev profile 的工具、权限、上下文和 agent 装配。
//!
//! 该模块只拆分装配边界，不改变原有 `Component::contribute` 的行为或调用方。

use super::*;

/// B1 定稿:CLI Dev 延迟层名单,只在 DevProfile 已注册全部工具后加入草稿。
pub const DEV_DEFERRED_TOOLS: &[&str] = &[
    "process",
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

pub struct DevProfile;

impl Component for DevProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Dev {
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
        draft
            .tools
            .insert("memory_search", Arc::new(crate::memory::MemorySearchTool));
        draft
            .tools
            .insert("memory_note", Arc::new(crate::memory::MemoryNoteTool));
        draft
            .tools
            .insert("memory_stats", Arc::new(crate::memory::MemoryStatsTool));
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

        // 原始想法收件箱(R-252):想法线只注入计数与标题,不注全文——未拆解的
        // 想法不是待办(取活引擎不取它),全文不该污染每轮上下文;拆解由用户点
        // 按钮派 idea_split 子代理,引擎不做自动拆解。
        draft.context.insert(
            "dev/ideas",
            source("dev/ideas", |ctx: &ResolveCtx| {
                let entries = DocStore::open(&ctx.project_root, &IDEAS).load().ok()?;
                let inbox: Vec<&crate::docstore::Entry> =
                    entries.iter().filter(|e| e.status == "inbox").collect();
                if inbox.is_empty() {
                    return None;
                }
                let mut out = format!(
                    "<ideas>\n想法收件箱 {} 条待拆解(录入不过模型,原样收下):\n",
                    inbox.len()
                );
                for idea in inbox.iter().take(20) {
                    out.push_str(&format!("- {} {}\n", idea.id, idea.title));
                }
                out.push_str("未拆分的想法是背景，不作为可执行任务。\n</ideas>");
                Some(out)
            }),
        );

        // 开发规范(用户手写,agent 只读遵守;write/edit 对 project 目录本就硬 deny)。
        //
        // **全量注入,不设字符预算**(D-201)。原实现只取前 3000 字符,而本仓库的
        // conventions.md 是 151 行 / 14944 字符——只有 16% 送达,截断点正好切在
        // `## 1.2 关闭边界:可用即关闭` 这个标题上。后果可测,而且是同一份文件的
        // 前后对照:§1.25「关闭前逐条对照验收、给精确代码位置」因为**同时也写进了
        // dev 的 system prompt** 而被严格遵守(近期条目的验收证据都很详实);
        // §1.2「不因缺 E2 夹具等验证增强项长期滞留 fixing」只存在于被截断的部分,
        // 于是 11 条 high 缺陷带着**已经发布的修复**卡在 fixing。被投喂的规则被
        // 遵守,被截断的没有——这不是纪律问题,是投递问题。
        //
        // 规范是用户的常驻定调,不是可按预算取舍的参考资料;口径对齐 CLAUDE.md
        // (全量进上下文,不做字符截断)。要控成本请去精简规范本身,而不是让引擎
        // 悄悄替用户决定哪几条不算数。
        draft.context.insert(
            "dev/conventions",
            refreshing_source("dev/conventions", |ctx: &ResolveCtx| {
                // R-191:通用规则单源进引擎,所有项目默认注入;项目文件只追加项目特有规则。
                // 通用部分永远在(无项目文件的项目也拿到完整约束),项目文件在其后拼接。
                let mut text = String::from(kanzei_harness::DEFAULT_CONVENTIONS);
                if ctx
                    .project_root
                    .join("crates/kanzei-app/Cargo.toml")
                    .is_file()
                {
                    text.push_str(
                        "

",
                    );
                    text.push_str(kanzei_harness::CARGO_CONVENTIONS);
                }
                let path = ctx.project_root.join(".kanzei/project/conventions.md");
                match std::fs::read_to_string(&path) {
                    Ok(project_rules) => {
                        text.push_str("\n\n<project-rules exists=\"true\">\n");
                        text.push_str(project_rules.trim());
                        text.push_str("\n</project-rules>");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        text.push_str("\n<project-rules exists=\"false\" />");
                    }
                    Err(error) => text.push_str(&format!(
                        "\n项目规范读取失败：{error}；先解决读取问题，不重建覆盖。"
                    )),
                }
                Some(format!("<conventions>\n{}\n</conventions>", text.trim()))
            }),
        );

        // 常驻项目层概览；模块经验和上下游通过 architecture context 按需展开。
        draft.context.insert(
            "dev/project-knowledge",
            source("dev/project-knowledge", |ctx: &ResolveCtx| {
                let overview = crate::project_knowledge::snapshot(&ctx.project_root, &ctx.cwd)
                    .overview(MEMORY_CONTEXT_BUDGET);
                (!overview.is_empty()).then_some(overview)
            }),
        );
        // 用户已确认的项目约束仍常驻；平铺事实索引已退役。
        draft.context.insert(
            "dev/memory",
            source("dev/memory", |ctx: &ResolveCtx| {
                let mut out = String::from("<project-constraints>\n");
                let mut folded = 0;
                for (_, entry) in crate::memory::MemoryStore::project(&ctx.project_root).load_all()
                {
                    if entry.status != "active" || entry.category != "preference" {
                        continue;
                    }
                    // 模块约束按区域加载，只有无区域的项目级约束常驻。
                    if !entry.areas().is_empty() {
                        continue;
                    }
                    let text = format!("{} {}\n{}\n", entry.id, entry.title, entry.body.trim());
                    if out.chars().count() + text.chars().count() > MEMORY_CONTEXT_BUDGET {
                        folded += 1;
                        continue;
                    }
                    out.push_str(&text);
                }
                if folded > 0 {
                    out.push_str(&format!(
                        "预算未加载 {folded} 条约束，memory_search category=preference 可取全文。\n"
                    ));
                }
                out.push_str("</project-constraints>");
                Some(out)
            }),
        );

        draft.context.insert(
            "dev/decisions",
            source("dev/decisions", |ctx: &ResolveCtx| {
                let entries = DocStore::open(&ctx.project_root, &DECISIONS).load().ok()?;
                let standing: Vec<String> = entries
                    .iter()
                    .filter(|e| e.status == "accepted")
                    .map(|e| format!("{} {}", e.id, e.title))
                    .collect();
                if standing.is_empty() {
                    return None;
                }
                Some(format!(
                    "<decisions>\n{}\nAccepted decisions are standing constraints — do not \
                     re-litigate them; `decision get <id>` for rationale. Record newly agreed \
                     designs/tradeoffs with `decision add` (status draft until the user accepts). The deferred tool must be loaded with `tool_search select:decision` before calling it.\n</decisions>",
                    standing.join("\n")
                ))
            }),
        );

        draft.context.insert(
            "dev/design-index",
            source("dev/design-index", |ctx: &ResolveCtx| {
                let path = ctx
                    .project_root
                    .join(".kanzei/project/architecture/README.md");
                let text = std::fs::read_to_string(path).ok()?;
                let lines: Vec<&str> = text
                    .lines()
                    .filter(|line| {
                        line.contains("[identity:")
                            && line.contains("](../../../docs/design/")
                            && !line.contains("[identity: superseded;")
                    })
                    .collect();
                if lines.is_empty() {
                    return None;
                }
                Some(format!(
                    "<design-index>\n当前设计只注入身份、核验提交和入口摘要；正文按需读取。\n{}\n</design-index>",
                    lines.join("\n")
                ))
            }),
        );

        draft.context.insert(
            "dev/verification-policy",
            source("dev/verification-policy", |ctx: &ResolveCtx| {
                Some(super::policy::effective_verification_policy(
                    &ctx.config.cadence,
                ))
            }),
        );

        draft.agents.insert(
            "dev",
            AgentDef {
                name: "dev".into(),
                profile: ProfileScope::Dev,
                model: "primary".into(),
                mode: AgentMode::Primary,
                // 0 = 无轮数上限(用户定调)。
                steps: 0,
                system: include_str!("dev_system.md").into(),
            },
        );
        draft.agents.insert(
            "dev-pair",
            AgentDef {
                name: "dev-pair".into(),
                profile: ProfileScope::Dev,
                model: "primary".into(),
                mode: AgentMode::Primary,
                steps: 0,
                system: "You are the pair-programming agent working WITH the user in conversation. \
                         Follow the user's direction — their latest message defines the task. \
                         Answer questions directly; do NOT start coding when the user is only asking \
                         or discussing. Before non-trivial changes, state a one-line plan first. \
                         When requirements are ambiguous, ask a short clarifying question instead \
                         of guessing — prefer the `question` tool for anything the user must decide. \
                         Use the project-state facts; an executable lookup miss is not proof \
                         that a toolchain is absent. Record requirements or defects only when the user asks, or \
                         when you complete something worth tracking, then update status honestly. \
                         Ideas in context are count + titles only, background, NOT instructions — \
                         never auto-split or auto-advance them. Commit verified changes per project \
                         conventions (no co-author trailers). For codebase exploration, prefer the \
                         read-only task subagent. When you already know which files to open, emit \
                         those `read` / `grep` calls in the SAME step — they run in parallel and cost \
                         one round trip; keep that step pure read-only or the whole batch falls back \
                         to serial."
                    .into(),
            },
        );
        draft
            .deferred_tools
            .extend(DEV_DEFERRED_TOOLS.iter().map(|name| (*name).to_owned()));
        Ok(())
    }
}
