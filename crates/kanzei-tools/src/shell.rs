//! shell 检测(移植 opencode core/src/shell.ts 的精简版)。
//! Windows 优先级:pwsh → powershell → cmd(git-bash 留到 M5,避免 MSYS 环境歧义)。

use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct DetectedShell {
    /// 展示名,如 "pwsh" / "powershell" / "cmd" / "sh"。
    pub name: &'static str,
    /// 可执行文件。
    pub program: PathBuf,
    /// 执行一条命令所需的前置参数,命令文本追加在最后。
    pub args: Vec<&'static str>,
}

pub fn detected_shell() -> &'static DetectedShell {
    static SHELL: OnceLock<DetectedShell> = OnceLock::new();
    SHELL.get_or_init(detect)
}

#[cfg(windows)]
fn detect() -> DetectedShell {
    if let Some(p) = which("pwsh.exe") {
        return DetectedShell {
            name: "pwsh",
            program: p,
            // D-582:子进程不继承交互会话的 Process=Bypass,裸 spawn 落回
            // LocalMachine/用户策略,遇到不满足签名要求的 .ps1 文件调用(如
            // `& .\scripts\verify.ps1`)会被 AuthorizationManager 直接拒绝、
            // 0 秒失败、脚本第一行都不执行。显式 Bypass 只影响本子进程。
            args: vec![
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
            ],
        };
    }
    if let Some(p) = which("powershell.exe") {
        return DetectedShell {
            name: "powershell",
            program: p,
            args: vec![
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
            ],
        };
    }
    let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
    DetectedShell {
        name: "cmd",
        program: comspec.into(),
        args: vec!["/d", "/s", "/c"],
    }
}

#[cfg(not(windows))]
fn detect() -> DetectedShell {
    for candidate in ["/bin/bash", "/bin/zsh", "/bin/sh"] {
        if std::path::Path::new(candidate).exists() {
            let name: &'static str = match candidate {
                "/bin/bash" => "bash",
                "/bin/zsh" => "zsh",
                _ => "sh",
            };
            return DetectedShell {
                name,
                program: candidate.into(),
                args: vec!["-c"],
            };
        }
    }
    DetectedShell {
        name: "sh",
        program: "/bin/sh".into(),
        args: vec!["-c"],
    }
}

#[cfg(windows)]
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 目标 pid 此刻是否还活着。
///
/// D-262 的验收判据是「进程树真的消失」而不是「击杀函数返回了」,这就需要一个能对
/// **任意 pid**(不只是自己持有的 `Child`)提问的活性原语:`Child::try_wait` 只能问
/// 直接子进程,问不了孙进程,也问不了别的 run 留下的后台进程。
///
/// Windows 实现用 `OpenProcess` + `GetExitCodeProcess`:句柄开不出来(内核对象已回收)
/// 或退出码不再是 `STILL_ACTIVE` 都算已死。只申请 `QUERY_LIMITED_INFORMATION`,
/// 这样即使目标已进入"退出但句柄未释放"的中间态也问得出真实退出码。
///
/// 已知边界:pid 会被系统复用,极端情况下可能把"新占用同一 pid 的进程"误判为目标还活着。
/// 用于击杀确认足够——误判方向是偏保守(报"没杀干净"),不会假绿。
#[cfg(windows)]
pub fn process_alive(pid: u32) -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, pid: u32) -> isize;
        fn GetExitCodeProcess(handle: isize, exit_code: *mut u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    // SAFETY: 三个调用都只读进程状态;句柄由本函数开、本函数关,不外泄。
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return false;
        }
        let mut code: u32 = 0;
        let queried = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        queried != 0 && code == STILL_ACTIVE
    }
}

/// 非 Windows 下 D-262 只保证可编译:`kill_tree` 本身仍是待实现的空实现,
/// 活性查询没有真源可问,一律报"未确认存活"。
#[cfg(not(windows))]
pub fn process_alive(pid: u32) -> bool {
    let _ = pid;
    false
}

