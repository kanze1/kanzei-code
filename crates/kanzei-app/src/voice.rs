//! Local voice transport. Audio stays on the configured loopback service.
//! Request ownership is explicit so canceling one conversation cannot stop another.
use base64::{engine::general_purpose::STANDARD, Engine};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, future::Future, sync::Mutex, time::Duration};
use tauri::{ipc::Channel, State};
use tokio::sync::watch;

#[derive(Default)]
pub struct VoiceState {
    requests: Mutex<HashMap<String, (String, watch::Sender<bool>)>>,
    service_start: tokio::sync::Mutex<()>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VoiceSettings {
    pub port: u16,
    pub language: String,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            port: 7388,
            language: "auto".into(),
        }
    }
}

fn config_path() -> Result<std::path::PathBuf, String> {
    kanzei_harness::kanzei_home()
        .map(|root| root.join("voice.json"))
        .ok_or_else(|| "无法定位用户配置目录".into())
}

fn validate_settings(settings: &VoiceSettings) -> Result<(), String> {
    if settings.port < 1024 || !["auto", "zh", "en", "ja"].contains(&settings.language.as_str()) {
        return Err("语音设置无效：端口须在 1024–65535，语言须为 auto/zh/en/ja".into());
    }
    Ok(())
}

#[tauri::command]
pub fn voice_settings_get() -> Result<VoiceSettings, String> {
    let path = config_path()?;
    let value = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("语音配置无法读取：{e}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => VoiceSettings::default(),
        Err(error) => return Err(error.to_string()),
    };
    validate_settings(&value)?;
    Ok(value)
}

#[tauri::command]
pub fn voice_settings_set(settings: VoiceSettings) -> Result<(), String> {
    validate_settings(&settings)?;
    let path = config_path()?;
    std::fs::create_dir_all(path.parent().ok_or("语音配置目录无效")?).map_err(|e| e.to_string())?;
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())
}

fn endpoint(settings: &VoiceSettings, path: &str) -> String {
    format!("http://127.0.0.1:{}{path}", settings.port)
}

#[tauri::command]
pub async fn voice_status() -> Result<Value, String> {
    let settings = voice_settings_get()?;
    service_health(&settings).await
}

pub(super) async fn service_health(settings: &VoiceSettings) -> Result<Value, String> {
    let response = client()?
        .get(endpoint(settings, "/health"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| format!("本机语音服务未连接（端口 {}）", settings.port))?;
    if !response.status().is_success() {
        return Err(format!("语音服务检查失败：HTTP {}", response.status()));
    }
    let health: Value = response
        .json()
        .await
        .map_err(|e| format!("语音服务响应无效：{e}"))?;
    if !health["ready"].is_boolean() {
        return Err("该端口未返回有效的语音服务状态".into());
    }
    Ok(health)
}

#[tauri::command]
pub async fn voice_start(state: State<'_, VoiceState>) -> Result<Value, String> {
    let settings = voice_settings_get()?;
    let root = config_path()?
        .parent()
        .ok_or("语音配置目录无效")?
        .to_path_buf();
    crate::voice_service::ensure_started(&settings, &root, &state.service_start).await
}

struct RequestGuard<'a> {
    state: &'a VoiceState,
    id: String,
}
impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut requests) = self.state.requests.lock() {
            requests.remove(&self.id);
        }
    }
}

fn register<'a>(
    state: &'a VoiceState,
    id: &str,
    session: &str,
) -> Result<(RequestGuard<'a>, watch::Receiver<bool>), String> {
    if id.is_empty() || id.len() > 128 || session.is_empty() || session.len() > 1024 {
        return Err("语音请求标识无效".into());
    }
    let mut requests = state.requests.lock().map_err(|e| e.to_string())?;
    if requests.contains_key(id) {
        return Err("语音请求已存在".into());
    }
    if requests.len() >= 4 {
        return Err("语音请求过多，请先停止旧请求".into());
    }
    let (tx, rx) = watch::channel(false);
    requests.insert(id.to_string(), (session.to_string(), tx));
    Ok((
        RequestGuard {
            state,
            id: id.to_string(),
        },
        rx,
    ))
}

async fn cancellable<T>(
    mut cancel: watch::Receiver<bool>,
    work: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    if *cancel.borrow() {
        return Err("voice_cancelled".into());
    }
    tokio::select! {
        biased;
        _ = cancel.changed() => Err("voice_cancelled".into()),
        result = work => result,
    }
}

