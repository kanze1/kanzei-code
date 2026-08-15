//! `kz replay-eval` 六臂回放评估(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:回放评估是「从历史 run.trace 提取 case、六臂真调 LLM、落 memory_eval」的
//! 独立变更理由,与 run/tracker/work 正交;拆出后加一条评估臂不必读懂 run 的装配
//! (照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):取根走 `main_project_root` 唯一通道;fast 档跑批,未配 fast
//! 回落 primary;首批 ≥30 case 可重复执行(R-163 批4 验收②)。

use std::sync::Arc;

use kanzei_harness::config::KanzeiConfig;
use kanzei_llm::{LlmClient, ProxyConfig};

use super::{explicit_main_root, main_project_root, PROJECT_ROOT_FLAG};

/// R-163 批4:六臂回放评估入口。
/// `kz replay-eval [--limit N]` 从历史 run.trace 提取 case(默认 30),
/// 六臂各自真调 LLM(fast 档),落 memory_eval 并打印对照报告。
/// 验收②:首批 ≥30 case 可重复执行——同一命令可反复跑,结果逐轮累积。
pub(crate) async fn replay_eval_cli(args: &[String]) -> anyhow::Result<()> {
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
        Arc::new(client),
        Arc::new(route),
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
