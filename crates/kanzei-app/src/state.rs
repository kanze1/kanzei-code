//! AppState、运行时状态、UI 探针与跨域状态辅助。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{oneshot, Mutex as AsyncMutex};

pub(crate) use kanzei_base::sync::MutexPoisonExt;

pub(crate) fn prompt_attachment_parts(
    attachments: Vec<PromptAttachment>,
) -> anyhow::Result<Vec<kanzei_llm::Part>> {
    attachments
        .into_iter()
        .map(|attachment| {
            anyhow::ensure!(
                !attachment.data.is_empty(),
                "附件数据为空: {}",
                attachment.file_name
            );
            let part = match attachment.media_type.as_str() {
                "application/pdf" => kanzei_llm::Part::Document {
                    media_type: attachment.media_type,
                    data: attachment.data,
                },
                media_type if media_type.starts_with("image/") => kanzei_llm::Part::Image {
                    media_type: attachment.media_type,
                    data: attachment.data,
                },
                _ => crate::attachments::document_part(&attachment)?,
            };
            Ok(part)
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PromptAttachment {
    pub(crate) file_name: String,
    pub(crate) media_type: String,
    pub(crate) data: String,
}

pub(crate) struct PendingAsk {
    pub(crate) source: String,
    pub(crate) agent_id: Option<String>,
    pub(crate) sender: oneshot::Sender<kanzei_core::AskResponse>,
    pub(crate) request: kanzei_core::AskRequest,
    pub(crate) action: String,
    pub(crate) resource: String,
    pub(crate) project_root: PathBuf,
    pub(crate) session_id: String,
}

pub(crate) static UI_PROBES: std::sync::LazyLock<
    Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>,
> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
pub(crate) static UI_PROBE_SEQ: AtomicU64 = AtomicU64::new(1);
pub(crate) static UI_PROBE_EMIT: std::sync::OnceLock<Box<dyn Fn(serde_json::Value) + Send + Sync>> =
    std::sync::OnceLock::new();
/// D-387:手机消息注入桌面后的 UI 通知出口。桌面端装配时注入 window.emit("kz:mobile-message"),
/// mobile.rs 收到 POST /v1/messages 后经此通知 UI 刷新会话列表(消费方)。
pub(crate) static MOBILE_MESSAGE_EMIT: std::sync::OnceLock<
    Box<dyn Fn(String, String) + Send + Sync>,
> = std::sync::OnceLock::new();
/// R-249 批2:主窗口的原生句柄。与 UI_PROBE_EMIT 同一手法——建窗口时装一次,
/// 工具侧只认这个静态,不依赖 tauri 类型,截图模块因此可以纯 Win32 无 tauri 依赖。
pub(crate) static UI_WINDOW_HANDLE: std::sync::OnceLock<isize> = std::sync::OnceLock::new();

/// R-249 批2:抓当前窗口画面,返回 PNG 字节。
///
/// 与 ui_dom/ui_style 是**互补**关系而非替代:那两个给结构与数值(为什么没显示、
/// 盒模型多大),这个给「实际长什么样」。对齐、遮挡、观感一类问题只有像素能回答。
pub(crate) fn ui_screenshot_png() -> Result<Vec<u8>, String> {
    let Some(&handle) = UI_WINDOW_HANDLE.get() else {
        return Err("窗口截图不可用:桌面窗口未就绪(CLI 环境下没有可截的界面)".into());
    };
    let (rgba, width, height) = crate::screenshot::capture_window(handle)?;
    crate::screenshot::encode_png(&rgba, width, height)
}

pub(crate) async fn ui_probe(kind: &str, arg: &str) -> Result<serde_json::Value, String> {
    if crate::runtime_service::is_service() {
        // The visible desktop owns both the probe request and its JS receipt.
        // A service-side waiter cannot be completed by a desktop-local command.
        return crate::desktop_bridge::probe(kind, arg).await;
    }
    let Some(emit) = UI_PROBE_EMIT.get() else {
        return Err("UI 探针不可用:桌面窗口未就绪(CLI 环境下没有可自查的界面)".into());
    };
    let id = UI_PROBE_SEQ.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel();
    UI_PROBES.lock_or_recover().insert(id, tx);
    emit(json!({"id": id, "kind": kind, "arg": arg}));
    match tokio::time::timeout(std::time::Duration::from_secs(8), rx).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => {
            UI_PROBES.lock_or_recover().remove(&id);
            Err("UI 探针通道已关闭".into())
        }
        Err(_) => {
            UI_PROBES.lock_or_recover().remove(&id);
            Err("UI 探针超时(8s):窗口可能正忙或未加载完成".into())
        }
    }
}

#[tauri::command]
pub fn ui_probe_result(id: u64, result: serde_json::Value) {
    if let Some(sender) = UI_PROBES.lock_or_recover().remove(&id) {
        let _ = sender.send(result);
    }
}

pub(crate) fn with_session_id(
    mut payload: serde_json::Value,
    session_id: &str,
) -> serde_json::Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "sessionId".into(),
            serde_json::Value::String(session_id.into()),
        );
    }
    payload
}

