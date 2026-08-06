//! kz — kanzei CLI。
//! `kz run "<prompt>"`         跑 agent 循环(harness 装配)
//! `kz req|defect|source|finding <action> [...]`  人用直通:直接操作项目文档
//! 配置:~/.kanzei/kanzei.toml + 项目 .kanzei/kanzei.toml;env 快捷覆盖见 usage。

use std::io::Write as _;
use std::sync::Arc;

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
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
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
        Some("req" | "defect" | "source" | "finding") => tracker_cli(&args).await,
        Some("run") => run_cli(&args[1..]).await,
        Some(_) => run_cli(&args).await,
        None => {
            usage();
            std::process::exit(2);
        }
    }
}

fn usage() {
    eprintln!("usage: kz run \"<prompt>\"");
    eprintln!("       kz <req|defect|source|finding> [list|get <id>|add <title>|close <id>]");
    eprintln!("config: ~/.kanzei/kanzei.toml + <project>/.kanzei/kanzei.toml");
    eprintln!("env 快捷覆盖: KANZEI_PROFILE=dev|research  KANZEI_AGENT  KANZEI_MODEL=<role|provider:model>  KANZEI_PROXY");
}

async fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let prompt = args.join(" ");
    if prompt.trim().is_empty() {
        usage();
        std::process::exit(2);
    }

    let cwd = std::env::current_dir()?;
    let config = Arc::new(KanzeiConfig::load(&cwd)?);
    let profile: ProfileKind = match std::env::var("KANZEI_PROFILE") {
        Ok(p) => p.parse().map_err(|e: String| anyhow::anyhow!(e))?,
        Err(_) => config.default_profile(),
    };
    let project_root = kanzei_harness::config::discover_project_root(&cwd)
        .unwrap_or_else(|| cwd.clone());
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

    let proxy = match std::env::var("KANZEI_PROXY").ok().or_else(|| config.proxy.clone()) {
        Some(p) if p == "off" => ProxyConfig::Disabled,
        Some(p) if p == "env" => ProxyConfig::Env,
        Some(p) if !p.is_empty() => ProxyConfig::Explicit(p),
        _ => ProxyConfig::Env,
    };
    let route = kanzei_core::build_route(&resolved, &proxy).await?;

    let client = LlmClient::new(&proxy)?;
    let runner_config = RunnerConfig { model: resolved.model.clone(), max_tokens: 8192 };
    let ctx = ToolCtx { cwd, project_root };

    eprintln!(
        "\x1b[90mprofile {:?} · agent {} · model {}:{}\x1b[0m",
        profile, agent.name, resolved.provider_name, resolved.model
    );

    let mut stdout = std::io::stdout();
    let mut on_event = move |event: RunEvent| match event {
        RunEvent::Text(text) => {
            let _ = write!(stdout, "{text}");
            let _ = stdout.flush();
        }
        RunEvent::Reasoning(_) => {}
        RunEvent::ToolStart { name, summary } => {
            let _ = writeln!(stdout, "\n\x1b[36m● {name}\x1b[0m {summary}");
        }
        RunEvent::ToolEnd { ok, preview, .. } => {
            let mark = if ok { "\x1b[32m✓\x1b[0m" } else { "\x1b[31m✗\x1b[0m" };
            let _ = writeln!(stdout, "  {mark} {preview}");
        }
        RunEvent::StepEnd { .. } => {}
    };
    let ask_root = ctx.project_root.clone();
    let mut ask = move |action: String, resource: String| -> kanzei_core::AskFuture {
        eprint!("\x1b[33m? {action}: {resource} [y 一次 / a 总是 / N 拒绝]\x1b[0m ");
        let mut line = String::new();
        let reply = if std::io::stdin().read_line(&mut line).is_ok() {
            match line.trim() {
                "y" | "Y" | "yes" => kanzei_core::AskReply::AllowOnce,
                "a" | "A" | "always" => {
                    let pattern =
                        kanzei_harness::config::generalize_resource(&action, &resource);
                    match kanzei_harness::config::append_allow_rule(&ask_root, &action, &pattern) {
                        Ok(path) => eprintln!(
                            "\x1b[90m已记住 {action} `{pattern}` → {}\x1b[0m",
                            path.display()
                        ),
                        Err(e) => eprintln!("\x1b[31m规则保存失败: {e}\x1b[0m"),
                    }
                    kanzei_core::AskReply::AlwaysAllow
                }
                _ => kanzei_core::AskReply::Deny,
            }
        } else {
            kanzei_core::AskReply::Deny
        };
        Box::pin(async move { reply })
    };

    let summary = run_once(
        &client, &route, &snapshot, &agent, &runner_config, &ctx, &prompt, &mut on_event, &mut ask,
    )
    .await?;

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
    Ok(())
}

/// 人用直通:不经 LLM,直接调 tracker 工具。
async fn tracker_cli(args: &[String]) -> anyhow::Result<()> {
    use kanzei_tools::docstore::{DEFECTS, FINDINGS, REQUIREMENTS, SOURCES};
    use kanzei_tools::tracker::TrackerTool;

    let tool = match args[0].as_str() {
        "req" => TrackerTool { tool_name: "req", noun: "requirement", kind: &REQUIREMENTS, requires_refs: None },
        "defect" => TrackerTool { tool_name: "defect", noun: "defect", kind: &DEFECTS, requires_refs: None },
        "source" => TrackerTool { tool_name: "source", noun: "source", kind: &SOURCES, requires_refs: None },
        "finding" => TrackerTool { tool_name: "finding", noun: "finding", kind: &FINDINGS, requires_refs: Some(&SOURCES) },
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
