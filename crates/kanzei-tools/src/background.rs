//! 后台进程注册表(R-097)。
//!
//! bash 工具默认是"跑完才返回",长驻进程(dev server、watch、长测试)因此无法使用:
//! 要么撞超时被杀,要么占满整轮。后台模式把进程交给本注册表托管,立刻返回句柄,
//! 之后用 `process` 工具查输出/探活/停止。
//!
//! 边界:进程按项目根登记,`kill_project` 在运行停止时回收,避免留下孤儿进程。
//!
//! D-174:托管项目里的后台任务额外登记 **owner 身份**(run_id/process_id/写仲裁键)
//! 与 **启动瞬间的托管基线**。没有身份就无法把异步写入归因到某次运行,没有基线就
//! 无法判断托管树的变化是不是这个进程造成的——两者是后台围栏的地基。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};

use tokio::io::AsyncReadExt;

use crate::managed::ManagedSnapshot;

/// 单个后台进程保留的输出上限。保留尾部——长驻进程的最新日志才有诊断价值。
const MAX_BACKGROUND_OUTPUT: usize = 256 * 1024;

/// 守卫的对账间隔。托管树实测 47 文件 / 1.4 MB,这个频率的成本可以忽略;
/// 它决定的是"越界写入最多能存在多久",不是回滚是否发生。
const GUARD_TICK: std::time::Duration = std::time::Duration::from_millis(300);

/// 后台任务的归属身份(D-174 验收①"可归因")。来自 ToolCtx 的 R-171 双键;
/// CLI 等未绑定身份的路径登记 `unowned`,如实表示"不知道属于谁"。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundOwner {
    pub run_id: String,
    pub process_id: String,
    pub write_key: String,
}

/// 一次越界写入的归因记录。围栏动作(隔离/回滚)之外单独留档,
/// 让 `process output` 与后续工具调用都能把"谁在什么时候写了什么"讲清楚。
#[derive(Debug, Clone)]
pub struct BreachRecord {
    pub at_ms: u128,
    /// 被改动的托管路径(相对项目根)。
    pub touched: Vec<String>,
    /// 改后内容的留证目录。
    pub quarantine: String,
    /// 实际回滚成功的文件数。
    pub restored: usize,
}

pub struct BackgroundProcess {
    pub id: String,
    pub command: String,
    pub project_root: String,
    pub workdir: String,
    /// 归属身份:租约体系里这个后台任务算谁的。
    pub owner: BackgroundOwner,
    pub started_at_ms: u128,
    pid: Option<u32>,
    output: Arc<Mutex<Vec<u8>>>,
    truncated: Arc<AtomicBool>,
    /// None = 仍在运行;Some(code) = 已退出(code 为 None 表示被信号/强杀结束)。
    exit: Arc<Mutex<Option<Option<i32>>>>,
    /// 托管树对账基线。守卫吸收合法写入时会推进它,所以是可变的。
    baseline: Arc<Mutex<ManagedSnapshot>>,
    breaches: Arc<Mutex<Vec<BreachRecord>>>,
}

impl BackgroundProcess {
    pub fn is_running(&self) -> bool {
        self.exit.lock().unwrap().is_none()
    }

    /// 当前对账基线的副本(守卫用)。
    pub(crate) fn baseline(&self) -> ManagedSnapshot {
        self.baseline.lock().unwrap().clone()
    }

    /// 推进对账基线:合法写入被吸收后调用,越界回滚后也要调用(树已回到基线)。
    pub(crate) fn set_baseline(&self, snapshot: ManagedSnapshot) {
        *self.baseline.lock().unwrap() = snapshot;
    }

    pub(crate) fn record_breach(&self, record: BreachRecord) {
        self.breaches.lock().unwrap().push(record);
    }

