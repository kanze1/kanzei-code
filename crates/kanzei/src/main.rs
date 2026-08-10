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
       kz <req|defect|source|finding> [list|get <id>|add <title>|close <id>]\n\
config: ~/.kanzei/kanzei.toml + <project>/.kanzei/kanzei.toml\n\
agent: dev(默认开发)、dev-pair(结伴开发)、research(只读研究)\n\
profile: KANZEI_PROFILE=dev|research|readonly；KANZEI_AGENT=dev|dev-pair|research|readonly\n\
model: KANZEI_MODEL=<role|provider:model>，例如 primary、fast、ollama:qwen3.5:4b\n\
proxy: KANZEI_PROXY=off|env|<proxy-url>\n"
}

fn usage() {
    eprint!("{}", usage_text());
}

fn parse_run_args(args: &[String]) -> (bool, bool, String) {
    let new_session = args.iter().any(|arg| arg == "--new");
    let readonly = args.iter().any(|arg| arg == "--readonly");
    let prompt = args
        .iter()
        .filter(|arg| arg.as_str() != "--new" && arg.as_str() != "--readonly")
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    (new_session, readonly, prompt)
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
        "项目根解析成了 HOME({}),而 ~/.kanzei 是全局配置根:项目数据落进去会和\
         全局配置、全局记忆混在一起。\n\
         先 cd 到具体项目目录再跑;确实想把某个目录当项目,就在它下面 mkdir .kanzei。",
        project_root.display()
    );
}

async fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let (new_session, readonly, prompt) = parse_run_args(args);
    if prompt.trim().is_empty() {
        usage();
        std::process::exit(2);
    }

    let cwd = std::env::current_dir()?;
    let (config, config_warnings) = KanzeiConfig::load_with_warnings(&cwd)?;
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
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    reject_home_as_project_root(&project_root)?;
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

    let agent = snapshot
        .select_agent(std::env::var("KANZEI_AGENT").ok().as_deref())?
        .clone();

    // 模型:KANZEI_MODEL 覆盖 agent 定义(快速试模型用)。
    let model_ref = std::env::var("KANZEI_MODEL").unwrap_or_else(|_| agent.model.clone());
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
    };
    let ctx = ToolCtx { cwd, project_root };

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
        }
    };

    // 开跑预检索(R-106):prompt 命中既有记忆时前置提示块(只给索引行)。
    // 队列里存的仍是用户原文;提示块只进本次运行。
    let run_prompt = match kanzei_tools::memory::prompt_hints(&ctx.project_root, &prompt) {
        Some(hints) => format!("{hints}\n\n{prompt}"),
        None => prompt.clone(),
    };
    let run_result = tokio::select! {
        result = run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &ctx,
            &run_prompt,
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
    // 轮末失败提炼与机械投递(R-105):不依赖模型自觉调用 memory_note。
    {
        let signals = kanzei_core::summarize_failures(this_run);
        if !signals.is_empty() {
            let store = kanzei_tools::memory::MemoryStore::project(&ctx.project_root);
            let delivered = kanzei_tools::memory::harvest_failures(&store, &signals);
            if delivered > 0 {
                eprintln!("\x1b[90m(memory: 投递 {delivered} 条失败观察待整理)\x1b[0m");
            }
        }
        // 条目关闭触发的根因→fact(R-105):完成条目的同时把根因原料投 inbox,
        // 由 memory-manager 提炼成 fact。闸门在 completed_entry 里用代码强制。
        if let Some(done) = kanzei_core::completed_entry(this_run) {
            let store = kanzei_tools::memory::MemoryStore::project(&ctx.project_root);
            if kanzei_tools::memory::harvest_entry_fact(&store, &done, &prompt, &signals) {
                eprintln!(
                    "\x1b[90m(memory: 已投递 {} 的根因候选待整理)\x1b[0m",
                    done.id
                );
            }
        }
    }
    // episode 落库(R-106):机械轨迹画像,R-099 度量与记忆系统共用。失败不影响本轮。
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
        }
        // 给这次输入一个结局:此后任何停止都不再把它追认为 cancelled。
        let _ = store.finish_input(&promoted.input_id, true);
    }
    // 轮末记忆整理(R-105):inbox 有草稿才起 manager 迷你 run,尽力而为。
    consolidate_memory_inbox(&config, &proxy, &client, &rctx, &ctx).await;
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

/// memory-manager 迷你 run:消化 inbox 草稿(写读分离的写端)。
/// fast 失败升级 primary;成功判据只看箱——清空才算消化完成。
async fn consolidate_memory_inbox(
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
    client: &LlmClient,
    rctx: &ResolveCtx,
    ctx: &ToolCtx,
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
    let prompt = format!("Consolidate these inbox notes into durable memory entries:\n\n{inbox}");
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
    match action {
        "get" | "close" | "update" | "repair_reused_id" => {
            if let Some(id) = args.get(2) {
                input["id"] = serde_json::json!(id);
            }
            if let Some(status) = args.get(3) {
                input["status"] = serde_json::json!(status);
            }
        }
        "add" => {
            let title = args[2..].join(" ");
            input["title"] = serde_json::json!(title);
        }
        _ => {}
    }
    // 追踪类子命令写的正是 .kanzei/project/*.md,和 run 一样不能落进 HOME(D-194)。
    let ctx = ToolCtx::new(std::env::current_dir()?);
    reject_home_as_project_root(&ctx.project_root)?;
    let output = tool.execute(input, &ctx).await;
    if output.is_error {
        eprintln!("{}", output.content);
        std::process::exit(1);
    }
    println!("{}", output.content);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{cli_exit_code, parse_run_args, persist_always_allow, usage_text};
    use kanzei_core::AskReply;

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
    fn readonly_flag_is_parsed_and_stripped_from_prompt() {
        let args = vec![
            "--readonly".to_string(),
            "分析".to_string(),
            "代码".to_string(),
        ];
        assert_eq!(
            parse_run_args(&args),
            (false, true, "分析 代码".to_string())
        );
    }
    #[test]
    fn halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero() {
        assert_eq!(cli_exit_code(true), 3);
        assert_eq!(cli_exit_code(false), 0);
    }
    #[test]
    fn run_new_flag_is_removed_from_prompt() {
        let args = vec![
            "--new".to_string(),
            "开始".to_string(),
            "新会话".to_string(),
        ];
        assert_eq!(
            parse_run_args(&args),
            (true, false, "开始 新会话".to_string())
        );
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
