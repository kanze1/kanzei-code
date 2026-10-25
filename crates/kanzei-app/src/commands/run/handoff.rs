//! An explicit conversation handoff freezes the source's readable current segment
//! into the destination input. Queue promotion must never reread a changing source.

use kanzei_llm::{Message, Part, Role};
use serde::Deserialize;

use crate::{normalized_project_root, process_session_id, runtime_for, AppState, MutexPoisonExt};

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffSource {
    pub(crate) project_dir: String,
    pub(crate) process_id: String,
}

pub(super) fn attach_snapshot(
    state: &AppState,
    source: HandoffSource,
    instruction: &str,
) -> Result<String, String> {
    let root = normalized_project_root(std::path::Path::new(&source.project_dir));
    let process = state
        .processes
        .lock_or_recover()
        .get(&source.process_id)
        .cloned()
        .ok_or("来源对话已关闭，未交接；请返回来源对话重试")?;
    if process.origin_project.0 != root {
        return Err("来源对话不属于指定项目，未交接".into());
    }
    let session_id = process_session_id(&root, Some(&process.id));
    let runtime = runtime_for(state, &session_id);
    // Share the source's lifecycle lock with start/reset/delete. Once it is idle,
    // the finished surface is durable and cannot switch segments during this read.
    let _source_lifecycle = runtime.lifecycle.lock_or_recover();
    if runtime.is_running() {
        return Err("来源对话仍在运行，请等回复完成后重新交接；结论已保留".into());
    }
    let messages = crate::conversation::conversation_get(
        root.display().to_string(),
        None,
        Some(process.id.clone()),
    )?;
    let transcript = readable_transcript(&messages);
    if transcript.is_empty() {
        return Err("来源对话没有可交接的上下文，请先完成讨论；结论已保留".into());
    }
    let captured_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    let snapshot = serde_json::json!({
        "project":root, "conversation":process.id, "session_id":session_id,
        "captured_at_ms":captured_at_ms, "transcript":transcript,
    });
    let context = kanzei_harness::ToolOutput::ok(snapshot.to_string())
        .with_external_source(
            kanzei_harness::ExternalSourceKind::Conversation,
            format!("conversation:{session_id}"),
        )
        .model_content();
    Ok(format!("{instruction}\n\n{context}"))
}

fn readable_transcript(messages: &[Message]) -> String {
    let mut turns = Vec::new();
    for message in messages {
        let mut parts = Vec::new();
        for part in &message.parts {
            let text = match part {
                Part::Text { text } => text.clone(),
                Part::ToolCall { id, name, input } => {
                    format!("[工具调用 {name} · {id}]\n{input}")
                }
                Part::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => format!(
                    "[工具结果 {call_id} · {}]\n{content}",
                    if *is_error {
                        "错误标记"
                    } else {
                        "无错误标记"
                    }
                ),
                Part::Image { media_type, .. } => {
                    format!("[来源图片附件：{media_type}；此文字快照不含图片内容]")
                }
                Part::Document { media_type, .. } => {
                    format!("[来源文档附件：{media_type}；此文字快照不含附件内容]")
                }
                Part::Hosted { kind, raw, .. } => format!("[供应商工具 {kind}]\n{raw}"),
                Part::Reasoning { .. } => continue,
            };
            if !text.is_empty() {
                parts.push(text);
            }
        }
        if !parts.is_empty() {
            let speaker = match message.role {
                Role::User => match message.origin {
                    kanzei_llm::MessageOrigin::Human => "用户",
                    kanzei_llm::MessageOrigin::Engine => "引擎消息",
                    kanzei_llm::MessageOrigin::Notification => "通知",
                    kanzei_llm::MessageOrigin::Agent => "Agent",
                    kanzei_llm::MessageOrigin::Tool => "工具",
                    kanzei_llm::MessageOrigin::Summary => "摘要",
                    kanzei_llm::MessageOrigin::Unspecified => "来源未记录",
                },
                Role::Assistant => "助手",
            };
            turns.push(format!("[{speaker}]\n{}", parts.join("\n\n")));
        }
    }
    turns.join("\n\n")
}

#[cfg(test)]
mod tests;