    pub fn breaches(&self) -> Vec<BreachRecord> {
        self.breaches.lock().unwrap().clone()
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
///
/// `owner` 是归属身份,`baseline` 必须是 **spawn 之前** 拍下的托管镜像——
/// 晚一刻拍就会把这个进程自己的副作用算进基线,围栏从此永远看不见它。
pub fn register(
    mut child: tokio::process::Child,
    command: String,
    project_root: &Path,
    workdir: &Path,
    owner: BackgroundOwner,
    baseline: ManagedSnapshot,
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
        owner,
        started_at_ms: now_ms(),
        pid,
        output,
        truncated,
        exit,
        baseline: Arc::new(Mutex::new(baseline)),
        breaches: Arc::new(Mutex::new(Vec::new())),
    });
    registry().lock().unwrap().insert(id, process.clone());
    // 托管项目才需要守卫;非托管项目没有托管树可对账,不必空转。
    if crate::managed::managed_scope_exists(project_root) {
        install_absorber_once();
        spawn_guard(process.clone());
    }
    process
}

/// 安装"吸收合法写入"的回调(harness 侧的窗口关闭时回调过来)。
///
/// harness 不依赖 tools,快照与基线都在这一侧,所以只能反向注入。回调里做文件 IO
/// 是有意的:窗口关闭发生在工具执行完成之后,此刻磁盘状态才是最终的;而且注册表
/// 为空时立刻返回,没有后台任务的绝大多数场景零成本。
fn install_absorber_once() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        kanzei_harness::managed_fence::set_absorber(|spec| {
            for process in running_processes() {
                let root = PathBuf::from(&process.project_root);
                let current = ManagedSnapshot::capture(&root);
                let mut baseline = process.baseline();
                baseline.absorb_from(&current, spec.prefixes);
                process.set_baseline(baseline);
            }
        });
    });
}

/// 全部仍在运行的后台进程(跨项目)。锁只在克隆期间持有,后续文件 IO 不占锁。
fn running_processes() -> Vec<Arc<BackgroundProcess>> {
    registry()
        .lock()
        .unwrap()
        .values()
        .filter(|p| p.is_running())
        .cloned()
        .collect()
}

/// 后台守卫:周期性把托管树与基线对账,越界即隔离 + 回滚 + 终止进程树。
///
/// 进程退出后还会再对账一次——命令可能在最后一刻写盘,漏掉这一次就等于漏掉整次越界。
fn spawn_guard(process: Arc<BackgroundProcess>) {
    tokio::spawn(async move {
        loop {
            let was_running = process.is_running();
            reconcile(&process, true).await;
            if !was_running {
                break;
            }
            tokio::time::sleep(GUARD_TICK).await;
        }
    });
}

/// 一次对账。返回 Some = 检测到越界(已隔离并回滚)。
///
/// `kill_on_breach` 为真时连带终止进程树:后台进程通常会持续重试写入,
/// 只回滚不终止就是无限回滚循环。
async fn reconcile(process: &Arc<BackgroundProcess>, kill_on_breach: bool) -> Option<BreachRecord> {
    let root = PathBuf::from(&process.project_root);
    if !crate::managed::managed_scope_exists(&root) {
        return None;
    }
    let baseline = process.baseline();
    let current = ManagedSnapshot::capture(&root);
    let change = crate::managed::diff(&baseline, &current)?;
    // 分流:此刻有专用工具窗口覆盖的路径有合法解释,本轮放过——真正的吸收在
    // 窗口关闭时做(守卫是周期采样的,整个窗口可能落在两次采样之间)。
    let (_legitimate, breach) = change.partition(kanzei_harness::managed_fence::write_in_progress);
    if breach.is_empty() {
        // 全部有合法解释:推进基线,避免下一轮把同一批变化重复报一遍。
        process.set_baseline(current);
        return None;
    }
    let (quarantine, restored) = crate::managed::quarantine_and_restore(
        &root,
        &baseline,
        &breach,
        &format!("bg-{}", process.id),
    );
    let record = BreachRecord {
        at_ms: now_ms(),
        touched: breach.touched().into_iter().cloned().collect(),
        quarantine: quarantine.display().to_string(),
        restored,
    };
    process.record_breach(record.clone());
    if kill_on_breach {
        if let Some(pid) = process.pid {
            crate::shell::kill_tree(pid).await;
        }
    }
    // 回滚之后重新取基线:树已经回到基线状态,但窗口内的合法改动要一并吸收。
    process.set_baseline(ManagedSnapshot::capture(&root));
    Some(record)
}

