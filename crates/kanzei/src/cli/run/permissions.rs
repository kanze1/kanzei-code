use std::path::PathBuf;

use super::super::memory::persist_always_allow;

pub(crate) fn make_ask(
    ask_root: PathBuf,
    interactive: bool,
    non_interactive_policy: kanzei_harness::config::NonInteractive,
    allowlist: Vec<(String, String)>,
) -> impl FnMut(kanzei_core::AskRequest) -> kanzei_core::AskFuture + Send {
    move |request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        let response = match request {
            kanzei_core::AskRequest::Question {
                question,
                options,
                default,
                multiple,
            } => {
                eprint!("\x1b[33m? {question}");
                if !options.is_empty() {
                    if multiple {
                        eprint!(" [可多选,逗号分隔]");
                    }
                    eprint!(
                        " [{}]",
                        options
                            .iter()
                            .map(|o| o.label.as_str())
                            .collect::<Vec<_>>()
                            .join(" / ")
                    );
                }
                // R-328:带注解的选项在 CLI 里逐行列出——一行挤不下「选它意味着
                // 什么」,而那正是提问的原因。无注解的选项不多打一行。
                for option in options.iter().filter(|o| o.note.is_some()) {
                    eprint!(
                        "
[90m    {} — {}[33m",
                        option.label,
                        option.note.as_deref().unwrap_or_default()
                    );
                }
                if let Some(default) = default {
                    eprint!(" (默认: {default})");
                }
                eprint!("\x1b[0m ");
                let mut line = String::new();
                if std::io::stdin().read_line(&mut line).is_ok() && !line.trim().is_empty() {
                    kanzei_core::AskResponse::Answer(line.trim().to_string())
                } else {
                    kanzei_core::AskResponse::Cancelled
                }
            }
            kanzei_core::AskRequest::Permission { action, resource } => {
                if !interactive {
                    // R-183:非交互通道不读 stdin,按配置策略分流(缺省 deny)。
                    // 拒绝/放行都走 drive 层的 PermissionResolved 事件落轨迹。
                    let reply = super::super::non_interactive_decision(
                        non_interactive_policy,
                        &allowlist,
                        &action,
                        &resource,
                    );
                    kanzei_core::AskResponse::Permission(reply)
                } else {
                    eprint!("\x1b[33m? {action}: {resource} [y 一次 / a 总是 / N 拒绝]\x1b[0m ");
                    let mut line = String::new();
                    let reply = if std::io::stdin().read_line(&mut line).is_ok() {
                        match line.trim() {
                            "y" | "Y" | "yes" => kanzei_core::AskReply::AllowOnce,
                            "a" | "A" | "always" => {
                                match persist_always_allow(&ask_root, &action, &resource) {
                                    Ok(reply) => reply,
                                    Err(error) => {
                                        eprintln!(
                                            "\x1b[31m总是允许规则保存失败: {error};本次拒绝\x1b[0m"
                                        );
                                        kanzei_core::AskReply::Deny
                                    }
                                }
                            }
                            _ => kanzei_core::AskReply::Deny,
                        }
                    } else {
                        kanzei_core::AskReply::Deny
                    };
                    kanzei_core::AskResponse::Permission(reply)
                }
            }
        };
        Box::pin(async move { response })
    }
}
