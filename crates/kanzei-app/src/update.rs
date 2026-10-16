//! 更新命令与版本检查。

use std::path::Path;
use std::process::Command;

use serde_json::json;

use kanzei_harness::KanzeiConfig;
use kanzei_llm::ProxyConfig;

mod receipt;
use receipt::{Phase, UpdateReceipt};

pub(crate) fn pending_path(exe: &Path) -> std::path::PathBuf {
    let name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("kzapp.exe");
    exe.with_file_name(format!("{name}.pending"))
}

pub(crate) fn validate_installer(bytes: &[u8]) -> Result<(), String> {
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

pub(crate) fn update_helper_path() -> std::path::PathBuf {
    std::env::temp_dir().join("kanzei-update-helper.exe")
}
pub(crate) fn update_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("kanzei-update.log")
}
pub(crate) fn update_log(line: &str) {
    update_log_at(&update_log_path(), line);
}

pub(crate) fn update_log_at(path: &Path, line: &str) {
    use std::io::Write as _;
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > 256 * 1024) {
        let _ = std::fs::remove_file(path);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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

/// 更新检查的判定结果。D-287:两态(有新版 / 已是最新)会把三件完全不同的事
/// 糊成同一句「已是最新」——
/// ①本地就是那个发布(**真的**最新);
/// ②本地构建晚于最新发布(自举机的常态),于是界面显示成「当前版本 a7a122a」
///   紧挨着「已是最新(build-c99304f)」,两个 hash 摆在一起自相矛盾;
/// ③根本没法比(dev 构建没有构建时间戳,D-265)。
/// 判定本身没错,错的是把「无法比较」和「本地领先」都说成「已是最新」。
/// 每一态都必须能说出自己的理由(D-004:任何不做的理由都要说出来)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReleaseVerdict {
    /// 发布比本地新,可以装。
    Update,
    /// 本地跑的就是最新发布。
    Latest,
    /// 本地构建晚于最新发布——没有可装的东西,但也不是「已是最新」。
    Ahead,
    /// 本地是开发构建(未注入 KANZEI_BUILD_INFO),与发布版没有可比的基准。
    DevBuild,
    /// 有 hash 但缺可比的时间戳(发布无 published_at,或戳相同而 hash 不同)。
    Unknown,
}

impl ReleaseVerdict {
    /// 前端按这个字符串分支渲染文案,不再自己拼「已是最新(别人的 hash)」。
    pub(crate) fn status(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Latest => "latest",
            Self::Ahead => "ahead",
            Self::DevBuild => "dev",
            Self::Unknown => "unknown",
        }
    }
}

pub(crate) fn release_verdict(
    current_info: &str,
    tag: &str,
    published_at: Option<&str>,
) -> ReleaseVerdict {
    let current_hash = current_info.split_whitespace().next().unwrap_or("dev");
    if current_hash == "dev" {
        return ReleaseVerdict::DevBuild;
    }
    if tag.is_empty() {
        return ReleaseVerdict::Unknown;
    }
    if tag == format!("build-{current_hash}") {
        return ReleaseVerdict::Latest;
    }
    let (Some((local_stamp, date_only)), Some(release_stamp)) = (
        build_stamp(current_info),
        published_at.and_then(timestamp_digits),
    ) else {
        return ReleaseVerdict::Unknown;
    };
    // 老的 date-only 构建戳只精确到天,只能按天比——同一天分不出先后,判 Unknown。
    let width = if date_only { 8 } else { 14 };
    match release_stamp[..width].cmp(&local_stamp[..width]) {
        std::cmp::Ordering::Greater => ReleaseVerdict::Update,
        std::cmp::Ordering::Less => ReleaseVerdict::Ahead,
        std::cmp::Ordering::Equal => ReleaseVerdict::Unknown,
    }
}

/// 「有没有新版可装」= 五态里唯一的 `Update`。生产路径直接读 `release_verdict`
/// (它还要区分 Ahead/DevBuild/Unknown 来决定文案),这里只留给既有回归测试用。
#[cfg(test)]
pub(crate) fn release_is_newer(current_info: &str, tag: &str, published_at: Option<&str>) -> bool {
    release_verdict(current_info, tag, published_at) == ReleaseVerdict::Update
}

pub(crate) fn build_stamp(info: &str) -> Option<(String, bool)> {
    let token = info.split_whitespace().nth(1)?;
    let digits = timestamp_digits(token)?;
    Some((
        digits,
        token.chars().filter(|c| c.is_ascii_digit()).count() < 14,
    ))
}

