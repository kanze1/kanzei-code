//! 后台进程注册表(R-097)。
//!
//! bash 工具默认是"跑完才返回",长驻进程(dev server、watch、长测试)因此无法使用:
//! 要么撞超时被杀,要么占满整轮。后台模式把进程交给本注册表托管,立刻返回句柄,
//! 之后用 `process` 工具查输出/探活/停止。
//!
//! 边界:进程按项目根登记,`kill_project` 在运行停止时回收,避免留下孤儿进程。

use kanzei_base::sync::MutexPoisonExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use serde::{Deserialize, Serialize};

mod lifecycle;
pub mod monitor;
pub use lifecycle::{get, kill_process, kill_project, list, stop, stop_result};

/// 用户界面只停止指定项目的一条后台进程，不能误停其它项目或整个运行。
pub async fn stop_for_project(root: &std::path::Path, id: &str) -> Result<bool, String> {
    let process = get(id).ok_or_else(|| format!("后台进程 {id} 不存在或已回收"))?;
    if crate::worktree::worktree_key(std::path::Path::new(&process.project_root))
        != crate::worktree::worktree_key(root)
    {
        return Err("后台进程不属于当前项目".into());
    }
    let stopped = stop_result(id).await?;
    if process.is_running() {
        return Err(format!("未能终止后台进程 {id}，进程仍在运行"));
    }
    Ok(stopped)
}

mod persistent;
use lifecycle::{next_id, now_ms, project_hash, registry};
#[cfg(test)]
use persistent::save_registry;
pub use persistent::{
    adopt_persistent, discover_persistent, kill_registered, kill_registered_result,
    mark_registry_failed, PersistentEntry,
};
use persistent::{insert_registry_entry, remove_registry_entry};
mod registration;
#[cfg(test)]
use registration::append_bounded;
pub(super) use registration::read_log_tail;
#[cfg(test)]
use registration::register;
pub(crate) use registration::register_with_mailbox;
pub(crate) use registration::{register_captured, CapturedOutput};

#[cfg(test)]
pub(crate) fn prune_finished_process_for_test(id: &str) -> bool {
    let mut registered = registry().lock_or_recover();
    let Some(process) = registered.get(id).cloned() else {
        return false;
    };
    let mut history = std::collections::HashMap::from([(id.to_string(), process)]);
    registration::prune_finished(&mut history, 0);
    if !history.is_empty() {
        return false;
    }
    registered.remove(id);
    true
}

type TaskJoin = futures::future::Shared<futures::future::BoxFuture<'static, Result<(), String>>>;
enum CompletionState {
    NotRequired,
    Pending,
    Running(TaskJoin),
}
struct TaskCompletion {
    task: Mutex<CompletionState>,
    ready: tokio::sync::Notify,
}
impl TaskCompletion {
    fn new(required: bool) -> Self {
        Self {
            task: Mutex::new(if required {
                CompletionState::Pending
            } else {
                CompletionState::NotRequired
            }),
            ready: tokio::sync::Notify::new(),
        }
    }
    fn publish(&self, handle: tokio::task::JoinHandle<Result<(), String>>) {
        let completion = async move {
            handle
                .await
                .map_err(|error| format!("后台进程清理任务异常结束：{error}"))?
        }
        .boxed()
        .shared();
        *self.task.lock_or_recover() = CompletionState::Running(completion);
        self.ready.notify_waiters();
    }
    async fn wait(&self) -> Result<(), String> {
        loop {
            let notified = self.ready.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let completion = {
                match &*self.task.lock_or_recover() {
                    CompletionState::NotRequired => return Ok(()),
                    CompletionState::Pending => None,
                    CompletionState::Running(completion) => Some(completion.clone()),
                }
            };
            if let Some(completion) = completion {
                return completion.await;
            }
            notified.await;
        }
    }
    fn completed(&self) -> Result<Result<(), String>, String> {
        let completion = {
            match &*self.task.lock_or_recover() {
                CompletionState::NotRequired => return Ok(Ok(())),
                CompletionState::Pending => return Err("后台进程清理任务尚未登记完成".into()),
                CompletionState::Running(completion) => completion.clone(),
            }
        };
        completion
            .now_or_never()
            .ok_or_else(|| "后台进程清理任务仍在收尾".into())
    }
}

/// 单个后台进程保留的输出上限。保留尾部——长驻进程的最新日志才有诊断价值。
const MAX_BACKGROUND_OUTPUT: usize = 256 * 1024;
/// persistent 日志在内存中的尾部上限；磁盘日志仍按块追加，避免全量驻留。
const MAX_BACKGROUND_FULL_OUTPUT: usize = 4 * 1024 * 1024;