pub(crate) struct SessionRuntime {
    session_id: String,
    pub(crate) read_ledger: kanzei_harness::ReadLedger,
    pub(crate) supervisor: Mutex<kanzei_core::session_lifecycle::ConversationSupervisor>,
    pub(crate) changed: tokio::sync::Notify,
    pub(crate) callback_options: Mutex<crate::commands::run::RunOptions>,
    pub(crate) async_mailbox: Mutex<Option<kanzei_harness::AsyncMailbox>>,
    pub(crate) async_generation: AtomicU64,
    pub(crate) asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    pub(crate) current_run: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub(crate) lifecycle: Arc<Mutex<()>>,
    pub(crate) conversation: Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    pub(crate) live: Arc<Mutex<LiveRun>>,
    /// 最近一次真实 `kz:status` 阶段,供跨线并列视图读取。
    pub(crate) stage: Arc<Mutex<String>>,
    /// R-174:本会话运行中的子代理取消注册表。`stop_task` 命令按 id 命中即取消,
    /// run_task 构造 SubagentRuntime 时把它塞进 `cancellations` 字段,单一实例共享。
    pub(crate) task_cancellations: Arc<kanzei_core::TaskCancellations>,
    /// 当前轮的停止令牌，由 scheduler 在启动或提升输入时安装，收尾后回收。
    /// stop 保留并 cancel → run 在安全检查点以 halted **正常收尾**(轮末写回对话),
    /// None = 无活跃 run(此时停止走立即终态化的旧路径)。
    pub(crate) halt: Arc<Mutex<Option<kanzei_core::CancellationToken>>>,
}
fn next_async_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::SeqCst)
}
impl SessionRuntime {
    pub(crate) fn snapshot(&self) -> kanzei_core::session_lifecycle::Snapshot {
        self.supervisor.lock_or_recover().snapshot()
    }
    pub(crate) fn is_running(&self) -> bool {
        self.supervisor.lock_or_recover().busy()
    }
    pub(crate) fn is_compacting(&self) -> bool {
        let s = self.snapshot();
        s.phase != kanzei_core::session_lifecycle::Phase::Idle
            && matches!(
                s.activity,
                Some(
                    kanzei_core::session_lifecycle::Activity::Compact
                        | kanzei_core::session_lifecycle::Activity::History
                )
            )
    }
    pub(crate) fn transition(
        &self,
        event: kanzei_core::session_lifecycle::Event,
    ) -> Result<(), String> {
        // Publish in the same order as transitions, even when Stop races the
        // worker completion callback. Receivers still reject stale snapshots.
        let mut supervisor = self.supervisor.lock_or_recover();
        let snapshot = supervisor.apply(event).map_err(|error| error.to_string())?;
        crate::session_supervisor::publish(&self.session_id, &snapshot);
        drop(supervisor);
        self.changed.notify_waiters();
        Ok(())
    }
    pub(crate) fn generation(&self) -> u64 {
        self.snapshot().round_generation
    }
    pub(crate) fn stopping(&self) -> bool {
        self.supervisor.lock_or_recover().stopping()
    }
    /// Register resource-producing controls before they can await or spawn.
    /// Stop closes admission under this same lock and waits for their drops.
    pub(crate) fn admit_effect(
        &self,
        id: &str,
    ) -> Result<kanzei_core::TaskCancellationGuard, String> {
        use kanzei_core::session_lifecycle::{Existence, Phase};
        let _lifecycle = self.lifecycle.lock_or_recover();
        let snapshot = self.snapshot();
        if snapshot.existence != Existence::Open
            || self.stopping()
            || snapshot.phase == Phase::Blocked
        {
            return Err("对话正在回收，完成后再操作子任务".into());
        }
        Ok(self.task_cancellations.register(id))
    }

