//! kanzei-tools: 内置工具 + 双模式 profile 组件。

pub mod architecture;
pub mod memory_consolidation;
/// 原子写原语下沉到 kanzei-llm(依赖图最底层,D-261):llm 的 auth/store 与
/// tools 的 docstore/test_record/memory/files 共用同一套,仓里不再养第二份。
pub use kanzei_base::atomic_file;
pub use kanzei_base::content_hash;
pub use kanzei_base::write_log;
/// R-203:memory/、docstore、embed、replay_eval 拆入 kanzei-memory crate,经再导出
/// 保持 `kanzei_tools::{memory,docstore,embed,replay_eval}` 全部调用点零改动。
pub use kanzei_memory::docstore;
pub use kanzei_memory::embed;
pub use kanzei_memory::memory;
pub use kanzei_memory::replay_eval;
mod background;
mod base;
pub mod bash;
mod browser_tool;
pub mod conventions;
mod cross_tree;
mod edit;
pub mod files;
pub mod frontend;
mod git;
pub mod git_batches;
mod glob;
mod grep;
mod latex_tool;
mod managed;
mod plot_tool;
mod process;
mod question;
mod read;
pub mod research_loop;
pub mod research_plan;
pub mod research_write;
pub use read::pdf_to_text;
pub mod run;
mod shell;
pub mod test_record;
mod todowrite;
pub mod tracker;
/// D-413:研究工作台要在应用内打开文献正文,桌面命令直接复用本工具的抓取与
/// HTML→文本管线(不另造第二套抓取逻辑,免得代理/超时/截断口径分叉)。
pub mod webfetch;
mod websearch;
pub mod work;
/// R-207:worktree 生命周期内核(建线/回执/回滚/合并预检),桌面与 CLI 共用。
pub mod worktree;
mod write;

pub mod profiles;
pub mod subagent;
pub mod symbols;

pub use background::kill_process as kill_background_processes_for_process;
/// 运行停止时回收本项目的后台进程,避免留下孤儿 dev server(R-097)。
pub use background::kill_project as kill_background_processes;
pub use base::BaseComponent;
/// R-177 内容③:线清单的真源是 `git worktree list --porcelain`。解析器不新造,
/// 从 `merge_ff` 已在用的那一个抽出来复用。
pub use git::{parse_worktree_list, WorktreeEntry};
pub use profiles::{
    frontend_inspection_guidance, prompt_tool_mentions, DevProfile, ReadonlyProfile,
    ResearchProfile,
};
pub use shell::detected_shell;
pub use subagent::{explore_agent, writer_agent, SubagentBase, WritableSubagentBase};
pub use work::{
    active_claims_by_line, release_line_claims, resolve_work_decision, resolved_control_prompt,
    resolved_control_prompt_of, ResolvedControlState, WorkDecision, WorkTool,
};

use kanzei_harness::Tool;

/// 工具输入解析的公共入口:serde 失败时返回纠错反馈而不是崩溃。
/// ToolOutput 是统一的纠错回馈契约，此处保留完整错误值而不改变调用方错误语义。
#[allow(clippy::result_large_err)]
pub(crate) fn parse_input<T: serde::de::DeserializeOwned>(
    tool: &dyn Tool,
    input: serde_json::Value,
) -> Result<T, kanzei_harness::ToolOutput> {
    let raw = input.to_string();
    serde_json::from_value(input)
        .map_err(|e| kanzei_harness::tool::repair_hint(tool, &raw, &e.to_string()))
}

/// 联网工具的代理策略:与 LLM 请求同一套(配置驱动,loopback 豁免)。
///
/// **配置取 `ctx.project_root`,不取 `ctx.cwd`**(R-182 内容④)。代理是主根资产;
/// 线上线后 cwd 是 worktree,那里的 `.kanzei/kanzei.toml` 是被 git checkout 出来的
/// 分支副本,读它等于让「能不能联网」取决于这条线的分支停在哪一代。判据与 F6 同源:
/// 凡 `.kanzei/**` 资产走 project_root,凡仓库源码走 cwd。
///
/// webfetch 与 websearch 两处原本各写一遍同样的 match,一并收敛到这里。
pub(crate) fn tool_proxy(ctx: &kanzei_harness::ToolCtx) -> kanzei_llm::proxy::ProxyConfig {
    use kanzei_llm::proxy::ProxyConfig;
    match kanzei_harness::KanzeiConfig::load_at_root(&ctx.project_root)
        .ok()
        .and_then(|c| c.proxy)
    {
        Some(p) if p == "off" => ProxyConfig::Disabled,
        Some(p) if p == "env" => ProxyConfig::Env,
        Some(p) if !p.is_empty() => ProxyConfig::Explicit(p),
        _ => ProxyConfig::Env,
    }
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
