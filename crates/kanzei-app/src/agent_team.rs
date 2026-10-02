//! Desktop controls for the same durable tasks exposed to the main agent.
use crate::{normalized_project_root, process_session_id, runtime_for, AppState};
use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
use tauri::{Emitter, State, Window};

#[tauri::command]
pub(crate) async fn agent_team_command(
    window: Window,
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
    input: Value,
) -> Result<Value, String> {
    execute(&window, &state, &project_dir, process_id.as_deref(), input)
        .await
        .map_err(|e| e.to_string())
}
pub(crate) async fn execute(
    window: &Window,
    state: &AppState,
    project_dir: &str,
    process_id: Option<&str>,
    input: Value,
) -> anyhow::Result<Value> {
    let root = normalized_project_root(Path::new(project_dir));
    let owner = process_session_id(&root, process_id);
    let action = input["action"].as_str().unwrap_or("list");
    let mut team = kanzei_tools::team::find(&root, &owner);
    if team.is_none() && matches!(action, "list" | "get") {
        let store = kanzei_tools::team::store::TeamStore::open(&root, &owner)?;
        for job in store.list()? {
            if job.active() {
                store.update(&job.id, |j| {
                    j.state = "interrupted".into();
                    j.latest = "运行已中断，可继续此任务".into();
                    j.revision += 1;
                })?;
            }
        }
        if action == "list" {
            return Ok(json!(store.list()?));
        }
        let id = input["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("需要子任务 id"))?;
        return Ok(json!({"job":store.get(id)?,"history":store.history(id)?}));
    }
    let process = match process_id {
        Some(id) if id != crate::state::default_process_id(&root) => state
            .processes
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("原执行的对话不存在"))?,
        _ => crate::ensure_default_process(state, &root),
    };
    if normalized_project_root(&process.origin_project.0) != root {
        anyhow::bail!("子任务所属项目不匹配");
    }
    let profile = process.profile.lock().unwrap().clone();
    if profile.as_deref() == Some("readonly") && !matches!(action, "list" | "get") {
        anyhow::bail!("讨论不能启动或接管执行任务");
    }
    let code_root = process
        .worktree_path
        .as_ref()
        .map(|p| p.0.clone())
        .unwrap_or_else(|| root.clone());
    if team.is_none() {
        let config = Arc::new(kanzei_harness::KanzeiConfig::load_at_root(&root)?);
        let model = process.model.lock().unwrap().clone();
        let resolved = config.resolve_model(model.as_deref().unwrap_or("primary"))?;
        let proxy = match config.proxy.as_deref() {
            Some("off") => kanzei_llm::ProxyConfig::Disabled,
            Some("env") | None => kanzei_llm::ProxyConfig::Env,
            Some(p) => kanzei_llm::ProxyConfig::Explicit(p.into()),
        };
        let route = kanzei_core::build_route(&resolved, &proxy).await?;
        let rctx = kanzei_harness::ResolveCtx {
            profile: if profile.as_deref() == Some("research") {
                kanzei_harness::ProfileKind::Research
            } else {
                kanzei_harness::ProfileKind::Dev
            },
            cwd: code_root.clone(),
            project_root: root.clone(),
            config: config.clone(),
        };
        let session = runtime_for(state, &owner);
        let mut runtime = kanzei_tools::run::build_subagent_runtime(
            &rctx,
            &config,
            &proxy,
            &resolved,
            &route,
            Some(state.coordinator.clone()),
            Some(session.task_cancellations.clone()),
            None,
            None,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("子代理不可用"))?;
        runtime.options.ask_policy = Some(kanzei_core::AskPolicy::Interactive);
        let asks = session.asks.clone();
        let seq = state.ask_seq.clone();
        let ask_window = window.clone();
        let ask_root = root.clone();
        let ask_owner = owner.clone();
        runtime.ask_router = Some(Arc::new(move |request| {
            let mut ask = crate::run::events::build_ask_handler(
                asks.clone(),
                seq.clone(),
                "子代理",
                &ask_window,
                ask_root.clone(),
                ask_owner.clone(),
            );
            ask(request)
        }));
        let events_window = window.clone();
        team = Some(kanzei_tools::team::AgentTeam::attach(
            rctx,
            kanzei_harness::ToolCtx::new(code_root.clone(), root.clone())
                .with_session_id(owner.clone())
                .with_identity(
                    kanzei_tools::worktree::worktree_key(&code_root),
                    kanzei_tools::worktree::worktree_key(&root),
                    format!("team-{}", crate::run::now_ms()),
                    process.id.clone(),
                ),
            runtime,
            kanzei_llm::LlmClient::new(&proxy)?,
            Some(Arc::new(move |job| {
                let _ =
                    events_window.emit("kz:agent-job", json!({"sessionId":job.owner,"job":job}));
            })),
        )?);
    }
    // UI adoption joins the existing writer arbitration. Calls from the main runner
    // already own that lease and use AgentTeam::command directly.
    let _lease = if action == "adopt" {
        Some(
            state
                .coordinator
                .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
                    write_scope: code_root,
                    run_id: format!("adopt-{}", crate::run::now_ms()),
                    process_id: process.id.clone(),
                    reason: "adopt reviewed child changes".into(),
                })
                .await
                .map_err(anyhow::Error::msg)?,
        )
    } else {
        None
    };
    let team = team.unwrap();
    team.set_mailbox(crate::async_mailbox::for_session(
        window,
        &root,
        &owner,
        Some(process.id.clone()),
    ));
    team.set_ask_router(crate::run::events::build_team_ask_router(
        runtime_for(state, &owner).asks.clone(),
        state.ask_seq.clone(),
        window,
        root,
        owner,
    ));
    team.ui_command(input).await
}
