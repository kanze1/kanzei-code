//! R-173 批6:阶段流水线接线的行为锚点。
//!
//! 重点是**分段驱动不能弄坏历史**:修正段是第二次 `run_once`,它的 `prior` 接的是
//! 实现段跑完的完整 `messages`。轮末统计口径(`&summary.messages[prior.len()..]`)
//! 依赖这条,错了会让 token 计数、失败提炼、episode 画像一起歪掉。

use std::sync::{Arc, Mutex};

use kanzei_harness::orchestration::{PhaseObserver, ProjectExecutionCoordinator};
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use kanzei_llm::{Message, Part};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::phase_pipeline::PhasePipeline;

async fn serve_response(listener: &TcpListener, response: serde_json::Value) {
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
}

fn text_response(text: &str, input: u64, output: u64) -> serde_json::Value {
    json!({
        "choices": [{
            "index": 0,
            "delta": {"content": text},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": input, "completion_tokens": output}
    })
}

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<String>>,
}

impl PhaseObserver for Recorder {
    fn observe(&self, event: &kanzei_harness::orchestration::OrchestrationEvent) {
        let payload = event.payload();
        let label = match event.event_type() {
            "orchestration.phase_changed" => {
                format!("phase:{}", payload["phase"].as_str().unwrap_or_default())
            }
            other => other.to_string(),
        };
        self.events.lock().unwrap().push(label);
    }
}

struct Fixture {
    project: std::path::PathBuf,
    ctx: ToolCtx,
    config: Arc<KanzeiConfig>,
    snapshot: Arc<kanzei_harness::HarnessSnapshot>,
    sub_snapshot: Arc<kanzei_harness::HarnessSnapshot>,
    agent: kanzei_harness::AgentDef,
    route: kanzei_llm::Route,
    client: kanzei_llm::LlmClient,
    coordinator: Arc<kanzei_core::orchestration::MemoryCoordinator>,
    recorder: Arc<Recorder>,
}

fn fixture(tag: &str, address: std::net::SocketAddr) -> Fixture {
    let project = std::env::temp_dir().join(format!(
        "kz-r173-b6-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();
    let config = Arc::new(KanzeiConfig::default());
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
    Fixture {
        ctx: ToolCtx::new(project.clone(), project.clone()),
        project,
        config,
        snapshot,
        sub_snapshot,
        agent: kanzei_harness::AgentDef {
            name: "dev-pair".into(),
            profile: kanzei_harness::ProfileScope::Dev,
            model: "mock".into(),
            mode: kanzei_harness::AgentMode::Primary,
            steps: 4,
            system: "test".into(),
        },
        route: kanzei_llm::Route::openai_at(&format!("http://{address}/v1"), Some("k")),
        client: kanzei_llm::LlmClient::new(&kanzei_llm::ProxyConfig::Disabled).unwrap(),
        coordinator: Arc::new(kanzei_core::orchestration::MemoryCoordinator::new()),
        recorder: Arc::new(Recorder::default()),
    }
}

impl Fixture {
    fn subagent_rt(&self) -> kanzei_core::SubagentRuntime {
        kanzei_core::SubagentRuntime {
            snapshot: self.sub_snapshot.clone(),
            agent: kanzei_tools::explore_agent(),
            fast: (self.route.clone(), "mock".into()),
            primary: (self.route.clone(), "mock".into()),
            fast_service_tier: None,
            primary_service_tier: None,
            max_tokens: 256,
            timeout_secs: 30,
            limits: self.config.limits.clone(),
            coordinator: Some(self.coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>),
        }
    }

    fn runner_config(&self) -> kanzei_core::RunnerConfig {
        kanzei_core::RunnerConfig {
            model: "mock".into(),
            max_tokens: 256,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: None,
            context_limit: None,
            limits: self.config.limits.clone(),
            recall: None,
            execution_policy:
                kanzei_harness::orchestration::ExecutionPolicy::ReadParallelWriteSerial,
        }
    }

    fn pipeline(&self) -> PhasePipeline {
        PhasePipeline::start(
            self.coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>,
            self.recorder.clone() as Arc<dyn PhaseObserver>,
            self.project.clone(),
            "run_b6",
            "proc_b6",
            &self.config.limits,
            None, // 未配 [models] scout:沿用模板的 fast
        )
    }

    fn events(&self) -> Vec<String> {
        self.recorder.events.lock().unwrap().clone()
    }
}

/// 实现段跑完时的 summary(手工造,免得再起一段假 provider)。
fn implementation_summary() -> kanzei_core::RunSummary {
    kanzei_core::RunSummary {
        text: "实现段:改了 drive.rs".into(),
        usage: kanzei_llm::Usage {
            input: 100,
            output: 20,
            reasoning: 0,
            cache_read: 5,
            cache_write: 7,
        },
        steps: 3,
        halted_by_user: false,
        messages: vec![
            Message::user_text("原始用户消息"),
            Message::assistant(vec![Part::Text {
                text: "实现段:改了 drive.rs".into(),
            }]),
        ],
        context_report: vec![("agent/system".into(), 10)],
        overflow_traces: vec!["impl-overflow".into()],
    }
}

/// 编排派发的子代理必须把内部进度上抛,且走**既有事件形状**。
///
/// 这条测试就是前端(R-174 子代理面板)的契约:事件名、id、payload 字段改了它会红。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 编排派发的子代理上抛既有形状的进度事件() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        // 5 个勘察角色并发连上来,响应一致。
        for _ in 0..5 {
            serve_response(&listener, text_response("勘察结论:drive.rs:57", 1, 1)).await;
        }
    });

    let fx = fixture("progress", address);
    let rt = fx.subagent_rt();
    let mut pipeline = fx.pipeline();
    let seen: Arc<Mutex<Vec<kanzei_core::RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let mut on_event = move |event: kanzei_core::RunEvent| sink.lock().unwrap().push(event);

    let brief = pipeline
        .scout(&fx.client, &rt, &fx.ctx, "修 R-173", &mut on_event)
        .await
        .expect("勘察应当成功");
    server.await.unwrap();
    pipeline.abort("test done");

    assert!(brief.contains("勘察简报"));
    let events = seen.lock().unwrap();

    // ① ToolStart:每个角色一条,name 沿用 "task"(UI 已有的子代理块渲染直接可用)。
    let starts: Vec<(&String, &serde_json::Value)> = events
        .iter()
        .filter_map(|e| match e {
            kanzei_core::RunEvent::ToolStart {
                id, name, input, ..
            } if name == "task" => Some((id, input)),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 5, "5 个勘察角色各应有一条 ToolStart");
    let mut ids: Vec<&str> = starts.iter().map(|(id, _)| id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![
            "architecture_scout",
            "docs_scout",
            "runtime_scout",
            "test_scout",
            "write_surface_scout",
        ],
        "id 必须就是角色名——面板靠它配对 start/progress/end"
    );
    for (id, input) in &starts {
        assert_eq!(
            input["phase"], "scouting",
            "input.phase 是面板分区的依据,不能缺"
        );
        assert_eq!(input["role"].as_str(), Some(id.as_str()));
        assert!(
            input["prompt"]
                .as_str()
                .is_some_and(|p| p.contains("修 R-173")),
            "input.prompt 要能展开看到派给这个角色的完整指令(R-095)"
        );
    }

    // ② TaskProgress:子代理内部轮次/工具进度按 id 挂回对应角色。
    let progress: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            kanzei_core::RunEvent::TaskProgress { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert!(
        !progress.is_empty(),
        "子代理的内部进度必须上抛,不能像批6 那样被丢弃"
    );
    for id in &progress {
        assert!(
            ids.contains(&id.as_str()),
            "TaskProgress 的 id 必须能配到某个角色块: {id}"
        );
    }

    // ③ ToolEnd:每个角色一条,ok 反映终态。
    let ends: Vec<(&String, bool)> = events
        .iter()
        .filter_map(|e| match e {
            kanzei_core::RunEvent::ToolEnd { id, name, ok, .. } if name == "task" => {
                Some((id, *ok))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        ends.len(),
        5,
        "每个角色都要有终态事件,面板才能从 Running 移出"
    );
    assert!(ends.iter().all(|(_, ok)| *ok));
    // start 必须全部早于 end(面板先建块再收尾)。
    let first_end = events
        .iter()
        .position(|e| matches!(e, kanzei_core::RunEvent::ToolEnd { .. }))
        .unwrap();
    let last_start = events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, kanzei_core::RunEvent::ToolStart { .. }))
        .map(|(i, _)| i)
        .next_back()
        .unwrap();
    assert!(last_start < first_end, "5 个块必须先全部建起来再收尾");
    drop(events);
    std::fs::remove_dir_all(&fx.project).ok();
}

/// 复核有发现 → 跑修正段;历史必须接续,用量/步数必须合并。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 复核有发现时修正段接续历史且用量合并() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    // 3 个复核角色并发连上来,顺序不定 —— 所以三条响应必须一样,断言才确定。
    // 第 4 条是修正段那次主对话调用。
    let server = tokio::spawn(async move {
        for _ in 0..3 {
            serve_response(
                &listener,
                text_response("run.rs:520 新增分支没有测试", 1, 1),
            )
            .await;
        }
        serve_response(&listener, text_response("修正段:补了测试", 40, 8)).await;
    });

    let fx = fixture("fixup", address);
    let rt = fx.subagent_rt();
    let mut pipeline = fx.pipeline();
    pipeline.scout_skipped().await.unwrap();
    pipeline.begin_implementation().await.unwrap();
    assert_eq!(
        fx.coordinator
            .snapshot(&fx.project)
            .writer_run_id
            .as_deref(),
        Some("run_b6"),
        "实现阶段应持有写租约"
    );

    let impl_summary = implementation_summary();
    let impl_messages = impl_summary.messages.clone();
    let mut on_event = |_e: kanzei_core::RunEvent| {};
    let mut ask = |_r: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
    };
    let stage = |_n: &str, _d: String| {};

    let merged = crate::run::run_review_and_fixup(
        &mut pipeline,
        &fx.client,
        &fx.route,
        &fx.snapshot,
        &fx.agent,
        &fx.runner_config(),
        &fx.ctx,
        "修 R-173",
        Some(&rt),
        impl_summary,
        &mut on_event,
        &mut ask,
        &stage,
    )
    .await
    .expect("复核+修正应当成功");
    server.await.unwrap();
    pipeline.finish();

    // ① 历史接续:修正段的 messages 以实现段的 messages 为前缀,一条不少、顺序不变。
    assert!(
        merged.messages.len() > impl_messages.len(),
        "修正段必须在实现段历史之后追加,而不是另起一段"
    );
    for (index, original) in impl_messages.iter().enumerate() {
        let carried = &merged.messages[index];
        assert_eq!(
            format!("{:?}", carried.role),
            format!("{:?}", original.role),
            "第 {index} 条消息的角色在分段后被改写了"
        );
        assert_eq!(
            format!("{:?}", carried.parts),
            format!("{:?}", original.parts),
            "第 {index} 条消息的内容在分段后被改写了"
        );
    }
    // 轮末统计切片(&messages[prior.len()..])仍然覆盖两段全部内容:
    // prior 为空时切片就是全部 messages。
    assert_eq!(
        merged.messages.first().map(|m| format!("{:?}", m.parts)),
        impl_messages.first().map(|m| format!("{:?}", m.parts)),
        "切片起点必须还是原始用户消息"
    );

    // ② 用量与步数合并:用户看到的是"这一条消息一共花了多少"。
    assert_eq!(merged.usage.input, 100 + 40);
    assert_eq!(merged.usage.output, 20 + 8);
    assert_eq!(merged.usage.cache_read, 5);
    assert_eq!(merged.usage.cache_write, 7);
    assert_eq!(merged.steps, 3 + 1);
    assert!(
        merged
            .overflow_traces
            .contains(&"impl-overflow".to_string()),
        "实现段的溢出轨迹不能在合并时被丢掉"
    );
    assert!(merged.text.contains("修正段"));

    // ③ 阶段轨迹完整,且复核发生在写租约交出之后。
    let events = fx.events();
    let phases: Vec<&String> = events.iter().filter(|e| e.starts_with("phase:")).collect();
    assert_eq!(
        phases.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        vec![
            "phase:baseline",
            "phase:scouting",
            "phase:synthesis",
            "phase:implementation",
            "phase:integration",
            "phase:review",
            "phase:fixup",
            "phase:finished",
        ]
    );
    let released = events
        .iter()
        .position(|e| e == "orchestration.writer.released")
        .expect("复核前必须交出写租约");
    let review = events.iter().position(|e| e == "phase:review").unwrap();
    assert!(released < review, "不变量 9:复核必须在写租约释放之后启动");
    // 两段写区间:实现段一次、修正段一次,各自成对。
    assert_eq!(
        events
            .iter()
            .filter(|e| *e == "orchestration.writer.acquired")
            .count(),
        2
    );
    assert!(
        fx.coordinator.snapshot(&fx.project).writer_run_id.is_none(),
        "收尾后不得残留写租约"
    );
    std::fs::remove_dir_all(&fx.project).ok();
}

