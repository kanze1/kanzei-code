//! Experimental agent team: durable identities, isolated writers, background work,
//! dependency scheduling and resumable messages. The model and UI use one command API.
pub mod store;
#[cfg(test)]
mod tests;
mod tools;
pub(crate) mod workspace;

use anyhow::{bail, Context, Result};
use futures::FutureExt;
use kanzei_core::store::session_execution::{try_acquire, SessionExecutionGuard};
use kanzei_core::{CancellationToken, DelegationFuture, DelegationHost, RunEvent, SubagentRuntime};
use kanzei_harness::{
    AsyncMailbox, AsyncNotice, ConfigComponent, Harness, MarkdownComponent, ResolveCtx, ToolCtx,
    ToolOutput,
};
use kanzei_llm::{LlmClient, Message, ToolSpec};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use store::{AgentJob, AgentMessage, TeamStore};
use tokio::sync::{Notify, Semaphore};

pub type TeamEvent = Arc<dyn Fn(&AgentJob) + Send + Sync>;
pub type TeamAsk =
    Arc<dyn Fn(&AgentJob, kanzei_core::AskRequest) -> kanzei_core::AskFuture + Send + Sync>;
type ParentAsk = Arc<dyn Fn(kanzei_core::AskRequest) -> kanzei_core::AskFuture + Send + Sync>;
#[derive(Clone)]
pub struct AgentTeam(Arc<Inner>);
// Exact private predicate in the existing failure description; no new schema.
const CLEANUP_FAILED_PREFIX: &str = "后台清理未完成：";
fn cleanup_failed(job: &AgentJob) -> bool {
    job.latest.starts_with(CLEANUP_FAILED_PREFIX)
}
fn cleanup_failure(error: &str) -> String {
    format!("{CLEANUP_FAILED_PREFIX}{error}；明确续做前须成功重试清理")
}
struct Inner {
    root: PathBuf,
    ctx: ToolCtx,
    owner: String,
    store: TeamStore,
    config: Mutex<ResolveCtx>,
    runtime: Mutex<SubagentRuntime>,
    client: LlmClient,
    lifecycle: Mutex<()>,
    active: Mutex<HashMap<String, Arc<ChildWorker>>>,
    parent: Mutex<Vec<Message>>,
    event: Mutex<Option<TeamEvent>>,
    ask_router: Mutex<Option<TeamAsk>>,
    mailbox: Mutex<Option<AsyncMailbox>>,
    child_mailboxes: Mutex<HashMap<String, AsyncMailbox>>,
    child_ledgers: Mutex<HashMap<String, kanzei_harness::ReadLedger>>,
    notification_lock: Mutex<()>,
    changed: Notify,
    slots: Arc<Semaphore>,
    spawn_lock: tokio::sync::Mutex<()>,
}
struct ChildWorker {
    cancel: CancellationToken,
    owner: Arc<SessionExecutionGuard>,
}
impl std::ops::Deref for ChildWorker {
    type Target = CancellationToken;
    fn deref(&self) -> &Self::Target {
        &self.cancel
    }
}
// The registry retains teams after a turn. Only a live worker retains its child
// owner, including cancellation cleanup and callbacks that can still checkpoint.
struct WorkerRegistration {
    team: AgentTeam,
    id: String,
    worker: Arc<ChildWorker>,
}
impl Drop for WorkerRegistration {
    fn drop(&mut self) {
        let mut active = self.team.0.active.lock().unwrap_or_else(|e| e.into_inner());
        if active
            .get(&self.id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.worker))
        {
            active.remove(&self.id);
            self.worker.cancel();
            self.team.0.changed.notify_waiters();
        }
    }
}
static TEAMS: OnceLock<Mutex<HashMap<String, AgentTeam>>> = OnceLock::new();
fn key(root: &Path, owner: &str) -> String {
    format!("{}|{owner}", crate::worktree::worktree_key(root))
}
pub fn find(root: &Path, owner: &str) -> Option<AgentTeam> {
    TEAMS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get(&key(root, owner))
        .cloned()
}
fn get_or_register(
    root: &Path,
    owner: &str,
    create: impl FnOnce() -> Result<AgentTeam>,
) -> Result<AgentTeam> {
    let mut teams = TEAMS.get_or_init(Default::default).lock().unwrap();
    let key = key(root, owner);
    if let Some(team) = teams.get(&key) {
        return Ok(team.clone());
    }
    // Creation only opens/reconciles the store and constructs the owner. It must
    // never emit an event, publish a mailbox notice, or wait for a worker.
    let team = create()?;
    teams.insert(key, team.clone());
    Ok(team)
}

fn try_child_owner(store: &TeamStore, id: &str) -> std::io::Result<SessionExecutionGuard> {
    // Stable across attempts and separate from the parent's round/session owner.
    let identity = serde_json::to_string(&(store.owner(), id)).map_err(std::io::Error::other)?;
    try_acquire(store.state_path(), &format!("team:{identity}"))
}
fn claim_child(store: &TeamStore, id: &str) -> Result<Arc<SessionExecutionGuard>> {
    try_child_owner(store, id).map(Arc::new).map_err(|error| {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            anyhow::anyhow!("子任务 {id} 正由其他执行者运行，暂不能接管")
        } else {
            anyhow::Error::new(error).context(format!("取得子任务 {id} 执行所有权失败"))
        }
    })
}
fn recover_interrupted(store: &TeamStore) -> Result<()> {
    for job in store.list()? {
        if job.active() {
            let _owner = match try_child_owner(store, &job.id) {
                Ok(owner) => owner,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(error) => return Err(error.into()),
            };
            recover_owned_child(store, &job.id, &_owner)?;
        }
    }
    Ok(())
}
fn recover_owned_child(
    store: &TeamStore,
    id: &str,
    _owner: &SessionExecutionGuard,
) -> Result<bool> {
    // Candidate snapshots are not evidence of a live or orphaned worker.
    if !store.get(id)?.active() {
        return Ok(false);
    }
    let mut recovered = false;
    store.update(id, |job| {
        if job.active() {
            job.state = "interrupted".into();
            job.latest = "运行已中断，可继续此任务".into();
            job.updated_at = now().max(job.updated_at + 1);
            job.revision += 1;
            recovered = true;
        }
    })?;
    Ok(recovered)
}

