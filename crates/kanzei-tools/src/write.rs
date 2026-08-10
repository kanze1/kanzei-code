//! write 工具。设计红线 5:结构化文本写入后做语法校验,坏格式以 warning 告知模型。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
struct WriteInput {
    /// 文件路径(绝对或相对 cwd)
    #[serde(alias = "file_path", alias = "filepath", alias = "file")]
    path: String,
    /// 完整文件内容
    #[serde(alias = "text", alias = "contents")]
    content: String,
}

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> String {
        "Write a file (overwrites). Params: path, content.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(WriteInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["path"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::write_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: WriteInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let path = ctx
            .cwd
            .join(kanzei_harness::permission::normalize_resource(&input.path));
        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolOutput::error(format!("cannot create {}: {e}", parent.display()));
            }
        }
        // 覆写前抓旧内容,给 UI 出 diff(看得见改了什么,R-015)。
        let previous = tokio::fs::read_to_string(&path).await.ok();
        if let Err(e) = tokio::fs::write(&path, input.content.as_bytes()).await {
            return ToolOutput::error(format!("cannot write {}: {e}", path.display()));
        }
        let mut message = format!("wrote {} bytes to {}", input.content.len(), path.display());
        if let Some(warning) = validate_syntax(&path, &input.content) {
            message.push_str(&format!("\nWARNING: {warning}"));
        }
        let display = match previous {
            Some(old) => diff_display(&input.path, &old, &input.content),
            None => serde_json::json!({
                "kind": "create",
                "path": input.path,
                "bytes": input.content.len(),
                "preview": input.content.lines().take(30).collect::<Vec<_>>().join("\n"),
            }),
        };
        ToolOutput::ok(message).with_display(display)
    }
}

/// 统一 diff 展示(edit/write 共用)。上限截断,防止巨型文件撑爆前端。
pub(crate) fn diff_display(path: &str, old: &str, new: &str) -> serde_json::Value {
    let diff = similar::TextDiff::from_lines(old, new);
    let mut additions = 0usize;
    let mut deletions = 0usize;
    let mut text = String::new();
    let mut lines = Vec::new();
    let mut old_line = 1usize;
    let mut new_line = 1usize;
    let mut truncated = false;
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Insert => {
                additions += 1;
                "+"
            }
            similar::ChangeTag::Delete => {
                deletions += 1;
                "-"
            }
            similar::ChangeTag::Equal => " ",
        };
        let value = change.value().trim_end_matches('\n');
        if text.len() < 24 * 1024 {
            let (kind, old, new) = match change.tag() {
                similar::ChangeTag::Insert => ("add", None, Some(new_line)),
                similar::ChangeTag::Delete => ("del", Some(old_line), None),
                similar::ChangeTag::Equal => ("ctx", Some(old_line), Some(new_line)),
            };
            lines.push(serde_json::json!({
                "kind": kind,
                "old_line": old,
                "new_line": new,
                "text": value,
            }));
        } else {
            truncated = true;
        }
        match change.tag() {
            similar::ChangeTag::Insert => new_line += 1,
            similar::ChangeTag::Delete => old_line += 1,
            similar::ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
            }
        }
        // 等值上下文只保留短窗口由前端裁剪;此处限制总量。
        if text.len() < 24 * 1024 {
            text.push_str(sign);
            text.push_str(change.value());
            if !change.value().ends_with('\n') {
                text.push('\n');
            }
        }
    }
    if text.len() >= 24 * 1024 {
        truncated = true;
        text.push_str("(diff 截断)\n");
    }
    serde_json::json!({
        "kind": "diff",
        "path": path,
        "language": diff_language(path),
        "diff": text,
        "lines": lines,
        "additions": additions,
        "deletions": deletions,
        "truncated": truncated,
    })
}

fn diff_language(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "json" => "json",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "css" => "css",
        "html" | "htm" => "html",
        "py" => "python",
        "ps1" | "sh" | "bash" => "shell",
        _ => "text",
    }
}