/// 后台任务的归属身份。来自 ToolCtx 的双键; CLI 等未绑定身份的路径登记 `unowned`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundOwner {
    pub run_id: String,
    pub process_id: String,
    pub write_key: String,
}

pub struct BackgroundProcess {
    stdin: tokio::sync::Mutex<Option<tokio::process::ChildStdin>>,
    pub id: String,
    pub command: String,
    pub project_root: String,
    pub workdir: String,
    /// 归属身份:这个后台任务属于哪次运行与会话。
    pub owner: BackgroundOwner,
    /// 默认跨轮存活，所属会话/子任务明确停止才清理。persistent 服务还可跨应用恢复。
    pub persistent: bool,
    /// 落盘日志路径(仅 persistent 服务有)。
    pub log_path: Option<PathBuf>,
    /// persistent 服务的日志尾部(内存有界,磁盘文件保留完整追加记录)。
    full_output: Arc<Mutex<Vec<u8>>>,
    pub started_at_ms: u128,
    pid: Option<u32>,
    output: Arc<Mutex<Vec<u8>>>,
    output_total: Arc<std::sync::atomic::AtomicU64>,
    truncated: Arc<AtomicBool>,
    /// None = 仍在运行;Some(code) = 已退出(code 为 None 表示被信号/强杀结束)。
    exit: Arc<Mutex<Option<Option<i32>>>>,
    exit_completion: TaskCompletion,
}

impl BackgroundProcess {
    pub async fn send_input(&self, text: &str, close: bool) -> Result<(), String> {
        use tokio::io::AsyncWriteExt;
        if !self.is_running() {
            return Err("进程已经结束".into());
        }
        if text.len() > 64 * 1024 {
            return Err("单次输入最多 64 KiB".into());
        }
        let mut input = self.stdin.lock().await;
        let stream = input
            .as_mut()
            .ok_or("该进程没有交互输入；用 bash interactive=true 启动")?;
        kanzei_harness::tool_pipeline::with_timeout(
            async {
                stream.write_all(text.as_bytes()).await?;
                stream.flush().await
            },
            std::time::Duration::from_secs(2),
        )
        .await
        .map_err(|_| "输入等待超时，部分字节可能已发送，请先检查输出".to_string())?
        .map_err(|error| error.to_string())?;
        if close {
            input.take();
        }
        Ok(())
    }
    pub fn is_running(&self) -> bool {
        self.exit.lock_or_recover().is_none()
    }

    /// 终止树已被确认收敛时立即写入终态。等待任务稍后回收 Child 句柄，不能让
    /// 这段调度延迟把已杀进程继续暴露成 running。
    pub(crate) fn mark_terminated(&self) {
        let mut exit = self.exit.lock_or_recover();
        if exit.is_none() {
            *exit = Some(None);
        }
    }

    pub fn exit_code(&self) -> Option<Option<i32>> {
        *self.exit.lock_or_recover()
    }

