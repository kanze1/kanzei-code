//! AppState、运行时状态、UI 探针与跨域状态辅助。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::oneshot;

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
    if !live.trace.is_empty() {
        let _ = store.append_event(
            session_id,
            "run.trace",
            &json!({"events": live.trace, "outcome": outcome}),
        );
    }
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

#[derive(Debug, Clone)]
pub(crate) struct ProcessHandle {
    pub(crate) id: String,
    pub(crate) origin_project: String,
    pub(crate) project_dir: String,
    pub(crate) worktree_path: Option<String>,
    pub(crate) model: Arc<Mutex<Option<String>>>,
    pub(crate) profile: Arc<Mutex<Option<String>>>,
    pub(crate) reasoning: Arc<Mutex<Option<String>>>,
    pub(crate) subagent_enabled: Arc<AtomicBool>,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProcessInfo {
    pub(crate) id: String,
    pub(crate) origin_project: String,
    pub(crate) project_dir: String,
    pub(crate) worktree_path: Option<String>,
    pub(crate) session_id: String,
    pub(crate) model: Option<String>,
    pub(crate) profile: Option<String>,
    pub(crate) reasoning: Option<String>,
    pub(crate) subagent: bool,
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
    pub(crate) mobile_service: Arc<Mutex<Option<MobileService>>>,
    /// 自主推进(鞭挞)状态按会话隔离：控件输入经 auto_state_update 同步，
    /// 轮末由 run.rs 只消费所属会话的控制器，不能串扰后台进程。
    pub(crate) auto_runs: Arc<Mutex<HashMap<String, crate::auto_run::AutoRunController>>>,
    /// R-171 项目级执行协调器:所有 ProcessHandle 共享同一实例,按规范化主根分桶。
    /// 「并行查、串行写」的强制点——主对话 writer run 在这里获取写租约。
    pub(crate) coordinator: Arc<kanzei_core::orchestration::MemoryCoordinator>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            runtimes: Arc::new(Mutex::new(HashMap::new())),
            ask_seq: Arc::new(AtomicU64::new(0)),
            processes: Arc::new(Mutex::new(HashMap::new())),
            mobile_service: Arc::new(Mutex::new(None)),
            auto_runs: Arc::new(Mutex::new(HashMap::new())),
            coordinator: Arc::new(kanzei_core::orchestration::MemoryCoordinator::new()),
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
            model: Arc::new(Mutex::new(None)),
            profile: Arc::new(Mutex::new(None)),
            reasoning: Arc::new(Mutex::new(None)),
            subagent_enabled: Arc::new(AtomicBool::new(true)),
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
    ProcessInfo {
        id: process.id.clone(),
        origin_project: process.origin_project.clone(),
        project_dir: process.project_dir.clone(),
        worktree_path: process.worktree_path.clone(),
        session_id,
        model: process.model.lock().unwrap().clone(),
        profile: process.profile.lock().unwrap().clone(),
        reasoning: process.reasoning.lock().unwrap().clone(),
        subagent: process.subagent_enabled.load(Ordering::SeqCst),
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
        } => {
            json!({"kind":"question","id":id,"question":question,"options":options,"default":default})
        }
    };
    with_session_id(payload, &pending.session_id)
}
