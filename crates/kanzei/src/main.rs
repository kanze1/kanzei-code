//! kz — kanzei CLI。
//! `kz run "<prompt>"`         跑 agent 循环(harness 装配)
//! `kz req|defect|source|finding <action> [...]`  人用直通:直接操作项目文档
//! 配置:~/.kanzei/kanzei.toml + 项目 .kanzei/kanzei.toml;env 快捷覆盖见 usage。

use std::io::Write as _;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_core::{run_once, RunEvent, RunnerConfig};
use kanzei_harness::{
    ConfigComponent, Harness, KanzeiConfig, MarkdownComponent, ProfileKind, ResolveCtx, Tool,
    ToolCtx,
};
use kanzei_llm::{LlmClient, ProxyConfig};
use kanzei_tools::{BaseComponent, DevProfile, ReadonlyProfile, ResearchProfile};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
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
        Some("req" | "defect" | "source" | "finding" | "goal" | "decision") => {
            tracker_cli(&args).await
        }
        Some("work") => work_cli(&args[1..]).await,
        Some("replay-eval") => replay_eval_cli(&args[1..]).await,
        Some("run") => run_cli(&args[1..]).await,
        Some(_) => run_cli(&args).await,
        None => {
            usage();
            std::process::exit(2);
        }
    }
}

fn cli_exit_code(halted_by_user: bool) -> i32 {
    if halted_by_user {
        3
    } else {
        0
    }
}

fn usage_text() -> &'static str {
    "usage: kz run \"<prompt>\"\n\
       kz run --new \"<prompt>\"  # 丢弃当前会话上下文并从新会话开始\n\
       kz run --readonly \"<prompt>\"  # 只读档位:读/检索放行,写与命令硬拒绝\n\
       kz replay-eval [--limit N]     # 六臂回放评估:历史 run.trace 提取 case,fake 档真调\n\
       kz work next [--requirement-first]  # 结构化取活裁决\n\
       kz work claim <id> [--reason <text>] # 原子占用 selected；覆盖时理由必填\n\
       kz <req|defect|source|finding> [list|get <id>|add <title>|close <id>]\n\
project-root: --project-root <path>  # 显式主根;worktree 里跑也照样落主根的 .kanzei\n\
project-root: KANZEI_PROJECT_ROOT=<path>  # 同上的环境变量形态;优先级 参数 > 环境变量 > 从 cwd 发现\n\
config: ~/.kanzei/kanzei.toml + <project>/.kanzei/kanzei.toml\n\
agent: dev(默认开发)、dev-pair(结伴开发)、research(只读研究)\n\
profile: KANZEI_PROFILE=dev|research|readonly；KANZEI_AGENT=dev|dev-pair|research|readonly\n\
model: KANZEI_MODEL=<role|provider:model>，例如 primary、fast、ollama:qwen3.5:4b\n\
proxy: KANZEI_PROXY=off|env|<proxy-url>\n"
}

fn usage() {
    eprint!("{}", usage_text());
}

/// `kz run` 的解析结果。
///
/// R-182:新增 `--project-root` 之后,开关不再全是布尔——带值的开关必须把
/// **flag 与它的值两个 token 都**从 prompt 里剥掉,否则路径会被当提示词发给模型。
#[derive(Debug, PartialEq, Eq)]
struct RunArgs {
    new_session: bool,
    readonly: bool,
    project_root: Option<std::path::PathBuf>,
    prompt: String,
}

const PROJECT_ROOT_FLAG: &str = "--project-root";
const PROJECT_ROOT_ENV: &str = "KANZEI_PROJECT_ROOT";

fn parse_run_args(args: &[String]) -> RunArgs {
    let new_session = args.iter().any(|arg| arg == "--new");
    let readonly = args.iter().any(|arg| arg == "--readonly");
    let mut project_root = None;
    let mut words: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--new" | "--readonly" => {}
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
            _ => words.push(arg),
        }
        index += 1;
    }
    RunArgs {
        new_session,
        readonly,
        project_root,
        prompt: words.join(" "),
    }
}

/// 显式主根的**唯一**合成点:参数 > 环境变量 > (None = 交给发现式)。
///
/// `KANZEI_PROJECT_ROOT` trim 后非空才算设置——与既有的 KANZEI_PROFILE/AGENT/
/// MODEL/PROXY 同构,空串一律视为「没设」。
fn explicit_main_root(flag: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    explicit_main_root_from(flag, std::env::var(PROJECT_ROOT_ENV).ok())
}

