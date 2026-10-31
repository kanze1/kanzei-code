mod api;
mod store;
pub(crate) mod tools;
use crate::{AppState, MutexPoisonExt};
use api::Api;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Mutex, time::Duration};
use tauri::{Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Settings {
    pub auto_browse: bool,
    pub interval_minutes: u64,
    pub quiet_start: u32,
    pub quiet_end: u32,
    pub keyword: String,
    pub model: Option<String>,
    pub daily_comments: u32,
    pub comment_interval_seconds: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_browse: false,
            interval_minutes: 60,
            quiet_start: 0,
            quiet_end: 8,
            keyword: String::new(),
            model: None,
            daily_comments: 30,
            comment_interval_seconds: 120,
        }
    }
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Stored {
    settings: Settings,
    account: u64,
    name: String,
    cookie: String,
    last_auto: i64,
}
struct Challenge {
    id: String,
    key: String,
    image: String,
    status: String,
    expires: i64,
    next_poll: i64,
    polling: bool,
}
#[derive(Default)]
struct Control {
    stored: Stored,
    api: Option<Api>,
    db: Option<store::Store>,
    challenge: Option<Challenge>,
    error: String,
    paused: bool,
}
#[derive(Default)]
pub(crate) struct BiliState {
    control: Mutex<Control>,
    stop: CancellationToken,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}
