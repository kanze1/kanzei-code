//! QQ is an input/output adapter for the existing projectless session owner.
//! All peers use engine-owned social workspaces and typed channel delivery.
pub(crate) mod component;
pub(crate) mod config;
mod image;
mod input;
mod media;
mod plain_text;
mod protocol;
mod questions;
pub(crate) mod setup;
mod stickers;
mod store;

use crate::{AppState, MutexPoisonExt};
use chrono::Timelike;
pub(crate) use component::ConversationComponent;
use kanzei_harness::{QqConversation, QqPeerKind};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Mutex, time::Duration};
use tauri::{Emitter, Listener, Manager, State};
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;

use config::Stored;
use input::{mentions_self, text};
pub(crate) use store::Peer;
use store::Store;

/// The channel cursor and reset commit share the same receive lock. A new
/// incoming prompt cannot capture pre-reset recent chat after this boundary.
pub(crate) fn clear_context(
    qq: &QqState,
    state: &AppState,
    project: &str,
    process: Option<&str>,
) -> Result<(), String> {
    let root = crate::normalized_project_root(std::path::Path::new(project));
    let Some(identity) = QqConversation::from_root(&root) else {
        return crate::conversation::clear_conversation(state, project, process);
    };
    let _transition = qq.context_transition.lock_or_recover();
    let floor = {
        let control = qq.control.lock_or_recover();
        database(&control)
            .and_then(|db| db.message_watermark(&identity))
            .map_err(|e| e.to_string())?
    };
    crate::conversation::clear_conversation_at(state, project, process, Some(floor))
}

fn context_message_floor(root: &std::path::Path, session: &str) -> anyhow::Result<i64> {
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root))?;
    let resets = store.list_events_by_type(session, 0, "conversation.reset")?;
    Ok(resets
        .last()
        .and_then(|event| event.payload["qq_message_floor"].as_i64())
        .unwrap_or(0))
}

#[derive(Default)]
struct Control {
    config: Stored,
    store: Option<Store>,
    client: Option<protocol::Client>,
    account: String,
    nickname: String,
    connected: bool,
    error: String,
}

#[derive(Default)]
pub(crate) struct QqState {
    setup: setup::SetupState,
    control: Mutex<Control>,
    context_transition: Mutex<()>,
    changed: Notify,
    stop: CancellationToken,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

fn directory() -> Result<PathBuf, String> {
    kanzei_harness::kanzei_home()
        .map(|p| p.join("qq"))
        .ok_or_else(|| "无法确定 QQ 存储目录".into())
}
fn database(control: &Control) -> anyhow::Result<&Store> {
    control
        .store
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("QQ 服务未初始化"))
}
fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
pub(crate) struct InitiativeSnapshot {
    pub available: bool,
    pub quiet: bool,
    pub legacy_minutes: u32,
    pub message_sequence: i64,
}
pub(crate) fn initiative_snapshot(
    app: &tauri::AppHandle,
    identity: &QqConversation,
) -> anyhow::Result<InitiativeSnapshot> {
    let qq = app.state::<QqState>();
    let c = qq.control.lock_or_recover();
    let rule = c
        .config
        .settings
        .groups
        .iter()
        .find(|group| group.id == identity.peer);
    let available = c.config.settings.enabled
        && c.connected
        && c.account == identity.account
        && match identity.kind {
            QqPeerKind::Friend => c.config.settings.friends_enabled,
            QqPeerKind::Group => rule.is_some_and(|group| group.enabled),
        };
    let quiet = identity.kind == QqPeerKind::Group
        && rule.is_some_and(|group| group.quiet(chrono::Local::now().hour()));
    let legacy_minutes = if identity.kind == QqPeerKind::Group {
        rule.map(|rule| u32::try_from(rule.proactive_minutes))
            .transpose()?
            .unwrap_or(0)
    } else {
        0
    };
    Ok(InitiativeSnapshot {
        available,
        quiet,
        legacy_minutes,
        message_sequence: database(&c)?.latest_sequence(identity)?,
    })
}
fn error(app: &tauri::AppHandle, message: impl ToString) {
    let state = app.state::<QqState>();
    state.control.lock_or_recover().error = message.to_string();
    let _ = app.emit("kz:qq", json!({"changed":true}));
}