/// 击杀**之前**把整棵进程树的 pid 拍下来(含 `root` 自己,广度优先)。
///
/// 为什么必须先拍:D-262 的验收是「进程树真的消失」,而击杀之后就再也问不出树的形状了
/// ——父子关系随着进程消失一起消失。只查根 pid 的确认会漏掉真正的残留:实测就抓到过
/// 「根已消失、孙进程还在跑」的窗口(终止是异步的,`taskkill /t` 打印成功不等于此刻已没)。
///
/// 与 `taskkill /t` 同口径:都按 `ParentProcessId` 认后代,所以拍到的集合就是 taskkill
/// 会去处理的集合。两条共同的固有边界:①快照之后新生的子进程不在集合里(`kill_tree`
/// 用"重拍几轮"来收窄这个缺口);②pid 被系统复用时,可能把无关进程认成后代。
///
/// ②要当真:这个集合不只是用来确认死活,`kill_tree` 会逐个终止它。但风险并不比
/// `taskkill /t` 大——taskkill 用的是同一条 ppid 规则,而且它的快照是在自己启动**之后**
/// 才拍的(实测启动就要 1 秒到十几秒),我们这里从拍到杀只隔微秒级,窗口反而更紧。
#[cfg(windows)]
fn process_tree(root: u32) -> Vec<u32> {
    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        fn Process32FirstW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const INVALID_HANDLE_VALUE: isize = -1;

    let mut pairs: Vec<(u32, u32)> = Vec::new();
    // SAFETY: 快照句柄由本函数开、本函数关;entry 是本地栈变量,dw_size 按 API 要求填好。
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot == 0 {
            return vec![root];
        }
        let mut entry: ProcessEntry32W = std::mem::zeroed();
        entry.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                pairs.push((entry.th32_process_id, entry.th32_parent_process_id));
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }

    let mut tree = vec![root];
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for (pid, ppid) in &pairs {
            // pid 0 是 System Idle 的占位;它当不了谁的后代,顺手挡掉自环。
            if *ppid == parent && *pid != parent && *pid != 0 && !tree.contains(pid) {
                tree.push(*pid);
                frontier.push(*pid);
            }
        }
    }
    tree
}

/// 终止**单个** pid。这是 D-262 之后的主击杀手段。
///
/// 它自己不认识"树",所以必须配 [`process_tree`] 一起用:先拍出整棵树的名单,再对名单里
/// 每个成员各来一发。这样不依赖任何外部可执行文件——一次系统调用,微秒级,没有进程创建,
/// 也就没有 `taskkill.exe` 那种"机器越忙越起不动"的病(实测忙时启动就要十几秒)。
///
/// 返回 false 的常见原因是"这一瞬间它已经自己走了"(`OpenProcess` 开不出句柄),
/// 所以调用方不要把它当失败信号看;成败一律由随后的活性确认判定。
#[cfg(windows)]
fn terminate_process(pid: u32) -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, pid: u32) -> isize;
        fn TerminateProcess(handle: isize, exit_code: u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    const PROCESS_TERMINATE: u32 = 0x0001;
    // SAFETY: 句柄由本函数开、本函数关;TerminateProcess 只作用于该句柄。
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle == 0 {
            return false;
        }
        let killed = TerminateProcess(handle, 1);
        CloseHandle(handle);
        killed != 0
    }
}