/// 复核全部无发现 → **不跑**修正段,summary 原样返回,本轮仍是一次 run_once。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 复核无发现时不额外跑一段() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..3 {
            serve_response(&listener, text_response("NO_ISSUES", 1, 1)).await;
        }
        // 第 4 条故意不服务:一旦修正段被误触发,这里会挂住直到测试超时。
    });

    let fx = fixture("clean", address);
    let rt = fx.subagent_rt();
    let mut pipeline = fx.pipeline();
    pipeline.scout_skipped().await.unwrap();
    pipeline.begin_implementation().await.unwrap();

    let impl_summary = implementation_summary();
    let impl_len = impl_summary.messages.len();
    let mut on_event = |_e: kanzei_core::RunEvent| {};
    let mut ask = |_r: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
    };
    let stage = |_n: &str, _d: String| {};

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        crate::run::run_review_and_fixup(
            &mut pipeline,
            &fx.client,
            &fx.route,
            &fx.snapshot,
            &fx.agent,
            &fx.runner_config(),
            &fx.ctx,
            "修 R-173",
            Some(&rt),
            impl_summary,
            &mut on_event,
            &mut ask,
            &stage,
        ),
    )
    .await
    .expect("复核无发现时不应再发起模型调用")
    .expect("复核应当成功");
    pipeline.finish();
    server.abort();

    assert_eq!(result.usage.input, 100, "没跑修正段,用量不该变");
    assert_eq!(result.steps, 3);
    assert_eq!(result.messages.len(), impl_len, "历史不该被追加");

    let events = fx.events();
    let phases: Vec<&String> = events.iter().filter(|e| e.starts_with("phase:")).collect();
    assert_eq!(
        phases.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        vec![
            "phase:baseline",
            "phase:scouting",
            "phase:synthesis",
            "phase:implementation",
            "phase:integration",
            "phase:review",
            "phase:finished",
        ],
        "无发现时不应出现 fixup 阶段"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| *e == "orchestration.writer.acquired")
            .count(),
        1,
        "无发现时只取一次写租约"
    );
    std::fs::remove_dir_all(&fx.project).ok();
}

