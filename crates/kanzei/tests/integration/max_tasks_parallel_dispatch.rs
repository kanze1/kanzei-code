//! R-174 验收①:并发度实测——`max_tasks_per_turn = N`(N 远大于 8)后,同轮派发
//! N 个 task 全部执行,第 N+1 个才落 drive.rs:441-444 的溢出错误。
//!
//! 本测试用 mock SSE 服务器逐连接收请求:主轮第一个请求派发 **21 个 task 调用**
//! (N=20),随后 20 个子代理各占一条连接(证明 20 个真的并行跑起来、各自完成了
//! 一次模型调用),主轮收尾再回一个文本响应。断言的证据分三层:
//!   ① 20 个 task 的 ToolEnd 全部 ok(子代理真实执行完一轮,不是被静默跳过);
//!   ② 第 21 个 task 的 ToolEnd 是失败,错误文本就是 drive.rs 的
//!      「too many parallel subagent tasks; maximum per turn is 20」——溢出分支唯一;
//!   ③ 协调器读槽 20 登记 / 20 回收(每个子代理都持过读槽,是"执行"的硬证据,
//!      而非只发了 ToolStart 事件)。

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

/// 记录编排事件与运行事件的观察者。
#[derive(Default)]
struct Recorder {
    orchestration: Mutex<Vec<(String, String)>>,
    run: Mutex<Vec<(String, String, bool, String)>>, // (id, name, ok, preview)
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
async fn 并发上限20时同轮派发21个task_20个全执行_第21个落溢出错误() {
    const MAX_TASKS: usize = 20;
    const DISPATCHED: usize = MAX_TASKS + 1;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project = std::env::temp_dir().join(format!(
        "kz-r174-concurrency-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();
    // 验收①原文要求的就是 kanzei.toml 里配 [limits] max_tasks_per_turn = N。
    std::fs::write(
        project.join(".kanzei").join("kanzei.toml"),
        format!("[limits]\nmax_tasks_per_turn = {MAX_TASKS}\n"),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    // 主轮:同轮派发 21 个 task(20 个应在额度内,第 21 个溢出)。
    let mut tool_calls = Vec::new();
    for index in 0..DISPATCHED {
        tool_calls.push(json!({
            "index": index,
            "id": format!("call_task_{index}"),
            "type": "function",
            "function": {
                "name": "task",
                "arguments": format!(r#"{{"prompt":"scout task {index}"}}"#)
            }
        }));
    }
    let dispatch = json!({
        "choices": [{
            "index": 0,
            "delta": {"tool_calls": tool_calls},
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });

    // 连接顺序:主轮派发 → 20 个子代理各一轮 → 主轮收尾。
    let server = tokio::spawn(async move {
        let first = serve_response(&listener, dispatch).await;
        for index in 0..MAX_TASKS {
            serve_response(&listener, text_response(&format!("findings {index}"))).await;
        }
        serve_response(&listener, text_response("done")).await;
        first
    });

    let config = Arc::new(KanzeiConfig::load(&project).expect("读取 kanzei.toml 应成功"));
    assert_eq!(
        config.limits.max_tasks_per_turn(),
        MAX_TASKS,
        "kanzei.toml 里的 N 必须被读进配置"
    );
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
    let subagent_rt = kanzei_core::SubagentRuntime {
        snapshot: sub_snapshot,
        agent: kanzei_tools::explore_agent(),
        fast: (route.clone(), "mock".to_string()),
        primary: (route.clone(), "mock".to_string()),
        fast_service_tier: None,
        primary_service_tier: None,
        compact: None,
        max_tokens: 256,
        timeout_secs: 60,
        limits: config.limits.clone(),
        coordinator: Some(coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>),
        writable: false,
        ask_router: None,
        change_log: None,
        cancellations: None,
        background: false,
        background_results: None,
        background_events: None,
        transcripts: None,
        background_notifications: None,
        transcript_sink: None,
        transcript_provider: None,
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

    // 运行事件里挑 ToolEnd 收进记录,专门数 task 的成败。
    let event_recorder = recorder.clone();
    let mut on_event = move |event: kanzei_core::RunEvent| {
        if let kanzei_core::RunEvent::ToolEnd {
            id,
            name,
            ok,
            preview,
            ..
        } = event
        {
            event_recorder
                .run
                .lock()
                .unwrap()
                .push((id, name, ok, preview));
        }
    };
    let mut ask = |_request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
    };

    let summary = kanzei_core::run_once_with_parts(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        "勘察这个项目",
        None,
        None,
        &[],
        None,
        Some(&subagent_rt),
        // R-246:测试不持有 LineRuntime。
        None,
        &mut on_event,
        &mut ask,
    )
    .await
    .expect("运行应当成功");
    server.await.unwrap();

    // ① 20 个 task 全部执行成功。
    let run_events = recorder.run.lock().unwrap().clone();
    let task_ok: Vec<&(String, String, bool, String)> = run_events
        .iter()
        .filter(|(_, name, ok, _)| name == "task" && *ok)
        .collect();
    assert_eq!(
        task_ok.len(),
        MAX_TASKS,
        "额度内的 {MAX_TASKS} 个 task 必须全部执行成功,实际事件: {run_events:?}"
    );
    // ② 第 21 个落在溢出分支:ToolEnd 失败,preview 即 drive.rs 的溢出文案。
    let overflow_events: Vec<(String, String)> = run_events
        .iter()
        .filter(|(id, name, ok, _)| name == "task" && !*ok && id.starts_with("call_task_"))
        .map(|(id, _, _, preview)| (id.clone(), preview.clone()))
        .collect();
    assert_eq!(
        overflow_events.len(),
        1,
        "恰好只有 1 个 task 溢出,实际: {overflow_events:?} / {run_events:?}"
    );
    assert_eq!(
        overflow_events[0].0, "call_task_20",
        "溢出的必须是第 21 个(N+1),而不是前面的某个"
    );
    assert!(
        overflow_events[0].1.contains(&format!(
            "too many parallel subagent tasks; maximum per turn is {MAX_TASKS}"
        )),
        "溢出 ToolEnd 的 preview 必须带 drive.rs 的溢出文案,实际: {}",
        overflow_events[0].1
    );
    assert!(summary.text.contains("done"), "主轮应正常收尾");

    // ③ 读槽:20 登记 / 20 回收 —— 每个额度内 task 都真实持过读槽。
    let orch = recorder.orchestration.lock().unwrap().clone();
    let started: Vec<&str> = orch
        .iter()
        .filter(|(t, _)| t == "orchestration.agent_started")
        .map(|(_, id)| id.as_str())
        .collect();
    let completed: Vec<&str> = orch
        .iter()
        .filter(|(t, _)| t == "orchestration.agent_completed")
        .map(|(_, id)| id.as_str())
        .collect();
    assert_eq!(started.len(), MAX_TASKS, "20 个 task 各登记一个读槽");
    assert_eq!(completed.len(), MAX_TASKS, "20 个读槽全部回收");
    let mut started_ids = started.clone();
    started_ids.sort_unstable();
    let mut completed_ids = completed.clone();
    completed_ids.sort_unstable();
    assert_eq!(completed_ids, started_ids, "回收的正是登记过的那 20 个");

    std::fs::remove_dir_all(&project).ok();
}