    /// 当前已捕获的输出(stdout 与 stderr 合并,按到达顺序)。
    pub fn output(&self) -> String {
        let buf = self.output.lock_or_recover();
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// persistent 服务的日志尾部(内存有界,磁盘文件保留完整追加记录)。
    pub fn full_log(&self) -> String {
        let buf = self.full_output.lock_or_recover();
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Cursor is an absolute byte position, even after the bounded buffer drops its head.
    pub fn output_since(&self, cursor: u64) -> (u64, String, bool) {
        let bytes = self.output.lock_or_recover();
        let total = self.output_total.load(Ordering::SeqCst);
        let floor = total.saturating_sub(bytes.len() as u64);
        let start = cursor.saturating_sub(floor).min(bytes.len() as u64) as usize;
        let mut end = bytes.len();
        if let Err(error) = std::str::from_utf8(&bytes[start..]) {
            if error.error_len().is_none() {
                end = start + error.valid_up_to();
            }
        }
        (
            floor + end as u64,
            String::from_utf8_lossy(&bytes[start..end]).into_owned(),
            cursor < floor,
        )
    }

    pub fn truncated(&self) -> bool {
        self.truncated.load(Ordering::SeqCst)
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use std::path::Path;

    use kanzei_harness::{Tool, ToolCtx};

    fn test_owner() -> BackgroundOwner {
        BackgroundOwner {
            run_id: "run-test".into(),
            process_id: "proc-test".into(),
            write_key: "key-test".into(),
        }
    }

    #[tokio::test]
    async fn pending_completion_is_not_empty_and_all_waiters_join_same_task() {
        let completion = Arc::new(TaskCompletion::new(true));
        assert!(completion.completed().unwrap_err().contains("尚未登记"));
        let first = completion.clone();
        let cancelled = tokio::spawn(async move { first.wait().await });
        let second = completion.clone();
        let waiting = tokio::spawn(async move { second.wait().await });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        let (release, held) = tokio::sync::oneshot::channel();
        completion.publish(tokio::spawn(async move {
            held.await.unwrap();
            Ok(())
        }));
        cancelled.abort();
        let _ = cancelled.await;
        assert!(completion.completed().unwrap_err().contains("仍在收尾"));
        assert!(!waiting.is_finished());
        release.send(()).unwrap();
        assert!(waiting.await.unwrap().is_ok());
        assert!(completion.wait().await.is_ok());
        assert!(completion.completed().unwrap().is_ok());
    }

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-bg-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei")).unwrap();
        dir
    }

    fn ctx_for(root: &Path, run_id: &str) -> ToolCtx {
        ctx_for_with_process(root, run_id, &format!("proc-{run_id}"))
    }

    fn ctx_for_with_process(root: &Path, run_id: &str, process_id: &str) -> ToolCtx {
        ToolCtx {
            cwd: root.to_path_buf(),
            project_root: root.to_path_buf(),
            run_id: Some(run_id.into()),
            process_id: Some(process_id.into()),
            ..Default::default()
        }
    }

    async fn start_background(root: &Path, command: &str, run_id: &str) -> String {
        start_background_with_process(root, command, run_id, &format!("proc-{run_id}")).await
    }

    async fn start_background_with_process(
        root: &Path,
        command: &str,
        run_id: &str,
        process_id: &str,
    ) -> String {
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": command, "background": true }),
                &ctx_for_with_process(root, run_id, process_id),
            )
            .await;
        assert!(!out.is_error, "后台启动不该失败: {}", out.content);
        out.content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("输出里应有 process_id")
            .trim()
            .to_string()
    }

    async fn wait_until(mut ready: impl FnMut() -> bool, max_ms: u64) -> bool {
        for _ in 0..(max_ms / 50) {
            if ready() {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        ready()
    }

    fn linger() -> &'static str {
        match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 300",
            "cmd" => "ping -n 300 127.0.0.1 >nul",
            _ => "sleep 300",
        }
    }

    #[tokio::test]
    async fn terminal_completion_callback_has_final_output_and_keeps_result_queryable() {
        let root = temp_project("callback");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ctx = ctx_for(&root, "callback-owner");
        ctx.async_mailbox = Some(kanzei_harness::AsyncMailbox::new(move |n| {
            tx.send(n).map_err(|e| e.to_string())
        }));
        let output = crate::bash::BashTool
            .execute(
                serde_json::json!({"command":"echo CALLBACK_FINAL_OUTPUT","background":true}),
                &ctx,
            )
            .await;
        assert!(!output.is_error, "{}", output.content);
        let id = output
            .content
            .lines()
            .find_map(|s| s.strip_prefix("process_id: "))
            .unwrap()
            .trim();
        let notice = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(notice.text.contains("CALLBACK_FINAL_OUTPUT"));
        let process = get(id).expect("completed output retained");
        assert_eq!(process.exit_code(), Some(Some(0)));
        assert!(process.output().contains("CALLBACK_FINAL_OUTPUT"));
        assert_eq!(notice.id, format!("terminal:{id}"));
        assert!(rx.try_recv().is_err(), "one exit produces one callback");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn scoped_terminal_stop_rejects_foreign_project_and_keeps_other_process_alive() {
        let root = temp_project("scoped-stop");
        let other = temp_project("foreign-stop");
        let command = match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 30",
            "cmd" => "ping -n 31 127.0.0.1 >nul",
            _ => "sleep 30",
        };
        let first = start_background(&root, command, "scoped-first").await;
        let second = start_background(&root, command, "scoped-second").await;
        let foreign = stop_for_project(&other, &first).await;
        let first_survived = get(&first).unwrap().is_running();
        let stopped = stop_for_project(&root, &first).await;
        let other_survived = get(&second).unwrap().is_running();
        let again = stop_for_project(&root, &first).await;
        let absent = stop_for_project(&root, "not-a-background-id").await;
        stop(&first).await;
        stop(&second).await;
        assert!(foreign.is_err());
        assert!(first_survived);
        assert_eq!(stopped, Ok(true));
        assert!(other_survived);
        assert_eq!(again, Ok(false));
        assert!(absent.is_err());
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&other).ok();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn persistent_idle_output_flushes_before_process_exit() {
        let root = temp_project("idle-flush");
        let shell = crate::shell::detected_shell();
        let separator = if shell.name == "cmd" { " & " } else { "; " };
        let command = format!("echo AUDIT_IDLE_READY{separator}{}", linger());
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({"command":command,"background":true,"persistent":true}),
                &ctx_for(&root, "idle-flush"),
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let id = out
            .content
            .lines()
            .find_map(|s| s.strip_prefix("process_id: "))
            .unwrap();
        let process = get(id).unwrap();
        assert!(wait_until(|| process.output().contains("AUDIT_IDLE_READY"), 10_000).await);
        let log = process.log_path.as_ref().unwrap();
        let flushed = wait_until(
            || std::fs::read_to_string(log).is_ok_and(|text| text.contains("AUDIT_IDLE_READY")),
            4000,
        )
        .await;
        let still_running = process.is_running();
        stop_result(id).await.unwrap();
        assert!(
            still_running,
            "flush must happen while the service is idle and alive"
        );
        assert!(
            flushed,
            "small output must reach disk without another read or EOF"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn persistent_registration_failure_stops_the_spawned_child_and_preserves_evidence() {
        let root = temp_project("registration-failure");
        let path = persistent::registry_path(&root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "invalid registry evidence").unwrap();
        let shell = crate::shell::detected_shell();
        let mut command = tokio::process::Command::new(&shell.program);
        crate::hide_console(command.as_std_mut());
        let child = command
            .args(&shell.args)
            .arg(linger())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let result = register_with_mailbox(
            child,
            linger().into(),
            &root,
            &root,
            test_owner(),
            true,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(
            !crate::shell::process_alive(pid),
            "failure must not orphan the spawned service"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "invalid registry evidence"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn 按线路停止只回收目标owner的后台进程() {
        let root = temp_project("stop-owner");
        let target = start_background(&root, linger(), "line-a").await;
        let sibling = start_background_with_process(&root, linger(), "line-a", "proc-line-b").await;
        let target_process = get(&target).expect("目标进程应登记");
        let sibling_process = get(&sibling).expect("同线路进程应登记");
        assert_eq!(kill_process(&root, "proc-line-a").await, 1);
        let stopped = wait_until(|| !target_process.is_running(), 5_000).await;
        assert!(stopped, "目标线路后台进程应停止");
        assert!(sibling_process.is_running(), "其它线路后台进程必须保持运行");
        assert!(stop(&sibling).await);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn 输出超上限时丢头留尾并标记截断() {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let truncated = Arc::new(AtomicBool::new(false));
        append_bounded(&buf, &truncated, &vec![b'a'; MAX_BACKGROUND_OUTPUT]);
        assert!(!truncated.load(Ordering::SeqCst));
        append_bounded(&buf, &truncated, b"TAIL");
        assert!(truncated.load(Ordering::SeqCst));
        let got = buf.lock_or_recover();
        assert_eq!(got.len(), MAX_BACKGROUND_OUTPUT);
        assert!(got.ends_with(b"TAIL"));
    }

    #[test]
    fn id_递增且不重复() {
        let a = next_id();
        let b = next_id();
        assert_ne!(a, b);
        assert!(a.starts_with("bg"));
    }

    #[tokio::test]
    async fn 后台进程可托管_可读输出_可停止() {
        let shell = crate::shell::detected_shell();
        let root = std::env::temp_dir().join("kanzei-bg-test");
        std::fs::create_dir_all(&root).unwrap();

        let mut echo = tokio::process::Command::new(&shell.program);
        echo.args(&shell.args)
            .arg("echo kanzei-bg-ok")
            .current_dir(&root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let handle = register(
            echo.spawn().expect("spawn echo"),
            "echo kanzei-bg-ok".into(),
            &root,
            &root,
            test_owner(),
            false,
        )
        .await;
        assert!(get(&handle.id).is_some(), "进程应登记在注册表");
        assert_eq!(handle.owner, test_owner(), "后台任务必须带 owner 身份");
        assert!(
            list(&root).iter().any(|p| p.id == handle.id),
            "应出现在本项目的进程列表里"
        );

        for _ in 0..100 {
            if !handle.is_running() && handle.output().contains("kanzei-bg-ok") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(!handle.is_running(), "命令应已退出");
        assert!(
            handle.output().contains("kanzei-bg-ok"),
            "应捕获到 stdout,实际: {:?}",
            handle.output()
        );
        assert!(!stop(&handle.id).await, "已结束的进程不该报告为被终止");
    }

    #[tokio::test]
    async fn background_跨轮和并行查询不清理其他owner_停止才回收() {
        let root = temp_project("persist");
        let result = futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
            let persist_id = crate::bash::BashTool
                .execute(
                    serde_json::json!({ "command": "node -e \"require('net').createServer().listen(0, '127.0.0.1')\"", "background": true, "persistent": true }),
                    &ctx_for(&root, "run-A"),
                )
                .await;
            assert!(
                !persist_id.is_error,
                "persistent 后台启动失败: {}",
                persist_id.content
            );
            let persist_id = persist_id
                .content
                .lines()
                .find_map(|line| line.strip_prefix("process_id: "))
                .expect("应有 process_id")
                .trim()
                .to_string();

            let default_id = crate::bash::BashTool
                .execute(
                    serde_json::json!({ "command": "node -e \"require('net').createServer().listen(0, '127.0.0.1')\"", "background": true }),
                    &ctx_for(&root, "run-A"),
                )
                .await;
            assert!(
                !default_id.is_error,
                "默认后台启动失败: {}",
                default_id.content
            );
            let default_id = default_id
                .content
                .lines()
                .find_map(|line| line.strip_prefix("process_id: "))
                .expect("应有 process_id")
                .trim()
                .to_string();

            assert!(get(&persist_id).map(|p| p.is_running()).unwrap_or(false));
            assert!(get(&default_id).map(|p| p.is_running()).unwrap_or(false));

            let observed = crate::process::ProcessTool
                .execute(
                    serde_json::json!({"action":"list"}),
                    &ctx_for(&root, "run-B"),
                )
                .await;
            assert!(!observed.is_error);
            assert!(get(&default_id).unwrap().is_running());
            let command = crate::bash::BashTool
                .execute(
                    serde_json::json!({"command":"echo other-owner"}),
                    &ctx_for(&root, "run-B"),
                )
                .await;
            assert!(!command.is_error);
            assert!(get(&default_id).unwrap().is_running());
            let finished = kill_project(&root).await;
            assert_eq!(finished, 1, "只应收掉默认档位那一个: {finished}");
            assert!(get(&persist_id).map(|p| p.is_running()).unwrap_or(false));
            let gone = wait_until(
                || get(&default_id).map(|p| !p.is_running()).unwrap_or(true),
                3000,
            )
            .await;
            assert!(gone, "默认档位应在项目回收时被收掉");
        }))
        .await;
        for process in list(&root) {
            stop_result(&process.id)
                .await
                .expect("reclaim test-owned background process");
        }
        std::fs::remove_dir_all(&root).unwrap();
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    #[tokio::test]
    async fn persistent_日志落盘_超256k不丢头_退出后可回看() {
        let root = temp_project("logdisk");
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({
                    "command": "1..3000 | ForEach-Object { 'x' * 100 }",
                    "background": true,
                    "persistent": true,
                }),
                &ctx_for(&root, "run-A"),
            )
            .await;
        assert!(!out.is_error, "persistent 后台启动失败: {}", out.content);
        let id = out
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();

        let process = get(&id).expect("persistent 进程应在自然退出前登记");
        let log_path = process
            .log_path
            .clone()
            .expect("persistent 服务应有落盘路径");
        process.exit_completion.wait().await.unwrap();
        assert!(!process.is_running());
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.len() > 256 * 1024);
        assert!(content.starts_with(&"x".repeat(100)));
        let full = process.full_log();
        assert!(full.len() <= MAX_BACKGROUND_FULL_OUTPUT);
        assert!(process.truncated());
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn persistent_注册表登记_强杀后重开可发现_可接管可杀掉() {
        let root = temp_project("registry");
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": linger(), "background": true, "persistent": true }),
                &ctx_for(&root, "run-A"),
            )
            .await;
        assert!(!out.is_error, "persistent 后台启动失败: {}", out.content);
        let id = out
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();

        let path = persistent::registry_path(&root);
        let text = std::fs::read_to_string(&path).expect("注册表文件应存在");
        assert!(text.contains(&id));
        assert!(text.contains("run-A"));

        let entries = discover_persistent(&root).unwrap();
        let (entry, alive) = entries
            .iter()
            .find(|(e, _)| e.id == id)
            .expect("discover 应列出上次未终结的服务");
        assert!(alive);
        assert_eq!(entry.command, linger());

        let adopted = adopt_persistent(&root, &id)
            .await
            .unwrap()
            .expect("running 服务应可接管");
        assert!(adopted.is_running());
        assert_eq!(adopted.pid, Some(entry.pid));
        assert!(adopted.log_path.is_some());
        assert_eq!(adopted.owner.run_id, "run-A");

        assert!(kill_registered(&root, &id).await);
        let gone = wait_until(|| !crate::shell::process_alive(entry.pid), 15_000).await;
        assert!(gone);
        assert!(discover_persistent(&root).unwrap().is_empty());
        std::fs::remove_file(persistent::registry_path(&root)).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn persistent_幽灵条目_discover标失败并清理不留幽灵() {
        let root = temp_project("ghost");
        let ghost = PersistentEntry {
            id: "bg-ghost".into(),
            command: "ghost command".into(),
            project_root: root.display().to_string(),
            workdir: root.display().to_string(),
            owner: BackgroundOwner {
                run_id: "run-dead".into(),
                process_id: "proc-dead".into(),
                write_key: "key-dead".into(),
            },
            started_at_ms: now_ms(),
            pid: u32::MAX - 1,
            log: "bg-ghost.log".into(),
        };
        save_registry(&root, &[ghost]).unwrap();

        let out = crate::process::ProcessTool
            .execute(
                serde_json::json!({"action": "discover"}),
                &ctx_for(&root, "run-new"),
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("bg-ghost [failed (pruned)]"));
        assert!(discover_persistent(&root).unwrap().is_empty());
        std::fs::remove_file(persistent::registry_path(&root)).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn persistent_项目回收跳过_默认档位照常回收() {
        let root = temp_project("reap");
        let default_id = start_background(&root, linger(), "run-reap").await;
        let persist_real = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": linger(), "background": true, "persistent": true }),
                &ctx_for(&root, "run-reap"),
            )
            .await;
        assert!(!persist_real.is_error, "{}", persist_real.content);
        let persist_id = persist_real
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();

        assert!(get(&default_id).unwrap().is_running());
        assert!(get(&persist_id).unwrap().is_running());

        let killed = kill_project(&root).await;
        assert!(killed >= 1);
        let default_gone = wait_until(
            || get(&default_id).map(|p| !p.is_running()).unwrap_or(true),
            15_000,
        )
        .await;
        assert!(default_gone);
        assert!(get(&persist_id).map(|p| p.is_running()).unwrap_or(false));
        let persist_process = get(&persist_id).expect("persistent 进程应登记");
        assert!(stop(&persist_id).await);
        let gone = wait_until(|| !persist_process.is_running(), 15_000).await;
        assert!(gone);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn process_工具_discover_adopt_kill_全链路() {
        let root = temp_project("proc-registry");
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": linger(), "background": true, "persistent": true }),
                &ctx_for(&root, "run-proc"),
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let id = out
            .content
            .lines()
            .find_map(|line| line.strip_prefix("process_id: "))
            .expect("应有 process_id")
            .trim()
            .to_string();
        let ctx = ctx_for(&root, "run-proc");

        let discovered = crate::process::ProcessTool
            .execute(serde_json::json!({"action": "discover"}), &ctx)
            .await;
        assert!(!discovered.is_error, "{}", discovered.content);
        assert!(discovered.content.contains(&format!("{id} [running]")));
        assert!(
            discovered.content.contains("\"action\":\"adopt\"")
                && discovered.content.contains("\"action\":\"kill\"")
        );

        let adopted = crate::process::ProcessTool
            .execute(serde_json::json!({"action": "adopt", "id": id}), &ctx)
            .await;
        assert!(!adopted.is_error, "{}", adopted.content);
        assert!(adopted.content.contains("adopted"));
        assert!(get(&id).unwrap().is_running());

        let killed = crate::process::ProcessTool
            .execute(serde_json::json!({"action": "kill", "id": id}), &ctx)
            .await;
        assert!(!killed.is_error, "{}", killed.content);
        assert!(killed.content.contains("killed"));
        assert!(discover_persistent(&root).unwrap().is_empty());
        std::fs::remove_file(persistent::registry_path(&root)).ok();
        std::fs::remove_dir_all(&root).ok();
    }
}