fn dir() -> Result<PathBuf, String> {
    kanzei_harness::kanzei_home()
        .map(|p| p.join("bilibili"))
        .ok_or("无法确定 B站数据目录".into())
}
fn save(control: &Control) -> Result<(), String> {
    kanzei_base::atomic_file::write_atomic(
        &dir()?.join("settings.json"),
        &serde_json::to_string_pretty(&control.stored).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn db(control: &Control) -> Result<&store::Store, String> {
    control.db.as_ref().ok_or("B站服务未初始化".into())
}
fn connection(state: &BiliState) -> Result<(Api, Stored), String> {
    let c = state.control.lock_or_recover();
    Ok((c.api.clone().ok_or("B站服务未初始化")?, c.stored.clone()))
}
fn observed(state: &BiliState, result: Result<Value, api::Error>) -> Result<Value, String> {
    match result {
        Ok(value) => Ok(value),
        Err(e) => {
            let mut c = state.control.lock_or_recover();
            c.error = e.to_string();
            c.paused |= e.pause();
            Err(e.to_string())
        }
    }
}
fn validate(settings: &Settings) -> Result<(), String> {
    if settings.interval_minutes == 0
        || settings.interval_minutes > 525600
        || settings.quiet_start > 23
        || settings.quiet_end > 23
        || settings.daily_comments == 0
        || settings.comment_interval_seconds > 31_536_000
    {
        return Err("浏览间隔、安静时段或评论配额无效".into());
    }
    Ok(())
}
pub(crate) fn install(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<BiliState>();
    let directory = dir()?;
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let stored = match std::fs::read_to_string(directory.join("settings.json")) {
        Ok(text) => serde_json::from_str::<Stored>(&text).map_err(|e| format!("B站配置损坏: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Stored::default()),
        Err(e) => Err(e.to_string()),
    };
    {
        let mut c = state.control.lock_or_recover();
        c.api = Some(Api::new()?);
        c.db = Some(store::Store::open(&directory.join("activity.db"))?);
        match stored {
            Ok(stored) => match validate(&stored.settings) {
                Ok(()) => c.stored = stored,
                Err(e) => c.error = e,
            },
            Err(e) => c.error = e,
        }
    }
    let owner = app.clone();
    *state.task.lock_or_recover() = Some(tauri::async_runtime::spawn(async move {
        let state = owner.state::<BiliState>();
        let mut timer = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {_=state.stop.cancelled()=>break,_=timer.tick()=>{
                let due={let c=state.control.lock_or_recover();let s=&c.stored.settings;use chrono::Timelike;let hour=chrono::Local::now().hour();let quiet=if s.quiet_start==s.quiet_end{false}else if s.quiet_start<s.quiet_end{hour>=s.quiet_start&&hour<s.quiet_end}else{hour>=s.quiet_start||hour<s.quiet_end};s.auto_browse&&!c.paused&&c.stored.account>0&&!quiet&&chrono::Utc::now().timestamp_millis()-c.stored.last_auto>=(s.interval_minutes as i64)*60_000};
                if due{if let Err(e)=start_browse(&owner,true){let mut c=state.control.lock_or_recover();c.error=e;c.paused=true;let _=owner.emit("kz:bili",json!({"changed":true}));}}
            }}
        }
    }));
    Ok(())
}
pub(crate) async fn shutdown(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<BiliState>();
    state.stop.cancel();
    let task = state.task.lock_or_recover().take();
    if let Some(task) = task {
        task.await.map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub(crate) fn bili_status(state: State<'_, BiliState>) -> Result<Value, String> {
    let c = state.control.lock_or_recover();
    let conversation = if c.stored.account > 0 {
        Some(crate::normalized_project_root(
            &kanzei_harness::bilibili_conversation_root(&c.stored.account.to_string())
                .map_err(|e| e.to_string())?,
        ))
    } else {
        None
    };
    Ok(
        json!({"settings":c.stored.settings,"account":c.stored.account,"name":c.stored.name,"logged_in":!c.stored.cookie.is_empty(),"error":c.error,"paused":c.paused,"conversation_root":conversation,"records":db(&c)?.records()?}),
    )
}
#[tauri::command]
pub(crate) fn bili_settings_save(
    state: State<'_, BiliState>,
    settings: crate::bili::Settings,
) -> Result<(), String> {
    validate(&settings)?;
    let mut c = state.control.lock_or_recover();
    let old = c.stored.clone();
    c.stored.settings = settings;
    if let Err(e) = save(&c) {
        c.stored = old;
        return Err(e);
    }
    c.paused = false;
    c.error.clear();
    Ok(())
}
async fn accept_credentials(
    state: &BiliState,
    cookie: String,
    challenge: Option<&str>,
) -> Result<Value, String> {
    api::validate_cookie(&cookie)?;
    let (api, _) = connection(state)?;
    let nav = observed(state, api.account(&cookie).await)?;
    let account = nav["mid"].as_u64().ok_or("B站账户编号无效")?;
    let name = nav["uname"].as_str().ok_or("B站账户名称无效")?.to_string();
    let mut c = state.control.lock_or_recover();
    if let Some(id) = challenge {
        if c.challenge.as_ref().is_none_or(|q| {
            q.id != id
                || q.expires < chrono::Utc::now().timestamp_millis()
                || !matches!(q.status.as_str(), "waiting" | "scanned")
        }) {
            return Err("二维码登录已取消或过期，原账号未改变".into());
        }
    }
    let old = c.stored.clone();
    c.stored.account = account;
    c.stored.name = name.clone();
    c.stored.cookie = cookie;
    c.paused = false;
    c.error.clear();
    if let Err(e) = save(&c) {
        c.stored = old;
        return Err(e);
    }
    if let Some(q) = c.challenge.as_mut() {
        q.status = if challenge.is_some() {
            "success"
        } else {
            "cancelled"
        }
        .into();
        q.key.clear();
        q.image.clear();
    }
    Ok(json!({"account":account,"name":name}))
}
#[tauri::command]
pub(crate) async fn bili_cookie_save(
    state: State<'_, BiliState>,
    cookie: String,
) -> Result<Value, String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let id = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    state.control.lock_or_recover().challenge = Some(Challenge {
        id: id.clone(),
        key: String::new(),
        image: String::new(),
        status: "waiting".into(),
        expires: chrono::Utc::now().timestamp_millis() + 180_000,
        next_poll: 0,
        polling: true,
    });
    accept_credentials(&state, cookie, Some(&id)).await
}
#[tauri::command]
pub(crate) fn bili_logout(state: State<'_, BiliState>) -> Result<(), String> {
    let mut c = state.control.lock_or_recover();
    let old = c.stored.clone();
    c.stored.cookie.clear();
    c.stored.account = 0;
    c.stored.name.clear();
    c.stored.settings.auto_browse = false;
    c.challenge = None;
    if let Err(e) = save(&c) {
        c.stored = old;
        return Err(e);
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn bili_login_start(state: State<'_, BiliState>) -> Result<Value, String> {
    let (api, _) = connection(&state)?;
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let id = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    state.control.lock_or_recover().challenge = Some(Challenge {
        id: id.clone(),
        key: String::new(),
        image: String::new(),
        status: "starting".into(),
        expires: 0,
        next_poll: 0,
        polling: false,
    });
    let data = observed(
        &state,
        api.get("/x/passport-login/web/qrcode/generate", "", &[])
            .await,
    )?;
    let url = data["url"].as_str().ok_or("B站未返回二维码")?;
    let parsed = reqwest::Url::parse(url).map_err(|_| "B站二维码地址无效")?;
    if parsed.scheme() != "https"
        || !matches!(
            parsed.host_str(),
            Some("passport.bilibili.com" | "account.bilibili.com")
        )
    {
        return Err("B站二维码来源无效".into());
    }
    let key = data["qrcode_key"]
        .as_str()
        .ok_or("B站未返回二维码标识")?
        .to_string();
    let image = crate::login_qr::image(url)?;
    let expires = chrono::Utc::now().timestamp_millis() + 180_000;
    let mut c = state.control.lock_or_recover();
    if c.challenge
        .as_ref()
        .is_none_or(|q| q.id != id || q.status != "starting")
    {
        return Err("二维码登录已被替换或取消".into());
    }
    c.challenge = Some(Challenge {
        id: id.clone(),
        key,
        image: image.clone(),
        status: "waiting".into(),
        expires,
        next_poll: 0,
        polling: false,
    });
    Ok(json!({"id":id,"status":"waiting","image":image,"expires":expires}))
}
#[tauri::command]
pub(crate) fn bili_login_cancel(state: State<'_, BiliState>, id: String) -> Result<(), String> {
    let mut c = state.control.lock_or_recover();
    if let Some(q) = c
        .challenge
        .as_mut()
        .filter(|q| q.id == id && matches!(q.status.as_str(), "starting" | "waiting" | "scanned"))
    {
        q.status = "cancelled".into();
        q.key.clear();
        q.image.clear();
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn bili_login_poll(
    state: State<'_, BiliState>,
    id: String,
) -> Result<Value, String> {
    let (api, key) = {
        let mut c = state.control.lock_or_recover();
        let api = c.api.clone().ok_or("B站服务未初始化")?;
        let q = c
            .challenge
            .as_mut()
            .filter(|q| q.id == id)
            .ok_or("二维码登录已失效")?;
        let now = chrono::Utc::now().timestamp_millis();
        if now >= q.expires {
            q.status = "expired".into();
            q.key.clear();
            q.image.clear();
        }
        if !matches!(q.status.as_str(), "waiting" | "scanned") || q.polling || now < q.next_poll {
            return Ok(json!({"id":id,"status":q.status}));
        }
        q.next_poll = now + 2000;
        q.polling = true;
        (api, q.key.clone())
    };
    let response = api
        .request(
            "/x/passport-login/web/qrcode/poll",
            "",
            &[("qrcode_key".into(), key)],
            false,
        )
        .await;
    {
        let mut c = state.control.lock_or_recover();
        let q = c
            .challenge
            .as_mut()
            .filter(|q| q.id == id && matches!(q.status.as_str(), "waiting" | "scanned"))
            .ok_or("二维码登录已取消或替换")?;
        q.polling = false;
        if response.is_err() {
            q.status = "error".into();
            q.key.clear();
            q.image.clear();
        }
    }
    let (data, cookies) = response.map_err(String::from)?;
    let status = match data["code"].as_i64() {
        Some(86101) => "waiting",
        Some(86090) => "scanned",
        Some(86038) => "expired",
        Some(0) => {
            let cookie = cookies
                .iter()
                .filter_map(|s| s.split(';').next())
                .filter(|s| {
                    s.starts_with("SESSDATA=")
                        || s.starts_with("bili_jct=")
                        || s.starts_with("buvid3=")
                })
                .collect::<Vec<_>>()
                .join("; ");
            accept_credentials(&state, cookie, Some(&id)).await?;
            return Ok(json!({"id":id,"status":"success"}));
        }
        _ => return Err("B站返回了未知登录状态".into()),
    };
    let mut c = state.control.lock_or_recover();
    let q = c
        .challenge
        .as_mut()
        .filter(|q| q.id == id)
        .ok_or("二维码登录已取消")?;
    if q.status == "cancelled" {
        return Err("二维码登录已取消".into());
    }
    q.status = status.into();
    if !matches!(status, "waiting" | "scanned") {
        q.key.clear();
        q.image.clear();
    }
    Ok(json!({"id":id,"status":status}))
}
#[tauri::command]
pub(crate) async fn bili_read(
    state: State<'_, BiliState>,
    action: String,
    bvid: Option<String>,
    keyword: Option<String>,
    page: Option<u32>,
    root: Option<u64>,
) -> Result<Value, String> {
    let (api, stored) = connection(&state)?;
    let page = page.unwrap_or(1);
    let result = match action.as_str() {
        "feed" => {
            api.feed(&stored.cookie, keyword.as_deref().unwrap_or(""), page)
                .await
        }
        "video" => {
            api.video(&stored.cookie, bvid.as_deref().ok_or("需要 BV 编号")?)
                .await
        }
        "comments" => {
            api.comments(
                &stored.cookie,
                bvid.as_deref().ok_or("需要 BV 编号")?,
                page,
                root,
            )
            .await
        }
        "transcript" => {
            api.transcript(&stored.cookie, bvid.as_deref().ok_or("需要 BV 编号")?)
                .await
        }
        _ => return Err("未知 B站读取操作".into()),
    };
    let data = observed(&state, result)?;
    if action == "video" || action == "transcript" {
        let video = if action == "video" {
            &data
        } else {
            &data["video"]
        };
        db(&state.control.lock_or_recover())?.browsed(
            stored.account,
            video["bvid"].as_str().ok_or("视频缺少 BV 编号")?,
            video["title"].as_str().ok_or("视频缺少标题")?,
            &action,
        )?;
    }
    Ok(data)
}
#[tauri::command]
pub(crate) async fn bili_draft_create(
    state: State<'_, BiliState>,
    bvid: String,
    body: String,
    root: Option<u64>,
    parent: Option<u64>,
) -> Result<Value, String> {
    if body.trim().is_empty() {
        return Err("评论正文不能为空".into());
    }
    let (api, stored) = connection(&state)?;
    if stored.account == 0 {
        return Err("请先登录 B站".into());
    }
    let video = observed(&state, api.video(&stored.cookie, &bvid).await)?;
    let aid = video["aid"].as_u64().ok_or("视频编号无效")?;
    if let Some(root) = root {
        let thread = observed(
            &state,
            api.comments(&stored.cookie, &bvid, 1, Some(root)).await,
        )?;
        let thread = &thread["comments"];
        if thread["root"]["rpid"].as_u64() != Some(root) {
            return Err("回复目标不属于这个视频".into());
        }
        if let Some(parent) = parent {
            if parent != root
                && !thread["replies"]
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|r| r["rpid"].as_u64() == Some(parent)))
            {
                return Err("目标楼层尚未读取或不属于这个讨论串".into());
            }
        }
    } else if parent.is_some() {
        return Err("回复楼层需要讨论串编号".into());
    }
    db(&state.control.lock_or_recover())?.draft(
        stored.account,
        &bvid,
        aid,
        root,
        parent,
        body.trim(),
    )
}
#[tauri::command]
pub(crate) fn bili_draft_edit(
    state: State<'_, BiliState>,
    id: String,
    body: String,
    revision: String,
) -> Result<Value, String> {
    if body.trim().is_empty() {
        return Err("评论正文不能为空".into());
    }
    db(&state.control.lock_or_recover())?.edit(&id, body.trim(), &revision)?;
    Ok(json!({"revision": kanzei_base::content_hash(body.trim().as_bytes())}))
}
#[tauri::command]
pub(crate) fn bili_draft_discard(state: State<'_, BiliState>, id: String) -> Result<(), String> {
    db(&state.control.lock_or_recover())?.discard(&id)
}
#[tauri::command]
pub(crate) async fn bili_publish(
    state: State<'_, BiliState>,
    id: String,
    revision: String,
) -> Result<Value, String> {
    let (api, stored) = connection(&state)?;
    if stored.account == 0 {
        return Err("请先登录 B站".into());
    }
    let nav = observed(&state, api.account(&stored.cookie).await)?;
    if nav["mid"] != stored.account {
        return Err("B站登录账号发生变化，未发送".into());
    }
    let draft = {
        let c = state.control.lock_or_recover();
        if c.stored.account != stored.account || c.stored.cookie != stored.cookie {
            return Err("发布前登录状态发生变化，未发送".into());
        }
        db(&c)?.begin(
            &id,
            stored.account,
            &revision,
            stored.settings.daily_comments,
            stored.settings.comment_interval_seconds,
        )?
    };
    let result = api
        .publish(
            &stored.cookie,
            draft["aid"].as_u64().ok_or("草稿视频编号无效")?,
            draft["body"].as_str().ok_or("草稿正文无效")?,
            draft["root"].as_u64(),
            draft["parent"].as_u64(),
        )
        .await;
    let result = match result {
        Ok(value)
            if value["rpid"].as_u64().is_some()
                || value["rpid_str"].as_str().is_some_and(|s| !s.is_empty()) =>
        {
            Ok(value)
        }
        Ok(_) => Err(api::Error::Transport),
        Err(e) => Err(e),
    };
    {
        let mut c = state.control.lock_or_recover();
        match &result {
            Ok(value) => db(&c)?.finish(&id, "sent", Some(value), None)?,
            Err(e) => {
                db(&c)?.finish(
                    &id,
                    if e.uncertain() { "unknown" } else { "failed" },
                    None,
                    Some(&e.to_string()),
                )?;
                c.error = e.to_string();
                c.paused |= e.pause();
            }
        }
    }
    result.map_err(String::from)
}
#[tauri::command]
pub(crate) fn bili_conversation_open(app: tauri::AppHandle) -> Result<Value, String> {
    let state = app.state::<BiliState>();
    let account = state.control.lock_or_recover().stored.account;
    if account == 0 {
        return Err("请先登录 B站".into());
    }
    let root = kanzei_harness::bilibili_conversation_root(&account.to_string())
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let root = crate::normalized_project_root(&root);
    let process =
        crate::processes::registry::ensure_channel_conversation(&app.state::<AppState>(), &root)?;
    Ok(json!({"root":root,"process_id":process.id}))
}
fn start_browse(app: &tauri::AppHandle, automatic: bool) -> Result<Value, String> {
    let qq = app.state::<BiliState>();
    let (_, stored) = connection(&qq)?;
    let target = bili_conversation_open(app.clone())?;
    let root = target["root"].as_str().ok_or("B站会话路径无效")?;
    let process = target["process_id"].as_str().ok_or("B站会话缺少编号")?;
    let session = crate::process_session_id(std::path::Path::new(root), Some(process));
    let state = app.state::<AppState>();
    if automatic
        && state
            .runtimes
            .lock_or_recover()
            .get(&session)
            .is_some_and(|r| r.is_running())
    {
        return Ok(target);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let prompt = if stored.settings.keyword.trim().is_empty() {
        "浏览一轮 B站热门视频，读读视频资料与评论，结合我们的记忆聊聊感兴趣的内容。想回复的内容先保存成评论草稿。".to_string()
    } else {
        format!("围绕「{}」浏览一轮 B站，读读视频资料与评论，结合我们的记忆聊聊感兴趣的内容。想回复的内容先保存成评论草稿。", stored.settings.keyword)
    };
    crate::commands::run::schedule_run(
        app.clone(),
        &state,
        root.into(),
        Some(process.into()),
        crate::commands::run::Submission::Prompt {
            prompt: prompt.clone(),
            delivery: kanzei_core::Delivery::Queue,
            attachments: None,
        },
        crate::commands::run::RunOptions {
            command: Some(crate::commands::run::InputCommand {
                id: format!("bili:{}:{now}", stored.account),
                payload_hash: kanzei_core::stable_json_hash(&json!(prompt)),
            }),
            execution_batch: false,
            work_item_id: None,
            profile: Some("dev-pair".into()),
            agent: Some("dev-pair".into()),
            model: stored.settings.model,
            work_priority: None,
            research_topic: None,
            autonomous: false,
            auto_allow: false,
            auto_round: automatic,
        },
    )?;
    {
        let mut c = qq.control.lock_or_recover();
        c.stored.last_auto = now;
        save(&c)?;
    }
    Ok(target)
}
#[tauri::command]
pub(crate) fn bili_browse(app: tauri::AppHandle) -> Result<Value, String> {
    start_browse(&app, false)
}
