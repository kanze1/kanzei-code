//! 桌面界面检查、交付与快速记录组件；能力说明随各自工具提供。

use std::sync::Arc;

use crate::ui_probe;
use kanzei_harness::{ResolveCtx, ToolCtx};
use kanzei_memory::docstore::{DEFECTS, IDEAS, REQUIREMENTS};

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct UiProbeInput {
    #[serde(default)]
    pub(crate) selector: Option<String>,
}

struct UiDomTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiDomTool {
    fn name(&self) -> &'static str {
        "ui_dom"
    }
    fn description(&self) -> String {
        "按 selector 读取当前运行中 Kanzei 窗口的 DOM、文本和元素属性，用于检查界面结构与状态。只读。".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap()
    }
    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolCtx,
    ) -> kanzei_harness::ToolOutput {
        let input: UiProbeInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(e) => return kanzei_harness::ToolOutput::error(format!("invalid input: {e}")),
        };
        let Some(selector) = input.selector.as_deref().filter(|s| !s.trim().is_empty()) else {
            return kanzei_harness::ToolOutput::error("需要 selector");
        };
        match ui_probe("dom", selector).await {
            Ok(value) => kanzei_harness::ToolOutput::ok(
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            ),
            Err(e) => kanzei_harness::ToolOutput::error(e),
        }
    }
}

struct UiConsoleTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiConsoleTool {
    fn name(&self) -> &'static str {
        "ui_console"
    }
    fn description(&self) -> String {
        "读取当前运行中 Kanzei 窗口自加载以来累积的 console 错误、警告和未捕获异常，用于排查界面运行问题。记录可能包含历史问题。只读。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap()
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolCtx,
    ) -> kanzei_harness::ToolOutput {
        match ui_probe("console", "").await {
            Ok(value) => kanzei_harness::ToolOutput::ok(
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            ),
            Err(e) => kanzei_harness::ToolOutput::error(e),
        }
    }
}

struct UiStyleTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiStyleTool {
    fn name(&self) -> &'static str {
        "ui_style"
    }
    fn description(&self) -> String {
        "按 selector 读取当前运行中 Kanzei 窗口内元素的计算样式与盒模型，包括 display、位置、尺寸和关键布局属性，用于排查可见性与布局问题。只读。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap()
    }
    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolCtx,
    ) -> kanzei_harness::ToolOutput {
        let input: UiProbeInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(e) => return kanzei_harness::ToolOutput::error(format!("invalid input: {e}")),
        };
        let Some(selector) = input.selector.as_deref().filter(|s| !s.trim().is_empty()) else {
            return kanzei_harness::ToolOutput::error("需要 selector");
        };
        match ui_probe("style", selector).await {
            Ok(value) => kanzei_harness::ToolOutput::ok(
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            ),
            Err(e) => kanzei_harness::ToolOutput::error(e),
        }
    }
}

/// 读取当前 Kanzei 窗口区域的屏幕像素；窗口遮挡也会进入截图。
struct UiScreenshotTool;
pub(crate) async fn capture_desktop(ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
    use kanzei_harness::Tool;
    UiScreenshotTool.execute(serde_json::json!({}), ctx).await
}
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiScreenshotTool {
    fn name(&self) -> &'static str {
        "ui_screenshot"
    }
    fn description(&self) -> String {
        "截取当前运行中 Kanzei 窗口区域的屏幕像素并返回图片，用于检查布局、配色与实际画面。遮挡窗口也会入画；整幅空白会报错，部分遮挡需结合画面判断。只读。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolCtx,
    ) -> kanzei_harness::ToolOutput {
        if crate::runtime_service::is_service() {
            return crate::desktop_bridge::screenshot(_ctx).await;
        }
        // 抓图是同步的 GDI 调用,挪到阻塞线程池——别占着 async 执行器。
        let captured = tokio::task::spawn_blocking(crate::state::ui_screenshot_png).await;
        match captured {
            Ok(Ok(png)) => {
                use base64::Engine;
                let bytes = png.len();
                let data = base64::engine::general_purpose::STANDARD.encode(&png);
                kanzei_harness::ToolOutput::ok(format!(
                    "[screenshot] 当前窗口画面 (image/png, {bytes} bytes) — 已作为图片附在本次结果里。"
                ))
                .with_images(vec![kanzei_harness::ToolImage {
                    media_type: "image/png".into(),
                    data,
                }])
            }
            Ok(Err(e)) => kanzei_harness::ToolOutput::error(e),
            Err(e) => kanzei_harness::ToolOutput::error(format!("截图任务异常: {e}")),
        }
    }
}

