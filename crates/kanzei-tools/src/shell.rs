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
            args: vec!["-NoProfile", "-NonInteractive", "-Command"],
        };
    }
    if let Some(p) = which("powershell.exe") {
        return DetectedShell {
            name: "powershell",
            program: p,
            args: vec!["-NoProfile", "-NonInteractive", "-Command"],
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
/// 会去处理的集合。两条共同的固有边界:①快照之后新生的子进程不在集合里;②pid 复用可能
/// 把无关进程认成后代。②只会让确认结果偏保守(多报一次"没杀干净"),不会让它假绿,
/// 而且兜底击杀只打根 pid,绝不会顺着这个集合去杀无辜进程。
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

/// 最后兜底:直接对根 pid 调 `TerminateProcess`。
///
/// 只在 taskkill 那条路彻底走不通时用。它**不杀树**(孙进程会变孤儿),所以绝不能
/// 放在 taskkill 之前——`taskkill /t` 是靠父子关系找后代的,先砍根就把树砍散了。
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

/// taskkill 自身跑完的等待上限。
///
/// **2 秒是实测过低的**(D-262 次因实证):本机 `taskkill.exe` 光进程启动就要
/// 1.0–4.2 秒(对一个不存在的 pid 连打三次:2907ms / 4230ms / 1071ms),2 秒这条线
/// 正压在延迟分布中间,机器一忙就必然踩空。这里给的余量按实测尾部再翻几倍;它只是
/// **可见性上限**,不是成功判据——判据是 `process_alive` 说目标没了(见 `kill_tree`)。
#[cfg(windows)]
const TASKKILL_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// taskkill 返回之后再给内核多少时间收尾。
///
/// 终止是异步的:`taskkill` 打印"成功已终止"、退出码 0,并不代表此刻进程对象已经消失。
/// 实测抓到过根进程已消失而孙进程还在的窗口——所以确认必须是"等到没",不是"看一眼"。
#[cfg(windows)]
const DEATH_CONFIRM_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(windows)]
const ALIVE_POLL: std::time::Duration = std::time::Duration::from_millis(50);

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
/// 返回 **true 仅当确认目标 pid 已经不在了**。调用方可以忽略返回值(现有调用点都忽略),
/// 失败路径一律另有 `tracing::warn!`——D-262 之前失败被 `let _ =` 吞得一丝不剩。
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
/// 所以修法不是把 2 秒改成 30 秒,而是三条一起改:
/// 1. **不再 `kill_on_drop`**。等不及了就只是停止等待,taskkill 继续在后台跑完它的活;
///    清理者的生命周期不再挂在等待者的耐心上。
/// 2. **成功判据换成目标真的消失**(`process_alive`),不再看 taskkill 的退出码——
///    退出码只用来产生可见信号。
/// 3. 不建无人消费的管道(`stdio` 全 null),顺手把"孙进程继承管道"这一整类风险除掉,
///    虽然实测它不是本例的原因。
/// 4. taskkill 走完还没死,再用 `TerminateProcess` 兜根 pid,并把结果如实报出去。
pub async fn kill_tree(pid: u32) -> bool {
    #[cfg(windows)]
    {
        // 已经不在了就别去打扰 taskkill:它一次调用就是秒级开销。
        if !process_alive(pid) {
            return true;
        }
        // 先拍树:击杀之后父子关系就没了,再想确认后代已经无从下手。
        let tree = process_tree(pid);
        let mut command = tokio::process::Command::new("taskkill");
        command
            .args(["/pid", &pid.to_string(), "/t", "/f"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // kill_on_drop 保持默认的 false:这正是 D-262 的根因所在,不要再打开。
        crate::hide_console_async(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                tracing::warn!(
                    target_pid = pid,
                    error = %err,
                    "kill_tree: taskkill 启动失败,改用 TerminateProcess 兜底"
                );
                if !terminate_process(pid) {
                    tracing::warn!(
                        target_pid = pid,
                        "kill_tree: TerminateProcess 也失败,进程树未被终止"
                    );
                }
                let alive = await_tree_death(&tree, DEATH_CONFIRM_WAIT).await;
                if !alive.is_empty() {
                    tracing::warn!(
                        target_pid = pid,
                        still_alive = ?alive,
                        "kill_tree: 无 taskkill 可用,进程树残留(TerminateProcess 只兜根进程)"
                    );
                }
                return alive.is_empty();
            }
        };

        match tokio::time::timeout(TASKKILL_WAIT, child.wait()).await {
            Ok(Ok(status)) if status.success() => {}
            Ok(Ok(status)) => {
                // 128 = 目标已不存在(taskkill 找不到进程),这在"进程刚好自然退出"时是正常的,
                // 死活由下面的 await_death 判,所以这里只记一笔而不下结论。
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
                // taskkill 会继续跑完;放弃等待不等于放弃击杀。
                tracing::warn!(
                    target_pid = pid,
                    wait_ms = TASKKILL_WAIT.as_millis(),
                    "kill_tree: taskkill 超出等待上限,不再等待(它仍在后台运行)"
                );
            }
        }

        // 成功判据只有这一条:拍下来的整棵树都不在了。taskkill 的退出码只进日志。
        let alive = await_tree_death(&tree, DEATH_CONFIRM_WAIT).await;
        if alive.is_empty() {
            return true;
        }
        tracing::warn!(
            target_pid = pid,
            still_alive = ?alive,
            "kill_tree: taskkill 之后仍有进程存活,改用 TerminateProcess 兜底(仅根进程,不含后代)"
        );
        if alive.contains(&pid) && !terminate_process(pid) {
            tracing::warn!(target_pid = pid, "kill_tree: TerminateProcess 兜底失败");
        }
        let alive = await_tree_death(&tree, DEATH_CONFIRM_WAIT).await;
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
        assert!(!root_alive, "D-262 验收①:根进程必须真的消失,而不是函数返回就算完");
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
        assert!(elapsed < Duration::from_millis(500), "应短路,不该真去起 taskkill:{elapsed:?}");
        drop(child);
    }
}
