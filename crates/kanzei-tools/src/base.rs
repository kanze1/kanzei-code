//! 基础组件:内置工具 + 环境 Context Source + 默认权限。

use std::sync::Arc;

use kanzei_harness::{
    refreshing_source, rule, source, Component, Effect, HarnessDraft, ResolveCtx,
};

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
        // R-269:浏览器工具(playwright-core 辅进程 headless 自检)。UI2-0926 #8 起权限分级:
        // 本机地址 / 本地文件 / 内联片段放行,外网仍 Ask(见下方 permissions)。
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
            // UI2-0926 #8:浏览器分级(资源形态见 browser_tool::resources_for,按解析后的 host 取)。
            // 本机回环、本地文件、内联片段与「还没打开页面」放行;其余外网仍 Ask。
            // 前缀带 `:`/`/` 分隔,`url:localhost.evil.com` 不会被 `url:localhost:*` 命中。
            rule("browser", "url:localhost", Effect::Allow),
            rule("browser", "url:localhost:*", Effect::Allow),
            rule("browser", "url:localhost/*", Effect::Allow),
            rule("browser", "url:127.0.0.1", Effect::Allow),
            rule("browser", "url:127.0.0.1:*", Effect::Allow),
            rule("browser", "url:127.0.0.1/*", Effect::Allow),
            rule("browser", "url:[::1]", Effect::Allow),
            rule("browser", "url:[::1]:*", Effect::Allow),
            rule("browser", "url:[::1]/*", Effect::Allow),
            rule("browser", "path:*", Effect::Allow),
            rule("browser", "html:*", Effect::Allow),
            rule("browser", "page:none", Effect::Allow),
            rule("latex", "*", Effect::Ask),
            rule("plot", "*", Effect::Ask),
        ]);

        draft.context.insert(
            "core/env",
            source("core/env", |ctx: &ResolveCtx| {
                // UI2-0926 #13:路径一律 simplify 形态——鞭挞轮曾把 `\\?\C:\…` 写进这一行。
                Some(format!(
                    "Environment: OS {}, cwd {}, project root {}, shell {}, profile {:?}.",
                    std::env::consts::OS,
                    crate::path_form::simplify(&ctx.cwd).display(),
                    crate::path_form::simplify(&ctx.project_root).display(),
                    detected_shell().name,
                    ctx.profile,
                ))
            }),
        );
        // UI2-0926 #13:项目状态事实(所有档位)。轮内每步刷新(bash 可能刚装了工具、建了文件),
        // 但探测结果有 30 秒缓存、bash 跑完即作废;事实不变时文本逐字节不变,不打断 prompt 缓存。
        // 代码树是工作树线时按 cwd 探测(工作树自己的 .git 文件、自己的清单),否则按项目根。
        draft.context.insert(
            "core/project-state",
            refreshing_source("core/project-state", |ctx: &ResolveCtx| {
                let tree = code_tree_root(&ctx.cwd, &ctx.project_root);
                let facts = crate::project_state::probe_cached(&tree);
                Some(crate::project_state::render(&facts))
            }),
        );
        Ok(())
    }
}

/// 代码树根:cwd 在项目根之内(或相等)时就是项目根;否则(工作树线)是 cwd 自己。
pub(crate) fn code_tree_root(
    cwd: &std::path::Path,
    project_root: &std::path::Path,
) -> std::path::PathBuf {
    let key = |path: &std::path::Path| {
        let text = crate::path_form::strip_verbatim(&path.to_string_lossy()).replace('/', "\\");
        let trimmed = text.trim_end_matches('\\').to_string();
        if cfg!(windows) {
            trimmed.to_lowercase()
        } else {
            trimmed
        }
    };
    if project_root.as_os_str().is_empty() {
        return cwd.to_path_buf();
    }
    let (cwd_key, root_key) = (key(cwd), key(project_root));
    if cwd_key == root_key
        || cwd_key.starts_with(&format!("{root_key}\\"))
        || cwd.as_os_str().is_empty()
    {
        project_root.to_path_buf()
    } else {
        cwd.to_path_buf()
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
        // UI2-0926 #13:建库是写操作,先问用户(自主轮 NonInteractive 下即拒)。
        assert_eq!(snapshot.evaluate("git", "init"), Effect::Ask);
    }

    // ── 分区:网页预览后端 ──
    /// UI2-0926 #8:browser 权限分级。资源由 browser_tool::resources_for 产出,
    /// 这里照真实判定路径(规范化后 evaluate)逐条断言。
    #[test]
    fn browser权限分级_本机与本地放行_外网仍问() {
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
        let effect = |input: serde_json::Value| {
            let resource = crate::browser_tool::resources_for(&input, None, None)
                .pop()
                .unwrap();
            let normalized =
                kanzei_harness::permission::normalize_resource_for_action("browser", &resource);
            (resource, snapshot.evaluate("browser", &normalized))
        };
        for allowed in [
            serde_json::json!({"url": "http://localhost:5173/"}),
            serde_json::json!({"url": "http://localhost/"}),
            serde_json::json!({"url": "http://localhost/app/x"}),
            serde_json::json!({"url": "http://127.0.0.1:4173/t/abc/r/def/index.html"}),
            serde_json::json!({"url": "http://[::1]:8080/"}),
            serde_json::json!({"path": "C:/proj/site/index.html"}),
            serde_json::json!({"html": "<p>x</p>"}),
            serde_json::json!({"action": "screenshot"}),
        ] {
            let (resource, decision) = effect(allowed.clone());
            assert_eq!(decision, Effect::Allow, "{allowed} → {resource}");
        }
        for asked in [
            serde_json::json!({"url": "https://example.com/"}),
            serde_json::json!({"url": "http://localhost.evil.com/"}),
            serde_json::json!({"url": "http://localhost:80@evil.com/"}),
            serde_json::json!({"url": "http://127.0.0.1.nip.io/"}),
        ] {
            let (resource, decision) = effect(asked.clone());
            assert_eq!(decision, Effect::Ask, "{asked} → {resource}");
        }
        // 只读档位的硬 deny 先于普通放行规则:本机地址也不行。
        let readonly_ctx = ResolveCtx {
            profile: ProfileKind::Readonly,
            ..ctx
        };
        let mut readonly = Harness::default();
        readonly.add(BaseComponent).add(crate::ReadonlyProfile);
        let readonly = readonly.resolve(&readonly_ctx).unwrap();
        assert_eq!(
            readonly.evaluate("browser", "url:localhost:5173"),
            Effect::Deny
        );
        assert_eq!(readonly.evaluate("browser", "html:12345678"), Effect::Deny);
    }
}
