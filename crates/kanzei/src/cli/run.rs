//! `kz run` 命令(R-256 批3,纯搬迁自 main.rs)。
//!
//! 独立理由:run 是 CLI 的核心命令——装配(配置/harness/模型/身份)→ 输入准入 →
//! run_once → 轮末落库,与 replay-eval/tracker/work 等适配命令正交;拆出后加一条
//! 运行期能力不必读懂 tracker 的 flag 解析(照 files_view.rs 模式)。
//!
//! 危险点(搬迁纪律):取根走 `main_project_root` 唯一通道(D-194/R-182);prompt 真源
//! 解析与 flag 剥除在 mod.rs(`resolve_run_prompt`/`parse_run_args`);RunnerConfig 与
//! 子代理运行时构造共用 kanzei_tools::run(对照表 #12/#16),CLI 传 None/None。

use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_harness::config::KanzeiConfig;
use kanzei_harness::defs::ProfileKind;
use kanzei_harness::{ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};
use kanzei_tools::ReadonlyProfile;

use super::memory::{consolidate_memory_inbox, persist_always_allow};
use super::{
    cli_exit_code, cli_identity_keys, explicit_main_root, interactive_stdin, main_project_root,
    non_interactive_decision, parse_allowlist, parse_run_args, resolve_run_prompt, usage, RunArgs,
};

