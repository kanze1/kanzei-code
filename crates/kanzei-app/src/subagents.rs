//! Button-scoped registration, idea decomposition and read-only defect review.
use crate::{run_once_with_parts, AppState, AskFuture};
use kanzei_core::{RunEvent, RunnerConfig};
use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};
use kanzei_memory::docstore::{DocStore, DEFECTS, IDEAS, REQUIREMENTS};
use std::sync::Arc;

const QUICK_DEFECT_SYSTEM: &str = "根据用户描述登记缺陷，按独立问题组织条目并查重。保留限定条件，只记录有依据的现象和复现；缺失信息写为具体待澄清问题，不补造事实。原文和来源由系统保存并绑定。返回实际登记或复用的 ID，未完成的部分如实说明。";
const QUICK_REQUIREMENT_SYSTEM: &str = "将用户意图登记为可追踪的需求，按可独立验收的行为组织并查重。保留明确的约束，正文和验收只写有依据的行为与可观察结果；未知信息记录为问题并保留草稿，不替用户补功能、阈值、技术方案或批准。原文和来源由系统保存并绑定。返回实际登记或复用的 ID，未完成的部分如实说明。";
const IDEA_SPLIT_SYSTEM: &str = "根据所选想法的完整记录，整理可独立追踪的需求或缺陷，并核对已有条目。保留原记录的限定条件，不补造功能、复现、阈值或批准；未知信息记录为问题。来源由系统绑定。说明实际登记或复用的条目及未解决的问题，不开展实现。";

/// These tasks use their configured model once; a failed write task is not rerun on a second model.
async fn run_button_agent(
    config: &KanzeiConfig,
    snapshot: &kanzei_harness::HarnessSnapshot,
    agent: &kanzei_harness::AgentDef,
    ctx: &ToolCtx,
    prompt: &str,
    execution_policy: kanzei_harness::orchestration::ExecutionPolicy,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> Result<kanzei_core::RunSummary, String> {
    let resolved = config
        .resolve_model(&agent.model)
        .map_err(|error| format!("模型配置失败: {error}"))?;
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(value) => ProxyConfig::Explicit(value.into()),
    };
    let route = kanzei_core::build_route(&resolved, &proxy)
        .await
        .map_err(|error| format!("模型路由失败: {error}"))?;
    let client = LlmClient::new(&proxy).map_err(|error| error.to_string())?;
    let runner = RunnerConfig {
        hosted_tools: Vec::new(),
        digest_model: None,
        intensity: kanzei_harness::HarnessIntensity::Autonomous,
        model: resolved.model.clone(),
        max_tokens: config.limits.subagent_max_tokens(),
        reasoning: kanzei_llm::ReasoningEffort::Off,
        service_tier: config.service_tier_for(&resolved),
        context_limit: resolved.provider.context_limit,
        limits: config.limits.clone(),
        recall: None,
        execution_policy,
        ask_policy: kanzei_core::AskPolicy::NonInteractive,
        halt: None,
    };
    let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
        Box::pin(async move {
            match request {
                kanzei_core::AskRequest::Permission { .. } => {
                    kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny)
                }
                kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
            }
        })
    };
    run_once_with_parts(
        &client,
        &route,
        snapshot,
        agent,
        &runner,
        ctx,
        prompt,
        None,
        &[],
        None,
        None,
        on_event,
        &mut ask,
    )
    .await
    .map_err(|error| format!("模型执行失败: {error}"))
}

fn task_issue(result: &Result<kanzei_core::RunSummary, String>) -> Option<String> {
    match result {
        Err(error) => Some(error.clone()),
        Ok(summary) if summary.halted_by_user => Some("本次执行已停止".into()),
        Ok(summary) => summary.incomplete_reason().map(str::to_owned),
    }
}

fn task_report(receipt: String, result: &Result<kanzei_core::RunSummary, String>) -> String {
    match result {
        Ok(summary) if !summary.text.trim().is_empty() => {
            format!("{receipt}\n\nAgent 说明：\n{}", summary.text.trim())
        }
        _ => receipt,
    }
}

