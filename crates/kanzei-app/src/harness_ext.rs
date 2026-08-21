//! 前端自查与快速记录组件。

use std::sync::Arc;

use crate::ui_probe;
use kanzei_harness::{ResolveCtx, ToolCtx};
use kanzei_tools::docstore::{DEFECTS, IDEAS, REQUIREMENTS};

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
        "读取当前运行中窗口里匹配选择器的 DOM 子树(标签、class、可见文本、层级)。\
         改完前端用它确认渲染结果——node --check 只查语法,查不出渲染成什么样。只读。"
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
        "读取当前窗口自加载以来累积的 console 错误与警告(含未捕获异常)。\
         前端改动后必查:ReferenceError 一类问题不会让页面白屏,只会让某一块悄悄失效。只读。"
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
        "读取匹配元素的计算样式与盒模型(display/位置/尺寸/关键布局属性)。\
         用来判断「为什么它没显示出来」「为什么挤成一团」,比猜 CSS 快得多。只读。"
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

/// R-249 批2:窗口截图。ui_dom/ui_style 读得到结构与数值,但看不见渲染结果——
/// 对齐、遮挡、观感一类问题只有像素能回答。
struct UiScreenshotTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiScreenshotTool {
    fn name(&self) -> &'static str {
        "ui_screenshot"
    }
    fn description(&self) -> String {
        "截取当前运行中窗口的实际画面,以图片返回。ui_dom/ui_style 给的是结构与数值,\
         这个给的是「看起来到底什么样」——对齐、间距、配色、观感只能靠它判断。\
         改完前端先用 ui_dom 确认结构、再用本工具确认观感。\
         注意:抓的是屏幕上该窗口矩形区域的像素,**压在上面的其它窗口会一并入画**。\
         看到画面里有明显不属于本应用的内容时,说明窗口被遮挡了——按那部分下判断是错的,\
         请说明情况而不是硬解读。整幅空白会直接报错,但部分遮挡检测不到。只读。"
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
        "Hand an existing file to the USER as a card in the conversation (name, size, open /          reveal-in-explorer, inline preview for images). Params: path; optional caption saying          what it is and why now. Use it for artifacts the user should look at or keep —          a generated report, chart, export. This is NOT for reading a file into your own          context: that is `read`. Deliver a finished deliverable when it is ready rather than          only mentioning its path in prose."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(DeliverInput)).unwrap()
    }

    /// 只读一个 stat,不改任何东西。
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
        kanzei_harness::ToolOutput::ok(summary).with_display(serde_json::json!({
            "kind": "file",
            "name": name,
            "path": shown,
            "bytes": bytes,
            "caption": caption,
        }))
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

pub(crate) struct FrontendToolsComponent;
impl kanzei_harness::Component for FrontendToolsComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        _ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
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
        Ok(())
    }
}

pub(crate) struct QuickCaptureComponent {
    pub(crate) capture: &'static str,
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
        draft.tools.insert(name, Arc::new(tool));
        draft.permissions.push(kanzei_harness::rule(
            name,
            "*",
            kanzei_harness::Effect::Allow,
        ));
        Ok(())
    }
}

/// R-252 验收③/⑤:想法拆解子代理的工具面——req/defect 可写(产出拆解条目),
/// idea 可读写(读原想法全文、拆解后把该想法 update 成 split + refs)。
pub(crate) struct IdeaSplitComponent;
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
                &REQUIREMENTS as &'static kanzei_tools::docstore::DocKind,
            ),
            (
                "defect",
                "defect",
                &DEFECTS as &'static kanzei_tools::docstore::DocKind,
            ),
            (
                "idea",
                "idea",
                &IDEAS as &'static kanzei_tools::docstore::DocKind,
            ),
        ] {
            let tool = kanzei_tools::tracker::TrackerTool {
                tool_name,
                noun,
                kind,
                requires_refs: None,
            };
            draft.tools.insert(tool_name, Arc::new(tool));
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
mod deliver_tests {
    use super::deliver_target;
    use kanzei_harness::ToolCtx;

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
        assert_eq!(dir_err.code, Some("DELIVER_IS_DIRECTORY"));
        let missing = deliver_target("nope.txt", &ctx(&root)).unwrap_err();
        assert_eq!(missing.code, Some("DELIVER_PATH_NOT_FOUND"));
        std::fs::remove_dir_all(&root).ok();
    }
}