pub(crate) async fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let RunArgs {
        new_session,
        readonly,
        project_root: root_flag,
        prompt,
        allow,
        prompt_file,
    } = parse_run_args(args);
    // R-238 ②:prompt 真源解析(--prompt-file 与位置参数互斥,失败给出明确报错)。
    let prompt = match resolve_run_prompt(&prompt, prompt_file.as_deref()) {
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
    let typed_writer_for_events = Arc::clone(&typed_writer);
    let mut on_event = move |event: kanzei_core::RunEvent| match event {
        kanzei_core::RunEvent::TurnStart { step, max_steps } => {
            typed_writer_for_events
                .lock()
                .unwrap()
                .turn_started(step, max_steps);
            if step > 1 {
                let label = if max_steps > 0 {
                    format!("第 {step}/{max_steps} 轮")
                } else {
                    format!("第 {step} 轮")
                };
                let _ = writeln!(stdout, "\n\x1b[90m── {label} ──\x1b[0m");
            }
        }
        kanzei_core::RunEvent::Text(text) => {
            typed_writer_for_events.lock().unwrap().push_text(&text);
            let _ = write!(stdout, "{text}");
            let _ = stdout.flush();
        }
        kanzei_core::RunEvent::Reasoning(_) => {}
        kanzei_core::RunEvent::AssistantMessageCommitted { step, message } => {
            typed_writer_for_events
                .lock()
                .unwrap()
                .assistant_committed(step, message)
        }
        kanzei_core::RunEvent::ToolResultsCommitted { step, message } => typed_writer_for_events
            .lock()
            .unwrap()
            .tool_results_committed(step, message),
        kanzei_core::RunEvent::ToolStart { name, summary, .. } => {
            let _ = writeln!(stdout, "\n\x1b[36m● {name}\x1b[0m {summary}");
        }
        kanzei_core::RunEvent::TaskProgress { text, .. } => {
            let _ = writeln!(stdout, "  \x1b[90m… {text}\x1b[0m");
        }
        // CLI 不逐段转印工具输出:ToolEnd 的预览已够,逐段会与正文流互相穿插。
        kanzei_core::RunEvent::ToolProgress { .. } => {}
        kanzei_core::RunEvent::Retry {
            attempt,
            max,
            delay_ms,
        } => {
            let _ = writeln!(
                stdout,
                "\x1b[33m重试 {attempt}/{max},等待 {delay_ms}ms\x1b[0m"
            );
        }
        kanzei_core::RunEvent::StreamRestart {
            attempt,
            max,
            delay_ms,
        } => {
            typed_writer_for_events.lock().unwrap().stream_restarted();
            let _ = writeln!(
                stdout,
                "\x1b[33m连接中断,重新请求本轮 {attempt}/{max},等待 {delay_ms}ms(本轮工具尚未执行,不会重复副作用)\x1b[0m"
            );
        }
        kanzei_core::RunEvent::ToolEnd { ok, preview, .. } => {
            let mark = if ok {
                "\x1b[32m✓\x1b[0m"
            } else {
                "\x1b[31m✗\x1b[0m"
            };
            let _ = writeln!(stdout, "  {mark} {preview}");
        }
        kanzei_core::RunEvent::ContextCompacted {
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
        kanzei_core::RunEvent::ContextPruned {
            cleared_results,
            before_tokens,
            after_tokens,
        } => {
            let _ = writeln!(
                stdout,
                "\x1b[90m已机械清理 {cleared_results} 条旧工具结果:约 {before_tokens} → {after_tokens} token(零 LLM)\x1b[0m"
            );
        }
        // 规则直接判定的不打扰终端;需要人介入或被硬门禁挡下的才出声(D-173)。
        // R-183:deny/会话层决策打印命中的规则原文(验收④轨迹)。
        kanzei_core::RunEvent::PermissionResolved {
            action,
            resource,
            decision,
            source,
            rule,
            ..
        } => {
            if source != "ruleset" || decision == "deny" {
                let rule_text = rule
                    .as_deref()
                    .map(|r| format!(" [规则: {r}]"))
                    .unwrap_or_default();
                let _ = writeln!(
                    stdout,
                    "  \x1b[90m权限 {action} {resource} → {decision}({source}){rule_text}\x1b[0m"
                );
            }
        }
        kanzei_core::RunEvent::StepEnd { .. } => {}
    };
    let ask_root = ctx.project_root.clone();
    // R-183:非交互分流参数在闭包外算好(开跑时定格),move 进闭包:
    // - interactive:stdin 是否 TTY(管道/重定向/后台 = 非交互,不读 stdin);
    // - non_interactive_policy:配置的三态策略(缺省 deny,fail-closed);
    // - allowlist:--allow 解析结果(仅 allow_listed 档参与决策)。
    let interactive = interactive_stdin();
    let non_interactive_policy = config.non_interactive_policy();
    let allowlist = parse_allowlist(&allow);
    let mut ask = move |request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        let response = match request {
            kanzei_core::AskRequest::Question {
                question,
                options,
                default,
                multiple,
            } => {
                eprint!("\x1b[33m? {question}");
                if !options.is_empty() {
                    if multiple {
                        eprint!(" [可多选,逗号分隔]");
                    }
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
                if !interactive {
                    // R-183:非交互通道不读 stdin,按配置策略分流(缺省 deny)。
                    // 拒绝/放行都走 drive 层的 PermissionResolved 事件落轨迹。
                    let reply = non_interactive_decision(
                        non_interactive_policy,
                        &allowlist,
                        &action,
                        &resource,
                    );
                    kanzei_core::AskResponse::Permission(reply)
                } else {
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
            }
        };
        Box::pin(async move { response })
    };

    // task 子代理运行时:R-256 与桌面共用 kanzei_tools::run::build_subagent_runtime(对照表
    // #16);CLI 单运行不参与共享仲裁(R-171 批6)、无前端停止按钮(R-174),传 None/None。
    let subagent_rt = kanzei_tools::run::build_subagent_runtime(
        &rctx, &config, &proxy, &resolved, &route, None, None,
    )
    .await?;

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
            let legacy: Vec<kanzei_llm::Message> = store
                .latest_event(&session_id, "conversation.updated")?
                .and_then(|event| serde_json::from_value(event.payload["messages"].clone()).ok())
                .unwrap_or_default();
            typed_writer.lock().unwrap().write_shadow_report(&legacy);
            typed_flush_task.abort();
            store.finalize_interrupt(&session_id)?;
            eprintln!("\n\x1b[33m(stopped by Ctrl+C)\x1b[0m");
            return Ok(());
        }
    };
    let store = kanzei_core::SessionStore::open(&state_path)?;
    match &run_result {
        Ok(summary) => {
            typed_writer
                .lock()
                .unwrap()
                .finish(if summary.halted_by_user {
                    kanzei_core::SessionTurnTerminal::Stopped
                } else {
                    kanzei_core::SessionTurnTerminal::Completed
                });
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
            typed_writer
                .lock()
                .unwrap()
                .write_shadow_report(&summary.messages);
        }
        Err(error) => {
            typed_writer
                .lock()
                .unwrap()
                .finish(kanzei_core::SessionTurnTerminal::Failed(error.to_string()));
            typed_writer.lock().unwrap().write_shadow_report(&prior);
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
    typed_flush_task.abort();
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
    // D-341/R-195:轮末自动处置 candidate——有真实当轮 episode 且复发≥3 的
    // 自动 promote,超期未处置的自动 deprecated 归档,其余保持 candidate。
    // 与 inbox 消化解耦:没有草稿也要跑,否则 candidate 永远躺着无人验收。
    // 机械判定不依赖 LLM,失败不阻塞收尾(报告仅用于打日志留证据)。
    if let Ok(report) = kanzei_tools::memory::reconcile_candidates(
        &ctx.project_root,
        current_episode_id,
        kanzei_tools::memory::CANDIDATE_MAX_AGE_DAYS,
    ) {
        if !report.promoted.is_empty() || !report.deprecated.is_empty() {
            eprintln!(
                "\x1b[90m(memory: candidate 自动处置: promote {} / deprecated {} / 未动 {} \
                 (文件 {}→{}, 索引 {}→{})\x1b[0m",
                report.promoted.len(),
                report.deprecated.len(),
                report.untouched.len(),
                report.candidate_files_before,
                report.candidate_files_after,
                report.candidate_index_before,
                report.candidate_index_after,
            );
        }
    }
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
