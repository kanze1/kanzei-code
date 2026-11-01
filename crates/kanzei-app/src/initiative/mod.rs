//! Proactive conversation policy. Memory remains owned by kanzei-memory and
//! messages by the original session and its channel's delivery store.
pub(crate) mod model;
pub(crate) mod outbound;
mod planner;
pub(crate) mod service;
mod store;
mod tools;

pub(crate) use tools::InitiativeComponent;

fn open_store() -> anyhow::Result<store::Store> {
    let home = kanzei_harness::kanzei_home()
        .ok_or_else(|| anyhow::anyhow!("无法确定主动话题的数据目录"))?;
    store::Store::open(&home.join("initiative/state.db"))
}

fn identity(ctx: &kanzei_harness::ToolCtx) -> anyhow::Result<model::Identity> {
    identity_at(
        &ctx.project_root,
        ctx.session_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("主动话题缺少原会话编号"))?,
        ctx.process_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("主动话题缺少原对话编号"))?,
    )
}

fn identity_at(
    root: &std::path::Path,
    session: &str,
    process: &str,
) -> anyhow::Result<model::Identity> {
    anyhow::ensure!(
        kanzei_harness::is_social_conversation(root),
        "主动话题只属于社交会话"
    );
    anyhow::ensure!(
        crate::process_session_id(root, Some(process)) == session,
        "主动话题会话归属不匹配"
    );
    let channel = match kanzei_harness::QqConversation::from_root(root) {
        Some(peer) if peer.kind == kanzei_harness::QqPeerKind::Group => model::Channel::QqGroup,
        Some(_) => model::Channel::QqFriend,
        None if kanzei_harness::is_bilibili_conversation(root) => model::Channel::Bilibili,
        None => model::Channel::Desktop,
    };
    Ok(model::Identity {
        session_id: session.into(),
        process_id: process.into(),
        root: crate::normalized_project_root(root),
        channel,
    })
}
