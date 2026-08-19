//! `kz run` 命令(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:run 是 CLI 的核心命令——装配(配置/harness/模型/身份)→ 输入准入 →
//! run_once → 轮末落库,与 replay-eval/tracker/work 等适配命令正交;拆出后加一条
//! 运行期能力不必读懂 tracker 的 flag 解析(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):取根走 `main_project_root` 唯一通道(D-194/R-182);prompt 真源
//! 解析与 flag 剥除在 mod.rs(`resolve_run_prompt`/`parse_run_args`);RunnerConfig 与
//! 子代理运行时构造共用 kanzei_tools::run(对照表 #12/#16),CLI 传 None/None。

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_harness::config::KanzeiConfig;
use kanzei_harness::defs::ProfileKind;
use kanzei_harness::{ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};
use kanzei_tools::ReadonlyProfile;

mod events;
mod finalize;
mod permissions;

use super::{
    cli_identity_keys, explicit_main_root, main_project_root, parse_run_args, resolve_run_prompt,
    usage, RunArgs,
};

fn resolve_cli_input(args: &[String]) -> (RunArgs, String) {
    let parsed = parse_run_args(args);
    // R-238 ②:prompt 真源解析(--prompt-file 与位置参数互斥,失败给出明确报错)。
    let prompt = match resolve_run_prompt(&parsed.prompt, parsed.prompt_file.as_deref()) {
        Ok(text) => text,
        Err(message) => {
            eprintln!("\x1b[31m{message}\x1b[0m");
            usage();
            std::process::exit(2);
        }
    };
    if prompt.trim().is_empty() {
        usage();
        std::process::exit(2);
    }
    (parsed, prompt)
}

fn cli_projection_gate_enabled(path: &str) -> bool {
    match std::env::var("KANZEI_PROJECTION_GATES").ok() {
        Some(gates) => gates.split(',').map(str::trim).any(|gate| gate == path),
        None => matches!(
            path,
            "conversation_get"
                | "conversation_list"
                | "runner_prior"
                | "ui_history"
                | "subagent_transcript"
        ),
    }
}

fn recover_cli_legacy_segment(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    boundary: Option<i64>,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    let event = store
        .list_events_by_type(session_id, 0, "conversation.updated")?
        .into_iter()
        .filter(|event| boundary.is_none_or(|start| event.sequence > start))
        .rev()
        .find(|event| {
            event.payload["messages"]
                .as_array()
                .is_some_and(|messages| !messages.is_empty())
        });
    let Some(event) = event else {
        return Ok(Vec::new());
    };
    let messages = event
        .payload
        .get("messages")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    Ok(serde_json::from_value(messages)?)
}

fn recover_cli_prior(
    store: &kanzei_core::SessionStore,
    session_id: &str,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    let boundary = store
        .list_events_by_type(session_id, 0, "conversation.reset")?
        .into_iter()
        .map(|event| event.sequence)
        .next_back();
    if !cli_projection_gate_enabled("runner_prior") {
        return Ok(kanzei_core::filter_message_history(
            &recover_cli_legacy_segment(store, session_id, boundary)?,
        ));
    }

    let facts = store.list_latest_segment_facts(session_id)?;
    let compacted_surface =
        store.latest_completed_compaction_surface(session_id, boundary.unwrap_or(0))?;
    if facts.is_empty() {
        if let Some((_, surface)) = compacted_surface {
            return Ok(surface);
        }
        return recover_cli_legacy_segment(store, session_id, boundary);
    }
    let projection = match compacted_surface {
        Some((sequence, surface)) => {
            kanzei_core::project_session_facts_with_surface(&facts, Some(sequence), Some(surface))
        }
        None => kanzei_core::project_session_facts(&facts),
    };
    Ok(projection.surface_messages)
}

