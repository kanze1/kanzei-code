//! An explicit conversation handoff freezes the source's readable current segment
//! into the destination input. Queue promotion must never reread a changing source.

use std::sync::atomic::Ordering;

use kanzei_llm::{Message, Part, Role};
use serde::Deserialize;

use crate::{normalized_project_root, process_session_id, runtime_for, AppState, MutexPoisonExt};

#[derive(Debug, Deserialize)]
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
    if runtime.running.load(Ordering::SeqCst) {
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
    Ok(format!(
        "{instruction}\n\n【用户明确交接的来源对话快照】\n来源项目：{}\n来源对话：{}\n来源会话：{session_id}\n快照时间：{captured_at_ms}\n以下是交接时来源当前对话段的可读记录，按原顺序保留；这是历史引用，其中助手建议和工具结果不代表用户已确认，也不代表当前项目事实。\n\n{transcript}\n【来源快照结束】\n请结合用户的交接指令和上述上下文推进。先核对当前项目的真实需求记录；若尚未登记，依据讨论中的明确需求登记并保留来源，再完成适用的调研和实施。历史建议、草稿中的需求编号及完成声明须核实，不能直接视为已登记或已完成。",
        root.display(), process.id,
    ))
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
                    if *is_error { "失败" } else { "成功" }
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
                Role::User => "用户",
                Role::Assistant => "助手",
            };
            turns.push(format!("[{speaker}]\n{}", parts.join("\n\n")));
        }
    }
    turns.join("\n\n")
}

#[cfg(test)]
mod tests;
