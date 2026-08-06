//! kz — kanzei CLI。M0:一次性问答 `kz run "<prompt>"`。

use std::io::Write as _;

use kanzei_core::{run_once, RunEvent, RunnerConfig};
use kanzei_harness::ToolCtx;
use kanzei_llm::{LlmClient, ProxyConfig, Route};
use kanzei_tools::{builtin_tools, detected_shell};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("--version" | "-V" | "version")) {
        println!("kanzei {} ({})", env!("CARGO_PKG_VERSION"), option_env!("KANZEI_BUILD_INFO").unwrap_or("dev"));
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("run") {
        args.remove(0);
    }
    let prompt = args.join(" ");
    if prompt.trim().is_empty() {
        eprintln!("usage: kz run \"<prompt>\"");
        eprintln!("env: KANZEI_PROVIDER=anthropic|openai|ollama (default anthropic)");
        eprintln!("     anthropic: ANTHROPIC_API_KEY (required), ANTHROPIC_BASE_URL");
        eprintln!("     openai:    KANZEI_BASE_URL, KANZEI_API_KEY/OPENAI_API_KEY");
        eprintln!("     ollama:    KANZEI_BASE_URL (default http://127.0.0.1:11434/v1), no key");
        eprintln!("     common:    KANZEI_MODEL, KANZEI_PROXY");
        std::process::exit(2);
    }

    let provider = std::env::var("KANZEI_PROVIDER").unwrap_or_else(|_| "anthropic".into());
    let (route, default_model) = match provider.as_str() {
        "anthropic" => {
            let api_key = std::env::var("ANTHROPIC_API_KEY")
                .or_else(|_| std::env::var("KANZEI_API_KEY"))
                .map_err(|_| anyhow::anyhow!("set ANTHROPIC_API_KEY (or KANZEI_API_KEY)"))?;
            let base = std::env::var("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".into());
            (Route::anthropic_at(&base, &api_key), "claude-sonnet-5")
        }
        "openai" => {
            let base = std::env::var("KANZEI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into());
            let key = std::env::var("KANZEI_API_KEY")
                .or_else(|_| std::env::var("OPENAI_API_KEY"))
                .ok();
            (Route::openai_at(&base, key.as_deref()), "gpt-5")
        }
        // 本地模型:简单工具调用快速响应/并行子代理的跑法,零配置直连。
        "ollama" => {
            let base = std::env::var("KANZEI_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".into());
            (Route::openai_at(&base, None), "qwen3")
        }
        other => anyhow::bail!("unknown KANZEI_PROVIDER `{other}` (anthropic|openai|ollama)"),
    };
    let model = std::env::var("KANZEI_MODEL").unwrap_or_else(|_| default_model.into());
    let proxy = match std::env::var("KANZEI_PROXY") {
        Ok(p) if !p.is_empty() => ProxyConfig::Explicit(p),
        _ => ProxyConfig::Env,
    };

    let cwd = std::env::current_dir()?;
    let shell = detected_shell();
    // 系统提示词预算制(设计红线 9):一段话,不写教程。
    let system = vec![format!(
        "You are kanzei, a coding agent in a terminal. Environment: OS {}, cwd {}, shell {}. \
         Use tools to inspect and change things instead of guessing; then answer concisely in the user's language.",
        std::env::consts::OS,
        cwd.display(),
        shell.name,
    )];

    let client = LlmClient::new(&proxy)?;
    let tools = builtin_tools();
    let config = RunnerConfig { model, max_tokens: 8192, max_steps: 24, system };
    let ctx = ToolCtx { cwd };

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

    let summary = run_once(&client, &route, &tools, &config, &ctx, &prompt, &mut on_event).await?;

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
