//! 社区高频交互辅助命令：
//! - `pending_asks_all`：全局待办收件箱（跨项目聚合所有活跃会话待审批请求）
//! - `shell_exec_quick`：`!命令` 本地 Shell 直通（严格受档位约束，带超时与截断保护）
//! - `side_question`：`/btw` 旁路只读提问（读取当前会话只读投影，绝不写入 SessionStore）

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::{normalized_project_root, pending_ask_payload, process_session_id, AppState};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SideTurn {
    pub(crate) role: String,
    pub(crate) text: String,
}

const MAX_SHELL_OUTPUT_BYTES: usize = 32 * 1024;
const MAX_SIDE_HISTORY_MESSAGES: usize = 24;
const MAX_SIDE_MSG_CHARS: usize = 2400;

fn truncate_utf8_bytes(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    if bytes.len() <= max_bytes {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    let mut end = max_bytes;
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    (String::from_utf8_lossy(&bytes[..end]).into_owned(), true)
}

/// 跨项目聚合全部活跃会话的待确认权限/提问请求，供全局收件箱（Inbox）直接审阅。
#[tauri::command]
pub(crate) fn pending_asks_all(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let runtimes: Vec<_> = state.runtimes.lock().unwrap().values().cloned().collect();
    let process_map: std::collections::HashMap<String, (String, String)> = {
        let procs = state.processes.lock().unwrap();
        procs
            .values()
            .map(|p| {
                let sid = process_session_id(&p.origin_project.0, Some(&p.id));
                (
                    sid,
                    (p.id.clone(), p.origin_project.0.display().to_string()),
                )
            })
            .collect()
    };
    let mut out = Vec::new();
    for runtime in runtimes {
        let asks = runtime.asks.lock().unwrap();
        for (id, pending) in asks.iter() {
            let mut payload = pending_ask_payload(*id, pending);
            payload["projectDir"] = json!(pending.project_root.display().to_string());
            if let Some((pid, origin)) = process_map.get(&pending.session_id) {
                payload["processId"] = json!(pid);
                payload["projectDir"] = json!(origin);
            }
            out.push(payload);
        }
    }
    out.sort_by_key(|v| v["id"].as_u64().unwrap_or(0));
    Ok(out)
}

fn resolve_execution_cwd(
    state: &AppState,
    root: &Path,
    process_id: Option<&str>,
) -> Result<(PathBuf, Option<String>), String> {
    let mut cwd = root.to_path_buf();
    let mut profile: Option<String> = None;
    if let Some(pid) = process_id.filter(|v| !v.trim().is_empty()) {
        if let Some(proc_handle) = state.processes.lock().unwrap().get(pid).cloned() {
            profile = proc_handle.profile.lock().unwrap().clone();
            if let Some(wt) = &proc_handle.worktree_path {
                cwd = wt.0.clone();
            }
        }
    }
    if crate::general_chat::is_general_root(root) {
        let session = process_session_id(root, process_id);
        let general_cwd = kanzei_harness::general_conversation_workspace(root, &session);
        std::fs::create_dir_all(&general_cwd).map_err(|e| e.to_string())?;
        cwd = general_cwd;
    }
    Ok((cwd, profile))
}

/// `!命令` 本地 Shell 直通执行：仅允许在非只读、非隔离模式下运行，带超时与输出截断。
#[tauri::command]
pub(crate) async fn shell_exec_quick(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
    command: String,
    timeout_secs: Option<u64>,
) -> Result<serde_json::Value, String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err("请输入要执行的命令".into());
    }
    if trimmed.len() > 4096 {
        return Err("命令长度超过 4096 字符上限".into());
    }
    let root = normalized_project_root(Path::new(&project_dir));
    let (cwd, profile) = resolve_execution_cwd(&state, &root, process_id.as_deref())?;
    if matches!(profile.as_deref(), Some("readonly" | "isolated")) {
        return Err("当前对话处于只读或隔离档位，禁止执行本地 Shell 命令".into());
    }
    if !cwd.is_dir() {
        return Err(format!("工作目录不存在：{}", cwd.display()));
    }
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(30).clamp(1, 120));
    let started = Instant::now();

    #[cfg(windows)]
    let mut cmd = {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = tokio::process::Command::new("pwsh");
        c.args(["-NoProfile", "-NonInteractive", "-Command", trimmed]);
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };

    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", trimmed]);
        c
    };

    cmd.current_dir(&cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .map_err(|e| format!("无法启动本地 Shell 进程: {e}"))?;

    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            let (stdout, t1) = truncate_utf8_bytes(&output.stdout, MAX_SHELL_OUTPUT_BYTES);
            let (stderr, t2) = truncate_utf8_bytes(&output.stderr, MAX_SHELL_OUTPUT_BYTES);
            Ok(json!({
                "command": trimmed,
                "cwd": cwd.display().to_string(),
                "exitCode": output.status.code().unwrap_or(-1),
                "stdout": stdout,
                "stderr": stderr,
                "durationMs": started.elapsed().as_millis() as u64,
                "truncated": t1 || t2,
                "timedOut": false,
            }))
        }
        Ok(Err(error)) => Err(format!("读取命令输出失败: {error}")),
        Err(_) => Ok(json!({
            "command": trimmed,
            "cwd": cwd.display().to_string(),
            "exitCode": -1,
            "stdout": "",
            "stderr": format!("命令执行超过 {} 秒已超时终止", timeout.as_secs()),
            "durationMs": started.elapsed().as_millis() as u64,
            "truncated": false,
            "timedOut": true,
        })),
    }
}