#[tauri::command]
pub fn voice_cancel(state: State<'_, VoiceState>, session_id: String) -> Result<(), String> {
    let requests = state.requests.lock().map_err(|e| e.to_string())?;
    for (session, cancel) in requests.values() {
        if session == &session_id {
            let _ = cancel.send(true);
        }
    }
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceChunk {
    request_id: String,
    session_id: String,
    sequence: u32,
    sample_rate: u32,
    pcm: String,
    done: bool,
}

#[tauri::command]
pub async fn voice_speak(
    state: State<'_, VoiceState>,
    request_id: String,
    session_id: String,
    text: String,
    on_chunk: Channel<VoiceChunk>,
) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > 600 {
        return Err("单段播报须为 1–600 字".into());
    }
    let settings = voice_settings_get()?;
    let (_guard, cancel) = register(&state, &request_id, &session_id)?;
    cancellable(cancel, async {
        let response = client()?
            .post(endpoint(&settings, "/speak"))
            .json(&json!({"text":text,"language":settings.language}))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("语音合成失败：HTTP {}", response.status()));
        }
        if response
            .headers()
            .get("x-audio-format")
            .and_then(|v| v.to_str().ok())
            != Some("pcm-s16le")
            || response
                .headers()
                .get("x-sample-rate")
                .and_then(|v| v.to_str().ok())
                != Some("24000")
        {
            return Err("语音服务未返回 24kHz 单声道 PCM".into());
        }
        let mut stream = response.bytes_stream();
        let mut sequence = 0;
        let mut total = 0usize;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("语音流中断：{e}"))?;
            total += chunk.len();
            if total > 24_000 * 2 * 90 {
                return Err("单段语音超过 90 秒".into());
            }
            for packet in chunk.chunks(16_384) {
                on_chunk
                    .send(VoiceChunk {
                        request_id: request_id.clone(),
                        session_id: session_id.clone(),
                        sequence,
                        sample_rate: 24_000,
                        pcm: STANDARD.encode(packet),
                        done: false,
                    })
                    .map_err(|e| e.to_string())?;
                sequence += 1;
            }
        }
        if total == 0 || !total.is_multiple_of(2) {
            return Err("语音流为空或包含不完整采样".into());
        }
        on_chunk
            .send(VoiceChunk {
                request_id,
                session_id,
                sequence,
                sample_rate: 24_000,
                pcm: String::new(),
                done: true,
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn voice_transcribe(
    state: State<'_, VoiceState>,
    request_id: String,
    session_id: String,
    wav: String,
) -> Result<Value, String> {
    if wav.len() > 1_400_000 {
        return Err("录音超过 30 秒".into());
    }
    let bytes = STANDARD
        .decode(wav)
        .map_err(|_| "录音编码无效".to_string())?;
    if bytes.len() < 44 || bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("需要 WAV 录音".into());
    }
    let settings = voice_settings_get()?;
    let (_guard, cancel) = register(&state, &request_id, &session_id)?;
    cancellable(cancel, async {
        let response = client()?
            .post(endpoint(&settings, "/transcribe"))
            .query(&[("language", settings.language)])
            .header("content-type", "audio/wav")
            .body(bytes)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("语音识别失败：HTTP {}", response.status()));
        }
        let result: Value = response.json().await.map_err(|e| e.to_string())?;
        if !result["text"].is_string() {
            return Err("语音识别结果缺少文本".into());
        }
        Ok(result)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancellation_is_scoped_and_guards_release() {
        let state = VoiceState::default();
        let (a, cancel_a) = register(&state, "a", "session-a").unwrap();
        let (b, cancel_b) = register(&state, "b", "session-b").unwrap();
        assert!(register(&state, "a", "session-a").is_err());
        state.requests.lock().unwrap()["a"].1.send(true).unwrap();
        assert_eq!(
            cancellable(cancel_a, async { Ok(1) }).await.unwrap_err(),
            "voice_cancelled"
        );
        assert_eq!(cancellable(cancel_b, async { Ok(2) }).await.unwrap(), 2);
        drop(a);
        drop(b);
        assert!(state.requests.lock().unwrap().is_empty());
    }
    #[test]
    fn service_target_cannot_be_a_remote_url() {
        let settings = VoiceSettings::default();
        assert_eq!(endpoint(&settings, "/speak"), "http://127.0.0.1:7388/speak");
        assert!(validate_settings(&VoiceSettings {
            port: 80,
            ..settings
        })
        .is_err());
    }
}
