//! AppState、运行时状态、UI 探针与跨域状态辅助。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{oneshot, Mutex as AsyncMutex};

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
                _ => anyhow::bail!("不支持的附件类型: {}", attachment.media_type),
            };
            Ok(part)
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PromptAttachment {
    pub(crate) file_name: String,
    pub(crate) media_type: String,
    pub(crate) data: String,
}

pub(crate) struct PendingAsk {
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

pub(crate) async fn ui_probe(kind: &str, arg: &str) -> Result<serde_json::Value, String> {
    let Some(emit) = UI_PROBE_EMIT.get() else {
        return Err("UI 探针不可用:桌面窗口未就绪(CLI 环境下没有可自查的界面)".into());
    };
    let id = UI_PROBE_SEQ.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel();
    UI_PROBES.lock().unwrap().insert(id, tx);
    emit(json!({"id": id, "kind": kind, "arg": arg}));
    match tokio::time::timeout(std::time::Duration::from_secs(8), rx).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => {
            UI_PROBES.lock().unwrap().remove(&id);
            Err("UI 探针通道已关闭".into())
        }
        Err(_) => {
            UI_PROBES.lock().unwrap().remove(&id);
            Err("UI 探针超时(8s):窗口可能正忙或未加载完成".into())
        }
    }
}

