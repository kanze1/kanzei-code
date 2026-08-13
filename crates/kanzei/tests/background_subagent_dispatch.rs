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
    // R-175 B2 验收⑥:事件可回放——sink 收集后台子代理生命周期事件。
    let background_events: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
    // R-175 B4 验收⑦:通知收集(call_id, status)。闭包 move 一份,断言用另一份。
    let notifications_for_sink: Arc<Mutex<Vec<(String, String)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let notifications_for_sink_closed = notifications_for_sink.clone();
    let events_for_sink = background_events.clone();
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
        background_events: Some(Arc::new(
            move |_call_id: &str, payload: serde_json::Value| {
                events_for_sink.lock().unwrap().push(payload);
            },
        )),
        transcripts: None,
        // R-175 B4 验收⑦:通知走既有 agent_notifications 表——测试用 sink 收集
        // (call_id, status),断言后台子代理完成时收到 done(未新造并行通道)。
        background_notifications: Some(Arc::new(move |call_id: &str, status: &str| {
            notifications_for_sink_closed
                .lock()
                .unwrap()
                .push((call_id.to_string(), status.to_string()));
        })),
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

    // R-175 B2 验收⑥:生命周期事件可回放——sink 收到 task.lifecycle(done 终态)。
    let deadline_events = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let lifecycle = {
            let events = background_events.lock().unwrap();
            events
                .iter()
                .find(|e| {
                    e.get("kind").and_then(|k| k.as_str()) == Some("task.lifecycle")
                        && e.get("id").and_then(|i| i.as_str()) == Some(&task_id)
                })
                .cloned()
        };
        if let Some(lifecycle) = lifecycle {
            assert_eq!(
                lifecycle.get("state").and_then(|s| s.as_str()),
                Some("done"),
                "后台子代理完成应记 done 终态: {lifecycle:?}"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline_events,
            "5 秒内应收到 task.lifecycle 事件"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // R-175 B4 验收⑦:通知走既有 agent_notifications 表——完成时收到 (id, done)。
    let deadline_notify = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let notified = {
            let all = notifications_for_sink.lock().unwrap();
            all.iter()
                .any(|(id, status)| id == &task_id && status == "done")
        };
        if notified {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline_notify,
            "5 秒内应收到完成通知"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    std::fs::remove_dir_all(&project).ok();
}

/// R-175 B3 验收④:transcript 持久化 + 按 id 恢复续跑——同一 id 第二次调用
/// run_subagent 时,prior 应包含此前完整历史(不是从空历史重开)。
///
/// 场景:mock 服务器回两条响应(第一轮子代理的文本回复 + 续跑轮的子代理文本回复)。
/// 第一次 run_subagent(id=X, prompt "first task")完成后 transcripts[X] 落库;
/// 第二次 run_subagent(id=X, prompt "continue task")从 transcripts[X] 恢复 prior。
/// 断言的证据:
///   ① transcripts[X] 第一次调用后有历史(非空);
///   ② 第二次调用后 transcripts[X] 比第一次更长(续跑把新轮历史追加进同一 id);
///   ③ 第一次调用的回复文本出现在续跑后的 transcript 里(此前历史确实被带上)。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 同一id续跑_prior恢复此前transcript_不重开空历史() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project = std::env::temp_dir().join(format!("kz-r175-b3-{}-{suffix}", std::process::id()));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    let first_reply = "first task reply text";
    let second_reply = "continue task reply text";
    let _server = tokio::spawn(async move {
        let _ = serve_response(&listener, text_response(first_reply)).await;
        let _ = serve_response(&listener, text_response(second_reply)).await;
    });

    let config = Arc::new(KanzeiConfig::load(&project).expect("读取 kanzei.toml 应成功"));
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: project.clone(),
        project_root: project.clone(),
        config: config.clone(),
    };
    let mut sub_harness = Harness::default();
    sub_harness.add(kanzei_tools::SubagentBase);
    let sub_snapshot = sub_harness.resolve(&rctx).unwrap();

    let coordinator = Arc::new(kanzei_core::orchestration::MemoryCoordinator::default());
    let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("test-key"));
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let transcripts: Arc<Mutex<std::collections::HashMap<String, Vec<kanzei_llm::Message>>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
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
        background: false,
        background_results: None,
        background_events: None,
        transcripts: Some(transcripts.clone()),
        background_notifications: None,
    };
    let ctx = ToolCtx::new(project.clone(), project.clone());
    let id = "call_task_b3".to_string();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<kanzei_core::RunEvent>();

    // 第一次派发(run_read_agent 是 pub 入口,内部走 run_subagent,同一 agent_id
    // 续跑时 transcripts 恢复 prior——与 task 派发路径共用同一实现)。
    let first =
        kanzei_core::run_read_agent(&client, &subagent_rt, &ctx, &id, "first task", tx.clone())
            .await;
    assert!(!first.is_error, "第一次派发应成功: {}", first.content);
    assert!(first.content.contains(first_reply), "第一次回复应可见");

    // ① transcripts[id] 第一次调用后有历史(非空)。
    let first_len = transcripts.lock().unwrap().get(&id).map(|m| m.len());
    let first_len = first_len.expect("第一次派发后 transcripts[id] 应有历史");
    assert!(first_len > 0, "transcript 不应为空");

    // 第二次续跑(同 id)。
    let second = kanzei_core::run_read_agent(
        &client,
        &subagent_rt,
        &ctx,
        &id,
        "continue task",
        tx.clone(),
    )
    .await;
    assert!(!second.is_error, "续跑应成功: {}", second.content);
    assert!(second.content.contains(second_reply), "续跑回复应可见");

    // ② 续跑后 transcript 更长(新轮历史追加进同一 id,不是覆盖成空)。
    let second_len = transcripts
        .lock()
        .unwrap()
        .get(&id)
        .map(|m| m.len())
        .expect("续跑后 transcripts[id] 应有历史");
    assert!(
        second_len > first_len,
        "续跑应把新轮历史追加进同一 id: first={first_len}, second={second_len}"
    );

    // ③ 第一次回复文本出现在续跑后的 transcript 里(此前历史被带上,非空重开)。
    let all_text: String = transcripts
        .lock()
        .unwrap()
        .get(&id)
        .unwrap()
        .iter()
        .flat_map(|m| &m.parts)
        .filter_map(|p| match p {
            kanzei_llm::Part::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        all_text.contains(first_reply) && all_text.contains(second_reply),
        "续跑 transcript 应含两轮回复: {all_text}"
    );

    drop(rx);
    let _ = _server.await;
    std::fs::remove_dir_all(&project).ok();
}

