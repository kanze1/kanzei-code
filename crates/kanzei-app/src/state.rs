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
    /// D-342 协作式停止:当前 run 的停止令牌,每次 run_task 开始时换新。
    /// stop 取走并 cancel → run 在安全检查点以 halted **正常收尾**(轮末写回对话),
    /// None = 无活跃 run(此时停止走立即终态化的旧路径)。
    pub(crate) halt: Arc<Mutex<Option<kanzei_core::CancellationToken>>>,
    /// D-342:run 代数。兜底硬杀只对「停止时那一代」生效——宽限期内用户又发了
    /// 新任务时,代数已换,不得误杀新 run。
    pub(crate) run_generation: Arc<AtomicU64>,
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
) -> bool {
    let (run_id, index) = {
        let mut live = live.lock().unwrap();
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
        let mut live = live.lock().unwrap();
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
    /// 进程级「子代理」开关。默认开启；关闭时 task 工具不注册到本轮工具面。
    pub(crate) subagents_enabled: Arc<AtomicBool>,
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
    /// 见 [`ProcessHandle::subagents_enabled`]。前端 `process_list` 回显用。
    pub(crate) subagents_enabled: bool,
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
            asks: Arc::new(Mutex::new(HashMap::new())),
            current_run: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(Mutex::new(())),
            conversation: Arc::new(Mutex::new(HashMap::new())),
            live: Arc::new(Mutex::new(LiveRun::default())),
            stage: Arc::new(Mutex::new("空闲".into())),
            task_cancellations: Arc::new(kanzei_core::TaskCancellations::default()),
            halt: Arc::new(Mutex::new(None)),
            run_generation: Arc::new(AtomicU64::new(0)),
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
            origin_project: ProjectRoot(root.to_path_buf()),
            project_dir: ProjectRoot(root.to_path_buf()),
            worktree_path: None,
            branch: None,
            model: Arc::new(Mutex::new(None)),
            profile: Arc::new(Mutex::new(None)),
            reasoning: Arc::new(Mutex::new(None)),
            manual_models: Arc::new(Mutex::new(Vec::new())),
            // 默认关:用户要的是「显式打开才强制走七阶段」,默认开就不叫显式
            // (2026-08-11 用户定调)。
            phase_pipeline_enabled: Arc::new(AtomicBool::new(false)),
            subagents_enabled: Arc::new(AtomicBool::new(true)),
            tracker_writes_enabled: Arc::new(AtomicBool::new(false)),
        })
        .clone()
}
pub(crate) fn process_info(state: &AppState, process: &ProcessHandle) -> ProcessInfo {
    // D-367:project_dir 是 ProjectRoot(恒主根),直接取路径算 session_id。
    let root = &process.project_dir.0;
    let session_id = process_session_id(root, Some(&process.id));
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
        // ProcessInfo 是 IPC 输出类型,保持 String(前端契约不变)。
        origin_project: process.origin_project.0.display().to_string(),
        project_dir: process.project_dir.0.display().to_string(),
        worktree_path: process
            .worktree_path
            .as_ref()
            .map(|worktree| worktree.0.display().to_string()),
        branch: process.branch.clone(),
        session_id,
        model: process.model.lock().unwrap().clone(),
        profile: process.profile.lock().unwrap().clone(),
        reasoning: process.reasoning.lock().unwrap().clone(),
        manual_models: process.manual_models.lock().unwrap().clone(),
        phase_pipeline: process.phase_pipeline_enabled.load(Ordering::SeqCst),
        subagents_enabled: process.subagents_enabled.load(Ordering::SeqCst),
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
/// D-342:兜底硬杀前的宽限期。协作式停止下 run 会在最近的检查点(步首/流内/
/// 工具间)自行收尾并走轮末写回;只有工具挂死等异常才等到这个上限。
pub(crate) const STOP_GRACE_SECS: u64 = 30;

/// D-342 协作式停止(用户点「停止」走这里):置位停止令牌让 run 自行收尾,
/// **不立即 abort**——abort 到不了轮末写回,被打断轮的对话会整轮消失,这正是
/// D-342 的根因。队列清理(finalize_interrupt)仍然立即执行;兜底硬杀在宽限期
/// 后按代数比对触发,不误杀停止后新开的 run。
/// 关线/注销等终点动作请用 [`halt_runtime_immediately`](自 stop_runtime_and_finalize
/// 拆出的旧路径)——那里随后就删工作树,不能让 run 多活 30 秒。
pub(crate) fn stop_runtime_and_finalize(
    runtime: &Arc<SessionRuntime>,
    store: &kanzei_core::SessionStore,
    state_path: &Path,
    session_id: &str,
) -> Result<usize, kanzei_core::StoreError> {
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    let halt_token = runtime.halt.lock().unwrap().take();
    match halt_token {
        Some(token) if runtime.running.load(Ordering::SeqCst) => {
            token.cancel();
            // 挂在权限/问题弹窗上的 run:清 pending ask,sender drop → Cancelled →
            // 既有 UserDeclined 路径立刻 halted 收尾,不用等检查点。
            runtime.asks.lock().unwrap().clear();
            let generation = runtime.run_generation.load(Ordering::SeqCst);
            let runtime_bg = Arc::clone(runtime);
            let state_path = state_path.to_path_buf();
            let session_bg = session_id.to_string();
            // 兜底走独立线程(纯同步动作):不依赖 tauri/tokio 运行时,单测里也能构造。
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(STOP_GRACE_SECS));
                let _lifecycle = runtime_bg.lifecycle.lock().unwrap();
                if stale_run_needs_abort(
                    runtime_bg.run_generation.load(Ordering::SeqCst),
                    generation,
                    runtime_bg.running.load(Ordering::SeqCst),
                ) {
                    // 兜底:run 没能在宽限期内自行收尾(工具挂死等),回到硬杀,
                    // 轨迹/episode 先落库再 abort(与旧路径同序)。
                    if let Ok(store) = kanzei_core::SessionStore::open(&state_path) {
                        flush_live_run(&store, &session_bg, &runtime_bg.live, "halted");
                    }
                    if let Some(handle) = runtime_bg.current_run.lock().unwrap().take() {
                        handle.abort();
                    }
                    runtime_bg.running.store(false, Ordering::SeqCst);
                    *runtime_bg.stage.lock().unwrap() = "空闲".into();
                }
            });
            store.finalize_interrupt(session_id)
        }
        // 无活跃 run(或令牌已被上一轮收走):立即终态化,与旧行为一致。
        _ => {
            flush_live_run(store, session_id, &runtime.live, "halted");
            if let Some(handle) = runtime.current_run.lock().unwrap().take() {
                handle.abort();
            }
            runtime.asks.lock().unwrap().clear();
            runtime.running.store(false, Ordering::SeqCst);
            *runtime.stage.lock().unwrap() = "空闲".into();
            store.finalize_interrupt(session_id)
        }
    }
}

/// D-342:兜底硬杀的判定,抽成纯函数锁语义——只有「代数未换 && 仍在运行」才硬杀;
/// 宽限期内新开的 run(代数已 +1)不受停止兜底波及。
pub(crate) fn stale_run_needs_abort(
    current_generation: u64,
    stop_generation: u64,
    running: bool,
) -> bool {
    running && current_generation == stop_generation
}

/// 关线/注销的立即终态化(旧 stop_runtime_and_finalize 原样保留):调用方随后要
/// 删工作树/退役身份,不能给 run 宽限期。
pub(crate) fn halt_runtime_immediately(
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
