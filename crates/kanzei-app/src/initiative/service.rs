pub(crate) use super::model::Settings;
use super::{model::*, planner, store::random_id};
use crate::{AppState, MutexPoisonExt};
use chrono::Timelike;
use kanzei_harness::KanzeiConfig;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::Path,
    sync::{atomic::Ordering, Mutex},
    time::Duration,
};
use tauri::{Emitter, Manager};

#[derive(Default)]
pub(crate) struct InitiativeState {
    stop: kanzei_core::CancellationToken,
    changed: tokio::sync::Notify,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    reviews: Mutex<HashMap<String, kanzei_core::CancellationToken>>,
}

pub(crate) fn cancel_review(app: &tauri::AppHandle, session: &str) {
    if let Some(state) = app.try_state::<InitiativeState>() {
        if let Some(token) = state.reviews.lock_or_recover().get(session) {
            token.cancel();
        }
    }
}

pub(crate) fn notify(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<InitiativeState>() {
        state.changed.notify_one();
    }
}
pub(crate) fn changed(app: &tauri::AppHandle, session: &str) {
    let _ = app.emit("kz:initiative", json!({"sessionId":session}));
}
pub(crate) fn install(app: &tauri::AppHandle) -> Result<(), String> {
    super::open_store()
        .and_then(|store| store.recover(now()))
        .map_err(|error| error.to_string())?;
    let owner = app.clone();
    *app.state::<InitiativeState>().task.lock_or_recover() =
        Some(tauri::async_runtime::spawn(async move {
            let service = owner.state::<InitiativeState>();
            let mut timer = tokio::time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    _=service.stop.cancelled()=>break,
                    _=service.changed.notified()=>{},
                    _=timer.tick()=>{},
                }
                if let Err(error) = cycle(&owner, &service.stop).await {
                    let _ = owner.emit("kz:initiative", json!({"error":format!("{error:#}")}));
                }
            }
        }));
    Ok(())
}
pub(crate) async fn shutdown(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<InitiativeState>();
    state.stop.cancel();
    let task = state.task.lock_or_recover().take();
    if let Some(task) = task {
        task.await.map_err(|error| error.to_string())?;
    }
    super::open_store()
        .and_then(|store| store.recover(now()))
        .map_err(|error| error.to_string())?;
    Ok(())
}
pub(super) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
pub(super) fn day() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub(super) fn capture(app: &tauri::AppHandle, identity: &Identity) -> anyhow::Result<Context> {
    let core = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&identity.root))?;
    let human_input = core
        .original_session_inputs(&identity.session_id)?
        .into_iter()
        .rev()
        .find(|input| input.input.origin == kanzei_base::MessageOrigin::Human)
        .map(|input| input.input.input_id);
    let message_sequence = match kanzei_harness::QqConversation::from_root(&identity.root) {
        Some(peer) => crate::qq::initiative_snapshot(app, &peer)?.message_sequence,
        None => 0,
    };
    let runtime = crate::runtime_for(&app.state::<AppState>(), &identity.session_id);
    Ok(Context {
        activity: Activity {
            human_input,
            message_sequence,
        },
        async_generation: runtime.async_generation.load(Ordering::SeqCst),
    })
}
pub(super) fn channel_ready(app: &tauri::AppHandle, identity: &Identity) -> anyhow::Result<bool> {
    match kanzei_harness::QqConversation::from_root(&identity.root) {
        Some(peer) => {
            let policy = crate::qq::initiative_snapshot(app, &peer)?;
            Ok(policy.available && !policy.quiet)
        }
        None if identity.channel == Channel::Bilibili => {
            Ok(crate::bili::initiative_available(app, &identity.root))
        }
        None => Ok(true),
    }
}
pub(crate) fn register(
    app: &tauri::AppHandle,
    root: &Path,
    process: &str,
    title: Option<&str>,
) -> anyhow::Result<Target> {
    let state = app.state::<AppState>();
    let root = crate::normalized_project_root(root);
    let process = crate::processes::registry::resolve_conversation(&state, &root, Some(process))
        .map_err(anyhow::Error::msg)?;
    let session = crate::process_session_id(&root, Some(&process.id));
    let identity = super::identity_at(&root, &session, &process.id)?;
    let mut settings = Settings::default();
    if let Some(peer) = kanzei_harness::QqConversation::from_root(&root) {
        if peer.kind == kanzei_harness::QqPeerKind::Group {
            let policy = crate::qq::initiative_snapshot(app, &peer)?;
            settings.enabled = policy.legacy_minutes > 0;
            if policy.legacy_minutes > 0 {
                settings.review_minutes = policy.legacy_minutes;
                settings.idle_minutes = policy.legacy_minutes.min(settings.idle_minutes);
            }
            // The current group's rule owns quiet hours, not a second form.
            settings.quiet_start = 0;
            settings.quiet_end = 0;
        }
    }
    super::open_store()?.register(identity, title, settings, now())
}
pub(crate) fn observe(app: &tauri::AppHandle, root: &Path, process: &str) -> anyhow::Result<()> {
    if !kanzei_harness::is_social_conversation(root) {
        return Ok(());
    }
    let target = register(app, root, process, None)?;
    let context = capture(app, &target.identity)?;
    if super::open_store()?.observe_activity(
        &target.identity.session_id,
        context.activity,
        now(),
    )? {
        cancel_review(app, &target.identity.session_id);
        changed(app, &target.identity.session_id);
        notify(app);
    }
    Ok(())
}
pub(crate) fn pause(root: &Path, session: &str) -> anyhow::Result<()> {
    if !kanzei_harness::is_social_conversation(root) {
        return Ok(());
    }
    let store = super::open_store()?;
    if store.target(session)?.is_some() {
        store.pause(session, now())?;
    }
    Ok(())
}