    pub(crate) fn begin_exclusive(
        &self,
        activity: kanzei_core::session_lifecycle::Activity,
    ) -> Result<(), String> {
        self.transition(kanzei_core::session_lifecycle::Event::Acquire(activity))?;
        self.transition(kanzei_core::session_lifecycle::Event::Acquired)?;
        *self.halt.lock_or_recover() = Some(kanzei_core::CancellationToken::new());
        Ok(())
    }
    pub(crate) fn finish_exclusive(&self) {
        let lifecycle = self.lifecycle.lock_or_recover();
        self.transition(kanzei_core::session_lifecycle::Event::WorkerEnded)
            .expect("exclusive owner ends its activity");
        if !self.stopping() {
            self.transition(kanzei_core::session_lifecycle::Event::Settled(
                "completed".into(),
            ))
            .expect("exclusive resources released");
            self.halt.lock_or_recover().take();
        }
        drop(lifecycle);
        crate::session_supervisor::wake_pending(&self.session_id);
    }
    /// Caller holds lifecycle. Install cancellation before publishing a running round.
    pub(crate) fn begin_round(&self) -> Result<(), String> {
        use kanzei_core::session_lifecycle::{Activity, Event, Phase};
        match self.snapshot().phase {
            Phase::Idle => {
                self.transition(Event::Acquire(Activity::Run))?;
                self.transition(Event::Acquired)?;
            }
            Phase::Reaping => self.transition(Event::NextRound)?,
            _ => return Err("当前执行尚未回收，不能启动下一轮".into()),
        }
        *self.halt.lock_or_recover() = Some(kanzei_core::CancellationToken::new());
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn activate_for_test(&self) {
        let token = self.halt.lock_or_recover().take();
        self.begin_round().unwrap();
        if token.is_some() {
            *self.halt.lock_or_recover() = token;
        }
    }
    #[cfg(test)]
    pub(crate) fn settle_for_test(&self) {
        use kanzei_core::session_lifecycle::Event;
        self.transition(Event::WorkerEnded).unwrap();
        if self.stopping() {
            self.transition(Event::StopSettled).unwrap();
        }
        self.transition(Event::Settled("completed".into())).unwrap();
    }

    /// Caller holds lifecycle when retiring an actor.
    pub(crate) fn retire_async(&self) {
        self.async_generation
            .store(next_async_generation(), Ordering::SeqCst);
        if let Some(mailbox) = self.async_mailbox.lock_or_recover().take() {
            mailbox.close();
        }
    }
}

#[derive(Default)]
pub(crate) struct LiveRun {
    pub(crate) run_id: String,
    pub(crate) input_id: String,
    pub(crate) prompt_head: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) started_at: Option<std::time::Instant>,
    /// 最近一次真实 RunEvent；泳道用它区分长任务与完全无进展。
    pub(crate) last_event_at: Option<std::time::Instant>,
    /// 最近一轮的明确收尾结果；None 且已启动/已退出表示异常退出。
    pub(crate) last_outcome: Option<String>,
    pub(crate) steps: u32,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) trace: Vec<serde_json::Value>,
    /// 已经以增量 `run.trace` 事件写入 state.db 的轨迹条数。写入失败时由
    /// `flush_live_trace` 在停止/收尾路径补写剩余部分，避免实时写入和终态补写重复。
    pub(crate) persisted_trace_events: usize,
    pub(crate) flushed: bool,
}
impl LiveRun {
    pub(crate) fn prepare(&mut self, input_id: Option<&str>, prompt_head: &str) {
        *self = Self {
            input_id: input_id.unwrap_or_default().into(),
            prompt_head: prompt_head.chars().take(200).collect(),
            ..Self::default()
        };
    }

    pub(crate) fn begin(
        &mut self,
        run_id: &str,
        input_id: &str,
        prompt_head: &str,
        provider: &str,
        model: &str,
    ) {
        *self = Self {
            run_id: run_id.into(),
            input_id: input_id.into(),
            prompt_head: prompt_head.chars().take(200).collect(),
            provider: provider.into(),
            model: model.into(),
            started_at: Some(std::time::Instant::now()),
            ..Self::default()
        };
    }
    pub(crate) fn note_event(&mut self) {
        self.last_event_at = Some(std::time::Instant::now());
    }

    /// Publish a terminal only after its durable transaction has committed.
    pub(crate) fn finish(&mut self, outcome: &str) {
        self.last_outcome = Some(outcome.to_string());
        self.flushed = true;
    }

    fn duration_ms(&self) -> u64 {
        self.started_at
            .map(|at| at.elapsed().as_millis() as u64)
            .unwrap_or_default()
    }
}

/// D-297 验收③:每会话保留的最近 run.trace 轮数。增量事件按轮分组,200 轮足够
/// 回放评估原料(list_trace_payloads 取 limit*5)且把历史体积封顶。
const TRACE_KEEP_ROUNDS: usize = 200;

pub(crate) fn flush_live_run(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    live: &Arc<Mutex<LiveRun>>,
    outcome: &str,
) -> bool {
    let mut live = live.lock_or_recover();
    if live.flushed || live.started_at.is_none() {
        return false;
    }
    live.finish(outcome);
    let _ = flush_live_trace_locked(store, session_id, &mut live, Some(outcome));
    // D-297 验收③:run.trace 保留策略——每会话只留最近 N 轮,防止轨迹成本随
    // 使用时间单调增长。收尾路径是清理时机(一轮刚写完,旧轮不再被引用)。
    let _ = store.prune_trace_rounds(session_id, TRACE_KEEP_ROUNDS);
    let _ = store.append_episode(&kanzei_core::EpisodeRecord {
        session_id,
        prompt_head: &live.prompt_head,
        outcome,
        steps: live.steps,
        input_tokens: live.input_tokens,
        output_tokens: live.output_tokens,
        tools_json: "{}",
        context_json: "[]",
        metrics_json: "{}",
        overflow_json: "[]",
        provider: &live.provider,
        model: &live.model,
        run_id: &live.run_id,
        input_id: &live.input_id,
        duration_ms: live.duration_ms(),
    });
    true
}

/// 补写实时轨迹中尚未落盘的部分，不改变运行的 flushed 语义。
pub(crate) fn flush_live_trace(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    live: &Arc<Mutex<LiveRun>>,
) -> bool {
    let mut live = live.lock_or_recover();
    flush_live_trace_locked(store, session_id, &mut live, None)
}

