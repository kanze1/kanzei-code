//! 更新命令与版本检查。

use std::path::Path;
use std::process::Command;

use serde_json::json;

use kanzei_harness::KanzeiConfig;
use kanzei_llm::ProxyConfig;

pub(crate) fn pending_path(exe: &Path) -> std::path::PathBuf {
    let name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("kzapp.exe");
    exe.with_file_name(format!("{name}.pending"))
}

pub(crate) fn installer_path() -> std::path::PathBuf {
    std::env::temp_dir().join("kanzei-setup.exe")
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

pub(crate) fn clear_stale_installer() -> Vec<String> {
    let mut notes = Vec::new();
    let killed = Command::new("taskkill")
        .args(["/F", "/IM", "kanzei-setup.exe"])
        .output()
        .ok()
        .is_some_and(|out| out.status.success());
    if killed {
        notes.push("已清理残留的安装器进程".to_string());
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    let path = installer_path();
    if path.exists() && std::fs::remove_file(&path).is_err() {
        notes.push("旧安装包仍被占用,将直接覆盖".to_string());
    }
    notes
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

pub(crate) fn image_stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub(crate) fn image_replaced(
    before: Option<(std::time::SystemTime, u64)>,
    after: Option<(std::time::SystemTime, u64)>,
) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => before != after,
        _ => false,
    }
}

pub(crate) fn release_is_newer(current_info: &str, tag: &str, published_at: Option<&str>) -> bool {
    let current_hash = current_info.split_whitespace().next().unwrap_or("dev");
    if current_hash == "dev" || tag.is_empty() || tag.contains(current_hash) {
        return false;
    }
    let Some((local_stamp, date_only)) = build_stamp(current_info) else {
        return false;
    };
    let Some(release_stamp) = published_at.and_then(timestamp_digits) else {
        return false;
    };
    if date_only {
        release_stamp[..8] > local_stamp[..8]
    } else {
        release_stamp > local_stamp
    }
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
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
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
    let newer = release_is_newer(current, &tag, published_at);
    Ok(
        json!({ "current": current_hash, "latest": tag, "newer": newer, "url": url, "status": if newer { "update" } else { "latest" } }),
    )
}

#[tauri::command(rename = "update_install")]
pub(crate) async fn update_install_command(
    app: tauri::AppHandle,
    url: String,
) -> Result<String, String> {
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
    let helper = update_helper_path();
    let _ = std::fs::remove_file(&helper);
    std::fs::copy(&exe, &helper).map_err(|e| {
        format!(
            "准备更新交接程序失败:{e}。可手动运行 {} 完成安装。",
            path.display()
        )
    })?;
    update_log(&format!(
        "交接:helper={} 安装包={}",
        helper.display(),
        path.display()
    ));
    Command::new(&helper)
        .arg("--kz-install-helper")
        .arg(&path)
        .arg(&exe)
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|e| {
            format!(
                "启动更新交接失败:{e}。可手动运行 {} 完成安装。",
                path.display()
            )
        })?;
    let mb = bytes.len() / 1_048_576;
    let prefix = if notes.is_empty() {
        String::new()
    } else {
        format!("{};", notes.join(";"))
    };
    app.exit(0);
    Ok(format!(
        "{prefix}已下载 {mb} MB,正在退出并静默安装,装完会自动重新打开"
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
        if let (Some(installer), Some(exe), Some(parent)) = (
            args.get(2),
            args.get(3),
            args.get(4).and_then(|p| p.parse().ok()),
        ) {
            run_install_helper(Path::new(installer), Path::new(exe), parent);
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
    let _ = cmd.output();
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
fn run_install_helper(installer: &Path, exe: &Path, parent_pid: u32) {
    update_log(&format!(
        "helper 启动 installer={} exe={} parent={parent_pid}",
        installer.display(),
        exe.display()
    ));
    let before = image_stamp(exe);
    let exited = wait_for_parent_exit(parent_pid, std::time::Duration::from_secs(30));
    update_log(&format!("父进程退出={exited}"));
    std::thread::sleep(std::time::Duration::from_millis(600));
    cleanup_orphan_webviews();
    match Command::new(installer).arg("/S").output() {
        Ok(out) if out.status.success() => update_log("安装器 exit=0"),
        Ok(out) => {
            update_log(&format!(
                "安装器失败 exit={:?} stdout={} stderr={}",
                out.status.code(),
                String::from_utf8_lossy(&out.stdout).trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
            return;
        }
        Err(error) => {
            update_log(&format!("启动安装器失败: {error}"));
            return;
        }
    }
    let after = image_stamp(exe);
    update_log(&format!("安装前 exe={before:?} 安装后 exe={after:?}"));
    if !image_replaced(before, after) {
        update_log("安装器 exit=0 但 exe 未被替换(mtime/大小不变):目标可能被占用,或安装位与运行位不是同一个文件。保留安装包供手动执行。");
        let _ = Command::new(exe).spawn();
        return;
    }
    let _ = Command::new(exe).spawn();
    let _ = std::fs::remove_file(installer);
}
#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // 进程句柄与 PID 一一对应；退出码 STILL_ACTIVE 才表示该 PID 仍在运行。
    // 这不依赖 tasklist 的输出语言、列宽或 PID 子串匹配。
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0;
        let queried = GetExitCodeProcess(handle, &mut exit_code);
        let _ = CloseHandle(handle);
        queried != 0 && exit_code == STILL_ACTIVE as u32
    }
}
#[cfg(not(windows))]
fn process_alive(_pid: u32) -> bool {
    false
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
                    if Command::new(exe).spawn().is_ok() {
                        let _ = std::fs::remove_file(&backup);
                    } else {
                        let _ = std::fs::remove_file(exe);
                        let _ = std::fs::rename(&backup, exe);
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
mod install_verify_tests {
    use super::image_replaced;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// D-199:"退出码成功"不等于"文件换了"。这条判据是唯一能把
    /// 静默没装上与真更新分开的东西,它必须对每一种"没换"都判 false。
    #[test]
    fn 未替换的镜像一律不算更新成功() {
        let t = UNIX_EPOCH + Duration::from_secs(1_786_212_410);
        let stamp = Some((t, 22_449_664_u64));
        assert!(!image_replaced(stamp, stamp), "前后完全相同必须判为未替换");
        assert!(!image_replaced(None, stamp));
        assert!(!image_replaced(stamp, None));
        assert!(!image_replaced(None, None));
    }

    #[test]
    fn 时间或大小任一变化都算替换成功() {
        let t = UNIX_EPOCH + Duration::from_secs(1_786_212_410);
        let stamp = Some((t, 22_449_664_u64));
        assert!(image_replaced(
            stamp,
            Some((t + Duration::from_secs(1), 22_449_664))
        ));
        assert!(image_replaced(stamp, Some((t, 22_449_665))));
    }

    #[test]
    fn image_stamp_跟得上真实文件改动() {
        let path = std::env::temp_dir().join(format!(
            "kz-d199-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