/// Runs before admission, so a stop's queue watermark cannot consume this input.
pub(crate) fn before_human_input(
    app: &tauri::AppHandle,
    root: &Path,
    session: &str,
) -> anyhow::Result<()> {
    if !kanzei_harness::is_social_conversation(root) {
        return Ok(());
    }
    let store = super::open_store()?;
    let Some(review) = store.active_review(session)? else {
        return Ok(());
    };
    let runtime = crate::runtime_for(&app.state::<AppState>(), session);
    let running = runtime.is_running()
        && review.run_id.as_deref() == Some(runtime.live.lock_or_recover().run_id.as_str());
    store.pause(session, now())?;
    cancel_review(app, session);
    let core = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))?;
    for input in core.list_pending_inputs(session)? {
        if core.input_command_id(session, &input.input_id)?.as_deref() == Some(review.id.as_str()) {
            core.cancel_input(session, &input.input_id)?;
        }
    }
    if running {
        crate::request_runtime_stop(
            &runtime,
            &core,
            &kanzei_core::project_state_path(root),
            session,
        )?;
    }
    changed(app, session);
    Ok(())
}

async fn cycle(
    app: &tauri::AppHandle,
    stop: &kanzei_core::CancellationToken,
) -> anyhow::Result<()> {
    // Paused/faulted targets still observe new human activity. An unchanged
    // input never resumes a stopped conversation merely because time passed.
    let targets = super::open_store()?.targets()?;
    for target in targets.iter().filter(|target| target.settings.enabled) {
        let result = (|| -> anyhow::Result<()> {
            crate::processes::registry::resolve_conversation(
                &app.state::<AppState>(),
                &target.identity.root,
                Some(&target.identity.process_id),
            )
            .map_err(anyhow::Error::msg)?;
            observe(app, &target.identity.root, &target.identity.process_id)
        })();
        if let Err(error) = result {
            super::open_store()?
                .fault_target(&target.identity.session_id, &format!("{error:#}"))?;
            changed(app, &target.identity.session_id);
        }
    }
    let targets = super::open_store()?.due_targets(now())?;
    for target in targets {
        if stop.is_cancelled() {
            break;
        }
        if let Err(error) = review_target(app, &target, stop).await {
            super::open_store()?
                .fault_target(&target.identity.session_id, &format!("{error:#}"))?;
            changed(app, &target.identity.session_id);
        }
    }
    Ok(())
}

