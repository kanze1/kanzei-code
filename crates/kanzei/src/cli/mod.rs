//! CLI 命令域(R-256 批3,由 kz main.rs 拆分)。
//!
//! 独立理由:CLI 是「命令分发 + 装配」的变更理由——`main_entry` 按子命令分发,
//! 共享 helper(取根/身份键/交互判定/run 参数解析)是所有命令的共同底座;
//! 各子命令(run/eval/tracker/work/config/worktree/lock/memory)各自成模块,
//! 加一条命令不必读懂另一条的装配(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):取根必须走 `main_project_root` 唯一通道(D-194 HOME 拦截、
//! R-182 显式主根);run 的 prompt 真源解析(--prompt-file 与位置参数互斥,R-238 ②)
//! 与 `parse_run_args` 的 flag 剥除都在这里,搬迁不改任何解析语义。

use std::path::Path;

pub mod artifacts;
pub mod config;
pub mod eval;
pub mod lock;
pub mod memory;
pub mod metrics;
pub mod quarantine;
pub mod run;
pub mod shadow;
pub mod tracker;
pub mod work;
pub mod worktree;

/// CLI 命令分发入口(原 main.rs main() 的 match)。
pub async fn main_entry(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        Some("--version" | "-V" | "version") => {
            println!(
                "kanzei {} ({})",
                env!("CARGO_PKG_VERSION"),
                option_env!("KANZEI_BUILD_INFO").unwrap_or("dev")
            );
            Ok(())
        }
        Some("-h" | "--help" | "help") => {
            usage();
            Ok(())
        }
        Some(arg) if arg.starts_with('-') => {
            usage();
            anyhow::bail!("未知参数: {arg}");
        }
        Some("req" | "defect" | "source" | "finding" | "idea" | "decision") => {
            tracker::tracker_cli(args).await
        }
        Some("work") => work::work_cli(&args[1..]).await,
        Some("worktree") => worktree::worktree_cli(&args[1..]).await,
        Some("lock") => lock::lock_cli(&args[1..]).await,
        Some("artifacts") => artifacts::artifacts_cli(&args[1..]).await,
        Some("config") => config::config_cli(&args[1..]),
        Some("metrics") => metrics::metrics_cli(&args[1..]).await,
        Some("memory") => memory::memory_cli(&args[1..]).await,
        Some("quarantine") => quarantine::quarantine_cli(&args[1..]).await,
        Some("shadow") => shadow::shadow_cli(&args[1..]).await,
        Some("replay-eval") => eval::replay_eval_cli(&args[1..]).await,
        Some("run") => run::run_cli(&args[1..]).await,
        Some(_) => run::run_cli(args).await,
        None => {
            usage();
            std::process::exit(2);
        }
    }
}

pub(crate) fn cli_exit_code(halted_by_user: bool) -> i32 {
    if halted_by_user {
        3
    } else {
        0
    }
}

pub(crate) fn usage_text() -> &'static str {
    "usage: kz run \"<prompt>\"\n\
       kz run --new \"<prompt>\"  # 丢弃当前会话上下文并从新会话开始\n\
       kz run --readonly \"<prompt>\"  # 只读档位:读/检索放行,写与命令硬拒绝\n\
       kz run --no-subagents \"<prompt>\"  # 关闭本次 CLI 运行的 task 子代理工具（默认开启）\n\
       kz replay-eval [--limit N]     # 六臂回放评估:历史 run.trace 提取 case,fake 档真调\n\
       kz work next [--requirement-first]  # 结构化取活裁决\n\
       kz work claim <id> [--reason <text>] # 原子占用 Requirement/Defect/Work Unit\n\
       kz work create-unit --requirement R-xxx --objective <text> --acceptance <text> [--scope <path>] [--depends-on R-xxx/Wn] [--verify-with <cmd>]\n\
       kz work checkpoint R-xxx/Wn --summary <text> --next-action <text> [--decision <text>] [--retrieval-ref <ref>]\n\
       kz work verify R-xxx/Wn; kz work evidence R-xxx/Wn --criterion <exact> --evidence <ref>; kz work complete R-xxx/Wn\n\
       kz work block|unblock|supersede R-xxx/Wn --reason <text>; kz work get-unit R-xxx/Wn; kz work list-units [--requirement R-xxx]\n\
       kz worktree create <name>            # 建线:原子认领+凭据回滚,桌面/CLI 同一实现(R-207)\n\
       kz worktree merge-preview <path>     # 合并前冲突预检(merge-tree),不执行合并(R-207)\n\
       kz worktree merge <path>              # 安全非快进合并:冲突则保持双方并返回诊断\n\
       kz lock status                       # 外部写入者可见性:主根/git 工作树改动/活跃线(R-181)\n\
       kz config schema                     # kanzei.toml 用户面配置参考:全部已知键+说明+默认值(R-220)\n\
       kz artifacts stats [--json]         # 只读查看 state.db/WAL/freelist/artifact/telemetry 占用(R-245)\n\
       kz artifacts plan --dry-run [--json] # 列出 artifact 引用图与无引用清理候选,不写盘(R-245 B3)\n\
       kz metrics [--top N]                 # 巨石度量 + 条目关闭收尾链滚动遥测(R-258/R-311)\n\
       kz memory repair-index               # 按 Markdown 真源显式修复项目 INDEX/FTS(D-568/R-308)\n\
       kz memory review-global              # 无模型执行一次真实 project/global memory review(R-308)\n\
       kz quarantine [--dry-run|--apply]    # 隔离取证按日期/类型盘点或清理(D-566)\n\
       kz shadow [--mismatches]             # 会话投影 shadow gate 统计:未知差异=0 达标判定(R-242)\n\
       kz <req|defect|source|finding> [list|get <id>|add <title>|close <id>]\n\
project-root: --project-root <path>  # 显式主根;worktree 里跑也照样落主根的 .kanzei\n\
project-root: KANZEI_PROJECT_ROOT=<path>  # 同上的环境变量形态;优先级 参数 > 环境变量 > 从 cwd 发现\n\
config: ~/.kanzei/kanzei.toml + <project>/.kanzei/kanzei.toml\n\
agent: dev(默认开发)、dev-pair(结伴开发)、research(只读研究)\n\
profile: KANZEI_PROFILE=dev|research|readonly；KANZEI_AGENT=dev|dev-pair|research|readonly\n\
model: KANZEI_MODEL=<role|provider:model>，例如 primary、fast、ollama:qwen3.5:4b\n\
proxy: KANZEI_PROXY=off|env|<proxy-url>\n"
}