/// 收掉不属于当前 run 的后台任务(先终止,再做终态对账)。返回收尾的任务数。
///
/// **为什么现在要收掉跨 run 的后台任务**:D-174 的安全支点是「后台任务生命周期
/// ⊆ owner run 生命周期」。有了这条,加上项目级单 writer,后台任务存活期间唯一
/// 可能合法写托管路径的主体就是 owner run 的前台工具——守卫才有资格把"没有窗口
/// 解释的变化"一律判给后台进程。跨 run 存活会直接击穿这个前提:新 run 的专用
/// 工具写入会被上一个 run 的守卫当成越界回滚,正是验收③要防的误伤。
///
/// **这是本轮的安全降级,不是终态语义。** 跨轮长驻的受管后台服务由 **R-180** 承接
/// (子代理后台化 R-175/R-176 是它的需求来源):那时后台任务要显式继承 writer owner、
/// 转为受管任务并在租约之间正确交接,而不是像现在这样被下一个 run 收掉。
/// 更强的内核级隔离(受限令牌/ACL)是 **D-258**,与本函数无关但同属 D-174 的残余。
pub async fn finish_foreign_owners(project_root: &Path, current_run_id: Option<&str>) -> usize {
    let current = current_run_id.unwrap_or("unowned");
    let mut finished = 0usize;
    for process in list(project_root) {
        if !process.is_running() || process.owner.run_id == current {
            continue;
        }
        if let Some(pid) = process.pid {
            crate::shell::kill_tree(pid).await;
        }
        reconcile(&process, false).await;
        finished += 1;
    }
    finished
}

pub(crate) fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
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
///
/// 终止后立刻做一次终态对账:进程可能在被杀的前一刻写了托管文件,
/// 而守卫的下一跳最多要等一个 GUARD_TICK——停止路径不能把这个缺口留给运气。
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
    reconcile(&process, false).await;
    true
}

/// 回收本项目的全部后台进程并做终态对账。
///
/// 调用点:①运行停止(app/run.rs),避免留下孤儿 dev server;②writer 租约释放前的
/// 收尾——设计基线 parallel_read_serial_write_orchestration.md 要求"writer 结束前
/// 收尾或显式转为受管任务,不能提前释放租约",本函数就是那个收尾入口(R-173 接线)。
///
/// 返回实际终止的进程数。
pub async fn kill_project(project_root: &Path) -> usize {
    let mut killed = 0usize;
    for process in list(project_root) {
        if process.is_running() {
            if let Some(pid) = process.pid {
                crate::shell::kill_tree(pid).await;
                killed += 1;
            }
            reconcile(&process, false).await;
        }
    }
    killed
}

#[cfg(test)]
mod tests {
    use super::*;

    use kanzei_harness::{Tool, ToolCtx};

    fn test_owner() -> BackgroundOwner {
        BackgroundOwner {
            run_id: "run-test".into(),
            process_id: "proc-test".into(),
            write_key: "key-test".into(),
        }
    }

    const ORIGINAL_DEFECTS: &str = "# Defects\n\n## D-001 原始内容 [open]\n";

