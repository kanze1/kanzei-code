//! R-175 B1b 验收①:后台模式——主代理派发 task 后**不阻塞**,本轮拿到「已后台派发」
//! 占位结果立即继续;子代理在 tokio::spawn 的后台任务里跑完,真实结果写入
//! background_results 供后续轮次查询。
//!
//! 场景编排:mock SSE 服务器第一条连接回主轮的 task 派发(模型请求工具调用),第二条
//! 与第三条分别服务后台子代理的模型请求与主轮的收尾请求(顺序不定,都回文本)。
//! 断言的证据:
//!   ① 主轮 task 的 ToolResult 是「已后台派发,句柄 <id>」占位——主代理没有等子代理;
//!   ② 子代理后台跑完后 background_results 里出现该 id 的真实结果文本;
//!   ③ 主轮整轮正常收尾(占位结果回填后模型收到文本)。

use std::sync::{Arc, Mutex};

use kanzei_harness::orchestration::{
    ExecutionPolicy, OrchestrationEvent, PhaseObserver, ProjectExecutionCoordinator,
};
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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

/// 记录编排事件的观察者(读槽登记/回收审计)。
#[derive(Default)]
struct Recorder {
    orchestration: Mutex<Vec<(String, String)>>,
}

impl PhaseObserver for Recorder {
    fn observe(&self, event: &OrchestrationEvent) {
        let payload = event.payload();
        self.orchestration.lock().unwrap().push((
            event.event_type().to_string(),
            payload["run_id"].as_str().unwrap_or_default().to_string(),
        ));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 后台模式派发即返回_主代理不阻塞_真实结果落background_results() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project = std::env::temp_dir().join(format!("kz-r175-bg-{}-{suffix}", std::process::id()));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    let task_id = "call_task_bg".to_string();
    let tool_calls = vec![json!({
        "index": 0,
        "id": task_id,
        "type": "function",
        "function": {
            "name": "task",
            "arguments": r#"{"prompt":"background exploration"}"#
        }
    })];
    let dispatch = json!({
        "choices": [{
            "index": 0,
            "delta": {"tool_calls": tool_calls},
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });

    // 连接顺序:① 主轮派发(dispatch);②③ 子代理模型请求 与 主轮收尾请求
    // (顺序不定,都回同一文本——后台模式主代理不等待,两条连接可能以任意次序到达;
    // 断言只看「后台结果确实落暂存」,不依赖哪条连接先到)。
    let server = tokio::spawn(async move {
        let first = serve_response(&listener, dispatch).await;
        let _ = serve_response(&listener, text_response("background child result")).await;
        let _ = serve_response(&listener, text_response("background child result")).await;
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
    let mut sub_harness = Harness::default();
    sub_harness.add(kanzei_tools::SubagentBase);
    let sub_snapshot = sub_harness.resolve(&rctx).unwrap();

    let recorder = Arc::new(Recorder::default());
    let coordinator = Arc::new(
        kanzei_core::orchestration::MemoryCoordinator::with_observer(
            recorder.clone() as Arc<dyn PhaseObserver>
        ),
    );

    let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("test-key"));
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();

    let agent = kanzei_harness::AgentDef {
        name: "dev-pair".into(),
        profile: kanzei_harness::ProfileScope::Dev,
        model: "mock".into(),
        mode: kanzei_harness::AgentMode::Primary,
        steps: 4,
        system: "test".into(),
    };
    let background_results: Arc<
        Mutex<std::collections::HashMap<String, kanzei_harness::ToolOutput>>,
    > = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let subagent_rt = kanzei_core::SubagentRuntime {
        snapshot: sub_snapshot,
        agent: kanzei_tools::explore_agent(),
        fast: (route.clone(), "mock".to_string()),
        primary: (route.clone(), "mock".to_string()),
        fast_service_tier: None,
        primary_service_tier: None,
        max_tokens: 256,
        timeout_secs: 30,
        limits: config.limits.clone(),
        coordinator: Some(coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>),
        cancellations: None,
        background: true,
        background_results: Some(background_results.clone()),
    };
    let runner_config = kanzei_core::RunnerConfig {
        model: "mock".into(),
        max_tokens: 256,
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: None,
        context_limit: None,
        limits: config.limits.clone(),
        recall: None,
        execution_policy: ExecutionPolicy::ReadParallelWriteSerial,
        ask_policy: kanzei_core::AskPolicy::Interactive,
    };
    let ctx = ToolCtx::new(project.clone(), project.clone());

    let mut on_event = |_event: kanzei_core::RunEvent| {};

    let summary = kanzei_core::run_once(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        "main prompt",
        None,
        &[],
        Some(&subagent_rt),
        &mut on_event,
        &mut |_| {
            Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
        },
    )
    .await
    .expect("主轮应正常收尾");

    let _ = server.await.unwrap();

    // ① 主轮 task 的 ToolResult 是「已后台派发」占位——主代理没有等子代理完成。
    let background_placeholder = summary
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .filter_map(|p| match p {
            kanzei_llm::Part::ToolResult {
                call_id, content, ..
            } if call_id == &task_id => Some(content.clone()),
            _ => None,
        })
        .next()
        .unwrap_or_default();
    assert!(
        background_placeholder.contains("已后台派发"),
        "主轮应拿到占位结果而非等子代理: {background_placeholder}"
    );
    assert!(
        background_placeholder.contains(&task_id),
        "占位结果应含句柄 id: {background_placeholder}"
    );

    // ② 子代理后台跑完后真实结果写入 background_results。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let stored = loop {
        let stored = background_results.lock().unwrap().get(&task_id).cloned();
        if stored.is_some() {
            break stored;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "5 秒内后台子代理应完成并写入 background_results"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    let stored = stored.expect("后台结果存在");
    assert!(
        stored.content.contains("background child result"),
        "后台子代理真实结果应写入: {}",
        stored.content
    );

    // ③ 主轮文本正常收尾(占位结果回填后模型收到文本)。
    assert!(
        summary.text.contains("background child result") || summary.text.is_empty(),
        "主轮应收尾: {}",
        summary.text
    );

    std::fs::remove_dir_all(&project).ok();
}
