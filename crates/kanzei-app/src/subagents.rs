//! 独立子代理命令：快速结构化落库与缺陷审查。

use std::path::PathBuf;
use std::sync::Arc;

use crate::{run_once_with_parts, AppState, AskFuture};
use kanzei_core::{RunEvent, RunnerConfig};
use kanzei_harness::orchestration::ProjectExecutionCoordinator;
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use kanzei_llm::LlmClient;
use kanzei_llm::ProxyConfig;
use kanzei_tools::docstore::{DocStore, DEFECTS, REQUIREMENTS};

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
            max_tokens: 2048,
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
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
            &[],
            None,
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

#[cfg(test)]
mod tests {
    use super::{QUICK_CAPTURE_TAGS, QUICK_REQ_DEFECT_SYSTEM, QUICK_REQ_REQUIREMENT_SYSTEM};

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
            max_tokens: config.limits.max_tokens(),
            reasoning: kanzei_llm::ReasoningEffort::Off,
            service_tier: config.service_tier_for(&resolved),
            context_limit: resolved.provider.context_limit,
            limits: config.limits.clone(),
            model: resolved.model,
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
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
            &[],
            None,
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
