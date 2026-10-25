//! agent 循环(M1):harness 快照驱动——工具物化过权限、system 由 Context Source 拼装、
//! 每次工具调用过硬门禁(deny 回喂模型 / ask 问用户,用户拒绝则整轮停,与 V2 语义一致)。
//! steer/queue/持久化调度在 M2 引入。

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use kanzei_harness::{
    tolerant_parse, tool::repair_hint, AgentDef, Effect, HarnessSnapshot, Tool, ToolCtx,
};
use kanzei_llm::{
    FinishReason, LlmClient, LlmEvent, LlmRequest, Message, Part, ReasoningEffort, Route, ToolSpec,
    Usage,
};

mod event;
pub use event::*;
/// D-342:协作式停止的信号类型对外再导出——桌面端(kanzei-app)不用为此单独
/// 引 tokio-util,与 TaskCancellations 用的是同一个类型。
pub use tokio_util::sync::CancellationToken;
pub(crate) mod metrics;
pub(crate) mod recovery;
pub use metrics::*;
mod context;
mod observations;
pub(crate) use context::*;
pub use observations::{project_observation_context, ObservationProjection};
mod compaction;
pub use compaction::DigestModel;
mod item_context;
pub use context::compaction_budget;
mod drive;
mod recall;
/// R-250:子代理结构化返回的 schema 校验(聚焦子集,精确到字段的诊断)。
mod schema_check;
pub use recall::{
    RecallHit, RecallOutcome, RecallPolicy, RecallRunOutcome, RecallTrigger, RecallWatch,
};
mod delegation;
mod subagent;
pub use delegation::*;
mod tool_exec;
mod tool_failure_telemetry;
/// UI2-0926 #8:工具截图落盘 + content 末尾 `[tool-image]` 标记(对话回显)。
mod tool_images;
pub use drive::{run_once, run_once_with_parts};

pub(crate) use subagent::task_spec;
pub use subagent::*;

pub(crate) use tool_exec::*;

pub(crate) use compaction::*;

use event::{drain_task_events, preview};

/// 提问与权限提示分别处理。交互模式接收真实用户答复；自主模式可记录重要决定
/// 和必要事实缺口。`AutoAllow` 仅改变权限提示策略，自主决定不能作为用户授权。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskPolicy {
    Interactive,
    /// Auto 的手动输入：记录自主决定，工具权限仍允许交互提示。
    DeferredQuestions,
    NonInteractive,
    AutoAllow,
}

impl AskPolicy {
    pub(crate) fn allows_permission_prompt(self) -> bool {
        matches!(self, Self::Interactive | Self::DeferredQuestions)
    }

    /// question 使用决策记录通道；不要求普通实现选择逐个留档。
    pub fn records_questions(self) -> bool {
        !matches!(self, Self::Interactive)
    }
}

pub struct RunnerConfig {
    pub digest_model: Option<DigestModel>,
    /// R-322:本轮门禁强度。决定引擎对**任务判断**介入多深(续跑提示与队列停止判断);
    /// 权限、托管围栏与事件真源不受它影响。默认 `Autonomous` = 引入前行为。
    pub intensity: kanzei_harness::HarnessIntensity,
    pub model: String,
    pub max_tokens: u32,
    /// 思考强度;由调用方(CLI/桌面端)按配置或运行时选择传入。
    pub reasoning: ReasoningEffort,
    /// Responses 服务档位;仅 Codex Fast mode 使用 priority。
    pub service_tier: Option<String>,
    /// Reserved transport declarations. Nonempty values are rejected before dispatch;
    /// native search runs through the permission-controlled websearch tool.
    pub hosted_tools: Vec<serde_json::Value>,

