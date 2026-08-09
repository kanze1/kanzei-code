//! kzapp — kanzei Tauri 桌面端。
//! 前端为静态页面(ui/),经 command + event 通信:
//! run_prompt → kz:* 流式事件;kz:ask 权限弹窗 → answer_ask;stop_run 中止;
//! projects_* 多项目管理(~/.kanzei/app.json);settings_* 全局配置表单。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{Emitter, State, Window};
use tokio::sync::oneshot;

use kanzei_core::{run_once_with_parts, AskFuture, RunEvent, RunnerConfig};

mod files_view;
use kanzei_harness::{
    ConfigComponent, Harness, KanzeiConfig, MarkdownComponent, ProfileKind, ResolveCtx, ToolCtx,
};
use kanzei_llm::{LlmClient, ProxyConfig};
use kanzei_tools::docstore::{DocStore, DEFECTS, FINDINGS, GOALS, REQUIREMENTS, SOURCES};
use kanzei_tools::tracker::schedule_for_display;
use kanzei_tools::{BaseComponent, DevProfile, ResearchProfile};

/// 运行轨迹的时间戳(Unix 毫秒)。
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[derive(Debug, Clone, Deserialize)]
struct PromptAttachment {
    file_name: String,
    media_type: String,
    data: String,
}

fn prompt_attachment_parts(
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
/// 悬挂中的权限询问:除通道外携带上下文,支持"总是允许"落盘。
struct PendingAsk {
    sender: oneshot::Sender<kanzei_core::AskResponse>,
    request: kanzei_core::AskRequest,
    action: String,
    resource: String,
    project_root: PathBuf,
    session_id: String,
}

/// R-126:UI 自查桥。工具在后端发起请求 → 前端在**真实运行中的窗口**里取样 →
/// 结果经 oneshot 回到工具。不另起无头浏览器:那样看到的是空白页,和用户眼前的
/// 界面没有关系,查不出 D-164/D-165 那类"语法全对但渲染成一团"的问题。
static UI_PROBES: std::sync::LazyLock<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static UI_PROBE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// 发起 UI 探针的通道:由 setup 时装入,工具侧只认这个函数。
static UI_PROBE_EMIT: std::sync::OnceLock<Box<dyn Fn(serde_json::Value) + Send + Sync>> =
    std::sync::OnceLock::new();

async fn ui_probe(kind: &str, arg: &str) -> Result<serde_json::Value, String> {
    let Some(emit) = UI_PROBE_EMIT.get() else {
        return Err("UI 探针不可用:桌面窗口未就绪(CLI 环境下没有可自查的界面)".into());
    };
    let id = UI_PROBE_SEQ.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel();
    UI_PROBES.lock().unwrap().insert(id, tx);
    emit(json!({ "id": id, "kind": kind, "arg": arg }));
    // 前端可能正忙或没挂监听;超时要明确报出来,不能让工具悬着。
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

/// 前端把取样结果送回来。
#[tauri::command]
fn ui_probe_result(id: u64, result: serde_json::Value) {
    if let Some(sender) = UI_PROBES.lock().unwrap().remove(&id) {
        let _ = sender.send(result);
    }
}

fn with_session_id(mut payload: serde_json::Value, session_id: &str) -> serde_json::Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert("sessionId".into(), serde_json::Value::String(session_id.into()));
    }
    payload
}

struct SessionRuntime {
    asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    current_run: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    running: Arc<AtomicBool>,
    lifecycle: Arc<Mutex<()>>,
    conversation: Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    /// 在飞轮次的实时画像(D-179)。必须挂在 runtime 上而不是 run_task 的局部,
    /// 否则停止路径 `handle.abort()` 之后再也够不着它。
    live: Arc<Mutex<LiveRun>>,
}

/// 一轮跑到当前时刻为止已知的事实。停止时靠它把轨迹与 episode 补落库——
/// 原先这些全写在被 abort 的那个 task 里,于是**最值得复盘的运行(长到不得不停)
/// 反而一个字节都不留**:实测一次 41 分钟的运行只剩一条 stopped_by_user(D-179)。
#[derive(Default)]
struct LiveRun {
    run_id: String,
    input_id: String,
    prompt_head: String,
    provider: String,
    model: String,
    started_at: Option<std::time::Instant>,
    steps: u32,
    input_tokens: u64,
    output_tokens: u64,
    trace: Vec<serde_json::Value>,
    /// 轨迹与 episode 已经落过库。正常收尾与停止路径都会写,用它防重。
    flushed: bool,
}

impl LiveRun {
    fn begin(
        &mut self,
        run_id: &str,
        input_id: &str,
        prompt_head: &str,
        provider: &str,
        model: &str,
    ) {
        *self = LiveRun {
            run_id: run_id.into(),
            input_id: input_id.into(),
            prompt_head: prompt_head.chars().take(200).collect(),
            provider: provider.into(),
            model: model.into(),
            started_at: Some(std::time::Instant::now()),
            ..LiveRun::default()
        };
    }

    fn duration_ms(&self) -> u64 {
        self.started_at
            .map(|at| at.elapsed().as_millis() as u64)
            .unwrap_or_default()
    }
}

