//! 子代理容器清单的创建、升级与回滚命令。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContainerManifest {
    agent_id: String,
    version: String,
    status: String,
    permissions: Vec<String>,
    updated_at: i64,
}

fn agent_container_path(agent_id: &str) -> Result<PathBuf, String> {
    let safe: String = agent_id
        .trim()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') { ch } else { '-' })
        .collect();
    if safe.is_empty() {
        return Err("agent_id 不能为空".into());
    }
    Ok(kanzei_harness::kanzei_home()
        .unwrap_or_default()
        .join("agent-containers")
        .join(safe)
        .join("manifest.json"))
}

fn read_agent_container(agent_id: &str) -> Result<(PathBuf, AgentContainerManifest), String> {
    let path = agent_container_path(agent_id)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取代理容器失败: {e}"))?;
    let manifest = serde_json::from_str(&text).map_err(|e| format!("代理容器清单损坏: {e}"))?;
    Ok((path, manifest))
}

#[tauri::command]
pub fn agent_container_create(agent_id: String) -> Result<AgentContainerManifest, String> {
    let path = agent_container_path(&agent_id)?;
    if path.exists() {
        return Err(format!("代理容器已存在: {}", path.display()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let manifest = AgentContainerManifest {
        agent_id: agent_id.trim().to_owned(),
        version: "1".into(),
        status: "ready".into(),
        permissions: vec!["read".into()],
        updated_at: SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs() as i64,
    };
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(manifest)
}

#[tauri::command]
pub fn agent_container_upgrade(agent_id: String, version: String) -> Result<AgentContainerManifest, String> {
    let (path, mut manifest) = read_agent_container(&agent_id)?;
    let version = version.trim();
    if version.is_empty() {
        return Err("升级版本不能为空".into());
    }
    let backup = path.with_extension("json.previous");
    std::fs::copy(&path, &backup).map_err(|e| format!("保存升级回滚点失败: {e}"))?;
    manifest.version = version.to_owned();
    manifest.updated_at = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs() as i64;
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| format!("写入升级清单失败: {e}"))?;
    Ok(manifest)
}

#[tauri::command]
pub fn agent_container_rollback(agent_id: String) -> Result<AgentContainerManifest, String> {
    let (path, _) = read_agent_container(&agent_id)?;
    let backup = path.with_extension("json.previous");
    let text = std::fs::read_to_string(&backup).map_err(|e| format!("没有可用回滚点: {e}"))?;
    let manifest: AgentContainerManifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(manifest)
}