/// 兜底那发 taskkill 自身跑完的等待上限。
///
/// **旧实现的 2 秒是实测过低的**(D-262 次因实证):本机 `taskkill.exe` 光进程启动就要
/// 1.0–4.2 秒(对一个不存在的 pid 连打三次:2907ms / 4230ms / 1071ms),机器忙时实测
/// 超过 15 秒。2 秒那条线正压在延迟分布中间,一忙就必然踩空。
///
/// 注意这个常量现在只管兜底路径:主手段是 `TerminateProcess`,不用等任何进程启动。
/// 它也只是**可见性上限**而非成功判据——判据是 `process_alive` 说整棵树都没了(见 `kill_tree`)。
#[cfg(windows)]
const TASKKILL_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// 击杀之后再给内核多少时间收尾。
///
/// 终止是异步的:`TerminateProcess` 返回成功、`taskkill` 打印"成功已终止"并退出码 0,
/// 都不代表此刻进程对象已经消失。实测抓到过根已消失而孙进程还在的窗口(第一版修复就是
/// 在这里假绿过一次)——所以确认必须是"等到没",不是"看一眼"。
#[cfg(windows)]
const DEATH_CONFIRM_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(windows)]
const ALIVE_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// 每一轮击杀之后给内核的回收时间;等不到就再拍一轮名单。
#[cfg(windows)]
const ROUND_SETTLE: std::time::Duration = std::time::Duration::from_millis(200);

/// 跑一发 `taskkill /pid <pid> /t /f`,把失败信号如实记出来。
///
/// 不返回成败:taskkill 的退出码不是判据(128 = 找不到进程,在"目标刚好自然退出"时是
/// 正常结果),真正的判据是调用方随后对整棵树做的活性确认。
#[cfg(windows)]
async fn run_taskkill(pid: u32) {
    let mut command = tokio::process::Command::new("taskkill");
    command
        .args(["/pid", &pid.to_string(), "/t", "/f"])
        // 输出无人消费就别建管道:顺手除掉"后代继承管道导致读端等不到 EOF"这一类风险。
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // kill_on_drop 保持默认的 false:打开它正是 D-262 的根因,不要再动。
    crate::hide_console_async(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            tracing::warn!(
                target_pid = pid,
                error = %err,
                "kill_tree: taskkill 启动失败,只能靠 TerminateProcess 兜底"
            );
            return;
        }
    };
    match tokio::time::timeout(TASKKILL_WAIT, child.wait()).await {
        Ok(Ok(status)) if status.success() => {}
        Ok(Ok(status)) => {
            tracing::warn!(
                target_pid = pid,
                exit_code = ?status.code(),
                "kill_tree: taskkill 非零退出"
            );
        }
        Ok(Err(err)) => {
            tracing::warn!(target_pid = pid, error = %err, "kill_tree: 等待 taskkill 失败");
        }
        Err(_) => {
            // 关键:这里**只是不再等**。child 被 drop 时不会杀 taskkill(kill_on_drop=false),
            // taskkill 会继续跑完它的活;放弃等待不等于放弃击杀。
            tracing::warn!(
                target_pid = pid,
                wait_ms = TASKKILL_WAIT.as_millis(),
                "kill_tree: taskkill 超出等待上限,不再等待(它仍在后台运行)"
            );
        }
    }
}

/// 等到 `pids` 全部消失,或等到 `budget` 用尽。返回**仍然活着**的那些。
#[cfg(windows)]
async fn await_tree_death(pids: &[u32], budget: std::time::Duration) -> Vec<u32> {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        let alive: Vec<u32> = pids.iter().copied().filter(|p| process_alive(*p)).collect();
        if alive.is_empty() || tokio::time::Instant::now() >= deadline {
            return alive;
        }
        tokio::time::sleep(ALIVE_POLL).await;
    }
}

