//! kanzei-tools: 内置工具 + 双模式 profile 组件。

pub mod architecture;
mod background;
mod base;
mod bash;
pub mod docstore;
pub mod memory;
mod edit;
pub mod frontend;
mod glob;
mod grep;
mod git;
mod todowrite;
mod webfetch;
mod websearch;
mod question;
mod process;
mod read;
mod shell;
pub mod test_record;
pub mod tracker;
mod write;

pub mod profiles;
pub mod subagent;

pub use base::BaseComponent;
pub use profiles::{frontend_inspection_guidance, prompt_tool_mentions, DevProfile, ResearchProfile};
pub use subagent::{explore_agent, SubagentBase};
pub use shell::detected_shell;
/// 运行停止时回收本项目的后台进程,避免留下孤儿 dev server(R-097)。
pub use background::kill_project as kill_background_processes;

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