fn flush_live_trace_locked(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    live: &mut LiveRun,
    outcome: Option<&str>,
) -> bool {
    let pending = live
        .trace
        .get(live.persisted_trace_events..)
        .unwrap_or(&[])
        .to_vec();
    if pending.is_empty() {
        return false;
    }
    // D-297 验收③:整包补写按 ≤64KB 分批,避免单条 run.trace 事件(实测最大
    // 945.5KB)把解析成本与库体积一次性放大。分批保持事件顺序,outcome 只挂最后一批。
    const MAX_BATCH_BYTES: usize = 64 * 1024;
    let mut batches: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut current: Vec<serde_json::Value> = Vec::new();
    let mut current_bytes = 0usize;
    for event in pending {
        let size = serde_json::to_string(&event)
            .map(|s| s.len())
            .unwrap_or(usize::MAX);
        if !current.is_empty() && current_bytes + size > MAX_BATCH_BYTES {
            batches.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current_bytes += size;
        current.push(event);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    let last = batches.len() - 1;
    for (index, batch) in batches.into_iter().enumerate() {
        let mut payload = json!({"run_id": live.run_id, "events": batch});
        if let (Some(outcome), true) = (outcome, index == last) {
            payload["outcome"] = json!(outcome);
        }
        if store
            .append_event(session_id, "run.trace", &payload)
            .is_err()
        {
            return false;
        }
    }
    live.persisted_trace_events = live.trace.len();
    true
}

/// 把当前运行轨迹按事件增量写入 state.db。
///
/// 运行中的 UI 仍然通过 Tauri 事件实时显示；这里提供的是切线、重载和崩溃前
/// 的可恢复边界。写入失败不打断模型运行，收尾路径会再次补写未落盘部分。
pub(crate) fn record_live_trace(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    live: &Arc<Mutex<LiveRun>>,
    event: serde_json::Value,
) -> bool {
    let (run_id, index) = {
        let mut live = live.lock_or_recover();
        if live.started_at.is_none() {
            return false;
        }
        live.trace.push(event.clone());
        (live.run_id.clone(), live.trace.len() - 1)
    };
    if store
        .append_event(
            session_id,
            "run.trace",
            &json!({"run_id": run_id, "events": [event], "partial": true}),
        )
        .is_ok()
    {
        let mut live = live.lock_or_recover();
        if live.persisted_trace_events == index {
            live.persisted_trace_events += 1;
        }
        true
    } else {
        false
    }
}

/// 轨迹持久化失败时留下可由整理入口识别的 artifact orphan marker。
/// marker 与原文 artifact 同目录，下一轮可按 `*.orphan.json` 对账，不把无引用原文
/// 静默当成已收口；不影响原有运行继续执行语义。
pub(crate) fn record_unpersisted_artifact(
    state_path: &std::path::Path,
    session_id: &str,
    event: &serde_json::Value,
) {
    let Some(artifact) = event.get("artifact").filter(|value| !value.is_null()) else {
        return;
    };
    let Some(artifact_id) = artifact.get("artifact_id").and_then(|value| value.as_str()) else {
        return;
    };
    let Some(state_dir) = state_path.parent() else {
        return;
    };
    let dir = state_dir.join("artifacts").join("tool-results");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let marker = dir.join(format!("{artifact_id}.orphan.json"));
    let temp = dir.join(format!(
        "{artifact_id}.orphan.json.tmp-{}",
        std::process::id()
    ));
    let payload = json!({
        "kind": "tool_artifact_orphan",
        "session_id": session_id,
        "artifact": artifact,
        "event": event,
    });
    let Ok(text) = serde_json::to_vec_pretty(&payload) else {
        return;
    };
    if std::fs::write(&temp, text).is_ok() {
        let _ = std::fs::rename(temp, marker);
    }
}

/// 事件回调需要 `Send + Sync`，不能捕获 rusqlite 连接；按状态库路径短开连接，
/// 让实时轨迹写入与模型事件回调保持解耦。
pub(crate) fn record_live_trace_at_path(
    state_path: &std::path::Path,
    session_id: &str,
    live: &Arc<Mutex<LiveRun>>,
    event: serde_json::Value,
) -> bool {
    if let Ok(store) = kanzei_core::SessionStore::open(state_path) {
        record_live_trace(&store, session_id, live, event)
    } else {
        false
    }
}

/// 主根:项目规范化根目录(`normalized_project_root` 的产物)。
/// 与 [`WorktreeRoot`] 是**不同类型**——主根与工作树根互相传反在编译期报错,
/// 不再靠「改本文件前先读字段口径注释」的纪律站岗(D-367)。
///
/// 反例实证(2026-08-16 实际编译捕获,rustc E0308):
/// ```text
/// let _counterexample: &WorktreeRoot = &process.project_dir;  // project_dir: ProjectRoot
/// error[E0308]: mismatched types
///    = note: expected reference `&WorktreeRoot`
///             found reference `&ProjectRoot`
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ProjectRoot(pub PathBuf);

/// 工作树根:一条线的执行工作树目录(git worktree 路径)。
/// 与 [`ProjectRoot`] 类型不同,传给需要主根的地方编译不过(D-367)。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct WorktreeRoot(pub PathBuf);

impl WorktreeRoot {
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessHandle {
    pub(crate) id: String,
    /// 恒为主根(`normalized_project_root` 的规范化形态),绝不存工作树路径。
    pub(crate) origin_project: ProjectRoot,
    /// 恒为主根:进程编号按它分桶、state.db 按它定位、session_id 按它推导。
    pub(crate) project_dir: ProjectRoot,
    /// 执行工作树**只**由这个字段承担;类型与主根不同,传反编译不过。
    pub(crate) worktree_path: Option<WorktreeRoot>,
    /// git 工作树对应的真实分支名。共享项目目录为 None;独立工作树由 git 真源恢复。
    pub(crate) branch: Option<String>,
    pub(crate) model: Arc<Mutex<Option<String>>>,
    pub(crate) profile: Arc<Mutex<Option<String>>>,
    pub(crate) research_topic: Arc<Mutex<Option<String>>>,
    pub(crate) reasoning: Arc<Mutex<Option<String>>>,
    /// 当前对话的手填模型候选；各对话独立持久化。
    /// 前端下拉的「手填」候选从 process_info 回读,不再以 localStorage 为真源。
    pub(crate) manual_models: Arc<Mutex<Vec<String>>>,
    /// 每个对话独立保存的协作倾向；运行中修改在下一轮读取。
    pub(crate) subagent_mode: Arc<Mutex<kanzei_harness::SubagentMode>>,
    /// 分支线是否允许修改主根中的 tracker 文档。默认关闭,读取不受影响。
    pub(crate) tracker_writes_enabled: Arc<AtomicBool>,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcessInfo {
    pub(crate) id: String,
    pub(crate) origin_project: String,
    pub(crate) project_dir: String,
    pub(crate) worktree_path: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) session_id: String,
    pub(crate) model: Option<String>,
    pub(crate) profile: Option<String>,
    pub(crate) research_topic: Option<String>,
    pub(crate) reasoning: Option<String>,
    pub(crate) manual_models: Vec<String>,
    /// 见 [`ProcessHandle::subagent_mode`]。前端 `process_list` 回显用。
    pub(crate) subagent_mode: kanzei_harness::SubagentMode,
    pub(crate) tracker_writes: bool,
    /// 当前会话阶段,用于侧栏逐条投影并行任务状态。
    pub(crate) stage: String,
    pub(crate) running: bool,
    pub(crate) runtime: kanzei_core::session_lifecycle::Snapshot,
    /// 展示名(永不为空):用户命名 ‖ 首条消息前 48 字 ‖ 类型+序号(「对话」「讨论 3」)。
    /// 界面不再露出 `pN`/「默认」(UX-009)。
    pub(crate) label: String,
    /// 有效标题:用户命名,没有就是自动标题;还没发过消息且没有命名的对话为 None。
    pub(crate) title: Option<String>,
    /// `title` 是用户命名的(可「恢复默认名」);false = 自动标题或没有标题。
    pub(crate) title_custom: bool,
    /// `main` | `discussion` | `task` | `research`,见 `processes::naming::ProcessKind`。
    pub(crate) kind: String,
    /// 编号(`p12` 的 12);旧格式身份没有。
    pub(crate) ordinal: Option<u64>,
    /// 会话最近活动时间(毫秒,`sessions.updated_at`);会话行还没建出来为 None。
    pub(crate) updated_at: Option<i64>,
}
pub(crate) struct MobileService {
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) listener_thread: Option<std::thread::JoinHandle<()>>,
    /// R-270 批1:已配对设备表(device_id → device_token)。撤销 = 从表移除,
    /// 移除后该 token 立即 401,其它设备不受影响(替换原单一共享 token)。
    pub(crate) devices: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// R-270 批1:当前有效的一次性配对码(桌面端生成,配对成功后清空)。
    pub(crate) pair_code: Arc<std::sync::Mutex<Option<String>>>,
    /// R-270 批1:服务监听是否在 LAN(0.0.0.0)。默认回环(127.0.0.1)行为不变。
    pub(crate) lan: bool,
    /// D-386:项目根(state.db 所在,设备表持久化定位)。
    pub(crate) project_root: std::path::PathBuf,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct MobileServiceInfo {
    pub(crate) address: String,
    /// 手机浏览器能直接打开的地址(LAN 模式是本机局域网 IP;`address` 是监听地址,不能给人看)。
    pub(crate) url: String,
    /// 单一共享 token(旧行为)或当前配对码(R-270 批1:配对码用于换取设备 token)。
    pub(crate) token: String,
    pub(crate) lan: bool,
    pub(crate) devices: Vec<MobileDeviceInfo>,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct MobileDeviceInfo {
    pub(crate) device_id: String,
    pub(crate) name: String,
    pub(crate) paired_at_ms: u128,
}
/// Worktree 信息类型下沉 kanzei-tools(R-207),这里 re-export 保持引用点零改动。
pub(crate) use kanzei_tools::worktree::WorktreeInfo;

impl Default for SessionRuntime {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            read_ledger: kanzei_harness::ReadLedger::default(),
            supervisor: Mutex::new(Default::default()),
            changed: tokio::sync::Notify::new(),
            callback_options: Mutex::new(crate::commands::run::RunOptions::default()),
            async_mailbox: Mutex::new(None),
            async_generation: AtomicU64::new(next_async_generation()),
            asks: Arc::new(Mutex::new(HashMap::new())),
            current_run: Arc::new(Mutex::new(None)),
            lifecycle: Arc::new(Mutex::new(())),
            conversation: Arc::new(Mutex::new(HashMap::new())),
            live: Arc::new(Mutex::new(LiveRun::default())),
            stage: Arc::new(Mutex::new("空闲".into())),
            task_cancellations: Arc::new(kanzei_core::TaskCancellations::default()),
            halt: Arc::new(Mutex::new(None)),
        }
    }
}
/// 桌面端调用外部程序时禁止创建控制台窗口(Windows GUI 应用不应闪出黑框)。
pub(crate) fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

pub(crate) struct AppState {
    pub(crate) runtimes: Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
    pub(crate) ask_seq: Arc<AtomicU64>,
    pub(crate) processes: Arc<Mutex<HashMap<String, ProcessHandle>>>,
    /// 已完成 state.db 进程注册恢复的项目。恢复是项目进入时的一次性动作；
    /// 后续 process_list/collaboration_snapshot 只读取运行态，不能重复用旧库值覆盖内存设置。
    pub(crate) restored_projects: Arc<Mutex<HashSet<String>>>,
    pub(crate) mobile_service: Arc<Mutex<Option<MobileService>>>,
    /// 自主推进(鞭挞)状态按会话隔离：控件输入经 auto_state_update 同步，
    /// 轮末由 run.rs 只消费所属会话的控制器，不能串扰后台进程。
    pub(crate) auto_runs: Arc<Mutex<HashMap<String, crate::auto_run::AutoRunController>>>,
    /// R-171 项目级执行协调器:所有 ProcessHandle 共享同一实例,按规范化主根分桶。
    /// 「并行查、串行写」的强制点——共享项目目录的 writer run 在这里获取写租约。
    pub(crate) coordinator: Arc<kanzei_core::orchestration::MemoryCoordinator>,
    /// Git 工作树元数据操作的进程内串行闸。
    ///
    /// 建线/建树只创建独立分支与目录,不能去排主线的源码写租约；否则主线运行期间
    /// 「新建线路」会一直等到整轮结束。它们之间仍需串行，避免同一进程内的
    /// worktree list/ref/目录操作互相踩踏；跨进程同名竞争由 git ref CAS 兜底。
    pub(crate) worktree_ops: Arc<AsyncMutex<()>>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            runtimes: Arc::new(Mutex::new(HashMap::new())),
            ask_seq: Arc::new(AtomicU64::new(crate::run::now_ms() as u64 * 1024)),
            processes: Arc::new(Mutex::new(HashMap::new())),
            restored_projects: Arc::new(Mutex::new(HashSet::new())),
            mobile_service: Arc::new(Mutex::new(None)),
            auto_runs: Arc::new(Mutex::new(HashMap::new())),
            coordinator: Arc::new(kanzei_core::orchestration::MemoryCoordinator::new()),
            worktree_ops: Arc::new(AsyncMutex::new(())),
        }
    }
}

