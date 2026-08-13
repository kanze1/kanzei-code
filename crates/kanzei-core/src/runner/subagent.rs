//! 子代理域(R-155 B7):SubagentRuntime 运行时、task 工具 schema(task_spec)与
//! run_subagent(独立只读快照 + 空历史,ask 一律 Deny,run_once 递归经 dyn Box 断开)。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kanzei_harness::{AgentDef, HarnessSnapshot, ToolCtx};
use kanzei_llm::{LlmClient, ReasoningEffort, Route, ToolSpec, Usage};
use tokio_util::sync::CancellationToken;

use super::event::{AskFuture, AskReply, AskRequest, AskResponse, RunEvent, TaskTrace};
use super::{run_once, AskPolicy, RunnerConfig};

/// R-174:运行中**可单条取消**的子代理注册表。id = 模型 task 调用 id 或编排角色名。
/// `cancel` 命中后 token 即触发,drive/phase_pipeline 的 select 分支以「被停」终态收尾,
/// 读槽由 run_subagent future drop 时 RAII 释放。None(测试/CLI 单运行)不支持中途单条停止。
#[derive(Default)]
pub struct TaskCancellations {
    inner: Mutex<HashMap<String, CancellationToken>>,
}

/// 取消注册表的 RAII 注册句柄。
///
/// `run_subagent` 外层有墙钟 timeout；future 被 timeout 丢弃时，await 之后的
/// 手工 unregister 永远不会执行，因此注册必须由这个句柄的 Drop 负责回收。
pub struct TaskCancellationGuard {
    registry: Arc<TaskCancellations>,
    id: String,
    token: CancellationToken,
}

impl TaskCancellations {
    pub fn register(self: &Arc<Self>, id: &str) -> TaskCancellationGuard {
        let token = CancellationToken::new();
        self.inner
            .lock()
            .unwrap()
            .insert(id.to_string(), token.clone());
        TaskCancellationGuard {
            registry: Arc::clone(self),
            id: id.to_string(),
            token,
        }
    }
    /// 取消一个子代理;返回该 id 当时是否在运行(不在 = 已结束/不存在)。
    pub fn cancel(&self, id: &str) -> bool {
        let token = self.inner.lock().unwrap().remove(id);
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }
    /// 子代理自然结束后清理注册(token 已失效或从未被 cancel);幂等。
    pub fn unregister(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
    }
}

impl TaskCancellationGuard {
    fn token(&self) -> &CancellationToken {
        &self.token
    }
}

impl Drop for TaskCancellationGuard {
    fn drop(&mut self) {
        self.registry.unregister(&self.id);
    }
}

/// task 子代理运行时(R-004/R-012)。快照由调用方用 SubagentBase 组件构建,
/// 代码层面只含只读工具——子代理无人应答权限询问,必须做到零 ask。
#[derive(Clone)]
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
    /// R-171 批6:项目级协调器(可选)。Some 时子代理执行前申请读槽登记
    /// 「并行查」身份,结束 RAII 释放;None(纯 CLI 单运行/测试)不登记。
    pub coordinator:
        Option<std::sync::Arc<dyn kanzei_harness::orchestration::ProjectExecutionCoordinator>>,
    /// R-174:单条停止注册表(可选)。Some 时 drive/phase_pipeline 在子代理 future
    /// 上挂取消 token,`stop_task` 命令按 id 命中即取消;None(测试/CLI 单运行)不挂。
    pub cancellations: Option<Arc<TaskCancellations>>,
    /// R-175:后台模式。false(默认)= 轮内一次性调用:drive.rs 派发后等齐全部
    /// task 才继续;true = 后台化:派发即返回句柄,主代理本轮继续做别的,
    /// 子代理跨轮存活(注册表 + 持久化,见 R-175 内容①②)。
    pub background: bool,
}

