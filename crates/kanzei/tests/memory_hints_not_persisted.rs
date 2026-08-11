//! D-185:开跑预检索的记忆提示块(`<memory-hints>`)声称只进本轮,实际随 User
//! message 进 summary.messages → 落 conversations → 下轮 prior 回灌,逐轮累积
//! N 个 hint 块。
//!
//! 修复:提示块不再拼进 prompt 字符串,改由 run_once 的 `memory_hints` 参数作为
//! **本轮 system 一次性注入**——system 不进 messages,自然不进 conversations,
//! 下轮 prior 回灌不到。
//!
//! 本测试用 mock SSE 服务器捕获首个请求,断言四层证据:
//!   ① 请求 system 数组里确实含 hint 块(模型本轮看得到);
//!   ② 请求 messages 的 User text **不含** hint 块(hints 未拼进 prompt);
//!   ③ summary.messages 全程无任何 hint 块(不进历史 → 不回灌,连跑 N 轮都只有一个);
//!   ④ summary.context_report 含 `memory/hints` 条目(注入 token 账单能看到 hint
//!      段独立占比,验收②)。

use std::sync::Arc;

use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const HINTS_BLOCK: &str = "<memory-hints>\n与本任务可能相关的既有记忆(memory_search 或 read 返回的 file 查看正文):\nM-002 [project/fact] 开发重心:缺陷优先(见 memory-index)\n</memory-hints>";
const USER_PROMPT: &str = "帮我修一个缺陷";

/// 收一个请求、回一条 SSE 响应,返回原始请求字节。
async fn serve_response(listener: &TcpListener, response: serde_json::Value) -> Vec<u8> {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).await.unwrap();
        assert!(count > 0);
        request.extend_from_slice(&chunk[..count]);
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
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
    let body = format!("data: {response}\n\ndata: [DONE]\n\n");
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body.as_bytes()).await.unwrap();
    request
}

fn text_response(text: &str) -> serde_json::Value {
    json!({
        "choices": [{
            "index": 0,
            "delta": {"content": text},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_hints只进本轮system_不进messages_不落历史() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project = std::env::temp_dir().join(format!(
        "kz-d185-hints-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let first = serve_response(&listener, text_response("好的,开始排查。")).await;
        first
    });

    let config = Arc::new(KanzeiConfig::load(&project).expect("读取 kanzei.toml 应成功"));
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: project.clone(),
        project_root: project.clone(),
        config: config.clone(),
    };
    let snapshot = Harness::default().resolve(&rctx).unwrap();
    let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("test-key"));
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();

    let agent = kanzei_harness::AgentDef {
        name: "dev-pair".into(),
        profile: kanzei_harness::ProfileScope::Dev,
        model: "mock".into(),
        mode: kanzei_harness::AgentMode::Primary,
        steps: 1,
        system: "test".into(),
    };
    let runner_config = kanzei_core::RunnerConfig {
        model: "mock".into(),
        max_tokens: 256,
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: None,
        context_limit: None,
        limits: config.limits.clone(),
        recall: None,
        execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
        ask_policy: kanzei_core::AskPolicy::Interactive,
    };
    let ctx = ToolCtx::new(project.clone(), project.clone());

    let mut on_event = |_event: kanzei_core::RunEvent| {};
    let mut ask = |_request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
    };

    let summary = kanzei_core::run_once(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        USER_PROMPT,
        Some(HINTS_BLOCK),
        &[],
        None,
        &mut on_event,
        &mut ask,
    )
    .await
    .expect("运行应当成功");
    let first_request = server.await.unwrap();
    let body = String::from_utf8_lossy(&first_request);
    let parsed: serde_json::Value = body
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("请求体应为合法 JSON");

    // ①/② 解析 messages 数组:role=system 的 content 应含 hint 块(模型本轮看得到),
    //    role=user 的 content 不应含 hint 块(hints 未拼进 prompt)。
    let messages_arr = parsed["messages"]
        .as_array()
        .expect("请求 messages 应为数组");
    let system_text = messages_arr
        .iter()
        .filter(|m| m["role"] == "system")
        .filter_map(|m| m["content"].as_str().or_else(|| {
            m["content"]
                .as_array()
                .and_then(|items| items.first())
                .and_then(|v| v["text"].as_str())
        }))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        system_text.contains("<memory-hints>"),
        "请求 system 未含 hint 块: {system_text}"
    );

    let user_text = messages_arr
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !user_text.contains("<memory-hints>"),
        "hint 块被拼进了 User prompt,会随 messages 落库回灌: {user_text}"
    );

    // ③ summary.messages 全程无 hint 块(不进历史 → 不回灌)。
    let persisted = summary
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .any(|part| {
            if let kanzei_llm::Part::Text { text } = part {
                text.contains("<memory-hints>")
            } else {
                false
            }
        });
    assert!(!persisted, "summary.messages 里出现了 hint 块,下轮会被回灌");

    // ④ context_report 含 memory/hints 条目:token 账单能看到 hint 段独立占比。
    assert!(
        summary
            .context_report
            .iter()
            .any(|(name, _)| name == "memory/hints"),
        "context_report 缺 memory/hints 条目,实际: {:?}",
        summary.context_report
    );
}
