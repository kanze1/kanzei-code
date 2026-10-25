//! 子 Agent 的只读和可写工具组件及内置角色定义。
//! 工具权限由 harness 裁决，写操作的租约由运行时按工作树获取。

use std::sync::Arc;

use kanzei_harness::{
    rule, AgentDef, AgentMode, Component, Effect, HarnessDraft, ProfileScope, ResolveCtx,
};

/// 只读角色的工具面；Git 仅放行 status、diff 和 log。
pub struct SubagentBase;

impl Component for SubagentBase {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        crate::base::add_source_policy(draft);
        draft.tools.insert("read", Arc::new(crate::read::ReadTool));
        draft.tools.insert("glob", Arc::new(crate::glob::GlobTool));
        draft.tools.insert("grep", Arc::new(crate::grep::GrepTool));
        // 文件地图、符号和 Git 调查共用只读权限边界。
        draft
            .tools
            .insert("files", Arc::new(crate::files::FilesTool));
        draft
            .tools
            .insert("symbols", Arc::new(crate::symbols::SymbolsTool));
        draft.tools.insert("git", Arc::new(crate::git::GitTool));
        draft.permissions.extend([
            rule("read", "*", Effect::Allow),
            rule("glob", "*", Effect::Allow),
            rule("grep", "*", Effect::Allow),
            rule("files", "*", Effect::Allow),
            rule("symbols", "*", Effect::Allow),
            rule("git", "status", Effect::Allow),
            rule("git", "diff", Effect::Allow),
            rule("git", "log", Effect::Allow),
        ]);
        Ok(())
    }
}

/// 可写角色的工具面；写权限由 harness 裁决，运行时负责获取工作树写租约。
pub struct WritableSubagentBase;

impl Component for WritableSubagentBase {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        crate::base::add_source_policy(draft);
        draft.tools.insert("read", Arc::new(crate::read::ReadTool));
        draft.tools.insert("glob", Arc::new(crate::glob::GlobTool));
        draft.tools.insert("grep", Arc::new(crate::grep::GrepTool));
        draft
            .tools
            .insert("edit", Arc::new(crate::edit::EditTool::default()));
        draft
            .tools
            .insert("insert", Arc::new(crate::edit::InsertTool));
        draft
            .tools
            .insert("write", Arc::new(crate::write::WriteTool));
        draft
            .tools
            .insert("bash", Arc::new(crate::bash::BashTool::default()));
        draft.tools.insert("git", Arc::new(crate::git::GitTool));
        // 读取工具默认放行；写操作继续遵守用户和项目权限规则。
        draft.permissions.extend([
            rule("read", "*", Effect::Allow),
            rule("glob", "*", Effect::Allow),
            rule("grep", "*", Effect::Allow),
        ]);
        Ok(())
    }
}

/// explore 子代理定义:快速检索、结果即报告。
pub fn explore_agent() -> AgentDef {
    AgentDef {
        name: "explore".into(),
        profile: ProfileScope::All,
        model: "fast".into(),
        mode: AgentMode::Subagent,
        steps: 0,
        system: "你是只读探索子 Agent。根据分配的任务检索代码、文件、符号和 Git 事实，按需选择调查范围。围绕请求返回结果，并提供支持结论的来源位置；未找到或尚未确认的内容如实说明。".into(),
    }
}

/// 只读分析角色，使用主模型；与探索角色共享只读工具面。
pub fn plan_agent() -> AgentDef {
    AgentDef {
        name: "plan".into(),
        profile: ProfileScope::All,
        model: "primary".into(),
        mode: AgentMode::Subagent,
        steps: 0,
        system: "你是只读分析与规划子 Agent。结合任务需要理解现有代码、调用关系和约束，提出具体可行的方案，说明涉及范围、关键取舍和验证方式。对当前行为的判断提供必要来源；区分已查证事实、假设和需要主 Agent 决定的问题。".into(),
    }
}

/// 实现角色的基础定义，具体工具与工作目录由宿主运行时装配。
pub fn writer_agent() -> AgentDef {
    AgentDef {
        name: "writer".into(),
        profile: ProfileScope::All,
        model: "primary".into(),
        mode: AgentMode::Subagent,
        steps: 0,
        system: "你是可写执行子 Agent。围绕分配的目标和范围自主推进，理解现有实现并复用已有能力。需要修改时保留无关改动，并按任务需要验证结果，说明实际改动、验证情况和剩余问题。".into(),
    }
}