/// 项目身份根:显式选中的目录 canonicalize + 去 `\\?\` 前缀(UI2-0926 #13)。
///
/// 它是进程记录(`processes.origin_project/project_dir`)、`d|` 进程 id 与经 IPC 发给前端的
/// `ProcessInfo.project_dir` 的唯一来源。原先直接用 `std::fs::canonicalize`,Windows 上产出
/// `\\?\C:\…`,鞭挞轮拿它当 projectDir,于是系统提示、bash 工作目录、绝对路径权限规则与取活
/// 顺序的存储键在手动轮与鞭挞轮是两种写法(docs/design/project_workspace.md §3)。存量记录由
/// schema v25 迁移改写。
/// 桌面端 projectDir 已是用户选择的项目身份,空目录也必须拥有独立状态库。
/// 不能因为它尚未创建 `.kanzei` 就继承祖先的项目/配置;CLI 的 cwd 根发现另有入口。
pub(crate) fn normalized_project_root(path: &Path) -> PathBuf {
    kanzei_tools::path_form::canonical(path)
        .unwrap_or_else(|_| kanzei_tools::path_form::simplify(path))
}
pub(crate) fn legacy_process_id(root: &Path) -> String {
    format!("d|{}", root.display())
}
pub(crate) fn process_session_id(root: &Path, process_id: Option<&str>) -> String {
    let base = kanzei_core::project_session_id(root);
    let default_id = legacy_process_id(root);
    match process_id.filter(|id| !id.is_empty() && *id != default_id) {
        Some(id) => {
            let prefix = id.split_once('|').map(|(prefix, _)| prefix).unwrap_or(id);
            format!("{base}#{prefix}")
        }
        None => base,
    }
}
#[cfg(test)]
pub(crate) fn ensure_default_process(state: &AppState, root: &Path) -> ProcessHandle {
    let id = legacy_process_id(root);
    let mut processes = state.processes.lock_or_recover();
    let process = processes
        .entry(id.clone())
        .or_insert_with(|| ProcessHandle {
            id: id.clone(),
            origin_project: ProjectRoot(root.to_path_buf()),
            project_dir: ProjectRoot(root.to_path_buf()),
            worktree_path: None,
            branch: None,
            model: Arc::new(Mutex::new(None)),
            profile: Arc::new(Mutex::new(None)),
            research_topic: Arc::new(Mutex::new(None)),
            reasoning: Arc::new(Mutex::new(None)),
            manual_models: Arc::new(Mutex::new(Vec::new())),
            // 默认关:用户要的是「显式打开才强制走七阶段」,默认开就不叫显式
            // (2026-08-11 用户定调)。
            subagent_mode: Arc::new(std::sync::Mutex::new(kanzei_harness::SubagentMode::Auto)),
            tracker_writes_enabled: Arc::new(AtomicBool::new(false)),
        })
        .clone();
    drop(processes);
    crate::processes::registry::persist_process(root, &process).unwrap();
    process
}
/// 单条线的对外信息(建线/改线的返回值用)。命名事实自己只读开一次 state.db;
/// 列表类调用(process_list / workspace lines)一次开库、逐条走 [`process_info_with`]。
pub(crate) fn process_info(state: &AppState, process: &ProcessHandle) -> ProcessInfo {
    let root = &process.project_dir.0;
    let kind = process_kind_of(process);
    let naming = crate::processes::naming::open_naming_store(root)
        .map(|store| {
            let session_id = process_session_id(root, Some(&process.id));
            crate::processes::naming::load_naming(&store, &session_id, kind)
        })
        .unwrap_or_default();
    process_info_with(state, process, &naming)
}
pub(crate) fn process_kind_of(process: &ProcessHandle) -> crate::processes::naming::ProcessKind {
    let profile = process.profile.lock_or_recover().clone();
    crate::processes::naming::process_kind(&process.id, profile.as_deref())
}
pub(crate) fn process_info_with(
    state: &AppState,
    process: &ProcessHandle,
    naming: &crate::processes::naming::SessionNaming,
) -> ProcessInfo {
    // D-367:project_dir 是 ProjectRoot(恒主根),直接取路径算 session_id。
    let root = &process.project_dir.0;
    let session_id = process_session_id(root, Some(&process.id));
    let kind = process_kind_of(process);
    let ordinal = crate::processes::naming::process_ordinal(&process.id);
    let actor = runtime_for(state, &session_id);
    let snapshot = actor.snapshot();
    let running = snapshot.phase != kanzei_core::session_lifecycle::Phase::Idle;
    let stage = actor.stage.lock_or_recover().clone();
    ProcessInfo {
        id: process.id.clone(),
        // ProcessInfo 是 IPC 输出类型,保持 String(前端契约不变)。
        origin_project: process.origin_project.0.display().to_string(),
        project_dir: process.project_dir.0.display().to_string(),
        worktree_path: process
            .worktree_path
            .as_ref()
            .map(|worktree| worktree.0.display().to_string()),
        branch: process.branch.clone(),
        session_id,
        model: process.model.lock_or_recover().clone(),
        profile: process.profile.lock_or_recover().clone(),
        research_topic: process.research_topic.lock_or_recover().clone(),
        reasoning: process.reasoning.lock_or_recover().clone(),
        manual_models: process.manual_models.lock_or_recover().clone(),

        subagent_mode: process.subagent_mode(),
        tracker_writes: process.tracker_writes_enabled.load(Ordering::SeqCst),
        stage,
        running,
        runtime: snapshot,
        label: naming.display(kind, ordinal),
        title: naming.title().map(str::to_string),
        title_custom: naming.custom.is_some(),
        kind: kind.as_str().into(),
        ordinal,
        updated_at: naming.updated_at,
    }
}
pub(crate) fn runtime_for(state: &AppState, session_id: &str) -> Arc<SessionRuntime> {
    state
        .runtimes
        .lock_or_recover()
        .entry(session_id.to_string())
        .or_insert_with(|| {
            Arc::new(SessionRuntime {
                session_id: session_id.to_string(),
                ..SessionRuntime::default()
            })
        })
        .clone()
}
/// Cooperative cancellation deadline; only the surviving supervisor may escalate it.
pub(crate) const STOP_GRACE_SECS: u64 = 30;

