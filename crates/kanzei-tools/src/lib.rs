//! kanzei-tools: 内置工具 + 双模式 profile 组件。

pub mod memory_consolidation;
pub mod memory_dream;
/// 原子写、文件锁和字节指纹由 kanzei-base 提供,各工具共用同一套实现。
pub use kanzei_base::atomic_file;
pub use kanzei_base::content_hash;
pub use kanzei_base::path_form;
pub use kanzei_base::write_log;

pub mod background;
mod base;
pub mod bash;
mod browser_tool;
pub mod sandbox;
/// 浏览器工具公开面（外部 Playwright 无头后端与用户已授权 CDP 连接）。
pub mod browser {
    pub use crate::browser_tool::{
        browser_error, connection_resources, current_url, execute_headless, form_action_expression,
        format_console_item, input_schema, out_click, out_console, out_dom, out_eval, out_form,
        out_open, out_press, out_screenshot, out_scroll, out_wait, owner_key, parse_browser_input,
        parse_viewport, permission_resources, prepare_source, record_rendered_source,
        rendered_source_expression, resolve_nav_target, resources_for, screenshot_scope,
        scroll_scope, set_current_url, url_resource, uses_connection, validate, viewport_label,
        wait_scope, Backend, BrowserAction, BrowserInput, ConsoleItem, NavTarget, SourceScope,
        CONSOLE_ALL_LIMIT, MAX_EVAL_CHARS, MAX_SCREENSHOT_BYTES, MAX_WAIT_MS,
    };
}
mod edit;
mod file_access;
pub mod files;
mod git;
mod glob;
mod grep;
mod preferences;
/// UI2-0926 #8:网页预览静态服务(127.0.0.1 随机端口 + token,登记根与内存片段)。
pub mod preview_server;
mod process;
/// UI2-0926 #13:项目状态事实(空项目/Git 三态/技术栈/工具链),agent 上下文与桌面端共用。
pub mod project_state;
mod public_http;
mod read;
#[cfg(test)]
mod read_receipt_tests;
/// 记忆知识图谱与引用抽取的纯函数(R-368 B1 子集;docs/design/memory_knowledge_graph.md)。
pub mod refgraph;
mod search_walk;
mod web_content;
pub use read::pdf_to_text;
pub mod image;
mod shell;
mod web_refs;
pub mod webfetch;

mod websearch;
/// R-207:worktree 生命周期内核(建线/回执/回滚/合并预检),桌面与 CLI 共用。
pub mod worktree;
mod write;

pub mod profiles;
pub mod subagent;

pub use background::kill_process as kill_background_processes_for_process;
/// 运行停止时回收本项目的后台进程,避免留下孤儿 dev server(R-097)。
pub use background::kill_project as kill_background_processes;
pub use base::BaseComponent;
pub use git::observe as git_observe;
/// R-177 内容③:线清单的真源是 `git worktree list --porcelain`。解析器不新造,
/// 从 `merge_ff` 已在用的那一个抽出来复用。
pub use git::{parse_worktree_list, WorktreeEntry};
pub use profiles::{
    DevelopmentProfile, GeneralChatProfile, GeneralChatScope, IsolatedProfile, ReadonlyProfile,
    DEVELOPMENT_DEFERRED_TOOLS,
};
pub use shell::{detected_shell, fresh_path, kill_tree};
pub use subagent::{explore_agent, plan_agent, writer_agent, SubagentBase, WritableSubagentBase};

use kanzei_harness::Tool;

/// 工具输入解析的公共入口:serde 失败时返回纠错反馈而不是崩溃。
/// ToolOutput 是统一的纠错回馈契约，此处保留完整错误值而不改变调用方错误语义。
#[allow(clippy::result_large_err)]
pub(crate) fn parse_input<T: serde::de::DeserializeOwned>(
    tool: &dyn Tool,
    input: serde_json::Value,
) -> Result<T, kanzei_harness::ToolOutput> {
    serde_json::from_value(input)
        .map_err(|e| kanzei_harness::tool::repair_hint(tool, &e.to_string()))
}

/// 为路径类工具生成可执行的缺失路径诊断：只扫描目标同目录的文件，避免把
/// 整个项目树噪声塞进错误结果；memory 路径额外给出项目真源根目录。
pub(crate) fn missing_path_hint(
    path: &std::path::Path,
    raw_path: &str,
    project_root: &std::path::Path,
) -> String {
    let mut message = format!("path not found: {}", path.display());
    let looks_like_memory = path.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("memory")
    }) || raw_path
        .split(['/', char::from(92)])
        .any(|part| part.eq_ignore_ascii_case("memory"));
    if looks_like_memory {
        message.push_str(&format!(
            "\n正确的项目 memory 根路径: {}",
            project_root.join(".kanzei").join("memory").display()
        ));
    }

    let Some(parent) = path.parent() else {
        return message;
    };
    let Some(target_name) = path.file_name().map(|name| name.to_string_lossy()) else {
        return message;
    };
    let mut candidates: Vec<(usize, String)> = std::fs::read_dir(parent)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_file() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            Some((path_name_distance(&target_name, &name), name))
        })
        .collect();
    candidates.sort();
    candidates.truncate(5);
    if candidates.is_empty() {
        message.push_str(&format!(
            "\n同目录最近邻文件候选: {} 中没有可列出的文件",
            parent.display()
        ));
    } else {
        message.push_str(&format!("\n同目录最近邻文件候选 ({}):", parent.display()));
        for (_, name) in candidates {
            message.push_str(&format!("\n  - {name}"));
        }
    }
    message
}

fn path_name_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.to_lowercase().chars().collect();
    let right: Vec<char> = right.to_lowercase().chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (row, left_char) in left.iter().enumerate() {
        let mut current = vec![row + 1; right.len() + 1];
        for (column, right_char) in right.iter().enumerate() {
            current[column + 1] = if left_char == right_char {
                previous[column]
            } else {
                1 + previous[column]
                    .min(previous[column + 1])
                    .min(current[column])
            };
        }
        previous = current;
    }
    previous[right.len()]
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
    let proxy = kanzei_harness::KanzeiConfig::load_at_root(&ctx.project_root)
        .ok()
        .and_then(|c| c.proxy);
    configured_proxy(proxy.as_deref())
}

pub(crate) fn configured_proxy(proxy: Option<&str>) -> kanzei_llm::proxy::ProxyConfig {
    use kanzei_llm::proxy::ProxyConfig;
    match proxy {
        Some("off") => ProxyConfig::Disabled,
        Some("env") => ProxyConfig::Env,
        Some(p) if !p.is_empty() => ProxyConfig::Explicit(p.into()),
        _ => ProxyConfig::Env,
    }
}

/// D-393:latex/plot 等写盘工具的 workdir 路径边界校验。
/// Windows 上禁止外部子进程新建控制台窗口(D-238)。
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
