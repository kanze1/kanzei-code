//! D-342 协作式停止:停止令牌置位后,run 在安全检查点以 halted_by_user=true
//! **正常返回**——messages 完整交还(prior + 本轮已产出部分),不再靠 abort 硬杀
//! 丢掉被打断轮的对话。两个场景:
//!   ① 步首检查点:令牌先置位,run 一个 provider 请求都不发,立刻 halted 收尾,
//!      prior 与本轮用户消息原样在 messages 里(轮末写回的数据源完好)。
//!   ② 执行中停止:模型派发的 task 子代理挂起时置位令牌——task 等待被打断,
//!      缺席结果以取消占位配对,历史无孤儿 ToolCall(filter 后逐字节不变)。

use std::sync::{Arc, Mutex};

use kanzei_harness::orchestration::ExecutionPolicy;
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn serve_response(listener: &TcpListener, response: serde_json::Value) -> Vec<u8> {
    // 桩服务器绝不无限等待:请求数一旦对不上,要的是变红而不是挂死整个门禁。
    let (mut stream, _) = tokio::time::timeout(std::time::Duration::from_secs(20), listener.accept())
        .await
        .expect("等待模型请求超时。多半是生产侧的轮次数变了(例如权限被拒导致本轮提前收口),桩服务器还在等下一次请求——改测试对齐新契约,不要让 cargo test --workspace 静默挂死")
        .unwrap();
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

fn dev_fixture(tag: &str) -> (std::path::PathBuf, Arc<KanzeiConfig>) {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project =
        std::env::temp_dir().join(format!("kz-d342-{tag}-{}-{suffix}", std::process::id()));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();
    let config = Arc::new(KanzeiConfig::load(&project).expect("读取 kanzei.toml 应成功"));
    (project, config)
}

fn agent() -> kanzei_harness::AgentDef {
    kanzei_harness::AgentDef {
        name: "dev-pair".into(),
        profile: kanzei_harness::ProfileScope::Dev,
        model: "mock".into(),
        mode: kanzei_harness::AgentMode::Primary,
        steps: 4,
        system: "test".into(),
    }
}

fn runner_config(
    config: &KanzeiConfig,
    halt: kanzei_core::CancellationToken,
) -> kanzei_core::RunnerConfig {
    kanzei_core::RunnerConfig {
        model: "mock".into(),
        max_tokens: 256,
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: None,
        context_limit: None,
        limits: config.limits.clone(),
        recall: None,
        execution_policy: ExecutionPolicy::Default,
        ask_policy: kanzei_core::AskPolicy::Interactive,
        halt: Some(halt),
    }
}

/// ① 步首检查点:令牌先置位 → 不发任何请求、halted 正常返回、messages 完好。
/// route 指向一个**没有服务端**的端口:若检查点失守而真的发请求,连接错误会让
/// 断言以另一种方式失败——测试对「发了请求」零容忍。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn 停止先置位_步首收尾_prior与用户消息完整交还() {
    let (project, config) = dev_fixture("prehalt");
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: project.clone(),
        project_root: project.clone(),
        config: config.clone(),
    };
    let snapshot = Harness::default().resolve(&rctx).unwrap();
    // 只 bind 不 accept:请求若发出必然失败,反证检查点在请求之前。
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("test-key"));
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();

    let halt = kanzei_core::CancellationToken::new();
    halt.cancel();
    let runner_config = runner_config(&config, halt);
    let ctx = ToolCtx::new(project.clone(), project.clone());
    let prior = vec![kanzei_llm::Message::user_text(
        "上一轮做过的事:改了 store.rs",
    )];
    let mut on_event = |_event: kanzei_core::RunEvent| {};
    let mut ask = |_request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Cancelled })
    };

    let summary = kanzei_core::run_once_with_parts(
        &client,
        &route,
        &snapshot,
        &agent(),
        &runner_config,
        &ctx,
        "新的临时任务",
        None,
        &prior,
        None,
        None,
        &mut on_event,
        &mut ask,
    )
    .await
    .expect("halted 是正常返回,不是错误");

    assert!(summary.halted_by_user, "必须以 halted 收尾");
    assert_eq!(summary.steps, 0, "一个 provider 步都不该发生");
    let text: String = summary
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .filter_map(|p| match p {
            kanzei_llm::Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("上一轮做过的事:改了 store.rs"),
        "prior 必须原样在 messages 里(轮末写回数据源):\n{text}"
    );
    assert!(text.contains("新的临时任务"), "本轮用户消息也要在");
    std::fs::remove_dir_all(&project).ok();
}