pub(crate) fn timestamp_digits(value: &str) -> Option<String> {
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

#[tauri::command(rename = "update_check")]
pub(crate) async fn update_check_command() -> Result<serde_json::Value, String> {
    let current = option_env!("KANZEI_BUILD_INFO").unwrap_or("dev");
    let current_hash = current
        .split_whitespace()
        .next()
        .unwrap_or("dev")
        .to_string();
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
        return Ok(
            json!({ "current": current_hash, "status": "none", "message": "还没有发布过安装包(用 scripts/package.ps1 -Publish 发布第一版)" }),
        );
    }
    let body: serde_json::Value = resp
        .error_for_status()
        .map_err(|e| format!("更新检查失败: {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let tag = body["tag_name"].as_str().unwrap_or("").to_string();
    let url = body["assets"]
        .as_array()
        .and_then(|assets| {
            assets
                .iter()
                .find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(".exe")))
        })
        .and_then(|a| a["browser_download_url"].as_str())
        .unwrap_or("")
        .to_string();
    let published_at = body["published_at"]
        .as_str()
        .or_else(|| body["created_at"].as_str());
    let verdict = release_verdict(current, &tag, published_at);
    let newer = verdict == ReleaseVerdict::Update;
    Ok(
        json!({ "current": current_hash, "latest": tag, "newer": newer, "url": url, "expected_build": tag.strip_prefix("build-"), "status": verdict.status() }),
    )
}

fn validate_release_target(url: &str, expected: &str) -> Result<(), String> {
    if !(7..=40).contains(&expected.len()) || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("更新目标缺少有效的构建标识，请重新检查更新".into());
    }
    let prefix =
        format!("https://github.com/kanze1/kanzei-code/releases/download/build-{expected}/");
    let asset = url
        .strip_prefix(&prefix)
        .ok_or("安装包地址与预期构建不一致")?;
    if asset.is_empty() || asset.contains(['/', '?', '#']) || !asset.ends_with(".exe") {
        return Err("安装包必须是本仓库指定构建的 release 资源".into());
    }
    Ok(())
}

#[tauri::command(rename = "update_receipt")]
pub(crate) fn update_receipt_command() -> Result<Option<UpdateReceipt>, String> {
    let path = receipt::receipt_path()?;
    let _lock = kanzei_base::atomic_file::lock_exclusive(&path).map_err(|e| e.to_string())?;
    let Some(mut result) = receipt::read(&path)? else {
        return Ok(None);
    };
    let running = option_env!("KANZEI_BUILD_INFO")
        .unwrap_or("dev")
        .split_whitespace()
        .next()
        .unwrap_or("dev");
    if result.reconcile(running, receipt_owner_alive(&result)) {
        receipt::write(&path, &result)?;
    }
    Ok(Some(result))
}

#[tauri::command(rename = "update_install")]
pub(crate) async fn update_install_command(
    app: tauri::AppHandle,
    url: String,
    expected_build: String,
) -> Result<String, String> {
    validate_release_target(&url, &expected_build)?;
    let receipt_path = receipt::receipt_path()?;
    let mut result = UpdateReceipt {
        expected_build,
        source_build: option_env!("KANZEI_BUILD_INFO").unwrap_or("dev").to_owned(),
        started_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis(),
        installed_exe: std::env::current_exe().map_err(|e| e.to_string())?,
        phase: Phase::Prepared,
        message: "正在下载并准备安装".into(),
        owner_pid: std::process::id(),
        owner_started_at: process_instance(std::process::id()).ok_or("无法核验更新进程身份")?,
    };
    {
        let _lock =
            kanzei_base::atomic_file::lock_exclusive(&receipt_path).map_err(|e| e.to_string())?;
        if receipt::read(&receipt_path)?.is_some_and(|previous| {
            matches!(previous.phase, Phase::Prepared | Phase::Installing)
                && receipt_owner_alive(&previous)
        }) {
            return Err("已有更新任务正在进行".into());
        }
        receipt::write(&receipt_path, &result)?;
    }
    match prepare_installer(&url, &receipt_path, &result).await {
        Ok(message) => {
            app.exit(0);
            Ok(message)
        }
        Err(error) => {
            result.transition(Phase::Failed, error.clone());
            receipt::write(&receipt_path, &result)?;
            Err(error)
        }
    }
}