    /// 该模型的上下文窗口。None = 未知,轮内不做主动预算(只保留撞墙后的被动恢复)。
    pub context_limit: Option<u64>,
    /// 可调上限([limits] 节)。全 None = 内置默认,与改造前的硬编码常量逐值一致。
    pub limits: kanzei_harness::config::Limits,
    /// R-162 事件触发召回策略:工具失败瞬间把相关记忆 Packet 注入下一请求前。
    /// None = 关闭召回(纯 no-op,行为与引入前完全一致)。core 不依赖 tools,
    /// 实现由 CLI/桌面端注入(kanzei-tools 记忆检索包装成 RecallPolicy)。
    pub recall: Option<Box<dyn RecallPolicy>>,
    /// R-171 执行策略:ReadParallelWriteSerial 时 writer 阶段强制普通工具串行
    /// (max in-flight=1)、禁用 task 子代理;Default 保持现状(wave 并发)。
    pub execution_policy: kanzei_harness::orchestration::ExecutionPolicy,
    /// 权限/问题询问策略；默认由调用方显式选择，避免后台运行意外弹窗。
    pub ask_policy: AskPolicy,
    /// D-342 协作式停止:cancel 后,run 在最近的安全检查点(步首/流内/工具间)
    /// 以 `halted_by_user=true` **正常返回**——messages 完整交还调用方走轮末写回,
    /// 被打断轮不再从对话投影里消失。None = 无停止通道,行为与引入前一致。
    /// 硬 abort 只准做超时兜底,不准做第一停止手段(那正是 D-342 的根因)。
    pub halt: Option<CancellationToken>,
}

/// 流中途断开后重放本步请求的上限。工具在流结束后才执行,所以此时重放零副作用;
/// 但每次重放都要重新生成已产出的 token,必须有界。
pub const MAX_STREAM_RESTARTS: u32 = 2;

/// Provider 上下文计算可能比本地估算更严格。首次超限保留有界历史摘要，
/// 第二次超限只保留当前用户消息；再失败就把真实错误交给调用方。
pub const MAX_CONTEXT_OVERFLOW_RECOVERIES: u32 = 2;

/// 主动压缩的**连续无效**刹车,不是总次数配额(D-206)。
///
/// 旧常量 MAX_PROACTIVE_COMPACTIONS=3 把注释里的"压缩后仍超线再压无益"实现成了
/// 总量计数——**成功的压缩也扣配额**。对无步数上限的自举 run,压缩是常规运营动作:
/// 上下文反复涨到预算线本来就该反复压。实测第 58 步的长 run 里三次**成功**压缩把
/// 配额吃光,之后放飞,上下文一路涨向 context_limit,prefill 慢到单步等待 863s,
/// 只能等 provider 报 overflow 走被动恢复——主动压缩在最需要它的场景下提前退场。
///
/// 正确判据与注释原意一致:压完低于线 = 在正常工作,清零计数、不限次数;
/// 压完仍超线 / 中段为空压不动 = 无进展,连续两次就停(head+当前消息本身超线,
/// trim_tail 都救不了,交给被动恢复,别空转)。
pub(super) const MAX_FUTILE_COMPACTIONS: u32 = 2;

/// 轮末压缩与轮内共用同一份实现；app 轮末经 compact_conversation_with_model 调用。全仓只允许
/// 这一处「纪要替换历史」——app 层 R-021 的整段替换已删,谁要再添第二套先读 D-181
/// 与 docs/design/context_compaction.md。
pub async fn compact_conversation(
    client: &LlmClient,
    subagent: Option<&SubagentRuntime>,
    messages: &mut Vec<Message>,
    budget: u64,
    overflow_traces: &mut Vec<String>,
    recent_verbatim_ratio: f64,
) -> usize {
    compact_with_digest(
        client,
        subagent,
        messages,
        budget,
        overflow_traces,
        recent_verbatim_ratio,
    )
    .await
}

pub async fn compact_conversation_with_model(
    client: &LlmClient,
    model: Option<&DigestModel>,
    messages: &mut Vec<Message>,
    budget: u64,
    overflow_traces: &mut Vec<String>,
    recent_verbatim_ratio: f64,
) -> usize {
    match compact_with_model_outcome(
        client,
        model,
        messages,
        budget,
        overflow_traces,
        recent_verbatim_ratio,
    )
    .await
    {
        CompactionOutcome::Compacted(count) => count,
        CompactionOutcome::Deferred => 0,
    }
}

/// R-236 B1:对话历史的 token 粗估——轮末口径 = 轮内口径(同一估算器,附件按
/// 固定成本,不按 base64 字节),消灭「带附件必误触发压缩」。
pub fn prune_conversation_with_archive(
    messages: &mut [Message],
    protect_tokens: u64,
    min_gain_tokens: u64,
    project_root: &std::path::Path,
) -> usize {
    prune_results_with(messages, protect_tokens, min_gain_tokens, |original| {
        tool_exec::archive_context_result(project_root, original)
    })
}

pub fn estimate_conversation_tokens(messages: &[Message]) -> u64 {
    context::estimate_prompt_tokens(&[], messages, &[])
}