pub(crate) fn usage() {
    eprint!("{}", usage_text());
}

/// `kz run` 的解析结果。
///
/// R-182:新增 `--project-root` 之后,开关不再全是布尔——带值的开关必须把
/// **flag 与它的值两个 token 都**从 prompt 里剥掉,否则路径会被当提示词发给模型。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RunArgs {
    pub(crate) new_session: bool,
    pub(crate) readonly: bool,
    pub(crate) project_root: Option<std::path::PathBuf>,
    pub(crate) prompt: String,
    /// R-183:非交互(无 TTY)allowlist,格式 `action:resource`,可重复。
    /// 仅在 permissions.non_interactive = "allow_listed" 时参与决策。
    pub(crate) allow: Vec<String>,
    /// R-238 ②:从 UTF-8 文件读取 prompt(大文本交付正门,不进命令行参数)。
    /// 与位置参数 prompt 互斥;可与 --new/--readonly/--allow 组合。
    pub(crate) prompt_file: Option<std::path::PathBuf>,
    /// 进程级子代理开关,默认开启；`--no-subagents` 时不注册 task。
    pub(crate) subagents_enabled: bool,
}

pub(crate) const PROJECT_ROOT_FLAG: &str = "--project-root";
pub(crate) const PROJECT_ROOT_ENV: &str = "KANZEI_PROJECT_ROOT";
pub(crate) const ALLOW_FLAG: &str = "--allow";
pub(crate) const PROMPT_FILE_FLAG: &str = "--prompt-file";

pub(crate) fn parse_run_args(args: &[String]) -> RunArgs {
    let new_session = args.iter().any(|arg| arg == "--new");
    let readonly = args.iter().any(|arg| arg == "--readonly");
    let subagents_enabled = !args.iter().any(|arg| arg == "--no-subagents");
    let mut project_root = None;
    let mut allow: Vec<String> = Vec::new();
    let mut prompt_file = None;
    let mut words: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--new" | "--readonly" | "--no-subagents" => {}
            PROJECT_ROOT_FLAG => {
                // 取值并连同 flag 一起吃掉;缺值时只吃 flag(后面的 resolve 会
                // 按发现式取根,不会把 "--project-root" 当提示词发出去)。
                // D-270 缺口④:与 KANZEI_PROJECT_ROOT(env 侧 trim)对齐,带首尾
                // 空格的路径经参数进来也先 trim——否则同一条 HOME 输入经两条入口
                // 给出的理由不一致(参数侧被空格破成「路径不存在」)。
                if let Some(value) = args.get(index + 1) {
                    project_root = Some(std::path::PathBuf::from(value.trim()));
                    index += 1;
                }
            }
            ALLOW_FLAG => {
                // R-183:非交互 allowlist 条目,格式 `action:resource`。缺值只吃
                // flag(解析侧静默跳过,不把 "--allow" 当提示词)。
                if let Some(value) = args.get(index + 1) {
                    allow.push(value.clone());
                    index += 1;
                }
            }
            PROMPT_FILE_FLAG => {
                // R-238 ②:大文本 prompt 从文件读(不进命令行参数,避开 Windows
                // 32767 上限)。缺值只吃 flag,互斥/报错在 run_cli 侧。
                if let Some(value) = args.get(index + 1) {
                    prompt_file = Some(std::path::PathBuf::from(value));
                    index += 1;
                }
            }
            _ => words.push(arg),
        }
        index += 1;
    }
    RunArgs {
        new_session,
        readonly,
        project_root,
        prompt: words.join(" "),
        allow,
        prompt_file,
        subagents_enabled,
    }
}

/// R-238 ②:解析 run 的 prompt 真源——`--prompt-file` 优先且与位置参数互斥。
///
/// 返回 (prompt, 错误文案)。互斥/缺文件/非 UTF-8 都在这里给出明确报错,不进
/// run_cli 的模型路径。抽成纯函数只为可测:run_cli 要真跑一整轮才走得到。
pub(crate) fn resolve_run_prompt(
    positional: &str,
    prompt_file: Option<&std::path::Path>,
) -> Result<String, String> {
    let Some(path) = prompt_file else {
        return Ok(positional.to_string());
    };
    if !positional.trim().is_empty() {
        return Err(format!(
            "--prompt-file 与位置参数互斥:已给位置 prompt,又指定了 `{}`;大文本走文件、小提示走参数,二选一。",
            path.display()
        ));
    }
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) => Err(format!(
            "无法读取 --prompt-file `{}`: {error}(需存在且为 UTF-8 编码)。",
            path.display()
        )),
    }
}

