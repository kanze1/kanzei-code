//! 前端自查与快速记录组件。

use std::sync::Arc;

use crate::ui_probe;
use kanzei_harness::{ResolveCtx, ToolCtx};
use kanzei_tools::docstore::{DEFECTS, REQUIREMENTS};

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct UiProbeInput {
    #[serde(default)]
    pub(crate) selector: Option<String>,
}

struct UiDomTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiDomTool {
    fn name(&self) -> &'static str { "ui_dom" }
    fn description(&self) -> String { "读取当前运行中窗口里匹配选择器的 DOM 子树(标签、class、可见文本、层级)。\
         改完前端用它确认渲染结果——node --check 只查语法,查不出渲染成什么样。只读。".into() }
    fn input_schema(&self) -> serde_json::Value { serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap() }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        let input: UiProbeInput = match serde_json::from_value(input) { Ok(value) => value, Err(e) => return kanzei_harness::ToolOutput::error(format!("invalid input: {e}")) };
        let Some(selector) = input.selector.as_deref().filter(|s| !s.trim().is_empty()) else { return kanzei_harness::ToolOutput::error("需要 selector"); };
        match ui_probe("dom", selector).await { Ok(value) => kanzei_harness::ToolOutput::ok(value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())), Err(e) => kanzei_harness::ToolOutput::error(e) }
    }
}

struct UiConsoleTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiConsoleTool {
    fn name(&self) -> &'static str { "ui_console" }
    fn description(&self) -> String { "读取当前窗口自加载以来累积的 console 错误与警告(含未捕获异常)。\
         前端改动后必查:ReferenceError 一类问题不会让页面白屏,只会让某一块悄悄失效。只读。".into() }
    fn input_schema(&self) -> serde_json::Value { serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap() }
    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        match ui_probe("console", "").await { Ok(value) => kanzei_harness::ToolOutput::ok(value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())), Err(e) => kanzei_harness::ToolOutput::error(e) }
    }
}

struct UiStyleTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiStyleTool {
    fn name(&self) -> &'static str { "ui_style" }
    fn description(&self) -> String { "读取匹配元素的计算样式与盒模型(display/位置/尺寸/关键布局属性)。\
         用来判断「为什么它没显示出来」「为什么挤成一团」,比猜 CSS 快得多。只读。".into() }
    fn input_schema(&self) -> serde_json::Value { serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap() }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        let input: UiProbeInput = match serde_json::from_value(input) { Ok(value) => value, Err(e) => return kanzei_harness::ToolOutput::error(format!("invalid input: {e}")) };
        let Some(selector) = input.selector.as_deref().filter(|s| !s.trim().is_empty()) else { return kanzei_harness::ToolOutput::error("需要 selector"); };
        match ui_probe("style", selector).await { Ok(value) => kanzei_harness::ToolOutput::ok(value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())), Err(e) => kanzei_harness::ToolOutput::error(e) }
    }
}

pub(crate) struct FrontendToolsComponent;
impl kanzei_harness::Component for FrontendToolsComponent {
    fn contribute(&self, draft: &mut kanzei_harness::HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        draft.tools.insert("ui_dom", Arc::new(UiDomTool));
        draft.tools.insert("ui_console", Arc::new(UiConsoleTool));
        draft.tools.insert("ui_style", Arc::new(UiStyleTool));
        draft.tools.insert("frontend_locate", Arc::new(kanzei_tools::frontend::FrontendLocateTool));
        draft.tools.insert("frontend_check", Arc::new(kanzei_tools::frontend::FrontendCheckTool));
        for name in ["ui_dom", "ui_console", "ui_style", "frontend_locate", "frontend_check"] { draft.permissions.push(kanzei_harness::rule(name, "*", kanzei_harness::Effect::Allow)); }
        Ok(())
    }
}

pub(crate) struct QuickCaptureComponent { pub(crate) capture: &'static str }
impl kanzei_harness::Component for QuickCaptureComponent {
    fn contribute(&self, draft: &mut kanzei_harness::HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
        let tool = if self.capture == "defect" { kanzei_tools::tracker::TrackerTool { tool_name: "defect", noun: "defect", kind: &DEFECTS, requires_refs: None } } else { kanzei_tools::tracker::TrackerTool { tool_name: "req", noun: "requirement", kind: &REQUIREMENTS, requires_refs: None } };
        let name = tool.tool_name;
        draft.tools.insert(name, Arc::new(tool));
        draft.permissions.push(kanzei_harness::rule(name, "*", kanzei_harness::Effect::Allow));
        Ok(())
    }
}
