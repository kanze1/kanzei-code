use std::io::{stdout, Write as _};
use std::sync::{Arc, Mutex};

pub(crate) fn make_event_handler(
    typed_writer: Arc<Mutex<kanzei_core::TypedSessionWriter>>,
) -> impl FnMut(kanzei_core::RunEvent) + Send {
    let mut stdout = stdout();
    move |event| match event {
        kanzei_core::RunEvent::TurnStart { step, max_steps } => {
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
