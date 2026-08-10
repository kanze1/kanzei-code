//! 子代理域(R-155 B7):SubagentRuntime 运行时、task 工具 schema(task_spec)与
//! run_subagent(独立只读快照 + 空历史,ask 一律 Deny,run_once 递归经 dyn Box 断开)。

use std::sync::Arc;

use kanzei_harness::{AgentDef, HarnessSnapshot, ToolCtx};
use kanzei_llm::{LlmClient, ReasoningEffort, Route, ToolSpec};

use super::event::{AskFuture, AskReply, AskRequest, AskResponse, RunEvent, TaskTrace};
use super::{run_once, RunnerConfig};

/// task 子代理运行时(R-004/R-012)。快照由调用方用 SubagentBase 组件构建,
/// 代码层面只含只读工具——子代理无人应答权限询问,必须做到零 ask。
pub struct SubagentRuntime {
    pub snapshot: Arc<HarnessSnapshot>,
    pub agent: AgentDef,
    /// (route, model id):fast = 本地小模型跑机械检索。
    pub fast: (Route, String),
    /// primary = 主模型,给需要理解代码的任务。
    pub primary: (Route, String),
    /// 两条路由各自的服务档位(Codex Fast mode)。fast 与 primary 未必是同一供应商,
    /// 所以不能共用一个值——用哪条路由就带哪条的档位。
    pub fast_service_tier: Option<String>,
    pub primary_service_tier: Option<String>,
    pub max_tokens: u32,
    /// 单个子代理的墙钟上限(秒):本地模型多轮可能极慢,必须有界。
    pub timeout_secs: u64,
    /// 可调上限,随主运行链一起传下来。
    pub limits: kanzei_harness::config::Limits,
}

pub(crate) fn task_spec() -> ToolSpec {
    ToolSpec {
        name: "task".into(),
        description: "Delegate a narrow read-only exploration task (find files, call \
                      sites, usages; read and summarize code) to a subagent with tools \
                      read/glob/grep. Params: prompt (self-contained instruction saying \
                      exactly what to find and what to report back); optional model: \
                      \"fast\" (default, local model, mechanical searches) | \"primary\" \
                      (tasks needing code comprehension). Multiple task calls in one \
                      turn run in parallel."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Self-contained task: what to find and exactly what to report back"
                },
                "model": {
                    "type": "string",
                    "enum": ["fast", "primary"],
                    "description": "fast = local small model (default); primary = main model"
                }
            },
            "required": ["prompt"]
        }),
    }
}

/// 跑一个子代理:独立的只读快照 + 空历史,结果文本即 tool result。
/// 子代理内 ask 一律 Deny(无人应答);run_once 递归经 dyn Box 断开无限类型。
/// 内部轮次/工具事件折叠成 TaskProgress 经 progress 通道上抛(UI 实时可见)。
pub(crate) async fn run_subagent(
    client: &LlmClient,
    rt: &SubagentRuntime,
    ctx: &ToolCtx,
    parent_call_id: &str,
    input: &serde_json::Value,
    progress: tokio::sync::mpsc::UnboundedSender<RunEvent>,
) -> kanzei_harness::ToolOutput {
    let prompt = ["prompt", "task", "instruction", "query"]
        .iter()
        .find_map(|k| input.get(k).and_then(|v| v.as_str()))
        .unwrap_or("")
        .trim()
        .to_string();
    if prompt.is_empty() {
        return kanzei_harness::ToolOutput::error(
            "task requires a `prompt` string: a self-contained exploration instruction",
        );
    }
    let (route, model, service_tier) = match input.get("model").and_then(|v| v.as_str()) {
        Some("primary") => (&rt.primary.0, &rt.primary.1, &rt.primary_service_tier),
        _ => (&rt.fast.0, &rt.fast.1, &rt.fast_service_tier),
    };
    let config = RunnerConfig {
        model: model.clone(),
        max_tokens: rt.max_tokens,
        // 子代理是机械检索,不开思考:省钱且避免本地小模型不认该参数。
        reasoning: ReasoningEffort::Off,
        service_tier: service_tier.clone(),
        limits: rt.limits.clone(),
        // 子代理跑的是 fast 模型,窗口未必与主模型同源;这里不传上限,
        // 让它继续走撞墙后的被动恢复,不按主模型的预算误压。
        context_limit: None,
        // R-162:子代理是机械检索,不需要事件触发召回(记忆命中只面向主代理
        // 的失败瞬间决策;子代理注入会让同一失败双倍刷屏)。
        recall: None,
    };
    let mut on_event = |event: RunEvent| {
        let text = match &event {
            RunEvent::TurnStart { step, max_steps } => Some(if *max_steps > 0 {
                format!("第 {step}/{max_steps} 轮")
            } else {
                format!("第 {step} 轮")
            }),
            RunEvent::ToolStart { name, summary, .. } => {
                let head: String = summary.chars().take(80).collect();
                Some(format!("{name} {head}"))
            }
            _ => None,
        };
        let trace = match event {
            RunEvent::ToolStart {
                id, name, summary, ..
            } => Some(TaskTrace {
                child_id: id,
                phase: "start".into(),
                name,
                summary: Some(summary),
                ok: None,
                preview: None,
                display: None,
            }),
            RunEvent::ToolEnd {
                id,
                name,
                ok,
                preview,
                display,
            } => Some(TaskTrace {
                child_id: id,
                phase: "end".into(),
                name,
                summary: None,
                ok: Some(ok),
                preview: Some(preview),
                display,
            }),
            _ => None,
        };
        if let Some(text) = text {
            let _ = progress.send(RunEvent::TaskProgress {
                id: parent_call_id.to_string(),
                text,
                trace: trace.clone(),
            });
        } else if trace.is_some() {
            let _ = progress.send(RunEvent::TaskProgress {
                id: parent_call_id.to_string(),
                text: "子代理工具完成".into(),
                trace,
            });
        }
    };
    let mut ask = |_request: AskRequest| -> AskFuture {
        Box::pin(async { AskResponse::Permission(AskReply::Deny) })
    };
    // run_once 本身返回 boxed future,递归的无限类型在其签名处已断开。
    let fut = run_once(
        client,
        route,
        &rt.snapshot,
        &rt.agent,
        &config,
        ctx,
        &prompt,
        &[],
        None,
        &mut on_event,
        &mut ask,
    );
    match fut.await {
        Ok(summary) => {
            let text = if summary.text.trim().is_empty() {
                "(subagent finished without a text answer)".to_string()
            } else {
                summary.text
            };
            kanzei_harness::ToolOutput::ok(text)
        }
        Err(e) => kanzei_harness::ToolOutput::error(format!("subagent failed: {e}")),
    }
}
