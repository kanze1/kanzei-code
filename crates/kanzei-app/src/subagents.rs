//! 独立子代理命令：快速结构化落库与缺陷审查。

use std::path::PathBuf;
use std::sync::Arc;

use crate::{run_once_with_parts, AskFuture};
use kanzei_core::{AskRequest, ProxyConfig, RunEvent, RunnerConfig};
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use kanzei_llm::LlmClient;

#[tauri::command]
pub(crate) async fn quick_req(
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
    let project_root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let rctx = ResolveCtx { profile: ProfileKind::Dev, cwd: cwd.clone(), project_root: project_root.clone(), config: config.clone() };
    let mut harness = Harness::default();
    harness.add(crate::harness_ext::QuickCaptureComponent { capture });
    let snapshot = harness.resolve(&rctx).map_err(|e| e.to_string())?;
    let system = if capture == "defect" {
        "You capture ONE defect from the user's natural-language description. Call the `defect` tool exactly once with action \"add\": a concise title (<=40 chars, Chinese preferred, keep qualifier words like 用户/桌面端/CLI from the original), severity high|medium|low, fields = {\"复现\": concrete reproduction steps ONLY if the description actually contains them — NEVER invent or pad one; when not reproducible from the text, write \"待澄清: \" followed by the specific questions the user must answer, \"原始描述\": the user's original text verbatim}. Then reply with only the new id."
    } else {
        "You capture ONE requirement from the user's natural-language description. Call the `req` tool exactly once with action \"add\": a concise title (<=40 chars, Chinese preferred), fields = {\"priority\": suggested P0-P3, \"复杂度\": 小|中|大, \"验收\": one draft acceptance line, \"归属\": \"kanzei\", \"原始描述\": the user's original text verbatim}. Then reply with only the new id."
    };
    let agent = kanzei_harness::AgentDef { name: "quickcapture".into(), profile: kanzei_harness::ProfileScope::Dev, model: "fast".into(), mode: kanzei_harness::AgentMode::Subagent, steps: 4, system: system.into() };
    let proxy = match config.proxy.as_deref() { Some("off") => ProxyConfig::Disabled, Some("env") | None => ProxyConfig::Env, Some(p) => ProxyConfig::Explicit(p.to_string()) };
    let client = LlmClient::new(&proxy).map_err(|e| e.to_string())?;
    let tool_ctx = ToolCtx { cwd: cwd.clone(), project_root: project_root.clone() };
    let doc_kind = if capture == "defect" { &DEFECTS } else { &REQUIREMENTS };
    let store = DocStore::open(&project_root, doc_kind);
    let before: std::collections::HashSet<String> = store.load().map_err(|e| e.to_string())?.iter().map(|e| e.id.clone()).collect();
    let prompt = format!("描述(原文):\n{description}");
    for role in ["fast", "primary"] {
        let Ok(resolved) = config.resolve_model(role) else { continue };
        let Ok(route) = kanzei_core::build_route(&resolved, &proxy).await else { continue };
        let runner_config = RunnerConfig { model: resolved.model.clone(), max_tokens: 2048, reasoning: kanzei_llm::ReasoningEffort::Off, service_tier: config.service_tier_for(&resolved), context_limit: resolved.provider.context_limit, limits: config.limits.clone() };
        let mut on_event = |_event: RunEvent| {};
        let mut ask = |request: kanzei_core::AskRequest| -> AskFuture { Box::pin(async move { match request { kanzei_core::AskRequest::Permission { .. } => kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce), kanzei_core::AskRequest::Question { .. } => kanzei_core::AskResponse::Cancelled } }) };
        let _ = run_once_with_parts(&client, &route, &snapshot, &agent, &runner_config, &tool_ctx, &prompt, &[], None, None, &mut on_event, &mut ask).await;
        let after = store.load().map_err(|e| e.to_string())?;
        if let Some(new_entry) = after.iter().find(|e| !before.contains(&e.id)) { return Ok(format!("{} {}", new_entry.id, new_entry.title)); }
    }
    Err("子代理未能落库(fast/primary 均失败),请重试或在对话里直接说".into())
}