#[tauri::command]
pub(crate) async fn quick_req(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    description: String,
    kind: Option<String>,
) -> Result<String, String> {
    quick_req_with_coordinator(state.coordinator.clone(), project_dir, description, kind).await
}

pub(crate) async fn quick_req_with_coordinator(
    coordinator: Arc<kanzei_core::orchestration::MemoryCoordinator>,
    project_dir: String,
    description: String,
    kind: Option<String>,
) -> Result<String, String> {
    if description.trim().is_empty() {
        return Err("描述不能为空".into());
    }
    let capture = match kind.as_deref() {
        Some("defect") => "defect",
        Some("req") | None => "req",
        Some(other) => return Err(format!("未知登记类型: {other}")),
    };
    let root = crate::normalized_project_root(std::path::Path::new(&project_dir));
    let config = Arc::new(KanzeiConfig::load_at_root(&root).map_err(|error| error.to_string())?);
    let _lease = coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            write_scope: root.clone(),
            run_id: format!("quick_req_{}", crate::run::now_ms()),
            process_id: "quick_req".into(),
            reason: "tracker registration".into(),
        })
        .await
        .map_err(|error| format!("无法获取项目写租约: {error}"))?;
    let source = crate::tracker_capture::save_source(&root, &description)?;
    let rctx = ResolveCtx {
        profile: ProfileKind::DevAuto,
        cwd: root.clone(),
        project_root: root.clone(),
        config: config.clone(),
    };
    let mut harness = Harness::default();
    harness.add(crate::harness_ext::QuickCaptureComponent {
        capture,
        source: source.clone(),
    });
    let snapshot = harness.resolve(&rctx).map_err(|error| error.to_string())?;
    let agent = kanzei_harness::AgentDef {
        name: "quickcapture".into(),
        profile: kanzei_harness::ProfileScope::Development,
        model: "fast".into(),
        mode: kanzei_harness::AgentMode::Subagent,
        steps: 0,
        system: if capture == "defect" {
            QUICK_DEFECT_SYSTEM
        } else {
            QUICK_REQUIREMENT_SYSTEM
        }
        .into(),
    };
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let mut tool_errors = Vec::new();
    let mut on_event = |event: RunEvent| {
        if let RunEvent::ToolEnd {
            name,
            ok: false,
            content,
            ..
        } = event
        {
            if name == capture {
                tool_errors.push(content);
            }
        }
    };
    let result = run_button_agent(
        &config,
        &snapshot,
        &agent,
        &ctx,
        &description,
        kanzei_harness::orchestration::ExecutionPolicy::Default,
        &mut on_event,
    )
    .await;
    let ids = source.ids()?;
    let issue = task_issue(&result);
    if ids.is_empty() {
        let details = issue
            .or_else(|| (!tool_errors.is_empty()).then(|| tool_errors.join("；")))
            .unwrap_or_else(|| "没有登记或复用任何条目".into());
        return Err(format!("登记未完成: {details}"));
    }
    let kind = if capture == "defect" {
        &DEFECTS
    } else {
        &REQUIREMENTS
    };
    let store = DocStore::open(&root, kind);
    let mut entries = store.load().map_err(|error| error.to_string())?;
    entries.extend(store.load_archive().map_err(|error| error.to_string())?);
    let mut receipts = Vec::new();
    for id in ids {
        let entry = entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| format!("登记回执 {id} 对应的条目不存在"))?;
        receipts.push(format!("{} {}", entry.id, entry.title));
    }
    let receipt = task_report(receipts.join("\n"), &result);
    Ok(match issue {
        Some(issue) => format!("{receipt}\n部分登记失败: {issue}"),
        None => receipt,
    })
}

#[tauri::command]
pub(crate) async fn idea_split(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    id: String,
) -> Result<String, String> {
    idea_split_with_coordinator(state.coordinator.clone(), project_dir, id).await
}