#[tauri::command]
pub fn ui_probe_result(id: u64, result: serde_json::Value) {
    if let Some(sender) = UI_PROBES.lock().unwrap().remove(&id) {
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
    pub(crate) asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    pub(crate) current_run: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    pub(crate) running: Arc<AtomicBool>,
    pub(crate) lifecycle: Arc<Mutex<()>>,
    pub(crate) conversation: Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    pub(crate) live: Arc<Mutex<LiveRun>>,
    /// 最近一次真实 `kz:status` 阶段,供跨线并列视图读取。
    pub(crate) stage: Arc<Mutex<String>>,
    /// R-174:本会话运行中的子代理取消注册表。`stop_task` 命令按 id 命中即取消,
    /// run_task 构造 SubagentRuntime 时把它塞进 `cancellations` 字段,单一实例共享。
    pub(crate) task_cancellations: Arc<kanzei_core::TaskCancellations>,
}

#[derive(Default)]
pub(crate) struct LiveRun {
    pub(crate) run_id: String,
    pub(crate) input_id: String,
    pub(crate) prompt_head: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) started_at: Option<std::time::Instant>,
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
    let mut live = live.lock().unwrap();
    if live.flushed || live.started_at.is_none() {
        return false;
    }
    live.flushed = true;
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
    let mut live = live.lock().unwrap();
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
) {
    let (run_id, index) = {
        let mut live = live.lock().unwrap();
        if live.started_at.is_none() {
            return;
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
        let mut live = live.lock().unwrap();
        if live.persisted_trace_events == index {
            live.persisted_trace_events += 1;
        }
    }
}

/// 事件回调需要 `Send + Sync`，不能捕获 rusqlite 连接；按状态库路径短开连接，
/// 让实时轨迹写入与模型事件回调保持解耦。
pub(crate) fn record_live_trace_at_path(
    state_path: &std::path::Path,
    session_id: &str,
    live: &Arc<Mutex<LiveRun>>,
    event: serde_json::Value,
) {
    if let Ok(store) = kanzei_core::SessionStore::open(state_path) {
        record_live_trace(&store, session_id, live, event);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessHandle {
    pub(crate) id: String,
    pub(crate) origin_project: String,
    pub(crate) project_dir: String,
    pub(crate) worktree_path: Option<String>,
    /// git 工作树对应的真实分支名。默认线为 None;分支线由 git 真源恢复。
    pub(crate) branch: Option<String>,
    pub(crate) model: Arc<Mutex<Option<String>>>,
    pub(crate) profile: Arc<Mutex<Option<String>>>,
    pub(crate) reasoning: Arc<Mutex<Option<String>>>,
    /// 项目级手填模型候选(R-178 批3)。project 级数据挂默认进程行承载;
    /// 线进程该字段为 None 语义 = 「跟随项目默认进程的候选列表」。
    /// 前端下拉的「手填」候选从 process_info 回读,不再以 localStorage 为真源。
    pub(crate) manual_models: Arc<Mutex<Vec<String>>>,
    /// 界面上的「勘察复核」开关(2026-08-11 用户定调)。
    ///
    /// 它是**阶段流水线的总闸**:开 = 本进程每个任务都强制走
    /// 勘察 → 汇总屏障 → 实现 → 复核屏障 → 复核 →(有发现时)修正;
    /// 关 = 一问一答,与引入七阶段之前逐字节相同。
    ///
    /// 它**不**控制子代理的有无——关着的时候模型照样能自己派 `task`
    /// (`run.rs` 的 `subagent_rt` 无条件构造)。这个字段以前叫 `subagent_enabled`
    /// 且默认开,名不副实:关掉它连子代理运行时都没有,开着它也只是把 `task`
    /// 摆上桌、派不派全看模型,给不了「每个任务都勘察」的保证。
    pub(crate) phase_pipeline_enabled: Arc<AtomicBool>,
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
    pub(crate) reasoning: Option<String>,
    pub(crate) manual_models: Vec<String>,
    /// 见 [`ProcessHandle::phase_pipeline_enabled`]。前端 `process_list` 回显用。
    pub(crate) phase_pipeline: bool,
    pub(crate) tracker_writes: bool,
    /// 主代理拥有写入、比对、合并与发版职责;并行线/子代理只在自己的边界内工作。
    pub(crate) authority: String,
    /// 当前会话阶段,用于侧栏逐条投影并行任务状态。
    pub(crate) stage: String,
    pub(crate) running: bool,
    pub(crate) label: String,
}
pub(crate) struct MobileService {
    pub(crate) active: Arc<AtomicBool>,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct MobileServiceInfo {
    pub(crate) address: String,
    pub(crate) token: String,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct WorktreeInfo {
    pub(crate) path: String,
    pub(crate) branch: String,
    pub(crate) files: Vec<String>,
    pub(crate) clean: bool,
    pub(crate) diff: String,
    /// 占着这棵树的线 id(R-177 内容③:清单来自 git,绑定关系来自进程表)。
    /// None = git 认得这棵树但没有线绑着它(手工建的,或者线已经关了)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bound_process: Option<String>,
}

impl Default for SessionRuntime {
    fn default() -> Self {
        Self {
            asks: Arc::new(Mutex::new(HashMap::new())),
            current_run: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(Mutex::new(())),
            conversation: Arc::new(Mutex::new(HashMap::new())),
            live: Arc::new(Mutex::new(LiveRun::default())),
            stage: Arc::new(Mutex::new("空闲".into())),
            task_cancellations: Arc::new(kanzei_core::TaskCancellations::default()),
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
    /// 「并行查、串行写」的强制点——主对话 writer run 在这里获取写租约。
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
            ask_seq: Arc::new(AtomicU64::new(0)),
            processes: Arc::new(Mutex::new(HashMap::new())),
            restored_projects: Arc::new(Mutex::new(HashSet::new())),
            mobile_service: Arc::new(Mutex::new(None)),
            auto_runs: Arc::new(Mutex::new(HashMap::new())),
            coordinator: Arc::new(kanzei_core::orchestration::MemoryCoordinator::new()),
            worktree_ops: Arc::new(AsyncMutex::new(())),
        }
    }
}

pub(crate) fn normalized_project_root(path: &Path) -> PathBuf {
    let root =
        kanzei_harness::config::discover_project_root(path).unwrap_or_else(|| path.to_path_buf());
    std::fs::canonicalize(&root).unwrap_or(root)
}
pub(crate) fn default_process_id(root: &Path) -> String {
    format!("d|{}", root.display())
}
pub(crate) fn process_session_id(root: &Path, process_id: Option<&str>) -> String {
    let base = kanzei_core::project_session_id(root);
    let default_id = default_process_id(root);
    match process_id.filter(|id| !id.is_empty() && *id != default_id) {
        Some(id) => {
            let prefix = id.split_once('|').map(|(prefix, _)| prefix).unwrap_or(id);
            format!("{base}#{prefix}")
        }
        None => base,
    }
}
pub(crate) fn ensure_default_process(state: &AppState, root: &Path) -> ProcessHandle {
    let id = default_process_id(root);
    let mut processes = state.processes.lock().unwrap();
    processes
        .entry(id.clone())
        .or_insert_with(|| ProcessHandle {
            id: id.clone(),
            origin_project: root.display().to_string(),
            project_dir: root.display().to_string(),
            worktree_path: None,
            branch: None,
            model: Arc::new(Mutex::new(None)),
            profile: Arc::new(Mutex::new(None)),
            reasoning: Arc::new(Mutex::new(None)),
            manual_models: Arc::new(Mutex::new(Vec::new())),
            // 默认关:用户要的是「显式打开才强制走七阶段」,默认开就不叫显式
            // (2026-08-11 用户定调)。
            phase_pipeline_enabled: Arc::new(AtomicBool::new(false)),
            tracker_writes_enabled: Arc::new(AtomicBool::new(false)),
        })
        .clone()
}
pub(crate) fn process_info(state: &AppState, process: &ProcessHandle) -> ProcessInfo {
    let root = PathBuf::from(&process.project_dir);
    let session_id = process_session_id(&root, Some(&process.id));
    let running = state
        .runtimes
        .lock()
        .unwrap()
        .get(&session_id)
        .is_some_and(|runtime| runtime.running.load(Ordering::SeqCst));
    let stage = state
        .runtimes
        .lock()
        .unwrap()
        .get(&session_id)
        .map(|runtime| runtime.stage.lock().unwrap().clone())
        .unwrap_or_else(|| "空闲".into());
    ProcessInfo {
        id: process.id.clone(),
        origin_project: process.origin_project.clone(),
        project_dir: process.project_dir.clone(),
        worktree_path: process.worktree_path.clone(),
        branch: process.branch.clone(),
        session_id,
        model: process.model.lock().unwrap().clone(),
        profile: process.profile.lock().unwrap().clone(),
        reasoning: process.reasoning.lock().unwrap().clone(),
        manual_models: process.manual_models.lock().unwrap().clone(),
        phase_pipeline: process.phase_pipeline_enabled.load(Ordering::SeqCst),
        tracker_writes: process.tracker_writes_enabled.load(Ordering::SeqCst),
        authority: if process.id.starts_with("d|") {
            "primary".into()
        } else {
            "parallel".into()
        },
        stage,
        running,
        label: if process.id.starts_with("d|") {
            "默认".into()
        } else {
            process.id.split('|').next().unwrap_or("进程").into()
        },
    }
}
pub(crate) fn runtime_for(state: &AppState, session_id: &str) -> Arc<SessionRuntime> {
    state
        .runtimes
        .lock()
        .unwrap()
        .entry(session_id.to_string())
        .or_insert_with(|| Arc::new(SessionRuntime::default()))
        .clone()
}
pub(crate) fn stop_runtime_and_finalize(
    runtime: &SessionRuntime,
    store: &kanzei_core::SessionStore,
    session_id: &str,
) -> Result<usize, kanzei_core::StoreError> {
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    flush_live_run(store, session_id, &runtime.live, "halted");
    if let Some(handle) = runtime.current_run.lock().unwrap().take() {
        handle.abort();
    }
    runtime.asks.lock().unwrap().clear();
    runtime.running.store(false, Ordering::SeqCst);
    *runtime.stage.lock().unwrap() = "空闲".into();
    store.finalize_interrupt(session_id)
}
pub(crate) fn take_pending_ask(state: &AppState, id: u64) -> Option<PendingAsk> {
    state
        .runtimes
        .lock()
        .unwrap()
        .values()
        .find_map(|runtime| runtime.asks.lock().unwrap().remove(&id))
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
        } => {
            json!({"kind":"question","id":id,"question":question,"options":options,"default":default,"multiple":multiple})
        }
    };
    with_session_id(payload, &pending.session_id)
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
            let mut live = live.lock().unwrap();
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
            let mut live = live.lock().unwrap();
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