    /// 合法写入窗口(`managed_fence`)是**进程级**状态,守卫要跨任务观察它。
    /// 因此凡是断言"越界被抓 / 合法写入没被误伤"的测试都必须串行:否则 A 测试
    /// 开着的 defect 窗口会给 B 测试的越界写入当挡箭牌,断言随机假绿。
    fn serial() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    fn temp_managed_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-bgfence-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(dir.join(".kanzei/project/defects.md"), ORIGINAL_DEFECTS).unwrap();
        dir
    }

    fn ctx_for(root: &Path, run_id: &str) -> ToolCtx {
        ToolCtx {
            cwd: root.to_path_buf(),
            project_root: root.to_path_buf(),
            run_id: Some(run_id.into()),
            process_id: Some(format!("proc-{run_id}")),
            ..Default::default()
        }
    }

    /// 起一个后台任务,返回它的 process id。走真实的 bash 工具路径,
    /// 因为围栏的装配(owner、基线、守卫)全在那条路径上。
    async fn start_background(root: &Path, command: &str, run_id: &str) -> String {
        let out = crate::bash::BashTool
            .execute(
                serde_json::json!({ "command": command, "background": true }),
                &ctx_for(root, run_id),
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

    /// 让后台命令先等一会儿再写文件:等待期给守卫拍下基线的机会,
    /// 也让"写入发生在工具早已返回之后"这个 D-174 的核心场景成真。
    fn delayed_write_command(target: &Path, content: &str) -> String {
        let target = target.display().to_string().replace('\\', "/");
        match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => format!(
                "Start-Sleep -Milliseconds 400; [System.IO.File]::WriteAllText('{target}', '{content}')"
            ),
            "cmd" => format!("ping -n 2 127.0.0.1 >nul & echo {content}> \"{target}\""),
            _ => format!("sleep 0.4; printf {content} > '{target}'"),
        }
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

    /// 命令片段的连接符(cmd 不认 `;`)。
    fn joiner() -> &'static str {
        if crate::shell::detected_shell().name == "cmd" {
            " & "
        } else {
            "; "
        }
    }

    /// 命令前奏:起一个孙进程,并把它的 pid 写进临时文件回传。返回 `(片段, 临时文件)`。
    ///
    /// D-262 之后凡是断言"进程树真的没了"的测试都要用这个形态,原因有三条:
    /// ①必须有孙进程——`taskkill /t` 存在的全部理由就是后代,只查根 pid 查不出 `/t` 有没有生效;
    /// ②前奏必须排在命令**最前面**,孙进程要早于被测事件(停止/越界)就位,否则测试拿不到
    ///   断言对象,只能退化成"只查根"而看不见真正的孤儿残留;
    /// ③孙 pid 走临时文件回传而不是 stdout:后台任务的 stdout 由注册表持续抽着,别去抢。
    ///
    /// 回传用 `WriteAllText` 而不是 `Set-Content`:后者被 bash 工具的整文件覆写规则拦掉。
    fn tree_prelude(tag: &str) -> (String, PathBuf) {
        let marker = std::env::temp_dir().join(format!(
            "kz-d262-bg-{tag}-{}-{}.txt",
            std::process::id(),
            now_ms()
        ));
        let marker_str = marker.display().to_string().replace('\\', "/");
        let prelude = match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => format!(
                "$c = Start-Process -NoNewWindow -PassThru -FilePath (Get-Process -Id $PID).Path \
                 -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 300'; \
                 [System.IO.File]::WriteAllText('{marker_str}', \"$($c.Id)\")"
            ),
            "cmd" => format!(
                "start /b ping -n 300 127.0.0.1 >nul & echo 0 > \"{marker_str}\""
            ),
            _ => format!("sleep 300 & echo $! > '{marker_str}'"),
        };
        (prelude, marker)
    }

    /// 命令尾巴:活得远比测试长。
    ///
    /// 会自然退出的命令会让"已终止"断言因为**错误的原因**通过——这正是 D-174 当时
    /// 拆掉断言的顾虑,所以这里一律 300 秒。
    fn linger() -> &'static str {
        match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 300",
            "cmd" => "ping -n 300 127.0.0.1 >nul",
            _ => "sleep 300",
        }
    }

    /// 读回孙进程 pid;拿不到就返回 None(cmd/POSIX 分支不回传真 pid)。
    async fn grandchild_pid(marker: &Path) -> Option<u32> {
        for _ in 0..200 {
            if let Ok(text) = std::fs::read_to_string(marker) {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    let _ = std::fs::remove_file(marker);
                    return if pid == 0 { None } else { Some(pid) };
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        None
    }

    /// pwsh/powershell 下孙 pid 必须真的拿到手。
    ///
    /// 拿不到就说明测试形态坏了(比如前奏没排在最前面),断言会静默退化成"只查根进程"
    /// ——那正好是 D-262 里孤儿后代能一直活下去的那个盲区,不能让它悄悄发生。
    fn require_grandchild(pid: Option<u32>) -> Option<u32> {
        if matches!(crate::shell::detected_shell().name, "pwsh" | "powershell") {
            assert!(
                pid.is_some(),
                "pwsh 下必须回传孙进程 pid,否则本测试会静默退化成只查根进程"
            );
        }
        pid
    }

    /// 断言整棵树都不在了(D-262 验收①的口径:问 pid 的活性,不问函数的返回值)。
    ///
    /// 终止是异步的,所以这里是"等到没"而不是"看一眼";等不到就如实红。
    async fn assert_tree_gone(root: u32, grandchild: Option<u32>, whose: &str) {
        let pids: Vec<u32> = [Some(root), grandchild].into_iter().flatten().collect();
        let gone = wait_until(
            || !pids.iter().copied().any(crate::shell::process_alive),
            15_000,
        )
        .await;
        let alive: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|p| crate::shell::process_alive(*p))
            .collect();
        // 残留会占着测试机 300 秒,先收拾再断言。
        for pid in &alive {
            crate::shell::kill_tree(*pid).await;
        }
        assert!(gone, "{whose}:进程树必须真的消失,残留 pid {alive:?}");
    }

    /// 场景①启动:托管项目里的后台任务必须带 owner、带启动瞬间的基线、并挂上守卫。
    #[tokio::test]
    async fn 场景启动_托管项目后台任务登记owner与基线() {
        let _serial = serial().lock().await;
        let root = temp_managed_project("start");
        let id = start_background(&root, "echo started", "run-start").await;

        let process = get(&id).expect("应登记在注册表");
        assert_eq!(process.owner.run_id, "run-start", "必须带 owner run_id");
        assert_eq!(process.owner.process_id, "proc-run-start");
        assert!(
            list(&root).iter().any(|p| p.id == id),
            "应出现在本项目的进程列表里"
        );
        // 基线是 spawn 之前拍的:此刻托管树没被动过,基线应与当前一致。
        assert!(
            crate::managed::diff(&process.baseline(), &ManagedSnapshot::capture(&root)).is_none(),
            "启动瞬间的基线应与托管树当前状态一致"
        );
        assert!(process.breaches().is_empty(), "刚启动不该有越界记录");
        std::fs::remove_dir_all(&root).ok();
    }

    /// 场景②轮询:正常后台任务被反复观察时,托管树逐字节不变、不产生越界记录。
    /// 这条挡的是"围栏太敏感,把没碰托管树的任务也报成越界"。
    #[tokio::test]
    async fn 场景轮询_读输出不误判托管树也不产生越界记录() {
        let _serial = serial().lock().await;
        let root = temp_managed_project("poll");
        let id = start_background(&root, "echo polled", "run-poll").await;
        let ctx = ctx_for(&root, "run-poll");

        // 反复轮询,跨过多个守卫周期。
        for _ in 0..4 {
            let out = crate::process::ProcessTool
                .execute(
                    serde_json::json!({"action": "output", "id": id.clone()}),
                    &ctx,
                )
                .await;
            assert!(!out.is_error, "{}", out.content);
            assert!(
                !out.content.contains("[managed-files]"),
                "没碰托管树的任务不该被报成越界: {}",
                out.content
            );
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
        let listed = crate::process::ProcessTool
            .execute(serde_json::json!({"action": "list"}), &ctx)
            .await;
        assert!(
            listed.content.contains("owner=run-poll"),
            "{}",
            listed.content
        );
        assert!(
            !listed.content.contains("managed-breaches"),
            "{}",
            listed.content
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei/project/defects.md")).unwrap(),
            ORIGINAL_DEFECTS,
            "托管文件必须逐字节不变"
        );
        assert!(get(&id).unwrap().breaches().is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    /// 场景③停止:stop 走终止路径、做终态对账,而且**进程树真的消失**。
    ///
    /// D-174 交付时这条测试有意不断言"进程真的死了":当时 `shell::kill_tree` 恒定 2.008 秒
    /// 后返回而目标毫发无损(已单独登记为 D-262),断言只会长期挂红,而且当时用的是 5 秒
    /// 短命令——就算断言也会因为"命令自然退出"这个**错误的原因**通过。
    ///
    /// D-262 修复后按正确形态补回,三处都换了口径:①命令改成 300 秒长驻,自然退出不可能
    /// 冒充击杀成功;②带一个孙进程,`taskkill /t` 没生效就会留下残留;③断言问的是 pid 的
    /// 活性(`process_alive`),不是 `stop` 的返回值——旧实现能让"断言函数返回"的测试满分通过。
    #[tokio::test]
    async fn 场景停止_走终止路径并做终态对账_进程树真的消失() {
        let _serial = serial().lock().await;
        let root = temp_managed_project("stop");
        let (prelude, marker) = tree_prelude("stop");
        let command = format!("{prelude}{}{}", joiner(), linger());
        let id = start_background(&root, &command, "run-stop").await;
        let process = get(&id).unwrap();
        assert!(process.is_running(), "命令此刻应在运行");
        let root_pid = process.pid.expect("后台任务应有 pid");
        let grandchild = require_grandchild(grandchild_pid(&marker).await);
        assert!(
            crate::shell::process_alive(root_pid),
            "测试前提:根进程此刻应在运行"
        );
        if let Some(gc) = grandchild {
            assert!(
                crate::shell::process_alive(gc),
                "测试前提:孙进程此刻应在运行"
            );
        }

        let ctx = ctx_for(&root, "run-stop");
        let out = crate::process::ProcessTool
            .execute(
                serde_json::json!({"action": "stop", "id": id.clone()}),
                &ctx,
            )
            .await;
        // stop 找到了活着的进程并走了终止 + 终态对账这条路径。
        assert!(out.content.contains("stopped"), "{}", out.content);
        // D-262 验收③:`stop` 报了 stopped,进程就必须真的没了——影响①正是"名义 stopped、实则还在跑"。
        assert_tree_gone(root_pid, grandchild, "process stop").await;
        // 终态对账不该凭空造出越界:这个任务没碰过托管树。
        assert!(
            get(&id).unwrap().breaches().is_empty(),
            "没碰托管树的任务不该在停止对账时被报成越界"
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei/project/defects.md")).unwrap(),
            ORIGINAL_DEFECTS,
            "停止路径不得改动托管树"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 场景④越界写入:后台任务在工具早已返回之后写托管文档,
    /// 必须被隔离、回滚、终止进程树,并留下能追到 owner 的归因记录。
    #[tokio::test]
    async fn 场景越界_后台写托管文档被隔离回滚并归因到owner_且进程树被终止() {
        let _serial = serial().lock().await;
        let root = temp_managed_project("breach");
        let target = root.join(".kanzei/project/defects.md");
        // D-262 之前这里用的是"写完就自然退出"的命令,所以"越界即终止"根本无法断言
        // (进程反正会自己走,断言会因为错误的原因通过)。现在的形态是
        // 「先起孙进程 → 再越界写 → 然后长驻 300 秒」:孙进程必须早于越界事件就位,
        // 否则守卫在孙进程出生前就把根杀了,测试既拿不到孙 pid,也看不见孤儿残留。
        let (prelude, marker) = tree_prelude("breach");
        let command = format!(
            "{prelude}{j}{}{j}{}",
            delayed_write_command(&target, "BREACH"),
            linger(),
            j = joiner()
        );
        let id = start_background(&root, &command, "run-breach").await;
        let breach_root_pid = get(&id).unwrap().pid.expect("后台任务应有 pid");
        // 趁树还活着把孙 pid 拿到手:守卫杀完之后 marker 就再也不会被写了。
        let breach_grandchild = require_grandchild(grandchild_pid(&marker).await);
        // 工具已经返回,写入还没发生——这正是 D-174 描述的时序。
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            ORIGINAL_DEFECTS,
            "启动时刻还不该有写入"
        );

        let process = get(&id).unwrap();
        assert!(
            wait_until(|| !process.breaches().is_empty(), 10_000).await,
            "守卫应在若干个周期内抓到越界写入"
        );

        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            ORIGINAL_DEFECTS,
            "越界写入必须被回滚到基线内容"
        );
        let breach = process.breaches().remove(0);
        assert!(
            breach.touched.iter().any(|p| p.ends_with("defects.md")),
            "归因记录要点名被改的路径: {:?}",
            breach.touched
        );
        assert!(breach.restored >= 1, "至少回滚一个文件");
        assert!(
            Path::new(&breach.quarantine).is_dir(),
            "改后的内容必须留证在 {}",
            breach.quarantine
        );
        // D-262 补回的断言:越界即终止。命令是 300 秒长驻的,自然退出冒充不了击杀;
        // 守卫的 reconcile(kill_on_breach=true) 必须把整棵树杀掉,否则后台进程会持续
        // 重试写入,变成"只回滚不终止"的无限回滚循环(reconcile 的文档注释即此意)。
        assert_tree_gone(breach_root_pid, breach_grandchild, "越界即终止").await;

        // 归因必须能顺着 process 工具读出来,而不是只留在内存里。
        let out = crate::process::ProcessTool
            .execute(
                serde_json::json!({"action": "output", "id": id.clone()}),
                &ctx_for(&root, "run-breach"),
            )
            .await;
        assert!(out.content.contains("[managed-files]"), "{}", out.content);
        assert!(
            out.content.contains("owner run=run-breach"),
            "{}",
            out.content
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// 场景⑤并发合法写入:后台任务运行期间,专用工具在**合法写入窗口内**改托管
    /// 文档不得被回滚(验收③)。
    ///
    /// 对照组是这条测试的重点:同样的写入放在**窗口外**必须被回滚。少了对照组,
    /// 一个"恒真放行"的实现照样能让上半段假绿。
    #[tokio::test]
    async fn 场景并发_窗口内合法写入不误伤_窗口外同样写入被回滚() {
        let _serial = serial().lock().await;
        let root = temp_managed_project("concurrent");
        let target = root.join(".kanzei/project/defects.md");
        let long_running = match crate::shell::detected_shell().name {
            "pwsh" | "powershell" => "Start-Sleep -Seconds 30",
            "cmd" => "ping -n 30 127.0.0.1 >nul",
            _ => "sleep 30",
        };
        let id = start_background(&root, long_running, "run-conc").await;
        let process = get(&id).unwrap();

        // —— 窗口内:模拟 `defect` 工具的写入区间。
        const LEGIT: &str = "# Defects\n\n## D-001 专用工具改的 [fixing]\n";
        kanzei_harness::managed_fence::tool_scope("defect", async {
            std::fs::write(&target, LEGIT).unwrap();
        })
        .await;
        // 跨过好几个守卫周期,确认它不会被回滚。
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            LEGIT,
            "窗口内的合法写入被误伤了——这正是验收③要防的"
        );
        assert!(
            process.breaches().is_empty(),
            "合法写入不该产生越界记录: {:?}",
            process.breaches()
        );
        assert!(process.is_running(), "合法写入不该牵连后台进程");

        // —— 对照组:同样的写入,不开窗口。
        std::fs::write(&target, "# Defects\n\n## D-001 窗口外偷改 [open]\n").unwrap();
        assert!(
            wait_until(|| !process.breaches().is_empty(), 10_000).await,
            "窗口外的写入必须被判越界,否则上半段的通过毫无意义"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            LEGIT,
            "窗口外写入应回滚到上一次合法写入的内容(基线已被窗口吸收)"
        );
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
            test_owner(),
            ManagedSnapshot::capture(&root),
        );
        assert!(get(&handle.id).is_some(), "进程应登记在注册表");
        assert_eq!(handle.owner, test_owner(), "后台任务必须带 owner 身份");
        assert!(handle.breaches().is_empty(), "未越界时不该有归因记录");
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
