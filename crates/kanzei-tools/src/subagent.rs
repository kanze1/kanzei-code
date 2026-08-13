//! 子代理组件(R-004/R-012):task 工具派生的只读探索代理。
//! 快照只含 read/glob/grep——写/命令/联网在代码层面就不存在,无需权限弹窗。
//!
//! R-176:新增**可写子代理档位** `WritableSubagentBase`。它是独立组件,不是给
//! 只读档位加工具——只读白名单是审计资产,构造后与执行前各复核一次(验收⑦)。
//! 可写档位装配 read/glob/grep/edit/write/bash/git,**写工具的权限不预设 Allow**:
//! 由 harness 权限规则裁决(用户 deny 就拒绝),只有只读工具保留 Allow。写子代理
//! 必须自己 `acquire_writer_lease`(B2),不得继承主代理租约、不得绕过协调器。

use std::sync::Arc;

use kanzei_harness::{
    rule, AgentDef, AgentMode, Component, Effect, HarnessDraft, ProfileScope, ResolveCtx,
};

/// 子代理的工具集与权限:全只读、全放行(ask 在子代理里无人应答,必须为零)。
pub struct SubagentBase;

impl Component for SubagentBase {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        draft.tools.insert("read", Arc::new(crate::read::ReadTool));
        draft.tools.insert("glob", Arc::new(crate::glob::GlobTool));
        draft.tools.insert("grep", Arc::new(crate::grep::GrepTool));
        // R-218:勘察角色扩容——files(文件地图)与 git 只读子命令(status/diff/log,
        // 查 git 历史与工作树状态)。保持全 allow 零 ask:files 纯只读;git 只对
        // 只读 action 放行,写类 action(stage/commit/merge_ff/finalize)不设规则,
        // 子代理内 ask 恒 Deny → 被拒(验收②)。webfetch 暂不加(网络面未审计)。
        draft
            .tools
            .insert("files", Arc::new(crate::files::FilesTool));
        draft.tools.insert("git", Arc::new(crate::git::GitTool));
        draft.permissions.extend([
            rule("read", "*", Effect::Allow),
            rule("glob", "*", Effect::Allow),
            rule("grep", "*", Effect::Allow),
            rule("files", "*", Effect::Allow),
            rule("git", "status", Effect::Allow),
            rule("git", "diff", Effect::Allow),
            rule("git", "log", Effect::Allow),
        ]);
        Ok(())
    }
}

/// R-176 B1:可写子代理档位——装配写工具,但写权限不预设 Allow。
///
/// 与 [`SubagentBase`] 的区别只在工具面:只读档位代码层面不存在写工具
/// (审计资产,验收⑦);可写档位把写工具**装配进来**,权限交给 harness 规则集
/// 裁决——用户/项目的 deny 规则原样生效,写子代理不是免死金牌。
/// 装配点必须与 B2 的写租约强制成对:写子代理执行任何写工具前要先
/// `acquire_writer_lease`(写工具不得绕过协调器,验收②)。
pub struct WritableSubagentBase;

impl Component for WritableSubagentBase {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        draft.tools.insert("read", Arc::new(crate::read::ReadTool));
        draft.tools.insert("glob", Arc::new(crate::glob::GlobTool));
        draft.tools.insert("grep", Arc::new(crate::grep::GrepTool));
        draft
            .tools
            .insert("edit", Arc::new(crate::edit::EditTool::default()));
        draft
            .tools
            .insert("write", Arc::new(crate::write::WriteTool));
        draft.tools.insert("bash", Arc::new(crate::bash::BashTool));
        draft.tools.insert("git", Arc::new(crate::git::GitTool));
        // 只读三件套沿用全放行(无人应答权限询问);写工具不预设 Allow——
        // 由 harness 权限规则集(用户/项目规则)裁决,deny 即拒绝。
        draft.permissions.extend([
            rule("read", "*", Effect::Allow),
            rule("glob", "*", Effect::Allow),
            rule("grep", "*", Effect::Allow),
        ]);
        Ok(())
    }
}

/// explore 子代理定义:小步数、结果即报告。
pub fn explore_agent() -> AgentDef {
    AgentDef {
        name: "explore".into(),
        profile: ProfileScope::All,
        model: "fast".into(),
        mode: AgentMode::Subagent,
        steps: 12,
        system: "You are a read-only exploration subagent with tools read/glob/grep/files \
                 and git read-only subcommands (status/diff/log). Complete the given task \
                 precisely and reply with ONLY the requested information: file paths with \
                 line numbers, code excerpts, git history facts, or a short factual \
                 summary. No preamble, no suggestions. If nothing is found, state that \
                 explicitly."
            .into(),
    }
}

/// R-176 B1:可写子代理定义。写工具受权限门禁约束,不得绕过协调器。
pub fn writer_agent() -> AgentDef {
    AgentDef {
        name: "writer".into(),
        profile: ProfileScope::All,
        model: "primary".into(),
        mode: AgentMode::Subagent,
        steps: 24,
        system: "You are a writable implementation subagent. You may edit/write files and \
                 run bash/git, but every write action must go through the coordinator \
                 lease and permission rules — never bypass them. You are accountable for \
                 every file you change: report exactly which files you touched and why. \
                 Prefer small, verifiable steps; run the relevant tests before claiming \
                 completion."
            .into(),
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
            profile: ProfileKind::Dev,
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
            profile: ProfileKind::Dev,
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
        assert_eq!(names.len(), 5);
        for name in ["read", "glob", "grep", "files"] {
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

    /// R-176 验收⑦回归:只读子代理档位的白名单**未被本条放宽**——
    /// SubagentBase 仍只含 read/glob/grep,写工具在代码层面不存在。
    #[test]
    fn subagent_readonly_snapshot_unchanged_by_writable_component() {
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
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
                .any(|n| matches!(*n, "edit" | "write" | "bash")),
            "只读子代理快照不得含写工具: {names:?}"
        );
        // R-218:git 进入只读快照但只放行只读 action——工具在场不等于写权限在场。
        assert!(names.contains(&"git"), "git 应在只读快照(只读子命令)");
        assert_eq!(snapshot.evaluate("git", "commit"), Effect::Ask);
        // 只读快照必须仍是 5 件套(read/glob/grep/files/git)。
        assert_eq!(names.len(), 5, "只读快照工具面: {names:?}");
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
            profile: ProfileKind::Dev,
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
        for name in ["read", "glob", "grep", "edit", "write", "bash", "git"] {
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
