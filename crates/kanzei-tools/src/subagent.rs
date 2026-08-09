//! 子代理组件(R-004/R-012):task 工具派生的只读探索代理。
//! 快照只含 read/glob/grep——写/命令/联网在代码层面就不存在,无需权限弹窗。

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
        system: "You are a read-only exploration subagent with tools read/glob/grep. \
                 Complete the given task precisely and reply with ONLY the requested \
                 information: file paths with line numbers, code excerpts, or a short \
                 factual summary. No preamble, no suggestions. If nothing is found, \
                 state that explicitly."
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
        assert_eq!(names.len(), 3);
        for name in ["read", "glob", "grep"] {
            assert!(names.contains(&name), "missing {name}");
            assert_eq!(
                snapshot.evaluate(name, "anything"),
                kanzei_harness::Effect::Allow
            );
        }
    }
}
