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
            compact: None,
            max_tokens: 256,
            timeout_secs: 30,
            limits: self.config.limits.clone(),
            coordinator: Some(self.coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>),
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
        }
    }

    fn runner_config(&self) -> kanzei_core::RunnerConfig {
        kanzei_core::RunnerConfig {
            intensity: kanzei_harness::HarnessIntensity::Autonomous,
            model: "mock".into(),
            max_tokens: 256,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: None,
            context_limit: None,
            limits: self.config.limits.clone(),
            recall: None,
            execution_policy:
                kanzei_harness::orchestration::ExecutionPolicy::ReadParallelWriteSerial,
            ask_policy: kanzei_core::AskPolicy::Interactive,
            halt: None,
        }
    }

    fn pipeline(&self) -> PhasePipeline {
        PhasePipeline::start(
            self.coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>,
            self.recorder.clone() as Arc<dyn PhaseObserver>,
            self.project.clone(),
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
        last_input_tokens: Some(100),
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

/// **验收②的关闭依据**,同时是**闸门取值②(「勘察复核」开)的行为锚点**:
/// 开关打开 → 编排对象真的被构造 → 一条运行跑完七阶段,事件落 `session_events` 可回放。
///
/// 这条测试复刻 `run_task` 在流水线开启时的**完整调用序列**,用的是同一批生产函数:
/// `start_if_enabled`(闸门本身)→ `acquire_plain_lease_if_needed` → `PhasePipeline::scout`
/// → `begin_implementation` → `run_once_with_parts` → `run_review_and_fixup`。协调器、
/// 子代理、事件观察者、SQLite 全是真的,只有 provider 是假的。
///
/// 入口刻意从**闸门**开始而不是直接 `PhasePipeline::start`:2026-08-11 换闸门那次
/// (自主推进 → 进程级「勘察复核」开关)如果只改 run.rs,直接构造的测试照样全绿。
///
/// 覆盖不到的只有 `run_task` 外围那层 Tauri 胶水(`Window` 事件、store 生命周期
/// 记账)——它需要真实 Tauri Window,单测起不来。
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn 七阶段闭环轨迹落库可回放() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    // 连接顺序 = 阶段顺序:5 勘察 → 1 实现 → 3 复核 → 1 修正。
    let server = tokio::spawn(async move {
        for _ in 0..5 {
            serve_response(
                &listener,
                text_response("勘察:drive.rs:57 注册无门控", 2, 1),
            )
            .await;
        }
        serve_response(&listener, text_response("实现段:已按勘察结论改完", 50, 10)).await;
        for _ in 0..3 {
            serve_response(&listener, text_response("run.rs 新分支缺测试", 2, 1)).await;
        }
        serve_response(&listener, text_response("修正段:补齐测试", 30, 6)).await;
    });

    let fx = fixture("e2e", address);
    let session_id = "ses_r173_e2e";
    let db = fx.project.join(".kanzei").join("state.db");
    {
        let store = kanzei_core::SessionStore::open(&db).unwrap();
        store
            .create_session(session_id, &fx.project.display().to_string(), None)
            .unwrap();
    }
    // 真实观察者:事件经 OrchestrationEvent 单一出口落进真的 SQLite。
    let observer =
        Arc::new(crate::orchestration_trace::SessionEventObserver::open(&db, session_id).unwrap());
    // ---- 闸门取值②:「勘察复核」开 → 装配编排对象 ----
    let mut pipeline = crate::phase_pipeline::start_if_enabled(
        true, // 进程级「勘察复核」开关打开
        &fx.config,
        &kanzei_llm::ProxyConfig::Disabled,
        fx.coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>,
        observer.clone() as Arc<dyn PhaseObserver>,
        fx.project.clone(),
        fx.project.clone(),
        "run_e2e",
        "proc_e2e",
        &|_name: &str, _detail: String| {},
    )
    .await
    .expect("「勘察复核」开着时必须构造编排对象");

    // ---- 验收①第二分句(生产路径):流水线开启时当场不取租约 ----
    let plain_lease = crate::phase_pipeline::acquire_plain_lease_if_needed(
        true, // pipeline_on
        fx.coordinator.as_ref(),
        observer.as_ref(),
        &fx.project,
        &fx.project,
        "run_e2e",
        "proc_e2e",
        session_id,
    )
    .await
    .unwrap();
    assert!(
        plain_lease.is_none(),
        "流水线开启时 run_task 不得当场取租约"
    );
    assert!(
        fx.coordinator.snapshot(&fx.project).writer_run_id.is_none(),
        "勘察还没开始,项目里不该有 writer"
    );

    let rt = fx.subagent_rt();
    let mut on_event = |_e: kanzei_core::RunEvent| {};
    let mut ask = |_r: kanzei_core::AskRequest| -> kanzei_core::AskFuture {
        Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
    };
    let stage = |_n: &str, _d: String| {};

    // ---- 勘察 → 汇总屏障 ----
    let brief = pipeline
        .scout(&fx.client, &rt, &fx.ctx, "修 R-173", &mut on_event)
        .await
        .expect("勘察应当成功");
    assert!(brief.contains("勘察简报") && brief.contains("drive.rs:57"));
    assert!(
        fx.coordinator.snapshot(&fx.project).writer_run_id.is_none(),
        "汇总屏障刚过、begin_implementation 之前,项目仍不得有 writer(不变量 2)"
    );

    // ---- 实现:取租约 → 主对话 run_once ----
    pipeline.begin_implementation().await.unwrap();
    assert_eq!(
        fx.coordinator
            .snapshot(&fx.project)
            .writer_run_id
            .as_deref(),
        Some("run_e2e"),
        "实现阶段必须持有写租约"
    );
    let summary = kanzei_core::run_once_with_parts(
        &fx.client,
        &fx.route,
        &fx.snapshot,
        &fx.agent,
        &fx.runner_config(),
        &fx.ctx,
        &format!("{brief}\n\n修 R-173"),
        None,
        None,
        &[],
        None,
        Some(&rt),
        // R-246:测试夹具不持有 LineRuntime。
        None,
        &mut on_event,
        &mut ask,
    )
    .await
    .expect("实现段应当成功");

    // ---- 集成 → 复核屏障 → 复核 → 修正 ----
    let merged = crate::run::execution::run_review_and_fixup(
        &crate::run::execution::ReviewExec {
            client: &fx.client,
            route: &fx.route,
            snapshot: &fx.snapshot,
            agent: &fx.agent,
            runner_config: &fx.runner_config(),
            ctx: &fx.ctx,
            prompt: "修 R-173",
            subagent_rt: Some(&rt),
            stage: &stage,
        },
        &mut pipeline,
        summary,
        &mut on_event,
        &mut ask,
    )
    .await
    .expect("复核+修正应当成功");
    pipeline.finish();
    server.await.unwrap();

    assert!(merged.text.contains("修正段"));
    assert!(
        fx.coordinator.snapshot(&fx.project).writer_run_id.is_none(),
        "收尾后不得残留写租约(不变量 7)"
    );

    // ---- 回放:另开连接读回落库的事件流 ----
    let store = kanzei_core::SessionStore::open(&db).unwrap();
    let events = store.list_events(session_id, 0).unwrap();
    let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();

    // 七阶段按设计文档顺序完整落库。
    let phases: Vec<&str> = events
        .iter()
        .filter(|e| e.event_type == "orchestration.phase_changed")
        .map(|e| e.payload["phase"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        phases,
        vec![
            "baseline",
            "scouting",
            "synthesis",
            "implementation",
            "integration",
            "review",
            "fixup",
            "finished",
        ],
        "七阶段轨迹必须完整且按序"
    );

    // 汇总屏障 5 个角色全终态,复核屏障 3 个。
    let barriers: Vec<&kanzei_core::StoredEvent> = events
        .iter()
        .filter(|e| e.event_type == "orchestration.barrier_reached")
        .collect();
    assert_eq!(barriers.len(), 2);
    assert_eq!(barriers[0].payload["barrier"], "synthesis");
    assert_eq!(barriers[0].payload["agent_count"], 5);
    assert_eq!(barriers[0].payload["completed"], 5);
    assert_eq!(barriers[1].payload["barrier"], "review");
    assert_eq!(barriers[1].payload["agent_count"], 3);

    // **验收①第二分句的可回放证据**:第一次取租约必须晚于汇总屏障。
    let synthesis_barrier = events
        .iter()
        .position(|e| {
            e.event_type == "orchestration.barrier_reached" && e.payload["barrier"] == "synthesis"
        })
        .unwrap();
    let first_acquire = types
        .iter()
        .position(|t| *t == "orchestration.writer.acquired")
        .expect("必须有取租约事件");
    assert!(
        synthesis_barrier < first_acquire,
        "汇总屏障必须早于第一次取写租约(不变量 2),实际序列: {types:?}"
    );

    // **验收③的可回放证据**:复核阶段晚于写租约释放。
    let released = types
        .iter()
        .position(|t| *t == "orchestration.writer.released")
        .unwrap();
    let review_phase = events
        .iter()
        .position(|e| {
            e.event_type == "orchestration.phase_changed" && e.payload["phase"] == "review"
        })
        .unwrap();
    assert!(
        released < review_phase,
        "复核必须在写租约释放之后启动(不变量 9)"
    );

    // 两段写区间:实现一次、修正一次,各自成对且不重叠。
    assert_eq!(
        types
            .iter()
            .filter(|t| **t == "orchestration.writer.acquired")
            .count(),
        2
    );
    assert_eq!(
        types
            .iter()
            .filter(|t| **t == "orchestration.writer.released")
            .count(),
        2
    );

    // **验收①第一分句 + ④**:8 个只读代理各自登记并回收了读槽。
    assert_eq!(
        types
            .iter()
            .filter(|t| **t == "orchestration.agent_started")
            .count(),
        8,
        "5 勘察 + 3 复核 都要登记读槽"
    );
    assert_eq!(
        types
            .iter()
            .filter(|t| **t == "orchestration.agent_completed")
            .count(),
        8
    );
    // sequence 单调 = 回放顺序即真实顺序。
    let sequences: Vec<i64> = events.iter().map(|e| e.sequence).collect();
    assert!(sequences.windows(2).all(|w| w[0] < w[1]));
    std::fs::remove_dir_all(&fx.project).ok();
}

/// **闸门取值①:「勘察复核」关(新默认)** —— 一个编排对象都不构造,行为与引入
/// 七阶段之前一致:当场取写租约、事件流里只有 R-171 的 queued→acquired 两条。
///
/// 入口同样是生产闸门 `start_if_enabled`,并把它的返回值原样喂给
/// `acquire_plain_lease_if_needed(pipeline.is_some(), …)`——与 run.rs 里逐字相同的
/// 表达式。闸门若被改回旧形态(读自主推进开关),这条与上面那条七阶段闭环必有一红。
#[tokio::test]
async fn 勘察复核关闭时不构造编排对象且行为与引入前一致() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fx = fixture("plain", listener.local_addr().unwrap());
    let db = fx.project.join(".kanzei").join("state.db");
    {
        let store = kanzei_core::SessionStore::open(&db).unwrap();
        store.create_session("ses_plain", "p", None).unwrap();
    }
    let observer =
        crate::orchestration_trace::SessionEventObserver::open(&db, "ses_plain").unwrap();
    // 阶段汇报口的替身:关着时连 `[models] scout` 都不该去解析,一行都不该有。
    let stage_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let pipeline = crate::phase_pipeline::start_if_enabled(
        false, // 进程级「勘察复核」开关关闭
        &fx.config,
        &kanzei_llm::ProxyConfig::Disabled,
        fx.coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>,
        fx.recorder.clone() as Arc<dyn PhaseObserver>,
        fx.project.clone(),
        fx.project.clone(),
        "run_plain",
        "proc_plain",
        &|name: &str, detail: String| stage_lines.lock().unwrap().push(format!("{name}:{detail}")),
    )
    .await;
    assert!(
        pipeline.is_none(),
        "「勘察复核」关着时不得构造编排对象——「不构造就是关」"
    );
    assert!(
        fx.events().is_empty(),
        "关着时不该有任何编排事件,实得: {:?}",
        fx.events()
    );
    assert!(
        stage_lines.lock().unwrap().is_empty(),
        "关着时不该解析勘察路由(白跑一次建链),实得: {:?}",
        stage_lines.lock().unwrap()
    );

    let lease = crate::phase_pipeline::acquire_plain_lease_if_needed(
        pipeline.is_some(), // 与 run.rs 逐字相同的表达式
        fx.coordinator.as_ref(),
        &observer,
        &fx.project,
        &fx.project,
        "run_plain",
        "proc_plain",
        "ses_plain",
    )
    .await
    .unwrap();
    assert!(lease.is_some(), "非流水线路径必须当场取租约");
    assert_eq!(
        fx.coordinator
            .snapshot(&fx.project)
            .writer_run_id
            .as_deref(),
        Some("run_plain")
    );
    // queued→acquired 两条事件照旧落库(R-171 的审计闭环不受批6 影响);
    // 一条 phase_changed / barrier_reached 都不该有——那是"走了七阶段"的标志。
    let store = kanzei_core::SessionStore::open(&db).unwrap();
    let types: Vec<String> = store
        .list_events("ses_plain", 0)
        .unwrap()
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert_eq!(
        types,
        vec![
            "orchestration.writer.queued",
            "orchestration.writer.acquired"
        ],
        "关着时的事件流必须与引入七阶段前逐条相同"
    );
    drop(lease);
    assert!(fx.coordinator.snapshot(&fx.project).writer_run_id.is_none());
    std::fs::remove_dir_all(&fx.project).ok();
}

/// R-182 内容①:同一项目的两条线在**桌面端这条真实取租约的路径上**互不排队。
///
/// 上面那条 `写租约取得时机` 测的是「取不取」,这条测的是「按什么范围取」——
/// run.rs 传的是 `ctx.cwd`(本轮代码树),两条线的 cwd 是各自的 worktree。
/// 事件里的 project_root 仍是主根,两者分开传,审计字段不说谎。
#[tokio::test]
async fn 两条线走桌面端取租约路径互不排队_事件仍记主根() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fx = fixture("r182_two_lines", listener.local_addr().unwrap());
    let db = fx.project.join(".kanzei").join("state.db");
    {
        let store = kanzei_core::SessionStore::open(&db).unwrap();
        store.create_session("ses_line_a", "p", None).unwrap();
        store.create_session("ses_line_b", "p", None).unwrap();
    }
    let observer_a =
        crate::orchestration_trace::SessionEventObserver::open(&db, "ses_line_a").unwrap();
    let observer_b =
        crate::orchestration_trace::SessionEventObserver::open(&db, "ses_line_b").unwrap();
    let tree_a = fx.project.join("..").join("wt-a");
    let tree_b = fx.project.join("..").join("wt-b");

    let lease_a = crate::phase_pipeline::acquire_plain_lease_if_needed(
        false,
        fx.coordinator.as_ref(),
        &observer_a,
        &fx.project, // 事件里的项目根:主根
        &tree_a,     // 仲裁范围:线 A 的代码树
        "run_line_a",
        "proc_line_a",
        "ses_line_a",
    )
    .await
    .unwrap();
    assert!(lease_a.is_some());

    // 线 B 必须**当场**拿到——改前它会一直等线 A 整轮结束。
    let lease_b = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        crate::phase_pipeline::acquire_plain_lease_if_needed(
            false,
            fx.coordinator.as_ref(),
            &observer_b,
            &fx.project,
            &tree_b,
            "run_line_b",
            "proc_line_b",
            "ses_line_b",
        ),
    )
    .await
    .expect("线 B 不得排队等线 A(R-182 内容①)")
    .unwrap();
    assert!(lease_b.is_some());
    assert!(fx.coordinator.snapshot(&tree_a).waiting_writers.is_empty());
    assert!(fx.coordinator.snapshot(&tree_b).waiting_writers.is_empty());

    // 审计字段:事件落的是**主根**,不是 worktree 路径。
    let store = kanzei_core::SessionStore::open(&db).unwrap();
    let payloads: Vec<String> = store
        .list_events("ses_line_a", 0)
        .unwrap()
        .into_iter()
        .map(|e| e.payload.to_string())
        .collect();
    let main_root_fragment = fx
        .project
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(
        payloads.iter().any(|p| p.contains(&main_root_fragment)),
        "写租约事件必须记主根,实得: {payloads:?}"
    );
    assert!(
        !payloads.iter().any(|p| p.contains("wt-a")),
        "审计字段不得被换成 worktree 路径,实得: {payloads:?}"
    );
    drop(lease_a);
    drop(lease_b);
    std::fs::remove_dir_all(&fx.project).ok();
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