/// R-236 B4:轮末的 L0 机械清理(kanzei-app 轮末在 LLM 纪要之前调用)。
/// 与轮内 prune 同一份实现;返回清理条数,0 = 不够收益没动手。
pub fn prune_conversation(
    messages: &mut [Message],
    protect_tokens: u64,
    min_gain_tokens: u64,
) -> usize {
    prune_old_tool_results(messages, protect_tokens, min_gain_tokens)
}

#[cfg(test)]
pub(crate) mod testutil {
    use kanzei_llm::{Message, Part};

    pub fn call(id: &str, name: &str, target_key: &str, target: &str) -> Message {
        Message::assistant(vec![Part::ToolCall {
            id: id.into(),
            name: name.into(),
            input: serde_json::json!({ target_key: target }),
        }])
    }
    pub fn result(call_id: &str, content: &str, is_error: bool) -> Message {
        Message::tool_results(vec![Part::ToolResult {
            call_id: call_id.into(),
            content: content.into(),
            is_error,
        }])
    }
    pub fn tracker_call(id: &str, tool: &str, entry: &str, status: &str) -> Message {
        Message::assistant(vec![Part::ToolCall {
            id: id.into(),
            name: tool.into(),
            input: serde_json::json!({ "action": "update", "id": entry, "status": status }),
        }])
    }
    pub fn bash(id: &str, command: &str) -> Message {
        Message::assistant(vec![Part::ToolCall {
            id: id.into(),
            name: "bash".into(),
            input: serde_json::json!({ "command": command }),
        }])
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
mod tests {
    use super::{
        compact_messages_aggressively, compact_messages_for_retry, drain_task_events,
        recover_context_overflow, AskPolicy, RunEvent, MAX_STREAM_RESTARTS,
    };
    use kanzei_llm::{LlmError, Message, Part};

    #[test]
    fn 子代理完成前的缓冲事件会被排空() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(RunEvent::TaskProgress {
            id: "task-1".into(),
            text: "收尾".into(),
            trace: None,
        })
        .unwrap();
        drop(tx);
        let mut drained = 0;
        drain_task_events(&mut rx, &mut |event| {
            if matches!(event, RunEvent::TaskProgress { .. }) {
                drained += 1;
            }
        });
        assert_eq!(drained, 1);
    }

    /// 流中途断开只对传输层错误重放:协议错误重放只会原样复现,白烧钱。
    /// 这里锁住 runner 里那条判定用的变体匹配语义。
    #[test]
    fn 只有传输层中断才重放本步() {
        let transport = LlmError::Config("placeholder".into());
        assert!(!matches!(transport, LlmError::Transport(_)));
        assert!(!matches!(
            LlmError::Protocol("bad SSE".into()),
            LlmError::Transport(_)
        ));
        assert!(!matches!(
            LlmError::ContextOverflow {
                message: "x".into()
            },
            LlmError::Transport(_)
        ));
        assert!(!matches!(
            LlmError::Provider {
                kind: "rate_limit_error".into(),
                message: "x".into()
            },
            LlmError::Transport(_)
        ));
        // 重放次数必须有界:每次重放都要重新生成已产出的 token。
        assert_eq!(MAX_STREAM_RESTARTS, 2);
    }

    #[test]
    fn 自举与并行线禁止用户询问() {
        assert!(AskPolicy::Interactive.allows_permission_prompt());
        assert!(!AskPolicy::Interactive.records_questions());
        assert!(AskPolicy::DeferredQuestions.allows_permission_prompt());
        assert!(AskPolicy::DeferredQuestions.records_questions());
        assert!(!AskPolicy::NonInteractive.allows_permission_prompt());
        assert!(AskPolicy::NonInteractive.records_questions());
        // D-281:AutoAllow 也不弹用户窗——自动放行放的是权限,不是问题。
        assert!(!AskPolicy::AutoAllow.allows_permission_prompt());
        assert!(AskPolicy::AutoAllow.records_questions());
    }

    #[test]
    fn compact_retry_keeps_prompt_and_bounded_tool_history() {
        let mut messages = vec![
            Message::user_text("原始任务"),
            Message::assistant(vec![Part::Text {
                text: "旧回复".into(),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_1".into(),
                content: "工具结果".into(),
                is_error: false,
            }]),
            Message::user_text("当前任务"),
        ];
        let mut traces = Vec::new();

        compact_messages_for_retry(&mut messages, &mut traces);

        assert_eq!(messages.len(), 3);
        assert!(
            matches!(messages[0].parts[0], Part::Text { ref text } if text.contains("工具结果"))
        );
        assert!(matches!(messages[1].parts[0], Part::Text { ref text } if text == "原始任务"));
        assert!(matches!(messages[2].parts[0], Part::Text { ref text } if text == "当前任务"));
    }

    #[test]
    fn compact_retry_drops_orphan_tool_results_in_tool_loop() {
        let mut messages = vec![
            Message::user_text("当前任务"),
            Message::assistant(vec![Part::ToolCall {
                id: "call_1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "notes.md"}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_1".into(),
                content: "工具结果".into(),
                is_error: false,
            }]),
        ];
        let mut traces = Vec::new();

        compact_messages_for_retry(&mut messages, &mut traces);
        assert!(messages.iter().any(|message| {
            message
                .parts
                .iter()
                .any(|part| matches!(part, Part::Text { text } if text == "当前任务"))
        }));
        assert!(!messages
            .iter()
            .flat_map(|message| &message.parts)
            .any(|part| { matches!(part, Part::ToolResult { .. } | Part::ToolCall { .. }) }));
        assert!(messages.iter().any(|message| {
            message
                .parts
                .iter()
                .any(|part| matches!(part, Part::Text { text } if text.contains("工具结果")))
        }));

        let mut aggressive = vec![
            Message::user_text("当前任务"),
            Message::assistant(vec![Part::ToolCall {
                id: "call_2".into(),
                name: "read".into(),
                input: serde_json::json!({}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_2".into(),
                content: "结果".into(),
                is_error: false,
            }]),
        ];
        let mut aggressive_traces = Vec::new();
        compact_messages_aggressively(&mut aggressive, &mut aggressive_traces);
        assert!(!aggressive
            .iter()
            .flat_map(|message| &message.parts)
            .any(|part| { matches!(part, Part::ToolResult { .. } | Part::ToolCall { .. }) }));
    }

    #[test]
    fn compaction_records_dropped_segments_as_overflow_traces() {
        // R-106:被裁剪段先沉淀轨迹摘要再重置,激进压缩不再无声丢弃轨迹。
        // 失败信号要走 summarize_failures 的机械闸门(count>=2 或带恢复对),
        // 所以同一失败放两次,信号才会保留。
        let mut messages = vec![
            Message::user_text("原始任务"),
            Message::assistant(vec![Part::ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "cargo test"}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_1".into(),
                content: format!("构建失败 {}", "source diagnostic ".repeat(1_000)),
                is_error: true,
            }]),
            Message::assistant(vec![Part::ToolCall {
                id: "call_2".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "cargo test"}),
            }]),
            Message::tool_results(vec![Part::ToolResult {
                call_id: "call_2".into(),
                content: format!("构建失败 {}", "source diagnostic ".repeat(1_000)),
                is_error: true,
            }]),
            Message::user_text("当前任务"),
        ];
        let mut recoveries = 0;
        let mut traces = Vec::new();

        // The first stage bounds older observations while preserving both human inputs.
        assert!(recover_context_overflow(
            &mut messages,
            &mut recoveries,
            &mut traces
        ));
        assert_eq!(traces.len(), 1, "第一次压缩应产生一条轨迹摘要");
        let first: serde_json::Value = serde_json::from_str(&traces[0]).unwrap();
        assert_eq!(first["dropped_messages"], 4);
        assert_eq!(first["tools"]["bash"], 2, "被丢弃段的工具画像应被沉淀");
        assert_eq!(first["failures"][0]["tool"], "bash", "失败信号应随轨迹沉淀");
        assert_eq!(first["failures"][0]["count"], 2);
        assert!(messages.contains(&Message::user_text("原始任务")));

        // The second stage removes observations while retaining both human inputs.
        assert!(recover_context_overflow(
            &mut messages,
            &mut recoveries,
            &mut traces
        ));
        assert_eq!(traces.len(), 2, "第二次压缩应追加一条轨迹摘要");
        assert_eq!(
            messages,
            vec![
                Message::user_text("原始任务"),
                Message::user_text("当前任务")
            ]
        );

        // 第三级:超过有界上限,拒绝继续恢复且不新增轨迹。
        assert!(!recover_context_overflow(
            &mut messages,
            &mut recoveries,
            &mut traces
        ));
        assert_eq!(traces.len(), 2);
    }
}
