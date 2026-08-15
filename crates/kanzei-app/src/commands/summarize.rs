//! 对话总结 IPC(command 侧,纯搬迁自 run.rs,R-253 批0)。
//!
//! 独立理由:轮末/手动总结与运行编排零耦合——`summarize_chat` 是「对话页手动
//! 总结按钮」的入口,`fast_summarize` 是它的实现(fast 模型把 transcript 压成
//! 中文纪要)。两者只依赖 config/llm/文件系统,不经 run_task、不碰事件循环,
//! 留在 run.rs 里只是给「运行主链路」文件加行数(照 files_view.rs 模式)。

use std::path::{Path, PathBuf};

use serde_json::json;

use kanzei_harness::config::KanzeiConfig;
use kanzei_llm::{LlmClient, LlmEvent, LlmRequest, Message, ProxyConfig, ReasoningEffort};

/// 用 fast 模型把人机协作 transcript 压成中文纪要(原 run.rs 2443)。
pub(crate) async fn fast_summarize(cwd: &Path, transcript: &str) -> Result<String, String> {
    use futures::StreamExt;
    let config = KanzeiConfig::load(cwd).map_err(|e| e.to_string())?;
    let resolved = config.resolve_model("fast").map_err(|e| e.to_string())?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let request = LlmRequest { model: resolved.model.clone(), system: vec!["把下面的人机协作对话记录总结成简洁的中文纪要:做了什么、改了哪些文件、结论、遗留问题/下一步。markdown 列表,300 字以内。".into()], messages: vec![Message::user_text(transcript)], tools: vec![], max_tokens: 2048, temperature: None, reasoning: ReasoningEffort::Off, service_tier: config.service_tier_for(&resolved) };
    let mut stream = client
        .stream(&route, &request)
        .await
        .map_err(|e| e.to_string())?;
    let mut summary = String::new();
    while let Some(event) = stream.next().await {
        if let LlmEvent::TextDelta { text, .. } = event.map_err(|e| e.to_string())? {
            summary.push_str(&text);
        }
    }
    if summary.trim().is_empty() {
        return Err("模型没有产出总结(fast 模型是否在运行?)".into());
    }
    Ok(summary)
}

// render_transcript 已随 R-021 轮末整段替换一并删除(R-236 B1):轮末压缩的
// 纪要输入渲染统一走 core 的 render_for_digest,不留第二份渲染实现。

/// 对话总结 Tauri command(原 run.rs 2476):总结并落盘到 .kanzei/summaries/。
#[tauri::command]
pub(crate) async fn summarize_chat(
    project_dir: String,
    transcript: String,
) -> Result<serde_json::Value, String> {
    let cwd = PathBuf::from(&project_dir);
    let summary = fast_summarize(&cwd, &transcript).await?;
    let root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let dir = root.join(".kanzei").join("summaries");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("summary-{secs}.md"));
    std::fs::write(&path, &summary).map_err(|e| e.to_string())?;
    Ok(json!({ "summary": summary, "path": path.display().to_string() }))
}