/// D-419:单个角色完成后必须在其它角色仍等待屏障时立即发 ToolEnd。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 编排角色终态不等屏障立即上报() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    // 只响应第一个请求,但保持 listener 存活;其余角色会保持连接直到自己的超时。
    let server = tokio::spawn(async move {
        serve_response(&listener, text_response("快速完成", 1, 1)).await;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    });

    let fx = fixture("terminal_event_timing", address);
    let rt = fx.subagent_rt();
    let limits = kanzei_harness::config::Limits {
        subagent_timeout_secs: Some(2),
        barrier_timeout_secs: Some(4),
        max_tasks_per_turn: Some(5),
        ..Default::default()
    };
    let mut pipeline = PhasePipeline::start(
        fx.coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>,
        fx.recorder.clone() as Arc<dyn PhaseObserver>,
        fx.project.clone(),
        fx.project.clone(),
        "run_terminal_event_timing",
        "proc_terminal_event_timing",
        &limits,
        None,
    );
    let (end_tx, end_rx) = tokio::sync::oneshot::channel();
    let client = fx.client;
    let ctx = fx.ctx;
    let project = fx.project;
    let task = tokio::spawn(async move {
        let mut end_tx = Some(end_tx);
        let mut on_event = move |event: kanzei_core::RunEvent| {
            if let kanzei_core::RunEvent::ToolEnd { id, name, .. } = event {
                if name == "task" {
                    if let Some(tx) = end_tx.take() {
                        let _ = tx.send(id);
                    }
                }
            }
        };
        let result = pipeline
            .scout(&client, &rt, &ctx, "验证终态时机", &mut on_event)
            .await;
        pipeline.abort("test done");
        result
    });

    let id = tokio::time::timeout(std::time::Duration::from_secs(1), end_rx)
        .await
        .expect("单个角色完成后应在屏障结束前收到 ToolEnd")
        .expect("ToolEnd 通知通道不应提前关闭");
    assert!(
        [
            "architecture_scout",
            "docs_scout",
            "runtime_scout",
            "test_scout",
            "write_surface_scout",
        ]
        .contains(&id.as_str()),
        "ToolEnd id 必须对应角色: {id}"
    );
    assert!(
        !task.is_finished(),
        "仍有角色等待超时时,单个角色的 ToolEnd 不应等到整个屏障完成"
    );

    task.await
        .expect("勘察任务不应 panic")
        .expect("其余角色超时应作为失败结果收敛");
    server.abort();
    std::fs::remove_dir_all(project).ok();
}

