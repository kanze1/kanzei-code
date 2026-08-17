//! 运行事件域(R-155 B1):RunEvent/RunSummary/Ask*/preview,零内部依赖。
//! 拆分自 runner.rs(设计 §C B1);RunEvent/RunSummary/Ask* 经 mod.rs pub use 平铺,
//! drain_task_events/preview 仅 runner 内部使用,见 mod.rs 的 use event 导入。

use kanzei_harness::ToolArtifact;
use kanzei_llm::{FinishReason, Message, Usage};

#[derive(Clone, Debug)]
pub struct TaskTrace {
    pub child_id: String,
    pub phase: String,
    pub name: String,
    pub summary: Option<String>,
    pub ok: Option<bool>,
    pub outcome: Option<String>,
    pub code: Option<String>,
    pub preview: Option<String>,
    /// D-349:大结果外置后的可恢复引用元数据。
    pub artifact: Option<ToolArtifact>,
    pub display: Option<serde_json::Value>,
    /// R-174:子代理内部工具调用的**完整入参** JSON(transcript 数据源,验收⑤)。
    /// 活动面板摘要(summary)只给一行,信息量不足以复核"它到底拿什么参数调的",
    /// transcript 必须能展开每次调用的原始入参。
    pub input: Option<serde_json::Value>,
    /// R-174:子代理**累计 token**(StepEnd 逐次累加,面板「累计 token」字段数据源)。
    /// phase == "usage" 的 trace 携带本字段,前端据此刷新计数。
    pub usage: Option<Usage>,
    /// R-281:子代理 assistant 自己说的话。phase == "text" 时为完整文本，
    /// 供运行中阅读器实时追加，也让结束态不依赖被截断的 ToolEnd preview。
    pub text: Option<String>,
}

/// 面向 UI 的运行事件(CLI/桌面端都消费这一层,不直接碰 LlmEvent)。
/// R-281:TaskProgress 携带完整 assistant 文本，枚举变体尺寸差异是有意的数据契约。
#[allow(clippy::large_enum_variant)]
pub enum RunEvent {
    /// 一轮 provider 调用开始(UI 画轮次分隔)。
    TurnStart {
        step: u32,
        max_steps: u32,
    },
    Text(String),
    Reasoning(String),
    /// provider 一步的 assistant 消息已完整组装并进入 history。它先于任何工具
    /// 副作用发出，因此持久化层可在同一事务提交 assistant + tool_called 事实。
    AssistantMessageCommitted {
        step: u32,
        message: Message,
    },
    ToolStart {
        /// 工具调用 id:并行工具(task)结束顺序不定,UI 靠它配对 start/end。
        id: String,
        name: String,
        summary: String,
        /// 结构化入参原文。summary 是给人看的一行摘要,信息量不足以复核"它到底
        /// 拿什么参数调的";活动面板要能展开完整入参,只能从这里拿(R-095)。
        input: serde_json::Value,
    },
    /// 工具执行中的增量输出(bash 等长任务上报),UI 实时追加显示。
    /// 只在执行期有意义,不进历史轨迹——回放看 ToolEnd 的完整输出即可。
    ToolProgress {
        id: String,
        chunk: String,
    },
    ToolEnd {
        id: String,
        name: String,
        ok: bool,
        /// 机器可读终态:success|noop|needs_correction|needs_confirmation|failed。
        outcome: String,
        /// 稳定错误码；文案变化不应影响恢复策略、指标或 UI 分类。
        code: Option<String>,
        preview: String,
        /// 结构化展示(diff/终端块),见 ToolOutput::display。
        display: Option<serde_json::Value>,
        /// D-349:大结果写入 durable artifact 后的可恢复引用。
        artifact: Option<ToolArtifact>,
    },
    /// 本步整组工具结果已进入 history。包含真实结果、权限拒绝、未知工具和停止
    /// 占位；持久化层由此生成与模型 prior 一致的 tool result facts。
    ToolResultsCommitted {
        step: u32,
        message: Message,
    },
    /// 子代理运行中的实时状态(轮次/正在用的工具),挂在对应 task 块上。
    TaskProgress {
        id: String,
        text: String,
        trace: Option<TaskTrace>,
    },
    /// 权限判定结果(D-173 可观测性)。没有这条事件时,"这一轮用户到底点了几次
    /// 权限、哪些是规则直接放行的"事后完全无从查证——而这恰恰是判断硬门禁
    /// 有没有真正生效、用户被打扰了多少次的唯一依据。
    PermissionResolved {
        tool_call_id: String,
        action: String,
        resource: String,
        /// allow | deny | allow_once | always_allow | declined
        decision: &'static str,
        /// ruleset | session_approved | session_rule | user
        source: &'static str,
        /// R-183:命中的规则原文(仅 source=ruleset 且命中普通规则时有值;
        /// 硬 deny / 无规则匹配 / 会话层决策为 None)。用于可审计轨迹。
        rule: Option<String>,
    },
    /// 流建立前的临时网络错误重试,不会重放已建立流或工具副作用。
    Retry {
        attempt: u32,
        max: u32,
        delay_ms: u128,
    },
    /// 流中途断开后重放本步请求。本步工具尚未执行,零副作用;
    /// UI 收到后应丢弃本步已渲染的残缺输出,等待重新生成。
    StreamRestart {
        attempt: u32,
        max: u32,
        delay_ms: u128,
    },
    /// 轮内主动压缩:到达上下文预算线,就地裁剪历史(D-176)。
    /// 与撞墙后的被动恢复区分开——这条是"没撞墙就先让了路"。
    ContextCompacted {
        before_tokens: u64,
        after_tokens: u64,
        budget_tokens: u64,
        limit_tokens: u64,
        dropped_messages: usize,
    },
    /// R-236 B4:L0 机械清理——旧工具结果正文替换为占位符,零 LLM 调用。
    /// 先于 LLM 纪要执行;这条事件让「压缩触发频率下降」可被度量。
    ContextPruned {
        cleared_results: usize,
        before_tokens: u64,
        after_tokens: u64,
    },
    StepEnd {
        usage: Usage,
        reason: FinishReason,
    },
}