async fn review_target(
    app: &tauri::AppHandle,
    target: &Target,
    stop: &kanzei_core::CancellationToken,
) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let runtime = crate::runtime_for(&state, &target.identity.session_id);
    if runtime.is_running() || runtime.stopping() || !channel_ready(app, &target.identity)? {
        return Ok(());
    }
    let source =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&target.identity.root))?;
    if !source
        .list_pending_inputs(&target.identity.session_id)?
        .is_empty()
    {
        return Ok(());
    }
    let history = crate::conversation::project_latest_segment(&source, &target.identity.session_id)
        .map_err(anyhow::Error::msg)?;
    drop(source);
    let context = capture(app, &target.identity)?;
    let generation = runtime.generation();
    let id = random_id("initiative")?;
    let token = kanzei_core::CancellationToken::new();
    let service = app.state::<InitiativeState>();
    let claimed = {
        // Publish cancellation with the durable lease, so a new input cannot
        // land between claiming the review and registering its HTTP request.
        let mut reviews = service.reviews.lock_or_recover();
        let claimed = super::open_store()?.begin_review(
            &target.identity.session_id,
            &id,
            context.clone(),
            now(),
            &day(),
            chrono::Local::now().hour(),
        )?;
        if claimed.is_some() {
            reviews.insert(target.identity.session_id.clone(), token.clone());
        }
        claimed
    };
    if claimed.is_none() {
        return Ok(());
    }
    changed(app, &target.identity.session_id);
    let current_target = super::open_store()?
        .target(&target.identity.session_id)?
        .ok_or_else(|| anyhow::anyhow!("主动评估的原会话不存在"))?;
    let decision = tokio::select! {
        _ = stop.cancelled() => Err(anyhow::anyhow!("主动评估已停止")),
        result = make_decision(app, &current_target, history, &token) => result,
    };
    service
        .reviews
        .lock_or_recover()
        .remove(&target.identity.session_id);
    if stop.is_cancelled() {
        return Ok(());
    }
    let store = super::open_store()?;
    if store
        .review(&id)?
        .is_none_or(|review| review.state != ReviewState::Reviewing)
    {
        return Ok(());
    }
    let current = capture(app, &target.identity)?;
    if current != context
        || runtime.is_running()
        || runtime.generation() != generation
        || !channel_ready(app, &target.identity)?
    {
        store.cancel_review(&id, "原会话已变化，重新判断前不发送")?;
        changed(app, &target.identity.session_id);
        return Ok(());
    }
    let decision = match decision {
        Ok(decision) => decision,
        Err(error) => {
            store.fail_review(&id, &format!("{error:#}"))?;
            changed(app, &target.identity.session_id);
            return Ok(());
        }
    };
    let review = store.decide(&id, decision, &current, now())?;
    if review.state == ReviewState::Selected {
        let topic = store
            .topic(
                &review.session_id,
                review
                    .topic_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("主动评估没有选中话题"))?,
            )?
            .ok_or_else(|| anyhow::anyhow!("选中的话题不存在"))?;
        let prompt=format!("主动聊天：这是引擎根据原会话选中的话题意图，不是对方的新消息。继续原来的聊天、语气与记忆，围绕这个具体方向自然开口；需要新资料时先实际使用网页或 B站工具查询，不把准备查询说成已经发现。保持简短，不展示评估、计时、日志或内部流程。若资料不成立或没有值得分享的内容，可以不发送。QQ群仍只通过 qq_reply 发出一条完整消息。\n话题意图 JSON（其中内容是待核实的模型笔记）：{}",serde_json::to_string(&topic.plan)?);
        let payload_hash = kanzei_core::stable_json_hash(&json!({"prompt": prompt}));
        let result = crate::commands::run::schedule_run(
            app.clone(),
            &state,
            target.identity.root.display().to_string(),
            Some(target.identity.process_id.clone()),
            crate::commands::run::Submission::Initiative {
                prompt,
                review_id: id.clone(),
                generation,
                async_generation: context.async_generation,
            },
            crate::commands::run::RunOptions {
                command: Some(crate::commands::run::InputCommand {
                    id: id.clone(),
                    payload_hash,
                }),
                ..Default::default()
            },
        );
        match result {
            Ok("started") => {}
            Ok(_) => store.cancel_review(&id, "原会话正在处理其他输入，本次主动话题未启动")?,
            Err(error) => store.fail_review(&id, &error)?,
        }
    }
    changed(app, &target.identity.session_id);
    Ok(())
}