/// ② 执行中停止:task 子代理挂起时置位令牌——等待被打断、缺席结果取消占位、
/// 历史无孤儿(filter_message_history 后逐字节不变)。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 执行中停止_取消占位配对_历史无孤儿() {
    let (project, config) = dev_fixture("midhalt");
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: project.clone(),
        project_root: project.clone(),
        config: config.clone(),
    };
    let snapshot = Harness::default().resolve(&rctx).unwrap();
    let mut sub_harness = Harness::default();
    sub_harness.add(kanzei_tools::SubagentBase);
    let sub_snapshot = sub_harness.resolve(&rctx).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task_id = "call_task_hang".to_string();
    let dispatch = json!({
        "choices": [{
            "index": 0,
            "delta": {"tool_calls": [{
                "index": 0,
                "id": task_id,
                "type": "function",
                "function": {"name": "task", "arguments": r#"{"prompt":"hang until run stopped"}"#}
            }]},
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });
    // 连接顺序:① 主轮派发 task;② 子代理模型请求 accept 后通知测试体并挂起,
    // 直到 halt 让 run 收尾、future 被 drop、连接由 client 关闭。没有第三条连接:
    // halted 的 run 不许再发请求。
    let (hang_tx, hang_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let _ = serve_response(&listener, dispatch).await;
        // 桩服务器绝不无限等待:请求数一旦对不上,要的是变红而不是挂死整个门禁。
        let (mut stream, _) = tokio::time::timeout(std::time::Duration::from_secs(20), listener.accept())
        .await
        .expect("等待模型请求超时。多半是生产侧的轮次数变了(例如权限被拒导致本轮提前收口),桩服务器还在等下一次请求——改测试对齐新契约,不要让 cargo test --workspace 静默挂死")
        .unwrap();
        let _ = hang_tx.send(());
        let mut chunk = [0_u8; 4096];
        let _ = stream.read(&mut chunk).await; // 挂起直到取消关闭连接
    });

    let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("test-key"));
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let subagent_rt = kanzei_core::SubagentRuntime {
        snapshot: sub_snapshot,
        agent: kanzei_tools::explore_agent(),
        fast: (route.clone(), "mock".to_string()),
        primary: (route.clone(), "mock".to_string()),
        fast_service_tier: None,
        primary_service_tier: None,
        compact: None,
        max_tokens: 256,
        timeout_secs: 30,
        limits: config.limits.clone(),
        coordinator: None,
        writable: false,
        ask_router: None,
        change_log: None,
        cancellations: None,
        background: false,
        background_results: None,
        background_events: None,
        transcripts: None,
        background_notifications: None,
    };
    let halt = kanzei_core::CancellationToken::new();
    let runner_config = runner_config(&config, halt.clone());
    let ctx = ToolCtx::new(project.clone(), project.clone());
    let tool_ends: Arc<Mutex<Vec<(String, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    let ends = tool_ends.clone();
    let mut on_event = move |event: kanzei_core::RunEvent| {
        if let kanzei_core::RunEvent::ToolEnd { id, ok, .. } = event {
            ends.lock().unwrap().push((id, ok));
        }
    };
    let mut ask = |_request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Cancelled })
    };

    tokio::pin!(hang_rx);
    let agent = agent();
    let mut run_fut = Box::pin(kanzei_core::run_once_with_parts(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        "派一个会挂住的勘察任务",
        None,
        &[],
        None,
        Some(&subagent_rt),
        &mut on_event,
        &mut ask,
    ));
    let mut stopped = false;
    let summary = loop {
        tokio::select! {
            result = &mut run_fut => break result.expect("halted 是正常返回"),
            _ = &mut hang_rx, if !stopped => {
                // 子代理已挂起(它的模型请求已被 accept)——此刻置位停止令牌,
                // 模拟用户在工具执行中点「停止」。
                stopped = true;
                halt.cancel();
            }
        }
    };
    server.await.unwrap();

    assert!(summary.halted_by_user, "必须以 halted 收尾");
    // 被打断轮的调用有配对占位,filter 后逐字节不变 = 无孤儿。
    let filtered = kanzei_core::filter_message_history(&summary.messages);
    assert_eq!(
        serde_json::to_string(&filtered).unwrap(),
        serde_json::to_string(&summary.messages).unwrap(),
        "halted 历史不许有孤儿 ToolCall/ToolResult"
    );
    let results: Vec<String> = summary
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .filter_map(|p| match p {
            kanzei_llm::Part::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect();
    assert!(
        results.iter().any(|c| c.contains("cancelled")),
        "被打断的调用要以取消占位收尾,实际: {results:?}"
    );
    std::fs::remove_dir_all(&project).ok();
}
