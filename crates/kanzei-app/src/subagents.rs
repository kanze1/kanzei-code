//! 独立子代理命令：快速结构化落库与缺陷审查。

use std::path::PathBuf;
use std::sync::Arc;

use crate::{run_once_with_parts, AppState, AskFuture};
use kanzei_core::{RunEvent, RunnerConfig};
use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use kanzei_harness::Tool as _;
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use kanzei_llm::LlmClient;
use kanzei_llm::ProxyConfig;
use kanzei_tools::docstore::{DocStore, DEFECTS, IDEAS, REQUIREMENTS};

#[cfg(test)]
const QUICK_CAPTURE_TAGS: &str = "核心|后端|前端|模型|发布|流程";

const QUICK_REQ_DEFECT_SYSTEM: &str = "You capture ONE defect from the user's natural-language description. Call the `defect` tool exactly once with action \"add\": a concise title (<=40 chars, Chinese preferred, keep qualifier words like 用户/桌面端/CLI from the original), severity high|medium|low, fields = {\"标签\": pick ONE tag from [核心|后端|前端|模型|发布|流程] best matching the subject, \"复现\": concrete reproduction steps ONLY if the description actually contains them — NEVER invent or pad one; when not reproducible from the text, write \"待澄清: \" followed by the specific questions the user must answer, \"原始描述\": the user's original text verbatim}. Then reply with only the new id.";

const QUICK_REQ_REQUIREMENT_SYSTEM: &str = "You capture ONE requirement from the user's natural-language description. Call the `req` tool exactly once with action \"add\": a concise title (<=40 chars, Chinese preferred), fields = {\"标签\": pick ONE tag from [核心|后端|前端|模型|发布|流程] best matching the subject, \"priority\": suggested P0-P3, \"复杂度\": 小|中|大, \"验收\": one draft acceptance line, \"归属\": \"kanzei\", \"原始描述\": the user's original text verbatim}. Then reply with only the new id.";

