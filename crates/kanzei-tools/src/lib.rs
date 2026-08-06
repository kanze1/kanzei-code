//! kanzei-tools: 内置工具 + 双模式 profile 组件。

mod base;
mod bash;
pub mod docstore;
mod edit;
mod glob;
mod grep;
mod webfetch;
mod websearch;
mod read;
mod shell;
pub mod tracker;
mod write;

pub mod profiles;
pub mod subagent;

pub use base::BaseComponent;
pub use profiles::{DevProfile, ResearchProfile};
pub use subagent::{explore_agent, SubagentBase};
pub use shell::detected_shell;

use kanzei_harness::Tool;

/// 工具输入解析的公共入口:serde 失败时返回纠错反馈而不是崩溃。
pub(crate) fn parse_input<T: serde::de::DeserializeOwned>(
    tool: &dyn Tool,
    input: serde_json::Value,
) -> Result<T, kanzei_harness::ToolOutput> {
    let raw = input.to_string();
    serde_json::from_value(input)
        .map_err(|e| kanzei_harness::tool::repair_hint(tool, &raw, &e.to_string()))
}