/// R-183:CLI 是否具备交互应答能力——stdin 必须是 TTY 才算可交互。
/// 管道/重定向/CI/后台(stdin 关闭)一律视为非交互,不靠"读到 EOF"倒推(验收⑤)。
pub(crate) fn interactive_stdin() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// R-183:`--allow action:resource` 条目解析。首个 `:` 前是 action,其余是 resource
/// (resource 可含 `:` 或通配)。非法条目跳过并点名,避免静默丢规则。
pub(crate) fn parse_allowlist(items: &[String]) -> Vec<(String, String)> {
    items
        .iter()
        .filter_map(|item| {
            let (action, resource) = item.split_once(':')?;
            let action = action.trim();
            let resource = resource.trim();
            if action.is_empty() || resource.is_empty() {
                eprintln!("\x1b[33m--allow 忽略非法条目: `{item}`(应为 action:resource)\x1b[0m");
                return None;
            }
            Some((action.to_string(), resource.to_string()))
        })
        .collect()
}

/// R-183:非交互下的权限决策(纯函数,无 I/O,可单测——验收②⑤)。
///
/// - `Deny` / `RulesOnly`:规则外一律拒绝(与现状一致;RulesOnly 语义 = 只认预授权规则)。
/// - `AllowListed`:规则外先查本次显式 allowlist,命中放行一次,否则拒绝。
///
/// 匹配复用 `resource_match_for_action`,与规则集同一把尺(bash 结构化 JSON 资源
/// 需写结构化或通配 pattern,见 permission.rs D-269 口径)。
pub(crate) fn non_interactive_decision(
    policy: kanzei_harness::config::NonInteractive,
    allowlist: &[(String, String)],
    action: &str,
    resource: &str,
) -> kanzei_core::AskReply {
    match policy {
        kanzei_harness::config::NonInteractive::Deny
        | kanzei_harness::config::NonInteractive::RulesOnly => kanzei_core::AskReply::Deny,
        kanzei_harness::config::NonInteractive::AllowListed => {
            let hit = allowlist.iter().any(|(a, pattern)| {
                a == action
                    && kanzei_harness::permission::resource_match_for_action(
                        action, pattern, resource,
                    )
            });
            if hit {
                kanzei_core::AskReply::AllowOnce
            } else {
                kanzei_core::AskReply::Deny
            }
        }
    }
}

/// 显式主根的**唯一**合成点:参数 > 环境变量 > (None = 交给发现式)。
///
/// `KANZEI_PROJECT_ROOT` trim 后非空才算设置——与既有的 KANZEI_PROFILE/AGENT/
/// MODEL/PROXY 同构,空串一律视为「没设」。
pub(crate) fn explicit_main_root(flag: Option<&Path>) -> Option<std::path::PathBuf> {
    explicit_main_root_from(flag, std::env::var(PROJECT_ROOT_ENV).ok())
}