/// R-329:`deliver` 的入参。
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct DeliverInput {
    /// 要交付的文件路径(相对代码树或绝对)。
    pub(crate) path: String,
    /// 一句话说明这是什么、为什么现在给他。
    #[serde(default)]
    pub(crate) caption: Option<String>,
}

/// R-329:把一个**已经存在的产物**交到用户面前。
///
/// 与 `read` 的分工是「给谁看」:`read` 把内容读进**模型**的上下文,`deliver`
/// 在对话里给**用户**一张卡片(文件名、大小、打开 / 在资源管理器中定位)。
/// 报告、图、导出的 CSV 这类东西模型不需要再读一遍,用户却得知道它在哪——
/// 此前只能在正文里写一句路径,用户自己去翻。
///
/// 属于应用层而不是 kanzei-tools:它要往运行中的窗口发事件,与 ui_* 同理。
/// CLI 侧没有对话卡片可言,那边不注册它。
struct DeliverTool;

#[async_trait::async_trait]
impl kanzei_harness::Tool for DeliverTool {
    fn name(&self) -> &'static str {
        "deliver"
    }

    fn description(&self) -> String {
        "Deliver an existing file as a conversation card with its name, size and open/reveal actions. \
         Images include thumbnails and HTML includes a preview action. path identifies the artifact; caption describes it. \
         Returns a delivery receipt.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(DeliverInput)).unwrap()
    }

    /// 文件只读；交付收据通过 SQLite 事务追加。
    fn concurrency(
        &self,
        _input: &serde_json::Value,
        ctx: &ToolCtx,
    ) -> kanzei_harness::ToolConcurrency {
        kanzei_harness::ToolConcurrency::shared_worktree(ctx)
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string()]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        let input: DeliverInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(error) => {
                return kanzei_harness::ToolOutput::needs_correction(
                    "INVALID_TOOL_INPUT",
                    format!("invalid input for `deliver`: {error}; expected {{\"path\": \"...\"}}"),
                )
            }
        };
        let resolved = deliver_target(&input.path, ctx);
        let (path, meta) = match resolved {
            Ok(value) => value,
            Err(output) => return *output,
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| input.path.clone());
        let bytes = meta.len();
        let shown = path
            .display()
            .to_string()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let caption = input
            .caption
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty());
        // 文本位只写事实:模型不需要「已交付」之外的信息,卡片是给人看的。
        let summary = match caption {
            Some(caption) => format!("[delivered] {name} ({bytes} bytes) — {caption}"),
            None => format!("[delivered] {name} ({bytes} bytes)"),
        };
        let display = serde_json::json!({
            "kind": "file",
            "name": name,
            "path": shown,
            "bytes": bytes,
            "caption": caption,
        });
        match crate::deliveries::record(ctx, display, &meta) {
            Ok(display) => kanzei_harness::ToolOutput::ok(summary).with_display(display),
            Err(error) => kanzei_harness::ToolOutput::failed(
                "DELIVERY_RECEIPT_FAILED",
                format!(
                    "File exists, but delivery receipt could not be saved: {error}; retry deliver."
                ),
            ),
        }
    }
}

/// 解析并校验交付目标。
///
/// 错误装箱是 clippy `result_large_err` 的要求:`ToolOutput` 有 200+ 字节,
/// 让每次成功返回都背着这份体积不划算。
///
/// 目录、不存在的路径、以及**代码树之外**的路径一律拒绝:交付卡片会给用户一个
/// 「打开」按钮,把它指向工作树外的任意路径等于把本地文件系统的读取入口交给
/// 模型输入决定。越界是可机械判定的,就在这里判掉,不留给下游。
fn deliver_target(
    raw: &str,
    ctx: &ToolCtx,
) -> Result<(std::path::PathBuf, std::fs::Metadata), Box<kanzei_harness::ToolOutput>> {
    let candidate = std::path::Path::new(raw);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        ctx.cwd.join(candidate)
    };
    let path = match joined.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return Err(Box::new(kanzei_harness::ToolOutput::failed(
                "DELIVER_PATH_NOT_FOUND",
                format!("cannot deliver {raw}: {error}"),
            )))
        }
    };
    let root = ctx.cwd.canonicalize().unwrap_or_else(|_| ctx.cwd.clone());
    if !path.starts_with(&root) {
        return Err(Box::new(kanzei_harness::ToolOutput::needs_correction(
            "DELIVER_OUTSIDE_TREE",
            format!(
                "{raw} resolves outside the work tree ({}); deliver only files produced inside it",
                root.display()
            ),
        )));
    }
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(error) => {
            return Err(Box::new(kanzei_harness::ToolOutput::failed(
                "DELIVER_PATH_NOT_FOUND",
                format!("cannot stat {raw}: {error}"),
            )))
        }
    };
    if meta.is_dir() {
        return Err(Box::new(kanzei_harness::ToolOutput::needs_correction(
            "DELIVER_IS_DIRECTORY",
            format!("{raw} is a directory; deliver a single file"),
        )));
    }
    Ok((path, meta))
}