/// 进程树击杀(Windows 下 tokio 的 `Child::kill` 只杀直接子进程)。
///
/// 返回 **true 仅当确认整棵树(根 + 击杀前拍到的全部后代)都已经不在了**。调用方可以忽略
/// 返回值(现有调用点都忽略),失败路径一律另有 `tracing::warn!`——D-262 之前失败被
/// `let _ =` 吞得一丝不剩,`process stop` 于是能一边报 stopped 一边把进程留在那儿跑。
///
/// ## D-262:这里为什么不再有 `kill_on_drop` 和 `output()`
///
/// 旧实现是 `timeout(2s, command.output())` + `kill_on_drop(true)`。两者叠加的实际行为是
/// 「启动 taskkill → 两秒后把 taskkill 自己杀掉 → 返回」:超时丢弃 future 的那一刻,
/// `kill_on_drop` 杀的是清理者本人,目标进程树毫发无伤。
///
/// 修复前实测证实/证伪了两条候选次因(独立复现程序,Windows 11 + pwsh,每档都测
/// 「victim 何时死」而不是「函数何时返回」):
/// - **证伪**「`output()` 等管道 EOF 而管道被孙进程继承着,所以 taskkill 挂住」。
///   读管道 / 不读管道 / `status()`+`stdio(null)` 三档耗时 1097–1139ms,毫无差别;
///   拆开测量时 taskkill 进程退出与 stdout EOF 落在**同一毫秒**(1172ms/1172ms)。
///   管道从来不是阻塞点——`taskkill` 的管道是它自己 spawn 时才建的,时间上晚于目标树,
///   目标树不可能继承到。
/// - **证实**真正的次因是 `taskkill.exe` 的启动延迟本身量级就在秒级且方差极大
///   (1.0–4.2 秒),2 秒超时压在分布中间。冷启动那一发 taskkill 没跑完就被
///   `kill_on_drop` 打断,目标存活;后续热启动的几发在 2 秒内跑完,目标就死了——
///   这解释了这个缺陷为什么表现得像"偶尔能用"。
///
/// ## 为什么 taskkill 从"主手段"降级成"兜底"
///
/// 顺着上面的实测再往下量,taskkill 的启动延迟在**机器忙的时候**根本不是秒级而是十几秒:
/// 同一台机器上跑三条并行开发线时,一次 `kill_tree` 实测 20.04 秒,其中 15 秒是 taskkill
/// 超出等待上限、最后由 `TerminateProcess` 收尾的。这也顺带解释了缺陷证据②里那个"约 27 秒"
/// ——它不是 taskkill 被管道挂住,就是负载下的进程创建延迟。
///
/// 而这正是这条路径最要命的结构问题:**需要击杀进程的时刻,往往就是机器忙得起不动新进程的
/// 时刻**。把清理动作建立在"再 spawn 一个 exe"上,等于在最需要它的时候最不可用。
/// 所以现在的主手段是「先拍进程树名单,再逐个 `TerminateProcess`」——一次系统调用,
/// 微秒级,没有进程创建,没有冷启动方差,顺带也不会有 D-238 的控制台窗口问题。
/// taskkill 只留作兜底:万一有我们开不出句柄的成员(权限/保护进程),它还有一点机会。
///
/// 综上,修法是这几条一起改:
/// 1. **不再 `kill_on_drop`**。等不及了就只是停止等待,taskkill 继续在后台跑完它的活;
///    清理者的生命周期不再挂在等待者的耐心上。
/// 2. **成功判据换成整棵树真的消失**(`process_tree` + `process_alive`),不再看 taskkill
///    的退出码——退出码只用来产生可见信号。
/// 3. 击杀主手段换成快照 + `TerminateProcess`,不再依赖 spawn。
/// 4. 不建无人消费的管道(`stdio` 全 null),顺手把"后代继承管道"这一整类风险除掉,
///    虽然实测它不是本例的原因。
///
/// ## 为什么"根进程已经死了"不等于可以直接返回成功
///
/// bash 超时路径上,`tokio::time::timeout` 丢弃 capture future 的那一刻,shell child 的
/// `kill_on_drop(true)` 已经把 shell 自己杀了——`kill_tree` 拿到的根 pid 往往**本来就不在了**。
/// 此时若直接报成功,孙进程就永远留在那儿:这正是 D-262 影响②说的"被击杀的进程继续持有
/// 文件与端口"。`taskkill /t` 在这种局面下也无从下手,它要从一个活着的根往下走。
/// 快照这条路走得通:Windows 的 `PROCESSENTRY32` 保留创建者 pid,父进程死掉之后后代依然
/// 认得出来,孤儿一样拍得到、一样收得掉。
pub async fn kill_tree(pid: u32) -> bool {
    #[cfg(windows)]
    {
        // 每轮重拍名单再杀。重拍是为了收掉上一轮击杀间隙里新生的后代——`taskkill /t`
        // 有同样的固有缺口,区别只是它连缺口都不报。三轮足够收敛:根第一轮就没了,
        // 之后不会再有新的分支冒出来。
        let mut known: Vec<u32> = Vec::new();
        for _ in 0..3 {
            let tree = process_tree(pid);
            for member in &tree {
                if !known.contains(member) {
                    known.push(*member);
                }
            }
            let mut live: Vec<u32> = tree.into_iter().filter(|p| process_alive(*p)).collect();
            if live.is_empty() {
                break;
            }
            // 根排到最前面:先掐掉还在继续 spawn 的那个,后代才不会边杀边生。
            live.sort_by_key(|member| *member != pid);
            for stray in live {
                // 返回 false 常常只是"这一瞬间它已经自己走了",所以这里不逐个告警;
                // 成败一律由下面的活性确认统一判定。
                terminate_process(stray);
            }
            if await_tree_death(&known, ROUND_SETTLE).await.is_empty() {
                break;
            }
        }

        // 成功判据只有这一条:拍下来的整棵树都不在了。
        let alive = await_tree_death(&known, DEATH_CONFIRM_WAIT).await;
        if alive.is_empty() {
            return true;
        }

        tracing::warn!(
            target_pid = pid,
            still_alive = ?alive,
            "kill_tree: TerminateProcess 未清干净,退回 taskkill 兜底"
        );
        if process_alive(pid) {
            run_taskkill(pid).await;
        }
        let alive = await_tree_death(&known, DEATH_CONFIRM_WAIT).await;
        if !alive.is_empty() {
            tracing::warn!(
                target_pid = pid,
                still_alive = ?alive,
                "kill_tree: 全部手段用尽,进程树仍有残留"
            );
        }
        alive.is_empty()
    }
    #[cfg(not(windows))]
    {
        // 非 Windows 仍是空实现(D-262 只负责 Windows 这条真实失效路径),
        // 但不再假装成功:返回 false = 未确认目标已终止。
        tracing::warn!(
            target_pid = pid,
            "kill_tree: 非 Windows 平台尚未实现进程树击杀,目标未被终止"
        );
        false
    }
}

