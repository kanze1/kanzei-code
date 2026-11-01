//! The final output boundary uses current source facts, not the review snapshot.
use super::{model::*, service};
use crate::AppState;
use std::path::Path;
use tauri::{Emitter, Manager};

fn for_run(root: &Path, session: &str, run: &str) -> anyhow::Result<Option<Review>> {
    if !kanzei_harness::is_social_conversation(root) {
        return Ok(None);
    }
    super::open_store()?.review_for_run(session, run)
}
pub(crate) fn proactive(root: &Path, session: &str, run: &str) -> anyhow::Result<bool> {
    Ok(for_run(root, session, run)?.is_some())
}
pub(crate) fn validate_commit(
    app: &tauri::AppHandle,
    root: &Path,
    session: &str,
    run: &str,
    message: &kanzei_llm::Message,
) -> anyhow::Result<()> {
    let Some(review) = for_run(root, session, run)? else {
        return Ok(());
    };
    // A sent tool result/final acknowledgment remains real history even if a
    // new input arrives after its channel accepted the send.
    if matches!(
        review.state,
        ReviewState::Sent | ReviewState::Sending | ReviewState::Unknown
    ) {
        return Ok(());
    }
    anyhow::ensure!(
        review.state == ReviewState::Running,
        "主动话题已取消，旧输出不再提交"
    );
    let store = super::open_store()?;
    let target = store
        .target(session)?
        .ok_or_else(|| anyhow::anyhow!("主动话题原会话不存在"))?;
    let current = service::capture(app, &target.identity)?;
    use chrono::Timelike;
    let topic = store
        .topic(
            session,
            review
                .topic_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("主动话题缺少身份"))?,
        )?
        .ok_or_else(|| anyhow::anyhow!("主动话题不存在"))?;
    if current != review.context
        || target.revision != review.target_revision
        || !target.settings.enabled
        || target.state != TargetState::Ready
        || !service::channel_ready(app, &target.identity)?
        || target.settings.quiet(chrono::Local::now().hour())
        || Some(topic.revision) != review.topic_revision
        || topic.state != TopicState::Active
        || !topic.plan.available(service::now())
    {
        store.cancel_review(&review.id, "原会话、话题或接收状态已变化，旧输出不再提交")?;
        anyhow::bail!("原会话、话题或接收状态已变化，旧输出不再提交");
    }
    let final_text = message
        .parts
        .iter()
        .any(|part| matches!(part,kanzei_llm::Part::Text{text} if !text.trim().is_empty()))
        && !message
            .parts
            .iter()
            .any(|part| matches!(part, kanzei_llm::Part::ToolCall { .. }));
    if final_text
        && matches!(
            target.identity.channel,
            Channel::Desktop | Channel::Bilibili
        )
    {
        use chrono::Timelike;
        store.begin_send(
            &review.id,
            &current,
            service::now(),
            &service::day(),
            chrono::Local::now().hour(),
        )?;
    }
    Ok(())
}

/// Called under the QQ ingress/delivery lock, after channel checks and friend
/// verification. The supplied sequence belongs to that same locked database.
pub(crate) fn before_qq_send(
    app: &tauri::AppHandle,
    peer: &crate::qq::Peer,
    run: &str,
    sequence: i64,
) -> anyhow::Result<Option<String>> {
    let Some(review) = for_run(&peer.root, &peer.session_id, run)? else {
        return Ok(None);
    };
    let core = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&peer.root))?;
    let human_input = core
        .original_session_inputs(&peer.session_id)?
        .into_iter()
        .rev()
        .find(|input| input.input.origin == kanzei_base::MessageOrigin::Human)
        .map(|input| input.input.input_id);
    let runtime = crate::runtime_for(&app.state::<AppState>(), &peer.session_id);
    anyhow::ensure!(!runtime.stopping(), "原会话正在停止，未发送主动消息");
    let context = Context {
        activity: Activity {
            human_input,
            message_sequence: sequence,
        },
        async_generation: runtime
            .async_generation
            .load(std::sync::atomic::Ordering::SeqCst),
    };
    use chrono::Timelike;
    super::open_store()?.begin_send(
        &review.id,
        &context,
        service::now(),
        &service::day(),
        chrono::Local::now().hour(),
    )?;
    Ok(Some(review.id))
}
pub(crate) fn qq_delivered(
    app: &tauri::AppHandle,
    id: &str,
    outcome: ReviewState,
    receipt: &str,
    error: Option<String>,
) -> anyhow::Result<()> {
    let store = super::open_store()?;
    store.record_delivery(id, outcome, receipt, error, service::now())?;
    if let Some(review) = store.review(id)? {
        service::changed(app, &review.session_id);
    }
    Ok(())
}
pub(crate) fn failed(root: &Path, session: &str, run: &str, error: &str) -> anyhow::Result<()> {
    let Some(review) = for_run(root, session, run)? else {
        return Ok(());
    };
    let store = super::open_store()?;
    if review.state == ReviewState::Sending {
        store.record_delivery(
            &review.id,
            ReviewState::Unknown,
            &format!("run:{run}"),
            Some(error.into()),
            service::now(),
        )?;
    } else {
        store.fail_review(&review.id, error)?;
    }
    Ok(())
}
pub(crate) fn not_sent(root: &Path, session: &str, run: &str, reason: &str) -> anyhow::Result<()> {
    let Some(review) = for_run(root, session, run)? else {
        return Ok(());
    };
    if matches!(review.state, ReviewState::Running | ReviewState::Selected) {
        super::open_store()?.cancel_review(&review.id, reason)?;
    }
    Ok(())
}
pub(crate) fn finished(
    app: &tauri::AppHandle,
    root: &Path,
    session: &str,
    run: &str,
    summary: &kanzei_core::RunSummary,
) -> anyhow::Result<()> {
    let Some(mut review) = for_run(root, session, run)? else {
        return Ok(());
    };
    let store = super::open_store()?;
    let target = store
        .target(session)?
        .ok_or_else(|| anyhow::anyhow!("主动话题原会话不存在"))?;
    if matches!(
        target.identity.channel,
        Channel::Desktop | Channel::Bilibili
    ) && review.state == ReviewState::Sending
    {
        store.record_delivery(
            &review.id,
            if summary.halted_by_user {
                ReviewState::Unknown
            } else {
                ReviewState::Sent
            },
            &format!("session:{session}/run:{run}"),
            summary.halted_by_user.then(|| "输出提交期间已停止".into()),
            service::now(),
        )?;
        review = store
            .review(&review.id)?
            .ok_or_else(|| anyhow::anyhow!("主动评估记录不存在"))?;
    }
    if review.state == ReviewState::Sent && !summary.text.trim().is_empty() {
        app.emit(
            "kz:turn",
            crate::with_session_id(
                serde_json::json!({"step":summary.steps,"maxSteps":0}),
                session,
            ),
        )?;
        app.emit(
            "kz:text",
            crate::with_session_id(serde_json::json!({"text":summary.text}), session),
        )?;
    } else if review.state == ReviewState::Running {
        store.finish_without_message(&review.id, "本轮没有发出主动消息")?;
    }
    service::changed(app, session);
    Ok(())
}
