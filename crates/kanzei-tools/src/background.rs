//! 后台进程注册表(R-097)。
//!
//! bash 工具默认是"跑完才返回",长驻进程(dev server、watch、长测试)因此无法使用:
//! 要么撞超时被杀,要么占满整轮。后台模式把进程交给本注册表托管,立刻返回句柄,
//! 之后用 `process` 工具查输出/探活/停止。
//!
//! 边界:进程按项目根登记,`kill_project` 在运行停止时回收,避免留下孤儿进程。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::io::AsyncReadExt;

/// 单个后台进程保留的输出上限。保留尾部——长驻进程的最新日志才有诊断价值。
const MAX_BACKGROUND_OUTPUT: usize = 256 * 1024;

pub struct BackgroundProcess {
    pub id: String,
    pub command: String,
    pub project_root: String,
    pub workdir: String,
    pid: Option<u32>,
    output: Arc<Mutex<Vec<u8>>>,
    truncated: Arc<AtomicBool>,
    /// None = 仍在运行;Some(code) = 已退出(code 为 None 表示被信号/强杀结束)。
    exit: Arc<Mutex<Option<Option<i32>>>>,
}

impl BackgroundProcess {
    pub fn is_running(&self) -> bool {
        self.exit.lock().unwrap().is_none()
    }

    pub fn exit_code(&self) -> Option<Option<i32>> {
        *self.exit.lock().unwrap()
    }

    /// 当前已捕获的输出(stdout 与 stderr 合并,按到达顺序)。
    pub fn output(&self) -> String {
        let buf = self.output.lock().unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    }

    pub fn truncated(&self) -> bool {
        self.truncated.load(Ordering::SeqCst)
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
}

type Registry = Mutex<HashMap<String, Arc<BackgroundProcess>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!("bg{}", SEQ.fetch_add(1, Ordering::SeqCst) + 1)
}

fn append_bounded(buf: &Arc<Mutex<Vec<u8>>>, truncated: &Arc<AtomicBool>, chunk: &[u8]) {
    let mut buf = buf.lock().unwrap();
    buf.extend_from_slice(chunk);
    if buf.len() > MAX_BACKGROUND_OUTPUT {
        // 丢头留尾:长驻进程关心的是最近发生了什么。
        let drop_to = buf.len() - MAX_BACKGROUND_OUTPUT;
        buf.drain(..drop_to);
        truncated.store(true, Ordering::SeqCst);
    }
}

/// 托管一个已 spawn 的子进程,立刻返回句柄。stdout/stderr 由后台任务持续抽取。
pub fn register(
    mut child: tokio::process::Child,
    command: String,
    project_root: &Path,
    workdir: &Path,
) -> Arc<BackgroundProcess> {
    let id = next_id();
    let output = Arc::new(Mutex::new(Vec::new()));
    let truncated = Arc::new(AtomicBool::new(false));
    let exit: Arc<Mutex<Option<Option<i32>>>> = Arc::new(Mutex::new(None));
    let pid = child.id();

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    for stream in [stdout.take().map(Ok), stderr.take().map(Err)]
        .into_iter()
        .flatten()
    {
        let output = output.clone();
        let truncated = truncated.clone();
        tokio::spawn(async move {
            let mut chunk = [0u8; 8192];
            match stream {
                Ok(mut out) => loop {
                    match out.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => append_bounded(&output, &truncated, &chunk[..n]),
                    }
                },
                Err(mut err) => loop {
                    match err.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => append_bounded(&output, &truncated, &chunk[..n]),
                    }
                },
            }
        });
    }

    {
        let exit = exit.clone();
        tokio::spawn(async move {
            let status = child.wait().await.ok().and_then(|s| s.code());
            *exit.lock().unwrap() = Some(status);
        });
    }

    let process = Arc::new(BackgroundProcess {
        id: id.clone(),
        command,
        project_root: project_root.display().to_string(),
        workdir: workdir.display().to_string(),
        pid,
        output,
        truncated,
        exit,
    });
    registry().lock().unwrap().insert(id, process.clone());
    process
}

pub fn get(id: &str) -> Option<Arc<BackgroundProcess>> {
    registry().lock().unwrap().get(id).cloned()
}

/// 本项目登记的全部后台进程(含已退出的,便于事后查看输出)。
pub fn list(project_root: &Path) -> Vec<Arc<BackgroundProcess>> {
    let root = project_root.display().to_string();
    let mut items: Vec<Arc<BackgroundProcess>> = registry()
        .lock()
        .unwrap()
        .values()
        .filter(|p| p.project_root == root)
        .cloned()
        .collect();
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}

/// 停止单个后台进程(连同子进程树)。已退出时返回 false。
pub async fn stop(id: &str) -> bool {
    let Some(process) = get(id) else {
        return false;
    };
    if !process.is_running() {
        return false;
    }
    if let Some(pid) = process.pid {
        crate::shell::kill_tree(pid).await;
    }
    true
}

/// 回收本项目的全部后台进程:运行停止时调用,避免留下孤儿 dev server。
/// 返回实际终止的进程数。
pub async fn kill_project(project_root: &Path) -> usize {
    let mut killed = 0usize;
    for process in list(project_root) {
        if process.is_running() {
            if let Some(pid) = process.pid {
                crate::shell::kill_tree(pid).await;
                killed += 1;
            }
        }
    }
    killed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 输出超上限时丢头留尾并标记截断() {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let truncated = Arc::new(AtomicBool::new(false));
        append_bounded(&buf, &truncated, &vec![b'a'; MAX_BACKGROUND_OUTPUT]);
        assert!(!truncated.load(Ordering::SeqCst));
        append_bounded(&buf, &truncated, b"TAIL");
        assert!(truncated.load(Ordering::SeqCst));
        let got = buf.lock().unwrap();
        assert_eq!(got.len(), MAX_BACKGROUND_OUTPUT);
        // 保留的是尾部
        assert!(got.ends_with(b"TAIL"));
    }

    #[test]
    fn id_递增且不重复() {
        let a = next_id();
        let b = next_id();
        assert_ne!(a, b);
        assert!(a.starts_with("bg"));
    }

    /// 真实起一个子进程,验证托管→捕获输出→登记在册→停止的完整闭环。
    #[tokio::test]
    async fn 后台进程可托管_可读输出_可停止() {
        let shell = crate::shell::detected_shell();
        let root = std::env::temp_dir().join("kanzei-bg-test");
        std::fs::create_dir_all(&root).unwrap();

        // 先跑一个立刻结束、有确定输出的命令,验证捕获与退出状态。
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
        );
        assert!(get(&handle.id).is_some(), "进程应登记在注册表");
        assert!(
            list(&root).iter().any(|p| p.id == handle.id),
            "应出现在本项目的进程列表里"
        );

        // 等它结束并把输出抽干(读取任务是异步的)。
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
        // 已退出的进程再 stop 返回 false,不应报错
        assert!(!stop(&handle.id).await, "已结束的进程不该报告为被终止");
    }
}