pub(crate) async fn idea_split_with_coordinator(
    coordinator: Arc<kanzei_core::orchestration::MemoryCoordinator>,
    project_dir: String,
    id: String,
) -> Result<String, String> {
    let id = id.trim();
    if !id.starts_with("I-") {
        return Err(format!("`{id}` 不是想法编号"));
    }
    let root = crate::normalized_project_root(std::path::Path::new(&project_dir));
    let config = Arc::new(KanzeiConfig::load_at_root(&root).map_err(|error| error.to_string())?);
    let _lease = coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            write_scope: root.clone(),
            run_id: format!("idea_split_{}", crate::run::now_ms()),
            process_id: "idea_split".into(),
            reason: "idea decomposition".into(),
        })
        .await
        .map_err(|error| format!("无法获取项目写租约: {error}"))?;
    let idea_store = DocStore::open(&root, &IDEAS);
    let idea = idea_store
        .load()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| format!("想法 {id} 不存在"))?;
    if idea.status != "inbox" {
        return Err(format!(
            "想法 {id} 当前状态是 {}，只有收件箱中的想法可以拆解",
            idea.status
        ));
    }
    let original = kanzei_memory::docstore::render(&IDEAS, std::slice::from_ref(&idea));
    let source = crate::tracker_capture::save_source(&root, &original)?;
    let rctx = ResolveCtx {
        profile: ProfileKind::DevAuto,
        cwd: root.clone(),
        project_root: root.clone(),
        config: config.clone(),
    };
    let mut harness = Harness::default();
    harness.add(crate::harness_ext::IdeaSplitComponent {
        idea,
        source: source.clone(),
    });
    let snapshot = harness.resolve(&rctx).map_err(|error| error.to_string())?;
    let agent = kanzei_harness::AgentDef {
        name: "idea-split".into(),
        profile: kanzei_harness::ProfileScope::Development,
        model: "fast".into(),
        mode: kanzei_harness::AgentMode::Subagent,
        steps: 0,
        system: IDEA_SPLIT_SYSTEM.into(),
    };
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let mut on_event = |_event: RunEvent| {};
    let result = run_button_agent(
        &config,
        &snapshot,
        &agent,
        &ctx,
        &format!("将想法 {id} 拆解为需求或缺陷条目。"),
        kanzei_harness::orchestration::ExecutionPolicy::Default,
        &mut on_event,
    )
    .await;
    let issue = task_issue(&result);
    let current = idea_store
        .load()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| format!("原想法 {id} 已不存在"))?;
    if current.status == "split" {
        let refs = current
            .fields
            .iter()
            .find(|(key, _)| key == "refs")
            .map(|(_, value)| value.as_str())
            .ok_or_else(|| format!("想法 {id} 的完成状态缺少产出引用"))?;
        let receipt = task_report(format!("{id} → {refs}"), &result);
        return Ok(match issue {
            Some(issue) => format!("{receipt}\n状态已保存，后续模型执行未完整结束: {issue}"),
            None => receipt,
        });
    }
    let ids = source.ids()?;
    if ids.is_empty() {
        return Err(format!(
            "拆解未完成: {}",
            issue.unwrap_or_else(|| "没有登记、复用条目或提交完成引用".into())
        ));
    }
    let receipt = task_report(
        format!(
            "{id} 已登记 {}；想法仍在收件箱，拆解尚未确认完成。",
            ids.join(" ")
        ),
        &result,
    );
    Ok(match issue {
        Some(issue) => format!("{receipt}\n本次执行未完成: {issue}"),
        None => receipt,
    })
}

#[cfg(test)]
#[path = "subagents_capture_tests.rs"]
mod capture_tests;