pub(crate) fn install(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<QqState>();
    let dir = directory()?;
    let store = Store::open(&dir.join("transport.db")).map_err(|e| e.to_string())?;
    store.recover_deliveries().map_err(|e| e.to_string())?;
    let path = dir.join("settings.json");
    let config = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<Stored>(&text).map_err(|e| format!("QQ 配置损坏: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Stored::default()),
        Err(e) => Err(e.to_string()),
    };
    {
        let mut control = state.control.lock_or_recover();
        control.store = Some(store);
        match config {
            Ok(config) => match config.validate() {
                Ok(()) => control.config = config,
                Err(message) => control.error = message,
            },
            Err(message) => control.error = message,
        }
    }
    let owner = app.clone();
    *state.task.lock_or_recover() = Some(tauri::async_runtime::spawn(async move {
        service(owner).await;
    }));
    let owner = app.clone();
    app.listen("kz:ask", move |event| {
        let value: Value = match serde_json::from_str(event.payload()) {
            Ok(value) => value,
            Err(e) => {
                error(&owner, e);
                return;
            }
        };
        let owner = owner.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = forward_ask(&owner, value).await {
                error(&owner, e);
            }
        });
    });
    Ok(())
}

pub(crate) async fn shutdown(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<QqState>();
    state.stop.cancel();
    state.changed.notify_one();
    let task = state.task.lock_or_recover().take();
    if let Some(task) = task {
        task.await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn service(app: tauri::AppHandle) {
    let state = app.state::<QqState>();
    loop {
        let config = state.control.lock_or_recover().config.clone();
        if state.stop.is_cancelled() {
            return;
        }
        if !config.settings.enabled {
            tokio::select! {_=state.stop.cancelled()=>return,_=state.changed.notified()=>continue}
        }
        let cancel = state.stop.child_token();
        let (events_tx, mut events) = mpsc::unbounded_channel();
        let (client, task) = protocol::start(
            config.settings.url.clone(),
            config.token.clone(),
            config.settings.account.clone(),
            cancel.clone(),
            events_tx,
        );
        state.control.lock_or_recover().client = Some(client.clone());
        let mut clock = tokio::time::interval(Duration::from_secs(1));
        let mut reconfigure = false;
        loop {
            tokio::select! {
                _=state.stop.cancelled()=>break,
                _=state.changed.notified()=>{reconfigure=true;break;},
                _=clock.tick()=>{if let Err(e)=tick(&app,&client).await {error(&app,e);}},
                event=events.recv()=>match event {
                    Some(protocol::Event::Connected(info))=>{
                        match protocol::id(&info["user_id"]) {
                            Ok(account) if config.settings.account.is_empty()||config.settings.account==account=>{
                                {let mut c=state.control.lock_or_recover();c.account=account;c.nickname=info["nickname"].as_str().unwrap_or("").into();c.connected=true;c.error.clear();}
                                let _=app.emit("kz:qq",json!({"changed":true}));
                            },
                            Ok(_)=>{error(&app,"NapCat 登录账号与配置不一致，连接已停止");break;},
                            Err(e)=>{error(&app,e);break;}
                        }
                    },
                    Some(protocol::Event::Offline(message))=>{state.control.lock_or_recover().connected=false;error(&app,message);},
                    Some(protocol::Event::Incoming(value))=>{if let Err(e)=incoming(&app,&client,value).await {error(&app,e);}},
                    None=>break,
                }
            }
        }
        cancel.cancel();
        if let Err(e) = task.await {
            error(&app, e);
        }
        {
            let mut c = state.control.lock_or_recover();
            c.connected = false;
            c.client = None;
        }
        let _ = app.emit("kz:qq", json!({"changed":true}));
        if !state.stop.is_cancelled()
            && !reconfigure
            && state.control.lock_or_recover().config.settings.enabled
        {
            tokio::select! {_=state.stop.cancelled()=>return,_=state.changed.notified()=>{}}
        }
    }
}

async fn friend(client: &protocol::Client, id: &str) -> Result<bool, String> {
    let list = client.call("get_friend_list", json!({})).await?;
    let rows = list.as_array().ok_or("OneBot 好友列表格式无效")?;
    for row in rows {
        if protocol::id(&row["user_id"])? == id {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ensure_peer(
    app: &tauri::AppHandle,
    identity: QqConversation,
    title: &str,
) -> anyhow::Result<Peer> {
    let stored = {
        let qq = app.state::<QqState>();
        let c = qq.control.lock_or_recover();
        database(&c)?
            .peers()?
            .into_iter()
            .find(|p| p.identity == identity)
    };
    let title = if title == identity.peer {
        stored.as_ref().map(|p| p.title.as_str()).unwrap_or(title)
    } else {
        title
    };
    let root = identity.root()?;
    let state = app.state::<AppState>();
    let process = crate::processes::registry::ensure_channel_conversation(&state, &root)
        .map_err(anyhow::Error::msg)?;
    let root = process.project_dir.0.clone();
    let session_id = crate::process_session_id(&root, Some(&process.id));
    let peer = Peer {
        identity,
        title: title.into(),
        process_id: process.id.clone(),
        session_id,
        root,
        watermark: 0,
        last_started: 0,
    };
    let qq = app.state::<QqState>();
    let c = qq.control.lock_or_recover();
    database(&c)?.register(&peer)?;
    let peer = database(&c)?
        .peers()?
        .into_iter()
        .find(|p| p.identity == peer.identity)
        .ok_or_else(|| anyhow::anyhow!("QQ 会话登记失败"))?;
    drop(c);
    crate::initiative::service::register(app, &peer.root, &peer.process_id, Some(&peer.title))?;
    Ok(peer)
}

#[cfg(test)]
mod mention_tests {
    use super::*;
    #[test]
    fn incoming_media_keeps_platform_metadata_instead_of_disappearing() {
        let body = text(&json!({"message":[
            {"type":"face","data":{"id":14,"name":"微笑"}},
            {"type":"mface","data":{"emoji_package_id":123,"emoji_id":"actual-id","key":"actual-key"}},
            {"type":"image","data":{"url":"https://example.com/image.gif","sub_type":1,"summary":"庆祝"}},
            {"type":"file","data":{"file_id":"actual-file","name":"report.pdf","file_size":1234}}
        ]}));
        for value in [
            "微笑",
            "actual-id",
            "actual-key",
            "image.gif",
            "庆祝",
            "actual-file",
            "report.pdf",
            "1234",
        ] {
            assert!(body.contains(value), "{body}");
        }
    }
    #[test]
    fn only_protocol_mentions_to_connected_account_wake_group() {
        for id in [json!(100), json!("100")] {
            assert!(mentions_self(
                &json!({"message":[{"type":"at","data":{"qq":id}},{"type":"text","data":{"text":"hi"}}]}),
                "100"
            ));
        }
        for value in [
            json!({"message":[{"type":"at","data":{"qq":"all"}}]}),
            json!({"message":[{"type":"at","data":{"qq":101}}]}),
            json!({"message":[{"type":"text","data":{"text":"@100 [CQ:at,qq=100]"}}],"raw_message":"[CQ:at,qq=100]"}),
            json!({"message":"@100 hi"}),
            json!({"message":"[CQ:at,qq=1000] hi"}),
            json!({"message":"[CQ:at,qq=all] hi"}),
        ] {
            assert!(!mentions_self(&value, "100"), "{value}");
        }
        assert!(mentions_self(
            &json!({"message":"[CQ:at,qq=100,name=test] hi"}),
            "100"
        ));
        assert!(mentions_self(
            &json!({"raw_message":"[CQ:at,name=test,qq=100] hi"}),
            "100"
        ));
    }
}

async fn incoming(
    app: &tauri::AppHandle,
    client: &protocol::Client,
    value: Value,
) -> anyhow::Result<()> {
    let qq = app.state::<QqState>();
    let (account, settings) = {
        let c = qq.control.lock_or_recover();
        if !c.connected {
            return Ok(());
        }
        (c.account.clone(), c.config.settings.clone())
    };
    if protocol::id(&value["self_id"]).map_err(anyhow::Error::msg)? != account {
        anyhow::bail!("QQ 消息账号与连接身份不一致");
    }
    let sender = protocol::id(&value["user_id"]).map_err(anyhow::Error::msg)?;
    if sender == account {
        return Ok(());
    }
    let kind = match value["message_type"].as_str() {
        Some("private") => QqPeerKind::Friend,
        Some("group") => QqPeerKind::Group,
        _ => return Ok(()),
    };
    let peer = if kind == QqPeerKind::Friend {
        if !settings.friends_enabled
            || !friend(client, &sender).await.map_err(anyhow::Error::msg)?
        {
            return Ok(());
        }
        sender.clone()
    } else {
        let id = protocol::id(&value["group_id"]).map_err(anyhow::Error::msg)?;
        if !settings.groups.iter().any(|g| g.id == id && g.enabled) {
            return Ok(());
        }
        id
    };
    let identity = QqConversation::new(&account, kind, &peer)?;
    let nickname = value["sender"]["card"]
        .as_str()
        .filter(|v| !v.is_empty())
        .or_else(|| value["sender"]["nickname"].as_str())
        .unwrap_or(&sender);
    let title = if kind == QqPeerKind::Group {
        settings
            .groups
            .iter()
            .find(|g| g.id == peer)
            .map(|g| g.name.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&peer)
    } else {
        nickname
    };
    let peer = ensure_peer(app, identity, title)?;
    let body = text(&value);
    let images = input::images(&value);
    let message_id = value["message_id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| value["message_id"].as_i64().map(|id| id.to_string()))
        .ok_or_else(|| anyhow::anyhow!("QQ 消息缺少 message_id"))?;
    let control_command = kind == QqPeerKind::Friend && is_control_command(&body);
    let fresh = {
        let _transition = qq.context_transition.lock_or_recover();
        let mut c = qq.control.lock_or_recover();
        let db = c
            .store
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("QQ 服务未初始化"))?;
        let wake_prompt = if (kind == QqPeerKind::Friend && !control_command)
            || (kind == QqPeerKind::Group && mentions_self(&value, &account))
        {
            Some(serde_json::to_string(
                &json!({"message_id":message_id,"sender":sender,"nickname":nickname,"text":body,"images":images}),
            )?)
        } else {
            None
        };
        db.receive(
            &peer.identity,
            &store::Message {
                sequence: 0,
                message_id,
                sender: sender.clone(),
                nickname: nickname.into(),
                text: body.clone(),
                images,
                at: now(),
            },
            wake_prompt.as_deref(),
        )?
    };
    if !fresh {
        return Ok(());
    }
    crate::initiative::service::before_human_input(app, &peer.root, &peer.session_id)?;
    crate::initiative::service::observe(app, &peer.root, &peer.process_id)?;
    let _ = app.emit("kz:qq", json!({"changed":true,"sessionId":peer.session_id}));
    if control_command {
        handle_command(app, &peer, &body).await?;
    }
    Ok(())
}

async fn tick(app: &tauri::AppHandle, client: &protocol::Client) -> anyhow::Result<()> {
    let qq = app.state::<QqState>();
    let (account, settings, peers) = {
        let c = qq.control.lock_or_recover();
        if !c.connected {
            return Ok(());
        }
        (
            c.account.clone(),
            c.config.settings.clone(),
            database(&c)?.peers()?,
        )
    };
    for peer in peers
        .iter()
        .filter(|p| p.identity.account == account && p.identity.kind == QqPeerKind::Group)
    {
        let Some(rule) = settings
            .groups
            .iter()
            .find(|g| g.id == peer.identity.peer && g.enabled)
        else {
            continue;
        };
        let state = app.state::<AppState>();
        if state
            .runtimes
            .lock_or_recover()
            .get(&peer.session_id)
            .is_some_and(|r| r.is_running())
            || rule.quiet(chrono::Local::now().hour())
        {
            continue;
        }
        if now() - peer.last_started < (rule.interval_seconds as i64) * 1000 {
            continue;
        }
        let c = qq.control.lock_or_recover();
        let floor = context_message_floor(&peer.root, &peer.session_id)?;
        let records = database(&c)?.messages(&peer.identity, peer.watermark.max(floor))?;
        if records.is_empty() {
            continue;
        }
        let watermark = records.last().map(|m| m.sequence).unwrap_or(peer.watermark);
        let prompt = serde_json::to_string(&records)?;
        database(&c)?.prepare_job(peer, watermark, &prompt)?;
    }
    let jobs = {
        let c = qq.control.lock_or_recover();
        database(&c)?.pending_jobs()?
    };
    for job in jobs {
        if job.identity.account != account {
            continue;
        }
        let permitted = match job.identity.kind {
            QqPeerKind::Friend => {
                settings.friends_enabled
                    && friend(client, &job.identity.peer)
                        .await
                        .map_err(anyhow::Error::msg)?
            }
            QqPeerKind::Group => settings.groups.iter().any(|g| {
                g.id == job.identity.peer
                    && g.enabled
                    && (job.trigger == store::Trigger::Direct
                        || !g.quiet(chrono::Local::now().hour()))
            }),
        };
        if !permitted {
            continue;
        }
        let peer = ensure_peer(app, job.identity.clone(), &job.identity.peer)?;
        let floor = context_message_floor(&peer.root, &peer.session_id)?;
        if job.watermark <= floor {
            database(&qq.control.lock_or_recover())?.supersede_job(&job)?;
            continue;
        }
        let state = app.state::<AppState>();
        if job.identity.kind == QqPeerKind::Group
            && job.trigger != store::Trigger::Direct
            && state
                .runtimes
                .lock_or_recover()
                .get(&peer.session_id)
                .is_some_and(|r| r.is_running())
        {
            continue;
        }
        let input = database(&qq.control.lock_or_recover())?.input_batch(&job, floor)?;
        let prompt = input.prompt(&job)?;
        let (attachments, image_status) =
            image::prepare_input(client, &job.identity, &input, &peer).await?;
        // Loading pixels yields to a possible reset. Revalidate and admit under
        // the same channel lock used to commit a new context boundary.
        let _transition = qq.context_transition.lock_or_recover();
        if job.watermark <= context_message_floor(&peer.root, &peer.session_id)? {
            database(&qq.control.lock_or_recover())?.supersede_job(&job)?;
            continue;
        }
        crate::commands::run::schedule_run(
            app.clone(),
            &state,
            peer.root.display().to_string(),
            Some(peer.process_id),
            crate::commands::run::Submission::Prompt {
                prompt: format!("{prompt}{image_status}"),
                delivery: kanzei_core::Delivery::Queue,
                attachments: Some(attachments),
            },
            crate::commands::run::RunOptions {
                command: Some(crate::commands::run::InputCommand {
                    id: job.id.clone(),
                    payload_hash: kanzei_core::stable_json_hash(&json!(job.payload)),
                }),
                execution_batch: false,
                work_item_id: None,
                profile: Some("dev-pair".into()),
                agent: Some("dev-pair".into()),
                model: settings.model.clone(),
                work_priority: None,
                research_topic: None,
                autonomous: false,
                auto_allow: false,
                auto_round: job.trigger != store::Trigger::Direct,
            },
        )
        .map_err(anyhow::Error::msg)?;
        let mut c = qq.control.lock_or_recover();
        c.store
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("QQ 服务未初始化"))?
            .admitted(&job, now())?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum SendSource<'a> {
    Round(&'a str),
    Question { session: &'a str, id: u64 },
}
fn round_delivery_id(identity: &QqConversation, run: &str) -> String {
    format!(
        "{}:{run}",
        if identity.kind == QqPeerKind::Group {
            "group-round"
        } else {
            "round"
        }
    )
}
async fn send(
    app: &tauri::AppHandle,
    identity: &QqConversation,
    source: SendSource<'_>,
    message: media::PreparedMessage,
) -> Result<Value, String> {
    let id = match source {
        SendSource::Round(run) => round_delivery_id(identity, run),
        SendSource::Question { session, id } => format!("ask:{session}:{id}"),
    };
    let qq = app.state::<QqState>();
    let client = {
        let c = qq.control.lock_or_recover();
        if !c.config.settings.enabled || !c.connected || c.account != identity.account {
            return Err("QQ 账号未连接，未发送".into());
        }
        if identity.kind == QqPeerKind::Group
            && !c
                .config
                .settings
                .groups
                .iter()
                .any(|g| g.id == identity.peer && g.enabled)
        {
            return Err("这个群已暂停，未发送".into());
        }
        if identity.kind == QqPeerKind::Friend && !c.config.settings.friends_enabled {
            return Err("好友私聊已暂停，未发送".into());
        }
        c.client.clone().ok_or("QQ 未连接")?
    };
    if identity.kind == QqPeerKind::Friend && !friend(&client, &identity.peer).await? {
        return Err("对方已不在好友列表，未发送".into());
    }
    let initiative = {
        let c = qq.control.lock_or_recover();
        if !c.config.settings.enabled
            || !c.connected
            || c.account != identity.account
            || (identity.kind == QqPeerKind::Friend && !c.config.settings.friends_enabled)
            || (identity.kind == QqPeerKind::Group
                && !c
                    .config
                    .settings
                    .groups
                    .iter()
                    .any(|g| g.id == identity.peer && g.enabled))
        {
            return Err("QQ 接收规则已改变，未发送".into());
        }
        let db = database(&c).map_err(|error| error.to_string())?;
        if db
            .delivery(&id)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("这条发送已有回执，未重复发送；请查看发送记录".into());
        }
        let initiative = if let SendSource::Round(run) = source {
            let peer = db
                .peers()
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|peer| peer.identity == *identity)
                .ok_or("QQ 发送缺少原会话")?;
            if crate::initiative::outbound::proactive(&peer.root, &peer.session_id, run)
                .map_err(|error| error.to_string())?
                && identity.kind == QqPeerKind::Group
                && c.config.settings.groups.iter().any(|group| {
                    group.id == identity.peer && group.quiet(chrono::Local::now().hour())
                })
            {
                crate::initiative::outbound::not_sent(
                    &peer.root,
                    &peer.session_id,
                    run,
                    "已进入群聊安静时段",
                )
                .map_err(|error| error.to_string())?;
                return Err("已进入群聊安静时段，未发送主动消息".into());
            }
            crate::initiative::outbound::before_qq_send(
                app,
                &peer,
                run,
                db.latest_sequence(identity)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?
        } else {
            None
        };
        if !db
            .begin_send(identity, &id, &message.record, now())
            .map_err(|e| e.to_string())?
        {
            return Err("这条发送已有回执，未重复发送；请查看发送记录".into());
        }
        initiative
    };
    let key = if identity.kind == QqPeerKind::Group {
        "group_id"
    } else {
        "user_id"
    };
    let action = if identity.kind == QqPeerKind::Group {
        "send_group_msg"
    } else {
        "send_private_msg"
    };
    let mut params = json!({"message":message.wire});
    params[key] = json!(identity
        .peer
        .parse::<u64>()
        .map_err(|_| "QQ 对方号码无效")?);
    let result = client.call(action, params).await.and_then(|receipt| {
        if receipt["message_id"].as_i64().is_some()
            || receipt["message_id"]
                .as_str()
                .is_some_and(|v| !v.is_empty())
        {
            Ok(receipt)
        } else {
            Err(protocol::Failure::Unknown(
                "OneBot 未返回发送消息编号，送达结果未知".into(),
            ))
        }
    });
    {
        let c = qq.control.lock_or_recover();
        database(&c)
            .and_then(|db| db.finish_send(&id, &result))
            .map_err(|e| e.to_string())?;
    }
    if let Some(review) = initiative {
        let outcome = match &result {
            Ok(_) => crate::initiative::model::ReviewState::Sent,
            Err(error) if error.state() == "unknown" => {
                crate::initiative::model::ReviewState::Unknown
            }
            Err(_) => crate::initiative::model::ReviewState::Failed,
        };
        crate::initiative::outbound::qq_delivered(
            app,
            &review,
            outcome,
            &format!("qq:{id}"),
            result.as_ref().err().map(ToString::to_string),
        )
        .map_err(|error| error.to_string())?;
    }
    let _ = app.emit("kz:qq", json!({"changed":true}));
    result.map_err(String::from)
}

pub(crate) async fn round_finished(
    app: &tauri::AppHandle,
    root: &std::path::Path,
    run_id: &str,
    summary: &kanzei_core::RunSummary,
) {
    let Some(identity) = QqConversation::from_root(root) else {
        return;
    };
    if identity.kind != QqPeerKind::Friend
        || summary.halted_by_user
        || summary.text.trim().is_empty()
    {
        return;
    }
    // An explicit reply owns this round even if the API rejected it or its
    // outcome is unknown. Completion text must not become a second delivery.
    let delivery = {
        let qq = app.state::<QqState>();
        let c = qq.control.lock_or_recover();
        database(&c).and_then(|db| db.delivery(&round_delivery_id(&identity, run_id)))
    };
    match delivery {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(issue) => {
            error(app, format!("读取 QQ 发送回执失败：{issue}"));
            return;
        }
    }
    let Some(message) = media::PreparedMessage::text(&summary.text) else {
        return;
    };
    let result = send(app, &identity, SendSource::Round(run_id), message).await;
    if let Err(message) = result {
        let recorded = (|| -> anyhow::Result<()> {
            let qq = app.state::<QqState>();
            let c = qq.control.lock_or_recover();
            let peer = database(&c)?
                .peers()?
                .into_iter()
                .find(|peer| peer.identity == identity)
                .ok_or_else(|| anyhow::anyhow!("QQ 发送缺少原会话"))?;
            crate::initiative::outbound::not_sent(root, &peer.session_id, run_id, &message)
        })();
        error(
            app,
            match recorded {
                Ok(()) => message,
                Err(issue) => format!("{message}；记录未发送结局失败：{issue}"),
            },
        );
    }
}

fn is_control_command(body: &str) -> bool {
    matches!(
        body.trim().split(' ').next(),
        Some("/stop" | "/allow" | "/deny" | "/answer")
    )
}

async fn handle_command(app: &tauri::AppHandle, peer: &Peer, body: &str) -> anyhow::Result<bool> {
    if body.trim() == "/stop" {
        crate::commands::run::stop_run(
            app.clone(),
            app.state(),
            peer.root.display().to_string(),
            Some(peer.process_id.clone()),
        )
        .await
        .map_err(anyhow::Error::msg)?;
        return Ok(true);
    }
    let mut parts = body.splitn(3, ' ');
    let command = parts.next().unwrap_or("");
    if !matches!(command, "/allow" | "/deny" | "/answer") {
        return Ok(false);
    }
    let id: u64 = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("应答需要问题编号"))?
        .parse()?;
    let pending = crate::commands::run::pending_asks_get(
        app.state(),
        peer.root.display().to_string(),
        Some(peer.process_id.clone()),
    )
    .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        pending.iter().any(|p| p["id"].as_u64() == Some(id)),
        "问题不属于当前好友会话或已经结束"
    );
    let reply = match command {
        "/allow" => "once".into(),
        "/deny" => "deny".into(),
        _ => parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("回答不能为空"))?
            .into(),
    };
    crate::commands::run::answer_ask(app.clone(), app.state(), id, reply)
        .await
        .map_err(anyhow::Error::msg)?;
    Ok(true)
}

async fn forward_ask(app: &tauri::AppHandle, value: Value) -> anyhow::Result<()> {
    let session = value["sessionId"].as_str().unwrap_or("").to_owned();
    let peer = {
        let qq = app.state::<QqState>();
        let c = qq.control.lock_or_recover();
        database(&c)?
            .peers()?
            .into_iter()
            .find(|p| p.session_id == session && p.identity.kind == QqPeerKind::Friend)
    };
    if let Some(peer) = peer {
        let id = value["id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("QQ 问题缺少编号"))?;
        let text = questions::present(value)?;
        send(
            app,
            &peer.identity,
            SendSource::Question {
                session: &session,
                id,
            },
            media::PreparedMessage::text(&text)
                .ok_or_else(|| anyhow::anyhow!("QQ 问题正文为空"))?,
        )
        .await
        .map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn qq_status(state: State<'_, QqState>) -> Result<Value, String> {
    let c = state.control.lock_or_recover();
    let peers = database(&c)
        .and_then(Store::peers)
        .map_err(|e| e.to_string())?;
    Ok(
        json!({"settings":c.config.settings,"token_configured":!c.config.token.is_empty(),"connected":c.connected,"account":c.account,"nickname":c.nickname,"error":c.error,"peers":peers}),
    )
}

#[tauri::command]
pub(crate) fn qq_config_save(
    state: State<'_, QqState>,
    settings: crate::qq::config::Settings,
    token: Option<String>,
) -> Result<(), String> {
    let config = Stored {
        settings,
        token: token.unwrap_or_else(|| state.control.lock_or_recover().config.token.clone()),
    };
    save_config(&state, config)
}
fn update_config(
    state: &QqState,
    update: impl FnOnce(&mut Stored) -> Result<(), String>,
) -> Result<config::Settings, String> {
    let mut c = state.control.lock_or_recover();
    let mut config = c.config.clone();
    update(&mut config)?;
    config.validate()?;
    kanzei_base::atomic_file::write_atomic(
        &directory()?.join("settings.json"),
        &serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    c.config = config;
    c.error.clear();
    let settings = c.config.settings.clone();
    drop(c);
    state.changed.notify_one();
    Ok(settings)
}
fn save_config(state: &QqState, config: Stored) -> Result<(), String> {
    update_config(state, |stored| {
        *stored = config;
        Ok(())
    })
    .map(|_| ())
}
#[tauri::command]
pub(crate) fn qq_group_save(
    state: State<'_, QqState>,
    group: crate::qq::config::Group,
    existing_id: Option<String>,
) -> Result<config::Settings, String> {
    update_config(&state, |stored| {
        if let Some(id) = existing_id {
            if id != group.id {
                return Err("已保存规则的群号不能修改，请新建规则".into());
            }
            let slot = stored
                .settings
                .groups
                .iter_mut()
                .find(|g| g.id == id)
                .ok_or("这条群规则已被删除，请刷新")?;
            *slot = group;
        } else {
            if stored.settings.groups.iter().any(|g| g.id == group.id) {
                return Err("该群已有规则，请直接编辑".into());
            }
            stored.settings.groups.push(group);
        }
        Ok(())
    })
}
#[tauri::command]
pub(crate) fn qq_group_delete(
    state: State<'_, QqState>,
    id: String,
) -> Result<config::Settings, String> {
    update_config(&state, |stored| {
        let index = stored
            .settings
            .groups
            .iter()
            .position(|g| g.id == id)
            .ok_or("这条群规则已被删除，请刷新")?;
        stored.settings.groups.remove(index);
        Ok(())
    })
}

#[tauri::command]
pub(crate) async fn qq_directory(state: State<'_, QqState>) -> Result<Value, String> {
    let client = state
        .control
        .lock_or_recover()
        .client
        .clone()
        .ok_or("QQ 尚未连接")?;
    let friends = client.call("get_friend_list", json!({})).await?;
    let groups = client.call("get_group_list", json!({})).await?;
    Ok(json!({"friends":friends,"groups":groups}))
}

#[tauri::command]
pub(crate) fn qq_peer_open(
    app: tauri::AppHandle,
    identity: QqConversation,
) -> Result<Value, String> {
    let state = app.state::<QqState>();
    let found = {
        let c = state.control.lock_or_recover();
        database(&c)
            .and_then(Store::peers)
            .map_err(|e| e.to_string())?
            .into_iter()
            .any(|p| p.identity == identity)
    };
    if !found {
        return Err("QQ 会话尚未由真实消息创建".into());
    }
    serde_json::to_value(
        ensure_peer(&app, identity.clone(), &identity.peer).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn qq_peer_log(
    state: State<'_, QqState>,
    identity: QqConversation,
) -> Result<Value, String> {
    let c = state.control.lock_or_recover();
    let db = database(&c).map_err(|e| e.to_string())?;
    Ok(
        json!({"messages":db.recent_messages(&identity).map_err(|e|e.to_string())?,"deliveries":db.deliveries(&identity).map_err(|e|e.to_string())?}),
    )
}