/// R-175 B4 验收⑤:三种终态(失败/被停)都有确定归宿且读槽被释放——协调器快照
/// 在终态后不再残留该子代理的读者身份。超时路径由 drive.rs 后台分支的 timeout
/// 兜底(见 background_subagent_dispatch 的 drive 集成测试),这里覆盖 run_subagent
/// 直接路径的失败与被停两条;超时在 drive.rs 层(唯一包 timeout 的地方)。
///
/// 场景编排:
///   - 失败:mock SSE 回 HTTP 500 → run_once 报错 → run_subagent 返回 error,
///     `_read_permit` RAII 随函数返回释放;
///   - 被停:cancellations 注册表 cancel(id) → select! 的 cancelled 分支返回 error,
///     `_read_permit` 随函数返回释放。
/// 两条路径断言:协调器 snapshot().active_readers 不再包含该子代理的 run_id。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 失败与被停终态_读槽均释放_快照无残留读者() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project = std::env::temp_dir().join(format!("kz-r175-b4-{}-{suffix}", std::process::id()));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let config = Arc::new(KanzeiConfig::load(&project).expect("读取 kanzei.toml 应成功"));
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: project.clone(),
        project_root: project.clone(),
        config: config.clone(),
    };
    let mut sub_harness = Harness::default();
    sub_harness.add(kanzei_tools::SubagentBase);
    let sub_snapshot = sub_harness.resolve(&rctx).unwrap();

    let recorder = Arc::new(Recorder::default());
    let coordinator = Arc::new(
        kanzei_core::orchestration::MemoryCoordinator::with_observer(
            recorder.clone() as Arc<dyn PhaseObserver>
        ),
    );
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let ctx = ToolCtx::new(project.clone(), project.clone());
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<kanzei_core::RunEvent>();

    // ---- 失败路径:mock SSE 回 HTTP 500,run_once 报错 ----
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut chunk = [0_u8; 4096];
        let _ = stream.read(&mut chunk).await;
        let body = "internal error";
        let head = format!(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes()).await;
        let _ = stream.write_all(body.as_bytes()).await;
    });
    let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("test-key"));
    let subagent_rt = kanzei_core::SubagentRuntime {
        snapshot: sub_snapshot.clone(),
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
        background: false,
        background_results: None,
        background_events: None,
        transcripts: None,
        background_notifications: None,
    };
    let fail_id = "call_task_fail".to_string();
    let failed = kanzei_core::run_read_agent(
        &client,
        &subagent_rt,
        &ctx,
        &fail_id,
        "will fail",
        tx.clone(),
    )
    .await;
    assert!(
        failed.is_error,
        "mock 500 应使子代理失败: {}",
        failed.content
    );
    let _ = server.await;
    // 失败终态:快照不再残留该读者的身份。
    let snap = coordinator.snapshot(&project);
    assert!(
        !snap.active_readers.iter().any(|r| r == &fail_id),
        "失败终态后读槽应释放,active_readers 不得含 {fail_id}: {:?}",
        snap.active_readers
    );

    // ---- 被停路径:cancellations 注册表 cancel(id) ----
    let listener2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address2 = listener2.local_addr().unwrap();
    let (hang_tx, hang_rx) = tokio::sync::oneshot::channel::<()>();
    let server2 = tokio::spawn(async move {
        let (mut stream, _) = listener2.accept().await.unwrap();
        let _ = hang_tx.send(());
        let mut chunk = [0_u8; 4096];
        let _ = stream.read(&mut chunk).await; // 挂起直到连接被取消关闭
    });
    let route2 = kanzei_llm::Route::openai_at(&format!("http://{address2}/v1"), Some("test-key"));
    let cancellations = Arc::new(kanzei_core::TaskCancellations::default());
    let subagent_rt2 = kanzei_core::SubagentRuntime {
        snapshot: sub_snapshot,
        agent: kanzei_tools::explore_agent(),
        fast: (route2.clone(), "mock".to_string()),
        primary: (route2.clone(), "mock".to_string()),
        fast_service_tier: None,
        primary_service_tier: None,
        max_tokens: 256,
        timeout_secs: 30,
        limits: config.limits.clone(),
        coordinator: Some(coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>),
        cancellations: Some(cancellations.clone()),
        background: false,
        background_results: None,
        background_events: None,
        transcripts: None,
        background_notifications: None,
    };
    let stop_id = "call_task_stop".to_string();
    let stop_runner = tokio::spawn({
        let client = client.clone();
        let ctx = ctx.clone();
        let tx = tx.clone();
        let id = stop_id.clone();
        async move { kanzei_core::run_read_agent(&client, &subagent_rt2, &ctx, &id, "hang", tx).await }
    });
    let _ = hang_rx.await; // 子代理已挂起并持有读槽
    let cancelled = cancellations.cancel(&stop_id);
    assert!(cancelled, "stop_task 应命中运行中的子代理");
    let output = stop_runner.await.unwrap();
    assert!(
        output.is_error,
        "被停子代理应返回 error: {}",
        output.content
    );
    let _ = server2.await;
    // 被停终态:快照不再残留该读者的身份。
    let snap2 = coordinator.snapshot(&project);
    assert!(
        !snap2.active_readers.iter().any(|r| r == &stop_id),
        "被停终态后读槽应释放,active_readers 不得含 {stop_id}: {:?}",
        snap2.active_readers
    );

    std::fs::remove_dir_all(&project).ok();
}