#[cfg(test)]
mod tests {
    // The local provider drives real tools and explicitly completes the selected idea.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn idea_split_runs_subagent_and_marks_idea_split_with_real_refs() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        // 按 Content-Length 读完整请求再响应。
        // read_to_end 会等 EOF,而客户端在等响应——双向等待死锁(D-363 同类坑)。
        async fn serve_response(listener: &TcpListener, response: serde_json::Value) {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            let header_end = loop {
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
                if let Some(position) = request.windows(4).position(|w| w == b"\r\n\r\n") {
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
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
            }
            let body = format!("data: {response}\n\ndata: [DONE]\n\n");
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(body.as_bytes()).await.unwrap();
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        // The agent reads, registers, and explicitly submits completion refs.
        let server = tokio::spawn(async move {
            let tool_call = |name: &str, args: serde_json::Value| {
                serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "id": format!("call_{name}_{}", args["action"].as_str().unwrap()),
                                "type": "function",
                                "function": { "name": name, "arguments": args.to_string() }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }],
                    "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
                })
            };
            let resp1 = tool_call(
                "idea",
                serde_json::json!({ "action": "get", "id": "I-001" }),
            );
            let resp2 = tool_call(
                "req",
                serde_json::json!({
                    "action":"add","title":"冒烟需求","priority":"P2","tag":"后端",
                    "requirement":{"statement":"用户请求时，系统应保存拆解结果。","acceptance":[{"text":"保存后可读取拆解结果"}]}
                }),
            );
            let resp3 = tool_call(
                "defect",
                serde_json::json!({ "action": "add", "title": "冒烟缺陷", "severity": "medium",
                    "priority": "P2", "tag":"前端", "fields": { "复现": "待澄清" } }),
            );
            let complete = tool_call(
                "idea",
                serde_json::json!({"action":"update","id":"I-001","status":"split","refs":["R-001","D-001"]}),
            );
            let resp4 = serde_json::json!({
                "choices": [{ "index": 0, "delta": { "content": "R-001 D-001" }, "finish_reason": "stop" }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
            });
            for resp in [resp1, resp2, resp3, complete, resp4] {
                serve_response(&listener, resp).await;
            }
        });

        // 项目夹具:kanzei.toml 指向桩 provider;ideas.md 含一条 inbox 想法;
        // Empty target stores make source-bound records straightforward to inspect.
        let project = std::env::temp_dir().join(format!(
            "kz-idea-split-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(project.join(".kanzei/project")).unwrap();
        std::fs::write(
            project.join(".kanzei/kanzei.toml"),
            format!(
                "[models]\nprimary = \"mock:test-model\"\nfast = \"mock:test-model\"\n\n[providers.mock]\nprotocol = \"openai\"\nbase_url = \"http://{address}/v1\"\n"
            ),
        )
        .unwrap();
        std::fs::write(
            project.join(".kanzei/project/ideas.md"),
            "# Ideas\n\n## I-001 一个原始想法 [inbox]\n- 原文: 想法原文\n",
        )
        .unwrap();
        std::fs::write(
            project.join(".kanzei/project/requirements.md"),
            "# Requirements\n",
        )
        .unwrap();
        std::fs::write(project.join(".kanzei/project/defects.md"), "# Defects\n").unwrap();

        let sources = kanzei_harness::project_trust::status(&project).unwrap();
        kanzei_harness::project_trust::approve(&project, &sources.fingerprint).unwrap();

        let coordinator = std::sync::Arc::new(kanzei_core::orchestration::MemoryCoordinator::new());
        let result = super::idea_split_with_coordinator(
            coordinator,
            project.display().to_string(),
            "I-001".into(),
        )
        .await;
        assert!(result.is_ok(), "idea_split 应成功: {:?}", result);
        tokio::time::timeout(std::time::Duration::from_secs(20), server)
            .await
            .unwrap()
            .unwrap();
        let msg = result.unwrap();
        assert!(msg.starts_with("I-001 →"), "返回应含产出编号: {msg}");
        assert!(msg.contains("R-001") && msg.contains("D-001"), "{msg}");

        // 想法已转 split 且 refs 是真实 R/D(硬门禁放行即证明条目真实存在)。
        let ideas =
            kanzei_memory::docstore::DocStore::open(&project, &kanzei_memory::docstore::IDEAS)
                .load()
                .unwrap();
        let idea = ideas.iter().find(|e| e.id == "I-001").unwrap();
        assert_eq!(idea.status, "split", "拆解后想法必须转 split");
        let refs = idea.refs();
        assert!(refs.contains(&"R-001".to_string()), "{refs:?}");
        assert!(refs.contains(&"D-001".to_string()), "{refs:?}");
        // requirements/defects 真的落了条目。
        let reqs = kanzei_memory::docstore::DocStore::open(
            &project,
            &kanzei_memory::docstore::REQUIREMENTS,
        )
        .load()
        .unwrap();
        assert_eq!(reqs.len(), 1, "拆解必须真实新增需求条目");
        let defects =
            kanzei_memory::docstore::DocStore::open(&project, &kanzei_memory::docstore::DEFECTS)
                .load()
                .unwrap();
        assert_eq!(defects.len(), 1, "拆解必须真实新增缺陷条目");
        std::fs::remove_dir_all(&project).ok();
    }
}