/// D-262 验收①:断言的是「目标进程树真的消失」,不是「函数返回了」。
///
/// 旧实现能让任何"断言函数返回"的测试满分通过——它恒定 2.008 秒后返回,目标毫发无伤。
/// 所以这一组测试一律用 [`process_alive`] 对 pid 直接提问,而且必须把**孙进程**一起问:
/// `taskkill /t` 存在的全部理由就是后代,只查根 pid 的测试查不出 `/t` 有没有生效。
/// D-582:循环宿主(非交互式,不继承任何 Process=Bypass)裸 spawn 出的 pwsh/powershell
/// 子进程执行本地 .ps1 文件曾被 AuthorizationManager 在第 0 秒拒绝——本测试进程本身
/// 不是 PowerShell 宿主,子进程的执行策略只由 [`detected_shell`] 传的参数决定,贴近
/// 真实复现场景;回归目标是 `-ExecutionPolicy Bypass` 落地后该调用真实跑通。
#[cfg(all(test, windows))]
mod execution_policy_tests {
    use super::*;

    #[tokio::test]
    async fn spawned_shell_runs_local_ps1_file_without_inherited_bypass() {
        let shell = detected_shell();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let script_path =
            std::env::temp_dir().join(format!("kz-d582-probe-{}-{nanos}.ps1", std::process::id()));
        std::fs::write(&script_path, "Write-Output 'kz-d582-probe-ok'\r\n").unwrap();
        let command_text = format!("& '{}'", script_path.display());
        let mut command = tokio::process::Command::new(&shell.program);
        command
            .args(&shell.args)
            .arg(&command_text)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        crate::hide_console_async(&mut command);
        let output = command.output().await.expect("子进程应能启动");
        let _ = std::fs::remove_file(&script_path);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            output.status.success(),
            "裸 spawn 执行本地 .ps1 文件失败(D-582 复发形态):status={:?}, stdout={stdout}, stderr={stderr}",
            output.status
        );
        assert!(
            stdout.contains("kz-d582-probe-ok"),
            "脚本首行未执行,stdout={stdout}, stderr={stderr}"
        );
    }

    /// D-582 原始复现用的正是 `& .\scripts\verify.ps1`(T-1786922726507)。这里直接
    /// 用真实文件复现同一调用,只断言不再命中 AuthorizationManager 门槛——工作树在
    /// 开发中通常不干净,脚本自身会在拿到执行权之后抛"工作树不干净"提前退出,那是
    /// 预期内的、脚本逻辑内部的失败,不是本缺陷要修的"脚本第一行都跑不到"。
    #[tokio::test]
    async fn real_verify_ps1_clears_authorization_gate() {
        let shell = detected_shell();
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let mut command = tokio::process::Command::new(&shell.program);
        command
            .args(&shell.args)
            .arg("& .\\scripts\\verify.ps1")
            .current_dir(&repo_root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        crate::hide_console_async(&mut command);
        let child = command.spawn().expect("子进程应能启动");
        // 只要脚本跑起来就已经证伪 AuthorizationManager 门槛;不必等 13 步门禁跑完,
        // 给够拿到执行权+跑到"工作树不干净"检查的时间就够,超时直接判失败并杀进程。
        let wait =
            tokio::time::timeout(std::time::Duration::from_secs(20), child.wait_with_output());
        let output = match wait.await {
            Ok(result) => result.expect("等待子进程输出不应报错"),
            Err(_) => panic!("20 秒内脚本既未成功也未报出预期错误,判超时失败"),
        };
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            !stderr.contains("AuthorizationManager") && !stdout.contains("AuthorizationManager"),
            "D-582 复发:仍被 AuthorizationManager 挡在第一行之前。stdout={stdout}, stderr={stderr}"
        );
    }
}

