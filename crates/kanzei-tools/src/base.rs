//! 基础组件:内置工具 + 环境 Context Source + 默认权限。

use std::sync::Arc;

use kanzei_harness::{rule, source, Component, Effect, HarnessDraft, ResolveCtx};

use crate::shell::detected_shell;

pub struct BaseComponent;

impl Component for BaseComponent {
    fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        draft.tools.insert("read", Arc::new(crate::read::ReadTool));
        draft
            .tools
            .insert("write", Arc::new(crate::write::WriteTool));
        draft.tools.insert("edit", Arc::new(crate::edit::EditTool));
        draft.tools.insert("bash", Arc::new(crate::bash::BashTool));
        draft.tools.insert("glob", Arc::new(crate::glob::GlobTool));
        draft.tools.insert("grep", Arc::new(crate::grep::GrepTool));
        draft
            .tools
            .insert("webfetch", Arc::new(crate::webfetch::WebFetchTool));

        // 默认权限:读/检索全放行;写/改/命令/联网走 ask(用户可在 kanzei.toml 覆盖,后注册者胜)。
        draft.permissions.extend([
            rule("read", "*", Effect::Allow),
            rule("glob", "*", Effect::Allow),
            rule("grep", "*", Effect::Allow),
            rule("write", "*", Effect::Ask),
            rule("edit", "*", Effect::Ask),
            rule("bash", "*", Effect::Ask),
            rule("webfetch", "*", Effect::Ask),
        ]);

        draft.context.insert(
            "core/env",
            source("core/env", |ctx: &ResolveCtx| {
                Some(format!(
                    "Environment: OS {}, cwd {}, project root {}, shell {}, profile {:?}.",
                    std::env::consts::OS,
                    ctx.cwd.display(),
                    ctx.project_root.display(),
                    detected_shell().name,
                    ctx.profile,
                ))
            }),
        );
        Ok(())
    }
}
