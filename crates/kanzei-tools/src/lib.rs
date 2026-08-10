//! kanzei-tools: 内置工具 + 双模式 profile 组件。

pub mod architecture;
pub mod atomic_file;
mod background;
mod base;
mod bash;
pub mod docstore;
mod edit;
pub mod embed;
pub mod files;
pub mod frontend;
mod git;
pub mod git_batches;
mod glob;
mod grep;
pub mod memory;
mod process;
mod question;
mod read;
mod shell;
pub mod test_record;
mod todowrite;
pub mod tracker;
mod webfetch;
mod websearch;
mod write;

pub mod profiles;
pub mod replay_eval;
pub mod subagent;

/// 运行停止时回收本项目的后台进程,避免留下孤儿 dev server(R-097)。
pub use background::kill_project as kill_background_processes;
pub use base::BaseComponent;
pub use profiles::{
    frontend_inspection_guidance, prompt_tool_mentions, DevProfile, ReadonlyProfile,
    ResearchProfile,
};
pub use shell::detected_shell;
pub use subagent::{explore_agent, SubagentBase};

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

/// Windows 上禁止外部子进程新建控制台窗口(D-238)。
///
/// 桌面端是 GUI 进程(没有控制台可继承),不设 CREATE_NO_WINDOW 时,每次
/// spawn git/cargo/taskkill 等外部程序都会闪出一个黑色 cmd 窗口。std 与
/// tokio 两种 Command 各自有 creation_flags,统一收敛到这里,避免各处重复。
#[cfg(windows)]
pub(crate) fn hide_console(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn hide_console(_command: &mut std::process::Command) {}

#[cfg(windows)]
pub(crate) fn hide_console_async(command: &mut tokio::process::Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn hide_console_async(_command: &mut tokio::process::Command) {}
