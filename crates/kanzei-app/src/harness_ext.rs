//! 桌面界面检查、交付与快速记录组件；能力说明随各自工具提供。

use std::sync::Arc;

use crate::ui_probe;
use kanzei_harness::{ResolveCtx, ToolCtx};

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

pub(crate) const DESKTOP_DEFERRED_TOOLS: &[&str] = &[
    "ui_dom",
    "ui_console",
    "ui_style",
];

pub(crate) struct FrontendToolsComponent;
impl kanzei_harness::Component for FrontendToolsComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
        draft.tools.insert("ui_dom", Arc::new(UiDomTool));
        draft.tools.insert("ui_console", Arc::new(UiConsoleTool));
        draft.tools.insert("ui_style", Arc::new(UiStyleTool));
        for name in [
            "ui_dom",
            "ui_console",
            "ui_style",
        ] {
            draft.permissions.push(kanzei_harness::rule(
                name,
                "*",
                kanzei_harness::Effect::Allow,
            ));
        }
        if ctx.profile.is_development() {
            draft
                .deferred_tools
                .extend(DESKTOP_DEFERRED_TOOLS.iter().map(|name| (*name).to_owned()));
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

    #[test]
    fn 桌面与cli共用base_component的外部browser工具() {
        let desktop = crate::run::assembly::build_run_harness(ProfileKind::DevAuto);
        assert_eq!(browser_count(desktop), 1, "桌面复用唯一 browser 工具");

        let cli = kanzei_runtime::build_harness(ProfileKind::DevAuto, |_| {}, |_| {});
        assert_eq!(browser_count(cli), 1, "CLI 共用同一 browser 工具");

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