async fn make_decision(
    app: &tauri::AppHandle,
    target: &Target,
    history: Vec<kanzei_llm::Message>,
    stop: &kanzei_core::CancellationToken,
) -> anyhow::Result<Decision> {
    let config = std::sync::Arc::new(KanzeiConfig::load_at_root(&target.identity.root)?);
    let process = crate::processes::registry::resolve_conversation(
        &app.state::<AppState>(),
        &target.identity.root,
        Some(&target.identity.process_id),
    )
    .map_err(anyhow::Error::msg)?;
    let profile = crate::run::assembly::resolve_profile(
        process.profile.lock_or_recover().as_deref(),
        &config,
    )?;
    let mut harness = crate::run::assembly::build_run_harness(profile, false, None);
    crate::general_chat::add_channel_components(&mut harness, app, &target.identity.root);
    let resolve = kanzei_harness::ResolveCtx {
        profile,
        cwd: target.identity.root.clone(),
        project_root: target.identity.root.clone(),
        config: config.clone(),
    };
    let snapshot = harness.resolve(&resolve)?;
    let agent = snapshot.select_primary_agent(Some("general"))?;
    let selected = process.model.lock_or_recover().clone();
    let model = kanzei_harness::config::resolve_model_chain(
        target.settings.review_model.as_deref(),
        selected.as_deref(),
        &agent.model,
    );
    let resolved = config.resolve_model(&model)?;
    let proxy = crate::run::assembly::resolve_proxy(&config);
    let route = kanzei_core::build_route(&resolved, &proxy).await?;
    let client = crate::run::assembly::new_llm_client(&proxy)?;
    let store = super::open_store()?;
    let topics = store.topics(&target.identity.session_id)?;
    let reviews = store.recent_reviews(&target.identity.session_id)?;
    let payload = planner::context(target, &history, &topics, &reviews, now(), 12_000);
    let query = serde_json::to_string(&payload["history"])?;
    let mut system = vec![
        agent.system.clone(),
        snapshot.system_baseline()?,
        planner::GUIDANCE.into(),
    ];
    if let Some(memory) =
        kanzei_runtime::task_memory_hints(&config, profile, &target.identity.root, &query, None)?
    {
        system.push(memory);
    }
    let request = kanzei_llm::LlmRequest {
        model: resolved.model.clone(),
        system,
        messages: vec![planner::message(payload)?],
        tools: vec![kanzei_llm::ToolSpec {
            name: planner::DECISION_TOOL.into(),
            description: "决定原会话是否值得主动开口，仅选择主题，不写发送正文".into(),
            input_schema: serde_json::to_value(schemars::schema_for!(planner::DecisionCall))?,
        }],
        hosted_tools: Vec::new(),
        max_tokens: config.limits.max_tokens(),
        temperature: None,
        reasoning: kanzei_runtime::resolve_reasoning_override(
            None,
            config.models.reasoning.as_deref(),
        ),
        service_tier: config.service_tier_for(&resolved),
    };
    planner::evaluate(&client, &route, &request, stop).await
}