pub(crate) fn task_spec() -> ToolSpec {
    ToolSpec {
        name: "task".into(),
        description: "Delegate a narrow read-only exploration task (find files, call \
                      sites, usages; read and summarize code) to a subagent with ONLY \
                      read/glob/grep. It cannot write/edit files, run bash, inspect or \
                      change git state, merge, or publish/release; those authority-bearing \
                      actions belong to the primary agent. Params: prompt (self-contained \
                      instruction saying exactly what to find and what to report back); \
                      optional model: \"fast\" (default, local model, mechanical searches) | \
                      \"primary\" (tasks needing code comprehension). Multiple task calls \
                      in one turn run in parallel."
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
        // R-171:子代理是只读勘察/复核,不参与写仲裁,用默认执行策略。
        execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
        ask_policy: AskPolicy::NonInteractive,
    };
    let mut total_usage = Usage::default();
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
                id,
                name,
                summary,
                input,
            } => Some(TaskTrace {
                child_id: id,
                phase: "start".into(),
                name,
                summary: Some(summary),
                ok: None,
                preview: None,
                display: None,
                // R-174:完整入参原文进 trace,面板/transcript 可展开复核「到底拿什么调的」。
                input: Some(input),
                usage: None,
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
                input: None,
                usage: None,
            }),
            // R-174:子代理每轮 StepEnd 累计 token,以 phase="usage" 的 trace 上抛,
            // 前端据此刷新「累计 token」字段(transcript/面板共用同一数据源)。
            RunEvent::StepEnd { usage, .. } => {
                total_usage.input = total_usage.input.saturating_add(usage.input);
                total_usage.output = total_usage.output.saturating_add(usage.output);
                total_usage.reasoning = total_usage.reasoning.saturating_add(usage.reasoning);
                total_usage.cache_read = total_usage.cache_read.saturating_add(usage.cache_read);
                total_usage.cache_write = total_usage.cache_write.saturating_add(usage.cache_write);
                Some(TaskTrace {
                    child_id: parent_call_id.to_string(),
                    phase: "usage".into(),
                    name: String::new(),
                    summary: None,
                    ok: None,
                    preview: None,
                    display: None,
                    input: None,
                    usage: Some(total_usage),
                })
            }
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
    // R-171 批6:子代理是只读勘察/复核——申请读槽登记并行身份,结束自动释放。
    // 读槽只登记不阻塞(wave 并行),与 writer 租约是两套互不干扰的机制。
    let _read_permit = match rt.coordinator.as_ref() {
        Some(coord) => coord
            .acquire_read_slot(kanzei_harness::orchestration::ReadSlotRequest {
                project_root: ctx.project_root.clone(),
                run_id: parent_call_id.to_string(),
                process_id: rt.agent.name.clone(),
                agent_name: rt.agent.name.clone(),
            })
            .await
            .ok(),
        None => None,
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
        None,
        &[],
        None,
        &mut on_event,
        &mut ask,
    );
    // R-174:单条停止——注册表 Some 时注册本子代理的取消 token,`stop_task` 命中后
    // 立即触发 select 分支,以「被停」终态返回;读槽 `_read_permit` 随函数返回由 RAII
    // 释放。drive.rs 的 timeout 仍在外层墙钟兜底,二者正交(取消先到先得)。
    let cancellation = rt
        .cancellations
        .as_ref()
        .map(|reg| reg.register(parent_call_id));
    let mut fut = Box::pin(fut);
    let output = match &cancellation {
        Some(guard) => tokio::select! {
            biased;
            _ = guard.token().cancelled() => {
                let _ = progress.send(RunEvent::TaskProgress {
                    id: parent_call_id.to_string(),
                    text: "子代理已被停止".into(),
                    trace: Some(TaskTrace {
                        child_id: parent_call_id.to_string(),
                        phase: "cancelled".into(),
                        name: String::new(),
                        summary: None,
                        ok: None,
                        preview: None,
                        display: None,
                        input: None,
                        usage: None,
                    }),
                });
                kanzei_harness::ToolOutput::error(format!(
                    "subagent {parent_call_id} was stopped by the user"
                ))
            }
            result = &mut fut => match result {
                Ok(summary) => {
                    let text = if summary.text.trim().is_empty() {
                        "(subagent finished without a text answer)".to_string()
                    } else {
                        summary.text
                    };
                    kanzei_harness::ToolOutput::ok(text)
                }
                Err(e) => kanzei_harness::ToolOutput::error(format!("subagent failed: {e}")),
            },
        },
        None => match fut.await {
            Ok(summary) => {
                let text = if summary.text.trim().is_empty() {
                    "(subagent finished without a text answer)".to_string()
                } else {
                    summary.text
                };
                kanzei_harness::ToolOutput::ok(text)
            }
            Err(e) => kanzei_harness::ToolOutput::error(format!("subagent failed: {e}")),
        },
    };
    // `cancellation` 的 Drop 负责正常、失败、取消和外层 timeout 的统一清理。
    output
}

/// R-173:**编排对象直接派发**的只读勘察/复核代理。
///
/// 与 `task` 工具那条路的区别只在**谁决定派谁**:task 由模型在轮内自行派发,
/// 本函数由阶段编排对象按固定角色表派发(设计文档「推荐勘察角色」)。二者走的是
/// 同一个 [`run_subagent`],所以只读白名单、`ask` 恒 Deny、读槽登记与 RAII 回收
/// **完全一致**——不存在"编排派的子代理走了另一条没人管的路"。
///
/// `agent_id` 同时用作读槽身份键(并行角色靠它区分,见 `ReadPermit::run_id`)。
pub async fn run_read_agent(
    client: &LlmClient,
    rt: &SubagentRuntime,
    ctx: &ToolCtx,
    agent_id: &str,
    prompt: &str,
    progress: tokio::sync::mpsc::UnboundedSender<RunEvent>,
) -> kanzei_harness::ToolOutput {
    run_subagent(
        client,
        rt,
        ctx,
        agent_id,
        &serde_json::json!({ "prompt": prompt }),
        progress,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::TaskCancellations;
    use std::sync::Arc;

    #[test]
    fn cancellation_guard_drop_清理注册表且终态停止返回未运行() {
        let registry = Arc::new(TaskCancellations::default());
        let guard = registry.register("timed-out-task");
        drop(guard);
        assert!(
            !registry.cancel("timed-out-task"),
            "外层 timeout 丢弃 future 后，死 token 不得继续可取消"
        );
    }
}