/// R-175 B4 验收⑤第三条路径:超时——drive.rs 后台分支用 `tokio::time::timeout`
/// 兜底墙钟,超时后丢弃 run_subagent future,读槽随 future drop 的 RAII 释放。
/// 本测试直接验证同一语义:mock 服务器 accept 后**挂起不回应**,外部
/// tokio::time::timeout 把 run_read_agent future 丢弃,断言快照无残留读者。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 超时终态_读槽释放_快照无残留读者() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project =
        std::env::temp_dir().join(format!("kz-r175-b4-tmo-{}-{suffix}", std::process::id()));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let id = "call_task_timeout".to_string();

    // 服务器 accept 后挂起不回应(读一次请求后永久等待,直到客户端超时断开)。
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut chunk = [0_u8; 4096];
        let _ = stream.read(&mut chunk).await;
        let mut sink = [0_u8; 4096];
        let _ = stream.read(&mut sink).await; // 挂起:不写任何响应
        drop(stream);
    });

    let config = Arc::new(KanzeiConfig::load(&project).expect("读取 kanzei.toml 应成功"));
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: project.clone(),
        project_root: project.clone(),
        config: config.clone(),
    };
    let mut sub_harness = Harness::default();
    sub_harness.add(kanzei_tools::SubagentBase);
    let sub_snapshot = sub_harness.resolve(&rctx).unwrap();
    let coordinator = Arc::new(kanzei_core::orchestration::MemoryCoordinator::default());
    let route = kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("test-key"));
    let client = kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap();
    let ctx = ToolCtx::new(project.clone(), project.clone());
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
        background: false,
        background_results: None,
        background_events: None,
        transcripts: None,
        background_notifications: None,
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<kanzei_core::RunEvent>();

    // 子代理持有读槽(挂起中);外部 1 秒 timeout 丢弃 future——与 drive.rs 的
    // `tokio::time::timeout(bound, run_subagent(...))` 是同一丢弃语义。
    let timed_out = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        kanzei_core::run_read_agent(
            &client,
            &subagent_rt,
            &ctx,
            &id,
            "hang until timeout",
            tx.clone(),
        ),
    )
    .await;
    assert!(
        timed_out.is_err(),
        "挂起的子代理应在 1 秒后超时(future 被丢弃)"
    );
    let _ = server.await;
    // 超时终态:读槽随 future drop 释放,快照不再含该子代理的读者身份。
    let snap = coordinator.snapshot(&project);
    assert!(
        !snap.active_readers.iter().any(|r| r == &id),
        "超时终态后读槽应释放,active_readers 不得含 {id}: {:?}",
        snap.active_readers
    );
    drop(rx);
    std::fs::remove_dir_all(&project).ok();
}