#[cfg(all(test, windows))]
mod windows_kill_tree_tests {
    use super::*;
    use std::time::Duration;

    fn powershell() -> Option<PathBuf> {
        which("pwsh.exe").or_else(|| which("powershell.exe"))
    }

    /// 起一棵真有孙进程的目标树,返回 `(根 Child, 根 pid, 孙 pid)`。
    ///
    /// 形态刻意贴近 bash.rs 的长命令:根进程 stdout/stderr 是管道,孙进程用
    /// `Start-Process -NoNewWindow` 起因而继承同一套管道。孙 pid 经临时文件回传——
    /// 不走 stdout,免得读管道这件事本身影响被测行为。
    async fn spawn_victim_tree(tag: &str) -> Option<(tokio::process::Child, u32, u32)> {
        let shell = powershell()?;
        let marker = std::env::temp_dir().join(format!(
            "kz-d262-gc-{tag}-{}-{}.txt",
            std::process::id(),
            now_nanos()
        ));
        let _ = std::fs::remove_file(&marker);
        // 睡得远比测试需要的久:目标绝不能因为"自然退出"而让断言假绿。
        let script = format!(
            "$c = Start-Process -NoNewWindow -PassThru -FilePath '{}' -ArgumentList \
             '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 300'; \
             Set-Content -LiteralPath '{}' -Value $c.Id; Start-Sleep -Seconds 300",
            shell.display(),
            marker.display()
        );
        let mut command = tokio::process::Command::new(&shell);
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        crate::hide_console_async(&mut command);
        let child = command.spawn().expect("目标树应能启动");
        let root = child.id().expect("目标树根 pid");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(text) = std::fs::read_to_string(&marker) {
                if let Ok(grandchild) = text.trim().parse::<u32>() {
                    let _ = std::fs::remove_file(&marker);
                    return Some((child, root, grandchild));
                }
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "孙进程 60 秒内没起来,测试前提不成立"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn now_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    }

    /// 兜底清理:测试自身失败时不要给测试机留 300 秒的残留进程。
    fn force_cleanup(pids: [u32; 2]) {
        for pid in pids {
            if process_alive(pid) {
                let _ = std::process::Command::new("taskkill")
                    .args(["/pid", &pid.to_string(), "/t", "/f"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn kill_tree_让目标进程树真的消失_含孙进程() {
        let Some((mut child, root, grandchild)) = spawn_victim_tree("tree").await else {
            eprintln!("跳过:本机没有 pwsh/powershell");
            return;
        };
        assert!(process_alive(root), "测试前提:根进程此刻应在运行");
        assert!(process_alive(grandchild), "测试前提:孙进程此刻应在运行");

        // 击杀前先确认树形:孙进程必须真的挂在根下面,否则本测试测不到 `/t`。
        assert!(
            process_tree(root).contains(&grandchild),
            "测试前提:孙进程 {grandchild} 应是 {root} 的后代"
        );

        let started = std::time::Instant::now();
        let confirmed = kill_tree(root).await;
        let elapsed = started.elapsed();

        let (root_alive, grandchild_alive) = (process_alive(root), process_alive(grandchild));
        force_cleanup([root, grandchild]);
        let _ = child.start_kill();

        // 这条测试对机器负载敏感(taskkill 启动本身就是秒级),留一行实测耗时便于将来定位。
        eprintln!("kill_tree 实测耗时 {elapsed:?}(confirmed={confirmed})");
        assert!(confirmed, "kill_tree 应确认目标已终止");
        assert!(
            !root_alive,
            "D-262 验收①:根进程必须真的消失,而不是函数返回就算完"
        );
        assert!(
            !grandchild_alive,
            "D-262 验收①:孙进程必须一起消失——/t 不生效的话这里就是残留"
        );
        // 旧实现的签名症状是"恒定 2.008 秒后返回且目标存活"。这里不设下限(杀干净了越快越好),
        // 只设上限:超过等待预算说明兜底链路也没兜住。
        assert!(
            elapsed < TASKKILL_WAIT + DEATH_CONFIRM_WAIT * 2,
            "kill_tree 耗时 {elapsed:?} 超出预算"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_alive_能区分在跑和已退出_且已退出时kill_tree直接确认() {
        let mut command = tokio::process::Command::new("cmd");
        command
            .args(["/d", "/s", "/c", "exit 0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::hide_console_async(&mut command);
        let mut child = command.spawn().expect("cmd 应能启动");
        let pid = child.id().expect("pid");
        child.wait().await.expect("等待退出");
        // 注意:`child` 一直没被 drop,进程句柄还在,Windows 因此不会复用这个 pid——
        // 下面两条断言不会被 pid 复用污染,更不会让 kill_tree 打到无辜进程上。
        assert!(!process_alive(pid), "已退出的进程必须报 not alive");

        let started = std::time::Instant::now();
        let confirmed = kill_tree(pid).await;
        let elapsed = started.elapsed();
        assert!(confirmed, "目标已经不在了,kill_tree 应直接确认成功");
        // 短路生效才可能这么快:本机 taskkill 光启动就要 1.0–4.2 秒(见 kill_tree 文档)。
        assert!(
            elapsed < Duration::from_millis(500),
            "应短路,不该真去起 taskkill:{elapsed:?}"
        );
        drop(child);
    }
}