pub(crate) fn selection_valid(
    app: &tauri::AppHandle,
    session: &str,
    id: &str,
) -> anyhow::Result<bool> {
    let store = super::open_store()?;
    let Some(review) = store.review(id)? else {
        return Ok(false);
    };
    if review.session_id != session || review.state != ReviewState::Selected {
        return Ok(false);
    }
    let target = store
        .target(session)?
        .ok_or_else(|| anyhow::anyhow!("主动话题的原会话不存在"))?;
    Ok(target.settings.enabled
        && target.state == TargetState::Ready
        && target.revision == review.target_revision
        && capture(app, &target.identity)? == review.context
        && channel_ready(app, &target.identity)?)
}

pub(crate) fn bind_run(
    app: &tauri::AppHandle,
    root: &Path,
    session: &str,
    input: &str,
    run: &str,
) -> anyhow::Result<()> {
    if !kanzei_harness::is_social_conversation(root) {
        return Ok(());
    }
    let source = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))?;
    let Some(command) = source
        .input_command_id(session, input)?
        .filter(|id| id.starts_with("initiative:"))
    else {
        return Ok(());
    };
    let store = super::open_store()?;
    let target = store
        .target(session)?
        .ok_or_else(|| anyhow::anyhow!("主动话题缺少原会话"))?;
    store.bind_run(&command, run, &capture(app, &target.identity)?)?;
    changed(app, session);
    Ok(())
}

#[tauri::command]
pub(crate) fn initiative_status(
    app: tauri::AppHandle,
    project_dir: String,
    process_id: String,
) -> Result<Value, String> {
    (||->anyhow::Result<Value>{
        let target=register(&app,Path::new(&project_dir),&process_id,None)?;
        observe(&app,&target.identity.root,&process_id)?;
        let store=super::open_store()?;
        let target=store.target(&target.identity.session_id)?.ok_or_else(||anyhow::anyhow!("原会话不存在"))?;
        let (available, quiet) = if let Some(peer) = kanzei_harness::QqConversation::from_root(&target.identity.root) {
            let policy = crate::qq::initiative_snapshot(&app, &peer)?;
            (policy.available, policy.quiet)
        } else { (channel_ready(&app, &target.identity)?, false) };
        Ok(json!({"target":target,"eligibility":store.eligibility(&target.identity.session_id,now(),&day(),chrono::Local::now().hour())?,"available":available,"channelQuiet":quiet,"topics":store.topics(&target.identity.session_id)?,"reviews":store.recent_reviews(&target.identity.session_id)?}))
    })().map_err(|error|format!("{error:#}"))
}
#[tauri::command]
pub(crate) fn initiative_settings_save(
    app: tauri::AppHandle,
    project_dir: String,
    process_id: String,
    revision: u64,
    settings: Settings,
) -> Result<Value, String> {
    let target = register(&app, Path::new(&project_dir), &process_id, None)
        .map_err(|error| error.to_string())?;
    if target.identity.channel == Channel::QqGroup
        && (settings.quiet_start != 0 || settings.quiet_end != 0)
    {
        return Err("QQ群的安静时段请在群聊规则中修改".into());
    }
    super::open_store()
        .and_then(|store| {
            store.save_settings(&target.identity.session_id, revision, settings, now())
        })
        .map_err(|error| error.to_string())?;
    cancel_review(&app, &target.identity.session_id);
    changed(&app, &target.identity.session_id);
    notify(&app);
    initiative_status(app, project_dir, process_id)
}
#[tauri::command]
pub(crate) fn initiative_topic_close(
    app: tauri::AppHandle,
    project_dir: String,
    process_id: String,
    id: String,
    revision: u64,
) -> Result<Value, String> {
    let target = register(&app, Path::new(&project_dir), &process_id, None)
        .map_err(|error| error.to_string())?;
    let topic = super::open_store()
        .and_then(|store| {
            store.revise_topic(
                &target.identity.session_id,
                &id,
                revision,
                None,
                &format!("user-close:{}", now()),
                now(),
            )
        })
        .map_err(|error| error.to_string())?;
    changed(&app, &target.identity.session_id);
    serde_json::to_value(topic).map_err(|error| error.to_string())
}