fn summarize_message_for_side(message: &kanzei_llm::Message) -> Option<String> {
    let role = match message.role {
        kanzei_llm::Role::User => "用户",
        kanzei_llm::Role::Assistant => "助手",
    };
    let mut fragments = Vec::new();
    for part in &message.parts {
        match part {
            kanzei_llm::Part::Text { text } if !text.trim().is_empty() => {
                fragments.push(text.trim().to_string());
            }
            kanzei_llm::Part::ToolCall { name, .. } => {
                fragments.push(format!("[调用工具: {name}]"));
            }
            kanzei_llm::Part::ToolResult {
                content, is_error, ..
            } => {
                let first_line = content.lines().next().unwrap_or_default();
                let tag = if *is_error {
                    "工具错误"
                } else {
                    "工具结果"
                };
                let clipped: String = first_line.chars().take(180).collect();
                fragments.push(format!("[{tag}: {clipped}]"));
            }
            _ => {}
        }
    }
    if fragments.is_empty() {
        return None;
    }
    let joined: String = fragments
        .join("\n")
        .chars()
        .take(MAX_SIDE_MSG_CHARS)
        .collect();
    Some(format!("{role}: {joined}"))
}

const BTW_SYSTEM_PROMPT: &str = "你是 Kanzei 的旁路只读问答助手（/btw 模式）。\
你会收到当前主对话的只读上下文摘录以及用户的临时旁路问题。\
请直接、准确、简洁地回答用户的问题；你的回答不会写入主对话历史，也不具备修改文件或执行命令的权限。\
如果问题依赖主对话中没有出现的事实，请明确指出。";