/// 子代理关闭时:阶段序列仍然完整,空屏障照样留痕,写租约照样在复核前交出。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn 子代理关闭时阶段序列仍完整且空屏障留痕() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let fx = fixture("nosub", address);
    let mut pipeline = fx.pipeline();
    pipeline.scout_skipped().await.unwrap();
    pipeline.begin_implementation().await.unwrap();

    let mut on_event = |_e: kanzei_core::RunEvent| {};
    let mut ask = |_r: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
    };
    let stage = |_n: &str, _d: String| {};
    let result = crate::run::run_review_and_fixup(
        &mut pipeline,
        &fx.client,
        &fx.route,
        &fx.snapshot,
        &fx.agent,
        &fx.runner_config(),
        &fx.ctx,
        "修 R-173",
        None, // 子代理关闭
        implementation_summary(),
        &mut on_event,
        &mut ask,
        &stage,
    )
    .await
    .unwrap();
    pipeline.finish();

    assert_eq!(result.steps, 3, "没有复核角色时结果原样返回");
    let events = fx.events();
    // 两次 barrier(勘察 + 复核)都留了痕,而不是整段消失。
    assert_eq!(
        events
            .iter()
            .filter(|e| *e == "orchestration.barrier_reached")
            .count(),
        2,
        "空屏障也必须留痕,否则回放分不清「没勘察」与「没有勘察阶段」"
    );
    let released = events
        .iter()
        .position(|e| e == "orchestration.writer.released")
        .expect("即使没有复核角色,写租约也必须在复核阶段前交出");
    let review = events.iter().position(|e| e == "phase:review").unwrap();
    assert!(released < review);
    assert!(fx.coordinator.snapshot(&fx.project).writer_run_id.is_none());
    std::fs::remove_dir_all(&fx.project).ok();
}