/// UI2-0926 #8:桌面端的 `browser`。与 kanzei-tools 的无头版同名、同 schema、同输出格式,
/// 按 `preview::route` 分流:网页预览面板开着、正显示、且绑定的就是这条线 → 驱动面板
/// (用户看得见);其余一律无头。注册在 FrontendToolsComponent(装配线 middle 段),
/// 同名 insert 后注册者胜,覆盖 BaseComponent 里的无头版;CLI 装配线不经过这里。
struct DesktopBrowserTool;

#[async_trait::async_trait]
impl kanzei_harness::Tool for DesktopBrowserTool {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn description(&self) -> String {
        kanzei_tools::browser::description()
    }

    fn input_schema(&self) -> serde_json::Value {
        kanzei_tools::browser::input_schema()
    }

    fn permission_resources(
        &self,
        input: &serde_json::Value,
        ctx: &ToolCtx,
    ) -> Vec<kanzei_harness::permission::Resource> {
        kanzei_tools::browser::permission_resources(self.resources_with_ctx(input, ctx))
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        kanzei_tools::browser::resources_for(input, None, None)
    }

    /// Contextual actions authorize the same owned page even when it is hidden.
    fn resources_with_ctx(&self, input: &serde_json::Value, ctx: &ToolCtx) -> Vec<String> {
        if kanzei_tools::browser::uses_connection(input) {
            return kanzei_tools::browser::connection_resources(input, ctx);
        }
        if ctx.browser_owner.is_some() {
            let current =
                kanzei_tools::browser::current_url(kanzei_tools::browser::Backend::Headless, ctx);
            return kanzei_tools::browser::resources_for(input, Some(&ctx.cwd), current.as_deref());
        }
        crate::desktop_bridge::browser_resources(input, ctx)
    }

    fn concurrency(
        &self,
        _input: &serde_json::Value,
        ctx: &ToolCtx,
    ) -> kanzei_harness::ToolConcurrency {
        kanzei_harness::ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        if kanzei_tools::browser::uses_connection(&input) {
            return match kanzei_tools::browser::parse_browser_input(input) {
                Ok(input) => kanzei_tools::browser::execute_headless(input, ctx, false).await,
                Err(output) => *output,
            };
        }
        if ctx.browser_owner.is_some() {
            return match kanzei_tools::browser::parse_browser_input(input) {
                Ok(input) => kanzei_tools::browser::execute_headless(input, ctx, true).await,
                Err(output) => *output,
            };
        }
        crate::desktop_bridge::browser(input, ctx).await
    }
}

pub(crate) const DESKTOP_DEFERRED_TOOLS: &[&str] = &[
    "desktop",
    "ui_dom",
    "ui_console",
    "ui_style",
    "ui_screenshot",
    "frontend_locate",
    "frontend_check",
    "deliver",
];