/// `/btw` 旁路只读提问：共享当前对话只读快照，不写入 SessionStore、不调用工具。
#[tauri::command]
pub(crate) async fn side_question(
    state: State<'_, AppState>,
    project_dir: String,
    process_id: Option<String>,
    question: String,
    history: Option<Vec<SideTurn>>,
) -> Result<serde_json::Value, String> {
    let trimmed = question.trim();
    if trimmed.is_empty() {
        return Err("请输入旁路问题".into());
    }
    if trimmed.len() > 16_000 {
        return Err("旁路问题过长（上限 16 KB）".into());
    }
    let root = normalized_project_root(Path::new(&project_dir));
    let session_id = process_session_id(&root, process_id.as_deref());
    let general = crate::general_chat::is_general_root(&root);

    let mut config = if general {
        kanzei_harness::KanzeiConfig::load_global_with_warnings()
            .map(|(c, _)| c)
            .map_err(|e| e.to_string())?
    } else {
        kanzei_harness::KanzeiConfig::load_at_root(&root).map_err(|e| e.to_string())?
    };
    config.fill_defaults();

    let process_model = process_id
        .as_deref()
        .and_then(|pid| state.processes.lock().unwrap().get(pid).cloned())
        .and_then(|handle| handle.model.lock().unwrap().clone());
    let model_ref = process_model.as_deref().unwrap_or("primary");
    let resolved = config
        .resolve_model(model_ref)
        .or_else(|_| config.resolve_model("primary"))
        .map_err(|e| e.to_string())?;

    let proxy = match config.proxy.as_deref() {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(value) => kanzei_llm::ProxyConfig::Explicit(value.to_owned()),
    };

    // 只读提取当前会话最近一段消息作为上下文摘录，不打开写事务、不修改历史。
    let state_path = kanzei_core::project_state_path(&root);
    let context_excerpt = if state_path.is_file() {
        if let Ok(store) = kanzei_core::SessionStore::open_read_only(&state_path) {
            if let Ok(raw) = crate::conversation::project_latest_segment(&store, &session_id) {
                let filtered = kanzei_core::filter_message_history(&raw);
                let start = filtered.len().saturating_sub(MAX_SIDE_HISTORY_MESSAGES);
                let lines: Vec<String> = filtered[start..]
                    .iter()
                    .filter_map(summarize_message_for_side)
                    .collect();
                if lines.is_empty() {
                    None
                } else {
                    Some(format!(
                        "以下是当前主对话最近 {} 条消息的只读上下文摘录：\n\n{}",
                        lines.len(),
                        lines.join("\n\n---\n\n")
                    ))
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let mut system = vec![BTW_SYSTEM_PROMPT.to_string()];
    if let Some(excerpt) = context_excerpt {
        system.push(excerpt);
    }

    let mut messages = Vec::new();
    for turn in history.unwrap_or_default().into_iter().take(12) {
        let text = turn.text.trim();
        if text.is_empty() {
            continue;
        }
        if turn.role.eq_ignore_ascii_case("assistant") {
            messages.push(kanzei_llm::Message::assistant(vec![
                kanzei_llm::Part::Text {
                    text: text.to_string(),
                },
            ]));
        } else {
            messages.push(kanzei_llm::Message::user_text(text));
        }
    }
    messages.push(kanzei_llm::Message::user_text(trimmed));

    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|e| e.to_string())?;
    let client = kanzei_llm::LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let request = kanzei_llm::LlmRequest {
        model: resolved.model.clone(),
        system,
        messages,
        tools: Vec::new(),
        hosted_tools: Vec::new(),
        max_tokens: config.limits.max_tokens().min(4096),
        temperature: None,
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: config.service_tier_for(&resolved),
    };

    let (answer, input_tokens, output_tokens) =
        tokio::time::timeout(Duration::from_secs(120), async {
            let mut stream = client
                .stream(&route, &request)
                .await
                .map_err(|e| e.to_string())?;
            let mut output = String::new();
            let mut in_tok = 0u64;
            let mut out_tok = 0u64;
            while let Some(event) = stream.next().await {
                match event.map_err(|e| e.to_string())? {
                    kanzei_llm::LlmEvent::TextDelta { text, .. } => output.push_str(&text),
                    kanzei_llm::LlmEvent::StepFinish { usage, .. } => {
                        in_tok = usage.input;
                        out_tok = usage.output;
                    }
                    _ => {}
                }
            }
            Ok::<_, String>((output, in_tok, out_tok))
        })
        .await
        .map_err(|_| "旁路提问超时，请重试".to_string())??;

    Ok(json!({
        "answer": answer,
        "model": format!("{}:{}", resolved.provider_name, resolved.model),
        "inputTokens": input_tokens,
        "outputTokens": output_tokens,
    }))
}
