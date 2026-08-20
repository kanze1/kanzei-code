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
        draft
            .tools
            .insert("edit", Arc::new(crate::edit::EditTool::default()));
        draft
            .tools
            .insert("insert", Arc::new(crate::edit::InsertTool));
        draft.tools.insert("bash", Arc::new(crate::bash::BashTool));
        draft
            .tools
            .insert("process", Arc::new(crate::process::ProcessTool));
        draft.tools.insert("glob", Arc::new(crate::glob::GlobTool));
        draft.tools.insert("grep", Arc::new(crate::grep::GrepTool));
        draft
            .tools
            .insert("files", Arc::new(crate::files::FilesTool));
        // R-234 B1:符号级视图——files(行数)与 read(全文)之间的粒度空白。
        draft
            .tools
            .insert("symbols", Arc::new(crate::symbols::SymbolsTool));
        draft.tools.insert("git", Arc::new(crate::git::GitTool));
        draft
            .tools
            .insert("question", Arc::new(crate::question::QuestionTool));
        draft
            .tools
            .insert("todowrite", Arc::new(crate::todowrite::TodoWriteTool));
        draft
            .tools
            .insert("webfetch", Arc::new(crate::webfetch::WebFetchTool));
        // R-217:websearch 注册进基础档位(dev 可用),默认 Ask——交互轮放行,
        // 自主轮 NonInteractive 下 Ask 即拒;域名白名单规则可精确放行。
        draft
            .tools
            .insert("websearch", Arc::new(crate::websearch::WebSearchTool));
        // R-248:先行调研是 dev 的默认能力。start 写受控骨架、validate 只读核验；
        // 真实研究仍复用 research plan/loop/source/finding，不在这里分叉。
        draft
            .tools
            .insert("prior_art", Arc::new(crate::prior_art::PriorArtTool));
        // R-269:浏览器工具(playwright-core 辅进程 headless 自检)。默认 Ask——
        // 启动 headless 浏览器与截图都有副作用面,交互轮放行、自主轮按权限判定。
        draft
            .tools
            .insert("browser", Arc::new(crate::browser_tool::BrowserTool));
        // R-273:LaTeX 编译工具(输出 PDF+诊断;系统发行优先/回落 Tectonic)。
        draft
            .tools
            .insert("latex", Arc::new(crate::latex_tool::LatexTool));
        // R-274:科研绘图工具(Vega-Lite spec → PNG,经 images 通道回模型)。
        draft
            .tools
            .insert("plot", Arc::new(crate::plot_tool::PlotTool));

        // 默认权限:读/检索全放行;写/改/命令/联网走 ask(用户可在 kanzei.toml 覆盖,后注册者胜)。
        draft.permissions.extend([
            rule("read", "*", Effect::Allow),
            rule("glob", "*", Effect::Allow),
            rule("grep", "*", Effect::Allow),
            rule("files", "*", Effect::Allow),
            rule("symbols", "*", Effect::Allow),
            rule("git", "status", Effect::Allow),
            rule("git", "diff", Effect::Allow),
            rule("git", "log", Effect::Allow),
            rule("question", "*", Effect::Allow),
            rule("write", "*", Effect::Ask),
            rule("edit", "*", Effect::Ask),
            rule("insert", "*", Effect::Ask),
            rule("bash", "*", Effect::Ask),
            rule("webfetch", "*", Effect::Ask),
            rule("websearch", "*", Effect::Ask),
            rule("prior_art", "read:*", Effect::Allow),
            rule("prior_art", "write:*", Effect::Ask),
            rule("browser", "*", Effect::Ask),
            rule("latex", "*", Effect::Ask),
            rule("plot", "*", Effect::Ask),
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

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::{Harness, KanzeiConfig, ProfileKind};

    #[test]
    fn primary_base_exposes_structured_search_and_git() {
        let root = std::env::temp_dir();
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: std::sync::Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(BaseComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let names: Vec<&str> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| tool.name())
            .collect();
        assert!(
            names.contains(&"grep"),
            "primary agent must not fall back to bash for search"
        );
        assert!(
            names.contains(&"git"),
            "Git mutations need a structured channel"
        );
        assert_eq!(snapshot.evaluate("grep", "anything"), Effect::Allow);
        assert_eq!(snapshot.evaluate("git", "status"), Effect::Allow);
        assert_eq!(snapshot.evaluate("git", "stage"), Effect::Ask);
    }
}
