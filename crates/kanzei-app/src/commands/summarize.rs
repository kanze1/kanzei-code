//! Sidebar conversation export: optional summaries and a user-selected Markdown file.

use std::path::Path;

use serde_json::json;

use kanzei_harness::config::KanzeiConfig;
use kanzei_llm::{LlmClient, LlmEvent, LlmRequest, Message, ProxyConfig, ReasoningEffort};

/// 用 fast 模型把人机协作 transcript 压成中文纪要(原 run.rs 2443)。
pub(crate) async fn fast_summarize(cwd: &Path, transcript: &str) -> Result<String, String> {
    use futures::StreamExt;
    let config = KanzeiConfig::load_at_root(cwd).map_err(|e| e.to_string())?;
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
    let request = LlmRequest { model: resolved.model.clone(), system: vec!["把下面的人机协作对话记录总结成简洁的中文纪要:做了什么、改了哪些文件、结论、遗留问题/下一步。markdown 列表,300 字以内。".into()], messages: vec![Message::user_text(transcript)], tools: vec![],
        hosted_tools: vec![], max_tokens: 2048, temperature: None, reasoning: ReasoningEffort::Off, service_tier: config.service_tier_for(&resolved) };
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

/// Summarize a structured, conversation-scoped export supplied by the sidebar.
#[tauri::command]
pub(crate) async fn summarize_chat(
    project_dir: String,
    transcript: String,
) -> Result<serde_json::Value, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    let summary = fast_summarize(&root, &transcript).await?;
    Ok(json!({ "summary": summary }))
}

#[tauri::command]
pub(crate) async fn save_conversation_export(
    name: String,
    content: String,
) -> Result<Option<String>, String> {
    let filename: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let Some(destination) = rfd::AsyncFileDialog::new()
        .set_file_name(format!("{filename}.md"))
        .add_filter("Markdown", &["md"])
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let path = destination.path().to_path_buf();
    let saved = path.display().to_string();
    tokio::fs::write(path, content)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Some(saved))
}
