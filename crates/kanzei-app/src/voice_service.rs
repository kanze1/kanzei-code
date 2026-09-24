//! Start the registered local voice runtime, then wait for both models to be ready.
use crate::voice::{service_health, VoiceSettings};
use serde::Deserialize;
use serde_json::Value;
use std::{fs::OpenOptions, path::Path, process::Stdio, time::Duration};
use tokio::{process::Command, sync::Mutex, time::Instant};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Launcher {
    port: u16,
    program: String,
    args: Vec<String>,
    working_directory: String,
}

fn read_launcher(root: &Path, port: u16) -> Result<Launcher, String> {
    let bytes = std::fs::read(root.join("voice-launcher.json"))
        .map_err(|_| "语音服务尚未配置自动启动，请重新接入已安装的音色服务".to_string())?;
    let launcher: Launcher =
        serde_json::from_slice(&bytes).map_err(|e| format!("语音启动配置无法读取：{e}"))?;
    if launcher.port != port {
        return Err(format!(
            "已配置的音色服务使用 {} 端口，请在语音设置中改回该端口",
            launcher.port
        ));
    }
    if !Path::new(&launcher.program).is_absolute() || !Path::new(&launcher.program).is_file() {
        return Err("语音启动程序不存在，请重新接入音色服务".into());
    }
    if !Path::new(&launcher.working_directory).is_absolute()
        || !Path::new(&launcher.working_directory).is_dir()
        || launcher.args.is_empty()
    {
        return Err("语音服务目录或启动参数无效，请重新接入音色服务".into());
    }
    Ok(launcher)
}

pub async fn ensure_started(
    settings: &VoiceSettings,
    root: &Path,
    start: &Mutex<()>,
) -> Result<Value, String> {
    ensure_with_timeout(settings, root, start, Duration::from_secs(210)).await
}

async fn ensure_with_timeout(
    settings: &VoiceSettings,
    root: &Path,
    start: &Mutex<()>,
    timeout: Duration,
) -> Result<Value, String> {
    if let Ok(health) = service_health(settings).await {
        if health["ready"] == true {
            return Ok(health);
        }
    }
    // A second click or caller reuses the first startup instead of loading models twice.
    let _starting = start.lock().await;
    let mut last_health = service_health(settings).await.ok();
    if let Some(health) = last_health
        .as_ref()
        .filter(|health| health["ready"] == true)
    {
        return Ok(health.clone());
    }
    let log_path = root.join("voice-start.log");
    let mut child = None;
    if last_health.is_none() {
        let launcher = read_launcher(root, settings.port)?;
        let log = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .map_err(|e| format!("无法写入语音启动日志：{e}"))?;
        let error_log = log.try_clone().map_err(|e| e.to_string())?;
        let mut command = Command::new(&launcher.program);
        command
            .args(&launcher.args)
            .current_dir(&launcher.working_directory)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(error_log);
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        child = Some(
            command
                .spawn()
                .map_err(|e| format!("语音服务无法启动：{e}"))?,
        );
    }
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(health) = service_health(settings).await {
            if health["ready"] == true {
                return Ok(health);
            }
            last_health = Some(health);
        }
        if let Some(process) = child.as_mut() {
            if let Some(status) = process.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err(format!("语音服务启动失败，详情见 {}", log_path.display()));
                }
                // Some launchers exit after detaching their model supervisor.
                child = None;
            }
        }
        if Instant::now() >= deadline {
            let detail = last_health
                .as_ref()
                .and_then(|health| health["detail"].as_str())
                .filter(|detail| !detail.is_empty())
                .unwrap_or("模型仍未就绪");
            return Err(format!(
                "语音服务启动超时：{detail}。可稍后重试，详情见 {}",
                log_path.display()
            ));
        }
        tokio::time::sleep(Duration::from_millis(500).min(timeout)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn temp_root() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kanzei-voice-start-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn launcher(root: &Path, port: u16) {
        std::fs::write(
            root.join("voice-launcher.json"),
            serde_json::to_vec(&json!({
                "port": port, "program": std::env::current_exe().unwrap(),
                "args": ["--invalid-voice-launch-test-option"], "workingDirectory": root,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn mismatched_port_never_starts_a_different_voice() {
        let root = temp_root();
        launcher(&root, 7389);
        assert!(read_launcher(&root, 7388).err().unwrap().contains("7389"));
        assert!(read_launcher(&root, 7389).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ready_service_needs_no_launcher_or_restart() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let settings = VoiceSettings {
            port: listener.local_addr().unwrap().port(),
            ..Default::default()
        };
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            stream.read(&mut request).await.unwrap();
            let body = r#"{"ready":true,"voice":"selected-C"}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let root = temp_root();
        let health = ensure_started(&settings, &root, &Mutex::new(()))
            .await
            .unwrap();
        assert_eq!(health["voice"], "selected-C");
        assert!(!root.join("voice-start.log").exists());
        server.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn failed_launcher_returns_before_startup_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let root = temp_root();
        launcher(&root, port);
        let settings = VoiceSettings {
            port,
            ..Default::default()
        };
        let error = ensure_with_timeout(&settings, &root, &Mutex::new(()), Duration::from_secs(10))
            .await
            .unwrap_err();
        assert!(error.contains("启动失败"), "{error}");
        assert!(root.join("voice-start.log").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }
}