/// D-301:编排角色必须在自己的墙钟上界收敛,不能把唯一角色拖到外层屏障上界。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn 编排单角色超时在内层收敛且不触发屏障超时() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
        std::future::pending::<()>().await;
    });

    let fx = fixture("per_role_timeout", address);
    let mut rt = fx.subagent_rt();
    rt.timeout_secs = 1;
    let limits = kanzei_harness::config::Limits {
        subagent_timeout_secs: Some(1),
        barrier_timeout_secs: Some(3),
        max_tasks_per_turn: Some(1),
        ..Default::default()
    };
    let mut pipeline = PhasePipeline::start(
        fx.coordinator.clone() as Arc<dyn ProjectExecutionCoordinator>,
        fx.recorder.clone() as Arc<dyn PhaseObserver>,
        fx.project.clone(),
        fx.project.clone(),
        "run_per_role_timeout",
        "proc_per_role_timeout",
        &limits,
        None,
    );
    let seen: Arc<Mutex<Vec<kanzei_core::RunEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let mut on_event = move |event: kanzei_core::RunEvent| sink.lock().unwrap().push(event);

    let brief = pipeline
        .scout(&fx.client, &rt, &fx.ctx, "验证单角色墙钟", &mut on_event)
        .await
        .expect("角色超时应作为 ScoutOutcome 收敛,不应让流水线报错");
    pipeline.abort("test done");
    server.abort();

    assert!(
        brief.contains("超时(1s 未返回)"),
        "模型简报必须点名角色超时: {brief}"
    );
    assert!(
        !brief.contains("屏障自身触顶上界"),
        "内层角色超时不应被误报成屏障触顶: {brief}"
    );
    let events = seen.lock().unwrap();
    assert!(
        events.iter().any(|event| matches!(
            event,
            kanzei_core::RunEvent::ToolEnd { id, ok: false, .. }
                if id == "architecture_scout"
        )),
        "角色超时必须以失败 ToolEnd 终态上抛"
    );
    assert!(
        !fx.events()
            .iter()
            .any(|event| event == "orchestration.barrier_timed_out"),
        "角色已在内层收敛时不得产生 barrier_timed_out 事件"
    );
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

    let merged = crate::run::execution::run_review_and_fixup(
        &crate::run::execution::ReviewExec {
            client: &fx.client,
            route: &fx.route,
            snapshot: &fx.snapshot,
            agent: &fx.agent,
            runner_config: &fx.runner_config(),
            ctx: &fx.ctx,
            prompt: "修 R-173",
            subagent_rt: Some(&rt),
            stage: &stage,
        },
        &mut pipeline,
        impl_summary,
        &mut on_event,
        &mut ask,
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
        crate::run::execution::run_review_and_fixup(
            &crate::run::execution::ReviewExec {
                client: &fx.client,
                route: &fx.route,
                snapshot: &fx.snapshot,
                agent: &fx.agent,
                runner_config: &fx.runner_config(),
                ctx: &fx.ctx,
                prompt: "修 R-173",
                subagent_rt: Some(&rt),
                stage: &stage,
            },
            &mut pipeline,
            impl_summary,
            &mut on_event,
            &mut ask,
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
    let result = crate::run::execution::run_review_and_fixup(
        &crate::run::execution::ReviewExec {
            client: &fx.client,
            route: &fx.route,
            snapshot: &fx.snapshot,
            agent: &fx.agent,
            runner_config: &fx.runner_config(),
            ctx: &fx.ctx,
            prompt: "修 R-173",
            subagent_rt: None, // 子代理关闭
            stage: &stage,
        },
        &mut pipeline,
        implementation_summary(),
        &mut on_event,
        &mut ask,
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
