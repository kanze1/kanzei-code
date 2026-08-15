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

pub(crate) struct FrontendToolsComponent;
impl kanzei_harness::Component for FrontendToolsComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        _ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
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