pub(crate) struct FrontendToolsComponent;
impl kanzei_harness::Component for FrontendToolsComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
        draft.tools.insert("browser", Arc::new(DesktopBrowserTool));
        draft.tools.insert("deliver", Arc::new(DeliverTool));
        draft.tools.insert("ui_dom", Arc::new(UiDomTool));
        draft.tools.insert("ui_console", Arc::new(UiConsoleTool));
        draft.tools.insert("ui_style", Arc::new(UiStyleTool));
        draft
            .tools
            .insert("ui_screenshot", Arc::new(UiScreenshotTool));
        draft.tools.insert(
            "frontend_locate",
            Arc::new(kanzei_tools::frontend::FrontendLocateTool),
        );
        draft.tools.insert(
            "frontend_check",
            Arc::new(kanzei_tools::frontend::FrontendCheckTool),
        );
        for name in [
            // deliver 只做一次 stat 并发一张卡片,不改任何文件,与 UI 自查同档放行。
            "deliver",
            "ui_dom",
            "ui_console",
            "ui_style",
            // 截图是纯读:不改任何文件、不动窗口状态,与其余 UI 自查同档放行。
            "ui_screenshot",
            "frontend_locate",
            "frontend_check",
        ] {
            draft.permissions.push(kanzei_harness::rule(
                name,
                "*",
                kanzei_harness::Effect::Allow,
            ));
        }
        if ctx.profile.is_development() {
            draft.tools.insert(
                "desktop",
                Arc::new(crate::native_desktop::DesktopTool::default()),
            );
            draft
                .deferred_tools
                .extend(DESKTOP_DEFERRED_TOOLS.iter().map(|name| (*name).to_owned()));
        }
        Ok(())
    }
}

pub(crate) struct QuickCaptureComponent {
    pub(crate) capture: &'static str,
    pub(crate) source: crate::tracker_capture::CaptureSession,
}
impl kanzei_harness::Component for QuickCaptureComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        _ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
        let tool = if self.capture == "defect" {
            kanzei_tools::tracker::TrackerTool {
                tool_name: "defect",
                noun: "defect",
                kind: &DEFECTS,
                requires_refs: None,
            }
        } else {
            kanzei_tools::tracker::TrackerTool {
                tool_name: "req",
                noun: "requirement",
                kind: &REQUIREMENTS,
                requires_refs: None,
            }
        };
        let name = tool.tool_name;
        draft.tools.insert(
            name,
            Arc::new(crate::tracker_capture::RegistrationTracker {
                inner: tool,
                source: self.source.clone(),
            }),
        );
        draft.permissions.push(kanzei_harness::rule(
            name,
            "*",
            kanzei_harness::Effect::Allow,
        ));
        Ok(())
    }
}

