//! 运行事件域(R-155 B1):RunEvent/RunSummary/Ask*/preview,零内部依赖。
//! 拆分自 runner.rs(设计 §C B1);RunEvent/RunSummary/Ask* 经 mod.rs pub use 平铺,
//! drain_task_events/preview 仅 runner 内部使用,见 mod.rs 的 use event 导入。

use std::sync::{
    atomic::{AtomicBool, AtomicU32},
    Arc,
};

use kanzei_harness::ToolArtifact;
use kanzei_llm::{FinishReason, Message, Usage};

/// 子代理内部事件折叠出的一条轨迹(TaskProgress 的 trace)。
///
/// derive Default:各 phase 只填自己那几项,其余靠 `..Default::default()` 补齐,
/// 加字段时不必逐个改全部字面量。
#[derive(Clone, Debug, Default)]
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
    /// UI-0926 #8:phase == "meta" 时携带**实际选中**的人格名。`task` 入参里的
    /// `agent` 只是请求,选不中会静默回落默认人格(resolve_agent),卡片要显示真值。
    pub agent: Option<String>,
    /// UI-0926 #8:phase == "meta" 时携带实际模型 id。`input.model` 只有
    /// fast/primary 档位,用户看不出档位背后到底是哪个模型。
    pub model: Option<String>,
}

