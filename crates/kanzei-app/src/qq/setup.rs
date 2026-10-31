//! The desktop configures NapCat through its public WebUI API. Its credentials
//! stay in this process; only QR images and account state reach the renderer.
use super::{config::Stored, QqState};
use crate::MutexPoisonExt;
use serde_json::{json, Value};
use sha2::Digest;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::State;

#[derive(Default)]
pub(crate) struct SetupState(tokio::sync::Mutex<Option<Session>>);
struct Session {
    id: String,
    url: String,
    credential: String,
    http: reqwest::Client,
}

impl Session {
    async fn call(&self, path: &str, body: Value) -> Result<Value, String> {
        let response = self
            .http
            .post(format!("{}/api/{path}", self.url))
            .bearer_auth(&self.credential)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("NapCat 请求失败: {e}"))?;
        let value: Value = response
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if value["code"].as_i64() != Some(0) {
            return Err(format!("NapCat: {}", value["message"]));
        }
        Ok(value["data"].clone())
    }
}
fn config_path(directory: &Path) -> Result<PathBuf, String> {
    let candidates = [
        directory.join("config/webui.json"),
        directory.join("napcat/config/webui.json"),
        directory.join("resources/app/app_launcher/napcat/config/webui.json"),
    ];
    let found: Vec<_> = candidates.into_iter().filter(|p| p.is_file()).collect();
    if found.len() != 1 {
        return Err("请选择包含 NapCat config/webui.json 的程序目录".into());
    }
    Ok(found[0].clone())
}
fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|v| format!("{v:02x}")).collect())
}
#[tauri::command]
pub(crate) async fn qq_setup_connect(
    state: State<'_, QqState>,
    directory: Option<String>,
    url: Option<String>,
    password: Option<String>,
    totp: Option<String>,
) -> Result<Value, String> {
    let (url, password) = if let Some(directory) = directory {
        let path = config_path(Path::new(&directory))?;
        let value: Value =
            serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let port = value["port"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= 65535)
            .ok_or("NapCat WebUI 端口无效")?;
        let token = value["token"]
            .as_str()
            .ok_or("NapCat WebUI 没有配置登录密码")?
            .to_string();
        (format!("http://127.0.0.1:{port}"), token)
    } else {
        (
            url.ok_or("需要 NapCat 管理地址")?,
            password.ok_or("需要 NapCat 管理密码")?,
        )
    };
    let parsed = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"))
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("本机配置向导需要本机 NapCat 管理地址".into());
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut session = Session {
        id: random_token()?,
        url: url.trim_end_matches('/').into(),
        credential: String::new(),
        http,
    };
    let hash = format!(
        "{:x}",
        sha2::Sha256::digest(format!("{password}.napcat").as_bytes())
    );
    let result = session
        .call("auth/login", json!({"hash":hash,"totpCode":totp}))
        .await?;
    if result["require2FA"] == true {
        return Err("请输入 NapCat 的两步验证码后连接".into());
    }
    session.credential = result["Credential"]
        .as_str()
        .ok_or("NapCat 未返回管理凭据")?
        .into();
    let id = session.id.clone();
    *state.setup.0.lock().await = Some(session);
    Ok(json!({"id":id}))
}
#[tauri::command]
pub(crate) async fn qq_setup_pick_directory() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("选择 NapCat 程序目录")
            .pick_folder()
            .map(|p| p.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub(crate) async fn qq_setup_status(
    state: State<'_, QqState>,
    id: String,
) -> Result<Value, String> {
    let guard = state.setup.0.lock().await;
    let session = guard
        .as_ref()
        .filter(|s| s.id == id)
        .ok_or("NapCat 连接已失效，请重新连接")?;
    let status = session.call("QQLogin/CheckLoginStatus", json!({})).await?;
    if status["isLogin"] == true {
        let info = session.call("QQLogin/GetQQLoginInfo", json!({})).await?;
        Ok(
            json!({"online":true,"account":super::protocol::id(&info["uin"])?,"nickname":info["nick"]}),
        )
    } else {
        let url = status["qrcodeurl"].as_str().filter(|v| !v.is_empty());
        let image = url.map(crate::login_qr::image).transpose()?;
        Ok(
            json!({"online":false,"image":image,"phase":status["loginPhase"],"error":status["loginError"]}),
        )
    }
}
fn managed_network(mut config: Value, port: u16, token: &str) -> Result<Value, String> {
    let network = config["network"]
        .as_object_mut()
        .ok_or("NapCat 网络配置无效")?;
    let servers = network
        .entry("websocketServers")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("NapCat WebSocket 配置无效")?;
    let managed = json!({"name":"kanzei-code","enable":true,"host":"127.0.0.1","port":port,"messagePostFormat":"array","reportSelfMessage":false,"token":token,"heartInterval":30000,"debug":false});
    if servers
        .iter()
        .any(|s| s["name"] != "kanzei-code" && s["enable"] == true && s["port"] == port)
    {
        return Err("这个端口已被其他 NapCat 连接使用，请更换端口".into());
    }
    if let Some(existing) = servers.iter_mut().find(|s| s["name"] == "kanzei-code") {
        *existing = managed;
    } else {
        servers.push(managed);
    }
    Ok(config)
}
#[tauri::command]
pub(crate) async fn qq_setup_apply(
    state: State<'_, QqState>,
    id: String,
    port: u16,
) -> Result<(), String> {
    if port == 0 {
        return Err("连接端口必须大于零".into());
    }
    let guard = state.setup.0.lock().await;
    let session = guard
        .as_ref()
        .filter(|s| s.id == id)
        .ok_or("NapCat 连接已失效")?;
    let info = session.call("QQLogin/GetQQLoginInfo", json!({})).await?;
    let account = super::protocol::id(&info["uin"])?;
    let mut stored: Stored = state.control.lock_or_recover().config.clone();
    let token = random_token()?;
    let config = session.call("OB11Config/GetConfig", json!({})).await?;
    let config = managed_network(config, port, &token)?;
    session
        .call("OB11Config/SetConfig", json!({"config":config.to_string()}))
        .await?;
    // Read back the bridge before saving the app's side of the binding.
    let actual = session.call("OB11Config/GetConfig", json!({})).await?;
    if actual["network"]["websocketServers"]
        .as_array()
        .is_none_or(|rows| {
            !rows.iter().any(|s| {
                s["name"] == "kanzei-code"
                    && s["token"] == token
                    && s["port"] == port
                    && s["enable"] == true
            })
        })
    {
        return Err("NapCat 未确认保存连接配置".into());
    }
    stored.settings.enabled = true;
    stored.settings.account = account;
    stored.settings.url = format!("ws://127.0.0.1:{port}");
    stored.token = token;
    super::save_config(&state, stored)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_connection_preserves_unrelated_networks_and_rejects_port_collision() {
        let config = json!({"network":{"httpServers":[{"name":"other"}],"websocketServers":[{"name":"other","enable":true,"port":3001}]}});
        assert!(managed_network(config.clone(), 3001, "secret").is_err());
        let changed = managed_network(config.clone(), 3002, "secret").unwrap();
        assert_eq!(
            changed["network"]["httpServers"],
            config["network"]["httpServers"]
        );
        assert_eq!(
            changed["network"]["websocketServers"][0],
            config["network"]["websocketServers"][0]
        );
        assert_eq!(
            managed_network(changed, 3002, "rotated").unwrap()["network"]["websocketServers"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
}
