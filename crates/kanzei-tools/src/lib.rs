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
pub mod palette;
mod plot_tool;
mod process;
pub mod quarantine;
mod question;
mod read;
pub mod research_index;
pub mod research_loop;
pub mod research_plan;
pub mod research_verify;
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

/// D-393:latex/plot 等写盘工具的 workdir 路径边界校验。
///
/// 输入 workdir 必须是**相对路径**(绝对路径直接拒绝,防 `cwd.join` 替换基底)、
/// 不含 `..` 段(防穿越);canonicalize 后必须落在**研究工件目录**白名单
/// (`<cwd>/.kanzei/research` 或 `<cwd>/research` 子树)内——R-273/R-274 条目
/// 边界「限研究工件目录与显式指定目录」此前只存在于 schema 描述文本,这里落码。
///
/// 返回 canonicalize 后的路径(后续写盘基于它,白名单边界生效)。
pub(crate) fn resolve_research_workdir(
    cwd: &std::path::Path,
    workdir: &str,
) -> Result<std::path::PathBuf, String> {
    let workdir = workdir.trim();
    if workdir.is_empty() {
        return Err("workdir 不能为空".into());
    }
    let raw = std::path::Path::new(workdir);
    // 绝对路径拒绝:Windows 盘符/UNC 与 POSIX 根会让 join 替换基底;
    // has_root 兜底 Windows 的 root-relative(`/etc`、`\etc` 无盘符前缀也替换基底)。
    if raw.is_absolute() || raw.has_root() || workdir.contains(':') {
        return Err(format!(
            "workdir 必须是相对路径(绝对/根路径会让 join 替换项目基底): {workdir:?}"
        ));
    }
    // `..` 穿越拒绝(任意层级)。
    if workdir.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(format!("workdir 不得含 `..` 路径段(防穿越): {workdir:?}"));
    }
    let joined = cwd.join(workdir);
    let canonical = joined
        .canonicalize()
        .map_err(|e| format!("工作目录不存在或不可访问 {}: {e}", joined.display()))?;
    let cwd_canon = cwd
        .canonicalize()
        .map_err(|e| format!("cwd 不可解析: {e}"))?;
    let research_root = cwd_canon.join(".kanzei").join("research");
    let research_root_alt = cwd_canon.join("research");
    if !canonical.starts_with(&research_root) && !canonical.starts_with(&research_root_alt) {
        return Err(format!(
            "workdir 必须在研究工件目录内: {workdir:?} 解析为 {};\
             允许范围: {} 或 {}。\
             研究产物的 tex/spec/图统一放研究工件目录;确需其它目录请让用户手动处理。",
            canonical.display(),
            research_root.display(),
            research_root_alt.display()
        ));
    }
    Ok(canonical)
}

/// D-398:写者工具统一记写日志(路径+写后指纹+身份)——围栏收口对账的归因凭据。
/// 专用写者(写者工具)成功落盘后调用;先写文档再记日志(「写后」凭据,
/// write_log 模块头契约)。所有专用写者必须接线:test_record/conventions/
/// architecture/tracker 活动+归档——半上线(部分写者有凭据、部分没有)比不接
/// 线更危险:无凭据的合法写者会被围栏当越界回滚。
pub(crate) fn record_write_log(
    ctx: &kanzei_harness::ToolCtx,
    rel_path: &str,
    abs_path: &std::path::Path,
) {
    if let Ok(content) = std::fs::read(abs_path) {
        // D-399:record 失败至少告警(模块契约「宁可失败不静默」)——日志丢失 =
        // 该次写入失去归因凭据,围栏收口会把它当越界,必须让调用方看到。
        if let Err(e) = crate::write_log::record(
            &ctx.project_root,
            &crate::write_log::WriteLogEntry {
                at_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or_default(),
                path: rel_path.replace('\\', "/"),
                fingerprint: crate::content_hash(&content),
                content: content.clone(),
                run_id: ctx.run_id.clone(),
                process_id: ctx.process_id.clone(),
            },
        ) {
            eprintln!("[write-log] record failed for {rel_path}: {e}");
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-research-workdir-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// D-393:研究工件目录(.kanzei/research 与 research)内的相对路径放行,返回 canonical。
    #[test]
    fn workdir白名单_研究目录内放行() {
        let root = temp_root("ok");
        std::fs::create_dir_all(root.join(".kanzei").join("research").join("topic-a")).unwrap();
        std::fs::create_dir_all(root.join("research")).unwrap();
        let p = resolve_research_workdir(&root, ".kanzei/research/topic-a").unwrap();
        assert!(p.is_absolute(), "返回 canonical 绝对路径: {}", p.display());
        assert!(
            p.ends_with(".kanzei\\research\\topic-a") || p.ends_with(".kanzei/research/topic-a")
        );
        let p2 = resolve_research_workdir(&root, "research").unwrap();
        assert!(p2.ends_with("research"));
        // 目录不存在 → 明确报错。
        let err = resolve_research_workdir(&root, ".kanzei/research/missing").unwrap_err();
        assert!(err.contains("不存在"), "{err}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// D-393:绝对路径 / `..` / 研究目录之外的相对路径一律拒绝(任意路径可写收口)。
    #[test]
    fn workdir白名单_绝对路径与穿越拒绝() {
        let root = temp_root("reject");
        std::fs::create_dir_all(root.join(".kanzei").join("research")).unwrap();
        // 绝对路径(Windows 盘符)。
        let abs = resolve_research_workdir(&root, "C:\\Users\\public").unwrap_err();
        assert!(abs.contains("相对路径"), "绝对路径拒绝: {abs}");
        // 绝对路径(POSIX 根)。
        let abs2 = resolve_research_workdir(&root, "/etc").unwrap_err();
        assert!(abs2.contains("相对路径"), "{abs2}");
        // `..` 穿越。
        let dotdot = resolve_research_workdir(&root, ".kanzei/research/../..").unwrap_err();
        assert!(dotdot.contains(".."), "穿越拒绝: {dotdot}");
        // 研究目录之外(cwd 自身)。
        let outside = resolve_research_workdir(&root, ".").unwrap_err();
        assert!(outside.contains("研究工件目录"), "目录外拒绝: {outside}");
        // 空 workdir。
        let empty = resolve_research_workdir(&root, "  ").unwrap_err();
        assert!(empty.contains("不能为空"), "{empty}");
        std::fs::remove_dir_all(&root).ok();
    }
}