/// 面向 UI 的运行事件(CLI/桌面端都消费这一层,不直接碰 LlmEvent)。
/// R-281:TaskProgress 携带完整 assistant 文本，枚举变体尺寸差异是有意的数据契约。
#[allow(clippy::large_enum_variant)]
pub enum RunEvent {
    /// 一轮 provider 调用开始(UI 画轮次分隔)。
    TurnStart {
        step: u32,
        max_steps: u32,
        /// R-319:事件消费者可在最后一步授予一次受控收尾延长；core
        /// 在回调返回后读取该信号，不改变普通步数上限。
        budget_extension: Arc<AtomicU32>,
        /// 这次延长是「收尾落盘」而不是「收尾提交」——本轮改了源码却一次
        /// 都没提交。core 据此在延长的那几步注入收尾指令,让模型把半个事务落成
        /// 可恢复的检查点(提交 WIP + 写回进展),而不是接着写新代码。
        winddown: Arc<AtomicBool>,
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
        /// UI-0926 #6:给界面的结果正文,与写进历史的 `output.content` 同源
        /// (不含 `[tool_outcome=…]` 机器头)。上限 [`TOOL_END_UI_CONTENT_MAX`],
        /// 按字符边界截断——工具行摘要器要按工具解析正文,只有首行的 preview
        /// 做不到,实时与历史回放因此天然不一致。trace/typed 事件不存它。
        content: String,
        /// 截断前的正文字节数;`content.len() < content_bytes` 即被截断。
        content_bytes: usize,
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

/// `RunEvent::ToolEnd::content` 的上限(256 KiB)。bash 的 display.full 本就有
/// 200k,这里取同量级;IPC 每条最多多出这么多,前端不在 DOM 上保留正文。
pub const TOOL_END_UI_CONTENT_MAX: usize = 256 * 1024;

/// 给 UI 的工具结果正文:超过 [`TOOL_END_UI_CONTENT_MAX`] 时按字符边界截断。
/// 返回 (正文, 截断前字节数)。
pub fn ui_tool_content(text: &str) -> (String, usize) {
    let bytes = text.len();
    if bytes <= TOOL_END_UI_CONTENT_MAX {
        return (text.to_string(), bytes);
    }
    let mut cut = TOOL_END_UI_CONTENT_MAX;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    (text[..cut].to_string(), bytes)
}

impl RunEvent {
    /// 从工具输出一次性构造 ToolEnd:ok/outcome/code/preview/content/display/artifact
    /// 全部取自同一个 `ToolOutput`,避免各构造点漏填或各填一套。
    pub fn tool_end(id: String, name: String, output: &kanzei_harness::ToolOutput) -> Self {
        let (content, content_bytes) = ui_tool_content(&output.content);
        RunEvent::ToolEnd {
            id,
            name,
            ok: !output.is_error,
            outcome: output.outcome.as_str().into(),
            code: output.code.map(str::to_owned),
            preview: preview(&output.content),
            content,
            content_bytes,
            display: output.display.clone(),
            artifact: output.artifact.clone(),
        }
    }
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
    /// D-655:本轮稳定消息真源。与 `messages` 不同,它不受轮中上下文
    /// 压缩/prune/trim 的结构性删短影响,供轮末 episode/metrics/harvest 使用。
    pub round_messages: Vec<Message>,
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

/// R-328:一个可选答案。
///
/// `note` 是「**选它意味着什么**」。只给标签时用户得自己猜每个选项的后果,
/// 而后果恰恰是提问的原因——「用 A 方案还是 B 方案」这种问题,选项名本身
/// 从不足以决策。它是可选的:一句话能说清的问题不必强行配注解。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AskOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl AskOption {
    /// 无注解选项。旧调用方与「一眼能懂」的选项走这条。
    pub fn plain(label: impl Into<String>) -> Self {
        AskOption {
            label: label.into(),
            note: None,
        }
    }

    /// 从工具入参解析:既吃裸字符串,也吃 `{label, note|description}`。
    ///
    /// 两种形态都收是为了**向后兼容**:`options: ["A","B"]` 是既有调用形态,
    /// 换成只认对象会让所有历史提示词与旧会话重放一起失效。`description`
    /// 作为 `note` 的别名收下,因为模型更常写这个词。
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        if let Some(label) = value.as_str() {
            return Some(AskOption::plain(label));
        }
        let label = value.get("label").and_then(|v| v.as_str())?;
        let note = value
            .get("note")
            .or_else(|| value.get("description"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        Some(AskOption {
            label: label.to_owned(),
            note,
        })
    }
}

/// 裸标签直转。让 `vec!["是".into(), "否".into()]` 这类既有写法零改动继续成立——
/// 无注解本就是合法形态,不该为了引入注解逼所有调用方改写。
impl From<&str> for AskOption {
    fn from(label: &str) -> Self {
        AskOption::plain(label)
    }
}

impl From<String> for AskOption {
    fn from(label: String) -> Self {
        AskOption::plain(label)
    }
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
        options: Vec<AskOption>,
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

#[cfg(test)]
mod ask_option_tests {
    use super::AskOption;
    use serde_json::json;

    /// 两种入参形态都收:裸字符串是既有调用形态,换成只认对象会让所有历史
    /// 提示词与旧会话重放一起失效。
    #[test]
    fn 裸字符串与对象两种形态都解析() {
        assert_eq!(
            AskOption::from_json(&json!("方案 A")),
            Some(AskOption::plain("方案 A"))
        );
        assert_eq!(
            AskOption::from_json(&json!({"label": "方案 A", "note": "改动小,但留下技术债"})),
            Some(AskOption {
                label: "方案 A".into(),
                note: Some("改动小,但留下技术债".into()),
            })
        );
    }

    /// `description` 是 `note` 的别名——模型更常写这个词,不认它等于让注解静默丢失。
    #[test]
    fn description_是note的别名() {
        let parsed = AskOption::from_json(&json!({"label": "B", "description": "彻底但要两天"}));
        assert_eq!(parsed.and_then(|o| o.note), Some("彻底但要两天".into()));
    }

    /// 空白注解等于没写,不该占一行 UI。
    #[test]
    fn 空白注解视为无注解() {
        let parsed = AskOption::from_json(&json!({"label": "C", "note": "   "})).unwrap();
        assert_eq!(parsed.note, None);
    }

    /// 缺 label 的对象无法成为一个可点的选项,丢弃而不是造一个空按钮。
    #[test]
    fn 缺label的对象被丢弃() {
        assert_eq!(AskOption::from_json(&json!({"note": "孤儿注解"})), None);
        assert_eq!(AskOption::from_json(&json!(42)), None);
    }

    /// 无注解时不发 note 字段:UI 靠字段存在与否决定要不要多渲染一行。
    #[test]
    fn 无注解不序列化note字段() {
        let value = serde_json::to_value(AskOption::plain("是")).unwrap();
        assert_eq!(value, json!({"label": "是"}));
    }

    #[test]
    fn from字符串保持零改动兼容() {
        let from_str: AskOption = "是".into();
        let from_string: AskOption = String::from("否").into();
        assert_eq!(from_str, AskOption::plain("是"));
        assert_eq!(from_string, AskOption::plain("否"));
    }
}

#[cfg(test)]
mod tool_end_content_tests {
    use super::{ui_tool_content, RunEvent, TOOL_END_UI_CONTENT_MAX};
    use kanzei_harness::{ToolArtifact, ToolOutput};
    use serde_json::json;

    /// 多字节正文按字符边界截断:切在「汉」(3 字节)中间会 panic 或产出乱码。
    #[test]
    fn 超限正文按字符边界截断并保留原字节数() {
        let text = "汉".repeat(300 * 1024 / 3);
        let (content, bytes) = ui_tool_content(&text);
        assert_eq!(bytes, text.len());
        assert!(content.len() <= TOOL_END_UI_CONTENT_MAX);
        assert!(content.len() > TOOL_END_UI_CONTENT_MAX - 3);
        assert!(content.chars().all(|c| c == '汉'));
    }

    #[test]
    fn 小正文原样透传() {
        let text = "exit code: 0\nok";
        let (content, bytes) = ui_tool_content(text);
        assert_eq!(content, text);
        assert_eq!(bytes, text.len());
    }

    /// 构造器把 ToolOutput 的各字段原样搬进事件,preview 仍是首行 + 行数。
    #[test]
    fn tool_end_从工具输出填齐字段() {
        let artifact = ToolArtifact {
            artifact_id: "a1".into(),
            relative_path: "artifacts/a1.txt".into(),
            bytes: 9,
            sha256: "00".into(),
            retrieval_hint: "read it".into(),
        };
        let mut output = ToolOutput::noop("EDIT_NOOP", "same\nsecond line")
            .with_display(json!({"kind": "diff", "additions": 1}));
        output.artifact = Some(artifact.clone());
        let RunEvent::ToolEnd {
            id,
            name,
            ok,
            outcome,
            code,
            preview,
            content,
            content_bytes,
            display,
            artifact: got_artifact,
        } = RunEvent::tool_end("c1".into(), "edit".into(), &output)
        else {
            panic!("tool_end 必须构造 ToolEnd");
        };
        assert_eq!((id.as_str(), name.as_str()), ("c1", "edit"));
        assert!(!ok);
        assert_eq!(outcome, "noop");
        assert_eq!(code.as_deref(), Some("EDIT_NOOP"));
        assert_eq!(preview, "same (+1 lines)");
        assert_eq!(content, "same\nsecond line");
        assert_eq!(content_bytes, content.len());
        assert_eq!(display, Some(json!({"kind": "diff", "additions": 1})));
        assert_eq!(got_artifact, Some(artifact));
    }
}