pub(crate) fn explicit_main_root_from(
    flag: Option<&Path>,
    env: Option<String>,
) -> Option<std::path::PathBuf> {
    if let Some(flag) = flag {
        return Some(flag.to_path_buf());
    }
    env.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

/// D-194:HOME 不能当项目根。
///
/// `~/.kanzei` 是**全局**配置根(kanzei.toml、memory、app.json)。HOME 一旦成为项目根,
/// 项目级产物(state.db、project/ 追踪文件)就落进同一个目录和全局数据混在一起,而且
/// `project_memory_root(HOME)` 与 `global_memory_root()` 会是同一个目录——两个 scope
/// 的 INDEX.md/index.db/inbox.md 静默合流。D-189 已经堵住"子目录被吸上去";在 HOME 里
/// 直接开跑这条路要在入口拦:它是误撞(忘了 cd),不是用户的选择,宁可拒绝也不要静默
/// 写脏全局目录。本机 `~/.kanzei/project/defects.md` 就是这么留下的。
pub(crate) fn reject_home_as_project_root(project_root: &Path) -> anyhow::Result<()> {
    if !kanzei_harness::config::is_home_root(project_root) {
        return Ok(());
    }
    anyhow::bail!(
        "项目根解析成了全局配置根(HOME 或 KANZEI_HOME:{}):项目数据落进去会和\
         全局配置、全局记忆混在一起。\n\
         先 cd 到具体项目目录再跑;确实想把某个目录当项目,就在它下面 mkdir .kanzei。",
        project_root.display()
    );
}

/// CLI 三条入口(run / replay-eval / tracker)取主根的**唯一**通道。
///
/// 收成一个函数是为了让「显式入口必须过同一道 HOME 拦截」由**结构**保证,
/// 而不是靠三处各自记得调一次(D-194/D-189/D-186:`KANZEI_PROJECT_ROOT=%USERPROFILE%`
/// 这类误设会把项目产物写进全局配置根)。
///
/// 拦截调两次是有意的:
/// - 第一次打在**显式输入**上。它是纯路径比较、不看磁盘,所以哪怕 HOME 下既没有
///   `.kanzei` 也没有 `.git`,「你把主根写成 HOME 了」也一定会被点名,而不会被
///   「这看着不像项目根」的泛化报错盖过去。
/// - 第二次打在**解析结果**上,覆盖发现式那一路(今天就有的那条)。
pub(crate) fn main_project_root(
    explicit: Option<&Path>,
    cwd: &Path,
) -> anyhow::Result<std::path::PathBuf> {
    if let Some(explicit) = explicit {
        reject_home_as_project_root(explicit)?;
    }
    let project_root = kanzei_harness::config::resolve_project_root(explicit, cwd)?;
    reject_home_as_project_root(&project_root)?;
    Ok(project_root)
}

/// CLI 的两把执行身份键(R-182 内容④ / 验收⑤)。
///
/// - **工具级并发锁键 = 代码树(cwd)**。同一项目的 N 棵 worktree 各跑各的,
///   共用一把锁会让它们的写工具互相串死;
/// - **跨进程写仲裁键 = 主根**。主根 `.kanzei` 的 tracker/记忆是所有线唯一的
///   共享写点,键一旦随树分裂,跨进程单写仲裁就被绕过。
///
/// 改前两参都传 `project_root`,注释里还写着「CLI 是单工作树,代码树即项目根,
/// 两把键同源」——`--project-root` / `KANZEI_PROJECT_ROOT` 落地之后那句话就不
/// 成立了。抽成纯函数只为可测:`run_cli` 要真跑一整轮才走得到那一行。
pub(crate) fn cli_identity_keys(cwd: &Path, project_root: &Path) -> (String, String) {
    (
        cwd.display().to_string(),
        project_root.display().to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::lock::lock_status_report;
    use super::memory::persist_always_allow;
    use super::tracker::parse_tracker_flags;
    use super::{
        cli_exit_code, cli_identity_keys, explicit_main_root, explicit_main_root_from,
        main_project_root, non_interactive_decision, parse_allowlist, parse_run_args,
        resolve_run_prompt, usage_text, RunArgs, PROJECT_ROOT_ENV,
    };
    use kanzei_core::AskReply;
    use std::path::{Path, PathBuf};

    /// D-359:`--reason` 是公共 flag,reopen/void_id/fix_terminal 共用一处解析。
    /// 这三个动作都**强制必填** reason,而解析原先只写在 fix_terminal 分支里,
    /// 于是 `kz req reopen R-183 --reason "..."` 永远得到 "`reason` is required"。
    #[test]
    fn reason_是公共flag_不再被当成位置参数() {
        let args: Vec<String> = ["R-183", "--reason", "让位对象已归档,退回队列重新取活"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut input = serde_json::json!({ "action": "reopen" });
        let positional = parse_tracker_flags(&args, &mut input);
        assert_eq!(input["reason"], "让位对象已归档,退回队列重新取活");
        assert_eq!(
            positional,
            vec!["R-183"],
            "--reason 及其取值不得混进位置参数(否则 update/close 会把它当成 status)"
        );

        // 不给 --reason 时不得凭空造一个:必填校验留在动作层,CLI 只负责传达。
        let bare: Vec<String> = ["R-183"].iter().map(|s| s.to_string()).collect();
        let mut bare_input = serde_json::json!({ "action": "reopen" });
        parse_tracker_flags(&bare, &mut bare_input);
        assert_eq!(bare_input["reason"], serde_json::Value::Null);

        // fix_terminal 的 id/status 位置参数不因 --reason 插在中间而错位。
        let mixed: Vec<String> = ["D-172", "--reason", "归档终态纠错", "fixed"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut mixed_input = serde_json::json!({ "action": "fix_terminal" });
        let mixed_positional = parse_tracker_flags(&mixed, &mut mixed_input);
        assert_eq!(mixed_positional, vec!["D-172", "fixed"]);
        assert_eq!(mixed_input["reason"], "归档终态纠错");
    }

    /// 登记开关解析:add 与 update 共用一套,位置参数语义不变。
    /// 没有这套开关时 `kz defect add` 一律被 R-191 B3 的登记门禁拒掉,
    /// 而 update 写不了字段就意味着 CLI 走不到关闭(§1.25 要求 close 前写证据)。
    #[test]
    fn 登记开关解析_字段与位置参数各归各位() {
        let args: Vec<String> = [
            "标题前半",
            "--severity",
            "medium",
            "标题后半",
            "-p",
            "P2",
            "--tag",
            "核心",
            "--complexity",
            "中",
            "--ref",
            "R-221",
            "--ref",
            "D-570",
            "--prior-art",
            ".kanzei/research/r248-prior-art/prior-art.md",
            "--field",
            "复现=第一步=点开设置页",
            "-f",
            "验收=有测试",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut input = serde_json::json!({ "action": "add" });
        let positional = parse_tracker_flags(&args, &mut input);
        assert_eq!(positional, vec!["标题前半", "标题后半"]);
        assert_eq!(input["severity"], "medium");
        assert_eq!(input["priority"], "P2");
        assert_eq!(input["fields"]["标签"], "核心");
        assert_eq!(input["fields"]["复杂度"], "中");
        assert_eq!(input["refs"], serde_json::json!(["R-221", "D-570"]));
        assert_eq!(
            input["prior_art"],
            ".kanzei/research/r248-prior-art/prior-art.md"
        );
        assert_eq!(input["验收"], serde_json::Value::Null);
        assert_eq!(input["fields"]["验收"], "有测试");
        // 值里带等号只按第一个切,后面的等号原样留在值里。
        assert_eq!(input["fields"]["复现"], "第一步=点开设置页");

        // update 路径:位置参数是 id 与 status,字段照样能写(含 进展)。
        let args: Vec<String> = ["R-191", "doing", "--field", "进展=解除阻塞"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut input = serde_json::json!({ "action": "update" });
        let positional = parse_tracker_flags(&args, &mut input);
        assert_eq!(positional, vec!["R-191", "doing"]);
        assert_eq!(input["fields"]["进展"], "解除阻塞");

        // 无字段开关时不产出空的 fields 键(免得覆盖既有字段的语义被改变)。
        let args: Vec<String> = ["D-1", "fixed"].iter().map(|s| s.to_string()).collect();
        let mut input = serde_json::json!({ "action": "close" });
        parse_tracker_flags(&args, &mut input);
        assert_eq!(input["fields"], serde_json::Value::Null);
    }

    fn run_args(new_session: bool, readonly: bool, prompt: &str) -> RunArgs {
        RunArgs {
            new_session,
            readonly,
            project_root: None,
            prompt: prompt.to_string(),
            allow: Vec::new(),
            prompt_file: None,
            subagents_enabled: true,
        }
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    fn unique(name: &str) -> String {
        format!(
            "{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    /// R-182 验收⑤:从 worktree 跑 `kz` 时两把键必须分叉。
    ///
    /// 工具级并发锁键跟着代码树走(N 棵树互不串死),写仲裁键钉在主根
    /// (主根 `.kanzei` 是所有线唯一的共享写点)。主树运行时两者同值,
    /// 与改前逐字节相同。
    #[test]
    fn cli双键在worktree下必须分叉_主树下仍同源() {
        let main_root = Path::new("C:/proj/kanzei");
        let worktree = Path::new("C:/proj/.kanzei-worktree-kanzei.f7");

        let (worktree_key, write_key) = cli_identity_keys(worktree, main_root);
        assert_eq!(worktree_key, worktree.display().to_string());
        assert_eq!(write_key, main_root.display().to_string());
        assert_ne!(
            worktree_key, write_key,
            "worktree 里跑时两把键必须不同,否则同项目 N 棵树共用一把工具锁互相串死"
        );

        // 主树:cwd == 主根,两把键同值,行为与改前一致。
        let (worktree_key, write_key) = cli_identity_keys(main_root, main_root);
        assert_eq!(worktree_key, write_key);
    }

    #[test]
    fn usage_lists_agent_profile_and_model_selection() {
        let usage = usage_text();
        assert!(usage.contains("dev-pair"));
        assert!(usage.contains("KANZEI_PROFILE=dev|research"));
        assert!(usage.contains("KANZEI_MODEL=<role|provider:model>"));
        assert!(usage.contains("ollama:qwen3.5:4b"));
    }

    #[test]
    fn usage_lists_readonly_mode() {
        let usage = usage_text();
        assert!(usage.contains("--readonly"));
        assert!(usage.contains("--no-subagents"));
        assert!(usage.contains("KANZEI_PROFILE=dev|research|readonly"));
        assert!(usage.contains("KANZEI_AGENT=dev|dev-pair|research|readonly"));
    }

    #[test]
    fn usage_lists_explicit_project_root() {
        let usage = usage_text();
        assert!(usage.contains("--project-root"));
        assert!(usage.contains("KANZEI_PROJECT_ROOT"));
    }

    #[test]
    fn readonly_flag_is_parsed_and_stripped_from_prompt() {
        let args = strings(&["--readonly", "分析", "代码"]);
        assert_eq!(parse_run_args(&args), run_args(false, true, "分析 代码"));
    }
    #[test]
    fn no_subagents_flag_is_parsed_and_stripped_from_prompt() {
        let args = strings(&["--no-subagents", "只做", "实现"]);
        let parsed = parse_run_args(&args);
        assert!(!parsed.subagents_enabled);
        assert_eq!(parsed.prompt, "只做 实现");
    }

    #[test]
    fn halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero() {
        assert_eq!(cli_exit_code(true), 3);
        assert_eq!(cli_exit_code(false), 0);
    }
    #[test]
    fn run_new_flag_is_removed_from_prompt() {
        let args = strings(&["--new", "开始", "新会话"]);
        assert_eq!(parse_run_args(&args), run_args(true, false, "开始 新会话"));
    }

    /// 带值开关最常漏的一步:只剥 flag、把值留在提示词里,于是路径被当成提示词发给模型。
    #[test]
    fn project_root_flag_and_value_are_stripped_from_prompt() {
        let args = strings(&["--project-root", "C:/x", "hello", "world"]);
        let parsed = parse_run_args(&args);
        assert_eq!(parsed.prompt, "hello world");
        assert_eq!(parsed.project_root, Some(PathBuf::from("C:/x")));
        assert!(!parsed.new_session && !parsed.readonly);

        // 与其它开关混用、且不在首位时同样成立。
        let args = strings(&["--new", "写", "--project-root", "C:/x", "测试"]);
        let parsed = parse_run_args(&args);
        assert_eq!(parsed.prompt, "写 测试");
        assert_eq!(parsed.project_root, Some(PathBuf::from("C:/x")));
        assert!(parsed.new_session);

        // 缺值时也不能把开关本身当提示词发出去。
        let args = strings(&["改代码", "--project-root"]);
        let parsed = parse_run_args(&args);
        assert_eq!(parsed.prompt, "改代码");
        assert_eq!(parsed.project_root, None);
    }

    /// D-270 缺口④:两条入口对同一输入给同一条理由。`--project-root` 值带首尾空格
    /// 时也必须 trim(与 `KANZEI_PROJECT_ROOT` env 侧对齐),否则带空格的 HOME 经参数
    /// 进来会被报成「路径不存在」而不是「主根写成 HOME」。
    #[test]
    fn project_root_flag_trims_whitespace_like_env_does() {
        let args = strings(&["--project-root", "  C:/x  ", "hello"]);
        let parsed = parse_run_args(&args);
        assert_eq!(
            parsed.project_root,
            Some(PathBuf::from("C:/x")),
            "参数侧必须 trim 首尾空格,与 KANZEI_PROJECT_ROOT(env 侧 trim)一致"
        );
        assert_eq!(parsed.prompt, "hello");
    }

    /// 优先级定死:参数 > 环境变量 > 发现式(None 表示交给发现式)。
    /// `KANZEI_PROJECT_ROOT` 是进程级状态,而测试同进程并发跑:真读环境变量的用例
    /// 必须互斥,否则两条用例互相看见对方设的值,红绿都不可信。
    static PROJECT_ROOT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn explicit_main_root_prefers_flag_over_env() {
        let _guard = PROJECT_ROOT_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let flag = PathBuf::from("C:/flag-root");
        let env = Some("C:/env-root".to_string());
        assert_eq!(
            explicit_main_root_from(Some(&flag), env.clone()),
            Some(flag.clone())
        );
        assert_eq!(
            explicit_main_root_from(None, env),
            Some(PathBuf::from("C:/env-root"))
        );
        assert_eq!(explicit_main_root_from(None, None), None);
        // trim 后为空 = 没设,不是"设成了空路径"。
        assert_eq!(explicit_main_root_from(None, Some("   ".into())), None);

        // 真正读的是 KANZEI_PROJECT_ROOT 这个键(键名写错就没人发现)。
        std::env::set_var(PROJECT_ROOT_ENV, "C:/env-root");
        assert_eq!(
            explicit_main_root(None),
            Some(PathBuf::from("C:/env-root")),
            "环境变量键名必须是 {PROJECT_ROOT_ENV}"
        );
        assert_eq!(explicit_main_root(Some(&flag)), Some(flag));
        std::env::remove_var(PROJECT_ROOT_ENV);
    }

    /// 本机被 `is_home_root` 认成 HOME 的那个路径;拿不到就是环境异常,直接失败。
    fn real_home() -> PathBuf {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(parent) = kanzei_harness::home::kanzei_home()
            .as_deref()
            .and_then(Path::parent)
        {
            candidates.push(parent.to_path_buf());
        }
        for key in ["USERPROFILE", "HOME"] {
            if let Ok(value) = std::env::var(key) {
                candidates.push(PathBuf::from(value));
            }
        }
        candidates
            .into_iter()
            .find(|c| kanzei_harness::config::is_home_root(c))
            .expect("测试环境必须能解析出 HOME")
    }

    /// D-194 红线:新入口(--project-root / KANZEI_PROJECT_ROOT)不得绕过 HOME 拦截。
    /// `KANZEI_PROJECT_ROOT=%USERPROFILE%` 这类误设会把项目产物写进全局配置根。
    #[test]
    fn 显式主根同样过home拦截() {
        let home = real_home();
        let cwd = std::env::temp_dir();
        let error = main_project_root(Some(&home), &cwd)
            .expect_err("HOME 当主根必须被拒")
            .to_string();
        assert!(
            error.contains("全局配置根"),
            "必须是 D-194 那条拦截,而不是别的报错: {error}"
        );

        // 大小写/尾分隔符/正斜杠/`\\?\` 前缀等写法一样拦得住(dir_key 归一)。
        let text = home.display().to_string();
        let mut variants = vec![PathBuf::from(format!(
            "{text}{}",
            std::path::MAIN_SEPARATOR
        ))];
        #[cfg(windows)]
        variants.extend([
            PathBuf::from(text.to_lowercase()),
            PathBuf::from(text.replace('\\', "/")),
            PathBuf::from(format!(r"\\?\{text}")),
        ]);
        for variant in variants {
            let error = main_project_root(Some(&variant), &cwd)
                .expect_err("HOME 的等价写法必须被拒")
                .to_string();
            assert!(
                error.contains("全局配置根"),
                "{} 必须撞 D-194 那条拦截: {error}",
                variant.display()
            );
        }

        // 对照组:普通目录不受影响。
        let ok_root = std::env::temp_dir().join(format!(
            "kanzei-r182-home-gate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(ok_root.join(".kanzei")).unwrap();
        assert_eq!(main_project_root(Some(&ok_root), &cwd).unwrap(), ok_root);
        std::fs::remove_dir_all(ok_root).unwrap();
    }

    /// 含 `.` / `..` 的 HOME 写法必须被拦。
    ///
    /// 这是 D-194 的一条真洞,实测过:`KANZEI_PROJECT_ROOT=C:\Users\kanzei` 退出码 1
    /// 被拦,而 `C:\Users\kanzei\.` 与 `C:\Users\kanzei\Documents\..` 都退出码 0 一路跑通,
    /// project 级 state.db 被写进全局配置根 `~/.kanzei`。原因是 `dir_key` 不折叠 `.`/`..`,
    /// 而 `resolve_project_root` 的标记校验对这些写法照样成立(HOME 下有 `.kanzei`)——
    /// 两道拦截同时静默通过。
    ///
    /// 洞是 R-182 的显式主根入口打开的:在那之前根恒来自 `current_dir()`,写不出这种串。
    /// 所以这里**两条入口各测一遍**:参数与环境变量必须撞同一道拦截。
    #[test]
    fn 显式主根含点段一样过home拦截() {
        let home = real_home();
        let cwd = std::env::temp_dir();
        let sep = std::path::MAIN_SEPARATOR;
        let text = home.display().to_string();
        let mut forms = vec![
            format!("{text}{sep}."),
            format!("{text}{sep}Documents{sep}.."),
            format!("{text}{sep}.{sep}"),
            format!("{text}{sep}a{sep}..{sep}.{sep}b{sep}.."),
        ];
        #[cfg(windows)]
        {
            let slash = text.replace('\\', "/");
            forms.push(format!("{slash}/./"));
            forms.push(format!("{slash}/Documents/.."));
        }

        for form in forms {
            // 入口一:`--project-root` 参数。
            let flag = PathBuf::from(&form);
            let explicit =
                explicit_main_root_from(Some(&flag), None).expect("参数入口必须产出显式根");
            let error = main_project_root(Some(&explicit), &cwd)
                .expect_err("--project-root 指向 HOME 必须被拒")
                .to_string();
            assert!(
                error.contains("全局配置根"),
                "--project-root {form} 必须撞 D-194 那条拦截: {error}"
            );

            // 入口二:`KANZEI_PROJECT_ROOT` 环境变量,真读进程环境走一遍。
            let explicit = {
                let _guard = PROJECT_ROOT_ENV_LOCK
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                std::env::set_var(PROJECT_ROOT_ENV, &form);
                let resolved = explicit_main_root(None);
                std::env::remove_var(PROJECT_ROOT_ENV);
                resolved
            }
            .expect("环境变量入口必须产出显式根");
            let error = main_project_root(Some(&explicit), &cwd)
                .expect_err("KANZEI_PROJECT_ROOT 指向 HOME 必须被拒")
                .to_string();
            assert!(
                error.contains("全局配置根"),
                "KANZEI_PROJECT_ROOT={form} 必须撞 D-194 那条拦截: {error}"
            );
        }

        // 对照组:名字里带 `.` 的**合法**目录不是 `.` 段,不许被误拦。
        let ok_root = std::env::temp_dir().join(format!(
            "kanzei-d194-dot-gate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dotted = ok_root.join("v1.0").join("app");
        std::fs::create_dir_all(dotted.join(".kanzei")).unwrap();
        assert_eq!(main_project_root(Some(&dotted), &cwd).unwrap(), dotted);
        std::fs::remove_dir_all(ok_root).unwrap();
    }

    #[test]
    fn persist_always_allow_returns_always_only_after_successful_write() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-cli-always-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        let result = persist_always_allow(&root, "bash", "git status").unwrap();
        assert_eq!(result, AskReply::AlwaysAllow);
        assert!(root.join(".kanzei/kanzei.toml").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persist_always_allow_does_not_grant_when_config_write_fails() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-cli-always-fail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::write(root.join(".kanzei/kanzei.toml"), "[invalid\n").unwrap();
        assert!(persist_always_allow(&root, "bash", "git status").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// R-181 降级验收:可见性报告必须含主根与工作树状态(clean 或改动清单),
    /// 且只读不阻塞——报告永远能产出,即使 state.db 不存在(降级路径)。
    #[test]
    fn lock_status_report_lists_root_worktree_and_degrades_gracefully() {
        let root = std::env::temp_dir().join(format!(
            "kz-lock-status-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let report = lock_status_report(&root, &root);
        assert!(
            report.iter().any(|line| line.contains("project-root:")),
            "报告必须含主根: {:?}",
            report
        );
        assert!(
            report.iter().any(|line| line.contains("cwd:")),
            "报告必须含 cwd: {:?}",
            report
        );
        // state.db 不存在时走降级文案,不 panic。
        assert!(
            report.iter().any(|line| line.contains("活跃线")),
            "报告必须含活跃线段(至少降级文案): {:?}",
            report
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// R-181 降级验收:在真实 git 仓库里,未提交改动必须出现在可见性报告里
    /// (外部写入者的痕迹对 `kz lock status` 可见),clean 时明确说 clean。
    #[test]
    fn lock_status_report_shows_uncommitted_changes_in_git_repo() {
        let root = std::env::temp_dir().join(format!(
            "kz-lock-git-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".git")).unwrap();
        // 没有 git 仓库时:报告要么报 clean 要么报不可用,但绝不 panic。
        let report = lock_status_report(&root, &root);
        let joined = report.join("\n");
        assert!(
            joined.contains("工作树") || joined.contains("git status"),
            "无仓库时应给工作树状态或 git 报错: {joined}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    // ══ R-183 非交互通道:三态策略决策与 allowlist 解析(验收②⑤)══

    #[test]
    fn non_interactive_deny_规则外一律拒绝() {
        let reply = non_interactive_decision(
            kanzei_harness::config::NonInteractive::Deny,
            &[],
            "bash",
            r#"{"command":"git status","workdir":"C:/p"}"#,
        );
        assert_eq!(
            reply,
            AskReply::Deny,
            "deny 档:任何 Ask 一律拒绝(现状,缺省)"
        );
    }

    #[test]
    fn non_interactive_rules_only_不查allowlist_直接拒绝() {
        // RulesOnly = 只认预授权规则,Ask 即拒;即使 allowlist 命中也不放行。
        let allowlist = vec![(
            "bash".to_string(),
            r#"{"command":"git status","workdir":"C:/p"}"#.to_string(),
        )];
        let reply = non_interactive_decision(
            kanzei_harness::config::NonInteractive::RulesOnly,
            &allowlist,
            "bash",
            r#"{"command":"git status","workdir":"C:/p"}"#,
        );
        assert_eq!(
            reply,
            AskReply::Deny,
            "rules_only 档:规则外拒绝,allowlist 不参与"
        );
    }

    #[test]
    fn non_interactive_allow_listed_命中allowlist放行() {
        // bash 结构化资源须以结构化 pattern 授权(与规则集同一把尺,permission.rs D-269)。
        let allowlist = vec![(
            "bash".to_string(),
            r#"{"command":"git status","workdir":"C:/p"}"#.to_string(),
        )];
        let reply = non_interactive_decision(
            kanzei_harness::config::NonInteractive::AllowListed,
            &allowlist,
            "bash",
            r#"{"command":"git status","workdir":"C:/p"}"#,
        );
        assert_eq!(
            reply,
            AskReply::AllowOnce,
            "allow_listed 档:规则外命中本次 allowlist 放行"
        );
    }

    #[test]
    fn non_interactive_allow_listed_未命中拒绝() {
        let allowlist = vec![(
            "bash".to_string(),
            r#"{"command":"git status","workdir":"C:/p"}"#.to_string(),
        )];
        let reply = non_interactive_decision(
            kanzei_harness::config::NonInteractive::AllowListed,
            &allowlist,
            "bash",
            r#"{"command":"cargo test","workdir":"C:/p"}"#,
        );
        assert_eq!(
            reply,
            AskReply::Deny,
            "allow_listed 档:规则外未命中 allowlist 拒绝"
        );
    }

    #[test]
    fn non_interactive_allow_listed_纯字符串pattern不授权结构化资源() {
        // 与规则集同口径:非结构化 pattern 不得授权结构化 bash 请求(D-269 防线)。
        let allowlist = vec![("bash".to_string(), "git *".to_string())];
        let reply = non_interactive_decision(
            kanzei_harness::config::NonInteractive::AllowListed,
            &allowlist,
            "bash",
            r#"{"command":"git status","workdir":"C:/p"}"#,
        );
        assert_eq!(reply, AskReply::Deny, "纯字符串 pattern 对结构化资源不命中");
    }

    #[test]
    fn non_interactive_allow_listed_action不匹配拒绝() {
        // 同一条 resource 规则,action 不同不得放行(allowlist 是 (action, resource) 对)。
        let allowlist = vec![("bash".to_string(), "git *".to_string())];
        let reply = non_interactive_decision(
            kanzei_harness::config::NonInteractive::AllowListed,
            &allowlist,
            "read",
            "git status",
        );
        assert_eq!(reply, AskReply::Deny);
    }

    #[test]
    fn parse_allowlist_解析与非法条目跳过() {
        let parsed = parse_allowlist(&[
            "bash:git status".to_string(),
            "bash:git commit -m \"x:y\"".to_string(),
            "非法条目".to_string(),
            "read:".to_string(),
            ":empty_action".to_string(),
        ]);
        assert_eq!(
            parsed,
            vec![
                ("bash".to_string(), "git status".to_string()),
                ("bash".to_string(), "git commit -m \"x:y\"".to_string()),
            ],
            "首个冒号切 action/resource;resource 可含冒号;空 action 或空 resource 跳过"
        );
    }

    #[test]
    fn parse_run_args_allow_flag_可重复收集() {
        let args: Vec<String> = [
            "--allow",
            "bash:git status",
            "--readonly",
            "--allow",
            "bash:cargo test",
            "跑测试",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let run = parse_run_args(&args);
        assert_eq!(run.allow, vec!["bash:git status", "bash:cargo test"]);
        assert_eq!(run.prompt, "跑测试");
        assert!(run.readonly);
    }

    // ══ R-238 ②:--prompt-file 大文本交付(验收②)══

    #[test]
    fn resolve_run_prompt_从文件读取大文本() {
        let dir = std::env::temp_dir().join(unique("kz-prompt-file"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prompt.txt");
        let big = "第一行任务说明\n".to_string() + &"内容".repeat(5000);
        std::fs::write(&path, &big).unwrap();
        let resolved = resolve_run_prompt("", Some(&path)).unwrap();
        assert_eq!(resolved, big, "大文本应从文件原样读出,不进命令行参数");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_run_prompt_与位置参数互斥() {
        let dir = std::env::temp_dir().join(unique("kz-prompt-mutex"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prompt.txt");
        std::fs::write(&path, "file content").unwrap();
        let err = resolve_run_prompt("位置 prompt", Some(&path)).unwrap_err();
        assert!(
            err.contains("互斥"),
            "同时给出位置参数与 --prompt-file 必须拒绝: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_run_prompt_文件不存在给出明确报错() {
        let missing = std::path::PathBuf::from("Z:/definitely/not/exists/prompt.txt");
        let err = resolve_run_prompt("", Some(&missing)).unwrap_err();
        assert!(
            err.contains("无法读取 --prompt-file") && err.contains("UTF-8"),
            "缺文件/非 UTF-8 都要有明确报错: {err}"
        );
    }

    #[test]
    fn parse_run_args_prompt_file_flag() {
        let args: Vec<String> = ["--prompt-file", "C:/tmp/big.txt", "--new", "--readonly"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let run = parse_run_args(&args);
        assert_eq!(
            run.prompt_file,
            Some(std::path::PathBuf::from("C:/tmp/big.txt"))
        );
        assert!(run.new_session && run.readonly);
        assert!(run.prompt.is_empty(), "文件入口下不应有位置 prompt");
    }
}
