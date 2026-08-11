//! R-173 批4.5:writer 策略下「并行查」端到端可达。
//!
//! R-171 把 task 注册挂在 `!execution_policy.is_serial_writer()` 上,而桌面端主对话
//! 无条件设 `ReadParallelWriteSerial` —— 结果是主对话**根本不注册 task 工具**,
//! 「并行查」被整个关掉,`runner/subagent.rs` 里的读槽登记成了不可达代码。
//!
//! 本测试证明这条链现在真的通了,而不是"task 出现在 spec 里"就算数:
//!   ① 串行写策略下,发给模型的请求里确实带 task 工具;
//!   ② 模型同轮派发两个 task → 协调器里真的出现两个读槽;
//!   ③ 两个读槽**区间重叠**(两条 started 都早于任何一条 completed);
//!   ④ 子代理结束后读槽被回收,快照清空。

use std::sync::{Arc, Mutex};

use kanzei_harness::orchestration::{
    ExecutionPolicy, OrchestrationEvent, PhaseObserver, ProjectExecutionCoordinator,
};
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 收一个请求、回一条 SSE 响应,返回原始请求字节(用于断言发出去的工具清单)。
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

/// 记录编排事件的观察者。
#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<(String, String)>>,
}

impl PhaseObserver for Recorder {
    fn observe(&self, event: &OrchestrationEvent) {
        let payload = event.payload();
        self.events.lock().unwrap().push((
            event.event_type().to_string(),
            payload["run_id"].as_str().unwrap_or_default().to_string(),
        ));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 串行写策略下并行勘察真实可达_读槽被消费且重叠() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project =
        std::env::temp_dir().join(format!("kz-r173-scout-{}-{suffix}", std::process::id()));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    // 主轮:同轮派发两个 task(设计文档「推荐勘察角色」里的前两个)。
    let dispatch = json!({
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [
                    {
                        "index": 0,
                        "id": "call_scout_arch",
                        "type": "function",
                        "function": {
                            "name": "task",
                            "arguments": "{\"prompt\":\"architecture scout\"}"
                        }
                    },
                    {
                        "index": 1,
                        "id": "call_scout_runtime",
                        "type": "function",
                        "function": {
                            "name": "task",
                            "arguments": "{\"prompt\":\"runtime scout\"}"
                        }
                    }
                ]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    });

    // 连接顺序:主轮 → 两个子代理各自一轮 → 主轮收尾。
    let server = tokio::spawn(async move {
        let first = serve_response(&listener, dispatch).await;
        serve_response(&listener, text_response("architecture findings")).await;
        serve_response(&listener, text_response("runtime findings")).await;
        serve_response(&listener, text_response("done")).await;
        first
    });

    let config = Arc::new(KanzeiConfig::load(&project).unwrap_or_else(|_| KanzeiConfig::default()));
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: project.clone(),
        project_root: project.clone(),
        config: config.clone(),
    };
    // 主快照刻意留空:这样发出去的工具清单里出现 `task` 就只可能来自
    // drive 的注册分支,断言没有歧义。
    let snapshot = Harness::default().resolve(&rctx).unwrap();
    // 子代理快照走真实的 SubagentBase —— 只读白名单是**构造层面**的事实,
    // 这也是 writer 阶段允许跑 task 的安全依据。
    let mut sub_harness = Harness::default();
    sub_harness.add(kanzei_tools::SubagentBase);
    let sub_snapshot = sub_harness.resolve(&rctx).unwrap();
    let mut sub_tools: Vec<&str> = sub_snapshot
        .materialize_tools()
        .iter()
        .map(|t| t.name())
        .collect();
    sub_tools.sort_unstable();
    assert_eq!(
        sub_tools,
        vec!["glob", "grep", "read"],
        "子代理快照必须只有只读工具——这是 writer 阶段跑 task 安全的前提"
    );

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
        max_tokens: 256,
        timeout_secs: 30,
        limits: config.limits.clone(),
        coordinator: Some(coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>),
        cancellations: None,
    };
    let runner_config = kanzei_core::RunnerConfig {
        model: "mock".into(),
        max_tokens: 256,
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: None,
        context_limit: None,
        limits: config.limits.clone(),
        recall: None,
        // 关键:主对话就是 writer 阶段的策略。R-171 下这一行会让 task 消失。
        execution_policy: ExecutionPolicy::ReadParallelWriteSerial,
    };
    let ctx = ToolCtx::new(project.clone(), project.clone());
    let mut on_event = |_event: kanzei_core::RunEvent| {};
    let mut ask = |_request: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
    };

    // 验收④(读写共存):照 run.rs 的样子,整轮**真实持有**项目写租约。
    // 租约在 run_once_with_parts 全程不释放,所以下面出现的任何读槽事件,
    // 按定义都发生在 writer 活跃期间。
    let write_lease = coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            write_scope: project.clone(),
            run_id: "run_writer".into(),
            process_id: "proc_writer".into(),
            reason: "main conversation writer run".into(),
        })
        .await
        .expect("主对话应当拿到写租约");
    assert_eq!(
        coordinator.snapshot(&project).writer_run_id.as_deref(),
        Some("run_writer")
    );

    let summary = kanzei_core::run_once_with_parts(
        &client,
        &route,
        &snapshot,
        &agent,
        &runner_config,
        &ctx,
        "勘察这个项目",
        &[],
        None,
        Some(&subagent_rt),
        &mut on_event,
        &mut ask,
    )
    .await
    .expect("运行应当成功");
    let first_request = server.await.unwrap();

    // ① 串行写策略下,发出去的请求里确实带 task 工具。
    // 按 HTTP 头尾切,不要靠在整段里找 `{"model"` —— 那个串在嵌套的工具 schema
    // 里也会出现,切出来的是半个对象。
    let header_end = first_request
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("请求应有完整 HTTP 头")
        + 4;
    let body = String::from_utf8_lossy(&first_request[header_end..]);
    let request: serde_json::Value =
        serde_json::from_str(body.trim()).expect("请求体应是完整 JSON");
    let tool_names: Vec<&str> = request["tools"]
        .as_array()
        .expect("请求必须带 tools")
        .iter()
        .filter_map(|t| t["function"]["name"].as_str())
        .collect();
    assert!(
        tool_names.contains(&"task"),
        "ReadParallelWriteSerial 下 task 必须仍然注册给模型,实际工具清单: {tool_names:?}"
    );

    // ② + ③ 两个读槽真实登记,且区间重叠。
    let events = recorder.events.lock().unwrap().clone();
    let started: Vec<&String> = events
        .iter()
        .filter(|(t, _)| t == "orchestration.agent_started")
        .map(|(_, run_id)| run_id)
        .collect();
    let completed: Vec<&String> = events
        .iter()
        .filter(|(t, _)| t == "orchestration.agent_completed")
        .map(|(_, run_id)| run_id)
        .collect();
    assert_eq!(
        started.len(),
        2,
        "两个 task 必须各自登记一个读槽,实际事件: {events:?}"
    );
    assert_eq!(completed.len(), 2, "两个读槽都必须被回收");
    // 身份按 run_id(= 父 tool call id)区分:并行子代理的 agent_name 全都一样,
    // 只有 run_id 能分辨谁是谁(批4.5 修的那个回收键)。
    let mut started_ids: Vec<&str> = started.iter().map(|s| s.as_str()).collect();
    let mut completed_ids: Vec<&str> = completed.iter().map(|s| s.as_str()).collect();
    started_ids.sort_unstable();
    completed_ids.sort_unstable();
    assert_eq!(started_ids, vec!["call_scout_arch", "call_scout_runtime"]);
    assert_eq!(
        completed_ids, started_ids,
        "回收的必须正是登记过的那两个身份,不能张冠李戴"
    );

    let first_completed = events
        .iter()
        .position(|(t, _)| t == "orchestration.agent_completed")
        .unwrap();
    let last_started = events
        .iter()
        .enumerate()
        .filter(|(_, (t, _))| t == "orchestration.agent_started")
        .map(|(i, _)| i)
        .next_back()
        .unwrap();
    assert!(
        last_started < first_completed,
        "两个读槽必须区间重叠(两条 started 都早于任何一条 completed),实际: {events:?}"
    );

    // ④ 读写共存 + 读槽回收干净。
    let snapshot_after = coordinator.snapshot(&project);
    assert_eq!(
        snapshot_after.writer_run_id.as_deref(),
        Some("run_writer"),
        "写租约全程未释放——上面那两个读槽因此确实是在 writer 活跃期间登记并回收的\
         (验收④ 读写共存)"
    );
    assert!(
        snapshot_after.active_readers.is_empty(),
        "子代理结束后读槽必须清空,实际: {:?}",
        snapshot_after.active_readers
    );
    // 只读勘察全程没有去抢写租约:持有者自始至终是主对话那一个。
    assert!(
        !events
            .iter()
            .any(|(t, _)| t.starts_with("orchestration.writer.")
                && t != "orchestration.writer.acquired"),
        "只读勘察不应产生任何写租约排队/释放事件,实际: {events:?}"
    );

    assert!(summary.text.contains("done"));
    drop(write_lease);
    assert!(coordinator.snapshot(&project).writer_run_id.is_none());
    std::fs::remove_dir_all(&project).ok();
}
