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
        // 只读档位(R-102):分析类任务免配权限直接跑。
        // 工具集 = Base 的只读族(read/glob/grep/files/git 只读子命令) + webfetch;
        // 权限强制(read/write/edit 硬 deny、bash 禁用提示替代)在批2 落码,
        // 批1 只建档位概念与 agent,装配必须能解析出这个 profile。
        // 权限强制(批2):write/edit/bash 硬 deny(带替代指引),只读族显式放行。
        for action in ["read", "glob", "grep", "files"] {
            draft.permissions.push(rule(action, "*", Effect::Allow));
        }
        // git 只读子命令放行;状态/差异/日志是分析任务的主干工具。
        for subcommand in ["status", "diff", "log"] {
            draft
                .permissions
                .push(rule("git", subcommand, Effect::Allow));
        }
        // 只读档位下联网抓取放行(分析"外部事实"时的主要只读通道)。
        draft.permissions.push(rule("webfetch", "*", Effect::Allow));
        // 写与命令:硬 deny 且带合法替代指引——硬 deny 只说"不准走这条路",
        // 不说"那该怎么走"就是能力死区,模型会去找旁路(D-173)。
        // 用 ManagedResource 而非裸 push_hard_deny,拒绝理由能点名替代工具。
        for action in ["write", "edit", "insert", "bash"] {
            draft.permissions.push_managed_hard_deny(
                rule(action, "*", Effect::Deny),
                None,
                Some("只读档位:write/edit/insert/bash 一律禁止;需要结果请用 read/glob/grep/files/git status|diff|log/webfetch 观察,确需修改则告诉用户手动执行"),
            );
        }
        // task 子代理天然只读(SubagentBase 快照),无需规则——runner 直接放行。
        draft.agents.insert(
            "readonly",
            AgentDef {
                name: "readonly".into(),
                profile: ProfileScope::All,
                model: "primary".into(),
                mode: AgentMode::Primary,
                steps: 0,
                system: "You are the read-only analysis agent. You may READ, SEARCH and \
                         EXPLORE the repository (read/glob/grep/files/git status/diff/log, \
                         webfetch), but you MUST NOT modify anything: no write, no edit, no \
                         bash. Answer the user's question from what you can observe; if an \
                         answer requires writing or running commands, say exactly what would \
                         need to change and let the user do it."
                    .into(),
            },
        );
        Ok(())
    }
}