/// 把在飞画像落库(轨迹 + episode)。正常收尾与停止路径共用,幂等。
/// 返回是否真的写了——调用方据此避免重复写。
fn flush_live_run(
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
            &json!({ "events": live.trace, "outcome": outcome }),
        );
    }
    let _ = store.append_episode(&kanzei_core::EpisodeRecord {
        session_id,
        prompt_head: &live.prompt_head,
        outcome,
        steps: live.steps,
        input_tokens: live.input_tokens,
        output_tokens: live.output_tokens,
        // 被中断的轮次没有完整消息历史可统计,工具画像留给轨迹里的
        // tool.completed 记录——那份是逐次记的,不依赖收尾。
        tools_json: "{}",
        context_json: "[]",
        metrics_json: "{}",
        // 被裁剪段的沉淀在 RunSummary 里,中断路径拿不到;轨迹里的
        // context.compacted 记录仍在,不会无声丢失。
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
struct ProcessHandle {
    id: String,
    origin_project: String,
    project_dir: String,
    worktree_path: Option<String>,
    model: Arc<Mutex<Option<String>>>,
    profile: Arc<Mutex<Option<String>>>,
    /// 思考强度的每进程覆盖;None = 用配置里的默认档。
    reasoning: Arc<Mutex<Option<String>>>,
    subagent_enabled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
struct ProcessInfo {
    id: String,
    origin_project: String,
    project_dir: String,
    worktree_path: Option<String>,
    session_id: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    subagent: bool,
    running: bool,
    label: String,
}

struct MobileService {
    active: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
struct MobileServiceInfo {
    address: String,
    token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentContainerManifest {
    agent_id: String,
    version: String,
    status: String,
    permissions: Vec<String>,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
struct WorktreeInfo {
    path: String,
    branch: String,
    files: Vec<String>,
    clean: bool,
    diff: String,
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

#[derive(Default)]
struct AppState {
    runtimes: Arc<Mutex<HashMap<String, Arc<SessionRuntime>>>>,
    ask_seq: Arc<AtomicU64>,
    processes: Arc<Mutex<HashMap<String, ProcessHandle>>>,
    mobile_service: Arc<Mutex<Option<MobileService>>>,
}

fn normalized_project_root(path: &Path) -> PathBuf {
    let root = kanzei_harness::config::discover_project_root(path)
        .unwrap_or_else(|| path.to_path_buf());
    std::fs::canonicalize(&root).unwrap_or(root)
}

fn default_process_id(root: &Path) -> String {
    format!("d|{}", root.display())
}

fn process_session_id(root: &Path, process_id: Option<&str>) -> String {
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

fn ensure_default_process(state: &AppState, root: &Path) -> ProcessHandle {
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

fn process_info(state: &AppState, process: &ProcessHandle) -> ProcessInfo {
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

fn runtime_for(state: &AppState, session_id: &str) -> Arc<SessionRuntime> {
    state
        .runtimes
        .lock()
        .unwrap()
        .entry(session_id.to_string())
        .or_insert_with(|| Arc::new(SessionRuntime::default()))
        .clone()
}

fn stop_runtime_and_finalize(
    runtime: &SessionRuntime,
    store: &kanzei_core::SessionStore,
    session_id: &str,
) -> Result<usize, kanzei_core::StoreError> {
    // 生命周期锁必须覆盖 abort、running=false 与数据库收尾，阻止 promote 在
    // 两者之间插入；finalize_interrupt 再原子取消 pending/promoted 输入。
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    // 先落库再 abort(D-179):收尾代码全在被 abort 的那个 task 里,先杀后写等于
    // 什么都不写。顺序反过来,长轮被停止时轨迹与 episode 才留得下来。
    flush_live_run(store, session_id, &runtime.live, "halted");
    if let Some(handle) = runtime.current_run.lock().unwrap().take() {
        handle.abort();
    }
    runtime.asks.lock().unwrap().clear();
    runtime.running.store(false, Ordering::SeqCst);
    store.finalize_interrupt(session_id)
}

fn take_pending_ask(state: &AppState, id: u64) -> Option<PendingAsk> {
    state
        .runtimes
        .lock()
        .unwrap()
        .values()
        .find_map(|runtime| runtime.asks.lock().unwrap().remove(&id))
}


fn pending_ask_payload(id: u64, pending: &PendingAsk) -> serde_json::Value {
    let payload = match &pending.request {
        kanzei_core::AskRequest::Permission { action, resource } => json!({
            "kind": "permission",
            "id": id,
            "action": action,
            "resource": resource,
            "remember": kanzei_harness::config::generalize_resource(action, resource),
        }),
        kanzei_core::AskRequest::Question { question, options, default } => json!({
            "kind": "question",
            "id": id,
            "question": question,
            "options": options,
            "default": default,
        }),
    };
    with_session_id(payload, &pending.session_id)
}

fn pending_path(exe: &Path) -> PathBuf {
    let name = exe.file_name().and_then(|n| n.to_str()).unwrap_or("kzapp.exe");
    exe.with_file_name(format!("{name}.pending"))
}

/// 启动早期处理 release.ps1 留下的 pending 文件。自身不能覆盖自身，
/// 因此派生同一个二进制作为 helper，旧进程退出后由 helper 完成替换并重启。
fn startup_update() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--kz-update-helper") {
        let exe = args.get(2).map(PathBuf::from);
        let pending = args.get(3).map(PathBuf::from);
        if let (Some(exe), Some(pending)) = (exe, pending) {
            apply_pending_update(&exe, &pending);
        }
        return true;
    }
    // 应用内更新的交接 helper:发起方 kzapp 派生它之后立刻退出,让出镜像文件句柄。
    if args.get(1).map(String::as_str) == Some("--kz-install-helper") {
        let installer = args.get(2).map(PathBuf::from);
        let exe = args.get(3).map(PathBuf::from);
        let parent = args.get(4).and_then(|p| p.parse::<u32>().ok());
        if let (Some(installer), Some(exe), Some(parent)) = (installer, exe, parent) {
            run_install_helper(&installer, &exe, parent);
        }
        return true;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    // 上次更新的备份因镜像锁删不掉,会残留一份 .previous:启动时清理。
    let _ = std::fs::remove_file(exe.with_extension("exe.previous"));
    // helper 副本自己删不掉自己(运行中锁着),由下一次启动回收(D-182)。
    let _ = std::fs::remove_file(update_helper_path());
    let pending = pending_path(&exe);
    if !pending.is_file() { return false; }
    match Command::new(&exe)
        .arg("--kz-update-helper")
        .arg(&exe)
        .arg(&pending)
        .spawn()
    {
        Ok(_) => true,
        Err(error) => {
            eprintln!("kzapp:无法启动自更新 helper: {error}");
            false
        }
    }
}

/// 安装器交接:%TEMP% 里的 kanzei-setup.exe。集中一处,清理与写入用的是同一个路径。
fn installer_path() -> PathBuf {
    std::env::temp_dir().join("kanzei-setup.exe")
}

/// 下载的安装器必须是像样的 Windows 可执行文件。代理返回的 HTML 错误页、被截断的
/// 响应都会被这里挡下——否则要等交接完、app 已经退出了才发现装不上(D-124)。
fn validate_installer(bytes: &[u8]) -> Result<(), String> {
    const MIN_BYTES: usize = 1 << 20;
    if bytes.len() < MIN_BYTES {
        return Err(format!(
            "安装包只有 {} KB,不完整(可能是代理返回了错误页)。检查网络或代理后重试。",
            bytes.len() / 1024
        ));
    }
    if !bytes.starts_with(b"MZ") {
        return Err("下载到的不是 Windows 可执行文件,已放弃安装。检查网络或代理后重试。".into());
    }
    Ok(())
}

/// 清理上一次失败留下的僵尸安装器与临时文件。不清的话 NSIS 会一直握着 kzapp.exe
/// 的句柄,后续每次重试都撞同一个 os error 32,关闭重开也救不回来(D-124)。
fn clear_stale_installer() -> Vec<String> {
    let mut notes = Vec::new();
    let killed = Command::new("taskkill")
        .args(["/F", "/IM", "kanzei-setup.exe"])
        .output()
        .ok()
        .is_some_and(|out| out.status.success());
    if killed {
        notes.push("已清理残留的安装器进程".to_string());
        // taskkill 返回后句柄释放还需要一小会,否则紧接着的 remove_file 仍会失败。
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    let path = installer_path();
    if path.exists() && std::fs::remove_file(&path).is_err() {
        notes.push("旧安装包仍被占用,将直接覆盖".to_string());
    }
    notes
}

/// helper 进程:等发起更新的 kzapp 退出 → 静默安装 → 拉起新版本 → 删安装包。
/// 必须由独立进程做,因为安装器要替换的正是调用方自己的镜像文件。
/// 等发起更新的进程退出。NSIS 在文件被占用时会挂在隐藏对话框而不是失败退出,
/// 所以宁可多等,也不能在父进程还活着时就把安装器放出去。
/// 返回是否等到了退出(false = 等到超时它还活着)。
/// 单独成函数是为了能直接测这条时序不变量——测整个 helper 就得真去执行一个安装器。
fn wait_for_parent_exit(parent_pid: u32, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !process_alive(parent_pid) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    !process_alive(parent_pid)
}

/// D-171:清掉锁着我们 WebView2 数据目录的孤儿 msedgewebview2 进程。
///
/// 父 kzapp 被强杀(更新交接、任务管理器、崩溃)时 WebView2 子进程会存活,
/// 继续握着 `dev.kanzei.app/EBWebView` 的目录锁——下一个实例的 WebView 初始化
/// 失败,窗口就是一块黑。实测本机曾积累 6 个存活 7 小时的孤儿。
///
/// 安全边界:只杀**命令行里带我们数据目录**的 webview,且只在**没有别的 kzapp
/// 实例活着**时动手——别的实例还在,它的 webview 就不是孤儿。
fn cleanup_orphan_webviews() {
    // pid 直接嵌进脚本文本:`-Command` 模式下尾随参数不会成为 $args,
    // 会被拼进命令串导致解析错误(实测踩过)。
    let script = format!(
        r#"
$mine = {}
$others = @(Get-Process kzapp -ErrorAction SilentlyContinue | Where-Object {{ $_.Id -ne $mine }})
if ($others.Count -gt 0) {{ exit 0 }}
Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
  Where-Object {{ $_.CommandLine -match 'dev\.kanzei\.app' }} |
  ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}
"#,
        std::process::id()
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd.output();
}

/// 更新交接用的 helper 副本。**必须**是安装目录之外的一份拷贝(D-182):
/// 原实现直接 `Command::new(current_exe())` 起 helper,而那就是安装器要替换的
/// `kzapp.exe`——父进程退出后 helper 仍在跑同一个镜像,Windows 一直锁着它,
/// NSIS 覆盖不了,更新就此静默失败。
fn update_helper_path() -> PathBuf {
    std::env::temp_dir().join("kanzei-update-helper.exe")
}

/// 更新交接日志的生产落点(固定,便于用户/诊断时捞取)。
fn update_log_path() -> PathBuf {
    std::env::temp_dir().join("kanzei-update.log")
}

/// 更新交接日志。GUI 进程没有可见的 stderr,原实现只 `eprintln!`,
/// 于是"检查更新没反应"永远查不出原因——这才是真正卡住诊断的地方(D-182)。
fn update_log(line: &str) {
    update_log_at(&update_log_path(), line);
}

/// D-188:日志路径显式传入,测试写独立临时文件,不再污染生产日志。
fn update_log_at(path: &Path, line: &str) {
    use std::io::Write as _;
    // 有界:超过 256 KiB 从头重来,日志本身不该变成垃圾。
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > 256 * 1024) {
        let _ = std::fs::remove_file(path);
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "[{stamp}] {line}");
    }
    eprintln!("kzapp:update {line}");
}

/// 镜像的身份指纹(修改时间 + 大小):用来判断安装器到底换没换文件。
fn image_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// 安装器跑完之后,镜像到底换没换(D-199)。
///
/// 原实现只把安装**后**的 mtime 打进日志,从来没跟安装前的比过——于是
/// "安装器 exit=0 但一个字节没换"照样被记成"已拉起新版本"。这恰恰是调用处
/// 注释担心的那种情形(NSIS 在目标被占用时也可能报 exit=0 而什么都没换),
/// 护栏写了一半等于没写。实测 2026-08-09:两次「检查更新」都 exit=0、都写了
/// "已拉起新版本",而两次记录的 mtime 是同一个值(134306860100000000,
/// 即上一版的构建时间)——文件从未被替换,用户版本卡在旧版查不出原因。
///
/// 任一侧读不到就判为"没换":宁可多报一次可疑,也不要把静默失败说成成功。
fn image_replaced(before: Option<(SystemTime, u64)>, after: Option<(SystemTime, u64)>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => before != after,
        _ => false,
    }
}

fn run_install_helper(installer: &Path, exe: &Path, parent_pid: u32) {
    update_log(&format!(
        "helper 启动 installer={} exe={} parent={parent_pid}",
        installer.display(),
        exe.display()
    ));
    // 安装前留指纹:装完拿它比对,这是"到底换没换"的唯一判据。
    let before = image_stamp(exe);
    let exited = wait_for_parent_exit(parent_pid, std::time::Duration::from_secs(30));
    update_log(&format!("父进程退出={exited}"));
    // 句柄释放晚于进程退出,再让一手。
    std::thread::sleep(std::time::Duration::from_millis(600));
    // 交接前清孤儿 webview:发起方退出后它的 WebView2 子进程可能存活,
    // 既碍安装器替换文件,又会让新实例黑屏(D-171)。
    cleanup_orphan_webviews();
    // 用 output() 而不是 status():安装器的 stdout/stderr 是唯一能说明
    // "为什么没装上"的东西,丢掉它等于把诊断入口关死。
    match Command::new(installer).arg("/S").output() {
        Ok(out) if out.status.success() => {
            update_log("安装器 exit=0");
        }
        Ok(out) => {
            update_log(&format!(
                "安装器失败 exit={:?} stdout={} stderr={}",
                out.status.code(),
                String::from_utf8_lossy(&out.stdout).trim(),
                String::from_utf8_lossy(&out.stderr).trim(),
            ));
            update_log("保留安装包供手动执行,不重启");
            return;
        }
        Err(error) => {
            update_log(&format!("启动安装器失败: {error}"));
            return;
        }
    }
    // 装完核对:安装目录的镜像必须真的变新了。NSIS 在目标被占用时也可能
    // 报 exit=0 而什么都没换,只信退出码会把"静默没装上"当成成功。
    let after = image_stamp(exe);
    update_log(&format!("安装前 exe={before:?} 安装后 exe={after:?}"));
    if !image_replaced(before, after) {
        // 到这里说明:退出码说成功,文件却没动。两种已知成因——目标被占用
        // (WebView2 子进程、另一个实例),或安装位与运行位根本不是同一个文件
        // (容器/重定向环境,D-198)。安装包**不删**,留给用户手动执行。
        update_log(
            "安装器 exit=0 但 exe 未被替换(mtime/大小不变):目标可能被占用,\
             或安装位与运行位不是同一个文件。保留安装包供手动执行。",
        );
        match Command::new(exe).spawn() {
            Ok(_) => update_log("已拉起——仍是旧版本,更新未生效"),
            Err(error) => update_log(&format!("拉起失败: {error}(手动启动即可)")),
        }
        return;
    }
    match Command::new(exe).spawn() {
        Ok(_) => update_log("已拉起新版本"),
        Err(error) => update_log(&format!("拉起新版本失败: {error}(手动启动即可)")),
    }
    let _ = std::fs::remove_file(installer);
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .ok()
        .is_some_and(|out| String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()))
}

#[cfg(not(windows))]
fn process_alive(_pid: u32) -> bool {
    false
}

/// 安装器随包发的 kz CLI 同步到 `~\.cargo\bin\kz.exe`(D-175)。
///
/// 桌面端与 CLI 是两个独立安装通道,却共用同一个 `.kanzei/state.db`,而 schema
/// 迁移是单向的:只更新 kzapp 的话,一次 schema 变更就让机器上的旧 kz 直接
/// 打不开库(UnsupportedSchema)。安装器现在把 kz 一起装到应用目录,由这里
/// 搬到 CLI 该在的位置——安装器之后唯一会运行的东西就是本程序。
///
/// 三条约束:①只升不降,开发者手动 cargo install 的更新版本不会被安装包里的
/// 旧版盖掉;②标记文件让常态启动只读一个几十字节的文件,不起子进程;
/// ③任何一步失败都只记日志,绝不阻断启动——CLI 落后是退化,启动不了是事故。
fn sync_bundled_cli() {
    let Some(cargo_bin) = dirs::home_dir().map(|home| home.join(".cargo").join("bin")) else {
        return;
    };
    let ours = option_env!("KANZEI_BUILD_INFO").unwrap_or("dev");
    if ours == "dev" {
        return;
    }
    let marker = cargo_bin.join(".kz-synced");
    if std::fs::read_to_string(&marker).is_ok_and(|synced| synced.trim() == ours) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(sidecar) = exe.parent().map(|dir| dir.join("kz.exe")) else {
        return;
    };
    // 开发构建(cargo build,未过 bundler)没有这个 sidecar,直接跳过。
    if !sidecar.is_file() {
        return;
    }
    let target = cargo_bin.join("kz.exe");
    if target.is_file() && !cli_is_older(&target, ours) {
        let _ = std::fs::write(&marker, ours);
        return;
    }
    if let Err(error) = std::fs::create_dir_all(&cargo_bin).and_then(|()| {
        std::fs::copy(&sidecar, &target)?;
        std::fs::write(&marker, ours)
    }) {
        // kz 正在运行时会锁住镜像文件;不重试也不报错,下次启动自然会再来一遍。
        eprintln!("kzapp:同步 kz CLI 失败(下次启动重试): {error}");
    }
}

/// 已安装的 kz 是否比我们旧。跑不起来就保守判为旧,让安装包里的已知版本覆盖上去。
fn cli_is_older(target: &Path, ours: &str) -> bool {
    let Ok(output) = Command::new(target).arg("--version").output() else {
        return true;
    };
    installed_cli_is_older(&String::from_utf8_lossy(&output.stdout), ours)
}

/// `kz --version` 的输出形如 `kanzei 0.1.0 (0c9f903 20260808120442)`。
/// 解析不出构建戳(旧格式、被截断、根本不是 kz)时保守判为旧——两个二进制
/// 版本对不上本身就是要修的状态,让出厂版本覆盖上去比放着不管安全。
fn installed_cli_is_older(version_output: &str, ours: &str) -> bool {
    let Some(installed) = version_output
        .split('(')
        .nth(1)
        .and_then(|rest| rest.split(')').next())
    else {
        return true;
    };
    match (build_stamp(installed), build_stamp(ours)) {
        (Some((installed_stamp, _)), Some((our_stamp, _))) => installed_stamp < our_stamp,
        _ => true,
    }
}

fn apply_pending_update(exe: &Path, pending: &Path) {
    // 给父进程释放 Windows 映像文件锁留出时间；后续 rename 仍以重试为准。
    std::thread::sleep(std::time::Duration::from_millis(250));
    let backup = exe.with_extension("exe.previous");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        let _ = std::fs::remove_file(&backup);
        if std::fs::rename(exe, &backup).is_ok() {
            match std::fs::rename(pending, exe) {
                Ok(()) => {
                    match Command::new(exe).spawn() {
                        Ok(_) => { let _ = std::fs::remove_file(&backup); }
                        Err(error) => {
                            eprintln!("kzapp:新版本启动失败,回滚: {error}");
                            let _ = std::fs::remove_file(exe);
                            let _ = std::fs::rename(&backup, exe);
                        }
                    }
                    return;
                }
                Err(_) => {
                    let _ = std::fs::rename(&backup, exe);
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    eprintln!("kzapp:pending 更新失败,保留旧版本与 pending 文件");
}

#[cfg(test)]
mod state_tests;

#[cfg(test)]
mod permission_tests;

#[cfg(test)]
mod conversation_tests;

#[cfg(test)]
mod process_tests;

#[cfg(test)]
mod update_tests_update;

#[cfg(test)]
mod update_tests {
    use super::{
        default_process_id, pending_ask_payload, persist_always_allow, process_session_id,
        recover_messages_at, recover_messages_raw,
        conversation_prior, runtime_for, stop_runtime_and_finalize, take_pending_ask,
        with_session_id, AppState, PendingAsk, SessionRuntime,
    };
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::oneshot;

    /// 建一个"上级是项目、下面挂两个子目录"的现场,`with_data` 决定上级有没有数据。
    fn isolation_fixture(tag: &str, with_data: bool) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "kz-iso-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(base.join(".kanzei/project")).unwrap();
        if with_data {
            std::fs::write(
                base.join(".kanzei/project/requirements.md"),
                "# Requirements\n\n## R-900 上级的需求 [todo]\n",
            )
            .unwrap();
        }
        let a = base.join("projA");
        let b = base.join("projB");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        (base, a, b)
    }

    #[test]
    fn 祖先无数据时静默自动隔离_有数据时绝不擅自改根() {
        // 情形一:祖先是空项目 —— 补 .kanzei 前后用户看到的都是空,无损,应静默修。
        let (base, a, _b) = isolation_fixture("auto", false);
        assert!(super::ensure_project_isolated(&a), "祖先无数据时应自动隔离");
        assert_eq!(
            kanzei_harness::config::discover_project_root(&a).unwrap(),
            a,
            "自动隔离后应以自身为根",
        );
        // 幂等:已经自成一根就不该再"修"一次。
        assert!(!super::ensure_project_isolated(&a), "重复调用不应重复修复");
        std::fs::remove_dir_all(&base).ok();

        // 情形二:祖先有需求数据 —— 改根会让这个项目从"看得到那批条目"变成空,
        // 属于可见变化,必须留给用户确认,引擎不得擅自动手。
        let (base, a, _b) = isolation_fixture("manual", true);
        assert!(
            !super::ensure_project_isolated(&a),
            "祖先有数据时不得自动改根(会让用户以为条目丢了)",
        );
        assert_ne!(
            kanzei_harness::config::discover_project_root(&a).unwrap(),
            a,
            "未确认前应保持原样",
        );
        let info = super::project_root_info(a.display().to_string());
        assert!(info["shared"].as_bool().unwrap(), "必须如实报出共用状态");
        assert!(!info["autoRepaired"].as_bool().unwrap(), "这种情形不该声称已自动修复");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn 隔离体检一次报完全部共用项目() {
        // 一次性看清所有受影响的项目,而不是切一个发现一个。
        let (base, a, b) = isolation_fixture("report", true);
        let shared: Vec<String> = [&a, &b]
            .iter()
            .filter(|dir| {
                let resolved = kanzei_harness::config::discover_project_root(dir).unwrap();
                std::fs::canonicalize(&resolved).ok() != std::fs::canonicalize(dir).ok()
            })
            .map(|d| d.display().to_string())
            .collect();
        assert_eq!(shared.len(), 2, "两个子目录此时都共用上级");
        // 分离其中一个,另一个不受影响。
        super::project_detach(a.display().to_string()).unwrap();
        assert_eq!(kanzei_harness::config::discover_project_root(&a).unwrap(), a);
        assert_ne!(
            kanzei_harness::config::discover_project_root(&b).unwrap(),
            b,
            "分离 A 不该顺手改动 B",
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn 同一上级下的两个项目必须各自独立不串数据() {
        let base = std::env::temp_dir().join(format!(
            "kz-iso-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        // 上级目录本身是个项目(有 .kanzei)——这正是串数据的前提条件。
        std::fs::create_dir_all(base.join(".kanzei/project")).unwrap();
        std::fs::write(base.join(".kanzei/project/requirements.md"), "# Requirements\n\n## R-900 上级的需求 [todo]\n").unwrap();
        let a = base.join("projA");
        let b = base.join("projB");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        // 未初始化时:两个子目录都向上解析到同一个根 = 共用同一份需求。
        let root_a = kanzei_harness::config::discover_project_root(&a).unwrap();
        let root_b = kanzei_harness::config::discover_project_root(&b).unwrap();
        assert_eq!(root_a, root_b, "前提:未初始化时两者确实会落到同一个根");
        assert!(
            super::project_root_info(a.display().to_string())["shared"]
                .as_bool()
                .unwrap(),
            "共用上级时必须报出来,不能静默",
        );

        // 分离后:各自成根,互不可见。
        super::project_detach(a.display().to_string()).unwrap();
        let root_a = kanzei_harness::config::discover_project_root(&a).unwrap();
        assert_eq!(root_a, a, "分离后应以自身为根");
        assert_ne!(
            root_a,
            kanzei_harness::config::discover_project_root(&b).unwrap(),
            "分离后两个项目不得再共用同一个根",
        );
        assert!(
            !super::project_root_info(a.display().to_string())["shared"]
                .as_bool()
                .unwrap(),
            "分离后不该再报共用",
        );
        // 分离只建空间,不搬上级的既有条目——那些属于上级项目。
        assert!(
            !a.join(".kanzei/project/requirements.md").exists(),
            "分离不应把上级的需求复制过来",
        );
        assert!(
            base.join(".kanzei/project/requirements.md").exists(),
            "上级的需求不得被动到",
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn 保存前拦住指向不存在_provider_的模型角色() {
        let payload = |primary: &str, providers: Vec<(&str, &str)>| super::SettingsPayload {
            primary: primary.into(),
            fast: String::new(),
            proxy: "env".into(),
            reasoning: None,
            profile_default: None,
            profile: None,
            providers: providers
                .into_iter()
                .map(|(name, base)| super::ProviderPayload {
                    name: name.into(),
                    protocol: "openai".into(),
                    base_url: base.into(),
                    api_key_env: None,
                    api_key: None,
                    auth: None,
                    context_limit: None,
                })
                .collect(),
        };

        // provider 拼错 —— 此前要到真正发消息时才炸,那时早已离开设置页。
        let error =
            super::validate_model_roles(&payload("deepsek:deepseek-chat", vec![("deepseek", "https://api.deepseek.com/v1")]))
                .unwrap_err();
        assert!(error.contains("primary"), "{error}");
        assert!(error.contains("deepsek:deepseek-chat"), "错误里要带上填错的原文: {error}");

        // 少了冒号也不行。
        let error = super::validate_model_roles(&payload("deepseek-chat", vec![("deepseek", "x")])).unwrap_err();
        assert!(error.contains("provider:model"), "{error}");

        // 正确的放行;内置 provider(fill_defaults 补的 anthropic/ollama…)同样认。
        assert!(super::validate_model_roles(&payload(
            "deepseek:deepseek-chat",
            vec![("deepseek", "https://api.deepseek.com/v1")]
        ))
        .is_ok());
        assert!(super::validate_model_roles(&payload("anthropic:claude-sonnet-5", vec![])).is_ok());

        // 留空 = 用内置默认,不该被当成错误挡下。
        assert!(super::validate_model_roles(&payload("", vec![])).is_ok());
    }

    #[test]
    fn defect_review_snapshot_is_strictly_read_only() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-defect-review-tools-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let rctx = kanzei_harness::ResolveCtx {
            profile: kanzei_harness::ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(kanzei_harness::KanzeiConfig::default()),
        };
        let snapshot = super::defect_review_snapshot(&rctx).unwrap();
        let mut names: Vec<String> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["glob", "grep", "read"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn defect_review_rejects_empty_model_report() {
        let empty = kanzei_core::RunSummary {
            text: "  ".into(),
            usage: kanzei_llm::Usage::default(),
            steps: 1,
            halted_by_user: false,
            messages: vec![],
            context_report: vec![],
            overflow_traces: vec![],
        };
        assert!(super::defect_review_report(&empty).is_err());

        let report = kanzei_core::RunSummary {
            text: "# 缺陷审查\n\n有可复核证据".into(),
            usage: kanzei_llm::Usage::default(),
            steps: 1,
            halted_by_user: false,
            messages: vec![],
            context_report: vec![],
            overflow_traces: vec![],
        };
        assert!(super::defect_review_report(&report)
            .unwrap()
            .contains("可复核证据"));
    }

    #[tokio::test]
    async fn defect_review_empty_state_returns_without_model_call() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-defect-review-empty-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(root.join(".kanzei/project/defects.md"), "# Defects\n").unwrap();
        let result = super::defect_review(root.display().to_string())
            .await
            .unwrap();
        assert!(result.empty);
        assert_eq!(result.defect_count, 0);
        assert!(result.report.contains("没有活动缺陷"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn docs_snapshot_exposes_block_reasons_and_scheduler_order() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-docs-blocked-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(
            root.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-001 被阻塞 [todo]\n- 阻塞: 等待确认\n\n## R-002 可执行 [todo]\n",
        )
        .unwrap();
        let snapshot = super::docs_snapshot(root.display().to_string());
        let requirements = snapshot["requirements"].as_array().unwrap();
        assert_eq!(requirements[0]["id"], "R-002");
        assert_eq!(requirements[1]["id"], "R-001");
        assert_eq!(requirements[1]["blocked"], true);
        assert!(requirements[1]["block_reasons"][0]
            .as_str()
            .unwrap()
            .contains("等待确认"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn export_project_data_copies_selected_work_materials() {
        let base = std::env::temp_dir().join(format!(
            "kanzei-export-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = base.join("project");
        let output = base.join("output");
        std::fs::create_dir_all(project.join(".kanzei/memory")).unwrap();
        std::fs::create_dir_all(project.join(".kanzei/project")).unwrap();
        std::fs::write(project.join(".kanzei/memory/M-001.md"), "记忆").unwrap();
        std::fs::write(project.join(".kanzei/project/requirements.md"), "需求").unwrap();
        std::fs::write(project.join(".kanzei/project/defects.md"), "缺陷").unwrap();
        std::fs::write(project.join(".kanzei/kanzei.toml"), "[models]").unwrap();
        let result = super::export_project_data(super::ExportOptions {
            project_dir: project.display().to_string(),
            output_dir: output.display().to_string(),
            include_memory: true,
            include_requirements: true,
            include_defects: false,
            include_config: true,
        })
        .unwrap();
        let export_path = PathBuf::from(result["path"].as_str().unwrap());
        assert!(export_path.join(".kanzei/memory/M-001.md").is_file());
        assert!(export_path.join(".kanzei/project/requirements.md").is_file());
        assert!(export_path.join(".kanzei/kanzei.toml").is_file());
        assert!(!export_path.join(".kanzei/project/defects.md").exists());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn stopping_after_promote_cancels_promoted_and_pending_inputs_atomically() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-app-stop-promoted-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = kanzei_core::SessionStore::open(&root.join("state.db")).unwrap();
        let session_id = "session_stop_promoted";
        store
            .create_session(session_id, &root.display().to_string(), None)
            .unwrap();
        store
            .admit_input(session_id, "promoted", "先执行", kanzei_core::Delivery::Queue)
            .unwrap();
        store
            .admit_input(session_id, "pending", "后执行", kanzei_core::Delivery::Queue)
            .unwrap();
        assert_eq!(
            store.promote_next_input(session_id).unwrap().unwrap().input_id,
            "promoted"
        );

        let runtime = SessionRuntime::default();
        runtime.running.store(true, Ordering::SeqCst);
        let cancelled = stop_runtime_and_finalize(&runtime, &store, session_id).unwrap();

        assert_eq!(cancelled, 2);
        assert!(!runtime.running.load(Ordering::SeqCst));
        assert!(store.list_pending_inputs(session_id).unwrap().is_empty());
        assert_eq!(store.get_session(session_id).unwrap().unwrap().status, "idle");
        let event = store
            .latest_event(session_id, "session.status_changed")
            .unwrap()
            .unwrap();
        assert_eq!(event.payload["reason"], "stopped_by_user");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// D-179:停止不得再吃掉整轮轨迹。收尾代码全在被 abort 的 task 里,
    /// 先杀后写等于什么都不写——实测一次 41 分钟的运行只剩一条 stopped_by_user。
    #[test]
    fn 停止时在飞轨迹与episode先落库再abort() {
        let root = std::env::temp_dir().join(format!(
            "kz-stop-flush-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = kanzei_core::SessionStore::open(&root.join("state.db")).unwrap();
        let session_id = "session_stop_flush";
        store
            .create_session(session_id, &root.display().to_string(), None)
            .unwrap();

        let runtime = SessionRuntime::default();
        runtime.running.store(true, Ordering::SeqCst);
        {
            let mut live = runtime.live.lock().unwrap();
            live.begin("run_x", "input_x", "很长的一轮", "deepseek", "deepseek-v4-flash");
            live.steps = 37;
            live.input_tokens = 210_000;
            live.output_tokens = 14_000;
            live.trace.push(serde_json::json!({"kind": "tool.completed", "name": "bash"}));
        }

        stop_runtime_and_finalize(&runtime, &store, session_id).unwrap();

        let trace = store
            .latest_event(session_id, "run.trace")
            .unwrap()
            .expect("停止必须留下轨迹");
        assert_eq!(trace.payload["outcome"], "halted");
        assert_eq!(trace.payload["events"][0]["name"], "bash");

        let episodes = store.list_episodes(session_id, 5).unwrap();
        assert_eq!(episodes.len(), 1, "停止必须留下 episode");
        assert_eq!(episodes[0].2, "halted");
        assert_eq!(episodes[0].3, 37, "步数要取在飞画像的真实值");
        let identity = store.recent_episode_identities(session_id, 5).unwrap();
        assert_eq!(identity[0].1, "deepseek");
        assert_eq!(identity[0].3, "run_x");

        // 幂等:同一轮不会被写第二遍(正常收尾与停止路径都会调用)。
        stop_runtime_and_finalize(&runtime, &store, session_id).unwrap();
        assert_eq!(store.list_episodes(session_id, 5).unwrap().len(), 1);

        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn process_sessions_are_isolated_but_default_keeps_legacy_id() {
        let root = Path::new(r"C:\project");
        let default_id = default_process_id(root);
        assert_eq!(process_session_id(root, None), kanzei_core::project_session_id(root));
        assert_eq!(
            process_session_id(root, Some(&default_id)),
            kanzei_core::project_session_id(root)
        );
        assert_ne!(
            process_session_id(root, Some("p1|C:\\project")),
            process_session_id(root, Some("p2|C:\\project"))
        );
    }

    #[test]
    fn session_runtime_is_reused_per_session_and_isolated_between_sessions() {
        let state = AppState::default();
        let first = runtime_for(&state, "ses_a");
        let same = runtime_for(&state, "ses_a");
        let other = runtime_for(&state, "ses_b");
        assert!(Arc::ptr_eq(&first, &same));
        assert!(!Arc::ptr_eq(&first, &other));
    }

    #[test]
    fn pending_ask_lookup_stays_with_its_runtime_container() {
        let state = AppState::default();
        let first = runtime_for(&state, "ses_a");
        let second = runtime_for(&state, "ses_b");
        let (sender, _receiver) = oneshot::channel();
        first.asks.lock().unwrap().insert(
            7,
            PendingAsk {
                sender,
                request: kanzei_core::AskRequest::Question {
                    question: "继续?".into(),
                    options: Vec::new(),
                    default: None,
                },
                action: "question".into(),
                resource: "继续?".into(),
                project_root: PathBuf::from("project-a"),
                session_id: "ses_a".into(),
            },
        );
        assert_eq!(take_pending_ask(&state, 7).unwrap().session_id, "ses_a");
        assert!(take_pending_ask(&state, 7).is_none());
        assert!(second.asks.lock().unwrap().is_empty());
    }

    #[test]
    fn conversation_prior_prefers_existing_memory_over_persisted_snapshot() {
        let conversation = Arc::new(Mutex::new(HashMap::new()));
        let persisted = vec![kanzei_llm::Message::user_text("恢复快照")];
        assert_eq!(conversation_prior(&conversation, "ses", persisted.clone())[0].parts, persisted[0].parts);
        let existing = vec![kanzei_llm::Message::user_text("内存旧快照")];
        conversation.lock().unwrap().insert("ses".into(), existing.clone());
        let selected = conversation_prior(
            &conversation,
            "ses",
            vec![kanzei_llm::Message::user_text("最新持久化")],
        );
        assert_eq!(selected[0].parts, existing[0].parts);
    }

    #[test]
    fn recover_messages_filters_orphan_tool_calls_from_persisted_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-app-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = kanzei_core::SessionStore::open(&root.join("state.db")).unwrap();
        store.create_session("ses_history", &root.display().to_string(), None).unwrap();
        let messages = vec![
            kanzei_llm::Message::user_text("保留文本"),
            kanzei_llm::Message::assistant(vec![kanzei_llm::Part::ToolCall {
                id: "orphan".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "echo orphan"}),
            }]),
        ];
        store
            .append_event(
                "ses_history",
                "conversation.updated",
                &serde_json::json!({"messages": messages}),
            )
            .unwrap();
        let recovered = recover_messages_at(&store, "ses_history", None).unwrap();
        assert_eq!(recovered.len(), 1);
        assert!(matches!(recovered[0].parts[0], kanzei_llm::Part::Text { ref text } if text == "保留文本"));
        // 展示路径不过滤:未配对的工具部件对人仍然可见,否则历史"看不全"。
        let raw = recover_messages_raw(&store, "ses_history", None).unwrap();
        let raw_parts: usize = raw.iter().map(|m| m.parts.len()).sum();
        let filtered_parts: usize = recovered.iter().map(|m| m.parts.len()).sum();
        assert!(
            raw_parts > filtered_parts,
            "原文应含被过滤掉的孤儿工具部件: raw={raw_parts} filtered={filtered_parts}"
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }


    #[test]
    fn persist_always_allow_success_returns_always_allow_and_path() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-app-always-ok-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        let (reply, path) = persist_always_allow(&root, "bash", "git status").unwrap();
        assert_eq!(reply, kanzei_core::AskReply::AlwaysAllow);
        assert_eq!(path, root.join(".kanzei/kanzei.toml"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persist_always_allow_failure_returns_deny_path() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-app-always-fail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::write(root.join(".kanzei/kanzei.toml"), "[invalid\n").unwrap();
        assert!(persist_always_allow(&root, "bash", "git status").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn main() {
    if startup_update() { return; }
    // 安装器只装得了 kzapp,CLI 得由这里搬到位——两者共用一个库,版本必须同步(D-175)。
    sync_bundled_cli();
    // 窗口创建之前自清孤儿 webview(D-171):上一个实例被强杀留下的
    // msedgewebview2 会锁住数据目录,不清的话本次启动必黑屏。
    cleanup_orphan_webviews();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    tauri::Builder::default()
        .manage(AppState::default())
        // UI 探针的出口:装一次,之后工具侧只认 UI_PROBE_EMIT。
        .setup(|app| {
            let handle = app.handle().clone();
            let _ = UI_PROBE_EMIT.set(Box::new(move |payload| {
                let _ = handle.emit("kz:ui-probe", payload);
            }));
            // 窗口从 tauri.conf.json 自动创建改为这里手动创建(R-101 E2 harness):
            // 配置里 `"create": false`,由 from_config 按同一份配置建窗口,生产路径
            // 行为不变;仅当环境变量 KANZEI_E2E_CDP 非空时注入 --remote-debugging-port
            // 打开 WebView2 DevTools 协议,供 E2 脚本通过 CDP 驱动真实 UI。
            let window_config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .ok_or_else(|| "tauri.conf.json 未配置任何窗口".to_string())?;
            let mut builder = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?;
            if let Ok(port) = std::env::var("KANZEI_E2E_CDP") {
                if !port.trim().is_empty() {
                    builder = builder.additional_browser_args(&format!("--remote-debugging-port={}", port.trim()));
                }
            }
            builder.build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ui_probe_result,
            files_view::files_snapshot,
            files_view::file_preview,
            files_view::files_annotate,
            projects_get,
            projects_add,
            projects_init,
            projects_rename,
            projects_pick,
            projects_remove,
            projects_select,
            workspace_snapshot,
            docs_snapshot,
            run_prompt,
            stop_run,
            answer_ask,
            pending_asks_get,
            settings_get,
            settings_save,
            settings_open,
            export_pick_dir,
            export_project_data,
            permission_rules_get,
            permission_rule_delete,
            provider_test,
            update_check,
            update_install,
            quick_req,
            defect_review,
            memory_overview,
            memory_entries,
            memory_recalls,
            memory_entry_delete,
            memory_note_candidates,
            memory_note_discard,
            run_metrics,
            project_root_info,
            project_detach,
            projects_isolation_report,
            fast_model_status,
            fast_model_setup,
            memory_entry_save,
            memory_search_page,
            memory_context_bill,
            memory_consolidate,
            memory_focus_get,
            memory_focus_set,
            app_info,
            models_list,
            docs_update,
            docs_open,
            summarize_chat,
            git_status,
            conventions_init,
            conversation_clear,
            conversation_delete,
            docs_read,
            conversation_get,
            conversation_trace_get,
            conversation_list,
            list_pending_inputs,
            cancel_input,
            project_files,
            process_list,
            process_create,
            process_update,
            process_close,
            worktree_create,
            worktree_diff,
            worktree_merge,
            worktree_discard,
            test_runs_snapshot,
            test_run_record,
            mobile_service_start,
            mobile_service_stop,
            agent_container_create,
            agent_container_upgrade,
            agent_container_rollback
        ])
        .run(tauri::generate_context!())
        .expect("error while running kanzei app");
}

// ---------- 多项目管理(~/.kanzei/app.json) ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AppPrefs {
    #[serde(default)]
    projects: Vec<String>,
    #[serde(default)]
    current: Option<String>,
    /// 项目显示名映射;旧版 app.json 没有此字段时回退为目录名。
    #[serde(default)]
    names: HashMap<String, String>,
}

fn prefs_path() -> PathBuf {
    kanzei_harness::kanzei_home()
        .unwrap_or_default()
        .join("app.json")
}

fn load_prefs() -> AppPrefs {
    std::fs::read_to_string(prefs_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_prefs(prefs: &AppPrefs) {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(prefs).unwrap_or_default(),
    );
}

#[tauri::command]
fn process_list(state: State<'_, AppState>, project_dir: String) -> Result<Vec<ProcessInfo>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let default = ensure_default_process(&state, &root);
    let processes = state.processes.lock().unwrap();
    let mut result = processes
        .values()
        .filter(|process| process.origin_project == root.display().to_string())
        .map(|process| process_info(&state, process))
        .collect::<Vec<_>>();
    if !result.iter().any(|item| item.id == default.id) {
        result.push(process_info(&state, &default));
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

#[tauri::command]
fn process_create(
    state: State<'_, AppState>,
    project_dir: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    subagent: Option<bool>,
) -> Result<ProcessInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    ensure_default_process(&state, &root);
    let project = root.display().to_string();
    let mut processes = state.processes.lock().unwrap();
    let next = processes
        .values()
        .filter(|process| process.project_dir == project && process.id.starts_with("p"))
        .filter_map(|process| process.id.split('|').next()?.strip_prefix('p')?.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    let process = ProcessHandle {
        id: format!("p{next}|{project}"),
        origin_project: project.clone(),
        project_dir: project,
        worktree_path: None,
        model: Arc::new(Mutex::new(model.filter(|value| !value.trim().is_empty()))),
        profile: Arc::new(Mutex::new(profile.filter(|value| !value.trim().is_empty()))),
        reasoning: Arc::new(Mutex::new(reasoning.filter(|value| !value.trim().is_empty()))),
        subagent_enabled: Arc::new(AtomicBool::new(subagent.unwrap_or(true))),
    };
    let info = process_info(&state, &process);
    processes.insert(process.id.clone(), process);
    Ok(info)
}

#[tauri::command]
fn process_update(
    state: State<'_, AppState>,
    process_id: String,
    model: Option<String>,
    profile: Option<String>,
    reasoning: Option<String>,
    subagent: Option<bool>,
) -> Result<ProcessInfo, String> {
    let process = state
        .processes
        .lock()
        .unwrap()
        .get(&process_id)
        .cloned()
        .ok_or_else(|| format!("进程不存在: {process_id}"))?;
    if let Some(model) = model {
        *process.model.lock().unwrap() = Some(model).filter(|value| !value.trim().is_empty());
    }
    if let Some(profile) = profile {
        *process.profile.lock().unwrap() = Some(profile).filter(|value| !value.trim().is_empty());
    }
    if let Some(reasoning) = reasoning {
        // 空串 = 清除本进程覆盖,回落配置默认档。
        *process.reasoning.lock().unwrap() =
            Some(reasoning).filter(|value| !value.trim().is_empty());
    }
    if let Some(subagent) = subagent {
        process.subagent_enabled.store(subagent, Ordering::SeqCst);
    }
    Ok(process_info(&state, &process))
}

#[tauri::command]
fn process_close(state: State<'_, AppState>, process_id: String) -> Result<(), String> {
    let process = state
        .processes
        .lock()
        .unwrap()
        .get(&process_id)
        .cloned()
        .ok_or_else(|| format!("进程不存在: {process_id}"))?;
    let root = PathBuf::from(&process.project_dir);
    let session_id = process_session_id(&root, Some(&process_id));
    if let Some(runtime) = state.runtimes.lock().unwrap().get(&session_id).cloned() {
        if let Some(handle) = runtime.current_run.lock().unwrap().take() {
            handle.abort();
        }
        runtime.asks.lock().unwrap().clear();
        runtime.running.store(false, Ordering::SeqCst);
    }
    if process_id.starts_with("d|") {
        *process.model.lock().unwrap() = None;
        *process.profile.lock().unwrap() = None;
        process.subagent_enabled.store(true, Ordering::SeqCst);
    } else {
        state.processes.lock().unwrap().remove(&process_id);
    }
    Ok(())
}

fn worktree_command(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    hidden_command("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git 执行失败: {e}"))
}

fn worktree_field(root: &Path, worktree: &Path, field: &str) -> Result<String, String> {
    let output = worktree_command(worktree, &["branch", "--show-current"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        return Err(format!("工作树没有可合并分支: {}", worktree.display()));
    }
    if field == "branch" {
        Ok(branch)
    } else {
        let _ = root;
        Ok(branch)
    }
}

fn validate_worktree_path(root: &Path, worktree_path: &str) -> Result<PathBuf, String> {
    let worktree = std::fs::canonicalize(worktree_path)
        .map_err(|e| format!("工作树不存在或无法解析: {e}"))?;
    let parent = root
        .parent()
        .unwrap_or(root)
        .canonicalize()
        .unwrap_or_else(|_| root.parent().unwrap_or(root).to_path_buf());
    if !worktree.starts_with(&parent) || worktree == root {
        return Err("工作树必须位于项目同级目录,不能指向项目本身或外部路径".into());
    }
    Ok(worktree)
}

#[tauri::command]
fn worktree_create(project_dir: String, name: String) -> Result<WorktreeInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let safe_name: String = name
        .trim()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') { ch } else { '-' })
        .collect();
    if safe_name.is_empty() {
        return Err("工作树名称不能为空".into());
    }
    let parent = root.parent().unwrap_or(&root);
    let worktree = parent.join(format!(".kanzei-worktree-{safe_name}"));
    if worktree.exists() {
        return Err(format!("工作树已存在: {}", worktree.display()));
    }
    let branch = format!("kanzei/thread-{safe_name}");
    let output = worktree_command(&root, &[
        "worktree", "add", "-b", &branch, &worktree.display().to_string(), "HEAD",
    ])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(WorktreeInfo { path: worktree.display().to_string(), branch, files: Vec::new(), clean: true, diff: String::new() })
}

#[tauri::command]
fn worktree_diff(project_dir: String, worktree_path: String) -> Result<WorktreeInfo, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let branch = worktree_field(&root, &worktree, "branch")?;
    let output = worktree_command(&root, &["-C", &worktree.display().to_string(), "status", "--porcelain"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let diff_output = worktree_command(&root, &["-C", &worktree.display().to_string(), "diff", "--no-ext-diff", "--binary"])?;
    if !diff_output.status.success() {
        return Err(String::from_utf8_lossy(&diff_output.stderr).trim().to_string());
    }
    let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
    Ok(WorktreeInfo { path: worktree.display().to_string(), branch, clean: files.is_empty(), files, diff })
}

#[tauri::command]
fn worktree_merge(project_dir: String, worktree_path: String) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let branch = worktree_field(&root, &worktree, "branch")?;
    let check = worktree_command(&root, &["merge-tree", "--write-tree", "HEAD", &branch])?;
    if !check.status.success() {
        return Err(format!("合并前冲突检测失败,双方改动已保留:\n{}", String::from_utf8_lossy(&check.stdout)));
    }
    let output = worktree_command(&root, &["merge", "--no-ff", &branch])?;
    if !output.status.success() {
        return Err(format!("合并未完成,请在主项目中解决并保留工作树:\n{}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(format!("已合并工作树分支 {branch};工作树仍保留,可检查后显式放弃"))
}

#[tauri::command]
fn worktree_discard(project_dir: String, worktree_path: String) -> Result<String, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let worktree = validate_worktree_path(&root, &worktree_path)?;
    let output = worktree_command(&root, &["worktree", "remove", &worktree.display().to_string()])?;
    if !output.status.success() {
        return Err(format!("工作树未放弃: 工作树可能仍有未提交改动,已保留以便恢复:\n{}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(format!("已放弃工作树 {} 的工作目录;分支仍保留", worktree.display()))
}

#[tauri::command]
fn projects_get() -> AppPrefs {
    let mut prefs = load_prefs();
    prefs.projects.retain(|p| Path::new(p).is_dir());
    prefs.names.retain(|path, _| prefs.projects.contains(path));
    if prefs.projects.is_empty() {
        if let Ok(cwd) = std::env::current_dir() {
            prefs.projects.push(cwd.display().to_string());
        }
    }
    if prefs
        .current
        .as_deref()
        .map(|c| !Path::new(c).is_dir())
        .unwrap_or(true)
    {
        prefs.current = prefs.projects.first().cloned();
    }
    save_prefs(&prefs);
    prefs
}

#[tauri::command]
fn projects_init(path: String, name: Option<String>) -> Result<AppPrefs, String> {
    let dir = PathBuf::from(&path);
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建项目目录失败: {error}"))?;
    std::fs::create_dir_all(dir.join(".kanzei"))
        .map_err(|error| format!("创建项目配置目录失败: {error}"))?;
    let canonical = dir
        .canonicalize()
        .map(strip_verbatim)
        .unwrap_or(path.clone());
    let mut prefs = load_prefs();
    if !prefs.projects.contains(&canonical) {
        prefs.projects.push(canonical.clone());
    }
    let display_name = name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| base_name(&canonical));
    prefs.names.insert(canonical.clone(), display_name);
    prefs.current = Some(canonical);
    save_prefs(&prefs);
    Ok(projects_get())
}

#[tauri::command]
fn projects_rename(path: String, name: String) -> Result<AppPrefs, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("项目名称不能为空".into());
    }
    let mut prefs = load_prefs();
    if !prefs.projects.iter().any(|project| project == &path) {
        return Err("项目不在项目列表中".into());
    }
    prefs.names.insert(path, name.to_owned());
    save_prefs(&prefs);
    Ok(projects_get())
}

fn base_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_owned()
}

#[tauri::command]
fn projects_add(path: String) -> Result<AppPrefs, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("目录不存在: {path}"));
    }
    // D-170:必须就地建 `.kanzei`,否则 discover_project_root 会一路向上找,
    // 落到祖先目录(或最近的 .git)上——两个新加的项目就共用同一份
    // requirements.md/defects.md,需求在项目之间串。用户显式选的目录就是根。
    std::fs::create_dir_all(dir.join(".kanzei"))
        .map_err(|error| format!("创建项目配置目录失败: {error}"))?;
    let canonical = dir
        .canonicalize()
        .map(strip_verbatim)
        .unwrap_or(path.clone());
    let mut prefs = load_prefs();
    if !prefs.projects.contains(&canonical) {
        prefs.projects.push(canonical.clone());
    }
    prefs.current = Some(canonical);
    save_prefs(&prefs);
    Ok(projects_get())
}

/// R-136:fast 角色解析到的本地 Ollama 目标。fast 指向别的 provider 时返回 None——
/// 自动安装只对本地 Ollama 有意义,不该替用户改动他指定的外部模型。
fn ollama_fast_target() -> Option<(String, String)> {
    let mut config = KanzeiConfig::load(Path::new(".")).unwrap_or_default();
    config.fill_defaults();
    let resolved = config.resolve_model("fast").ok()?;
    if resolved.provider.base_url.contains("11434") {
        Some((resolved.provider.base_url.clone(), resolved.model))
    } else {
        None
    }
}

/// 本机回环请求专用 client:挂着系统代理反而连不上 127.0.0.1。
fn loopback_client(timeout_secs: u64) -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .ok()
}

fn ollama_cli_installed() -> bool {
    let mut cmd = Command::new("ollama");
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW:别闪黑框
    }
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

async fn ollama_service_up(base_url: &str) -> bool {
    let tags = format!("{}/api/tags", base_url.trim_end_matches("/v1"));
    let Some(client) = loopback_client(2) else { return false };
    client.get(&tags).send().await.map(|r| r.status().is_success()).unwrap_or(false)
}

async fn ollama_model_present(base_url: &str, model: &str) -> bool {
    let tags = format!("{}/api/tags", base_url.trim_end_matches("/v1"));
    let Some(client) = loopback_client(3) else { return false };
    let Ok(resp) = client.get(&tags).send().await else { return false };
    let Ok(v) = resp.json::<serde_json::Value>().await else { return false };
    v["models"]
        .as_array()
        .map(|models| {
            models.iter().any(|m| {
                m["name"]
                    .as_str()
                    // "qwen3.5:4b" 与 "qwen3.5:4b"/latest 尾缀都算命中。
                    .is_some_and(|n| n == model || n.trim_end_matches(":latest") == model)
            })
        })
        .unwrap_or(false)
}

/// /api/pull 的进度行 → 人话。返回 None 的行不值得刷给用户。
fn pull_progress_text(line: &serde_json::Value) -> Option<String> {
    let status = line["status"].as_str()?;
    match (line["completed"].as_u64(), line["total"].as_u64()) {
        (Some(done), Some(total)) if total > 0 => {
            let pct = done * 100 / total;
            Some(format!("{status} {pct}%({}/{} MB)", done >> 20, total >> 20))
        }
        _ => Some(status.to_string()),
    }
}

/// fast 子代理模型的就绪状态(R-136)。
#[tauri::command]
async fn fast_model_status() -> serde_json::Value {
    let Some((base_url, model)) = ollama_fast_target() else {
        return json!({ "managed": false });
    };
    let installed = ollama_cli_installed();
    let service_up = ollama_service_up(&base_url).await;
    let model_present = if service_up {
        ollama_model_present(&base_url, &model).await
    } else {
        false
    };
    json!({
        "managed": true,
        "model": model,
        "installed": installed,
        "serviceUp": service_up,
        "modelPresent": model_present,
        "ready": installed && service_up && model_present,
    })
}

/// 一键就绪(R-136):装 Ollama(winget)→ 起服务 → 拉模型,全程发 kz:fast-setup 进度。
/// 每步幂等:已满足的直接跳过,失败在哪步就停在哪步并说清下一步怎么办。
#[tauri::command]
async fn fast_model_setup(window: tauri::Window) -> Result<String, String> {
    let stage = |text: &str| {
        let _ = window.emit("kz:fast-setup", json!({ "text": text }));
    };
    let Some((base_url, model)) = ollama_fast_target() else {
        return Err("fast 角色当前指向非本地 Ollama 的 provider,不做自动安装。\
                    如需托管,把设置页的 fast 改回 ollama:<模型> 再试。"
            .into());
    };

    if !ollama_cli_installed() {
        stage("正在通过 winget 安装 Ollama(首次约数百 MB,取决于网速)…");
        let mut cmd = Command::new("winget");
        cmd.args([
            "install", "--id", "Ollama.Ollama", "--silent",
            "--accept-package-agreements", "--accept-source-agreements",
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let output = tauri::async_runtime::spawn_blocking(move || cmd.output())
            .await
            .map_err(|e| e.to_string())?
            .map_err(|_| {
                "本机没有 winget,无法自动安装。手动装:https://ollama.com/download 下载后重试。"
                    .to_string()
            })?;
        if !output.status.success() {
            return Err(format!(
                "winget 安装失败(退出码 {:?})。手动装:https://ollama.com/download 下载后重试。",
                output.status.code()
            ));
        }
        stage("Ollama 安装完成");
    }

    if !ollama_service_up(&base_url).await {
        stage("正在启动 Ollama 服务…");
        let mut cmd = Command::new("ollama");
        cmd.arg("serve");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        cmd.spawn().map_err(|e| format!("启动 ollama serve 失败:{e}"))?;
        // 刚装完首次起服务可能要几秒;轮询而不是猜一个 sleep。
        let mut up = false;
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if ollama_service_up(&base_url).await {
                up = true;
                break;
            }
        }
        if !up {
            return Err("Ollama 服务 20 秒内未就绪。手动跑一次 `ollama serve` 看报错。".into());
        }
        stage("Ollama 服务已就绪");
    }

    if !ollama_model_present(&base_url, &model).await {
        stage(&format!("正在拉取模型 {model}(体积以 GB 计,进度见下)…"));
        let pull = format!("{}/api/pull", base_url.trim_end_matches("/v1"));
        // 拉模型可能要很多分钟:专用长超时 client,靠流式进度证明还活着。
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .post(&pull)
            .json(&json!({ "name": model }))
            .send()
            .await
            .map_err(|e| format!("请求拉取失败:{e}"))?;
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut last_emit = String::new();
        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("拉取中断:{e}"))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buffer.find('\n') {
                let line: String = buffer.drain(..=pos).collect();
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                    continue;
                };
                if let Some(err) = v["error"].as_str() {
                    return Err(format!("拉取失败:{err}"));
                }
                if let Some(text) = pull_progress_text(&v) {
                    // 同一句状态(如反复的 pulling xx%)只在内容变化时发,别刷屏。
                    if text != last_emit {
                        stage(&text);
                        last_emit = text;
                    }
                }
            }
        }
        if !ollama_model_present(&base_url, &model).await {
            return Err(format!("拉取流结束但 {model} 仍不在本地模型列表里,重试一次看看。"));
        }
        stage(&format!("模型 {model} 已就绪"));
    }

    // 配置侧无需写盘:fill_defaults 已把 ollama provider 与 fast 默认补齐,
    // resolve_model("fast") 现在就能落到刚拉好的模型上。
    Ok(format!("fast 子代理已就绪:{model}"))
}

/// 某个根下是否已经存在"这个项目的东西"。判断依据是**用户可见的产物**:
/// 追踪文档、记忆、会话库。只有这些存在时,改根才会改变用户看到的内容。
fn root_has_data(root: &Path) -> bool {
    let k = root.join(".kanzei");
    ["project", "memory"]
        .iter()
        .any(|sub| k.join(sub).read_dir().map(|mut d| d.next().is_some()).unwrap_or(false))
        || k.join("state.db").is_file()
}

/// D-170 核心规则:**能无损修就自动修,会改变可见内容的才问用户**。
///
/// 未初始化的项目目录会被 `discover_project_root` 一路向上解析到祖先,于是共用
/// 同一祖先的几个项目读写同一份数据。要根治就得让每个注册项目自成一根,但对
/// 存量项目直接改根会让 `project_session_id` 变化、历史对话看起来消失。
///
/// 分两种情形:
/// - 祖先那边**没有任何项目数据** → 补 `.kanzei` 前后用户看到的都是空,无损,静默修;
/// - 祖先那边**有数据** → 不动。改了会让这个项目从"看得到那批条目"变成"空的",
///   属于可见变化,必须由用户在界面上确认(project_detach)。
///
/// 返回是否做了自动修复。
fn ensure_project_isolated(dir: &Path) -> bool {
    if dir.join(".kanzei").is_dir() {
        return false; // 已经自成一根
    }
    let Some(resolved) = kanzei_harness::config::discover_project_root(dir) else {
        return false;
    };
    if std::fs::canonicalize(&resolved).ok() == std::fs::canonicalize(dir).ok() {
        return false; // 解析结果就是自己
    }
    if root_has_data(&resolved) {
        return false; // 有数据,改根会改变所见,交给用户拍板
    }
    std::fs::create_dir_all(dir.join(".kanzei")).is_ok()
}

/// D-170:所选目录与实际生效的项目根是否一致。不一致 = 这个项目的需求/缺陷/会话
/// 其实存在祖先目录里,和共用同一祖先的其它项目**混在一起**。
#[tauri::command]
fn project_root_info(project_dir: String) -> serde_json::Value {
    let selected = PathBuf::from(&project_dir);
    let repaired = ensure_project_isolated(&selected);
    let resolved = kanzei_harness::config::discover_project_root(&selected)
        .unwrap_or_else(|| selected.clone());
    let same = std::fs::canonicalize(&selected).ok() == std::fs::canonicalize(&resolved).ok();
    json!({
        "selected": selected.display().to_string(),
        "resolved": resolved.display().to_string(),
        "shared": !same,
        // 无损自动修复过:界面无需打扰用户,但日志里要留痕。
        "autoRepaired": repaired,
    })
}

/// 全部注册项目的隔离体检:一次报完,而不是切一个发现一个。
/// 顺带对可无损修复的静默补齐。
#[tauri::command]
fn projects_isolation_report() -> serde_json::Value {
    let prefs = load_prefs();
    let mut shared = Vec::new();
    let mut repaired = Vec::new();
    for path in &prefs.projects {
        let dir = PathBuf::from(path);
        if !dir.is_dir() {
            continue;
        }
        if ensure_project_isolated(&dir) {
            repaired.push(path.clone());
            continue;
        }
        let resolved = kanzei_harness::config::discover_project_root(&dir)
            .unwrap_or_else(|| dir.clone());
        if std::fs::canonicalize(&resolved).ok() != std::fs::canonicalize(&dir).ok() {
            shared.push(json!({
                "project": path,
                "resolved": resolved.display().to_string(),
            }));
        }
    }
    json!({ "shared": shared, "autoRepaired": repaired })
}

/// 在所选目录就地建 `.kanzei`,把它从祖先项目里分离出来。
/// 只建目录,不搬数据:祖先那边的条目属于祖先项目,搬过来等于替用户做决定。
#[tauri::command]
fn project_detach(project_dir: String) -> Result<(), String> {
    let dir = PathBuf::from(&project_dir);
    if !dir.is_dir() {
        return Err(format!("目录不存在: {project_dir}"));
    }
    std::fs::create_dir_all(dir.join(".kanzei").join("project"))
        .map_err(|e| format!("创建项目空间失败: {e}"))?;
    // 回读校验:建完必须确实以自身为根,否则等于什么都没做却报了成功。
    let resolved = kanzei_harness::config::discover_project_root(&dir)
        .unwrap_or_else(|| dir.clone());
    if std::fs::canonicalize(&resolved).ok() != std::fs::canonicalize(&dir).ok() {
        return Err(format!(
            "已创建 {}/.kanzei,但项目根仍解析为 {} —— 请检查目录权限",
            dir.display(),
            resolved.display()
        ));
    }
    Ok(())
}

#[tauri::command]
async fn projects_pick() -> Result<Option<AppPrefs>, String> {
    let picked = rfd::AsyncFileDialog::new().pick_folder().await;
    match picked {
        Some(handle) => projects_add(handle.path().display().to_string()).map(Some),
        None => Ok(None),
    }
}

#[tauri::command]
fn projects_remove(path: String) -> AppPrefs {
    let mut prefs = load_prefs();
    prefs.projects.retain(|p| p != &path);
    prefs.names.remove(&path);
    if prefs.current.as_deref() == Some(path.as_str()) {
        prefs.current = prefs.projects.first().cloned();
    }
    save_prefs(&prefs);
    projects_get()
}

#[tauri::command]
fn projects_select(path: String) -> AppPrefs {
    let mut prefs = load_prefs();
    if prefs.projects.contains(&path) {
        // 切进来时顺手做无损隔离修复:未初始化且祖先无数据的项目在这里自成一根,
        // 用户完全无感。有数据的仍不动,由界面告警引导。
        ensure_project_isolated(Path::new(&path));
        prefs.current = Some(path);
    }
    save_prefs(&prefs);
    prefs
}

/// Windows canonicalize 会带 \\?\ 前缀,展示前剥掉。
fn strip_verbatim(p: PathBuf) -> String {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

#[tauri::command]
fn list_pending_inputs(
    project_dir: String,
    process_id: Option<String>,
) -> Result<Vec<kanzei_core::AdmittedInput>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let state_path = kanzei_core::project_state_path(&root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let session_id = process_session_id(&root, process_id.as_deref());
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    store
        .list_pending_inputs(&session_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_input(
    project_dir: String,
    input_id: String,
    process_id: Option<String>,
) -> Result<bool, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let state_path = kanzei_core::project_state_path(&root);
    let store = kanzei_core::SessionStore::open(&state_path).map_err(|e| e.to_string())?;
    let session_id = process_session_id(&root, process_id.as_deref());
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    let cancelled = store
        .cancel_input(&session_id, &input_id)
        .map_err(|error| error.to_string())?;
    if cancelled {
        store
            .append_event(
                &session_id,
                "prompt.cancelled",
                &json!({ "input_id": input_id }),
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(cancelled)
}

#[tauri::command]
fn workspace_snapshot() -> Result<serde_json::Value, String> {
    let prefs = projects_get();
    let mut projects = Vec::new();
    for path in &prefs.projects {
        // 与运行侧同源的项目根,否则工作区卡片的状态/历史与实际运行会话对不上(D-058)。
        let root = normalized_project_root(Path::new(path));
        let session_id = kanzei_core::project_session_id(&root);
        let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
            .map_err(|e| e.to_string())?;
        let session = store.create_session(&session_id, &root.display().to_string(), None)
            .map_err(|e| e.to_string())?;
        let conversations = conversation_list(path.clone(), None).unwrap_or_default();
        let pending = list_pending_inputs(path.clone(), None).unwrap_or_default();
        let recent = conversation_trace_get(path.clone(), None, None).unwrap_or_default();
        projects.push(json!({
            "path": path,
            "name": prefs.names.get(path).cloned().unwrap_or_else(|| base_name(path)),
            "current": prefs.current.as_deref() == Some(path.as_str()),
            "status": session.status,
            "updated_at": session.updated_at,
            "pending_count": pending.len(),
            "conversation": conversations.first(),
            "recent_activity": recent.into_iter().rev().take(8).collect::<Vec<_>>(),
        }));
    }
    Ok(json!({ "current": prefs.current, "projects": projects }))
}
// ---------- 项目文档 ----------

#[tauri::command]
fn docs_snapshot(project_dir: String) -> serde_json::Value {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    // 自动归档:终态条目移入 *-archive.md,侧边栏与 agent 上下文只剩进行中的。
    for kind in [&REQUIREMENTS, &DEFECTS, &GOALS] {
        let _ = DocStore::open(&root, kind).archive_terminal();
    }
    let archived = |kind: &'static kanzei_tools::docstore::DocKind| -> usize {
        DocStore::open(&root, kind)
            .load_archive()
            .map_or(0, |a| a.len())
    };
    let archived_entries = |kind: &'static kanzei_tools::docstore::DocKind| -> Vec<serde_json::Value> {
        DocStore::open(&root, kind)
            .load_archive()
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "title": e.title,
                    "status": e.status,
                    "severity": e.severity,
                    "fields": e.fields,
                    "closed": true,
                })
            })
            .collect()
    };
    let load = |kind: &'static kanzei_tools::docstore::DocKind| -> Vec<serde_json::Value> {
        let store = DocStore::open(&root, kind);
        let entries = store.load().unwrap_or_default();
        let scheduled: Vec<(kanzei_tools::docstore::Entry, Vec<String>)> =
            if kind.rel_path == REQUIREMENTS.rel_path || kind.rel_path == DEFECTS.rel_path {
                schedule_for_display(&ToolCtx::new(root.clone()), kind, &entries)
                    .map(|items| {
                        items
                            .into_iter()
                            .map(|item| (item.entry, item.block_reasons))
                            .collect()
                    })
                    .unwrap_or_else(|_| entries.iter().cloned().map(|entry| (entry, Vec::new())).collect())
            } else {
                entries.iter().cloned().map(|entry| (entry, Vec::new())).collect()
            };
        scheduled
            .into_iter()
            .map(|(e, block_reasons)| {
                json!({
                    "id": e.id,
                    "title": e.title,
                    "status": e.status,
                    "severity": e.severity,
                    "priority": e.fields.iter()
                        .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
                        .map(|(_, value)| value),
                    // R-051:复杂度(小/中/大),缺失前端显示"未评估"。
                    "complexity": e.fields.iter()
                        .find(|(key, _)| key == "复杂度" || key.eq_ignore_ascii_case("complexity"))
                        .map(|(_, value)| value),
                    "closed": kind.terminal.contains(&e.status.as_str()),
                    "blocked": !block_reasons.is_empty(),
                    "block_reasons": block_reasons,
                    "fields": e.fields,
                    // 展开面板需要:合法的下一步状态(硬门禁同款规则)。
                    "nextStatuses": kind.statuses.iter()
                        .filter(|s| {
                            **s != e.status
                                && DocStore::open(&root, kind).transition_allowed(&e.status, s).is_ok()
                        })
                        .collect::<Vec<_>>(),
                })
            })
            .collect()
    };
    let conventions_path = root.join(CONVENTIONS_REL);
    let conventions = match std::fs::read_to_string(&conventions_path) {
        Ok(text) => json!({
            "exists": true,
            "headings": text.lines()
                .filter(|l| l.starts_with('#'))
                .map(|l| l.trim_start_matches('#').trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>(),
        }),
        Err(_) => json!({ "exists": false, "headings": [] }),
    };
    json!({
        "conventions": conventions,
        "root": root.display().to_string(),
        "requirements": load(&REQUIREMENTS),
        "defects": load(&DEFECTS),
        "goals": load(&GOALS),
        "sources": load(&SOURCES),
        "findings": load(&FINDINGS),
        "archived": {
            "req": archived(&REQUIREMENTS),
            "defect": archived(&DEFECTS),
            "goal": archived(&GOALS),
            "source": archived(&SOURCES),
            "finding": archived(&FINDINGS),
        },
        "archived_entries": {
            "req": archived_entries(&REQUIREMENTS),
            "defect": archived_entries(&DEFECTS),
            "goal": archived_entries(&GOALS),
            "source": archived_entries(&SOURCES),
            "finding": archived_entries(&FINDINGS),
        },
    })
}

#[tauri::command]
fn test_runs_snapshot(project_dir: String) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    kanzei_tools::test_record::test_runs_snapshot(&root)
}

#[tauri::command]
fn test_run_record(
    project_dir: String,
    title: String,
    status: String,
    command: Option<String>,
    summary: Option<String>,
) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    kanzei_tools::test_record::append_test_run(
        &root,
        &title,
        &status,
        command.as_deref(),
        summary.as_deref(),
    )
}

fn collect_project_files(root: &Path, dir: &Path, query: &str, results: &mut Vec<String>) {
    if results.len() >= 50 { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if results.len() >= 50 { break; }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if matches!(name.as_str(), ".git" | ".kanzei" | "target" | "node_modules") { continue; }
            collect_project_files(root, &path, query, results);
        } else if path.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            if query.is_empty() || relative.to_ascii_lowercase().contains(&query.to_ascii_lowercase()) {
                results.push(relative);
            }
        }
    }
}

#[tauri::command]
fn project_files(project_dir: String, query: String) -> Result<Vec<String>, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    if !root.is_dir() { return Err(format!("项目目录不存在: {}", root.display())); }
    let mut results = Vec::new();
    collect_project_files(&root, &root, query.trim(), &mut results);
    Ok(results)
}
// ---------- 设置(全局 kanzei.toml 表单) ----------

fn global_config_path() -> PathBuf {
    kanzei_harness::kanzei_home()
        .unwrap_or_default()
        .join("kanzei.toml")
}

#[tauri::command]
fn settings_get(project_dir: Option<String>) -> serde_json::Value {
    let path = global_config_path();
    let mut config: KanzeiConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    config.fill_defaults();
    let providers: Vec<serde_json::Value> = config
        .providers
        .iter()
        .map(|(name, p)| {
            // 直填 key 优先;否则看 env 是否已设。
            let key_present = if p.api_key.as_deref().is_some_and(|k| !k.trim().is_empty()) {
                Some(true)
            } else {
                p.api_key_env
                    .as_deref()
                    .map(|env| std::env::var(env).map(|v| !v.is_empty()).unwrap_or(false))
            };
            json!({
                "name": name,
                "protocol": p.protocol,
                "baseUrl": p.base_url,
                "apiKeyEnv": p.api_key_env,
                "apiKey": p.api_key,
                "keyPresent": key_present,
                "auth": p.auth,
                "contextLimit": p.context_limit,
            })
        })
        .collect();
    // D-168:本页编辑的是全局文件,但运行时用的是 全局+项目 合并的结果。
    // 项目级 kanzei.toml 一旦也设了 models,这张表单显示的值就不生效——
    // 而此前完全没有提示,表现为"我改了保存了,跑起来还是旧的"。
    let effective = project_dir
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .and_then(|root| KanzeiConfig::load(&root).ok())
        .map(|merged| {
            json!({
                "primary": merged.models.primary,
                "fast": merged.models.fast,
                "reasoning": merged.models.reasoning,
            })
        });
    json!({
        "path": path.display().to_string(),
        "primary": config.models.primary,
        "fast": config.models.fast,
        "proxy": config.proxy.unwrap_or_else(|| "env".into()),
        "profileDefault": config.profile.default.unwrap_or_else(|| "dev".into()),
        "reasoning": config.models.reasoning.unwrap_or_else(|| "off".into()),
        "providers": providers,
        // 合并后的实际生效值;与上面的全局值不同就说明被项目级覆盖了。
        "effective": effective,
        "projectConfig": project_dir
            .as_deref()
            .and_then(|d| kanzei_harness::config::discover_project_root(Path::new(d)))
            .map(|root| root.join(".kanzei").join("kanzei.toml").display().to_string()),
    })
}

fn project_permission_config(project_dir: &str) -> PathBuf {
    kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| PathBuf::from(project_dir))
        .join(".kanzei")
        .join("kanzei.toml")
}

#[tauri::command]
fn permission_rules_get(project_dir: String) -> Result<serde_json::Value, String> {
    let path = project_permission_config(&project_dir);
    let config: KanzeiConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    let rules = config
        .permissions
        .rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.effect == kanzei_harness::permission::Effect::Allow)
        .map(|(index, rule)| json!({
            "index": index,
            "action": rule.action,
            "resource": rule.resource,
            "effect": rule.effect,
        }))
        .collect::<Vec<_>>();
    Ok(json!({ "path": path.display().to_string(), "rules": rules }))
}

#[tauri::command]
fn permission_rule_delete(project_dir: String, index: usize) -> Result<(), String> {
    let path = project_permission_config(&project_dir);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("读取权限规则失败: {error}"))?;
    let mut config: KanzeiConfig = toml::from_str(&text)
        .map_err(|error| format!("配置格式错误: {error}"))?;
    let Some(rule) = config.permissions.rules.get(index) else {
        return Err("权限规则不存在或已被删除".into());
    };
    if rule.effect != kanzei_harness::permission::Effect::Allow {
        return Err("只能删除已记住的放行规则".into());
    }
    config.permissions.rules.remove(index);
    let text = toml::to_string_pretty(&config).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("写入权限规则失败: {error}"))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPayload {
    primary: String,
    fast: String,
    proxy: String,
    /// 思考强度默认档:off/low/medium/high;缺省视为 off。
    #[serde(default)]
    reasoning: Option<String>,
    profile_default: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    providers: Vec<ProviderPayload>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderPayload {
    name: String,
    protocol: String,
    base_url: String,
    api_key_env: Option<String>,
    /// 直填 key(优先于 env;明文存 toml)。
    #[serde(default)]
    api_key: Option<String>,
    /// 特殊认证透传(codex);表单只读展示,不丢字段。
    #[serde(default)]
    auth: Option<String>,
    #[serde(default)]
    context_limit: Option<u64>,
}

/// 设置标量并保留原值上的空白/行尾注释装饰(注释挂在值上,直接赋值会连注释一起换掉)。
fn settings_set_value(table: &mut toml_edit::Table, key: &str, value: impl Into<toml_edit::Value>) {
    let mut value: toml_edit::Value = value.into();
    if let Some(existing) = table.get(key).and_then(|item| item.as_value()) {
        *value.decor_mut() = existing.decor().clone();
    }
    table[key] = toml_edit::Item::Value(value);
}

/// 表单键写入:Some 设置,None 移除(默认值不写进文件,保持配置精简)。
fn settings_set_or_remove(table: &mut toml_edit::Table, key: &str, value: Option<String>) {
    match value {
        Some(v) => settings_set_value(table, key, v),
        None => {
            table.remove(key);
        }
    }
}

/// "缺省即默认"的键(proxy/reasoning/profile.default):回落默认时若键已存在,
/// 写显式默认值而不是删除——toml_edit 删键会连带删掉挂在键上的用户注释。
fn settings_set_or_reset(
    table: &mut toml_edit::Table,
    key: &str,
    value: Option<String>,
    default_value: &str,
) {
    match value {
        Some(v) => settings_set_value(table, key, v),
        None if table.contains_key(key) => {
            settings_set_value(table, key, default_value.to_string());
        }
        None => {}
    }
}

fn settings_table<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    name: &str,
) -> Result<&'a mut toml_edit::Table, String> {
    doc.entry(name)
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| format!("配置节 `{name}` 不是表,无法保存设置"))
}

/// 保存前校验模型角色:`provider:model` 里的 provider 必须确实配了。
/// 拼错一个字母,此前要到真正发消息时才炸一句 "unknown provider",
/// 而那时用户早已离开设置页,根本联系不到是刚才填错了(D-168)。
fn validate_model_roles(payload: &SettingsPayload) -> Result<(), String> {
    let mut probe = KanzeiConfig::default();
    for p in &payload.providers {
        probe.providers.insert(
            p.name.trim().to_string(),
            kanzei_harness::config::ProviderConfig {
                protocol: p.protocol.clone(),
                base_url: p.base_url.clone(),
                api_key_env: p.api_key_env.clone(),
                api_key: p.api_key.clone(),
                auth: p.auth.clone(),
                context_limit: p.context_limit,
            },
        );
    }
    probe.fill_defaults();
    for (role, value) in [("primary", &payload.primary), ("fast", &payload.fast)] {
        let spec = value.trim();
        if spec.is_empty() {
            continue;
        }
        probe.resolve_model(spec).map_err(|e| {
            format!(
                "{role} 的模型 `{spec}` 无法解析:{e}。格式应为 provider:model,\
                 且 provider 必须是下面 Provider 表里的名称。"
            )
        })?;
    }
    Ok(())
}

fn settings_save_at_path(payload: SettingsPayload, path: &Path) -> Result<(), String> {
    validate_model_roles(&payload)?;
    // 以现有配置文本为底,只改设置页管理的键:注释、排版、未知字段原样保留(D-082)。
    // 文件存在但解析失败必须报错——静默回退默认值再覆写等于销毁用户配置。
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("读取配置失败 {}: {e}", path.display())),
    };
    toml::from_str::<KanzeiConfig>(&text)
        .map_err(|e| format!("现有配置无法解析,拒绝覆盖保存 {}: {e}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("现有配置无法解析,拒绝覆盖保存 {}: {e}", path.display()))?;

    let models = settings_table(&mut doc, "models")?;
    settings_set_or_remove(
        models,
        "primary",
        Some(payload.primary.trim().to_string()).filter(|s| !s.is_empty()),
    );
    settings_set_or_remove(
        models,
        "fast",
        Some(payload.fast.trim().to_string()).filter(|s| !s.is_empty()),
    );
    settings_set_or_reset(
        models,
        "reasoning",
        payload
            .reasoning
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| ["low", "medium", "high"].contains(&value.as_str())),
        "off",
    );

    settings_set_or_reset(
        doc.as_table_mut(),
        "proxy",
        match payload.proxy.trim() {
            "" | "env" => None,
            other => Some(other.to_string()),
        },
        "env",
    );

    let profile = settings_table(&mut doc, "profile")?;
    profile.set_implicit(true);
    settings_set_or_reset(
        profile,
        "default",
        payload
            .profile_default
            .or(payload.profile)
            .filter(|p| p == "dev" || p == "research"),
        "dev",
    );

    let providers = settings_table(&mut doc, "providers")?;
    providers.set_implicit(true);
    for p in payload.providers {
        let name = p.name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let Some(provider) = providers
            .entry(&name)
            .or_insert(toml_edit::table())
            .as_table_mut()
        else {
            return Err(format!("配置节 `providers.{name}` 不是表,无法保存设置"));
        };
        settings_set_value(provider, "protocol", p.protocol.trim().to_string());
        settings_set_value(
            provider,
            "base_url",
            p.base_url.trim().trim_end_matches('/').to_string(),
        );
        settings_set_or_remove(
            provider,
            "api_key_env",
            p.api_key_env.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        );
        settings_set_or_remove(
            provider,
            "api_key",
            p.api_key.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        );
        settings_set_or_remove(provider, "auth", p.auth.filter(|s| !s.is_empty()));
        match p.context_limit {
            Some(limit) => settings_set_value(provider, "context_limit", limit as i64),
            None => {
                provider.remove("context_limit");
            }
        }
    }

    let text = doc.to_string();
    // 写盘前自校验:引擎绝不产出自己读不回来的配置。
    toml::from_str::<KanzeiConfig>(&text)
        .map_err(|e| format!("保存结果自校验失败,已放弃写入: {e}"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command]
fn settings_save(payload: SettingsPayload) -> Result<(), String> {
    settings_save_at_path(payload, &global_config_path())
}

#[tauri::command]
fn settings_open() -> Result<(), String> {
    let path = global_config_path();
    if !path.is_file() {
        settings_save(SettingsPayload {
            primary: String::new(),
            fast: String::new(),
            proxy: "env".into(),
            reasoning: None,
            profile_default: None,
            profile: None,
            providers: vec![],
        })?;
    }
    hidden_command("cmd")
        .args(["/c", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ExportOptions {
    project_dir: String,
    output_dir: String,
    include_memory: bool,
    include_requirements: bool,
    include_defects: bool,
    include_config: bool,
}

fn copy_export_file(root: &Path, destination: &Path, relative: &str, files: &mut Vec<String>) -> Result<(), String> {
    let source = root.join(relative);
    if !source.is_file() {
        return Ok(());
    }
    let target = destination.join(relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败: {e}"))?;
    }
    std::fs::copy(&source, &target).map_err(|e| format!("导出 {} 失败: {e}", source.display()))?;
    files.push(relative.replace('\\', "/"));
    Ok(())
}

fn copy_export_tree(source: &Path, destination: &Path, relative: &str, files: &mut Vec<String>) -> Result<(), String> {
    if !source.is_dir() {
        return Ok(());
    }
    for item in std::fs::read_dir(source).map_err(|e| format!("读取导出目录失败: {e}"))? {
        let item = item.map_err(|e| format!("读取导出条目失败: {e}"))?;
        let child_relative = Path::new(relative).join(item.file_name());
        let child_source = item.path();
        if child_source.is_dir() {
            copy_export_tree(&child_source, destination, &child_relative.display().to_string(), files)?;
        } else if child_source.is_file() {
            let relative_text = child_relative.display().to_string();
            let target = destination.join(&child_relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败: {e}"))?;
            }
            std::fs::copy(&child_source, &target)
                .map_err(|e| format!("导出 {} 失败: {e}", child_source.display()))?;
            files.push(relative_text.replace('\\', "/"));
        }
    }
    Ok(())
}

#[tauri::command]
async fn export_pick_dir() -> Result<Option<String>, String> {
    Ok(rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|handle| handle.path().display().to_string()))
}

#[tauri::command]
fn export_project_data(options: ExportOptions) -> Result<serde_json::Value, String> {
    let root = normalized_project_root(Path::new(&options.project_dir));
    let output_base = PathBuf::from(options.output_dir.trim());
    if output_base.as_os_str().is_empty() {
        return Err("请先选择导出目录".into());
    }
    std::fs::create_dir_all(&output_base).map_err(|e| format!("创建导出目录失败: {e}"))?;
    let root_canonical = root.canonicalize().map_err(|e| format!("项目目录无法解析: {e}"))?;
    let output_canonical = output_base
        .canonicalize()
        .map_err(|e| format!("导出目录无法解析: {e}"))?;
    if output_canonical.starts_with(&root_canonical) {
        return Err("导出目录不能位于项目目录内".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let destination = output_canonical.join(format!("kanzei-export-{stamp}"));
    std::fs::create_dir_all(&destination).map_err(|e| format!("创建导出包目录失败: {e}"))?;
    let mut files = Vec::new();
    if options.include_memory {
        copy_export_tree(&root.join(".kanzei/memory"), &destination, ".kanzei/memory", &mut files)?;
    }
    if options.include_requirements {
        for relative in [
            ".kanzei/project/requirements.md",
            ".kanzei/project/requirements-archive.md",
        ] {
            copy_export_file(&root, &destination, relative, &mut files)?;
        }
    }
    if options.include_defects {
        for relative in [".kanzei/project/defects.md", ".kanzei/project/defects-archive.md"] {
            copy_export_file(&root, &destination, relative, &mut files)?;
        }
    }
    if options.include_config {
        copy_export_file(&root, &destination, ".kanzei/kanzei.toml", &mut files)?;
    }
    if files.is_empty() {
        let _ = std::fs::remove_dir_all(&destination);
        return Err("没有可导出的工作资料".into());
    }
    files.sort();
    Ok(json!({ "path": destination.display().to_string(), "files": files }))
}

/// R-126:让 agent 能自查真实运行中的界面。改完前端只跑 node --check 检不出
/// 渲染问题——D-164(编辑表单渲染成一片无标题输入框)、D-165(同一字段渲染两遍)
/// 都是语法完全正确却明显不对的例子,只有看真实渲染结果才发现得了。
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UiProbeInput {
    /// CSS 选择器,如 `#req-list .doc-item`。dom / style 需要,console 忽略。
    #[serde(default)]
    selector: Option<String>,
}

struct UiDomTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiDomTool {
    fn name(&self) -> &'static str {
        "ui_dom"
    }
    fn description(&self) -> String {
        "读取当前运行中窗口里匹配选择器的 DOM 子树(标签、class、可见文本、层级)。\
         改完前端用它确认渲染结果——node --check 只查语法,查不出渲染成什么样。只读。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap()
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        let input: UiProbeInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(e) => return kanzei_harness::ToolOutput::error(format!("invalid input: {e}")),
        };
        let Some(selector) = input.selector.as_deref().filter(|s| !s.trim().is_empty()) else {
            return kanzei_harness::ToolOutput::error("需要 selector");
        };
        match ui_probe("dom", selector).await {
            Ok(value) => kanzei_harness::ToolOutput::ok(
                value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()),
            ),
            Err(e) => kanzei_harness::ToolOutput::error(e),
        }
    }
}

struct UiConsoleTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiConsoleTool {
    fn name(&self) -> &'static str {
        "ui_console"
    }
    fn description(&self) -> String {
        "读取当前窗口自加载以来累积的 console 错误与警告(含未捕获异常)。\
         前端改动后必查:ReferenceError 一类问题不会让页面白屏,只会让某一块悄悄失效。只读。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap()
    }
    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        match ui_probe("console", "").await {
            Ok(value) => kanzei_harness::ToolOutput::ok(
                value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()),
            ),
            Err(e) => kanzei_harness::ToolOutput::error(e),
        }
    }
}

struct UiStyleTool;
#[async_trait::async_trait]
impl kanzei_harness::Tool for UiStyleTool {
    fn name(&self) -> &'static str {
        "ui_style"
    }
    fn description(&self) -> String {
        "读取匹配元素的计算样式与盒模型(display/位置/尺寸/关键布局属性)。\
         用来判断「为什么它没显示出来」「为什么挤成一团」,比猜 CSS 快得多。只读。"
            .into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(UiProbeInput)).unwrap()
    }
    async fn execute(&self, input: serde_json::Value, _ctx: &ToolCtx) -> kanzei_harness::ToolOutput {
        let input: UiProbeInput = match serde_json::from_value(input) {
            Ok(value) => value,
            Err(e) => return kanzei_harness::ToolOutput::error(format!("invalid input: {e}")),
        };
        let Some(selector) = input.selector.as_deref().filter(|s| !s.trim().is_empty()) else {
            return kanzei_harness::ToolOutput::error("需要 selector");
        };
        match ui_probe("style", selector).await {
            Ok(value) => kanzei_harness::ToolOutput::ok(
                value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()),
            ),
            Err(e) => kanzei_harness::ToolOutput::error(e),
        }
    }
}

/// 前端自查与定位工具集(R-126)。全部只读,受既有权限契约约束。
struct FrontendToolsComponent;
impl kanzei_harness::Component for FrontendToolsComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        _ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
        draft.tools.insert("ui_dom", Arc::new(UiDomTool));
        draft.tools.insert("ui_console", Arc::new(UiConsoleTool));
        draft.tools.insert("ui_style", Arc::new(UiStyleTool));
        draft
            .tools
            .insert("frontend_locate", Arc::new(kanzei_tools::frontend::FrontendLocateTool));
        draft
            .tools
            .insert("frontend_check", Arc::new(kanzei_tools::frontend::FrontendCheckTool));
        // 只读工具无需逐次确认;它们不写任何东西,权限风险为零。
        for name in ["ui_dom", "ui_console", "ui_style", "frontend_locate", "frontend_check"] {
            draft
                .permissions
                .push(kanzei_harness::rule(name, "*", kanzei_harness::Effect::Allow));
        }
        Ok(())
    }
}

/// R-053 快速记录:只挂单个 tracker 工具的最小组件(独立迷你 run 专用)。
struct QuickCaptureComponent {
    capture: &'static str, // "req" | "defect"
}
impl kanzei_harness::Component for QuickCaptureComponent {
    fn contribute(
        &self,
        draft: &mut kanzei_harness::HarnessDraft,
        _ctx: &ResolveCtx,
    ) -> anyhow::Result<()> {
        let tool = if self.capture == "defect" {
            kanzei_tools::tracker::TrackerTool {
                tool_name: "defect",
                noun: "defect",
                kind: &DEFECTS,
                requires_refs: None,
            }
        } else {
            kanzei_tools::tracker::TrackerTool {
                tool_name: "req",
                noun: "requirement",
                kind: &REQUIREMENTS,
                requires_refs: None,
            }
        };
        let name = tool.tool_name;
        draft.tools.insert(name, Arc::new(tool));
        draft
            .permissions
            .push(kanzei_harness::rule(name, "*", kanzei_harness::Effect::Allow));
        Ok(())
    }
}

const DEFECT_REVIEW_SYSTEM: &str = "You are a read-only defect review agent. You only have read, glob, and grep. \
Read .kanzei/project/defects.md first, then verify every active defect against relevant code, tests, and design documents. \
Reply in Chinese Markdown with: 1. summary and active defect count; 2. categories; 3. likely duplicates with IDs; \
4. impact of each defect; 5. suggested priority with reasons; 6. verifiable evidence using exact file paths, functions, \
and line numbers; 7. concrete next steps. Do not modify files, run commands, update trackers, or claim unverified facts.";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DefectReviewResult {
    empty: bool,
    report: String,
    defect_count: usize,
}

fn defect_review_snapshot(
    rctx: &ResolveCtx,
) -> anyhow::Result<Arc<kanzei_harness::HarnessSnapshot>> {
    let mut harness = Harness::default();
    harness
        .add(kanzei_tools::SubagentBase)
        .add(ConfigComponent);
    harness.resolve(rctx)
}

fn defect_review_report(summary: &kanzei_core::RunSummary) -> Result<String, String> {
    let report = summary.text.trim();
    if report.is_empty() {
        Err("审查模型没有返回报告".into())
    } else {
        Ok(report.to_string())
    }
}

/// R-092:独立只读缺陷审查。它不进入主 conversation/queue，也不持有任何写工具；
/// fast 失败后回退 primary，结果直接返回前端的 Markdown 查看器。
#[tauri::command]
async fn defect_review(project_dir: String) -> Result<DefectReviewResult, String> {
    let cwd = PathBuf::from(&project_dir);
    let config = Arc::new(KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let defects = DocStore::open(&project_root, &DEFECTS)
        .load()
        .map_err(|e| e.to_string())?;
    if defects.is_empty() {
        return Ok(DefectReviewResult {
            empty: true,
            report: "当前没有活动缺陷。".into(),
            defect_count: 0,
        });
    }

    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let snapshot = defect_review_snapshot(&rctx).map_err(|e| e.to_string())?;
    let mut agent = kanzei_tools::explore_agent();
    agent.name = "defect-review".into();
    agent.system = DEFECT_REVIEW_SYSTEM.into();
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(value) => ProxyConfig::Explicit(value.to_string()),
    };
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let tool_ctx = ToolCtx {
        cwd,
        project_root,
    };
    let prompt = format!(
        "审查当前项目 defects.md 中的 {} 条活动缺陷。逐条核对真实代码、测试和调用方，输出约定的 Markdown 报告。",
        defects.len()
    );
    let mut last_error = "没有可用的 fast 或 primary 模型".to_string();
    for role in ["fast", "primary"] {
        let resolved = match config.resolve_model(role) {
            Ok(value) => value,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let route = match kanzei_core::build_route(&resolved, &proxy).await {
            Ok(value) => value,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let runner_config = RunnerConfig {
            model: resolved.model,
            max_tokens: 8192,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            context_limit: resolved.provider.context_limit,
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
            Box::pin(async move {
                match request {
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        match run_once_with_parts(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &tool_ctx,
            &prompt,
            &[],
            None,
            None,
            &mut on_event,
            &mut ask,
        )
        .await
        {
            Ok(summary) => match defect_review_report(&summary) {
                Ok(report) => {
                    return Ok(DefectReviewResult {
                        empty: false,
                        report,
                        defect_count: defects.len(),
                    });
                }
                Err(error) => last_error = error,
            },
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(format!("缺陷自动审查失败:{last_error}"))
}

/// R-053:自然语言描述 → 独立子代理结构化落库。与主对话完全并行,
/// 不碰 conversation/queue/lifecycle;fast 落库失败自动升级 primary 重试一次。
#[tauri::command]
async fn quick_req(
    project_dir: String,
    description: String,
    kind: Option<String>,
) -> Result<String, String> {
    let description = description.trim().to_string();
    if description.is_empty() {
        return Err("描述不能为空".into());
    }
    let capture: &'static str = match kind.as_deref() {
        Some("defect") => "defect",
        _ => "req",
    };
    let cwd = PathBuf::from(&project_dir);
    let config = Arc::new(KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let mut harness = Harness::default();
    harness.add(QuickCaptureComponent { capture });
    let snapshot = harness.resolve(&rctx).map_err(|e| e.to_string())?;
    let system = if capture == "defect" {
        // D-205:复现字段禁止编造。旧文案只说 "if inferable",推断不出时模型的默认
        // 行为就是硬挤一个伪复现(实例 D-204:「复现: 查看 SOP 时」),伪复现看起来
        // 像真的,下游拿着它开工只会猜错或空转,还没人知道该回去问用户。
        "You capture ONE defect from the user's natural-language description. Call the \
         `defect` tool exactly once with action \"add\": a concise title (<=40 chars, \
         Chinese preferred, keep qualifier words like 用户/桌面端/CLI from the original), \
         severity high|medium|low, fields = {\"复现\": concrete reproduction steps ONLY \
         if the description actually contains them — NEVER invent or pad one; when not \
         reproducible from the text, write \"待澄清: \" followed by the specific \
         questions the user must answer, \"原始描述\": the user's original text \
         verbatim}. Then reply with only the new id."
    } else {
        "You capture ONE requirement from the user's natural-language description. Call \
         the `req` tool exactly once with action \"add\": a concise title (<=40 chars, \
         Chinese preferred), fields = {\"priority\": suggested P0-P3, \"复杂度\": 小|中|大, \
         \"验收\": one draft acceptance line, \"归属\": \"kanzei\", \"原始描述\": the \
         user's original text verbatim}. Then reply with only the new id."
    };
    let agent = kanzei_harness::AgentDef {
        name: "quickcapture".into(),
        profile: kanzei_harness::ProfileScope::Dev,
        model: "fast".into(),
        mode: kanzei_harness::AgentMode::Subagent,
        steps: 4,
        system: system.into(),
    };
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let tool_ctx = ToolCtx {
        cwd: cwd.clone(),
        project_root: project_root.clone(),
    };
    let doc_kind = if capture == "defect" { &DEFECTS } else { &REQUIREMENTS };
    let store = DocStore::open(&project_root, doc_kind);
    let before: std::collections::HashSet<String> = store
        .load()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|e| e.id.clone())
        .collect();
    let prompt = format!("描述(原文):\n{description}");
    for role in ["fast", "primary"] {
        let Ok(resolved) = config.resolve_model(role) else {
            continue;
        };
        let Ok(route) = kanzei_core::build_route(&resolved, &proxy).await else {
            continue;
        };
        let runner_config = RunnerConfig {
            model: resolved.model.clone(),
            max_tokens: 2048,
            // 快记是机械结构化,不开思考。
            reasoning: kanzei_llm::ReasoningEffort::Off,
            context_limit: resolved.provider.context_limit,
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
            Box::pin(async move {
                match request {
                    // 快照里只有 req 工具,放行是安全的;问题一律取消(无人应答)。
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        let _ = run_once_with_parts(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &tool_ctx,
            &prompt,
            &[],
            None,
            None,
            &mut on_event,
            &mut ask,
        )
        .await;
        // 成功判据不信模型嘴,只看库:落了新条目才算数。
        let after = store.load().map_err(|e| e.to_string())?;
        if let Some(new_entry) = after.iter().find(|e| !before.contains(&e.id)) {
            return Ok(format!("{} {}", new_entry.id, new_entry.title));
        }
    }
    Err("子代理未能落库(fast/primary 均失败),请重试或在对话里直接说".into())
}

// ---------- Memory 页(R-107):透明化——分级概览/条目/账单/检索/整理 ----------

fn memory_stores_for(project_dir: &str) -> Vec<kanzei_tools::memory::MemoryStore> {
    let cwd = PathBuf::from(project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let mut stores = vec![kanzei_tools::memory::MemoryStore::project(&root)];
    stores.extend(kanzei_tools::memory::MemoryStore::global());
    stores
}

#[tauri::command]
fn memory_overview(project_dir: String) -> serde_json::Value {
    let mut scopes = Vec::new();
    for store in memory_stores_for(&project_dir) {
        let entries = store.load_all();
        let hits = store.hits_map();
        let mut categories = serde_json::Map::new();
        for cat in kanzei_tools::memory::CATEGORIES {
            let of_cat: Vec<_> = entries.iter().filter(|(_, e)| e.category == *cat).collect();
            let active = of_cat.iter().filter(|(_, e)| e.status == "active").count();
            let bytes: usize = of_cat
                .iter()
                .map(|(_, e)| e.body.len() + e.title.len() + e.description.len())
                .sum();
            let last = of_cat
                .iter()
                .map(|(_, e)| e.updated.clone())
                .max()
                .unwrap_or_default();
            categories.insert(
                cat.to_string(),
                json!({
                    "active": active,
                    "stale": of_cat.len() - active,
                    "bytes": bytes,
                    "last": last,
                }),
            );
        }
        scopes.push(json!({
            "scope": store.scope.label(),
            "root": store.root.display().to_string(),
            "total": entries.len(),
            "hitsTotal": hits.values().sum::<u64>(),
            "categories": categories,
            "inboxPending": store.pending_notes(),
            "integrity": store.integrity_issues(),
        }));
    }
    json!({ "scopes": scopes })
}

#[tauri::command]
fn memory_entries(
    project_dir: String,
    scope: String,
    category: Option<String>,
) -> Result<serde_json::Value, String> {
    for store in memory_stores_for(&project_dir) {
        if store.scope.label() == scope {
            let profile = store.hit_profile();
            let list: Vec<serde_json::Value> = store
                .load_all()
                .into_iter()
                .filter(|(_, e)| category.as_deref().is_none_or(|c| e.category == c))
                .map(|(path, e)| {
                    let (hits, last_hit_at) = profile.get(&e.id).copied().unwrap_or((0, 0));
                    json!({
                        "id": e.id,
                        "category": e.category,
                        "title": e.title,
                        "description": e.description,
                        "status": e.status,
                        "updated": e.updated,
                        "source": e.source,
                        "refs": e.refs(),
                        "hits": hits,
                        // R-125 效果画像:最近命中时间为 0 表示从未命中过。
                        "lastHitAt": last_hit_at,
                        "path": path.display().to_string(),
                        "body": e.body,
                    })
                })
                .collect();
            return Ok(json!(list));
        }
    }
    Err(format!("未知记忆域: {scope}"))
}

/// R-099 + R-127:最近若干轮的运行画像,供调试面板按轮次展示与跨轮对比。
/// 与冗余度量共用 `summarize_metrics` 的同一份口径,不另算一套。
#[tauri::command]
fn run_metrics(project_dir: String, limit: Option<usize>) -> Result<serde_json::Value, String> {
    let root = PathBuf::from(&project_dir);
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let session_id = kanzei_core::project_session_id(&root);
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let rows = store
        .recent_episodes(&session_id, limit)
        .map_err(|e| e.to_string())?;
    let rounds: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(at, prompt, outcome, steps, input, output, tools, context, metrics)| {
            let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap_or(json!({}));
            let metrics_value = parse(&metrics);
            json!({
                "at": at,
                "prompt": prompt,
                "outcome": outcome,
                "steps": steps,
                "inputTokens": input,
                "outputTokens": output,
                "tools": parse(&tools),
                "context": parse(&context),
                "metrics": metrics_value,
                // 空对象代表那一轮早于度量落地,前端要能与"全零"区分开。
                "measured": metrics.trim() != "{}" && !metrics.trim().is_empty(),
            })
        })
        .collect();
    Ok(json!({ "rounds": rounds }))
}

/// R-124:待用户处置的草稿候选(重点是 SOP —— 它是用户的常用模板,
/// 不能由 agent 自己决定入库)。两级记忆合并返回。
#[tauri::command]
fn memory_note_candidates(project_dir: String) -> serde_json::Value {
    let mut out = Vec::new();
    for store in memory_stores_for(&project_dir) {
        for (hint, summary, detail) in store.pending_note_list() {
            // 指纹是丢弃时的定位键,从摘要里原样取出。
            let fingerprint = summary
                .rfind('[')
                .and_then(|i| summary[i..].find(']').map(|j| summary[i..i + j + 1].to_string()))
                .unwrap_or_default();
            out.push(json!({
                "scope": store.scope.label(),
                "hint": hint,
                "summary": summary,
                "detail": detail,
                "fingerprint": fingerprint,
            }));
        }
    }
    json!(out)
}

#[tauri::command]
fn memory_note_discard(project_dir: String, scope: String, fingerprint: String) -> Result<bool, String> {
    for store in memory_stores_for(&project_dir) {
        if store.scope.label() == scope {
            return store.discard_note(&fingerprint).map_err(|e| e.to_string());
        }
    }
    Err(format!("未知记忆域: {scope}"))
}

/// R-125:最近若干轮的召回明细(两级记忆合并,新的在前)。
#[tauri::command]
fn memory_recalls(project_dir: String, limit: Option<usize>) -> serde_json::Value {
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let mut rounds: Vec<kanzei_tools::memory::RecallRound> = Vec::new();
    for store in memory_stores_for(&project_dir) {
        rounds.extend(store.recalls(limit));
    }
    rounds.sort_by(|a, b| b.at.cmp(&a.at));
    rounds.truncate(limit);
    let total: usize = rounds.len();
    let with_fetch = rounds
        .iter()
        .filter(|r| r.hits.iter().any(|h| h.fetched))
        .count();
    json!({
        "rounds": rounds,
        // 采纳率 = 至少有一条召回内容被真正拉取的轮次占比。这是「记忆有没有用」
        // 唯一机械可判的口径:只注入索引行,不拉正文就等于没用上。
        "rounds_total": total,
        "rounds_with_fetch": with_fetch,
    })
}

#[tauri::command]
fn memory_entry_save(
    project_dir: String,
    scope: String,
    id: String,
    title: Option<String>,
    description: Option<String>,
    body: Option<String>,
    status: Option<String>,
) -> Result<(), String> {
    for store in memory_stores_for(&project_dir) {
        if store.scope.label() == scope {
            store
                .update(
                    &id,
                    title.as_deref(),
                    description.as_deref(),
                    body.as_deref(),
                    status.as_deref(),
                )
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err(format!("未知记忆域: {scope}"))
}

/// R-125:删除一条记忆。stale 只是降权,仍占索引与列表;长期零命中的条目要能真正清掉。
/// 文件是真源,删文件后重建派生索引即可——不做软删除,避免"删了还在"的困惑。
#[tauri::command]
fn memory_entry_delete(project_dir: String, scope: String, id: String) -> Result<(), String> {
    for store in memory_stores_for(&project_dir) {
        if store.scope.label() == scope {
            let Some((path, _)) = store.load_all().into_iter().find(|(_, e)| e.id == id) else {
                return Err(format!("记忆 {id} 不存在(可能已被删除)"));
            };
            std::fs::remove_file(&path).map_err(|e| format!("删除 {} 失败: {e}", path.display()))?;
            store.refresh_derived().map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err(format!("未知记忆域: {scope}"))
}

#[tauri::command]
fn memory_search_page(project_dir: String, query: String) -> serde_json::Value {
    let mut out = Vec::new();
    for store in memory_stores_for(&project_dir) {
        if let Ok(found) = store.search(&query, None, None, 8) {
            for h in found {
                out.push(json!({
                    "id": h.entry.id,
                    "scope": h.entry.scope,
                    "category": h.entry.category,
                    "title": h.entry.title,
                    "description": h.entry.description,
                    "status": h.entry.status,
                    "snippet": h.snippet,
                    "hits": h.hits,
                }));
            }
        }
    }
    json!(out)
}

/// 开发重心偏好条目的标题前缀:切换与手改共用同一条记忆,不新增。
const FOCUS_TITLE_PREFIX: &str = "开发重心";

#[tauri::command]
fn memory_focus_get(project_dir: String) -> serde_json::Value {
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let store = kanzei_tools::memory::MemoryStore::project(&root);
    match store.find_preference(FOCUS_TITLE_PREFIX) {
        Some(entry) => json!({
            "id": entry.id,
            "title": entry.title,
            "body": entry.body,
            "updated": entry.updated,
        }),
        None => serde_json::Value::Null,
    }
}

/// 取活重心写入 preference 记忆(用户直写路径)。真源是记忆文件——
/// 提示词由它生成,所以开关与提示词不可能再互相矛盾(D-128)。
#[tauri::command]
fn memory_focus_set(
    project_dir: String,
    title: String,
    body: String,
) -> Result<serde_json::Value, String> {
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let store = kanzei_tools::memory::MemoryStore::project(&root);
    let entry = store
        .upsert_preference(
            FOCUS_TITLE_PREFIX,
            title.trim(),
            "取活/排优先级时必读:当前项目该先做什么",
            body.trim(),
        )
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": entry.id, "title": entry.title, "body": entry.body }))
}

#[tauri::command]
fn memory_context_bill(project_dir: String) -> serde_json::Value {
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let state = kanzei_core::project_state_path(&root);
    let session = kanzei_core::project_session_id(&root);
    let Ok(store) = kanzei_core::SessionStore::open(&state) else {
        return json!({ "bill": [], "episodes": [] });
    };
    let bill: serde_json::Value = store
        .latest_episode_context(&session)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!([]));
    let episodes: Vec<serde_json::Value> = store
        .list_episodes(&session, 8)
        .unwrap_or_default()
        .into_iter()
        .map(|(at, prompt, outcome, steps, tools)| {
            json!({
                "at": at,
                "prompt": prompt,
                "outcome": outcome,
                "steps": steps,
                "tools": serde_json::from_str::<serde_json::Value>(&tools).unwrap_or(json!({})),
            })
        })
        .collect();
    json!({ "bill": bill, "episodes": episodes })
}

#[tauri::command]
async fn memory_consolidate(project_dir: String) -> Result<serde_json::Value, String> {
    consolidate_memory_inbox(project_dir.clone()).await;
    let cwd = PathBuf::from(&project_dir);
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let store = kanzei_tools::memory::MemoryStore::project(&root);
    Ok(json!({ "pending": store.pending_notes() }))
}

/// 轮末记忆整理(R-105):把 inbox 草稿交给 memory-manager 迷你 run 消化。
/// 与主会话完全并行(quick_req 同款模式);失败留箱下轮再试,成功判据只看箱。
async fn consolidate_memory_inbox(project_dir: String) {
    let cwd = PathBuf::from(&project_dir);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let store = kanzei_tools::memory::MemoryStore::project(&project_root);
    if store.pending_notes() == 0 {
        return;
    }
    let inbox = store.read_inbox();
    let Ok(config) = KanzeiConfig::load(&cwd) else {
        return;
    };
    let config = Arc::new(config);
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let mut harness = Harness::default();
    harness.add(kanzei_tools::memory::MemoryManagerComponent);
    let Ok(snapshot) = harness.resolve(&rctx) else {
        return;
    };
    let agent = kanzei_tools::memory::manager_agent();
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let Ok(client) = LlmClient::new(&proxy) else {
        return;
    };
    let tool_ctx = ToolCtx {
        cwd: cwd.clone(),
        project_root: project_root.clone(),
    };
    let prompt = format!("Consolidate these inbox notes into durable memory entries:\n\n{inbox}");
    // primary 优先(fast 兜底):记忆注入之后每一轮,写错一条就长期误导。
    // 实测 fast 把失败次数误读成事实("需要约 7 次重试才能成功",M-003 已校正)。
    for role in ["primary", "fast"] {
        let Ok(resolved) = config.resolve_model(role) else {
            continue;
        };
        let Ok(route) = kanzei_core::build_route(&resolved, &proxy).await else {
            continue;
        };
        let runner_config = RunnerConfig {
            model: resolved.model.clone(),
            max_tokens: 4096,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            context_limit: resolved.provider.context_limit,
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
            Box::pin(async move {
                match request {
                    // 快照里只有 memory_* 工具,放行安全;问题一律取消(无人应答)。
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        let _ = run_once_with_parts(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &tool_ctx,
            &prompt,
            &[],
            None,
            None,
            &mut on_event,
            &mut ask,
        )
        .await;
        if store.pending_notes() == 0 {
            return;
        }
    }
}

/// 仅当 release 的发布时间晚于本地构建时间时才允许提示更新。
/// `KANZEI_BUILD_INFO` 的旧格式只有 yyyy-MM-dd,对旧构建采用“必须晚一天”
/// 的保守判定；新格式使用 UTC 的 yyyyMMddHHmmss，避免开发构建被同日 release 覆盖。
fn release_is_newer(current_info: &str, tag: &str, published_at: Option<&str>) -> bool {
    let current_hash = current_info.split_whitespace().next().unwrap_or("dev");
    if current_hash == "dev" || tag.is_empty() || tag.contains(current_hash) {
        return false;
    }
    let Some((local_stamp, date_only)) = build_stamp(current_info) else {
        return false;
    };
    let Some(release_stamp) = published_at.and_then(timestamp_digits) else {
        // 没有可信发布时间时宁可不装，避免把未知版本当成升级。
        return false;
    };
    if date_only {
        release_stamp[..8] > local_stamp[..8]
    } else {
        release_stamp > local_stamp
    }
}

fn build_stamp(info: &str) -> Option<(String, bool)> {
    let token = info.split_whitespace().nth(1)?;
    let digits = timestamp_digits(token)?;
    Some((digits, token.chars().filter(|c| c.is_ascii_digit()).count() < 14))
}

fn timestamp_digits(value: &str) -> Option<String> {
    let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    if digits.len() >= 14 {
        Some(digits[..14].to_string())
    } else {
        Some(format!("{digits:0<14}"))
    }
}

/// 应用内检查更新:比对 GitHub Releases 最新 build 标签与当前构建号。
#[tauri::command]
async fn update_check() -> Result<serde_json::Value, String> {
    let current = option_env!("KANZEI_BUILD_INFO").unwrap_or("dev");
    let current_hash = current.split_whitespace().next().unwrap_or("dev").to_string();
    let config = KanzeiConfig::load(Path::new(".")).unwrap_or_default();
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let client = kanzei_llm::proxy::build_http_client(&proxy).map_err(|e| e.to_string())?;
    let resp = client
        .get("https://api.github.com/repos/kanze1/kanzei-code/releases/latest")
        .header("user-agent", "kanzei-app")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("请求失败:{e}"))?;
    if resp.status().as_u16() == 404 {
        return Ok(json!({ "current": current_hash, "status": "none",
            "message": "还没有发布过安装包(用 scripts/package.ps1 -Publish 发布第一版)" }));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let tag = body["tag_name"].as_str().unwrap_or("").to_string();
    let url = body["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find(|a| {
                a["name"].as_str().is_some_and(|n| n.ends_with(".exe"))
            })
        })
        .and_then(|a| a["browser_download_url"].as_str())
        .unwrap_or("")
        .to_string();
    let published_at = body["published_at"]
        .as_str()
        .or_else(|| body["created_at"].as_str());
    // 只在 release 确实晚于本地构建时提示，禁止 hash 不相等就把本地较新构建降级。
    let newer = release_is_newer(current, &tag, published_at);
    Ok(json!({
        "current": current_hash,
        "latest": tag,
        "newer": newer,
        "url": url,
        "status": if newer { "update" } else { "latest" },
    }))
}

/// 下载校验 → 清理残留 → 静默启动安装器 → 立即退出自身,由 helper 装完拉起新版本。
/// 不能只是 spawn 安装器就返回:安装器要替换的就是本进程的镜像,自己不退出必然
/// 撞 os error 32,而 NSIS 遇占用会挂成僵尸把后续重试也锁死(D-124)。
#[tauri::command]
async fn update_install(app: tauri::AppHandle, url: String) -> Result<String, String> {
    if !url.starts_with("https://github.com/kanze1/kanzei-code/") {
        return Err("仅允许本仓库 release 资源".into());
    }
    let config = KanzeiConfig::load(Path::new(".")).unwrap_or_default();
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let client = kanzei_llm::proxy::build_http_client(&proxy).map_err(|e| e.to_string())?;
    let bytes = client
        .get(&url)
        .header("user-agent", "kanzei-app")
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| format!("下载失败:{e}"))?
        .error_for_status()
        .map_err(|e| format!("下载失败:{e}"))?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    validate_installer(&bytes)?;
    let notes = clear_stale_installer();
    let path = installer_path();
    std::fs::write(&path, &bytes)
        .map_err(|e| format!("写入安装包失败:{e}(检查 %TEMP% 是否可写或被杀软占用)"))?;
    let exe = std::env::current_exe().map_err(|e| format!("无法定位自身路径:{e}"))?;
    // helper 必须跑安装目录**之外**的副本(D-182):直接用 exe 起 helper 时,
    // 父进程退出后 helper 仍锁着同一个 kzapp.exe 镜像,NSIS 替换不了,
    // 于是安装静默失败——下载成功、文件一个没换,正是实测到的现象。
    let helper = update_helper_path();
    let _ = std::fs::remove_file(&helper);
    std::fs::copy(&exe, &helper).map_err(|e| {
        format!("准备更新交接程序失败:{e}。可手动运行 {} 完成安装。", path.display())
    })?;
    update_log(&format!("交接:helper={} 安装包={}", helper.display(), path.display()));
    Command::new(&helper)
        .arg("--kz-install-helper")
        .arg(&path)
        .arg(&exe)
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|e| {
            format!("启动更新交接失败:{e}。可手动运行 {} 完成安装。", path.display())
        })?;
    let mb = bytes.len() / 1_048_576;
    let prefix = if notes.is_empty() { String::new() } else { format!("{};", notes.join(";")) };
    // helper 已接管,退出让出镜像句柄;装完由 helper 拉起新版本。
    app.exit(0);
    Ok(format!("{prefix}已下载 {mb} MB,正在退出并静默安装,装完会自动重新打开"))
}

/// 设置页"测试"按钮:按当前表单值直接探测 provider(不落盘),401/超时给出可操作提示。
#[tauri::command]
async fn provider_test(
    protocol: String,
    base_url: String,
    api_key_env: Option<String>,
    api_key: Option<String>,
    auth: Option<String>,
    proxy: Option<String>,
) -> Result<String, String> {
    if matches!(auth.as_deref(), Some("codex") | Some("claude")) {
        return Ok("订阅登录态通道,无需 key 测试".into());
    }
    let key = api_key
        .filter(|k| !k.trim().is_empty())
        .or_else(|| api_key_env.as_deref().and_then(|e| std::env::var(e).ok()))
        .filter(|k| !k.trim().is_empty());
    let config = KanzeiConfig::load(Path::new(".")).unwrap_or_default();
    let proxy_value = proxy.or(config.proxy);
    let proxy = match proxy_value.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let client = kanzei_llm::proxy::build_http_client(&proxy).map_err(|e| e.to_string())?;
    let base = base_url.trim_end_matches('/');
    let request = match protocol.as_str() {
        "anthropic" => {
            let mut r = client
                .get(format!("{base}/v1/models"))
                .header("anthropic-version", "2023-06-01");
            if let Some(k) = &key {
                r = r.header("x-api-key", k);
            }
            r
        }
        _ => {
            let mut r = client.get(format!("{base}/models"));
            if let Some(k) = &key {
                r = r.bearer_auth(k);
            }
            r
        }
    };
    match request.timeout(std::time::Duration::from_secs(15)).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            Ok(match status {
                200 => format!("✓ 可用(HTTP 200{})", if key.is_some() { ",key 有效" } else { ",无鉴权" }),
                401 | 403 => format!(
                    "✗ key 无效(HTTP {status})——检查 key 是否过期/复制完整;moonshot 注意 .cn 与 .ai 的 key 不通用"
                ),
                404 => "✗ 端点 404——base_url 可能不对(需要以 /v1 结尾?)".into(),
                _ => format!("? HTTP {status}——通道可达但响应异常"),
            })
        }
        Err(e) if e.is_timeout() => Ok("✗ 超时——检查网络/代理设置(本地服务不走代理)".into()),
        Err(e) if e.is_connect() => Ok("✗ 连接失败——服务未启动或代理不通".into()),
        Err(e) => Ok(format!("✗ 请求失败:{e}")),
    }
}

/// 侧边栏直接改状态/关闭(走同一套 TrackerTool 硬门禁,不绕过状态机)。
#[tauri::command]
async fn docs_update(
    project_dir: String,
    kind: String,
    action: String,
    id: String,
    status: Option<String>,
    title: Option<String>,
    priority: Option<String>,
    fields: Option<serde_json::Value>,
    order: Option<Vec<String>>,
) -> Result<String, String> {
    use kanzei_harness::Tool as _;
    use kanzei_tools::docstore::{DEFECTS as D, FINDINGS as F, REQUIREMENTS as R, SOURCES as S};
    use kanzei_tools::tracker::TrackerTool;
    let tool = match kind.as_str() {
        "req" => TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &R,
            requires_refs: None,
        },
        "defect" => TrackerTool {
            tool_name: "defect",
            noun: "defect",
            kind: &D,
            requires_refs: None,
        },
        "source" => TrackerTool {
            tool_name: "source",
            noun: "source",
            kind: &S,
            requires_refs: None,
        },
        "finding" => TrackerTool {
            tool_name: "finding",
            noun: "finding",
            kind: &F,
            requires_refs: Some(&S),
        },
        "goal" => TrackerTool {
            tool_name: "goal",
            noun: "goal",
            kind: &GOALS,
            requires_refs: None,
        },
        other => return Err(format!("unknown kind `{other}`")),
    };
    let mut input = json!({ "action": action, "id": id });
    if let Some(order) = order.filter(|o| !o.is_empty()) {
        input["order"] = json!(order);
    }
    if let Some(status) = status {
        input["status"] = json!(status);
    }
    if let Some(title) = title.filter(|t| !t.trim().is_empty()) {
        input["title"] = json!(title);
    }
    if let Some(priority) = priority.filter(|p| !p.trim().is_empty()) {
        input["priority"] = json!(priority);
    }
    if let Some(fields) = fields.filter(|f| f.is_object()) {
        input["fields"] = fields;
    }
    let ctx = kanzei_harness::ToolCtx::new(PathBuf::from(&project_dir));
    let output = tool.execute(input, &ctx).await;
    if output.is_error {
        Err(output.content)
    } else {
        Ok(output.content)
    }
}

/// kind → 文档路径(docs_open / docs_read 共用)。
fn docs_path(project_dir: &str, kind: &str) -> Result<PathBuf, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| PathBuf::from(project_dir));
    let path = match kind {
        "req" => root.join(kanzei_tools::docstore::REQUIREMENTS.rel_path),
        "defect" => root.join(kanzei_tools::docstore::DEFECTS.rel_path),
        "goal" => root.join(kanzei_tools::docstore::GOALS.rel_path),
        "conventions" => root.join(CONVENTIONS_REL),
        "architecture" => root.join(".kanzei/project/architecture/README.md"),
        // 归档文件:req-archive / defect-archive / goal-archive
        "req-archive" => DocStore::open(&root, &REQUIREMENTS).archive_file(),
        "defect-archive" => DocStore::open(&root, &DEFECTS).archive_file(),
        "goal-archive" => DocStore::open(&root, &GOALS).archive_file(),
        "source" => root.join(kanzei_tools::docstore::SOURCES.rel_path),
        "finding" => root.join(kanzei_tools::docstore::FINDINGS.rel_path),
        "report" => root.join(".kanzei/research/report.md"),
        "source-archive" => DocStore::open(&root, &SOURCES).archive_file(),
        "finding-archive" => DocStore::open(&root, &FINDINGS).archive_file(),
        other => return Err(format!("unknown kind `{other}`")),
    };
    if !path.is_file() {
        return Err(format!("文档还不存在:{}", path.display()));
    }
    Ok(path)
}

/// 用系统默认程序打开文档原文(应用内查看器的"外部打开"兜底)。
#[tauri::command]
fn docs_open(project_dir: String, kind: String) -> Result<(), String> {
    let path = docs_path(&project_dir, &kind)?;
    hidden_command("cmd")
        .args(["/c", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// git 概览:分支 + 未提交改动数(状态栏显示)。
#[tauri::command]
async fn git_status(project_dir: String) -> Result<serde_json::Value, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    tokio::task::spawn_blocking(move || {
        let run = |args: &[&str]| -> Option<String> {
            let out = hidden_command("git")
                .args(args)
                .current_dir(&root)
                .output()
                .ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"]);
        let changes = run(&["status", "--porcelain"])
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);
        let last = run(&["log", "-1", "--format=%h %s"]);
        json!({ "branch": branch, "changes": changes, "last": last })
    })
    .await
    .map_err(|e| e.to_string())
}

const CONVENTIONS_REL: &str = ".kanzei/project/conventions.md";

/// 桌面端调用外部程序时禁止创建控制台窗口(Windows GUI 应用不应闪出黑框)。
fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

fn mobile_json_response(status: &str, body: &serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes()
    .into_iter()
    .chain(body)
    .collect()
}

fn mobile_query(path: &str, key: &str) -> Option<String> {
    path.split('?')
        .nth(1)?
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.replace('+', " "))
}

fn mobile_authorized(request: &str, token: &str) -> bool {
    request.lines().any(|line| {
        line.to_ascii_lowercase()
            .strip_prefix("authorization: bearer ")
            .is_some_and(|value| value.trim() == token)
    })
}

fn handle_mobile_connection(mut stream: TcpStream, project_root: PathBuf, token: String) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let Ok(count) = stream.read(&mut chunk) else { return };
        if count == 0 { return; }
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if buffer.len() > 65_536 { return; }
    };
    let request_head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    if !mobile_authorized(&request_head, &token) {
        let _ = stream.write_all(&mobile_json_response("401 Unauthorized", &json!({"error": "device_revoked_or_unauthorized"})));
        return;
    }
    let request_line = request_head.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let content_length = request_head
        .lines()
        .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(str::to_owned))
        // 标准头是 "Content-Length: 123",冒号后有空格;不 trim 会解析失败退回 0,
        // 导致 body 恒为空、所有 POST 都因缺字段返回 400(D-063)。
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < header_end + content_length {
        let Ok(count) = stream.read(&mut chunk) else { return };
        if count == 0 { return; }
        buffer.extend_from_slice(&chunk[..count]);
    }
    let body = &buffer[header_end..header_end + content_length];
    let state_path = kanzei_core::project_state_path(&project_root);
    let response = match (method, path.split('?').next().unwrap_or_default()) {
        ("GET", "/health") => mobile_json_response("200 OK", &json!({"status": "ok", "transport": "local_http"})),
        ("GET", "/v1/notifications") => {
            let Some(thread_id) = mobile_query(path, "thread_id") else {
                let _ = stream.write_all(&mobile_json_response("400 Bad Request", &json!({"error": "thread_id_required"})));
                return;
            };
            let device_id = mobile_query(path, "device_id").unwrap_or_else(|| "paired-device".into());
            let cursor = mobile_query(path, "cursor")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_else(|| kanzei_core::SessionStore::open(&state_path).ok()
                    .and_then(|store| store.delivery_cursor(&device_id, &thread_id).ok())
                    .unwrap_or(0));
            match kanzei_core::SessionStore::open(&state_path)
                .and_then(|store| {
                    let events = store.replay_notifications(&thread_id, cursor, 100)?;
                    if let Some(last) = events.last() {
                        store.set_delivery_cursor(&device_id, &thread_id, last.sequence)?;
                    }
                    Ok(events)
                }) {
                Ok(events) => mobile_json_response("200 OK", &json!({"events": events, "cursor": events.last().map(|event| event.sequence).unwrap_or(cursor)})),
                Err(error) => mobile_json_response("500 Internal Server Error", &json!({"error": error.to_string()})),
            }
        }
        ("POST", "/v1/messages") => {
            let payload: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
            let Some(thread_id) = payload.get("thread_id").and_then(|value| value.as_str()) else {
                let _ = stream.write_all(&mobile_json_response("400 Bad Request", &json!({"error": "thread_id_required"})));
                return;
            };
            match kanzei_core::SessionStore::open(&state_path).and_then(|store| {
                store.create_session(&thread_id, &project_root.display().to_string(), None)?;
                store.append_event(&thread_id, "mobile.message", &payload)?;
                Ok(())
            }) {
                Ok(()) => mobile_json_response("202 Accepted", &json!({"accepted": true})),
                Err(error) => mobile_json_response("500 Internal Server Error", &json!({"error": error.to_string()})),
            }
        }
        _ => mobile_json_response("404 Not Found", &json!({"error": "not_found"})),
    };
    let _ = stream.write_all(&response);
}

#[tauri::command]
fn mobile_service_start(
    state: State<'_, AppState>,
    project_dir: String,
    port: Option<u16>,
) -> Result<MobileServiceInfo, String> {
    if state.mobile_service.lock().unwrap().is_some() {
        return Err("移动端桥接服务已经启动".into());
    }
    let root = normalized_project_root(Path::new(&project_dir));
    let listener = TcpListener::bind(("127.0.0.1", port.unwrap_or(0)))
        .map_err(|e| format!("移动端桥接服务启动失败: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("设置本机服务非阻塞失败: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?.to_string();
    let token = format!(
        "kz-mobile-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos()
    );
    let active = Arc::new(AtomicBool::new(true));
    let thread_active = active.clone();
    let thread_root = root.clone();
    let thread_token = token.clone();
    std::thread::spawn(move || {
        while thread_active.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => handle_mobile_connection(stream, thread_root.clone(), thread_token.clone()),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(_) => break,
            }
        }
    });
    let info = MobileServiceInfo { address: address.clone(), token: token.clone() };
    *state.mobile_service.lock().unwrap() = Some(MobileService { active });
    Ok(info)
}

#[tauri::command]
fn mobile_service_stop(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(service) = state.mobile_service.lock().unwrap().take() {
        service.active.store(false, Ordering::SeqCst);
        Ok(())
    } else {
        Err("移动端桥接服务当前未启动".into())
    }
}

fn agent_container_path(agent_id: &str) -> Result<PathBuf, String> {
    let safe: String = agent_id
        .trim()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') { ch } else { '-' })
        .collect();
    if safe.is_empty() {
        return Err("agent_id 不能为空".into());
    }
    Ok(kanzei_harness::kanzei_home()
        .unwrap_or_default()
        .join("agent-containers")
        .join(safe)
        .join("manifest.json"))
}

fn read_agent_container(agent_id: &str) -> Result<(PathBuf, AgentContainerManifest), String> {
    let path = agent_container_path(agent_id)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取代理容器失败: {e}"))?;
    let manifest = serde_json::from_str(&text).map_err(|e| format!("代理容器清单损坏: {e}"))?;
    Ok((path, manifest))
}

#[tauri::command]
fn agent_container_create(agent_id: String) -> Result<AgentContainerManifest, String> {
    let path = agent_container_path(&agent_id)?;
    if path.exists() {
        return Err(format!("代理容器已存在: {}", path.display()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let manifest = AgentContainerManifest {
        agent_id: agent_id.trim().to_owned(),
        version: "1".into(),
        status: "ready".into(),
        permissions: vec!["read".into()],
        updated_at: SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs() as i64,
    };
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(manifest)
}

#[tauri::command]
fn agent_container_upgrade(agent_id: String, version: String) -> Result<AgentContainerManifest, String> {
    let (path, mut manifest) = read_agent_container(&agent_id)?;
    let version = version.trim();
    if version.is_empty() {
        return Err("升级版本不能为空".into());
    }
    let backup = path.with_extension("json.previous");
    std::fs::copy(&path, &backup).map_err(|e| format!("保存升级回滚点失败: {e}"))?;
    manifest.version = version.to_owned();
    manifest.updated_at = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs() as i64;
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| format!("写入升级清单失败: {e}"))?;
    Ok(manifest)
}

#[tauri::command]
fn agent_container_rollback(agent_id: String) -> Result<AgentContainerManifest, String> {
    let (path, _) = read_agent_container(&agent_id)?;
    let backup = path.with_extension("json.previous");
    let text = std::fs::read_to_string(&backup).map_err(|e| format!("没有可用回滚点: {e}"))?;
    let manifest: AgentContainerManifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(manifest)
}

/// 开发规范模板(不存在时一键创建;用户手写维护,agent 只读注入)。
#[tauri::command]
fn conventions_init(project_dir: String) -> Result<String, String> {
    let root = kanzei_harness::config::discover_project_root(Path::new(&project_dir))
        .unwrap_or_else(|| PathBuf::from(&project_dir));
    let path = root.join(CONVENTIONS_REL);
    if path.is_file() {
        return Ok(path.display().to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &path,
        "# 开发规范\n\n## 代码风格\n- \n\n## 提交规范\n- \n\n## 测试要求\n- \n\n## 禁止事项\n- \n",
    )
    .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// 对话总结:fast 模型生成纪要并存档到 .kanzei/summaries/。
/// fast 模型跑一段总结(手动「总结」与 R-021 自动压缩共用)。
async fn fast_summarize(cwd: &Path, transcript: &str) -> Result<String, String> {
    use futures::StreamExt;
    let config = KanzeiConfig::load(cwd).map_err(|e| e.to_string())?;
    let resolved = config.resolve_model("fast").map_err(|e| e.to_string())?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let request = kanzei_llm::LlmRequest {
        model: resolved.model.clone(),
        system: vec![
            "把下面的人机协作对话记录总结成简洁的中文纪要:做了什么、改了哪些文件、\
             结论、遗留问题/下一步。markdown 列表,300 字以内。"
                .into(),
        ],
        messages: vec![kanzei_llm::Message::user_text(transcript)],
        tools: vec![],
        max_tokens: 2048,
        temperature: None,
        // 纪要总结不需要思考预算。
        reasoning: kanzei_llm::ReasoningEffort::Off,
    };
    let mut stream = client
        .stream(&route, &request)
        .await
        .map_err(|e| e.to_string())?;
    let mut summary = String::new();
    while let Some(event) = stream.next().await {
        if let kanzei_llm::LlmEvent::TextDelta { text, .. } = event.map_err(|e| e.to_string())? {
            summary.push_str(&text);
        }
    }
    if summary.trim().is_empty() {
        return Err("模型没有产出总结(fast 模型是否在运行?)".into());
    }
    Ok(summary)
}

/// 压缩用的对话文本化(工具结果截断,总量有界)。
fn render_transcript(messages: &[kanzei_llm::Message]) -> String {
    let mut out = String::new();
    'outer: for message in messages {
        for part in &message.parts {
            match part {
                kanzei_llm::Part::Text { text } => {
                    out.push_str(match message.role {
                        kanzei_llm::Role::User => "[用户] ",
                        kanzei_llm::Role::Assistant => "[助手] ",
                    });
                    out.push_str(text);
                    out.push('\n');
                }
                kanzei_llm::Part::ToolCall { name, input, .. } => {
                    out.push_str(&format!("[工具调用] {name} {input}\n"));
                }
                kanzei_llm::Part::ToolResult { content, .. } => {
                    let snippet: String = content.chars().take(1500).collect();
                    out.push_str(&format!("[工具结果] {snippet}\n"));
                }
                _ => {}
            }
            if out.len() > 100_000 {
                break 'outer;
            }
        }
    }
    out
}

#[tauri::command]
async fn summarize_chat(
    project_dir: String,
    transcript: String,
) -> Result<serde_json::Value, String> {
    let cwd = PathBuf::from(&project_dir);
    let summary = fast_summarize(&cwd, &transcript).await?;
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let dir = root.join(".kanzei").join("summaries");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("summary-{secs}.md"));
    std::fs::write(&path, &summary).map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "path": path.display().to_string() }))
}

// ---------- 运行 ----------

fn persist_always_allow(
    project_root: &Path,
    action: &str,
    resource: &str,
) -> Result<(kanzei_core::AskReply, PathBuf), String> {
    let pattern = kanzei_harness::config::generalize_resource(action, resource);
    let path = kanzei_harness::config::append_allow_rule(project_root, action, &pattern)
        .map_err(|error| error.to_string())?;
    Ok((kanzei_core::AskReply::AlwaysAllow, path))
}

/// reply: "deny" | "once" | "always"。always 先把泛化规则写进项目配置再放行。
#[tauri::command]
fn answer_ask(window: Window, state: State<'_, AppState>, id: u64, reply: String) {
    let Some(pending) = take_pending_ask(&state, id) else {
        return;
    };
    if matches!(pending.request, kanzei_core::AskRequest::Question { .. }) {
        let response = if reply.trim().is_empty() || reply == "cancel" {
            kanzei_core::AskResponse::Cancelled
        } else {
            kanzei_core::AskResponse::Answer(reply)
        };
        let _ = pending.sender.send(response);
        return;
    }
    let decision = match reply.as_str() {
        "always" => {
            let pattern =
                kanzei_harness::config::generalize_resource(&pending.action, &pending.resource);
            match persist_always_allow(
                &pending.project_root,
                &pending.action,
                &pending.resource,
            ) {
                Ok((reply, path)) => {
                    let _ = window.emit("kz:status", with_session_id(json!({
                        "stage": "权限",
                        "detail": format!("已记住:{} {pattern} → {}", pending.action, path.display()),
                    }), &pending.session_id));
                    reply
                }
                Err(error) => {
                    let _ = window.emit(
                        "kz:status",
                        with_session_id(
                            json!({
                                "stage": "权限",
                                "detail": format!("规则保存失败:{error};本次拒绝"),
                            }),
                            &pending.session_id,
                        ),
                    );
                    kanzei_core::AskReply::Deny
                }
            }
        }
        "once" => kanzei_core::AskReply::AllowOnce,
        _ => kanzei_core::AskReply::Deny,
    };
    let _ = pending.sender.send(kanzei_core::AskResponse::Permission(decision));
}

#[tauri::command]
fn pending_asks_get(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let runtime = runtime_for(&state, &session_id);
    let asks = runtime.asks.lock().unwrap();
    Ok(asks
        .iter()
        .map(|(id, pending)| pending_ask_payload(*id, pending))
        .collect())
}

/// 可选模型清单:角色(primary/fast)+ codex 三型号 + ollama 已装模型(动态查询)。
#[tauri::command]
async fn models_list(project_dir: Option<String>) -> Result<serde_json::Value, String> {
    let cwd = project_dir
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .ok_or("no working dir")?;
    let config = KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?;

    let mut items: Vec<serde_json::Value> = Vec::new();
    for role in ["primary", "fast"] {
        if let Ok(r) = config.resolve_model(role) {
            items.push(json!({
                "id": role,
                "label": format!("{role} → {}:{}", r.provider_name, r.model),
            }));
        }
    }
    for (name, p) in &config.providers {
        if p.auth.as_deref() == Some("codex") {
            // ChatGPT 订阅当前仅这三个型号(2026-08)。
            for m in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
                items.push(json!({"id": format!("{name}:{m}"), "label": format!("{name}:{m}")}));
            }
        } else if p.auth.as_deref() == Some("claude") {
            // 实际可用型号(2026-08):Opus 5 / Sonnet 5 / Haiku 4.5。
            for m in [
                "claude-opus-5",
                "claude-sonnet-5",
                "claude-haiku-4-5-20251001",
            ] {
                items.push(json!({"id": format!("{name}:{m}"), "label": format!("{name}:{m}")}));
            }
        } else if p.protocol == "openai" || p.protocol == "openai-responses" {
            // D-167:任何 OpenAI 兼容端点(DeepSeek/OpenRouter/Kimi/自建 vLLM…)都走标准
            // GET {base_url}/models。早期这里只硬编码了 codex/claude/ollama 三种,别的
            // provider 加进配置后一个模型都列不出来,等于配了也用不了。
            // Ollama 例外:它的 /v1/models 不全,原生 /api/tags 才是真源。
            if p.base_url.contains("11434") {
                push_ollama_models(&mut items, name, &p.base_url).await;
                continue;
            }
            let key = p
                .api_key
                .clone()
                .filter(|k| !k.trim().is_empty())
                .or_else(|| p.api_key_env.as_deref().and_then(|e| std::env::var(e).ok()));
            let url = format!("{}/models", p.base_url.trim_end_matches('/'));
            let proxy = match config.proxy.as_deref() {
                Some("off") => ProxyConfig::Disabled,
                Some("env") | None => ProxyConfig::Env,
                Some(custom) => ProxyConfig::Explicit(custom.to_string()),
            };
            let Ok(client) = kanzei_llm::proxy::build_http_client(&proxy) else {
                continue;
            };
            let mut request = client.get(&url).timeout(std::time::Duration::from_secs(6));
            if let Some(k) = &key {
                request = request.bearer_auth(k);
            }
            // 探测失败不算错误:端点可能没实现 /models,或 key 还没配好。
            // 手填入口(前端的「＋ 手填模型…」)始终可用,所以这里静默跳过即可。
            if let Ok(resp) = request.send().await {
                if let Ok(v) = resp.json::<serde_json::Value>().await {
                    for m in v["data"].as_array().unwrap_or(&Vec::new()) {
                        if let Some(id) = m["id"].as_str() {
                            items.push(json!({
                                "id": format!("{name}:{id}"),
                                "label": format!("{name}:{id}"),
                            }));
                        }
                    }
                }
            }
        } else if p.base_url.contains("11434") {
            push_ollama_models(&mut items, name, &p.base_url).await;
        }
    }
    Ok(json!(items))
}

/// Ollama 的模型清单走原生 /api/tags:它的 /v1/models 不完整。
/// 本机服务不走代理——挂了代理反而连不上 127.0.0.1。
async fn push_ollama_models(items: &mut Vec<serde_json::Value>, name: &str, base_url: &str) {
    let tags_url = format!("{}/api/tags", base_url.trim_end_matches("/v1"));
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    else {
        return;
    };
    let Ok(resp) = client.get(&tags_url).send().await else {
        return;
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return;
    };
    for m in v["models"].as_array().unwrap_or(&Vec::new()) {
        if let Some(n) = m["name"].as_str() {
            items.push(json!({
                "id": format!("{name}:{n}"),
                "label": format!("{name}:{n}"),
            }));
        }
    }
}

/// 开新对话:清空会话内多轮历史,并写入空的持久化投影。
#[tauri::command]
fn conversation_clear(state: State<'_, AppState>, project_dir: String, process_id: Option<String>) -> Result<(), String> {
    // 会话 ID 必须与运行/写入侧同源:裸 discover 不做 canonicalize,算出的 session_id
    // 与运行侧不同,历史恢复、清空、删除会落到另一个会话上(D-058)。
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    store
        .append_event(
            &session_id,
            "conversation.updated",
            &json!({ "messages": [] }),
        )
        .map_err(|e| e.to_string())?;
    runtime_for(&state, &session_id)
        .conversation
        .lock()
        .unwrap()
        .insert(session_id.clone(), Vec::new());
    Ok(())
}

#[tauri::command]
fn conversation_get(
    state: State<'_, AppState>,
    project_dir: String,
    sequence: Option<i64>,
    process_id: Option<String>,
) -> Result<Vec<kanzei_llm::Message>, String> {
    // 会话 ID 必须与运行/写入侧同源:裸 discover 不做 canonicalize,算出的 session_id
    // 与运行侧不同,历史恢复、清空、删除会落到另一个会话上(D-058)。
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    // 展示给用户的是原文(未配对的工具调用也要看得见);作为下一轮 prior 的那份仍须过滤,
    // 否则孤儿 tool_result 会让 provider 直接 400(D-053/D-054)。
    let raw = recover_messages_raw(&store, &session_id, sequence).map_err(|e| e.to_string())?;
    runtime_for(&state, &session_id)
        .conversation
        .lock()
        .unwrap()
        .insert(session_id.clone(), kanzei_core::filter_message_history(&raw));
    Ok(raw)
}

#[tauri::command]
fn conversation_trace_get(
    project_dir: String,
    sequence: Option<i64>,
    process_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    // 会话 ID 必须与运行/写入侧同源:裸 discover 不做 canonicalize,算出的 session_id
    // 与运行侧不同,历史恢复、清空、删除会落到另一个会话上(D-058)。
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    let events = store.list_events(&session_id, 0).map_err(|e| e.to_string())?;
    let limit = sequence.unwrap_or(i64::MAX);
    let mut segment_start = 0;
    for event in &events {
        if event.sequence > limit {
            break;
        }
        if event.event_type == "conversation.updated"
            && event.payload["messages"].as_array().map_or(false, Vec::is_empty)
        {
            segment_start = event.sequence;
        }
    }
    Ok(events
        .into_iter()
        .filter(|event| {
            event.event_type == "run.trace"
                && event.sequence > segment_start
                && event.sequence <= limit
        })
        .map(|event| event.payload)
        .collect())
}

#[tauri::command]
fn conversation_list(project_dir: String, process_id: Option<String>) -> Result<Vec<serde_json::Value>, String> {
    // 会话 ID 必须与运行/写入侧同源:裸 discover 不做 canonicalize,算出的 session_id
    // 与运行侧不同,历史恢复、清空、删除会落到另一个会话上(D-058)。
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .create_session(&session_id, &root.display().to_string(), None)
        .map_err(|e| e.to_string())?;
    // 按"对话段"分组:同一段对话每轮都会追加快照,只展示每段最新的那份;
    // 清空快照(新对话)是分段边界。sequences 携带整段快照,供批量删除。
    let mut segments: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut open = false;
    for event in store
        .list_events(&session_id, 0)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|event| event.event_type == "conversation.updated")
    {
        let messages = event.payload["messages"].as_array();
        let count = messages.map_or(0, Vec::len);
        if count == 0 {
            open = false;
            continue;
        }
        if !open {
            segments.push(Vec::new());
            open = true;
        }
        let title = messages
            .and_then(|items| items.iter().find(|item| item["role"] == "user"))
            .and_then(|item| item["parts"].as_array())
            .and_then(|parts| parts.iter().find(|part| part["type"] == "text"))
            .and_then(|part| part["text"].as_str())
            .unwrap_or("新对话");
        segments.last_mut().unwrap().push(json!({
            "sequence": event.sequence,
            "created_at": event.created_at,
            "title": title.chars().take(48).collect::<String>(),
            "message_count": count,
        }));
    }
    Ok(segments
        .into_iter()
        .filter(|snapshots| !snapshots.is_empty())
        .map(|snapshots| {
            let sequences: Vec<i64> = snapshots
                .iter()
                .filter_map(|s| s["sequence"].as_i64())
                .collect();
            let last = snapshots.last().cloned().unwrap_or_default();
            json!({
                "sequence": last["sequence"],
                "created_at": last["created_at"],
                "title": last["title"],
                "message_count": last["message_count"],
                "sequences": sequences,
            })
        })
        .collect())
}

/// 批量删除历史对话快照(只删 conversation.updated,不动调度事件)。
#[tauri::command]
fn conversation_delete(project_dir: String, sequences: Vec<i64>, process_id: Option<String>) -> Result<usize, String> {
    // 会话 ID 必须与运行/写入侧同源:裸 discover 不做 canonicalize,算出的 session_id
    // 与运行侧不同,历史恢复、清空、删除会落到另一个会话上(D-058)。
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .delete_events_by_sequence(&session_id, "conversation.updated", &sequences)
        .map_err(|e| e.to_string())
}

/// 应用内查看文档:返回原文,前端直接渲染(markdown/代码),不再强制跳外部工具。
#[tauri::command]
fn docs_read(project_dir: String, kind: String) -> Result<serde_json::Value, String> {
    let path = docs_path(&project_dir, &kind)?;
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))?;
    Ok(json!({
        "path": path.display().to_string(),
        "name": path.file_name().and_then(|n| n.to_str()).unwrap_or(&kind),
        "content": content,
    }))
}

#[tauri::command]
fn app_info() -> serde_json::Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "build": option_env!("KANZEI_BUILD_INFO").unwrap_or("dev"),
    })
}

#[tauri::command]
fn stop_run(
    window: Window,
    state: State<'_, AppState>,
    project_dir: Option<String>,
    process_id: Option<String>,
) {
    let target_project = project_dir.as_ref().map(PathBuf::from).map(|cwd| {
        normalized_project_root(&cwd)
    });
    let target_session = target_project
        .as_ref()
        .map(|root| process_session_id(root, process_id.as_deref()));
    let runtimes: Vec<Arc<SessionRuntime>> = state
        .runtimes
        .lock()
        .unwrap()
        .iter()
        .filter(|(session_id, runtime)| {
            target_session.as_ref().map_or(true, |target| target == *session_id)
                && runtime.running.load(Ordering::SeqCst)
        })
        .map(|(_, runtime)| runtime.clone())
        .collect();
    if runtimes.is_empty() {
        let _ = window.emit(
            "kz:error",
            with_session_id(
                json!({ "message": "目标项目当前没有可停止的运行" }),
                target_session.as_deref().unwrap_or(""),
            ),
        );
        return;
    }
    let mut cancelled = None;
    for runtime in runtimes {
        let result = target_project.clone().map(|root| {
            let session_id = target_session
                .clone()
                .unwrap_or_else(|| kanzei_core::project_session_id(&root));
            let state_path = kanzei_core::project_state_path(&root);
            kanzei_core::SessionStore::open(&state_path)
                .and_then(|store| stop_runtime_and_finalize(&runtime, &store, &session_id))
        });
        cancelled = result;
    }
    match cancelled.transpose() {
        Ok(Some(count)) => {
            let _ = window.emit(
                "kz:stopped",
                with_session_id(
                    json!({ "cancelled_queue": count }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
        }
        Ok(None) => {
            let _ = window.emit(
                "kz:stopped",
                with_session_id(
                    json!({ "cancelled_queue": 0 }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
        }
        Err(error) => {
            let _ = window.emit(
                "kz:error",
                with_session_id(
                    json!({ "message": format!("停止时清理排队输入失败: {error}") }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
            let _ = window.emit(
                "kz:stopped",
                with_session_id(
                    json!({ "cancelled_queue": 0 }),
                    target_session.as_deref().unwrap_or(""),
                ),
            );
        }
    }

    // 后台进程不随 abort 结束:不回收会留下孤儿 dev server 占端口(R-097)。
    if let Some(root) = target_project {
        let window = window.clone();
        let session = target_session.clone().unwrap_or_default();
        tauri::async_runtime::spawn(async move {
            let killed = kanzei_tools::kill_background_processes(&root).await;
            if killed > 0 {
                let _ = window.emit(
                    "kz:status",
                    with_session_id(
                        json!({ "stage": "停止", "detail": format!("已回收 {killed} 个后台进程") }),
                        &session,
                    ),
                );
            }
        });
    }
}

fn parse_delivery(value: Option<&str>) -> anyhow::Result<kanzei_core::Delivery> {
    match value.unwrap_or("queue") {
        "steer" => Ok(kanzei_core::Delivery::Steer),
        "queue" => Ok(kanzei_core::Delivery::Queue),
        other => Err(anyhow::anyhow!("未知输入交付模式: {other}")),
    }
}

fn report_persistence_failure(
    window: &Window,
    session_id: &str,
    operation: &str,
    error: impl std::fmt::Display,
) {
    let message = format!("运行结果已保留，但{operation}失败: {error}");
    tracing::warn!("{message}");
    let _ = window.emit(
        "kz:error",
        with_session_id(json!({ "message": message }), session_id),
    );
}
fn append_run_notification(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    status: &str,
    summary: impl Into<String>,
    requires_action: bool,
) -> anyhow::Result<()> {
    store.append_notification_atomic(
        session_id,
        status,
        &summary.into(),
        requires_action,
    )?;
    Ok(())
}
fn admit_input(
    project_dir: &str,
    session_id: &str,
    prompt: &str,
    delivery: kanzei_core::Delivery,
) -> anyhow::Result<kanzei_core::AdmittedInput> {
    let project_root = normalized_project_root(Path::new(project_dir));
    let state_path = kanzei_core::project_state_path(&project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(&session_id, &project_root.display().to_string(), None)?;
    let input_id = format!(
        "input_{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let input = store.admit_input(&session_id, &input_id, prompt, delivery)?;
    store.append_event(
        &session_id,
        "prompt.admitted",
        &json!({ "input_id": input_id, "delivery": if matches!(delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
    )?;
    Ok(input)
}

fn promote_next_input(project_dir: &str, session_id: &str) -> anyhow::Result<Option<kanzei_core::AdmittedInput>> {
    let project_root = normalized_project_root(Path::new(project_dir));
    let state_path = kanzei_core::project_state_path(&project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    let Some(input) = store.promote_next_input(&session_id)? else {
        return Ok(None);
    };
    store.append_event(
        &session_id,
        "prompt.promoted",
        &json!({ "input_id": input.input_id, "delivery": if matches!(input.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
    )?;
    Ok(Some(input))
}

#[tauri::command]
async fn run_prompt(
    window: Window,
    state: State<'_, AppState>,
    prompt: String,
    project_dir: String,
    profile: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    work_priority: Option<String>,
    delivery: Option<String>,
    attachments: Option<Vec<PromptAttachment>>,
    process_id: Option<String>,
) -> Result<(), String> {
    let delivery = parse_delivery(delivery.as_deref()).map_err(|e| e.to_string())?;
    let project_root = normalized_project_root(Path::new(&project_dir));
    let process = if let Some(process_id) = process_id.as_deref() {
        let process = state
            .processes
            .lock()
            .unwrap()
            .get(process_id)
            .cloned()
            .ok_or_else(|| format!("进程不存在: {process_id}"))?;
        if process.project_dir != project_root.display().to_string() {
            return Err("进程不属于当前项目".into());
        }
        process
    } else {
        ensure_default_process(&state, &project_root)
    };
    let session_id = process_session_id(&project_root, Some(&process.id));
    let profile = profile.or_else(|| process.profile.lock().unwrap().clone());
    let model = model.or_else(|| process.model.lock().unwrap().clone());
    let reasoning = process.reasoning.lock().unwrap().clone();
    let subagent_enabled = process.subagent_enabled.load(Ordering::SeqCst);
    let runtime = runtime_for(&state, &session_id);
    let _lifecycle = runtime.lifecycle.lock().unwrap();
    {
        if runtime.running.load(Ordering::SeqCst) {
            if attachments.as_ref().is_some_and(|items| !items.is_empty()) {
                return Err("当前任务运行中不能排队附件，请等待本轮完成后再发送".into());
            }
            let queued = admit_input(&project_dir, &session_id, &prompt, delivery).map_err(|e| e.to_string())?;
            let _ = window.emit(
                "kz:status",
                with_session_id(
                    json!({ "stage": "排队", "detail": format!("已排队，前方输入将依次执行（{}）", queued.input_id) }),
                    &session_id,
                ),
            );
            return Ok(());
        }
        runtime.running.store(true, Ordering::SeqCst);
    }
    let asks = runtime.asks.clone();
    let ask_seq = state.ask_seq.clone();
    let running = runtime.running.clone();
    let lifecycle = runtime.lifecycle.clone();
    let conversation = runtime.conversation.clone();
    let live_run = runtime.live.clone();

    let runtime_for_task = runtime.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut next_input = None;
        let mut next_prompt = prompt;
        let mut next_attachments = attachments;
        // 会话是否因失败而停:只影响 kz:idle 的 reason,前端据此区分"跑完了"与"崩了"。
        let mut idle_reason = "completed";
        loop {
            let result = run_task(
                &window,
                asks.clone(),
                ask_seq.clone(),
                next_prompt,
                next_attachments.take(),
                project_dir.clone(),
                session_id.clone(),
                subagent_enabled,
                profile.clone(),
                agent.clone(),
                model.clone(),
                work_priority.clone(),
                reasoning.clone(),
                conversation.clone(),
                live_run.clone(),
                delivery,
                next_input.take(),
            )
            .await;
            if let Err(e) = &result {
                let message = e.to_string();
                let lower = message.to_lowercase();
                let hint = if ["timed out", "timeout", "connect", "dns", "connection"]
                    .iter()
                    .any(|k| lower.contains(k))
                {
                    "\n提示:疑似网络不通。若需代理,在设置页把代理设为「指定地址」(如 http://127.0.0.1:12000)后重试;本地模型(ollama)不受代理影响。"
                } else {
                    ""
                };
                let _ = window.emit(
                    "kz:error",
                    with_session_id(json!({ "message": format!("{message}{hint}") }), &session_id),
                );
            }
            if result.is_err() {
                let _lifecycle = lifecycle.lock().unwrap();
                running.store(false, Ordering::SeqCst);
                idle_reason = "failed";
                break;
            }
            // 必须写回外层 next_input:此处若新建绑定,promote 出来的输入会被丢弃,
            // 下一轮 run_task 收到 None 后按新输入重新 admit,导致队列顺序反转并重复入库。
            next_input = {
                let _lifecycle = lifecycle.lock().unwrap();
                match promote_next_input(&project_dir, &session_id) {
                    Ok(input) => {
                        if input.is_none() {
                            running.store(false, Ordering::SeqCst);
                        }
                        input
                    }
                    Err(error) => {
                        let _ = window.emit(
                            "kz:error",
                            with_session_id(json!({ "message": error.to_string() }), &session_id),
                        );
                        running.store(false, Ordering::SeqCst);
                        idle_reason = "failed";
                        None
                    }
                }
            };
            let Some(input) = next_input.clone() else {
                break;
            };
            next_prompt = input.prompt.clone();
            let _ = window.emit(
                "kz:status",
                with_session_id(
                    json!({ "stage": "排队", "detail": format!("开始执行排队输入（{}）", input.input_id) }),
                    &session_id,
                ),
            );
        }
        // 会话级终态(R-086)。kz:done 是**一轮**的终点:排队输入会让上面这个 loop
        // 接着跑下一轮,期间 runtime.running 一直是 true。前端若拿 kz:done 判空闲,
        // 多轮运行会在第一轮后就显示成已结束。只有 loop 真正退出后发的 kz:idle
        // 才代表"这个会话停了",UI 的会话状态机只认它收敛终态。
        // 被 stop_run abort 时这里不会执行——那条路径自己发 kz:stopped。
        let _ = window.emit(
            "kz:idle",
            with_session_id(json!({ "reason": idle_reason }), &session_id),
        );
        // 自然完成/失败时释放会话容器中的句柄;stop_run abort 后则由 stop 路径取走。
        runtime_for_task.current_run.lock().unwrap().take();
    });
    *runtime.current_run.lock().unwrap() = Some(handle);
    // spawn 可能在句柄安装前快速结束,避免把已结束句柄重新放回容器。
    if !runtime.running.load(Ordering::SeqCst) {
        runtime.current_run.lock().unwrap().take();
    }
    Ok(())
}

fn recover_messages(
    store: &kanzei_core::SessionStore,
    session_id: &str,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    recover_messages_at(store, session_id, None)
}

/// 落库历史原文,不做任何过滤——**展示用**。
/// 与 recover_messages_at 的区别是后者会丢掉未配对的 tool_use/tool_result:
/// 那是喂给 provider 的硬性要求(D-053/D-054),但对人展示时丢内容就是"会话看不全"。
fn recover_messages_raw(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    sequence: Option<i64>,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    let event = match sequence {
        Some(sequence) => store
            .list_events(session_id, 0)?
            .into_iter()
            .find(|event| event.sequence == sequence && event.event_type == "conversation.updated"),
        None => store.latest_event(session_id, "conversation.updated")?,
    };
    let Some(event) = event else {
        return Ok(Vec::new());
    };
    let messages = event
        .payload
        .get("messages")
        .cloned()
        .unwrap_or_else(|| json!([]));
    Ok(serde_json::from_value(messages)?)
}

/// 可安全作为下一轮 prior 的历史:强制 tool_use/tool_result 配对不变量。
fn recover_messages_at(
    store: &kanzei_core::SessionStore,
    session_id: &str,
    sequence: Option<i64>,
) -> anyhow::Result<Vec<kanzei_llm::Message>> {
    Ok(kanzei_core::filter_message_history(&recover_messages_raw(
        store, session_id, sequence,
    )?))
}

fn conversation_prior(
    conversation: &Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    session_id: &str,
    persisted: Vec<kanzei_llm::Message>,
) -> Vec<kanzei_llm::Message> {
    let mut conversations = conversation.lock().unwrap();
    let conv = conversations.entry(session_id.to_string()).or_default();
    if conv.is_empty() && !persisted.is_empty() {
        *conv = persisted;
    }
    conv.clone()
}

async fn run_task(
    window: &Window,
    asks: Arc<Mutex<HashMap<u64, PendingAsk>>>,
    ask_seq: Arc<AtomicU64>,
    prompt: String,
    attachments: Option<Vec<PromptAttachment>>,
    project_dir: String,
    session_id: String,
    subagent_enabled: bool,
    profile: Option<String>,
    agent_name: Option<String>,
    model_override: Option<String>,
    work_priority: Option<String>,
    reasoning_override: Option<String>,
    conversation: Arc<Mutex<HashMap<String, Vec<kanzei_llm::Message>>>>,
    live_run: Arc<Mutex<LiveRun>>,
    delivery: kanzei_core::Delivery,
    promoted_input: Option<kanzei_core::AdmittedInput>,
) -> anyhow::Result<()> {
    // 阶段汇报:让前端每一步都有着落(用户反馈:要详细指示)。
    let stage = |name: &str, detail: String| {
        let _ = window.emit(
            "kz:status",
            with_session_id(json!({ "stage": name, "detail": detail }), &session_id),
        );
    };

    let cwd = PathBuf::from(&project_dir);
    anyhow::ensure!(cwd.is_dir(), "工作目录不存在: {project_dir}");

    stage("配置", format!("加载 {}", cwd.display()));
    let (config, config_warnings) = KanzeiConfig::load_with_warnings(&cwd)?;
    let config = Arc::new(config);
    for warning in &config_warnings {
        stage("配置", warning.clone());
    }
    for warning in config.bash_permission_warnings() {
        stage("权限", warning);
    }
    let profile: ProfileKind = match profile.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => p.parse().map_err(|e: String| anyhow::anyhow!(e))?,
        None => config.default_profile(),
    };
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let rctx = ResolveCtx {
        profile,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };

    let mut harness = Harness::default();
    harness
        .add(BaseComponent)
        .add(DevProfile)
        .add(ResearchProfile)
        // 前端自查与定位工具只在桌面端有意义(需要真实运行中的窗口);
        // 顺序在 profile 之后、Config 之前,用户配置仍可覆盖。
        .add(FrontendToolsComponent)
        .add(MarkdownComponent)
        .add(ConfigComponent);
    let snapshot = harness.resolve(&rctx)?;
    let mut agent = snapshot.select_agent(agent_name.as_deref())?.clone();
    let work_priority = match work_priority.as_deref() {
        Some("requirement-first") => "requirement-first",
        _ => "defect-first",
    };
    if profile == ProfileKind::Dev {
        // 前端自查段跟着 FrontendToolsComponent 走:注册了这 5 个工具的装配线才追加。
        // 写死在 dev 基础提示词里的话,CLI 侧会被指向 5 个根本不存在的工具。
        agent.system.push('\n');
        agent.system.push('\n');
        agent.system.push_str(kanzei_tools::frontend_inspection_guidance());
        let (first, second) = if work_priority == "requirement-first" {
            ("requirements.md", "defects.md")
        } else {
            ("defects.md", "requirements.md")
        };
        agent.system.push_str(&format!(
            "\n\nWork selection mode for this run: {work_priority}. Scan {first} from top to bottom first; only after it has no workable item scan {second}. This run's selected mode overrides the default queue order in the surrounding project context."
        ));
    }
    stage(
        "装配",
        format!(
            "harness 就绪:agent {} · {} 个工具",
            agent.name,
            snapshot.materialize_tools().len()
        ),
    );

    // 界面模型下拉直选优先于 agent 定义。
    let model_ref = model_override
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| agent.model.clone());
    let resolved = config.resolve_model(&model_ref)?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    stage(
        "鉴权",
        format!(
            "{}:{}{}",
            resolved.provider_name,
            resolved.model,
            if resolved.provider.auth.is_some() {
                "(订阅登录态,可能刷新令牌)"
            } else {
                ""
            }
        ),
    );
    let route = kanzei_core::build_route(&resolved, &proxy).await?;
    stage("请求", "已发起,等待模型响应…".into());
    let client = LlmClient::new(&proxy)?;
    let runner_config = RunnerConfig {
        model: resolved.model.clone(),
        max_tokens: 8192,
        // 每进程选择优先,未选则用 kanzei.toml 的 [models] reasoning 默认档。
        reasoning: reasoning_override
            .as_deref()
            .or(config.models.reasoning.as_deref())
            .map(kanzei_llm::ReasoningEffort::parse)
            .unwrap_or_default(),
        // 轮内主动压缩的预算基准(D-176)。轮末那次压缩保留作兜底,但长轮/自动续跑
        // 根本轮不到它,真正起作用的是这条。
        context_limit: resolved.provider.context_limit,
    };
    let ctx = ToolCtx { cwd, project_root };

    let state_path = kanzei_core::project_state_path(&ctx.project_root);
    let store = kanzei_core::SessionStore::open(&state_path)?;
    store.create_session(&session_id, &ctx.project_root.display().to_string(), None)?;
    let is_new_input = promoted_input.is_none();
    let promoted = if let Some(input) = promoted_input {
        input
    } else {
        let input_id = format!(
            "input_{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        store.admit_input(&session_id, &input_id, &prompt, delivery)?;
        store.append_event(
            &session_id,
            "prompt.admitted",
            &json!({ "input_id": input_id, "delivery": if matches!(delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
        )?;
        store
            .promote_next_input(&session_id)?
            .ok_or_else(|| anyhow::anyhow!("无法提升已提交的桌面端输入"))?
    };
    if is_new_input {
        store.append_event(
            &session_id,
            "prompt.promoted",
            &json!({ "input_id": promoted.input_id, "delivery": if matches!(promoted.delivery, kanzei_core::Delivery::Steer) { "steer" } else { "queue" } }),
        )?;
    }
    let prompt = promoted.prompt;
    // promoted → running,并记住本轮身份与墙钟(D-173)。少了 running/completed 这段
    // 生命周期,跑完的输入永远停在 promoted,以后任何一次停止都会把它追认成 cancelled。
    let promoted_input_id = promoted.input_id.clone();
    store.start_input(&promoted_input_id)?;
    let run_id = format!(
        "run_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );
    let run_started = std::time::Instant::now();
    store.set_status(&session_id, "running")?;
    append_run_notification(&store, &session_id, "running", "任务已开始", false)?;
    store.append_event(
        &session_id,
        "session.status_changed",
        &json!({ "status": "running" }),
    )?;
    let _ = window.emit(
        "kz:meta",
        with_session_id(json!({
            "profile": format!("{profile:?}").to_lowercase(),
            "agent": agent.name,
            "model": format!("{}:{}", resolved.provider_name, resolved.model),
            "contextLimit": resolved.provider.context_limit,
        }), &session_id),
    );

    let event_window = window.clone();
    let session_id_for_events = session_id.clone();
    let emit_event = move |name: &str, payload: serde_json::Value| {
        event_window.emit(name, with_session_id(payload, &session_id_for_events))
    };
    // 轨迹与统计写进 runtime 的 live 画像,停止路径才够得着(D-179)。
    let live = live_run.clone();
    live.lock().unwrap().begin(
        &run_id,
        &promoted_input_id,
        &prompt,
        &resolved.provider_name,
        &resolved.model,
    );
    let trace_log = live.clone();
    // D-173 可观测性:主代理的工具调用原先只实时发给 UI,一条也不落库——
    // 于是"时间花在模型、shell 还是等用户""用户点了几次权限"事后统统无从查证,
    // 只能从最终对话快照反推。这里按 id 记开始时刻,收尾时连耗时一起写进 run.trace。
    let tool_started: Arc<Mutex<HashMap<String, std::time::Instant>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut on_event = move |event: RunEvent| {
        let elapsed_ms = |id: &str| -> Option<u128> {
            tool_started
                .lock()
                .unwrap()
                .remove(id)
                .map(|at| at.elapsed().as_millis())
        };
        let _ = match event {
            RunEvent::TurnStart { step, max_steps } => {
                {
                    let mut live = trace_log.lock().unwrap();
                    live.steps = live.steps.max(step);
                    live.trace.push(json!({
                        "kind": "turn.started", "step": step, "at": now_ms(),
                    }));
                }
                emit_event("kz:turn", json!({ "step": step, "maxSteps": max_steps }))
            }
            RunEvent::Text(text) => emit_event("kz:text", json!({ "text": text })),
            RunEvent::Reasoning(text) => emit_event("kz:reasoning", json!({ "text": text })),
            RunEvent::ToolStart {
                id,
                name,
                summary,
                input,
            } => {
                tool_started
                    .lock()
                    .unwrap()
                    .insert(id.clone(), std::time::Instant::now());
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "tool.started", "id": id, "name": name,
                    "summary": summary, "at": now_ms(),
                }));
                emit_event(
                    "kz:tool-start",
                    json!({ "id": id, "name": name, "summary": summary, "input": input }),
                )
            }
            RunEvent::ToolEnd {
                id,
                name,
                ok,
                preview,
                display,
            } => {
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "tool.completed", "id": id, "name": name, "ok": ok,
                    "durationMs": elapsed_ms(&id), "at": now_ms(),
                    // 失败原因要留档,成功的预览不必——轨迹不是第二份对话记录。
                    "error": (!ok).then(|| preview.chars().take(400).collect::<String>()),
                }));
                emit_event(
                    "kz:tool-end",
                    json!({ "id": id, "name": name, "ok": ok, "preview": preview, "display": display }),
                )
            }
            // 轮内主动压缩:UI 要看得见"什么时候让的路、让掉了多少",
            // 否则历史突然变短只会被当成 bug(D-176)。
            RunEvent::ContextCompacted {
                before_tokens,
                after_tokens,
                budget_tokens,
                limit_tokens,
                dropped_messages,
            } => {
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "context.compacted", "before": before_tokens, "after": after_tokens,
                    "budget": budget_tokens, "limit": limit_tokens,
                    "dropped": dropped_messages, "at": now_ms(),
                }));
                emit_event(
                    "kz:status",
                    json!({
                        "stage": "压缩",
                        "detail": format!(
                            "上下文约 {}k 已达 {}k 预算线(上限 {}k),就地压缩为 {}k,裁掉 {dropped_messages} 条历史",
                            before_tokens / 1000, budget_tokens / 1000,
                            limit_tokens / 1000, after_tokens / 1000
                        ),
                    }),
                )
            }
            RunEvent::PermissionResolved {
                tool_call_id,
                action,
                resource,
                decision,
                source,
            } => {
                trace_log.lock().unwrap().trace.push(json!({
                    "kind": "permission.resolved", "id": tool_call_id, "action": action,
                    "resource": resource, "decision": decision, "source": source, "at": now_ms(),
                }));
                Ok(())
            }
            // 子代理实时状态:挂到对应 task 块的进度行,并附带可展开的子工具轨迹。
            RunEvent::TaskProgress { id, text, trace } => {
                let payload = json!({
                    "id": id,
                    "text": text,
                    "trace": trace.map(|item| json!({
                        "child_id": item.child_id,
                        "phase": item.phase,
                        "name": item.name,
                        "summary": item.summary,
                        "ok": item.ok,
                        "preview": item.preview,
                        "display": item.display,
                    })),
                });
                trace_log.lock().unwrap().trace.push(payload.clone());
                emit_event("kz:task-progress", payload)
            },
            RunEvent::Retry { attempt, max, delay_ms } => emit_event(
                "kz:status",
                json!({ "stage": "重试", "detail": format!("网络请求暂时失败,第 {attempt}/{max} 次重试,等待 {delay_ms}ms") }),
            ),
            // 本步工具尚未执行,重放零副作用;前端需丢弃本步已渲染的残缺输出。
            RunEvent::StreamRestart { attempt, max, delay_ms } => emit_event(
                "kz:stream-restart",
                json!({
                    "attempt": attempt,
                    "max": max,
                    "delayMs": delay_ms,
                    "detail": format!("连接中断,重新请求本轮 {attempt}/{max},等待 {delay_ms}ms"),
                }),
            ),
            // 每步累计:停止时 episode 才有真实 token 数,而不是写个 0 冒充。
            RunEvent::StepEnd { usage, .. } => {
                {
                    let mut live = trace_log.lock().unwrap();
                    live.input_tokens += usage.input;
                    live.output_tokens += usage.output;
                }
                emit_event(
                    "kz:step",
                    json!({
                        "input": usage.input, "output": usage.output,
                        "cacheRead": usage.cache_read, "cacheWrite": usage.cache_write,
                    }),
                )
            }
        };
    };

    let ask_window = window.clone();
    let ask_root = ctx.project_root.clone();
    let ask_session_id = session_id.clone();
    let mut ask = move |request: kanzei_core::AskRequest| -> AskFuture {
        let (sender, receiver) = oneshot::channel();
        let id = ask_seq.fetch_add(1, Ordering::SeqCst);
        let (action, resource, payload) = match &request {
            kanzei_core::AskRequest::Permission { action, resource } => (
                action.clone(),
                resource.clone(),
                json!({ "kind": "permission", "id": id, "action": action, "resource": resource, "remember": kanzei_harness::config::generalize_resource(action, resource) }),
            ),
            kanzei_core::AskRequest::Question { question, options, default } => (
                "question".into(),
                question.clone(),
                json!({ "kind": "question", "id": id, "question": question, "options": options, "default": default }),
            ),
        };
        let payload = with_session_id(payload, &ask_session_id);
        asks.lock().unwrap().insert(
            id,
            PendingAsk { sender, request, action, resource, project_root: ask_root.clone(), session_id: ask_session_id.clone() },
        );
        let _ = ask_window.emit("kz:ask", payload);
        Box::pin(async move { receiver.await.unwrap_or(kanzei_core::AskResponse::Cancelled) })
    };

    // 会话连续:同项目续上内存历史；应用重启后从事件日志恢复最近一次完整消息投影。
    let persisted = recover_messages(&store, &session_id)?;
    let prior = conversation_prior(&conversation, &session_id, persisted);
    if !prior.is_empty() {
        stage("会话", format!("延续对话({} 条历史消息)", prior.len()));
    }

    // task 子代理运行时:独立只读快照;fast 角色缺席时两个档位都退回主模型。
    let subagent_rt = if subagent_enabled {
        let mut sub_harness = Harness::default();
        sub_harness
            .add(kanzei_tools::SubagentBase)
            .add(ConfigComponent);
        let sub_snapshot = sub_harness.resolve(&rctx)?;
        let fast = match config.resolve_model("fast") {
            Ok(r) => (kanzei_core::build_route(&r, &proxy).await)
                .ok()
                .map(|fr| (fr, r.model.clone())),
            Err(_) => None,
        };
        Some(kanzei_core::SubagentRuntime {
            snapshot: sub_snapshot,
            agent: kanzei_tools::explore_agent(),
            fast: fast.unwrap_or_else(|| (route.clone(), resolved.model.clone())),
            primary: (route.clone(), resolved.model.clone()),
            max_tokens: 4096,
            // 纯兜底(用户定调:不设短限),防子代理失控挂死整轮。
            timeout_secs: 900,
        })
    } else {
        None
    };

    let initial_parts = prompt_attachment_parts(attachments.unwrap_or_default())?;
    if !initial_parts.is_empty() {
        let image_count = initial_parts
            .iter()
            .filter(|part| matches!(part, kanzei_llm::Part::Image { .. }))
            .count();
        let document_count = initial_parts
            .iter()
            .filter(|part| matches!(part, kanzei_llm::Part::Document { .. }))
            .count();
        stage(
            "附件",
            format!(
                "已接收 {} 个附件，转换为 {} 个图片、{} 个文档输入，准备发送给 agent",
                initial_parts.len(),
                image_count,
                document_count
            ),
        );
    }

    // 开跑预检索(R-106):prompt 命中既有记忆时前置索引提示块;历史存用户原文。
    let run_prompt = match kanzei_tools::memory::prompt_hints(&ctx.project_root, &prompt) {
        Some(hints) => format!("{hints}\n\n{prompt}"),
        None => prompt.clone(),
    };
    let run_result = run_once_with_parts(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        &run_prompt,
        &prior,
        (!initial_parts.is_empty()).then_some(initial_parts.as_slice()),
        subagent_rt.as_ref(),
        &mut on_event,
        &mut ask,
    )
    .await;
    let store = match kanzei_core::SessionStore::open(&state_path) {
        Ok(store) => Some(store),
        Err(error) => {
            report_persistence_failure(window, &session_id, "打开会话数据库", error);
            None
        }
    };
    if let Some(store) = store.as_ref() {
        match &run_result {
            Ok(summary) => {
                if let Err(error) = store.set_status(&session_id, "idle") {
                    report_persistence_failure(window, &session_id, "写入 idle 状态", error);
                }
                if let Err(error) = store.append_event(
                    &session_id,
                    "session.status_changed",
                    &json!({ "status": "idle" }),
                ) {
                    report_persistence_failure(window, &session_id, "写入完成状态事件", error);
                }
                if let Err(error) = store.append_event(
                    &session_id,
                    "run.completed",
                    &json!({
                        "steps": summary.steps,
                        "halted_by_user": summary.halted_by_user,
                        "input": summary.usage.input,
                        "output": summary.usage.output,
                        // 上下文账单(R-106):各注入源字符数,UI 与度量共用。
                        "context": summary.context_report,
                    }),
                ) {
                    report_persistence_failure(window, &session_id, "写入完成事件", error);
                }
                // 本轮切片:summary.messages = prior + 本轮;统计与失败提炼都只看本轮,
                // 否则历史失败反复上报、工具计数累计全历史(R-099 基线失真)。
                let this_run = &summary.messages[prior.len().min(summary.messages.len())..];
                // 轮末失败提炼与机械投递(R-105):不依赖模型自觉调用 memory_note。
                let signals = kanzei_core::summarize_failures(this_run);
                if !signals.is_empty() {
                    let memory = kanzei_tools::memory::MemoryStore::project(&ctx.project_root);
                    kanzei_tools::memory::harvest_failures(&memory, &signals);
                }
                // SOP 提炼(R-124):只在本轮确实完成了一个完整条目时触发,闸门在
                // completed_entry 里用代码强制。SOP 是用户的常用模板,所以只产候选,
                // 落到 global 候选箱等用户一键采纳——agent 不能自己决定入库。
                // 根因→fact(R-105):同一次收口把根因原料投项目 inbox,由 manager
                // 提炼成 fact——SOP 判 NOOP 时根因仍有记忆价值。
                if let Some(done) = kanzei_core::completed_entry(this_run) {
                    if let Some(global) = kanzei_tools::memory::MemoryStore::global() {
                        kanzei_tools::memory::harvest_sop(&global, &done, &prompt);
                    }
                    kanzei_tools::memory::harvest_entry_fact(
                        &kanzei_tools::memory::MemoryStore::project(&ctx.project_root),
                        &done,
                        &prompt,
                        &signals,
                    );
                }
                // episode 落库(R-106):机械轨迹画像。失败不阻塞收尾。
                let _ = store.append_episode(&kanzei_core::EpisodeRecord {
                    session_id: &session_id,
                    prompt_head: &prompt,
                    outcome: if summary.halted_by_user { "halted" } else { "completed" },
                    steps: summary.steps,
                    input_tokens: summary.usage.input,
                    output_tokens: summary.usage.output,
                    tools_json: &serde_json::to_string(&kanzei_core::summarize_tools(this_run))
                        .unwrap_or_default(),
                    context_json: &serde_json::to_string(&summary.context_report)
                        .unwrap_or_default(),
                    // R-099 调用画像:与冗余治理共用同一份口径,别处不再各算各的。
                    metrics_json: &serde_json::to_string(&kanzei_core::summarize_metrics(this_run))
                        .unwrap_or_default(),
                    // D-173:轮次归属与墙钟。缺了它们,复盘只能从"当前配置"反推模型,
                    // 而配置随时会变——最基本的事实都无法证伪。
                    provider: &resolved.provider_name,
                    model: &resolved.model,
                    run_id: &run_id,
                    input_id: &promoted_input_id,
                    duration_ms: run_started.elapsed().as_millis() as u64,
                    // R-106:上下文溢出压缩丢弃的轨迹段沉淀为 episode 的一部分,
                    // 让溢出路径不再无声丢弃轨迹,复盘时可通过 episodes.overflow_json 查回。
                    overflow_json: &serde_json::to_string(&summary.overflow_traces)
                        .unwrap_or_default(),
                });
                let _ = store.finish_input(&promoted_input_id, true);
                // 富 episode(带工具画像/上下文账单)已写,标记防重:停止路径的
                // flush_live_run 不该再补一条信息量更少的(D-179)。
                live.lock().unwrap().flushed = true;
                if let Err(error) = append_run_notification(
                    store,
                    &session_id,
                    "succeeded",
                    "任务完成",
                    false,
                ) {
                    report_persistence_failure(window, &session_id, "写入完成通知", error);
                }
                // 轮末记忆整理(R-105):独立任务消化 inbox 草稿,不阻塞完成事件。
                tauri::async_runtime::spawn(consolidate_memory_inbox(project_dir.clone()));
            }
            Err(error) => {
                if let Err(persistence_error) = store.set_status(&session_id, "failed") {
                    report_persistence_failure(
                        window,
                        &session_id,
                        "写入失败状态",
                        persistence_error,
                    );
                }
                if let Err(persistence_error) = store.append_event(
                    &session_id,
                    "session.status_changed",
                    &json!({ "status": "failed" }),
                ) {
                    report_persistence_failure(
                        window,
                        &session_id,
                        "写入失败状态事件",
                        persistence_error,
                    );
                }
                if let Err(persistence_error) = store.append_event(
                    &session_id,
                    "run.failed",
                    &json!({ "error": error.to_string() }),
                ) {
                    report_persistence_failure(window, &session_id, "写入失败事件", persistence_error);
                }
                // 失败轮次原先在 `let summary = run_result?;` 处提前返回,轨迹与
                // episode 一并丢失——和被停止的轮次是同一个洞(D-179)。
                flush_live_run(store, &session_id, &live, "failed");
                let _ = store.finish_input(&promoted_input_id, false);
                if let Err(persistence_error) = append_run_notification(
                    store,
                    &session_id,
                    "failed",
                    error.to_string(),
                    false,
                ) {
                    report_persistence_failure(window, &session_id, "写入失败通知", persistence_error);
                }
            }
        }
    }
    let summary = run_result?;

    let history_len = summary.messages.len();
    // R-076:本轮工具画像随 kz:done 带给前端,鞭挞据此判定「实质进展」——
    // 只算本轮切片,不含 prior,否则历史工具调用让每一轮都看着像有动作。
    let this_run_tools =
        kanzei_core::summarize_tools(&summary.messages[prior.len().min(summary.messages.len())..]);
    conversation
        .lock()
        .unwrap()
        .insert(session_id.clone(), summary.messages);

    // R-021 自动压缩:历史估算超过上下文上限 70% 时,fast 模型出纪要并替换历史。
    // 估算用 len/4(与压缩预检同源的粗粒度);失败保留原历史,绝不丢上下文。
    if let Some(limit) = resolved.provider.context_limit {
        let estimate = {
            let conversations = conversation.lock().unwrap();
            let conv = conversations.get(&session_id).cloned().unwrap_or_default();
            serde_json::to_string(&conv)
                .map(|s| s.len() as u64 / 4)
                .unwrap_or(0)
        };
        if estimate > limit * 7 / 10 {
            stage(
                "压缩",
                format!(
                    "历史约 {}k token,超过 {}k 的 70%,自动压缩中…",
                    estimate / 1000,
                    limit / 1000
                ),
            );
            let transcript = {
                let conversations = conversation.lock().unwrap();
                let conv = conversations.get(&session_id).cloned().unwrap_or_default();
                render_transcript(&conv)
            };
            match fast_summarize(&ctx.cwd, &transcript).await {
                Ok(digest) => {
                    conversation.lock().unwrap().insert(
                        session_id.clone(),
                        vec![kanzei_llm::Message::user_text(format!(
                            "(系统:此前对话已自动压缩为以下纪要,基于它继续)\n{digest}"
                        ))],
                    );
                    let _ = window.emit(
                        "kz:compacted",
                        with_session_id(json!({ "summary": digest }), &session_id),
                    );
                }
                Err(e) => stage("压缩", format!("压缩失败:{e}(保留原历史)")),
            }
        }
    }

    let messages = conversation
        .lock()
        .unwrap()
        .get(&session_id)
        .cloned()
        .unwrap_or_default();
    let trace = live.lock().unwrap().trace.clone();
    if let Some(store) = store.as_ref() {
        if !trace.is_empty() {
            if let Err(error) = store.append_event(&session_id, "run.trace", &json!({ "events": trace })) {
                report_persistence_failure(window, &session_id, "写入运行轨迹", error);
            }
        }
        if let Err(error) = store.append_event(
            &session_id,
            "conversation.updated",
            &json!({ "messages": messages }),
        ) {
            report_persistence_failure(window, &session_id, "写入对话历史", error);
        }
    }
    let _ = window.emit(
        "kz:done",
        with_session_id(json!({
            "steps": summary.steps,
            "halted": summary.halted_by_user,
            "history": history_len,
            "input": summary.usage.input,
            "output": summary.usage.output,
            "cacheRead": summary.usage.cache_read,
            "cacheWrite": summary.usage.cache_write,
            "tools": this_run_tools,
        }), &session_id),
    );
    Ok(())
}


#[cfg(test)]
mod install_verify_tests {
    use super::image_replaced;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// D-199:"退出码成功"不等于"文件换了"。这条判据是唯一能把
    /// 静默没装上与真更新分开的东西,它必须对每一种"没换"都判 false。
    #[test]
    fn 未替换的镜像一律不算更新成功() {
        let t = UNIX_EPOCH + Duration::from_secs(1_786_212_410);
        let stamp = Some((t, 22_449_664_u64));

        // 实测形态:两次「检查更新」都 exit=0,前后 mtime 与大小一模一样。
        assert!(!image_replaced(stamp, stamp), "前后完全相同必须判为未替换");
        // 任一侧读不到:宁可多报可疑,也不能说成成功。
        assert!(!image_replaced(None, stamp));
        assert!(!image_replaced(stamp, None));
        assert!(!image_replaced(None, None));
    }

    #[test]
    fn 时间或大小任一变化都算替换成功() {
        let t = UNIX_EPOCH + Duration::from_secs(1_786_212_410);
        let stamp = Some((t, 22_449_664_u64));
        // 新构建通常两者都变;但只变一个也是真的换了,不能漏判成失败——
        // 漏判会让用户看到"更新未生效"却其实已经生效,比不报更让人不敢信。
        assert!(image_replaced(stamp, Some((t + Duration::from_secs(1), 22_449_664))));
        assert!(image_replaced(stamp, Some((t, 22_449_665))));
    }

    /// 真实文件上跑一遍:touch 之后指纹必须变。纯比较函数测不到
    /// `image_stamp` 取的字段对不对,而取错字段的话上面两条全绿也没用。
    #[test]
    fn image_stamp_跟得上真实文件改动() {
        let path = std::env::temp_dir().join(format!(
            "kz-d199-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::write(&path, b"old").unwrap();
        let before = super::image_stamp(&path);
        assert!(before.is_some());
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&path, b"replaced-with-longer-content").unwrap();
        let after = super::image_stamp(&path);
        assert!(image_replaced(before, after), "{before:?} -> {after:?}");
        std::fs::remove_file(&path).unwrap();
    }
}

#[cfg(test)]
mod assembly_tests {
    use super::*;

    /// D-195 的桌面这一半:追加前端自查段的装配线,必须真的注册了那段点名的每个工具。
    /// CLI 那一半在 kanzei-tools::profiles 的同名测试里(它守的是"别把这段写回基础
    /// 提示词")。两条合起来才是机制——D-190 只把文字挪了个地方,组件注册(run_task
    /// 里的 FrontendToolsComponent)与提示词追加(紧邻 work-priority 那几行)仍是两处
    /// 各写各的,没有任何东西保证同进同退。谁摘掉组件而留下追加,这里立刻红。
    #[test]
    fn 桌面装配线必须注册前端自查段点名的每个工具() {
        let root = PathBuf::from("C:/kanzei-d195-app-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        // run_task 的装配线,但不加 MarkdownComponent:它读真实 ~/.kanzei,
        // 会让这条测试的结果取决于跑测试的机器上放了什么。
        let mut harness = Harness::default();
        harness
            .add(BaseComponent)
            .add(DevProfile)
            .add(ResearchProfile)
            .add(FrontendToolsComponent)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let tools: Vec<String> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();

        let mentioned =
            kanzei_tools::prompt_tool_mentions(kanzei_tools::frontend_inspection_guidance());
        // 提取不出名字说明提取规则坏了,不是装配对了——那种绿是假的。
        assert_eq!(
            mentioned.len(),
            5,
            "前端自查段应点名 5 个工具,实际提取到 {mentioned:?}"
        );
        for tool in mentioned {
            assert!(
                tools.contains(&tool),
                "桌面装配线追加了点名 `{tool}` 的提示词,却没注册它;已注册: {tools:?}"
            );
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn settings_save_preserves_handwritten_permission_rules() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-settings-{}.toml",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::write(
            &path,
            "[permissions]
[[permissions.rules]]
action = \"bash\"
resource = \"{\\\"command\\\":\\\"git status\\\",\\\"workdir\\\":\\\".\\\"}\"
effect = \"allow\"
",
        ).unwrap();
        settings_save_at_path(SettingsPayload {
            primary: "anthropic:claude-sonnet-5".into(),
            fast: String::new(),
            proxy: "env".into(),
            reasoning: None,
            profile_default: None,
            profile: None,
            providers: vec![],
        }, &path).unwrap();
        let config: KanzeiConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config.permissions.rules.len(), 1);
        assert_eq!(config.permissions.rules[0].action, "bash");
        assert_eq!(config.models.primary.as_deref(), Some("anthropic:claude-sonnet-5"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settings_save_preserves_comments_and_unknown_fields() {
        // D-082 完整不变量:保存不得破坏注释、排版与 schema 未知的字段。
        let path = std::env::temp_dir().join(format!(
            "kanzei-settings-preserve-{}.toml",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::write(
            &path,
            "# 顶部注释:手写配置\nproxy = \"http://127.0.0.1:7890\"\n\n[models]\nprimary = \"anthropic:claude-sonnet-5\" # 主模型\n\n[future_section]\nnew_field = \"来自新版本\"\n\n[[permissions.rules]]\naction = \"read\"\nresource = \"*/.env\"\neffect = \"deny\"\n",
        )
        .unwrap();
        settings_save_at_path(
            SettingsPayload {
                primary: "anthropic:claude-opus-5".into(),
                fast: "ollama:qwen3.5:4b".into(),
                proxy: "env".into(),
                reasoning: Some("high".into()),
                profile_default: Some("dev".into()),
                profile: None,
                providers: vec![],
            },
            &path,
        )
        .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        for expected in [
            "# 顶部注释:手写配置",
            "# 主模型",
            "[future_section]",
            "new_field = \"来自新版本\"",
        ] {
            assert!(saved.contains(expected), "missing preserved text: {expected}\n---\n{saved}");
        }
        // proxy 回落默认:已存在的键写显式 "env" 而不是删除(删除会连带删掉挂在键上的注释)。
        assert!(saved.contains("proxy = \"env\""), "proxy should reset to env:\n{saved}");
        let config: KanzeiConfig = toml::from_str(&saved).unwrap();
        assert_eq!(config.models.primary.as_deref(), Some("anthropic:claude-opus-5"));
        assert_eq!(config.models.reasoning.as_deref(), Some("high"));
        assert_eq!(config.permissions.rules.len(), 1);
        assert_eq!(config.permissions.rules[0].resource, "*/.env");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settings_save_refuses_to_overwrite_unparseable_config() {
        // 解析失败绝不允许"回退默认值再覆写"——那等于销毁用户配置(D-082 的事故路径)。
        let path = std::env::temp_dir().join(format!(
            "kanzei-settings-broken-{}.toml",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let broken = "[models\nprimary = 不是合法 toml";
        std::fs::write(&path, broken).unwrap();
        let result = settings_save_at_path(
            SettingsPayload {
                primary: "anthropic:claude-sonnet-5".into(),
                fast: String::new(),
                proxy: "env".into(),
                reasoning: None,
                profile_default: None,
                profile: None,
                providers: vec![],
            },
            &path,
        );
        assert!(result.is_err(), "saving over a broken config must fail");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken, "file must be untouched");
        let _ = std::fs::remove_file(path);
    }
}
