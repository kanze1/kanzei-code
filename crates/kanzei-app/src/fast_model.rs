//! fast 模型就绪检查与 Ollama 一键安装。

use serde_json::json;
use std::path::Path;
use std::process::Command;
use tauri::Emitter;

use kanzei_harness::KanzeiConfig;

/// fast 子代理模型的就绪状态(R-136)。
#[tauri::command]
pub async fn fast_model_status() -> serde_json::Value {
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
pub async fn fast_model_setup(window: tauri::Window) -> Result<String, String> {
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
            "install",
            "--id",
            "Ollama.Ollama",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
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
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd.spawn()
            .map_err(|e| format!("启动 ollama serve 失败:{e}"))?;
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
                    if text != last_emit {
                        stage(&text);
                        last_emit = text;
                    }
                }
            }
        }
        if !ollama_model_present(&base_url, &model).await {
            return Err(format!(
                "拉取流结束但 {model} 仍不在本地模型列表里,重试一次看看。"
            ));
        }
        stage(&format!("模型 {model} 已就绪"));
    }

    Ok(format!("fast 子代理已就绪:{model}"))
}

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
        cmd.creation_flags(0x0800_0000);
    }
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

pub(crate) async fn ollama_service_up(base_url: &str) -> bool {
    let tags = format!("{}/api/tags", base_url.trim_end_matches("/v1"));
    let Some(client) = loopback_client(2) else {
        return false;
    };
    client
        .get(&tags)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

async fn ollama_model_present(base_url: &str, model: &str) -> bool {
    let tags = format!("{}/api/tags", base_url.trim_end_matches("/v1"));
    let Some(client) = loopback_client(3) else {
        return false;
    };
    let Ok(resp) = client.get(&tags).send().await else {
        return false;
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return false;
    };
    v["models"]
        .as_array()
        .map(|models| {
            models.iter().any(|m| {
                m["name"]
                    .as_str()
                    .is_some_and(|n| n == model || n.trim_end_matches(":latest") == model)
            })
        })
        .unwrap_or(false)
}

pub(crate) fn pull_progress_text(line: &serde_json::Value) -> Option<String> {
    let status = line["status"].as_str()?;
    match (line["completed"].as_u64(), line["total"].as_u64()) {
        (Some(done), Some(total)) if total > 0 => {
            let pct = done * 100 / total;
            Some(format!(
                "{status} {pct}%({}/{} MB)",
                done >> 20,
                total >> 20
            ))
        }
        _ => Some(status.to_string()),
    }
}