const DEFECT_REVIEW_SYSTEM: &str = "只读审查当前项目的活动缺陷。结合相关代码、测试和设计判断缺陷是否仍成立，指出重复、影响与优先级，并给出可定位的证据和后续建议。按发现组织报告，区分已核实与待核实；阅读测试源码不等于运行测试。";

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DefectReviewResult {
    pub(crate) empty: bool,
    pub(crate) report: String,
    pub(crate) defect_count: usize,
}

pub(crate) fn defect_review_snapshot(
    rctx: &ResolveCtx,
) -> anyhow::Result<Arc<kanzei_harness::HarnessSnapshot>> {
    let mut harness = Harness::default();
    harness
        .add(kanzei_tools::SubagentBase)
        .add(crate::ConfigComponent);
    let mut context = rctx.clone();
    context.profile = ProfileKind::Readonly;
    harness.resolve(&context)
}

pub(crate) fn defect_review_report(summary: &kanzei_core::RunSummary) -> Result<String, String> {
    let report = summary.text.trim();
    if report.is_empty() {
        Err("审查模型没有返回报告".into())
    } else {
        let issue = if summary.halted_by_user {
            Some("审查已停止")
        } else {
            summary.incomplete_reason()
        };
        Ok(match issue {
            Some(issue) => format!("{report}\n\n审查未完成：{issue}"),
            None => report.into(),
        })
    }
}

#[tauri::command]
pub(crate) async fn defect_review(project_dir: String) -> Result<DefectReviewResult, String> {
    let cwd = crate::normalized_project_root(std::path::Path::new(&project_dir));
    let project_root = cwd.clone();
    let config = Arc::new(KanzeiConfig::load_at_root(&project_root).map_err(|e| e.to_string())?);
    let defects = DocStore::open(&project_root, &DEFECTS)
        .load()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|entry| !DEFECTS.terminal.contains(&entry.status.as_str()))
        .collect::<Vec<_>>();
    if defects.is_empty() {
        return Ok(DefectReviewResult {
            empty: true,
            report: "当前没有活动缺陷。".into(),
            defect_count: 0,
        });
    }
    let rctx = ResolveCtx {
        profile: ProfileKind::DevAuto,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let snapshot = defect_review_snapshot(&rctx).map_err(|e| e.to_string())?;
    let mut agent = kanzei_tools::explore_agent();
    agent.name = "defect-review".into();
    agent.system = DEFECT_REVIEW_SYSTEM.into();
    let tool_ctx = ToolCtx::new(cwd, project_root);
    let prompt = format!(
        "审查活动缺陷 {}。记录文件：{}。给出发现与依据。",
        defects
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>()
            .join("、"),
        DEFECTS.rel_path
    );
    let mut on_event = |_event: RunEvent| {};
    let summary = run_button_agent(
        &config,
        &snapshot,
        &agent,
        &tool_ctx,
        &prompt,
        kanzei_harness::orchestration::ExecutionPolicy::ReadOnly,
        &mut on_event,
    )
    .await?;
    Ok(DefectReviewResult {
        empty: false,
        report: defect_review_report(&summary)?,
        defect_count: defects.len(),
    })
}