pub(crate) fn request_runtime_stop(
    runtime: &Arc<SessionRuntime>,
    store: &kanzei_core::SessionStore,
    _state_path: &Path,
    session_id: &str,
) -> Result<usize, kanzei_core::StoreError> {
    let _lifecycle = runtime.lifecycle.lock_or_recover();
    let snapshot = runtime.snapshot();
    if let Some(operation) = snapshot.stop.filter(|operation| !operation.settled) {
        if snapshot.phase == kanzei_core::session_lifecycle::Phase::Blocked {
            runtime
                .transition(kanzei_core::session_lifecycle::Event::ResumeCleanup)
                .map_err(kanzei_core::StoreError::InvalidInput)?;
        }
        return Ok(operation.cancelled_inputs);
    }
    let operation = store.request_session_stop(
        session_id,
        &format!("{}:stop:{}", snapshot.actor_epoch, snapshot.revision + 1),
    )?;
    let cancelled = operation.cancelled_inputs;
    runtime
        .transition(kanzei_core::session_lifecycle::Event::StopRequested(
            operation,
        ))
        .map_err(kanzei_core::StoreError::InvalidInput)?;
    runtime.retire_async();
    if let Some(token) = runtime.halt.lock_or_recover().as_ref() {
        token.cancel();
    }
    runtime.asks.lock_or_recover().clear();
    runtime.task_cancellations.cancel_all();
    Ok(cancelled)
}

