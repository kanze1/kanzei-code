//! kanzei-tools: 内置工具。M0:read / write / bash。

mod bash;
mod read;
mod shell;
mod write;

pub use shell::detected_shell;

use kanzei_harness::Tool;

pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(read::ReadTool),
        Box::new(write::WriteTool),
        Box::new(bash::BashTool),
    ]
}

/// 工具输入解析的公共入口:serde 失败时返回纠错反馈而不是崩溃。
pub(crate) fn parse_input<T: serde::de::DeserializeOwned>(
    tool: &dyn Tool,
    input: serde_json::Value,
) -> Result<T, kanzei_harness::ToolOutput> {
    let raw = input.to_string();
    serde_json::from_value(input)
        .map_err(|e| kanzei_harness::tool::repair_hint(tool, &raw, &e.to_string()))
}