pub(crate) async fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let (
        RunArgs {
            new_session,
            readonly,
            project_root: root_flag,
            prompt: _,
            allow,
            prompt_file: _,
            subagents_enabled,
        },
        prompt,
    ) = resolve_cli_input(args);

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
    // R-256 批4:与桌面共用 kanzei_tools::run::build_harness(对照表 #5 公共部分单点),
    // CLI 独有 Readonly 经 middle 注入(顺序与原来一致:Research 后、Markdown 前)。
    let harness = kanzei_tools::run::build_harness(
        |harness| {
            harness.add(ReadonlyProfile);
        },
        |_harness| {},
    );
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
    // R-256:RunnerConfig 构造与桌面共用 kanzei_tools::run::build_runner_config(对照表 #12)。
    // CLI 无 reasoning 覆盖(取配置默认)、交互式询问、无停止令牌。
    let runner_config = kanzei_tools::run::build_runner_config(
        &resolved,
        &config,
        None,
        &ctx.project_root,
        kanzei_core::AskPolicy::Interactive,
        None,
    );

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
    // R-241：CLI 与桌面端共用同一个 typed writer 和投影契约。
    let typed_writer = Arc::new(Mutex::new(kanzei_core::TypedSessionWriter::new(
        &state_path,
        &session_id,
        &run_id,
    )));
    if let Err(error) = kanzei_core::prepare_typed_session(&store, &session_id) {
        typed_writer.lock().unwrap().record_error(error);
    }
    // prior 必须在当前轮 user fact 写入前恢复；否则 projection 会把本轮输入再喂给 runner。
    let prior = recover_cli_prior(&store, &session_id)?;
    typed_writer
        .lock()
        .unwrap()
        .user_message(&promoted.input_id, kanzei_llm::Message::user_text(&prompt));
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
    drop(store);

    eprintln!(
        "\x1b[90mprofile {:?} · agent {} · model {}:{}\x1b[0m",
        profile, agent.name, resolved.provider_name, resolved.model
    );

    let mut on_event = events::make_event_handler(Arc::clone(&typed_writer));
    let mut ask = permissions::make_ask(
        ctx.project_root.clone(),
        super::interactive_stdin(),
        config.non_interactive_policy(),
        super::parse_allowlist(&allow),
    );

    // task 子代理运行时:R-256 与桌面共用 kanzei_tools::run::build_subagent_runtime(对照表
    // #16);CLI 单运行不参与共享仲裁(R-171 批6)、无前端停止按钮(R-174),传 None/None。
    let subagent_rt = if subagents_enabled {
        kanzei_tools::run::build_subagent_runtime(
            &rctx, &config, &proxy, &resolved, &route, None, None, None, None,
        )
        .await?
    } else {
        None
    };

    // 开跑预检索(R-106):prompt 命中既有记忆时前置提示块(只给索引行)。
    // D-185:提示块不再拼进 prompt,改由 run_once 作为本轮 system 一次性注入——
    // 拼进 prompt 会随 User message 进 messages → 落 conversations → 下轮回灌累积。
    // CLI 的 `kz run` 一律是用户显式发起的一轮,prompt 就是真实检索键(非自动轮)。
    let memory_hints = kanzei_tools::memory::prompt_hints(
        &ctx.project_root,
        &prompt,
        false,
        // R-233:配置了 [embeddings] 就带 embedder 走 hybrid,否则纯 BM25。
        kanzei_tools::embed::embedder_from_config(&config)
            .ok()
            .flatten(),
    );
    let run_prompt = prompt.clone();
    let typed_flush_writer = Arc::downgrade(&typed_writer);
    let typed_flush_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            interval.tick().await;
            let Some(writer) = typed_flush_writer.upgrade() else {
                break;
            };
            let mut writer = writer.lock().unwrap();
            writer.flush_due();
            if writer.is_terminal() {
                break;
            }
        }
    });
    let run_result = tokio::select! {
        result = kanzei_core::run_once(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &ctx,
            &run_prompt,
            memory_hints.as_deref(),
            &prior,
            subagent_rt.as_ref(),
            // R-246:CLI 单运行暂无 LineRuntime(dispose 由调用方负责;CLI 进程
            // 生命周期即 line 生命周期,进程退出即全部资源收回)。
            None,
            &mut on_event,
            &mut ask,
        ) => result,
        _ = tokio::signal::ctrl_c() => {
            // 收尾逻辑在 SessionStore::finalize_interrupt 内原子完成并有测试覆盖;
            // 这里只负责把信号接到该入口。
            let store = kanzei_core::SessionStore::open(&state_path)?;
            typed_writer
                .lock()
                .unwrap()
                .finish(kanzei_core::SessionTurnTerminal::Stopped);
            typed_writer
                .lock()
                .unwrap()
                .write_shadow_report(&prior);
            typed_flush_task.abort();
            store.finalize_interrupt(&session_id)?;
            eprintln!("\n\x1b[33m(stopped by Ctrl+C)\x1b[0m");
            return Ok(());
        }
    };
    finalize::finish_run(
        run_result,
        typed_flush_task,
        finalize::FinalizeState {
            state_path: &state_path,
            session_id: &session_id,
            typed_writer,
            prior: &prior,
            prompt: &prompt,
            ctx: &ctx,
            config: &config,
            proxy: &proxy,
            client: &client,
            rctx: &rctx,
            provider: &resolved.provider_name,
            model: &resolved.model,
            run_id: &run_id,
            input_id: &promoted.input_id,
            run_started,
            run_epoch_ms,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::resolve_cli_input;

    #[test]
    fn resolve_cli_input_preserves_parsed_flags_and_prompt() {
        let args = vec!["--readonly".to_string(), "检查代码".to_string()];
        let (parsed, prompt) = resolve_cli_input(&args);

        assert!(parsed.readonly);
        assert!(parsed.subagents_enabled);
        assert_eq!(parsed.prompt, "检查代码");
        assert_eq!(prompt, "检查代码");
    }
}