#[tauri::command]
pub(crate) async fn quick_req(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    description: String,
    kind: Option<String>,
) -> Result<String, String> {
    let description = description.trim().to_string();
    if description.is_empty() {
        return Err("描述不能为空".into());
    }
    let capture: &'static str = match kind.as_deref() {
        Some("defect") => "defect",
        _ => "req",
    };
    let cwd = PathBuf::from(&project_dir);
    let config = Arc::new(KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    // R-171 批4:quick_req 是独立写入口(直接写 requirements/defects),必须接入
    // 项目级写仲裁——若主对话 writer run 正在写,这里排队等待,不能绕过协调器
    // (验收⑤/设计不变量 8)。RAII:子代理跑完(或失败)即释放。
    let _lease = state
        .coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            write_scope: project_root.clone(),
            run_id: format!("quick_req_{}", crate::run::now_ms()),
            process_id: "quick_req".into(),
            reason: "quick capture write".into(),
        })
        .await
        .map_err(|e| format!("无法获取项目写租约: {e}"))?;
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let mut harness = Harness::default();
    harness.add(crate::harness_ext::QuickCaptureComponent { capture });
    let snapshot = harness.resolve(&rctx).map_err(|e| e.to_string())?;
    let system = if capture == "defect" {
        QUICK_REQ_DEFECT_SYSTEM
    } else {
        QUICK_REQ_REQUIREMENT_SYSTEM
    };
    let agent = kanzei_harness::AgentDef {
        name: "quickcapture".into(),
        profile: kanzei_harness::ProfileScope::Dev,
        model: "fast".into(),
        mode: kanzei_harness::AgentMode::Subagent,
        steps: 4,
        system: system.into(),
    };
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let tool_ctx = ToolCtx {
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        ..Default::default()
    };
    let doc_kind = if capture == "defect" {
        &DEFECTS
    } else {
        &REQUIREMENTS
    };
    let store = DocStore::open(&project_root, doc_kind);
    let before: std::collections::HashSet<String> = store
        .load()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|e| e.id.clone())
        .collect();
    let prompt = format!("描述(原文):\n{description}");
    for role in ["fast", "primary"] {
        let Ok(resolved) = config.resolve_model(role) else {
            continue;
        };
        let Ok(route) = kanzei_core::build_route(&resolved, &proxy).await else {
            continue;
        };
        let runner_config = RunnerConfig {
            model: resolved.model.clone(),
            max_tokens: config.limits.subagent_max_tokens(),
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
            ask_policy: kanzei_core::AskPolicy::NonInteractive,
            halt: None,
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
            Box::pin(async move {
                match request {
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        let _ = run_once_with_parts(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &tool_ctx,
            &prompt,
            None,
            None,
            &[],
            None,
            None,
            // R-246:子代理探索 run 不持有 LineRuntime(子代理禁嵌套 owner)。
            None,
            &mut on_event,
            &mut ask,
        )
        .await;
        let after = store.load().map_err(|e| e.to_string())?;
        if let Some(new_entry) = after.iter().find(|e| !before.contains(&e.id)) {
            return Ok(format!("{} {}", new_entry.id, new_entry.title));
        }
    }
    Err("子代理未能落库(fast/primary 均失败),请重试或在对话里直接说".into())
}

/// R-252 验收③/⑤:想法拆解子代理的系统提示——读原想法全文、拆成需求/缺陷,
/// 保留原话限定词、不编造验收与复现;只产出条目,不自己动想法状态(主进程收口)。
const IDEA_SPLIT_SYSTEM: &str = "You split ONE raw idea into concrete requirements and defects. \
Call `idea get <id>` first to read the idea's full original text. Then create 1+ entries with \
`req add` and/or `defect add`: each title <=40 chars (Chinese preferred, keep qualifier words \
like 用户/桌面端/CLI from the original), fields = {\"标签\": ONE tag from [核心|后端|前端|模型|发布|流程], \
\"原始描述\": the idea's original text verbatim, \"验收\" for req (one draft line) / \"复现\" for \
defect (concrete steps ONLY if the original actually contains them — never invent or pad one; \
otherwise write 待澄清), \"priority\": suggested P0-P3, \"复杂度\": 小|中|大 for req}. \
Do NOT update the idea's own status — the main process does that after verifying the new ids. \
Then reply with only the new ids.";

/// R-252 验收⑤:人点「拆解」按钮后派出的子代理命令。照 quick_req 模式:
/// 写租约 + 组件挂 req/defect/idea + before/after 差集取真实新增 ID。
/// 子代理只负责产出 R-/D- 条目;主进程在差集拿到真实新增 ID 后,把该想法
/// 转 split 并写 refs(经 actions::update_close 的 refs 硬门禁校验)。
/// 主体抽成 [`idea_split_with_coordinator`] 以便 fake server 集成测试直接调用。
#[tauri::command]
pub(crate) async fn idea_split(
    state: tauri::State<'_, AppState>,
    project_dir: String,
    id: String,
) -> Result<String, String> {
    idea_split_with_coordinator(state.coordinator.clone(), project_dir, id).await
}

/// idea_split 主体(coordinator 注入,测试可绕过 tauri::State 直接调用)。
pub(crate) async fn idea_split_with_coordinator(
    coordinator: Arc<kanzei_core::orchestration::MemoryCoordinator>,
    project_dir: String,
    id: String,
) -> Result<String, String> {
    let id = id.trim().to_string();
    if !id.starts_with("I-") {
        return Err(format!("`{id}` 不是想法编号(I-xxx)"));
    }
    let cwd = PathBuf::from(&project_dir);
    let config = Arc::new(KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    // 想法必须真实存在且处于 inbox(inbox 才能拆;split/dropped 是终态)。
    let idea_store = DocStore::open(&project_root, &IDEAS);
    let idea = idea_store
        .load()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("想法 {id} 不存在"))?;
    if idea.status != "inbox" {
        return Err(format!(
            "想法 {id} 当前状态是 {};只有 inbox 想法可以拆解。",
            idea.status
        ));
    }
    // 与 quick_req 同源:独立写入口必须接入项目级写仲裁,不绕过协调器。
    let _lease = coordinator
        .acquire_writer_lease(kanzei_harness::orchestration::WriterLeaseRequest {
            write_scope: project_root.clone(),
            run_id: format!("idea_split_{}", crate::run::now_ms()),
            process_id: "idea_split".into(),
            reason: "idea split write".into(),
        })
        .await
        .map_err(|e| format!("无法获取项目写租约: {e}"))?;
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let mut harness = Harness::default();
    harness.add(crate::harness_ext::IdeaSplitComponent);
    let snapshot = harness.resolve(&rctx).map_err(|e| e.to_string())?;
    let agent = kanzei_harness::AgentDef {
        name: "idea-split".into(),
        profile: kanzei_harness::ProfileScope::Dev,
        model: "fast".into(),
        mode: kanzei_harness::AgentMode::Subagent,
        steps: 8,
        system: IDEA_SPLIT_SYSTEM.into(),
    };
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(p) => ProxyConfig::Explicit(p.to_string()),
    };
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let tool_ctx = ToolCtx {
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        ..Default::default()
    };
    // before/after 差集:requirements + defects 两条线都要看,取真实新增 ID。
    let before_ids = |kind: &'static kanzei_tools::docstore::DocKind| -> Result<std::collections::HashSet<String>, String> {
        Ok(DocStore::open(&project_root, kind)
            .load()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|e| e.id.clone())
            .collect())
    };
    let before_req = before_ids(&REQUIREMENTS)?;
    let before_def = before_ids(&DEFECTS)?;
    // 想法全文是拆解的输入(子代理先 `idea get <id>` 读原话);提示词不含 system 全文。
    let prompt = format!("拆解想法 {id}:先 idea get {id} 读全文,再产出需求/缺陷条目。");
    let mut last_error = String::from("没有可用的 fast 或 primary 模型");
    for role in ["fast", "primary"] {
        let resolved = match config.resolve_model(role) {
            Ok(value) => value,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let route = match kanzei_core::build_route(&resolved, &proxy).await {
            Ok(value) => value,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let runner_config = RunnerConfig {
            model: resolved.model.clone(),
            max_tokens: config.limits.subagent_max_tokens(),
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
            ask_policy: kanzei_core::AskPolicy::NonInteractive,
            halt: None,
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
            Box::pin(async move {
                match request {
                    kanzei_core::AskRequest::Permission { .. } => {
                        kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                    }
                    kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled,
                }
            })
        };
        let _ = run_once_with_parts(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &tool_ctx,
            &prompt,
            None,
            None,
            &[],
            None,
            None,
            // R-246:子代理探索 run 不持有 LineRuntime(子代理禁嵌套 owner)。
            None,
            &mut on_event,
            &mut ask,
        )
        .await;
        // 差集:req/defect 两条线新增的 ID 就是真实拆解产出。
        let new_req: Vec<String> = DocStore::open(&project_root, &REQUIREMENTS)
            .load()
            .map_err(|e| e.to_string())?
            .iter()
            .filter(|e| !before_req.contains(&e.id))
            .map(|e| e.id.clone())
            .collect();
        let new_def: Vec<String> = DocStore::open(&project_root, &DEFECTS)
            .load()
            .map_err(|e| e.to_string())?
            .iter()
            .filter(|e| !before_def.contains(&e.id))
            .map(|e| e.id.clone())
            .collect();
        let mut produced = new_req;
        produced.extend(new_def);
        if produced.is_empty() {
            last_error = "子代理没有产出任何新的需求/缺陷条目".into();
            continue;
        }
        // 主进程收口:把想法转 split 并写 refs(触发硬门禁,refs 必须是真实 R/D)。
        let tool = kanzei_tools::tracker::TrackerTool {
            tool_name: "idea",
            noun: "idea",
            kind: &IDEAS,
            requires_refs: None,
        };
        let gate = tool
            .execute(
                serde_json::json!({
                    "action": "update",
                    "id": id,
                    "status": "split",
                    "refs": produced,
                }),
                &tool_ctx,
            )
            .await;
        if gate.is_error {
            return Err(format!(
                "拆解产出 {produced:?} 但转 split 被门禁拒绝: {}",
                gate.content
            ));
        }
        return Ok(format!("{id} → {}", produced.join(" ")));
    }
    Err(format!("拆解失败: {last_error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        IDEA_SPLIT_SYSTEM, QUICK_CAPTURE_TAGS, QUICK_REQ_DEFECT_SYSTEM,
        QUICK_REQ_REQUIREMENT_SYSTEM,
    };

    // R-112 验收④:quick capture 自动建议分类——两条 system 提示必须引导子代理
    // 从受控词表里选一个标签,且词表与引擎侧 check_tag 的 DocKind.tags 保持一致。
    #[test]
    fn quick_capture_prompts_suggest_controlled_vocabulary_tag() {
        for prompt in [QUICK_REQ_DEFECT_SYSTEM, QUICK_REQ_REQUIREMENT_SYSTEM] {
            assert!(
                prompt.contains("标签"),
                "提示必须让子代理填「标签」字段: {prompt}"
            );
            assert!(
                prompt.contains(QUICK_CAPTURE_TAGS),
                "提示必须带上受控词表 {QUICK_CAPTURE_TAGS}: {prompt}"
            );
            assert!(
                prompt.contains("pick ONE tag"),
                "提示必须要求单选: {prompt}"
            );
        }
        // 词表与引擎侧校验真源一致:Req/Defect 的 DocKind.tags 就是这份词表。
        use kanzei_tools::docstore::{DEFECTS, REQUIREMENTS};
        let expected: Vec<&str> = QUICK_CAPTURE_TAGS.split('|').collect();
        assert_eq!(REQUIREMENTS.tags.unwrap().to_vec(), expected);
        assert_eq!(DEFECTS.tags.unwrap().to_vec(), expected);
    }

    // D-205 验收①+②(快记信息保真)机械回归:prompt 层禁止编造复现、推断不出写
    // 待澄清问题清单、保留原文关键限定词——这些是防 D-204 类伪复现的 prompt 防线,
    // 必须被契约测试锁死,防止后续文案改动悄悄把防线改回退。
    #[test]
    fn quick_capture_defect_prompt_forbids_fabricated_repro_and_keeps_qualifiers() {
        let p = QUICK_REQ_DEFECT_SYSTEM;
        assert!(
            p.contains("NEVER invent or pad one"),
            "复现字段必须禁止编造/填充: {p}"
        );
        assert!(
            p.contains("待澄清"),
            "推断不出复现时必须写「待澄清」而非编造: {p}"
        );
        assert!(
            p.contains("questions the user must answer"),
            "待澄清必须带具体问题清单: {p}"
        );
        assert!(
            p.contains("keep qualifier words"),
            "必须保留原文关键限定词(用户/桌面端/CLI 等): {p}"
        );
        assert!(
            p.contains("original text verbatim"),
            "原始描述必须逐字保留原文: {p}"
        );
    }

    // R-252 验收③/⑤:idea_split 系统提示的契约锁——子代理必须 ①先读想法全文
    // (idea get)再产出 ②产出走 req add/defect add ③保留原话限定词/原始描述逐字
    // ④不自己动想法状态(转 split 由主进程收口,经 refs 硬门禁)。这些是「拆解
    // 必须真实产出且可追溯」的 prompt 防线,文案改动不得悄悄拆掉。
    #[test]
    fn idea_split_prompt_contract_guards_real_splitting() {
        let p = IDEA_SPLIT_SYSTEM;
        assert!(p.contains("idea get"), "提示必须让子代理先读想法全文: {p}");
        assert!(
            p.contains("req add") && p.contains("defect add"),
            "提示必须让子代理用 req add / defect add 产出条目: {p}"
        );
        assert!(
            p.contains("original text verbatim"),
            "原始描述必须逐字保留原文: {p}"
        );
        assert!(
            p.contains("keep qualifier words"),
            "必须保留原文关键限定词: {p}"
        );
        assert!(
            p.contains("Do NOT update the idea's own status"),
            "子代理不得自己转 split——主进程收口才经过 refs 硬门禁: {p}"
        );
        assert!(
            p.contains("never invent or pad one"),
            "复现字段禁止编造: {p}"
        );
    }

    // R-252 验收⑤:idea_split 子代理跑通一次真实拆解(fake server 集成测试)。
    // 桩服务器按序响应:①子代理读想法全文(idea get)→ ②产出 req add → ③产出
    // defect add → ④结束。真实链路的全部落盘(requirements.md/defects.md 新增、
    // 想法转 split + refs 经硬门禁)都是生产代码执行,不是桩。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn idea_split_runs_subagent_and_marks_idea_split_with_real_refs() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        // 照 phase_pipeline_tests::serve_response:按 Content-Length 读完整请求再响应。
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
        // 子代理调用序列:idea get → req add → defect add → 结束文本。
        let server = tokio::spawn(async move {
            let tool_call = |name: &str, args: serde_json::Value| {
                serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "id": format!("call_{name}"),
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
                serde_json::json!({ "action": "add", "title": "冒烟需求", "priority": "P2",
                    "complexity": "中", "fields": { "标签": "后端", "验收": "一条验收", "原始描述": "想法原文" } }),
            );
            let resp3 = tool_call(
                "defect",
                serde_json::json!({ "action": "add", "title": "冒烟缺陷", "severity": "medium",
                    "priority": "P2", "fields": { "标签": "前端", "复现": "待澄清", "原始描述": "想法原文" } }),
            );
            let resp4 = serde_json::json!({
                "choices": [{ "index": 0, "delta": { "content": "R-001 D-001" }, "finish_reason": "stop" }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
            });
            for resp in [resp1, resp2, resp3, resp4] {
                serve_response(&listener, resp).await;
            }
        });

        // 项目夹具:kanzei.toml 指向桩 provider;ideas.md 含一条 inbox 想法;
        // requirements/defects 空(差集从空出发,新增即产出)。
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

        let coordinator = std::sync::Arc::new(kanzei_core::orchestration::MemoryCoordinator::new());
        let result = super::idea_split_with_coordinator(
            coordinator,
            project.display().to_string(),
            "I-001".into(),
        )
        .await;
        server.await.unwrap();
        assert!(result.is_ok(), "idea_split 应成功: {:?}", result);
        let msg = result.unwrap();
        assert!(msg.starts_with("I-001 →"), "返回应含产出编号: {msg}");
        assert!(msg.contains("R-001") && msg.contains("D-001"), "{msg}");

        // 想法已转 split 且 refs 是真实 R/D(硬门禁放行即证明条目真实存在)。
        let ideas =
            kanzei_tools::docstore::DocStore::open(&project, &kanzei_tools::docstore::IDEAS)
                .load()
                .unwrap();
        let idea = ideas.iter().find(|e| e.id == "I-001").unwrap();
        assert_eq!(idea.status, "split", "拆解后想法必须转 split");
        let refs = idea.refs();
        assert!(refs.contains(&"R-001".to_string()), "{refs:?}");
        assert!(refs.contains(&"D-001".to_string()), "{refs:?}");
        // requirements/defects 真的落了条目。
        let reqs =
            kanzei_tools::docstore::DocStore::open(&project, &kanzei_tools::docstore::REQUIREMENTS)
                .load()
                .unwrap();
        assert_eq!(reqs.len(), 1, "拆解必须真实新增需求条目");
        let defects =
            kanzei_tools::docstore::DocStore::open(&project, &kanzei_tools::docstore::DEFECTS)
                .load()
                .unwrap();
        assert_eq!(defects.len(), 1, "拆解必须真实新增缺陷条目");
        std::fs::remove_dir_all(&project).ok();
    }
}

const DEFECT_REVIEW_SYSTEM: &str = "You are a read-only defect review agent. You only have read, glob, and grep. \\
Read .kanzei/project/defects.md first, then verify every active defect against relevant code, tests, and design documents. \\
Reply in Chinese Markdown with: 1. summary and active defect count; 2. categories; 3. likely duplicates with IDs; \\
4. impact of each defect; 5. suggested priority with reasons; 6. verifiable evidence using exact file paths, functions, \\
and line numbers; 7. concrete next steps. Do not modify files, run commands, update trackers, or claim unverified facts.";

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
    harness.resolve(rctx)
}

pub(crate) fn defect_review_report(summary: &kanzei_core::RunSummary) -> Result<String, String> {
    let report = summary.text.trim();
    if report.is_empty() {
        Err("审查模型没有返回报告".into())
    } else {
        Ok(report.to_string())
    }
}

#[tauri::command]
pub(crate) async fn defect_review(project_dir: String) -> Result<DefectReviewResult, String> {
    let cwd = PathBuf::from(&project_dir);
    let config = Arc::new(KanzeiConfig::load(&cwd).map_err(|e| e.to_string())?);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let defects = DocStore::open(&project_root, &DEFECTS)
        .load()
        .map_err(|e| e.to_string())?;
    if defects.is_empty() {
        return Ok(DefectReviewResult {
            empty: true,
            report: "当前没有活动缺陷。".into(),
            defect_count: 0,
        });
    }
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let snapshot = defect_review_snapshot(&rctx).map_err(|e| e.to_string())?;
    let mut agent = kanzei_tools::explore_agent();
    agent.name = "defect-review".into();
    agent.system = DEFECT_REVIEW_SYSTEM.into();
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(value) => ProxyConfig::Explicit(value.to_string()),
    };
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let tool_ctx = ToolCtx {
        cwd,
        project_root,
        ..Default::default()
    };
    let prompt = format!("审查当前项目 defects.md 中的 {} 条活动缺陷。逐条核对真实代码、测试和调用方，输出约定的 Markdown 报告。", defects.len());
    let mut last_error = "没有可用的 fast 或 primary 模型".to_string();
    for role in ["fast", "primary"] {
        let resolved = match config.resolve_model(role) {
            Ok(value) => value,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let route = match kanzei_core::build_route(&resolved, &proxy).await {
            Ok(value) => value,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let runner_config = RunnerConfig {
            max_tokens: config.limits.subagent_max_tokens(),
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
            model: resolved.model,
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
            ask_policy: kanzei_core::AskPolicy::NonInteractive,
            halt: None,
        };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |_request: kanzei_core::AskRequest| -> AskFuture {
            Box::pin(async { kanzei_core::AskResponse::Permission(kanzei_core::AskReply::Deny) })
        };
        match run_once_with_parts(
            &client,
            &route,
            &snapshot,
            &agent,
            &runner_config,
            &tool_ctx,
            &prompt,
            None,
            None,
            &[],
            None,
            None,
            // R-246:子代理探索 run 不持有 LineRuntime(子代理禁嵌套 owner)。
            None,
            &mut on_event,
            &mut ask,
        )
        .await
        {
            Ok(summary) => match defect_review_report(&summary) {
                Ok(report) => {
                    return Ok(DefectReviewResult {
                        empty: false,
                        report,
                        defect_count: defects.len(),
                    })
                }
                Err(error) => last_error = error,
            },
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(format!("缺陷自动审查失败:{last_error}"))
}
