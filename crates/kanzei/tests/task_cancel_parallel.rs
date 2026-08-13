//! R-174 验收④:单条停止——运行中的子代理被 `stop_task` 取消后以「被停」终态收尾,
//! 读槽被释放,主对话整轮不受影响(不是 stop_run 那种整轮中止)。
//!
//! 场景编排:mock SSE 服务器第一条连接回主轮的 task 派发,第二条连接(子代理的
//! 模型请求)**挂起不回应**——子代理因此一直持着读槽「运行中」。服务器 accept 到
//! 子代理连接后经 oneshot 通知主测试体,此时才调用注册表 cancel(task_id),时序确定:
//!   ① run_subagent 的取消分支触发:TaskProgress 抛 phase="cancelled" trace;
//!   ② ToolEnd 以「被停」终态(ok=false + "was stopped by the user")收尾;
//!   ③ 协调器读槽 agent_completed 出现(RAII 释放,不是悬空);
//!   ④ 主轮继续完成(整轮未被中止)。

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

/// 记录编排事件与运行事件(含 TaskProgress trace)的观察者。
#[derive(Default)]
struct Recorder {
    orchestration: Mutex<Vec<(String, String)>>,
    run: Mutex<Vec<(String, String, bool, String)>>, // (id, name, ok, preview)
    usage_traces: Mutex<Vec<(String, u64)>>,         // (task_id, total_input_tokens)
    cancelled_traces: Mutex<Vec<String>>,            // phase == "cancelled" 的 id
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
async fn 运行中的task被单条停止_以被停终态收尾_读槽释放_主轮不受影响() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project =
        std::env::temp_dir().join(format!("kz-r174-cancel-{}-{suffix}", std::process::id()));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    let task_id = "call_task_hang".to_string();
    let tool_calls = vec![json!({
        "index": 0,
        "id": task_id,
        "type": "function",
        "function": {
            "name": "task",
            "arguments": r#"{"prompt":"hang until cancelled"}"#
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

    // 连接顺序:① 主轮派发;② 子代理模型请求——accept 后通知主测试体「已挂起」,
    // 然后挂起不回应,直到 run_subagent future 被取消(drop)连接由 client 侧关闭;
    // ③ 主轮收尾文本。
    let (hang_tx, hang_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let first = serve_response(&listener, dispatch).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = hang_tx.send(());
        let mut chunk = [0_u8; 4096];
        let _ = stream.read(&mut chunk).await; // 挂起直到连接被取消关闭
        drop(stream);
        let _ = serve_response(&listener, text_response("done")).await;
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
    let cancellations = Arc::new(kanzei_core::TaskCancellations::default());
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
        writable: false,
        ask_router: None,
        change_log: None,
        cancellations: Some(cancellations.clone()),
        background: false,
        background_results: None,
        background_events: None,
        transcripts: None,
        background_notifications: None,
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
        halt: None,
    };
    let ctx = ToolCtx::new(project.clone(), project.clone());

    let event_recorder = recorder.clone();
    let mut on_event = move |event: kanzei_core::RunEvent| match event {
        kanzei_core::RunEvent::ToolEnd {
            id,
            name,
            ok,
            preview,
            ..
        } => {
            event_recorder
                .run
                .lock()
                .unwrap()
                .push((id, name, ok, preview));
        }
        kanzei_core::RunEvent::TaskProgress {
            id,
            trace: Some(trace),
            ..
        } => match trace.phase.as_str() {
            "usage" => {
                if let Some(usage) = trace.usage {
                    event_recorder
                        .usage_traces
                        .lock()
                        .unwrap()
                        .push((id.clone(), usage.input));
                }
            }
            "cancelled" => {
                event_recorder
                    .cancelled_traces
                    .lock()
                    .unwrap()
                    .push(id.clone());
            }
            _ => {}
        },
        _ => {}
    };
    let mut ask = |_request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
    };

    // run_once_with_parts 与「子代理已挂起」信号同轮等待:先收到信号就 cancel,
    // 再等 run 收尾。run future 必须 pin 后用 &mut——tokio::select! 每次 poll 会
    // 重建分支 future,直接写 run_once_with_parts(...) 会让主轮每轮迭代都从头开始。
    tokio::pin!(hang_rx);
    let mut run_fut = Box::pin(kanzei_core::run_once_with_parts(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        "跑一个会挂住的勘察任务",
        None,
        &[],
        None,
        Some(&subagent_rt),
        &mut on_event,
        &mut ask,
    ));
    let mut hung = false;
    let summary;
    loop {
        tokio::select! {
            result = &mut run_fut => {
                summary = result.expect("运行应当成功");
                break;
            }
            _ = &mut hang_rx, if !hung => {
                // 子代理已挂起(accept 到它的模型请求)——此刻它持着读槽,是「运行中」。
                hung = true;
                let hit = cancellations.cancel(&task_id);
                assert!(hit, "运行中的 task 必须能命中注册表");
            }
        }
    }
    server.await.unwrap();

    // ① 取消分支的 phase="cancelled" trace 出现过。
    let cancelled = recorder.cancelled_traces.lock().unwrap().clone();
    assert!(
        cancelled.contains(&task_id),
        "取消后必须抛 phase=cancelled 的 TaskProgress,实际: {cancelled:?}"
    );
    // ② ToolEnd 以「被停」终态收尾:ok=false + 被停文案。
    let run_events = recorder.run.lock().unwrap().clone();
    let task_end: Vec<&(String, String, bool, String)> = run_events
        .iter()
        .filter(|(id, name, _, _)| name == "task" && *id == task_id)
        .collect();
    assert_eq!(
        task_end.len(),
        1,
        "task 恰好一个 ToolEnd,实际: {run_events:?}"
    );
    let (_, _, ok, preview) = task_end[0];
    assert!(!ok, "被停的 task 必须是失败终态(ok=false)");
    assert!(
        preview.contains("was stopped by the user"),
        "被停终态的 preview 必须带 run_subagent 取消分支文案,实际: {preview}"
    );
    // ③ 读槽被释放:agent_completed 出现(RAII 在 future drop 时回收)。
    let orch = recorder.orchestration.lock().unwrap().clone();
    let completed: Vec<&str> = orch
        .iter()
        .filter(|(t, id)| t == "orchestration.agent_completed" && id.as_str() == task_id)
        .map(|(_, id)| id.as_str())
        .collect();
    assert_eq!(
        completed.len(),
        1,
        "取消后读槽必须回收,实际编排事件: {orch:?}"
    );
    // ④ 主轮正常收尾,未被整轮中止。
    assert!(!summary.text.is_empty(), "主轮应能继续完成");
    assert!(
        recorder.usage_traces.lock().unwrap().is_empty(),
        "挂起的子代理没有完成任何一轮,不应有 usage trace"
    );

    std::fs::remove_dir_all(&project).ok();
}