async fn prepare_installer(
    url: &str,
    receipt_path: &Path,
    result: &UpdateReceipt,
) -> Result<String, String> {
    let config = KanzeiConfig::load(Path::new(".")).map_err(|e| e.to_string())?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let client = kanzei_llm::proxy::build_http_client(&proxy).map_err(|e| e.to_string())?;
    let bytes = client
        .get(url)
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
    // Each attempt owns its files; no taskkill by process name or fixed TEMP
    // artifact can interfere with an unrelated installer.
    let directory = receipt_path
        .parent()
        .ok_or("更新目录不存在")?
        .join(format!("{}-{}", result.started_at_ms, result.owner_pid));
    std::fs::create_dir(&directory).map_err(|e| format!("创建更新目录失败: {e}"))?;
    let installer = directory.join("kanzei-setup.exe");
    let helper = directory.join("kanzei-update-helper.exe");
    std::fs::write(&installer, &bytes).map_err(|e| format!("写入安装包失败: {e}"))?;
    std::fs::copy(&result.installed_exe, &helper).map_err(|e| format!("准备安装助手失败: {e}"))?;
    crate::runtime_service::shutdown_for_update().await?;
    // Spawn and durable ownership transfer share a lock with helper startup.
    // A reopened window cannot mistake the exited parent for an abandoned job.
    let _handoff =
        kanzei_base::atomic_file::lock_exclusive(receipt_path).map_err(|e| e.to_string())?;
    let mut child = Command::new(&helper)
        .arg("--kz-install-helper")
        .arg(&installer)
        .arg(&result.installed_exe)
        .arg(std::process::id().to_string())
        .arg(receipt_path)
        .spawn()
        .map_err(|e| {
            crate::runtime_service::cancel_update_shutdown();
            format!("启动安装程序失败: {e}")
        })?;
    let mut handed_off = result.clone();
    handed_off.owner_pid = child.id();
    handed_off.message = "安装助手已接收任务，正在等待应用退出".into();
    let mut persist_handoff = || -> Result<(), String> {
        handed_off.owner_started_at = process_instance(child.id()).ok_or("无法核验安装助手身份")?;
        receipt::write(receipt_path, &handed_off)
    };
    if let Err(error) = persist_handoff() {
        let stopped = child.kill().and_then(|()| child.wait().map(|_| ()));
        crate::runtime_service::cancel_update_shutdown();
        return Err(format!(
            "更新交接记录保存失败: {error}; 助手停止结果: {stopped:?}"
        ));
    }
    Ok(format!(
        "已下载 {} MB，正在退出并安装构建 {}；下次启动将核验安装结果",
        bytes.len() / 1_048_576,
        result.expected_build
    ))
}

