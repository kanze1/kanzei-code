//! Desktop controls for the same durable tasks exposed to the main agent.
use crate::{normalized_project_root, process_session_id, runtime_for, AppState};
use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
};
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
    execute_impl(window, state, project_dir, process_id, input, None).await
}

pub(crate) struct QuestionReply {
    pub(crate) owner: String,
    pub(crate) child: String,
    pub(crate) question_id: u64,
    pub(crate) generation: u64,
    pub(crate) mailbox: kanzei_harness::AsyncMailbox,
    pub(crate) notice: kanzei_harness::AsyncNotice,
}

pub(crate) async fn reply_to_question(
    window: &Window,
    state: &AppState,
    project_dir: &str,
    process_id: Option<&str>,
    reply: QuestionReply,
) -> anyhow::Result<Value> {
    execute_impl(
        window,
        state,
        project_dir,
        process_id,
        json!({"action":"message"}),
        Some(reply),
    )
    .await
}

async fn execute_impl(
    window: &Window,
    state: &AppState,
    project_dir: &str,
    process_id: Option<&str>,
    input: Value,
    reply: Option<QuestionReply>,
) -> anyhow::Result<Value> {
    let root = normalized_project_root(Path::new(project_dir));
    let owner = process_session_id(&root, process_id);
    if reply.as_ref().is_some_and(|reply| reply.owner != owner) {
        anyhow::bail!("问题所属对话不匹配");
    }
    let action = input["action"].as_str().unwrap_or("list");
    let mut team = kanzei_tools::team::find(&root, &owner);
    if team.is_none() && matches!(action, "list" | "get") {
        let store = kanzei_tools::team::store_for_inspection(&root, &owner)?;
        if action == "list" {
            return Ok(json!(store.list()?));
        }
        let id = input["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("需要子任务 id"))?;
        return Ok(json!({"job":store.get(id)?,"history":store.history(id)?}));
    }
    let process = match process_id {
        Some(id) => state
            .processes
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("原执行的对话不存在"))?,
        _ => return Err(anyhow::anyhow!("请选择一段对话")),
    };
    if normalized_project_root(&process.origin_project.0) != root {
        anyhow::bail!("子任务所属项目不匹配");
    }
    let profile = process.profile.lock().unwrap().clone();
    if !process.subagent_mode().enabled() && matches!(action, "spawn" | "restart") {
        anyhow::bail!("当前对话已关闭子代理，不能派发新任务");
    }
    if profile.as_deref() == Some("readonly") && action == "adopt" {
        anyhow::bail!("讨论不能整合代码修改");
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
            } else if profile.as_deref() == Some("readonly") {
                kanzei_harness::ProfileKind::Readonly
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
        runtime.options.mode = process.subagent_mode();
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
    if !process.subagent_mode().enabled() && matches!(action, "resume" | "message") {
        team.set_policy(
            kanzei_harness::SubagentMode::Off,
            &kanzei_harness::KanzeiConfig::load_at_root(&process.origin_project.0)?.limits,
        );
    }
    let session = runtime_for(state, &owner);
    if let Some(reply) = &reply {
        check_reply_generation(&session, reply.generation)?;
        team.set_mailbox(reply.mailbox.clone());
    } else {
        team.set_mailbox(crate::async_mailbox::for_session(
            window,
            &root,
            &owner,
            Some(process.id.clone()),
        ));
    }
    team.set_ask_router(crate::run::events::build_team_ask_router(
        runtime_for(state, &owner).asks.clone(),
        state.ask_seq.clone(),
        window,
        root,
        owner,
    ));
    if let Some(reply) = reply {
        team.question_reply(&reply.child, reply.question_id, reply.notice)
    } else {
        team.ui_command(input).await
    }
}

fn check_reply_generation(runtime: &crate::SessionRuntime, generation: u64) -> anyhow::Result<()> {
    if runtime.async_generation.load(Ordering::SeqCst) != generation {
        anyhow::bail!("原任务已停止，旧回复不能重新启动它");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn child_reply_bound_before_async_assembly_is_rejected_if_stopped_during_it() {
        let runtime = Arc::new(crate::SessionRuntime::default());
        let generation = runtime.async_generation.load(Ordering::SeqCst);
        let (entered, entering) = tokio::sync::oneshot::channel();
        let (resume, resuming) = tokio::sync::oneshot::channel();
        let assembling = runtime.clone();
        let reply = tokio::spawn(async move {
            entered.send(()).unwrap();
            resuming.await.unwrap();
            check_reply_generation(&assembling, generation)
        });
        entering.await.unwrap();
        {
            let _lifecycle = runtime.lifecycle.lock().unwrap();
            runtime.retire_async();
        }
        resume.send(()).unwrap();
        assert!(reply.await.unwrap().is_err());
        check_reply_generation(&runtime, runtime.async_generation.load(Ordering::SeqCst)).unwrap();
    }
}
