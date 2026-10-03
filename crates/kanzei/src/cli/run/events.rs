use std::io::{stdout, Write as _};
use std::sync::{Arc, Mutex};

pub(crate) fn make_event_handler(
    typed_writer: Arc<Mutex<kanzei_core::TypedSessionWriter>>,
) -> impl FnMut(kanzei_core::RunEvent) + Send {
    let mut stdout = stdout();
    move |event| match event {
        kanzei_core::RunEvent::InputMessageCommitted { input_id, message } => {
            typed_writer
                .lock()
                .unwrap()
                .steering_message(&input_id, message);
        }
        kanzei_core::RunEvent::TurnStart {
            step, max_steps, ..
        } => {
            typed_writer.lock().unwrap().turn_started(step, max_steps);
            if step > 1 {
                let label = if max_steps > 0 {
                    format!("第 {step}/{max_steps} 轮")
                } else {
                    format!("第 {step} 轮")
                };
                let _ = writeln!(stdout, "\n\x1b[90m── {label} ──\x1b[0m");
            }
        }
        kanzei_core::RunEvent::Text(text) => {
            typed_writer.lock().unwrap().push_text(&text);
            let _ = write!(stdout, "{text}");
            let _ = stdout.flush();
        }
        kanzei_core::RunEvent::Reasoning(_) => {}
        kanzei_core::RunEvent::AssistantMessageCommitted { step, message } => typed_writer
            .lock()
            .unwrap()
            .assistant_committed(step, message),
        kanzei_core::RunEvent::ToolResultsCommitted { step, message } => typed_writer
            .lock()
            .unwrap()
            .tool_results_committed(step, message),
        kanzei_core::RunEvent::ToolStart { name, summary, .. } => {
            let _ = writeln!(stdout, "\n\x1b[36m● {name}\x1b[0m {summary}");
        }
        kanzei_core::RunEvent::HostedTool {
            name,
            kind,
            status,
            detail,
            ..
        } => {
            let query = detail["action"]["query"]
                .as_str()
                .or_else(|| detail["action"]["queries"][0]["q"].as_str())
                .unwrap_or("");
            let citations = detail.as_array().map(Vec::len).unwrap_or(0);
            let summary = if kind == "citations" {
                format!("{citations} numbered sources")
            } else if query.is_empty() {
                kind.clone()
            } else {
                query.to_string()
            };
            let _ = writeln!(stdout, "\n\x1b[36m◇ {name} ({status})\x1b[0m {summary}");
        }
        kanzei_core::RunEvent::TaskProgress { text, .. } => {
            let _ = writeln!(stdout, "  \x1b[90m… {text}\x1b[0m");
        }
        // CLI 不逐段转印工具输出:ToolEnd 的预览已够,逐段会与正文流互相穿插。
        kanzei_core::RunEvent::ToolProgress { .. } => {}
        kanzei_core::RunEvent::Retry {
            attempt,
            max,
            delay_ms,
        } => {
            let _ = writeln!(
                stdout,
                "\x1b[33m重试 {attempt}/{max},等待 {delay_ms}ms\x1b[0m"
            );
        }
        kanzei_core::RunEvent::StreamRestart {
            attempt,
            max,
            delay_ms,
        } => {
            typed_writer.lock().unwrap().stream_restarted();
            let _ = writeln!(
                stdout,
                "\x1b[33m连接中断,重新请求本轮 {attempt}/{max},等待 {delay_ms}ms(本轮工具尚未执行,不会重复副作用)\x1b[0m"
            );
        }
        kanzei_core::RunEvent::ToolEnd { ok, preview, .. } => {
            let mark = if ok {
                "\x1b[32m✓\x1b[0m"
            } else {
                "\x1b[31m✗\x1b[0m"
            };
            let _ = writeln!(stdout, "  {mark} {preview}");
        }
        kanzei_core::RunEvent::ContextCompacted {
            before_tokens,
            after_tokens,
            limit_tokens,
            dropped_messages,
            ..
        } => {
            let _ = writeln!(
                stdout,
                "\x1b[90m上下文到线,已压缩:约 {before_tokens} → {after_tokens} token(上限 {limit_tokens},裁掉 {dropped_messages} 条)\x1b[0m"
            );
        }
        kanzei_core::RunEvent::ContextPruned {
            cleared_results,
            before_tokens,
            after_tokens,
        } => {
            let _ = writeln!(
                stdout,
                "\x1b[90m已机械清理 {cleared_results} 条旧工具结果:约 {before_tokens} → {after_tokens} token(零 LLM)\x1b[0m"
            );
        }
        kanzei_core::RunEvent::WorkContextPrepared {
            report,
            source,
            surface,
            accepted,
        } => {
            let saved = typed_writer
                .lock()
                .unwrap()
                .commit_work_context_surface(&report, &source, &surface);
            accepted.store(saved, std::sync::atomic::Ordering::Release);
            if saved {
                let _ = writeln!(
                    stdout,
                    "{} → {}：已收起 {} 份旧读取/搜索结果，约减少 {} token；原文可回读",
                    report.from_item,
                    report.to_item,
                    report.archived_results,
                    report.before_tokens.saturating_sub(report.after_tokens)
                );
            } else {
                let _ = writeln!(stdout, "整理结果未保存，保留原上下文继续工作");
            }
        }
        // 规则直接判定的不打扰终端;需要人介入或被硬门禁挡下的才出声(D-173)。
        // R-183:deny/会话层决策打印命中的规则原文(验收④轨迹)。
        kanzei_core::RunEvent::PermissionResolved {
            action,
            resource,
            decision,
            source,
            rule,
            ..
        } => {
            if source != "ruleset" || decision == "deny" {
                let rule_text = rule
                    .as_deref()
                    .map(|r| format!(" [规则: {r}]"))
                    .unwrap_or_default();
                let _ = writeln!(
                    stdout,
                    "  \x1b[90m权限 {action} {resource} → {decision}({source}){rule_text}\x1b[0m"
                );
            }
        }
        kanzei_core::RunEvent::StepEnd { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_core::{
        prepare_typed_session, project_session_facts, project_session_facts_with_surface,
        SessionStore, TypedSessionWriter,
    };
    use kanzei_llm::{Message, Part};
    use serde_json::json;

    #[test]
    fn work_context_handler_forwards_original_source_and_rejects_later_mobile_fact() {
        for mobile_arrives in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "kz-cli-work-source-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let path = kanzei_core::project_state_path(&root);
            let store = SessionStore::open(&path).unwrap();
            store
                .create_session("ses", root.to_str().unwrap(), None)
                .unwrap();
            let source = vec![
                Message::user_text("保留用户要求"),
                Message::assistant(vec![Part::ToolCall {
                    id: "read".into(),
                    name: "read".into(),
                    input: json!({"path":"a.rs"}),
                }]),
                Message::tool_results(vec![Part::ToolResult {
                    call_id: "read".into(),
                    content: "旧读取正文".into(),
                    is_error: false,
                }]),
            ];
            store
                .append_event("ses", "conversation.updated", &json!({"messages":source}))
                .unwrap();
            prepare_typed_session(&store, "ses").unwrap();
            store.set_status("ses", "running").unwrap();
            let mut writer = TypedSessionWriter::new(&path, "ses", "run");
            writer.turn_started(1, 0);
            let writer = Arc::new(Mutex::new(writer));
            let mut handler = make_event_handler(writer.clone());
            let mut surface = source.clone();
            if let Part::ToolResult { content, .. } = &mut surface[2].parts[0] {
                *content = "旧观察已收起".into();
            }
            let mobile = Message::user_text("手机新输入必须保留");
            if mobile_arrives {
                let mut phone = TypedSessionWriter::new(&path, "ses", "mobile");
                phone.user_message("mobile-input", mobile.clone());
                assert!(phone.errors().is_empty(), "{:?}", phone.errors());
            }
            let accepted = Arc::new(std::sync::atomic::AtomicBool::new(false));
            handler(kanzei_core::RunEvent::WorkContextPrepared {
                report: kanzei_core::runner::WorkContextReport {
                    from_item: "R-001".into(),
                    to_item: "R-002".into(),
                    related: false,
                    archived_results: 1,
                    before_tokens: 8_000,
                    after_tokens: 3_000,
                },
                source: source.clone(),
                surface: surface.clone(),
                accepted: accepted.clone(),
            });
            assert_eq!(
                accepted.load(std::sync::atomic::Ordering::Acquire),
                !mobile_arrives
            );
            assert!(writer.lock().unwrap().errors().is_empty());
            let facts = store.list_session_facts("ses").unwrap();
            let committed = store.latest_completed_compaction_surface("ses", 0).unwrap();
            if mobile_arrives {
                assert!(committed.is_none());
                let mut expected = source;
                expected.push(mobile);
                assert_eq!(project_session_facts(&facts).surface_messages, expected);
            } else {
                let (sequence, committed) = committed.unwrap();
                assert_eq!(
                    project_session_facts_with_surface(&facts, Some(sequence), Some(committed))
                        .surface_messages,
                    surface
                );
            }
            drop(handler);
            drop(writer);
            drop(store);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