pub(crate) fn startup_update() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--kz-update-helper") {
        if let (Some(exe), Some(pending)) = (args.get(2), args.get(3)) {
            apply_pending_update(Path::new(exe), Path::new(pending));
        }
        return true;
    }
    if args.get(1).map(String::as_str) == Some("--kz-install-helper") {
        if let (Some(installer), Some(exe), Some(parent), Some(receipt_path)) = (
            args.get(2),
            args.get(3),
            args.get(4).and_then(|p| p.parse().ok()),
            args.get(5),
        ) {
            run_install_helper(
                Path::new(installer),
                Path::new(exe),
                parent,
                Path::new(receipt_path),
            );
        }
        return true;
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let _ = std::fs::remove_file(exe.with_extension("exe.previous"));
    let _ = std::fs::remove_file(update_helper_path());
    let pending = pending_path(&exe);
    if !pending.is_file() {
        return false;
    }
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

pub(crate) fn cleanup_orphan_webviews() {
    let script = format!(
        r#"$mine = {}
$others = @(Get-Process kzapp -ErrorAction SilentlyContinue | Where-Object {{ $_.Id -ne $mine }})
if ($others.Count -gt 0) {{ exit 0 }}
Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object {{ $_.CommandLine -match 'dev\.kanzei\.app' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"#,
        std::process::id()
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    // D-254:部分 Windows 机器的 CIM/WMI 服务会永久卡住。这里发生在 Tauri
    // 建窗之前，等待无上限就会表现成“kzapp 进程存在但窗口永远不出现”。
    // 清理只是启动前维护动作，5 秒仍未返回就终止查询并继续建窗。
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
        }
    }
}
pub(crate) fn wait_for_parent_exit(parent_pid: u32, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !process_alive(parent_pid) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    !process_alive(parent_pid)
}
fn run_install_helper(installer: &Path, exe: &Path, parent_pid: u32, receipt_path: &Path) {
    let run = || -> Result<(), String> {
        let mut result = {
            let _handoff = kanzei_base::atomic_file::lock_exclusive(receipt_path)
                .map_err(|e| e.to_string())?;
            let mut result = receipt::read(receipt_path)?.ok_or("缺少更新交接记录")?;
            if result.installed_exe != exe
                || result.owner_pid != std::process::id()
                || !receipt_owner_alive(&result)
                || result.phase != Phase::Prepared
            {
                return Err("更新交接记录与安装任务不一致".into());
            }
            result.transition(Phase::Installing, "正在等待应用退出并安装".into());
            receipt::write(receipt_path, &result)?;
            result
        };
        let install = || -> Result<(), String> {
            if !wait_for_parent_exit(parent_pid, std::time::Duration::from_secs(30)) {
                return Err("应用未能在安装期限内退出，安装已取消".into());
            }
            cleanup_orphan_webviews();
            let out = Command::new(installer)
                .arg("/S")
                .output()
                .map_err(|e| format!("启动安装器失败: {e}"))?;
            if !out.status.success() {
                return Err(format!(
                    "安装器失败 exit={:?}: {} {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stdout).trim(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            Ok(())
        };
        match install() {
            Ok(()) => result.transition(
                Phase::AwaitingRestart,
                format!("安装器已结束。下次启动将核验构建 {}", result.expected_build),
            ),
            Err(error) => result.transition(Phase::Failed, error),
        }
        update_log(&result.message);
        receipt::write(receipt_path, &result)?;
        if result.phase == Phase::AwaitingRestart {
            let _ = std::fs::remove_file(installer);
        }
        Ok(())
    };
    if let Err(error) = run() {
        update_log(&format!("安装助手失败: {error}"));
    }
}
fn process_alive(pid: u32) -> bool {
    process_instance(pid).is_some()
}
fn receipt_owner_alive(receipt: &UpdateReceipt) -> bool {
    process_instance(receipt.owner_pid) == Some(receipt.owner_started_at)
}
#[cfg(windows)]
fn process_instance(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // A durable owner is a process instance, not a reusable PID. Query the
    // creation time and liveness through the same handle.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut exit_code = 0;
        let queried = GetExitCodeProcess(handle, &mut exit_code);
        let empty = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (empty, empty, empty, empty);
        let timed = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
        let _ = CloseHandle(handle);
        (queried != 0 && timed != 0 && exit_code == STILL_ACTIVE as u32)
            .then_some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}
#[cfg(not(windows))]
fn process_instance(_pid: u32) -> Option<u64> {
    None
}
pub(crate) fn sync_bundled_cli() {
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
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(sidecar) = exe.parent().map(|dir| dir.join("kz.exe")) else {
        return;
    };
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
        eprintln!("kzapp:同步 kz CLI 失败(下次启动重试): {error}");
    }
}
fn cli_is_older(target: &Path, ours: &str) -> bool {
    let Ok(output) = Command::new(target).arg("--version").output() else {
        return true;
    };
    installed_cli_is_older(&String::from_utf8_lossy(&output.stdout), ours)
}
pub(crate) fn installed_cli_is_older(version_output: &str, ours: &str) -> bool {
    let Some(installed) = version_output
        .split('(')
        .nth(1)
        .and_then(|rest| rest.split(')').next())
    else {
        return true;
    };
    match (build_stamp(installed), build_stamp(ours)) {
        (Some((a, _)), Some((b, _))) => a < b,
        _ => true,
    }
}
fn apply_pending_update(exe: &Path, pending: &Path) {
    std::thread::sleep(std::time::Duration::from_millis(250));
    let backup = exe.with_extension("exe.previous");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        let _ = std::fs::remove_file(&backup);
        if std::fs::rename(exe, &backup).is_ok() {
            match std::fs::rename(pending, exe) {
                Ok(()) => {
                    // 更新交接只负责替换文件，不替用户重新启动应用。否则用户关闭
                    // kzapp 后，仍在等待父进程的 helper 会在这里制造一个新实例。
                    let _ = std::fs::remove_file(&backup);
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
mod install_verify_tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn durable_owner_rejects_a_reused_pid() {
        let pid = std::process::id();
        let started = process_instance(pid).unwrap();
        let mut receipt = UpdateReceipt {
            expected_build: "1234567".into(),
            source_build: "abcdef0".into(),
            started_at_ms: 0,
            installed_exe: "kzapp.exe".into(),
            phase: Phase::Prepared,
            message: String::new(),
            owner_pid: pid,
            owner_started_at: started,
        };
        assert!(receipt_owner_alive(&receipt));
        receipt.owner_started_at = started.wrapping_add(1);
        assert!(!receipt_owner_alive(&receipt));
        receipt.owner_pid = 0;
        assert!(!receipt_owner_alive(&receipt));
    }
    #[test]
    fn update_target_is_bound_to_exact_release_asset() {
        assert!(validate_release_target("https://github.com/kanze1/kanzei-code/releases/download/build-1234567/kanzei-setup.exe", "1234567").is_ok());
        for url in [
            "https://github.com/kanze1/kanzei-code/releases/download/build-7654321/kanzei-setup.exe",
            "https://github.com/kanze1/kanzei-code/other.exe",
            "https://github.com/kanze1/kanzei-code/releases/download/build-1234567/../other.exe",
        ] { assert!(validate_release_target(url, "1234567").is_err()); }
        assert!(validate_release_target("", "dev").is_err());
    }
}