/// Busy children owned by another process are read without recovering them.
pub fn store_for_inspection(root: &Path, owner: &str) -> Result<TeamStore> {
    let teams = TEAMS.get_or_init(Default::default).lock().unwrap();
    if let Some(team) = teams.get(&key(root, owner)) {
        return Ok(team.0.store.clone());
    }
    let store = TeamStore::open(root, owner)?;
    recover_interrupted(&store)?;
    Ok(store)
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn fresh_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "agent-{}-{}",
        now(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

fn result_summary(job: &AgentJob) -> Value {
    json!({"id":job.id,"name":job.name,"role":job.role,"model":job.model,"state":job.state,
        "outcome":job.outcome,"result":short_text(&job.result, 2400),"latest":short_text(&job.latest, 400),"worktree":job.worktree,
        "result_chars":job.result.chars().count(),"result_truncated":job.result.chars().count()>2400,
        "files":job.files.iter().take(32).collect::<Vec<_>>(),"file_count":job.files.len(),
        "depends_on":job.depends_on,"attempt":job.attempt,"revision":job.revision,
        "created_at":job.created_at,"updated_at":job.updated_at})
}

fn short_text(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn page<T: serde::Serialize>(items: &[T], input: &Value) -> Value {
    let offset = input["offset"].as_u64().unwrap_or(0) as usize;
    let limit = input["limit"].as_u64().unwrap_or(10).clamp(1, 20) as usize;
    let end = offset.saturating_add(limit).min(items.len());
    json!({"items":items.get(offset..end).unwrap_or_default(),"offset":offset,
        "total":items.len(),"next_offset":(end<items.len()).then_some(end)})
}

impl AgentTeam {
    pub fn attach(
        config: ResolveCtx,
        mut ctx: ToolCtx,
        mut runtime: SubagentRuntime,
        client: LlmClient,
        event: Option<TeamEvent>,
    ) -> Result<Self> {
        let owner = ctx.session_id.clone().context("子代理需要明确的主会话")?;
        // A team outlives a turn; never retain or drain its parent's turn writer.
        ctx.input_inbox = None;
        runtime.options.host = None;
        // Each worker installs its own durable child transcript callbacks below.
        // Keeping the parent's callbacks here would pin its turn/session writer
        // in the long-lived TEAMS registry after the parent round completes.
        runtime.transcript_sink = None;
        runtime.transcript_provider = None;
        let root = ctx.project_root.clone();
        let mailbox = ctx.async_mailbox.clone();
        let team = get_or_register(&root, &owner, || {
            let store = TeamStore::open(&ctx.project_root, &owner)?;
            // An interrupted process has no live worker. Never replay writes automatically.
            recover_interrupted(&store)?;
            let team = Self(Arc::new(Inner {
                mailbox: Mutex::new(ctx.async_mailbox.clone()),
                child_mailboxes: Mutex::new(HashMap::new()),
                child_ledgers: Mutex::new(HashMap::new()),
                notification_lock: Mutex::new(()),
                root: ctx.project_root.clone(),
                ctx,
                owner: owner.clone(),
                store,
                config: Mutex::new(config.clone()),
                runtime: Mutex::new(runtime.clone()),
                client,
                lifecycle: Mutex::new(()),
                active: Mutex::new(HashMap::new()),
                parent: Mutex::new(Vec::new()),
                event: Mutex::new(event.clone()),
                ask_router: Mutex::new(None),
                changed: Notify::new(),
                slots: Arc::new(Semaphore::new(4)),
                spawn_lock: tokio::sync::Mutex::new(()),
            }));
            Ok(team)
        })?;
        if let Some(mailbox) = mailbox {
            team.set_mailbox(mailbox);
        }
        *team.0.runtime.lock().unwrap() = runtime;
        *team.0.config.lock().unwrap() = config;
        if event.is_some() {
            *team.0.event.lock().unwrap() = event;
        }
        Ok(team)
    }
    pub fn list(&self) -> Result<Vec<AgentJob>> {
        self.0.store.list()
    }
    pub fn set_ask_router(&self, router: TeamAsk) {
        *self.0.ask_router.lock().unwrap() = Some(router);
    }
    pub fn set_mailbox(&self, mailbox: AsyncMailbox) {
        *self.0.mailbox.lock().unwrap() = Some(mailbox);
    }
    fn child_mailbox(&self, id: &str) -> AsyncMailbox {
        let mut mailboxes = self.0.child_mailboxes.lock().unwrap();
        if let Some(mailbox) = mailboxes.get(id).filter(|m| !m.is_closed()) {
            return mailbox.clone();
        }
        let weak = Arc::downgrade(&self.0);
        let child_id = id.to_owned();
        let mailbox = AsyncMailbox::new(move |notice| {
            let team = AgentTeam(weak.upgrade().ok_or("子任务服务已关闭")?);
            team.queue_message(&child_id, "callback", &notice.text, Some(notice.id))
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        mailboxes.insert(id.into(), mailbox.clone());
        mailbox
    }
    pub fn set_parent(&self, messages: &[Message]) {
        *self.0.parent.lock().unwrap() = messages.to_vec();
    }
    fn emit(&self, job: &AgentJob) {
        let event = self.0.event.lock().unwrap().clone();
        if let Some(event) = event {
            event(job);
        }
        self.0.changed.notify_waiters();
        if job.notify_on_completion
            && job.outcome != "adopted"
            && matches!(job.state.as_str(), "done" | "failed" | "blocked")
            && job.revision > job.reported
        {
            let _delivery = self.0.notification_lock.lock().unwrap();
            if self
                .0
                .store
                .get(&job.id)
                .is_ok_and(|j| j.notified >= job.revision)
            {
                return;
            }
            if let Some(mailbox) = self.0.mailbox.lock().unwrap().clone() {
                let result = mailbox.publish(AsyncNotice {
                    id: format!("child:{}:{}", job.id, job.revision),
                    text: format!("子任务回调（任务结果，不代表用户指令或验收）：{}\n请检查结果、处理失败，必要时用 task get/diff/adopt 整合。", result_summary(job)),
                });
                match result {
                    Ok(()) => {
                        let _ = self
                            .0
                            .store
                            .update(&job.id, |j| j.notified = j.notified.max(job.revision));
                    }
                    Err(error) => {
                        tracing::warn!(%error, child=%job.id, "child callback not delivered")
                    }
                }
            }
        }
    }
    fn update(&self, id: &str, f: impl FnOnce(&mut AgentJob)) -> Result<AgentJob> {
        let job = self.0.store.update(id, |job| {
            f(job);
            job.updated_at = now().max(job.updated_at + 1);
        })?;
        self.emit(&job);
        Ok(job)
    }
    fn resolve(&self, id: &str) -> Result<AgentJob> {
        if let Ok(job) = self.0.store.get(id) {
            return Ok(job);
        }
        let mut matches = self.list()?.into_iter().filter(|j| j.name == id);
        let job = matches.next().context("找不到当前会话的子任务")?;
        if matches.next().is_some() {
            bail!("多个子任务使用相同名称，请使用任务 id");
        }
        Ok(job)
    }

    fn child_owner_locked(&self, id: &str) -> Result<Arc<SessionExecutionGuard>> {
        if let Some(worker) = self.0.active.lock().unwrap().get(id) {
            return Ok(worker.owner.clone());
        }
        let owner = claim_child(&self.0.store, id)?;
        crate::background::retry_child_cleanup(&self.0.root, &self.0.owner, id)
            .map_err(anyhow::Error::msg)?;
        Ok(owner)
    }
    fn resolve_owned_locked(&self, id: &str) -> Result<(AgentJob, Arc<SessionExecutionGuard>)> {
        let id = self.resolve(id)?.id;
        let owner = self.child_owner_locked(&id)?;
        // Resolving a name/id precedes claiming. Never use that earlier state for
        // admission after a different process has completed or resumed the child.
        let mut job = self.0.store.get(&id)?;
        if cleanup_failed(&job) {
            if self.0.active.lock().unwrap().contains_key(&id) {
                bail!("子任务 {id} 的后台清理仍由执行者处理，请稍后重试");
            }
            if !crate::background::has_child_cleanup_records(&self.0.root, &self.0.owner, &id) {
                bail!("子任务 {id} 的原执行者后台清理证据不可用；请先在原执行者成功处理，不能猜测已恢复");
            }
            // child_owner_locked has actually joined and restored the retained
            // records; only that evidence permits clearing the exact marker.
            job = self.0.store.update(&id, |job| {
                job.latest = "后台清理重试成功，可明确续做".into();
                job.updated_at = now().max(job.updated_at + 1);
                job.revision += 1;
            })?;
        }
        Ok((job, owner))
    }

    pub fn stop(&self, id: &str) -> Result<()> {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        self.stop_locked(id)
    }
    fn stop_locked(&self, id: &str) -> Result<()> {
        let (job, _owner) = match self.resolve_owned_locked(id) {
            Ok(owned) => owned,
            Err(error) => {
                self.close_child_mailbox(id);
                if let Some(cancel) = self.0.active.lock().unwrap().get(id) {
                    cancel.cancel();
                }
                return Err(error);
            }
        };
        self.stop_owned_locked(&job, &_owner)
    }
    fn stop_owned_locked(&self, job: &AgentJob, _owner: &Arc<SessionExecutionGuard>) -> Result<()> {
        let questions_cancelled = kanzei_harness::pending_question::cancel_owner(
            &self.0.ctx.project_root,
            &self.0.owner,
            Some(&job.id),
        )
        .map_err(anyhow::Error::msg);
        if let Some(mailbox) = self.0.child_mailboxes.lock().unwrap().remove(&job.id) {
            mailbox.close();
        }
        let cancel = self.0.active.lock().unwrap().get(&job.id).cloned();
        let saved = self.update(&job.id, |j| {
            j.state = if cancel.is_some() {
                "stopping"
            } else {
                "stopped"
            }
            .into();
            j.latest = "已停止；只有明确续做或重新派发才会运行".into();
            for m in &mut j.messages {
                if m.state == "queued" {
                    m.state = "cancelled".into();
                }
            }
        });
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
        saved?;
        questions_cancelled?;
        Ok(())
    }
    pub fn stop_all(&self) {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        for mailbox in self.0.child_mailboxes.lock().unwrap().values() {
            mailbox.close();
        }
        let ids: Vec<_> = self.0.active.lock().unwrap().keys().cloned().collect();
        for id in ids {
            if let Err(error) = self.stop_locked(&id) {
                tracing::error!(%error, child=id, "child stop state could not be saved");
            }
        }
    }
    fn close_child_mailbox(&self, id: &str) {
        if let Some(mailbox) = self.0.child_mailboxes.lock().unwrap().remove(id) {
            mailbox.close();
        }
    }
    pub fn has_updates(&self) -> bool {
        self.list()
            .map(|jobs| jobs.iter().any(|j| j.active() || j.revision > j.reported))
            .unwrap_or(true)
    }
    // A model tool response already delivers these exact revisions. UI reads must
    // not consume them, and an update arriving after this snapshot stays pending.
    fn acknowledge_result(&self, value: &Value) -> Result<()> {
        if let Some(jobs) = value.as_array() {
            for job in jobs {
                self.acknowledge_result(job)?;
            }
        } else if let Some(job) = value.get("job") {
            self.acknowledge_result(job)?;
        } else if let Some(jobs) = value.get("jobs") {
            self.acknowledge_result(jobs)?;
        } else if let (Some(id), Some(revision)) =
            (value["id"].as_str(), value["revision"].as_u64())
        {
            self.0.store.update(id, |job| {
                job.reported = job.reported.max(revision);
            })?;
        }
        Ok(())
    }
    pub async fn command(&self, call_id: &str, input: Value) -> Result<Value> {
        let action = input["action"].as_str().unwrap_or("spawn");
        match action {
            "list" => {
                let mut jobs = self.list()?;
                // Active children first, then the most recently updated records.
                jobs.sort_by_key(|j| {
                    (
                        std::cmp::Reverse(j.active()),
                        std::cmp::Reverse(j.updated_at),
                    )
                });
                let summaries: Vec<_> = jobs
                    .iter()
                    .map(|job| {
                        let mut summary = result_summary(job);
                        summary["result"] = json!(short_text(&job.result, 400));
                        summary["result_truncated"] = json!(job.result.chars().count() > 400);
                        summary
                    })
                    .collect();
                let mut value = page(&summaries, &input);
                value["jobs"] = value["items"].take();
                value.as_object_mut().unwrap().remove("items");
                Ok(value)
            }
            "get" => {
                let j = self.resolve(input["id"].as_str().context("需要子任务 id")?)?;
                let mut value = json!({"job":result_summary(&j)});
                match input["view"].as_str().unwrap_or("summary") {
                    "summary" => {}
                    "history" => value["history"] = page(&self.0.store.history(&j.id)?, &input),
                    "trace" => value["trace"] = page(&j.trace, &input),
                    "messages" => value["messages"] = page(&j.messages, &input),
                    "result" => value["result"] = json!(j.result),
                    "prompt" => value["prompt"] = json!(j.prompt),
                    other => bail!("未知子任务详情：{other}"),
                }
                Ok(value)
            }
            "spawn" => self.spawn(call_id, &input).await,
            "restart" => {
                let job = {
                    let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
                    let (job, owner) =
                        self.resolve_owned_locked(input["id"].as_str().context("需要子任务 id")?)?;
                    if job.active() {
                        self.stop_owned_locked(&job, &owner)?;
                    }
                    self.close_child_mailbox(&job.id);
                    job
                };
                self.spawn("", &json!({
                    "prompt":input["prompt"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or(&job.prompt),
                    "description":job.name,"agent":job.role,"model":job.model_tier,
                    "schema":job.schema,"depends_on":job.depends_on,"replaces":job.id,
                    "background":true,"context":"fresh"
                })).await
            }
            "message" | "resume" => {
                let j = self.resolve(input["id"].as_str().context("需要子任务 id")?)?;
                self.queue_message(
                    &j.id,
                    "main",
                    input["prompt"].as_str().unwrap_or(""),
                    input["message_id"].as_str().map(str::to_owned),
                )
            }
            "stop" => {
                let j = self.resolve(input["id"].as_str().context("需要子任务 id")?)?;
                self.stop(&j.id)?;
                Ok(json!({"id":j.id,"state":"stopping"}))
            }
            "wait" | "collect" => {
                let wanted = input["id"]
                    .as_str()
                    .map(|id| self.resolve(id))
                    .transpose()?;
                loop {
                    let notify = self.0.changed.notified();
                    let (jobs, workers) = {
                        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
                        let jobs = self.list()?;
                        let mut workers: std::collections::HashSet<_> =
                            self.0.active.lock().unwrap().keys().cloned().collect();
                        for job in jobs
                            .iter()
                            .filter(|job| wanted.as_ref().is_none_or(|wanted| wanted.id == job.id))
                        {
                            if workers.contains(&job.id) {
                                continue;
                            }
                            match try_child_owner(&self.0.store, &job.id) {
                                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                    // A remote worker (or its final callback) is
                                    // still live. Use the existing DB/notify wait.
                                    workers.insert(job.id.clone());
                                }
                                Err(error) => return Err(error.into()),
                                Ok(owner) => {
                                    if action == "wait"
                                        && job.active()
                                        && recover_owned_child(&self.0.store, &job.id, &owner).map_err(|error| anyhow::anyhow!("子任务 {} 没有运行中的执行者；恢复状态保存失败：{error:#}", job.id))?
                                    {
                                        bail!("子任务 {} 没有运行中的执行者；状态保存或启动失败，可明确续做或停止", job.id);
                                    }
                                }
                            }
                        }
                        (jobs, workers)
                    };
                    let active = jobs.iter().any(|j| {
                        (j.active() || workers.contains(&j.id))
                            && wanted.as_ref().is_none_or(|w| w.id == j.id)
                    });
                    if !active || action == "collect" {
                        let results: Vec<_> = jobs
                            .into_iter()
                            .filter(|j| wanted.as_ref().is_none_or(|w| w.id == j.id))
                            .filter(|j| action != "collect" || j.revision > j.reported)
                            .filter(|j| {
                                action != "collect" || !j.active() && !workers.contains(&j.id)
                            })
                            .collect();
                        if action == "collect" {
                            for job in &results {
                                self.0.store.update(&job.id, |j| {
                                    j.reported = j.reported.max(job.revision)
                                })?;
                            }
                            return Ok(json!(results
                                .iter()
                                .map(result_summary)
                                .collect::<Vec<_>>()));
                        }
                        return Ok(json!(results
                            .iter()
                            .map(result_summary)
                            .collect::<Vec<_>>()));
                    }
                    tokio::select! {_=notify=>{},_=tokio::time::sleep(Duration::from_millis(500))=>{}}
                }
            }
            "diff" => {
                let j = self.resolve(input["id"].as_str().context("需要子任务 id")?)?;
                Ok(
                    json!({"id":j.id,"files":j.files,"diff":workspace::diff(&self.0.ctx.cwd,j.base.as_deref().context("无工作树")?,j.head.as_deref().context("子任务尚未产生改动快照")?)?}),
                )
            }
            "adopt" => Ok(json!(self.adopt_with(
                input["id"].as_str().context("需要子任务 id")?,
                workspace::adopt,
            )?)),
            _ => bail!("未知子任务操作：{action}"),
        }
    }
    fn adopt_with(
        &self,
        id: &str,
        apply: impl FnOnce(&Path, &str, &str) -> Result<()>,
    ) -> Result<AgentJob> {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        if self.0.config.lock().unwrap().profile != kanzei_harness::ProfileKind::Dev {
            bail!("当前模式不能采纳代码改动");
        }
        let (job, _owner) = self.resolve_owned_locked(id)?;
        if job.active() || job.state != "done" {
            bail!("先等待子任务完成，再检查和采纳改动");
        }
        if job.outcome == "adopted" {
            return Ok(job);
        }
        apply(
            &self.0.ctx.cwd,
            job.base.as_deref().context("无工作树")?,
            job.head.as_deref().context("没有改动快照")?,
        )?;
        self.update(&job.id, |job| {
            job.outcome = "adopted".into();
            job.revision += 1;
        })
    }
    /// Desktop panels inspect complete persisted records without filling a model's
    /// context or acknowledging result revisions merely by opening a panel.
    pub async fn ui_command(&self, input: Value) -> Result<Value> {
        match input["action"].as_str().unwrap_or("list") {
            "list" => Ok(json!(self.list()?)),
            "get" => {
                let job = self.resolve(input["id"].as_str().context("需要子任务 id")?)?;
                Ok(json!({"history":self.0.store.history(&job.id)?,"job":job}))
            }
            _ => self.command("", input).await,
        }
    }
    async fn spawn(&self, call_id: &str, input: &Value) -> Result<Value> {
        let creation = self.0.spawn_lock.lock().await;
        let prompt = input["prompt"].as_str().unwrap_or("").trim();
        if prompt.is_empty() {
            bail!("请说明子任务目标和预期结果");
        }
        let role = input["agent"].as_str().unwrap_or("general");
        let persona = self
            .0
            .runtime
            .lock()
            .unwrap()
            .roster
            .iter()
            .find(|agent| agent.name == role)
            .cloned();
        if persona.is_none()
            && !matches!(
                role,
                "explore" | "plan" | "general" | "implement" | "verify"
            )
        {
            bail!("未知子代理角色：{role}");
        }
        if self.0.config.lock().unwrap().profile != kanzei_harness::ProfileKind::Dev
            && matches!(role, "general" | "implement" | "verify")
        {
            bail!("当前模式仅允许 explore / plan 子任务");
        }
        if self.list()?.iter().filter(|j| j.active()).count() >= 16 {
            bail!("已有 16 个未结束子任务，请先收取结果");
        }
        let id = if !call_id.is_empty()
            && call_id.len() < 100
            && call_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            call_id.to_owned()
        } else {
            fresh_id()
        };
        let name = input["description"]
            .as_str()
            .or(input["name"].as_str())
            .unwrap_or(role)
            .to_owned();
        let model = input["model"]
            .as_str()
            .unwrap_or_else(|| {
                persona
                    .as_ref()
                    .map(|agent| agent.model.as_str())
                    .unwrap_or(if role == "explore" { "fast" } else { "primary" })
            })
            .to_owned();
        if !matches!(model.as_str(), "fast" | "primary") {
            bail!("model 仅支持 fast / primary");
        }
        let depends_on = input["depends_on"]
            .as_array()
            .map(|ids| {
                ids.iter()
                    .map(|id| self.resolve(id.as_str().unwrap_or("")).map(|j| j.id))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        let history = if input["context"].as_str() == Some("fork") {
            self.0.parent.lock().unwrap().clone()
        } else {
            Vec::new()
        };
        let job = AgentJob {
            notify_on_completion: input["background"].as_bool() != Some(false),
            replaces: input["replaces"].as_str().map(str::to_owned),
            notified: 0,
            id: id.clone(),
            owner: self.0.owner.clone(),
            project_dir: self.0.root.to_string_lossy().into_owned(),
            process_id: self.0.ctx.process_id.clone(),
            trace: Vec::new(),
            trace_seq: 0,
            name,
            role: role.into(),
            model: model.clone(),
            model_tier: model,
            prompt: prompt.into(),
            schema: input.get("schema").filter(|v| v.is_object()).cloned(),
            state: "queued".into(),
            outcome: "pending".into(),
            latest: "等待执行".into(),
            result: String::new(),
            worktree: None,
            base: None,
            head: None,
            files: Vec::new(),
            depends_on,
            created_at: now(),
            updated_at: now(),
            attempt: 0,
            revision: 0,
            reported: 0,
            messages: vec![AgentMessage {
                id: fresh_id(),
                from: "main".into(),
                text: prompt.into(),
                state: "queued".into(),
                at: now(),
            }],
        };
        {
            let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
            let owner = self.child_owner_locked(&id)?;
            self.0.store.insert(&job, &history)?;
            self.emit(&job);
            self.launch_owned_locked(&id, owner)?;
        }
        drop(creation);
        if input["background"].as_bool() == Some(false) {
            return Box::pin(self.command("", json!({"action":"wait","id":id}))).await;
        }
        Ok(
            json!({"id":id,"state":"queued","background":true,"message":"已派发。桌面端完成后自动回传；可以继续独立工作或回复用户。task list/get/collect 查询，wait 仅用于必须等待的依赖。未完成前不要报告成功。"}),
        )
    }
    pub async fn message(&self, id: &str, from: &str, text: &str) -> Result<Value> {
        self.queue_message(id, from, text, None)
    }
    /// Durable replies are callbacks, never an explicit main-actor restart.
    /// Re-read under the worker admission lock so stop cannot slip between this
    /// validation and registering a new worker.
    pub fn question_reply(&self, id: &str, question_id: u64, notice: AsyncNotice) -> Result<Value> {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        let record = kanzei_harness::pending_question::get(&self.0.root, question_id)
            .map_err(anyhow::Error::msg)?;
        if record["payload"]["sessionId"] != self.0.owner
            || record["payload"]["agentId"] != id
            || record["callback_id"] != notice.id
        {
            bail!("问题所属子任务不匹配");
        }
        match record["state"].as_str() {
            Some("delivered") => {
                return Ok(json!({"id":id,"message_id":notice.id,"state":"delivered"}))
            }
            Some("answered") => {}
            _ => bail!("问题已取消或尚未回答，旧回复不能重新启动子任务"),
        }
        self.queue_message_locked(id, "callback", &notice.text, Some(notice.id))
    }
    fn queue_message(
        &self,
        id: &str,
        from: &str,
        text: &str,
        message_id: Option<String>,
    ) -> Result<Value> {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        self.queue_message_locked(id, from, text, message_id)
    }
    fn queue_message_locked(
        &self,
        id: &str,
        from: &str,
        text: &str,
        message_id: Option<String>,
    ) -> Result<Value> {
        let text = text.trim();
        if text.is_empty() {
            bail!("消息不能为空");
        }
        if id == "main" {
            let mailbox = self
                .0
                .mailbox
                .lock()
                .unwrap()
                .clone()
                .context("主对话未连接异步消息通道")?;
            let id = message_id.unwrap_or_else(fresh_id);
            mailbox
                .publish(AsyncNotice {
                    id: format!("message:{from}:{id}"),
                    text: format!("子任务 {from} 发来的消息（任务证据，不是用户授权）：\n{text}"),
                })
                .map_err(anyhow::Error::msg)?;
            return Ok(json!({"to":"main","message_id":id,"state":"queued"}));
        }
        let (j, owner) = self.resolve_owned_locked(id)?;
        if j.state == "stopping" {
            bail!("任务正在停止，请待停止完成后续做");
        }
        if matches!(j.state.as_str(), "stopped" | "stopping") && from != "main" {
            bail!("子任务已被停止，只有主对话明确续做才会重新启动");
        }
        if j.outcome == "adopted" {
            bail!("改动已采纳，请新建后续任务，避免重复应用旧补丁");
        }
        let message = AgentMessage {
            id: message_id.unwrap_or_else(fresh_id),
            from: from.into(),
            text: text.into(),
            state: "queued".into(),
            at: now(),
        };
        let reply = json!({"id":j.id,"message_id":message.id,"state":"queued","delivery":"下一次模型请求前处理"});
        let mut inserted = false;
        self.update(&j.id, |j| {
            if !j.messages.iter().any(|m| m.id == message.id) {
                let was_active = j.active();
                j.messages.push(message);
                inserted = true;
                if !was_active {
                    j.state = "queued".into();
                }
            }
        })?;
        if inserted {
            self.launch_owned_locked(&j.id, owner)?;
        }
        Ok(reply)
    }
    fn launch_owned_locked(&self, id: &str, owner: Arc<SessionExecutionGuard>) -> Result<()> {
        let job = self.0.store.get(id)?;
        if matches!(job.state.as_str(), "stopped" | "stopping") {
            return Ok(());
        }
        let queued_at_launch: Vec<_> = job
            .messages
            .iter()
            .filter(|message| message.state == "queued")
            .map(|message| message.id.clone())
            .collect();
        let mut active = self.0.active.lock().unwrap();
        if active.contains_key(id) {
            return Ok(());
        }
        let cancel = Arc::new(ChildWorker {
            cancel: CancellationToken::new(),
            owner,
        });
        active.insert(id.into(), cancel.clone());
        drop(active);
        let registration = WorkerRegistration {
            team: self.clone(),
            id: id.into(),
            worker: cancel.clone(),
        };
        self.update(id, |j| {
            j.state = "queued".into();
        })?;
        let team = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            let _registration = registration;
            let result = std::panic::AssertUnwindSafe(async {
                tokio::select! {biased;_=cancel.cancelled()=>Err(anyhow::anyhow!("子任务已停止")),r=team.worker(&id, &cancel)=>r}
            }).catch_unwind().await.unwrap_or_else(|_| {
                cancel.cancel();
                Err(anyhow::anyhow!("子任务执行异常，已停止并清理执行者"))
            });
            team.settle_worker(&id, &cancel, &result, &queued_at_launch)
                .await;
        });
        Ok(())
    }

    async fn reap_worker_processes(&self, id: &str) {
        let reaped = std::panic::AssertUnwindSafe(crate::background::kill_child_processes(
            &self.0.root,
            &self.0.owner,
            id,
        ))
        .catch_unwind()
        .await;
        if reaped.is_err()
            || crate::background::child_processes(&self.0.root, &self.0.owner, id)
                .iter()
                .any(|process| !process.persistent && process.is_running())
        {
            let _ = self.0.store.update(id, |job| {
                job.state = "failed".into();
                job.latest =
                    cleanup_failure("后台进程未能终止；执行者保持到实际退出，请明确停止后台进程");
                job.revision += 1;
            });
        }
    }

    async fn settle_worker(
        &self,
        id: &str,
        cancel: &Arc<ChildWorker>,
        result: &Result<()>,
        queued_at_launch: &[String],
    ) {
        let settled = std::panic::AssertUnwindSafe(async {
            // Failed attempts cannot leave an unowned writing process behind;
            // explicit continuation receives the same guard after old cleanup.
            let failed =
                result.is_err() || self.0.store.get(id).is_ok_and(|job| job.state == "failed");
            if cancel.is_cancelled() || failed {
                self.reap_worker_processes(id).await;
            }
            let cleanup = tokio::select! { biased;
                _ = cancel.cancelled() => {
                    self.reap_worker_processes(id).await;
                    crate::background::wait_child_cleanup(&self.0.root, &self.0.owner, id).await
                },
                cleanup = crate::background::wait_child_cleanup(&self.0.root, &self.0.owner, id) => cleanup,
            };
            match cleanup {
                Ok(notes) if notes.is_empty() => {
                    self.finish_worker(id, cancel, result, true, queued_at_launch);
                }
                Ok(notes) => {
                    let error = Err(anyhow::anyhow!(notes.join("；")));
                    self.finish_worker(id, cancel, &error, true, queued_at_launch);
                }
                Err(error) => {
                    self.0.store.update(id, |job| {
                        job.state = "failed".into();
                        job.latest = cleanup_failure(&error);
                        job.revision += 1;
                    })?;
                }
            }
            Ok::<(), anyhow::Error>(())
        })
        .catch_unwind()
        .await;
        if !matches!(settled, Ok(Ok(()))) {
            let stopped = cancel.is_cancelled();
            cancel.cancel();
            self.reap_worker_processes(id).await;
            let cleanup =
                crate::background::wait_child_cleanup(&self.0.root, &self.0.owner, id).await;
            // An event/continuation callback may itself be the panic source. Do
            // not call it again during emergency finalization, or release before
            // cleanup. Late transcript callbacks still retain this worker guard.
            let saved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
                self.close_child_mailbox(id);
                let _ = kanzei_harness::pending_question::cancel_owner(
                    &self.0.root,
                    &self.0.owner,
                    Some(id),
                );
                self.0.store.update(id, |job| {
                    job.state = if stopped { "stopped" } else { "failed" }.into();
                    job.latest = match &cleanup {
                        Ok(_) => "执行者收尾异常，子进程已清理；可明确续做".into(),
                        Err(error) => cleanup_failure(&format!("执行者收尾异常：{error}")),
                    };
                    job.updated_at = now().max(job.updated_at + 1);
                    job.revision += 1;
                })
            }));
            if !matches!(saved, Ok(Ok(_))) {
                tracing::error!(
                    child = id,
                    "child cleanup completed but emergency state could not be saved"
                );
            }
        }
    }

    fn finish_worker(
        &self,
        id: &str,
        cancel: &Arc<ChildWorker>,
        result: &Result<()>,
        cleaned: bool,
        queued_at_launch: &[String],
    ) -> bool {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        if !self
            .0
            .active
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, cancel))
        {
            return true;
        }
        // Stop can arrive after worker() returned Ok. Keep ownership until its
        // processes are reaped, and never wait for them under lifecycle.
        if cancel.is_cancelled() && !cleaned {
            self.close_child_mailbox(id);
            return false;
        }
        let error = if cancel.is_cancelled() {
            Some("子任务已停止".to_owned())
        } else {
            result.as_ref().err().map(ToString::to_string)
        };
        if let Some(error) = error {
            if let Err(error) = self.update(id, |j| {
                j.state = if cancel.is_cancelled() {
                    "stopped"
                } else if j.messages.iter().any(|message| {
                    message.state == "queued" && !queued_at_launch.contains(&message.id)
                }) {
                    "queued"
                } else {
                    "failed"
                }
                .into();
                j.latest = error;
                j.revision += 1;
            }) {
                tracing::error!(%error, child=id, "child final state could not be saved");
            }
        } else if self.0.store.get(id).is_ok_and(|j| {
            j.state == "failed"
                && j.messages.iter().any(|message| {
                    message.state == "queued" && !queued_at_launch.contains(&message.id)
                })
        }) {
            // The runner reports model/tool failures as a failed ToolOutput,
            // so worker() may return Ok after saving a failed state.
            if let Err(error) = self.update(id, |j| {
                j.state = "queued".into();
                j.revision += 1;
            }) {
                tracing::error!(%error, child=id, "child continuation state could not be saved");
            }
        }
        self.0.active.lock().unwrap().remove(id);
        self.0.changed.notify_waiters();
        // Message admission and cleanup use the same owner lock, so an explicit
        // continuation queued before cleanup is handed to exactly one new worker.
        if !cancel.is_cancelled()
            && self.0.store.get(id).is_ok_and(|j| {
                matches!(j.state.as_str(), "done" | "queued")
                    && j.messages.iter().any(|m| m.state == "queued")
            })
        {
            if let Err(error) = self.launch_owned_locked(id, cancel.owner.clone()) {
                tracing::error!(%error, child=id, "child continuation could not be launched");
            }
        }
        true
    }

    fn check_worker_locked(
        &self,
        id: &str,
        cancel: &Arc<ChildWorker>,
        attempt: Option<u32>,
    ) -> Result<AgentJob> {
        if cancel.is_cancelled()
            || !self
                .0
                .active
                .lock()
                .unwrap()
                .get(id)
                .is_some_and(|current| Arc::ptr_eq(current, cancel))
        {
            bail!("子任务执行已结束");
        }
        let job = self.0.store.get(id)?;
        if matches!(job.state.as_str(), "stopped" | "stopping")
            || attempt.is_some_and(|attempt| job.attempt != attempt)
        {
            bail!("子任务执行已结束");
        }
        Ok(job)
    }

    fn worker_update(
        &self,
        id: &str,
        cancel: &Arc<ChildWorker>,
        attempt: Option<u32>,
        f: impl FnOnce(&mut AgentJob),
    ) -> Result<AgentJob> {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        self.check_worker_locked(id, cancel, attempt)?;
        self.update(id, f)
    }

    fn transcript_sink(
        &self,
        id: &str,
        worker: &Arc<ChildWorker>,
        attempt: u32,
    ) -> kanzei_core::BackgroundEventSink {
        let team = self.clone();
        let worker = worker.clone();
        let id = id.to_owned();
        Arc::new(move |_, payload| {
            let _lifecycle = team.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
            if team
                .check_worker_locked(&id, &worker, Some(attempt))
                .is_err()
            {
                return;
            }
            if let Ok(messages) =
                serde_json::from_value::<Vec<Message>>(payload["messages"].clone())
            {
                if let Err(error) = team.0.store.checkpoint(&id, &messages) {
                    tracing::error!(%error, "child checkpoint failed");
                }
            }
        })
    }

    fn begin_ask(
        &self,
        job: &AgentJob,
        cancel: &Arc<ChildWorker>,
        request: kanzei_core::AskRequest,
        router: Option<&TeamAsk>,
        parent: Option<&ParentAsk>,
    ) -> kanzei_core::AskFuture {
        let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        if !self
            .check_worker_locked(&job.id, cancel, Some(job.attempt))
            .is_ok_and(|current| matches!(current.state.as_str(), "running" | "waiting_user"))
        {
            return Box::pin(async { kanzei_core::AskResponse::Cancelled });
        }
        if matches!(&request,kanzei_core::AskRequest::Permission{action,..} if action=="subagent-write")
        {
            return Box::pin(async {
                kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
            });
        }
        // Routers synchronously persist/register the question here; only their
        // returned reply future waits. Stop uses this same lifecycle boundary.
        if let Some(router) = router {
            router(job, request)
        } else if let Some(parent) = parent {
            parent(request)
        } else {
            Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
        }
    }
    fn observe(
        &self,
        id: &str,
        event: RunEvent,
        stream: &mut String,
        last_delta: &mut std::time::Instant,
    ) -> Result<()> {
        if let RunEvent::TaskProgress { text, trace, .. } = event {
            if trace.as_ref().is_some_and(|t| t.phase == "delta") {
                stream.push_str(trace.as_ref().and_then(|t| t.text.as_deref()).unwrap_or(""));
                if last_delta.elapsed() < Duration::from_millis(200) {
                    return Ok(());
                }
                *last_delta = std::time::Instant::now();
                self.update(id, |j| {
                    j.latest = stream
                        .chars()
                        .rev()
                        .take(240)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect()
                })?;
                return Ok(());
            }
            if trace.as_ref().is_some_and(|t| t.phase == "text") {
                stream.clear();
            }
            self.update(id,|j|{
                            j.latest=text;
                            if let Some(trace)=trace {
                                if let Some(model)=&trace.model{j.model=model.clone();}
                                j.trace_seq+=1;
                                j.trace.push(json!({"seq":j.trace_seq,"at":now(),"run_id":format!("{}:{}:{}",j.owner,j.id,j.attempt),"child_id":format!("{}:{}",j.attempt,trace.child_id),"phase":trace.phase,"name":trace.name,"summary":trace.summary,"ok":trace.ok,"outcome":trace.outcome,"code":trace.code,"preview":trace.preview,"input":trace.input,"usage":trace.usage,"text":trace.text,"model":trace.model,"agent":trace.agent}));
                                if j.trace.len()>200{j.trace.remove(0);}
                            }
                        })?;
        }
        Ok(())
    }

    async fn worker(&self, id: &str, cancel: &Arc<ChildWorker>) -> Result<()> {
        let initial = self.0.store.get(id)?;
        for dependency in &initial.depends_on {
            loop {
                let dep = self.0.store.get(dependency)?;
                if dep.state == "done" {
                    break;
                }
                if !dep.active() {
                    self.worker_update(id, cancel, None, |j| {
                        j.state = "blocked".into();
                        j.latest = format!("依赖 {} 尚未完成", dep.name);
                        j.revision += 1;
                    })?;
                    return Ok(());
                }
                self.worker_update(id, cancel, None, |j| {
                    j.state = "waiting".into();
                    j.latest = format!("等待 {}", dep.name);
                })?;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        let slot = Arc::new(Mutex::new(Some(
            self.0.slots.clone().acquire_owned().await?,
        )));
        let writing = matches!(initial.role.as_str(), "general" | "implement" | "verify");
        if writing && initial.worktree.is_none() {
            if !self.0.ctx.project_workflow {
                workspace::ensure_general_repository(&self.0.ctx.cwd, &self.0.ctx.project_root)?;
            }
            self.worker_update(id, cancel, None, |j| j.latest = "准备独立工作树".into())?;
            use sha2::{Digest, Sha256};
            let owner = format!("{:x}", Sha256::digest(self.0.owner.as_bytes()));
            let (tree, base) =
                workspace::prepare(&self.0.ctx.cwd, &format!("{}-{id}", &owner[..10]))?;
            self.worker_update(id, cancel, None, |j| {
                j.worktree = Some(tree.clone());
                j.base = Some(base);
            })?;
            // Dependent writers/verifiers inspect the candidate files, not only a
            // textual summary. Conflicts fail here and retain this checkout.
            for dependency in &initial.depends_on {
                let dep = self.0.store.get(dependency)?;
                if dep.outcome != "adopted" {
                    if let (Some(base), Some(head)) = (dep.base, dep.head) {
                        workspace::adopt(&tree, &base, &head)?;
                    }
                }
            }
        }
        loop {
            let job = self.0.store.get(id)?;
            let queued: Vec<_> = job
                .messages
                .iter()
                .filter(|m| m.state == "queued")
                .cloned()
                .collect();
            if queued.is_empty() {
                self.worker_update(id, cancel, None, |j| j.state = "done".into())?;
                return Ok(());
            }
            let running_job = self.worker_update(id, cancel, None, |j| {
                j.state = "running".into();
                j.attempt += 1;
                for m in &mut j.messages {
                    if queued.iter().any(|q| q.id == m.id) {
                        m.state = "received".into();
                    }
                }
            })?;
            let mut runtime = self.0.runtime.lock().unwrap().clone();
            runtime.options.host = None;
            runtime.background = false;
            runtime.agent = if let Some(persona) = runtime
                .roster
                .iter()
                .find(|agent| agent.name == job.role)
                .cloned()
            {
                persona
            } else if job.role == "explore" {
                crate::explore_agent()
            } else if job.role == "plan" {
                crate::plan_agent()
            } else {
                crate::writer_agent()
            };
            runtime.agent.name = job.name.clone();
            runtime.agent.model = job.model_tier.clone();
            runtime.agent.steps = kanzei_harness::defs::effective_agent_steps(
                runtime.agent.steps,
                kanzei_harness::AgentMode::Subagent,
            );
            runtime.writable = writing;
            let tree = job
                .worktree
                .clone()
                .unwrap_or_else(|| self.0.ctx.cwd.clone());
            let mut rctx = self.0.config.lock().unwrap().clone();
            rctx.cwd = tree.clone();
            let mut harness = Harness::default();
            if writing {
                harness.add(crate::WritableSubagentBase);
            } else {
                harness.add(crate::SubagentBase);
            }
            harness
                .add(MarkdownComponent)
                .add(ConfigComponent)
                .add(tools::TeamTools {
                    team: self.clone(),
                    id: id.into(),
                    tree: tree.clone(),
                    writing,
                })
                .add(crate::GeneralChatProfile);
            runtime.snapshot = harness.resolve(&rctx)?;
            runtime.roster.clear();
            let parent_router = runtime.ask_router.clone();
            let team_router = self.0.ask_router.lock().unwrap().clone();
            let ask_job = running_job.clone();
            let ask_cancel = cancel.clone();
            let ask_team = self.clone();
            let ask_id = id.to_owned();
            let ask_slot = slot.clone();
            let ask_slots = self.0.slots.clone();
            runtime.ask_router = Some(Arc::new(move |request| {
                let immediate = matches!(&request,kanzei_core::AskRequest::Permission{action,..} if action=="subagent-write");
                let background = matches!(
                    &request,
                    kanzei_core::AskRequest::Question {
                        background: true,
                        ..
                    }
                );
                let reply = ask_team.begin_ask(
                    &ask_job,
                    &ask_cancel,
                    request,
                    team_router.as_ref(),
                    parent_router.as_ref(),
                );
                if background || immediate {
                    return reply;
                }
                let team = ask_team.clone();
                let id = ask_id.clone();
                let slot = ask_slot.clone();
                let slots = ask_slots.clone();
                let cancel = ask_cancel.clone();
                let attempt = ask_job.attempt;
                Box::pin(async move {
                    slot.lock().unwrap().take();
                    if team
                        .worker_update(&id, &cancel, Some(attempt), |j| {
                            j.state = "waiting_user".into();
                            j.latest = "等待你的答复".into();
                        })
                        .is_err()
                    {
                        return kanzei_core::AskResponse::Cancelled;
                    }
                    let result = reply.await;
                    let permit = match slots.acquire_owned().await {
                        Ok(permit) => permit,
                        Err(_) => return kanzei_core::AskResponse::Cancelled,
                    };
                    if team
                        .worker_update(&id, &cancel, Some(attempt), |j| {
                            j.state = "running".into();
                            j.latest = "已收到答复，继续执行".into();
                        })
                        .is_err()
                    {
                        return kanzei_core::AskResponse::Cancelled;
                    }
                    *slot.lock().unwrap() = Some(permit);
                    result
                })
            }));
            let history_store = self.0.store.clone();
            let history_id = id.to_owned();
            let history_owner = cancel.owner.clone();
            runtime.transcript_provider = Some(Arc::new(move |_| {
                let _owner = &history_owner;
                history_store.history(&history_id).ok()
            }));
            runtime.transcript_sink = Some(self.transcript_sink(id, cancel, running_job.attempt));
            runtime.agent.system.push_str("\nYou own one delegated task. Work only in your assigned checkout. Do not modify the parent checkout, merge, publish, or claim project delivery. Run relevant checks and report actual evidence, failures, files, and remaining work. team_message sends findings to main (to=main) or another task; question with background=true asks the user without suspending independent work and routes the answer back here; agent_memory reads/writes your persistent project memory. A completed turn is a candidate result, not acceptance.");
            if job.role == "verify" {
                runtime.agent.system.push_str("\nYour primary job is independent verification. Run tests against the specified candidate, report reproducible failures. Do not silently fix the implementation you are verifying.");
            }
            let deps: Vec<_> = job
                .depends_on
                .iter()
                .filter_map(|d| self.0.store.get(d).ok())
                .collect();
            let prompt = format!(
                "{}\n\nAssigned checkout: {}\nDependency results: {}",
                queued
                    .iter()
                    .map(|m| format!("{}: {}", m.from, m.text))
                    .collect::<Vec<_>>()
                    .join("\n"),
                tree.display(),
                serde_json::to_string(&deps.iter().map(result_summary).collect::<Vec<_>>())?
            );
            let mut ctx = self.0.ctx.clone();
            ctx.read_ledger = Some(
                self.0
                    .child_ledgers
                    .lock()
                    .unwrap()
                    .entry(id.to_string())
                    .or_default()
                    .clone(),
            );
            ctx.cwd = tree.clone();
            ctx.worktree_key = Some(crate::worktree::worktree_key(&tree));
            ctx.process_id = Some(id.into());
            ctx.run_id = Some(format!("{}:{id}:{}", self.0.owner, job.attempt + 1));
            ctx.async_mailbox = Some({
                let _lifecycle = self.0.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
                self.check_worker_locked(id, cancel, Some(running_job.attempt))?;
                self.child_mailbox(id)
            });
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            // Only an explicit continuation (or the explicitly requested fork)
            // restores the durable child transcript. A new spawn starts fresh.
            let resume = job.attempt > 0 || !self.0.store.history(id)?.is_empty();
            let input =
                json!({"prompt":prompt,"model":job.model_tier,"schema":job.schema,"resume":resume});
            let future = kanzei_core::run_subagent(&self.0.client, &runtime, &ctx, id, &input, tx);
            tokio::pin!(future);
            let mut timer = tokio::time::interval(Duration::from_secs(1));
            let mut charged = Duration::ZERO;
            let mut last_tick = std::time::Instant::now();
            let mut stream = String::new();
            let mut last_delta = std::time::Instant::now();
            let output = loop {
                tokio::select! {
                    out=&mut future=>break out,
                    _=timer.tick()=>{
                        let elapsed = last_tick.elapsed(); last_tick = std::time::Instant::now();
                        if self.0.store.get(id)?.state != "waiting_user" { charged += elapsed; }
                        if charged >= Duration::from_secs(runtime.timeout_secs) { bail!("子任务超时，已保存上下文，可继续"); }
                    },
                    Some(event)=rx.recv()=>self.observe(id,event,&mut stream,&mut last_delta)?,
                }
            };
            // Completion can win select while the final usage/tool events are queued.
            while let Ok(event) = rx.try_recv() {
                self.observe(id, event, &mut stream, &mut last_delta)?;
            }
            if writing {
                let base = job.base.as_deref().context("missing child base")?;
                let (head, files) = workspace::result(&tree, base, id)?;
                self.worker_update(id, cancel, Some(running_job.attempt), |j| {
                    j.head = Some(head);
                    j.files = files;
                    j.outcome = "candidate".into();
                })?;
            }
            let failed = output.is_error || output.code == Some("subagent_empty_answer");
            self.worker_update(id, cancel, Some(running_job.attempt), |j| {
                j.result = output.content.clone();
                j.state = if failed { "failed" } else { "done" }.into();
                if output.code == Some("subagent_step_limit_reached") {
                    j.outcome = "needs_correction".into();
                } else if !failed && j.outcome == "needs_correction" {
                    j.outcome = "candidate".into();
                }
                j.latest = if failed {
                    output.content.clone()
                } else {
                    "已返回结果，等待整合".into()
                };
                j.revision += 1;
                for m in &mut j.messages {
                    if m.state == "received" {
                        m.state = "processed".into();
                    }
                }
            })?;
            if failed {
                return Ok(());
            }
        }
    }
}

impl DelegationHost for AgentTeam {
    fn set_parent_context(&self, messages: &[Message]) {
        self.set_parent(messages);
    }
    fn has_updates(&self) -> bool {
        AgentTeam::has_updates(self)
    }
    fn stop_all(&self) {
        AgentTeam::stop_all(self);
    }
    fn spec(&self) -> ToolSpec {
        let mut roles = vec![
            "explore".to_string(),
            "plan".into(),
            "general".into(),
            "implement".into(),
            "verify".into(),
        ];
        roles.extend(
            self.0
                .runtime
                .lock()
                .unwrap()
                .roster
                .iter()
                .map(|agent| agent.name.clone()),
        );
        roles.sort();
        roles.dedup();
        ToolSpec {
            name: "task".into(),
            description: "Delegate real work. Registered custom agents preserve their system prompt and step budget and run read-only. spawn supports explore/plan (read-only) and implement/verify/general (isolated writable Git checkout); background defaults true. Use message/resume to continue the SAME task/history, restart to create a NEW task retaining the old record. list/get return bounded status/result summaries; list uses offset/limit and returns jobs/total/next_offset. For explicit detail use get view=result or prompt; view=history, trace, messages returns a page of items with offset/limit (max 20) and next_offset. collect reads completed updates without waiting; wait only for an explicit dependency; diff/adopt integrates reviewed changes. Dependencies use existing task IDs. Never claim a dispatched task is complete. If scouts already cover the topic, specify the independent gap before adding another explore/plan task. Enable context=fork only when the full conversation is needed. Independent tasks may run concurrently.".into(),
            input_schema: json!({"type":"object","properties":{
                "action":{"type":"string","enum":["spawn","list","get","message","resume","restart","stop","wait","collect","diff","adopt"]},
                "id":{"type":"string"},"prompt":{"type":"string"},"description":{"type":"string"},
                "agent":{"type":"string","enum":roles},
                "model":{"type":"string","enum":["fast","primary"]},"schema":{"type":"object"},
                "background":{"type":"boolean"},"context":{"type":"string","enum":["fresh","fork"]},
                "view":{"type":"string","enum":["summary","history","trace","messages","result","prompt"]},
                "offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":20},
                "depends_on":{"type":"array","items":{"type":"string"}}
            }}),
        }
    }
    fn execute(&self, call_id: String, input: Value) -> DelegationFuture {
        let team = self.clone();
        Box::pin(async move {
            match team.command(&call_id, input).await {
                Ok(value) => match team.acknowledge_result(&value) {
                    Ok(()) => ToolOutput::ok(value.to_string()),
                    Err(e) => ToolOutput::error(e.to_string()),
                },
                Err(e) => ToolOutput::error(e.to_string()),
            }
        })
    }
}