/// 验证角色独立检查候选版本，不继承实现角色的修改目标。
pub(crate) fn verify_agent() -> AgentDef {
    AgentDef {
        name: "verify".into(),
        profile: ProfileScope::All,
        model: "primary".into(),
        mode: AgentMode::Subagent,
        steps: 0,
        system: "你是验证子 Agent。独立检查指定候选版本，给出结论和可复现的证据。发现实现问题时先报告，不把修复混入未授权的验证任务。按任务需要选择检查方式，如实说明已验证、未验证和剩余问题。".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::{ConfigComponent, Harness, KanzeiConfig, ProfileKind};

    #[test]
    fn subagent_snapshot_applies_user_read_deny() {
        let mut config = KanzeiConfig::default();
        config
            .permissions
            .rules
            .push(rule("read", "*/.env", Effect::Deny));
        let ctx = ResolveCtx {
            profile: ProfileKind::DevAuto,
            cwd: std::env::temp_dir(),
            project_root: std::env::temp_dir(),
            config: std::sync::Arc::new(config),
        };
        let mut harness = Harness::default();
        harness.add(SubagentBase).add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        assert_eq!(snapshot.evaluate("read", "project/.env"), Effect::Deny);
        assert_eq!(
            snapshot.evaluate("read", "project/src/main.rs"),
            Effect::Allow
        );
    }

    #[test]
    fn subagent_snapshot_is_read_only() {
        let ctx = ResolveCtx {
            profile: ProfileKind::DevAuto,
            cwd: std::env::temp_dir(),
            project_root: std::env::temp_dir(),
            config: std::sync::Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(SubagentBase);
        let snapshot = harness.resolve(&ctx).unwrap();
        let names: Vec<&str> = snapshot
            .materialize_tools()
            .iter()
            .map(|t| t.name())
            .collect();
        // R-218:只读快照扩容为 read/glob/grep/files/git(5 件)。files 纯只读;
        // git 只对 status/diff/log 放行(写 action 不在 Allow 规则里 → 子代理内被拒)。
        // R-234:symbols 符号视图加入,只读快照 6 件套。
        assert_eq!(names.len(), 6);
        for name in ["read", "glob", "grep", "files", "symbols"] {
            assert!(names.contains(&name), "missing {name}");
            assert_eq!(
                snapshot.evaluate(name, "anything"),
                kanzei_harness::Effect::Allow
            );
        }
        assert!(names.contains(&"git"), "missing git");
        for read_action in ["status", "diff", "log"] {
            assert_eq!(
                snapshot.evaluate("git", read_action),
                kanzei_harness::Effect::Allow,
                "git {read_action} 必须放行"
            );
        }
        for write_action in ["stage", "commit", "merge_ff", "finalize"] {
            assert_eq!(
                snapshot.evaluate("git", write_action),
                kanzei_harness::Effect::Ask,
                "git {write_action} 不得放行(ask 恒 Deny → 被拒)"
            );
        }
    }

    /// 可写组件的存在不能扩展只读角色的工具面。
    #[test]
    fn subagent_readonly_snapshot_unchanged_by_writable_component() {
        let ctx = ResolveCtx {
            profile: ProfileKind::DevAuto,
            cwd: std::env::temp_dir(),
            project_root: std::env::temp_dir(),
            config: std::sync::Arc::new(KanzeiConfig::default()),
        };
        // 只读档位单独装配:写工具绝不允许出现在只读快照里。
        let mut read_only = Harness::default();
        read_only.add(SubagentBase);
        let snapshot = read_only.resolve(&ctx).unwrap();
        let names: Vec<&str> = snapshot
            .materialize_tools()
            .iter()
            .map(|t| t.name())
            .collect();
        assert!(
            !names
                .iter()
                .any(|n| matches!(*n, "edit" | "insert" | "write" | "bash")),
            "只读子代理快照不得含写工具: {names:?}"
        );
        // R-218:git 进入只读快照但只放行只读 action——工具在场不等于写权限在场。
        assert!(names.contains(&"git"), "git 应在只读快照(只读子命令)");
        assert_eq!(snapshot.evaluate("git", "commit"), Effect::Ask);
        // R-234:symbols 符号视图加入只读快照(6 件套)。
        assert_eq!(names.len(), 6, "只读快照工具面: {names:?}");
    }

    /// R-176 B1:可写子代理档位含写工具;写工具权限**不预设 Allow**
    /// (用户 deny 规则必须原样生效),只读工具保持 Allow。
    #[test]
    fn writable_subagent_has_write_tools_but_permissions_follow_rules() {
        let mut config = KanzeiConfig::default();
        // 用户显式 deny bash 的某条资源:可写子代理里必须生效。
        config
            .permissions
            .rules
            .push(rule("bash", "rm -rf *", Effect::Deny));
        let ctx = ResolveCtx {
            profile: ProfileKind::DevAuto,
            cwd: std::env::temp_dir(),
            project_root: std::env::temp_dir(),
            config: std::sync::Arc::new(config),
        };
        let mut harness = Harness::default();
        harness.add(WritableSubagentBase).add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let names: Vec<&str> = snapshot
            .materialize_tools()
            .iter()
            .map(|t| t.name())
            .collect();
        for name in [
            "read", "glob", "grep", "edit", "insert", "write", "bash", "git",
        ] {
            assert!(names.contains(&name), "可写档位缺 {name}: {names:?}");
        }
        // 只读工具保持 Allow。
        for name in ["read", "glob", "grep"] {
            assert_eq!(snapshot.evaluate(name, "anything"), Effect::Allow);
        }
        // 写工具权限不预设 Allow:用户 deny 生效,其它资源按规则集(无匹配默认询问)。
        assert_eq!(
            snapshot.evaluate("bash", "rm -rf *"),
            Effect::Deny,
            "用户 deny 规则必须在可写子代理里生效"
        );
    }
}