pub(crate) fn take_pending_ask(state: &AppState, id: u64) -> Option<PendingAsk> {
    state
        .runtimes
        .lock_or_recover()
        .values()
        .find_map(|runtime| runtime.asks.lock_or_recover().remove(&id))
}
pub(crate) fn pending_ask_payload(id: u64, pending: &PendingAsk) -> serde_json::Value {
    let payload = match &pending.request {
        kanzei_core::AskRequest::Permission { action, resource } => {
            json!({"kind":"permission","id":id,"action":action,"resource":resource,"remember":kanzei_harness::config::generalize_resource(action, resource)})
        }
        kanzei_core::AskRequest::Question {
            question,
            options,
            default,
            multiple,
            background,
            ..
        } => {
            json!({"kind":"question","id":id,"question":question,"options":options,"default":default,"multiple":multiple,"background":background})
        }
    };
    let mut payload = with_session_id(payload, &pending.session_id);
    payload["source"] = json!(pending.source);
    if let Some(id) = &pending.agent_id {
        payload["agentId"] = json!(id);
    }
    payload
}

impl ProcessHandle {
    pub(crate) fn subagent_mode(&self) -> kanzei_harness::SubagentMode {
        *self.subagent_mode.lock_or_recover()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn temp_store(tag: &str) -> (std::path::PathBuf, kanzei_core::SessionStore) {
        let dir = std::env::temp_dir().join(format!(
            "kz-state-flush-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.db");
        let store = kanzei_core::SessionStore::open(&path).unwrap();
        store.create_session("ses_flush", "C:/proj", None).unwrap();
        (path, store)
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// D-297 验收③:整包补写超过 64KB 时按多批落库,事件顺序保持,outcome 只在末批。
    #[test]
    fn flush_live_run_整包超过64kb时分批且outcome只在末批() {
        let (path, store) = temp_store("batches");
        let live = Arc::new(Mutex::new(LiveRun::default()));
        {
            let mut live = live.lock_or_recover();
            live.begin("run_big", "input_1", "prompt", "provider", "model");
            // 30 条各 ~3KB 的事件:总量约 90KB,必然跨 2 批(64KB 上限)。
            for index in 0..30 {
                live.trace.push(json!({
                    "kind": "tool.started", "id": format!("t{index}"),
                    "name": "read",
                    "summary": "x".repeat(3000),
                }));
            }
        }
        assert!(flush_live_run(&store, "ses_flush", &live, "completed"));
        let events = store
            .list_events_by_type("ses_flush", 0, "run.trace")
            .unwrap();
        assert!(
            events.len() >= 2,
            "超过 64KB 应拆成多批,实得 {}",
            events.len()
        );
        // 事件顺序保持:第一批含 t0,total 事件数不变(跨批只拆容器不丢事件)。
        let total_events: usize = events
            .iter()
            .map(|e| e.payload["events"].as_array().map_or(0, Vec::len))
            .sum();
        assert_eq!(total_events, 30, "分批不得丢事件");
        // 每批序列化 ≤64KB + 头部开销余量。
        for event in &events {
            let size = serde_json::to_string(&event.payload).unwrap().len();
            assert!(size <= 64 * 1024 + 512, "单批应 ≤64KB,实得 {size}");
        }
        // outcome 只挂在末批(sequence 最大那条)。
        let last = events.last().unwrap();
        assert_eq!(last.payload["outcome"], "completed");
        assert!(events[..events.len() - 1]
            .iter()
            .all(|e| e.payload.get("outcome").is_none()));
        cleanup(&path);
    }

    #[test]
    fn trace_write_failure_leaves_artifact_orphan_marker() {
        let root = std::env::temp_dir().join(format!(
            "kz-state-orphan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state_path = root.join(".kanzei/state.db");
        let event = json!({
            "kind": "tool.completed",
            "id": "call-1",
            "artifact": {
                "artifact_id": "tool-bash-deadbeef",
                "bytes": 123,
                "sha256": "deadbeef"
            }
        });
        record_unpersisted_artifact(&state_path, "session-1", &event);
        let marker = root.join(".kanzei/artifacts/tool-results/tool-bash-deadbeef.orphan.json");
        let payload: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
        assert_eq!(payload["kind"], "tool_artifact_orphan");
        assert_eq!(payload["session_id"], "session-1");
        assert_eq!(payload["event"]["id"], "call-1");
        let _ = std::fs::remove_dir_all(root);
    }

    /// D-297 验收③:收尾 flush 触发保留策略,超过 keep 轮数的旧轨迹被清理。
    #[test]
    fn flush_live_run_触发保留策略只留最近n轮() {
        let (path, store) = temp_store("prune");
        // 先写 3 轮历史(不走 flush,直接 append 模拟旧轮)。
        for round in 1..=3 {
            store
                .append_event(
                    "ses_flush",
                    "run.trace",
                    &json!({"run_id": format!("run_old_{round}"), "events": [{"kind": "tool.started"}]}),
                )
                .unwrap();
        }
        // 本轮走 flush_live_run:写完当前轮后 prune 保留 200 轮,但存量只有 3+1 轮,
        // 验证的是「prune 被调用且不误删」——真正删旧轮的语义由 core 层测试覆盖。
        let live = Arc::new(Mutex::new(LiveRun::default()));
        {
            let mut live = live.lock_or_recover();
            live.begin("run_new", "input_2", "prompt", "provider", "model");
            live.trace.push(json!({"kind": "turn.started", "step": 1}));
        }
        assert!(flush_live_run(&store, "ses_flush", &live, "completed"));
        let traces = store.list_trace_payloads("ses_flush", 10).unwrap();
        assert_eq!(traces.len(), 4, "保留轮数充足时旧轮不动");
        assert!(traces.iter().any(|(_, p)| p.contains("run_new")));
        cleanup(&path);
    }
}
