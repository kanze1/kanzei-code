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
use kanzei_tools::{BaseComponent, DevProfile, ResearchProfile};

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
        Some("req" | "defect" | "source" | "finding" | "goal" | "decision") => tracker_cli(&args).await,
        Some("run") => run_cli(&args[1..]).await,
        Some(_) => run_cli(&args).await,
        None => {
            usage();
            std::process::exit(2);
        }
    }
}

fn cli_exit_code(halted_by_user: bool) -> i32 {
    if halted_by_user { 3 } else { 0 }
}

fn usage_text() -> &'static str {
    "usage: kz run \"<prompt>\"\n\
       kz run --new \"<prompt>\"  # 丢弃当前会话上下文并从新会话开始\n\
       kz <req|defect|source|finding> [list|get <id>|add <title>|close <id>]\n\
config: ~/.kanzei/kanzei.toml + <project>/.kanzei/kanzei.toml\n\
agent: dev(默认开发)、dev-pair(结伴开发)、research(只读研究)\n\
profile: KANZEI_PROFILE=dev|research；KANZEI_AGENT=dev|dev-pair|research\n\
model: KANZEI_MODEL=<role|provider:model>，例如 primary、fast、ollama:qwen3.5:4b\n\
proxy: KANZEI_PROXY=off|env|<proxy-url>\n"
}

fn usage() {
    eprint!("{}", usage_text());
}

fn parse_run_args(args: &[String]) -> (bool, String) {
    let new_session = args.iter().any(|arg| arg == "--new");
    let prompt = args
        .iter()
        .filter(|arg| arg.as_str() != "--new")
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    (new_session, prompt)
}

async fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let (new_session, prompt) = parse_run_args(args);
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
        Ok(p) => p.parse().map_err(|e: String| anyhow::anyhow!(e))?,
        Err(_) => config.default_profile(),
    };
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
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
        max_tokens: 8192,
        reasoning: config
            .models
            .reasoning
            .as_deref()
            .map(kanzei_llm::ReasoningEffort::parse)
            .unwrap_or_default(),
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
        RunEvent::Retry { attempt, max, delay_ms } => {
            let _ = writeln!(stdout, "\x1b[33m重试 {attempt}/{max},等待 {delay_ms}ms\x1b[0m");
        }
        RunEvent::StreamRestart { attempt, max, delay_ms } => {
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
        RunEvent::StepEnd { .. } => {}
    };
    let ask_root = ctx.project_root.clone();
    let mut ask = move |request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        let response = match request {
            kanzei_core::AskRequest::Question { question, options, default } => {
                eprint!("\x1b[33m? {question}");
                if !options.is_empty() { eprint!(" [{}]", options.join(" / ")); }
                if let Some(default) = default { eprint!(" (默认: {default})"); }
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
                        "a" | "A" | "always" => match persist_always_allow(&ask_root, &action, &resource) {
                            Ok(reply) => reply,
                            Err(error) => {
                                eprintln!("\x1b[31m总是允许规则保存失败: {error};本次拒绝\x1b[0m");
                                kanzei_core::AskReply::Deny
                            }
                        },
                        _ => kanzei_core::AskReply::Deny,
                    }
                } else { kanzei_core::AskReply::Deny };
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
                .map(|fr| (fr, r.model.clone())),
            Err(_) => None,
        };
        kanzei_core::SubagentRuntime {
            snapshot: sub_snapshot,
            agent: kanzei_tools::explore_agent(),
            fast: fast.unwrap_or_else(|| (route.clone(), resolved.model.clone())),
            primary: (route.clone(), resolved.model.clone()),
            max_tokens: 4096,
            // 纯兜底(用户定调:不设短限),防子代理失控挂死整轮。
            timeout_secs: 900,
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
    }
    // episode 落库(R-106):机械轨迹画像,R-099 度量与记忆系统共用。失败不影响本轮。
    {
        let outcome = if summary.halted_by_user { "halted" } else { "completed" };
        let tools = kanzei_core::summarize_tools(this_run);
        let store = kanzei_core::SessionStore::open(&state_path)?;
        let _ = store.append_episode(
            &session_id,
            &prompt,
            outcome,
            summary.steps,
            summary.usage.input,
            summary.usage.output,
            &serde_json::to_string(&tools).unwrap_or_default(),
            &serde_json::to_string(&summary.context_report).unwrap_or_default(),
            // R-099 调用画像:CLI 与桌面端落同一份口径,基线才可比。
            &serde_json::to_string(&kanzei_core::summarize_metrics(this_run)).unwrap_or_default(),
        );
    }
    // 轮末记忆整理(R-105):inbox 有草稿才起 manager 迷你 run,尽力而为。
    consolidate_memory_inbox(&config, &proxy, &client, &rctx, &ctx).await;
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
        "get" | "close" | "update" => {
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
    let ctx = ToolCtx::new(std::env::current_dir()?);
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
    fn halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero() {
        assert_eq!(cli_exit_code(true), 3);
        assert_eq!(cli_exit_code(false), 0);
    }
    #[test]
    fn run_new_flag_is_removed_from_prompt() {
        let args = vec!["--new".to_string(), "开始".to_string(), "新会话".to_string()];
        assert_eq!(parse_run_args(&args), (true, "开始 新会话".to_string()));
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