/// Registration is scoped to new R/D records; completion is an explicit, validated update of the selected idea.
pub(crate) struct IdeaSplitComponent {
    pub(crate) idea: kanzei_memory::docstore::Entry,
    pub(crate) source: crate::tracker_capture::CaptureSession,
}
impl kanzei_harness::Component for IdeaSplitComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        _ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
        for (tool_name, noun, kind) in [
            (
                "req",
                "requirement",
                &REQUIREMENTS as &'static kanzei_memory::docstore::DocKind,
            ),
            (
                "defect",
                "defect",
                &DEFECTS as &'static kanzei_memory::docstore::DocKind,
            ),
            (
                "idea",
                "idea",
                &IDEAS as &'static kanzei_memory::docstore::DocKind,
            ),
        ] {
            let inner = kanzei_tools::tracker::TrackerTool {
                tool_name,
                noun,
                kind,
                requires_refs: None,
            };
            let tool: Arc<dyn kanzei_harness::Tool> = if tool_name == "idea" {
                Arc::new(crate::tracker_capture::IdeaSplitTracker {
                    inner,
                    idea: self.idea.clone(),
                    source: self.source.clone(),
                })
            } else {
                Arc::new(crate::tracker_capture::RegistrationTracker {
                    inner,
                    source: self.source.clone(),
                })
            };
            draft.tools.insert(tool_name, tool);
            draft.permissions.push(kanzei_harness::rule(
                tool_name,
                "*",
                kanzei_harness::Effect::Allow,
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod browser_registration_tests {
    use kanzei_harness::{ConfigComponent, Harness, KanzeiConfig, ProfileKind, ResolveCtx};

    fn ctx() -> ResolveCtx {
        let root = std::path::PathBuf::from("C:/kanzei-browser-registration");
        ResolveCtx {
            profile: ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root,
            config: std::sync::Arc::new(KanzeiConfig::default()),
        }
    }

    fn browser_count(harness: Harness) -> usize {
        harness
            .resolve(&ctx())
            .unwrap()
            .materialize_tools()
            .iter()
            .filter(|tool| tool.name() == "browser")
            .count()
    }

    // ── 分区:网页预览后端 ──
    /// UI2-0926 #8:桌面装配线的 browser 由 FrontendToolsComponent 同名覆盖(只一个),
    /// CLI 装配线(没有 FrontendTools)仍是 kanzei-tools 的无头版。
    #[test]
    fn 桌面装配线的browser由前端组件覆盖_cli仍是无头版() {
        let mut only_frontend = Harness::default();
        only_frontend.add(super::FrontendToolsComponent);
        assert_eq!(
            browser_count(only_frontend),
            1,
            "FrontendToolsComponent 自己注册 browser"
        );

        let desktop = crate::run::assembly::build_run_harness(ProfileKind::DevAuto, false, None);
        assert_eq!(browser_count(desktop), 1, "同名覆盖,不重复");

        let cli = kanzei_runtime::build_harness(ProfileKind::DevAuto, |_| {}, |_| {});
        assert_eq!(browser_count(cli), 1, "CLI 仍有无头 browser");

        // 权限分级对桌面版同样生效(规则挂在 BaseComponent,按 action 名匹配)。
        let mut harness = Harness::default();
        harness
            .add(kanzei_tools::BaseComponent)
            .add(super::FrontendToolsComponent)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx()).unwrap();
        assert_eq!(
            snapshot.evaluate("browser", "url:localhost:5173"),
            kanzei_harness::Effect::Allow
        );
        assert_eq!(
            snapshot.evaluate("browser", "url:example.com"),
            kanzei_harness::Effect::Ask
        );
    }
}

#[cfg(test)]
mod deliver_tests {
    use super::{deliver_target, DeliverTool};
    use kanzei_harness::{Tool, ToolCtx};

    #[tokio::test]
    async fn deliver_does_not_acknowledge_when_receipt_cannot_be_saved() {
        let root = fixture("receipt-failure");
        std::fs::write(root.join(".kanzei"), "not a directory").unwrap();
        let result = DeliverTool
            .execute(serde_json::json!({"path":"report.html"}), &ctx(&root))
            .await;
        assert!(result.is_error);
        assert_eq!(result.code.as_deref(), Some("DELIVERY_RECEIPT_FAILED"));
        assert!(result.display.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn deliver_card_and_reloaded_receipt_resolve_the_same_file() {
        let root = fixture("durable-card");
        let result = DeliverTool
            .execute(serde_json::json!({"path":"report.html"}), &ctx(&root))
            .await;
        assert!(!result.is_error, "{}", result.content);
        let rows = crate::deliveries::delivered_files(root.display().to_string()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["status"], "available");
        assert_eq!(rows[0]["path"], result.display.unwrap()["path"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn fixture(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-deliver-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("report.html"), "<p>ok</p>").unwrap();
        root
    }

    fn ctx(root: &std::path::Path) -> ToolCtx {
        ToolCtx::new(root.to_path_buf(), root.to_path_buf())
    }

    #[test]
    fn 相对与绝对路径都能解析到同一文件() {
        let root = fixture("resolve");
        let ctx = ctx(&root);
        let (rel, meta) = deliver_target("report.html", &ctx).unwrap();
        let (abs, _) =
            deliver_target(&root.join("report.html").display().to_string(), &ctx).unwrap();
        assert_eq!(rel, abs);
        assert_eq!(meta.len(), 9);
        std::fs::remove_dir_all(&root).ok();
    }

    /// 交付卡片会给用户一个「打开」按钮;把它指向工作树之外等于把本地文件系统的
    /// 读取入口交给模型输入决定。越界是可机械判定的,就在这里判掉。
    #[test]
    fn 工作树之外的路径被拒绝() {
        let root = fixture("escape");
        let outside = root.parent().unwrap().join("kz-deliver-outsider.txt");
        std::fs::write(&outside, "secret").unwrap();
        let err = deliver_target(&outside.display().to_string(), &ctx(&root)).unwrap_err();
        assert!(err.is_error);
        assert!(
            err.content.contains("outside the work tree"),
            "拒绝理由要点名越界: {}",
            err.content
        );
        // `..` 逃逸走同一条判定(canonicalize 之后再比)。
        let escaped = deliver_target("../kz-deliver-outsider.txt", &ctx(&root));
        assert!(escaped.is_err(), "`..` 逃逸必须同样被拒");
        std::fs::remove_file(&outside).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 目录与不存在的路径给出可行动错误() {
        let root = fixture("bad");
        let dir_err = deliver_target("sub", &ctx(&root)).unwrap_err();
        assert_eq!(dir_err.code.as_deref(), Some("DELIVER_IS_DIRECTORY"));
        let missing = deliver_target("nope.txt", &ctx(&root)).unwrap_err();
        assert_eq!(missing.code.as_deref(), Some("DELIVER_PATH_NOT_FOUND"));
        std::fs::remove_dir_all(&root).ok();
    }
}