/// 写后语法校验(不阻断,只告知)。edit 工具复用。
pub(crate) fn validate_syntax(path: &std::path::Path, content: &str) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "json" => serde_json::from_str::<serde_json::Value>(content)
            .err()
            .map(|e| format!("file was written but is not valid JSON: {e}")),
        "toml" => content
            .parse::<toml::Table>()
            .err()
            .map(|e| format!("file was written but is not valid TOML: {e}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{diff_display, WriteTool};
    use crate::{BaseComponent, DevProfile};
    use kanzei_core::{AskFuture, AskReply, AskRequest, AskResponse, RunnerConfig};
    use kanzei_harness::{
        rule, ConfigComponent, Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx,
    };
    use kanzei_llm::{LlmClient, ProxyConfig, ReasoningEffort, Route};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn diff_display_exposes_language_counts_and_line_numbers() {
        let display = diff_display(
            "src/main.rs",
            "fn main() {\nold();\n}\n",
            "fn main() {\nnew();\n}\n",
        );
        assert_eq!(display["kind"], "diff");
        assert_eq!(display["language"], "rust");
        assert_eq!(display["additions"], 1);
        assert_eq!(display["deletions"], 1);
        assert_eq!(display["lines"][0]["old_line"], 1);
        assert_eq!(display["lines"][0]["new_line"], 1);
        assert_eq!(display["lines"][1]["kind"], "del");
        assert_eq!(display["lines"][1]["old_line"], 2);
        assert_eq!(display["lines"][2]["kind"], "add");
        assert_eq!(display["lines"][2]["new_line"], 2);
    }

    #[tokio::test]
    async fn permission_path_and_write落点使用同一规范化结果() {
        use kanzei_harness::permission::{Effect, Rule, Ruleset};
        use kanzei_harness::{Tool, ToolCtx};

        let root = std::env::temp_dir().join(format!(
            "kanzei-d050-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let input_path = ".KANZEI/./project/../allowed.md";
        let normalized = kanzei_harness::permission::normalize_resource(input_path);
        assert_eq!(normalized, ".kanzei/allowed.md");

        let deny = Ruleset::new(vec![Rule {
            action: "write".into(),
            resource: "*.kanzei/project/*".into(),
            effect: Effect::Deny,
        }]);
        assert_eq!(
            deny.evaluate(
                "write",
                &kanzei_harness::permission::normalize_resource(".KANZEI\\project\\secret.md")
            ),
            Effect::Deny
        );

        let output = WriteTool
            .execute(
                serde_json::json!({"path": input_path, "content": "落点一致"}),
                &ToolCtx {
                    cwd: root.clone(),
                    project_root: root.clone(),
                },
            )
            .await;
        assert!(!output.is_error, "{output:?}");
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei/allowed.md")).unwrap(),
            "落点一致"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn runner_hard_deny_blocks_real_write_tool_before_filesystem_side_effect() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let root = std::env::temp_dir().join(format!(
            "kanzei-d050-runner-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let input_path = if cfg!(windows) {
            r".KANZEI\project\requirements.md"
        } else {
            r".kanzei\project\requirements.md"
        };
        let call_input = serde_json::json!({
            "path": input_path,
            "content": "must not be written"
        });
        let first_response = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_d050",
                        "type": "function",
                        "function": {
                            "name": "write",
                            "arguments": call_input.to_string()
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });
        let second_response = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {"content": "权限拒绝"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for response in [first_response, second_response] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4096];
                let header_end = loop {
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(position) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        break position + 4;
                    }
                };
                let content_length = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                while request.len() < header_end + content_length {
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                }
                let body = format!("data: {}\n\ndata: [DONE]\n\n", response);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).await.unwrap();
                stream.write_all(body.as_bytes()).await.unwrap();
            }
        });

        let mut config = KanzeiConfig::default();
        config.permissions.rules.push(rule(
            "write",
            "*.kanzei/project/*",
            kanzei_harness::Effect::Ask,
        ));
        let resolve_ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(config),
        };
        let mut harness = Harness::default();
        harness
            .add(BaseComponent)
            .add(DevProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&resolve_ctx).unwrap();
        let agent = snapshot.select_agent(None).unwrap();
        let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
        let route = Route::openai_at(&format!("http://{address}/v1"), None);
        let runner_config = RunnerConfig {
            model: "mock".into(),
            max_tokens: 128,
            reasoning: ReasoningEffort::Off,
            service_tier: None,
            context_limit: None,
            limits: Default::default(),
            recall: None,
        };
        let tool_ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
        };
        let mut on_event = |_event| {};
        let ask_count = Arc::new(AtomicUsize::new(0));
        let ask_count_for_callback = Arc::clone(&ask_count);
        let mut ask = move |_request: AskRequest| -> AskFuture {
            ask_count_for_callback.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { AskResponse::Permission(AskReply::Deny) })
        };

        let summary = kanzei_core::run_once(
            &client,
            &route,
            snapshot.as_ref(),
            agent,
            &runner_config,
            &tool_ctx,
            "执行写入",
            &[],
            None,
            &mut on_event,
            &mut ask,
        )
        .await
        .unwrap();

        assert_eq!(ask_count.load(Ordering::SeqCst), 0);
        assert!(summary.messages.iter().flat_map(|message| &message.parts).any(
            |part| matches!(part, kanzei_llm::Part::ToolResult { content, is_error: true, .. } if content.contains("permission denied"))
        ));
        assert!(!root.join(".kanzei/project/requirements.md").exists());
        server.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diff_display_marks_large_output_as_truncated() {
        let old = "a\n".repeat(20_000);
        let new = "b\n".repeat(20_000);
        let display = diff_display("notes.txt", &old, &new);
        assert_eq!(display["language"], "text");
        assert_eq!(display["truncated"], true);
        assert!(display["lines"].as_array().unwrap().len() < 40_000);
    }
}