fn explicit_main_root_from(
    flag: Option<&std::path::Path>,
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
fn reject_home_as_project_root(project_root: &std::path::Path) -> anyhow::Result<()> {
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
fn main_project_root(
    explicit: Option<&std::path::Path>,
    cwd: &std::path::Path,
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
///成立了。抽成纯函数只为可测:`run_cli` 要真跑一整轮才走得到那一行。
fn cli_identity_keys(cwd: &std::path::Path, project_root: &std::path::Path) -> (String, String) {
    (
        cwd.display().to_string(),
        project_root.display().to_string(),
    )
}

async fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let RunArgs {
        new_session,
        readonly,
        project_root: root_flag,
        prompt,
    } = parse_run_args(args);
    if prompt.trim().is_empty() {
        usage();
        std::process::exit(2);
    }

    let cwd = std::env::current_dir()?;
    // R-182:取根必须在配置加载**之前**——配置本身就挂在主根下面,
    // 先按 cwd 加载再改根,worktree 里读到的会是被 checkout 出来的分支副本。
    let project_root =
        main_project_root(explicit_main_root(root_flag.as_deref()).as_deref(), &cwd)?;
    let (config, config_warnings) = KanzeiConfig::load_with_warnings_at_root(&project_root)?;
    let config = Arc::new(config);
    for warning in &config_warnings {
        eprintln!("\x1b[33m{warning}\x1b[0m");
    }
    for warning in config.bash_permission_warnings() {
        eprintln!("\x1b[33m{warning}\x1b[0m");
    }
    let profile: ProfileKind = match std::env::var("KANZEI_PROFILE") {
        Ok(_) if readonly => ProfileKind::Readonly,
        Ok(p) => p.parse().map_err(|e: String| anyhow::anyhow!(e))?,
        Err(_) if readonly => ProfileKind::Readonly,
        Err(_) => config.default_profile(),
    };
    let rctx = ResolveCtx {
        profile,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };

    // 装配顺序即覆盖顺序:内置 → profile → 用户 markdown → 用户 toml(用户永远最后、永远赢)。
    let mut harness = Harness::default();
    harness
        .add(BaseComponent)
        .add(DevProfile)
        .add(ResearchProfile)
        .add(ReadonlyProfile)
        .add(MarkdownComponent)
        .add(ConfigComponent);
    let snapshot = harness.resolve(&rctx)?;

    let mut agent = snapshot
        .select_agent(std::env::var("KANZEI_AGENT").ok().as_deref())?
        .clone();
    if profile == ProfileKind::Dev {
        agent
            .system
            .push_str(&kanzei_tools::resolved_control_prompt(
                &cwd,
                &project_root,
                kanzei_harness::auto_run::WorkPriority::DefectFirst,
            ));
    }

    // 模型:KANZEI_MODEL 覆盖 agent 定义(快速试模型用)。R-178 P2 五层链 ①②③:
    // CLI 无线/进程概念(② 恒 None),本轮直选 = KANZEI_MODEL → agent 默认;
    // ④⑤ 由 config.resolve_model 承担。与桌面共用同一真源。
    let model_ref = kanzei_harness::config::resolve_model_chain(
        std::env::var("KANZEI_MODEL").ok().as_deref(),
        None,
        &agent.model,
    );
    let resolved = config.resolve_model(&model_ref)?;

    let proxy = match std::env::var("KANZEI_PROXY")
        .ok()
        .or_else(|| config.proxy.clone())
    {
        Some(p) if p == "off" => ProxyConfig::Disabled,
        Some(p) if p == "env" => ProxyConfig::Env,
        Some(p) if !p.is_empty() => ProxyConfig::Explicit(p),
        _ => ProxyConfig::Env,
    };
    let route = kanzei_core::build_route(&resolved, &proxy).await?;

    let client = LlmClient::new(&proxy)?;
    // R-141:根在入口(上面 main_project_root)解析一次,这里显式传下去。
    // R-182:显式主根时 cwd 与 project_root **第一次可能不相等**(worktree 里
    // cwd 是那棵树、主根是 .kanzei 托管文档的真源),所以两者必须分别传。
    //
    let (worktree_key, write_key) = cli_identity_keys(&cwd, &project_root);
    let ctx = ToolCtx::new(cwd, project_root.clone())
        .with_work_priority(kanzei_harness::auto_run::WorkPriority::DefectFirst)
        .with_identity(
            worktree_key,
            write_key,
            format!(
                "cli_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default()
            ),
            "cli".into(),
        );
    let runner_config = RunnerConfig {
        model: resolved.model.clone(),
        max_tokens: config.limits.max_tokens(),
        reasoning: config
            .models
            .reasoning
            .as_deref()
            .map(kanzei_llm::ReasoningEffort::parse)
            .unwrap_or_default(),
        service_tier: config.service_tier_for(&resolved),
        // 轮内主动压缩的预算基准(D-176)。
        context_limit: resolved.provider.context_limit,
        limits: config.limits.clone(),
        // R-162 事件触发召回:工具失败瞬间注入相关记忆 Packet(验收⑤ CLI 侧)。
        recall: Some(Box::new(kanzei_tools::memory::FailureRecallPolicy::new(
            &ctx.project_root,
        ))),
        // R-171:CLI 单运行实例用默认策略;桌面端多进程场景才启用串行写。
        execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
        ask_policy: kanzei_core::AskPolicy::Interactive,
    };

    let session_id = kanzei_core::project_session_id(&ctx.project_root);
    let state_path = kanzei_core::project_state_path(&ctx.project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(&session_id, &ctx.project_root.display().to_string(), None)?;
    if new_session {
        let cleared = store.clear_conversation(&session_id)?;
        store.append_event(
            &session_id,
            "conversation.reset",
            &serde_json::json!({ "cleared": cleared }),
        )?;
    }
    let input_id = format!(
        "input_{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    store.admit_input(
        &session_id,
        &input_id,
        &prompt,
        kanzei_core::Delivery::Queue,
    )?;
    store.append_event(
        &session_id,
        "prompt.admitted",
        &serde_json::json!({ "input_id": input_id, "delivery": "queue" }),
    )?;
    let promoted = store
        .promote_next_queue(&session_id)?
        .ok_or_else(|| anyhow::anyhow!("无法提升已提交的 CLI 输入"))?;
    store.append_event(
        &session_id,
        "prompt.promoted",
        &serde_json::json!({ "input_id": promoted.input_id, "delivery": "queue" }),
    )?;
    // promoted → running:输入的生命周期必须有"开始执行"这一步,否则跑完的输入
    // 永远停在 promoted,以后任何一次停止都会把它追认为 cancelled(D-173)。
    store.start_input(&promoted.input_id)?;
    // 本轮身份与墙钟:episode 落库时要能回答"哪一轮、跑了多久、用的什么模型"。
    let run_id = format!(
        "run_{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let run_started = std::time::Instant::now();
    // 本轮开始墙钟毫秒:R-161 回填 recall_events 的 episode_id 用(开跑预检索
    // 先于 episode 落库,只能靠时间窗归因到本轮)。
    let run_epoch_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
    store.set_status(&session_id, "running")?;
    store.append_event(
        &session_id,
        "session.status_changed",
        &serde_json::json!({ "status": "running" }),
    )?;
    let prior = store
        .latest_event(&session_id, "conversation.updated")?
        .map(|event| {
            let messages = serde_json::from_value::<Vec<kanzei_llm::Message>>(
                event.payload.get("messages").cloned().unwrap_or_default(),
            )?;
            Ok::<_, anyhow::Error>(kanzei_core::filter_message_history(&messages))
        })
        .transpose()?
        .unwrap_or_default();
    drop(store);

    eprintln!(
        "\x1b[90mprofile {:?} · agent {} · model {}:{}\x1b[0m",
        profile, agent.name, resolved.provider_name, resolved.model
    );

    let mut stdout = std::io::stdout();
    let mut on_event = move |event: RunEvent| match event {
        RunEvent::TurnStart { step, max_steps } => {
            if step > 1 {
                let label = if max_steps > 0 {
                    format!("第 {step}/{max_steps} 轮")
                } else {
                    format!("第 {step} 轮")
                };
                let _ = writeln!(stdout, "\n\x1b[90m── {label} ──\x1b[0m");
            }
        }
        RunEvent::Text(text) => {
            let _ = write!(stdout, "{text}");
            let _ = stdout.flush();
        }
        RunEvent::Reasoning(_) => {}
        RunEvent::ToolStart { name, summary, .. } => {
            let _ = writeln!(stdout, "\n\x1b[36m● {name}\x1b[0m {summary}");
        }
        RunEvent::TaskProgress { text, .. } => {
            let _ = writeln!(stdout, "  \x1b[90m… {text}\x1b[0m");
        }
        // CLI 不逐段转印工具输出:ToolEnd 的预览已够,逐段会与正文流互相穿插。
        RunEvent::ToolProgress { .. } => {}
        RunEvent::Retry {
            attempt,
            max,
            delay_ms,
        } => {
            let _ = writeln!(
                stdout,
                "\x1b[33m重试 {attempt}/{max},等待 {delay_ms}ms\x1b[0m"
            );
        }
        RunEvent::StreamRestart {
            attempt,
            max,
            delay_ms,
        } => {
            let _ = writeln!(
                stdout,
                "\x1b[33m连接中断,重新请求本轮 {attempt}/{max},等待 {delay_ms}ms(本轮工具尚未执行,不会重复副作用)\x1b[0m"
            );
        }
        RunEvent::ToolEnd { ok, preview, .. } => {
            let mark = if ok {
                "\x1b[32m✓\x1b[0m"
            } else {
                "\x1b[31m✗\x1b[0m"
            };
            let _ = writeln!(stdout, "  {mark} {preview}");
        }
        RunEvent::ContextCompacted {
            before_tokens,
            after_tokens,
            limit_tokens,
            dropped_messages,
            ..
        } => {
            let _ = writeln!(
                stdout,
                "\x1b[90m上下文到线,已压缩:约 {before_tokens} → {after_tokens} token(上限 {limit_tokens},裁掉 {dropped_messages} 条)\x1b[0m"
            );
        }
        // 规则直接判定的不打扰终端;需要人介入或被硬门禁挡下的才出声(D-173)。
        RunEvent::PermissionResolved {
            action,
            resource,
            decision,
            source,
            ..
        } => {
            if source != "ruleset" || decision == "deny" {
                let _ = writeln!(
                    stdout,
                    "  \x1b[90m权限 {action} {resource} → {decision}({source})\x1b[0m"
                );
            }
        }
        RunEvent::StepEnd { .. } => {}
    };
    let ask_root = ctx.project_root.clone();
    let mut ask = move |request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        let response = match request {
            kanzei_core::AskRequest::Question {
                question,
                options,
                default,
            } => {
                eprint!("\x1b[33m? {question}");
                if !options.is_empty() {
                    eprint!(" [{}]", options.join(" / "));
                }
                if let Some(default) = default {
                    eprint!(" (默认: {default})");
                }
                eprint!("\x1b[0m ");
                let mut line = String::new();
                if std::io::stdin().read_line(&mut line).is_ok() && !line.trim().is_empty() {
                    kanzei_core::AskResponse::Answer(line.trim().to_string())
                } else {
                    kanzei_core::AskResponse::Cancelled
                }
            }
            kanzei_core::AskRequest::Permission { action, resource } => {
                eprint!("\x1b[33m? {action}: {resource} [y 一次 / a 总是 / N 拒绝]\x1b[0m ");
                let mut line = String::new();
                let reply = if std::io::stdin().read_line(&mut line).is_ok() {
                    match line.trim() {
                        "y" | "Y" | "yes" => kanzei_core::AskReply::AllowOnce,
                        "a" | "A" | "always" => {
                            match persist_always_allow(&ask_root, &action, &resource) {
                                Ok(reply) => reply,
                                Err(error) => {
                                    eprintln!(
                                        "\x1b[31m总是允许规则保存失败: {error};本次拒绝\x1b[0m"
                                    );
                                    kanzei_core::AskReply::Deny
                                }
                            }
                        }
                        _ => kanzei_core::AskReply::Deny,
                    }
                } else {
                    kanzei_core::AskReply::Deny
                };
                kanzei_core::AskResponse::Permission(reply)
            }
        };
        Box::pin(async move { response })
    };

    // task 子代理运行时:独立只读快照;fast 角色缺席时两个档位都退回主模型。
    let subagent_rt = {
        let mut sub_harness = Harness::default();
        sub_harness
            .add(kanzei_tools::SubagentBase)
            .add(ConfigComponent);
        let sub_snapshot = sub_harness.resolve(&rctx)?;
        let fast = match config.resolve_model("fast") {
            Ok(r) => (kanzei_core::build_route(&r, &proxy).await)
                .ok()
                .map(|fr| (fr, r.model.clone(), config.service_tier_for(&r))),
            Err(_) => None,
        };
        let primary_tier = config.service_tier_for(&resolved);
        let fast_tier = fast
            .as_ref()
            .map(|(_, _, tier)| tier.clone())
            .unwrap_or_else(|| primary_tier.clone());
        kanzei_core::SubagentRuntime {
            snapshot: sub_snapshot,
            agent: kanzei_tools::explore_agent(),
            fast: fast
                .map(|(r, m, _)| (r, m))
                .unwrap_or_else(|| (route.clone(), resolved.model.clone())),
            primary: (route.clone(), resolved.model.clone()),
            fast_service_tier: fast_tier,
            primary_service_tier: primary_tier,
            max_tokens: config.limits.subagent_max_tokens(),
            // 纯兜底(用户定调:不设短限),防子代理失控挂死整轮。
            timeout_secs: config.limits.subagent_timeout_secs(),
            limits: config.limits.clone(),
            // R-171 批6:CLI 单运行不参与共享仲裁,不登记读槽。
            coordinator: None,
            // R-174:CLI 单运行无前端停止按钮,不挂取消注册表。
            cancellations: None,
        }
    };

    // 开跑预检索(R-106):prompt 命中既有记忆时前置提示块(只给索引行)。
    // D-185:提示块不再拼进 prompt,改由 run_once 作为本轮 system 一次性注入——
    // 拼进 prompt 会随 User message 进 messages → 落 conversations → 下轮回灌累积。
    // CLI 的 `kz run` 一律是用户显式发起的一轮,prompt 就是真实检索键(非自动轮)。
    let memory_hints = kanzei_tools::memory::prompt_hints(&ctx.project_root, &prompt, false);
    let run_prompt = prompt.clone();
    let run_result = tokio::select! {
        result = run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &ctx,
            &run_prompt,
            memory_hints.as_deref(),
            &prior,
            Some(&subagent_rt),
            &mut on_event,
            &mut ask,
        ) => result,
        _ = tokio::signal::ctrl_c() => {
            // 收尾逻辑在 SessionStore::finalize_interrupt 内原子完成并有测试覆盖;
            // 这里只负责把信号接到该入口。
            let store = kanzei_core::SessionStore::open(&state_path)?;
            store.finalize_interrupt(&session_id)?;
            eprintln!("\n\x1b[33m(stopped by Ctrl+C)\x1b[0m");
            return Ok(());
        }
    };
    let store = kanzei_core::SessionStore::open(&state_path)?;
    match &run_result {
        Ok(summary) => {
            store.set_status(&session_id, "idle")?;
            store.append_event(
                &session_id,
                "session.status_changed",
                &serde_json::json!({ "status": "idle" }),
            )?;
            store.append_event(
                &session_id,
                "conversation.updated",
                &serde_json::json!({ "messages": summary.messages }),
            )?;
        }
        Err(error) => {
            store.set_status(&session_id, "failed")?;
            store.append_event(
                &session_id,
                "session.status_changed",
                &serde_json::json!({ "status": "failed" }),
            )?;
            store.append_event(
                &session_id,
                "run.failed",
                &serde_json::json!({ "error": error.to_string() }),
            )?;
        }
    }
    let summary = run_result?;

    if summary.halted_by_user {
        eprintln!("\n\x1b[33m(stopped: permission declined)\x1b[0m");
    }
    println!(
        "\n\x1b[90m— steps {} · in {} (cache r{} w{}) · out {}\x1b[0m",
        summary.steps,
        summary.usage.input,
        summary.usage.cache_read,
        summary.usage.cache_write,
        summary.usage.output
    );
    let context_total: usize = summary.context_report.iter().map(|(_, n)| n).sum();
    println!(
        "\x1b[90m— context {} 源 {} 字符\x1b[0m",
        summary.context_report.len(),
        context_total
    );
    // 本轮切片:summary.messages = prior + 本轮。统计与失败提炼都只看本轮,
    // 否则历史失败会被反复上报、工具计数也会累计全历史(R-099 基线失真)。
    let this_run = &summary.messages[prior.len().min(summary.messages.len())..];
    // 轮末采集(D-229/D-214):CLI 与桌面端共用 harvest_end_of_run——失败提炼 →
    // 条目收口判定 → SOP 候选(项目 inbox,落库目标 global)→ 根因 fact 候选(项目
    // inbox)。SOP 通道 D-229 起双端一致,D-214 起候选投项目 inbox 进消化通道。
    {
        let (delivered, sop, fact) =
            kanzei_tools::memory::harvest_end_of_run(&ctx.project_root, &prompt, this_run);
        if delivered > 0 {
            eprintln!("\x1b[90m(memory: 投递 {delivered} 条失败观察待整理)\x1b[0m");
        }
        if sop {
            eprintln!("\x1b[90m(memory: 已投递候选 SOP 待用户采纳)\x1b[0m");
        }
        if fact {
            eprintln!("\x1b[90m(memory: 已投递根因候选待整理)\x1b[0m");
        }
    }
    // episode 落库(R-106):机械轨迹画像,R-099 度量与记忆系统共用。失败不影响本轮。
    // R-213:当轮 episode_id 留到轮末代填给 memory manager——episode 轮末才落库,
    // manager 在轮内自报不出真实 id,不代填则 provenance 校验会拦下一切晋升。
    let mut current_episode_id: Option<i64> = None;
    {
        let outcome = if summary.halted_by_user {
            "halted"
        } else {
            "completed"
        };
        let tools = kanzei_core::summarize_tools(this_run);
        let store = kanzei_core::SessionStore::open(&state_path)?;
        if let Ok(episode_id) = store.append_episode(&kanzei_core::EpisodeRecord {
            session_id: &session_id,
            prompt_head: &prompt,
            outcome,
            steps: summary.steps,
            input_tokens: summary.usage.input,
            output_tokens: summary.usage.output,
            tools_json: &serde_json::to_string(&tools).unwrap_or_default(),
            context_json: &serde_json::to_string(&summary.context_report).unwrap_or_default(),
            // R-099 调用画像:CLI 与桌面端落同一份口径,基线才可比。
            metrics_json: &serde_json::to_string(&kanzei_core::summarize_metrics(this_run))
                .unwrap_or_default(),
            // D-173:轮次归属。没有这几列时,"这一轮跑的哪个模型"只能靠当前配置反推。
            provider: &resolved.provider_name,
            model: &resolved.model,
            run_id: &run_id,
            input_id: &promoted.input_id,
            duration_ms: run_started.elapsed().as_millis() as u64,
            // R-106:上下文溢出压缩丢弃的轨迹段沉淀为 episode 的一部分,
            // 让溢出路径不再无声丢弃轨迹,复盘时可通过 episodes.overflow_json 查回。
            overflow_json: &serde_json::to_string(&summary.overflow_traces).unwrap_or_default(),
        }) {
            // R-161:本轮开跑预检索的 recall_events 归因到该 episode,可 join 查询。
            let _ = store.link_recall_events_to_episode(episode_id, run_epoch_ms);
            current_episode_id = Some(episode_id);
        }
        // 给这次输入一个结局:此后任何停止都不再把它追认为 cancelled。
        let _ = store.finish_input(&promoted.input_id, true);
    }
    // 轮末记忆整理(R-105):inbox 有草稿才起 manager 迷你 run,尽力而为。
    // R-213:把当轮 episode_id 代填给 manager,晋升证据才能指向真实轮次。
    consolidate_memory_inbox(&config, &proxy, &client, &rctx, &ctx, current_episode_id).await;
    // R-169:CLI 轮末消费自主推进状态机的 backlog 单源(与桌面端同一实现,
    // kanzei_tools::tracker::backlog_status;D-229 类桌面端独占能力架构债消除)。
    // CLI 无交互循环不做自动续跑,只在无可推进条目时提示,与桌面端刹车一致。
    match kanzei_tools::tracker::backlog_status(&ctx.project_root) {
        kanzei_harness::auto_run::BacklogStatus::AllBlocked => {
            eprintln!("\x1b[33m(auto: 需求与缺陷全部被阻塞,自主推进无可用目标)\x1b[0m");
        }
        kanzei_harness::auto_run::BacklogStatus::Empty => {
            eprintln!("\x1b[33m(auto: 需求与缺陷已清空,自主推进无可用目标)\x1b[0m");
        }
        _ => {}
    }
    let exit_code = cli_exit_code(summary.halted_by_user);
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

/// R-163 批4:六臂回放评估入口。
/// `kz replay-eval [--limit N]` 从历史 run.trace 提取 case(默认 30),
/// 六臂各自真调 LLM(fast 档),落 memory_eval 并打印对照报告。
/// 验收②:首批 ≥30 case 可重复执行——同一命令可反复跑,结果逐轮累积。
async fn replay_eval_cli(args: &[String]) -> anyhow::Result<()> {
    let limit = args
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(30);

    let root_flag = args
        .iter()
        .position(|a| a == PROJECT_ROOT_FLAG)
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);

    let cwd = std::env::current_dir()?;
    // 与 run 同构:取根先于配置加载,显式主根同样过 HOME 拦截。
    let project_root =
        main_project_root(explicit_main_root(root_flag.as_deref()).as_deref(), &cwd)?;
    let (config, config_warnings) = KanzeiConfig::load_with_warnings_at_root(&project_root)?;
    let config = Arc::new(config);
    for warning in &config_warnings {
        eprintln!("\x1b[33m{warning}\x1b[0m");
    }

    // fast 档跑批;未配 fast 时回落 primary。
    let model_ref = std::env::var("KANZEI_MODEL")
        .ok()
        .or_else(|| config.models.fast.clone())
        .or_else(|| config.models.primary.clone())
        .ok_or_else(|| anyhow::anyhow!("未配置模型:kanzei.toml [models] 缺 primary"))?;
    let resolved = config.resolve_model(&model_ref)?;
    let proxy = match std::env::var("KANZEI_PROXY")
        .ok()
        .or_else(|| config.proxy.clone())
    {
        Some(p) if p == "off" => ProxyConfig::Disabled,
        Some(p) if p == "env" => ProxyConfig::Env,
        Some(p) if !p.is_empty() => ProxyConfig::Explicit(p),
        _ => ProxyConfig::Env,
    };
    let route = kanzei_core::build_route(&resolved, &proxy).await?;
    let client = LlmClient::new(&proxy)?;

    // 提取 case:最近 run.trace → 解析(带失败步骤的才值得回放)。
    let session_id = kanzei_core::project_session_id(&project_root);
    let state_path = kanzei_core::project_state_path(&project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(&session_id, &project_root.display().to_string(), None)?;
    // 多取 5 倍,过滤掉无失败步骤与解析失败的,凑满 limit。
    let traces = store.list_trace_payloads(&session_id, limit.saturating_mul(5))?;
    let mut cases: Vec<kanzei_core::replay::ReplayCase> = Vec::new();
    for (event_id, payload) in &traces {
        let Some(case) = kanzei_core::replay::parse_trace_payload(payload, event_id) else {
            continue;
        };
        if case.tool_failures() > 0 {
            cases.push(case);
        }
        if cases.len() >= limit {
            break;
        }
    }
    if cases.is_empty() {
        eprintln!("\x1b[33m(replay-eval: 库里没有可回放的失败轨迹——先跑几轮 kz run 再评估)\x1b[0m");
        return Ok(());
    }

    let provider = kanzei_tools::replay_eval::ReplayMemoryProvider::new(&project_root);
    let decider = kanzei_tools::replay_eval::LlmDecider::new(
        std::sync::Arc::new(client),
        std::sync::Arc::new(route),
        resolved.model.clone(),
    );
    eprintln!(
        "replay-eval: {} case, model={}, limit={}",
        cases.len(),
        resolved.model,
        limit
    );
    let mut all_decisions: Vec<Vec<kanzei_core::replay::ReplayDecision>> = Vec::new();
    for case in &cases {
        let decisions = kanzei_core::replay::run_arms(
            case,
            &provider,
            &decider,
            &store,
            &resolved.model,
            "replay-eval-v1",
        )
        .await?;
        all_decisions.push(decisions);
    }
    println!(
        "{}",
        kanzei_core::replay::render_report(&cases, &all_decisions, &resolved.model)
    );
    Ok(())
}

/// memory-manager 迷你 run:消化 inbox 草稿(写读分离的写端)。
/// fast 失败升级 primary;成功判据只看箱——清空才算消化完成。
async fn consolidate_memory_inbox(
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
    client: &LlmClient,
    rctx: &ResolveCtx,
    ctx: &ToolCtx,
    current_episode_id: Option<i64>,
) {
    let store = kanzei_tools::memory::MemoryStore::project(&ctx.project_root);
    if store.pending_notes() == 0 {
        return;
    }
    let inbox = store.read_inbox();
    let mut harness = Harness::default();
    harness.add(kanzei_tools::memory::MemoryManagerComponent);
    let Ok(snapshot) = harness.resolve(rctx) else {
        return;
    };
    let agent = kanzei_tools::memory::manager_agent();
    // R-213:引擎轮末代填当轮 episode_id——manager 在轮内自报不出真实 id(episode 轮末
    // 才落库、list_episodes 不含 id),不注入的话 memory_promote 的 provenance 校验会
    // 拦下一切晋升,候选记忆永远升不了 active。
    let prompt = kanzei_tools::memory::consolidation_prompt(&inbox, current_episode_id);
    // primary 优先(fast 兜底):记忆会注入之后每一轮的上下文,写错一条就长期误导。
    // 实测 fast(qwen3.5:4b)把失败**次数**误读成事实内容,生成了"需要约 7 次重试才能成功"
    // 这种编造结论(M-003 已人工校正)。manager 每轮至多跑一次、prompt 仅数 KB,
    // 用主模型换蒸馏质量是划算的。
    for role in ["primary", "fast"] {
        let Ok(resolved) = config.resolve_model(role) else {
            continue;
        };
        let Ok(route) = kanzei_core::build_route(&resolved, proxy).await else {
            continue;
        };
        let runner_config = RunnerConfig {
            model: resolved.model.clone(),
            max_tokens: 4096,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
            ask_policy: kanzei_core::AskPolicy::NonInteractive,
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
            Box::pin(async move {
                match request {
                    // 快照里只有 memory_* 工具,放行安全;问题一律取消(无人应答)。
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        let _ = run_once(
            client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            ctx,
            &prompt,
            None,
            &[],
            None,
            &mut on_event,
            &mut ask,
        )
        .await;
        if store.pending_notes() == 0 {
            eprintln!("\x1b[90m(memory: inbox 已整理入库)\x1b[0m");
            return;
        }
    }
    eprintln!("\x1b[90m(memory: inbox 未消化,留待下轮)\x1b[0m");
}

fn persist_always_allow(
    project_root: &std::path::Path,
    action: &str,
    resource: &str,
) -> anyhow::Result<kanzei_core::AskReply> {
    let pattern = kanzei_harness::config::generalize_resource(action, resource);
    kanzei_harness::config::append_allow_rule(project_root, action, &pattern)?;
    Ok(kanzei_core::AskReply::AlwaysAllow)
}

/// 人用直通:不经 LLM,直接调 tracker 工具。
/// tracker 子命令的字段开关解析(add / update 共用),返回剩下的位置参数。
///
/// R-191 B3 起 req/defect 登记是硬约束(缺 severity/priority/复杂度/标签即拒),
/// 而这条 CLI 入口原先只会拼标题——`kz defect add` 一律被自己的门禁拒掉。
/// 支持:`--severity/-s`、`--priority/-p`、`--complexity`、`--tag`、
/// `--field 键=值`(可重复,写 复现/根因/影响/期望/验收/进展 等任意字段)。
/// 位置参数语义不变:add 拼成标题,update 取第一个作 id、第二个作 status。
fn parse_tracker_flags(args: &[String], input: &mut serde_json::Value) -> Vec<String> {
    let mut positional: Vec<String> = Vec::new();
    let mut fields = serde_json::Map::new();
    let mut rest = args.iter();
    while let Some(word) = rest.next() {
        match word.as_str() {
            "--severity" | "-s" => {
                if let Some(v) = rest.next() {
                    input["severity"] = serde_json::json!(v);
                }
            }
            "--priority" | "-p" => {
                if let Some(v) = rest.next() {
                    input["priority"] = serde_json::json!(v);
                }
            }
            "--complexity" => {
                if let Some(v) = rest.next() {
                    fields.insert("复杂度".into(), serde_json::json!(v));
                }
            }
            "--tag" => {
                if let Some(v) = rest.next() {
                    fields.insert("标签".into(), serde_json::json!(v));
                }
            }
            "--field" | "-f" => {
                if let Some(v) = rest.next() {
                    if let Some((key, value)) = v.split_once('=') {
                        fields.insert(key.trim().into(), serde_json::json!(value));
                    }
                }
            }
            other => positional.push(other.to_string()),
        }
    }
    if !fields.is_empty() {
        input["fields"] = serde_json::Value::Object(fields);
    }
    positional
}

async fn tracker_cli(args: &[String]) -> anyhow::Result<()> {
    use kanzei_tools::docstore::{DECISIONS, DEFECTS, FINDINGS, GOALS, REQUIREMENTS, SOURCES};
    use kanzei_tools::tracker::TrackerTool;

    let tool = match args[0].as_str() {
        "goal" => TrackerTool {
            tool_name: "goal",
            noun: "goal",
            kind: &GOALS,
            requires_refs: None,
        },
        "decision" => TrackerTool {
            tool_name: "decision",
            noun: "decision",
            kind: &DECISIONS,
            requires_refs: None,
        },
        "req" => TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        },
        "defect" => TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &DEFECTS,
            requires_refs: None,
        },
        "source" => TrackerTool {
            tool_name: "source",
            noun: "source",
            kind: &SOURCES,
            requires_refs: None,
        },
        "finding" => TrackerTool {
            tool_name: "finding",
            noun: "finding",
            kind: &FINDINGS,
            requires_refs: Some(&SOURCES),
        },
        _ => unreachable!(),
    };
    let action = args.get(1).map(String::as_str).unwrap_or("list");
    let mut input = serde_json::json!({ "action": action });
    if action == "list" {
        // 人在 CLI 主动查看仍允许；agent 运行期完整双队列由 tracker 守护拒绝。
        input["reason"] = serde_json::json!("human_cli");
    }
    match action {
        "get" | "close" | "update" | "repair_reused_id" => {
            // D-284:update 也要能写字段与进展。只收 id/status 的话 CLI 走不到关闭——
            // §1.25 要求验收证据必须在 close 前写进进展字段,close 后条目归档就改不动。
            let positional = parse_tracker_flags(&args[2..], &mut input);
            if let Some(id) = positional.first() {
                input["id"] = serde_json::json!(id);
            }
            if let Some(status) = positional.get(1) {
                input["status"] = serde_json::json!(status);
            }
        }
        // D-329:这些动作原先落在 `_ => {}`,位置参数 id 根本没接——CLI 一律报
        // "id is required",工具自己指路的清理通道(raw_lines/raw_delete)在命令行侧
        // 不可用。raw_delete 的第二个位置参数是序号。
        "raw_lines" | "reopen" | "archive" | "void_id" | "repair_missing_id" => {
            let positional = parse_tracker_flags(&args[2..], &mut input);
            if let Some(id) = positional.first() {
                input["id"] = serde_json::json!(id);
            }
        }
        "raw_delete" => {
            let positional = parse_tracker_flags(&args[2..], &mut input);
            if let Some(id) = positional.first() {
                input["id"] = serde_json::json!(id);
            }
            if let Some(ordinal) = positional.get(1).and_then(|raw| raw.parse::<u64>().ok()) {
                input["ordinal"] = serde_json::json!(ordinal);
            }
        }
        "add" => {
            let positional = parse_tracker_flags(&args[2..], &mut input);
            input["title"] = serde_json::json!(positional.join(" "));
        }
        _ => {}
    }
    // 追踪类子命令写的正是 .kanzei/project/*.md,和 run 一样不能落进 HOME(D-194)。
    // R-182 / D-267:这条入口原先是发现式取根,于是在 worktree 里第一层就命中被
    // checkout 出来的 `.kanzei` **分支副本**——两棵树相隔 10 秒各跑 `kz defect add`,
    // 各自在自己的副本上算 next_id,**都拿到 D-267**。改走显式主根:
    // `KANZEI_PROJECT_ROOT` 指哪写哪,没设时行为与今天逐字节相同。
    // (tracker 的位置参数会把 `add` 后面的词全部拼成标题,所以这条入口只认环境变量,
    //  不认 `--project-root` 开关。)
    let cwd = std::env::current_dir()?;
    let project_root = main_project_root(explicit_main_root(None).as_deref(), &cwd)?;
    let ctx = ToolCtx::new(cwd, project_root);
    let output = tool.execute(input, &ctx).await;
    if output.is_error {
        eprintln!("{}", output.content);
        std::process::exit(1);
    }
    println!("{}", output.content);
    Ok(())
}

async fn work_cli(args: &[String]) -> anyhow::Result<()> {
    let action = args.first().map(String::as_str).unwrap_or("next");
    if !matches!(action, "next" | "claim") {
        anyhow::bail!("work action 必须是 next 或 claim");
    }
    let priority = if args.iter().any(|arg| arg == "--requirement-first") {
        kanzei_harness::auto_run::WorkPriority::RequirementFirst
    } else {
        kanzei_harness::auto_run::WorkPriority::DefectFirst
    };
    let mut input = serde_json::json!({"action": action});
    if action == "claim" {
        let id = args
            .get(1)
            .filter(|id| !id.starts_with('-'))
            .ok_or_else(|| anyhow::anyhow!("work claim 需要 R-xxx 或 D-xxx"))?;
        input["id"] = serde_json::json!(id);
        if let Some(position) = args.iter().position(|arg| arg == "--reason") {
            if let Some(reason) = args.get(position + 1) {
                input["reason"] = serde_json::json!(reason);
            }
        }
    }
    let cwd = std::env::current_dir()?;
    let project_root = main_project_root(explicit_main_root(None).as_deref(), &cwd)?;
    let ctx = ToolCtx::new(cwd, project_root).with_work_priority(priority);
    let output = kanzei_tools::WorkTool.execute(input, &ctx).await;
    if output.is_error {
        anyhow::bail!(output.content);
    }
    println!("{}", output.content);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        cli_exit_code, cli_identity_keys, explicit_main_root, explicit_main_root_from,
        main_project_root, parse_run_args, parse_tracker_flags, persist_always_allow, usage_text,
        RunArgs, PROJECT_ROOT_ENV,
    };
    use kanzei_core::AskReply;
    use std::path::{Path, PathBuf};

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
        }
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
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
}