pub(super) fn drain_task_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<RunEvent>,
    on_event: &mut dyn FnMut(RunEvent),
) {
    while let Ok(event) = rx.try_recv() {
        on_event(event);
    }
}

pub struct RunSummary {
    pub text: String,
    pub usage: Usage,
    /// 最近一次 provider 请求的真实 input token 数；None 表示本轮没有拿到有效 usage。
    /// 轮末上下文触发优先使用它，避免把 system/tool schema 与附件再次按本地粗估计量。
    pub last_input_tokens: Option<u64>,
    pub steps: u32,
    /// 用户拒绝权限导致的提前停止。
    pub halted_by_user: bool,
    /// 本次运行结束时的完整消息历史(含本次),调用方保存后可作为下次 prior 传入,
    /// 实现跨消息连续对话(M2 落盘前的内存态方案)。
    pub messages: Vec<Message>,
    /// 上下文账单(R-106):system 各注入源的字符数(agent/system 在首位)。
    pub context_report: Vec<(String, usize)>,
    /// 上下文溢出压缩时被丢弃的轨迹摘要(R-106):每次压缩产生一条,
    /// 由调用方随 episode 沉淀(episodes.overflow_json),避免激进压缩
    /// 无声丢弃轨迹——D-088 的溢出路径可复盘。
    pub overflow_traces: Vec<String>,
}

// 轨迹的工具调用画像:工具名 → 调用次数(episode 与 R-099 度量共用)。
// 注意:传入 `&summary.messages` 会把 prior 全历史一起统计;要本轮画像请传。

/// 权限询问的用户决定。AlwaysAllow 的持久化由 UI 层负责(写入项目配置),
/// runner 只负责本次会话内不再重复询问。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskReply {
    Deny,
    AllowOnce,
    AlwaysAllow,
}

/// 权限询问回调的返回值:异步等待用户决定(CLI 同步问、桌面端走事件+oneshot)。
#[derive(Clone, Debug)]
pub enum AskRequest {
    Permission {
        action: String,
        resource: String,
    },
    Question {
        question: String,
        options: Vec<String>,
        default: Option<String>,
        multiple: bool,
    },
}

#[derive(Clone, Debug)]
pub enum AskResponse {
    Permission(AskReply),
    Answer(String),
    Cancelled,
}

/// 交互询问回调的返回值:异步等待权限决定或用户答案。
pub type AskFuture = std::pin::Pin<Box<dyn std::future::Future<Output = AskResponse> + Send>>;

pub(super) fn preview(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("");
    let mut p = match first_line.char_indices().nth(120) {
        Some((idx, _)) => format!("{}…", &first_line[..idx]),
        None => first_line.to_string(),
    };
    let lines = content.lines().count();
    if lines > 1 {
        p.push_str(&format!(" (+{} lines)", lines - 1));
    }
    p
}
